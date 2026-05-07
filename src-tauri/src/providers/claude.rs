//! Claude Code provider — reads `~/.claude/projects/<slug>/*.jsonl`.

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
use super::{ParsedSession, ResumeInvocation, SessionDetail, SessionProvider, ID_CLAUDE};

pub struct ClaudeProvider;

impl SessionProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        ID_CLAUDE
    }
    fn label(&self) -> &'static str {
        "Claude Code"
    }
    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn list_for_project(&self, project_path: &Path) -> anyhow::Result<Vec<ParsedSession>> {
        let canon = canonicalize_or_self(project_path);
        let Some(dir) = claude_dir_for_project(&canon)? else {
            return Ok(Vec::new());
        };

        let mut out: Vec<ParsedSession> = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            match parse_session_file(&path) {
                Ok(Some(p)) => out.push(p),
                Ok(None) => {}
                Err(err) => {
                    tracing::trace!(?err, path = %path.display(), "claude: parse failed");
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
            provider: ID_CLAUDE.into(),
            command: "claude".into(),
            args: vec!["--resume".into(), detail.id.clone()],
            cwd,
        }
    }

    fn new_invocation(&self, project_path: &Path) -> ResumeInvocation {
        ResumeInvocation {
            provider: ID_CLAUDE.into(),
            command: "claude".into(),
            args: Vec::new(),
            cwd: project_path.to_string_lossy().into_owned(),
        }
    }
}

// ---------- JSONL row schema ----------

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

pub fn claude_dir_for_project(project_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let canon_target = canonicalize_or_self(project_path);

    let Some(root) = claude_projects_root() else {
        return Ok(None);
    };
    if !root.exists() {
        return Ok(None);
    }

    let expected_slug = path_to_slug(&canon_target);

    let direct = root.join(&expected_slug);
    if direct.is_dir() {
        if let Some(cwd) = read_cwd_from_dir(&direct)? {
            if paths_equal(&cwd, &canon_target) {
                return Ok(Some(direct));
            }
        }
    }

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

fn claude_projects_root() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("projects"))
}

fn path_to_slug(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_start_matches('/');
    let replaced = trimmed.replace(['/', '\\'], "-");
    format!("-{replaced}")
}

fn read_cwd_from_dir(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let newest = newest_jsonl(dir)?;
    let Some(path) = newest else {
        return Ok(None);
    };
    read_cwd_from_file(&path)
}

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

    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let status = derive_status(duration_end, mtime);

    Ok(Some(ParsedSession {
        provider: ID_CLAUDE.into(),
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
        source_path: Some(path.to_path_buf()),
    }))
}

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

// ---------- Tests ----------

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
        p.push(format!("{prefix}-{ns}"));
        p
    }

    #[test]
    fn parses_minimal_session() {
        let tmp = tempfile_path("atlas_claude_min");
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
        assert_eq!(parsed.provider, ID_CLAUDE);
        assert_eq!(parsed.turns, 2);
        assert_eq!(parsed.title, "hello world");
        assert_eq!(parsed.last, "follow up?");
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.cwd.as_deref(), Some("/tmp/p"));
        assert_eq!(parsed.duration, "2m 14s");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn skips_malformed_and_tool_results() {
        let tmp = tempfile_path("atlas_claude_tolerant");
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(f, "{{ not json").unwrap();
            writeln!(
                f,
                r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-04-18T09:00:00Z"}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:00Z","cwd":"/tmp/p","message":{{"role":"user","content":"first"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:01Z","message":{{"role":"user","content":[{{"type":"tool_result","content":"ok","tool_use_id":"x"}}]}}}}"#
            )
            .unwrap();
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
}
