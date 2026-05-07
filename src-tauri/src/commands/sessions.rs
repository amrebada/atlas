//! sessions.list(project_id, provider?)  -> Session[]
//! sessions.resume_info(session_id, provider?) -> ResumeInfo

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::sessions::SessionsManager;
use crate::storage::types::Session;
use crate::storage::{AppContext, Db};

#[tracing::instrument(
    level = "info",
    skip_all,
    fields(project_id = %project_id, provider = provider.as_deref().unwrap_or("*")),
)]
#[tauri::command]
pub async fn sessions_list(
    state: State<'_, Arc<SessionsManager>>,
    db: State<'_, Db>,
    ctx: State<'_, AppContext>,
    project_id: String,
    provider: Option<String>,
) -> Result<Vec<Session>, String> {
    let project = db
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    let providers_settings = crate::storage::settings::load(&ctx.app_data_dir)
        .await
        .map(|s| s.providers)
        .unwrap_or_default();

    let path = std::path::PathBuf::from(project.path);
    let mgr: Arc<SessionsManager> = Arc::clone(&state);
    let provider_filter = provider.clone();
    let start = std::time::Instant::now();
    let sessions = tauri::async_runtime::spawn_blocking(move || {
        mgr.list_for_project(&path, &providers_settings, provider_filter.as_deref())
    })
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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResumeInfo {
    pub session_id: String,
    pub provider: String,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
}

#[tracing::instrument(
    level = "info",
    skip_all,
    fields(session_id = %session_id, provider = provider.as_deref().unwrap_or("*")),
)]
#[tauri::command]
pub async fn sessions_resume_info(
    state: State<'_, Arc<SessionsManager>>,
    session_id: String,
    provider: Option<String>,
) -> Result<ResumeInfo, String> {
    // Prefer the explicit (provider, id) lookup when the caller knows which
    // provider the session belongs to; fall back to a scan otherwise.
    let detail = match provider.as_deref() {
        Some(p) => state.session_detail_for(p, &session_id),
        None => state.session_detail(&session_id),
    }
    .ok_or_else(|| format!("session not cached: {session_id} (call sessions.list first)"))?;

    let registry = state.registry().clone();
    let provider_impl = registry
        .get(&detail.provider)
        .ok_or_else(|| format!("unknown provider for session: {}", detail.provider))?;

    let invocation = provider_impl.resume_invocation(&detail);

    if !Path::new(&invocation.cwd).exists() {
        tracing::warn!(cwd = %invocation.cwd, session_id, "resume cwd no longer exists");
    }

    Ok(ResumeInfo {
        session_id: detail.id,
        provider: invocation.provider,
        cwd: invocation.cwd,
        command: invocation.command,
        args: invocation.args,
    })
}
