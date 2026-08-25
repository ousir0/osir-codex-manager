//! `.codexskin` import: a theme package zipped with `theme.json` at the
//! archive root. Extraction is paranoid (zip-slip, size/entry caps), the
//! extracted tree must pass the full `load_theme` validation, and the final
//! placement into `<themes_root>/<id>` is transactional — an existing install
//! of the same id is swapped out and restored on any failure.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::theme::{load_theme, summarize, ThemeSummary};
use crate::{Result, ThemeEngineError};

/// Caps chosen far above any real package (biggest studio theme ≈ 3 MB
/// zipped, ~40 entries) but low enough to shrug off a zip bomb.
const MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ENTRIES: usize = 500;
const DREAMSKIN_TARGET_BYTES: usize = 1_350_000;

fn err(message: impl Into<String>) -> ThemeEngineError {
    ThemeEngineError::Theme(message.into())
}

/// A zip entry name is accepted only as a plain relative path — no roots, no
/// parent hops, no drive prefixes (`ZipFile::enclosed_name` plus belt and
/// braces for portability).
fn safe_entry_path(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        Some(path.to_path_buf())
    } else {
        None
    }
}

fn dreamskin_css() -> &'static str {
    r#"html.codex-theme-studio { color-scheme: dark; }
html.codex-theme-studio body {
  background: linear-gradient(90deg, color-mix(in srgb, var(--cts-color-background) 86%, transparent), color-mix(in srgb, var(--cts-color-background) 48%, transparent)), var(--cts-asset-background) center / cover no-repeat fixed, var(--cts-color-background) !important;
  color: var(--cts-color-text) !important;
}
html.codex-theme-studio .app-theme { background: transparent !important; }
html.codex-theme-studio .app-shell-left-panel {
  background: color-mix(in srgb, var(--cts-color-panel) 90%, transparent) !important;
  border-right: 1px solid var(--cts-color-line) !important;
  backdrop-filter: blur(18px) saturate(1.08);
}
html.codex-theme-studio :is(main.main-surface, div.main-surface) {
  background: color-mix(in srgb, var(--cts-color-background) 38%, transparent) !important;
}
html.codex-theme-studio :is(input, textarea, [contenteditable="true"], [role="dialog"]) {
  background-color: color-mix(in srgb, var(--cts-color-panel) 86%, transparent) !important;
  border-color: var(--cts-color-line) !important;
  color: var(--cts-color-text) !important;
}
html.codex-theme-studio :is(button, [role="button"]):hover {
  background-color: color-mix(in srgb, var(--cts-color-accent) 16%, transparent) !important;
}
html.codex-theme-studio :is(button, input, textarea, [contenteditable="true"]):focus-visible {
  outline: 2px solid var(--cts-color-accent) !important;
  outline-offset: 2px;
}
"#
}

fn dreamskin_fallback_css() -> &'static str {
    r#"
html.codex-theme-studio [data-ds-part="root"] { color: var(--ds-theme-color-text) !important; }
html.codex-theme-studio [data-ds-part="main"] { background-color: var(--ds-theme-color-background) !important; }
html.codex-theme-studio [data-ds-part="sidebar"] {
  background-color: color-mix(in srgb, var(--ds-theme-color-panel) 90%, transparent) !important;
  border-right: 1px solid var(--ds-theme-color-line) !important;
  backdrop-filter: blur(var(--ds-theme-surface-blur));
}
html.codex-theme-studio [data-ds-part="composer"] {
  background-color: color-mix(in srgb, var(--ds-theme-color-panel-alt) 92%, transparent) !important;
  border: 1px solid var(--ds-theme-color-line) !important;
  color: var(--ds-theme-color-text) !important;
}
html.codex-theme-studio [data-ds-part="message"] {
  background-color: color-mix(in srgb, var(--ds-theme-color-panel) 86%, transparent) !important;
  border: 1px solid var(--ds-theme-color-line) !important;
  color: var(--ds-theme-color-text) !important;
}
html.codex-theme-studio [data-ds-part="dialog"] {
  background-color: color-mix(in srgb, var(--ds-theme-color-panel-alt) 94%, transparent) !important;
  border: 1px solid var(--ds-theme-color-line) !important;
  color: var(--ds-theme-color-text) !important;
  backdrop-filter: blur(var(--ds-theme-surface-blur));
}
html.codex-theme-studio [data-ds-part="composer"] :is(button, [role="button"]):hover {
  background-color: color-mix(in srgb, var(--ds-theme-color-accent) 18%, transparent) !important;
}
html.codex-theme-studio [data-ds-part="composer"] :is(input, textarea, [contenteditable="true"]):focus-visible {
  outline: 2px solid var(--ds-theme-color-accent) !important;
  outline-offset: 2px;
}
"#
}

