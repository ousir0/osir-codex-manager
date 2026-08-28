//! Local OpenCodex integration used by the multi-model setup flow.
//!
//! The manager owns only the entries it records in its state file. Existing
//! OpenCodex providers and models are preserved. Every write is validated by
//! OpenCodex when available, atomically committed, and paired with the
//! existing single-step backup mechanism.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use directories::BaseDirs;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::rand_core::OsRng;
use rsa::sha2::Sha256 as RsaSha256;
use rsa::{Oaep, RsaPrivateKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use toml_edit::{value, DocumentMut, Item, Table};
use url::Url;
use uuid::Uuid;
use zip::ZipArchive;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::app::{atomic_file, codex_sessions, paths};
use crate::errors::AppError;

const DEFAULT_PORT: u16 = 10100;
const DEFAULT_PROVIDER_ID: &str = "opencodex";
const DEFAULT_VERSION: &str = "2.22.0";
const MANAGED_NODE_VERSION: &str = "22.19.0";
const COMPONENT_MANIFEST_URL: &str = "https://app.osirclaw.com/components/opencodex/index.json";
const COMPONENT_MANIFEST_FALLBACK_URL: &str = "https://raw.githubusercontent.com/ousir0/osir-codex-manager/main/components/opencodex/index.json";
const OSIRAPI_DESKTOP_CONNECT_URL: &str = "https://osirclaw.com/codex-manager/connect";
const OSIRAPI_DESKTOP_EXCHANGE_URL: &str = "https://api.osirclaw.com/api/v1/codex-install/desktop/exchange";
const MAX_ROUTE_COUNT: usize = 32;
const MAX_MODELS_PER_ROUTE: usize = 256;
const MAX_ID_LEN: usize = 96;
const MAX_VALUE_LEN: usize = 4096;
const ROUTE_CHECK_RETRY_DELAYS_MS: [u64; 3] = [400, 1_200, 2_500];
static MANAGER_UPDATE_RECONCILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn configure_background_command(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
}

fn configure_opencodex_environment(command: &mut Command) {
    if let Ok(paths) = integration_paths() {
        if let Some(home) = paths.opencodex_config.parent() {
            command.env("OPENCODEX_HOME", home);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexStatus {
    pub enabled: bool,
    pub installed: bool,
    pub version: Option<String>,
    pub port: u16,
    pub service_state: String,
    pub codex_provider_id: String,
    pub config_path: String,
    pub catalog_path: String,
    pub model_count: usize,
    pub routes: Vec<OpenCodexRoute>,
    pub backup_available: bool,
    pub error: Option<String>,
    pub connection_status: String,
    pub account: Option<OpenCodexAccountSummary>,
    pub environment: OpenCodexEnvironmentStatus,
    pub requires_codex_restart: bool,
}

fn restart_required_path(paths: &IntegrationPaths) -> PathBuf {
    paths.state.with_extension("codex-restart-required")
}

fn restart_applied_path(paths: &IntegrationPaths) -> PathBuf {
    paths.state.with_extension("codex-restart-applied")
}

fn manager_update_reconcile_path(paths: &IntegrationPaths) -> PathBuf {
    paths.state.with_extension("opencodex-reconcile-required")
}

fn manager_runtime_version_path(paths: &IntegrationPaths) -> PathBuf {
    paths.state.with_extension("manager-runtime-version")
}

fn manager_runtime_needs_reconcile(
    current_version: &str,
    recorded_version: Option<&str>,
    marker_exists: bool,
) -> bool {
    marker_exists || recorded_version.map(str::trim) != Some(current_version)
}

fn osir_model_supported_in_codex(route_id: &str, model: &str) -> bool {
    if !route_id.starts_with("osirapi-") {
        return true;
    }
    let normalized = model.trim().to_ascii_lowercase();
    if route_id == "osirapi-gemini" && normalized == "gemini-2.0-flash" {
        return false;
    }
    !normalized.contains("image")
        && !normalized.contains("video")
        && !normalized.contains("veo")
        && !normalized.contains("grok-imagine")
}

fn preferred_osir_model(route_id: &str, models: &[String]) -> Option<String> {
    let preferred: &[&str] = match route_id {
        "osirapi-openai" => &["gpt-5.6-sol", "gpt-5.6", "gpt-5.5"],
        "osirapi-claude" => &["claude-opus-5", "claude-sonnet-5", "claude-opus-4-8"],
        "osirapi-gemini" => &[
            "gemini-3.7-flash",
            "gemini-3.6-flash",
            "gemini-3.5-flash",
            "gemini-2.5-pro",
            "gemini-2.5-flash",
        ],
        "osirapi-grok" => &["grok-4.6", "grok-4.5", "grok-4.3"],
        _ => &[],
    };
    preferred
        .iter()
        .find(|candidate| models.iter().any(|model| model == **candidate))
        .map(|candidate| (*candidate).to_string())
        .or_else(|| models.first().cloned())
}

fn sanitize_osir_routes(routes: &mut [OpenCodexRouteInput]) -> Result<(), AppError> {
    for route in routes.iter_mut() {
        route
            .models
            .retain(|model| osir_model_supported_in_codex(&route.id, model));
        if route.models.is_empty() {
            return Err(AppError::Engine(format!(
                "{} 没有可用于 Codex Responses 文本调用的模型",
                route.label
            )));
        }
        if !route.models.iter().any(|model| model == &route.default_model) {
            route.default_model = preferred_osir_model(&route.id, &route.models)
                .ok_or_else(|| AppError::Engine(format!("{} 没有可用默认模型", route.label)))?;
        }
    }
    Ok(())
}

fn sanitize_saved_osir_config(config: &mut JsonMap<String, JsonValue>) -> Result<bool, AppError> {
    let Some(providers) = config.get_mut("providers").and_then(JsonValue::as_object_mut) else {
        return Ok(false);
    };
    let route_ids = providers
        .keys()
        .filter(|id| id.starts_with("osirapi-"))
        .cloned()
        .collect::<Vec<_>>();
    let mut changed = false;
    let mut removed_routes = BTreeSet::new();
    for route_id in &route_ids {
        let Some(provider) = providers.get_mut(route_id).and_then(JsonValue::as_object_mut) else {
            continue;
        };
        let before = provider
            .get("models")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(JsonValue::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let models = before
            .iter()
            .filter(|model| osir_model_supported_in_codex(route_id, model))
            .cloned()
            .collect::<Vec<_>>();
        if models.is_empty() {
            removed_routes.insert(route_id.clone());
            continue;
        }
        if models != before {
            provider.insert("models".to_string(), json!(models));
            changed = true;
        }
        let default_is_valid = provider
            .get("defaultModel")
            .and_then(JsonValue::as_str)
            .is_some_and(|default| models.iter().any(|model| model == default));
        if !default_is_valid {
            let replacement = preferred_osir_model(route_id, &models)
                .ok_or_else(|| AppError::Engine(format!("{route_id} 没有可用默认模型")))?;
            provider.insert("defaultModel".to_string(), JsonValue::String(replacement));
            changed = true;
        }
    }
    for route_id in &removed_routes {
        providers.remove(route_id);
        changed = true;
    }
    if let Some(models) = config.get_mut("customModels").and_then(JsonValue::as_array_mut) {
        let before = models.len();
        models.retain(|entry| {
            let Some(provider) = entry.get("provider").and_then(JsonValue::as_str) else {
                return true;
            };
            if removed_routes.contains(provider) {
                return false;
            }
            let Some(model) = entry.get("modelId").and_then(JsonValue::as_str) else {
                return true;
            };
            osir_model_supported_in_codex(provider, model)
        });
        changed |= models.len() != before;
    }
    if config
        .get("defaultProvider")
        .and_then(JsonValue::as_str)
        .is_some_and(|provider| removed_routes.contains(provider))
    {
        if let Some(replacement) = route_ids.iter().find(|id| !removed_routes.contains(*id)) {
            config.insert("defaultProvider".to_string(), JsonValue::String(replacement.clone()));
            changed = true;
        }
    }
    Ok(changed)
}

fn record_current_manager_runtime(paths: &IntegrationPaths) -> Result<(), AppError> {
    atomic_file::write_atomic(
        &manager_runtime_version_path(paths),
        env!("CARGO_PKG_VERSION").as_bytes(),
    )
    .map_err(|error| AppError::Internal(format!("记录 Manager 运行版本失败：{error}")))
}

/// Persist the intent to reload OpenCodex after a Manager self-update. The
/// daemon may outlive the Manager process, so the new Manager must explicitly
/// reload it before exposing the saved provider routes as usable.
pub(crate) fn mark_manager_update_pending() -> Result<(), AppError> {
    let paths = integration_paths()?;
    atomic_file::write_atomic(
        &manager_update_reconcile_path(&paths),
        b"manager update pending\n",
    )
    .map_err(|error| AppError::Internal(format!("记录 OpenCodex 更新重载状态失败：{error}")))
}

pub(crate) fn clear_manager_update_pending() {
    if let Ok(paths) = integration_paths() {
        let marker = manager_update_reconcile_path(&paths);
        if marker.is_file() {
            let _ = fs::remove_file(marker);
        }
    }
}

pub(crate) fn reconcile_after_manager_update() -> Result<(), AppError> {
    let lock = MANAGER_UPDATE_RECONCILE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
    let paths = integration_paths()?;
    let marker = manager_update_reconcile_path(&paths);
    let recorded_version = fs::read_to_string(manager_runtime_version_path(&paths)).ok();
    if !manager_runtime_needs_reconcile(
        env!("CARGO_PKG_VERSION"),
        recorded_version.as_deref(),
        marker.is_file(),
    ) {
        return Ok(());
    }

    // There may be no OpenCodex installation yet. The marker is harmless in
    // that case and should not make first-run status fail forever.
    if ocx_program().is_none() {
        record_current_manager_runtime(&paths)?;
        let _ = fs::remove_file(marker);
        return Ok(());
    }
    let mut config = load_config(&paths.opencodex_config)?;
    if sanitize_saved_osir_config(&mut config)? {
        write_json(&paths.opencodex_config, &JsonValue::Object(config))?;
    }
    let state = effective_state(&paths)?;
    if !state.enabled {
        record_current_manager_runtime(&paths)?;
        let _ = fs::remove_file(marker);
        return Ok(());
    }

    restart_service_and_wait_ready()?;
    ocx_output(&["sync"])?;
    if !catalog_has_models(&paths.catalog) {
        return Err(AppError::Engine(
            "Manager 更新后 OpenCodex 没有生成可用模型目录".to_string(),
        ));
    }
    refresh_codex_catalog_binding(&paths)?;
    let config = load_config(&paths.opencodex_config)?;
    let routes = configured_routes_from_config(&config, &state.managed_provider_ids);
    if !routes.is_empty() && !catalog_contains_enabled_routes(&paths.catalog, &routes) {
        return Err(AppError::Engine(
            "Manager 更新后 OpenCodex 未加载全部供应商模型，请点击同步重试".to_string(),
        ));
    }
    let mut failed_routes = Vec::new();
    for route in routes.iter().filter(|route| route.enabled) {
        let check = check_route(&route.id, &route.default_model)?;
        if !check.available {
            failed_routes.push(format!(
                "{}/{}：{}",
                check.route_id, check.model, check.detail
            ));
        }
    }
    if !failed_routes.is_empty() {
        return Err(AppError::Engine(format!(
            "Manager 更新后供应商模型验证未全部通过：{}",
            failed_routes.join("；")
        )));
    }
    if crate::app::codex_theme::codex_running() {
        mark_codex_restart_required_at(&paths)?;
    }
    record_current_manager_runtime(&paths)?;
    fs::remove_file(marker)
        .map_err(|error| AppError::Internal(format!("清除 OpenCodex 更新重载状态失败：{error}")))?;
    Ok(())
}

fn codex_configuration_revision(paths: &IntegrationPaths) -> Option<String> {
    let mut digest = Sha256::new();
    digest.update(fs::read(&paths.codex_config).ok()?);
    if paths.catalog.is_file() {
        digest.update(fs::read(&paths.catalog).ok()?);
    }
    Some(
        digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

fn configuration_requires_restart(
    codex_running: bool,
    opencodex_enabled: bool,
    marker_exists: bool,
    current_revision: Option<&str>,
    applied_revision: Option<&str>,
) -> bool {
    codex_running
        && (marker_exists
            || opencodex_enabled
                && current_revision
                    .is_some_and(|revision| applied_revision != Some(revision)))
}

fn mark_codex_restart_required_at(paths: &IntegrationPaths) -> Result<(), AppError> {
    atomic_file::write_atomic(&restart_required_path(paths), b"codex catalog changed\n")
        .map_err(|error| AppError::Internal(format!("记录 Codex 重启状态失败：{error}")))
}

pub(crate) fn mark_codex_restart_required() -> Result<(), AppError> {
    mark_codex_restart_required_at(&integration_paths()?)
}

pub(crate) fn clear_codex_restart_required() -> Result<(), AppError> {
    let paths = integration_paths()?;
    if let Some(revision) = codex_configuration_revision(&paths) {
        atomic_file::write_atomic(&restart_applied_path(&paths), revision.as_bytes())
            .map_err(|error| AppError::Internal(format!("记录 Codex 已加载配置失败：{error}")))?;
    }
    let marker = restart_required_path(&paths);
    if marker.exists() {
        fs::remove_file(marker)
            .map_err(|error| AppError::Internal(format!("清除 Codex 重启状态失败：{error}")))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexOAuthProgress {
    pub stage: String,
    pub state: String,
    pub step: usize,
    pub total: usize,
    pub title: String,
    pub detail: String,
}

fn oauth_progress(
    stage: &str,
    state: &str,
    step: usize,
    title: &str,
    detail: &str,
) -> OpenCodexOAuthProgress {
    OpenCodexOAuthProgress {
        stage: stage.to_string(),
        state: state.to_string(),
        step,
        total: 4,
        title: title.to_string(),
        detail: detail.to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexEnvironmentStatus {
    pub platform: String,
    pub architecture: String,
    pub supported: bool,
    pub runtime_state: String,
    pub install_strategy: String,
    pub node_version: Option<String>,
    pub npm_available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexAccountSummary {
    #[serde(alias = "user_id")]
    pub user_id: i64,
    #[serde(alias = "display_name")]
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub balance: f64,
    pub subscriptions: Vec<OpenCodexSubscriptionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexSubscriptionSummary {
    pub id: i64,
    #[serde(alias = "group_name")]
    pub group_name: Option<String>,
    pub status: String,
    #[serde(alias = "expires_at")]
    pub expires_at: Option<String>,
    #[serde(alias = "days_remaining")]
    pub days_remaining: i32,
    #[serde(alias = "monthly_used_usd")]
    pub monthly_used_usd: f64,
    #[serde(alias = "monthly_limit_usd")]
    pub monthly_limit_usd: f64,
    #[serde(alias = "monthly_remaining_usd")]
    pub monthly_remaining_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexRoute {
    pub id: String,
    pub label: String,
    pub adapter: String,
    pub base_url: String,
    pub default_model: String,
    pub models: Vec<String>,
    pub enabled: bool,
    pub api_key_configured: bool,
    pub availability: String,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexRouteCheck {
    pub route_id: String,
    pub model: String,
    pub available: bool,
    pub retryable: bool,
    pub detail: String,
    pub checked_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexRouteInput {
    pub id: String,
    pub label: String,
    pub adapter: String,
    pub base_url: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    pub default_model: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodexConfigInput {
    pub enabled: bool,
    pub port: u16,
    pub codex_provider_id: String,
    pub default_route: String,
    #[serde(default)]
    pub routes: Vec<OpenCodexRouteInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ManagedState {
    enabled: bool,
    port: u16,
    codex_provider_id: String,
    #[serde(default)]
    managed_provider_ids: Vec<String>,
    locked_route: Option<String>,
    #[serde(default)]
    route_health: BTreeMap<String, String>,
    connection: Option<OpenCodexAccountSummary>,
    #[serde(default)]
    signed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedemptionState {
    private_key: String,
    public_key: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
struct EncryptedBundle {
    wrapped_key: String,
    iv: String,
    ciphertext: String,
}

#[derive(Debug, Deserialize)]
struct CodexInstallProvider {
    platform: String,
    provider: String,
    api_key: String,
    adapter: String,
    base_url: String,
    models: Vec<String>,
    recommended_model: String,
}

#[derive(Debug, Deserialize)]
struct CodexInstallPayload {
    providers: Vec<CodexInstallProvider>,
    account: Option<OpenCodexAccountSummary>,
}

#[derive(Debug, Deserialize)]
struct RedeemResponse {
    encrypted_bundle: EncryptedBundle,
}

#[derive(Debug, Deserialize)]
struct DesktopExchangeResponse {
    encrypted_bundle: EncryptedBundle,
}

#[derive(Debug)]
struct OAuthCallback {
    code: String,
    state: String,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComponentTarget {
    url: String,
    github_url: String,
    sha256: String,
    #[serde(default)]
    bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ComponentManifest {
    version: String,
    targets: BTreeMap<String, ComponentTarget>,
}

#[derive(Debug, Clone)]
struct IntegrationPaths {
    codex_config: PathBuf,
    catalog: PathBuf,
    opencodex_config: PathBuf,
    state: PathBuf,
}

fn integration_paths() -> Result<IntegrationPaths, AppError> {
    let home = BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| AppError::Internal("无法定位当前用户目录".to_string()))?;
    let codex_home = paths::codex_home_dir()
        .ok_or_else(|| AppError::Internal("无法定位 Codex 配置目录".to_string()))?;
    let state_root = paths::data_dir()
        .ok_or_else(|| AppError::Internal("无法定位 Codex Manager 数据目录".to_string()))?;
    Ok(IntegrationPaths {
        codex_config: codex_home.join("config.toml"),
        catalog: codex_home.join("opencodex-catalog.json"),
        opencodex_config: home.join(".opencodex").join("config.json"),
        state: state_root.join("opencodex").join("managed-state.json"),
    })
}

fn codex_takeover_backup_path(paths: &IntegrationPaths) -> PathBuf {
    paths
        .state
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("codex-config.before-opencodex.toml")
}

fn default_codex_config_bytes(paths: &IntegrationPaths) -> Result<Vec<u8>, AppError> {
    let current = fs::read(&paths.codex_config)
        .map_err(|error| AppError::Internal(format!("读取 Codex config.toml 失败：{error}")))?;
    if !codex_proxy_provider_is_loopback(&paths.codex_config) {
        return Ok(current);
    }
    let backup = codex_takeover_backup_path(paths);
    if backup.is_file() {
        let raw = fs::read(&backup)
            .map_err(|error| AppError::Internal(format!("读取 OpenCodex 接管备份失败：{error}")))?;
        String::from_utf8(raw.clone())
            .map_err(|error| AppError::Engine(format!("OpenCodex 接管备份不是 UTF-8：{error}")))?
            .parse::<DocumentMut>()
            .map_err(|error| AppError::Engine(format!("OpenCodex 接管备份无效：{error}")))?;
        return Ok(raw);
    }

    let raw = String::from_utf8(current)
        .map_err(|error| AppError::Engine(format!("本地代理 config.toml 不是 UTF-8：{error}")))?;
    let mut document = raw
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Engine(format!("config.toml 格式错误：{error}")))?;
    let model = document
        .get("model")
        .and_then(Item::as_str)
        .and_then(|model| model.rsplit('/').next())
        .filter(|model| !model.is_empty())
        .unwrap_or("gpt-5.6-sol")
        .to_string();
    document["model_provider"] = value("osir");
    document["model"] = value(model);
    document.remove("model_catalog_json");
    if !document.contains_key("model_providers") {
        document["model_providers"] = toml_edit::table();
    }
    let providers = document["model_providers"]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("model_providers 必须是 TOML 表".to_string()))?;
    let provider = providers
        .entry("osir")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("model_providers.osir 必须是 TOML 表".to_string()))?;
    provider["name"] = value("OSIR");
    provider["base_url"] = value("https://api.osirclaw.com/v1");
    provider["wire_api"] = value("responses");
    provider["requires_openai_auth"] = value(true);
    Ok(document.to_string().into_bytes())
}

pub fn default_codex_config_candidate() -> Result<String, AppError> {
    let paths = integration_paths()?;
    String::from_utf8(default_codex_config_bytes(&paths)?)
        .map_err(|error| AppError::Engine(format!("默认 Codex 配置不是 UTF-8：{error}")))
}

fn session_routes(routes: &[OpenCodexRouteInput]) -> Vec<codex_sessions::SessionRoute> {
    routes
        .iter()
        .map(|route| codex_sessions::SessionRoute { id: route.id.clone(), models: route.models.clone() })
        .collect()
}

pub fn repair_default_session_index() -> Result<usize, AppError> {
    let paths = integration_paths()?;
    if codex_proxy_provider_is_loopback(&paths.codex_config) || !paths.codex_config.is_file() {
        return Ok(0);
    }
    let target = fs::read(&paths.codex_config)
        .map_err(|error| AppError::Internal(format!("读取当前默认 config.toml 失败：{error}")))?;
    let target_provider = config_provider(&target)?;
    let config = load_config(&paths.opencodex_config).unwrap_or_default();
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let managed_provider_ids = inferred_managed_provider_ids(&config, &models);
    let routes = session_routes(&configured_routes_from_config(&config, &managed_provider_ids));
    codex_sessions::migrate(
        codex_sessions::SessionTarget::Default {
            provider: &target_provider,
            opencodex_provider: DEFAULT_PROVIDER_ID,
        },
        &routes,
    )
}

fn config_provider(raw: &[u8]) -> Result<String, AppError> {
    let document = String::from_utf8(raw.to_vec())
        .map_err(|error| AppError::Engine(format!("Codex 配置不是 UTF-8：{error}")))?
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Engine(format!("Codex 配置无效：{error}")))?;
    Ok(document
        .get("model_provider")
        .and_then(Item::as_str)
        .filter(|provider| !provider.trim().is_empty())
        .unwrap_or("openai")
        .to_string())
}

fn restore_optional_file(path: &Path, bytes: Option<&[u8]>) -> Result<(), AppError> {
    match bytes {
        Some(bytes) => atomic_file::write_atomic(path, bytes)
            .map_err(|error| AppError::Internal(format!("恢复切换前文件失败：{error}"))),
        None => {
            if path.exists() {
                fs::remove_file(path)
                    .map_err(|error| AppError::Internal(format!("清理切换中间文件失败：{error}")))?;
            }
            Ok(())
        }
    }
}

fn codex_proxy_provider_is_loopback(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else { return false };
    let Ok(document) = raw.parse::<DocumentMut>() else { return false };
    if document
        .get("openai_base_url")
        .and_then(Item::as_str)
        .is_some_and(|url| url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:"))
    {
        return true;
    }
    let provider_id = document
        .get("model_provider")
        .and_then(Item::as_str)
        .unwrap_or_default();
    document
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(provider_id))
        .and_then(Item::as_table)
        .and_then(|provider| provider.get("base_url"))
        .and_then(Item::as_str)
        .is_some_and(|url| {
            Url::parse(url)
                .ok()
                .map(|parsed| {
                    let is_loopback = parsed
                        .host_str()
                        .map(|host| matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1"))
                        .unwrap_or(false);
                    is_loopback && parsed.path().ends_with("/v1")
                })
                .unwrap_or(false)
        })
}

fn should_reconcile_codex_ownership(enabled: bool, codex_is_loopback: bool) -> bool {
    (!enabled && codex_is_loopback) || (enabled && !codex_is_loopback)
}

fn preserve_codex_config_before_takeover(
    paths: &IntegrationPaths,
    state: &ManagedState,
) -> Result<(), AppError> {
    let backup = codex_takeover_backup_path(paths);
    if backup.is_file() || state.enabled || !paths.codex_config.is_file() {
        return Ok(());
    }
    if codex_proxy_provider_is_loopback(&paths.codex_config) {
        let previous = atomic_file::backup_path(&paths.codex_config);
        if previous.is_file() {
            fs::copy(previous, &backup).map_err(|error| {
                AppError::Internal(format!("保存已有 Codex 配置备份失败：{error}"))
            })?;
        }
        return Ok(());
    }
    let raw = fs::read(&paths.codex_config)
        .map_err(|error| AppError::Internal(format!("读取接管前 config.toml 失败：{error}")))?;
    atomic_file::write_atomic(&backup, &raw)
        .map_err(|error| AppError::Internal(format!("保存接管前 config.toml 备份失败：{error}")))
}

/// Leave OpenCodex installed, but release ownership of Codex's active provider.
/// This is used whenever the user switches back to single-provider mode.
pub fn disable_for_single_provider() -> Result<(), AppError> {
    let paths = integration_paths()?;
    let mut state = load_state(&paths.state);
    let takeover_active = state.enabled || codex_proxy_provider_is_loopback(&paths.codex_config);
    if !takeover_active {
        repair_default_session_index()?;
        let _ = fs::remove_file(codex_takeover_backup_path(&paths));
        return Ok(());
    }

    let backup = codex_takeover_backup_path(&paths);
    let target = default_codex_config_bytes(&paths)?;
    let target_provider = config_provider(&target)?;
    let opencodex_provider = if state.codex_provider_id.trim().is_empty() {
        DEFAULT_PROVIDER_ID.to_string()
    } else {
        state.codex_provider_id.clone()
    };
    let config = load_config(&paths.opencodex_config).unwrap_or_default();
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let managed_provider_ids = inferred_managed_provider_ids(&config, &models);
    let routes = session_routes(&configured_routes_from_config(&config, &managed_provider_ids));
    let previous_config = fs::read(&paths.codex_config).ok();
    let previous_state = fs::read(&paths.state).ok();
    let previous_backup = fs::read(&backup).ok();

    let result: Result<(), AppError> = (|| {
        atomic_file::write_atomic(&paths.codex_config, &target)
            .map_err(|error| AppError::Internal(format!("恢复默认 config.toml 失败：{error}")))?;
        state.enabled = false;
        state.codex_provider_id.clear();
        write_json(
            &paths.state,
            &serde_json::to_value(state)
                .map_err(|error| AppError::Internal(format!("保存默认配置模式状态失败：{error}")))?,
        )?;
        codex_sessions::migrate(
            codex_sessions::SessionTarget::Default {
                provider: &target_provider,
                opencodex_provider: &opencodex_provider,
            },
            &routes,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = restore_optional_file(&paths.codex_config, previous_config.as_deref());
        let _ = restore_optional_file(&paths.state, previous_state.as_deref());
        let _ = restore_optional_file(&backup, previous_backup.as_deref());
        return Err(AppError::Engine(format!("切换默认配置失败，已恢复原配置：{error}")));
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

fn checked_id(value: &str, label: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ID_LEN {
        return Err(AppError::Engine(format!("{label}长度无效")));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(AppError::Engine(format!("{label}只能包含字母、数字、连字符、下划线或点")));
    }
    Ok(trimmed.to_string())
}

fn checked_text(value: &str, label: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_VALUE_LEN || trimmed.chars().any(char::is_control) {
        return Err(AppError::Engine(format!("{label}无效")));
    }
    Ok(trimmed.to_string())
}

fn checked_url(value: &str) -> Result<String, AppError> {
    let parsed = url::Url::parse(value.trim())
        .map_err(|error| AppError::Engine(format!("Base URL 无效：{error}")))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err(AppError::Engine("Base URL 仅支持 http 或 https".to_string()));
    }
    let host = parsed.host_str().unwrap_or_default();
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if parsed.scheme() == "http" && !loopback {
        return Err(AppError::Engine("非本机 Base URL 必须使用 HTTPS".to_string()));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AppError::Engine("Base URL 不能包含查询参数或片段".to_string()));
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

fn load_state(path: &Path) -> ManagedState {
    let (state, _) = atomic_file::read_with_recovery::<ManagedState>(path);
    state.unwrap_or_default()
}

fn write_json(path: &Path, value: &JsonValue) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| AppError::Internal(format!("序列化 OpenCodex 配置失败：{error}")))?;
    atomic_file::write_atomic(path, &bytes)
        .map_err(|error| AppError::Internal(format!("原子保存 OpenCodex 配置失败：{error}")))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::Internal(format!("收紧 OpenCodex 配置权限失败：{error}")))?;
    let reread = fs::read(path)
        .map_err(|error| AppError::Internal(format!("回读 OpenCodex 配置失败：{error}")))?;
    if reread != bytes {
        return Err(AppError::Internal("OpenCodex 配置回读不一致".to_string()));
    }
    Ok(())
}

fn load_config(path: &Path) -> Result<JsonMap<String, JsonValue>, AppError> {
    if !path.exists() {
        return Ok(JsonMap::new());
    }
    if path.is_symlink() {
        return Err(AppError::Engine("OpenCodex 配置是符号链接，拒绝改写".to_string()));
    }
    let raw = fs::read(path)
        .map_err(|error| AppError::Internal(format!("读取 OpenCodex 配置失败：{error}")))?;
    serde_json::from_slice::<JsonValue>(&raw)
        .map_err(|error| AppError::Engine(format!("OpenCodex 配置不是有效 JSON：{error}")))?
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::Engine("OpenCodex 配置顶层必须是对象".to_string()))
}

fn component_target_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64" | "arm64") => Some("darwin-arm64"),
        ("macos", "x86_64" | "amd64" | "x64") => Some("darwin-x64"),
        ("windows", "aarch64" | "arm64") => Some("windows-arm64"),
        ("windows", "x86_64" | "amd64" | "x64") => Some("windows-x64"),
        ("linux", "aarch64" | "arm64") => Some("linux-arm64"),
        ("linux", "x86_64" | "amd64" | "x64") => Some("linux-x64"),
        _ => None,
    }
}

fn component_target() -> Result<&'static str, AppError> {
    component_target_for(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or(AppError::UnsupportedPlatform)
}

fn managed_component_roots() -> Vec<PathBuf> {
    let Some(data_dir) = paths::data_dir() else { return Vec::new() };
    let Ok(target) = component_target() else { return Vec::new() };
    let components = data_dir.join("opencodex").join("components");
    vec![
        components.join("current").join(target),
        components.join(DEFAULT_VERSION).join(target),
    ]
}

fn managed_component_invocation() -> Option<(String, Vec<String>)> {
    managed_component_roots().into_iter().find_map(|root| {
        let node = root.join(if cfg!(target_os = "windows") { "runtime/node.exe" } else { "runtime/bin/node" });
        let launcher = root.join("opencodex/node_modules/@bitkyc08/opencodex/bin/ocx.mjs");
        (node.is_file() && launcher.is_file())
            .then(|| (node.display().to_string(), vec![launcher.display().to_string()]))
    })
}

fn command_version(program: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    configure_background_command(&mut command);
    configure_opencodex_environment(&mut command);
    let output = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    String::from_utf8(output.stdout).ok().map(|value| value.trim().trim_start_matches('v').to_string()).filter(|value| !value.is_empty())
}

fn executable_candidates(name: &str) -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(name)];
    let home = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    if cfg!(target_os = "windows") {
        for root in [std::env::var_os("APPDATA"), std::env::var_os("NVM_HOME"), std::env::var_os("ProgramFiles")]
            .into_iter()
            .flatten()
        {
            let root = PathBuf::from(root);
            candidates.push(root.join(name));
            candidates.push(root.join("nodejs").join(name));
        }
    } else {
        for root in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
            candidates.push(PathBuf::from(root).join(name));
        }
        if let Some(home) = home {
            for root in [".local/bin", ".volta/bin", ".npm-global/bin"] {
                candidates.push(home.join(root).join(name));
            }
            let nvm_versions = home.join(".nvm/versions/node");
            if let Ok(entries) = fs::read_dir(nvm_versions) {
                let mut versions = entries.flatten().map(|entry| entry.path().join("bin").join(name)).collect::<Vec<_>>();
                versions.sort_by(|left, right| right.cmp(left));
                candidates.extend(versions);
            }
        }
    }
    candidates
}

fn first_command(name: &str, args: &[&str]) -> Option<(PathBuf, String)> {
    executable_candidates(name).into_iter().find_map(|candidate| {
        command_version(&candidate, args).map(|version| (candidate, version))
    })
}

fn node_version() -> Option<String> {
    first_command(if cfg!(target_os = "windows") { "node.exe" } else { "node" }, &["--version"]).map(|(_, version)| version)
}

fn system_node_command() -> Option<(PathBuf, String)> {
    first_command(if cfg!(target_os = "windows") { "node.exe" } else { "node" }, &["--version"])
}

fn node_supported(version: &str) -> bool {
    version.split('.').next().and_then(|value| value.parse::<u32>().ok()).is_some_and(|major| major >= 18)
}

fn npm_available() -> bool {
    system_npm_command().is_some()
}

fn system_npm_command() -> Option<PathBuf> {
    first_command(if cfg!(target_os = "windows") { "npm.cmd" } else { "npm" }, &["--version"]).map(|(path, _)| path)
}

fn node_distribution_target_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64" | "arm64") => Some("darwin-arm64"),
        ("macos", "x86_64" | "amd64" | "x64") => Some("darwin-x64"),
        ("windows", "aarch64" | "arm64") => Some("win-arm64"),
        ("windows", "x86_64" | "amd64" | "x64") => Some("win-x64"),
        ("linux", "aarch64" | "arm64") => Some("linux-arm64"),
        ("linux", "x86_64" | "amd64" | "x64") => Some("linux-x64"),
        _ => None,
    }
}

fn managed_node_root() -> Result<PathBuf, AppError> {
    let target = node_distribution_target_for(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or(AppError::UnsupportedPlatform)?;
    paths::data_dir()
        .map(|dir| dir.join("opencodex").join("node").join(MANAGED_NODE_VERSION).join(target))
        .ok_or_else(|| AppError::Internal("无法定位 OpenCodex Node 运行时目录".to_string()))
}

fn managed_node_executable() -> Option<PathBuf> {
    let root = managed_node_root().ok()?;
    let path = root.join(if cfg!(target_os = "windows") { "node.exe" } else { "bin/node" });
    path.is_file().then_some(path)
}

fn managed_npm_cli() -> Option<PathBuf> {
    let root = managed_node_root().ok()?;
    let path = if cfg!(target_os = "windows") {
        root.join("node_modules/npm/bin/npm-cli.js")
    } else {
        root.join("lib/node_modules/npm/bin/npm-cli.js")
    };
    path.is_file().then_some(path)
}

fn private_npm_invocation() -> Option<(String, Vec<String>)> {
    let runtime = managed_runtime_dir().ok()?;
    let launcher = runtime.join("node_modules/@bitkyc08/opencodex/bin/ocx.mjs");
    if !launcher.is_file() { return None; }
    if let Some(node) = managed_node_executable() {
        return Some((node.display().to_string(), vec![launcher.display().to_string()]));
    }
    let (node, version) = system_node_command()?;
    node_supported(&version).then(|| (node.display().to_string(), vec![launcher.display().to_string()]))
}

fn system_ocx_invocation() -> Option<(String, Vec<String>)> {
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &["ocx.cmd", "opencodex.cmd", "ocx.exe", "opencodex.exe", "ocx", "opencodex"]
    } else {
        &["ocx", "opencodex"]
    };
    candidates.iter().find_map(|candidate| {
        first_command(candidate, &["--version"]).map(|(path, _)| (path.display().to_string(), Vec::new()))
    })
}

fn ocx_invocation() -> Option<(String, Vec<String>)> {
    managed_component_invocation()
        .or_else(private_npm_invocation)
        .or_else(system_ocx_invocation)
}

fn ocx_program() -> Option<String> {
    ocx_invocation().map(|(program, _)| program)
}

fn component_sha256(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path).map_err(|error| AppError::Internal(format!("读取 OpenCodex 组件失败：{error}")))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn download_component(url: &str, path: &Path, expected_bytes: Option<u64>) -> Result<(), AppError> {
    let parsed = Url::parse(url).map_err(|error| AppError::Engine(format!("OpenCodex 组件地址无效：{error}")))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Engine("OpenCodex 组件仅允许通过 HTTPS 下载".to_string()));
    }
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| AppError::Internal(format!("初始化 OpenCodex 下载器失败：{error}")))?
        .get(url)
        .send()
        .map_err(|error| AppError::Engine(format!("下载 OpenCodex 组件失败：{error}")))?;
    if !response.status().is_success() {
        return Err(AppError::Engine(format!("OpenCodex 组件下载失败：HTTP {}", response.status())));
    }
    const MAX_COMPONENT_BYTES: u64 = 512 * 1024 * 1024;
    if response.content_length().is_some_and(|size| size > MAX_COMPONENT_BYTES) {
        return Err(AppError::Engine("OpenCodex 组件大小异常，已停止下载".to_string()));
    }
    let mut output = fs::File::create(path).map_err(|error| AppError::Internal(format!("创建 OpenCodex 下载文件失败：{error}")))?;
    let copied = std::io::copy(&mut response.take(MAX_COMPONENT_BYTES + 1), &mut output)
        .map_err(|error| AppError::Engine(format!("保存 OpenCodex 组件失败：{error}")))?;
    if copied > MAX_COMPONENT_BYTES || expected_bytes.is_some_and(|size| size != copied) {
        let _ = fs::remove_file(path);
        return Err(AppError::Engine("OpenCodex 组件大小校验失败".to_string()));
    }
    Ok(())
}

fn extract_component(zip_path: &Path, destination: &Path) -> Result<(), AppError> {
    let file = fs::File::open(zip_path).map_err(|error| AppError::Internal(format!("打开 OpenCodex 组件失败：{error}")))?;
    let mut archive = ZipArchive::new(file).map_err(|error| AppError::Engine(format!("OpenCodex 组件压缩包无效：{error}")))?;
    let temp = destination.with_extension(format!("tmp-{}", std::process::id()));
    if temp.exists() { fs::remove_dir_all(&temp).ok(); }
    fs::create_dir_all(&temp).map_err(|error| AppError::Internal(format!("创建 OpenCodex 组件目录失败：{error}")))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| AppError::Engine(format!("读取 OpenCodex 组件失败：{error}")))?;
        let Some(relative) = entry.enclosed_name().map(|path| path.to_path_buf()) else { return Err(AppError::Engine("OpenCodex 组件包含不安全路径".to_string())); };
        let target = temp.join(relative);
        if entry.is_dir() { fs::create_dir_all(&target).map_err(|error| AppError::Internal(format!("解压 OpenCodex 组件失败：{error}")))?; continue; }
        if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|error| AppError::Internal(format!("解压 OpenCodex 组件失败：{error}")))?; }
        let mut output = fs::File::create(&target).map_err(|error| AppError::Internal(format!("写入 OpenCodex 组件失败：{error}")))?;
        std::io::copy(&mut entry, &mut output).map_err(|error| AppError::Internal(format!("写入 OpenCodex 组件失败：{error}")))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|error| AppError::Internal(format!("恢复 OpenCodex 组件权限失败：{error}")))?;
        }
    }
    if destination.exists() { fs::remove_dir_all(destination).map_err(|error| AppError::Internal(format!("替换 OpenCodex 组件失败：{error}")))?; }
    fs::rename(temp, destination).map_err(|error| AppError::Internal(format!("启用 OpenCodex 组件失败：{error}")))?;
    Ok(())
}

fn stripped_archive_path(path: &Path) -> Result<Option<PathBuf>, AppError> {
    let components = path.components().collect::<Vec<_>>();
    if components.len() <= 1 { return Ok(None); }
    if components.iter().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(AppError::Engine("Node 运行时压缩包包含不安全路径".to_string()));
    }
    Ok(Some(components.iter().skip(1).fold(PathBuf::new(), |mut result, component| {
        if let Component::Normal(value) = component { result.push(value); }
        result
    })))
}

fn extract_node_archive(archive_path: &Path, destination: &Path) -> Result<(), AppError> {
    let temp = destination.with_extension(format!("tmp-{}", std::process::id()));
    if temp.exists() { fs::remove_dir_all(&temp).ok(); }
    fs::create_dir_all(&temp).map_err(|error| AppError::Internal(format!("创建 Node 运行时目录失败：{error}")))?;
    if archive_path.extension().and_then(|value| value.to_str()) == Some("zip") {
        let file = fs::File::open(archive_path).map_err(|error| AppError::Internal(format!("打开 Node 运行时失败：{error}")))?;
        let mut archive = ZipArchive::new(file).map_err(|error| AppError::Engine(format!("Node 运行时压缩包无效：{error}")))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| AppError::Engine(format!("读取 Node 运行时失败：{error}")))?;
            let enclosed = entry.enclosed_name().ok_or_else(|| AppError::Engine("Node 运行时包含不安全路径".to_string()))?;
            let Some(relative) = stripped_archive_path(&enclosed)? else { continue };
            let target = temp.join(relative);
            if entry.is_dir() { fs::create_dir_all(&target).map_err(|error| AppError::Internal(format!("解压 Node 运行时失败：{error}")))?; continue; }
            if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|error| AppError::Internal(format!("解压 Node 运行时失败：{error}")))?; }
            let mut output = fs::File::create(&target).map_err(|error| AppError::Internal(format!("写入 Node 运行时失败：{error}")))?;
            std::io::copy(&mut entry, &mut output).map_err(|error| AppError::Internal(format!("写入 Node 运行时失败：{error}")))?;
        }
    } else {
        let file = fs::File::open(archive_path).map_err(|error| AppError::Internal(format!("打开 Node 运行时失败：{error}")))?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries().map_err(|error| AppError::Engine(format!("读取 Node 运行时失败：{error}")))? {
            let mut entry = entry.map_err(|error| AppError::Engine(format!("读取 Node 运行时失败：{error}")))?;
            if !entry.header().entry_type().is_file() && !entry.header().entry_type().is_dir() { continue; }
            let path = entry.path().map_err(|error| AppError::Engine(format!("读取 Node 运行时路径失败：{error}")))?;
            let Some(relative) = stripped_archive_path(&path)? else { continue };
            let target = temp.join(relative);
            if entry.header().entry_type().is_dir() { fs::create_dir_all(&target).map_err(|error| AppError::Internal(format!("解压 Node 运行时失败：{error}")))?; continue; }
            if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|error| AppError::Internal(format!("解压 Node 运行时失败：{error}")))?; }
            entry.unpack(&target).map_err(|error| AppError::Internal(format!("解压 Node 运行时失败：{error}")))?;
        }
    }
    if destination.exists() { fs::remove_dir_all(destination).map_err(|error| AppError::Internal(format!("替换 Node 运行时失败：{error}")))?; }
    if let Some(parent) = destination.parent() { fs::create_dir_all(parent).map_err(|error| AppError::Internal(format!("创建 Node 运行时父目录失败：{error}")))?; }
    fs::rename(temp, destination).map_err(|error| AppError::Internal(format!("启用 Node 运行时失败：{error}")))?;
    Ok(())
}

