//! Script CRUD + run IPC commands. Owned by **P3**.

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::scripts;
use crate::storage::types::Script;
use crate::storage::Db;

/// `scripts.list` - return the merged script set for a project.
#[tauri::command]
pub async fn scripts_list(
    state: State<'_, Db>,
    project_id: String,
) -> Result<Vec<Script>, String> {
    let project_path = resolve_project_path(&state, &project_id).await?;

    let stored = state
        .scripts_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    if !stored.is_empty() {
        return Ok(stored);
    }

    // No stored scripts yet - try to derive from sources on disk.
    let parsed = scripts::discover_scripts(&project_path).map_err(|e| e.to_string())?;
    Ok(parsed)
}

/// `scripts.upsert` - insert or replace a script row by `Script::id`.
#[tauri::command]
pub async fn scripts_upsert(
    state: State<'_, Db>,
    project_id: String,
    script: Script,
) -> Result<(), String> {
    state
        .scripts_upsert(&project_id, &script)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `scripts.delete` - remove a script by id. No-op if absent.
#[tauri::command]
pub async fn scripts_delete(
    state: State<'_, Db>,
    project_id: String,
    script_id: String,
) -> Result<(), String> {
    state
        .scripts_delete(&project_id, &script_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `scripts.run` - spawn each requested script in the project's `cwd`.
#[tauri::command]
pub async fn scripts_run(
    app: AppHandle,
    state: State<'_, Db>,
    project_id: String,
    script_ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let project_path = resolve_project_path(&state, &project_id).await?;

    // Resolve script ids → Script rows. Prefer the stored set so the user's
    let stored = state
        .scripts_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    let parsed = if stored.is_empty() {
        scripts::discover_scripts(&project_path).map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };

    let pool: Vec<&Script> = stored.iter().chain(parsed.iter()).collect();
    let mut invocation_ids = Vec::with_capacity(script_ids.len());
    for sid in &script_ids {
        let script = pool
            .iter()
            .find(|s| &s.id == sid)
            .ok_or_else(|| format!("unknown script id: {sid}"))?;
        let invocation = scripts::run(&app, &project_id, script, &project_path)
            .await
            .map_err(|e| format!("spawn {}: {e}", script.name))?;
        invocation_ids.push(invocation);
    }

    Ok(invocation_ids)
}

/// Resolve a project id to its absolute path on disk. Bubbles a friendly
async fn resolve_project_path(db: &Db, project_id: &str) -> Result<PathBuf, String> {
    let project = db
        .get_project(project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    Ok(PathBuf::from(project.path))
}
