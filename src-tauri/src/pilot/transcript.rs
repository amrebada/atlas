//! Reading and interpreting a Claude Code session transcript.
//!
//! Atlas observes a wrapped `claude` session **only** by tailing its JSONL
//! transcript (`~/.claude/projects/<slug>/<session-id>.jsonl`). This module
//! turns that append-only file into the signals the pilot orchestrator needs:
//!
//!   * **sentinels** — `<<ATLAS:*>>` control lines the atlas skill emits,
//!   * **todos** — `TodoWrite` tool calls (the epic task list, mirrored),
//!   * **messages** — assistant / user text, for the per-epic chat thread.
//!
//! Activity vs. idle is left to the orchestrator: any event from a poll is
//! activity; silence past a threshold is idle. This module stays a pure
//! file → events transform so it is trivially testable.

#![allow(dead_code)]

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

// =====================================================================
// Sentinels.
// =====================================================================

/// A control sentinel the atlas skill emits as the whole final line of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    GateReqs,
    GatePrd,
    GateEpics,
    TaskDone,
    EpicDone,
    NeedsInput,
}

impl Sentinel {
    /// Match one already-trimmed line against the sentinel vocabulary.
    pub fn from_line(line: &str) -> Option<Sentinel> {
        match line {
            "<<ATLAS:GATE:REQS>>" => Some(Sentinel::GateReqs),
            "<<ATLAS:GATE:PRD>>" => Some(Sentinel::GatePrd),
            "<<ATLAS:GATE:EPICS>>" => Some(Sentinel::GateEpics),
            "<<ATLAS:TASK_DONE>>" => Some(Sentinel::TaskDone),
            "<<ATLAS:EPIC_DONE>>" => Some(Sentinel::EpicDone),
            "<<ATLAS:NEEDS_INPUT>>" => Some(Sentinel::NeedsInput),
            _ => None,
        }
    }

    /// Detect a sentinel as the **last non-empty line** of an assistant
    /// message. The skill contract requires a sentinel to stand alone on the
    /// final line — anything else (a sentinel quoted mid-paragraph, fenced in
    /// a code block) is intentionally not matched.
    pub fn from_message(text: &str) -> Option<Sentinel> {
        let last = text.lines().rev().find(|l| !l.trim().is_empty())?;
        Sentinel::from_line(last.trim())
    }

    /// A terminal sentinel ends the turn and the orchestrator must react.
    pub fn is_terminal(self) -> bool {
        true
    }
}

// =====================================================================
// Todos (mirrored from the epic task list via the TodoWrite tool).
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    fn parse(s: &str) -> TodoStatus {
        match s {
            "in_progress" => TodoStatus::InProgress,
            "completed" => TodoStatus::Completed,
            _ => TodoStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

/// Progress derived from a TodoWrite snapshot: `(completed, total)`.
pub fn todo_progress(todos: &[TodoItem]) -> (usize, usize) {
    let done = todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    (done, todos.len())
}

// =====================================================================
// Interpreted events.
// =====================================================================

/// An interpreted transcript row. One JSONL row can yield more than one
/// event (an assistant row may carry both text and a TodoWrite call).
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptEvent {
    /// An assistant turn's combined text, with its last-line sentinel if any.
    AssistantTurn {
        text: String,
        sentinel: Option<Sentinel>,
    },
    /// A `TodoWrite` tool call — the full todo list at that point.
    Todos(Vec<TodoItem>),
    /// A real user message (tool-result rows are excluded).
    UserMessage(String),
}

// ---------- JSONL row schema (only the fields we read) ----------

#[derive(Debug, Deserialize)]
struct Row {
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<Msg>,
}

#[derive(Debug, Deserialize)]
struct Msg {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<Value>,
}

/// Interpret one parsed row, pushing any events it carries onto `out`.
fn interpret_row(row: &Row, out: &mut Vec<TranscriptEvent>) {
    let Some(msg) = &row.message else { return };
    match row.kind.as_deref() {
        Some("assistant") => {
            if let Some(todos) = extract_todos(msg.content.as_ref()) {
                out.push(TranscriptEvent::Todos(todos));
            }
            if let Some(text) = extract_text(msg.content.as_ref()) {
                let sentinel = Sentinel::from_message(&text);
                out.push(TranscriptEvent::AssistantTurn { text, sentinel });
            }
        }
        Some("user") => {
            // Skip synthetic user rows that only carry tool results.
            if msg.role.as_deref().unwrap_or("user") != "user" {
                return;
            }
            if let Some(text) = extract_text(msg.content.as_ref()) {
                out.push(TranscriptEvent::UserMessage(text));
            }
        }
        _ => {}
    }
}

/// Join all `text` blocks of a message's content. `None` if there is no text.
fn extract_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(items) => {
            let mut buf = String::new();
            for item in items {
                let Some(obj) = item.as_object() else { continue };
                if obj.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = obj.get("text").and_then(Value::as_str) {
                        if !buf.is_empty() {
                            buf.push('\n');
                        }
                        buf.push_str(t);
                    }
                }
            }
            (!buf.is_empty()).then_some(buf)
        }
        _ => None,
    }
}

/// Pull the todo list out of a `TodoWrite` tool-use block, if present.
fn extract_todos(content: Option<&Value>) -> Option<Vec<TodoItem>> {
    let Value::Array(items) = content? else {
        return None;
    };
    for item in items {
        let obj = item.as_object()?;
        if obj.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        if obj.get("name").and_then(Value::as_str) != Some("TodoWrite") {
            continue;
        }
        let todos = obj.get("input")?.get("todos")?.as_array()?;
        let parsed = todos
            .iter()
            .filter_map(|t| {
                let o = t.as_object()?;
                let content = o.get("content").and_then(Value::as_str)?.to_string();
                let status = o
                    .get("status")
                    .and_then(Value::as_str)
                    .map(TodoStatus::parse)
                    .unwrap_or(TodoStatus::Pending);
                Some(TodoItem { content, status })
            })
            .collect();
        return Some(parsed);
    }
    None
}