fn install_managed_node() -> Result<(), AppError> {
    if managed_node_executable().is_some() && managed_npm_cli().is_some() { return Ok(()); }
    let target = node_distribution_target_for(std::env::consts::OS, std::env::consts::ARCH).ok_or(AppError::UnsupportedPlatform)?;
    let extension = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
    let filename = format!("node-v{MANAGED_NODE_VERSION}-{target}.{extension}");
    let base_url = format!("https://nodejs.org/dist/v{MANAGED_NODE_VERSION}");
    let data_dir = paths::data_dir().ok_or_else(|| AppError::Internal("无法定位 OpenCodex 下载目录".to_string()))?;
    let downloads = data_dir.join("opencodex").join("downloads");
    fs::create_dir_all(&downloads).map_err(|error| AppError::Internal(format!("创建 OpenCodex 下载目录失败：{error}")))?;
    let checksums = downloads.join(format!("SHASUMS256-{MANAGED_NODE_VERSION}.txt"));
    download_component(&format!("{base_url}/SHASUMS256.txt"), &checksums, None)?;
    let checksum_text = fs::read_to_string(&checksums).map_err(|error| AppError::Internal(format!("读取 Node 校验清单失败：{error}")))?;
    let expected = checksum_text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == filename).then(|| hash.to_string())
    }).ok_or_else(|| AppError::Engine(format!("Node 官方校验清单没有 {filename}")))?;
    let archive = downloads.join(&filename);
    if !archive.is_file() || component_sha256(&archive).ok().as_deref() != Some(expected.as_str()) {
        let temp = archive.with_extension(format!("download-{}", std::process::id()));
        download_component(&format!("{base_url}/{filename}"), &temp, None)?;
        if component_sha256(&temp)? != expected {
            fs::remove_file(&temp).ok();
            return Err(AppError::Engine("Node 运行时 SHA-256 校验失败".to_string()));
        }
        fs::rename(temp, &archive).map_err(|error| AppError::Internal(format!("保存 Node 运行时失败：{error}")))?;
    }
    let destination = managed_node_root()?;
    extract_node_archive(&archive, &destination)?;
    if managed_node_executable().is_none() || managed_npm_cli().is_none() {
        return Err(AppError::Engine("Node 运行时安装后不完整".to_string()));
    }
    Ok(())
}

