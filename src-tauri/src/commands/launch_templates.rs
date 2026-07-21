//! Launch template IPC commands - user-defined Claude Code session prompts.

use tauri::State;

use crate::storage::launch_templates::{list, remove, upsert};
use crate::storage::types::LaunchTemplate;
use crate::storage::AppContext;

/// `launch_templates.list` - all launch templates in stored order; empty
/// list when none exist yet.
#[tauri::command]
pub async fn launch_templates_list(
    state: State<'_, AppContext>,
) -> Result<Vec<LaunchTemplate>, String> {
    list(&state.app_data_dir)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `launch_templates.upsert` - insert or replace a launch template by id.
/// Empty id or label is rejected.
#[tauri::command]
pub async fn launch_templates_upsert(
    state: State<'_, AppContext>,
    template: LaunchTemplate,
) -> Result<(), String> {
    upsert(&state.app_data_dir, template)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `launch_templates.remove` - delete a launch template by id. Unknown ids
/// are a no-op (idempotent).
#[tauri::command]
pub async fn launch_templates_remove(
    state: State<'_, AppContext>,
    id: String,
) -> Result<(), String> {
    remove(&state.app_data_dir, &id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
