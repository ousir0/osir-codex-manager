#!/usr/bin/env python3
"""Back up Codex history, then archive everything except the 100 most recent threads."""
from __future__ import annotations

import argparse, hashlib, json, os, shutil, sqlite3, sys
from datetime import datetime, timezone
from pathlib import Path

CODEX = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")).expanduser()
DEFAULT_ROOT = Path.home() / "Library/Application Support/com.osir.OSIR.CodexManager/codex-session-retention-backups"

def die(msg: str) -> None:
    print(f"错误：{msg}", file=sys.stderr); raise SystemExit(1)

def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""): h.update(chunk)
    return h.hexdigest()

def stamp(v) -> int:
    try: return int(v or 0)
    except (TypeError, ValueError): return 0

def backup_sqlite(source: Path, target: Path) -> None:
    with sqlite3.connect(f"file:{source}?mode=ro", uri=True) as src, sqlite3.connect(target) as dst:
        src.backup(dst)

def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--backup-root", type=Path, default=DEFAULT_ROOT)
    args = ap.parse_args()
    if shutil.which("pgrep") and os.system("pgrep -x codex >/dev/null 2>&1") == 0:
        die("Codex 正在运行，请退出 Codex 后再执行备份和归档，避免复制或移动活跃文件。")
    db = CODEX / "state_5.sqlite"
    if not db.is_file(): die(f"找不到 {db}")
    uri = f"file:{db}?mode=ro"
    with sqlite3.connect(uri, uri=True) as src:
        cols = {r[1] for r in src.execute("PRAGMA table_info(threads)")}
        if "rollout_path" not in cols: die("threads 表缺少 rollout_path")
        parts = []
        for c in ("recency_at_ms", "updated_at_ms", "created_at_ms"):
            if c in cols: parts.append(f"NULLIF({c}, 0)")
        for c in ("recency_at", "updated_at", "created_at"):
            if c in cols: parts.append(f"NULLIF({c} * 1000, 0)")
        expr = "COALESCE(" + ",".join(parts) + ",0)" if parts else "0"
        rows = src.execute(f"SELECT id, rollout_path, {expr} AS activity_time FROM threads ORDER BY activity_time DESC, id DESC").fetchall()
    if not rows: die("没有找到会话")
    keep, archive = rows[:100], rows[100:]
    print(f"会话总数：{len(rows)}；保留：{len(keep)}；归档：{len(archive)}")
    print(f"第 1 个：{keep[0][0]} / {keep[0][2]}; 第 {len(keep)} 个：{keep[-1][0]} / {keep[-1][2]}")
    if len(rows) > 100: print(f"第 101 个：{archive[0][0]} / {archive[0][2]}")
    if args.dry_run: return
    stamp_name = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = args.backup_root / stamp_name
    sessions = out / "sessions"
    sessions.mkdir(parents=True, mode=0o700)
    files = []
    for name in ("state_5.sqlite", "thread_history_1.sqlite"):
        source = CODEX / name
        if source.is_file():
            target = out / name
            if name == "state_5.sqlite": backup_sqlite(source, target)
            else: backup_sqlite(source, target)
            files.append({"source": str(source), "backup": str(target), "sha256": digest(target), "size": target.stat().st_size})
    for thread_id, rollout, activity in rows:
        source = Path(rollout).expanduser()
        if not source.is_file(): continue
        target = sessions / f"{thread_id}.jsonl"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        files.append({"threadId": thread_id, "source": str(source), "backup": str(target), "sha256": digest(target), "size": target.stat().st_size, "activityTime": activity})
    manifest = {"createdAt": datetime.now(timezone.utc).isoformat(), "codexHome": str(CODEX), "total": len(rows), "kept": [r[0] for r in keep], "archived": [r[0] for r in archive], "files": files}
    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    # Logical archive is reversible and leaves SQLite/JSONL paths intact.
    with sqlite3.connect(db) as dst:
        dst.execute("BEGIN IMMEDIATE")
        if "archived" in cols:
            for thread_id, _, _ in archive: dst.execute("UPDATE threads SET archived=1 WHERE id=?", (thread_id,))
        dst.commit()
    print(f"完成：{out}")

if __name__ == "__main__": main()
