//! Todo IPC commands.
//!
//! Beyond CRUD, every write path also (a) stamps `done_at` when a
//! todo flips into the done state and (b) recomputes the score cache
//! for any milestone whose membership might have changed. This keeps
//! the milestone success-rate dial live without needing a periodic
//! reconciler.

#![allow(dead_code)]

use chrono::Utc;
use tauri::AppHandle;

use crate::commands::milestones::recompute_milestone_score;
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

/// Recompute one or two milestones (deduped) after a todo write.
/// Errors are logged but not propagated — score cache freshness
/// shouldn't block the user's edit.
async fn refresh_milestones(state: &Db, project_id: &str, a: Option<&str>, b: Option<&str>) {
    let mut seen: Option<&str> = None;
    for id in [a, b].into_iter().flatten() {
        if seen == Some(id) {
            continue;
        }
        seen = Some(id);
        if let Err(e) = recompute_milestone_score(state, project_id, id).await {
            tracing::warn!(
                error = %e,
                project_id,
                milestone_id = id,
                "milestone score recompute after todo write failed"
            );
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
    // Snapshot the old milestone membership so we can recompute it
    // even when the user reassigns the todo to a different milestone.
    let old_milestone = state
        .todos_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .into_iter()
        .find(|t| t.id == todo.id)
        .and_then(|t| t.milestone_id);

    // Auto-stamp `done_at` when the todo flips into done state and the
    // caller didn't provide one. Keeps frontends simple.
    let mut to_write = todo.clone();
    to_write.project_id = Some(project_id.clone());
    if to_write.done && to_write.done_at.is_none() {
        to_write.done_at = Some(Utc::now().to_rfc3339());
    }
    if !to_write.done {
        to_write.done_at = None;
    }

    state
        .todos_upsert(&project_id, &to_write)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    refresh_milestones(
        &state,
        &project_id,
        old_milestone.as_deref(),
        to_write.milestone_id.as_deref(),
    )
    .await;
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
    let old_milestone = state
        .todos_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .into_iter()
        .find(|t| t.id == todo_id)
        .and_then(|t| t.milestone_id);

    state
        .todos_delete(&project_id, &todo_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    refresh_milestones(&state, &project_id, old_milestone.as_deref(), None).await;
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
    // Toggle through the upsert path so we can stamp done_at + run the
    // milestone refresh consistently. (The Db::todos_toggle helper is
    // still used by older code; we duplicate the flip here for the
    // command surface.)
    let mut t = state
        .todos_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .into_iter()
        .find(|t| t.id == todo_id)
        .ok_or_else(|| format!("todo {todo_id} not found"))?;

    t.done = !t.done;
    if t.done {
        t.done_at = Some(Utc::now().to_rfc3339());
    } else {
        t.done_at = None;
    }
    let milestone = t.milestone_id.clone();

    state
        .todos_upsert(&project_id, &t)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    refresh_milestones(&state, &project_id, milestone.as_deref(), None).await;
    emit_count(&app, &state, &project_id).await;
    Ok(())
}
