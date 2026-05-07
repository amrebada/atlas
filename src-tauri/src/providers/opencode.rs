//! OpenCode CLI provider.
//!
//! Modern OpenCode (≥ 1.0.10x) stores everything in SQLite at
//! `~/.local/share/opencode/opencode.db` (override via `$OPENCODE_DATA_DIR`).
//! Older versions kept per-session JSON files under `storage/session/...`;
//! we read the DB when present and fall back to the JSON layout otherwise so
//! historical sessions are not lost.
//!
//! Resume: `opencode --session <id>` continues a specific session in the
//! TUI. (The CLI also accepts `-c` / `--continue` for the last session and
//! `--fork` to branch from one.)

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OpenFlags};
use serde::Deserialize;

use super::shared::{
    canonicalize_or_self, derive_status, format_duration, home_dir, paths_equal, truncate_prompt,
};
use super::{ParsedSession, ResumeInvocation, SessionDetail, SessionProvider, ID_OPENCODE};

pub struct OpenCodeProvider;

impl SessionProvider for OpenCodeProvider {
    fn id(&self) -> &'static str {
        ID_OPENCODE
    }
    fn label(&self) -> &'static str {
        "OpenCode CLI"
    }
    fn binary_name(&self) -> &'static str {
        "opencode"
    }

    fn list_for_project(&self, project_path: &Path) -> anyhow::Result<Vec<ParsedSession>> {
        let Some(root) = opencode_data_root() else {
            return Ok(Vec::new());
        };
        if !root.exists() {
            return Ok(Vec::new());
        }
        let canon = canonicalize_or_self(project_path);

        let mut out = Vec::new();
        // Modern path: SQLite.
        let db_path = root.join("opencode.db");
        if db_path.is_file() {
            match list_from_sqlite(&db_path, &canon) {
                Ok(parsed) => out.extend(parsed),
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        path = %db_path.display(),
                        "opencode: sqlite read failed; falling back to JSON",
                    );
                }
            }
        }

        // Legacy path: per-session JSON. Always run too — picks up history
        // from versions before the SQLite migration.
        let storage = root.join("storage");
        if storage.is_dir() {
            if let Err(err) = list_from_storage(&storage, &canon, &mut out) {
                tracing::trace!(?err, "opencode: legacy storage scan failed");
            }
        }

        // De-dup by id (newer SQLite entries win — they came first in the Vec).
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| seen.insert(p.id.clone()));
        Ok(out)
    }

    fn resume_invocation(&self, detail: &SessionDetail) -> ResumeInvocation {
        let cwd = detail
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        ResumeInvocation {
            provider: ID_OPENCODE.into(),
            command: "opencode".into(),
            args: vec!["--session".into(), detail.id.clone()],
            cwd,
        }
    }

    fn new_invocation(&self, project_path: &Path) -> ResumeInvocation {
        ResumeInvocation {
            provider: ID_OPENCODE.into(),
            command: "opencode".into(),
            args: Vec::new(),
            cwd: project_path.to_string_lossy().into_owned(),
        }
    }
}

// ---------- Storage roots ----------

fn opencode_data_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OPENCODE_DATA_DIR") {
        return Some(PathBuf::from(p));
    }
    home_dir().map(|h| h.join(".local").join("share").join("opencode"))
}

// ---------- SQLite reader (modern OpenCode) ----------
//
// Schema (verified against opencode 1.0.51):
//   project(id, worktree, vcs, name, time_created, time_updated, ...)
//   session(id, project_id, slug, directory, title, version, time_created,
//           time_updated, time_archived, model, agent, ...)
//   message(id, session_id, time_created, time_updated, data)
//   session_message(id, session_id, type, time_created, time_updated, data)
//     -- legacy table; current schema stores role inside `message.data`.
//
// `time_*` columns are integer ms-since-epoch. The role is encoded inside
// the `data` JSON blob (`{"role":"user"|"assistant",…}`), so we extract via
// `json_extract`.

struct DbSession {
    id: String,
    title: String,
    directory: String,
    time_created: i64,
    time_updated: i64,
    time_archived: Option<i64>,
    model: Option<String>,
}

