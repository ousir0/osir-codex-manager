//! Keeps Codex's thread index usable when Manager changes model providers.
//!
//! Index changes alone are not durable: Codex replays session_meta on resume.
//! While Codex is closed, repair that metadata and its cached byte offsets too.
//! Message/tool lines remain byte-for-byte unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};

use crate::app::paths;
use crate::errors::AppError;

#[path = "codex_session_metadata.rs"]
mod metadata;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRoute {
    pub id: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTarget<'a> {
    Default {
        provider: &'a str,
        opencodex_provider: &'a str,
    },
    OpenCodex {
        provider: &'a str,
        default_provider: &'a str,
        default_route: &'a str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadUpdate {
    id: String,
    provider: String,
    model: Option<String>,
}

fn state_database_path() -> Result<PathBuf, AppError> {
    paths::codex_home_dir()
        .map(|home| {
            let configured = fs::read_to_string(home.join("config.toml")).ok()
                .and_then(|raw| raw.parse::<toml_edit::DocumentMut>().ok())
                .and_then(|doc| doc.get("sqlite_home").and_then(toml_edit::Item::as_str).map(PathBuf::from));
            configured.filter(|path| path.is_absolute()).unwrap_or(home).join("state_5.sqlite")
        })
        .ok_or_else(|| AppError::Internal("无法定位 Codex 会话数据库".to_string()))
}

fn session_backup_path() -> Result<PathBuf, AppError> {
    paths::data_dir()
        .map(|root| root.join("opencodex").join("codex-session-index.before-switch.sqlite"))
        .ok_or_else(|| AppError::Internal("无法定位 Codex Manager 数据目录".to_string()))
}

/// Records only providers explicitly switched through Manager. This gives
/// offline repair a bounded ownership list instead of claiming every provider.
pub fn remember_provider_switch(config: &Path, previous: &str, next: &str) -> Result<(), AppError> {
    if previous == next { return Ok(()); }
    let path = config.with_extension("manager-session-providers.json");
    let mut providers = provider_switch_sources(config)?;
    if providers.iter().any(|provider| provider == previous) { return Ok(()); }
    providers.push(previous.to_string());
    crate::app::atomic_file::write_atomic(&path, &serde_json::to_vec(&providers).map_err(|error| AppError::Internal(error.to_string()))?)
        .map_err(|error| AppError::Internal(format!("保存会话供应商迁移记录失败：{error}")))
}

pub fn provider_switch_sources(config: &Path) -> Result<Vec<String>, AppError> {
    let path = config.with_extension("manager-session-providers.json");
    if !path.exists() { return Ok(Vec::new()); }
    if path.is_symlink() { return Err(AppError::Engine("会话供应商迁移记录是符号链接".into())); }
    serde_json::from_slice(&fs::read(path).map_err(|error| AppError::Internal(error.to_string()))?)
        .map_err(|error| AppError::Engine(format!("会话供应商迁移记录无效：{error}")))
}

/// Session files are shared by Desktop and CLI. Installation detection alone
/// can miss a different bundle or a running standalone app-server.
pub(crate) fn runtime_is_running() -> bool {
    if crate::app::codex_theme::codex_running() { return true; }
    #[cfg(unix)]
    {
        // pgrep's distinct exit codes let an unavailable process check fail
        // closed rather than interpreting an error as permission to rewrite.
        std::process::Command::new("/usr/bin/pgrep").args(["-x", "codex"])
            .output().map(|output| output.status.code() != Some(1)).unwrap_or(true)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("tasklist.exe")
            .args(["/FI", "IMAGENAME eq codex.exe", "/FO", "CSV", "/NH"])
            .creation_flags(0x08000000).output()
            .map(|output| !output.status.success() || String::from_utf8_lossy(&output.stdout).to_ascii_lowercase().contains("codex.exe"))
            .unwrap_or(true)
    }
    #[cfg(not(any(unix, windows)))]
    { true }
}

fn known_models(routes: &[SessionRoute]) -> (BTreeMap<String, String>, BTreeMap<String, Vec<String>>) {
    let mut full_to_bare = BTreeMap::new();
    let mut bare_to_full = BTreeMap::<String, Vec<String>>::new();
    for route in routes {
        for model in &route.models {
            let full = format!("{}/{}", route.id, model);
            full_to_bare.insert(full.clone(), model.clone());
            bare_to_full.entry(model.clone()).or_default().push(full);
        }
    }
    (full_to_bare, bare_to_full)
}

fn preferred_route<'a>(candidates: &'a [String], default_route: &str) -> Option<&'a str> {
    let default_provider = default_route.split('/').next().unwrap_or_default();
    candidates
        .iter()
        .find(|candidate| candidate.starts_with(&format!("{default_provider}/")))
        .or_else(|| candidates.first())
        .map(String::as_str)
}