fn fit_dreamskin_background(source: &Path) -> Result<Vec<u8>> {
    let raw = std::fs::read(source).map_err(|e| err(format!("读取 DreamSkin 背景失败: {e}")))?;
    let image = image::load_from_memory(&raw).map_err(|e| err(format!("解码 DreamSkin 背景失败: {e}")))?;
    for width in [1920_u32, 1600, 1366, 1280, 1024] {
        let resized = if image.width() > width {
            image.resize(width, u32::MAX, image::imageops::FilterType::Lanczos3)
        } else {
            image.clone()
        };
        for quality in [82_u8, 74, 66, 58, 50] {
            let mut output = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality)
                .encode_image(&resized)
                .map_err(|e| err(format!("转换 DreamSkin 背景失败: {e}")))?;
            if output.len() <= DREAMSKIN_TARGET_BYTES {
                return Ok(output);
            }
        }
    }
    Err(err("DreamSkin 背景转换后仍超过 Manager 主题资源上限"))
}

fn normalize_dreamskin_colors(raw: Option<&serde_json::Value>) -> serde_json::Value {
    let Some(map) = raw.and_then(serde_json::Value::as_object) else {
        return serde_json::json!({});
    };
    let mut output = serde_json::Map::new();
    for (key, value) in map {
        let mut normalized = String::new();
        for (index, ch) in key.chars().enumerate() {
            if ch.is_ascii_uppercase() {
                if index > 0 { normalized.push('-'); }
                normalized.push(ch.to_ascii_lowercase());
            } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
                normalized.push(ch);
            }
        }
        if !normalized.is_empty() && normalized.len() <= 64 {
            if let Some(value) = value.as_str() {
                output.insert(normalized, serde_json::Value::String(value.trim().to_string()));
            }
        }
    }
    serde_json::Value::Object(output)
}

