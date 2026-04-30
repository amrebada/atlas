//! Schema migration utilities for the planner feature.
//!
//! P1 ships an *additive* schema: all new `Todo` fields are optional and
//! existing `.atlas/todos.json` files load unchanged. The single piece
//! that needs interpretation is the legacy free-form `due` string —
//! `try_parse_legacy_due` translates the patterns we see in the wild
//! ("today", "tomorrow", weekday names, ISO-8601 prefixes) into
//! structured ISO dates the new `deadline` field can hold.
//!
//! The function is intentionally conservative: anything it doesn't
//! recognise returns `None`, leaving the legacy `due` in place for a
//! human to reconcile. No data is destroyed at parse time.

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc, Weekday};

/// Try to interpret a legacy `Todo.due` string as an ISO-8601 date.
///
/// Returns the parsed date as `YYYY-MM-DD`, or `None` if the input
/// doesn't match any recognised pattern. The reference time is
/// passed in (rather than read via `Utc::now`) so callers can make the
/// behaviour deterministic in tests and reproducible for migrations
/// that batch-process many files.
pub fn try_parse_legacy_due(input: &str, now: DateTime<Utc>) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let today = now.date_naive();

    // ISO-8601 prefix — accept anything whose leading 10 characters
    // parse as `YYYY-MM-DD`. Tolerates trailing time-of-day or zone
    // suffixes ("2026-05-12T09:00:00Z") because the deadline field is
    // date-only.
    if trimmed.len() >= 10 {
        if let Ok(d) = NaiveDate::parse_from_str(&trimmed[..10], "%Y-%m-%d") {
            return Some(d.format("%Y-%m-%d").to_string());
        }
    }

    // `today` / `tomorrow` keywords.
    match lower.as_str() {
        "today" => return Some(today.format("%Y-%m-%d").to_string()),
        "tomorrow" => {
            return today
                .checked_add_days(Days::new(1))
                .map(|d| d.format("%Y-%m-%d").to_string());
        }
        _ => {}
    }

    // Weekday names (full or common abbreviations). Resolves to the
    // soonest matching date on or after today (if today is Friday and
    // the user typed "fri", they meant today).
    let target = match lower.as_str() {
        "monday" | "mon" => Some(Weekday::Mon),
        "tuesday" | "tue" | "tues" => Some(Weekday::Tue),
        "wednesday" | "wed" => Some(Weekday::Wed),
        "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thu),
        "friday" | "fri" => Some(Weekday::Fri),
        "saturday" | "sat" => Some(Weekday::Sat),
        "sunday" | "sun" => Some(Weekday::Sun),
        _ => None,
    };
    if let Some(weekday) = target {
        let mut d = today;
        for _ in 0..7 {
            if d.weekday() == weekday {
                return Some(d.format("%Y-%m-%d").to_string());
            }
            d = d.checked_add_days(Days::new(1))?;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Wednesday 2026-04-29 12:00:00 UTC — referenced by every test so
    /// "today" / "tomorrow" / weekday math is deterministic.
    fn anchor() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 29, 12, 0, 0).unwrap()
    }

    #[test]
    fn parses_today_keyword() {
        assert_eq!(
            try_parse_legacy_due("today", anchor()),
            Some("2026-04-29".to_string())
        );
        assert_eq!(
            try_parse_legacy_due("  Today ", anchor()),
            Some("2026-04-29".to_string())
        );
    }

    #[test]
    fn parses_tomorrow_keyword() {
        assert_eq!(
            try_parse_legacy_due("tomorrow", anchor()),
            Some("2026-04-30".to_string())
        );
    }

    #[test]
    fn parses_iso_prefix() {
        assert_eq!(
            try_parse_legacy_due("2026-05-12", anchor()),
            Some("2026-05-12".to_string())
        );
        // Trailing time-of-day is tolerated.
        assert_eq!(
            try_parse_legacy_due("2026-05-12T09:00:00Z", anchor()),
            Some("2026-05-12".to_string())
        );
    }

    #[test]
    fn weekday_resolves_to_next_occurrence() {
        // Anchor is Wednesday — "fri" should land 2 days later.
        assert_eq!(
            try_parse_legacy_due("fri", anchor()),
            Some("2026-05-01".to_string())
        );
        assert_eq!(
            try_parse_legacy_due("Friday", anchor()),
            Some("2026-05-01".to_string())
        );
    }

    #[test]
    fn weekday_today_resolves_to_today() {
        // Anchor is Wednesday — "wed" / "wednesday" means today.
        assert_eq!(
            try_parse_legacy_due("wed", anchor()),
            Some("2026-04-29".to_string())
        );
        assert_eq!(
            try_parse_legacy_due("wednesday", anchor()),
            Some("2026-04-29".to_string())
        );
    }

    #[test]
    fn weekday_in_past_wraps_to_next_week() {
        // Anchor is Wednesday — "tue" wraps to the following week.
        assert_eq!(
            try_parse_legacy_due("tue", anchor()),
            Some("2026-05-05".to_string())
        );
    }

    #[test]
    fn unrecognised_returns_none() {
        assert_eq!(try_parse_legacy_due("", anchor()), None);
        assert_eq!(try_parse_legacy_due("   ", anchor()), None);
        assert_eq!(try_parse_legacy_due("eventually", anchor()), None);
        assert_eq!(try_parse_legacy_due("next sprint", anchor()), None);
        assert_eq!(try_parse_legacy_due("not-a-date", anchor()), None);
    }
}
