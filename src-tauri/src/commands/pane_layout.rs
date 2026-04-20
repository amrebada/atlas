//! Matches the iter-6 brief:

#![allow(dead_code)] // P6 wires these into `generate_handler![..]` at integration time.

use tauri::State;

use crate::storage::types::PaneLayout;
use crate::storage::Db;

/// `pane_layout.get` - read the persisted layout for a project.
#[tauri::command]
pub async fn pane_layout_get(
    state: State<'_, Db>,
    project_id: String,
) -> Result<Option<PaneLayout>, String> {
    state
        .pane_layout_get(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `pane_layout.save` - atomically persist the project's current
#[tauri::command]
pub async fn pane_layout_save(
    state: State<'_, Db>,
    project_id: String,
    layout: PaneLayout,
) -> Result<(), String> {
    state
        .pane_layout_save(&project_id, &layout)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `pane_layout.clear` - delete the persisted layout for a project.
#[tauri::command]
pub async fn pane_layout_clear(state: State<'_, Db>, project_id: String) -> Result<(), String> {
    state
        .pane_layout_clear(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
