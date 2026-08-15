//! Safe, narrowly-scoped management for `~/.codex/config.toml` and auth.json.
//!
//! Structured edits use `toml_edit`, preserving comments and unknown keys.
//! Whole-file edits are validated before the same atomic writer creates a
//! single-step `.bak`, then verified by an exact read-back and a second parse.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::app::{atomic_file, paths};
use crate::errors::AppError;

const MAX_CONFIG_BYTES: usize = 2 * 1024 * 1024;
const MAX_AUTH_BYTES: usize = 2 * 1024 * 1024;
const MAX_API_KEY_LEN: usize = 16 * 1024;
const MAX_ID_LEN: usize = 96;
const MAX_VALUE_LEN: usize = 8192;
const MAX_MODELS_BYTES: usize = 2 * 1024 * 1024;
const MAX_MODEL_COUNT: usize = 2_000;
const MAX_MODEL_ID_LEN: usize = 256;
const REASONING_EFFORTS: [&str; 5] = ["minimal", "low", "medium", "high", "xhigh"];
const PERSONALITIES: [&str; 3] = ["none", "friendly", "pragmatic"];
const APPROVAL_POLICIES: [&str; 3] = ["untrusted", "on-request", "never"];
const SANDBOX_MODES: [&str; 3] = ["read-only", "workspace-write", "danger-full-access"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConfigReport {
    pub path: String,
    pub auth_path: String,
    pub exists: bool,
    pub raw: String,
    pub redacted_raw: String,
    pub parse_error: Option<String>,
    pub model: String,
    pub provider: String,
    pub base_url: String,
    pub reasoning_effort: String,
    pub personality: String,
    pub approval_policy: String,
    pub sandbox_mode: String,
    pub disable_response_storage: bool,
    pub goal_mode: bool,
    pub providers: Vec<CodexProviderProfile>,
    pub mcp_servers: Vec<CodexMcpServer>,
    pub backup_available: bool,
    pub api_key_configured: bool,
    pub auth_error: Option<String>,
    pub codex_running: bool,
    pub image_generation_compatibility: bool,
    pub image_generation_api_key_configured: bool,
    pub image_generation_model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub wire_api: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMcpServer {
    pub name: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub has_sensitive_values: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConfigValidation {
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexBasicConfigInput {
    pub model: String,
    pub provider: String,
    pub base_url: String,
    pub reasoning_effort: String,
    pub personality: String,
    pub approval_policy: String,
    pub sandbox_mode: String,
    pub disable_response_storage: bool,
    pub goal_mode: bool,
    pub image_generation_compatibility: bool,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMcpServerInput {
    pub original_name: Option<String>,
    pub name: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
}

fn config_path() -> Result<PathBuf, AppError> {
    paths::codex_home_dir()
        .map(|dir| dir.join("config.toml"))
        .ok_or_else(|| AppError::Internal("无法定位 ~/.codex/config.toml".to_string()))
}

fn auth_path_for_config(config_path: &Path) -> PathBuf {
    config_path.with_file_name("auth.json")
}

fn load_auth_object(path: &Path) -> Result<JsonMap<String, JsonValue>, AppError> {
    if !path.exists() {
        return Ok(JsonMap::new());
    }
    let raw = fs::read(path)
        .map_err(|error| AppError::Internal(format!("读取 auth.json 失败：{error}")))?;
    if raw.len() > MAX_AUTH_BYTES {
        return Err(AppError::Engine(format!(
            "auth.json 超过 {} MiB，拒绝处理",
            MAX_AUTH_BYTES / 1024 / 1024
        )));
    }
    let value: JsonValue = serde_json::from_slice(&raw)
        .map_err(|error| AppError::Engine(format!("auth.json 格式错误：{error}")))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::Engine("auth.json 顶层必须是 JSON 对象".to_string()))
}

fn auth_status(path: &Path) -> (bool, Option<String>) {
    match load_auth_object(path) {
        Ok(auth) => match auth.get("OPENAI_API_KEY") {
            None | Some(JsonValue::Null) => (false, None),
            Some(JsonValue::String(value)) => (!value.trim().is_empty(), None),
            Some(_) => (
                false,
                Some("auth.json 中的 OPENAI_API_KEY 必须是字符串".to_string()),
            ),
        },
        Err(error) => (false, Some(error.to_string())),
    }
}

fn parse_document(raw: &str) -> Result<DocumentMut, AppError> {
    if raw.len() > MAX_CONFIG_BYTES {
        return Err(AppError::Engine(format!(
            "config.toml 超过 {} MiB，拒绝处理",
            MAX_CONFIG_BYTES / 1024 / 1024
        )));
    }
    if raw.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    raw.parse::<DocumentMut>()
        .map_err(|error| AppError::Engine(format!("TOML 格式错误：{error}")))
}

fn string_at(table: &dyn toml_edit::TableLike, key: &str) -> String {
    table
        .get(key)
        .and_then(Item::as_str)
        .unwrap_or_default()
        .to_string()
}

fn sensitive_table(table: &Table) -> bool {
    table.iter().any(|(key, item)| {
        let normalized = key.to_ascii_lowercase();
        normalized == "env"
            || normalized == "http_headers"
            || normalized == "headers"
            || sensitive_key(&normalized)
            || item.as_table().is_some_and(sensitive_table)
    })
}

fn sensitive_key(key: &str) -> bool {
    let key = key.trim_matches(['\'', '"']).to_ascii_lowercase();
    if key.ends_with("_env_var") {
        return false;
    }
    key.contains("api_key")
        || key.contains("password")
        || key.contains("secret")
        || key == "authorization"
        || key == "token"
        || key.ends_with("_token")
        || key == "experimental_bearer_token"
}

fn redact_value(item: &mut toml_edit::Value, sensitive: bool) {
    if sensitive {
        *item = toml_edit::Value::from("********");
        return;
    }
    match item {
        toml_edit::Value::Array(array) => {
            for value in array.iter_mut() {
                redact_value(value, false);
            }
        }
        toml_edit::Value::InlineTable(table) => {
            for (key, value) in table.iter_mut() {
                let child_sensitive = matches!(
                    key.to_ascii_lowercase().as_str(),
                    "env" | "headers" | "http_headers"
                ) || sensitive_key(&key);
                redact_value(value, child_sensitive);
            }
        }
        _ => {}
    }
}

fn redact_item(item: &mut Item, sensitive: bool) {
    match item {
        Item::None => {}
        Item::Value(value) => redact_value(value, sensitive),
        Item::Table(table) => {
            for (key, child) in table.iter_mut() {
                let child_sensitive = sensitive
                    || matches!(
                        key.to_ascii_lowercase().as_str(),
                        "env" | "headers" | "http_headers"
                    )
                    || sensitive_key(&key);
                redact_item(child, child_sensitive);
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (key, child) in table.iter_mut() {
                    redact_item(child, sensitive || sensitive_key(&key));
                }
            }
        }
    }
}

fn redact_invalid_raw(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    for line in raw.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let ending = &line[body.len()..];
        let trimmed = body.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
            output.push_str(body);
            output.push_str(ending);
            continue;
        }
        if let Some(eq) = body.find('=') {
            output.push_str(&body[..=eq]);
            output.push_str(" \"********\"");
        } else {
            output.push_str(&body[..body.len() - trimmed.len()]);
            output.push_str("# hidden while TOML is invalid");
        }
        output.push_str(ending);
    }
    output
}

fn redact_raw(raw: &str) -> String {
    let Ok(mut document) = parse_document(raw) else {
        return redact_invalid_raw(raw);
    };
    for (key, item) in document.as_table_mut().iter_mut() {
        redact_item(item, sensitive_key(&key));
    }
    document.to_string()
}

fn mcp_servers(document: &DocumentMut) -> Vec<CodexMcpServer> {
    let Some(servers) = document.get("mcp_servers").and_then(Item::as_table) else {
        return Vec::new();
    };
    let mut result = servers
        .iter()
        .filter_map(|(name, item)| {
            let table = item.as_table()?;
            let command = table
                .get("command")
                .and_then(Item::as_str)
                .map(str::to_string);
            let url = table.get("url").and_then(Item::as_str).map(str::to_string);
            let declared = table.get("type").and_then(Item::as_str).unwrap_or_default();
            let transport = if url.is_some() || matches!(declared, "http" | "sse") {
                "http"
            } else {
                "stdio"
            };
            let args = table
                .get("args")
                .and_then(Item::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(|entry| entry.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Some(CodexMcpServer {
                name: name.to_string(),
                enabled: table.get("enabled").and_then(Item::as_bool).unwrap_or(true),
                transport: transport.to_string(),
                command,
                args,
                url,
                has_sensitive_values: sensitive_table(table),
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|model| model.name.to_lowercase());
    result
}

fn provider_profiles(document: &DocumentMut) -> Vec<CodexProviderProfile> {
    let Some(providers) = document.get("model_providers").and_then(Item::as_table) else {
        return Vec::new();
    };
    let mut result = providers
        .iter()
        .filter_map(|(id, item)| {
            let table = item.as_table()?;
            Some(CodexProviderProfile {
                id: id.to_string(),
                name: {
                    let name = string_at(table, "name");
                    if name.is_empty() {
                        id.to_string()
                    } else {
                        name
                    }
                },
                base_url: string_at(table, "base_url"),
                wire_api: string_at(table, "wire_api"),
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|provider| provider.id.to_lowercase());
    result
}

fn image_generation_compatibility(document: &DocumentMut) -> bool {
    let native_enabled = document
        .get("features")
        .and_then(Item::as_table)
        .and_then(|features| features.get("image_generation"))
        .and_then(Item::as_bool);
    if let Some(native_enabled) = native_enabled {
        return !native_enabled;
    }

    // Read the marker emitted by older manager versions so upgrading does not
    // silently switch a user's selected image path back to the native tool.
    let provider = string_at(document.as_table(), "model_provider");
    let Some(table) = document
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(&provider))
        .and_then(Item::as_table)
    else {
        return false;
    };
    let requires_auth = table
        .get("requires_openai_auth")
        .and_then(Item::as_bool)
        .unwrap_or(true);
    let actor = table
        .get("http_headers")
        .and_then(Item::as_value)
        .and_then(|value| value.as_inline_table())
        .and_then(|headers| headers.get("x-openai-actor-authorization"))
        .and_then(|value| value.as_str());
    !requires_auth && actor.is_some()
}

fn image_generation_key_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("imagegen-relay.json")
}

fn separate_image_generation_api_key_configured(config_path: &Path) -> bool {
    let path = image_generation_key_path(config_path);
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<JsonMap<String, JsonValue>>(&raw).ok())
        .and_then(|map| {
            map.get("api_key")
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|key| !key.trim().is_empty())
}

fn separate_image_generation_model(config_path: &Path) -> String {
    fs::read_to_string(image_generation_key_path(config_path))
        .ok()
        .and_then(|raw| serde_json::from_str::<JsonMap<String, JsonValue>>(&raw).ok())
        .and_then(|map| map.get("model").and_then(JsonValue::as_str).map(str::to_owned))
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| "gpt-image-2".to_string())
}

fn report_for_path(path: &Path, codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let auth_path = auth_path_for_config(path);
    let (api_key_configured, auth_error) = auth_status(&auth_path);
    let exists = path.is_file();
    let raw = if exists {
        fs::read_to_string(path)
            .map_err(|error| AppError::Internal(format!("读取 config.toml 失败：{error}")))?
    } else {
        String::new()
    };
    let redacted_raw = redact_raw(&raw);
    let parsed = parse_document(&raw);
    let (
        parse_error,
        model,
        provider,
        base_url,
        reasoning_effort,
        personality,
        approval_policy,
        sandbox_mode,
        disable_response_storage,
        goal_mode,
        providers,
        servers,
        image_compatibility,
        image_api_key_configured,
        image_model,
    ) = match parsed {
        Ok(document) => {
            let provider = string_at(document.as_table(), "model_provider");
            let base_url = document
                .get("model_providers")
                .and_then(Item::as_table)
                .and_then(|providers| providers.get(&provider))
                .and_then(Item::as_table)
                .map(|table| string_at(table, "base_url"))
                .unwrap_or_default();
            let image_compatibility = image_generation_compatibility(&document);
            let image_api_key_configured = separate_image_generation_api_key_configured(path);
            (
                None,
                string_at(document.as_table(), "model"),
                provider,
                base_url,
                string_at(document.as_table(), "model_reasoning_effort"),
                string_at(document.as_table(), "personality"),
                string_at(document.as_table(), "approval_policy"),
                string_at(document.as_table(), "sandbox_mode"),
                document
                    .get("disable_response_storage")
                    .and_then(Item::as_bool)
                    .unwrap_or(false),
                document
                    .get("features")
                    .and_then(Item::as_table)
                    .and_then(|features| features.get("goals"))
                    .and_then(Item::as_bool)
                    .unwrap_or(false),
                provider_profiles(&document),
                mcp_servers(&document),
                image_compatibility,
                image_api_key_configured,
                separate_image_generation_model(path),
            )
        }
        Err(error) => (
            Some(error.to_string()),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            false,
            false,
            Vec::new(),
            Vec::new(),
            false,
            false,
            "gpt-image-2".to_string(),
        ),
    };
    Ok(CodexConfigReport {
        path: path.display().to_string(),
        auth_path: auth_path.display().to_string(),
        exists,
        raw,
        redacted_raw,
        parse_error,
        model,
        provider,
        base_url,
        reasoning_effort,
        personality,
        approval_policy,
        sandbox_mode,
        disable_response_storage,
        goal_mode,
        providers,
        mcp_servers: servers,
        backup_available: atomic_file::backup_path(path).is_file(),
        api_key_configured,
        auth_error,
        codex_running,
        image_generation_compatibility: image_compatibility,
        image_generation_api_key_configured: image_api_key_configured,
        image_generation_model: image_model,
    })
}

pub fn report(codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    report_for_path(&path, codex_running)
}

pub fn validate(raw: &str) -> CodexConfigValidation {
    match parse_document(raw) {
        Ok(_) => CodexConfigValidation {
            valid: true,
            error: None,
        },
        Err(error) => CodexConfigValidation {
            valid: false,
            error: Some(error.to_string()),
        },
    }
}

#[cfg(unix)]
fn tighten_auth_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    let backup = atomic_file::backup_path(path);
    for candidate in [path, backup.as_path()] {
        if candidate.is_file() {
            fs::set_permissions(candidate, fs::Permissions::from_mode(0o600)).map_err(|error| {
                AppError::Internal(format!("收紧 auth.json 文件权限失败：{error}"))
            })?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_auth_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

fn write_auth_verified(path: &Path, auth: &JsonMap<String, JsonValue>) -> Result<(), AppError> {
    if path.is_symlink() {
        return Err(AppError::Engine(
            "auth.json 是符号链接，为避免改写错误目标，管理器拒绝保存".to_string(),
        ));
    }
    let mut rendered = serde_json::to_vec_pretty(&JsonValue::Object(auth.clone()))
        .map_err(|error| AppError::Internal(format!("序列化 auth.json 失败：{error}")))?;
    rendered.push(b'\n');
    atomic_file::write_atomic(path, &rendered)
        .map_err(|error| AppError::Internal(format!("原子保存 auth.json 失败：{error}")))?;
    tighten_auth_permissions(path)?;

    let written = fs::read(path)
        .map_err(|error| AppError::Internal(format!("回读 auth.json 失败：{error}")))?;
    if written != rendered {
        return Err(AppError::Internal(
            "auth.json 保存后回读内容不一致".to_string(),
        ));
    }
    let verified: JsonValue = serde_json::from_slice(&written)
        .map_err(|error| AppError::Internal(format!("回读 auth.json 校验失败：{error}")))?;
    if verified != JsonValue::Object(auth.clone()) {
        return Err(AppError::Internal(
            "auth.json 保存后 JSON 校验不一致".to_string(),
        ));
    }
    Ok(())
}

fn set_api_key_at(path: &Path, api_key: &str) -> Result<(), AppError> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(AppError::Engine("API Key 不能为空".to_string()));
    }
    if api_key.len() > MAX_API_KEY_LEN || api_key.chars().any(char::is_control) {
        return Err(AppError::Engine("API Key 格式无效或长度超限".to_string()));
    }
    if path.is_symlink() {
        return Err(AppError::Engine(
            "auth.json 是符号链接，为避免改写错误目标，管理器拒绝保存".to_string(),
        ));
    }
    let mut auth = load_auth_object(path)?;
    if auth.get("OPENAI_API_KEY").and_then(JsonValue::as_str) == Some(api_key) {
        tighten_auth_permissions(path)?;
        return Ok(());
    }
    auth.insert(
        "OPENAI_API_KEY".to_string(),
        JsonValue::String(api_key.to_string()),
    );
    write_auth_verified(path, &auth)
}

fn delete_api_key_at(path: &Path) -> Result<(), AppError> {
    if path.is_symlink() {
        return Err(AppError::Engine(
            "auth.json 是符号链接，为避免改写错误目标，管理器拒绝保存".to_string(),
        ));
    }
    let mut auth = load_auth_object(path)?;
    if auth.remove("OPENAI_API_KEY").is_none() {
        if path.is_file() {
            tighten_auth_permissions(path)?;
        }
        return Ok(());
    }
    write_auth_verified(path, &auth)
}

pub fn set_api_key(api_key: &str, codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let config_path = config_path()?;
    set_api_key_at(&auth_path_for_config(&config_path), api_key)?;
    report_for_path(&config_path, codex_running)
}

pub fn delete_api_key(codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let config_path = config_path()?;
    delete_api_key_at(&auth_path_for_config(&config_path))?;
    report_for_path(&config_path, codex_running)
}

pub fn set_image_generation_api_key(
    api_key: &str,
    model: &str,
    codex_running: bool,
) -> Result<CodexConfigReport, AppError> {
    let api_key = checked_value(api_key, "生图 API Key")?;
    if api_key.is_empty() || api_key.chars().any(char::is_control) {
        return Err(AppError::Engine("生图 API Key 不能为空".to_string()));
    }
    let model = checked_value(model, "生图模型")?;
    let model = if model.is_empty() { "gpt-image-2" } else { model.as_str() };
    let path = config_path()?;
    let base_url = report_for_path(&path, codex_running)?.base_url;
    if base_url.trim().is_empty() {
        return Err(AppError::Engine("请先配置生图 API Base URL".to_string()));
    }
    let key_path = image_generation_key_path(&path);
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::Internal(format!("创建 Codex 配置目录失败：{error}")))?;
    }
    let payload = serde_json::json!({ "base_url": base_url, "api_key": api_key, "model": model });
    write_verified_json(&key_path, &payload.to_string())?;
    install_image_generation_skill(&path)?;
    report_for_path(&path, codex_running)
}

pub fn delete_image_generation_api_key(codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    let key_path = image_generation_key_path(&path);
    if key_path.exists() {
        fs::remove_file(&key_path)
            .map_err(|error| AppError::Internal(format!("删除独立生图 API Key 失败：{error}")))?;
    }
    report_for_path(&path, codex_running)
}

fn models_endpoint(base_url: &str) -> Result<url::Url, AppError> {
    let base_url = if base_url.trim().is_empty() {
        "https://api.openai.com/v1"
    } else {
        base_url.trim()
    };
    let mut endpoint = url::Url::parse(base_url)
        .map_err(|_| AppError::Engine("Base URL 无效，无法获取模型".to_string()))?;
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(AppError::Engine(
            "Base URL 不能包含用户名或密码".to_string(),
        ));
    }
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(AppError::Engine(
            "获取模型仅支持 http 或 https Base URL".to_string(),
        ));
    }
    let is_loopback = endpoint.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if endpoint.scheme() == "http" && !is_loopback {
        return Err(AppError::Engine(
            "非本机 Base URL 必须使用 HTTPS，避免 API Key 明文传输".to_string(),
        ));
    }
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    let path = endpoint.path().trim_end_matches('/');
    endpoint.set_path(&format!("{path}/models"));
    Ok(endpoint)
}

fn parse_models_response(raw: &[u8]) -> Result<Vec<String>, AppError> {
    if raw.len() > MAX_MODELS_BYTES {
        return Err(AppError::Engine("模型列表响应超过 2 MiB".to_string()));
    }
    let response: ModelsResponse = serde_json::from_slice(raw)
        .map_err(|_| AppError::Engine("模型接口未返回标准的 data 数组".to_string()))?;
    let mut models = response
        .data
        .into_iter()
        .map(|entry| entry.id.trim().to_string())
        .filter(|id| {
            !id.is_empty() && id.len() <= MAX_MODEL_ID_LEN && !id.chars().any(char::is_control)
        })
        .collect::<Vec<_>>();
    models.sort_unstable();
    models.dedup();
    models.truncate(MAX_MODEL_COUNT);
    if models.is_empty() {
        return Err(AppError::Engine("模型接口没有返回可用模型".to_string()));
    }
    Ok(models)
}

fn curl_command() -> std::process::Command {
    let command = std::process::Command::new(if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "/usr/bin/curl"
    });
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

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

pub fn fetch_models(base_url: &str) -> Result<Vec<String>, AppError> {
    let config_path = config_path()?;
    let auth = load_auth_object(&auth_path_for_config(&config_path))?;
    let api_key = auth
        .get("OPENAI_API_KEY")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| AppError::Engine("请先保存 API Key，再获取模型".to_string()))?;
    if api_key.len() > MAX_API_KEY_LEN || api_key.chars().any(char::is_control) {
        return Err(AppError::Engine(
            "auth.json 中的 API Key 格式无效".to_string(),
        ));
    }
    let endpoint = models_endpoint(base_url)?;
    let mut command = curl_command();
    let mut child = command
        .args([
            "-sS",
            "--fail",
            "--proto",
            "=http,https",
            "--max-time",
            "15",
            "--max-filesize",
            "2097152",
            "--header",
            "@-",
            endpoint.as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AppError::Engine(format!("curl 不可用：{error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Internal("无法向模型请求写入凭据".to_string()))?;
    stdin
        .write_all(
            format!("Authorization: Bearer {api_key}\nAccept: application/json\n").as_bytes(),
        )
        .map_err(|error| AppError::Internal(format!("写入模型请求凭据失败：{error}")))?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|error| AppError::Engine(format!("获取模型失败：{error}")))?;
    if !output.status.success() {
        return Err(AppError::Engine(format!(
            "模型接口请求失败（curl 状态 {}），请检查 Base URL、API Key 和网络",
            output.status
        )));
    }
    parse_models_response(&output.stdout)
}

/// Atomically write a validated config. Running Codex is allowed; the UI
/// surfaces that the new values take effect after the next restart.
fn write_verified(path: &Path, raw: &str) -> Result<(), AppError> {
    parse_document(raw)?;
    if path.is_symlink() {
        return Err(AppError::Engine(
            "config.toml 是符号链接，为避免改写错误目标，管理器拒绝保存".to_string(),
        ));
    }
    if path.is_file() {
        let current = fs::read(path)
            .map_err(|error| AppError::Internal(format!("读取 config.toml 失败：{error}")))?;
        if current == raw.as_bytes() {
            return Ok(());
        }
    }
    atomic_file::write_atomic(path, raw.as_bytes())
        .map_err(|error| AppError::Internal(format!("原子保存 config.toml 失败：{error}")))?;
    let written = fs::read_to_string(path)
        .map_err(|error| AppError::Internal(format!("回读 config.toml 失败：{error}")))?;
    if written != raw {
        return Err(AppError::Internal(
            "config.toml 保存后回读内容不一致".to_string(),
        ));
    }
    parse_document(&written)?;
    Ok(())
}

fn write_verified_json(path: &Path, raw: &str) -> Result<(), AppError> {
    serde_json::from_str::<JsonValue>(raw)
        .map_err(|error| AppError::Internal(format!("独立生图配置无效：{error}")))?;
    atomic_file::write_atomic(path, raw.as_bytes())
        .map_err(|error| AppError::Internal(format!("保存独立生图配置失败：{error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            AppError::Internal(format!("收紧独立生图配置文件权限失败：{error}"))
        })?;
    }
    Ok(())
}

fn sync_image_generation_base_url(config_path: &Path, base_url: &str) -> Result<(), AppError> {
    let key_path = image_generation_key_path(config_path);
    let Ok(raw) = fs::read_to_string(&key_path) else {
        return Ok(());
    };
    let Ok(mut payload) = serde_json::from_str::<JsonMap<String, JsonValue>>(&raw) else {
        return Ok(());
    };
    if payload
        .get("api_key")
        .and_then(JsonValue::as_str)
        .map_or(true, |key| key.trim().is_empty())
    {
        return Ok(());
    }
    payload.insert(
        "base_url".to_string(),
        JsonValue::String(base_url.to_string()),
    );
    let rendered = serde_json::to_string(&payload)
        .map_err(|error| AppError::Internal(format!("序列化独立生图配置失败：{error}")))?;
    write_verified_json(&key_path, &rendered)
}

fn install_image_generation_skill(config_path: &Path) -> Result<(), AppError> {
    let codex_home = config_path
        .parent()
        .ok_or_else(|| AppError::Internal("无法定位 Codex 配置目录".to_string()))?;
    let skill_dir = codex_home.join("skills").join("imagegen-relay");
    let scripts_dir = skill_dir.join("scripts");
    fs::create_dir_all(&scripts_dir)
        .map_err(|error| AppError::Internal(format!("创建生图技能目录失败：{error}")))?;
    let skill = include_str!("../../resources/skills/imagegen-relay/SKILL.md");
    let script = include_str!("../../resources/skills/imagegen-relay/scripts/imagegen_relay.py");
    atomic_file::write_atomic(&skill_dir.join("SKILL.md"), skill.as_bytes())
        .map_err(|error| AppError::Internal(format!("安装生图技能说明失败：{error}")))?;
    atomic_file::write_atomic(&scripts_dir.join("imagegen_relay.py"), script.as_bytes())
        .map_err(|error| AppError::Internal(format!("安装生图技能脚本失败：{error}")))?;
    install_ecommerce_skills(codex_home)?;
    Ok(())
}

fn install_ecommerce_skills(codex_home: &Path) -> Result<(), AppError> {
    let skills = [
        ("ecom-single-image", include_str!("../../resources/skills/ecom-single-image/SKILL.md")),
        ("ecom-five-hero-images", include_str!("../../resources/skills/ecom-five-hero-images/SKILL.md")),
        ("ecom-detail-set", include_str!("../../resources/skills/ecom-detail-set/SKILL.md")),
    ];
    for (name, body) in skills {
        let dir = codex_home.join("skills").join(name);
        fs::create_dir_all(&dir)
            .map_err(|error| AppError::Internal(format!("创建电商技能目录失败：{error}")))?;
        atomic_file::write_atomic(&dir.join("SKILL.md"), body.as_bytes())
            .map_err(|error| AppError::Internal(format!("安装电商技能 {name} 失败：{error}")))?;
    }
    let single_scripts = codex_home.join("skills").join("ecom-single-image").join("scripts");
    fs::create_dir_all(&single_scripts)
        .map_err(|error| AppError::Internal(format!("创建电商生成脚本目录失败：{error}")))?;
    let generator = include_str!("../../resources/skills/ecom-single-image/scripts/generate_image.py");
    atomic_file::write_atomic(&single_scripts.join("generate_image.py"), generator.as_bytes())
        .map_err(|error| AppError::Internal(format!("安装电商生成脚本失败：{error}")))?;

    let template_dir = codex_home
        .join("skills")
        .join("ecom-detail-set")
        .join("references")
        .join("templates");
    fs::create_dir_all(&template_dir)
        .map_err(|error| AppError::Internal(format!("创建电商模板目录失败：{error}")))?;
    let templates = [
        ("01-hero-image.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/01-hero-image.json")),
        ("02-lifestyle-scene.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/02-lifestyle-scene.json")),
        ("03-flat-lay.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/03-flat-lay.json")),
        ("04-detail-macro.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/04-detail-macro.json")),
        ("05-poster-banner.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/05-poster-banner.json")),
        ("06-social-media.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/06-social-media.json")),
        ("07-ugc-style.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/07-ugc-style.json")),
        ("08-model-showcase.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/08-model-showcase.json")),
        ("09-before-after.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/09-before-after.json")),
        ("10-packaging.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/10-packaging.json")),
        ("11-infographic.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/11-infographic.json")),
        ("12-creative-concept.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/12-creative-concept.json")),
        ("13-size-spec.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/13-size-spec.json")),
        ("14-multi-product.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/14-multi-product.json")),
        ("15-livestream.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/15-livestream.json")),
        ("16-try-on-virtual.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/16-try-on-virtual.json")),
        ("17-exploded-view.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/17-exploded-view.json")),
        ("18-ghost-mannequin.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/18-ghost-mannequin.json")),
        ("19-multi-angle-grid.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/19-multi-angle-grid.json")),
        ("20-magazine-editorial.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/20-magazine-editorial.json")),
        ("21-seasonal-campaign.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/21-seasonal-campaign.json")),
        ("22-luxury-atmospherics.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/22-luxury-atmospherics.json")),
        ("23-device-mockup.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/23-device-mockup.json")),
        ("24-storefront.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/24-storefront.json")),
        ("25-sports-campaign.json", include_str!("../../resources/skills/ecom-detail-set/references/templates/25-sports-campaign.json")),
    ];
    for (name, body) in templates {
        atomic_file::write_atomic(&template_dir.join(name), body.as_bytes())
            .map_err(|error| AppError::Internal(format!("安装电商模板 {name} 失败：{error}")))?;
    }
    Ok(())
}

