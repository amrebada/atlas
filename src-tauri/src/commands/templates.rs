//! Template IPC commands - owned by **D5**.

#![allow(dead_code)]

use tauri::State;

use crate::storage::settings::load;
use crate::storage::templates::{
    create_project, list_all, remove_user, upsert_user, CreateProjectParams,
};
use crate::storage::types::Template;
use crate::storage::AppContext;

/// `settings.templates.list` - built-in + user-added templates. Built-ins
#[tauri::command]
pub async fn templates_list(state: State<'_, AppContext>) -> Result<Vec<Template>, String> {
    let settings = load(&state.app_data_dir)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    Ok(list_all(&settings).await)
}

/// `settings.templates.add` (upsert) - insert or replace a user template
#[tauri::command]
pub async fn templates_upsert(
    state: State<'_, AppContext>,
    template: Template,
) -> Result<(), String> {
    upsert_user(&state.app_data_dir, template)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `settings.templates.remove` - delete a user template by id.
#[tauri::command]
pub async fn templates_remove(
    state: State<'_, AppContext>,
    id: String,
) -> Result<(), String> {
    remove_user(&state.app_data_dir, &id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `templates.create_project` - scaffold a new project folder from a
#[tauri::command]
pub async fn templates_create_project(
    state: State<'_, AppContext>,
    params: CreateProjectParams,
) -> Result<String, String> {
    create_project(&state, params)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
