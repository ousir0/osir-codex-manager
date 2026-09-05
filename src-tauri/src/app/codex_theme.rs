//! Codex UI theme orchestration. Thin coordination over `codex-theme-engine`:
//! resolve theme packages from local roots, keep the in-process daemon fed,
//! and drive the restart path (graceful quit → native config.toml appearance
//! sections → relaunch with a loopback CDP port → inject).
//!
//! CDP launch is platform-specific (`open -n` on macOS and Chromium arguments
//! through the Windows portable/MSIX launchers); other platforms report
//! `supported: false` so the UI can explain the unavailable actions.

use std::path::{Path, PathBuf};
use std::time::Duration;

use codex_theme_engine::codex_theme::{parse_codex_theme, CodexTheme, ValidateOptions};
use codex_theme_engine::daemon::{run_daemon, DaemonStatus, Directive};
use codex_theme_engine::layout::{
    apply_model_picker_layout_fix_once, run_model_picker_layout_daemon,
};
use codex_theme_engine::native::{has_backup, NativeSettingsSnapshot, NativeThemePaths};
use codex_theme_engine::native_hot;
use codex_theme_engine::theme::{list_themes, load_theme, LoadedTheme, ThemeSummary};
use codex_theme_engine::transaction::{self, BeginInput, NativeTransaction, Phase};
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Mutex};

use crate::app::paths;
use crate::app::settings_store::AppSettings;
use crate::errors::AppError;

/// Fixed loopback CDP port. Deliberately not scanned-for (yet): the port only
/// exists while a themed Codex runs, and a collision surfaces as an explicit
/// error rather than a silent bind elsewhere the daemon wouldn't find.
pub const THEME_CDP_PORT: u16 = 9345;

/// Wait after Codex's PIDs disappear before touching config.toml — Codex
/// persists its in-memory config on exit *after* the process count reaches
/// zero (measured in the studio; writing earlier gets clobbered).
const CONFIG_SETTLE: Duration = Duration::from_secs(2);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const QUIT_TIMEOUT_SECS: u64 = 30;
const CDP_WAIT: Duration = Duration::from_secs(45);

/// How a restart-based flow syncs the native layer: try-on requires a verified
/// hot sync so the UI never reports a full theme while only CSS was applied;
/// a full apply may fall back to the stopped-Codex transactional file write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeSync {
    Hot,
    HotThenFile,
    CssOnly,
}

/// New native mutations are refused while an unresolved transaction's
/// evidence exists (SPEC §9: after recovery_required, no new transactions).
fn ensure_no_pending_tx() -> Result<(), AppError> {
    let root = tx_root()?;
    match transaction::pending_transaction(&root) {
        Ok(None) => Ok(()),
        Ok(Some(pending)) => Err(AppError::Engine(format!(
            "存在未完结的原生主题事务（txId={}，phase={:?}）——重启 Manager 可触发自动恢复，或手动检查 {}",
            pending.journal.tx_id,
            pending.journal.phase,
            pending.dir.display()
        ))),
        Err(error) => Err(AppError::Engine(error.to_string())),
    }
}

/// Best-effort preimage rollback for a transaction whose write may have
/// landed. Terminal outcome is journaled either way: `rolled_back` on
/// success, `recovery_required` (evidence kept) when the rollback itself
/// cannot be trusted.
fn rollback_tx_to_preimage(mut tx: NativeTransaction, paths: &NativeThemePaths, cause: &str) {
    let _ = tx.note_error(cause);
    let _ = tx.set_phase(Phase::RollingBack);
    let preimage = match tx.preimage_text() {
        Ok(text) => text,
        Err(error) => {
            let _ = tx.recovery_required(&format!("{cause}; 读取 preimage 失败: {error}"));
            return;
        }
    };
    if codex_theme_engine::native::sha256_hex(preimage.as_bytes()) != tx.preimage_sha256() {
        let _ = tx.recovery_required(&format!("{cause}; preimage 校验失败"));
        return;
    }
    match codex_theme_engine::native::write_config_atomic(paths, &preimage) {
        Ok(()) => {
            if let Err(error) = tx.rolled_back() {
                log::warn!("回滚成功但事务清理失败: {error}");
            }
        }
        Err(error) => {
            let _ = tx.recovery_required(&format!("{cause}; 回滚写入失败: {error}"));
        }
    }
}

/// Write a hot-path settings snapshot into config.toml under a transaction.
/// Caller contract: Codex is not running.
fn write_snapshot_with_tx(
    snapshot: &NativeSettingsSnapshot,
    operation: &str,
) -> Result<(), AppError> {
    let paths = native_paths()?;
    let root = tx_root()?;
    let mut tx = NativeTransaction::begin(BeginInput {
        root: &root,
        config: &paths.config,
        operation,
        theme_id: None,
        was_codex_running: false,
        previous_active_theme: None,
    })
    .map_err(|e| AppError::Engine(e.to_string()))?;
    let outcome = (|| -> Result<(), AppError> {
        let current = std::fs::read_to_string(&paths.config)
            .map_err(|e| AppError::Internal(format!("读取 config.toml 失败: {e}")))?;
        let planned = codex_theme_engine::native::plan_native_config(
            &current,
            &codex_theme_engine::native::snapshot_plan(snapshot),
        )
        .map_err(|e| AppError::Engine(e.to_string()))?;
        tx.stage(&planned)
            .map_err(|e| AppError::Engine(e.to_string()))?;
        codex_theme_engine::native::write_config_atomic(&paths, &planned)
            .map_err(|e| AppError::Engine(e.to_string()))?;
        let on_disk = std::fs::read_to_string(&paths.config)
            .map_err(|e| AppError::Internal(format!("回读 config.toml 失败: {e}")))?;
        codex_theme_engine::native::verify_commit(&current, &planned, &on_disk)
            .map_err(|e| AppError::Engine(e.to_string()))?;
        tx.set_phase(Phase::ConfigCommitted)
            .map_err(|e| AppError::Engine(e.to_string()))?;
        Ok(())
    })();
    match outcome {
        Ok(()) => tx.commit().map_err(|e| AppError::Engine(e.to_string())),
        Err(error) => {
            rollback_tx_to_preimage(tx, &paths, &error.to_string());
            Err(error)
        }
    }
}

/// Startup recovery (SPEC §10): resolve any crashed native transaction by the
/// default decision table, then reconcile a leftover try-on stash — a crash
/// mid-try-on leaves Codex's persisted config themed while nothing was kept.
pub fn recover_native_theme_on_startup() {
    let (Ok(root), Ok(paths)) = (tx_root(), native_paths()) else {
        return;
    };
    match transaction::recover_pending(&root, &paths.config) {
        Ok(Some(action)) => log::warn!("native theme transaction recovery: {action:?}"),
        Ok(None) => {}
        Err(error) => log::warn!("native theme transaction recovery failed: {error}"),
    }
    if let Some(stash) = read_stash() {
        let settings = AppSettings::load();
        if settings.codex_theme.is_none() && !codex_running() {
            match write_snapshot_with_tx(&stash.settings, "snapshot_restore") {
                Ok(()) => {
                    remove_stash();
                    log::info!("restored pre-try-on native settings from stash");
                }
                Err(error) => log::warn!("try-on stash restore failed: {error}"),
            }
        }
    }
}

#[derive(Default)]
pub struct ThemeService {
    inner: Mutex<ServiceInner>,
}

