//! Script CRUD + run IPC commands. Owned by **P3**.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::scripts;
use crate::storage::types::Script;
use crate::storage::Db;

/// Per-script invocation payload for `scripts_run_with_env`. Lets the
/// frontend supply user-edited env values (from the run-env modal) on
/// top of the script's stored defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInvocation {
    pub script_id: String,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// `scripts.list` - return the merged script set for a project.
#[tauri::command]
pub async fn scripts_list(state: State<'_, Db>, project_id: String) -> Result<Vec<Script>, String> {
    let project_path = resolve_project_path(&state, &project_id).await?;

    let stored = state
        .scripts_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    let parsed = scripts::discover_scripts(&project_path).map_err(|e| e.to_string())?;
    Ok(merge_scripts(stored, parsed))
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

    // Resolve script ids → Script rows. Stored entries override discovered
    // ones by id so user edits to auto-detected scripts are honored.
    let stored = state
        .scripts_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    let parsed = scripts::discover_scripts(&project_path).map_err(|e| e.to_string())?;
    let pool = merge_scripts(stored, parsed);

    let mut invocation_ids = Vec::with_capacity(script_ids.len());
    for sid in &script_ids {
        let script = pool
            .iter()
            .find(|s| &s.id == sid)
            .ok_or_else(|| format!("unknown script id: {sid}"))?;
        let env = script
            .env_defaults
            .iter()
            .map(|v| (v.key.clone(), v.default.clone()))
            .collect();
        let invocation = scripts::run(&app, &project_id, script, &project_path, env)
            .await
            .map_err(|e| format!("spawn {}: {e}", script.name))?;
        invocation_ids.push(invocation);
    }

    Ok(invocation_ids)
}

/// `scripts.runWithEnv` - like `scripts_run` but each invocation carries
/// a user-supplied env map that overrides the script's stored defaults.
#[tauri::command]
pub async fn scripts_run_with_env(
    app: AppHandle,
    state: State<'_, Db>,
    project_id: String,
    invocations: Vec<ScriptInvocation>,
) -> Result<Vec<String>, String> {
    let project_path = resolve_project_path(&state, &project_id).await?;

    let stored = state
        .scripts_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    let parsed = scripts::discover_scripts(&project_path).map_err(|e| e.to_string())?;
    let pool = merge_scripts(stored, parsed);

    let mut invocation_ids = Vec::with_capacity(invocations.len());
    for inv in &invocations {
        let script = pool
            .iter()
            .find(|s| s.id == inv.script_id)
            .ok_or_else(|| format!("unknown script id: {}", inv.script_id))?;
        let id = scripts::run(&app, &project_id, script, &project_path, inv.env.clone())
            .await
            .map_err(|e| format!("spawn {}: {e}", script.name))?;
        invocation_ids.push(id);
    }

    Ok(invocation_ids)
}

/// Merge stored scripts with auto-discovered ones. Stored entries take
/// precedence by id (so a user's edits to an auto-detected script stick),
/// and discovered scripts not present in `stored` are appended after.
fn merge_scripts(stored: Vec<Script>, parsed: Vec<Script>) -> Vec<Script> {
    let mut out = stored;
    for s in parsed {
        if !out.iter().any(|existing| existing.id == s.id) {
            out.push(s);
        }
    }
    out
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
