//! Timeline IPC commands. Only user-pinned projects appear in the
//! timeline view; all reads against unpinned projects are skipped so
//! the query stays cheap even with many indexed projects.

#![allow(dead_code, unused_variables)]

use chrono::{Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use ts_rs::TS;

use crate::commands::routines::refresh_all as refresh_routines;
use crate::events;
use crate::storage::planner_io;
use crate::storage::types::{
    Milestone, MilestoneStatus, ProjectFilter, ProjectId, Routine, RoutineInstance, TimelineConfig,
};
use crate::storage::{AppContext, Db};

/// One row in the timeline — a single pinned project with its
/// milestones and project-scoped routine instances clipped to the
/// visible window.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct TimelineRow {
    pub project_id: ProjectId,
    pub project_name: String,
    /// Hex color of the project — drives bar fill.
    pub project_color: String,
    pub milestones: Vec<Milestone>,
    pub routines: Vec<Routine>,
    pub routine_instances: Vec<RoutineInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct TimelineData {
    pub config: TimelineConfig,
    pub rows: Vec<TimelineRow>,
    /// Inclusive start of the visible window, `YYYY-MM-DD`.
    pub start: String,
    /// Inclusive end of the visible window, `YYYY-MM-DD`.
    pub end: String,
    /// Today in local time, `YYYY-MM-DD`.
    pub today: String,
}

#[tauri::command]
pub async fn timeline_config_get(
    ctx: tauri::State<'_, AppContext>,
) -> Result<TimelineConfig, String> {
    planner_io::load_timeline_config(&ctx.app_data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn timeline_pin_project(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    project_id: ProjectId,
) -> Result<TimelineConfig, String> {
    let mut cfg =
        planner_io::load_timeline_config(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    if !cfg.pinned_project_ids.iter().any(|id| id == &project_id) {
        cfg.pinned_project_ids.push(project_id);
    }
    planner_io::save_timeline_config(&ctx.app_data_dir, &cfg).map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:timeline_changed", "pinned");
    Ok(cfg)
}

#[tauri::command]
pub async fn timeline_unpin_project(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    project_id: ProjectId,
) -> Result<TimelineConfig, String> {
    let mut cfg =
        planner_io::load_timeline_config(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    cfg.pinned_project_ids.retain(|id| id != &project_id);
    planner_io::save_timeline_config(&ctx.app_data_dir, &cfg).map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:timeline_changed", "unpinned");
    Ok(cfg)
}

#[tauri::command]
pub async fn timeline_set_range(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    visible_range: String,
) -> Result<TimelineConfig, String> {
    let normalised = match visible_range.as_str() {
        "week" | "month" => visible_range,
        other => return Err(format!("invalid range {other:?} (expected week | month)")),
    };
    let mut cfg =
        planner_io::load_timeline_config(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    cfg.visible_range = normalised;
    planner_io::save_timeline_config(&ctx.app_data_dir, &cfg).map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:timeline_changed", "range");
    Ok(cfg)
}

/// Build the timeline payload. The visible window is anchored on
/// today: week → `[today-1d, today+6d]`, month → `[today-7d, today+22d]`.
/// Pass `range_override` to render a different span without touching
/// the persisted config.
#[tauri::command]
pub async fn timeline_query(
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
    range_override: Option<String>,
) -> Result<TimelineData, String> {
    refresh_routines(&ctx).await.map_err(|e| e.to_string())?;

    let cfg =
        planner_io::load_timeline_config(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let effective_range = range_override.unwrap_or_else(|| cfg.visible_range.clone());

    let today = Utc::now().date_naive();
    let (start, end) = match effective_range.as_str() {
        "week" => (
            today.checked_sub_days(Days::new(1)).unwrap_or(today),
            today.checked_add_days(Days::new(6)).unwrap_or(today),
        ),
        _ => (
            today.checked_sub_days(Days::new(7)).unwrap_or(today),
            today.checked_add_days(Days::new(22)).unwrap_or(today),
        ),
    };
    let start_iso = start.format("%Y-%m-%d").to_string();
    let end_iso = end.format("%Y-%m-%d").to_string();

    // Index of all projects so we can resolve pinned ids → projects
    // (and silently skip ids that no longer exist).
    let projects = db
        .list_projects(ProjectFilter {
            include_archived: true,
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let project_by_id: std::collections::HashMap<String, _> = projects
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();

    let routines = planner_io::load_routines(&ctx.app_data_dir).unwrap_or_default();
    let instances = planner_io::load_instances(&ctx.app_data_dir).unwrap_or_default();

    let mut rows: Vec<TimelineRow> = Vec::new();
    for pid in &cfg.pinned_project_ids {
        let project = match project_by_id.get(pid) {
            Some(p) => p,
            None => continue,
        };
        let milestones = db.milestones_list(pid).await.unwrap_or_default();
        let visible_milestones: Vec<Milestone> = milestones
            .into_iter()
            .filter(|m| {
                // Bars run from creation (clamped to the window start) to
                // the deadline. Keep any milestone whose deadline is
                // inside the window OR whose deadline is in the future
                // (to render an "ends-after-window" bar).
                m.deadline >= start_iso || m.status == MilestoneStatus::Active
            })
            .collect();

        let project_routines: Vec<Routine> = routines
            .iter()
            .filter(|r| r.project_id.as_deref() == Some(pid.as_str()))
            .cloned()
            .collect();
        let routine_ids: std::collections::HashSet<&str> = project_routines
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        let row_instances: Vec<RoutineInstance> = instances
            .iter()
            .filter(|i| routine_ids.contains(i.routine_id.as_str()))
            .filter(|i| i.scheduled_for >= start_iso && i.scheduled_for <= end_iso)
            .cloned()
            .collect();

        rows.push(TimelineRow {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            project_color: project.color.clone(),
            milestones: visible_milestones,
            routines: project_routines,
            routine_instances: row_instances,
        });
    }

    let mut effective_cfg = cfg.clone();
    effective_cfg.visible_range = effective_range;

    Ok(TimelineData {
        config: effective_cfg,
        rows,
        start: start_iso,
        end: end_iso,
        today: today.format("%Y-%m-%d").to_string(),
    })
}