#[derive(Default)]
struct ServiceInner {
    directive_tx: Option<watch::Sender<Directive>>,
    status_rx: Option<watch::Receiver<DaemonStatus>>,
    model_picker_layout_tx: Option<watch::Sender<bool>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeStatusReport {
    /// Whether this platform can theme Codex (macOS and Windows).
    pub supported: bool,
    /// The persisted selection (settings), i.e. what launches will apply.
    pub active_theme: Option<String>,
    /// Live daemon snapshot, when one is running.
    pub daemon: Option<DaemonStatus>,
    /// A CDP endpoint answers on the theme port right now.
    pub cdp_ready: bool,
    /// Codex processes are running (regardless of CDP).
    pub codex_running: bool,
    /// A pristine config.toml appearance backup exists (full restore possible).
    pub native_backup_present: bool,
    /// Where managed skins currently live (downloads/imports land here).
    pub store_dir: Option<PathBuf>,
    /// A pre-try-on native settings stash exists (a try-on is undoable).
    pub try_on_stash: bool,
    /// An unresolved native transaction blocks further native theme ops (the
    /// UI must surface a recovery entry instead of pretending all is well).
    pub recovery_required: bool,
}

fn theme_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Best-effort installed-version hint for native settings discovery. The
/// renderer adapters still validate their target module and fall back to the
/// other supported train, because an update can replace the installed app
/// between detection and launch.
#[cfg(target_os = "windows")]
fn codex_version_hint() -> Option<String> {
    installed_windows_codex()
        .ok()
        .map(|installed| installed.version)
}

#[cfg(target_os = "macos")]
fn codex_version_hint() -> Option<String> {
    crate::app::mac_update::detect_managed_installed().map(|installed| installed.short_version)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn codex_version_hint() -> Option<String> {
    None
}

/// Where managed skins live: the user-chosen store from settings, else the
/// platform default (macOS: Application Support; Windows: LOCALAPPDATA —
/// megabytes of re-downloadable content must not roam with a domain profile).
pub fn store_dir(settings: &AppSettings) -> Result<PathBuf, AppError> {
    if let Some(dir) = settings
        .codex_theme_store_dir
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        return Ok(PathBuf::from(dir));
    }
    paths::default_skins_store_dir()
        .ok_or_else(|| AppError::Internal("无法定位主题存储目录".to_string()))
}

/// Managed skin store + optional dev root from settings. Dev packages shadow
/// managed ones by id.
fn theme_roots(settings: &AppSettings) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = settings
        .codex_theme_dir
        .as_deref()
        .filter(|d| !d.trim().is_empty())
    {
        roots.push(PathBuf::from(dir));
    }
    if let Ok(store) = store_dir(settings) {
        roots.push(store);
    }
    roots
}

/// Move one directory across an arbitrary boundary: fast rename first,
/// recursive copy + delete when the rename crosses filesystems.
fn move_dir(src: &Path, dst: &Path) -> Result<(), AppError> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let target = dst.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_tree(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), &target)?;
            }
        }
        Ok(())
    }
    copy_tree(src, dst)
        .map_err(|e| AppError::Internal(format!("迁移 {} 失败: {e}", src.display())))?;
    std::fs::remove_dir_all(src).map_err(|e| AppError::Internal(format!("清理旧目录失败: {e}")))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreMigrationReport {
    pub from: PathBuf,
    pub to: PathBuf,
    pub moved: Vec<String>,
    /// Ids skipped because the destination already had them.
    pub skipped: Vec<String>,
}

/// Migrate every valid skin package from `old` to `new`. Conflicting ids are
/// left in place (destination wins — never destroy what the user already has
/// at the target). Leftover staging debris is not migrated.
pub fn migrate_store(old: &Path, new: &Path) -> Result<StoreMigrationReport, AppError> {
    std::fs::create_dir_all(new)
        .map_err(|e| AppError::Internal(format!("创建主题目录失败: {e}")))?;
    let mut report = StoreMigrationReport {
        from: old.to_path_buf(),
        to: new.to_path_buf(),
        moved: Vec::new(),
        skipped: Vec::new(),
    };
    let Ok(entries) = std::fs::read_dir(old) else {
        return Ok(report); // old store never materialized — nothing to move
    };
    for entry in entries.flatten() {
        let src = entry.path();
        if !src.is_dir() || load_theme(&src).is_err() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let dst = new.join(&name);
        if dst.exists() {
            report.skipped.push(name);
            continue;
        }
        move_dir(&src, &dst)?;
        report.moved.push(name);
    }
    Ok(report)
}

/// Delete a managed skin from the store directory. Only the store copy is
/// removed — a dev checkout of the same id (the user's source) lives under
/// `codex_theme_dir` and is never touched. Guards against path traversal and
/// refuses to remove anything that isn't a real package directory.
pub fn delete_store_skin(settings: &AppSettings, skin_id: &str) -> Result<(), AppError> {
    let plain = !skin_id.is_empty()
        && !skin_id.contains('/')
        && !skin_id.contains('\\')
        && !skin_id.contains("..")
        && skin_id != "."
        && skin_id != "..";
    if !plain {
        return Err(AppError::Engine(format!("非法皮肤 id: {skin_id}")));
    }
    let store = store_dir(settings)?;
    let dir = store.join(skin_id);
    // Lexical containment: the target must sit directly under the store.
    if dir.parent() != Some(store.as_path()) {
        return Err(AppError::Engine("路径越界，拒绝删除".to_string()));
    }
    // Refuse a symlink outright: the is_dir / theme.json checks below both
    // follow links, so a link planted in the store could smuggle the delete to
    // its target. Store packages are always real directories placed by
    // move_dir — a link here is never legitimate. (symlink_metadata does not
    // follow; a missing path falls through to the "not found" check below.)
    if let Ok(meta) = std::fs::symlink_metadata(&dir) {
        if meta.file_type().is_symlink() {
            return Err(AppError::Engine(format!("{skin_id} 是符号链接，拒绝删除")));
        }
    }
    if !dir.is_dir() {
        return Err(AppError::Engine(format!("商店中没有该皮肤: {skin_id}")));
    }
    // Never rm -rf an arbitrary directory: it must carry a package manifest.
    if !dir.join("theme.json").is_file() {
        return Err(AppError::Engine(format!(
            "{skin_id} 不是有效皮肤目录，拒绝删除"
        )));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| AppError::Internal(format!("删除皮肤失败: {e}")))?;
    log::info!("deleted store skin id={skin_id}");
    Ok(())
}

fn native_paths() -> Result<NativeThemePaths, AppError> {
    let config = paths::codex_home_dir()
        .ok_or_else(|| AppError::Internal("无法定位 ~/.codex".to_string()))?
        .join("config.toml");
    let backup = paths::data_dir()
        .ok_or_else(|| AppError::Internal("无法定位数据目录".to_string()))?
        .join("codex-theme-native-backup.json");
    Ok(NativeThemePaths { config, backup })
}

/// Transaction evidence root for the stopped-Codex file path (journal +
/// preimage per mutation; see engine `transaction`).
fn tx_root() -> Result<PathBuf, AppError> {
    Ok(paths::data_dir()
        .ok_or_else(|| AppError::Internal("无法定位数据目录".to_string()))?
        .join("codex-theme-native-tx"))
}

// ── try-on stash ────────────────────────────────────────────────────────────
// The official hot import persists immediately through Codex's own store, so
// a try-on must first capture the user's live appearance settings; cancelling
// hot-imports them back. Created once per try-on session (switching skins
// keeps the ORIGINAL), consumed by 保留 (keep) or cancel.

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TryOnStash {
    saved_at: String,
    /// The most recently tried-on theme (informational).
    theme_id: String,
    settings: NativeSettingsSnapshot,
}

fn stash_path() -> Result<PathBuf, AppError> {
    Ok(paths::data_dir()
        .ok_or_else(|| AppError::Internal("无法定位数据目录".to_string()))?
        .join("codex-theme-tryon-stash.json"))
}

