//! Recents ring buffer IPC commands - owned by **D7**.

use tauri::State;

use crate::storage::types::Project;
use crate::storage::Db;

/// Default recents cap. Matches the hard cap inside `Db::recents_push`'s
const DEFAULT_RECENTS_LIMIT: u32 = 20;

/// `recents.push` - record that the user opened `project_id`. Idempotent
#[tauri::command]
pub async fn recents_push(
    state: State<'_, Db>,
    project_id: String,
) -> Result<(), String> {
    state
        .recents_push(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `recents.list` - N most recent projects, newest first. `limit` of 0
#[tauri::command]
pub async fn recents_list(
    state: State<'_, Db>,
    limit: Option<u32>,
) -> Result<Vec<Project>, String> {
    let lim = match limit {
        Some(0) | None => DEFAULT_RECENTS_LIMIT,
        Some(n) => n,
    };
    state
        .recents_list(lim)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
