//! Routine recurrence engine — RRULE mini-parser, materialiser, miss
//! detector, and score aggregator.
//!
//! Supported subset of RFC 5545 (intentionally narrow for MVP):
//!   * `FREQ=DAILY[;INTERVAL=N]`
//!   * `FREQ=WEEKLY;BYDAY=MO,TU,...`
//!   * `COUNT=N` or `UNTIL=YYYYMMDD` bounds (mutually exclusive)
//!
//! When ICS export needs full RFC-5545 conformance (P6) we'll swap in
//! the `rrule` crate. The narrow subset is enough for "every N days"
//! and "specific weekdays" — the only patterns the picker exposes.
//!
//! All dates are `chrono::NaiveDate`; routines are intentionally
//! date-only (no time-of-day) to avoid DST drift on instance dates.

#![allow(dead_code)]

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc, Weekday};

use crate::score_engine::{
    BASE_TASK_POINTS, FAIL_POINTS_PER_DAY_LATE, LATE_PENALTY_PER_DAY, LATE_SUCCESS_FLOOR,
};
use crate::storage::types::{ExtensionEvent, ExtensionReason, Goal, Routine, RoutineInstance};

/// Hard cap on a single materialisation pass. Both the picker and
/// the engine enforce this to keep an unbounded RRULE from running
/// the table off the page.
pub const MAX_INSTANCES_PER_PASS: usize = 730;

/// Default forward window the engine materialises ahead of "today".
pub const DEFAULT_HORIZON_DAYS: i64 = 90;

/// Cap on failing points one routine can accrue per local day. Without
/// this, a vacation collapses lifetime success rate.
pub const FAIL_CAP_PER_ROUTINE_PER_DAY: f64 = 50.0;

// =====================================================================
// Mini-parser.
// =====================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cadence {
    /// Every `interval` days, anchored at the routine's `start_date`.
    Daily { interval: u32 },
    /// Every week, on the specified weekdays.
    Weekdays { days: Vec<Weekday> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Count(u32),
    Until(NaiveDate),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRRule {
    pub cadence: Cadence,
    pub bound: Bound,
}

/// Parse an RRULE string from our supported subset. Accepts the value
/// portion only (without a leading `RRULE:` prefix) — both forms are
/// tolerated for paste-friendliness.
pub fn parse_rrule(input: &str) -> anyhow::Result<ParsedRRule> {
    let s = input.trim().trim_start_matches("RRULE:").trim();
    if s.is_empty() {
        anyhow::bail!("empty RRULE");
    }

    let mut freq: Option<&str> = None;
    let mut interval: u32 = 1;
    let mut byday: Option<Vec<Weekday>> = None;
    let mut count: Option<u32> = None;
    let mut until: Option<NaiveDate> = None;

    for part in s.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("malformed RRULE part {part:?} (expected KEY=VALUE)"))?;
        match k.trim().to_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v.trim() {
                    "DAILY" => "DAILY",
                    "WEEKLY" => "WEEKLY",
                    other => anyhow::bail!("unsupported FREQ {other:?} (Daily | Weekly only)"),
                })
            }
            "INTERVAL" => {
                interval = v.trim().parse().map_err(|_| {
                    anyhow::anyhow!("INTERVAL must be a positive integer, got {v:?}")
                })?;
                if interval == 0 {
                    anyhow::bail!("INTERVAL must be ≥ 1");
                }
            }
            "BYDAY" => {
                let mut ds = Vec::new();
                for d in v.split(',') {
                    let d = d.trim().to_uppercase();
                    let day = match d.as_str() {
                        "MO" => Weekday::Mon,
                        "TU" => Weekday::Tue,
                        "WE" => Weekday::Wed,
                        "TH" => Weekday::Thu,
                        "FR" => Weekday::Fri,
                        "SA" => Weekday::Sat,
                        "SU" => Weekday::Sun,
                        other => anyhow::bail!("unsupported BYDAY value {other:?}"),
                    };
                    if !ds.contains(&day) {
                        ds.push(day);
                    }
                }
                if ds.is_empty() {
                    anyhow::bail!("BYDAY must list at least one weekday");
                }
                byday = Some(ds);
            }
            "COUNT" => {
                count = Some(
                    v.trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("COUNT must be a positive integer"))?,
                );
            }
            "UNTIL" => {
                // Accept `YYYYMMDD` (RFC 5545) or `YYYY-MM-DD`.
                let raw = v.trim();
                let parsed = NaiveDate::parse_from_str(raw, "%Y%m%d")
                    .or_else(|_| NaiveDate::parse_from_str(raw, "%Y-%m-%d"))
                    .map_err(|_| anyhow::anyhow!("UNTIL must be YYYYMMDD or YYYY-MM-DD"))?;
                until = Some(parsed);
            }
            // Silently ignore the few RFC parts we don't model — keeps
            // partial round-trips working when ICS-imported strings
            // carry extras like WKST.
            "WKST" | "BYMONTH" | "BYMONTHDAY" | "BYSETPOS" | "BYWEEKNO" => {}
            other => anyhow::bail!("unsupported RRULE key {other:?}"),
        }
    }

    if count.is_some() && until.is_some() {
        anyhow::bail!("COUNT and UNTIL are mutually exclusive");
    }

    let cadence = match freq {
        Some("DAILY") => Cadence::Daily { interval },
        Some("WEEKLY") => {
            let days = byday.ok_or_else(|| {
                anyhow::anyhow!("FREQ=WEEKLY requires BYDAY in the supported subset")
            })?;
            Cadence::Weekdays { days }
        }
        Some(_) => unreachable!(),
        None => anyhow::bail!("RRULE missing required FREQ"),
    };
    let bound = match (count, until) {
        (Some(n), _) => Bound::Count(n),
        (_, Some(d)) => Bound::Until(d),
        _ => Bound::None,
    };

    Ok(ParsedRRule { cadence, bound })
}