// =====================================================================
// Incremental reader.
// =====================================================================

/// Tails one session transcript file, yielding only newly-appended rows on
/// each `poll`. Cheap to poll on a timer.
pub struct TranscriptReader {
    path: PathBuf,
    /// Byte offset just past the last fully-consumed line.
    offset: u64,
}

impl TranscriptReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read every line appended since the previous call and return the
    /// interpreted events in file order. A missing or unchanged file yields
    /// an empty vec. A trailing partial line (no newline yet) is left for the
    /// next poll so a half-flushed row is never parsed.
    pub fn poll(&mut self) -> anyhow::Result<Vec<TranscriptEvent>> {
        let mut file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(anyhow::anyhow!("open {}: {e}", self.path.display())),
        };

        // Guard against a replaced/truncated file: restart from the top.
        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
        }

        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        if buf.is_empty() {
            return Ok(Vec::new());
        }

        // Only consume up to the last complete line.
        let Some(last_nl) = buf.rfind('\n') else {
            return Ok(Vec::new());
        };
        let complete = &buf[..=last_nl];
        self.offset += complete.len() as u64;

        let mut events = Vec::new();
        for line in complete.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Row>(line) {
                Ok(row) => interpret_row(&row, &mut events),
                Err(err) => tracing::trace!(?err, "pilot: skipping malformed transcript line"),
            }
        }
        Ok(events)
    }
}

// =====================================================================
// Session-file discovery.
// =====================================================================

/// Every `*.jsonl` session file currently in a Claude project slug dir.
/// The orchestrator snapshots this before spawning `claude`, then diffs
/// after, to learn the new session's id (its file stem).
pub fn session_files(slug_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(slug_dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    out
}

/// The transcript path for a known session id within a slug dir.
pub fn session_file(slug_dir: &Path, session_id: &str) -> PathBuf {
    slug_dir.join(format!("{session_id}.jsonl"))
}

// =====================================================================
// Tests.
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn sentinel_only_matches_last_line_alone() {
        assert_eq!(
            Sentinel::from_message("doing the work\n<<ATLAS:TASK_DONE>>"),
            Some(Sentinel::TaskDone)
        );
        // Trailing blank lines are ignored.
        assert_eq!(
            Sentinel::from_message("done\n<<ATLAS:EPIC_DONE>>\n\n  "),
            Some(Sentinel::EpicDone)
        );
        // Sentinel mid-paragraph does not count.
        assert_eq!(
            Sentinel::from_message("I will emit <<ATLAS:TASK_DONE>> now\nok"),
            None
        );
        // Unknown token.
        assert_eq!(Sentinel::from_message("<<ATLAS:BOGUS>>"), None);
    }

    #[test]
    fn extracts_todowrite_snapshot() {
        let content = serde_json::json!([
            { "type": "text", "text": "updating todos" },
            { "type": "tool_use", "name": "TodoWrite", "id": "t1", "input": { "todos": [
                { "content": "task a", "status": "completed", "activeForm": "doing a" },
                { "content": "task b", "status": "in_progress", "activeForm": "doing b" },
                { "content": "task c", "status": "pending", "activeForm": "doing c" }
            ] } }
        ]);
        let todos = extract_todos(Some(&content)).expect("todos");
        assert_eq!(todos.len(), 3);
        assert_eq!(todo_progress(&todos), (1, 3));
        assert_eq!(todos[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn reader_yields_only_new_complete_lines() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("session.jsonl");

        // Missing file → empty, no error.
        let mut reader = TranscriptReader::new(&path);
        assert!(reader.poll()?.is_empty());

        let assistant = |text: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":{}}}]}}}}"#,
                serde_json::to_string(text).unwrap()
            )
        };

        // First write: one complete line + a partial line (no newline).
        {
            let mut f = fs::File::create(&path)?;
            writeln!(f, "{}", assistant("first turn"))?;
            write!(f, "{}", &assistant("partial")[..20])?;
        }
        let events = reader.poll()?;
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], TranscriptEvent::AssistantTurn { text, .. } if text == "first turn"));

        // Complete the partial line and append a sentinel turn.
        {
            let mut f = fs::OpenOptions::new().append(true).open(&path)?;
            // Overwriting won't work with append; instead just add fresh rows.
            writeln!(f)?;
            writeln!(f, "{}", assistant("second turn\n<<ATLAS:TASK_DONE>>"))?;
        }
        let events = reader.poll()?;
        // The dangling partial line is now terminated; it parses (malformed
        // JSON → skipped) and the new sentinel turn is delivered.
        let sentinel_turn = events.iter().find_map(|e| match e {
            TranscriptEvent::AssistantTurn { sentinel, .. } => *sentinel,
            _ => None,
        });
        assert_eq!(sentinel_turn, Some(Sentinel::TaskDone));

        // Nothing new → empty.
        assert!(reader.poll()?.is_empty());
        Ok(())
    }

    #[test]
    fn session_file_discovery() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        fs::File::create(dir.path().join("aaa.jsonl"))?;
        fs::File::create(dir.path().join("bbb.jsonl"))?;
        fs::File::create(dir.path().join("notes.txt"))?;
        let mut found: Vec<_> = session_files(dir.path())
            .into_iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        found.sort();
        assert_eq!(found, vec!["aaa.jsonl", "bbb.jsonl"]);
        assert_eq!(
            session_file(dir.path(), "xyz"),
            dir.path().join("xyz.jsonl")
        );
        Ok(())
    }
}
