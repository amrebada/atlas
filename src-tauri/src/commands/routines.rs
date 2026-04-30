//! Routine IPC commands. Lazy materialisation + miss detection runs
//! on every list/instances query so the UI stays correct without a
//! background scheduler in P3 (the dedicated tick lands in P4 alongside
//! the Today notification surface).

#![allow(dead_code)]

use chrono::{Days, NaiveDate, Utc};
use tauri::AppHandle;

use crate::events;
use crate::routine_engine::{
    apply_overdue, materialize_routine, projected_completion as projection_compute, routine_score,
    DEFAULT_HORIZON_DAYS,
};
use crate::storage::planner_io;
use crate::storage::types::{ProjectId, Routine, RoutineId, RoutineInstance, RoutineInstanceId};
use crate::storage::AppContext;

/// Materialise + miss-check every routine, persisting any deltas. The
/// outcome is idempotent — repeated calls are no-ops once the time
/// window is full.
pub async fn refresh_all(ctx: &AppContext) -> anyhow::Result<()> {
    let routines = planner_io::load_routines(&ctx.app_data_dir)?;
    let mut instances = planner_io::load_instances(&ctx.app_data_dir)?;
    let today = Utc::now().date_naive();
    let horizon = today
        .checked_add_days(Days::new(DEFAULT_HORIZON_DAYS as u64))
        .unwrap_or(today);

    let mut any_change = false;
    let mut routines_to_save: Vec<Routine> = routines.clone();

    // 1. Materialise forward 90 days for each non-paused routine.
    for routine in &routines {
        let new = match materialize_routine(routine, horizon, &instances) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    routine_id = %routine.id,
                    "routine materialise failed; skipping"
                );
                continue;
            }
        };
        if !new.is_empty() {
            instances.extend(new);
            any_change = true;
        }
    }

    // 2. Apply 24h-grace overdue logic per routine. We slice the
    //    instances belonging to one routine, modify in place, then write
    //    them back into the global vec.
    let now = Utc::now();
    for routine in &routines {
        // Index map → slice. Avoiding the borrow checker dance by
        // re-fetching mutable refs after pulling the indexes.
        let indexes: Vec<usize> = instances
            .iter()
            .enumerate()
            .filter(|(_, i)| i.routine_id == routine.id)
            .map(|(idx, _)| idx)
            .collect();
        if indexes.is_empty() {
            continue;
        }
        let mut owned: Vec<RoutineInstance> =
            indexes.iter().map(|&i| instances[i].clone()).collect();
        let res = apply_overdue(routine, &mut owned, now);
        if !res.newly_missed.is_empty() {
            for (slot, original) in indexes.iter().zip(owned) {
                instances[*slot] = original;
            }
            any_change = true;
            // Append AutoMissed extension events to the routine.
            if !res.extension_events.is_empty() {
                if let Some(r) = routines_to_save.iter_mut().find(|r| r.id == routine.id) {
                    r.extensions.extend(res.extension_events);
                }
            }
        }
    }

    // 3. Recompute success/fail caches on each routine + Goal::Count completed.
    for r in routines_to_save.iter_mut() {
        let s = routine_score(r, &instances);
        r.success_points = s.success_points;
        r.failing_points = s.failing_points;
        if let crate::storage::types::Goal::Count {
            ref mut completed, ..
        } = r.goal
        {
            *completed = s.completed_count as i64;
        }
    }

    if any_change {
        planner_io::save_instances(&ctx.app_data_dir, &instances)?;
    }
    // Always persist routines (cheap; covers score-only changes).
    planner_io::save_routines(&ctx.app_data_dir, &routines_to_save)?;
    Ok(())
}

#[tauri::command]
pub async fn routines_list(
    ctx: tauri::State<'_, AppContext>,
    project_id: Option<ProjectId>,
) -> Result<Vec<Routine>, String> {
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let all = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    Ok(match project_id {
        Some(pid) => all
            .into_iter()
            .filter(|r| r.project_id.as_deref() == Some(pid.as_str()))
            .collect(),
        None => all,
    })
}

