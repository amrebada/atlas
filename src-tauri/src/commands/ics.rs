//! ICS export IPC commands.
//!
//! Files land in `<app_data>/atlas/ics/`:
//!   * one `<project-slug>.ics` per project that has any milestone or
//!     project-scoped routine,
//!   * a single `global.ics` for routines without a `projectId`,
//!   * a combined `today.ics` covering everything (60 days).
//!
//! The set is regenerated atomically every time the user clicks
//! "Export ICS" — calendars subscribed to the file path will pick up
//! changes on their next poll.

#![allow(dead_code)]

use std::path::PathBuf;

use crate::editors;
use crate::ics_builder::{build_calendar, build_milestone_event, build_routine_event};
use crate::storage::json::write_atomic;
use crate::storage::planner_io;
use crate::storage::types::{MilestoneStatus, ProjectFilter};
use crate::storage::{AppContext, Db};

/// Resolve `<app_data>/atlas/ics/`. Created on first export.
fn ics_dir(ctx: &AppContext) -> PathBuf {
    ctx.app_data_dir.join("ics")
}

/// Slugify a project name for use as a filename. Lowercase ASCII +
/// hyphens; anything outside [a-z 0-9] becomes `-`. Collapses runs.
fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

/// Regenerate every per-project `.ics` plus `global.ics` and the
/// combined `today.ics`. Returns the absolute path of the directory
/// the UI should reveal.
#[tauri::command]
pub async fn ics_export_all(
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
) -> Result<String, String> {
    let dir = ics_dir(&ctx);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create ics dir: {e}"))?;

    let projects = db
        .list_projects(ProjectFilter::default())
        .await
        .map_err(|e| e.to_string())?;
    let routines = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;

    // ----- per-project files -----
    let mut combined: Vec<String> = Vec::new();
    for p in &projects {
        let mut events: Vec<String> = Vec::new();
        let milestones = db.milestones_list(&p.id).await.unwrap_or_default();
        for m in &milestones {
            if matches!(m.status, MilestoneStatus::Cancelled) {
                continue;
            }
            if m.title.trim().is_empty() {
                continue;
            }
            events.push(build_milestone_event(m, &p.name));
        }
        for r in routines
            .iter()
            .filter(|r| r.project_id.as_deref() == Some(p.id.as_str()))
        {
            if r.title.trim().is_empty() || r.paused {
                continue;
            }
            if let Some(evt) = build_routine_event(r, Some(&p.name)) {
                events.push(evt);
            }
        }
        if events.is_empty() {
            // Don't write empty files — they confuse calendar apps and
            // litter the directory after a user clears their planner.
            continue;
        }
        let cal = build_calendar(&format!("Atlas — {}", p.name), &events);
        let file = dir.join(format!("{}.ics", slugify(&p.name)));
        write_atomic(&file, cal.as_bytes()).map_err(|e| e.to_string())?;
        combined.extend(events);
    }

    // ----- global routines -----
    let mut global_events: Vec<String> = Vec::new();
    for r in routines.iter().filter(|r| r.project_id.is_none()) {
        if r.title.trim().is_empty() || r.paused {
            continue;
        }
        if let Some(evt) = build_routine_event(r, None) {
            global_events.push(evt);
        }
    }
    if !global_events.is_empty() {
        let cal = build_calendar("Atlas — Global routines", &global_events);
        write_atomic(&dir.join("global.ics"), cal.as_bytes()).map_err(|e| e.to_string())?;
        combined.extend(global_events);
    }

    // ----- combined -----
    let combined_cal = build_calendar("Atlas — All", &combined);
    write_atomic(&dir.join("today.ics"), combined_cal.as_bytes()).map_err(|e| e.to_string())?;

    Ok(dir.to_string_lossy().to_string())
}

/// Export a single project's `.ics` and return the file path.
#[tauri::command]
pub async fn ics_export_project(
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
    project_id: String,
) -> Result<String, String> {
    let dir = ics_dir(&ctx);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create ics dir: {e}"))?;

    let project = db
        .get_project(&project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown project {project_id}"))?;

    let mut events: Vec<String> = Vec::new();
    let milestones = db.milestones_list(&project.id).await.unwrap_or_default();
    for m in &milestones {
        if matches!(m.status, MilestoneStatus::Cancelled) {
            continue;
        }
        if m.title.trim().is_empty() {
            continue;
        }
        events.push(build_milestone_event(m, &project.name));
    }
    let routines = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    for r in routines
        .iter()
        .filter(|r| r.project_id.as_deref() == Some(project.id.as_str()))
    {
        if r.title.trim().is_empty() || r.paused {
            continue;
        }
        if let Some(evt) = build_routine_event(r, Some(&project.name)) {
            events.push(evt);
        }
    }

    let cal = build_calendar(&format!("Atlas — {}", project.name), &events);
    let file = dir.join(format!("{}.ics", slugify(&project.name)));
    write_atomic(&file, cal.as_bytes()).map_err(|e| e.to_string())?;
    Ok(file.to_string_lossy().to_string())
}

/// Show the ics directory in the platform file manager. The user can
/// drag-drop the files into their calendar app from there.
#[tauri::command]
pub async fn ics_reveal_dir(ctx: tauri::State<'_, AppContext>) -> Result<(), String> {
    let dir = ics_dir(&ctx);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create ics dir: {e}"))?;
    let dir_owned = dir.clone();
    tauri::async_runtime::spawn_blocking(move || editors::reveal(&dir_owned))
        .await
        .map_err(|e| format!("reveal join: {e}"))?
        .map_err(|e| e.to_string())
}
