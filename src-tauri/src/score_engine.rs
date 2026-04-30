//! Pure scoring functions for the planner feature.
//!
//! Every function is deterministic over its inputs (callers pass `now`
//! rather than reading the clock) so the engine is trivially testable
//! and reproducible across the client/server boundary.
//!
//! Constants mirror the values in `plans/atlas-planner-deadlines-routines.md`
//! §4d (task scoring) and §4b (akrasia horizon).

#![allow(dead_code)]

use chrono::{DateTime, Duration, Utc};

use crate::storage::types::{ExtensionReason, Priority, Todo};

// --------------------------------------------------------------------
// Tunables — single source of truth for the planner's "feel".
// --------------------------------------------------------------------

/// Base success points awarded for completing a P2 task on time.
/// Scaled by `Priority::weight()`.
pub const BASE_TASK_POINTS: f64 = 100.0;

/// On-time-completion success-points multiplier floor. A task done very
/// late still earns at least 50 % of its base success points so the
/// signal isn't "all or nothing" (see Habitica partial-credit research).
pub const LATE_SUCCESS_FLOOR: f64 = 0.50;

/// Per-day decay applied to success points when a task is completed
/// after its deadline.
pub const LATE_PENALTY_PER_DAY: f64 = 0.10;

/// Per-day failing-point base accrual for late or missed work.
/// Scaled by `Priority::weight()`.
pub const FAIL_POINTS_PER_DAY_LATE: f64 = 10.0;

/// Akrasia-horizon length. User-initiated soften extensions that land
/// within `now + AKRASIA_DAYS` cost failing points.
pub const AKRASIA_DAYS: i64 = 7;

/// Per-day soften cost applied inside the horizon. Scaled by priority.
pub const SOFTEN_COST_PER_DAY_INSIDE_HORIZON: f64 = 50.0;

// --------------------------------------------------------------------
// Types.
// --------------------------------------------------------------------

/// Success and failing points contributed by a single task.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TaskPoints {
    pub success: f64,
    pub fail: f64,
}

/// Aggregated milestone scoring: in-flight rolling rate plus the totals
/// the milestone should persist (`success_points`, `failing_points`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MilestoneScore {
    pub success_points: f64,
    pub failing_points: f64,
    /// `success / (success + fail)` — 1.0 if both are zero.
    pub rolling_rate: f64,
}

// --------------------------------------------------------------------
// Pure functions.
// --------------------------------------------------------------------

/// Compute success/fail points for one todo, given the current time.
///
/// Behaviour:
/// * Done on time → full base points (priority-weighted).
/// * Done late → success points decay 10 %/day (floor 50 %); fail
///   points accrue 10 × priority/day-late.
/// * Open & deadline passed → 0 success, full fail accrual to today.
/// * Open & deadline future, or no deadline at all → zeros.
pub fn task_points(todo: &Todo, now: DateTime<Utc>) -> TaskPoints {
    let priority = todo.priority.unwrap_or_default();
    let base = BASE_TASK_POINTS * priority.weight();
    let weight = priority.weight();

    let deadline = todo.deadline.as_ref().and_then(|s| parse_iso(s));

    if todo.done {
        let done_at = todo
            .done_at
            .as_ref()
            .and_then(|s| parse_iso(s))
            .unwrap_or(now);
        match deadline {
            Some(d) if done_at > d => {
                let days_late = days_between(d, done_at);
                let success = base * (1.0 - LATE_PENALTY_PER_DAY * days_late).max(LATE_SUCCESS_FLOOR);
                let fail = days_late * FAIL_POINTS_PER_DAY_LATE * weight;
                TaskPoints { success, fail }
            }
            _ => TaskPoints {
                success: base,
                fail: 0.0,
            },
        }
    } else if let Some(d) = deadline {
        if now > d {
            let days_late = days_between(d, now);
            let fail = days_late * FAIL_POINTS_PER_DAY_LATE * weight;
            TaskPoints {
                success: 0.0,
                fail,
            }
        } else {
            TaskPoints::default()
        }
    } else {
        TaskPoints::default()
    }
}

/// Aggregate per-todo points into a milestone score.
pub fn milestone_score(todos: &[Todo], now: DateTime<Utc>) -> MilestoneScore {
    let mut s = 0.0_f64;
    let mut f = 0.0_f64;
    for t in todos {
        let p = task_points(t, now);
        s += p.success;
        f += p.fail;
    }
    let rolling_rate = if s + f <= 0.0 { 1.0 } else { s / (s + f) };
    MilestoneScore {
        success_points: s,
        failing_points: f,
        rolling_rate,
    }
}