#[tauri::command]
pub async fn routines_create(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    routine: Routine,
) -> Result<Routine, String> {
    let mut r = routine;
    if r.id.is_empty() {
        r.id = format!("rt_{}", uuid::Uuid::new_v4().simple());
    }
    if r.created_at.is_empty() {
        r.created_at = Utc::now().to_rfc3339();
    }
    // Defensive — server owns the score caches.
    r.success_points = 0.0;
    r.failing_points = 0.0;
    r.extensions.clear();

    planner_io::upsert_routine(&ctx.app_data_dir, &r).map_err(|e| e.to_string())?;
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:routine_changed", &r.id);
    Ok(r)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn routines_update(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    routine_id: RoutineId,
    title: Option<String>,
    description: Option<String>,
    rrule: Option<String>,
    priority: Option<crate::storage::types::Priority>,
    estimate: Option<i64>,
) -> Result<Routine, String> {
    let mut all = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let r = all
        .iter_mut()
        .find(|r| r.id == routine_id)
        .ok_or_else(|| format!("routine {routine_id} not found"))?;

    if let Some(t) = title {
        r.title = t;
    }
    r.description = description.or(r.description.clone());
    if let Some(rr) = rrule {
        // Validate before persisting so the user gets feedback at edit time.
        crate::routine_engine::parse_rrule(&rr).map_err(|e| e.to_string())?;
        r.rrule = rr;
    }
    if let Some(p) = priority {
        r.priority = p;
    }
    r.estimate = estimate.or(r.estimate);
    let updated = r.clone();

    planner_io::save_routines(&ctx.app_data_dir, &all).map_err(|e| e.to_string())?;
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:routine_changed", &routine_id);
    Ok(updated)
}

#[tauri::command]
pub async fn routines_delete(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    routine_id: RoutineId,
) -> Result<bool, String> {
    let removed =
        planner_io::delete_routine(&ctx.app_data_dir, &routine_id).map_err(|e| e.to_string())?;
    if removed {
        let _ = events::emit_planner(&app, "planner:routine_removed", &routine_id);
    }
    Ok(removed)
}

#[tauri::command]
pub async fn routines_pause(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    routine_id: RoutineId,
    paused: bool,
) -> Result<Routine, String> {
    let mut all = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let r = all
        .iter_mut()
        .find(|r| r.id == routine_id)
        .ok_or_else(|| format!("routine {routine_id} not found"))?;
    r.paused = paused;
    r.paused_from = if paused {
        Some(Utc::now().to_rfc3339())
    } else {
        None
    };
    let updated = r.clone();
    planner_io::save_routines(&ctx.app_data_dir, &all).map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:routine_changed", &routine_id);
    Ok(updated)
}

#[tauri::command]
pub async fn routines_instances(
    ctx: tauri::State<'_, AppContext>,
    routine_id: RoutineId,
    from: String,
    to: String,
) -> Result<Vec<RoutineInstance>, String> {
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let lo = NaiveDate::parse_from_str(&from, "%Y-%m-%d")
        .map_err(|_| "from must be YYYY-MM-DD".to_string())?;
    let hi = NaiveDate::parse_from_str(&to, "%Y-%m-%d")
        .map_err(|_| "to must be YYYY-MM-DD".to_string())?;
    let all = planner_io::load_instances(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let mut out: Vec<RoutineInstance> = all
        .into_iter()
        .filter(|i| i.routine_id == routine_id)
        .filter(|i| {
            NaiveDate::parse_from_str(&i.scheduled_for, "%Y-%m-%d")
                .map(|d| d >= lo && d <= hi)
                .unwrap_or(false)
        })
        .collect();
    out.sort_by(|a, b| a.scheduled_for.cmp(&b.scheduled_for));
    Ok(out)
}

#[tauri::command]
pub async fn routines_complete_instance(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    instance_id: RoutineInstanceId,
    completed_at: Option<String>,
) -> Result<RoutineInstance, String> {
    let mut all = planner_io::load_instances(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let inst = all
        .iter_mut()
        .find(|i| i.id == instance_id)
        .ok_or_else(|| format!("instance {instance_id} not found"))?;
    inst.done_at = Some(completed_at.unwrap_or_else(|| Utc::now().to_rfc3339()));
    inst.skipped = None;
    let routine_id = inst.routine_id.clone();
    let updated = inst.clone();
    planner_io::save_instances(&ctx.app_data_dir, &all).map_err(|e| e.to_string())?;
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:routine_changed", &routine_id);
    Ok(updated)
}

#[tauri::command]
pub async fn routines_skip_instance(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    instance_id: RoutineInstanceId,
) -> Result<RoutineInstance, String> {
    let mut all = planner_io::load_instances(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let inst = all
        .iter_mut()
        .find(|i| i.id == instance_id)
        .ok_or_else(|| format!("instance {instance_id} not found"))?;
    inst.skipped = Some(true);
    inst.done_at = None;
    let routine_id = inst.routine_id.clone();
    let updated = inst.clone();
    planner_io::save_instances(&ctx.app_data_dir, &all).map_err(|e| e.to_string())?;
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let _ = events::emit_planner(&app, "planner:routine_changed", &routine_id);
    Ok(updated)
}

#[tauri::command]
pub async fn routines_materialize(
    ctx: tauri::State<'_, AppContext>,
    horizon_days: i64,
) -> Result<i64, String> {
    let _ = horizon_days; // refresh_all uses DEFAULT_HORIZON_DAYS for MVP
    refresh_all(&ctx).await.map_err(|e| e.to_string())?;
    let all = planner_io::load_instances(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    Ok(all.len() as i64)
}

/// Projected completion for a Goal::Count routine — naive estimate
/// (today + cadence × remaining). Returns null for non-Count goals.
#[tauri::command]
pub async fn routines_projected_completion(
    ctx: tauri::State<'_, AppContext>,
    routine_id: RoutineId,
) -> Result<Option<String>, String> {
    let routines = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let r = match routines.into_iter().find(|r| r.id == routine_id) {
        Some(r) => r,
        None => return Ok(None),
    };
    let instances = planner_io::load_instances(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    let proj = projection_compute(&r, &instances, Utc::now().date_naive());
    Ok(proj.map(|d| d.format("%Y-%m-%d").to_string()))
}