fn list_from_sqlite(db_path: &Path, project_canon: &Path) -> anyhow::Result<Vec<ParsedSession>> {
    // Read-only + immutable lets us open the DB while OpenCode itself has
    // it open with WAL. `mode=ro` rejects writes; `immutable=1` disables
    // shared-locking and avoids contention on the WAL.
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    // Find every project row whose worktree matches the project path.
    let project_ids = matching_project_ids(&conn, project_canon)?;
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut sessions: Vec<DbSession> = Vec::new();
    {
        // SQLite IN-list bind expansion is per-row painful; just stream
        // sessions and filter in Rust.
        let mut stmt = conn.prepare(
            "SELECT id, project_id, COALESCE(title,''), COALESCE(directory,''), \
                    time_created, time_updated, time_archived, model \
             FROM session ORDER BY time_updated DESC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let project_id: String = row.get(1)?;
            if !project_ids.contains(&project_id) {
                continue;
            }
            sessions.push(DbSession {
                id: row.get(0)?,
                title: row.get(2)?,
                directory: row.get(3)?,
                time_created: row.get(4)?,
                time_updated: row.get(5)?,
                time_archived: row.get(6).ok(),
                model: row.get(7).ok(),
            });
        }
    }
    if sessions.is_empty() {
        return Ok(Vec::new());
    }

    // Pull message stats in one pass: total + last user message preview.
    let mut out = Vec::with_capacity(sessions.len());
    for s in sessions {
        let (turns, last) = message_stats(&conn, &s.id);
        let when = ms_to_utc(s.time_created);
        let last_ts = ms_to_utc(s.time_updated);
        let mtime = SystemTime::UNIX_EPOCH
            + std::time::Duration::from_millis(s.time_updated.max(0) as u64);
        let status = if s.time_archived.is_some() {
            crate::storage::types::SessionStatus::Archived
        } else {
            derive_status(last_ts, mtime)
        };

        let title = if s.title.is_empty() {
            "Untitled session".to_string()
        } else {
            truncate_prompt(&s.title, 80)
        };

        out.push(ParsedSession {
            provider: ID_OPENCODE.into(),
            id: s.id,
            title,
            when,
            turns,
            duration: format_duration(last_ts - when),
            status,
            last,
            model: s.model.filter(|s| !s.is_empty()),
            branch: None,
            cwd: if s.directory.is_empty() {
                None
            } else {
                Some(s.directory)
            },
            source_path: None,
        });
    }

    Ok(out)
}

fn matching_project_ids(
    conn: &Connection,
    project_canon: &Path,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let mut ids = std::collections::HashSet::new();
    let mut stmt = conn.prepare("SELECT id, worktree FROM project")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let worktree: String = row.get(1)?;
        if worktree.is_empty() {
            continue;
        }
        if paths_equal(Path::new(&worktree), project_canon) {
            ids.insert(id);
        }
    }
    Ok(ids)
}

/// Returns `(turn_count, last_user_text)` for a session. Turn count is the
/// number of `user` messages, mirroring Claude/Codex semantics.
///
/// We try the modern `message` table first (role inside the `data` JSON
/// blob), then fall back to the legacy `session_message` table that older
/// OpenCode builds populated.
fn message_stats(conn: &Connection, session_id: &str) -> (u32, String) {
    if let Some(stats) = stats_from_message_table(conn, session_id) {
        return stats;
    }
    stats_from_session_message_table(conn, session_id).unwrap_or((0, String::new()))
}