/// Failing-point cost of pushing a milestone deadline from `from` to
/// `to`, given the priority and reason for the move. Auto-missed and
/// pause-driven moves cost zero (their points come from the score
/// engine elsewhere). User soften / override is priced by how many
/// days the extension lands inside the akrasia horizon — landing
/// further out (`to >= now + 7 days`) is free planning, anything
/// closer is "softening" and bills points.
pub fn cost_of_extension(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    priority: Priority,
    reason: ExtensionReason,
) -> f64 {
    match reason {
        ExtensionReason::AutoMissed | ExtensionReason::Paused => 0.0,
        ExtensionReason::UserSoften | ExtensionReason::UserOverride => {
            let horizon = now + Duration::days(AKRASIA_DAYS);
            let lo = from.max(now);
            let hi = to.min(horizon);
            if hi <= lo {
                return 0.0;
            }
            let days = (hi - lo).num_seconds() as f64 / 86_400.0;
            days * SOFTEN_COST_PER_DAY_INSIDE_HORIZON * priority.weight()
        }
    }
}

// --------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------

fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    // ISO date (`YYYY-MM-DD`) or full RFC3339 timestamp. Date-only
    // strings are interpreted as 23:59:59 UTC of that day so deadlines
    // align with end-of-day expectations.
    if s.len() == 10 {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(23, 59, 59))
            .map(|n| n.and_utc())
    } else {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }
}

fn days_between(earlier: DateTime<Utc>, later: DateTime<Utc>) -> f64 {
    if later <= earlier {
        0.0
    } else {
        (later - earlier).num_seconds() as f64 / 86_400.0
    }
}