// =====================================================================
// Expansion.
// =====================================================================

/// Expand an RRULE into concrete dates falling in `[from, to]`,
/// anchored at the routine's `start`. Bounds and the global cap are
/// applied.
///
/// Returns dates ascending; never includes dates before `start`.
pub fn expand(
    rule: &ParsedRRule,
    start: NaiveDate,
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<NaiveDate> {
    if to < start || to < from {
        return Vec::new();
    }
    let lo = from.max(start);
    let hi = to;

    let mut out = Vec::new();
    match &rule.cadence {
        Cadence::Daily { interval } => {
            let step = (*interval).max(1) as i64;
            // Walk the recurrence from `start` until we pass `hi` or hit a bound.
            let mut idx: u32 = 0;
            let mut emitted: u32 = 0;
            loop {
                let d = match start.checked_add_days(Days::new((idx as i64 * step) as u64)) {
                    Some(v) => v,
                    None => break,
                };
                if d > hi {
                    break;
                }
                if let Bound::Until(u) = rule.bound {
                    if d > u {
                        break;
                    }
                }
                if let Bound::Count(n) = rule.bound {
                    if emitted >= n {
                        break;
                    }
                }
                emitted += 1;
                if d >= lo {
                    out.push(d);
                    if out.len() >= MAX_INSTANCES_PER_PASS {
                        break;
                    }
                }
                idx += 1;
            }
        }
        Cadence::Weekdays { days } => {
            // For weekly we walk day-by-day; each match counts toward Bound::Count.
            let mut d = start;
            let mut emitted: u32 = 0;
            while d <= hi {
                if let Bound::Until(u) = rule.bound {
                    if d > u {
                        break;
                    }
                }
                if days.contains(&d.weekday()) {
                    if let Bound::Count(n) = rule.bound {
                        if emitted >= n {
                            break;
                        }
                    }
                    emitted += 1;
                    if d >= lo {
                        out.push(d);
                        if out.len() >= MAX_INSTANCES_PER_PASS {
                            break;
                        }
                    }
                }
                d = match d.checked_add_days(Days::new(1)) {
                    Some(v) => v,
                    None => break,
                };
            }
        }
    }
    out
}

// =====================================================================
// Materialisation.
// =====================================================================

/// Generate any new instances for `routine` from `start_date` up to
/// `horizon_to`, returning *only* the instances that don't yet exist
/// in `existing`. Idempotent — calling repeatedly is safe and produces
/// no churn once the window is full.
///
/// The returned instances carry zeros for the score fields; the caller
/// (typically `Db::routine_instances_upsert_many`) appends them to
/// the global instances file.
pub fn materialize_routine(
    routine: &Routine,
    horizon_to: NaiveDate,
    existing: &[RoutineInstance],
) -> anyhow::Result<Vec<RoutineInstance>> {
    if routine.paused {
        return Ok(Vec::new());
    }
    let parsed = parse_rrule(&routine.rrule)?;
    let start = NaiveDate::parse_from_str(&routine.start_date, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("routine.start_date must be YYYY-MM-DD"))?;

    // Apply Goal::Deadline as an effective upper bound — never schedule
    // past the user's stated end date.
    let effective_to = match &routine.goal {
        Goal::Deadline { until } => {
            let d = NaiveDate::parse_from_str(until, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("routine.goal.until must be YYYY-MM-DD"))?;
            horizon_to.min(d)
        }
        _ => horizon_to,
    };

    let dates = expand(&parsed, start, start, effective_to);

    // Goal::Count caps total scheduled instances at `target`.
    let cap = match &routine.goal {
        Goal::Count { target, .. } => Some(*target as usize),
        _ => None,
    };

    let mut existing_dates: std::collections::HashSet<&str> = existing
        .iter()
        .filter(|i| i.routine_id == routine.id)
        .map(|i| i.scheduled_for.as_str())
        .collect();

    let mut new = Vec::new();
    let scheduled_so_far_for_routine = existing
        .iter()
        .filter(|i| i.routine_id == routine.id)
        .count();

    for d in dates {
        if let Some(c) = cap {
            if scheduled_so_far_for_routine + new.len() >= c {
                break;
            }
        }
        let iso = d.format("%Y-%m-%d").to_string();
        if existing_dates.insert(Box::leak(iso.clone().into_boxed_str())) {
            // `insert` returned true → wasn't there yet. Add it.
            new.push(RoutineInstance {
                id: format!("ri_{}_{}", short_id(&routine.id), iso),
                routine_id: routine.id.clone(),
                scheduled_for: iso,
                done_at: None,
                skipped: None,
                extension_contribution: 0,
                failing_points: 0.0,
                success_points: 0.0,
            });
        }
    }
    Ok(new)
}

fn short_id(s: &str) -> String {
    s.chars().take(8).collect()
}

// =====================================================================
// Miss detection (24-hour grace).
// =====================================================================

/// Result of a single overdue-check pass.
#[derive(Debug, Clone, Default)]
pub struct OverdueResult {
    /// Indices into the input slice for instances that transitioned to
    /// "missed" (failing points just accrued).
    pub newly_missed: Vec<usize>,
    /// Optional extension event to record on the routine for `Goal::Count`
    /// projections — `None` for Deadline / Indefinite goals.
    pub extension_events: Vec<ExtensionEvent>,
}

/// Apply the 24-hour grace miss logic to an instance slice in place.
/// Per-routine daily fail-cap is enforced against the *cumulative*
/// failing points the routine's instances have accrued so far today.
pub fn apply_overdue(
    routine: &Routine,
    instances: &mut [RoutineInstance],
    now: DateTime<Utc>,
) -> OverdueResult {
    if routine.paused {
        return OverdueResult::default();
    }
    let weight = routine.priority.weight();
    let per_miss_fail = FAIL_POINTS_PER_DAY_LATE * weight;

    // Pre-tally how much fail this routine already accrued *today* so
    // we can enforce FAIL_CAP_PER_ROUTINE_PER_DAY without dropping a
    // backlog of misses on the user.
    let today_iso = now.date_naive().format("%Y-%m-%d").to_string();
    let mut accrued_today: f64 = 0.0;
    for i in instances.iter() {
        if i.failing_points > 0.0 && i.scheduled_for == today_iso {
            accrued_today += i.failing_points;
        }
    }

    let mut out = OverdueResult::default();

    for (idx, inst) in instances.iter_mut().enumerate() {
        if inst.done_at.is_some() || inst.skipped == Some(true) {
            continue;
        }
        if inst.failing_points > 0.0 {
            continue; // already processed
        }
        let scheduled = match NaiveDate::parse_from_str(&inst.scheduled_for, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        // End-of-day for the scheduled date, then +24h grace.
        let scheduled_eod = scheduled
            .and_hms_opt(23, 59, 59)
            .map(|n| n.and_utc())
            .unwrap();
        let missed_threshold = scheduled_eod + chrono::Duration::hours(24);
        if now < missed_threshold {
            continue;
        }

        // Apply per-day cap (only counts misses scheduled for today).
        let mut to_apply = per_miss_fail;
        if inst.scheduled_for == today_iso {
            let remaining = (FAIL_CAP_PER_ROUTINE_PER_DAY - accrued_today).max(0.0);
            to_apply = to_apply.min(remaining);
            accrued_today += to_apply;
        }
        inst.failing_points = to_apply;
        out.newly_missed.push(idx);

        if let Goal::Count { .. } = routine.goal {
            // Count goals: every miss extends the projected completion
            // by one cadence step. We log the event without re-applying
            // the points (instance already carries them) so the routine
            // total isn't double-counted.
            let cadence_days = cadence_days_estimate(routine).unwrap_or(1);
            let from_iso = inst.scheduled_for.clone();
            let to_iso = scheduled
                .checked_add_days(Days::new(cadence_days as u64))
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| from_iso.clone());
            inst.extension_contribution = cadence_days as i64;
            out.extension_events.push(ExtensionEvent {
                from: from_iso,
                to: to_iso,
                reason: ExtensionReason::AutoMissed,
                failing_points_applied: 0.0, // points live on the instance
                at: now.to_rfc3339(),
                note: None,
            });
        }
    }
    out
}

/// Best-effort cadence in days. For weekly routines we approximate
/// `7 / N` so the projection slides at a reasonable pace.
pub fn cadence_days_estimate(routine: &Routine) -> Option<u32> {
    let parsed = parse_rrule(&routine.rrule).ok()?;
    Some(match parsed.cadence {
        Cadence::Daily { interval } => interval.max(1),
        Cadence::Weekdays { days } => {
            let n = days.len().max(1) as u32;
            (7 / n).max(1)
        }
    })
}

// =====================================================================
// Scoring + projected completion.
// =====================================================================

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoutineScore {
    pub success_points: f64,
    pub failing_points: f64,
    pub completed_count: u32,
}

/// Recompute success/fail totals for one routine from its instances.
/// Pure — caller writes the result back.
pub fn routine_score(routine: &Routine, instances: &[RoutineInstance]) -> RoutineScore {
    let weight = routine.priority.weight();
    let base = BASE_TASK_POINTS * weight;

    let mut s = 0.0_f64;
    let mut f = 0.0_f64;
    let mut completed: u32 = 0;

    for inst in instances.iter().filter(|i| i.routine_id == routine.id) {
        if let Some(done_at) = inst.done_at.as_deref() {
            let scheduled = match NaiveDate::parse_from_str(&inst.scheduled_for, "%Y-%m-%d") {
                Ok(d) => d.and_hms_opt(23, 59, 59).map(|n| n.and_utc()).unwrap(),
                Err(_) => continue,
            };
            let done = DateTime::parse_from_rfc3339(done_at)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(scheduled);
            let days_late = if done > scheduled {
                (done - scheduled).num_seconds() as f64 / 86_400.0
            } else {
                0.0
            };
            let success = base * (1.0 - LATE_PENALTY_PER_DAY * days_late).max(LATE_SUCCESS_FLOOR);
            s += success;
            completed += 1;
        }
        // Failing points already live on the instance from the miss tick.
        f += inst.failing_points;
    }

    RoutineScore {
        success_points: s,
        failing_points: f,
        completed_count: completed,
    }
}

/// Naive projection: today + cadence × (target - completed). Returns
/// `None` for non-Count goals or unparseable cadences. For Count goals
/// where the target has already been hit, returns the date of the
/// final completion.
pub fn projected_completion(
    routine: &Routine,
    instances: &[RoutineInstance],
    today: NaiveDate,
) -> Option<NaiveDate> {
    let target = match &routine.goal {
        Goal::Count { target, .. } => *target,
        _ => return None,
    };
    let completed: i64 = instances
        .iter()
        .filter(|i| i.routine_id == routine.id && i.done_at.is_some())
        .count() as i64;
    let remaining = (target - completed).max(0);
    if remaining == 0 {
        // Already complete — return the latest done_at scheduled date.
        let last = instances
            .iter()
            .filter(|i| i.routine_id == routine.id && i.done_at.is_some())
            .filter_map(|i| NaiveDate::parse_from_str(&i.scheduled_for, "%Y-%m-%d").ok())
            .max();
        return last;
    }
    let cadence = cadence_days_estimate(routine)? as i64;
    today.checked_add_days(Days::new((cadence * remaining) as u64))
}

// =====================================================================
// Tests.
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::Priority;
    use chrono::TimeZone;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn at(y: i32, m: u32, day: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, day, h, 0, 0).unwrap()
    }

    fn mk_routine(rrule: &str, start: &str, goal: Goal) -> Routine {
        Routine {
            id: "r1".into(),
            project_id: None,
            title: "ship video".into(),
            description: None,
            rrule: rrule.into(),
            start_date: start.into(),
            goal,
            priority: Priority::P2,
            estimate: None,
            paused: false,
            paused_from: None,
            success_points: 0.0,
            failing_points: 0.0,
            extensions: Vec::new(),
            created_at: "2026-04-30T00:00:00Z".into(),
        }
    }

    // ---- parser ----

    #[test]
    fn parses_daily_default_interval() {
        let p = parse_rrule("FREQ=DAILY").unwrap();
        assert_eq!(p.cadence, Cadence::Daily { interval: 1 });
        assert_eq!(p.bound, Bound::None);
    }

    #[test]
    fn parses_daily_with_interval_and_count() {
        let p = parse_rrule("FREQ=DAILY;INTERVAL=2;COUNT=100").unwrap();
        assert_eq!(p.cadence, Cadence::Daily { interval: 2 });
        assert_eq!(p.bound, Bound::Count(100));
    }

    #[test]
    fn parses_weekly_byday() {
        let p = parse_rrule("FREQ=WEEKLY;BYDAY=MO,WE,FR").unwrap();
        match p.cadence {
            Cadence::Weekdays { days } => {
                assert_eq!(days, vec![Weekday::Mon, Weekday::Wed, Weekday::Fri])
            }
            other => panic!("expected weekdays, got {other:?}"),
        }
    }

    #[test]
    fn parses_until_in_both_formats() {
        assert_eq!(
            parse_rrule("FREQ=DAILY;UNTIL=20260615").unwrap().bound,
            Bound::Until(d(2026, 6, 15))
        );
        assert_eq!(
            parse_rrule("FREQ=DAILY;UNTIL=2026-06-15").unwrap().bound,
            Bound::Until(d(2026, 6, 15))
        );
    }

    #[test]
    fn rejects_count_and_until_together() {
        let err = parse_rrule("FREQ=DAILY;COUNT=10;UNTIL=20260101");
        assert!(err.is_err());
    }

    #[test]
    fn rejects_unsupported_freq() {
        assert!(parse_rrule("FREQ=MONTHLY").is_err());
    }

    #[test]
    fn rrule_prefix_is_tolerated() {
        let p = parse_rrule("RRULE:FREQ=DAILY;INTERVAL=3").unwrap();
        assert_eq!(p.cadence, Cadence::Daily { interval: 3 });
    }

    // ---- expansion ----

    #[test]
    fn expand_daily_every_two_days() {
        let p = parse_rrule("FREQ=DAILY;INTERVAL=2").unwrap();
        let dates = expand(&p, d(2026, 5, 1), d(2026, 5, 1), d(2026, 5, 10));
        assert_eq!(
            dates,
            vec![
                d(2026, 5, 1),
                d(2026, 5, 3),
                d(2026, 5, 5),
                d(2026, 5, 7),
                d(2026, 5, 9),
            ]
        );
    }

    #[test]
    fn expand_count_caps_total() {
        let p = parse_rrule("FREQ=DAILY;COUNT=3").unwrap();
        let dates = expand(&p, d(2026, 5, 1), d(2026, 5, 1), d(2026, 5, 30));
        assert_eq!(dates, vec![d(2026, 5, 1), d(2026, 5, 2), d(2026, 5, 3)]);
    }

    #[test]
    fn expand_weekly_byday_filters_correctly() {
        // 2026-05-01 is a Friday.
        let p = parse_rrule("FREQ=WEEKLY;BYDAY=MO,FR").unwrap();
        let dates = expand(&p, d(2026, 5, 1), d(2026, 5, 1), d(2026, 5, 14));
        assert_eq!(
            dates,
            vec![d(2026, 5, 1), d(2026, 5, 4), d(2026, 5, 8), d(2026, 5, 11)]
        );
    }

    #[test]
    fn expand_respects_until_bound() {
        let p = parse_rrule("FREQ=DAILY;UNTIL=20260505").unwrap();
        let dates = expand(&p, d(2026, 5, 1), d(2026, 5, 1), d(2026, 5, 30));
        assert_eq!(dates.len(), 5);
        assert_eq!(dates.last(), Some(&d(2026, 5, 5)));
    }

    // ---- materialise ----

    #[test]
    fn materialize_is_idempotent() {
        let r = mk_routine(
            "FREQ=DAILY;COUNT=5",
            "2026-05-01",
            Goal::Count {
                target: 5,
                completed: 0,
            },
        );
        let first = materialize_routine(&r, d(2026, 5, 30), &[]).unwrap();
        assert_eq!(first.len(), 5);
        let second = materialize_routine(&r, d(2026, 5, 30), &first).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn materialize_stops_at_count_target() {
        // Goal Count(3) caps total instances even if RRULE is unbounded.
        let r = mk_routine(
            "FREQ=DAILY",
            "2026-05-01",
            Goal::Count {
                target: 3,
                completed: 0,
            },
        );
        let first = materialize_routine(&r, d(2026, 5, 30), &[]).unwrap();
        assert_eq!(first.len(), 3);
    }

    #[test]
    fn materialize_paused_routine_is_noop() {
        let mut r = mk_routine("FREQ=DAILY", "2026-05-01", Goal::Indefinite);
        r.paused = true;
        let out = materialize_routine(&r, d(2026, 5, 30), &[]).unwrap();
        assert!(out.is_empty());
    }

    // ---- overdue ----

    #[test]
    fn apply_overdue_marks_24h_late_instances_missed() {
        let r = mk_routine(
            "FREQ=DAILY",
            "2026-05-01",
            Goal::Count {
                target: 100,
                completed: 0,
            },
        );
        let mut insts = vec![RoutineInstance {
            id: "i1".into(),
            routine_id: "r1".into(),
            scheduled_for: "2026-05-01".into(),
            done_at: None,
            skipped: None,
            extension_contribution: 0,
            failing_points: 0.0,
            success_points: 0.0,
        }];
        // 25h past end-of-day on 2026-05-01.
        let now = at(2026, 5, 3, 1);
        let res = apply_overdue(&r, &mut insts, now);
        assert_eq!(res.newly_missed, vec![0]);
        assert!(insts[0].failing_points > 0.0);
        assert_eq!(res.extension_events.len(), 1);
        assert_eq!(res.extension_events[0].reason, ExtensionReason::AutoMissed);
    }

    #[test]
    fn apply_overdue_skips_done_and_within_grace() {
        let r = mk_routine("FREQ=DAILY", "2026-05-01", Goal::Indefinite);
        let mut insts = vec![
            RoutineInstance {
                id: "i1".into(),
                routine_id: "r1".into(),
                scheduled_for: "2026-05-01".into(),
                done_at: Some("2026-05-01T20:00:00Z".into()),
                skipped: None,
                extension_contribution: 0,
                failing_points: 0.0,
                success_points: 0.0,
            },
            RoutineInstance {
                id: "i2".into(),
                routine_id: "r1".into(),
                scheduled_for: "2026-05-02".into(),
                done_at: None,
                skipped: None,
                extension_contribution: 0,
                failing_points: 0.0,
                success_points: 0.0,
            },
        ];
        // Only 12h past end-of-day on 2026-05-02 — still inside grace.
        let now = at(2026, 5, 3, 12);
        let res = apply_overdue(&r, &mut insts, now);
        assert!(res.newly_missed.is_empty());
        assert_eq!(insts[0].failing_points, 0.0);
        assert_eq!(insts[1].failing_points, 0.0);
    }

    #[test]
    fn apply_overdue_does_not_double_charge() {
        // Running the tick twice should be a no-op the second time.
        let r = mk_routine("FREQ=DAILY", "2026-05-01", Goal::Indefinite);
        let mut insts = vec![RoutineInstance {
            id: "i1".into(),
            routine_id: "r1".into(),
            scheduled_for: "2026-05-01".into(),
            done_at: None,
            skipped: None,
            extension_contribution: 0,
            failing_points: 0.0,
            success_points: 0.0,
        }];
        let now = at(2026, 5, 3, 1);
        let _ = apply_overdue(&r, &mut insts, now);
        let again = apply_overdue(&r, &mut insts, now);
        assert!(again.newly_missed.is_empty());
    }

    // ---- score / projection ----

    #[test]
    fn score_aggregates_done_and_missed() {
        let r = mk_routine("FREQ=DAILY", "2026-05-01", Goal::Indefinite);
        let mut insts = vec![
            RoutineInstance {
                id: "i1".into(),
                routine_id: "r1".into(),
                scheduled_for: "2026-05-01".into(),
                done_at: Some("2026-05-01T12:00:00Z".into()),
                skipped: None,
                extension_contribution: 0,
                failing_points: 0.0,
                success_points: 0.0,
            },
            RoutineInstance {
                id: "i2".into(),
                routine_id: "r1".into(),
                scheduled_for: "2026-05-02".into(),
                done_at: None,
                skipped: None,
                extension_contribution: 0,
                failing_points: 12.0, // pre-set as if a tick already ran
                success_points: 0.0,
            },
        ];
        let _ = &mut insts;
        let s = routine_score(&r, &insts);
        assert_eq!(s.completed_count, 1);
        assert!(s.success_points > 0.0);
        assert_eq!(s.failing_points, 12.0);
    }

    #[test]
    fn projection_extends_naturally_with_misses() {
        let r = mk_routine(
            "FREQ=DAILY;INTERVAL=2",
            "2026-05-01",
            Goal::Count {
                target: 100,
                completed: 0,
            },
        );
        // 0 done so far, today is May 5.
        let proj = projected_completion(&r, &[], d(2026, 5, 5));
        // 100 remaining * 2 days = 200 days from May 5 → 2026-11-21.
        assert_eq!(proj, Some(d(2026, 11, 21)));
    }
}
