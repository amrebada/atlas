//! Home dashboard - cross-project activity analytics.
//!
//! `home_dashboard(days)` walks every non-archived repo's local branches,
//! keeps the commits authored by the configured git identity inside the
//! window, and folds them into:
//!
//! * a per-day series (commits, distinct projects, estimated minutes),
//! * hour-of-day / weekday histograms,
//! * per-project rollups + the most recent commits,
//! * an "attention" list (dirty / ahead / behind) read straight from the
//!   index.
//!
//! "Minutes" is the git-hours heuristic: consecutive commits closer than
//! [`SESSION_GAP_SECS`] are one working session and the gap counts as
//! work; the first commit of a session is credited [`SESSION_HEAD_SECS`].

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};
use git2::{Repository, Sort};
use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use crate::storage::types::{Project, ProjectFilter};
use crate::storage::Db;

/// Two commits closer than this belong to the same working session.
const SESSION_GAP_SECS: i64 = 2 * 60 * 60;
/// Credit for the (unobservable) work before a session's first commit.
const SESSION_HEAD_SECS: i64 = 30 * 60;
/// Hard cap on commits walked per repo so a monorepo can't stall the page.
const MAX_COMMITS_PER_REPO: usize = 3000;
/// Diff stats are only computed for commits this recent (they're pricey).
const STATS_WINDOW_DAYS: i64 = 7;
const MAX_STATS_PER_REPO: usize = 150;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct HomeDay {
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    pub commits: u32,
    /// Distinct projects committed to that day.
    pub projects: u32,
    /// Estimated minutes worked (see module docs).
    pub minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct HomeCommit {
    pub project_id: String,
    pub project_name: String,
    pub project_color: String,
    /// Short (7-char) sha.
    pub sha: String,
    pub summary: String,
    /// RFC-3339 in local time.
    pub authored_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct HomeProject {
    pub project_id: String,
    pub name: String,
    pub color: String,
    pub branch: String,
    pub commits7d: u32,
    pub commits30d: u32,
    pub minutes7d: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_commit_at: Option<String>,
    #[ts(type = "number")]
    pub dirty: i64,
    #[ts(type = "number")]
    pub ahead: i64,
    #[ts(type = "number")]
    pub behind: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct HomeTotals {
    pub commits_today: u32,
    pub commits7d: u32,
    pub commits30d: u32,
    pub active_projects7d: u32,
    pub additions7d: u32,
    pub deletions7d: u32,
    pub minutes_today: u32,
    pub minutes7d: u32,
    pub minutes30d: u32,
    /// Consecutive days (ending today or yesterday) with ≥1 commit.
    pub streak_days: u32,
    pub dirty_projects: u32,
    pub ahead_projects: u32,
    pub behind_projects: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct HomeDashboard {
    pub generated_at: String,
    /// Git identity the commits were filtered by (`None` → every author).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub range_days: u32,
    /// Oldest → newest, exactly `range_days` entries.
    pub days: Vec<HomeDay>,
    /// Commits per local hour-of-day over the last 30 days (24 buckets).
    pub hours: Vec<u32>,
    /// Commits per weekday over the last 30 days, Monday first (7 buckets).
    pub weekdays: Vec<u32>,
    pub recent_commits: Vec<HomeCommit>,
    /// Projects with activity in the window, most active first.
    pub projects: Vec<HomeProject>,
    /// Projects that are dirty / ahead / behind, most urgent first.
    pub attention: Vec<HomeProject>,
    pub totals: HomeTotals,
}

/// `home.dashboard` - build the home-page analytics for the last `days`
/// (default 84 = 12 weeks, clamped to 7..=365).
#[tracing::instrument(level = "info", skip_all, fields(days = ?days))]
#[tauri::command]
pub async fn home_dashboard(
    state: State<'_, Db>,
    days: Option<u32>,
) -> Result<HomeDashboard, String> {
    let range_days = days.unwrap_or(84).clamp(7, 365);
    let filter = ProjectFilter {
        include_archived: false,
        ..ProjectFilter::default()
    };
    let projects = state
        .list_projects(filter)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    tauri::async_runtime::spawn_blocking(move || {
        let identity = git_identity();
        build_dashboard(&projects, identity, Local::now(), range_days)
    })
    .await
    .map_err(|e| format!("join blocking: {e}"))
}

// ---- implementation ------------------------------------------------------

/// `(name, email)` from the default git config, lower-cased for matching.
#[derive(Debug, Clone)]
pub(crate) struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl Identity {
    fn label(&self) -> Option<String> {
        self.name.clone().or_else(|| self.email.clone())
    }
    fn matches(&self, name: Option<&str>, email: Option<&str>) -> bool {
        if self.name.is_none() && self.email.is_none() {
            return true;
        }
        let email_hit = match (&self.email, email) {
            (Some(mine), Some(theirs)) => mine.eq_ignore_ascii_case(theirs.trim()),
            _ => false,
        };
        let name_hit = match (&self.name, name) {
            (Some(mine), Some(theirs)) => mine.eq_ignore_ascii_case(theirs.trim()),
            _ => false,
        };
        email_hit || name_hit
    }
}

fn git_identity() -> Identity {
    let cfg = match git2::Config::open_default() {
        Ok(c) => c,
        Err(_) => {
            return Identity {
                name: None,
                email: None,
            }
        }
    };
    let get = |k: &str| {
        cfg.get_string(k)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    Identity {
        name: get("user.name"),
        email: get("user.email"),
    }
}

/// One commit by the user, already localised.
struct Hit {
    project_idx: usize,
    when: DateTime<Local>,
    sha: String,
    summary: String,
    additions: Option<u32>,
    deletions: Option<u32>,
}

pub(crate) fn build_dashboard(
    projects: &[Project],
    identity: Identity,
    now: DateTime<Local>,
    range_days: u32,
) -> HomeDashboard {
    let today = now.date_naive();
    let window_start = Local
        .from_local_datetime(
            &(today - Duration::days(i64::from(range_days) - 1))
                .and_hms_opt(0, 0, 0)
                .expect("midnight"),
        )
        .single()
        .unwrap_or(now);
    let since_secs = window_start.timestamp();
    let stats_since = (now - Duration::days(STATS_WINDOW_DAYS)).timestamp();

    let mut hits: Vec<Hit> = Vec::new();
    for (idx, p) in projects.iter().enumerate() {
        match collect_hits(Path::new(&p.path), idx, &identity, since_secs, stats_since) {
            Ok(mut h) => hits.append(&mut h),
            Err(e) => tracing::debug!(error = %e, path = %p.path, "home: skip repo"),
        }
    }
    hits.sort_by_key(|h| h.when);

    // ---- day series + histograms ----------------------------------------
    let mut days: Vec<HomeDay> = (0..range_days)
        .map(|i| HomeDay {
            date: (today - Duration::days(i64::from(range_days) - 1 - i64::from(i)))
                .format("%Y-%m-%d")
                .to_string(),
            commits: 0,
            projects: 0,
            minutes: 0,
        })
        .collect();
    let day_count = days.len();
    let day_index = move |d: chrono::NaiveDate| -> Option<usize> {
        let off = (d - (today - Duration::days(i64::from(range_days) - 1))).num_days();
        if off >= 0 && (off as usize) < day_count {
            Some(off as usize)
        } else {
            None
        }
    };
    let mut day_projects: Vec<HashSet<usize>> = vec![HashSet::new(); days.len()];
    let mut hours = vec![0u32; 24];
    let mut weekdays = vec![0u32; 7];
    let thirty = now - Duration::days(30);
    let seven = now - Duration::days(7);
    let today_start = Local
        .from_local_datetime(&today.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .unwrap_or(now);

    let mut totals = HomeTotals::default();
    let mut per_project: HashMap<usize, HomeProject> = HashMap::new();
    let mut active7: HashSet<usize> = HashSet::new();

    // Minutes via the session heuristic, attributed to each commit's day
    // and project.
    let mut prev: Option<DateTime<Local>> = None;
    for h in &hits {
        let credit_secs = match prev {
            Some(p) if (h.when - p).num_seconds() < SESSION_GAP_SECS => {
                (h.when - p).num_seconds().max(0)
            }
            _ => SESSION_HEAD_SECS,
        };
        prev = Some(h.when);
        let minutes = (credit_secs / 60) as u32;

        if let Some(i) = day_index(h.when.date_naive()) {
            days[i].commits += 1;
            days[i].minutes += minutes;
            day_projects[i].insert(h.project_idx);
        }
        if h.when >= thirty {
            hours[h.when.hour() as usize] += 1;
            weekdays[h.when.weekday().num_days_from_monday() as usize] += 1;
            totals.commits30d += 1;
            totals.minutes30d += minutes;
        }
        if h.when >= seven {
            totals.commits7d += 1;
            totals.minutes7d += minutes;
            totals.additions7d += h.additions.unwrap_or(0);
            totals.deletions7d += h.deletions.unwrap_or(0);
            active7.insert(h.project_idx);
        }
        if h.when >= today_start {
            totals.commits_today += 1;
            totals.minutes_today += minutes;
        }

        let p = &projects[h.project_idx];
        let entry = per_project
            .entry(h.project_idx)
            .or_insert_with(|| project_row(p));
        if h.when >= thirty {
            entry.commits30d += 1;
        }
        if h.when >= seven {
            entry.commits7d += 1;
            entry.minutes7d += minutes;
        }
        entry.last_commit_at = Some(h.when.to_rfc3339());
    }
    for (i, set) in day_projects.iter().enumerate() {
        days[i].projects = set.len() as u32;
    }
    totals.active_projects7d = active7.len() as u32;
    totals.streak_days = streak(&days);

    // ---- attention list (index-only, no git walk) -------------------------
    let mut attention: Vec<HomeProject> = projects
        .iter()
        .filter(|p| p.dirty > 0 || p.ahead > 0 || p.behind > 0)
        .map(|p| {
            let mut row = project_row(p);
            if let Some(idx) = projects.iter().position(|q| q.id == p.id) {
                if let Some(seen) = per_project.get(&idx) {
                    row.commits7d = seen.commits7d;
                    row.commits30d = seen.commits30d;
                    row.minutes7d = seen.minutes7d;
                    row.last_commit_at = seen.last_commit_at.clone();
                }
            }
            row
        })
        .collect();
    totals.dirty_projects = attention.iter().filter(|p| p.dirty > 0).count() as u32;
    totals.ahead_projects = attention.iter().filter(|p| p.ahead > 0).count() as u32;
    totals.behind_projects = attention.iter().filter(|p| p.behind > 0).count() as u32;
    attention.sort_by(|a, b| {
        b.ahead
            .cmp(&a.ahead)
            .then(b.dirty.cmp(&a.dirty))
            .then(b.behind.cmp(&a.behind))
            .then(a.name.cmp(&b.name))
    });
    attention.truncate(12);

    // ---- per-project rollup -------------------------------------------------
    let mut project_rows: Vec<HomeProject> = per_project.into_values().collect();
    project_rows.sort_by(|a, b| {
        b.commits7d
            .cmp(&a.commits7d)
            .then(b.commits30d.cmp(&a.commits30d))
            .then(b.last_commit_at.cmp(&a.last_commit_at))
    });
    project_rows.truncate(10);

    // ---- recent commits -----------------------------------------------------
    let recent_commits: Vec<HomeCommit> = hits
        .iter()
        .rev()
        .take(25)
        .map(|h| {
            let p = &projects[h.project_idx];
            HomeCommit {
                project_id: p.id.clone(),
                project_name: p.name.clone(),
                project_color: p.color.clone(),
                sha: h.sha.clone(),
                summary: h.summary.clone(),
                authored_at: h.when.to_rfc3339(),
                additions: h.additions,
                deletions: h.deletions,
            }
        })
        .collect();

    HomeDashboard {
        generated_at: now.to_rfc3339(),
        author: identity.label(),
        range_days,
        days,
        hours,
        weekdays,
        recent_commits,
        projects: project_rows,
        attention,
        totals,
    }
}

fn project_row(p: &Project) -> HomeProject {
    HomeProject {
        project_id: p.id.clone(),
        name: p.name.clone(),
        color: p.color.clone(),
        branch: p.branch.clone(),
        commits7d: 0,
        commits30d: 0,
        minutes7d: 0,
        last_commit_at: None,
        dirty: p.dirty,
        ahead: p.ahead,
        behind: p.behind,
    }
}

/// Consecutive days with commits, counting back from today (or from
/// yesterday when today is still empty, so a streak isn't broken mid-day).
fn streak(days: &[HomeDay]) -> u32 {
    let mut iter = days.iter().rev().peekable();
    if let Some(last) = iter.peek() {
        if last.commits == 0 {
            iter.next();
        }
    }
    iter.take_while(|d| d.commits > 0).count() as u32
}

/// Walk every local branch of the repo at `path` and return the user's
/// commits newer than `since_secs`. Diff stats are attached for commits
/// newer than `stats_since`, up to [`MAX_STATS_PER_REPO`].
fn collect_hits(
    path: &Path,
    project_idx: usize,
    identity: &Identity,
    since_secs: i64,
    stats_since: i64,
) -> anyhow::Result<Vec<Hit>> {
    let repo = Repository::open(path)?;
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME)?;
    if walk.push_glob("refs/heads/*").is_err() {
        walk.push_head()?;
    }

    let mut out: Vec<Hit> = Vec::new();
    let mut seen: HashSet<git2::Oid> = HashSet::new();
    let mut stats_budget = MAX_STATS_PER_REPO;

    for oid in walk.take(MAX_COMMITS_PER_REPO) {
        let oid = oid?;
        if !seen.insert(oid) {
            continue;
        }
        let commit = repo.find_commit(oid)?;
        let secs = commit.time().seconds();
        if secs < since_secs {
            // TIME sorting is newest-first; everything after this is older.
            break;
        }
        let author = commit.author();
        if !identity.matches(author.name(), author.email()) {
            continue;
        }
        let when = match Local.timestamp_opt(secs, 0).single() {
            Some(t) => t,
            None => continue,
        };

        let (additions, deletions) = if secs >= stats_since && stats_budget > 0 {
            stats_budget -= 1;
            match diff_stats(&repo, &commit) {
                Some((a, d)) => (Some(a), Some(d)),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        out.push(Hit {
            project_idx,
            when,
            sha: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("").trim().to_string(),
            additions,
            deletions,
        });
    }
    Ok(out)
}

fn diff_stats(repo: &Repository, commit: &git2::Commit<'_>) -> Option<(u32, u32)> {
    let tree = commit.tree().ok()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
        .ok()?;
    let stats = diff.stats().ok()?;
    Some((
        stats.insertions().min(u32::MAX as usize) as u32,
        stats.deletions().min(u32::MAX as usize) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::Lang;
    use git2::{Signature, Time};
    use std::fs;

    struct TmpDir(std::path::PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "atlas-home-test-{}-{}-{}",
                tag,
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            fs::create_dir_all(&base).unwrap();
            Self(base)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Commit a one-line file change at an explicit unix time + author.
    fn commit_at(repo: &Repository, dir: &Path, n: usize, secs: i64, who: (&str, &str)) {
        fs::write(dir.join("f.txt"), format!("line {n}\n")).unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let sig = Signature::new(who.0, who.1, &Time::new(secs, 0)).unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, &format!("c{n}"), &tree, &parents)
            .unwrap();
    }

    fn project_at(path: &Path, name: &str, dirty: i64, ahead: i64) -> Project {
        Project {
            id: format!("id-{name}"),
            name: name.into(),
            path: path.to_string_lossy().into_owned(),
            language: Lang::Rust,
            color: "#fff".into(),
            branch: "main".into(),
            dirty,
            ahead,
            behind: 0,
            loc: 0,
            size: String::new(),
            size_bytes: 0,
            disk_size: String::new(),
            disk_bytes: 0,
            last_opened: None,
            pinned: false,
            tags: vec![],
            todos_count: 0,
            notes_count: 0,
            time: String::new(),
            archived: false,
            collection_ids: vec![],
            author: None,
        }
    }

    #[test]
    fn dashboard_buckets_days_sessions_and_filters_author() {
        let tmp = TmpDir::new("dash");
        let repo = Repository::init(&tmp.0).unwrap();
        let me = ("Me", "me@example.com");
        let other = ("Other", "other@example.com");

        // Fixed "now": 2026-10-11 15:00 local.
        let now = Local
            .with_ymd_and_hms(2026, 10, 11, 15, 0, 0)
            .single()
            .unwrap();
        let t = |days_ago: i64, hour: u32| -> i64 {
            (now - Duration::days(days_ago))
                .with_hour(hour)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .timestamp()
        };

        // Today: one session of three commits 10:00, 10:30, 11:00 → 30 + 30 + 30 min.
        commit_at(&repo, &tmp.0, 1, t(0, 10), me);
        commit_at(&repo, &tmp.0, 2, t(0, 10) + 1800, me);
        commit_at(&repo, &tmp.0, 3, t(0, 11), me);
        // Yesterday: a single commit (30 min head credit) + someone else's.
        commit_at(&repo, &tmp.0, 4, t(1, 9), me);
        commit_at(&repo, &tmp.0, 5, t(1, 12), other);
        // Outside a 7-day window but inside 14.
        commit_at(&repo, &tmp.0, 6, t(10, 20), me);

        let projects = vec![project_at(&tmp.0, "alpha", 2, 1)];
        let identity = Identity {
            name: Some("Me".into()),
            email: Some("me@example.com".into()),
        };
        let d = build_dashboard(&projects, identity, now, 14);

        assert_eq!(d.days.len(), 14);
        let last = d.days.last().unwrap();
        assert_eq!(last.date, "2026-10-11");
        assert_eq!(last.commits, 3);
        assert_eq!(last.minutes, 90);
        assert_eq!(last.projects, 1);
        assert_eq!(d.days[12].commits, 1, "other author filtered out");
        assert_eq!(d.days[12].minutes, 30);

        assert_eq!(d.totals.commits_today, 3);
        assert_eq!(d.totals.commits7d, 4);
        assert_eq!(d.totals.commits30d, 5);
        assert_eq!(d.totals.minutes_today, 90);
        assert_eq!(d.totals.streak_days, 2);
        assert_eq!(d.totals.active_projects7d, 1);
        assert_eq!(d.totals.dirty_projects, 1);
        assert_eq!(d.totals.ahead_projects, 1);
        // Stats were computed for the recent commits.
        assert!(d.totals.additions7d >= 4, "{}", d.totals.additions7d);

        assert_eq!(d.hours[10], 2);
        assert_eq!(d.hours[11], 1);
        assert_eq!(d.hours.iter().sum::<u32>(), 5);

        assert_eq!(d.recent_commits.len(), 5);
        assert_eq!(d.recent_commits[0].summary, "c3");
        assert_eq!(d.projects.len(), 1);
        assert_eq!(d.projects[0].commits7d, 4);
        assert_eq!(d.attention.len(), 1);
        assert_eq!(d.attention[0].commits7d, 4);
        assert_eq!(d.author.as_deref(), Some("Me"));
    }

    #[test]
    fn streak_tolerates_empty_today() {
        let mk = |c: &[u32]| -> Vec<HomeDay> {
            c.iter()
                .map(|&n| HomeDay {
                    date: String::new(),
                    commits: n,
                    projects: 0,
                    minutes: 0,
                })
                .collect()
        };
        assert_eq!(streak(&mk(&[0, 1, 1, 1])), 3);
        assert_eq!(streak(&mk(&[0, 1, 1, 0])), 2);
        assert_eq!(streak(&mk(&[1, 0, 1, 0, 0])), 0);
        assert_eq!(streak(&mk(&[])), 0);
    }

    #[test]
    fn identity_without_config_matches_everyone() {
        let id = Identity {
            name: None,
            email: None,
        };
        assert!(id.matches(Some("x"), Some("y@z")));
        let id = Identity {
            name: Some("Me".into()),
            email: Some("ME@example.com".into()),
        };
        assert!(id.matches(None, Some("me@example.com")));
        assert!(id.matches(Some("me"), None));
        assert!(!id.matches(Some("Other"), Some("o@example.com")));
    }

    #[test]
    fn non_repo_projects_are_skipped() {
        let tmp = TmpDir::new("norepo");
        let projects = vec![project_at(&tmp.0, "plain", 0, 0)];
        let d = build_dashboard(
            &projects,
            Identity {
                name: None,
                email: None,
            },
            Local::now(),
            7,
        );
        assert_eq!(d.days.len(), 7);
        assert_eq!(d.totals.commits30d, 0);
        assert!(d.recent_commits.is_empty());
    }
}
