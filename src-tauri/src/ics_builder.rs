//! ICS (RFC 5545) generator for milestones + routines.
//!
//! v1 export uses `VEVENT` even for tasks because Google Calendar
//! silently drops `VTODO` (see plan §8 / research). All emitted events
//! are non-busy (`TRANSP:TRANSPARENT`), so they don't block calendar
//! freeness checks. CRLF line endings + 75-octet line folding are
//! enforced per spec — many ad-hoc generators skip both and Apple
//! Calendar / Outlook reject silently.

#![allow(dead_code)]

use chrono::{DateTime, NaiveDate, Utc};

use crate::routine_engine::{parse_rrule, Bound};
use crate::storage::types::{Goal, Milestone, MilestoneStatus, Routine};

/// Wrap a list of pre-built `VEVENT` blocks into a complete VCALENDAR
/// payload. `name` shows up in calendar apps as the calendar title.
pub fn build_calendar(name: &str, events: &[String]) -> String {
    let mut out = String::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, "PRODID:-//Atlas Planner//EN");
    push_line(&mut out, "CALSCALE:GREGORIAN");
    push_line(&mut out, "METHOD:PUBLISH");
    push_line(&mut out, &format!("X-WR-CALNAME:{}", escape_text(name)));
    for evt in events {
        out.push_str(evt);
    }
    push_line(&mut out, "END:VCALENDAR");
    out
}

/// Build a single `VEVENT` for a milestone deadline. All-day on
/// the deadline date.
pub fn build_milestone_event(milestone: &Milestone, project_name: &str) -> String {
    let mut out = String::new();
    push_line(&mut out, "BEGIN:VEVENT");
    push_line(
        &mut out,
        &format!("UID:milestone-{}@atlas.local", milestone.id),
    );
    push_line(
        &mut out,
        &format!("SUMMARY:{}", escape_text(&milestone.title)),
    );
    if let Some(desc) = milestone.description.as_deref() {
        if !desc.trim().is_empty() {
            push_line(&mut out, &format!("DESCRIPTION:{}", escape_text(desc)));
        }
    }
    let dtstamp = milestone
        .created_at
        .parse::<DateTime<Utc>>()
        .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_else(|_| Utc::now().format("%Y%m%dT%H%M%SZ").to_string());
    push_line(&mut out, &format!("DTSTAMP:{}", dtstamp));
    push_line(
        &mut out,
        &format!("DTSTART;VALUE=DATE:{}", date_compact(&milestone.deadline)),
    );
    // All-day events: DTEND is the day *after* DTSTART per RFC 5545.
    let dtend = next_day_compact(&milestone.deadline);
    push_line(&mut out, &format!("DTEND;VALUE=DATE:{}", dtend));
    push_line(
        &mut out,
        &format!(
            "STATUS:{}",
            match milestone.status {
                MilestoneStatus::Cancelled => "CANCELLED",
                _ => "CONFIRMED",
            }
        ),
    );
    push_line(&mut out, "TRANSP:TRANSPARENT");
    push_line(
        &mut out,
        &format!(
            "CATEGORIES:{}",
            escape_text(&format!("Atlas,{},milestone", project_name))
        ),
    );
    push_line(&mut out, "END:VEVENT");
    out
}

