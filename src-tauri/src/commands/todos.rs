//! Todo IPC commands - owned by **D3**.

#![allow(dead_code)]

use tauri::AppHandle;

use crate::events;
use crate::storage::types::Todo;
use crate::storage::Db;

async fn emit_count(app: &AppHandle, state: &Db, project_id: &str) {
    match state.todos_list(project_id).await {
        Ok(todos) => {
            let open = todos.iter().filter(|t| !t.done).count() as u32;
            let _ = events::emit_project_updated(
                app,
                project_id,
                serde_json::json!({ "todosCount": open }),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, project_id, "todos count emit: read failed");
        }
    }
}

#[tauri::command]
pub async fn todos_list(
    state: tauri::State<'_, Db>,
    project_id: String,
) -> Result<Vec<Todo>, String> {
    state
        .todos_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

#[tauri::command]
pub async fn todos_upsert(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: String,
    todo: Todo,
) -> Result<(), String> {
    state
        .todos_upsert(&project_id, &todo)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    emit_count(&app, &state, &project_id).await;
    Ok(())
}

#[tauri::command]
pub async fn todos_delete(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: String,
    todo_id: String,
) -> Result<(), String> {
    state
        .todos_delete(&project_id, &todo_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    emit_count(&app, &state, &project_id).await;
    Ok(())
}

#[tauri::command]
pub async fn todos_toggle(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: String,
    todo_id: String,
) -> Result<(), String> {
    state
        .todos_toggle(&project_id, &todo_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    emit_count(&app, &state, &project_id).await;
    Ok(())
}