fn read_stash() -> Option<TryOnStash> {
    let path = stash_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_stash(theme_id: &str, settings: NativeSettingsSnapshot) -> Result<(), AppError> {
    let path = stash_path()?;
    let stash = TryOnStash {
        saved_at: format!(
            "unix:{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ),
        theme_id: theme_id.to_string(),
        settings,
    };
    let rendered = serde_json::to_string_pretty(&stash)
        .map_err(|e| AppError::Internal(format!("试穿备份序列化失败: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, rendered)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| AppError::Internal(format!("写入试穿备份失败: {e}")))
}

fn remove_stash() {
    if let Ok(path) = stash_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Keeping a try-on consumes its undo stash (command layer hook).
pub fn consume_try_on_stash() {
    remove_stash();
}

// ── native block parsing ────────────────────────────────────────────────────

/// Strict delivery-profile parse for the FULL apply path: missing/invalid
/// codexTheme (including absent codeThemeIds) is a hard error — SPEC forbids
/// writing half a native theme.
fn strict_native(theme: &LoadedTheme) -> Result<CodexTheme, AppError> {
    let block = theme.codex_theme.as_ref().ok_or_else(|| {
        AppError::Engine("该主题包没有 codexTheme 原生主题块，无法完整应用".to_string())
    })?;
    parse_codex_theme(block, ValidateOptions::default())
        .map_err(|e| AppError::Engine(e.to_string()))
}

/// Lenient parse for try-on: legacy packages without codeThemeIds degrade to
/// palette-only; a malformed block degrades to CSS-only (logged, not fatal —
/// a broken native block must not take the CSS try-on down with it).
fn lenient_native(theme: &LoadedTheme) -> Option<CodexTheme> {
    let block = theme.codex_theme.as_ref()?;
    match parse_codex_theme(
        block,
        ValidateOptions {
            require_code_theme_ids: false,
            ..Default::default()
        },
    ) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            log::warn!("codexTheme 校验失败，试穿降级为仅 CSS: {error}");
            None
        }
    }
}

/// Which root a gallery entry came from. Dev packages shadow store packages
/// per id at *resolution* time; the gallery still needs to see both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeOrigin {
    Dev,
    Store,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeListEntry {
    #[serde(flatten)]
    pub summary: ThemeSummary,
    pub origin: ThemeOrigin,
}

/// Every package from every root, dev root first (same precedence as
/// `resolve_theme`), annotated with its origin. Deliberately NOT deduplicated:
/// a dev checkout masks the store copy for resolution, but the store tab
/// compares catalog versions against the STORE copy — after an online update
/// the shadowed store version must still be observable, or the update button
/// never flips to "installed".
pub fn merged_theme_list(settings: &AppSettings) -> Vec<ThemeListEntry> {
    let mut entries = Vec::new();
    if let Some(dir) = settings
        .codex_theme_dir
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        entries.extend(
            list_themes(Path::new(dir))
                .into_iter()
                .map(|summary| ThemeListEntry {
                    summary,
                    origin: ThemeOrigin::Dev,
                }),
        );
    }
    if let Ok(store) = store_dir(settings) {
        entries.extend(
            list_themes(&store)
                .into_iter()
                .map(|summary| ThemeListEntry {
                    summary,
                    origin: ThemeOrigin::Store,
                }),
        );
    }
    entries
}

fn resolve_theme(settings: &AppSettings, theme_ref: &str) -> Result<PathBuf, AppError> {
    for root in theme_roots(settings) {
        if let Ok(dir) = codex_theme_engine::theme::resolve_theme_dir(&root, theme_ref) {
            return Ok(dir);
        }
    }
    Err(AppError::Engine(format!("未找到主题: {theme_ref}")))
}

/// Canonical id for persisting a selection: a `theme_ref` may be a dev path,
/// but settings always store the package's own id.
pub fn resolve_theme_for_keep(settings: &AppSettings, theme_ref: &str) -> Result<String, AppError> {
    let dir = resolve_theme(settings, theme_ref)?;
    let theme = load_theme(&dir).map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(theme.config.id)
}

// ── Online catalog ─────────────────────────────────────────────────────────
// OSIR hosts the catalog and packages on its own application domain.
// Catalog URLs stay relative and are resolved ONLY against this fixed base,
// so a hostile catalog cannot redirect downloads elsewhere. All transfers go
// through system curl (Windows 10+ ships curl.exe) with https pinned, size
// caps, and a sha256 gate before anything reaches the importer.

const SKINS_BASES: &[&str] = &[
    "https://osirvedio.cn-nb1.rains3.com/codex-skins/dreamskin/v1",
    "https://app.osirclaw.com/skins",
    "https://app.osirclaw.com/skins/dreamskin",
    "https://api.dreamskin.cc",
];
const CATALOG_MAX_BYTES: &str = "1048576"; // 1 MB index.json cap
const PACK_MAX_BYTES: &str = "52428800"; // 50 MB archive cap (importer re-checks)

fn default_catalog_installable() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSkin {
    pub id: String,
    #[serde(rename = "themeId", default)]
    pub theme_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub appearance: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub codex_verified: Option<String>,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub pack: String,
    #[serde(default)]
    pub preview: String,
    /// Theme category for store grouping (e.g. "wallpaper", "anime", "stars",
    /// "tech", "guofeng", "games"). Absent → grouped under "other" in the UI.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub art: Option<CatalogArt>,
    #[serde(rename = "previewStyle", default)]
    pub preview_style: Option<CatalogPreviewStyle>,
    #[serde(default)]
    pub rights_status: Option<String>,
    #[serde(default = "default_catalog_installable")]
    pub installable: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(rename = "sourceUrl", default)]
    pub source_url: Option<String>,
    #[serde(rename = "sourcePackageUrl", default)]
    pub source_package_url: Option<String>,
    /// Origin used to keep package downloads on the catalog's selected source.
    #[serde(rename = "sourceBase", default)]
    source_base: String,
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArt {
    #[serde(default)]
    pub focus_x: Option<f64>,
    #[serde(default)]
    pub focus_y: Option<f64>,
    #[serde(default)]
    pub safe_area: Option<String>,
    #[serde(default)]
    pub task_mode: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPreviewStyle {
    #[serde(default = "default_preview_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub blur: f64,
    #[serde(default = "default_preview_radius")]
    pub radius: f64,
    #[serde(default = "default_preview_border_alpha")]
    pub border_alpha: f64,
    #[serde(default = "default_preview_shadow")]
    pub shadow: String,
    #[serde(default)]
    pub parts: Vec<String>,
    #[serde(default)]
    pub hover: bool,
    #[serde(default)]
    pub focus_visible: bool,
}

fn default_preview_opacity() -> f64 { 1.0 }
fn default_preview_radius() -> f64 { 12.0 }
fn default_preview_border_alpha() -> f64 { 0.14 }
fn default_preview_shadow() -> String { "soft".to_string() }

#[derive(Debug, Clone, serde::Deserialize)]
struct CatalogIndex {
    #[serde(default)]
    skins: Vec<CatalogSkin>,
}

/// A catalog-relative path is plain (`packs/x.codexskin`) — no scheme, no
/// authority, no parent hops. Everything else is rejected before URL joining.
fn safe_catalog_path(base: &str, rel: &str) -> Result<String, AppError> {
    if !SKINS_BASES.contains(&base) {
        return Err(AppError::Engine("皮肤目录源不受信任".to_string()));
    }
    let ok = !rel.is_empty()
        && !rel.contains("://")
        && !rel.starts_with('/')
        && !rel.contains("..")
        && rel
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b));
    if ok {
        Ok(format!("{base}/{rel}"))
    } else {
        Err(AppError::Engine(format!("目录条目路径非法: {rel}")))
    }
}

fn curl_bin() -> &'static str {
    if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "/usr/bin/curl"
    }
}

fn curl_command() -> std::process::Command {
    let command = std::process::Command::new(curl_bin());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        // The manager is a GUI application. Without CREATE_NO_WINDOW, every
        // catalog or preview request opens a visible curl.exe console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        command
    }
}

/// Fetch a URL to stdout via system curl: https only, no retries into other
/// protocols, hard timeout and size cap. `--retry` covers the transient
/// connection resets this route sees in the wild (CDN + long-haul links).
fn curl_fetch_attempt(
    url: &str,
    max_bytes: &str,
    timeout_secs: &str,
    schannel_best_effort: bool,
) -> Result<std::process::Output, AppError> {
    let mut command = curl_command();
    #[cfg(target_os = "windows")]
    if schannel_best_effort {
        command.arg("--ssl-revoke-best-effort");
    }
    #[cfg(not(target_os = "windows"))]
    let _ = schannel_best_effort;
    command
        .args([
            "-sSfL",
            "--proto",
            "=https",
            "--retry",
            "2",
            "--max-time",
            timeout_secs,
            "--max-filesize",
            max_bytes,
            url,
        ])
        .output()
        .map_err(|e| AppError::Engine(format!("curl 不可用: {e}")))
}

#[cfg(target_os = "windows")]
fn curl_supports_schannel_best_effort() -> bool {
    curl_command()
        .args(["--help", "all"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout).contains("--ssl-revoke-best-effort")
        })
}

#[cfg(any(target_os = "windows", test))]
fn is_schannel_revocation_offline(exit_code: Option<i32>, stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    exit_code == Some(35)
        && (stderr.contains("CRYPT_E_REVOCATION_OFFLINE") || stderr.contains("0x80092013"))
}

fn curl_fetch(url: &str, max_bytes: &str, timeout_secs: &str) -> Result<Vec<u8>, AppError> {
    let output = curl_fetch_attempt(url, max_bytes, timeout_secs, false)?;
    #[cfg(target_os = "windows")]
    let output = if !output.status.success()
        && is_schannel_revocation_offline(output.status.code(), &output.stderr)
    {
        if curl_supports_schannel_best_effort() {
            log::warn!(
                "theme catalog Schannel revocation endpoint unavailable; retrying best-effort url={}",
                crate::app::logging::redact_url(url)
            );
            curl_fetch_attempt(url, max_bytes, timeout_secs, true)?
        } else {
            log::warn!(
                "theme catalog Schannel revocation endpoint unavailable and curl lacks safe retry support url={}",
                crate::app::logging::redact_url(url)
            );
            output
        }
    } else {
        output
    };
    if !output.status.success() {
        return Err(AppError::Engine(format!(
            "下载失败 ({}): {}",
            output.status,
            crate::app::logging::redact_url(url)
        )));
    }
    Ok(output.stdout)
}

