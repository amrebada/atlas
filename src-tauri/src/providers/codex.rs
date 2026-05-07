//! Codex CLI provider — reads `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//!
//! Codex stores sessions by date, not by project, so we walk the most
//! recent N days and filter by `cwd`. Bound the walk so cold projects
//! don't read thousands of files.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::shared::{
    canonicalize_or_self, derive_status, format_duration, home_dir, parse_ts, paths_equal,
    truncate_prompt,
};
use super::{ParsedSession, ResumeInvocation, SessionDetail, SessionProvider, ID_CODEX};

/// How many recent date directories we scan during discovery.
/// Override with `ATLAS_CODEX_LOOKBACK_DAYS` for testing.
const DEFAULT_LOOKBACK_DAYS: u32 = 90;

pub struct CodexProvider;

impl SessionProvider for CodexProvider {
    fn id(&self) -> &'static str {
        ID_CODEX
    }
    fn label(&self) -> &'static str {
        "Codex CLI"
    }
    fn binary_name(&self) -> &'static str {
        "codex"
    }

    fn list_for_project(&self, project_path: &Path) -> anyhow::Result<Vec<ParsedSession>> {
        let Some(root) = codex_sessions_root() else {
            return Ok(Vec::new());
        };
        if !root.exists() {
            return Ok(Vec::new());
        }
        let canon = canonicalize_or_self(project_path);
        let lookback = lookback_days();
        let mut files = Vec::new();
        collect_recent_jsonl(&root, lookback, &mut files)?;

        let mut out: Vec<ParsedSession> = Vec::new();
        for path in files {
            match parse_session_file(&path) {
                Ok(Some(parsed)) => {
                    let session_cwd = parsed
                        .cwd
                        .as_deref()
                        .map(Path::new)
                        .map(canonicalize_or_self);
                    let belongs = match session_cwd {
                        Some(c) => paths_equal(&c, &canon),
                        None => false,
                    };
                    if belongs {
                        out.push(parsed);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::trace!(?err, path = %path.display(), "codex: parse failed");
                }
            }
        }
        Ok(out)
    }

    fn resume_invocation(&self, detail: &SessionDetail) -> ResumeInvocation {
        let cwd = detail
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        ResumeInvocation {
            provider: ID_CODEX.into(),
            command: "codex".into(),
            args: vec!["resume".into(), detail.id.clone()],
            cwd,
        }
    }

    fn new_invocation(&self, project_path: &Path) -> ResumeInvocation {
        ResumeInvocation {
            provider: ID_CODEX.into(),
            command: "codex".into(),
            args: Vec::new(),
            cwd: project_path.to_string_lossy().into_owned(),
        }
    }
}

// ---------- Storage layout ----------

fn codex_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("CODEX_HOME") {
        return Some(PathBuf::from(h));
    }
    home_dir().map(|h| h.join(".codex"))
}

fn codex_sessions_root() -> Option<PathBuf> {
    codex_home().map(|h| h.join("sessions"))
}

fn lookback_days() -> u32 {
    std::env::var("ATLAS_CODEX_LOOKBACK_DAYS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_LOOKBACK_DAYS)
}

/// Walk `<root>/YYYY/MM/DD/*.jsonl`, taking up to `lookback` of the newest
/// day directories.
fn collect_recent_jsonl(root: &Path, lookback: u32, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let mut day_dirs: Vec<PathBuf> = Vec::new();
    for y in numeric_subdirs(root)? {
        for m in numeric_subdirs(&y)? {
            for d in numeric_subdirs(&m)? {
                day_dirs.push(d);
            }
        }
    }
    // Sort newest-first by path (string compare works because YYYY/MM/DD
    // segments are zero-padded numeric).
    day_dirs.sort();
    day_dirs.reverse();
    day_dirs.truncate(lookback as usize);

    for day in day_dirs {
        let Ok(read_dir) = std::fs::read_dir(&day) else { continue };
        for entry in read_dir.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(p);
            }
        }
    }
    Ok(())
}

fn numeric_subdirs(parent: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !parent.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(parent)? {
        let Ok(entry) = entry else { continue };
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if name.chars().all(|c| c.is_ascii_digit()) {
                out.push(p);
            }
        }
    }
    Ok(out)
}

// ---------- Per-file parser ----------

/// Codex JSONL events vary in shape across versions. We tolerate any row
/// with `timestamp` and a `cwd` somewhere in the early rows, plus user /
/// assistant messages keyed by `type`.
#[derive(Debug, Deserialize)]
struct Row {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
    #[serde(default)]
    message: Option<Value>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default, rename = "session_id")]
    session_id_alt: Option<String>,
}