fn planned_update(
    id: String,
    provider: String,
    model: Option<String>,
    target: SessionTarget<'_>,
    full_to_bare: &BTreeMap<String, String>,
    bare_to_full: &BTreeMap<String, Vec<String>>,
) -> Option<ThreadUpdate> {
    let current_model = model.as_deref().unwrap_or_default();
    match target {
        SessionTarget::Default { provider: target_provider, opencodex_provider } => {
            let routed_model = full_to_bare.get(current_model);
            if provider != target_provider && provider != opencodex_provider && provider != "openai" { return None; }
            if provider != opencodex_provider && provider != "openai" && routed_model.is_none() {
                return None;
            }
            let next_model = routed_model.cloned().or_else(|| {
                if provider == opencodex_provider {
                    current_model.rsplit_once('/').map(|(_, bare)| bare.to_string()).or_else(|| model.clone())
                } else {
                    model.clone()
                }
            });
            if provider == target_provider && next_model == model {
                return None;
            }
            Some(ThreadUpdate { id, provider: target_provider.to_string(), model: next_model })
        }
        SessionTarget::OpenCodex { provider: target_provider, default_provider, default_route } => {
            if provider != target_provider && provider != default_provider && provider != "openai" { return None; }
            if full_to_bare.contains_key(current_model) {
                if provider == target_provider {
                    return None;
                }
                return Some(ThreadUpdate { id, provider: target_provider.to_string(), model });
            }
            // Older OpenCodex integrations stored bare OpenAI model ids in
            // threads and kept a hidden catalog alias to make them resolve.
            // The picker catalog no longer carries those duplicate aliases,
            // so migrate an already-routed bare thread to its canonical
            // provider/model selector at the save boundary.
            if provider == target_provider {
                let route = bare_to_full
                    .get(current_model)
                    .and_then(|routes| preferred_route(routes, default_route))
                    .unwrap_or(default_route);
                return Some(ThreadUpdate {
                    id,
                    provider: target_provider.to_string(),
                    model: Some(route.to_string()),
                });
            }
            if provider != default_provider && provider != "openai" {
                return None;
            }
            let route = if current_model.is_empty() {
                default_route.to_string()
            } else {
                bare_to_full
                    .get(current_model)
                    .and_then(|routes| preferred_route(routes, default_route))
                    .unwrap_or(default_route)
                    .to_string()
            };
            Some(ThreadUpdate {
                id,
                provider: target_provider.to_string(),
                model: Some(route),
            })
        }
    }
}

fn validate_threads_schema(connection: &Connection) -> Result<bool, AppError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| AppError::Internal(format!("读取 Codex 会话表结构失败：{error}")))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| AppError::Internal(format!("读取 Codex 会话表结构失败：{error}")))?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| AppError::Internal(format!("解析 Codex 会话表结构失败：{error}")))?;
    if columns.is_empty() {
        return Ok(false);
    }
    for required in ["id", "model_provider", "model"] {
        if !columns.contains(required) {
            return Err(AppError::Engine(format!(
                "当前 Codex 会话索引缺少 {required} 字段，已取消配置切换以保护历史记录"
            )));
        }
    }
    Ok(true)
}

fn create_consistent_backup(connection: &Connection, backup: &Path) -> Result<(), AppError> {
    let parent = backup
        .parent()
        .ok_or_else(|| AppError::Internal("会话备份路径无父目录".to_string()))?;
    fs::create_dir_all(parent)
        .map_err(|error| AppError::Internal(format!("创建会话备份目录失败：{error}")))?;
    if backup.exists() {
        fs::remove_file(backup)
            .map_err(|error| AppError::Internal(format!("更新旧会话备份失败：{error}")))?;
    }
    connection
        .execute("VACUUM main INTO ?1", params![backup.to_string_lossy().as_ref()])
        .map_err(|error| AppError::Internal(format!("备份 Codex 会话索引失败：{error}")))?;
    Ok(())
}

