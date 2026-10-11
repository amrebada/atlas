//! Terminal pane IPC commands. Owned by **P6**.

use tauri::State;

use crate::storage::types::PaneId;
use crate::storage::AppContext;
use crate::terminal::{OpenRequest, PaneDto, TerminalManager};

/// `terminal.open` - spawn a new PTY pane. `claude` launches pick up the
/// configured `--permission-mode` (Settings → Providers) here so every
/// entry point (new session, resume, launch templates, skills) agrees.
#[tauri::command]
pub async fn terminal_open(
    state: State<'_, TerminalManager>,
    ctx: State<'_, AppContext>,
    mut req: OpenRequest,
) -> Result<PaneId, String> {
    match crate::storage::settings::load(&ctx.app_data_dir).await {
        Ok(settings) => crate::providers::claude::inject_permission_mode(
            &mut req,
            settings.providers.claude_permission_mode,
        ),
        Err(e) => {
            tracing::warn!(error = %e, "terminal_open: settings load failed; no permission mode")
        }
    }
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