fn install_component_from_manifest() -> Result<(), AppError> {
    let data_dir = paths::data_dir().ok_or_else(|| AppError::Internal("无法定位 OpenCodex 组件目录".to_string()))?;
    let component_target = component_target()?;
    let manifest_path = data_dir.join("opencodex").join("component-manifest.json");
    if let Some(parent) = manifest_path.parent() { fs::create_dir_all(parent).ok(); }
    let manifest_bytes = [COMPONENT_MANIFEST_URL, COMPONENT_MANIFEST_FALLBACK_URL].iter().find_map(|url| {
        let temp = manifest_path.with_extension(format!("download-{}", std::process::id()));
        if download_component(url, &temp, None).is_ok() { let bytes = fs::read(&temp).ok(); fs::remove_file(&temp).ok(); bytes } else { None }
    }).ok_or_else(|| AppError::Engine("暂时无法获取 OpenCodex 组件清单".to_string()))?;
    atomic_file::write_atomic(&manifest_path, &manifest_bytes).map_err(|error| AppError::Internal(format!("保存 OpenCodex 组件清单失败：{error}")))?;
    let manifest: ComponentManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| AppError::Engine(format!("OpenCodex 组件清单无效：{error}")))?;
    let target = manifest.targets.get(component_target).ok_or_else(|| AppError::Engine(format!("组件服务尚未发布 {component_target} 的 OpenCodex 包")))?;
    let archive = data_dir.join("opencodex").join(format!("component-{}-{}.zip", component_target, manifest.version));
    if !archive.is_file() || component_sha256(&archive).ok().as_deref() != Some(target.sha256.as_str()) {
        let temp = archive.with_extension(format!("download-{}", std::process::id()));
        download_component(&target.url, &temp, target.bytes)
            .or_else(|_| download_component(&target.github_url, &temp, target.bytes))?;
        if component_sha256(&temp)? != target.sha256 { fs::remove_file(&temp).ok(); return Err(AppError::Engine("OpenCodex 组件 SHA-256 校验失败".to_string())); }
        fs::rename(temp, &archive).map_err(|error| AppError::Internal(format!("保存 OpenCodex 组件失败：{error}")))?;
    }
    let destination = data_dir.join("opencodex").join("components").join("current").join(component_target);
    extract_component(&archive, &destination)
}

fn managed_runtime_dir() -> Result<PathBuf, AppError> {
    paths::data_dir()
        .map(|dir| dir.join("opencodex").join("runtime").join(DEFAULT_VERSION))
        .ok_or_else(|| AppError::Internal("无法定位 OpenCodex 组件目录".to_string()))
}

fn new_redemption_state() -> Result<RedemptionState, AppError> {
    let private = RsaPrivateKey::new(&mut OsRng, 3072)
        .map_err(|error| AppError::Internal(format!("生成连接加密密钥失败：{error}")))?;
    Ok(RedemptionState {
        private_key: private
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|error| AppError::Internal(format!("编码连接私钥失败：{error}")))?
            .to_string(),
        public_key: private
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|error| AppError::Internal(format!("编码连接公钥失败：{error}")))?,
        idempotency_key: Uuid::new_v4().to_string(),
    })
}

fn redemption_path(paths: &IntegrationPaths, ticket: &str) -> PathBuf {
    let digest = Sha256::digest(ticket.as_bytes());
    let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    paths
        .state
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("redemptions")
        .join(format!("{hash}.json"))
}

fn load_or_create_redemption(paths: &IntegrationPaths, ticket: &str) -> Result<(RedemptionState, PathBuf), AppError> {
    let path = redemption_path(paths, ticket);
    if path.is_file() {
        let raw = fs::read(&path)
            .map_err(|error| AppError::Internal(format!("读取连接码状态失败：{error}")))?;
        let existing = serde_json::from_slice::<RedemptionState>(&raw)
            .map_err(|error| AppError::Engine(format!("连接码状态无效：{error}")))?;
        if !existing.private_key.is_empty() && !existing.public_key.is_empty() && !existing.idempotency_key.is_empty() {
            return Ok((existing, path));
        }
    }
    let state = new_redemption_state()?;
    write_json(
        &path,
        &serde_json::to_value(&state)
            .map_err(|error| AppError::Internal(format!("保存连接码状态失败：{error}")))?,
    )?;
    Ok((state, path))
}

fn extract_osir_ticket(value: &str) -> Result<String, AppError> {
    let found = value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .find(|token| {
            token.len() == 52
                && token.starts_with("ocx_")
                && token.chars().skip(4).all(|ch| ch.is_ascii_hexdigit())
        });
    found
        .map(str::to_string)
        .ok_or_else(|| AppError::Engine("未识别到有效的 OSIRAPI 连接码".to_string()))
}

fn redeem_ticket(ticket: &str, state: &RedemptionState) -> Result<RedeemResponse, AppError> {
    let payload = serde_json::to_vec(&json!({
        "ticket": ticket,
        "client_public_key": state.public_key,
        "idempotency_key": state.idempotency_key,
        "installer_version": "codex-manager-0.5.3",
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
    }))
    .map_err(|error| AppError::Internal(format!("生成 OSIRAPI 连接请求失败：{error}")))?;
    let response = post_json(
        "https://api.osirclaw.com/api/v1/codex-install/tickets/redeem",
        &payload,
        "OSIRAPI 连接码兑换失败",
    )?;
    let data = response.get("data").cloned().unwrap_or(response);
    serde_json::from_value::<RedeemResponse>(data)
        .map_err(|error| AppError::Engine(format!("OSIRAPI 连接配置格式无效：{error}")))
}

fn exchange_osir_oauth(
    authorization_code: &str,
    state: &str,
    redirect_uri: &str,
    code_verifier: &str,
    redemption: &RedemptionState,
) -> Result<DesktopExchangeResponse, AppError> {
    let payload = serde_json::to_vec(&json!({
        "authorization_code": authorization_code,
        "state": state,
        "redirect_uri": redirect_uri,
        "code_verifier": code_verifier,
        "client_public_key": redemption.public_key,
        "idempotency_key": redemption.idempotency_key,
        "installer_version": format!("codex-manager-{}", env!("CARGO_PKG_VERSION")),
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
    }))
    .map_err(|error| AppError::Internal(format!("生成 OSIRAPI OAuth 请求失败：{error}")))?;
    let response = post_json(
        OSIRAPI_DESKTOP_EXCHANGE_URL,
        &payload,
        "OSIRAPI OAuth 兑换失败",
    )?;
    let data = response.get("data").cloned().unwrap_or(response);
    serde_json::from_value::<DesktopExchangeResponse>(data)
        .map_err(|error| AppError::Engine(format!("OSIRAPI OAuth 配置格式无效：{error}")))
}

fn post_json(endpoint: &str, payload: &[u8], fallback_error: &str) -> Result<JsonValue, AppError> {
    let parsed = Url::parse(endpoint).map_err(|error| AppError::Internal(format!("OSIRAPI 地址无效：{error}")))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Engine("OSIRAPI 请求仅允许通过 HTTPS".to_string()));
    }
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| AppError::Internal(format!("初始化 OSIRAPI 请求失败：{error}")))?
        .post(endpoint)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .body(payload.to_vec())
        .send()
        .map_err(|error| AppError::Engine(format!("{fallback_error}：{error}")))?;
    let status = response.status();
    let body = response
        .json::<JsonValue>()
        .map_err(|error| AppError::Engine(format!("{fallback_error}：响应格式无效：{error}")))?;
    if !status.is_success() {
        let message = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(JsonValue::as_str)
            .unwrap_or(fallback_error);
        return Err(AppError::Engine(message.to_string()));
    }
    Ok(body)
}

fn decrypt_bundle(state: &RedemptionState, encrypted: EncryptedBundle) -> Result<CodexInstallPayload, AppError> {
    let private = RsaPrivateKey::from_pkcs8_pem(&state.private_key)
        .map_err(|error| AppError::Internal(format!("读取连接私钥失败：{error}")))?;
    let decode = |value: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| AppError::Engine("OSIRAPI 加密配置编码无效".to_string()))
    };
    let aes_key = private
        .decrypt(Oaep::new::<RsaSha256>(), &decode(&encrypted.wrapped_key)?)
        .map_err(|_| AppError::Engine("OSIRAPI 加密配置无法解密".to_string()))?;
    let iv = decode(&encrypted.iv)?;
    if iv.len() != 12 {
        return Err(AppError::Engine("OSIRAPI 加密配置 IV 无效".to_string()));
    }
    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|_| AppError::Engine("OSIRAPI 加密配置密钥无效".to_string()))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), decode(&encrypted.ciphertext)?.as_ref())
        .map_err(|_| AppError::Engine("OSIRAPI 加密配置校验失败".to_string()))?;
    serde_json::from_slice::<CodexInstallPayload>(&plaintext)
        .map_err(|error| AppError::Engine(format!("OSIRAPI 解密后的配置无效：{error}")))
}

fn platform_label(platform: &str) -> &str {
    match platform {
        "openai" => "GPT",
        "anthropic" => "Claude",
        "gemini" => "Gemini",
        "grok" => "Grok",
        _ => "OSIR",
    }
}

fn validate_codex_install_payload(payload: &CodexInstallPayload) -> Result<(), AppError> {
    if payload.providers.is_empty() || payload.providers.len() > MAX_ROUTE_COUNT {
        return Err(AppError::Engine(format!(
            "OSIRAPI 返回的订阅路由数量无效：实际收到 {} 条",
            payload.providers.len()
        )));
    }
    let mut platforms = BTreeSet::new();
    let mut provider_ids = BTreeSet::new();
    for provider in &payload.providers {
        let platform = provider.platform.trim();
        let provider_id = checked_id(&provider.provider, "OSIRAPI Provider ID")?;
        if platform.is_empty() || !platforms.insert(platform) || !provider_ids.insert(provider_id) {
            return Err(AppError::Engine("OSIRAPI 返回了重复或无效的订阅平台路由".to_string()));
        }
        if provider.api_key.trim().is_empty() || provider.models.is_empty() {
            return Err(AppError::Engine(format!(
                "OSIRAPI 的 {} 订阅路由缺少密钥或模型",
                platform_label(platform)
            )));
        }
        if provider.adapter.trim() != "openai-responses"
            || !provider.models.iter().any(|model| model == &provider.recommended_model)
        {
            return Err(AppError::Engine(format!(
                "OSIRAPI 的 {} 订阅路由格式无效",
                platform_label(platform)
            )));
        }
    }
    Ok(())
}

