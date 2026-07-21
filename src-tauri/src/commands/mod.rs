//! Command handlers exposed to the frontend via `tauri::invoke`.

pub mod clipboard;
pub mod collections;
pub mod disk;
pub mod editors;
pub mod agent;
pub mod files;
pub mod git;
pub mod ics;
pub mod launch_templates;
pub mod mcp;
pub mod milestones;
pub mod notes;
pub mod palette;
pub mod pane_layout;
pub mod pilot;
pub mod planner;
pub mod projects;
pub mod providers;
pub mod recents;
pub mod routines;
pub mod scripts;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod tags;
pub mod templates;
pub mod terminal;
pub mod timeline;
pub mod todos;
pub mod watchers;

/// Returns the running app version string (compile-time from `CARGO_PKG_VERSION`).
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