/// Build a single `VEVENT` for a recurring routine. Carries an
/// `RRULE:` line so calendars expand the recurrence client-side.
/// Returns `None` for unparseable routine RRULEs (we'd rather skip
/// than emit an invalid event).
pub fn build_routine_event(routine: &Routine, project_name: Option<&str>) -> Option<String> {
    let parsed = parse_rrule(&routine.rrule).ok()?;

    let mut rrule_line = String::from("RRULE:");
    match &parsed.cadence {
        crate::routine_engine::Cadence::Daily { interval } => {
            rrule_line.push_str("FREQ=DAILY");
            if *interval > 1 {
                rrule_line.push_str(&format!(";INTERVAL={}", interval));
            }
        }
        crate::routine_engine::Cadence::Weekdays { days } => {
            rrule_line.push_str("FREQ=WEEKLY;BYDAY=");
            let codes: Vec<&str> = days
                .iter()
                .map(|w| match w {
                    chrono::Weekday::Mon => "MO",
                    chrono::Weekday::Tue => "TU",
                    chrono::Weekday::Wed => "WE",
                    chrono::Weekday::Thu => "TH",
                    chrono::Weekday::Fri => "FR",
                    chrono::Weekday::Sat => "SA",
                    chrono::Weekday::Sun => "SU",
                })
                .collect();
            rrule_line.push_str(&codes.join(","));
        }
    }
    // RFC 5545 forbids COUNT and UNTIL together — `parse_rrule`
    // already rejects that, so we can append at most one bound.
    match &parsed.bound {
        Bound::Count(n) => rrule_line.push_str(&format!(";COUNT={}", n)),
        Bound::Until(d) => rrule_line.push_str(&format!(";UNTIL={}", d.format("%Y%m%d"))),
        Bound::None => {
            // For Goal::Count we synthesize an UNTIL one cadence past
            // the projected end so calendars don't render an unbounded
            // recurrence past the user's intent.
            if let Goal::Count { target, .. } = &routine.goal {
                if let Some(cad) = crate::routine_engine::cadence_days_estimate(routine) {
                    if let Ok(start) = NaiveDate::parse_from_str(&routine.start_date, "%Y-%m-%d") {
                        let total_days = (cad as i64) * *target;
                        if let Some(end) =
                            start.checked_add_days(chrono::Days::new(total_days as u64))
                        {
                            rrule_line.push_str(&format!(";UNTIL={}", end.format("%Y%m%d")));
                        }
                    }
                }
            } else if let Goal::Deadline { until } = &routine.goal {
                if let Ok(d) = NaiveDate::parse_from_str(until, "%Y-%m-%d") {
                    rrule_line.push_str(&format!(";UNTIL={}", d.format("%Y%m%d")));
                }
            }
        }
    }

    let mut out = String::new();
    push_line(&mut out, "BEGIN:VEVENT");
    push_line(&mut out, &format!("UID:routine-{}@atlas.local", routine.id));
    push_line(
        &mut out,
        &format!("SUMMARY:{}", escape_text(&routine.title)),
    );
    if let Some(desc) = routine.description.as_deref() {
        if !desc.trim().is_empty() {
            push_line(&mut out, &format!("DESCRIPTION:{}", escape_text(desc)));
        }
    }
    let dtstamp = routine
        .created_at
        .parse::<DateTime<Utc>>()
        .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_else(|_| Utc::now().format("%Y%m%dT%H%M%SZ").to_string());
    push_line(&mut out, &format!("DTSTAMP:{}", dtstamp));
    push_line(
        &mut out,
        &format!("DTSTART;VALUE=DATE:{}", date_compact(&routine.start_date)),
    );
    let dtend = next_day_compact(&routine.start_date);
    push_line(&mut out, &format!("DTEND;VALUE=DATE:{}", dtend));
    push_line(&mut out, &rrule_line);
    push_line(&mut out, "STATUS:CONFIRMED");
    push_line(&mut out, "TRANSP:TRANSPARENT");
    push_line(
        &mut out,
        &format!(
            "CATEGORIES:{}",
            escape_text(&format!(
                "Atlas,{},routine",
                project_name.unwrap_or("global")
            ))
        ),
    );
    push_line(&mut out, "END:VEVENT");
    Some(out)
}

// ---------------------------------------------------------------
// Encoding helpers.
// ---------------------------------------------------------------

const CRLF: &str = "\r\n";
const FOLD_LIMIT: usize = 75;

fn push_line(out: &mut String, line: &str) {
    out.push_str(&fold_line(line));
    out.push_str(CRLF);
}

/// RFC 5545 §3.1: lines longer than 75 *octets* must be folded with
/// `CRLF` followed by a single space. Continuation lines may not
/// exceed 75 octets either, so we fold iteratively. Octet length
/// (UTF-8 byte count) matters here — character count would under-fold
/// non-ASCII strings.
pub fn fold_line(line: &str) -> String {
    let bytes = line.as_bytes();
    if bytes.len() <= FOLD_LIMIT {
        return line.to_string();
    }
    let mut out = String::with_capacity(bytes.len() + bytes.len() / FOLD_LIMIT * 3);
    let mut i = 0;
    let mut first = true;
    while i < bytes.len() {
        let limit = if first { FOLD_LIMIT } else { FOLD_LIMIT - 1 };
        let mut end = (i + limit).min(bytes.len());
        // Avoid splitting in the middle of a UTF-8 sequence.
        while end < bytes.len() && (bytes[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        let slice = std::str::from_utf8(&bytes[i..end]).expect("byte boundaries respected above");
        if !first {
            out.push_str(CRLF);
            out.push(' ');
        }
        out.push_str(slice);
        first = false;
        i = end;
    }
    out
}

/// RFC 5545 §3.3.11: text values escape `\\`, `,`, `;`, and newlines.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {} // strip — \n already covers the line-break case
            _ => out.push(ch),
        }
    }
    out
}

fn date_compact(iso: &str) -> String {
    // `YYYY-MM-DD` → `YYYYMMDD` (the form RFC 5545 expects).
    iso.replace('-', "")
}

fn next_day_compact(iso: &str) -> String {
    NaiveDate::parse_from_str(iso, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.succ_opt())
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_else(|| date_compact(iso))
}

