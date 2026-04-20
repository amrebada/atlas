//! Reads `~/.claude/projects/<slug>/*.jsonl` where each line is one event in

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::types::{Session, SessionStatus};

// ---------- Parsed-session DTO ----------

/// In-memory representation of a single parsed JSONL file. Converts to the
#[derive(Debug, Clone)]
pub struct ParsedSession {
    /// UUID from the file stem.
    pub id: String,
    /// Truncated first user prompt (≤ 80 chars).
    pub title: String,
    /// Timestamp of the first user message.
    pub when: DateTime<Utc>,
    /// Number of user prompts with textual content.
    pub turns: u32,
    /// Pretty `"<n>m <n>s"` / `"<n>h <n>m"` derived from first→last timestamp.
    pub duration: String,
    /// `"active" | "idle" | "archived"` - cheap heuristic (see `derive_status`).
    pub status: SessionStatus,
    /// Last user prompt, truncated.
    pub last: String,
    /// Best model string encountered (last assistant `message.model` wins).
    pub model: Option<String>,
    /// Branch name if the session logged a `gitBranch` ≠ `"HEAD"`.
    pub branch: Option<String>,
    /// Working directory seen on the first user event, used to resolve the
    pub cwd: Option<String>,
}

impl ParsedSession {
    /// Project the parsed session into the public DTO. `project_path` comes
    pub fn into_session(self, project_path: String) -> Session {
        Session {
            id: self.id,
            project_path,
            title: self.title,
            when: self.when.to_rfc3339(),
            turns: self.turns as i64,
            duration: self.duration,
            status: self.status,
            last: self.last,
            model: self.model.unwrap_or_default(),
            branch: self.branch,
        }
    }
}

// ---------- JSONL row schema (narrow, tolerant) ----------

#[derive(Debug, Deserialize)]
struct Row {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    model: Option<String>,
}

// ---------- Slug discovery ----------

/// Claude encodes a project's absolute path into a slug by replacing each
pub fn claude_dir_for_project(project_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let canon_target = canonicalize_or_self(project_path);

    let Some(root) = claude_projects_root() else {
        return Ok(None);
    };
    if !root.exists() {
        return Ok(None);
    }

    // Cheap heuristic: start with slugs whose generated form matches the
    let expected_slug = path_to_slug(&canon_target);

    // First pass: exact-match slug dir.
    let direct = root.join(&expected_slug);
    if direct.is_dir() {
        if let Some(cwd) = read_cwd_from_dir(&direct)? {
            if paths_equal(&cwd, &canon_target) {
                return Ok(Some(direct));
            }
        }
    }

    // Second pass: scan every slug dir, read cwd from each, compare.
    for entry in std::fs::read_dir(&root)? {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                tracing::trace!(?err, "read_dir entry in claude projects root");
                continue;
            }
        };
        let slug_dir = entry.path();
        if !slug_dir.is_dir() {
            continue;
        }
        match read_cwd_from_dir(&slug_dir) {
            Ok(Some(cwd)) if paths_equal(&cwd, &canon_target) => return Ok(Some(slug_dir)),
            Ok(_) => {}
            Err(err) => tracing::trace!(?err, dir = %slug_dir.display(), "read_cwd_from_dir"),
        }
    }

    Ok(None)
}

/// Best-effort canonicalization - symlinks may not resolve on every OS, so
fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let ca = canonicalize_or_self(a);
    let cb = canonicalize_or_self(b);
    ca == cb || a == b
}

/// `~/.claude/projects` - None when HOME can't be resolved.
fn claude_projects_root() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Minimal HOME resolver. We avoid pulling `dirs` for a one-line helper;
fn home_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("HOME") {
        return Some(PathBuf::from(h));
    }
    #[cfg(windows)]
    if let Some(h) = std::env::var_os("USERPROFILE") {
        return Some(PathBuf::from(h));
    }
    None
}

