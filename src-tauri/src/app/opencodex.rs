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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

use crate::app::{atomic_file, paths};
use crate::errors::AppError;

const DEFAULT_PORT: u16 = 10100;
const DEFAULT_PROVIDER_ID: &str = "opencodex";
const DEFAULT_VERSION: &str = "2.22.0";
const COMPONENT_MANIFEST_URL: &str = "https://app.osirclaw.com/components/opencodex/index.json";
const COMPONENT_MANIFEST_FALLBACK_URL: &str = "https://raw.githubusercontent.com/ousir0/osir-codex-manager/main/components/opencodex/index.json";
const OSIRAPI_DESKTOP_CONNECT_URL: &str = "https://osirclaw.com/codex-manager/connect";
const OSIRAPI_DESKTOP_EXCHANGE_URL: &str = "https://api.osirclaw.com/api/v1/codex-install/desktop/exchange";
const MAX_ROUTE_COUNT: usize = 32;
const MAX_MODELS_PER_ROUTE: usize = 256;
const MAX_ID_LEN: usize = 96;
const MAX_VALUE_LEN: usize = 4096;

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
}

#[derive(Debug, Deserialize)]
struct ComponentManifest {
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

fn component_target() -> &'static str {
    match (cfg!(target_os = "windows"), cfg!(target_arch = "aarch64")) {
        (true, true) => "windows-arm64",
        (true, false) => "windows-x64",
        (false, true) => "darwin-arm64",
        (false, false) => "darwin-x64",
    }
}

fn managed_component_root() -> Option<PathBuf> {
    paths::data_dir().map(|dir| dir.join("opencodex").join("components").join(DEFAULT_VERSION).join(component_target()))
}

fn ocx_invocation() -> Option<(String, Vec<String>)> {
    if let Some(root) = managed_component_root() {
        let node = root.join(if cfg!(target_os = "windows") { "runtime/node.exe" } else { "runtime/bin/node" });
        let launcher = root.join("opencodex/node_modules/@bitkyc08/opencodex/bin/ocx.mjs");
        if node.is_file() && launcher.is_file() {
            return Some((node.display().to_string(), vec![launcher.display().to_string()]));
        }
    }
    for candidate in if cfg!(target_os = "windows") { ["ocx.cmd", "ocx"] } else { ["ocx", "ocx"] } {
        if Command::new(candidate).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success()) {
            return Some((candidate.to_string(), Vec::new()));
        }
    }
    None
}

fn ocx_program() -> Option<String> {
    ocx_invocation().map(|(program, _)| program)
}

