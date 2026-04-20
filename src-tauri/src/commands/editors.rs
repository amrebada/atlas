//! editors.detect()                            -> EditorEntry[]

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::editors::{self, EditorEntry};
use crate::events;
use crate::storage::Db;

/// `editors.detect` - return every editor Atlas knows about, with
#[tauri::command]
pub async fn editors_detect() -> Result<Vec<EditorEntry>, String> {
    // Detection is cheap but touches the filesystem. Hop onto the
    tauri::async_runtime::spawn_blocking(editors::detect_installed)
        .await
        .map_err(|e| format!("detect join: {e}"))
}

/// `editors.open_project` - launch the chosen editor against the
#[tauri::command]
pub async fn editors_open_project(
    app: AppHandle,
    state: State<'_, Db>,
    project_id: String,
    editor_id: Option<String>,
) -> Result<(), String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    // Resolve the editor. TODO: when the settings store lands, read
    let wanted_id = editor_id.unwrap_or_else(|| "vscode".to_string());

    let detected = editors::detect_installed();
    let chosen = detected
        .iter()
        .find(|e| e.id == wanted_id)
        .ok_or_else(|| format!("unknown editor id: {wanted_id}"))?;

    if !chosen.present {
        return Err(format!(
            "{} is not installed on this machine",
            chosen.name
        ));
    }

    let path = PathBuf::from(&project.path);
    let chosen_clone = chosen.clone();

    // `editors::launch` spawns detached but still does a few
    tauri::async_runtime::spawn_blocking(move || editors::launch(&chosen_clone, &path))
        .await
        .map_err(|e| format!("launch join: {e}"))?
        .map_err(|e: anyhow::Error| e.to_string())?;

    // Bump `last_opened` + recents so the list view's "opened" column
    if let Err(e) = state.recents_push(&project_id).await {
        tracing::warn!(error = %e, project_id = %project_id, "recents_push after editor open failed");
    } else {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = events::emit_project_updated(
            &app,
            &project_id,
            serde_json::json!({ "lastOpened": now }),
        );
    }

    Ok(())
}

/// `editors.reveal` - show the project folder in the platform file
#[tauri::command]
pub async fn editors_reveal(state: State<'_, Db>, project_id: String) -> Result<(), String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    let path = PathBuf::from(&project.path);
    tauri::async_runtime::spawn_blocking(move || editors::reveal(&path))
        .await
        .map_err(|e| format!("reveal join: {e}"))?
        .map_err(|e: anyhow::Error| e.to_string())
}
