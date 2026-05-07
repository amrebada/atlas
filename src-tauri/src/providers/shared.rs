//! Cross-provider helpers: HOME resolution, path canonicalization, status
//! heuristics, PATH lookup. Pulled out of the original `sessions.rs` so all
//! providers share consistent semantics.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::storage::types::SessionStatus;

/// Resolve the user's home directory without dragging in the `dirs` crate.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("HOME") {
        return Some(PathBuf::from(h));
    }
    #[cfg(windows)]
    if let Some(h) = std::env::var_os("USERPROFILE") {
        return Some(PathBuf::from(h));
    }
    None
}

/// Best-effort canonicalize - falls back to the input on error so unit
/// tests that operate on synthetic paths still match.
pub fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn paths_equal(a: &Path, b: &Path) -> bool {
    let ca = canonicalize_or_self(a);
    let cb = canonicalize_or_self(b);
    ca == cb || a == b
}

/// PATH lookup: returns the first executable matching `name`, if any.
/// `which` crate would handle the cross-platform PATHEXT logic for us, but
/// we want to avoid adding a dep just for this; manual scan is sufficient
/// because providers only need a presence check.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .ok()
            .map(|s| s.split(';').map(|e| e.to_lowercase()).collect())
            .unwrap_or_else(|| vec![".exe".into(), ".cmd".into(), ".bat".into()])
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Mtime-based status. "fresh" = touched within 5 minutes => Active.
pub fn derive_status(_last_event: DateTime<Utc>, mtime: SystemTime) -> SessionStatus {
    let now = SystemTime::now();
    let fresh = match now.duration_since(mtime) {
        Ok(d) => d.as_secs() < 5 * 60,
        Err(_) => true,
    };
    if fresh {
        SessionStatus::Active
    } else {
        SessionStatus::Idle
    }
}

/// Format a `chrono::Duration` as `"<n>h <nn>m"` / `"<n>m <nn>s"` / `"<n>s"`.
pub fn format_duration(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Single-line trim + truncation to `max` chars with `…` suffix.
pub fn truncate_prompt(s: &str, max: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

