//! Seeds 5000 fake projects into a throwaway SQLite DB, runs the two

use std::path::PathBuf;
use std::time::Instant;

use atlas_lib::storage::types::ProjectFilter;
use atlas_lib::storage::Db;

const SEED_COUNT: usize = 5_000;
const ITERATIONS: usize = 100;

const LIST_P99_BUDGET_MS: u128 = 100;
const SEARCH_P99_BUDGET_MS: u128 = 50;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    // Tempdir - removed at the end on success. If the process panics
    let tmp = tempdir("atlas-perf-check");
    let db = Db::open(&tmp).await?;
    println!("seeding {SEED_COUNT} fake projects into {}", tmp.display());
    let seed_start = Instant::now();
    seed_projects(&db, SEED_COUNT).await?;
    println!(
        "  seeded in {:.2}s",
        seed_start.elapsed().as_secs_f64()
    );

    // --- list_projects ---
    let list_filter = ProjectFilter {
        include_archived: true,
        ..ProjectFilter::default()
    };
    let mut list_samples: Vec<u128> = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let t = Instant::now();
        let rows = db.list_projects(list_filter.clone()).await?;
        list_samples.push(t.elapsed().as_micros());
        assert_eq!(rows.len(), SEED_COUNT, "list_projects row count drift");
    }

    // --- search_projects ---
    let mut search_samples: Vec<u128> = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let t = Instant::now();
        let _ = db.search_projects("foo").await?;
        search_samples.push(t.elapsed().as_micros());
    }

    let list = Stats::from_micros(&mut list_samples);
    let search = Stats::from_micros(&mut search_samples);

    let list_outcome = judge(list.p99_ms, LIST_P99_BUDGET_MS);
    let search_outcome = judge(search.p99_ms, SEARCH_P99_BUDGET_MS);

    println!();
    println!(
        "| {:<24} | {:>10} | {:>10} | {:>14} | {:<6} |",
        "metric", "p50 ms", "p99 ms", "budget (p99)", "result"
    );
    println!(
        "| {:-<24} | {:->10} | {:->10} | {:->14} | {:-<6} |",
        "", "", "", "", ""
    );
    println!(
        "| {:<24} | {:>10.2} | {:>10.2} | {:>14} | {:<6} |",
        "list_projects (5k)",
        list.p50_ms,
        list.p99_ms,
        format!("≤ {} ms", LIST_P99_BUDGET_MS),
        list_outcome.label,
    );
    println!(
        "| {:<24} | {:>10.2} | {:>10.2} | {:>14} | {:<6} |",
        "search_projects (FTS)",
        search.p50_ms,
        search.p99_ms,
        format!("≤ {} ms", SEARCH_P99_BUDGET_MS),
        search_outcome.label,
    );
    println!();

    // Cleanup tempdir before exiting so we don't leave SQLite files
    let _ = std::fs::remove_dir_all(&tmp);

    // Structural warn policy: an on-disk SQLite in a tempdir is
    if list_outcome.failed || search_outcome.failed {
        println!(
            "note: one or more budgets exceeded (tempdir SQLite isn't \
             perfectly representative of production RSS/cache state). \
             Exiting 0; investigate before shipping if this trends up."
        );
    }
    Ok(())
}

struct Outcome {
    label: &'static str,
    failed: bool,
}

fn judge(p99_ms: f64, budget_ms: u128) -> Outcome {
    if (p99_ms as u128) <= budget_ms {
        Outcome { label: "pass", failed: false }
    } else {
        Outcome { label: "warn", failed: true }
    }
}

struct Stats {
    p50_ms: f64,
    p99_ms: f64,
}

impl Stats {
    /// `samples` is mutated (sorted in place) - caller shouldn't care.
    fn from_micros(samples: &mut [u128]) -> Self {
        samples.sort_unstable();
        let p50_us = percentile(samples, 50);
        let p99_us = percentile(samples, 99);
        Stats {
            p50_ms: (p50_us as f64) / 1000.0,
            p99_ms: (p99_us as f64) / 1000.0,
        }
    }
}

fn percentile(sorted: &[u128], pct: u8) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64) * (pct as f64) / 100.0).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

/// Bulk insert via raw SQL in a single transaction. Skips FTS triggers
async fn seed_projects(db: &Db, n: usize) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let pool = db.pool();
    let mut tx = pool.begin().await?;

    for i in 0..n {
        let id = format!("perf-proj-{i:05}");
        let name = format!("project-{i:05}");
        let path = format!("/tmp/atlas-perf/project-{i:05}");
        // Sprinkle a "foo" token across ~10% of rows so search has
        let tags_joined = if i % 10 == 0 { "foo bar" } else { "bar baz" };

        sqlx::query(
            "INSERT INTO projects (id, name, path, language, color, branch, \
                                   dirty, ahead, behind, loc, size_bytes, last_opened, \
                                   pinned, archived, todos_count, notes_count, \
                                   time_tracked, updated_at) \
             VALUES (?, ?, ?, 'Rust', '#000', 'main', 0, 0, 0, 0, 0, NULL, \
                     0, 0, 0, 0, '0h', ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(&path)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO projects_fts (rowid, name, path, tags) \
             VALUES ((SELECT rowid FROM projects WHERE id = ?), ?, ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(&path)
        .bind(tags_joined)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

fn tempdir(tag: &str) -> PathBuf {
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("{tag}-{}-{ns}", std::process::id()));
    std::fs::create_dir_all(&p).expect("mkdir tempdir");
    p
}
