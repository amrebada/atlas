//! Watcher IPC commands.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;

use crate::events;
use crate::events::DiscoveryPhase;
use crate::storage::Db;
use crate::watcher::WatcherManager;

const PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

/// IPC-shaped mirror of `types::WatchRoot`. We don't share the storage
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchRootDto {
    pub path: String,
    pub depth: u8,
    pub repo_count: u32,
}

/// IPC `watchers_list` → spec `settings.watchers.list`.
#[tauri::command]
pub async fn watchers_list(
    state: tauri::State<'_, WatcherManager>,
    db: tauri::State<'_, Db>,
) -> Result<Vec<WatchRootDto>, String> {
    let roots = state.list_roots();
    let mut out = Vec::with_capacity(roots.len());
    for (path, depth) in roots {
        let count = db
            .count_projects_under(&path)
            .await
            .map_err(|e: anyhow::Error| e.to_string())?;
        out.push(WatchRootDto {
            path: path.to_string_lossy().into_owned(),
            depth,
            repo_count: count,
        });
    }
    Ok(out)
}

/// IPC `watchers_add` → spec `settings.watchers.add`.
#[tauri::command]
pub async fn watchers_add(
    app: AppHandle,
    state: tauri::State<'_, WatcherManager>,
    db: tauri::State<'_, Db>,
    path: String,
    depth: Option<u8>,
) -> Result<(), String> {
    let depth = depth.unwrap_or(3);
    let root = PathBuf::from(&path);
    let display_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());

    // User-visible: scan starting. Toast is the short summary; the
    let _ = events::emit_toast(
        &app,
        "info",
        &format!("Scanning {display_name} for git repos…"),
    );
    let _ = events::emit_discovery_progress(
        &app,
        &path,
        DiscoveryPhase::Walking,
        None,
        0,
        None,
    );

    state
        .add_root(root.clone(), depth)
        .map_err(|e| format!("add watcher {path}: {e}"))?;

    db.add_watcher(&root, depth)
        .await
        .map_err(|e: anyhow::Error| format!("persist watcher: {e}"))?;

    // ── Discovery, split in two stages ────────────────────────────────────
    let app_for_scan = app.clone();
    let root_for_scan = root.clone();
    let root_label_for_scan = path.clone();
    let scan_result = tauri::async_runtime::spawn_blocking(move || {
        use crate::storage::discovery::scan_root_with_progress;

        // Throttled progress. The first real callback emits immediately
        let mut last_tick: Option<Instant> = None;
        let mut total_ticks = 0usize;
        let mut emitted_ticks = 0usize;
        let result = scan_root_with_progress(
            &root_for_scan,
            depth,
            |current: &Path, found: usize| {
                total_ticks += 1;
                let now = Instant::now();
                let should_emit = match last_tick {
                    None => true,
                    Some(t) => now.duration_since(t) >= PROGRESS_THROTTLE,
                };
                if !should_emit {
                    return;
                }
                last_tick = Some(now);
                emitted_ticks += 1;
                let current_str = current.to_string_lossy();
                if let Err(e) = events::emit_discovery_progress(
                    &app_for_scan,
                    &root_label_for_scan,
                    DiscoveryPhase::Walking,
                    Some(&current_str),
                    found,
                    None,
                ) {
                    tracing::warn!(error = %e, "emit discovery:progress failed");
                }
            },
        );
        tracing::info!(total_ticks, emitted_ticks, "discovery walker finished");
        result
    })
    .await
    .map_err(|e| format!("join blocking: {e}"))?;

    let repos = match scan_result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), path = %path, "initial discovery failed");
            let _ = events::emit_toast(
                &app,
                "error",
                &format!("Discovery failed in {display_name}: {e}"),
            );
            let _ = events::emit_discovery_progress(
                &app,
                &path,
                DiscoveryPhase::Done,
                None,
                0,
                None,
            );
            return Ok(());
        }
    };

    // Stage B - upsert each discovered repo. Done on the async task so
    let mut new_ids: Vec<String> = Vec::new();
    let new_count = {
        let mut new = 0usize;
        for repo in &repos {
            let id = crate::storage::db::project_id_for_path(&repo.path);
            let existed = db.get_project(&id).await.ok().flatten().is_some();
            if let Err(e) = db.upsert_discovered(repo).await {
                tracing::warn!(error = %e, path = %repo.path.display(), "upsert_discovered failed");
                continue;
            }
            if !existed {
                new += 1;
                new_ids.push(id);
            }
        }
        tracing::info!(path = %path, depth, total = repos.len(), new, "discovery upsert complete");
        new
    };

    // added. Each refresh runs the walker on the blocking pool and emits
    for id in new_ids {
        let db_clone = (*db).clone();
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::commands::projects::spawn_metrics_refresh(db_clone, app_clone, id).await;
        });
    }

    let total = db
        .count_projects_under(&root)
        .await
        .map_err(|e: anyhow::Error| format!("count projects: {e}"))?;

    // User-visible: done scanning. The tooltip flips to the git-status
    let _ = events::emit_toast(
        &app,
        "success",
        &format!(
            "Found {total} project{} in {display_name} ({new_count} new). Reading git status…",
            if total == 1 { "" } else { "s" }
        ),
    );

    state.refresh_git_status_under(root);

    Ok(())
}

/// IPC `watchers_remove` → spec `settings.watchers.remove`.
#[tauri::command]
pub async fn watchers_remove(
    state: tauri::State<'_, WatcherManager>,
    db: tauri::State<'_, Db>,
    path: String,
) -> Result<(), String> {
    let root = Path::new(&path);
    state
        .remove_root(root)
        .map_err(|e| format!("remove watcher {path}: {e}"))?;
    db.remove_watcher(root)
        .await
        .map_err(|e: anyhow::Error| format!("persist remove: {e}"))?;
    Ok(())
}
