//! providers.list                      -> ProviderInfo[]
//! providers.new_invocation(id, project_id) -> ResumeInvocation

use std::sync::Arc;

use tauri::State;

use crate::providers::{ProviderInfo, ProvidersRegistry, ResumeInvocation};
use crate::sessions::SessionsManager;
use crate::storage::{AppContext, Db};

#[tracing::instrument(level = "info", skip_all)]
#[tauri::command]
pub async fn providers_list(
    state: State<'_, Arc<SessionsManager>>,
    ctx: State<'_, AppContext>,
) -> Result<Vec<ProviderInfo>, String> {
    let providers_settings = crate::storage::settings::load(&ctx.app_data_dir)
        .await
        .map(|s| s.providers)
        .unwrap_or_default();
    let registry: Arc<ProvidersRegistry> = state.registry().clone();
    let infos = tauri::async_runtime::spawn_blocking(move || registry.describe_all(&providers_settings))
        .await
        .map_err(|e| format!("join blocking: {e}"))?;
    Ok(infos)
}

#[tracing::instrument(level = "info", skip_all, fields(provider_id = %provider_id, project_id = %project_id))]
#[tauri::command]
pub async fn providers_new_invocation(
    state: State<'_, Arc<SessionsManager>>,
    db: State<'_, Db>,
    provider_id: String,
    project_id: String,
) -> Result<ResumeInvocation, String> {
    let project = db
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    let registry = state.registry().clone();
    let provider = registry
        .get(&provider_id)
        .ok_or_else(|| format!("unknown provider: {provider_id}"))?;
    let project_path = std::path::PathBuf::from(project.path);
    Ok(provider.new_invocation(&project_path))
}