fn parse_session_file(path: &Path) -> anyhow::Result<Option<ParsedSession>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut session_id: Option<String> = None;
    let mut first_user_prompt: Option<(DateTime<Utc>, String)> = None;
    let mut last_user_prompt: Option<(DateTime<Utc>, String)> = None;
    let mut last_timestamp: Option<DateTime<Utc>> = None;
    let mut turns: u32 = 0;
    let mut model: Option<String> = None;
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
                tracing::trace!(?err, "codex: skip malformed jsonl");
                continue;
            }
        };

        if session_id.is_none() {
            session_id = row.session_id.clone().or(row.session_id_alt.clone());
        }

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
            // Some Codex schemas embed cwd inside payload.
            if cwd.is_none() {
                if let Some(c) = row
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("cwd"))
                    .and_then(|v| v.as_str())
                {
                    if !c.is_empty() {
                        cwd = Some(c.to_string());
                    }
                }
            }
        }

        // Look for a model name embedded in payload or message blocks.
        if model.is_none() {
            if let Some(m) = row
                .payload
                .as_ref()
                .and_then(|p| p.get("model"))
                .and_then(|v| v.as_str())
            {
                if !m.is_empty() {
                    model = Some(m.to_string());
                }
            }
        }

        // User messages may live under `message` (string or content array)
        // or under `payload.input` / `payload.text`.
        let role = row
            .message
            .as_ref()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str())
            .or(row.kind.as_deref());

        if role == Some("user") || row.kind.as_deref() == Some("user") {
            let text = extract_user_text(row.message.as_ref())
                .or_else(|| extract_user_text(row.payload.as_ref()));
            if let Some(t) = text {
                let at = ts.unwrap_or_else(Utc::now);
                if first_user_prompt.is_none() {
                    first_user_prompt = Some((at, t.clone()));
                }
                last_user_prompt = Some((at, t));
                turns = turns.saturating_add(1);
            }
        }
    }

    let Some((when, first_text)) = first_user_prompt else {
        return Ok(None);
    };
    let (last_ts, last_text) = last_user_prompt.unwrap_or_else(|| (when, first_text.clone()));
    let duration_end = last_timestamp.unwrap_or(last_ts);

    // Codex `rollout-<ts>-<sessionid>.jsonl`. Prefer the embedded session id;
    // fall back to filename suffix.
    let id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|stem| {
                stem.strip_prefix("rollout-")
                    .and_then(|s| s.split('-').last())
                    .unwrap_or(stem)
                    .to_string()
            })
            .unwrap_or_else(|| "session".to_string())
    });

    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    Ok(Some(ParsedSession {
        provider: ID_CODEX.into(),
        id,
        title: truncate_prompt(&first_text, 80),
        when,
        turns,
        duration: format_duration(duration_end - when),
        status: derive_status(duration_end, mtime),
        last: truncate_prompt(&last_text, 160),
        model,
        branch: None,
        cwd,
        source_path: Some(path.to_path_buf()),
    }))
}

/// Best-effort text extraction tolerant of multiple Codex payload shapes:
/// - plain string under `content` / `text` / `input`
/// - array of `{type:"text", text:"..."}` blocks
fn extract_user_text(blob: Option<&Value>) -> Option<String> {
    let blob = blob?;
    // Direct string fields.
    for key in ["content", "text", "input", "prompt"] {
        if let Some(s) = blob.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    // Content as a string at the message root.
    if let Some(Value::String(s)) = blob.get("content") {
        if !s.is_empty() {
            return Some(s.clone());
        }
    }
    // Content array of text blocks.
    if let Some(arr) = blob.get("content").and_then(|v| v.as_array()) {
        let mut buf = String::new();
        for item in arr {
            if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn tempfile_path(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("{prefix}-{ns}.jsonl"));
        p
    }

    #[test]
    fn parses_simple_codex_rollout() {
        let tmp = tempfile_path("atlas_codex_simple");
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(
                f,
                r#"{{"type":"session_meta","sessionId":"abc-123","timestamp":"2026-04-18T10:00:00Z","payload":{{"cwd":"/tmp/p","model":"o4-mini"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:00Z","message":{{"role":"user","content":"first prompt"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:05:00Z","message":{{"role":"user","content":[{{"type":"text","text":"second prompt"}}]}}}}"#
            )
            .unwrap();
        }
        let parsed = parse_session_file(&tmp).unwrap().expect("session");
        assert_eq!(parsed.provider, ID_CODEX);
        assert_eq!(parsed.id, "abc-123");
        assert_eq!(parsed.turns, 2);
        assert_eq!(parsed.title, "first prompt");
        assert_eq!(parsed.last, "second prompt");
        assert_eq!(parsed.cwd.as_deref(), Some("/tmp/p"));
        assert_eq!(parsed.model.as_deref(), Some("o4-mini"));
        let _ = std::fs::remove_file(&tmp);
    }
}
