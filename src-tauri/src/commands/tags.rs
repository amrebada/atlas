//! Tag IPC commands - owned by **D2**.

use crate::storage::Db;

/// `tags.list` - distinct tags across all projects, alphabetical.
#[tauri::command]
pub async fn tags_list(state: tauri::State<'_, Db>) -> Result<Vec<String>, String> {
    state
        .list_tags()
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `tags.add` - attach `tag` to `project_id`. Idempotent (UNIQUE
#[tauri::command]
pub async fn tags_add(
    state: tauri::State<'_, Db>,
    project_id: String,
    tag: String,
) -> Result<(), String> {
    state
        .add_tag(&project_id, &tag)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `tags.remove` - detach `tag` from `project_id`. No-op if absent.
#[tauri::command]
pub async fn tags_remove(
    state: tauri::State<'_, Db>,
    project_id: String,
    tag: String,
) -> Result<(), String> {
    state
        .remove_tag(&project_id, &tag)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