fn fetch_catalog_from(base: &str) -> Result<Vec<CatalogSkin>, AppError> {
    let url = safe_catalog_path(base, "index.json")?;
    let bytes = curl_fetch(&url, CATALOG_MAX_BYTES, "15")?;
    let index: CatalogIndex = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Engine(format!("皮肤目录解析失败: {e}")))?;
    let mut skins: Vec<CatalogSkin> = index
        .skins
        .into_iter()
        .filter(|s| !s.id.is_empty() && !s.pack.is_empty() && s.sha256.len() == 64)
        .map(|mut skin| {
            // The catalog may carry a historical sourceBase. The source that
            // actually served this catalog is the safest first download
            // mirror, so prefer the pinned base selected by this fetch.
            skin.source_base = base.to_string();
            skin
        })
        .collect();
    skins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skins)
}

pub fn fetch_catalog() -> Result<Vec<CatalogSkin>, AppError> {
    let mut failures = Vec::new();
    let mut merged = std::collections::BTreeMap::new();
    for base in SKINS_BASES {
        match fetch_catalog_from(base) {
            Ok(skins) => {
                for skin in skins {
                    merged.entry(skin.id.clone()).or_insert(skin);
                }
            }
            Err(error) => {
                log::warn!(
                    "theme catalog source failed base={} error={error}",
                    crate::app::logging::redact_url(base)
                );
                failures.push(error.to_string());
            }
        }
    }
    if !merged.is_empty() {
        return Ok(merged.into_values().collect());
    }
    Err(AppError::Engine(format!(
        "所有皮肤目录源均不可用: {}",
        failures.join("；")
    )))
}

/// On-disk cache for catalog preview thumbnails, keyed by a per-URL FNV-1a hash
/// PLUS the skin's version. A new cover published at the same `previews/<id>`
/// path (with a version bump in index.json) therefore misses the stale cache
/// and re-fetches, while repeat opens of the same version stay cache-hits. Any
/// miss or IO error simply falls back to the network.
fn preview_cache_path(url: &str, version: &str) -> Option<PathBuf> {
    let vsafe: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let vsafe = if vsafe.is_empty() {
        "v".to_string()
    } else {
        vsafe
    };
    Some(paths::cache_dir()?.join("catalog-previews").join(format!(
        "{:016x}-{}.img",
        crate::app::staging::fnv1a64(url.as_bytes()),
        vsafe
    )))
}

/// Cheap structural check that bytes look like a WebP (RIFF container + WEBP
/// fourcc) — gates what we cache and rejects partial/corrupt hits.
fn preview_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.len() >= 3 && bytes[0..3] == [0xff, 0xd8, 0xff] {
        Some("image/jpeg")
    } else if bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        Some("image/png")
    } else {
        None
    }
}

fn read_cached_preview(url: &str, version: &str) -> Option<Vec<u8>> {
    let path = preview_cache_path(url, version)?;
    let bytes = std::fs::read(&path).ok()?;
    if preview_mime(&bytes).is_some() {
        Some(bytes)
    } else {
        // A truncated/corrupt entry (e.g. a crash mid-write) would otherwise
        // hit forever — drop it so the caller re-fetches from the network.
        let _ = std::fs::remove_file(&path);
        None
    }
}

