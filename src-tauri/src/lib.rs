//! Atlas - Tauri core entry point.

// clippy 1.93 flags pre-existing D1 module doc indentation in
#![allow(clippy::doc_overindented_list_items)]

use tauri::Manager;
use tracing_subscriber::EnvFilter;

mod agent;
mod commands;
mod crash;
mod editors;
mod events;
mod git;
mod ics_builder;
mod mcp;
mod metrics;
mod path_bootstrap;
mod pilot;
pub mod providers;
mod routine_engine;
mod score_engine;
mod scripts;
mod sessions;
pub mod storage;
mod terminal;
mod tray;
mod util;
mod watcher;

use providers::ProvidersRegistry;
use sessions::SessionsManager;
use std::sync::Arc;
use pilot::PilotManager;
use storage::sync::SyncWorker;
use storage::{AppContext, Db};
use terminal::TerminalManager;
use watcher::WatcherManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Per-module levels driven by RUST_LOG; sensible default for dev.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,atlas_lib=debug")),
        )
        .init();

    // Run BEFORE any PTY / child spawn so every descendant inherits the
    // real login-shell PATH. Safe to call on every platform; no-op
    // outside macOS.
    path_bootstrap::bootstrap();

    tauri::Builder::default()
        // Decorum must be registered BEFORE other plugins that touch the
        .plugin(tauri_plugin_decorum::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        // P9 - "launch at login" backing plugin. Enables/disables are
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Auto-update — verifies bundles against the public key baked
        // into `tauri.conf.json`, downloads in-place, and lets the JS
        // side trigger a relaunch via `plugin-process`.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Open SQLite index at $APP_DATA/atlas/atlas.db and hand it to
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("resolve app_data_dir: {e}"))?
                .join("atlas");

            let db = tauri::async_runtime::block_on(Db::open(&app_data))
                .map_err(|e| format!("open atlas.db at {}: {e}", app_data.display()))?;
            tracing::info!(path = %app_data.display(), "atlas db opened");

            // Arm the opt-in crash log. Must land AFTER Db::open so the
            crash::install_panic_hook(&app_data);

            let watcher = WatcherManager::new(app.handle().clone(), db.clone())
                .map_err(|e| format!("start watcher manager: {e}"))?;

            // Restore persisted watchers. Any failure re-adding a single
            let restored = tauri::async_runtime::block_on(db.list_watchers())
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "list_watchers failed; starting with none");
                    Vec::new()
                });
            for (path, depth) in restored {
                if let Err(e) = watcher.add_root(path.clone(), depth) {
                    tracing::warn!(error = %e, path = %path.display(), "restore watcher failed");
                }
            }

            // Prime git status for every indexed project so `branch`,
            watcher.refresh_all_git_status();

            // needed to read/write `settings.json` + `templates.json`
            let ctx = AppContext {
                app_data_dir: app_data.clone(),
                db: db.clone(),
            };

            // master fd + child process handle; commands under
            let terminal = TerminalManager::new(app.handle().clone());

            // drift between `.atlas/*.json` mtime and DB `updated_at`,
            let sync_worker = SyncWorker::spawn(db.clone());

            app.manage(db.clone());
            app.manage(watcher);
            let registry = Arc::new(ProvidersRegistry::with_defaults());
            app.manage(Arc::clone(&registry));
            let sessions_mgr = Arc::new(SessionsManager::new(registry));
            app.manage(Arc::clone(&sessions_mgr));
            app.manage(ctx);
            app.manage(terminal);
            app.manage(sync_worker);
            // Atlas Pilot orchestrator — drives wrapped `claude` sessions.
            app.manage(PilotManager::new(app.handle().clone()));
            // Resume any epic that was mid-run when Atlas last closed.
            // Deferred onto the async runtime so the orchestrator's
            // `tokio::spawn` has a runtime context and the app has settled.
            {
                let app_handle = app.handle().clone();
                let pilot_data_dir = app_data.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    app_handle
                        .state::<PilotManager>()
                        .resume_all(&pilot_data_dir);
                });
            }

            // Approval registry — shared between the MCP server (which
            // emits requests) and the Tauri command (which the UI calls
            // when the user clicks Approve / Reject).
            let approvals = mcp::ApprovalRegistry::new(app.handle().clone());
            app.manage(Arc::clone(&approvals));

            // Spawn the embedded MCP server (remote-control feature).
            // Phase 1: env-var gated, default-off; see `mcp::maybe_spawn`.
            mcp::maybe_spawn(
                db,
                sessions_mgr,
                app_data.clone(),
                approvals,
                app.handle().clone(),
            );

            // Phase 4.0a: outbound agent that pipes commands from a relay
            // backend into the MCP server (eventually). Off by default;
            // opted-in via ATLAS_AGENT_ENABLED + ATLAS_AGENT_TOKEN.
            agent::maybe_spawn(app.handle().clone());

            // P9 - apply persisted "launch at login" + "menu bar agent"
            let persisted = tauri::async_runtime::block_on(
                crate::storage::settings::load(&app_data),
            );
            match persisted {
                Ok(s) => {
                    apply_autolaunch_pref(app.handle(), s.general.launch_at_login);
                    if s.general.menu_bar_agent {
                        // Setup runs synchronously on a non-async thread,
                        let recents = tauri::async_runtime::block_on(async {
                            app.state::<crate::storage::Db>().recents_list(5).await
                        })
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "tray: recents_list at startup failed");
                            Vec::new()
                        });
                        if let Err(e) = tray::install(app.handle(), recents) {
                            tracing::warn!(error = %e, "tray install at startup failed");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "settings load at startup failed; skipping autostart/tray sync");
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::projects::projects_list,
            commands::projects::projects_get,
            commands::projects::projects_search,
            commands::projects::projects_seed_fixtures,
            commands::projects::projects_discover,
            commands::projects::projects_pin,
            commands::projects::projects_archive,
            commands::projects::projects_rename,
            commands::projects::projects_set_tags,
            commands::projects::projects_reorder_pinned,
            commands::projects::projects_move_to_trash,
            commands::projects::projects_repair,
            commands::projects::projects_refresh_metrics,
            commands::git::git_branch_list,
            commands::git::git_checkout,
            commands::git::git_commit,
            commands::git::git_stash,
            commands::git::git_push,
            commands::watchers::watchers_list,
            commands::watchers::watchers_add,
            commands::watchers::watchers_remove,
            commands::tags::tags_list,
            commands::tags::tags_add,
            commands::tags::tags_remove,
            commands::collections::collections_list,
            commands::collections::collections_upsert,
            commands::collections::collections_remove,
            commands::scripts::scripts_list,
            commands::scripts::scripts_upsert,
            commands::scripts::scripts_delete,
            commands::scripts::scripts_run,
            commands::scripts::scripts_run_with_env,
            commands::files::files_list,
            commands::files::files_diff,
            commands::todos::todos_list,
            commands::todos::todos_upsert,
            commands::todos::todos_delete,
            commands::todos::todos_toggle,
            commands::sessions::sessions_list,
            commands::sessions::sessions_resume_info,
            commands::providers::providers_list,
            commands::providers::providers_new_invocation,
            commands::notes::notes_list,
            commands::notes::notes_get,
            commands::notes::notes_upsert,
            commands::notes::notes_delete,
            commands::notes::notes_pin,
            commands::notes::notes_search,
            commands::clipboard::clipboard_write_text,
            commands::clipboard::clipboard_write_html,
            commands::collections::collections_members,
            commands::collections::collections_set_members,
            commands::collections::collections_create,
            commands::collections::collections_rename,
            commands::collections::collections_update_color,
            commands::collections::collections_delete,
            commands::collections::collections_reorder,
            commands::collections::collections_add_project,
            commands::collections::collections_remove_project,
            commands::collections::collections_projects,
            commands::editors::editors_detect,
            commands::editors::editors_open_project,
            commands::editors::editors_reveal,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::templates::templates_list,
            commands::templates::templates_upsert,
            commands::templates::templates_remove,
            commands::launch_templates::launch_templates_list,
            commands::launch_templates::launch_templates_upsert,
            commands::launch_templates::launch_templates_remove,
            commands::palette::palette_query,
            commands::recents::recents_push,
            commands::recents::recents_list,
            commands::terminal::terminal_open,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_close,
            commands::terminal::terminal_list,
            commands::disk::disk_scan,
            commands::disk::disk_clean,
            // system - polish pass (P-lane): sidebar home-volume row
            commands::system::system_disk_usage,
            commands::pane_layout::pane_layout_get,
            commands::pane_layout::pane_layout_save,
            commands::pane_layout::pane_layout_clear,
            commands::templates::templates_create_project,
            // Planner feature — P1 schema only; handlers stubbed until
            // their respective phases (P2 milestones, P3 routines, P4 today).
            commands::milestones::milestones_list,
            commands::milestones::milestones_create,
            commands::milestones::milestones_update,
            commands::milestones::milestones_extend,
            commands::milestones::milestones_set_status,
            commands::milestones::milestones_delete,
            commands::routines::routines_list,
            commands::routines::routines_create,
            commands::routines::routines_update,
            commands::routines::routines_delete,
            commands::routines::routines_pause,
            commands::routines::routines_instances,
            commands::routines::routines_complete_instance,
            commands::routines::routines_skip_instance,
            commands::routines::routines_materialize,
            commands::routines::routines_projected_completion,
            commands::planner::planner_today,
            commands::planner::planner_session_start,
            commands::planner::planner_pause_all,
            commands::planner::planner_score_summary,
            commands::planner::planner_extension_log,
            commands::timeline::timeline_config_get,
            commands::timeline::timeline_pin_project,
            commands::timeline::timeline_unpin_project,
            commands::timeline::timeline_set_range,
            commands::timeline::timeline_query,
            commands::ics::ics_export_all,
            commands::ics::ics_export_project,
            commands::ics::ics_reveal_dir,
            commands::mcp::mcp_approval_resolve,
            commands::agent::agent_pairing_info,
            commands::agent::agent_pair_envelope,
            commands::pilot::pilot_create,
            commands::pilot::pilot_list,
            commands::pilot::pilot_get,
            commands::pilot::pilot_history,
            commands::pilot::pilot_transcript,
            commands::pilot::pilot_artifact_read,
            commands::pilot::pilot_artifact_write,
            commands::pilot::pilot_approve_gate,
            commands::pilot::pilot_send_message,
            commands::pilot::pilot_pause,
            commands::pilot::pilot_resume,
            commands::pilot::pilot_interrupt,
            commands::pilot::pilot_start_planning,
            commands::pilot::pilot_start_epic,
            commands::pilot::pilot_resume_run,
            commands::pilot::pilot_open_window,
            commands::pilot::pilot_install_skill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Atlas");
}

/// Drive the `tauri-plugin-autostart` manager toward the desired state.
pub(crate) fn apply_autolaunch_pref(app: &tauri::AppHandle, desired: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    let current = match mgr.is_enabled() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "autolaunch.is_enabled failed; skipping sync");
            return;
        }
    };
    if current == desired {
        return;
    }
    let res = if desired { mgr.enable() } else { mgr.disable() };
    if let Err(e) = res {
        tracing::warn!(error = %e, desired, "autolaunch toggle failed");
    } else {
        tracing::info!(enabled = desired, "autolaunch state updated");
    }
}