fn timestamp_marker() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn random_urlsafe_value() -> String {
    let mut bytes = [0u8; 32];
    let mut rng = OsRng;
    rsa::rand_core::RngCore::fill_bytes(&mut rng, &mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn open_external_browser(url: &str) -> Result<(), AppError> {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
    result
        .map(|_| ())
        .map_err(|error| AppError::Engine(format!("无法打开 OSIRAPI 浏览器授权页：{error}")))
}

fn callback_http_response(stream: &mut std::net::TcpStream, success: bool, message: &str) {
    let accent = if success { "#36d399" } else { "#fb7185" };
    let icon = if success { "✓" } else { "!" };
    let title = if success { "授权回调已收到" } else { "授权没有完成" };
    let second_step = if success { "回调已收到" } else { "回调未完成" };
    let second_icon = if success { "✓" } else { "!" };
    let body = format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title} · Codex Manager</title></head><body style=\"margin:0;min-height:100vh;background:#0b1020;color:#eef2ff;font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;padding:24px;box-sizing:border-box\"><main style=\"width:min(520px,100%);background:linear-gradient(145deg,#17213b,#10172c);border:1px solid rgba(148,163,184,.22);border-radius:28px;padding:34px;box-shadow:0 24px 80px rgba(0,0,0,.38);box-sizing:border-box\"><div style=\"display:flex;align-items:center;gap:10px;font-size:13px;letter-spacing:.16em;color:#a5b4fc;font-weight:700\"><span style=\"width:28px;height:28px;border-radius:9px;background:#6366f1;color:white;display:inline-flex;align-items:center;justify-content:center;font-size:12px;letter-spacing:0\">CX</span> CODEX MANAGER</div><div style=\"width:72px;height:72px;border-radius:24px;background:{accent}22;color:{accent};display:flex;align-items:center;justify-content:center;font-size:42px;font-weight:700;margin:38px 0 22px\">{icon}</div><h1 style=\"font-size:30px;line-height:1.2;margin:0 0 12px;letter-spacing:-.03em\">{title}</h1><p style=\"font-size:16px;line-height:1.7;color:#cbd5e1;margin:0\">{message}</p><section style=\"display:grid;gap:10px;margin-top:28px\"><div style=\"display:flex;align-items:center;gap:12px;padding:14px 16px;border-radius:16px;background:rgba(255,255,255,.06)\"><b style=\"color:#36d399\">✓</b><span>浏览器登录 OSIRAPI</span><small style=\"margin-left:auto;color:#94a3b8\">已完成</small></div><div style=\"display:flex;align-items:center;gap:12px;padding:14px 16px;border-radius:16px;background:rgba(255,255,255,.06)\"><b style=\"color:{accent}\">{second_icon}</b><span>返回 Codex Manager</span><small style=\"margin-left:auto;color:#94a3b8\">{second_step}</small></div><div style=\"display:flex;align-items:center;gap:12px;padding:14px 16px;border-radius:16px;background:rgba(255,255,255,.06)\"><b style=\"color:#fbbf24\">…</b><span>安装并同步本地模型</span><small style=\"margin-left:auto;color:#94a3b8\">请看管理器</small></div></section><p style=\"font-size:13px;line-height:1.6;color:#94a3b8;margin:24px 0 0\">{}</p></main><script>setTimeout(function(){{try{{window.close()}}catch(_){{}}}},1800)</script></body></html>",
        if success { "请返回 Codex Manager，等待模型配置完成。此页面可以关闭。" } else { "请返回 Codex Manager 查看错误信息并重新尝试。" },
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn wait_for_oauth_callback(listener: TcpListener, expected_state: &str) -> Result<OAuthCallback, AppError> {
    listener
        .set_nonblocking(true)
        .map_err(|error| AppError::Internal(format!("设置 OAuth 回调监听失败：{error}")))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0u8; 8192];
                let size = stream
                    .read(&mut buffer)
                    .map_err(|error| AppError::Engine(format!("读取 OAuth 回调失败：{error}")))?;
                let request = String::from_utf8_lossy(&buffer[..size]);
                let Some(target) = request
                    .lines()
                    .next()
                    .and_then(|line| line.strip_prefix("GET "))
                    .and_then(|line| line.split_whitespace().next())
                else {
                    callback_http_response(&mut stream, false, "授权回调格式无效，请返回管理器重试。");
                    continue;
                };
                let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
                    callback_http_response(&mut stream, false, "授权回调地址无效，请返回管理器重试。");
                    continue;
                };
                if url.path() != "/oauth/callback" {
                    callback_http_response(&mut stream, false, "授权回调路径无效，请返回管理器重试。");
                    continue;
                }
                let params = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
                let state = params.get("state").cloned().unwrap_or_default();
                if state != expected_state {
                    callback_http_response(&mut stream, false, "授权状态不匹配，请返回管理器重试。");
                    continue;
                }
                let error = params.get("error").cloned();
                let code = params.get("code").cloned().unwrap_or_default();
                callback_http_response(
                    &mut stream,
                    error.is_none(),
                    if error.is_none() {
                        "授权回调已收到，Manager 正在继续完成模型配置。"
                    } else {
                        "OSIRAPI 已取消或拒绝本次授权，请返回 Manager 重试。"
                    },
                );
                return Ok(OAuthCallback { code, state, error });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(AppError::Engine("OSIRAPI 浏览器授权超时，请重新连接".to_string()));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(AppError::Engine(format!("OAuth 回调监听失败：{error}"))),
        }
    }
}

pub fn select_route(route_id: &str, model: &str) -> Result<OpenCodexStatus, AppError> {
    let paths = integration_paths()?;
    let state = effective_state(&paths)?;
    if !state.enabled {
        return Err(AppError::Engine("OpenCodex 多模型尚未启用，请先保存并同步配置".to_string()));
    }
    let route_id = checked_id(route_id, "路由 ID")?;
    let model = checked_text(model, "模型名称")?;
    if !state.managed_provider_ids.iter().any(|id| id == &route_id) {
        return Err(AppError::Engine("只能锁定 Manager 管理的模型路由".to_string()));
    }
    let mut config = load_config(&paths.opencodex_config)?;
    let provider = config
        .get_mut("providers")
        .and_then(JsonValue::as_object_mut)
        .and_then(|providers| providers.get_mut(&route_id))
        .and_then(JsonValue::as_object_mut)
        .ok_or_else(|| AppError::Engine("模型路由不存在或已被删除".to_string()))?;
    provider.insert("defaultModel".to_string(), JsonValue::String(model.clone()));
    config.insert("defaultProvider".to_string(), JsonValue::String(route_id.clone()));
    let next = JsonValue::Object(config);
    validate_candidate(&paths.opencodex_config, &next)?;
    write_json(&paths.opencodex_config, &next)?;
    let port = state.port.max(1);
    let codex_provider_id = if state.codex_provider_id.is_empty() {
        DEFAULT_PROVIDER_ID.to_string()
    } else {
        state.codex_provider_id.clone()
    };
    write_codex_proxy_config(&paths.codex_config, &paths.catalog, &codex_provider_id, port, &format!("{route_id}/{model}"))?;
    let next_state = ManagedState { enabled: true, port, codex_provider_id, managed_provider_ids: state.managed_provider_ids, locked_route: Some(route_id), route_health: state.route_health, connection: state.connection, signed_out: state.signed_out };
    write_json(&paths.state, &serde_json::to_value(next_state).map_err(|error| AppError::Internal(format!("保存锁定路由失败：{error}")))?)?;
    status_at(&paths)
}

pub fn remove_model(route_id: &str, model: &str) -> Result<OpenCodexStatus, AppError> {
    let paths = integration_paths()?;
    let state = effective_state(&paths)?;
    if !state.enabled {
        return Err(AppError::Engine("OpenCodex 多模型尚未启用，请先保存并同步配置".to_string()));
    }
    let route_id = checked_id(route_id, "路由 ID")?;
    let model = checked_text(model, "模型名称")?;
    if !state.managed_provider_ids.iter().any(|id| id == &route_id) {
        return Err(AppError::Engine("只能管理 Manager 接管的模型路由".to_string()));
    }
    let mut config = load_config(&paths.opencodex_config)?;
    let models = config
        .get_mut("customModels")
        .and_then(JsonValue::as_array_mut)
        .ok_or_else(|| AppError::Engine("OpenCodex 模型目录不存在".to_string()))?;
    let before = models.len();
    models.retain(|entry| {
        !(entry.get("provider").and_then(JsonValue::as_str) == Some(&route_id)
            && entry.get("modelId").and_then(JsonValue::as_str) == Some(&model))
    });
    if models.len() == before {
        return Err(AppError::Engine("要移除的模型不存在".to_string()));
    }
    let remaining_managed = models
        .iter()
        .filter(|entry| {
            entry
                .get("provider")
                .and_then(JsonValue::as_str)
                .is_some_and(|provider| state.managed_provider_ids.iter().any(|id| id == provider))
        })
        .count();
    if remaining_managed == 0 {
        return Err(AppError::Engine("至少保留一个可用模型，避免 Codex 选择器为空".to_string()));
    }
    let provider_models = models
        .iter()
        .filter(|entry| entry.get("provider").and_then(JsonValue::as_str) == Some(&route_id))
        .filter_map(|entry| entry.get("modelId").and_then(JsonValue::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let providers = config
        .get_mut("providers")
        .and_then(JsonValue::as_object_mut)
        .ok_or_else(|| AppError::Engine("OpenCodex 路由配置不存在".to_string()))?;
    if provider_models.is_empty() {
        providers.remove(&route_id);
    } else if let Some(provider) = providers.get_mut(&route_id).and_then(JsonValue::as_object_mut) {
        let current_default = provider.get("defaultModel").and_then(JsonValue::as_str);
        if current_default == Some(model.as_str()) {
            provider.insert("defaultModel".to_string(), JsonValue::String(provider_models[0].clone()));
        }
    }
    let managed_provider_ids = state
        .managed_provider_ids
        .iter()
        .filter(|id| providers.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let default_route = inferred_default_route(&config, &managed_provider_ids)
        .ok_or_else(|| AppError::Engine("移除后没有可用的默认模型路由".to_string()))?;
    config.insert("defaultProvider".to_string(), JsonValue::String(default_route.split('/').next().unwrap_or_default().to_string()));
    let next = JsonValue::Object(config);
    validate_candidate(&paths.opencodex_config, &next)?;
    write_json(&paths.opencodex_config, &next)?;
    write_codex_proxy_config(
        &paths.codex_config,
        &paths.catalog,
        &state.codex_provider_id,
        state.port.max(1),
        &default_route,
    )?;
    if let Err(error) = ocx_output(&["sync"]) {
        let _ = restore();
        return Err(error);
    }
    let locked_route = state
        .locked_route
        .filter(|locked| managed_provider_ids.iter().any(|id| id == locked));
    let route_health = state
        .route_health
        .into_iter()
        .filter(|(key, _)| managed_provider_ids.iter().any(|id| key.starts_with(&format!("{id}/"))))
        .collect();
    write_json(
        &paths.state,
        &serde_json::to_value(ManagedState {
            enabled: true,
            port: state.port.max(1),
            codex_provider_id: state.codex_provider_id,
            managed_provider_ids,
            locked_route,
            route_health,
            connection: state.connection,
            signed_out: state.signed_out,
        })
        .map_err(|error| AppError::Internal(format!("保存模型移除状态失败：{error}")))?,
    )?;
    status_at(&paths)
}

fn is_transient_route_check_error(error: &AppError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    ["status 429", "status 502", "status 503", "status 504", "(429)", "(502)", "(503)", "(504)"]
        .iter()
        .any(|marker| message.contains(marker))
        || [
            "bad gateway",
            "service unavailable",
            "gateway timeout",
            "connection reset",
            "connection refused",
            "timed out",
            "timeout",
            "temporarily unavailable",
            "temporary failure",
            "server overloaded",
        ]
        .iter()
        .any(|marker| message.contains(marker))
}

fn route_check_with_retry<F>(mut attempt: F, retry_delays: &[Duration]) -> Result<Vec<u8>, AppError>
where
    F: FnMut() -> Result<Vec<u8>, AppError>,
{
    let mut retry_index = 0;
    loop {
        match attempt() {
            Ok(output) => return Ok(output),
            Err(error) if is_transient_route_check_error(&error) && retry_index < retry_delays.len() => {
                thread::sleep(retry_delays[retry_index]);
                retry_index += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn check_route(route_id: &str, model: &str) -> Result<OpenCodexRouteCheck, AppError> {
    let route_id = checked_id(route_id, "路由 ID")?;
    let model = checked_text(model, "模型名称")?;
    let route = format!("{route_id}/{model}");
    let retry_delays = ROUTE_CHECK_RETRY_DELAYS_MS.map(Duration::from_millis);
    let check = match route_check_with_retry(
        || ocx_output(&["access", "test", &route, "--protocol", "responses", "--json"]),
        &retry_delays,
    ) {
        Ok(_) => OpenCodexRouteCheck { route_id: route_id.clone(), model: model.clone(), available: true, retryable: false, detail: "路由验证成功".to_string(), checked_at: timestamp_marker() },
        Err(error) => {
            let retryable = is_transient_route_check_error(&error);
            OpenCodexRouteCheck { route_id: route_id.clone(), model: model.clone(), available: false, retryable, detail: error.to_string(), checked_at: timestamp_marker() }
        }
    };
    if let Ok(paths) = integration_paths() {
        let mut state = effective_state(&paths).unwrap_or_else(|_| load_state(&paths.state));
        let health = if check.available { "verified" } else if check.retryable { "degraded" } else { "offline" };
        state.route_health.insert(route, health.to_string());
        if let Ok(value) = serde_json::to_value(state) {
            let _ = write_json(&paths.state, &value);
        }
    }
    Ok(check)
}

pub fn ensure_ready_for_codex() -> Result<(), AppError> {
    reconcile_after_manager_update()?;
    let current = status()?;
    if !current.enabled {
        return Ok(());
    }
    let paths = integration_paths()?;
    if current.service_state == "ready" && current.model_count > 0 && catalog_has_models(&paths.catalog) {
        return Ok(());
    }
    let recovered = start()?;
    if recovered.service_state != "ready" || recovered.model_count == 0 || !catalog_has_models(&paths.catalog) {
        return Err(AppError::Engine("OpenCodex 多模型已启用，但服务未 ready；为避免 Codex 启动后不可用，已阻止启动。请先修复或恢复备份。".to_string()));
    }
    Ok(())
}

fn apply_codex_install_payload_with_progress<F>(
    payload: CodexInstallPayload,
    progress: &F,
) -> Result<OpenCodexStatus, AppError>
where
    F: Fn(OpenCodexOAuthProgress),
{
    validate_codex_install_payload(&payload)?;
    let account = payload.account.clone();
    let mut routes = payload
        .providers
        .into_iter()
        .map(|provider| OpenCodexRouteInput {
            id: provider.provider,
            label: platform_label(&provider.platform).to_string(),
            adapter: provider.adapter,
            base_url: provider.base_url,
            api_key: Some(provider.api_key),
            models: provider.models,
            default_model: provider.recommended_model,
            enabled: true,
        })
        .collect::<Vec<_>>();
    sanitize_osir_routes(&mut routes)?;
    let default_route = routes
        .iter()
        .find(|route| route.id.contains("openai"))
        .or_else(|| routes.first())
        .map(|route| format!("{}/{}", route.id, route.default_model))
        .ok_or_else(|| AppError::Engine("OSIRAPI 未返回默认模型".to_string()))?;
    progress(oauth_progress(
        "config",
        "running",
        3,
        "正在写入模型配置",
        "保存订阅 Key、模型路由和 Codex 模型目录。",
    ));
    let routes_to_verify = routes.clone();
    let configured = save(OpenCodexConfigInput {
        enabled: true,
        port: DEFAULT_PORT,
        codex_provider_id: DEFAULT_PROVIDER_ID.to_string(),
        default_route,
        routes,
    })?;
    let paths = integration_paths()?;
    let mut state = load_state(&paths.state);
    state.connection = account;
    state.signed_out = false;
    write_json(
        &paths.state,
        &serde_json::to_value(state)
            .map_err(|error| AppError::Internal(format!("保存 OSIRAPI 连接状态失败：{error}")))?,
    )?;
    progress(oauth_progress(
        "verify",
        "running",
        4,
        "正在验证订阅模型",
        "确认每个供应商的推荐模型都能通过 OpenCodex 正确路由。",
    ));
    let mut checks = Vec::new();
    for route in routes_to_verify.iter().filter(|route| route.enabled) {
        checks.push(check_route(&route.id, &route.default_model)?);
    }
    let mut verified = status()?;
    verified.error = configured.error;
    let failures = checks.iter().filter(|check| !check.available).collect::<Vec<_>>();
    if !failures.is_empty() {
        let detail = failures
            .iter()
            .map(|check| format!("{}/{}：{}", check.route_id, check.model, check.detail))
            .collect::<Vec<_>>()
            .join("；");
        if failures.iter().all(|check| check.retryable) {
            verified.error = Some(format!("授权和模型同步已完成，但部分供应商遇到临时网络异常：{detail}"));
        } else {
            verified.connection_status = "error".to_string();
            verified.error = Some(format!("模型已同步，但部分供应商路由验证失败：{detail}"));
        }
    }
    Ok(verified)
}

fn apply_codex_install_payload(payload: CodexInstallPayload) -> Result<OpenCodexStatus, AppError> {
    apply_codex_install_payload_with_progress(payload, &|_| {})
}

pub fn connect_osir_code(code: &str) -> Result<OpenCodexStatus, AppError> {
    let ticket = extract_osir_ticket(code)?;
    if ocx_program().is_none() {
        install()?;
    }
    let paths = integration_paths()?;
    let (redemption, redemption_file) = load_or_create_redemption(&paths, &ticket)?;
    let response = redeem_ticket(&ticket, &redemption)?;
    let payload = decrypt_bundle(&redemption, response.encrypted_bundle)?;
    let status = apply_codex_install_payload(payload)?;
    let _ = fs::remove_file(redemption_file);
    Ok(status)
}

pub fn connect_osir_oauth_with_progress<F>(progress: F) -> Result<OpenCodexStatus, AppError>
where
    F: Fn(OpenCodexOAuthProgress),
{
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| AppError::Engine(format!("无法启动 OAuth 本机回调：{error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| AppError::Internal(format!("读取 OAuth 回调端口失败：{error}")))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
    let state = random_urlsafe_value();
    let code_verifier = random_urlsafe_value();
    let code_challenge = pkce_challenge(&code_verifier);
    log::info!("OSIRAPI desktop OAuth started callback_port={port}");
    let mut authorization_url = Url::parse(OSIRAPI_DESKTOP_CONNECT_URL)
        .map_err(|error| AppError::Internal(format!("OSIRAPI 授权地址无效：{error}")))?;
    authorization_url.query_pairs_mut()
        .append_pair("state", &state)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256");
    open_external_browser(authorization_url.as_str())?;
    progress(oauth_progress(
        "browser",
        "running",
        0,
        "等待浏览器授权",
        "请在浏览器完成登录；成功后标签页会自动关闭。",
    ));
    let callback = wait_for_oauth_callback(listener, &state)?;
    log::info!("OSIRAPI desktop OAuth callback received");
    if let Some(error) = callback.error.filter(|value| !value.is_empty()) {
        return Err(AppError::Engine(format!("OSIRAPI 授权未完成：{error}")));
    }
    if callback.code.is_empty() || callback.state != state {
        return Err(AppError::Engine("OSIRAPI 授权回调无效，请重新连接".to_string()));
    }
    progress(oauth_progress(
        "exchange",
        "running",
        1,
        "授权成功，正在读取账户",
        "正在获取订阅、专用 Key 和可用模型。",
    ));
    let redemption = new_redemption_state()?;
    let response = exchange_osir_oauth(
        &callback.code,
        &callback.state,
        &redirect_uri,
        &code_verifier,
        &redemption,
    )?;
    let payload = decrypt_bundle(&redemption, response.encrypted_bundle)?;
    if payload.account.is_none() {
        return Err(AppError::Engine(
            "OSIRAPI 授权成功，但服务端未返回账户摘要；已停止写入本地配置，请重新连接"
                .to_string(),
        ));
    }
    log::info!(
        "OSIRAPI desktop OAuth exchange decoded providers={} account_present=true",
        payload.providers.len()
    );
    progress(oauth_progress(
        "runtime",
        "running",
        2,
        "正在准备本机组件",
        "检查并启动 OpenCodex；首次使用可能需要安装运行时。",
    ));
    if ocx_program().is_none() {
        install()?;
    } else if status()?.service_state != "ready" {
        start()?;
    }
    let status = apply_codex_install_payload_with_progress(payload, &progress)?;
    log::info!(
        "OSIRAPI desktop OAuth applied connection_status={} account_present={} routes={} models={}",
        status.connection_status,
        status.account.is_some(),
        status.routes.len(),
        status.model_count
    );
    progress(oauth_progress(
        "complete",
        "success",
        4,
        "连接完成",
        "账户、模型配置和默认路由均已就绪。",
    ));
    Ok(status)
}

pub fn connect_osir_oauth() -> Result<OpenCodexStatus, AppError> {
    connect_osir_oauth_with_progress(|_| {})
}

pub fn install() -> Result<OpenCodexStatus, AppError> {
    if ocx_program().is_some() {
        return start();
    }
    if install_component_from_manifest().is_ok() && ocx_program().is_some() {
        return start();
    }
    let (npm_program, npm_prefix, managed_node_bin) = if let Some(node) = managed_node_executable() {
        let npm_cli = managed_npm_cli().ok_or_else(|| AppError::Engine("Manager 私有 Node 缺少 npm".to_string()))?;
        let bin = node.parent().map(Path::to_path_buf);
        (node, vec![npm_cli.to_string_lossy().to_string()], bin)
    } else if node_version().as_deref().is_some_and(node_supported) && npm_available() {
        (system_npm_command().ok_or_else(|| AppError::Engine("未找到可用 npm".to_string()))?, Vec::new(), None)
    } else {
        install_managed_node()?;
        let node = managed_node_executable().ok_or_else(|| AppError::Engine("Node 运行时安装后不可用".to_string()))?;
        let npm_cli = managed_npm_cli().ok_or_else(|| AppError::Engine("Manager 私有 Node 缺少 npm".to_string()))?;
        let bin = node.parent().map(Path::to_path_buf);
        (node, vec![npm_cli.to_string_lossy().to_string()], bin)
    };
    let runtime = managed_runtime_dir()?;
    fs::create_dir_all(&runtime)
        .map_err(|error| AppError::Internal(format!("创建 OpenCodex 组件目录失败：{error}")))?;
    let prefix = runtime.to_string_lossy().to_string();
    let package = format!("@bitkyc08/opencodex@{DEFAULT_VERSION}");
    let mut args = npm_prefix;
    args.extend(["install".to_string(), "--prefix".to_string(), prefix, "--no-save".to_string(), package]);
    let mut command = Command::new(&npm_program);
    configure_background_command(&mut command);
    configure_opencodex_environment(&mut command);
    command.args(args);
    if let Some(bin) = managed_node_bin {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        if let Ok(path) = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&inherited))) {
            command.env("PATH", path);
        }
    }
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| AppError::Engine(format!("无法启动 Node/npm 安装 OpenCodex：{error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Engine(if detail.is_empty() {
            format!("OpenCodex 组件安装失败：{}", output.status)
        } else {
            format!("OpenCodex 组件安装失败：{detail}")
        }));
    }
    if ocx_program().is_none() {
        return Err(AppError::Engine("OpenCodex 安装后未找到可执行组件".to_string()));
    }
    start()
}

pub fn start() -> Result<OpenCodexStatus, AppError> {
    ocx_output(&["service"])?;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let current = status()?;
        if current.service_state == "ready" { return Ok(current); }
        if std::time::Instant::now() >= deadline {
            return Err(AppError::Engine(current.error.unwrap_or_else(|| "OpenCodex 服务启动超时，请稍后重试".to_string())));
        }
        thread::sleep(Duration::from_millis(300));
    }
}

fn restart_service_and_wait_ready() -> Result<(), AppError> {
    ocx_output(&["restart"])?;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let (state, detail) = service_state(true);
        if state == "ready" {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(AppError::Engine(detail.unwrap_or_else(|| {
                "OpenCodex 重启后未能进入 ready 状态".to_string()
            })));
        }
        thread::sleep(Duration::from_millis(300));
    }
}

fn ocx_output(args: &[&str]) -> Result<Vec<u8>, AppError> {
    let (program, prefix) = ocx_invocation().ok_or_else(|| {
        AppError::Engine("未检测到 OpenCodex；请先安装多模型组件".to_string())
    })?;
    let mut command = Command::new(program);
    configure_background_command(&mut command);
    configure_opencodex_environment(&mut command);
    let output = command
        .args(prefix)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| AppError::Engine(format!("无法执行 OpenCodex：{error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Engine(if detail.is_empty() {
            format!("OpenCodex 命令失败：{}", output.status)
        } else {
            format!("OpenCodex 命令失败：{detail}")
        }));
    }
    Ok(output.stdout)
}

