//! A `PtyPane` owns everything a single pseudo-terminal needs:

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::storage::types::{PaneId, PaneKind, PaneStatus};

/// Request shape for `TerminalManager::open` (and the `terminal.open` IPC
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub kind: PaneKind,
    pub cwd: PathBuf,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub script_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
}

/// Flat, IPC-safe snapshot of a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneDto {
    pub id: PaneId,
    pub kind: PaneKind,
    pub title: String,
    pub status: PaneStatus,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// One entry in the manager's pane map.
pub struct PtyPane {
    pub id: PaneId,
    pub kind: PaneKind,
    pub title: String,
    pub cwd: PathBuf,
    pub branch: Option<String>,
    pub script_id: Option<String>,
    pub session_id: Option<String>,

    /// Current derived status; the ticker flips `Active` → `Idle` after
    pub status: PaneStatus,

    /// Last time the reader task pushed a chunk. Used by the idle ticker.
    pub last_output_at: Instant,

    /// The `portable_pty` master handle - owned so we can `resize` it.
    pub master: Box<dyn portable_pty::MasterPty + Send>,

    /// Writer taken from the master; drains stdin from `terminal.write`.
    pub writer: Box<dyn std::io::Write + Send>,

    /// Child killer; `close()` sends SIGHUP via this handle.
    pub killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,

    /// Background reader task; aborted on `close()` to tear down cleanly.
    pub reader_task: tokio::task::JoinHandle<()>,

    /// Ticker that degrades `Active` → `Idle` on silence. Aborted on close.
    pub ticker_task: tokio::task::JoinHandle<()>,
}

impl PtyPane {
    /// Project the pane into its IPC-safe snapshot.
    pub fn to_dto(&self) -> PaneDto {
        PaneDto {
            id: self.id.clone(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            status: self.status.clone(),
            cwd: self.cwd.to_string_lossy().into_owned(),
            branch: self.branch.clone(),
            script_id: self.script_id.clone(),
            session_id: self.session_id.clone(),
        }
    }
}