pub fn save_raw(raw: &str, codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    write_verified(&path, raw)?;
    report_for_path(&path, codex_running)
}

fn checked_id(value: &str, label: &str, allow_empty: bool) -> Result<String, AppError> {
    let trimmed = value.trim();
    if allow_empty && trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.is_empty()
        || trimmed.len() > MAX_ID_LEN
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(AppError::Engine(format!(
            "{label} 只能包含字母、数字、短横线和下划线，且不超过 {MAX_ID_LEN} 个字符"
        )));
    }
    Ok(trimmed.to_string())
}

fn checked_value(value: &str, label: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.len() > MAX_VALUE_LEN {
        return Err(AppError::Engine(format!(
            "{label} 不能超过 {MAX_VALUE_LEN} 个字符"
        )));
    }
    Ok(trimmed.to_string())
}

fn set_or_remove_string(document: &mut DocumentMut, key: &str, raw: &str) {
    if raw.is_empty() {
        document.remove(key);
    } else {
        document[key] = value(raw);
    }
}

fn load_document(path: &Path) -> Result<DocumentMut, AppError> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let raw = fs::read_to_string(path)
        .map_err(|error| AppError::Internal(format!("读取 config.toml 失败：{error}")))?;
    parse_document(&raw)
}

fn apply_basic(document: &mut DocumentMut, input: CodexBasicConfigInput) -> Result<(), AppError> {
    let model = checked_value(&input.model, "模型")?;
    let provider = checked_id(&input.provider, "供应商标识", true)?;
    let base_url = checked_value(&input.base_url, "Base URL")?;
    let reasoning = input.reasoning_effort.trim().to_ascii_lowercase();
    if !reasoning.is_empty() && !REASONING_EFFORTS.contains(&reasoning.as_str()) {
        return Err(AppError::Engine("不支持的推理等级".to_string()));
    }
    let personality = input.personality.trim().to_ascii_lowercase();
    if !personality.is_empty() && !PERSONALITIES.contains(&personality.as_str()) {
        return Err(AppError::Engine("不支持的 Personality".to_string()));
    }
    let approval_policy = input.approval_policy.trim().to_ascii_lowercase();
    if !approval_policy.is_empty() && !APPROVAL_POLICIES.contains(&approval_policy.as_str()) {
        return Err(AppError::Engine("不支持的审批策略".to_string()));
    }
    let sandbox_mode = input.sandbox_mode.trim().to_ascii_lowercase();
    if !sandbox_mode.is_empty() && !SANDBOX_MODES.contains(&sandbox_mode.as_str()) {
        return Err(AppError::Engine("不支持的沙箱模式".to_string()));
    }
    if !base_url.is_empty() {
        if provider.is_empty() {
            return Err(AppError::Engine(
                "填写 Base URL 时必须同时填写供应商标识".to_string(),
            ));
        }
        let parsed = url::Url::parse(&base_url)
            .map_err(|error| AppError::Engine(format!("Base URL 无效：{error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AppError::Engine(
                "Base URL 仅支持 http 或 https".to_string(),
            ));
        }
    }
    set_or_remove_string(document, "model", &model);
    set_or_remove_string(document, "model_provider", &provider);
    set_or_remove_string(document, "model_reasoning_effort", &reasoning);
    set_or_remove_string(document, "personality", &personality);
    set_or_remove_string(document, "approval_policy", &approval_policy);
    set_or_remove_string(document, "sandbox_mode", &sandbox_mode);
    document["disable_response_storage"] = value(input.disable_response_storage);
    if !document.contains_key("features") {
        document["features"] = toml_edit::table();
    }
    let features = document["features"]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("features 必须是 TOML 表".to_string()))?;
    features["goals"] = value(input.goal_mode);
    // Relay mode routes image requests through the separately installed skill;
    // disable the native image extension so the two paths cannot compete.
    features["image_generation"] = value(!input.image_generation_compatibility);

    let provider_table_exists = document
        .get("model_providers")
        .and_then(Item::as_table)
        .is_some_and(|providers| providers.contains_key(&provider));
    if !provider.is_empty() && (!base_url.is_empty() || provider_table_exists) {
        if !document.contains_key("model_providers") {
            document["model_providers"] = toml_edit::table();
        }
        let providers = document["model_providers"]
            .as_table_mut()
            .ok_or_else(|| AppError::Engine("model_providers 必须是 TOML 表".to_string()))?;
        if !providers.contains_key(&provider) {
            let mut created = Table::new();
            created["name"] = value(if provider == "awai" {
                "AWAI"
            } else {
                &provider
            });
            created["wire_api"] = value("responses");
            created["requires_openai_auth"] = value(true);
            providers.insert(&provider, Item::Table(created));
        }
        let selected = providers
            .get_mut(&provider)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| {
                AppError::Engine(format!("model_providers.{provider} 必须是 TOML 表"))
            })?;
        if base_url.is_empty() {
            selected.remove("base_url");
        } else {
            selected["base_url"] = value(&base_url);
        }
        // The relay skill authenticates independently. Keep the main provider
        // on the normal chat auth path and remove legacy relay-only markers.
        selected["requires_openai_auth"] = value(true);
        if let Some(headers) = selected
            .get_mut("http_headers")
            .and_then(Item::as_value_mut)
            .and_then(toml_edit::Value::as_inline_table_mut)
        {
            headers.remove("x-openai-actor-authorization");
        }
    }
    Ok(())
}

