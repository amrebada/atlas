//! Planner IPC commands — Today view, session-start notification,
//! pause-all, score summary, extension log.
//!
//! All scoring is server-side so the headline ranking is consistent
//! between the in-app panel and the OS notification. The frontend
//! just renders what it gets.

#![allow(dead_code, unused_variables)]

use chrono::{Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

use crate::commands::routines::refresh_all as refresh_routines;
use crate::events;
use crate::storage::planner_io;
use crate::storage::types::{
    ExtensionEvent, MilestoneId, MilestoneStatus, PlannerState, Priority, Project, ProjectFilter,
    ProjectId, RoutineId, ScoreSnapshot, Todo,
};
use crate::storage::{AppContext, Db};

// =====================================================================
// Public DTOs (re-emitted via ts-rs into rust.ts).
// =====================================================================

/// One Today-view item. Discriminated on `kind` so the React side can
/// render todos / milestones / routine-instances with one component
/// list.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum TodayItem {
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "todo")]
    Todo {
        id: String,
        project_id: ProjectId,
        project_name: String,
        text: String,
        priority: Priority,
        deadline: Option<String>,
        #[ts(type = "number")]
        score: f64,
        #[ts(type = "number")]
        days_overdue: i64,
        pinned_today: bool,
    },
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "milestone-deadline")]
    MilestoneDeadline {
        id: MilestoneId,
        project_id: ProjectId,
        project_name: String,
        title: String,
        deadline: String,
        priority: Priority,
        #[ts(type = "number")]
        score: f64,
    },
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "routine-instance")]
    RoutineInstance {
        id: String,
        routine_id: RoutineId,
        project_id: Option<ProjectId>,
        project_name: Option<String>,
        title: String,
        scheduled_for: String,
        priority: Priority,
        #[ts(type = "number")]
        score: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct PlannerToday {
    pub must_do: Vec<TodayItem>,
    pub could_do: Vec<TodayItem>,
    pub top_priority: Option<TodayItem>,
    pub deadlines_tomorrow: Vec<TodayItem>,
    /// Sum of estimates across must-do items, in minutes.
    #[ts(type = "number")]
    pub total_estimate_minutes: i64,
    /// True when the global pause-all flag is set; the UI uses this to
    /// pulse a banner so the user remembers they paused.
    pub paused_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ScoreSummary {
    pub lifetime: ScoreSnapshot,
    pub rolling30d: ScoreSnapshot,
    pub daily: Vec<ScoreSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct SessionStartResult {
    /// True when the headline notification just fired (first session of
    /// the local day). False on subsequent calls within the same day.
    pub fired: bool,
    /// Local `YYYY-MM-DD` of the session start.
    pub local_date: String,
    /// Snapshot of today right at session start, if the call fired.
    pub today: Option<PlannerToday>,
}

// =====================================================================
// Scoring constants (mirror plan §6).
// =====================================================================

const W_OVERDUE: f64 = 100.0;
const W_DEADLINE: f64 = 30.0;
const W_FAIL_PROJECTED: f64 = 20.0;
const W_ROUTINE_TODAY: f64 = 10.0;
const W_DIFFICULTY: f64 = 5.0;
const W_USER_PIN: f64 = 100.0;

// =====================================================================
// planner_today — the heavy aggregator.
// =====================================================================

#[tauri::command]
pub async fn planner_today(
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
) -> Result<PlannerToday, String> {
    // Lazy refresh of routines so missed-instances + new materialised
    // dates are correct *before* we score.
    refresh_routines(&ctx).await.map_err(|e| e.to_string())?;
    build_today(&db, &ctx).await.map_err(|e| e.to_string())
}

async fn build_today(db: &Db, ctx: &AppContext) -> anyhow::Result<PlannerToday> {
    let now = Utc::now();
    let today = now.date_naive();
    let tomorrow = today.checked_add_days(Days::new(1)).unwrap_or(today);
    let today_iso = today.format("%Y-%m-%d").to_string();
    let tomorrow_iso = tomorrow.format("%Y-%m-%d").to_string();

    let projects = db.list_projects(ProjectFilter::default()).await?;
    let project_by_id: std::collections::HashMap<String, Project> = projects
        .iter()
        .cloned()
        .map(|p| (p.id.clone(), p))
        .collect();

    // ---- todos (per project) ----
    let mut todo_items: Vec<TodayItem> = Vec::new();
    let mut total_estimate: i64 = 0;
    for p in &projects {
        let todos = db.todos_list(&p.id).await.unwrap_or_default();
        for t in todos {
            if t.done {
                continue;
            }
            // Skip todos with no text — orphan legacy rows or quick-add
            // misfires shouldn't clutter the Today list.
            if t.text.trim().is_empty() {
                continue;
            }
            let deadline = t.deadline.clone();
            let days_overdue = days_overdue(deadline.as_deref(), today);
            let due_today_or_pinned =
                deadline.as_deref() == Some(today_iso.as_str()) || t.pinned_today.unwrap_or(false);
            let in_scope = days_overdue > 0 || due_today_or_pinned;
            let could_do = !in_scope && deadline.as_deref().is_some_and(|d| d > today_iso.as_str());
            if !in_scope && !could_do {
                continue;
            }
            let priority = t.priority.unwrap_or_default();
            let score = score_todo(&t, today, days_overdue);
            if let Some(est) = t.estimate {
                if in_scope {
                    total_estimate += est;
                }
            }
            todo_items.push(TodayItem::Todo {
                id: t.id,
                project_id: p.id.clone(),
                project_name: p.name.clone(),
                text: t.text,
                priority,
                deadline,
                score,
                days_overdue,
                pinned_today: t.pinned_today.unwrap_or(false),
            });
        }
    }

    // ---- milestone deadlines (today + tomorrow + overdue) ----
    let mut milestone_today_items: Vec<TodayItem> = Vec::new();
    let mut deadlines_tomorrow: Vec<TodayItem> = Vec::new();
    for p in &projects {
        let milestones = db.milestones_list(&p.id).await.unwrap_or_default();
        for m in milestones {
            if matches!(
                m.status,
                MilestoneStatus::Done | MilestoneStatus::Cancelled | MilestoneStatus::Missed
            ) {
                continue;
            }
            if m.title.trim().is_empty() {
                continue;
            }
            let dl = m.deadline.clone();
            // Milestones land on the Today list when overdue OR due today.
            let is_today = dl == today_iso;
            let overdue = NaiveDate::parse_from_str(&dl, "%Y-%m-%d")
                .map(|d| d < today)
                .unwrap_or(false);
            let is_tomorrow = dl == tomorrow_iso;

            if is_today || overdue {
                let score = W_DEADLINE * if overdue { 50.0 } else { 1.0 }
                    + W_DIFFICULTY * priority_difficulty(m.priority);
                milestone_today_items.push(TodayItem::MilestoneDeadline {
                    id: m.id.clone(),
                    project_id: p.id.clone(),
                    project_name: p.name.clone(),
                    title: m.title.clone(),
                    deadline: dl.clone(),
                    priority: m.priority,
                    score,
                });
            } else if is_tomorrow {
                deadlines_tomorrow.push(TodayItem::MilestoneDeadline {
                    id: m.id,
                    project_id: p.id.clone(),
                    project_name: p.name.clone(),
                    title: m.title,
                    deadline: dl,
                    priority: m.priority,
                    score: W_DEADLINE, // ranked-low; this list is informational
                });
            }
        }
    }

    // ---- routine instances (today + overdue) ----
    let routines = planner_io::load_routines(&ctx.app_data_dir).unwrap_or_default();
    let routines_by_id: std::collections::HashMap<String, _> = routines
        .iter()
        .cloned()
        .map(|r| (r.id.clone(), r))
        .collect();
    let instances = planner_io::load_instances(&ctx.app_data_dir).unwrap_or_default();
    let mut routine_items: Vec<TodayItem> = Vec::new();
    for inst in &instances {
        if inst.done_at.is_some() || inst.skipped == Some(true) {
            continue;
        }
        let r = match routines_by_id.get(&inst.routine_id) {
            Some(r) if !r.paused => r,
            _ => continue,
        };
        if r.title.trim().is_empty() {
            continue;
        }
        let scheduled = inst.scheduled_for.as_str();
        let is_today = scheduled == today_iso;
        let is_overdue = scheduled < today_iso.as_str();
        if !is_today && !is_overdue {
            continue;
        }
        let project_name = r
            .project_id
            .as_deref()
            .and_then(|pid| project_by_id.get(pid))
            .map(|p| p.name.clone());
        let days_overdue_routine = if is_overdue {
            (today - NaiveDate::parse_from_str(scheduled, "%Y-%m-%d").unwrap_or(today))
                .num_days()
                .max(0)
        } else {
            0
        };
        let score = W_OVERDUE * days_overdue_routine as f64
            + W_ROUTINE_TODAY
            + W_DIFFICULTY * priority_difficulty(r.priority);
        routine_items.push(TodayItem::RoutineInstance {
            id: inst.id.clone(),
            routine_id: r.id.clone(),
            project_id: r.project_id.clone(),
            project_name,
            title: r.title.clone(),
            scheduled_for: inst.scheduled_for.clone(),
            priority: r.priority,
            score,
        });
        if let Some(est) = r.estimate {
            total_estimate += est;
        }
    }

    // ---- merge + tier split ----
    let mut combined: Vec<TodayItem> = Vec::new();
    combined.extend(todo_items);
    combined.extend(milestone_today_items);
    combined.extend(routine_items);

    let (must_do, could_do): (Vec<_>, Vec<_>) = combined
        .into_iter()
        .partition(|item| is_must_do(item, &today_iso));

    let mut must_do = must_do;
    let mut could_do = could_do;
    must_do.sort_by(|a, b| score_of(b).total_cmp(&score_of(a)));
    could_do.sort_by(|a, b| score_of(b).total_cmp(&score_of(a)));

    let top_priority = must_do.first().or_else(|| could_do.first()).cloned();

    let planner_state = planner_io::load_planner_state(&ctx.app_data_dir).unwrap_or_default();

    Ok(PlannerToday {
        must_do,
        could_do,
        top_priority,
        deadlines_tomorrow,
        total_estimate_minutes: total_estimate,
        paused_all: planner_state.paused_all,
    })
}

fn is_must_do(item: &TodayItem, today_iso: &str) -> bool {
    match item {
        TodayItem::Todo {
            days_overdue,
            deadline,
            pinned_today,
            ..
        } => *pinned_today || *days_overdue > 0 || deadline.as_deref() == Some(today_iso),
        TodayItem::MilestoneDeadline { deadline, .. } => deadline.as_str() <= today_iso,
        TodayItem::RoutineInstance { scheduled_for, .. } => scheduled_for.as_str() <= today_iso,
    }
}

fn score_of(item: &TodayItem) -> f64 {
    match item {
        TodayItem::Todo { score, .. } => *score,
        TodayItem::MilestoneDeadline { score, .. } => *score,
        TodayItem::RoutineInstance { score, .. } => *score,
    }
}

fn score_todo(t: &Todo, today: NaiveDate, days_overdue: i64) -> f64 {
    let priority = t.priority.unwrap_or_default();
    let mut score = 0.0;
    score += W_OVERDUE * days_overdue.max(0) as f64;
    if let Some(dl) = t.deadline.as_deref() {
        if let Ok(d) = NaiveDate::parse_from_str(dl, "%Y-%m-%d") {
            if d >= today {
                let days_until = (d - today).num_days().max(0) as f64;
                score += W_DEADLINE * (1.0 / (days_until + 1.0));
            }
        }
    }
    score += W_DIFFICULTY * priority_difficulty(priority);
    if t.pinned_today.unwrap_or(false) {
        score += W_USER_PIN;
    }
    score
}

fn priority_difficulty(p: Priority) -> f64 {
    match p {
        Priority::P0 => 1.5,
        Priority::P1 => 1.0,
        Priority::P2 => 0.4,
        Priority::P3 => 0.2,
    }
}

fn days_overdue(deadline: Option<&str>, today: NaiveDate) -> i64 {
    match deadline {
        Some(d) => match NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            Ok(parsed) if parsed < today => (today - parsed).num_days(),
            _ => 0,
        },
        None => 0,
    }
}

// =====================================================================
// Session-start notification.
// =====================================================================

#[tauri::command]
pub async fn planner_session_start(
    app: AppHandle,
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
) -> Result<SessionStartResult, String> {
    let now = Utc::now();
    let local_date = now.date_naive().format("%Y-%m-%d").to_string();

    let mut state = planner_io::load_planner_state(&ctx.app_data_dir).map_err(|e| e.to_string())?;

    // Already fired today → no-op.
    if state.last_session_date.as_deref() == Some(local_date.as_str()) {
        return Ok(SessionStartResult {
            fired: false,
            local_date,
            today: None,
        });
    }

    // First session of the local day → build today + emit + persist.
    let today = build_today(&db, &ctx).await.map_err(|e| e.to_string())?;
    state.last_session_date = Some(local_date.clone());
    state.last_notification_at = Some(now.to_rfc3339());

    // Capture a daily score snapshot derived from per-project milestone
    // caches so the rolling-30d / lifetime numbers have history.
    if let Ok(snapshot) = build_daily_snapshot(&db, &ctx, &local_date).await {
        let exists = state.score_snapshots.iter().any(|s| s.date == local_date);
        if !exists {
            state.score_snapshots.push(snapshot);
        }
        // Keep ~365 days; trim older entries to avoid unbounded growth.
        if state.score_snapshots.len() > 365 {
            let drop = state.score_snapshots.len() - 365;
            state.score_snapshots.drain(0..drop);
        }
    }

    planner_io::save_planner_state(&ctx.app_data_dir, &state).map_err(|e| e.to_string())?;

    let payload = serde_json::to_value(&today).map_err(|e| e.to_string())?;
    let _ = app.emit("planner:notification", &payload);

    Ok(SessionStartResult {
        fired: true,
        local_date,
        today: Some(today),
    })
}

async fn build_daily_snapshot(
    db: &Db,
    ctx: &AppContext,
    date: &str,
) -> anyhow::Result<ScoreSnapshot> {
    let projects = db.list_projects(ProjectFilter::default()).await?;
    let mut s = 0.0_f64;
    let mut f = 0.0_f64;
    for p in projects {
        let ms = db.milestones_list(&p.id).await.unwrap_or_default();
        for m in ms {
            s += m.success_points;
            f += m.failing_points;
        }
    }
    let routines = planner_io::load_routines(&ctx.app_data_dir).unwrap_or_default();
    for r in routines {
        s += r.success_points;
        f += r.failing_points;
    }
    let total = s + f;
    let rate = if total <= 0.0 { 1.0 } else { s / total };
    Ok(ScoreSnapshot {
        date: date.to_string(),
        success_points: s,
        failing_points: f,
        success_rate: rate,
    })
}

// =====================================================================
// Pause-all + extension log + score summary.
// =====================================================================

#[tauri::command]
pub async fn planner_pause_all(
    app: AppHandle,
    ctx: tauri::State<'_, AppContext>,
    paused: bool,
) -> Result<PlannerState, String> {
    let mut state = planner_io::load_planner_state(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    state.paused_all = paused;
    state.paused_from = if paused {
        Some(Utc::now().to_rfc3339())
    } else {
        None
    };
    planner_io::save_planner_state(&ctx.app_data_dir, &state).map_err(|e| e.to_string())?;

    // Cascade to every routine so the engine's per-routine guard kicks
    // in. Existing instances stay; new ones don't materialise.
    let mut routines = planner_io::load_routines(&ctx.app_data_dir).map_err(|e| e.to_string())?;
    for r in routines.iter_mut() {
        r.paused = paused;
        r.paused_from = state.paused_from.clone();
    }
    planner_io::save_routines(&ctx.app_data_dir, &routines).map_err(|e| e.to_string())?;

    let _ = events::emit_planner(&app, "planner:paused_changed", &paused.to_string());
    Ok(state)
}

#[tauri::command]
pub async fn planner_score_summary(
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
    project_id: Option<ProjectId>,
    range_days: i64,
) -> Result<ScoreSummary, String> {
    // Lifetime numbers come from the per-project milestone caches +
    // global routine caches. Daily series comes from the persisted
    // snapshots in `planner_state.json`.
    let mut lifetime_s = 0.0_f64;
    let mut lifetime_f = 0.0_f64;

    let projects = db
        .list_projects(ProjectFilter::default())
        .await
        .map_err(|e| e.to_string())?;
    for p in &projects {
        if let Some(target) = project_id.as_deref() {
            if p.id != target {
                continue;
            }
        }
        let ms = db.milestones_list(&p.id).await.unwrap_or_default();
        for m in ms {
            lifetime_s += m.success_points;
            lifetime_f += m.failing_points;
        }
    }
    if project_id.is_none() {
        let routines = planner_io::load_routines(&ctx.app_data_dir).unwrap_or_default();
        for r in routines {
            lifetime_s += r.success_points;
            lifetime_f += r.failing_points;
        }
    }
    let lifetime_total = lifetime_s + lifetime_f;
    let lifetime_rate = if lifetime_total <= 0.0 {
        1.0
    } else {
        lifetime_s / lifetime_total
    };
    let today_iso = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let lifetime = ScoreSnapshot {
        date: today_iso.clone(),
        success_points: lifetime_s,
        failing_points: lifetime_f,
        success_rate: lifetime_rate,
    };

    let state = planner_io::load_planner_state(&ctx.app_data_dir).unwrap_or_default();
    let cutoff_30 = Utc::now()
        .date_naive()
        .checked_sub_days(Days::new(30))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let mut rolling_s = 0.0_f64;
    let mut rolling_f = 0.0_f64;
    for snap in state.score_snapshots.iter().filter(|s| s.date >= cutoff_30) {
        rolling_s += snap.success_points;
        rolling_f += snap.failing_points;
    }
    let rolling_total = rolling_s + rolling_f;
    let rolling30d = ScoreSnapshot {
        date: today_iso,
        success_points: rolling_s,
        failing_points: rolling_f,
        success_rate: if rolling_total <= 0.0 {
            1.0
        } else {
            rolling_s / rolling_total
        },
    };

    let cutoff_range = Utc::now()
        .date_naive()
        .checked_sub_days(Days::new(range_days.max(0) as u64))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let daily: Vec<ScoreSnapshot> = state
        .score_snapshots
        .into_iter()
        .filter(|s| s.date >= cutoff_range)
        .collect();

    Ok(ScoreSummary {
        lifetime,
        rolling30d,
        daily,
    })
}

#[tauri::command]
pub async fn planner_extension_log(
    db: tauri::State<'_, Db>,
    ctx: tauri::State<'_, AppContext>,
    project_id: Option<ProjectId>,
    milestone_id: Option<MilestoneId>,
    routine_id: Option<RoutineId>,
) -> Result<Vec<ExtensionEvent>, String> {
    let mut out: Vec<ExtensionEvent> = Vec::new();

    if routine_id.is_none() {
        let projects = db
            .list_projects(ProjectFilter::default())
            .await
            .map_err(|e| e.to_string())?;
        for p in projects {
            if let Some(target) = project_id.as_deref() {
                if p.id != target {
                    continue;
                }
            }
            let ms = db.milestones_list(&p.id).await.unwrap_or_default();
            for m in ms {
                if let Some(mid) = milestone_id.as_deref() {
                    if m.id != mid {
                        continue;
                    }
                }
                out.extend(m.extensions);
            }
        }
    }

    if milestone_id.is_none() {
        let routines = planner_io::load_routines(&ctx.app_data_dir).unwrap_or_default();
        for r in routines {
            if let Some(target) = routine_id.as_deref() {
                if r.id != target {
                    continue;
                }
            }
            if project_id.is_some() && r.project_id.as_deref() != project_id.as_deref() {
                continue;
            }
            out.extend(r.extensions);
        }
    }

    out.sort_by(|a, b| b.at.cmp(&a.at));
    Ok(out)
}