fn migrate_at(
    database: &Path,
    backup: &Path,
    target: SessionTarget<'_>,
    routes: &[SessionRoute],
) -> Result<usize, AppError> {
    if !database.is_file() {
        return Ok(0);
    }
    if database.is_symlink() {
        return Err(AppError::Engine("Codex 会话数据库是符号链接，已取消切换".to_string()));
    }
    let mut connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| AppError::Internal(format!("打开 Codex 会话数据库失败：{error}")))?;
    connection
        .busy_timeout(Duration::from_secs(8))
        .map_err(|error| AppError::Internal(format!("设置会话数据库等待时间失败：{error}")))?;
    if !validate_threads_schema(&connection)? {
        return Ok(0);
    }

    let (full_to_bare, bare_to_full) = known_models(routes);
    let updates = {
        let mut statement = connection
            .prepare("SELECT id, model_provider, model FROM threads")
            .map_err(|error| AppError::Internal(format!("读取 Codex 会话索引失败：{error}")))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|error| AppError::Internal(format!("读取 Codex 会话索引失败：{error}")))?;
        let mut updates = Vec::new();
        for row in rows {
            let (id, provider, model) = row
                .map_err(|error| AppError::Internal(format!("解析 Codex 会话索引失败：{error}")))?;
            if let Some(update) = planned_update(id, provider, model, target, &full_to_bare, &bare_to_full) {
                updates.push(update);
            }
        }
        updates
    };
    if updates.is_empty() {
        return Ok(0);
    }

    create_consistent_backup(&connection, backup)?;
    let transaction = connection
        .transaction()
        .map_err(|error| AppError::Internal(format!("开始会话迁移事务失败：{error}")))?;
    for update in &updates {
        transaction
            .execute(
                "UPDATE threads SET model_provider = ?1, model = ?2 WHERE id = ?3",
                params![update.provider, update.model, update.id],
            )
            .map_err(|error| AppError::Internal(format!("更新 Codex 会话索引失败：{error}")))?;
    }
    transaction
        .commit()
        .map_err(|error| AppError::Internal(format!("提交 Codex 会话迁移失败：{error}")))?;
    Ok(updates.len())
}

pub fn migrate(target: SessionTarget<'_>, routes: &[SessionRoute]) -> Result<usize, AppError> {
    migrate_at(&state_database_path()?, &session_backup_path()?, target, routes)
}

/// Call only after Codex has stopped; a running writer retains its file handle.
pub fn repair_resume_metadata(target: SessionTarget<'_>, routes: &[SessionRoute]) -> Result<usize, AppError> {
    if runtime_is_running() {
        return Err(AppError::Engine("Codex 已启动，旧会话修复将在关闭后继续".into()));
    }
    let database = state_database_path()?;
    let root = session_backup_path()?.with_file_name("session-resume-repairs");
    metadata::repair_with_guard(&database, &root, target, routes, || !runtime_is_running())
}

#[cfg(test)]
mod tests {
    use super::{migrate_at, SessionRoute, SessionTarget};
    use rusqlite::Connection;
    use uuid::Uuid;