fn convert_dreamskin_v1(staging: &Path) -> Result<()> {
    let theme_path = staging.join("theme.json");
    let raw: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&theme_path).map_err(|e| err(format!("读取 DreamSkin theme.json 失败: {e}")))?,
    ).map_err(|e| err(format!("DreamSkin theme.json 无效: {e}")))?;
    if raw.get("schemaVersion").and_then(serde_json::Value::as_u64) != Some(1) {
        return Ok(());
    }
    let manifest = std::fs::read(staging.join("manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .unwrap_or_default();
    let id = raw.get("id").and_then(serde_json::Value::as_str).unwrap_or("dreamskin-theme");
    let name = raw.get("name").and_then(serde_json::Value::as_str).unwrap_or(id);
    let image_name = raw.get("image").and_then(serde_json::Value::as_str)
        .ok_or_else(|| err("DreamSkin theme.json 缺少 image"))?;
    let image_rel = safe_entry_path(image_name).ok_or_else(|| err("DreamSkin image 路径不安全"))?;
    let converted = fit_dreamskin_background(&staging.join(image_rel))?;
    std::fs::create_dir_all(staging.join("assets")).map_err(|e| err(format!("创建 DreamSkin assets 失败: {e}")))?;
    std::fs::create_dir_all(staging.join("previews")).map_err(|e| err(format!("创建 DreamSkin previews 失败: {e}")))?;
    std::fs::write(staging.join("assets/background.jpg"), &converted).map_err(|e| err(format!("写入 DreamSkin 背景失败: {e}")))?;
    std::fs::write(staging.join("previews/home.jpg"), &converted).map_err(|e| err(format!("写入 DreamSkin 预览失败: {e}")))?;
    let appearance = match raw.get("appearance").and_then(serde_json::Value::as_str) {
        Some("dark") => "dark", Some("light") => "light", _ => "dual",
    };
    let author = manifest.pointer("/publisher/displayName").and_then(serde_json::Value::as_str).unwrap_or("DreamSkin 社区作者");
    let license = manifest.get("license").and_then(serde_json::Value::as_str).unwrap_or("未声明");
    let version = manifest.get("version").and_then(serde_json::Value::as_str).unwrap_or("0.0.0");
    let converted_theme = serde_json::json!({
        "schemaVersion": 2, "id": id, "name": name,
        "description": format!("来源于 DreamSkin 社区的主题：{name}"),
        "version": version, "author": author, "appearance": appearance,
        "license": license, "category": "dreamskin-community",
        "tags": ["dreamskin", "community"], "previews": ["previews/home.jpg"],
        "colors": normalize_dreamskin_colors(raw.get("colors")),
        "assets": {"background": "assets/background.jpg"}
    });
    let encoded = serde_json::to_vec_pretty(&converted_theme).map_err(|e| err(format!("生成 Manager 主题配置失败: {e}")))?;
    std::fs::write(&theme_path, encoded).map_err(|e| err(format!("写入 Manager 主题配置失败: {e}")))?;
    let source_css = std::fs::read_to_string(staging.join("theme.css")).unwrap_or_default();
    let css = format!(
        "{}\n{}\n{}",
        dreamskin_css(),
        dreamskin_fallback_css(),
        source_css.trim(),
    );
    std::fs::write(staging.join("theme.css"), css).map_err(|e| err(format!("写入 Manager 主题样式失败: {e}")))?;
    Ok(())
}

/// Import a `.codexskin` archive into `themes_root`, returning the installed
/// theme's summary. The archive must carry `theme.json` at its root.
pub fn import_codexskin(archive_path: &Path, themes_root: &Path) -> Result<ThemeSummary> {
    let archive_meta = std::fs::metadata(archive_path)
        .map_err(|e| err(format!("无法读取主题包: {e}")))?;
    if archive_meta.len() > MAX_ARCHIVE_BYTES {
        return Err(err("主题包超过大小上限 (50MB)"));
    }
    let file =
        std::fs::File::open(archive_path).map_err(|e| err(format!("无法打开主题包: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| err(format!("主题包不是有效的 zip: {e}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(err("主题包条目数超过上限"));
    }
    if archive.by_name("theme.json").is_err() {
        return Err(err(
            "主题包缺少根级 theme.json（.codexskin 的内容必须位于压缩包根目录）",
        ));
    }

    std::fs::create_dir_all(themes_root)
        .map_err(|e| err(format!("无法创建主题目录: {e}")))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let staging = themes_root.join(format!(".import-{}-{nonce}", std::process::id()));
    let outcome = extract_and_place(&mut archive, &staging, themes_root);
    let _ = std::fs::remove_dir_all(&staging);
    outcome
}

fn extract_and_place(
    archive: &mut zip::ZipArchive<std::fs::File>,
    staging: &Path,
    themes_root: &Path,
) -> Result<ThemeSummary> {
    std::fs::create_dir_all(staging).map_err(|e| err(format!("创建临时目录失败: {e}")))?;

    let mut unpacked: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| err(format!("读取压缩包条目失败: {e}")))?;
        let Some(relative) = entry
            .enclosed_name()
            .and_then(|p| safe_entry_path(p.to_string_lossy().as_ref()))
        else {
            return Err(err(format!("主题包含非法路径条目: {}", entry.name())));
        };
        if entry.is_dir() {
            std::fs::create_dir_all(staging.join(&relative))
                .map_err(|e| err(format!("解压目录失败: {e}")))?;
            continue;
        }
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_UNPACKED_BYTES {
            return Err(err("主题包解压后超过大小上限"));
        }
        let target = staging.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| err(format!("解压目录失败: {e}")))?;
        }
        let mut contents = Vec::with_capacity(entry.size().min(8 * 1024 * 1024) as usize);
        entry
            .take(MAX_UNPACKED_BYTES)
            .read_to_end(&mut contents)
            .map_err(|e| err(format!("解压 {} 失败: {e}", relative.display())))?;
        std::fs::write(&target, contents)
            .map_err(|e| err(format!("写入 {} 失败: {e}", relative.display())))?;
    }

    convert_dreamskin_v1(staging)?;
    // Full package validation before anything touches the live gallery.
    let theme = load_theme(staging)?;
    let id = theme.config.id.clone();

    // Transactional placement: park any existing install of this id, move the
    // new tree in, drop the parked copy only on success.
    let destination = themes_root.join(&id);
    let parked = themes_root.join(format!(".replaced-{id}-{}", std::process::id()));
    let had_previous = destination.exists();
    if had_previous {
        std::fs::rename(&destination, &parked)
            .map_err(|e| err(format!("暂存旧版本失败: {e}")))?;
    }
    match std::fs::rename(staging, &destination) {
        Ok(()) => {
            if had_previous {
                let _ = std::fs::remove_dir_all(&parked);
            }
            let installed = load_theme(&destination)?;
            Ok(summarize(installed))
        }
        Err(error) => {
            if had_previous {
                let _ = std::fs::rename(&parked, &destination);
            }
            Err(err(format!("安装主题失败: {error}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    fn minimal_entries(id: &str) -> Vec<(String, Vec<u8>)> {
        vec![
            (
                "theme.json".to_string(),
                format!(
                    r##"{{"schemaVersion":2,"id":"{id}","name":"T","version":"1.0.0",
                        "colors":{{"accent":"#abc"}},
                        "previews":["previews/home.webp"]}}"##
                )
                .into_bytes(),
            ),
            ("theme.css".to_string(), b"html.codex-theme-studio {}\n".to_vec()),
            ("previews/home.webp".to_string(), vec![0x52, 0x49, 0x46, 0x46, 1, 2]),
        ]
    }

    #[test]
    fn imports_a_valid_codexskin_and_replaces_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("themes");
        let skin = tmp.path().join("pkg.codexskin");
        let entries = minimal_entries("import-me");
        let refs: Vec<(&str, &[u8])> =
            entries.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        write_zip(&skin, &refs);

        let summary = import_codexskin(&skin, &root).unwrap();
        assert_eq!(summary.id, "import-me");
        assert_eq!(summary.meta.version.as_deref(), Some("1.0.0"));
        assert!(summary.preview.as_ref().unwrap().ends_with("previews/home.webp"));
        assert!(root.join("import-me/theme.json").is_file());

        // Re-import (upgrade path) replaces in place and leaves no debris.
        let summary2 = import_codexskin(&skin, &root).unwrap();
        assert_eq!(summary2.id, "import-me");
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "staging/parked dirs must be cleaned");
    }

    #[test]
    fn rejects_zip_slip_and_missing_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("themes");

        let evil = tmp.path().join("evil.codexskin");
        write_zip(
            &evil,
            &[
                ("theme.json", br#"{"schemaVersion":2,"id":"evil"}"# as &[u8]),
                ("../escape.txt", b"boom"),
            ],
        );
        assert!(import_codexskin(&evil, &root).is_err());
        assert!(!tmp.path().join("escape.txt").exists());

        let hollow = tmp.path().join("hollow.codexskin");
        write_zip(&hollow, &[("readme.md", b"hi" as &[u8])]);
        let error = import_codexskin(&hollow, &root).unwrap_err().to_string();
        assert!(error.contains("theme.json"), "{error}");
    }

    #[test]
    fn rejects_invalid_package_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("themes");
        let bad = tmp.path().join("bad.codexskin");
        // Valid zip, but schemaVersion is wrong — must fail load_theme and
        // leave the gallery untouched.
        write_zip(
            &bad,
            &[("theme.json", br#"{"schemaVersion":1,"id":"bad"}"# as &[u8])],
        );
        assert!(import_codexskin(&bad, &root).is_err());
        assert!(!root.join("bad").exists());
    }

    #[test]
    fn imports_a_codexskin_with_motion() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("themes");
        let skin = tmp.path().join("motion.codexskin");
        // A package carrying an mp4 motion asset (plus its required static intro
        // fallback) must import without any extra rejection path.
        write_zip(
            &skin,
            &[
                (
                    "theme.json",
                    br#"{"schemaVersion":2,"id":"motion-skin","name":"M","version":"1.0.0",
                        "assets":{"intro":"assets/intro.webp"},
                        "motionAssets":{"intro-video":"assets/intro-video.mp4"},
                        "previews":["previews/home.webp"]}"# as &[u8],
                ),
                ("theme.css", b"html.codex-theme-studio {}\n"),
                ("assets/intro.webp", b"RIFF\x01\x02"),
                ("assets/intro-video.mp4", b"\x00\x00\x00\x18ftypisom"),
                ("previews/home.webp", b"RIFF\x01\x02"),
            ],
        );

        let summary = import_codexskin(&skin, &root).unwrap();
        assert_eq!(summary.id, "motion-skin");
        // The installed package validates and exposes the motion asset with its
        // video mime — ready for the payload's dedicated motion data-URL map.
        let installed = crate::theme::load_theme(&root.join("motion-skin")).unwrap();
        assert!(installed.motion_assets.contains_key("intro-video"));
        assert_eq!(installed.motion_assets["intro-video"].mime, "video/mp4");
    }

    #[test]
    fn imports_and_converts_a_dreamskin_v1_package() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("themes");
        let skin = tmp.path().join("dreamskin.zip");
        let background = {
            let image = image::DynamicImage::new_rgb8(64, 36);
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80)
                .encode_image(&image)
                .unwrap();
            bytes
        };
        write_zip(
            &skin,
            &[
                (
                    "manifest.json",
                    br#"{"version":"0.1.0","license":"MIT","publisher":{"displayName":"Dream Author"}}"#,
                ),
                (
                    "theme.json",
                    br##"{"schemaVersion":1,"id":"dream-v1","name":"Dream V1","image":"background.jpg","appearance":"dark","colors":{"background":"#101010","panel":"#181818","accentAlt":"#55e7ff","panelAlt":"#202020","text":"#f4f7ff","muted":"#999999","line":"#333333"}}"##,
                ),
                ("theme.css", br#"[data-ds-part="root"] { color: #fff; }"#),
                ("background.jpg", background.as_slice()),
            ],
        );
        let summary = import_codexskin(&skin, &root).unwrap();
        assert_eq!(summary.id, "dream-v1");
        assert_eq!(summary.meta.version.as_deref(), Some("0.1.0"));
        assert_eq!(summary.meta.author.as_deref(), Some("Dream Author"));
        assert_eq!(summary.meta.license.as_deref(), Some("MIT"));
        let loaded = crate::theme::load_theme(&root.join("dream-v1")).unwrap();
        assert_eq!(loaded.config.colors.get("accent-alt").map(String::as_str), Some("#55e7ff"));
        assert_eq!(loaded.config.colors.get("panel-alt").map(String::as_str), Some("#202020"));
        assert!(root.join("dream-v1/assets/background.jpg").is_file());
        assert!(root.join("dream-v1/previews/home.jpg").is_file());
        let css = std::fs::read_to_string(root.join("dream-v1/theme.css")).unwrap();
        assert!(css.contains("[data-ds-part=\"root\"]"));
        assert!(css.contains("[data-ds-part=\"message\"]"));
        assert!(css.contains("--ds-theme-color-accent"));
    }
}