fn component_sha256(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path).map_err(|error| AppError::Internal(format!("读取 OpenCodex 组件失败：{error}")))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn download_component(url: &str, path: &Path) -> Result<(), AppError> {
    let output = Command::new(if cfg!(target_os = "windows") { "curl.exe" } else { "/usr/bin/curl" })
        .args(["-fsSL", "--proto", "=https", "--max-time", "300", url, "-o", path.to_string_lossy().as_ref()])
        .output()
        .map_err(|error| AppError::Engine(format!("下载 OpenCodex 组件失败：{error}")))?;
    if !output.status.success() { return Err(AppError::Engine("OpenCodex 组件下载失败，请检查网络后重试".to_string())); }
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

fn install_component_from_manifest() -> Result<(), AppError> {
    let data_dir = paths::data_dir().ok_or_else(|| AppError::Internal("无法定位 OpenCodex 组件目录".to_string()))?;
    let manifest_path = data_dir.join("opencodex").join("component-manifest.json");
    if let Some(parent) = manifest_path.parent() { fs::create_dir_all(parent).ok(); }
    let manifest_bytes = [COMPONENT_MANIFEST_URL, COMPONENT_MANIFEST_FALLBACK_URL].iter().find_map(|url| {
        let temp = manifest_path.with_extension(format!("download-{}", std::process::id()));
        if download_component(url, &temp).is_ok() { let bytes = fs::read(&temp).ok(); fs::remove_file(&temp).ok(); bytes } else { None }
    }).ok_or_else(|| AppError::Engine("暂时无法获取 OpenCodex 组件清单".to_string()))?;
    atomic_file::write_atomic(&manifest_path, &manifest_bytes).map_err(|error| AppError::Internal(format!("保存 OpenCodex 组件清单失败：{error}")))?;
    let manifest: ComponentManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| AppError::Engine(format!("OpenCodex 组件清单无效：{error}")))?;
    let target = manifest.targets.get(component_target()).ok_or_else(|| AppError::Engine("当前平台没有可用的 OpenCodex 组件".to_string()))?;
    let archive = data_dir.join("opencodex").join(format!("component-{}-{}.zip", component_target(), DEFAULT_VERSION));
    if !archive.is_file() || component_sha256(&archive).ok().as_deref() != Some(target.sha256.as_str()) {
        let temp = archive.with_extension(format!("download-{}", std::process::id()));
        download_component(&target.url, &temp).or_else(|_| download_component(&target.github_url, &temp))?;
        if component_sha256(&temp)? != target.sha256 { fs::remove_file(&temp).ok(); return Err(AppError::Engine("OpenCodex 组件 SHA-256 校验失败".to_string())); }
        fs::rename(temp, &archive).map_err(|error| AppError::Internal(format!("保存 OpenCodex 组件失败：{error}")))?;
    }
    let destination = data_dir.join("opencodex").join("components").join(DEFAULT_VERSION).join(component_target());
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
    let endpoint = "https://api.osirclaw.com/api/v1/codex-install/tickets/redeem";
    let mut child = Command::new(if cfg!(target_os = "windows") { "curl.exe" } else { "/usr/bin/curl" })
        .args([
            "-sS",
            "--fail-with-body",
            "--proto",
            "=https",
            "--max-time",
            "30",
            "--header",
            "Content-Type: application/json",
            "--header",
            "Cache-Control: no-store",
            "--data-binary",
            "@-",
            endpoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Engine(format!("无法连接 OSIRAPI：{error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| AppError::Internal("无法写入 OSIRAPI 连接请求".to_string()))?
        .write_all(&payload)
        .map_err(|error| AppError::Internal(format!("写入 OSIRAPI 连接请求失败：{error}")))?;
    let output = child
        .wait_with_output()
        .map_err(|error| AppError::Engine(format!("OSIRAPI 连接失败：{error}")))?;
    let response = serde_json::from_slice::<JsonValue>(&output.stdout).ok();
    if !output.status.success() {
        let message = response
            .as_ref()
            .and_then(|body| body.get("message").or_else(|| body.get("error")))
            .and_then(JsonValue::as_str)
            .unwrap_or("OSIRAPI 连接码兑换失败");
        return Err(AppError::Engine(message.to_string()));
    }
    let data = response
        .and_then(|body| body.get("data").cloned().or(Some(body)))
        .ok_or_else(|| AppError::Engine("OSIRAPI 未返回连接配置".to_string()))?;
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
    let mut child = Command::new(if cfg!(target_os = "windows") { "curl.exe" } else { "/usr/bin/curl" })
        .args([
            "-sS",
            "--fail-with-body",
            "--proto",
            "=https",
            "--max-time",
            "30",
            "--header",
            "Content-Type: application/json",
            "--header",
            "Cache-Control: no-store",
            "--data-binary",
            "@-",
            OSIRAPI_DESKTOP_EXCHANGE_URL,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Engine(format!("无法连接 OSIRAPI：{error}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| AppError::Internal("无法写入 OSIRAPI OAuth 请求".to_string()))?
        .write_all(&payload)
        .map_err(|error| AppError::Internal(format!("写入 OSIRAPI OAuth 请求失败：{error}")))?;
    let output = child
        .wait_with_output()
        .map_err(|error| AppError::Engine(format!("OSIRAPI OAuth 请求失败：{error}")))?;
    let response = serde_json::from_slice::<JsonValue>(&output.stdout).ok();
    if !output.status.success() {
        let message = response
            .as_ref()
            .and_then(|body| body.get("message").or_else(|| body.get("error")))
            .and_then(JsonValue::as_str)
            .unwrap_or("OSIRAPI OAuth 兑换失败");
        return Err(AppError::Engine(message.to_string()));
    }
    let data = response
        .and_then(|body| body.get("data").cloned().or(Some(body)))
        .ok_or_else(|| AppError::Engine("OSIRAPI OAuth 未返回配置".to_string()))?;
    serde_json::from_value::<DesktopExchangeResponse>(data)
        .map_err(|error| AppError::Engine(format!("OSIRAPI OAuth 配置格式无效：{error}")))
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
        "grok" => "Grok",
        _ => "OSIR",
    }
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
    let next_state = ManagedState { enabled: true, port, codex_provider_id, managed_provider_ids: state.managed_provider_ids, locked_route: Some(route_id), route_health: state.route_health };
    write_json(&paths.state, &serde_json::to_value(next_state).map_err(|error| AppError::Internal(format!("保存锁定路由失败：{error}")))?)?;
    status_at(&paths)
}

pub fn remove_model(route_id: &str, model: &str) -> Result<OpenCodexStatus, AppError> {
    let paths = integration_paths()?;
    let state = effective_state(&paths)?;
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
        })
        .map_err(|error| AppError::Internal(format!("保存模型移除状态失败：{error}")))?,
    )?;
    status_at(&paths)
}

pub fn check_route(route_id: &str, model: &str) -> Result<OpenCodexRouteCheck, AppError> {
    let route_id = checked_id(route_id, "路由 ID")?;
    let model = checked_text(model, "模型名称")?;
    let route = format!("{route_id}/{model}");
    let check = match ocx_output(&["access", "test", &route, "--protocol", "responses", "--json"]) {
        Ok(_) => OpenCodexRouteCheck { route_id: route_id.clone(), model: model.clone(), available: true, detail: "路由验证成功".to_string(), checked_at: timestamp_marker() },
        Err(error) => OpenCodexRouteCheck { route_id: route_id.clone(), model: model.clone(), available: false, detail: error.to_string(), checked_at: timestamp_marker() },
    };
    if let Ok(paths) = integration_paths() {
        let mut state = effective_state(&paths).unwrap_or_else(|_| load_state(&paths.state));
        state.route_health.insert(route, if check.available { "verified" } else { "offline" }.to_string());
        if let Ok(value) = serde_json::to_value(state) {
            let _ = write_json(&paths.state, &value);
        }
    }
    Ok(check)
}

pub fn ensure_ready_for_codex() -> Result<(), AppError> {
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

fn apply_codex_install_payload(payload: CodexInstallPayload) -> Result<OpenCodexStatus, AppError> {
    if payload.providers.is_empty() || payload.providers.len() > MAX_ROUTE_COUNT {
        return Err(AppError::Engine("OSIRAPI 未返回可用多模型路由".to_string()));
    }
    let routes = payload
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
    let default_route = routes
        .iter()
        .find(|route| route.id.contains("openai"))
        .or_else(|| routes.first())
        .map(|route| format!("{}/{}", route.id, route.default_model))
        .ok_or_else(|| AppError::Engine("OSIRAPI 未返回默认模型".to_string()))?;
    let configured = save(OpenCodexConfigInput {
        enabled: true,
        port: DEFAULT_PORT,
        codex_provider_id: DEFAULT_PROVIDER_ID.to_string(),
        default_route,
        routes,
    })?;
    let synced = sync()?;
    Ok(OpenCodexStatus { error: configured.error.or(synced.error), ..synced })
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

pub fn connect_osir_oauth() -> Result<OpenCodexStatus, AppError> {
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
    let mut authorization_url = Url::parse(OSIRAPI_DESKTOP_CONNECT_URL)
        .map_err(|error| AppError::Internal(format!("OSIRAPI 授权地址无效：{error}")))?;
    authorization_url.query_pairs_mut()
        .append_pair("state", &state)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256");
    open_external_browser(authorization_url.as_str())?;
    let callback = wait_for_oauth_callback(listener, &state)?;
    if let Some(error) = callback.error.filter(|value| !value.is_empty()) {
        return Err(AppError::Engine(format!("OSIRAPI 授权未完成：{error}")));
    }
    if callback.code.is_empty() || callback.state != state {
        return Err(AppError::Engine("OSIRAPI 授权回调无效，请重新连接".to_string()));
    }
    let redemption = new_redemption_state()?;
    let response = exchange_osir_oauth(
        &callback.code,
        &callback.state,
        &redirect_uri,
        &code_verifier,
        &redemption,
    )?;
    let payload = decrypt_bundle(&redemption, response.encrypted_bundle)?;
    if ocx_program().is_none() {
        install()?;
    } else if status()?.service_state != "ready" {
        start()?;
    }
    apply_codex_install_payload(payload)
}

pub fn install() -> Result<OpenCodexStatus, AppError> {
    if ocx_program().is_some() {
        return start();
    }
    if install_component_from_manifest().is_ok() && ocx_program().is_some() {
        return start();
    }
    let runtime = managed_runtime_dir()?;
    fs::create_dir_all(&runtime)
        .map_err(|error| AppError::Internal(format!("创建 OpenCodex 组件目录失败：{error}")))?;
    let prefix = runtime.to_string_lossy().to_string();
    let package = format!("@bitkyc08/opencodex@{DEFAULT_VERSION}");
    let output = Command::new(if cfg!(target_os = "windows") { "npm.cmd" } else { "npm" })
        .args(["install", "--prefix", &prefix, "--no-save", &package])
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
    status()
}

fn ocx_output(args: &[&str]) -> Result<Vec<u8>, AppError> {
    let (program, prefix) = ocx_invocation().ok_or_else(|| {
        AppError::Engine("未检测到 OpenCodex；请先安装多模型组件".to_string())
    })?;
    let output = Command::new(program)
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
    models: &[JsonValue],
    locked_route: Option<&str>,
    route_health: &BTreeMap<String, String>,
) -> Option<OpenCodexRoute> {
    let provider = config.get("providers")?.as_object()?.get(id)?.as_object()?;
    let models = models
        .iter()
        .filter(|model| model.get("provider").and_then(JsonValue::as_str) == Some(id))
        .filter_map(|model| model.get("modelId").and_then(JsonValue::as_str).map(str::to_string))
        .collect::<Vec<_>>();
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
        api_key_configured: provider
            .get("apiKey")
            .and_then(JsonValue::as_str)
            .is_some_and(|key| !key.trim().is_empty()),
        availability: if provider.get("disabled").and_then(JsonValue::as_bool) == Some(true) {
            "offline".to_string()
        } else if let Some(health) = route_health.get(id) {
            health.clone()
        } else if provider.get("apiKey").and_then(JsonValue::as_str).is_some_and(|key| !key.trim().is_empty()) {
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
    let provider_ids = models
        .iter()
        .filter_map(|model| model.get("provider").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    provider_ids
        .into_iter()
        .filter(|id| {
            config
                .get("providers")
                .and_then(JsonValue::as_object)
                .and_then(|providers| providers.get(*id))
                .and_then(JsonValue::as_object)
                .is_some_and(|provider| {
                    provider.get("baseUrl").and_then(JsonValue::as_str).is_some()
                        && provider.get("defaultModel").and_then(JsonValue::as_str).is_some()
                })
        })
        .map(str::to_string)
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
    let uninitialized = state.port == 0
        && state.codex_provider_id.is_empty()
        && state.managed_provider_ids.is_empty()
        && state.locked_route.is_none();
    if !uninitialized || inferred_ids.is_empty() || !codex_uses_opencodex(paths) {
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
    write_json(
        &paths.state,
        &serde_json::to_value(&adopted)
            .map_err(|error| AppError::Internal(format!("保存已有 OpenCodex 接管状态失败：{error}")))?,
    )?;
    Ok(adopted)
}

fn status_at(paths: &IntegrationPaths) -> Result<OpenCodexStatus, AppError> {
    let state = effective_state(paths)?;
    let config = load_config(&paths.opencodex_config).unwrap_or_default();
    let installed = ocx_program().is_some();
    let (service_state, error) = service_state(installed);
    let models = config
        .get("customModels")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let routes = state
        .managed_provider_ids
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
    })
}

pub fn status() -> Result<OpenCodexStatus, AppError> {
    status_at(&integration_paths()?)
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
    document["model_provider"] = value(provider_id);
    document["model"] = value(default_route);
    document["model_catalog_json"] = value(catalog.display().to_string());
    if !document.contains_key("model_providers") {
        document["model_providers"] = toml_edit::table();
    }
    let providers = document["model_providers"]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("model_providers 必须是 TOML 表".to_string()))?;
    if !providers.contains_key(provider_id) {
        providers.insert(provider_id, Item::Table(Table::new()));
    }
    let provider = providers
        .get_mut(provider_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| AppError::Engine("OpenCodex Provider 必须是 TOML 表".to_string()))?;
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
    fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<JsonValue>(&raw).ok())
        .and_then(|value| value.get("models").and_then(JsonValue::as_array).cloned())
        .is_some_and(|models| !models.is_empty())
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

pub fn save(input: OpenCodexConfigInput) -> Result<OpenCodexStatus, AppError> {
    if !input.enabled {
        return Err(AppError::Engine("停用多模型请使用恢复按钮，避免丢失当前配置".to_string()));
    }
    if ocx_program().is_none() {
        return Err(AppError::Engine("未检测到 OpenCodex；请先安装多模型组件".to_string()));
    }
    let paths = integration_paths()?;
    let (provider_id, default_route, routes) = validate_input(&input)?;
    let prior = load_state(&paths.state);
    let config = load_config(&paths.opencodex_config)?;
    let candidate = build_opencodex_config(config, &routes, &prior.managed_provider_ids, input.port)?;
    validate_candidate(&paths.opencodex_config, &candidate)?;
    write_json(&paths.opencodex_config, &candidate)?;
    write_codex_proxy_config(&paths.codex_config, &paths.catalog, &provider_id, input.port, &default_route)?;
    let state = ManagedState {
        enabled: false,
        port: input.port,
        codex_provider_id: provider_id.clone(),
        managed_provider_ids: routes.iter().map(|route| route.id.clone()).collect(),
        locked_route: None,
        route_health: BTreeMap::new(),
    };
    let state_json = serde_json::to_value(state)
        .map_err(|error| AppError::Internal(format!("序列化多模型状态失败：{error}")))?;
    write_json(&paths.state, &state_json)?;
    if let Err(error) = ocx_output(&["sync"]) {
        let _ = restore();
        return Err(error);
    }
    if !catalog_has_models(&paths.catalog) {
        let _ = restore();
        return Err(AppError::Engine("OpenCodex 同步完成但没有生成可用模型目录；已恢复原配置".to_string()));
    }
    let enabled_state = ManagedState {
        enabled: true,
        port: input.port,
        codex_provider_id: provider_id,
        managed_provider_ids: routes.iter().map(|route| route.id.clone()).collect(),
        locked_route: None,
        route_health: BTreeMap::new(),
    };
    write_json(
        &paths.state,
        &serde_json::to_value(enabled_state)
            .map_err(|error| AppError::Internal(format!("保存多模型启用状态失败：{error}")))?,
    )?;
    status_at(&paths)
}

pub fn sync() -> Result<OpenCodexStatus, AppError> {
    ocx_output(&["sync"])?;
    status()
}

pub fn restore() -> Result<OpenCodexStatus, AppError> {
    let paths = integration_paths()?;
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
    if restored.is_empty() {
        return Err(AppError::Engine("没有可恢复的 OpenCodex 配置备份".to_string()));
    }
    let state = JsonValue::Object(JsonMap::new());
    write_json(&paths.state, &state)?;
    status_at(&paths)
}

#[cfg(test)]
mod tests {
    use super::{
        build_opencodex_config, decrypt_bundle, extract_osir_ticket, inferred_default_route,
        inferred_managed_provider_ids, pkce_challenge, validate_input, wait_for_oauth_callback,
        CodexInstallPayload,
        EncryptedBundle, OpenCodexConfigInput, OpenCodexRouteInput, RedemptionState,
    };
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::Engine;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::rand_core::OsRng;
    use rsa::sha2::Sha256 as RsaSha256;
    use rsa::{Oaep, RsaPrivateKey};
    use serde_json::{json, Map as JsonMap};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

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
        assert!(!providers.contains_key("old"));
        let models = next["customModels"].as_array().unwrap();
        assert!(models.iter().any(|model| model["provider"] == "keep"));
        assert!(models.iter().any(|model| model["provider"] == "osir-gpt"));
        assert!(!models.iter().any(|model| model["provider"] == "old"));
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
            }]
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