    fn setup() -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("codex-session-migration-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT);",
        ).unwrap();
        (database, root.join("backup.sqlite"))
    }

    fn routes() -> Vec<SessionRoute> {
        vec![
            SessionRoute { id: "osirapi-openai".into(), models: vec!["gpt-5.6-sol".into()] },
            SessionRoute { id: "osirapi-claude".into(), models: vec!["claude-opus-5".into()] },
        ]
    }

    #[test]
    fn default_provider_switch_repairs_native_threads_and_preserves_bare_models() {
        let (database, backup) = setup();
        let connection = Connection::open(&database).unwrap();
        connection.execute("INSERT INTO threads VALUES ('a','openai','gpt-5.4')", []).unwrap();
        connection.execute("INSERT INTO threads VALUES ('b','opencodex','gpt-5.4')", []).unwrap();
        assert_eq!(migrate_at(&database, &backup, SessionTarget::Default { provider: "osir", opencodex_provider: "opencodex" }, &routes()).unwrap(), 2);
        let count:i64=connection.query_row("SELECT count(*) FROM threads WHERE model_provider='osir' AND model='gpt-5.4'", [], |row| row.get(0)).unwrap();
        assert_eq!(count,2);
        drop(connection);std::fs::remove_dir_all(database.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_ownership_records_only_explicit_switches_once() {
        let (database, _) = setup();
        let config=database.with_file_name("config.toml");
        super::remember_provider_switch(&config,"custom-a","custom-b").unwrap();
        super::remember_provider_switch(&config,"custom-a","custom-b").unwrap();
        super::remember_provider_switch(&config,"custom-b","custom-b").unwrap();
        assert_eq!(super::provider_switch_sources(&config).unwrap(),vec!["custom-a"]);
        std::fs::remove_dir_all(database.parent().unwrap()).unwrap();
    }

    #[test]
    fn migrates_opencodex_and_legacy_routed_threads_to_default() {
        let (database, backup) = setup();
        let connection = Connection::open(&database).unwrap();
        connection.execute("INSERT INTO threads VALUES ('a','opencodex','osirapi-openai/gpt-5.6-sol')", []).unwrap();
        connection.execute("INSERT INTO threads VALUES ('b','openai','osirapi-claude/claude-opus-5')", []).unwrap();
        connection.execute("INSERT INTO threads VALUES ('c','other','unrelated/model')", []).unwrap();
        drop(connection);

        assert_eq!(migrate_at(&database, &backup, SessionTarget::Default { provider: "osir", opencodex_provider: "opencodex" }, &routes()).unwrap(), 2);
        let connection = Connection::open(&database).unwrap();
        let values = connection.prepare("SELECT id, model_provider, model FROM threads ORDER BY id").unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(values, vec![
            ("a".into(), "osir".into(), "gpt-5.6-sol".into()),
            ("b".into(), "osir".into(), "claude-opus-5".into()),
            ("c".into(), "other".into(), "unrelated/model".into()),
        ]);
        assert!(backup.is_file());
        drop(connection);
        std::fs::remove_dir_all(database.parent().unwrap()).unwrap();
    }

    #[test]
    fn maps_default_threads_back_to_saved_opencodex_routes() {
        let (database, backup) = setup();
        let connection = Connection::open(&database).unwrap();
        connection.execute("INSERT INTO threads VALUES ('a','osir','gpt-5.6-sol')", []).unwrap();
        connection.execute("INSERT INTO threads VALUES ('b','osir','unknown')", []).unwrap();
        connection.execute("INSERT INTO threads VALUES ('c','other','osirapi-claude/claude-opus-5')", []).unwrap();
        drop(connection);

        assert_eq!(migrate_at(&database, &backup, SessionTarget::OpenCodex { provider: "opencodex", default_provider: "osir", default_route: "osirapi-openai/gpt-5.6-sol" }, &routes()).unwrap(), 2);
        let connection = Connection::open(&database).unwrap();
        let values = connection.prepare("SELECT id, model_provider, model FROM threads ORDER BY id").unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(values, vec![
            ("a".into(), "opencodex".into(), "osirapi-openai/gpt-5.6-sol".into()),
            ("b".into(), "opencodex".into(), "osirapi-openai/gpt-5.6-sol".into()),
            ("c".into(), "other".into(), "osirapi-claude/claude-opus-5".into()),
        ]);
        drop(connection);
        std::fs::remove_dir_all(database.parent().unwrap()).unwrap();
    }

    #[test]
    fn upgrades_bare_opencodex_threads_without_publishing_picker_aliases() {
        let (database, backup) = setup();
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "INSERT INTO threads VALUES ('a','opencodex','gpt-5.6-sol')",
                [],
            )
            .unwrap();
        drop(connection);

        assert_eq!(
            migrate_at(
                &database,
                &backup,
                SessionTarget::OpenCodex {
                    provider: "opencodex",
                    default_provider: "osir",
                    default_route: "osirapi-openai/gpt-5.6-sol"
                },
                &routes()
            )
            .unwrap(),
            1
        );
        let connection = Connection::open(&database).unwrap();
        let value = connection
            .query_row(
                "SELECT model_provider, model FROM threads WHERE id = 'a'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(value, ("opencodex".into(), "osirapi-openai/gpt-5.6-sol".into()));
        assert!(backup.is_file());
        drop(connection);
        std::fs::remove_dir_all(database.parent().unwrap()).unwrap();
    }
}
