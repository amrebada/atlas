//! The iter-3 non-PTY implementation streamed `script:output:<invocation_id>`

use std::path::Path;

use tauri::AppHandle;
use tauri::Manager;

use crate::storage::types::{PaneKind, Script};
use crate::terminal::{OpenRequest, TerminalManager};

/// Spawn `script` inside a PTY pane rooted at `cwd` and return the
/// resulting pane id. `env` is merged on top of the inherited env: any
/// pair here overrides the parent process value for this invocation only.
pub async fn run(
    app: &AppHandle,
    project_id: &str,
    script: &Script,
    cwd: &Path,
    env: Vec<(String, String)>,
) -> anyhow::Result<String> {
    let manager = app
        .try_state::<TerminalManager>()
        .ok_or_else(|| anyhow::anyhow!("TerminalManager state not registered"))?;

    tracing::info!(
        project = %project_id,
        script = %script.name,
        cwd = %cwd.display(),
        env_count = env.len(),
        "spawning script pane"
    );

    // `sh -c "<cmd>"` preserves the pipelines / redirects the user
    let shell = TerminalManager::default_shell();

    let req = OpenRequest {
        kind: PaneKind::Script,
        cwd: cwd.to_path_buf(),
        command: Some(shell),
        args: vec!["-c".to_string(), script.cmd.clone()],
        env,
        title: Some(script.name.clone()),
        branch: None,
        script_id: Some(script.id.clone()),
        session_id: None,
        cols: None,
        rows: None,
    };

    manager
        .open(req)
        .map_err(|e| anyhow::anyhow!("open script pane {}: {e}", script.name))
}

#[cfg(test)]
mod tests {
    // The script runner is now a thin forward to `TerminalManager::open`;
}