/// Encode an absolute path into the slug Claude Code uses on disk. The
fn path_to_slug(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_start_matches('/');
    let replaced = trimmed.replace(['/', '\\'], "-");
    format!("-{replaced}")
}

/// Read the first available `cwd` field from the newest JSONL in `dir`.
fn read_cwd_from_dir(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let newest = newest_jsonl(dir)?;
    let Some(path) = newest else {
        return Ok(None);
    };
    read_cwd_from_file(&path)
}

/// Walk a JSONL file until we encounter a row with a non-empty `cwd`.
fn read_cwd_from_file(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for (idx, line) in reader.lines().enumerate() {
        if idx > 500 {
            break;
        }
        let Ok(line) = line else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: Row = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                tracing::trace!(?err, path = %path.display(), "malformed jsonl line");
                continue;
            }
        };
        if let Some(cwd) = row.cwd {
            if !cwd.is_empty() {
                return Ok(Some(PathBuf::from(cwd)));
            }
        }
    }
    Ok(None)
}

/// Return the most recently modified `.jsonl` file inside `dir`, or None.
fn newest_jsonl(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        match &newest {
            Some((cur, _)) if *cur >= mtime => {}
            _ => newest = Some((mtime, path)),
        }
    }
    Ok(newest.map(|(_, p)| p))
}

// ---------- Single-file parser ----------

/// Parse one session JSONL file. Returns `Ok(None)` for files with zero
pub fn parse_session_file(path: &Path) -> anyhow::Result<Option<ParsedSession>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut first_user_prompt: Option<(DateTime<Utc>, String)> = None;
    let mut last_user_prompt: Option<(DateTime<Utc>, String)> = None;
    let mut last_timestamp: Option<DateTime<Utc>> = None;
    let mut turns: u32 = 0;
    let mut model: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut cwd: Option<String> = None;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: Row = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                tracing::trace!(?err, "skip malformed jsonl line");
                continue;
            }
        };

        let ts = row.timestamp.as_deref().and_then(parse_ts);
        if let Some(t) = ts {
            last_timestamp = Some(last_timestamp.map_or(t, |cur| cur.max(t)));
        }

        if cwd.is_none() {
            if let Some(c) = row.cwd.as_ref() {
                if !c.is_empty() {
                    cwd = Some(c.clone());
                }
            }
        }
        if branch.is_none() {
            if let Some(b) = row.git_branch.as_ref() {
                if !b.is_empty() && b != "HEAD" {
                    branch = Some(b.clone());
                }
            }
        }

        match row.kind.as_deref() {
            Some("user") => {
                if let Some(msg) = &row.message {
                    if msg.role.as_deref().unwrap_or("user") != "user" {
                        continue;
                    }
                    if let Some(text) = extract_user_text(msg.content.as_ref()) {
                        let at = ts.unwrap_or_else(Utc::now);
                        if first_user_prompt.is_none() {
                            first_user_prompt = Some((at, text.clone()));
                        }
                        last_user_prompt = Some((at, text));
                        turns = turns.saturating_add(1);
                    }
                }
            }
            Some("assistant") => {
                if let Some(m) = row.message.as_ref().and_then(|m| m.model.clone()) {
                    if !m.is_empty() {
                        model = Some(m);
                    }
                }
            }
            _ => {}
        }
    }

    let Some((when, first_text)) = first_user_prompt else {
        return Ok(None);
    };
    let (last_ts, last_text) = last_user_prompt.unwrap_or_else(|| (when, first_text.clone()));
    let duration_end = last_timestamp.unwrap_or(last_ts);

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session")
        .to_string();

    // Re-stat for status - the file may have been updated since we opened
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let status = derive_status(duration_end, mtime);

    Ok(Some(ParsedSession {
        id,
        title: truncate_prompt(&first_text, 80),
        when,
        turns,
        duration: format_duration(duration_end - when),
        status,
        last: truncate_prompt(&last_text, 160),
        model,
        branch,
        cwd,
    }))
}

