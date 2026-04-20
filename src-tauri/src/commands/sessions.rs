//! sessions.list(project_id)          -> Session[]

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::sessions::SessionsManager;
use crate::storage::types::Session;
use crate::storage::Db;

/// `sessions.list(project_id)` - returns every Claude Code session Atlas
#[tracing::instrument(
    level = "info",
    skip_all,
    fields(project_id = %project_id),
)]
#[tauri::command]
pub async fn sessions_list(
    state: State<'_, Arc<SessionsManager>>,
    db: State<'_, Db>,
    project_id: String,
) -> Result<Vec<Session>, String> {
    let project = db
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    // Parsing a whole JSONL can be heavy (multi-MB files observed).
    let path = std::path::PathBuf::from(project.path);
    let mgr: Arc<SessionsManager> = Arc::clone(&state);
    let start = std::time::Instant::now();
    let sessions = tauri::async_runtime::spawn_blocking(move || mgr.list_for_project(&path))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| e.to_string())?;
    tracing::info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        count = sessions.len(),
        "sessions.list complete",
    );
    Ok(sessions)
}

/// PTY spawn - the UI can already present them in a "copy command" menu.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResumeInfo {
    pub session_id: String,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
}

/// `sessions.resume_info(session_id)` - the iter-4 stub for the /// `sessions.open_in_claude`. Ret...
#[tracing::instrument(
    level = "info",
    skip_all,
    fields(session_id = %session_id),
)]
#[tauri::command]
pub async fn sessions_resume_info(
    state: State<'_, Arc<SessionsManager>>,
    session_id: String,
) -> Result<ResumeInfo, String> {
    let detail = state
        .session_detail(&session_id)
        .ok_or_else(|| format!("session not cached: {session_id} (call sessions.list first)"))?;

    let cwd = detail
        .cwd
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());

    // Sanity-check the cwd still exists on disk; otherwise the resume would
    if !Path::new(&cwd).exists() {
        tracing::warn!(cwd, session_id, "resume cwd no longer exists");
    }

    Ok(ResumeInfo {
        session_id: detail.id,
        cwd,
        command: "claude".to_string(),
        args: vec!["--resume".to_string(), session_id],
    })
}
