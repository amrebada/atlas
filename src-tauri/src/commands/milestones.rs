//! Milestone IPC commands. Persistence + akrasia-cost extension flow.
//! Score recomputation runs here (rather than in `Db`) because it
//! needs a `now` clock reference, which we deliberately keep at the
//! command boundary so persistence stays pure.

#![allow(dead_code)]

use chrono::{DateTime, Utc};

use crate::events;
use crate::score_engine::{cost_of_extension, milestone_score};
use crate::storage::types::{
    ExtensionEvent, ExtensionReason, Milestone, MilestoneId, MilestoneStatus, Priority, ProjectId,
};
use crate::storage::Db;
use tauri::AppHandle;

#[tauri::command]
pub async fn milestones_list(
    state: tauri::State<'_, Db>,
    project_id: ProjectId,
) -> Result<Vec<Milestone>, String> {
    state
        .milestones_list(&project_id)
        .await
        .map_err(|e| e.to_string())
}

/// Create a new milestone. Caller supplies the full struct; the server
/// stamps `original_deadline` to match `deadline` and zeroes the score
/// fields if they weren't set (defensive — frontend never owns those).
#[tauri::command]
pub async fn milestones_create(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: ProjectId,
    milestone: Milestone,
) -> Result<Milestone, String> {
    let mut m = milestone;
    m.project_id = project_id.clone();
    if m.original_deadline.is_empty() {
        m.original_deadline = m.deadline.clone();
    }
    m.success_points = 0.0;
    m.failing_points = 0.0;
    m.extensions.clear();

    state
        .milestones_upsert(&project_id, &m)
        .await
        .map_err(|e| e.to_string())?;
    let _ = events::emit_project_updated(
        &app,
        &project_id,
        serde_json::json!({ "milestoneChanged": m.id }),
    );
    Ok(m)
}

/// Update mutable milestone fields. Preserves `original_deadline`,
/// `extensions`, and the score caches.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn milestones_update(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: ProjectId,
    milestone_id: MilestoneId,
    title: Option<String>,
    description: Option<String>,
    priority: Option<Priority>,
    order: Option<i64>,
) -> Result<Milestone, String> {
    let mut existing = state
        .milestones_get(&project_id, &milestone_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("milestone {milestone_id} not found"))?;

    if let Some(t) = title {
        existing.title = t;
    }
    existing.description = description.or(existing.description);
    if let Some(p) = priority {
        existing.priority = p;
    }
    if let Some(o) = order {
        existing.order = o;
    }

    state
        .milestones_upsert(&project_id, &existing)
        .await
        .map_err(|e| e.to_string())?;
    let _ = events::emit_project_updated(
        &app,
        &project_id,
        serde_json::json!({ "milestoneChanged": existing.id }),
    );
    Ok(existing)
}

/// Move a milestone's deadline. Costs failing points if the move lands
/// inside the akrasia horizon (UserSoften / UserOverride). Always
/// records an `ExtensionEvent` so the user sees the history.
#[tauri::command]
pub async fn milestones_extend(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: ProjectId,
    milestone_id: MilestoneId,
    new_deadline: String,
    reason: ExtensionReason,
    note: Option<String>,
) -> Result<Milestone, String> {
    let mut m = state
        .milestones_get(&project_id, &milestone_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("milestone {milestone_id} not found"))?;

    let now = Utc::now();
    let from = parse_iso_or_today(&m.deadline, now);
    let to = parse_iso_or_today(&new_deadline, now);
    let cost = cost_of_extension(from, to, now, m.priority, reason);

    let event = ExtensionEvent {
        from: m.deadline.clone(),
        to: new_deadline.clone(),
        reason,
        failing_points_applied: cost,
        at: now.to_rfc3339(),
        note,
    };
    m.extensions.push(event);
    m.failing_points += cost;
    m.deadline = new_deadline;

    state
        .milestones_upsert(&project_id, &m)
        .await
        .map_err(|e| e.to_string())?;
    let _ = events::emit_project_updated(
        &app,
        &project_id,
        serde_json::json!({ "milestoneChanged": m.id, "extensionApplied": cost }),
    );
    Ok(m)
}

/// Set milestone status. Going to Done stamps `done_at`; going back to
/// Active clears it. Recomputes the score cache so the UI dial is
/// fresh on the next read.
#[tauri::command]
pub async fn milestones_set_status(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: ProjectId,
    milestone_id: MilestoneId,
    status: MilestoneStatus,
) -> Result<Milestone, String> {
    let mut m = state
        .milestones_get(&project_id, &milestone_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("milestone {milestone_id} not found"))?;

    m.status = status;
    m.done_at = match status {
        MilestoneStatus::Done | MilestoneStatus::Cancelled | MilestoneStatus::Missed => {
            Some(Utc::now().to_rfc3339())
        }
        MilestoneStatus::Planned | MilestoneStatus::Active => None,
    };

    // Refresh the score cache so the final stored numbers reflect
    // every member-todo's contribution at completion time.
    let todos = state
        .milestone_member_todos(&project_id, &milestone_id)
        .await
        .map_err(|e| e.to_string())?;
    let score = milestone_score(&todos, Utc::now());
    // Auto-extension cost stays in `failing_points` even after recompute.
    let extension_fail: f64 = m.extensions.iter().map(|e| e.failing_points_applied).sum();
    m.success_points = score.success_points;
    m.failing_points = score.failing_points + extension_fail;

    state
        .milestones_upsert(&project_id, &m)
        .await
        .map_err(|e| e.to_string())?;
    let _ = events::emit_project_updated(
        &app,
        &project_id,
        serde_json::json!({ "milestoneChanged": m.id, "status": format!("{:?}", status) }),
    );
    Ok(m)
}

#[tauri::command]
pub async fn milestones_delete(
    app: AppHandle,
    state: tauri::State<'_, Db>,
    project_id: ProjectId,
    milestone_id: MilestoneId,
) -> Result<bool, String> {
    let removed = state
        .milestones_delete(&project_id, &milestone_id)
        .await
        .map_err(|e| e.to_string())?;
    if removed {
        let _ = events::emit_project_updated(
            &app,
            &project_id,
            serde_json::json!({ "milestoneRemoved": milestone_id }),
        );
    }
    Ok(removed)
}

/// Refresh the milestone's score cache from its current member todos.
/// Called by the todo write paths so the success-rate dial stays live.
pub async fn recompute_milestone_score(
    db: &Db,
    project_id: &str,
    milestone_id: &str,
) -> anyhow::Result<()> {
    let mut m = match db.milestones_get(project_id, milestone_id).await? {
        Some(m) => m,
        None => return Ok(()),
    };
    let todos = db.milestone_member_todos(project_id, milestone_id).await?;
    let score = milestone_score(&todos, Utc::now());
    let extension_fail: f64 = m.extensions.iter().map(|e| e.failing_points_applied).sum();
    m.success_points = score.success_points;
    m.failing_points = score.failing_points + extension_fail;
    db.milestones_upsert(project_id, &m).await
}

fn parse_iso_or_today(s: &str, fallback: DateTime<Utc>) -> DateTime<Utc> {
    if s.len() == 10 {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(23, 59, 59))
            .map(|n| n.and_utc())
            .unwrap_or(fallback)
    } else {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(fallback)
    }
}