// ---------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::{Goal, MilestoneStatus, Priority};

    fn mk_milestone() -> Milestone {
        Milestone {
            id: "m1".into(),
            project_id: "p1".into(),
            title: "Ship v0.4".into(),
            description: Some("First public alpha".into()),
            deadline: "2026-05-15".into(),
            original_deadline: "2026-05-15".into(),
            status: MilestoneStatus::Active,
            priority: Priority::P0,
            order: 0,
            todo_ids: Vec::new(),
            extensions: Vec::new(),
            success_points: 0.0,
            failing_points: 0.0,
            created_at: "2026-04-30T12:00:00Z".into(),
            done_at: None,
        }
    }

    fn mk_routine(rrule: &str, goal: Goal) -> Routine {
        Routine {
            id: "r1".into(),
            project_id: Some("p1".into()),
            title: "Ship a video".into(),
            description: None,
            rrule: rrule.into(),
            start_date: "2026-05-01".into(),
            goal,
            priority: Priority::P1,
            estimate: None,
            paused: false,
            paused_from: None,
            success_points: 0.0,
            failing_points: 0.0,
            extensions: Vec::new(),
            created_at: "2026-04-30T12:00:00Z".into(),
        }
    }

    #[test]
    fn fold_line_under_limit_is_passthrough() {
        assert_eq!(fold_line("SHORT:value"), "SHORT:value");
    }

    #[test]
    fn fold_line_long_input_is_split_with_continuation_space() {
        let long = "X".repeat(150);
        let folded = fold_line(&long);
        for line in folded.split("\r\n") {
            assert!(
                line.len() <= FOLD_LIMIT,
                "line {:?} exceeds {FOLD_LIMIT}",
                line.len()
            );
        }
        // Continuation lines start with a single space.
        let parts: Vec<&str> = folded.split("\r\n").collect();
        for part in parts.iter().skip(1) {
            assert!(part.starts_with(' '), "continuation must start with space");
        }
    }

    #[test]
    fn escape_text_handles_special_chars() {
        assert_eq!(escape_text("a, b; c\nd\\e"), "a\\, b\\; c\\nd\\\\e");
    }

    #[test]
    fn milestone_event_has_required_fields() {
        let evt = build_milestone_event(&mk_milestone(), "Atlas");
        // Must contain core fields and use CRLF.
        assert!(evt.contains("BEGIN:VEVENT\r\n"));
        assert!(evt.contains("UID:milestone-m1@atlas.local\r\n"));
        assert!(evt.contains("SUMMARY:Ship v0.4\r\n"));
        assert!(evt.contains("DTSTART;VALUE=DATE:20260515\r\n"));
        // DTEND is the day after for an all-day event.
        assert!(evt.contains("DTEND;VALUE=DATE:20260516\r\n"));
        assert!(evt.contains("STATUS:CONFIRMED\r\n"));
        assert!(evt.contains("TRANSP:TRANSPARENT\r\n"));
        assert!(evt.contains("END:VEVENT\r\n"));
    }

    #[test]
    fn routine_event_count_goal_emits_count_bound() {
        let r = mk_routine(
            "FREQ=DAILY;INTERVAL=2",
            Goal::Count {
                target: 10,
                completed: 0,
            },
        );
        let evt = build_routine_event(&r, Some("Atlas")).unwrap();
        assert!(evt.contains("RRULE:FREQ=DAILY;INTERVAL=2;UNTIL="));
    }

    #[test]
    fn routine_event_explicit_count_bound_passes_through() {
        let r = mk_routine("FREQ=DAILY;COUNT=5", Goal::Indefinite);
        let evt = build_routine_event(&r, None).unwrap();
        assert!(evt.contains("RRULE:FREQ=DAILY;COUNT=5\r\n"));
    }

    #[test]
    fn routine_event_weekly_byday() {
        let r = mk_routine("FREQ=WEEKLY;BYDAY=MO,FR", Goal::Indefinite);
        let evt = build_routine_event(&r, Some("Atlas")).unwrap();
        assert!(evt.contains("RRULE:FREQ=WEEKLY;BYDAY=MO,FR"));
    }

    #[test]
    fn routine_event_invalid_rrule_returns_none() {
        let r = mk_routine("FREQ=MONTHLY", Goal::Indefinite);
        assert!(build_routine_event(&r, Some("Atlas")).is_none());
    }

    #[test]
    fn calendar_envelope_is_well_formed() {
        let evt = build_milestone_event(&mk_milestone(), "Atlas");
        let cal = build_calendar("Atlas — Test", &[evt]);
        assert!(cal.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(cal.contains("VERSION:2.0\r\n"));
        assert!(cal.contains("PRODID:-//Atlas Planner//EN\r\n"));
        assert!(cal.contains("X-WR-CALNAME:Atlas — Test\r\n"));
        assert!(cal.ends_with("END:VCALENDAR\r\n"));
    }
}
