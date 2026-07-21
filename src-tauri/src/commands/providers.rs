//! providers.list                      -> ProviderInfo[]
//! providers.new_invocation(id, project_id) -> ResumeInvocation
//! claude_skills_list(project_id?)     -> ClaudeSkill[]

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::providers::claude::{claude_home, discover_skills, ClaudeSkill};
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

/// Resolve a project id to its on-disk path for skills discovery. Unlike
/// `providers_new_invocation`, an unknown id (or a lookup failure) is NOT an
/// error — the caller just skips project-scoped skills.
async fn skills_project_path(db: &Db, project_id: Option<&str>) -> Option<PathBuf> {
    let id = project_id?;
    match db.get_project(id).await {
        Ok(Some(project)) => Some(PathBuf::from(project.path)),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(
                ?err,
                "claude_skills_list: project lookup failed; skipping project scope"
            );
            None
        }
    }
}

#[tracing::instrument(level = "info", skip_all, fields(project_id = ?project_id))]
#[tauri::command]
pub async fn claude_skills_list(
    state: State<'_, AppContext>,
    project_id: Option<String>,
) -> Result<Vec<ClaudeSkill>, String> {
    let project_path = skills_project_path(&state.db, project_id.as_deref()).await;
    let skills = tauri::async_runtime::spawn_blocking(move || {
        let Some(home) = claude_home() else {
            return Vec::new();
        };
        discover_skills(&home, project_path.as_deref())
    })
    .await
    .map_err(|e| format!("join blocking: {e}"))?;
    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract: an unknown project id must not error the skills list — it
    /// silently drops the project scope.
    #[tokio::test]
    async fn unknown_project_id_skips_project_scope() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        assert_eq!(skills_project_path(&db, Some("no-such-id")).await, None);
        assert_eq!(skills_project_path(&db, None).await, None);
        Ok(())
    }
}
