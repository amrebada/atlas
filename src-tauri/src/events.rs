//! Typed event emitters (Rust → TS).

// `emit_project_discovered`, `emit_project_removed`, and `emit_toast`
#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::git::GitStatus;
use crate::storage::types::Project;

/// Payload of `project:updated`. `patch` is a partial Project shape as
#[derive(Debug, Clone, Serialize)]
struct ProjectUpdatedPayload<'a> {
    id: &'a str,
    patch: Value,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectDiscoveredPayload<'a> {
    project: &'a Project,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectRemovedPayload<'a> {
    id: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitStatusPayload<'a> {
    id: &'a str,
    dirty: u32,
    ahead: u32,
    behind: u32,
    branch: &'a str,
    /// `None` for unborn / empty repos (no HEAD commit to read the author
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
struct ToastPayload<'a> {
    kind: &'a str,
    message: &'a str,
}

/// Phases of a long-running background job the UI should show progress for.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoveryPhase {
    /// Walking the filesystem looking for `.git` dirs.
    Walking,
    /// Indexed scan complete, now refreshing git status per repo.
    GitStatus,
    /// Fully done; UI can hide the progress surface.
    Done,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryProgressPayload<'a> {
    /// The watch root that started this job - used as the key when multiple
    root: &'a str,
    phase: DiscoveryPhase,
    /// Latest path being scanned / processed (for hover display).
    current: Option<&'a str>,
    /// Cumulative items processed so far in the current phase.
    found: usize,
    /// Total items for this phase, if knowable (git-status phase knows;
    total: Option<usize>,
}

/// Emit `project:updated { id, patch }`.
pub fn emit_project_updated(app: &AppHandle, id: &str, patch: Value) -> anyhow::Result<()> {
    app.emit("project:updated", ProjectUpdatedPayload { id, patch })
        .map_err(|e| anyhow::anyhow!("emit project:updated: {e}"))
}

/// Emit `project:discovered { project }` when a watcher sees a new repo.
pub fn emit_project_discovered(app: &AppHandle, project: &Project) -> anyhow::Result<()> {
    app.emit("project:discovered", ProjectDiscoveredPayload { project })
        .map_err(|e| anyhow::anyhow!("emit project:discovered: {e}"))
}

/// Emit `project:removed { id }` when a watched repo disappears.
pub fn emit_project_removed(app: &AppHandle, id: &str) -> anyhow::Result<()> {
    app.emit("project:removed", ProjectRemovedPayload { id })
        .map_err(|e| anyhow::anyhow!("emit project:removed: {e}"))
}

/// Emit `git:status { id, dirty, ahead, behind, branch, author? }`.
pub fn emit_git_status(app: &AppHandle, id: &str, status: &GitStatus) -> anyhow::Result<()> {
    app.emit(
        "git:status",
        GitStatusPayload {
            id,
            dirty: status.dirty,
            ahead: status.ahead,
            behind: status.behind,
            branch: &status.branch,
            author: status.author.as_deref(),
        },
    )
    .map_err(|e| anyhow::anyhow!("emit git:status: {e}"))
}

/// Emit `toast { kind, message }` - a lightweight user-facing notification.
pub fn emit_toast(app: &AppHandle, kind: &str, message: &str) -> anyhow::Result<()> {
    app.emit("toast", ToastPayload { kind, message })
        .map_err(|e| anyhow::anyhow!("emit toast: {e}"))
}

/// Emit `discovery:progress` - used to live-update the Scanning tooltip
pub fn emit_discovery_progress(
    app: &AppHandle,
    root: &str,
    phase: DiscoveryPhase,
    current: Option<&str>,
    found: usize,
    total: Option<usize>,
) -> anyhow::Result<()> {
    app.emit(
        "discovery:progress",
        DiscoveryProgressPayload {
            root,
            phase,
            current,
            found,
            total,
        },
    )
    .map_err(|e| anyhow::anyhow!("emit discovery:progress: {e}"))
}

// The iter-3 `script:output:<id>` / `script:exit:<id>` events were

/// Payload for `terminal:data:<pane_id>`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalDataPayload<'a> {
    /// Single stream for PTY output - the master fd doesn't distinguish
    stream: &'a str,
    chunk: &'a [u8],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalExitPayload {
    /// `Some(i32)` on normal exit, `None` when `child.wait()` itself
    code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskProgressPayload<'a> {
    project_id: &'a str,
    /// Number of entries visited so far.
    scanned: u64,
    /// Cumulative bytes summed so far.
    total_bytes: u64,
}

/// Emit `terminal:data:<pane_id>` with a single chunk from the PTY
pub fn emit_terminal_data(app: &AppHandle, pane_id: &str, chunk: &[u8]) -> anyhow::Result<()> {
    let event = format!("terminal:data:{pane_id}");
    app.emit(
        &event,
        TerminalDataPayload {
            stream: "data",
            chunk,
        },
    )
    .map_err(|e| anyhow::anyhow!("emit {event}: {e}"))
}

/// Emit `terminal:exit:<pane_id>` when the PTY child process terminates.
pub fn emit_terminal_exit(app: &AppHandle, pane_id: &str, code: Option<i32>) -> anyhow::Result<()> {
    let event = format!("terminal:exit:{pane_id}");
    app.emit(&event, TerminalExitPayload { code })
        .map_err(|e| anyhow::anyhow!("emit {event}: {e}"))
}

/// Emit `disk:progress:<project_id>` mid-scan so the UI can show a live
pub fn emit_disk_progress(
    app: &AppHandle,
    project_id: &str,
    scanned: u64,
    total_bytes: u64,
) -> anyhow::Result<()> {
    let event = format!("disk:progress:{project_id}");
    app.emit(
        &event,
        DiskProgressPayload {
            project_id,
            scanned,
            total_bytes,
        },
    )
    .map_err(|e| anyhow::anyhow!("emit {event}: {e}"))
}
