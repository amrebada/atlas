//! Atlas - Tauri core entry point.

// clippy 1.93 flags pre-existing D1 module doc indentation in
#![allow(clippy::doc_overindented_list_items)]

use tauri::Manager;
use tracing_subscriber::EnvFilter;

mod commands;
mod crash;
mod editors;
mod events;
mod git;
mod metrics;
mod scripts;
mod sessions;
pub mod storage;
mod terminal;
mod tray;
mod util;
mod watcher;

use sessions::SessionsManager;
use std::sync::Arc;
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

            app.manage(db);
            app.manage(watcher);
            app.manage(Arc::new(SessionsManager::new()));
            app.manage(ctx);
            app.manage(terminal);
            app.manage(sync_worker);

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
            commands::files::files_list,
            commands::todos::todos_list,
            commands::todos::todos_upsert,
            commands::todos::todos_delete,
            commands::todos::todos_toggle,
            commands::sessions::sessions_list,
            commands::sessions::sessions_resume_info,
            commands::notes::notes_list,
            commands::notes::notes_get,
            commands::notes::notes_upsert,
            commands::notes::notes_delete,
            commands::notes::notes_pin,
            commands::notes::notes_search,
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