/// A `message.content` can be:
fn extract_user_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(items) => {
            let mut buf = String::new();
            for item in items {
                if let Some(obj) = item.as_object() {
                    if obj.get("type").and_then(Value::as_str) == Some("text") {
                        if let Some(t) = obj.get("text").and_then(Value::as_str) {
                            if !buf.is_empty() {
                                buf.push('\n');
                            }
                            buf.push_str(t);
                        }
                    }
                }
            }
            if buf.is_empty() {
                None
            } else {
                Some(buf)
            }
        }
        _ => None,
    }
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn truncate_prompt(s: &str, max: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn format_duration(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Heuristic status:
fn derive_status(_last_event: DateTime<Utc>, mtime: SystemTime) -> SessionStatus {
    let now = SystemTime::now();
    let fresh = match now.duration_since(mtime) {
        Ok(d) => d.as_secs() < 5 * 60,
        Err(_) => true, // clock skew → treat as fresh
    };
    if fresh {
        SessionStatus::Active
    } else {
        SessionStatus::Idle
    }
}

// ---------- Cache + manager ----------

#[derive(Debug, Clone)]
struct CachedSession {
    parsed: ParsedSession,
    mtime: SystemTime,
}

/// Tauri-managed singleton. Re-parses a session file only when its mtime
#[derive(Default)]
pub struct SessionsManager {
    by_path: Mutex<HashMap<PathBuf, CachedSession>>,
    /// `project_path → ~/.claude/projects/<slug>` reverse-lookup cache.
    slug_cache: Mutex<HashMap<PathBuf, PathBuf>>,
}

impl SessionsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve the slug dir for a project, consulting the reverse-lookup
    pub fn claude_dir_for(&self, project_path: &Path) -> anyhow::Result<Option<PathBuf>> {
        let key = canonicalize_or_self(project_path);
        if let Some(cached) = self.slug_cache.lock().unwrap().get(&key).cloned() {
            if cached.is_dir() {
                return Ok(Some(cached));
            }
        }
        let Some(dir) = claude_dir_for_project(&key)? else {
            return Ok(None);
        };
        self.slug_cache.lock().unwrap().insert(key, dir.clone());
        Ok(Some(dir))
    }

    /// List every session for `project_path`, newest first.
    pub fn list_for_project(&self, project_path: &Path) -> anyhow::Result<Vec<Session>> {
        let Some(dir) = self.claude_dir_for(project_path)? else {
            return Ok(Vec::new());
        };

        let project_path_str = project_path.to_string_lossy().into_owned();
        let mut sessions: Vec<(DateTime<Utc>, Session)> = Vec::new();

        for entry in std::fs::read_dir(&dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else {
                continue;
            };

            let parsed = {
                let cached = self
                    .by_path
                    .lock()
                    .unwrap()
                    .get(&path)
                    .filter(|c| c.mtime == mtime)
                    .cloned();
                match cached {
                    Some(c) => Some(c.parsed),
                    None => match parse_session_file(&path) {
                        Ok(Some(p)) => {
                            self.by_path.lock().unwrap().insert(
                                path.clone(),
                                CachedSession {
                                    parsed: p.clone(),
                                    mtime,
                                },
                            );
                            Some(p)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            tracing::trace!(?err, path = %path.display(), "parse_session_file failed");
                            None
                        }
                    },
                }
            };

            if let Some(p) = parsed {
                let when = p.when;
                sessions.push((when, p.into_session(project_path_str.clone())));
            }
        }

        sessions.sort_by_key(|b| std::cmp::Reverse(b.0));
        Ok(sessions.into_iter().map(|(_, s)| s).collect())
    }

    /// Look up a parsed session by its uuid. Linear scan across the cache  -
    pub fn session_detail(&self, session_id: &str) -> Option<SessionDetail> {
        let guard = self.by_path.lock().unwrap();
        for (path, cached) in guard.iter() {
            if cached.parsed.id == session_id {
                return Some(SessionDetail {
                    id: cached.parsed.id.clone(),
                    cwd: cached.parsed.cwd.clone(),
                    model: cached.parsed.model.clone(),
                    branch: cached.parsed.branch.clone(),
                    file_path: path.to_string_lossy().into_owned(),
                });
            }
        }
        None
    }
}

/// Supplementary info returned by `sessions.session_detail`. Kept in
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub id: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub branch: Option<String>,
    pub file_path: String,
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Synthesize a minimal JSONL file with one user prompt + one assistant
    #[test]
    fn parses_minimal_session() {
        let tmp = tempfile_path("atlas_sess_parse_min");
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:00Z","cwd":"/tmp/p","gitBranch":"main","message":{{"role":"user","content":"hello world"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"assistant","timestamp":"2026-04-18T10:00:05Z","message":{{"role":"assistant","model":"claude-opus-4-7","content":"hi"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:02:14Z","message":{{"role":"user","content":"follow up?"}}}}"#
            )
            .unwrap();
        }

        let parsed = parse_session_file(&tmp).unwrap().expect("some session");
        assert_eq!(parsed.turns, 2);
        assert_eq!(parsed.title, "hello world");
        assert_eq!(parsed.last, "follow up?");
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.cwd.as_deref(), Some("/tmp/p"));
        assert_eq!(parsed.duration, "2m 14s");

        let _ = std::fs::remove_file(&tmp);
    }

    /// Malformed / unknown-type lines and `tool_result` content arrays
    #[test]
    fn skips_malformed_and_tool_results() {
        let tmp = tempfile_path("atlas_sess_parse_tolerant");
        {
            let mut f = File::create(&tmp).unwrap();
            // malformed line
            writeln!(f, "{{ not json").unwrap();
            // unknown type
            writeln!(
                f,
                r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-04-18T09:00:00Z"}}"#
            )
            .unwrap();
            // real user turn
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:00Z","cwd":"/tmp/p","message":{{"role":"user","content":"first"}}}}"#
            )
            .unwrap();
            // tool-result user row - must NOT count
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:01Z","message":{{"role":"user","content":[{{"type":"tool_result","content":"ok","tool_use_id":"x"}}]}}}}"#
            )
            .unwrap();
            // text-block user row - counts
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:01:00Z","message":{{"role":"user","content":[{{"type":"text","text":"second"}}]}}}}"#
            )
            .unwrap();
        }

        let parsed = parse_session_file(&tmp).unwrap().expect("some session");
        assert_eq!(parsed.turns, 2);
        assert_eq!(parsed.title, "first");
        assert_eq!(parsed.last, "second");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn path_to_slug_matches_claude_encoding() {
        assert_eq!(
            path_to_slug(Path::new("/Users/amre/workspace/atlas")),
            "-Users-amre-workspace-atlas"
        );
        assert_eq!(
            path_to_slug(Path::new("/tmp/one-day-build/cli")),
            "-tmp-one-day-build-cli"
        );
    }

    /// Slug reverse-mapping: write a fake `~/.claude/projects/<slug>/foo.jsonl`
    #[test]
    fn reverse_maps_slug_via_cwd() {
        let root = tempfile_path("atlas_sess_slug_root");
        let projects_dir = root.join(".claude").join("projects");
        let slug_dir = projects_dir.join("-some-project");
        std::fs::create_dir_all(&slug_dir).unwrap();
        let jsonl = slug_dir.join("abc.jsonl");
        std::fs::write(
            &jsonl,
            r#"{"type":"user","timestamp":"2026-04-18T10:00:00Z","cwd":"/some/other/project","message":{"role":"user","content":"hi"}}
"#,
        )
        .unwrap();

        let prev_home = std::env::var_os("HOME");
        // SAFETY: tests in this crate run serially per-process by default
        unsafe {
            std::env::set_var("HOME", &root);
        }

        let result = claude_dir_for_project(Path::new("/some/other/project")).unwrap();

        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }

        assert_eq!(result.as_deref(), Some(slug_dir.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    fn tempfile_path(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("{prefix}-{ns}"));
        p
    }
}