fn stats_from_message_table(conn: &Connection, session_id: &str) -> Option<(u32, String)> {
    // Bail if the table is missing.
    let mut check = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='message'")
        .ok()?;
    if !check.exists([]).ok()? {
        return None;
    }

    let turns: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM message \
             WHERE session_id = ?1 AND json_extract(data, '$.role') = 'user'",
            params![session_id],
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .map(|n| n.max(0) as u32)
        .unwrap_or(0);

    let last_data: Option<String> = conn
        .query_row(
            "SELECT data FROM message \
             WHERE session_id = ?1 AND json_extract(data, '$.role') = 'user' \
             ORDER BY time_created DESC LIMIT 1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .ok();

    let last = last_data
        .as_deref()
        .and_then(extract_message_text)
        .map(|s| truncate_prompt(&s, 160))
        .unwrap_or_default();
    Some((turns, last))
}

fn stats_from_session_message_table(
    conn: &Connection,
    session_id: &str,
) -> Option<(u32, String)> {
    let mut check = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='session_message'")
        .ok()?;
    if !check.exists([]).ok()? {
        return None;
    }

    let turns: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM session_message WHERE session_id = ?1 AND type = 'user'",
            params![session_id],
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .map(|n| n.max(0) as u32)
        .unwrap_or(0);

    let last_data: Option<String> = conn
        .query_row(
            "SELECT data FROM session_message \
             WHERE session_id = ?1 AND type = 'user' \
             ORDER BY time_created DESC LIMIT 1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .ok();

    let last = last_data
        .as_deref()
        .and_then(extract_message_text)
        .map(|s| truncate_prompt(&s, 160))
        .unwrap_or_default();
    Some((turns, last))
}

/// Best-effort text extraction from a `session_message.data` JSON blob.
/// OpenCode's message body is provider-specific; we walk a few common
/// shapes (string, `{ text }`, `{ content: [...] }`, `{ parts: [...] }`).
fn extract_message_text(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;

    // Direct string.
    if let Some(s) = v.as_str() {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }

    // {"text": "..."}
    if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    // {"prompt": "..."}
    if let Some(s) = v.get("prompt").and_then(|x| x.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }

    // {"content": "..."} or {"content": [...]}
    if let Some(content) = v.get("content") {
        if let Some(s) = content.as_str() {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
        if let Some(arr) = content.as_array() {
            let mut buf = String::new();
            for item in arr {
                if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(t);
                }
            }
            if !buf.is_empty() {
                return Some(buf);
            }
        }
    }

    // {"parts": [{type: "text", text: "..."}]}
    if let Some(parts) = v.get("parts").and_then(|x| x.as_array()) {
        let mut buf = String::new();
        for item in parts {
            if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(t);
            }
        }
        if !buf.is_empty() {
            return Some(buf);
        }
    }

    None
}

fn ms_to_utc(ms: i64) -> DateTime<Utc> {
    let secs = ms.max(0) / 1000;
    let nsec = (ms.max(0) % 1000) as u32 * 1_000_000;
    Utc.timestamp_opt(secs, nsec).single().unwrap_or_else(Utc::now)
}

// ---------- Legacy JSON reader ----------
//
// Pre-SQLite OpenCode kept everything as JSON files under `storage/`. We
// keep this path so historical sessions remain visible after the user
// upgrades.

fn list_from_storage(
    storage: &Path,
    project_canon: &Path,
    out: &mut Vec<ParsedSession>,
) -> anyhow::Result<()> {
    let project_dir = storage.join("project");
    let session_dir = storage.join("session");
    let message_dir = storage.join("message");
    if !project_dir.exists() || !session_dir.exists() {
        return Ok(());
    }

    let mut project_ids: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&project_dir)? {
        let Ok(entry) = entry else { continue };
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(Some(idx)) = parse_project_index(&p) {
            if !idx.worktree.is_empty()
                && paths_equal(Path::new(&idx.worktree), project_canon)
            {
                project_ids.push(idx.id);
            }
        }
    }

    for project_id in &project_ids {
        let bucket = session_dir.join(project_id);
        if !bucket.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&bucket)? {
            let Ok(entry) = entry else { continue };
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(Some(parsed)) = parse_session_info(&p, &message_dir) {
                out.push(parsed);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProjectIndex {
    #[serde(default)]
    id: String,
    #[serde(default)]
    worktree: String,
}

fn parse_project_index(path: &Path) -> anyhow::Result<Option<ProjectIndex>> {
    let file = File::open(path)?;
    Ok(serde_json::from_reader(BufReader::new(file)).ok())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyTime {
    created: Option<f64>,
    updated: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacySession {
    id: Option<String>,
    title: Option<String>,
    time: Option<LegacyTime>,
    created: Option<f64>,
    updated: Option<f64>,
    directory: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
}

fn parse_session_info(path: &Path, message_dir: &Path) -> anyhow::Result<Option<ParsedSession>> {
    let file = File::open(path)?;
    let raw: LegacySession = match serde_json::from_reader(BufReader::new(file)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let id = raw
        .id
        .clone()
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "session".to_string());
    let session_cwd = raw
        .directory
        .clone()
        .or(raw.cwd.clone())
        .filter(|s| !s.is_empty());
    let created = raw
        .time
        .as_ref()
        .and_then(|t| t.created)
        .or(raw.created);
    let updated = raw
        .time
        .as_ref()
        .and_then(|t| t.updated)
        .or(raw.updated);
    let when = legacy_to_utc(created).unwrap_or_else(|| {
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        system_to_utc(mtime)
    });
    let last_ts = legacy_to_utc(updated).unwrap_or(when);
    let title = raw
        .title
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| truncate_prompt(s, 80))
        .unwrap_or_else(|| "Untitled session".to_string());
    let (turns, model) = legacy_scan_messages(message_dir, &id);
    let model = raw.model.filter(|s| !s.is_empty()).or(model);
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(Some(ParsedSession {
        provider: ID_OPENCODE.into(),
        id,
        title,
        when,
        turns,
        duration: format_duration(last_ts - when),
        status: derive_status(last_ts, mtime),
        last: String::new(),
        model,
        branch: None,
        cwd: session_cwd,
        source_path: Some(path.to_path_buf()),
    }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyMessage {
    role: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
}

fn legacy_scan_messages(message_dir: &Path, session_id: &str) -> (u32, Option<String>) {
    let dir = message_dir.join(session_id);
    if !dir.is_dir() {
        return (0, None);
    }
    let mut turns: u32 = 0;
    let mut last_model: Option<String> = None;
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return (0, None),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(file) = File::open(&p) else { continue };
        let msg: LegacyMessage = match serde_json::from_reader(BufReader::new(file)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if msg.role.as_deref() == Some("user") {
            turns = turns.saturating_add(1);
        }
        if let Some(m) = msg.model_id.filter(|s| !s.is_empty()) {
            last_model = Some(m);
        }
    }
    (turns, last_model)
}

fn legacy_to_utc(n: Option<f64>) -> Option<DateTime<Utc>> {
    let n = n?;
    let ms = if n > 1e12 { n } else { n * 1000.0 };
    let secs = (ms / 1000.0).floor() as i64;
    Utc.timestamp_opt(secs, 0).single()
}

fn system_to_utc(t: SystemTime) -> DateTime<Utc> {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Utc.timestamp_opt(secs, 0).single().unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::io::Write;
    use std::path::PathBuf;

    fn unique_tmpdir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("atlas-opencode-{tag}-{ns}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn build_db(path: &Path, worktree: &str) {
        // Mirrors modern OpenCode (≥ 1.0.10x): role lives inside `message.data`.
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT NOT NULL, vcs TEXT, name TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE session (
               id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, slug TEXT,
               directory TEXT, title TEXT NOT NULL, version TEXT,
               time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
               time_archived INTEGER, model TEXT, agent TEXT
             );
             CREATE TABLE message (
               id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
               time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
               data TEXT NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project (id, worktree, vcs, time_created, time_updated) VALUES (?, ?, 'git', 0, 0)",
            params!["proj1", worktree],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, model)
             VALUES ('ses_a', 'proj1', 'a', ?, 'Title A', '1.0', 1771613348089, 1771613472015, 'glm-4.6')",
            params![worktree],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES
             ('m1','ses_a',1,1,'{\"role\":\"user\",\"text\":\"hello world\"}'),
             ('m2','ses_a',2,2,'{\"role\":\"assistant\",\"text\":\"hi\"}'),
             ('m3','ses_a',3,3,'{\"role\":\"user\",\"parts\":[{\"type\":\"text\",\"text\":\"second prompt\"}]}')",
            [],
        )
        .unwrap();
    }

    fn build_legacy_db(path: &Path, worktree: &str) {
        // Pre-1.0.10x schema: explicit `session_message.type` column.
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT NOT NULL, vcs TEXT, name TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE session (
               id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, slug TEXT,
               directory TEXT, title TEXT NOT NULL, version TEXT,
               time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
               time_archived INTEGER, model TEXT, agent TEXT
             );
             CREATE TABLE session_message (
               id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,
               time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
               data TEXT NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project (id, worktree, vcs, time_created, time_updated) VALUES (?, ?, 'git', 0, 0)",
            params!["proj1", worktree],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, model)
             VALUES ('ses_legacy', 'proj1', 'l', ?, 'Legacy', '0.9', 1771613348089, 1771613472015, NULL)",
            params![worktree],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, time_created, time_updated, data) VALUES
             ('m1','ses_legacy','user',1,1,'{\"text\":\"old prompt\"}'),
             ('m2','ses_legacy','assistant',2,2,'{\"text\":\"old reply\"}')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn sqlite_reader_returns_session_for_matching_worktree() {
        let dir = unique_tmpdir("sqlite-match");
        let target = dir.join("project-on-disk");
        std::fs::create_dir_all(&target).unwrap();
        let db = dir.join("opencode.db");
        build_db(&db, &target.to_string_lossy());

        let parsed = list_from_sqlite(&db, &target).unwrap();
        assert_eq!(parsed.len(), 1);
        let s = &parsed[0];
        assert_eq!(s.id, "ses_a");
        assert_eq!(s.title, "Title A");
        assert_eq!(s.turns, 2);
        assert_eq!(s.last, "second prompt");
        assert_eq!(s.model.as_deref(), Some("glm-4.6"));
        assert_eq!(s.cwd.as_deref(), Some(target.to_str().unwrap()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sqlite_reader_legacy_session_message_table() {
        let dir = unique_tmpdir("sqlite-legacy");
        let target = dir.join("project");
        std::fs::create_dir_all(&target).unwrap();
        let db = dir.join("opencode.db");
        build_legacy_db(&db, &target.to_string_lossy());

        let parsed = list_from_sqlite(&db, &target).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "ses_legacy");
        assert_eq!(parsed[0].turns, 1);
        assert_eq!(parsed[0].last, "old prompt");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sqlite_reader_skips_other_projects() {
        let dir = unique_tmpdir("sqlite-skip");
        let on_disk = dir.join("on-disk");
        std::fs::create_dir_all(&on_disk).unwrap();
        let other = dir.join("other-project");
        std::fs::create_dir_all(&other).unwrap();
        let db = dir.join("opencode.db");
        build_db(&db, &on_disk.to_string_lossy());

        let parsed = list_from_sqlite(&db, &other).unwrap();
        assert!(parsed.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_session_with_nested_time() {
        let dir = unique_tmpdir("legacy-nested-time");
        let session = dir.join("ses_x.json");
        let mut f = File::create(&session).unwrap();
        writeln!(
            f,
            r#"{{"id":"ses_x","title":"Refactor","time":{{"created":1771613348089,"updated":1771613472015}},"directory":"/tmp/proj"}}"#
        )
        .unwrap();
        drop(f);
        let messages_root = dir.join("message");
        std::fs::create_dir_all(messages_root.join("ses_x")).unwrap();
        let m1 = messages_root.join("ses_x").join("msg_1.json");
        let mut mf = File::create(&m1).unwrap();
        writeln!(
            mf,
            r#"{{"id":"msg_1","role":"user","modelID":"glm-4.6"}}"#
        )
        .unwrap();
        drop(mf);
        let parsed = parse_session_info(&session, &messages_root)
            .unwrap()
            .expect("session");
        assert_eq!(parsed.id, "ses_x");
        assert_eq!(parsed.title, "Refactor");
        assert_eq!(parsed.turns, 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