fn version() -> Option<String> {
    let bytes = ocx_output(&["--version"]).ok()?;
    let raw = String::from_utf8_lossy(&bytes);
    raw.split_whitespace().last().map(str::to_string)
}

fn service_state(installed: bool) -> (String, Option<String>) {
    if !installed {
        return ("missing".to_string(), None);
    }
    match ocx_output(&["health", "--json"]) {
        Ok(raw) => match serde_json::from_slice::<JsonValue>(&raw) {
            Ok(value) if value.get("ok").and_then(JsonValue::as_bool) == Some(true) => {
                ("ready".to_string(), None)
            }
            Ok(_) => ("unhealthy".to_string(), Some("OpenCodex 健康检查未通过".to_string())),
            Err(_) => ("unknown".to_string(), Some("OpenCodex 健康检查返回无法识别的数据".to_string())),
        },
        Err(error) => ("stopped".to_string(), Some(error.to_string())),
    }
}

fn route_from_config(
    id: &str,
    config: &JsonMap<String, JsonValue>,
    catalog_models: &[JsonValue],
    locked_route: Option<&str>,
    route_health: &BTreeMap<String, String>,
) -> Option<OpenCodexRoute> {
    let provider = config.get("providers")?.as_object()?.get(id)?.as_object()?;
    // `customModels` is the normal synced representation, but a provider
    // added or authorized directly inside OpenCodex can be visible in the
    // provider config before the next catalog sync. Read its authoritative
    // `models` array as a fallback so Manager can show it immediately.
    let mut models = catalog_models
        .iter()
        .filter(|model| model.get("provider").and_then(JsonValue::as_str) == Some(id))
        .filter_map(|model| model.get("modelId").and_then(JsonValue::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    if models.is_empty() {
        models = provider
            .get("models")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(JsonValue::as_str)
            .map(str::to_string)
            .collect();
    }
    models.sort();
    models.dedup();
    let api_key_configured = provider
        .get("apiKey")
        .and_then(JsonValue::as_str)
        .is_some_and(|key| !key.trim().is_empty())
        || provider
            .get("authMode")
            .and_then(JsonValue::as_str)
            .is_some_and(|mode| !matches!(mode.to_ascii_lowercase().as_str(), "" | "key" | "none"));
    Some(OpenCodexRoute {
        id: id.to_string(),
        label: provider
            .get("label")
            .and_then(JsonValue::as_str)
            .unwrap_or(id)
            .to_string(),
        adapter: provider
            .get("adapter")
            .and_then(JsonValue::as_str)
            .unwrap_or("openai-responses")
            .to_string(),
        base_url: provider
            .get("baseUrl")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
        default_model: provider
            .get("defaultModel")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
        models,
        enabled: provider.get("disabled").and_then(JsonValue::as_bool) != Some(true),
        api_key_configured,
        availability: if provider.get("disabled").and_then(JsonValue::as_bool) == Some(true) {
            "offline".to_string()
        } else if let Some(health) = provider
            .get("defaultModel")
            .and_then(JsonValue::as_str)
            .and_then(|model| route_health.get(&format!("{id}/{model}")))
            .or_else(|| route_health.get(id))
        {
            health.clone()
        } else if api_key_configured {
            "configured".to_string()
        } else {
            "unknown".to_string()
        },
        locked: locked_route == Some(id),
    })
}

fn inferred_managed_provider_ids(
    config: &JsonMap<String, JsonValue>,
    models: &[JsonValue],
) -> Vec<String> {
    let custom_model_ids = models
        .iter()
        .filter_map(|model| model.get("provider").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    config
        .get("providers")
        .and_then(JsonValue::as_object)
        .into_iter()
        .flat_map(|providers| providers.iter())
        .filter(|(id, provider)| {
            let Some(provider) = provider.as_object() else { return false };
            let has_endpoint = provider.get("baseUrl").and_then(JsonValue::as_str).is_some_and(|url| !url.trim().is_empty())
                && provider.get("defaultModel").and_then(JsonValue::as_str).is_some_and(|model| !model.trim().is_empty());
            let has_provider_models = provider
                .get("models")
                .and_then(JsonValue::as_array)
                .is_some_and(|items| items.iter().any(JsonValue::is_string));
            has_endpoint && (has_provider_models || custom_model_ids.contains(id.as_str()))
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn codex_uses_opencodex(paths: &IntegrationPaths) -> bool {
    let Ok(raw) = fs::read_to_string(&paths.codex_config) else {
        return false;
    };
    raw.contains("127.0.0.1:")
        && raw.contains("/v1")
        && raw.contains(&paths.catalog.display().to_string())
        && catalog_has_models(&paths.catalog)
}

fn inferred_default_route(
    config: &JsonMap<String, JsonValue>,
    managed_provider_ids: &[String],
) -> Option<String> {
    let providers = config.get("providers").and_then(JsonValue::as_object)?;
    let provider_id = config
        .get("defaultProvider")
        .and_then(JsonValue::as_str)
        .filter(|id| managed_provider_ids.iter().any(|managed| managed == *id))
        .or_else(|| managed_provider_ids.first().map(String::as_str))?;
    let default_model = providers
        .get(provider_id)?
        .get("defaultModel")
        .and_then(JsonValue::as_str)?;
    Some(format!("{provider_id}/{default_model}"))
}

fn effective_state(paths: &IntegrationPaths) -> Result<ManagedState, AppError> {
    let state = load_state(&paths.state);
    let config = load_config(&paths.opencodex_config)?;
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let inferred_ids = inferred_managed_provider_ids(&config, &models);
    // OpenCodex can be authorized or edited outside Manager (`ocx login`,
    // its dashboard, or another client). When that external flow has already
    // wired Codex to the local proxy, adopt the current provider set instead
    // of treating it as stale state. This keeps Manager and OpenCodex in sync
    // without requiring a second authorization in Manager.
    if inferred_ids.is_empty() || !codex_uses_opencodex(paths) {
        return Ok(state);
    }
    let mut adopted = state;
    adopted.enabled = true;
    adopted.port = config
        .get("port")
        .and_then(JsonValue::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_PORT);
    adopted.codex_provider_id = DEFAULT_PROVIDER_ID.to_string();
    adopted.managed_provider_ids = inferred_ids;
    adopted.locked_route = config
        .get("defaultProvider")
        .and_then(JsonValue::as_str)
        .filter(|id| adopted.managed_provider_ids.iter().any(|managed| managed == *id))
        .map(str::to_string);
    if let Some(default_route) = inferred_default_route(&config, &adopted.managed_provider_ids) {
        write_codex_proxy_config(
            &paths.codex_config,
            &paths.catalog,
            &adopted.codex_provider_id,
            adopted.port,
            &default_route,
        )?;
    }
    if adopted != load_state(&paths.state) {
        write_json(
            &paths.state,
            &serde_json::to_value(&adopted)
                .map_err(|error| AppError::Internal(format!("保存已有 OpenCodex 接管状态失败：{error}")))?,
        )?;
    }
    Ok(adopted)
}

fn status_at(paths: &IntegrationPaths) -> Result<OpenCodexStatus, AppError> {
    let state = effective_state(paths)?;
    let codex_is_loopback = codex_proxy_provider_is_loopback(&paths.codex_config);
    if should_reconcile_codex_ownership(state.enabled, codex_is_loopback) {
        // Reconcile stale ownership left by an older Manager version or an
        // external edit to config.toml before exposing status to the UI.
        disable_for_single_provider()?;
        return status_at(paths);
    }
    let config = load_config(&paths.opencodex_config).unwrap_or_default();
    let installed = ocx_program().is_some();
    let (service_state, error) = service_state(installed);
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let discovered_provider_ids = inferred_managed_provider_ids(&config, &models);
    let display_provider_ids = state
        .managed_provider_ids
        .iter()
        .chain(discovered_provider_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let routes = display_provider_ids
        .iter()
        .filter_map(|id| route_from_config(id, &config, &models, state.locked_route.as_deref(), &state.route_health))
        .collect::<Vec<_>>();
    let port = config
        .get("port")
        .and_then(JsonValue::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .unwrap_or(if state.port > 0 { state.port } else { DEFAULT_PORT });
    let codex_provider_id = if state.codex_provider_id.is_empty() {
        DEFAULT_PROVIDER_ID.to_string()
    } else {
        state.codex_provider_id
    };
    let routes_ready = service_state == "ready" && !routes.is_empty();
    let has_configured_credentials = routes.iter().any(|route| route.enabled && route.api_key_configured);
    let connection_status = if state.signed_out {
        "signedOut"
    } else if routes_ready && (state.connection.is_some() || has_configured_credentials) {
        // `ocx login` or the OpenCodex dashboard can create valid provider
        // credentials without Manager's OSIR account exchange. Treat those
        // routes as connected while leaving account billing details empty.
        "connected"
    } else if state.enabled { "error" } else { "notConnected" }.to_string();
    let platform = std::env::consts::OS.to_string();
    let architecture = std::env::consts::ARCH.to_string();
    let supported = component_target_for(&platform, &architecture).is_some();
    let managed = managed_component_invocation().is_some();
    let private = private_npm_invocation().is_some();
    let system = system_ocx_invocation().is_some();
    let system_node = node_version();
    let runtime_state = if managed { "managed" } else if private { "privateNpm" } else if system { "system" } else if system_node.as_deref().is_some_and(node_supported) { "node" } else if supported { "missing" } else { "unsupported" };
    let install_strategy = if managed || system { "reuse" } else if supported { "managedComponent" } else if system_node.as_deref().is_some_and(node_supported) && npm_available() { "privateNpm" } else { "unavailable" };
    let codex_running = crate::app::codex_theme::codex_running();
    let marker_requires_restart = restart_required_path(paths).is_file();
    let current_revision = codex_configuration_revision(paths);
    let applied_revision = fs::read_to_string(restart_applied_path(paths)).ok();
    Ok(OpenCodexStatus {
        enabled: state.enabled,
        installed,
        version: version(),
        port,
        service_state,
        codex_provider_id,
        config_path: paths.opencodex_config.display().to_string(),
        catalog_path: paths.catalog.display().to_string(),
        model_count: routes.iter().map(|route| route.models.len()).sum(),
        routes,
        backup_available: atomic_file::backup_path(&paths.opencodex_config).is_file()
            || atomic_file::backup_path(&paths.codex_config).is_file(),
        error,
        connection_status,
        account: state.connection,
        environment: OpenCodexEnvironmentStatus {
            platform,
            architecture,
            supported,
            runtime_state: runtime_state.to_string(),
            install_strategy: install_strategy.to_string(),
            node_version: system_node,
            npm_available: npm_available(),
            detail: if managed { "已发现 Manager 自带运行时" } else if system { "已发现系统 OpenCodex" } else if supported { "可下载当前平台的自带运行时" } else { "当前系统或 CPU 暂无可用安装包" }.to_string(),
        },
        requires_codex_restart: configuration_requires_restart(
            codex_running,
            state.enabled,
            marker_requires_restart,
            current_revision.as_deref(),
            applied_revision.as_deref().map(str::trim),
        ),
    })
}

pub fn status() -> Result<OpenCodexStatus, AppError> {
    reconcile_after_manager_update()?;
    let paths = integration_paths()?;
    // Repair legacy model ids as part of every status read while OpenCodex
    // owns Codex, so an upgrade self-heals without another mode toggle.
    let state = effective_state(&paths)?;
    if state.enabled && paths.catalog.is_file() {
        let config = load_config(&paths.opencodex_config)?;
        let models = config
            .get("customModels")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let provider_ids = inferred_managed_provider_ids(&config, &models);
        let routes = configured_routes_from_config(&config, &provider_ids);
        if !routes.is_empty() {
            normalize_synced_catalog(&paths.catalog, &routes)?;
        }
    }
    status_at(&paths)
}

fn validate_input(input: &OpenCodexConfigInput) -> Result<(String, String, Vec<OpenCodexRouteInput>), AppError> {
    if input.port == 0 {
        return Err(AppError::Engine("本机端口必须在 1 到 65535 之间".to_string()));
    }
    let provider_id = checked_id(&input.codex_provider_id, "Codex Provider ID")?;
    let default_route = checked_text(&input.default_route, "默认模型路由")?;
    if input.routes.is_empty() || input.routes.len() > MAX_ROUTE_COUNT {
        return Err(AppError::Engine(format!("需要配置 1 到 {MAX_ROUTE_COUNT} 个模型路由")));
    }
    let mut ids = BTreeSet::new();
    let mut routes = Vec::with_capacity(input.routes.len());
    for route in &input.routes {
        let id = checked_id(&route.id, "路由 ID")?;
        if !ids.insert(id.clone()) {
            return Err(AppError::Engine("路由 ID 不能重复".to_string()));
        }
        let label = checked_text(&route.label, "路由名称")?;
        if route.adapter.trim() != "openai-responses" {
            return Err(AppError::Engine("首版仅支持 OpenAI Responses 路由".to_string()));
        }
        let base_url = checked_url(&route.base_url)?;
        let default_model = checked_text(&route.default_model, "默认模型")?;
        if route.models.is_empty() || route.models.len() > MAX_MODELS_PER_ROUTE {
            return Err(AppError::Engine(format!("每条路由需要配置 1 到 {MAX_MODELS_PER_ROUTE} 个模型")));
        }
        let mut models = BTreeSet::new();
        for model in &route.models {
            models.insert(checked_text(model, "模型名称")?);
        }
        if !models.contains(&default_model) {
            return Err(AppError::Engine("默认模型必须包含在当前路由模型列表中".to_string()));
        }
        if let Some(key) = &route.api_key {
            let key = key.trim();
            if key.len() > MAX_VALUE_LEN || key.chars().any(char::is_control) {
                return Err(AppError::Engine("API Key 格式无效".to_string()));
            }
        }
        routes.push(OpenCodexRouteInput {
            id,
            label,
            adapter: "openai-responses".to_string(),
            base_url,
            api_key: route.api_key.clone(),
            models: models.into_iter().collect(),
            default_model,
            enabled: route.enabled,
        });
    }
    if !routes.iter().any(|route| route.enabled) {
        return Err(AppError::Engine("至少需要启用一条模型路由".to_string()));
    }
    if !routes.iter().any(|route| format!("{}/{}", route.id, route.default_model) == default_route) {
        return Err(AppError::Engine("默认模型路由必须指向某个已配置的默认模型".to_string()));
    }
    Ok((provider_id, default_route, routes))
}

fn build_opencodex_config(
    mut config: JsonMap<String, JsonValue>,
    routes: &[OpenCodexRouteInput],
    prior_managed: &[String],
    port: u16,
) -> Result<JsonValue, AppError> {
    {
        let providers = config
            .entry("providers")
            .or_insert_with(|| JsonValue::Object(JsonMap::new()))
            .as_object_mut()
            .ok_or_else(|| AppError::Engine("OpenCodex providers 必须是对象".to_string()))?;
        for id in prior_managed {
            providers.remove(id);
        }
        for route in routes {
            let mut provider = JsonMap::new();
            provider.insert("adapter".to_string(), JsonValue::String(route.adapter.clone()));
            provider.insert("baseUrl".to_string(), JsonValue::String(route.base_url.clone()));
            provider.insert("label".to_string(), JsonValue::String(route.label.clone()));
            provider.insert("defaultModel".to_string(), JsonValue::String(route.default_model.clone()));
            // OpenCodex 2.22 defaults an untyped provider to its native auth
            // path. Declare the OSIR route as an API-key provider explicitly;
            // otherwise a valid gateway key is reported as expired/invalid.
            provider.insert("authMode".to_string(), JsonValue::String("key".to_string()));
            // Keep the provider's authoritative model list explicit. OpenCodex
            // uses it to decode provider/model selectors back to the bare model
            // id before sending the request upstream. Relying only on
            // customModels can leave a stale namespaced selector on first boot.
            provider.insert("models".to_string(), json!(route.models));
            if !route.enabled {
                provider.insert("disabled".to_string(), JsonValue::Bool(true));
            }
            if let Some(api_key) = route.api_key.as_deref().map(str::trim).filter(|key| !key.is_empty()) {
                provider.insert("apiKey".to_string(), JsonValue::String(api_key.to_string()));
            }
            providers.insert(route.id.clone(), JsonValue::Object(provider));
        }
    }
    {
        let models = config
            .entry("customModels")
            .or_insert_with(|| JsonValue::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| AppError::Engine("OpenCodex customModels 必须是数组".to_string()))?;
        models.retain(|model| {
            model
                .get("provider")
                .and_then(JsonValue::as_str)
                .is_none_or(|provider| !prior_managed.iter().any(|id| id == provider))
        });
        for route in routes {
            for model in &route.models {
                models.push(json!({
                    "id": Uuid::new_v4().to_string(),
                    "provider": route.id,
                    "modelId": model,
                    "displayName": format!("{} · {}", model, route.label),
                    "contextWindow": 200000,
                    "inputModalities": ["text", "image"],
                    "reasoningEfforts": ["low", "medium", "high", "xhigh", "max", "ultra"],
                    "defaultReasoningEffort": "high",
                }));
            }
        }
    }
    let first_enabled = routes
        .iter()
        .find(|route| route.enabled)
        .map(|route| route.id.clone())
        .ok_or_else(|| AppError::Engine("没有可用模型路由".to_string()))?;
    config.insert("defaultProvider".to_string(), JsonValue::String(first_enabled));
    config.insert("port".to_string(), JsonValue::from(port));
    config.insert("codexShimAutoRestore".to_string(), JsonValue::Bool(false));
    config.insert("emptyCompletionRetry".to_string(), JsonValue::Bool(false));
    if let Some(openai) = config
        .get_mut("providers")
        .and_then(JsonValue::as_object_mut)
        .and_then(|providers| providers.get_mut("openai"))
        .and_then(JsonValue::as_object_mut)
    {
        openai.insert("disabled".to_string(), JsonValue::Bool(true));
    }
    Ok(JsonValue::Object(config))
}

fn write_codex_proxy_config(
    path: &Path,
    catalog: &Path,
    provider_id: &str,
    port: u16,
    default_route: &str,
) -> Result<(), AppError> {
    if path.is_symlink() {
        return Err(AppError::Engine("config.toml 是符号链接，拒绝改写".to_string()));
    }
    let raw = if path.exists() {
        fs::read_to_string(path)
            .map_err(|error| AppError::Internal(format!("读取 config.toml 失败：{error}")))?
    } else {
        String::new()
    };
    let mut document = raw
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Engine(format!("config.toml 格式错误：{error}")))?;
    // OpenCodex is a custom local provider. Do not masquerade as the built-in
    // `openai` provider: the Codex desktop client uses this distinction when
    // deciding which model catalog entries are eligible for its selector.
    let provider_id = if provider_id.trim().is_empty() {
        DEFAULT_PROVIDER_ID
    } else {
        provider_id.trim()
    };
    document["model_provider"] = value(provider_id);
    document["model"] = value(default_route);
    document["model_catalog_json"] = value(catalog.display().to_string());
    document.remove("openai_base_url");
    if !document.contains_key("model_providers") {
        document["model_providers"] = toml_edit::table();
    }
    let providers = document["model_providers"]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("model_providers 必须是 TOML 表".to_string()))?;
    let provider = providers
        .entry(provider_id)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| AppError::Engine(format!("model_providers.{provider_id} 必须是 TOML 表")))?;
    provider["name"] = value("OpenCodex 多模型路由");
    provider["base_url"] = value(format!("http://127.0.0.1:{port}/v1"));
    provider["wire_api"] = value("responses");
    provider["requires_openai_auth"] = value(false);
    let rendered = document.to_string();
    atomic_file::write_atomic(path, rendered.as_bytes())
        .map_err(|error| AppError::Internal(format!("原子保存 config.toml 失败：{error}")))?;
    let reread = fs::read_to_string(path)
        .map_err(|error| AppError::Internal(format!("回读 config.toml 失败：{error}")))?;
    reread
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Internal(format!("保存后 config.toml 无法解析：{error}")))?;
    Ok(())
}

fn catalog_has_models(path: &Path) -> bool {
    catalog_model_slugs(path).is_some_and(|models| !models.is_empty())
}

fn catalog_model_slugs(path: &Path) -> Option<BTreeSet<String>> {
    fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<JsonValue>(&raw).ok())
        .and_then(|value| value.get("models").and_then(JsonValue::as_array).cloned())
        .map(|models| {
            models
                .into_iter()
                .filter_map(|model| model.get("slug").and_then(JsonValue::as_str).map(str::to_string))
                .collect()
        })
}

fn catalog_contains_enabled_routes(path: &Path, routes: &[OpenCodexRouteInput]) -> bool {
    let Some(actual) = catalog_model_slugs(path) else {
        return false;
    };
    routes
        .iter()
        .filter(|route| route.enabled)
        .flat_map(|route| route.models.iter().map(move |model| format!("{}/{}", route.id, model)))
        .all(|slug| actual.contains(&slug))
}

fn configured_routes_from_config(
    config: &JsonMap<String, JsonValue>,
    managed_provider_ids: &[String],
) -> Vec<OpenCodexRouteInput> {
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    managed_provider_ids
        .iter()
        .filter_map(|id| {
            let route = route_from_config(id, config, &models, None, &BTreeMap::new())?;
            if route.models.is_empty() || route.default_model.is_empty() || route.base_url.is_empty() {
                return None;
            }
            Some(OpenCodexRouteInput {
                id: route.id,
                label: route.label,
                adapter: route.adapter,
                base_url: route.base_url,
                api_key: None,
                models: route.models,
                default_model: route.default_model,
                enabled: route.enabled,
            })
        })
        .collect()
}

/// Normalize metadata only for models that the running OpenCodex service
/// actually exposed. Never inject configured-but-unroutable models into the
/// Codex catalog: doing so creates a selectable model that falls through to the
/// default provider at request time.
fn normalize_synced_catalog(
    path: &Path,
    routes: &[OpenCodexRouteInput],
) -> Result<(), AppError> {
    let raw = fs::read(path)
        .map_err(|error| AppError::Internal(format!("读取 OpenCodex 模型目录失败：{error}")))?;
    let mut catalog = serde_json::from_slice::<JsonValue>(&raw)
        .map_err(|error| AppError::Engine(format!("OpenCodex 模型目录不是有效 JSON：{error}")))?;
    let models = catalog
        .get_mut("models")
        .and_then(JsonValue::as_array_mut)
        .ok_or_else(|| AppError::Engine("OpenCodex 模型目录缺少 models 数组".to_string()))?;
    let mut existing = models
        .iter()
        .filter_map(|model| model.get("slug").and_then(JsonValue::as_str))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut changed = false;
    // Older Codex rollouts persist bare OpenAI model ids (for example
    // `gpt-5.5`), while the current catalog uses routed ids such as
    // `osirapi-openai/gpt-5.5`. Keep a hidden compatibility row only when the
    // running service exposed the corresponding routed model.
    for route in routes.iter().filter(|route| route.enabled) {
        let route_id = route.id.to_ascii_lowercase();
        let route_label = route.label.to_ascii_lowercase();
        let is_openai_route = route_id.contains("openai")
            || route_label.contains("openai")
            || route_label.contains("gpt")
            || route
                .models
                .iter()
                .any(|model| model.starts_with("gpt-") || model.starts_with("codex-"));
        if !is_openai_route {
            continue;
        }
        for model in &route.models {
            if !(model.starts_with("gpt-") || model.starts_with("codex-")) || existing.contains(model) {
                continue;
            }
            let routed_slug = format!("{}/{}", route.id, model);
            let Some(mut entry) = models
                .iter()
                .find(|entry| entry.get("slug").and_then(JsonValue::as_str) == Some(routed_slug.as_str()))
                .cloned()
            else {
                continue;
            };
            let object = entry
                .as_object_mut()
                .ok_or_else(|| AppError::Engine("OpenCodex 模型目录条目格式无效".to_string()))?;
            object.insert("slug".to_string(), JsonValue::String(model.clone()));
            object.insert("display_name".to_string(), JsonValue::String(format!("{} · {}", model, route.label)));
            object.insert("description".to_string(), JsonValue::String(format!("历史会话兼容别名 · OpenCodex -> {}", route.id)));
            object.insert("default_reasoning_level".to_string(), JsonValue::String("high".to_string()));
            object.insert("supported_in_api".to_string(), JsonValue::Bool(true));
            // Keep the alias resolvable for legacy sessions, but do not show
            // it as a second selectable row in the current model picker.
            object.insert("visibility".to_string(), JsonValue::String("hide".to_string()));
            object.insert("hidden".to_string(), JsonValue::Bool(true));
            models.push(entry);
            existing.insert(model.clone());
            changed = true;
        }
    }

    // Existing catalogs may already contain bare aliases from an earlier
    // Manager release. Normalize them too, then sort the complete managed
    // catalog into a stable platform order. The Codex client keeps the file
    // order when rendering models, so doing this at the source avoids
    // release-to-release selector reshuffling.
    let openai_models: BTreeSet<String> = routes
        .iter()
        .filter(|route| route.enabled)
        .filter(|route| {
            let id = route.id.to_ascii_lowercase();
            let label = route.label.to_ascii_lowercase();
            id.contains("openai") || label.contains("openai") || label.contains("gpt")
        })
        .flat_map(|route| route.models.iter().cloned())
        .collect();
    for model in models.iter_mut() {
        let Some(slug) = model.get("slug").and_then(JsonValue::as_str) else {
            continue;
        };
        if openai_models.contains(slug) && !slug.contains('/') {
            if let Some(object) = model.as_object_mut() {
                let already_hidden = object.get("hidden").and_then(JsonValue::as_bool).unwrap_or(false);
                if !already_hidden || object.get("visibility").and_then(JsonValue::as_str) != Some("hide") {
                    object.insert("visibility".to_string(), JsonValue::String("hide".to_string()));
                    object.insert("hidden".to_string(), JsonValue::Bool(true));
                    changed = true;
                }
            }
        }
    }
    let before_order = models
        .iter()
        .map(|model| {
            (
                model.get("slug").and_then(JsonValue::as_str).unwrap_or_default().to_string(),
                model.get("hidden").and_then(JsonValue::as_bool).unwrap_or(false),
            )
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| {
        let key = |model: &JsonValue| {
            let slug = model.get("slug").and_then(JsonValue::as_str).unwrap_or_default();
            let hidden = model.get("hidden").and_then(JsonValue::as_bool).unwrap_or(false);
            let group = match slug.split('/').next().unwrap_or_default() {
                "osirapi-openai" => 0,
                "osirapi-claude" => 1,
                "osirapi-gemini" => 2,
                "osirapi-grok" => 3,
                _ => 4,
            };
            (hidden, group, slug.to_ascii_lowercase())
        };
        key(left).cmp(&key(right))
    });
    let after_order = models
        .iter()
        .map(|model| {
            (
                model.get("slug").and_then(JsonValue::as_str).unwrap_or_default().to_string(),
                model.get("hidden").and_then(JsonValue::as_bool).unwrap_or(false),
            )
        })
        .collect::<Vec<_>>();
    if before_order != after_order {
        changed = true;
    }
    if changed {
        let bytes = serde_json::to_vec_pretty(&catalog)
            .map_err(|error| AppError::Internal(format!("序列化 OpenCodex 模型目录失败：{error}")))?;
        atomic_file::write_atomic(path, &bytes)
            .map_err(|error| AppError::Internal(format!("保存 OpenCodex 模型目录失败：{error}")))?;
    }
    Ok(())
}

fn refresh_codex_catalog_binding(paths: &IntegrationPaths) -> Result<(), AppError> {
    let state = effective_state(paths)?;
    if !state.enabled {
        return Ok(());
    }
    let config = load_config(&paths.opencodex_config)?;
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    // Always rediscover providers from the live OpenCodex config. This is the
    // bridge for providers authorized/added outside Manager.
    let managed_provider_ids = inferred_managed_provider_ids(&config, &models);
    let Some(default_route) = inferred_default_route(&config, &managed_provider_ids) else {
        return Ok(());
    };
    let configured_routes = configured_routes_from_config(&config, &managed_provider_ids);
    if !configured_routes.is_empty() {
        normalize_synced_catalog(&paths.catalog, &configured_routes)?;
    }
    let provider_id = if state.codex_provider_id.is_empty() {
        DEFAULT_PROVIDER_ID
    } else {
        state.codex_provider_id.as_str()
    };
    let port = config
        .get("port")
        .and_then(JsonValue::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .unwrap_or(state.port.max(1));
    write_codex_proxy_config(
        &paths.codex_config,
        &paths.catalog,
        provider_id,
        port,
        &default_route,
    )
}

fn validate_candidate(path: &Path, candidate: &JsonValue) -> Result<(), AppError> {
    let candidate_path = path.with_extension("json.manager-candidate");
    let bytes = serde_json::to_vec_pretty(candidate)
        .map_err(|error| AppError::Internal(format!("生成 OpenCodex 候选配置失败：{error}")))?;
    fs::write(&candidate_path, bytes)
        .map_err(|error| AppError::Internal(format!("写入 OpenCodex 候选配置失败：{error}")))?;
    let result = ocx_output(&["config", "validate", candidate_path.to_string_lossy().as_ref()]);
    let _ = fs::remove_file(&candidate_path);
    result.map(|_| ())
}

fn restore_save_snapshot(
    paths: &IntegrationPaths,
    opencodex_config: Option<&[u8]>,
    codex_config: Option<&[u8]>,
    catalog: Option<&[u8]>,
    state: &ManagedState,
) -> Result<(), AppError> {
    for (path, bytes) in [
        (&paths.opencodex_config, opencodex_config),
        (&paths.codex_config, codex_config),
        (&paths.catalog, catalog),
    ] {
        if let Some(bytes) = bytes {
            atomic_file::write_atomic(path, bytes)
                .map_err(|error| AppError::Internal(format!("恢复配置快照失败：{error}")))?;
        }
    }
    write_json(
        &paths.state,
        &serde_json::to_value(state)
            .map_err(|error| AppError::Internal(format!("恢复多模型状态失败：{error}")))?,
    )?;
    if !state.enabled {
        let _ = fs::remove_file(codex_takeover_backup_path(paths));
    }
    Ok(())
}

fn rollback_reloaded_save(
    paths: &IntegrationPaths,
    opencodex_config: Option<&[u8]>,
    codex_config: Option<&[u8]>,
    catalog: Option<&[u8]>,
    state: &ManagedState,
) {
    let _ = restore_save_snapshot(paths, opencodex_config, codex_config, catalog, state);
    // The failed candidate may already be loaded in the running process. Load
    // the restored snapshot as well so disk and runtime cannot diverge.
    let _ = restart_service_and_wait_ready();
}

pub fn save(input: OpenCodexConfigInput) -> Result<OpenCodexStatus, AppError> {
    if !input.enabled {
        return Err(AppError::Engine("停用多模型请使用恢复按钮，避免丢失当前配置".to_string()));
    }
    if ocx_program().is_none() {
        return Err(AppError::Engine("未检测到 OpenCodex；请先安装多模型组件".to_string()));
    }
    let paths = integration_paths()?;
    let codex_was_running = crate::app::codex_theme::codex_running();
    let (provider_id, requested_default_route, mut routes) = validate_input(&input)?;
    sanitize_osir_routes(&mut routes)?;
    let requested_route_id = requested_default_route
        .split_once('/')
        .map(|(route, _)| route);
    let default_route = requested_route_id
        .and_then(|route_id| routes.iter().find(|route| route.id == route_id && route.enabled))
        .or_else(|| routes.iter().find(|route| route.enabled))
        .map(|route| format!("{}/{}", route.id, route.default_model))
        .ok_or_else(|| AppError::Engine("没有可用默认模型路由".to_string()))?;
    let prior = load_state(&paths.state);
    let previous_config = fs::read(&paths.opencodex_config).ok();
    let previous_codex_config = fs::read(&paths.codex_config).ok();
    let previous_catalog = fs::read(&paths.catalog).ok();
    let previous_state = prior.clone();
    let config = load_config(&paths.opencodex_config)?;
    let candidate = build_opencodex_config(config, &routes, &prior.managed_provider_ids, input.port)?;
    validate_candidate(&paths.opencodex_config, &candidate)?;
    preserve_codex_config_before_takeover(&paths, &prior)?;
    write_json(&paths.opencodex_config, &candidate)?;
    let state = ManagedState {
        enabled: false,
        port: input.port,
        codex_provider_id: provider_id.clone(),
        managed_provider_ids: routes.iter().map(|route| route.id.clone()).collect(),
        locked_route: None,
        route_health: BTreeMap::new(),
        connection: prior.connection.clone(),
        signed_out: prior.signed_out,
    };
    let state_json = serde_json::to_value(state)
        .map_err(|error| AppError::Internal(format!("序列化多模型状态失败：{error}")))?;
    write_json(&paths.state, &state_json)?;
    if let Err(error) = restart_service_and_wait_ready() {
        rollback_reloaded_save(
            &paths,
            previous_config.as_deref(),
            previous_codex_config.as_deref(),
            previous_catalog.as_deref(),
            &previous_state,
        );
        return Err(error);
    }
    if let Err(error) = ocx_output(&["sync"]) {
        rollback_reloaded_save(
            &paths,
            previous_config.as_deref(),
            previous_codex_config.as_deref(),
            previous_catalog.as_deref(),
            &previous_state,
        );
        return Err(error);
    }
    if let Err(error) = normalize_synced_catalog(&paths.catalog, &routes) {
        rollback_reloaded_save(
            &paths,
            previous_config.as_deref(),
            previous_codex_config.as_deref(),
            previous_catalog.as_deref(),
            &previous_state,
        );
        return Err(error);
    }
    if !catalog_contains_enabled_routes(&paths.catalog, &routes) {
        rollback_reloaded_save(
            &paths,
            previous_config.as_deref(),
            previous_codex_config.as_deref(),
            previous_catalog.as_deref(),
            &previous_state,
        );
        return Err(AppError::Engine("OpenCodex 运行时未加载全部供应商模型；已恢复原配置".to_string()));
    }
    // Codex observes config.toml, not the generated catalog file. The first
    // config write above can therefore race ahead of `ocx sync` and make the
    // picker cache only the default model. Rewrite the same binding after the
    // complete catalog exists so a running Codex reloads every synced model.
    write_codex_proxy_config(&paths.codex_config, &paths.catalog, &provider_id, input.port, &default_route)?;
    if codex_was_running {
        mark_codex_restart_required_at(&paths)?;
    }
    let enabled_state = ManagedState {
        enabled: true,
        port: input.port,
        codex_provider_id: provider_id,
        managed_provider_ids: routes.iter().map(|route| route.id.clone()).collect(),
        locked_route: None,
        route_health: BTreeMap::new(),
        connection: prior.connection,
        signed_out: prior.signed_out,
    };
    write_json(
        &paths.state,
        &serde_json::to_value(enabled_state)
            .map_err(|error| AppError::Internal(format!("保存多模型启用状态失败：{error}")))?,
    )?;
    status_at(&paths)
}

pub fn sync() -> Result<OpenCodexStatus, AppError> {
    let codex_was_running = crate::app::codex_theme::codex_running();
    // A provider may have been added while the daemon was already running.
    // Reload disk config before asking the runtime to regenerate Codex's
    // catalog, otherwise sync can faithfully reproduce a stale provider set.
    restart_service_and_wait_ready()?;
    ocx_output(&["sync"])?;
    let paths = integration_paths()?;
    if !catalog_has_models(&paths.catalog) {
        return Err(AppError::Engine("OpenCodex 同步完成但没有生成可用模型目录".to_string()));
    }
    refresh_codex_catalog_binding(&paths)?;
    if codex_was_running {
        mark_codex_restart_required_at(&paths)?;
    }
    status_at(&paths)
}

/// Activate the last saved OpenCodex routes without asking the user to enter
/// them again. This is the explicit mode switch from the default config.
pub fn activate_saved() -> Result<OpenCodexStatus, AppError> {
    if ocx_program().is_none() {
        return Err(AppError::Engine("未检测到 OpenCodex；请先安装多模型组件".to_string()));
    }
    let paths = integration_paths()?;
    let codex_was_running = crate::app::codex_theme::codex_running();
    let prior = load_state(&paths.state);
    let config = load_config(&paths.opencodex_config)?;
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let managed_provider_ids = inferred_managed_provider_ids(&config, &models);
    let routes = configured_routes_from_config(&config, &managed_provider_ids);
    if routes.is_empty() {
        return Err(AppError::Engine("尚未保存 OpenCodex 模型路由，请先完成一次多模型配置".to_string()));
    }
    let default_route = inferred_default_route(&config, &managed_provider_ids)
        .ok_or_else(|| AppError::Engine("OpenCodex 没有可用的默认模型路由".to_string()))?;
    let provider_id = if prior.codex_provider_id.is_empty() {
        DEFAULT_PROVIDER_ID.to_string()
    } else {
        prior.codex_provider_id.clone()
    };
    let port = config
        .get("port")
        .and_then(JsonValue::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .unwrap_or(if prior.port > 0 { prior.port } else { DEFAULT_PORT });
    let takeover_backup = codex_takeover_backup_path(&paths);
    let restart_marker = restart_required_path(&paths);
    let previous_config = fs::read(&paths.codex_config).ok();
    let previous_state = fs::read(&paths.state).ok();
    let previous_backup = fs::read(&takeover_backup).ok();
    let previous_restart_marker = fs::read(&restart_marker).ok();
    let default_provider = previous_config
        .as_deref()
        .map(config_provider)
        .transpose()?
        .unwrap_or_else(|| "openai".to_string());
    let routes_for_sessions = session_routes(&routes);

    let result: Result<OpenCodexStatus, AppError> = (|| {
        preserve_codex_config_before_takeover(&paths, &prior)?;
        if service_state(true).0 == "ready" {
            restart_service_and_wait_ready()?;
        } else {
            start()?;
        }
        ocx_output(&["sync"])?;
        normalize_synced_catalog(&paths.catalog, &routes)?;
        if !catalog_contains_enabled_routes(&paths.catalog, &routes) {
            return Err(AppError::Engine("OpenCodex 模型目录不完整，未启用多模型接管".to_string()));
        }
        let (default_route_id, default_model) = default_route
            .split_once('/')
            .ok_or_else(|| AppError::Engine("OpenCodex 默认模型路由无效".to_string()))?;
        let route_check = check_route(default_route_id, default_model)?;
        if !route_check.available {
            return Err(AppError::Engine(format!(
                "OpenCodex 默认模型验证失败，已保持当前配置：{}",
                route_check.detail
            )));
        }
        write_codex_proxy_config(&paths.codex_config, &paths.catalog, &provider_id, port, &default_route)?;
        write_json(
            &paths.state,
            &serde_json::to_value(ManagedState {
                enabled: true,
                port,
                codex_provider_id: provider_id.clone(),
                managed_provider_ids: managed_provider_ids.clone(),
                locked_route: prior.locked_route.clone(),
                route_health: prior.route_health.clone(),
                connection: prior.connection.clone(),
                signed_out: prior.signed_out,
            })
            .map_err(|error| AppError::Internal(format!("保存 OpenCodex 启用状态失败：{error}")))?,
        )?;
        if codex_was_running {
            mark_codex_restart_required_at(&paths)?;
        }
        let status = status_at(&paths)?;
        codex_sessions::migrate(
            codex_sessions::SessionTarget::OpenCodex {
                provider: &provider_id,
                default_provider: &default_provider,
                default_route: &default_route,
            },
            &routes_for_sessions,
        )?;
        Ok(status)
    })();
    match result {
        Ok(status) => Ok(status),
        Err(error) => {
            let _ = restore_optional_file(&paths.codex_config, previous_config.as_deref());
            let _ = restore_optional_file(&paths.state, previous_state.as_deref());
            let _ = restore_optional_file(&takeover_backup, previous_backup.as_deref());
            let _ = restore_optional_file(&restart_marker, previous_restart_marker.as_deref());
            Err(AppError::Engine(format!("启用 OpenCodex 失败，已恢复原配置：{error}")))
        }
    }
}

pub fn restore() -> Result<OpenCodexStatus, AppError> {
    let paths = integration_paths()?;
    let takeover_backup = codex_takeover_backup_path(&paths);
    let restored = [
        (&paths.opencodex_config, "OpenCodex 配置"),
        (&paths.codex_config, "Codex 配置"),
    ]
    .iter()
    .filter(|(path, _)| atomic_file::backup_path(path).is_file())
    .map(|(path, label)| {
        fs::copy(atomic_file::backup_path(path), path)
            .map_err(|error| AppError::Internal(format!("恢复{label}失败：{error}")))
            .map(|_| ())
    })
    .collect::<Result<Vec<_>, _>>()?;
    if restored.is_empty() && !takeover_backup.is_file() {
        return Err(AppError::Engine("没有可恢复的 OpenCodex 配置备份".to_string()));
    }
    if takeover_backup.is_file() {
        let raw = fs::read(&takeover_backup)
            .map_err(|error| AppError::Internal(format!("读取 Codex 接管备份失败：{error}")))?;
        String::from_utf8(raw.clone())
            .map_err(|error| AppError::Engine(format!("Codex 接管备份不是 UTF-8：{error}")))?
            .parse::<DocumentMut>()
            .map_err(|error| AppError::Engine(format!("Codex 接管备份无效：{error}")))?;
        atomic_file::write_atomic(&paths.codex_config, &raw)
            .map_err(|error| AppError::Internal(format!("恢复 Codex 接管前配置失败：{error}")))?;
        let _ = fs::remove_file(&takeover_backup);
    }
    let state = JsonValue::Object(JsonMap::new());
    write_json(&paths.state, &state)?;
    status_at(&paths)
}

pub fn disconnect_osir() -> Result<OpenCodexStatus, AppError> {
    let paths = integration_paths()?;
    let mut state = load_state(&paths.state);
    state.connection = None;
    state.signed_out = true;
    state.enabled = false;
    write_json(
        &paths.state,
        &serde_json::to_value(state)
            .map_err(|error| AppError::Internal(format!("保存 OSIRAPI 退出状态失败：{error}")))?,
    )?;
    status_at(&paths)
}

#[cfg(test)]
mod tests {
    use super::{
        build_opencodex_config, catalog_contains_enabled_routes, normalize_synced_catalog,
        component_target_for, configuration_requires_restart, configured_routes_from_config, decrypt_bundle,
        extract_osir_ticket, inferred_default_route, inferred_managed_provider_ids, pkce_challenge,
        is_transient_route_check_error, manager_runtime_needs_reconcile, route_check_with_retry, route_from_config,
        sanitize_saved_osir_config,
        should_reconcile_codex_ownership, validate_input, wait_for_oauth_callback,
        node_distribution_target_for, node_supported, stripped_archive_path, CodexInstallPayload,
        validate_codex_install_payload, write_codex_proxy_config, EncryptedBundle, OpenCodexConfigInput, OpenCodexRouteInput, RedemptionState,
    };
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::Engine;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::rand_core::OsRng;
    use rsa::sha2::Sha256 as RsaSha256;
    use rsa::{Oaep, RsaPrivateKey};
    use serde_json::{json, Map as JsonMap, Value as JsonValue};
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;
    use uuid::Uuid;

    use crate::errors::AppError;

    fn input() -> OpenCodexConfigInput {
        OpenCodexConfigInput {
            enabled: true,
            port: 10100,
            codex_provider_id: "opencodex".to_string(),
            default_route: "osir-gpt/gpt-5.6-sol".to_string(),
            routes: vec![OpenCodexRouteInput {
                id: "osir-gpt".to_string(),
                label: "GPT".to_string(),
                adapter: "openai-responses".to_string(),
                base_url: "https://api.osirclaw.com/v1".to_string(),
                api_key: Some("secret".to_string()),
                models: vec!["gpt-5.6-sol".to_string()],
                default_model: "gpt-5.6-sol".to_string(),
                enabled: true,
            }],
        }
    }

    #[test]
    fn rejects_a_default_route_outside_the_selected_models() {
        let mut value = input();
        value.default_route = "osir-gpt/gpt-5.6-terra".to_string();
        assert!(validate_input(&value).is_err());
    }

    #[test]
    fn retries_transient_route_failures_until_the_model_responds() {
        let attempts = Cell::new(0);
        let result = route_check_with_retry(
            || {
                let current = attempts.get() + 1;
                attempts.set(current);
                if current < 3 {
                    Err(AppError::Engine("unexpected status 502 Bad Gateway".to_string()))
                } else {
                    Ok(b"ok".to_vec())
                }
            },
            &[Duration::ZERO, Duration::ZERO, Duration::ZERO],
        );
        assert_eq!(result.unwrap(), b"ok");
        assert_eq!(attempts.get(), 3);
    }

    #[test]
    fn stops_after_all_transient_route_retries_are_exhausted() {
        let attempts = Cell::new(0);
        let result = route_check_with_retry(
            || {
                attempts.set(attempts.get() + 1);
                Err(AppError::Engine("connection reset by peer".to_string()))
            },
            &[Duration::ZERO, Duration::ZERO, Duration::ZERO],
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 4);
    }

    #[test]
    fn does_not_retry_deterministic_route_authentication_failures() {
        let attempts = Cell::new(0);
        let result = route_check_with_retry(
            || {
                attempts.set(attempts.get() + 1);
                Err(AppError::Engine("unexpected status 401 Unauthorized".to_string()))
            },
            &[Duration::ZERO, Duration::ZERO, Duration::ZERO],
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 1);
        assert!(!is_transient_route_check_error(&AppError::Engine(
            "unexpected status 401 Unauthorized".to_string()
        )));
    }

    #[test]
    fn reconciles_only_mismatched_codex_ownership_states() {
        assert!(!should_reconcile_codex_ownership(false, false));
        assert!(should_reconcile_codex_ownership(false, true));
        assert!(should_reconcile_codex_ownership(true, false));
        assert!(!should_reconcile_codex_ownership(true, true));
    }

    #[test]
    fn requests_restart_only_for_a_running_codex_with_unapplied_configuration() {
        assert!(configuration_requires_restart(
            true,
            true,
            false,
            Some("new"),
            Some("old")
        ));
        assert!(configuration_requires_restart(
            true,
            false,
            true,
            None,
            None
        ));
        assert!(!configuration_requires_restart(
            true,
            true,
            false,
            Some("same"),
            Some("same")
        ));
        assert!(!configuration_requires_restart(
            false,
            true,
            true,
            Some("new"),
            Some("old")
        ));
    }

    #[test]
    fn reconciles_on_first_run_version_change_or_explicit_update_marker() {
        assert!(manager_runtime_needs_reconcile("0.5.32", None, false));
        assert!(manager_runtime_needs_reconcile(
            "0.5.32",
            Some("0.5.31\n"),
            false,
        ));
        assert!(manager_runtime_needs_reconcile(
            "0.5.32",
            Some("0.5.32"),
            true,
        ));
        assert!(!manager_runtime_needs_reconcile(
            "0.5.32",
            Some("0.5.32\n"),
            false,
        ));
    }

    #[test]
    fn sanitizes_legacy_osir_models_and_repairs_a_removed_default() {
        let mut config = JsonMap::from_iter([
            ("defaultProvider".to_string(), json!("osirapi-gemini")),
            (
                "providers".to_string(),
                json!({
                    "osirapi-gemini": {
                        "label": "Gemini",
                        "baseUrl": "https://api.osirclaw.com/v1",
                        "defaultModel": "gemini-2.0-flash",
                        "models": ["gemini-2.0-flash", "gemini-3.7-flash", "gemini-3.7-flash-image"]
                    }
                }),
            ),
            (
                "customModels".to_string(),
                json!([
                    {"provider": "osirapi-gemini", "modelId": "gemini-2.0-flash"},
                    {"provider": "osirapi-gemini", "modelId": "gemini-3.7-flash"},
                    {"provider": "osirapi-gemini", "modelId": "gemini-3.7-flash-image"}
                ]),
            ),
        ]);

        assert!(sanitize_saved_osir_config(&mut config).unwrap());
        assert_eq!(config["defaultProvider"], json!("osirapi-gemini"));
        assert_eq!(
            config["providers"]["osirapi-gemini"]["defaultModel"],
            json!("gemini-3.7-flash")
        );
        assert_eq!(
            config["providers"]["osirapi-gemini"]["models"],
            json!(["gemini-3.7-flash"])
        );
        assert_eq!(config["customModels"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn maps_supported_platforms_without_cross_platform_fallbacks() {
        assert_eq!(component_target_for("macos", "aarch64"), Some("darwin-arm64"));
        assert_eq!(component_target_for("windows", "x86_64"), Some("windows-x64"));
        assert_eq!(component_target_for("linux", "aarch64"), Some("linux-arm64"));
        assert_eq!(component_target_for("freebsd", "x86_64"), None);
        assert_eq!(node_distribution_target_for("linux", "x86_64"), Some("linux-x64"));
    }

    #[test]
    fn accepts_a_dynamic_subscription_provider_set_including_gemini() {
        let payload = CodexInstallPayload {
            providers: vec![
                super::CodexInstallProvider {
                    platform: "openai".into(),
                    provider: "osirapi-openai".into(),
                    api_key: "secret-openai".into(),
                    adapter: "openai-responses".into(),
                    base_url: "https://api.osirclaw.com/v1".into(),
                    models: vec!["gpt-5.6-sol".into()],
                    recommended_model: "gpt-5.6-sol".into(),
                },
                super::CodexInstallProvider {
                    platform: "gemini".into(),
                    provider: "osirapi-gemini".into(),
                    api_key: "secret-gemini".into(),
                    adapter: "openai-responses".into(),
                    base_url: "https://api.osirclaw.com/v1".into(),
                    models: vec!["gemini-2.5-pro".into(), "gemini-2.5-flash".into()],
                    recommended_model: "gemini-2.5-pro".into(),
                },
            ],
            account: None,
        };
        assert!(validate_codex_install_payload(&payload).is_ok());
    }

    #[test]
    fn accepts_only_node_18_or_newer_for_npm_fallback() {
        assert!(!node_supported("16.20.2"));
        assert!(node_supported("18.20.8"));
        assert!(node_supported("22.19.0"));
        assert!(!node_supported("not-a-version"));
    }

    #[test]
    fn strips_archive_root_and_rejects_unsafe_paths() {
        assert_eq!(
            stripped_archive_path(std::path::Path::new("node-v22.19.0/bin/node")).unwrap(),
            Some(std::path::PathBuf::from("bin/node"))
        );
        assert!(stripped_archive_path(std::path::Path::new("../outside")).is_err());
    }

    #[test]
    fn preserves_unmanaged_providers_and_replaces_managed_models() {
        let (_, _, routes) = validate_input(&input()).unwrap();
        let config = JsonMap::from_iter([(
            "providers".to_string(),
            json!({"keep":{"adapter":"openai-responses"},"old":{"adapter":"openai-responses"}}),
        ), (
            "customModels".to_string(),
            json!([
                {"provider":"keep","modelId":"keep-model"},
                {"provider":"old","modelId":"old-model"}
            ]),
        )]);
        let next = build_opencodex_config(config, &routes, &["old".to_string()], 10100).unwrap();
        let providers = next["providers"].as_object().unwrap();
        assert!(providers.contains_key("keep"));
        assert!(providers.contains_key("osir-gpt"));
        assert_eq!(providers["osir-gpt"]["authMode"], json!("key"));
        assert!(providers["osir-gpt"].get("apiKeyTransport").is_none());
        assert_eq!(
            providers["osir-gpt"]["models"],
            json!(["gpt-5.6-sol"])
        );
        assert!(!providers.contains_key("old"));
        let models = next["customModels"].as_array().unwrap();
        assert!(models.iter().any(|model| model["provider"] == "keep"));
        assert!(models.iter().any(|model| model["provider"] == "osir-gpt"));
        assert!(!models.iter().any(|model| model["provider"] == "old"));
    }

    #[test]
    fn requires_every_enabled_route_model_in_the_generated_catalog() {
        let root = std::env::temp_dir().join(format!("opencodex-catalog-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = root.join("catalog.json");
        let routes = vec![
            OpenCodexRouteInput {
                id: "osirapi-openai".to_string(),
                label: "GPT".to_string(),
                adapter: "openai-responses".to_string(),
                base_url: "https://api.osirclaw.com/v1".to_string(),
                api_key: Some("secret".to_string()),
                models: vec!["gpt-5.6".to_string(), "gpt-5.6-sol".to_string()],
                default_model: "gpt-5.6-sol".to_string(),
                enabled: true,
            },
            OpenCodexRouteInput {
                id: "disabled".to_string(),
                label: "Disabled".to_string(),
                adapter: "openai-responses".to_string(),
                base_url: "https://api.osirclaw.com/v1".to_string(),
                api_key: None,
                models: vec!["ignored".to_string()],
                default_model: "ignored".to_string(),
                enabled: false,
            },
        ];
        std::fs::write(
            &catalog,
            serde_json::to_vec(&json!({"models":[{"slug":"osirapi-openai/gpt-5.6-sol"}]})).unwrap(),
        )
        .unwrap();
        assert!(!catalog_contains_enabled_routes(&catalog, &routes));
        std::fs::write(
            &catalog,
            serde_json::to_vec(&json!({"models":[
                {"slug":"osirapi-openai/gpt-5.6"},
                {"slug":"osirapi-openai/gpt-5.6-sol"}
            ]})).unwrap(),
        )
        .unwrap();
        assert!(catalog_contains_enabled_routes(&catalog, &routes));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn does_not_publish_gemini_models_missing_from_the_runtime_catalog() {
        let root = std::env::temp_dir().join(format!("opencodex-catalog-fallback-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = root.join("catalog.json");
        std::fs::write(
            &catalog,
            serde_json::to_vec(&json!({"models":[{"slug":"osirapi-openai/gpt-5.6-sol","display_name":"GPT-5.6-Sol"}]})).unwrap(),
        )
        .unwrap();
        let mut gpt = input().routes[0].clone();
        gpt.id = "osirapi-openai".to_string();
        let gemini = OpenCodexRouteInput {
            id: "osirapi-gemini".into(),
            label: "Gemini".into(),
            adapter: "openai-responses".into(),
            base_url: "https://api.osirclaw.com/v1".into(),
            api_key: Some("secret-gemini".into()),
            models: vec!["gemini-3.7-flash".into()],
            default_model: "gemini-3.7-flash".into(),
            enabled: true,
        };
        let routes = vec![gpt, gemini];
        normalize_synced_catalog(&catalog, &routes).unwrap();
        assert!(!catalog_contains_enabled_routes(&catalog, &routes));
        let value: JsonValue = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
        let slugs = value["models"].as_array().unwrap().iter()
            .filter_map(|model| model["slug"].as_str()).collect::<BTreeSet<_>>();
        assert!(!slugs.contains("osirapi-gemini/gemini-3.7-flash"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn appends_bare_openai_aliases_for_legacy_sessions() {
        let root = std::env::temp_dir().join(format!("opencodex-catalog-legacy-alias-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = root.join("catalog.json");
        std::fs::write(
            &catalog,
            serde_json::to_vec(&json!({"models":[{
                "slug":"osirapi-openai/gpt-5.5",
                "display_name":"gpt-5.5 · GPT",
                "supported_reasoning_levels":[{"effort":"high","description":"High"}],
                "visibility":"list"
            }]})).unwrap(),
        ).unwrap();
        let mut route = input().routes[0].clone();
        route.id = "osirapi-openai".to_string();
        route.models = vec!["gpt-5.5".to_string()];
        normalize_synced_catalog(&catalog, &[route]).unwrap();
        let value: JsonValue = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
        let slugs = value["models"].as_array().unwrap().iter()
            .filter_map(|model| model["slug"].as_str()).collect::<BTreeSet<_>>();
        assert!(slugs.contains("osirapi-openai/gpt-5.5"));
        assert!(slugs.contains("gpt-5.5"));
        let alias = value["models"].as_array().unwrap().iter().find(|model| model["slug"] == "gpt-5.5").unwrap();
        assert_eq!(alias["hidden"], json!(true));
        assert_eq!(alias["visibility"], json!("hide"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn does_not_append_a_legacy_alias_for_an_unexposed_openai_model() {
        let root = std::env::temp_dir().join(format!("opencodex-catalog-no-fake-alias-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = root.join("catalog.json");
        std::fs::write(
            &catalog,
            serde_json::to_vec(&json!({"models":[{"slug":"osirapi-openai/gpt-5.6-sol"}]})).unwrap(),
        )
        .unwrap();
        let mut route = input().routes[0].clone();
        route.id = "osirapi-openai".to_string();
        route.models = vec!["gpt-5.5".to_string()];
        normalize_synced_catalog(&catalog, &[route]).unwrap();
        let value: JsonValue = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
        let slugs = value["models"].as_array().unwrap().iter()
            .filter_map(|model| model["slug"].as_str()).collect::<BTreeSet<_>>();
        assert!(!slugs.contains("gpt-5.5"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sorts_visible_models_by_provider_and_hides_legacy_aliases() {
        let root = std::env::temp_dir().join(format!("opencodex-catalog-sort-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = root.join("catalog.json");
        std::fs::write(
            &catalog,
            serde_json::to_vec(&json!({"models":[
                {"slug":"gpt-5.5"},
                {"slug":"osirapi-grok/grok-4.6"},
                {"slug":"osirapi-openai/gpt-5.5"},
                {"slug":"osirapi-claude/claude-opus-5"}
            ]})).unwrap(),
        ).unwrap();
        let mut gpt = input().routes[0].clone();
        gpt.models = vec!["gpt-5.5".to_string()];
        let claude = OpenCodexRouteInput { id: "osirapi-claude".into(), label: "Claude".into(), adapter: "openai-responses".into(), base_url: "http://localhost".into(), api_key: None, models: vec!["claude-opus-5".into()], default_model: "claude-opus-5".into(), enabled: true };
        let grok = OpenCodexRouteInput { id: "osirapi-grok".into(), label: "Grok".into(), adapter: "openai-responses".into(), base_url: "http://localhost".into(), api_key: None, models: vec!["grok-4.6".into()], default_model: "grok-4.6".into(), enabled: true };
        normalize_synced_catalog(&catalog, &[gpt, claude, grok]).unwrap();
        let value: JsonValue = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        let slugs = models.iter().filter_map(|m| m["slug"].as_str()).collect::<Vec<_>>();
        assert_eq!(slugs[..3], ["osirapi-openai/gpt-5.5", "osirapi-claude/claude-opus-5", "osirapi-grok/grok-4.6"]);
        let alias = models.iter().find(|m| m["slug"] == "gpt-5.5").unwrap();
        assert_eq!(alias["hidden"], json!(true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_opencodex_as_a_custom_codex_provider() {
        let root = std::env::temp_dir().join(format!("opencodex-codex-config-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("config.toml");
        let catalog = root.join("catalog.json");
        std::fs::write(&config, "model_provider = \"openai\"\nopenai_base_url = \"http://127.0.0.1:10100/v1\"\n").unwrap();
        write_codex_proxy_config(&config, &catalog, "opencodex", 10100, "osirapi-openai/gpt-5.6-sol").unwrap();
        let document = std::fs::read_to_string(&config).unwrap().parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(document["model_provider"].as_str(), Some("opencodex"));
        assert!(document.get("openai_base_url").is_none());
        let provider = document["model_providers"]["opencodex"].as_table().unwrap();
        assert_eq!(provider["base_url"].as_str(), Some("http://127.0.0.1:10100/v1"));
        assert_eq!(provider["wire_api"].as_str(), Some("responses"));
        assert_eq!(provider["requires_openai_auth"].as_bool(), Some(false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rebuilds_catalog_routes_from_saved_config_after_a_later_sync() {
        let (_, _, routes) = validate_input(&input()).unwrap();
        let value = build_opencodex_config(JsonMap::new(), &routes, &[], 10100).unwrap();
        let config = value.as_object().unwrap();
        let rebuilt = configured_routes_from_config(config, &["osir-gpt".to_string()]);
        assert_eq!(rebuilt.len(), 1);
        assert_eq!(rebuilt[0].id, "osir-gpt");
        assert_eq!(rebuilt[0].models, vec!["gpt-5.6-sol"]);
        assert!(rebuilt[0].enabled);
    }

    #[test]
    fn discovers_provider_added_directly_in_opencodex_config() {
        let config = JsonMap::from_iter([(
            "providers".to_string(),
            json!({
                "oauth-claude": {
                    "adapter": "openai-responses",
                    "baseUrl": "https://provider.example/v1",
                    "defaultModel": "claude-sonnet",
                    "label": "我的 Claude",
                    "authMode": "oauth",
                    "models": ["claude-sonnet", "claude-haiku"]
                }
            }),
        )]);
        let ids = inferred_managed_provider_ids(&config, &[]);
        assert_eq!(ids, vec!["oauth-claude"]);
        let route = route_from_config("oauth-claude", &config, &[], None, &BTreeMap::new()).unwrap();
        assert_eq!(route.models, vec!["claude-haiku", "claude-sonnet"]);
        assert!(route.api_key_configured);
        assert_eq!(route.availability, "configured");
    }

    #[test]
    fn maps_default_model_health_from_the_complete_route_key() {
        let config = JsonMap::from_iter([(
            "providers".to_string(),
            json!({
                "osirapi-openai": {
                    "adapter": "openai-responses",
                    "baseUrl": "https://api.osirclaw.com/v1",
                    "defaultModel": "gpt-5.6-sol",
                    "apiKey": "secret",
                    "models": ["gpt-5.6-sol"]
                }
            }),
        )]);
        let health = BTreeMap::from([(
            "osirapi-openai/gpt-5.6-sol".to_string(),
            "degraded".to_string(),
        )]);
        let route = route_from_config("osirapi-openai", &config, &[], None, &health).unwrap();
        assert_eq!(route.availability, "degraded");
    }

    #[test]
    fn infers_existing_manager_routes_from_custom_models_when_state_is_missing() {
        let config = JsonMap::from_iter([
            (
                "defaultProvider".to_string(),
                json!("osirapi-openai"),
            ),
            (
                "providers".to_string(),
                json!({
                    "openai": {"disabled": true},
                    "osirapi-openai": {"baseUrl": "https://api.osirclaw.com/v1", "defaultModel": "gpt-5.6-sol"},
                    "osirapi-claude": {"baseUrl": "https://api.osirclaw.com/v1", "defaultModel": "claude-opus-5"}
                }),
            ),
            (
                "customModels".to_string(),
                json!([
                    {"provider":"osirapi-openai","modelId":"gpt-5.6-sol"},
                    {"provider":"osirapi-claude","modelId":"claude-opus-5"}
                ]),
            ),
        ]);
        let models = config["customModels"].as_array().unwrap();
        assert_eq!(
            inferred_managed_provider_ids(&config, models),
            vec!["osirapi-claude", "osirapi-openai"]
        );
        assert_eq!(
            inferred_default_route(
                &config,
                &["osirapi-claude".to_string(), "osirapi-openai".to_string()]
            ),
            Some("osirapi-openai/gpt-5.6-sol".to_string())
        );
    }

    #[test]
    fn extracts_only_a_well_formed_osir_connection_ticket() {
        let ticket = format!("ocx_{}", "a".repeat(48));
        assert_eq!(extract_osir_ticket(&format!("请导入：{ticket}")).unwrap(), ticket);
        assert!(extract_osir_ticket("ocx_not-a-ticket").is_err());
    }

    #[test]
    fn decrypts_the_existing_osir_rsa_and_aes_gcm_bundle_shape() {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
        let state = RedemptionState {
            private_key: private.to_pkcs8_pem(LineEnding::LF).unwrap().to_string(),
            public_key: private.to_public_key().to_public_key_pem(LineEnding::LF).unwrap(),
            idempotency_key: "test".to_string(),
        };
        let plaintext = serde_json::to_vec(&json!({
            "providers": [{
                "platform": "openai",
                "provider": "osirapi-openai",
                "api_key": "secret",
                "adapter": "openai-responses",
                "base_url": "https://api.osirclaw.com/v1",
                "models": ["gpt-5.6-sol"],
                "recommended_model": "gpt-5.6-sol"
            }],
            "account": {
                "user_id": 41,
                "display_name": "订阅用户",
                "email": "user@example.com",
                "balance": 12.5,
                "subscriptions": [{
                    "id": 91,
                    "group_name": "多模型订阅",
                    "status": "active",
                    "expires_at": "2026-09-20T00:00:00Z",
                    "days_remaining": 31,
                    "monthly_used_usd": 8.5,
                    "monthly_limit_usd": 100.0,
                    "monthly_remaining_usd": 91.5
                }]
            }
        })).unwrap();
        let aes_key = [7_u8; 32];
        let iv = [9_u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&aes_key).unwrap();
        let ciphertext = cipher.encrypt(Nonce::from_slice(&iv), plaintext.as_ref()).unwrap();
        let wrapped = private
            .to_public_key()
            .encrypt(&mut OsRng, Oaep::new::<RsaSha256>(), &aes_key)
            .unwrap();
        let result: CodexInstallPayload = decrypt_bundle(&state, EncryptedBundle {
            wrapped_key: base64::engine::general_purpose::STANDARD.encode(wrapped),
            iv: base64::engine::general_purpose::STANDARD.encode(iv),
            ciphertext: base64::engine::general_purpose::STANDARD.encode(ciphertext),
        }).unwrap();
        assert_eq!(result.providers[0].provider, "osirapi-openai");
        assert_eq!(result.providers[0].models, vec!["gpt-5.6-sol"]);
        let account = result.account.expect("account summary");
        assert_eq!(account.user_id, 41);
        assert_eq!(account.display_name.as_deref(), Some("订阅用户"));
        assert_eq!(account.subscriptions[0].days_remaining, 31);
        assert_eq!(account.subscriptions[0].monthly_remaining_usd, 91.5);
    }

    #[test]
    fn generates_the_standard_pkce_s256_challenge() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn accepts_a_loopback_oauth_callback_with_matching_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            let lf = String::from_utf8(vec![10]).unwrap();
            let request = [
                "GET /oauth/callback?code=auth-code&state=expected-state HTTP/1.1",
                "Host: 127.0.0.1",
                "",
                "",
            ]
            .join(&lf);
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200 OK"));
            assert!(response.contains("授权回调已收到"));
            assert!(response.contains("安装并同步本地模型"));
        });
        let callback = wait_for_oauth_callback(listener, "expected-state").unwrap();
        client.join().unwrap();
        assert_eq!(callback.code, "auth-code");
        assert_eq!(callback.state, "expected-state");
        assert!(callback.error.is_none());
    }
}