// --------------------------------------------------------------------
// Tests.
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    fn mk_todo(priority: Priority, deadline: Option<&str>) -> Todo {
        Todo {
            id: "t".into(),
            done: false,
            text: "x".into(),
            due: None,
            created_at: "2026-04-01T00:00:00Z".into(),
            project_id: None,
            milestone_id: None,
            routine_instance_id: None,
            priority: Some(priority),
            deadline: deadline.map(String::from),
            estimate: None,
            pinned_today: None,
            done_at: None,
        }
    }

    // ---- task_points ----

    #[test]
    fn open_no_deadline_is_zero() {
        let now = at(2026, 5, 1, 12);
        assert_eq!(task_points(&mk_todo(Priority::P2, None), now), TaskPoints::default());
    }

    #[test]
    fn open_future_deadline_is_zero() {
        let now = at(2026, 5, 1, 12);
        let t = mk_todo(Priority::P1, Some("2026-05-10"));
        assert_eq!(task_points(&t, now), TaskPoints::default());
    }

    #[test]
    fn done_on_time_pays_full_base() {
        let now = at(2026, 5, 5, 12);
        let mut t = mk_todo(Priority::P0, Some("2026-05-10"));
        t.done = true;
        t.done_at = Some("2026-05-05T12:00:00Z".into());
        let p = task_points(&t, now);
        // P0 weight 2.0 → base 200.
        assert!((p.success - 200.0).abs() < 1e-6);
        assert_eq!(p.fail, 0.0);
    }

    #[test]
    fn done_late_decays_success_and_accrues_fail() {
        let now = at(2026, 5, 13, 12);
        // Deadline 2026-05-10 (interpreted as 23:59:59 UTC). Done 3 days
        // later at noon → ~2.5 days late.
        let mut t = mk_todo(Priority::P2, Some("2026-05-10"));
        t.done = true;
        t.done_at = Some("2026-05-13T12:00:00Z".into());
        let p = task_points(&t, now);
        // success = 100 * max(0.5, 1 - 0.10*2.5) = 100 * 0.75 = 75
        assert!((p.success - 75.0).abs() < 1e-3);
        // fail = 2.5 days * 10 * 1.0 = 25
        assert!((p.fail - 25.0).abs() < 1e-3);
    }

    #[test]
    fn done_very_late_floors_at_half_base() {
        let now = at(2026, 6, 10, 12);
        let mut t = mk_todo(Priority::P2, Some("2026-05-10"));
        t.done = true;
        t.done_at = Some("2026-06-10T12:00:00Z".into());
        let p = task_points(&t, now);
        // Way past 5 days late → success floors at 0.5 * base = 50.
        assert!((p.success - 50.0).abs() < 1e-3);
        assert!(p.fail > 0.0);
    }

    #[test]
    fn open_overdue_only_accrues_fail() {
        let now = at(2026, 5, 13, 12);
        let t = mk_todo(Priority::P0, Some("2026-05-10"));
        let p = task_points(&t, now);
        assert_eq!(p.success, 0.0);
        // ~2.5 days late * 10 * 2.0 (P0) = 50
        assert!((p.fail - 50.0).abs() < 1e-3);
    }

    // ---- milestone_score aggregation ----

    #[test]
    fn aggregation_handles_mixed_states() {
        let now = at(2026, 5, 13, 12);
        let mut on_time = mk_todo(Priority::P2, Some("2026-05-10"));
        on_time.done = true;
        on_time.done_at = Some("2026-05-09T12:00:00Z".into());
        let mut late = mk_todo(Priority::P2, Some("2026-05-10"));
        late.done = true;
        late.done_at = Some("2026-05-13T12:00:00Z".into());
        let overdue = mk_todo(Priority::P2, Some("2026-05-10"));
        let pending = mk_todo(Priority::P2, Some("2026-05-25"));
        let s = milestone_score(&[on_time, late, overdue, pending], now);
        assert!(s.success_points > 0.0);
        assert!(s.failing_points > 0.0);
        assert!(s.rolling_rate > 0.0 && s.rolling_rate < 1.0);
    }

    #[test]
    fn aggregation_empty_milestone_is_perfect() {
        let now = at(2026, 5, 1, 12);
        let s = milestone_score(&[], now);
        assert_eq!(s.rolling_rate, 1.0);
        assert_eq!(s.success_points, 0.0);
        assert_eq!(s.failing_points, 0.0);
    }

    // ---- cost_of_extension ----

    #[test]
    fn auto_missed_costs_nothing() {
        let now = at(2026, 5, 1, 12);
        let from = at(2026, 5, 1, 0);
        let to = at(2026, 5, 5, 0);
        let c = cost_of_extension(from, to, now, Priority::P0, ExtensionReason::AutoMissed);
        assert_eq!(c, 0.0);
    }

    #[test]
    fn paused_costs_nothing() {
        let now = at(2026, 5, 1, 12);
        let from = at(2026, 5, 1, 0);
        let to = at(2026, 5, 5, 0);
        let c = cost_of_extension(from, to, now, Priority::P0, ExtensionReason::Paused);
        assert_eq!(c, 0.0);
    }

    #[test]
    fn extension_outside_horizon_is_free() {
        let now = at(2026, 5, 1, 12);
        // Both `from` and `to` past `now + 7 days`.
        let from = at(2026, 5, 20, 0);
        let to = at(2026, 5, 25, 0);
        let c = cost_of_extension(from, to, now, Priority::P2, ExtensionReason::UserSoften);
        assert_eq!(c, 0.0);
    }

    #[test]
    fn extension_inside_horizon_costs_proportional_days() {
        let now = at(2026, 5, 1, 0);
        let from = at(2026, 5, 1, 0); // == now
        let to = at(2026, 5, 5, 0); // 4 days inside horizon
        let c = cost_of_extension(from, to, now, Priority::P2, ExtensionReason::UserSoften);
        // 4 days * 50 * 1.0 = 200
        assert!((c - 200.0).abs() < 1e-3);
    }

    #[test]
    fn extension_priority_scales_cost() {
        let now = at(2026, 5, 1, 0);
        let from = at(2026, 5, 1, 0);
        let to = at(2026, 5, 5, 0);
        let p0 = cost_of_extension(from, to, now, Priority::P0, ExtensionReason::UserSoften);
        let p3 = cost_of_extension(from, to, now, Priority::P3, ExtensionReason::UserSoften);
        // P0 (2x) is 4x P3 (0.5x).
        assert!((p0 / p3 - 4.0).abs() < 1e-3);
    }

    #[test]
    fn extension_partly_inside_horizon_only_charges_inside_portion() {
        let now = at(2026, 5, 1, 0);
        let from = at(2026, 5, 5, 0); // 4 days into horizon
        let to = at(2026, 5, 15, 0); // 14 days out — past horizon end (day 8)
        let c = cost_of_extension(from, to, now, Priority::P2, ExtensionReason::UserSoften);
        // Inside-horizon portion: from day 5 to day 8 (= now + 7) → 3 days.
        // 3 days * 50 * 1.0 = 150
        assert!((c - 150.0).abs() < 1e-3);
    }

    #[test]
    fn extension_with_overdue_origin_starts_at_now() {
        // Original deadline already passed — the extension is fully a
        // "softening from now" move; cost is measured from `now`, not
        // from the historical deadline.
        let now = at(2026, 5, 10, 0);
        let from = at(2026, 5, 1, 0); // 9 days overdue
        let to = at(2026, 5, 14, 0); // 4 days inside horizon
        let c = cost_of_extension(from, to, now, Priority::P2, ExtensionReason::UserSoften);
        // 4 days * 50 * 1.0 = 200
        assert!((c - 200.0).abs() < 1e-3);
    }
}