fn write_cached_preview(url: &str, version: &str, bytes: &[u8]) {
    // Only cache plausibly-complete WebP bytes, and write via a temp file +
    // atomic rename so a crash or full disk never leaves a non-empty partial
    // that later reads back as a valid hit.
    if preview_mime(bytes).is_none() {
        return;
    }
    let Some(path) = preview_cache_path(url, version) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension("img.tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Catalog cover preview as a data URL (WebP, ≤ 2 MB by convention). Served from
/// the on-disk cache when present (keyed by url + version), otherwise fetched
/// once over the network and cached so later opens don't re-hit the mirror.
pub fn catalog_preview_data_url(preview_rel: &str, version: &str) -> Result<String, AppError> {
    use base64::Engine as _;
    let mut failures = Vec::new();
    for base in SKINS_BASES {
        let url = safe_catalog_path(base, preview_rel)?;
        let fetched = match read_cached_preview(&url, version) {
            Some(cached) => Ok(cached),
            None => curl_fetch(&url, "2097152", "15"),
        };
        match fetched {
            Ok(bytes) if preview_mime(&bytes).is_some() => {
                write_cached_preview(&url, version, &bytes);
                return Ok(format!(
                    "data:{};base64,{}",
                    preview_mime(&bytes).unwrap_or("image/webp"),
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ));
            }
            Ok(_) => failures.push(format!("{} 返回的预览格式无效", base)),
            Err(error) => failures.push(error.to_string()),
        }
    }
    Err(AppError::Engine(format!(
        "所有皮肤预览源均不可用: {}",
        failures.join("；")
    )))
}

/// Download + sha256-gate + install one catalog skin. Returns the installed
/// summary (the importer re-validates everything structurally).
pub fn install_from_catalog(
    skin_id: &str,
) -> Result<codex_theme_engine::theme::ThemeSummary, AppError> {
    use sha2::Digest as _;
    let skin = fetch_catalog()?
        .into_iter()
        .find(|s| s.id == skin_id)
        .ok_or_else(|| AppError::Engine(format!("目录中没有该皮肤: {skin_id}")))?;
    if !skin.installable {
        return Err(AppError::Engine(
            "该 DreamSkin 主题的许可证尚未完成再分发确认，当前只提供预览和来源直链".to_string(),
        ));
    }
    let mut source_order = Vec::with_capacity(SKINS_BASES.len());
    if SKINS_BASES.contains(&skin.source_base.as_str()) {
        source_order.push(skin.source_base.as_str());
    }
    source_order.extend(
        SKINS_BASES
            .iter()
            .copied()
            .filter(|base| *base != skin.source_base),
    );
    let mut failures = Vec::new();
    let mut downloaded = None;
    for base in source_order {
        let url = safe_catalog_path(base, &skin.pack)?;
        match curl_fetch(&url, PACK_MAX_BYTES, "120") {
            Ok(bytes) => {
                let digest = sha2::Sha256::digest(&bytes);
                let hex = digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                if hex.eq_ignore_ascii_case(&skin.sha256) {
                    downloaded = Some(bytes);
                    break;
                }
                failures.push(format!("{base} 返回的皮肤包校验失败"));
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    let bytes = downloaded.ok_or_else(|| {
        AppError::Engine(format!("所有皮肤包源均不可用: {}", failures.join("；")))
    })?;

    let staging = std::env::temp_dir().join(format!(
        "codexskin-online-{}-{}.codexskin",
        std::process::id(),
        skin_id
    ));
    std::fs::write(&staging, &bytes)
        .map_err(|e| AppError::Internal(format!("写入临时包失败: {e}")))?;
    let themes_root = store_dir(&AppSettings::load())?;
    let outcome = codex_theme_engine::import::import_codexskin(&staging, &themes_root)
        .map_err(|e| AppError::Engine(e.to_string()));
    let _ = std::fs::remove_file(&staging);
    log::info!(
        "online skin install id={skin_id} version={} ok={}",
        skin.version,
        outcome.is_ok()
    );
    outcome
}

/// Cover preview as a data URL; None when the theme has no preview, can't be
/// resolved, or the image fails to read (gallery falls back to swatch art).
pub fn preview_data_url(settings: &AppSettings, theme_ref: &str) -> Option<String> {
    use base64::Engine as _;
    let dir = resolve_theme(settings, theme_ref).ok()?;
    let theme = load_theme(&dir).ok()?;
    let rel = theme.meta.previews.first()?;
    let path = dir.join(rel);
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return None,
    };
    let bytes = std::fs::read(&path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

impl ThemeService {
    /// Ensure the reconciliation daemon is running and return its directive
    /// handle. One daemon per manager process, lazily started.
    async fn directive_handle(&self) -> watch::Sender<Directive> {
        let mut inner = self.inner.lock().await;
        if let Some(tx) = &inner.directive_tx {
            if !tx.is_closed() {
                return tx.clone();
            }
        }
        let (directive_tx, directive_rx) = watch::channel::<Directive>(None);
        let (status_tx, status_rx) = watch::channel(DaemonStatus::default());
        tauri::async_runtime::spawn(run_daemon(THEME_CDP_PORT, directive_rx, status_tx));
        inner.directive_tx = Some(directive_tx.clone());
        inner.status_rx = Some(status_rx);
        directive_tx
    }

    async fn daemon_status(&self) -> Option<DaemonStatus> {
        let inner = self.inner.lock().await;
        inner.status_rx.as_ref().map(|rx| rx.borrow().clone())
    }

    /// Enable the stock model-picker repair for the OpenCodex launch path.
    /// It has its own narrow CSS payload, so a missing theme selection cannot
    /// disable the layout fix.
    pub async fn enable_model_picker_layout_fix(&self) -> Result<(), AppError> {
        {
            let mut inner = self.inner.lock().await;
            if let Some(tx) = inner
                .model_picker_layout_tx
                .as_ref()
                .filter(|tx| !tx.is_closed())
            {
                tx.send(true)
                    .map_err(|_| AppError::Internal("模型选择器修复守护未运行".to_string()))?;
            } else {
                let (tx, rx) = watch::channel(true);
                tauri::async_runtime::spawn(run_model_picker_layout_daemon(
                    THEME_CDP_PORT,
                    rx,
                ));
                inner.model_picker_layout_tx = Some(tx);
            }
        }

        match apply_model_picker_layout_fix_once(THEME_CDP_PORT, Duration::from_secs(8)).await {
            Ok(count) => log::info!("model picker layout fix applied to {count} Codex renderer(s)"),
            Err(error) => log::debug!("model picker layout fix will retry in daemon: {error}"),
        }
        Ok(())
    }

    /// Disable the stock model-picker repair when the manager no longer owns
    /// the OpenCodex launch path.
    pub async fn disable_model_picker_layout_fix(&self) {
        let inner = self.inner.lock().await;
        if let Some(tx) = &inner.model_picker_layout_tx {
            let _ = tx.send(false);
        }
    }

    pub async fn status(&self, settings: &AppSettings) -> ThemeStatusReport {
        let cdp_ready = codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await;
        let native_backup = native_paths().map(|p| has_backup(&p)).unwrap_or(false);
        let recovery_required = tx_root()
            .ok()
            .and_then(|root| transaction::pending_transaction(&root).ok().flatten())
            .is_some();
        ThemeStatusReport {
            supported: theme_supported(),
            active_theme: settings.codex_theme.clone(),
            daemon: self.daemon_status().await,
            cdp_ready,
            codex_running: codex_running(),
            native_backup_present: native_backup,
            store_dir: store_dir(settings).ok(),
            try_on_stash: read_stash().is_some(),
            recovery_required,
        }
    }

    /// Live try-on: requires a debuggable Codex on the theme port. Hot-first
    /// end to end — the native palettes/code themes hot-import through Codex's
    /// own settings API (stashing the user's live values once per try-on
    /// session, since the import persists immediately), then the CSS layer
    /// injects. Does not touch the manager's persisted selection — the caller
    /// decides whether to keep it.
    pub async fn try_on(&self, settings: &AppSettings, theme_ref: &str) -> Result<(), AppError> {
        if !theme_supported() {
            return Err(AppError::UnsupportedPlatform);
        }
        ensure_no_pending_tx()?;
        let dir = resolve_theme(settings, theme_ref)?;
        // Validate eagerly so a broken package fails the command, not the
        // daemon's next tick.
        let theme = load_theme(&dir).map_err(|e| AppError::Engine(e.to_string()))?;
        if !codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
            return Err(AppError::Engine(
                "codex-not-debuggable: Codex 未以调试模式运行".to_string(),
            ));
        }
        if let Some(native) = lenient_native(&theme) {
            // A codex-theme-v1 package promises native appearance values. Do
            // not silently claim success when only its CSS layer could apply.
            self.hot_apply_native(&theme.config.id, &native, true)
                .await?;
        }
        let handle = self.directive_handle().await;
        handle
            .send(Some(dir))
            .map_err(|_| AppError::Internal("主题守护未运行".to_string()))
    }

    /// Hot-write a theme's native settings over CDP (running Codex, official
    /// settings path, live repaint). The file-level baseline is captured from
    /// disk BEFORE the first write — the hot import persists through Codex
    /// within moments, after which the disk no longer holds the user's state.
    /// `stash_first` additionally captures the live values once per try-on
    /// session so cancel can hot-restore them.
    async fn hot_apply_native(
        &self,
        theme_id: &str,
        native: &CodexTheme,
        stash_first: bool,
    ) -> Result<(), AppError> {
        let paths = native_paths()?;
        codex_theme_engine::native::backup_native_theme(&paths)
            .map_err(|e| AppError::Engine(e.to_string()))?;

        let mut targets =
            codex_theme_engine::cdp::connect_codex_targets(THEME_CDP_PORT, Duration::from_secs(8))
                .await
                .map_err(|e| AppError::Engine(e.to_string()))?;
        let session = targets.remove(0).session;
        for extra in targets {
            extra.session.close();
        }
        let version_hint = codex_version_hint();

        let outcome: Result<(), AppError> = async {
            native_hot::ensure_api(&session, version_hint.as_deref())
                .await
                .map_err(|e| AppError::Engine(e.to_string()))?;
            let before = native_hot::read_snapshot(&session, version_hint.as_deref())
                .await
                .map_err(|e| AppError::Engine(e.to_string()))?;
            if stash_first && read_stash().is_none() {
                write_stash(theme_id, before.clone())?;
            }
            let entries = native_hot::theme_write_entries(native);
            if let Err(error) =
                native_hot::write_values(&session, &entries, version_hint.as_deref()).await
            {
                // Best-effort immediate revert to the pre-write values so a
                // half-written palette never survives.
                let revert = native_hot::snapshot_write_entries(&before);
                if let Err(revert_error) =
                    native_hot::write_values(&session, &revert, version_hint.as_deref()).await
                {
                    log::error!("原生设置写入失败且回写也失败: {revert_error}");
                }
                return Err(AppError::Engine(error.to_string()));
            }
            Ok(())
        }
        .await;
        session.close();
        outcome
    }

    /// Poll the daemon until it confirms the target theme on at least one
    /// connected renderer (SPEC §8.9 — a directive `send` alone proves
    /// nothing). Prewarm shells don't pass the daemon's Codex probe, so a
    /// connected target is a real window.
    async fn wait_daemon_theme(&self, theme_id: &str, timeout: Duration) -> Result<(), AppError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(status) = self.daemon_status().await {
                if status.theme_id.as_deref() == Some(theme_id)
                    && status.connected_targets > 0
                    && status.last_error.is_none()
                {
                    return Ok(());
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(AppError::Engine(
                    "注入确认超时：守护未能在 Codex 窗口确认主题".to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    /// Cancel a try-on: remove the CSS layer and hot-import the stashed
    /// pre-try-on native settings back. With Codex stopped the snapshot is
    /// written straight into config.toml (transactionally) instead.
    pub async fn cancel_try_on(&self) -> Result<(), AppError> {
        self.turn_off_live().await?;
        let Some(stash) = read_stash() else {
            return Ok(());
        };
        if codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
            let mut targets = codex_theme_engine::cdp::connect_codex_targets(
                THEME_CDP_PORT,
                Duration::from_secs(8),
            )
            .await
            .map_err(|e| AppError::Engine(e.to_string()))?;
            let session = targets.remove(0).session;
            for extra in targets {
                extra.session.close();
            }
            let version_hint = codex_version_hint();
            let entries = native_hot::snapshot_write_entries(&stash.settings);
            let outcome =
                native_hot::write_values(&session, &entries, version_hint.as_deref()).await;
            session.close();
            outcome.map_err(|e| AppError::Engine(e.to_string()))?;
            remove_stash();
            Ok(())
        } else if !codex_running() {
            let snapshot = stash.settings.clone();
            tauri::async_runtime::spawn_blocking(move || {
                write_snapshot_with_tx(&snapshot, "snapshot_restore")
            })
            .await
            .map_err(|e| AppError::Internal(format!("试穿撤销任务失败: {e}")))??;
            remove_stash();
            Ok(())
        } else {
            Err(AppError::Engine(
                "Codex 正在运行但没有调试端口，暂时无法撤销原生试穿——退出 Codex 后重试，或使用完全恢复".to_string(),
            ))
        }
    }

    /// Live removal (renderers back to stock). Persisted selection is cleared
    /// by the command layer; config.toml is only touched by `off_full`.
    pub async fn turn_off_live(&self) -> Result<(), AppError> {
        let inner = self.inner.lock().await;
        if let Some(tx) = &inner.directive_tx {
            let _ = tx.send(None);
        }
        Ok(())
    }

    /// After a store migration, re-point a live directive whose theme dir
    /// moved with the store (the daemon rebuilds its payload from the new
    /// path on the next tick; injected renderers are untouched either way).
    pub async fn rebase_directive(&self, old_root: &Path, new_root: &Path) {
        let inner = self.inner.lock().await;
        let Some(tx) = &inner.directive_tx else {
            return;
        };
        let current = tx.borrow().clone();
        if let Some(dir) = current {
            if let Ok(rel) = dir.strip_prefix(old_root) {
                let rebased = new_root.join(rel);
                if rebased.join("theme.json").is_file() {
                    let _ = tx.send(Some(rebased));
                }
            }
        }
    }

    /// Try-on that first puts Codex into debug mode: graceful quit → relaunch
    /// with the loopback CDP port → hot native + inject. It never writes
    /// config.toml directly and never persists a selection; the top banner's
    /// 保留 is what makes a try-on stick.
    pub async fn try_on_with_restart(
        &self,
        settings: &AppSettings,
        theme_ref: &str,
    ) -> Result<(), AppError> {
        self.restart_debuggable_and_inject(settings, theme_ref, NativeSync::Hot)
            .await
    }

    /// Full apply, hot-first: with a debuggable Codex already up it is a pure
    /// hot switch (official settings import + CSS, injection confirmed before
    /// returning). Otherwise one restart into CDP mode, then the same hot
    /// path; only if the hot path fails does it fall back to the stopped-
    /// Codex transactional config.toml write.
    pub async fn apply_with_restart(
        &self,
        settings: &AppSettings,
        theme_ref: &str,
    ) -> Result<(), AppError> {
        if !theme_supported() {
            return Err(AppError::UnsupportedPlatform);
        }
        ensure_no_pending_tx()?;
        let dir = resolve_theme(settings, theme_ref)?;
        let theme = load_theme(&dir).map_err(|e| AppError::Engine(e.to_string()))?;
        let native = match theme.codex_theme.as_ref() {
            Some(_) => {
                let parsed = strict_native(&theme)?;
                // §4 contract gate: both official share strings must derive and
                // round-trip before anything is written anywhere.
                codex_theme_engine::codex_theme::verified_share_strings(&parsed)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                Some(parsed)
            }
            None => None,
        };
        let theme_id = theme.config.id.clone();

        if codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
            let native_result = match native.as_ref() {
                Some(native) => self.hot_apply_native(&theme_id, native, false).await,
                None => Ok(()),
            };
            match native_result {
                Ok(()) => {
                    let handle = self.directive_handle().await;
                    handle
                        .send(Some(dir.clone()))
                        .map_err(|_| AppError::Internal("主题守护未运行".to_string()))?;
                    self.wait_daemon_theme(&theme_id, Duration::from_secs(30))
                        .await?;
                    remove_stash(); // applied == kept: the try-on stash is consumed
                    return Ok(());
                }
                Err(error) => {
                    log::warn!("热应用失败，降级为停机写 config.toml: {error}");
                    if let Some(native) = native.as_ref() {
                        return self.file_apply_flow(settings, &dir, &theme_id, native).await;
                    }
                    return Err(error);
                }
            }
        }
        self.restart_debuggable_and_inject(
            settings,
            theme_ref,
            if native.is_some() { NativeSync::HotThenFile } else { NativeSync::CssOnly },
        )
            .await
    }

    /// Launch or reconnect Codex with the renderer-only model picker repair.
    /// OpenCodex needs a debuggable renderer because stock Codex otherwise
    /// offers no supported way to keep this CSS alive across reloads.
    pub async fn launch_with_model_picker_layout_fix(
        &self,
        settings: &AppSettings,
    ) -> Result<(), AppError> {
        if !theme_supported() {
            return Err(AppError::UnsupportedPlatform);
        }

        if codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
            self.enable_model_picker_layout_fix().await?;
            return Ok(());
        }

        self.launch_debuggable_codex(settings, "启动").await?;
        self.enable_model_picker_layout_fix().await
    }

    /// Force a real stop/start cycle while keeping the model picker repair
    /// enabled. This is the explicit restart counterpart to the launch hook.
    pub async fn restart_with_model_picker_layout_fix(
        &self,
        settings: &AppSettings,
    ) -> Result<(), AppError> {
        if !theme_supported() {
            return Err(AppError::UnsupportedPlatform);
        }

        self.launch_debuggable_codex(settings, "重启").await?;
        self.enable_model_picker_layout_fix().await
    }

    async fn launch_debuggable_codex(
        &self,
        settings: &AppSettings,
        action: &str,
    ) -> Result<(), AppError> {
        let disable_self_updates = settings.disable_codex_self_updates;
        tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
            let installed = installed_codex_path()?;
            if codex_running() {
                quit_codex(&installed)?;
                std::thread::sleep(CONFIG_SETTLE);
            }
            launch_codex_with_cdp(&installed, THEME_CDP_PORT, disable_self_updates)
        })
        .await
        .map_err(|e| AppError::Internal(format!("模型选择器修复{action}任务失败: {e}")))??;

        let deadline = tokio::time::Instant::now() + CDP_WAIT;
        while !codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
            if tokio::time::Instant::now() >= deadline {
                return Err(AppError::Engine("Codex 已启动但调试端口未就绪".to_string()));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    async fn restart_debuggable_and_inject(
        &self,
        settings: &AppSettings,
        theme_ref: &str,
        sync: NativeSync,
    ) -> Result<(), AppError> {
        if !theme_supported() {
            return Err(AppError::UnsupportedPlatform);
        }
        ensure_no_pending_tx()?;
        let dir = resolve_theme(settings, theme_ref)?;
        let theme = load_theme(&dir).map_err(|e| AppError::Engine(e.to_string()))?;
        let theme_id = theme.config.id.clone();
        let native = match sync {
            NativeSync::Hot => lenient_native(&theme),
            NativeSync::HotThenFile => {
                let strict = strict_native(&theme)?;
                codex_theme_engine::codex_theme::verified_share_strings(&strict)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                Some(strict)
            }
            NativeSync::CssOnly => None,
        };

        // Plain relaunch into CDP mode — the native layer is synced hot after
        // the debug port is up, so nothing here races Codex's config persist.
        let disable_self_updates = settings.disable_codex_self_updates;
        tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
            let installed = installed_codex_path()?;
            quit_codex(&installed)?;
            launch_codex_with_cdp(&installed, THEME_CDP_PORT, disable_self_updates)
        })
        .await
        .map_err(|e| AppError::Internal(format!("主题应用任务失败: {e}")))??;

        let deadline = tokio::time::Instant::now() + CDP_WAIT;
        while !codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
            if tokio::time::Instant::now() >= deadline {
                return Err(AppError::Engine("Codex 已启动但调试端口未就绪".to_string()));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if let Some(native) = &native {
            let stash_first = matches!(sync, NativeSync::Hot);
            match self.hot_apply_native(&theme_id, native, stash_first).await {
                Ok(()) => {}
                Err(error) if matches!(sync, NativeSync::HotThenFile) => {
                    log::warn!("热应用失败，降级为停机写 config.toml: {error}");
                    return self
                        .file_apply_flow(settings, &dir, &theme_id, native)
                        .await;
                }
                Err(error) => return Err(error),
            }
        }

        let handle = self.directive_handle().await;
        handle
            .send(Some(dir))
            .map_err(|_| AppError::Internal("主题守护未运行".to_string()))?;
        if matches!(sync, NativeSync::HotThenFile | NativeSync::CssOnly) {
            self.wait_daemon_theme(&theme_id, Duration::from_secs(30))
                .await?;
        }
        if matches!(sync, NativeSync::HotThenFile) {
            remove_stash();
        }
        Ok(())
    }

    /// The stopped-Codex fallback (SPEC §8): quiesce → transactional
    /// config.toml write (preimage + journal + staged + post-verify) →
    /// relaunch debuggable → inject → daemon confirmation → commit. Any
    /// failure after the config commit rolls the preimage back and restores
    /// the previous run state.
    async fn file_apply_flow(
        &self,
        settings: &AppSettings,
        dir: &Path,
        theme_id: &str,
        native: &CodexTheme,
    ) -> Result<(), AppError> {
        let disable_self_updates = settings.disable_codex_self_updates;
        let previous_active = settings.codex_theme.clone();
        let native_clone = native.clone();
        let id = theme_id.to_string();

        // Blocking half: quit, settle, write-under-transaction, relaunch.
        let mut tx =
            tauri::async_runtime::spawn_blocking(move || -> Result<NativeTransaction, AppError> {
                let installed = installed_codex_path()?;
                let was_running = codex_running();
                quit_codex(&installed)?;
                std::thread::sleep(CONFIG_SETTLE);
                let paths = native_paths()?;
                let root = tx_root()?;
                let mut tx = NativeTransaction::begin(BeginInput {
                    root: &root,
                    config: &paths.config,
                    operation: "apply",
                    theme_id: Some(id.clone()),
                    was_codex_running: was_running,
                    previous_active_theme: previous_active,
                })
                .map_err(|e| AppError::Engine(e.to_string()))?;
                let staged_write = (|| -> Result<(), AppError> {
                    tx.set_phase(Phase::CodexStopped)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    codex_theme_engine::native::backup_native_theme(&paths)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    let current = std::fs::read_to_string(&paths.config)
                        .map_err(|e| AppError::Internal(format!("读取 config.toml 失败: {e}")))?;
                    let planned = codex_theme_engine::native::plan_native_config(
                        &current,
                        &codex_theme_engine::native::apply_plan(&native_clone),
                    )
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                    tx.stage(&planned)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    codex_theme_engine::native::write_config_atomic(&paths, &planned)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    let on_disk = std::fs::read_to_string(&paths.config)
                        .map_err(|e| AppError::Internal(format!("回读 config.toml 失败: {e}")))?;
                    codex_theme_engine::native::verify_commit(&current, &planned, &on_disk)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    tx.set_phase(Phase::ConfigCommitted)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    Ok(())
                })();
                match staged_write {
                    Ok(()) => {
                        launch_codex_with_cdp(&installed, THEME_CDP_PORT, disable_self_updates)?;
                        let _ = tx.set_phase(Phase::CodexLaunched);
                        Ok(tx)
                    }
                    Err(error) => {
                        rollback_tx_to_preimage(tx, &paths, &error.to_string());
                        // Codex was quit but nothing was committed — put the
                        // previous run state back.
                        if was_running {
                            let _ = launch_codex_plain();
                        }
                        Err(error)
                    }
                }
            })
            .await
            .map_err(|e| AppError::Internal(format!("主题应用任务失败: {e}")))??;

        // Async half: CDP ready → inject → daemon confirmation → commit.
        let post_launch: Result<(), AppError> = async {
            let deadline = tokio::time::Instant::now() + CDP_WAIT;
            while !codex_theme_engine::cdp::cdp_http_ready(THEME_CDP_PORT).await {
                if tokio::time::Instant::now() >= deadline {
                    return Err(AppError::Engine("Codex 已启动但调试端口未就绪".to_string()));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let handle = self.directive_handle().await;
            handle
                .send(Some(dir.to_path_buf()))
                .map_err(|_| AppError::Internal("主题守护未运行".to_string()))?;
            self.wait_daemon_theme(theme_id, Duration::from_secs(30))
                .await?;
            Ok(())
        }
        .await;

        match post_launch {
            Ok(()) => {
                let _ = tx.set_phase(Phase::InjectionVerified);
                tx.commit().map_err(|e| AppError::Engine(e.to_string()))?;
                remove_stash();
                Ok(())
            }
            Err(error) => {
                // §9: config already committed — quiesce, restore preimage,
                // and put the previous state back as far as possible.
                let was_running = tx.journal().was_codex_running;
                let error_text = error.to_string();
                tauri::async_runtime::spawn_blocking(move || {
                    let paths = match native_paths() {
                        Ok(paths) => paths,
                        Err(e) => {
                            let _ = tx.recovery_required(&format!("{error_text}; 回滚失败: {e}"));
                            return;
                        }
                    };
                    if let Ok(installed) = installed_codex_path() {
                        let _ = quit_codex(&installed);
                        std::thread::sleep(CONFIG_SETTLE);
                    }
                    rollback_tx_to_preimage(tx, &paths, &error_text);
                    if was_running {
                        let _ = launch_codex_plain();
                    }
                })
                .await
                .map_err(|e| AppError::Internal(format!("回滚任务失败: {e}")))?;
                Err(error)
            }
        }
    }

    /// Full restore (SPEC §11 off_full): live removal + quiesce Codex, put
    /// every managed unit back from the baseline under a transaction (an
    /// introduced key/section is deleted, the user's raw lines return
    /// verbatim), relaunch plainly (no CDP port). The baseline is only
    /// dropped after the restore commit verified.
    pub async fn off_full(&self) -> Result<(), AppError> {
        if !theme_supported() {
            return Err(AppError::UnsupportedPlatform);
        }
        ensure_no_pending_tx()?;
        self.turn_off_live().await?;
        tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
            let installed = installed_codex_path()?;
            let was_running = codex_running();
            quit_codex(&installed)?;
            std::thread::sleep(CONFIG_SETTLE);
            let paths = native_paths()?;
            if !has_backup(&paths) {
                // Nothing to restore — still honor the run state.
                if was_running {
                    launch_codex_plain()?;
                }
                remove_stash();
                return Ok(());
            }
            let root = tx_root()?;
            let mut tx = NativeTransaction::begin(BeginInput {
                root: &root,
                config: &paths.config,
                operation: "off_full",
                theme_id: None,
                was_codex_running: was_running,
                previous_active_theme: None,
            })
            .map_err(|e| AppError::Engine(e.to_string()))?;
            let outcome = (|| -> Result<(), AppError> {
                tx.set_phase(Phase::CodexStopped)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                let current = std::fs::read_to_string(&paths.config)
                    .map_err(|e| AppError::Internal(format!("读取 config.toml 失败: {e}")))?;
                let planned = codex_theme_engine::native::planned_restore_text(&paths, &current)
                    .map_err(|e| AppError::Engine(e.to_string()))?
                    .ok_or_else(|| AppError::Engine("原生备份缺失".to_string()))?;
                tx.stage(&planned)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                codex_theme_engine::native::write_config_atomic(&paths, &planned)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                let on_disk = std::fs::read_to_string(&paths.config)
                    .map_err(|e| AppError::Internal(format!("回读 config.toml 失败: {e}")))?;
                codex_theme_engine::native::verify_commit(&current, &planned, &on_disk)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                tx.set_phase(Phase::ConfigCommitted)
                    .map_err(|e| AppError::Engine(e.to_string()))?;
                Ok(())
            })();
            match outcome {
                Ok(()) => {
                    // Restore committed and verified — only now may the
                    // baseline go away.
                    codex_theme_engine::native::drop_backup(&paths)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    tx.commit().map_err(|e| AppError::Engine(e.to_string()))?;
                    remove_stash();
                    if was_running {
                        launch_codex_plain()?;
                    }
                    Ok(())
                }
                Err(error) => {
                    rollback_tx_to_preimage(tx, &paths, &error.to_string());
                    if was_running {
                        let _ = launch_codex_plain();
                    }
                    Err(error)
                }
            }
        })
        .await
        .map_err(|e| AppError::Internal(format!("主题还原任务失败: {e}")))?
    }
}

#[cfg(target_os = "macos")]
fn installed_codex_path() -> Result<PathBuf, AppError> {
    crate::app::mac_update::detect_managed_installed()
        .map(|installed| PathBuf::from(installed.path))
        .ok_or_else(|| AppError::Engine("没有可用的 Codex 安装".to_string()))
}

#[cfg(target_os = "macos")]
pub(crate) fn codex_running() -> bool {
    installed_codex_path()
        .map(|path| codex_mac_engine::swap::codex_running_at(&path))
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn quit_codex(installed: &std::path::Path) -> Result<(), AppError> {
    codex_mac_engine::swap::quit_codex_at(installed, QUIT_TIMEOUT_SECS)
        .map_err(|e| AppError::Engine(format!("退出 Codex 失败: {e}")))
}

/// `open -n -a <bundle> --args --remote-debugging-…`: `-n` is required for
/// argument delivery (without a new instance, `open` merely activates the
/// running app and drops the args) — callers must have quiesced Codex first.
#[cfg(target_os = "macos")]
fn launch_codex_with_cdp(
    installed: &std::path::Path,
    port: u16,
    disable_self_updates: bool,
) -> Result<(), AppError> {
    crate::app::opencodex::reconcile_after_manager_update()?;
    if disable_self_updates {
        crate::app::codex_self_update::sync_setting(true)?;
    }
    log::info!(
        "launching Codex with CDP port={port} path={}",
        installed.display()
    );
    let mut command = std::process::Command::new("/usr/bin/open");
    crate::app::codex_self_update::apply_to_command(&mut command, disable_self_updates);
    command
        .arg("-n")
        .arg("-a")
        .arg(installed)
        .arg("--args")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--lang=zh-CN")
        .arg(format!(
            "--proxy-pac-url={}",
            codex_win_engine::osir_i18n_proxy_pac_url()
        ))
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::Engine(format!("以调试模式打开 Codex 失败: {e}")))
}

#[cfg(target_os = "macos")]
fn launch_codex_plain() -> Result<(), AppError> {
    crate::app::opencodex::reconcile_after_manager_update()?;
    crate::app::mac_update::launch_codex()
}

#[cfg(target_os = "windows")]
fn windows_domain_settings() -> crate::domain::settings::AppSettings {
    let saved = AppSettings::load();
    let mut settings = crate::domain::settings::AppSettings::new(String::new(), saved.install_root);
    settings.disable_codex_self_updates = saved.disable_codex_self_updates;
    settings
}

#[cfg(target_os = "windows")]
fn installed_windows_codex() -> Result<codex_win_engine::InstalledWindowsCodex, AppError> {
    crate::app::win_update::win_install_status(&windows_domain_settings())
        .installed
        .ok_or_else(|| AppError::Engine("没有可用的 Codex 安装".to_string()))
}

#[cfg(target_os = "windows")]
fn installed_codex_path() -> Result<PathBuf, AppError> {
    installed_windows_codex().map(|installed| PathBuf::from(installed.path))
}

#[cfg(target_os = "windows")]
pub(crate) fn codex_running() -> bool {
    installed_codex_path()
        .and_then(|path| {
            codex_win_engine::codex_running_for_root(&path)
                .map_err(|e| AppError::Engine(e.to_string()))
        })
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn quit_codex(installed: &std::path::Path) -> Result<(), AppError> {
    codex_win_engine::close_codex_gracefully_for_root(QUIT_TIMEOUT_SECS, installed)
        .map_err(|e| AppError::Engine(format!("退出 Codex 失败: {e}")))
}

#[cfg(target_os = "windows")]
fn launch_codex_with_cdp(
    installed: &std::path::Path,
    port: u16,
    disable_self_updates: bool,
) -> Result<(), AppError> {
    crate::app::opencodex::reconcile_after_manager_update()?;
    if disable_self_updates {
        crate::app::codex_self_update::sync_setting(true)?;
    }
    let detected = installed_windows_codex()?;
    if !codex_win_engine::same_windows_path(Path::new(&detected.path), installed) {
        return Err(AppError::Engine(format!(
            "Codex 安装位置在重启前发生变化：{} -> {}",
            installed.display(),
            detected.path
        )));
    }
    log::info!(
        "launching Windows Codex with CDP port={port} path={}",
        installed.display()
    );
    codex_win_engine::launch_codex_with_options(
        &detected,
        codex_win_engine::LaunchOptions {
            disable_codex_self_updates: disable_self_updates,
            remote_debugging_port: Some(port),
            proxy_pac_url: Some(codex_win_engine::osir_i18n_proxy_pac_url()),
        },
    )
    .map_err(|e| AppError::Engine(format!("以调试模式打开 Codex 失败: {e}")))
}

#[cfg(target_os = "windows")]
fn launch_codex_plain() -> Result<(), AppError> {
    crate::app::opencodex::reconcile_after_manager_update()?;
    crate::app::win_update::launch_codex(&windows_domain_settings())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn installed_codex_path() -> Result<PathBuf, AppError> {
    Err(AppError::UnsupportedPlatform)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn codex_running() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn quit_codex(_installed: &std::path::Path) -> Result<(), AppError> {
    Err(AppError::UnsupportedPlatform)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch_codex_with_cdp(
    _installed: &std::path::Path,
    _port: u16,
    _disable_self_updates: bool,
) -> Result<(), AppError> {
    Err(AppError::UnsupportedPlatform)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch_codex_plain() -> Result<(), AppError> {
    Err(AppError::UnsupportedPlatform)
}

/// Gracefully restart the managed Codex without changing its configuration.
/// The caller has already surfaced the action as a restart, but we re-check
/// the process here so a stale renderer cannot quit a newly closed session.
pub fn restart_plain() -> Result<(), AppError> {
    let installed = installed_codex_path()?;
    if codex_running() {
        quit_codex(&installed)?;
        std::thread::sleep(CONFIG_SETTLE);
    }
    launch_codex_plain()
}

/// Launch hook for the ordinary 〔打开 Codex〕 action: when a theme is the
/// persisted selection, launching through the manager transparently becomes
/// "launch debuggable + keep themed".
pub async fn launch_with_active_theme(
    service: &ThemeService,
    settings: &AppSettings,
) -> Result<bool, AppError> {
    let Some(theme_ref) = settings.codex_theme.clone() else {
        return Ok(false);
    };
    if !theme_supported() {
        return Ok(false);
    }
    match service.apply_with_restart(settings, &theme_ref).await {
        Ok(()) => Ok(true),
        Err(error) => {
            // A broken/missing theme must never brick the launch button:
            // surface in logs, launch plainly.
            log::warn!("themed launch failed, falling back to plain launch: {error}");
            Ok(false)
        }
    }
}

/// Force a real restart while preserving the persisted theme. A missing or
/// broken theme falls back to the plain restart in the command layer, just as
/// ordinary launch falls back to a plain open.
pub async fn restart_with_active_theme(
    service: &ThemeService,
    settings: &AppSettings,
) -> Result<bool, AppError> {
    let Some(theme_ref) = settings.codex_theme.clone() else {
        return Ok(false);
    };
    if !theme_supported() {
        return Ok(false);
    }
    match service
        .restart_debuggable_and_inject(settings, &theme_ref, NativeSync::HotThenFile)
        .await
    {
        Ok(()) => Ok(true),
        Err(error) => {
            log::warn!("themed restart failed, falling back to plain restart: {error}");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod catalog_network_tests {
    use super::{is_schannel_revocation_offline, safe_catalog_path, SKINS_BASES};

    #[test]
    fn catalog_paths_are_relative_and_stay_on_allowlisted_sources() {
        let primary = safe_catalog_path(SKINS_BASES[0], "packs/theme-1.0.0.codexskin").unwrap();
        assert_eq!(
            primary,
            "https://osirvedio.cn-nb1.rains3.com/codex-skins/dreamskin/v1/packs/theme-1.0.0.codexskin"
        );
        assert!(safe_catalog_path(SKINS_BASES[0], "../secret").is_err());
        assert!(safe_catalog_path(SKINS_BASES[0], "https://evil.example/skin").is_err());
        assert!(safe_catalog_path("https://evil.example", "packs/skin.codexskin").is_err());
    }

    #[test]
    fn detects_only_the_windows_revocation_offline_tls_error() {
        let stderr = b"curl: (35) schannel: CRYPT_E_REVOCATION_OFFLINE (0x80092013)";
        assert!(is_schannel_revocation_offline(Some(35), stderr));
        assert!(!is_schannel_revocation_offline(Some(6), stderr));
        assert!(!is_schannel_revocation_offline(
            Some(35),
            b"curl: (35) schannel: SEC_E_UNTRUSTED_ROOT"
        ));
    }
}

#[cfg(all(test, unix))]
mod delete_store_skin_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_dir(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("{name}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn settings_with_store(store: &Path) -> AppSettings {
        AppSettings {
            codex_theme_store_dir: Some(store.to_string_lossy().into_owned()),
            ..AppSettings::default()
        }
    }

    #[test]
    fn removes_a_real_package() {
        let base = test_dir("delete-real");
        let store = base.join("store");
        let pkg = store.join("good");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("theme.json"), b"{}").unwrap();

        delete_store_skin(&settings_with_store(&store), "good").unwrap();
        assert!(!pkg.exists(), "the real package should be deleted");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn refuses_a_symlink_and_spares_its_target() {
        let base = test_dir("delete-symlink");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();

        // A real package the link resolves to; its manifest would let the
        // follow-through is_dir()/theme.json checks pass without the guard.
        let target = base.join("outside");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("theme.json"), b"{}").unwrap();

        let link = store.join("linky");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = delete_store_skin(&settings_with_store(&store), "linky").unwrap_err();
        assert!(
            matches!(err, AppError::Engine(_)),
            "a symlink must be refused"
        );
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink itself must be left in place",
        );
        assert!(
            target.join("theme.json").is_file(),
            "the link target must survive untouched",
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
