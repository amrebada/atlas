//! Terminal pane IPC commands. Owned by **P6**.

use tauri::State;

use crate::storage::types::PaneId;
use crate::terminal::{OpenRequest, PaneDto, TerminalManager};

/// `terminal.open` - spawn a new PTY pane.
#[tauri::command]
pub async fn terminal_open(
    state: State<'_, TerminalManager>,
    req: OpenRequest,
) -> Result<PaneId, String> {
    state.open(req).map_err(|e| e.to_string())
}

/// `terminal.write` - send `data` (raw UTF-8 bytes, typically a key or a
#[tauri::command]
pub async fn terminal_write(
    state: State<'_, TerminalManager>,
    pane_id: PaneId,
    data: String,
) -> Result<(), String> {
    state
        .write(&pane_id, data.as_bytes())
        .map_err(|e| e.to_string())
}

/// `terminal.resize` - propagate new terminal dimensions. xterm.js fires
#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, TerminalManager>,
    pane_id: PaneId,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .resize(&pane_id, cols, rows)
        .map_err(|e| e.to_string())
}

/// `terminal.close` - kill the pane's child (SIGHUP on Unix) and drop it
#[tauri::command]
pub async fn terminal_close(
    state: State<'_, TerminalManager>,
    pane_id: PaneId,
) -> Result<(), String> {
    state.close(&pane_id).map_err(|e| e.to_string())
}

/// `terminal.list` - snapshot of every live pane, in unspecified order.
#[tauri::command]
pub async fn terminal_list(state: State<'_, TerminalManager>) -> Result<Vec<PaneDto>, String> {
    Ok(state.list())
}