pub fn save_basic(
    input: CodexBasicConfigInput,
    codex_running: bool,
) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    let mut document = load_document(&path)?;
    apply_basic(&mut document, input)?;
    write_verified(&path, &document.to_string())?;
    let provider = string_at(document.as_table(), "model_provider");
    let base_url = document
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(&provider))
        .and_then(Item::as_table)
        .map(|table| string_at(table, "base_url"))
        .unwrap_or_default();
    sync_image_generation_base_url(&path, &base_url)?;
    report_for_path(&path, codex_running)
}

fn apply_mcp(document: &mut DocumentMut, input: CodexMcpServerInput) -> Result<(), AppError> {
    let name = checked_id(&input.name, "MCP 名称", false)?;
    let original = input
        .original_name
        .as_deref()
        .map(|value| checked_id(value, "原 MCP 名称", false))
        .transpose()?;
    let transport = input.transport.trim().to_ascii_lowercase();
    if !matches!(transport.as_str(), "stdio" | "http") {
        return Err(AppError::Engine(
            "MCP 传输类型必须是 stdio 或 http".to_string(),
        ));
    }
    if !document.contains_key("mcp_servers") {
        document["mcp_servers"] = toml_edit::table();
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| AppError::Engine("mcp_servers 必须是 TOML 表".to_string()))?;
    if original.as_deref() != Some(name.as_str()) && servers.contains_key(&name) {
        return Err(AppError::Engine(format!("MCP 名称已存在：{name}")));
    }
    let mut server = original
        .as_deref()
        .and_then(|old| servers.remove(old))
        .or_else(|| servers.remove(&name))
        .and_then(|item| item.into_table().ok())
        .unwrap_or_default();
    server["enabled"] = value(input.enabled);
    server["type"] = value(&transport);
    if transport == "stdio" {
        let command = checked_value(input.command.as_deref().unwrap_or_default(), "MCP 命令")?;
        if command.is_empty() {
            return Err(AppError::Engine("stdio MCP 必须填写命令".to_string()));
        }
        if input.args.len() > 128 {
            return Err(AppError::Engine("MCP 参数不能超过 128 项".to_string()));
        }
        server["command"] = value(command);
        server.remove("url");
        if input.args.is_empty() {
            server.remove("args");
        } else {
            let mut args = Array::new();
            for argument in input.args {
                args.push(checked_value(&argument, "MCP 参数")?);
            }
            server["args"] = Item::Value(toml_edit::Value::Array(args));
        }
    } else {
        let endpoint = checked_value(input.url.as_deref().unwrap_or_default(), "MCP URL")?;
        let parsed = url::Url::parse(&endpoint)
            .map_err(|error| AppError::Engine(format!("MCP URL 无效：{error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AppError::Engine("MCP URL 仅支持 http 或 https".to_string()));
        }
        server["url"] = value(endpoint);
        server.remove("command");
        server.remove("args");
    }
    servers.insert(&name, Item::Table(server));
    Ok(())
}

pub fn upsert_mcp(
    input: CodexMcpServerInput,
    codex_running: bool,
) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    let mut document = load_document(&path)?;
    apply_mcp(&mut document, input)?;
    write_verified(&path, &document.to_string())?;
    report_for_path(&path, codex_running)
}

pub fn delete_mcp(name: &str, codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    let name = checked_id(name, "MCP 名称", false)?;
    let mut document = load_document(&path)?;
    let removed = document
        .get_mut("mcp_servers")
        .and_then(Item::as_table_mut)
        .and_then(|servers| servers.remove(&name));
    if removed.is_none() {
        return Err(AppError::Engine(format!("找不到 MCP：{name}")));
    }
    write_verified(&path, &document.to_string())?;
    report_for_path(&path, codex_running)
}

pub fn restore_backup(codex_running: bool) -> Result<CodexConfigReport, AppError> {
    let path = config_path()?;
    let backup = atomic_file::backup_path(&path);
    let raw = fs::read_to_string(&backup)
        .map_err(|error| AppError::Internal(format!("读取 config.toml 备份失败：{error}")))?;
    parse_document(&raw)?;
    write_verified(&path, &raw)?;
    report_for_path(&path, codex_running)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn test_path(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("codex-config-{name}-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("config.toml")
    }

    #[test]
    fn report_extracts_active_provider_and_masks_secrets() {
        let path = test_path("report");
        fs::write(
            &path,
            r#"model = "gpt-5"
model_provider = "awai"
model_reasoning_effort = "high"
disable_response_storage = true
personality = "pragmatic"
approval_policy = "never"
sandbox_mode = "danger-full-access"

[features]
goals = true

[model_providers.awai]
base_url = "https://api.awai.cc/v1"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example/v1"
wire_api = "chat"

[mcp_servers.demo]
type = "stdio"
command = "npx"
args = ["-y", "demo"]

[mcp_servers.demo.env]
API_KEY = """secret-value
second-secret"""
"#,
        )
        .unwrap();
        let report = report_for_path(&path, false).unwrap();
        assert_eq!(report.model, "gpt-5");
        assert_eq!(report.provider, "awai");
        assert_eq!(report.base_url, "https://api.awai.cc/v1");
        assert_eq!(report.providers.len(), 2);
        assert_eq!(report.providers[0].id, "awai");
        assert_eq!(report.providers[1].id, "backup");
        assert_eq!(report.personality, "pragmatic");
        assert_eq!(report.approval_policy, "never");
        assert_eq!(report.sandbox_mode, "danger-full-access");
        assert!(report.disable_response_storage);
        assert!(report.goal_mode);
        assert_eq!(report.mcp_servers.len(), 1);
        assert!(report.mcp_servers[0].has_sensitive_values);
        assert!(report.raw.contains("secret-value"));
        assert!(!report.redacted_raw.contains("secret-value"));
        assert!(!report.redacted_raw.contains("second-secret"));
        assert!(report.redacted_raw.contains("********"));
    }

    #[test]
    fn atomic_save_validates_and_keeps_previous_version() {
        let path = test_path("atomic");
        fs::write(&path, "model = \"old\"\n").unwrap();
        write_verified(&path, "model = \"new\"\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "model = \"new\"\n");
        assert_eq!(
            fs::read_to_string(atomic_file::backup_path(&path)).unwrap(),
            "model = \"old\"\n"
        );
        let error = write_verified(&path, "model = [\n").unwrap_err();
        assert!(error.to_string().contains("TOML"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "model = \"new\"\n");
    }

    #[test]
    fn running_config_writes_are_atomic_and_verified() {
        let path = test_path("running");
        fs::write(&path, "model = \"old\"\n").unwrap();
        write_verified(&path, "model = \"new\"\n").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "model = \"new\"\n");
    }

    #[test]
    fn invalid_config_still_loads_for_raw_repair() {
        let path = test_path("invalid");
        fs::write(&path, "model = [\n").unwrap();
        let report = report_for_path(&path, false).unwrap();
        assert!(report.parse_error.is_some());
        assert_eq!(report.raw, "model = [\n");
    }

    #[test]
    fn basic_edit_preserves_comments_and_unknown_fields() {
        let mut document = r#"# keep this comment
unknown_flag = true
model = "old"
model_provider = "awai"

[model_providers.awai]
name = "Original name"
base_url = "https://old.example/v1"
custom_capability = "keep"
"#
        .parse::<DocumentMut>()
        .unwrap();
        apply_basic(
            &mut document,
            CodexBasicConfigInput {
                model: "gpt-5.4".to_string(),
                provider: "awai".to_string(),
                base_url: "https://api.awai.cc/v1".to_string(),
                reasoning_effort: "xhigh".to_string(),
                personality: "pragmatic".to_string(),
                approval_policy: "never".to_string(),
                sandbox_mode: "danger-full-access".to_string(),
                disable_response_storage: true,
                goal_mode: true,
                image_generation_compatibility: false,
            },
        )
        .unwrap();
        let rendered = document.to_string();
        assert!(rendered.contains("# keep this comment"));
        assert!(rendered.contains("unknown_flag = true"));
        assert!(rendered.contains("custom_capability = \"keep\""));
        assert!(rendered.contains("model = \"gpt-5.4\""));
        assert!(rendered.contains("base_url = \"https://api.awai.cc/v1\""));
        assert!(rendered.contains("personality = \"pragmatic\""));
        assert!(rendered.contains("approval_policy = \"never\""));
        assert!(rendered.contains("sandbox_mode = \"danger-full-access\""));
        assert!(rendered.contains("disable_response_storage = true"));
        assert!(rendered.contains("goals = true"));
    }

    #[test]
    fn relay_mode_disables_only_native_image_generation() {
        let mut document = r#"model_provider = "awai"

[model_providers.awai]
base_url = "https://api.awai.cc/v1"
requires_openai_auth = true
"#
        .parse::<DocumentMut>()
        .unwrap();
        let input = CodexBasicConfigInput {
            model: "gpt-5.6-sol".to_string(),
            provider: "awai".to_string(),
            base_url: "https://api.awai.cc/v1".to_string(),
            reasoning_effort: String::new(),
            personality: String::new(),
            approval_policy: String::new(),
            sandbox_mode: String::new(),
            disable_response_storage: false,
            goal_mode: false,
            image_generation_compatibility: true,
        };
        apply_basic(&mut document, input.clone()).unwrap();
        assert!(image_generation_compatibility(&document));
        assert_eq!(
            document["features"]["image_generation"].as_bool(),
            Some(false)
        );
        let provider = document["model_providers"]["awai"].as_table().unwrap();
        assert_eq!(provider["requires_openai_auth"].as_bool(), Some(true));
        assert!(provider.get("http_headers").is_none());

        apply_basic(
            &mut document,
            CodexBasicConfigInput {
                image_generation_compatibility: false,
                ..input
            },
        )
        .unwrap();
        assert!(!image_generation_compatibility(&document));
        assert_eq!(
            document["features"]["image_generation"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn image_key_sync_updates_base_url_and_protects_permissions() {
        let config_path = test_path("image-key-sync");
        let key_path = image_generation_key_path(&config_path);
        fs::write(
            &key_path,
            r#"{"base_url":"https://old.example/v1","api_key":"secret"}"#,
        )
        .unwrap();

        sync_image_generation_base_url(&config_path, "https://new.example/v1").unwrap();
        let payload: JsonValue = serde_json::from_slice(&fs::read(&key_path).unwrap()).unwrap();
        assert_eq!(payload["base_url"], "https://new.example/v1");
        assert_eq!(payload["api_key"], "secret");
        assert_eq!(
            separate_image_generation_api_key_configured(&config_path),
            true
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&key_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn model_endpoint_requires_https_except_for_loopback() {
        assert_eq!(
            models_endpoint("https://api.awai.cc/v1").unwrap().as_str(),
            "https://api.awai.cc/v1/models"
        );
        assert!(models_endpoint("http://api.awai.cc/v1").is_err());
        assert_eq!(
            models_endpoint("http://127.0.0.1:11434/v1")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:11434/v1/models"
        );
    }

    #[test]
    fn model_response_is_filtered_sorted_and_deduplicated() {
        let models = parse_models_response(
            br#"{"data":[{"id":"gpt-5.6-terra"},{"id":" gpt-5.6-sol "},{"id":"gpt-5.6-sol"},{"id":""}]}"#,
        )
        .unwrap();
        assert_eq!(models, vec!["gpt-5.6-sol", "gpt-5.6-terra"]);
        assert!(parse_models_response(br#"{"object":"list"}"#).is_err());
    }

    #[test]
    fn mcp_edit_preserves_sensitive_and_extension_fields() {
        let mut document = r#"[mcp_servers.demo]
type = "stdio"
command = "old-command"
args = ["old"]
enabled = false
startup_timeout_sec = 25

[mcp_servers.demo.env]
API_KEY = "keep-secret"
"#
        .parse::<DocumentMut>()
        .unwrap();
        apply_mcp(
            &mut document,
            CodexMcpServerInput {
                original_name: Some("demo".to_string()),
                name: "demo".to_string(),
                enabled: true,
                transport: "stdio".to_string(),
                command: Some("npx".to_string()),
                args: vec!["-y".to_string(), "package".to_string()],
                url: None,
            },
        )
        .unwrap();
        let rendered = document.to_string();
        assert!(rendered.contains("command = \"npx\""));
        assert!(rendered.contains("args = [\"-y\", \"package\"]"));
        assert!(rendered.contains("startup_timeout_sec = 25"));
        assert!(rendered.contains("API_KEY = \"keep-secret\""));
        assert!(rendered.contains("enabled = true"));
    }

    #[test]
    fn api_key_write_preserves_auth_fields_and_never_enters_report() {
        let config_path = test_path("auth-preserve");
        let auth_path = auth_path_for_config(&config_path);
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "chatgpt",
  "tokens": { "access_token": "keep-token" },
  "OPENAI_API_KEY": "old-secret"
}"#,
        )
        .unwrap();

        set_api_key_at(&auth_path, "sk-new-secret").unwrap();
        let written: JsonValue = serde_json::from_slice(&fs::read(&auth_path).unwrap()).unwrap();
        assert_eq!(written["auth_mode"], "chatgpt");
        assert_eq!(written["tokens"]["access_token"], "keep-token");
        assert_eq!(written["OPENAI_API_KEY"], "sk-new-secret");

        let backup: JsonValue =
            serde_json::from_slice(&fs::read(atomic_file::backup_path(&auth_path)).unwrap())
                .unwrap();
        assert_eq!(backup["OPENAI_API_KEY"], "old-secret");

        let report = report_for_path(&config_path, false).unwrap();
        assert!(report.api_key_configured);
        assert_eq!(report.auth_path, auth_path.display().to_string());
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("sk-new-secret"));
        assert!(!serialized.contains("old-secret"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&auth_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(atomic_file::backup_path(&auth_path))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn api_key_delete_removes_only_the_key() {
        let config_path = test_path("auth-delete");
        let auth_path = auth_path_for_config(&config_path);
        fs::write(
            &auth_path,
            r#"{"OPENAI_API_KEY":"secret","tokens":{"refresh_token":"keep"}}"#,
        )
        .unwrap();

        delete_api_key_at(&auth_path).unwrap();
        let written: JsonValue = serde_json::from_slice(&fs::read(&auth_path).unwrap()).unwrap();
        assert!(written.get("OPENAI_API_KEY").is_none());
        assert_eq!(written["tokens"]["refresh_token"], "keep");
        assert!(
            !report_for_path(&config_path, false)
                .unwrap()
                .api_key_configured
        );
    }

    #[test]
    fn invalid_auth_is_reported_and_never_overwritten() {
        let config_path = test_path("auth-invalid");
        let auth_path = auth_path_for_config(&config_path);
        let invalid = b"{ not-json\n";
        fs::write(&auth_path, invalid).unwrap();

        let report = report_for_path(&config_path, false).unwrap();
        assert!(!report.api_key_configured);
        assert!(report.auth_error.is_some());
        assert!(set_api_key_at(&auth_path, "sk-replacement").is_err());
        assert_eq!(fs::read(&auth_path).unwrap(), invalid);
    }

    #[test]
    fn running_api_key_writes_are_allowed() {
        let config_path = test_path("auth-running");
        let auth_path = auth_path_for_config(&config_path);
        fs::write(&auth_path, r#"{"OPENAI_API_KEY":"old-secret"}"#).unwrap();

        set_api_key_at(&auth_path, "sk-new-secret").unwrap();
        assert_eq!(
            fs::read_to_string(&auth_path).unwrap(),
            "{\n  \"OPENAI_API_KEY\": \"sk-new-secret\"\n}\n"
        );
    }
}
