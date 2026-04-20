//! Settings IPC commands - owned by **D5**.

#![allow(dead_code)] // P5 wires these into `generate_handler![..]`; this lane registers the names

use tauri::{AppHandle, Manager, State};

use crate::storage::settings::{apply_patch, load};
use crate::storage::types::Settings;
use crate::storage::AppContext;

/// `settings.get` - return the current `Settings`, lazily materializing
#[tauri::command]
pub async fn settings_get(state: State<'_, AppContext>) -> Result<Settings, String> {
    load(&state.app_data_dir)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `settings.set` - shallow-merge a partial patch into the current
#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppContext>,
    patch: serde_json::Value,
) -> Result<Settings, String> {
    // Snapshot the pre-patch general settings so we can detect the
    let before = load(&state.app_data_dir)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    let after = apply_patch(&state.app_data_dir, patch)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    // Launch-at-login: delegate to the shared helper in `lib.rs` which
    if before.general.launch_at_login != after.general.launch_at_login {
        crate::apply_autolaunch_pref(&app, after.general.launch_at_login);
    }

    // Menu-bar agent: rebuild or remove the tray icon. Install is
    if before.general.menu_bar_agent != after.general.menu_bar_agent {
        let recents = if after.general.menu_bar_agent {
            use crate::storage::types::Project;
            use crate::storage::Db;
            match app.try_state::<Db>() {
                Some(db) => match db.recents_list(5).await {
                    Ok(list) => list,
                    Err(e) => {
                        tracing::warn!(error = %e, "tray: recents_list failed on settings toggle");
                        Vec::<Project>::new()
                    }
                },
                None => Vec::<Project>::new(),
            }
        } else {
            Vec::new()
        };
        crate::tray::set_enabled(&app, after.general.menu_bar_agent, recents);
    }

    Ok(after)
}
