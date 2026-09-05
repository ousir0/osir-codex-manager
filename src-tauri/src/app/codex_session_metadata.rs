//! Offline, journaled repair of resume metadata; never edits conversation lines.
use super::*;
use crate::app::atomic_file;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Serialize, Deserialize)]
struct OffsetChange {
    table: String,
    column: String,
    turn: Option<String>,
    before: i64,
    after: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Repair {
    id: String,
    rollout: PathBuf,
    backup: PathBuf,
    before_hash: String,
    after_hash: String,
    provider: String,
    model: Option<String>,
    previous_provider: String,
    previous_model: Option<String>,
    offsets: Vec<OffsetChange>,
}

fn fail(error: impl std::fmt::Display) -> AppError {
    AppError::Engine(format!(
        "修复旧会话认证路由失败（已保留备份，请勿删除修复目录）：{error}"
    ))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn replace_rollout(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(fail)?;
    let temporary = path.with_extension(format!("repair-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(fail)?;
        file.set_permissions(metadata.permissions()).map_err(fail)?;
        file.write_all(bytes).map_err(fail)?;
        if let Ok(modified) = metadata.modified() {
            file.set_times(fs::FileTimes::new().set_modified(modified))
                .map_err(fail)?;
        }
        file.sync_all().map_err(fail)?;
        drop(file);
        // The durable journal already owns the backup. A direct replacement
        // avoids a missing-path window and duplicate multi-gigabyte .bak files.
        fs::rename(&temporary, path).map_err(fail)?;
        #[cfg(unix)]
        fs::File::open(path.parent().ok_or_else(|| fail("会话目录不存在"))?)
            .map_err(fail)?
            .sync_all()
            .map_err(fail)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

type ByteShifts = Vec<(i64, i64)>;

#[derive(Deserialize)]
struct RolloutKind<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    payload: &'a serde_json::value::RawValue,
}

#[derive(Deserialize)]
struct EventKind<'a> {
    #[serde(borrow, rename = "type")]
    kind: Option<&'a str>,
}

fn transform(
    bytes: &[u8],
    id: &str,
    provider: &str,
    model: Option<&str>,
) -> Result<(Vec<u8>, ByteShifts), AppError> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut shifts = Vec::new();
    let mut end = 0_i64;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        end += line.len() as i64;
        // Validate every line, but avoid allocating message/tool trees. Real
        // histories can total several GiB; only route metadata needs a Value.
        let envelope: RolloutKind<'_> = serde_json::from_slice(line).map_err(fail)?;
        let settings = envelope.kind == "event_msg"
            && serde_json::from_str::<EventKind<'_>>(envelope.payload.get())
                .map_err(fail)?
                .kind
                == Some("thread_settings_applied");
        if envelope.kind != "session_meta" && !settings {
            output.extend_from_slice(line);
            continue;
        }
        let mut item: serde_json::Value = serde_json::from_slice(line).map_err(fail)?;
        let mut changed = false;
        if item["type"] == "session_meta"
            && item["payload"]["id"] == id
            && item["payload"]["model_provider"] != provider
        {
            item["payload"]["model_provider"] = provider.into();
            changed = true;
        }
        // Later settings snapshots also override the route on cold resume.
        if item["type"] == "event_msg"
            && item["payload"]["type"] == "thread_settings_applied"
            && item["payload"]["thread_id"] == id
        {
            let settings = item["payload"]["thread_settings"]
                .as_object_mut()
                .ok_or_else(|| fail("会话设置快照格式无效"))?;
            if settings
                .get("model_provider_id")
                .and_then(serde_json::Value::as_str)
                != Some(provider)
            {
                settings.insert("model_provider_id".into(), provider.into());
                changed = true;
            }
            if let Some(model) = model {
                if settings.get("model").and_then(serde_json::Value::as_str) != Some(model) {
                    settings.insert("model".into(), model.into());
                    changed = true;
                }
            }
        }
        if changed {
            serde_json::to_writer(&mut output, &item).map_err(fail)?;
            if line.ends_with(b"\n") {
                output.push(b'\n');
            }
            shifts.push((end, output.len() as i64 - end));
        } else {
            output.extend_from_slice(line);
        }
    }
    Ok((output, shifts))
}

fn translated(offset: i64, shifts: &ByteShifts) -> i64 {
    offset
        + shifts
            .iter()
            .rev()
            .find(|(end, _)| offset >= *end)
            .map(|(_, delta)| *delta)
            .unwrap_or(0)
}

fn open_database(path: &Path) -> Result<Connection, AppError> {
    if path.is_symlink() {
        return Err(fail("数据库为符号链接"));
    }
    let connection =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(fail)?;
    connection
        .busy_timeout(Duration::from_secs(8))
        .map_err(fail)?;
    Ok(connection)
}

const OFFSET_FIELDS: [(&str, &str); 3] = [
    ("thread_turns", "rollout_byte_offset"),
    ("thread_turns", "rollout_end_byte_offset"),
    (
        "thread_history_projection_state",
        "next_rollout_byte_offset",
    ),
];

fn read_offsets(
    history: &Path,
    id: &str,
    shifts: &ByteShifts,
) -> Result<Vec<OffsetChange>, AppError> {
    if !history.is_file() {
        return Ok(Vec::new());
    }
    let connection = open_database(history)?;
    let mut changes = Vec::new();
    for (table, column) in OFFSET_FIELDS {
        let columns = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(fail)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(fail)?
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(fail)?;
        if !columns.contains(column) {
            continue;
        }
        let key = if table == "thread_turns" {
            "turn_id"
        } else {
            "NULL"
        };
        let mut statement = connection
            .prepare(&format!(
                "SELECT {key}, {column} FROM {table} WHERE thread_id=?1 AND {column} IS NOT NULL"
            ))
            .map_err(fail)?;
        let rows = statement
            .query_map([id], |row| {
                Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(fail)?;
        for row in rows {
            let (turn, before) = row.map_err(fail)?;
            let after = translated(before, shifts);
            if before != after {
                changes.push(OffsetChange {
                    table: table.into(),
                    column: column.into(),
                    turn,
                    before,
                    after,
                });
            }
        }
    }
    Ok(changes)
}

fn finish(
    database: &Path,
    history: &Path,
    repair: &Repair,
    journal: &Path,
) -> Result<(), AppError> {
    if repair.rollout.is_symlink() || repair.backup.is_symlink() {
        return Err(fail("会话或备份为符号链接"));
    }
    // write_atomic may have moved the old path to .bak before a power loss.
    let current = match fs::read(&repair.rollout) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let bytes = fs::read(&repair.backup).map_err(fail)?;
            if hash(&bytes) != repair.before_hash {
                return Err(fail("会话备份校验失败"));
            }
            atomic_file::write_atomic(&repair.rollout, &bytes).map_err(fail)?;
            bytes
        }
        Err(error) => return Err(fail(error)),
    };
    let digest = hash(&current);
    if digest != repair.after_hash {
        if digest != repair.before_hash {
            return Err(fail("会话已被其他进程修改，暂停恢复以保护新增消息"));
        }
        let (updated, _) = transform(
            &current,
            &repair.id,
            &repair.provider,
            repair.model.as_deref(),
        )?;
        if hash(&updated) != repair.after_hash {
            return Err(fail("待恢复内容校验不一致"));
        }
        replace_rollout(&repair.rollout, &updated)?;
    }
    if !repair.offsets.is_empty() {
        let mut connection = open_database(history)?;
        let tx = connection.transaction().map_err(fail)?;
        for change in &repair.offsets {
            if !OFFSET_FIELDS.contains(&(change.table.as_str(), change.column.as_str())) {
                return Err(fail("修复日志包含未知缓存字段"));
            }
            let (table, column) = (&change.table, &change.column);
            let filter = if change.turn.is_some() {
                "thread_id=?1 AND turn_id=?2"
            } else {
                "thread_id=?1 AND ?2 IS NULL"
            };
            let current: i64 = tx
                .query_row(
                    &format!("SELECT {column} FROM {table} WHERE {filter}"),
                    params![repair.id, change.turn],
                    |row| row.get(0),
                )
                .map_err(fail)?;
            if current != change.before && current != change.after {
                return Err(fail("历史缓存已被其他进程更新，暂停恢复"));
            }
            tx.execute(
                &format!("UPDATE {table} SET {column}=?3 WHERE {filter}"),
                params![repair.id, change.turn, change.after],
            )
            .map_err(fail)?;
        }
        tx.commit().map_err(fail)?;
    }
    let connection = open_database(database)?;
    connection
        .execute(
            "UPDATE threads SET model_provider=?1, model=?2 WHERE id=?3",
            params![repair.provider, repair.model, repair.id],
        )
        .map_err(fail)?;
    // Keep the completed journal next to the original rollout so a rollback
    // has the exact old index values and cache offsets, not just message bytes.
    let completed = journal.with_extension("completed");
    if completed.exists() {
        if fs::read(&completed).map_err(fail)? != fs::read(journal).map_err(fail)? {
            return Err(fail("已完成的修复日志内容不一致"));
        }
        fs::remove_file(journal).map_err(fail)?;
    } else {
        fs::rename(journal, completed).map_err(fail)?;
    }
    Ok(())
}

#[cfg(test)]
fn repair_at(
    database: &Path,
    root: &Path,
    target: SessionTarget<'_>,
    routes: &[SessionRoute],
) -> Result<usize, AppError> {
    repair_with_guard(database, root, target, routes, || true)
}

#[cfg(test)]
fn repair_with_guard(
    database: &Path,
    root: &Path,
    target: SessionTarget<'_>,
    routes: &[SessionRoute],
    idle: impl Fn() -> bool,
) -> Result<usize, AppError> {
    repair_with_progress(database, root, target, routes, idle, |_, _, _| {})
}

pub(super) fn repair_with_progress(
    database: &Path,
    root: &Path,
    target: SessionTarget<'_>,
    routes: &[SessionRoute],
    idle: impl Fn() -> bool,
    progress: impl Fn(usize, usize, usize),
) -> Result<usize, AppError> {
    if !database.is_file() {
        return Ok(0);
    }
    let history = database.with_file_name("thread_history_1.sqlite");
    // Replay a durable per-thread journal before planning another migration.
    if root.exists() {
        for entry in fs::read_dir(root).map_err(fail)? {
            let path = entry.map_err(fail)?.path();
            if path.extension().is_some_and(|ext| ext == "pending") {
                if !idle() {
                    return Err(fail("Codex 在修复期间被打开，修复已安全暂停，已完成进度和备份均保留。请再次点击“重启 Codex”，等待修复完成后自动打开；期间请勿手动打开 Codex。"));
                }
                let repair: Repair =
                    serde_json::from_slice(&fs::read(&path).map_err(fail)?).map_err(fail)?;
                finish(database, &history, &repair, &path)?;
            }
        }
    }
    let connection = open_database(database)?;
    if !validate_threads_schema(&connection)? {
        return Ok(0);
    }
    let has_rollout: bool = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(fail)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(fail)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(fail)?
        .iter()
        .any(|name| name == "rollout_path");
    if !has_rollout {
        return Ok(0);
    }
    let (full_to_bare, bare_to_full) = known_models(routes);
    let columns = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(fail)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(fail)?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(fail)?;
    let time_expr = if columns.contains("recency_at_ms") {
        "CASE WHEN recency_at_ms > 0 THEN recency_at_ms ELSE 0 END"
    } else if columns.contains("updated_at_ms") {
        "CASE WHEN updated_at_ms > 0 THEN updated_at_ms ELSE 0 END"
    } else if columns.contains("created_at_ms") {
        "CASE WHEN created_at_ms > 0 THEN created_at_ms ELSE 0 END"
    } else if columns.contains("recency_at") {
        "CASE WHEN recency_at > 0 THEN recency_at * 1000 ELSE 0 END"
    } else if columns.contains("updated_at") {
        "CASE WHEN updated_at > 0 THEN updated_at * 1000 ELSE 0 END"
    } else if columns.contains("created_at") {
        "CASE WHEN created_at > 0 THEN created_at * 1000 ELSE 0 END"
    } else {
        "0"
    };
    let query = format!("SELECT id,model_provider,model,rollout_path FROM threads ORDER BY {time_expr} DESC, id DESC LIMIT 100");
    let rows = connection
        .prepare(&query)
        .map_err(fail)?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(fail)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(fail)?;
    let mut count = 0;
    let total = rows.len();
    progress(0, total, 0);
    for (scanned, (id, provider, model, rollout)) in rows.into_iter().enumerate() {
        progress(scanned, total, count);
        let update = planned_update(
            id.clone(),
            provider.clone(),
            model.clone(),
            target,
            &full_to_bare,
            &bare_to_full,
        );
        let target_provider = match target {
            SessionTarget::Default { provider, .. } | SessionTarget::OpenCodex { provider, .. } => {
                provider
            }
        };
        let update = match update {
            Some(update) => update,
            None if provider == target_provider => ThreadUpdate {
                id: id.clone(),
                provider: provider.clone(),
                model: model.clone(),
            },
            None => continue,
        };
        let rollout = PathBuf::from(rollout);
        if !rollout.is_file() {
            continue;
        }
        if rollout.is_symlink() || rollout.extension().is_none_or(|ext| ext != "jsonl") {
            return Err(fail("会话文件不是普通 JSONL"));
        }
        if fs::metadata(&rollout).map_err(fail)?.len() > 256 * 1024 * 1024 {
            return Err(fail("单个会话超过 256 MiB，暂停自动修复"));
        }
        let before = fs::read(&rollout).map_err(fail)?;
        let (after, shifts) = transform(&before, &id, &update.provider, update.model.as_deref())?;
        if shifts.is_empty() {
            continue;
        }
        if !idle() {
            return Err(fail("Codex 在修复期间被打开，修复已安全暂停，已完成进度和备份均保留。请再次点击“重启 Codex”，等待修复完成后自动打开；期间请勿手动打开 Codex。"));
        }
        let needed = (before.len() as u64)
            .saturating_mul(2)
            .saturating_add(fs::metadata(database).map_err(fail)?.len())
            .saturating_add(512 * 1024 * 1024);
        for path in [root, rollout.as_path()] {
            if crate::app::disk::available_space(path)?.is_some_and(|free| free < needed) {
                return Err(fail("磁盘空间不足，已暂停修复；请释放空间后重试"));
            }
        }
        fs::create_dir_all(root).map_err(fail)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(fail)?;
        }
        if count == 0 {
            create_consistent_backup(&connection, &root.join("index-before-repair.sqlite"))?;
        }
        let token = uuid::Uuid::new_v4().to_string();
        let backup = root.join(format!("{token}.jsonl"));
        atomic_file::write_atomic(&backup, &before).map_err(fail)?;
        let repair = Repair {
            id,
            rollout,
            backup,
            before_hash: hash(&before),
            after_hash: hash(&after),
            provider: update.provider,
            model: update.model,
            previous_provider: provider,
            previous_model: model,
            offsets: read_offsets(&history, &update.id, &shifts)?,
        };
        let journal = root.join(format!("{token}.pending"));
        atomic_file::write_atomic(&journal, &serde_json::to_vec(&repair).map_err(fail)?)
            .map_err(fail)?;
        if !idle() {
            return Err(fail("Codex 在修复期间被打开，修复已安全暂停，已完成进度和备份均保留。请再次点击“重启 Codex”，等待修复完成后自动打开；期间请勿手动打开 Codex。"));
        }
        finish(database, &history, &repair, &journal)?;
        count += 1;
    }
    progress(total, total, count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> SessionTarget<'static> {
        SessionTarget::OpenCodex {
            provider: "opencodex",
            default_provider: "osir",
            default_route: "test/gpt-5.4",
        }
    }
    fn routes() -> Vec<SessionRoute> {
        vec![SessionRoute {
            id: "test".into(),
            models: vec!["gpt-5.4".into()],
        }]
    }
    fn fixture() -> (PathBuf, PathBuf, PathBuf, Vec<u8>) {
        let root = std::env::temp_dir().join(format!("manager-metadata-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("state_5.sqlite");
        let rollout = root.join("rollout.jsonl");
        let bytes = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\",\"model_provider\":\"openai\",\"extra\":\"保留\"}}\n",
            "{ \"type\": \"response_item\", \"payload\": {\"text\":\"原始消息\"} }\n",
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\",\"model_provider\":\"openai\"}}\n",
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"fork-source\",\"model_provider\":\"openai\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\n"
        ).as_bytes().to_vec();
        fs::write(&rollout, &bytes).unwrap();
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,model_provider TEXT NOT NULL,model TEXT,rollout_path TEXT NOT NULL);").unwrap();
        connection
            .execute(
                "INSERT INTO threads VALUES('thread','openai','gpt-5.4',?1)",
                [rollout.to_str().unwrap()],
            )
            .unwrap();
        let history = Connection::open(root.join("thread_history_1.sqlite")).unwrap();
        history.execute_batch("CREATE TABLE thread_turns(thread_id TEXT,turn_id TEXT,rollout_byte_offset INTEGER,rollout_end_byte_offset INTEGER,status TEXT); CREATE TABLE thread_history_projection_state(thread_id TEXT PRIMARY KEY,next_rollout_byte_offset INTEGER,next_rollout_ordinal INTEGER);").unwrap();
        let first_end = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        history
            .execute(
                "INSERT INTO thread_turns VALUES('thread','turn',?1,?2,'completed')",
                params![first_end, bytes.len()],
            )
            .unwrap();
        history
            .execute(
                "INSERT INTO thread_history_projection_state VALUES('thread',?1,5)",
                [bytes.len()],
            )
            .unwrap();
        (root, database, rollout, bytes)
    }
    #[test]
    fn repairs_every_owned_meta_preserves_messages_and_translates_offsets() {
        let (root, database, rollout, before) = fixture();
        assert_eq!(
            repair_at(&database, &root.join("backups"), target(), &routes()).unwrap(),
            1
        );
        let after = fs::read(&rollout).unwrap();
        let old_lines = before
            .split_inclusive(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        let new_lines = after
            .split_inclusive(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        for index in [1, 3, 4] {
            assert_eq!(old_lines[index], new_lines[index]);
        }
        for index in [0, 2] {
            let value: serde_json::Value = serde_json::from_slice(new_lines[index]).unwrap();
            assert_eq!(value["payload"]["model_provider"], "opencodex");
        }
        let history = Connection::open(root.join("thread_history_1.sqlite")).unwrap();
        let offsets: (usize, usize, String) = history
            .query_row(
                "SELECT rollout_byte_offset,rollout_end_byte_offset,status FROM thread_turns",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            offsets,
            (new_lines[0].len(), after.len(), "completed".into())
        );
        let projected: (usize,i64) = history.query_row("SELECT next_rollout_byte_offset,next_rollout_ordinal FROM thread_history_projection_state",[],|row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(projected, (after.len(), 5));
        assert_eq!(
            repair_at(&database, &root.join("backups"), target(), &routes()).unwrap(),
            0
        );
        assert_eq!(fs::read(&rollout).unwrap(), after);
        drop(history);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn resumes_journal_after_rollout_replacement_and_partial_database_commit() {
        let (root, database, rollout, before) = fixture();
        let (after, shifts) =
            transform(&before, "thread", "opencodex", Some("test/gpt-5.4")).unwrap();
        let backup = root.join("original.jsonl");
        fs::write(&backup, &before).unwrap();
        let journal = root.join("repair.pending");
        let repair = Repair {
            id: "thread".into(),
            rollout: rollout.clone(),
            backup,
            before_hash: hash(&before),
            after_hash: hash(&after),
            provider: "opencodex".into(),
            model: Some("test/gpt-5.4".into()),
            previous_provider: "openai".into(),
            previous_model: Some("gpt-5.4".into()),
            offsets: read_offsets(&root.join("thread_history_1.sqlite"), "thread", &shifts)
                .unwrap(),
        };
        fs::write(&journal, serde_json::to_vec(&repair).unwrap()).unwrap();
        fs::write(&rollout, &after).unwrap();
        finish(
            &database,
            &root.join("thread_history_1.sqlite"),
            &repair,
            &journal,
        )
        .unwrap();
        // Crash after databases commit but before journal deletion: replay is idempotent.
        fs::write(&journal, serde_json::to_vec(&repair).unwrap()).unwrap();
        fs::remove_file(journal.with_extension("completed")).unwrap();
        finish(
            &database,
            &root.join("thread_history_1.sqlite"),
            &repair,
            &journal,
        )
        .unwrap();
        assert_eq!(fs::read(&rollout).unwrap(), after);
        assert!(!journal.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn settings_snapshots_follow_route_without_changing_permissions_or_foreign_threads() {
        let snapshot = serde_json::json!({"type":"event_msg","payload":{
            "type":"thread_settings_applied","thread_id":"thread",
            "thread_settings":{"model_provider_id":"openai","model":"old/model",
                "cwd":"/original","approval_policy":"on-request","permission_profile":{"type":"read-only"}}}});
        let mut foreign = snapshot.clone();
        foreign["payload"]["thread_id"] = "foreign".into();
        let bytes = format!("{snapshot}\n{foreign}\n").into_bytes();
        let (after, shifts) =
            transform(&bytes, "thread", "opencodex", Some("test/gpt-5.4")).unwrap();
        let lines = after
            .split_inclusive(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        let mut expected = snapshot;
        expected["payload"]["thread_settings"]["model_provider_id"] = "opencodex".into();
        expected["payload"]["thread_settings"]["model"] = "test/gpt-5.4".into();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(lines[0]).unwrap(),
            expected
        );
        assert_eq!(lines[1], format!("{foreign}\n").as_bytes());
        assert_eq!(translated(bytes.len() as i64, &shifts), after.len() as i64);
        assert!(
            transform(&after, "thread", "opencodex", Some("test/gpt-5.4"))
                .unwrap()
                .1
                .is_empty()
        );
    }
    #[test]
    fn running_backend_leaves_rollout_index_and_backups_untouched() {
        let (root, database, rollout, before) = fixture();
        assert!(repair_with_guard(
            &database,
            &root.join("backups"),
            target(),
            &routes(),
            || false
        )
        .is_err());
        assert_eq!(fs::read(&rollout).unwrap(), before);
        assert!(!root.join("backups").exists());
        let connection = Connection::open(&database).unwrap();
        let provider: String = connection
            .query_row("SELECT model_provider FROM threads", [], |row| row.get(0))
            .unwrap();
        assert_eq!(provider, "openai");
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pending_repair_replays_after_backend_starts_before_replacement() {
        let (root, database, rollout, before) = fixture();
        let calls = std::cell::Cell::new(0);
        let backups = root.join("backups");
        assert!(
            repair_with_guard(&database, &backups, target(), &routes(), || {
                calls.set(calls.get() + 1);
                calls.get() == 1
            })
            .is_err()
        );
        assert_eq!(fs::read(&rollout).unwrap(), before);
        assert!(fs::read_dir(&backups).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "pending")));
        repair_at(&database, &backups, target(), &routes()).unwrap();
        let expected = transform(&before, "thread", "opencodex", Some("test/gpt-5.4"))
            .unwrap()
            .0;
        assert_eq!(fs::read(&rollout).unwrap(), expected);
        assert!(!fs::read_dir(&backups).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "pending")));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unknown_provider_and_invalid_json_are_never_rewritten() {
        let (root, database, rollout, before) = fixture();
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "UPDATE threads SET model_provider='unrelated',model='test/gpt-5.4'",
                [],
            )
            .unwrap();
        assert_eq!(
            repair_at(&database, &root.join("backups"), target(), &routes()).unwrap(),
            0
        );
        assert_eq!(fs::read(&rollout).unwrap(), before);
        connection
            .execute("UPDATE threads SET model_provider='openai'", [])
            .unwrap();
        fs::write(&rollout, b"{partial").unwrap();
        assert!(repair_at(&database, &root.join("backups"), target(), &routes()).is_err());
        assert_eq!(fs::read(&rollout).unwrap(), b"{partial");
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "invoked by scripts/resume-continuity-smoke.py with an isolated fixture"]
    fn real_codex_fixture() {
        let root =
            PathBuf::from(std::env::var("MANAGER_RESUME_FIXTURE").expect("fixture required"));
        assert!(root.join(".manager-resume-fixture").is_file());
        assert!(root
            .canonicalize()
            .unwrap()
            .starts_with(std::env::temp_dir().canonicalize().unwrap()));
        let database = root.join(".codex/state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        let rollouts = connection
            .prepare("SELECT rollout_path FROM threads")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for rollout in rollouts {
            assert!(Path::new(&rollout)
                .canonicalize()
                .unwrap()
                .starts_with(root.canonicalize().unwrap()));
        }
        drop(connection);
        let target = if std::env::var("MANAGER_RESUME_TARGET").as_deref() == Ok("default") {
            SessionTarget::Default {
                provider: "openai",
                opencodex_provider: "opencodex",
            }
        } else {
            target()
        };
        repair_at(&database, &root.join("backups"), target, &routes()).unwrap();
        migrate_at(
            &database,
            &root.join("index-backup.sqlite"),
            target,
            &routes(),
        )
        .unwrap();
    }
}
