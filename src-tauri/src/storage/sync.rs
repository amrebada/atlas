//! Idle-time reconciliation between the per-project JSON "truth" and the

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::Row;
use tokio::sync::mpsc;

use crate::storage::db::project_id_for_path;
use crate::storage::json::atlas_file;
use crate::storage::Db;

const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Handle for the long-lived background sweep task.
pub struct SyncWorker {
    /// Owned sender; its `Drop` closes the channel which signals the
    cancel_tx: Option<mpsc::Sender<()>>,
}

impl SyncWorker {
    /// Spawn the background task on the current tokio runtime. Returns
    pub fn spawn(db: Db) -> Self {
        // Buffer of 1 is fine - we only ever use the channel as a
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>(1);

        // NOTE: Tauri's `setup(|app| ...)` closure does NOT execute inside
        tauri::async_runtime::spawn(run_sweep_loop(db, cancel_rx));

        Self {
            cancel_tx: Some(cancel_tx),
        }
    }

    /// Explicitly stop the worker. Equivalent to dropping `self` - the
    pub fn dispose(&mut self) {
        self.cancel_tx = None;
    }
}

impl Drop for SyncWorker {
    fn drop(&mut self) {
        // Explicit drop of the sender half triggers `recv().await == None`
        self.cancel_tx = None;
    }
}

/// Body of the long-lived sweep task. Loops on a 60 s interval until the
async fn run_sweep_loop(db: Db, mut cancel_rx: mpsc::Receiver<()>) {
    // First tick fires immediately on `Interval`. Skip it - we'd rather
    let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(e) = run_once(&db).await {
                    // Don't fail the loop on a single bad sweep - log
                    tracing::warn!(error = %e, "sync sweep errored; retrying on next tick");
                }
            }
            _ = cancel_rx.recv() => {
                tracing::debug!("sync worker received cancel; exiting");
                return;
            }
        }
    }
}

/// Upper bound on the number of "loc == 0" projects whose metrics get
const METRICS_REFRESH_CAP_PER_SWEEP: usize = 5;

/// One reconciliation pass. Public to the crate so the test suite and
pub(crate) async fn run_once(db: &Db) -> anyhow::Result<()> {
    let reconciled = reconcile_stale(db).await?;
    let dropped = drop_vanished(db).await?;
    let metrics = refresh_zero_metrics(db).await?;

    tracing::info!(
        reconciled = reconciled,
        dropped = dropped,
        metrics_refreshed = metrics,
        "sync sweep complete"
    );
    Ok(())
}

/// Pick up to `METRICS_REFRESH_CAP_PER_SWEEP` projects whose `loc` is
async fn refresh_zero_metrics(db: &Db) -> anyhow::Result<u32> {
    // Snapshot candidates up front so we're not holding a cursor while
    let rows = sqlx::query(
        "SELECT id, path FROM projects \
           WHERE loc = 0 \
           ORDER BY discovered_at ASC \
           LIMIT ?",
    )
    .bind(METRICS_REFRESH_CAP_PER_SWEEP as i64)
    .fetch_all(db.pool())
    .await?;

    let mut refreshed: u32 = 0;
    for row in rows {
        let id: String = row.try_get("id")?;
        let path_str: String = row.try_get("path")?;

        // Skip rows whose path has vanished - `drop_vanished` will
        match tokio::fs::metadata(&path_str).await {
            Ok(_) => {}
            Err(_) => continue,
        }

        match db.refresh_project_metrics(&id).await {
            Ok(_) => {
                refreshed = refreshed.saturating_add(1);
            }
            Err(err) => {
                tracing::trace!(
                    error = %err,
                    id = %id,
                    "refresh_project_metrics failed in sweep (will retry next tick)",
                );
            }
        }
    }

    Ok(refreshed)
}

/// For every row whose `updated_at` is older than the mtime of the
async fn reconcile_stale(db: &Db) -> anyhow::Result<u32> {
    // Snapshot all rows up front so we're not holding a cursor while
    let rows = sqlx::query("SELECT id, path, updated_at FROM projects")
        .fetch_all(db.pool())
        .await?;

    let mut reconciled: u32 = 0;
    for row in rows {
        let id: String = row.try_get("id")?;
        let path_str: String = row.try_get("path")?;
        let updated_at: String = row.try_get("updated_at")?;

        let project_path = Path::new(&path_str);
        let json_path = atlas_file(project_path, "project");

        // Use tokio::fs for the stat to avoid blocking the runtime on
        let mtime = match tokio::fs::metadata(&json_path).await {
            Ok(meta) => match meta.modified() {
                Ok(t) => t,
                Err(_) => continue, // platform doesn't report mtime
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                tracing::trace!(
                    error = %err,
                    path = %json_path.display(),
                    "metadata() failed in sync sweep"
                );
                continue;
            }
        };

        let db_ts = match parse_rfc3339(&updated_at) {
            Some(t) => t,
            None => continue, // malformed timestamp — skip, next write will heal
        };

        if mtime > db_ts {
            // The on-disk JSON is newer than our index. Re-index by
            if let Err(e) = db.resync_project_fts(&id).await {
                tracing::warn!(
                    error = %e,
                    id = %id,
                    "resync_project_fts failed during sweep"
                );
                continue;
            }
            let now = chrono::Utc::now().to_rfc3339();
            if let Err(e) = sqlx::query("UPDATE projects SET updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(&id)
                .execute(db.pool())
                .await
            {
                tracing::warn!(error = %e, id = %id, "bump updated_at failed");
                continue;
            }
            reconciled = reconciled.saturating_add(1);
        }
    }

    Ok(reconciled)
}

/// Drop rows whose path no longer exists on disk, provided they were
async fn drop_vanished(db: &Db) -> anyhow::Result<u32> {
    let rows = sqlx::query("SELECT id, path FROM projects WHERE source = 'discovery'")
        .fetch_all(db.pool())
        .await?;

    let mut dropped: u32 = 0;
    for row in rows {
        let id: String = row.try_get("id")?;
        let path_str: String = row.try_get("path")?;

        match tokio::fs::metadata(&path_str).await {
            Ok(_) => continue,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Vanished. Delete - ON DELETE CASCADE handles tags,
                if let Err(e) = sqlx::query("DELETE FROM projects WHERE id = ?")
                    .bind(&id)
                    .execute(db.pool())
                    .await
                {
                    tracing::warn!(error = %e, id = %id, "DELETE FROM projects failed");
                    continue;
                }
                let _ = sqlx::query("DELETE FROM notes_fts WHERE project_id = ?")
                    .bind(&id)
                    .execute(db.pool())
                    .await;
                let _ = sqlx::query("DELETE FROM todos_fts WHERE project_id = ?")
                    .bind(&id)
                    .execute(db.pool())
                    .await;
                dropped = dropped.saturating_add(1);
            }
            Err(err) => {
                tracing::trace!(
                    error = %err,
                    path = %path_str,
                    "metadata() failed for discovery row; skipping drop"
                );
            }
        }
    }

    Ok(dropped)
}

/// Best-effort RFC3339 → `SystemTime` conversion. We keep the parse
fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    let parsed = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    let secs = parsed.timestamp();
    let nanos = parsed.timestamp_subsec_nanos();
    if secs < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::new(secs as u64, nanos))
}

// ---------- legacy no-op helpers ----------

/// Queue an index refresh for a given project id. Currently a no-op  -
pub fn queue_index(_db: &Db, _project_id: &str) {}

/// Reindex a single `.atlas/*.json` file that changed on disk. Watcher
pub async fn reindex_path(_db: &Db, _path: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Drop index rows whose backing JSON has disappeared. Kept as a
pub async fn drop_orphans(db: &Db) -> anyhow::Result<usize> {
    Ok(drop_vanished(db).await? as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(prefix: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("{prefix}-{ns}-{}", std::process::id()));
        fs::create_dir_all(&p).expect("create tempdir");
        p
    }

    /// `drop_vanished` drops a discovery row whose path is gone, and
    #[tokio::test]
    async fn drop_vanished_only_touches_discovery_rows() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let now = chrono::Utc::now().to_rfc3339();

        // Row A - source='discovery', path doesn't exist ⇒ should drop.
        sqlx::query(
            "INSERT INTO projects (id, name, path, language, color, branch, \
                                   dirty, ahead, behind, loc, size_bytes, \
                                   last_opened, pinned, archived, todos_count, \
                                   notes_count, time_tracked, updated_at, \
                                   discovered_at, source) \
             VALUES ('a', 'A', '/tmp/atlas-test-vanished-a', 'Rust', '#fff', '', \
                     0, 0, 0, 0, 0, NULL, 0, 0, 0, 0, '', ?, ?, 'discovery')",
        )
        .bind(&now)
        .bind(&now)
        .execute(db.pool())
        .await?;

        // Row B - source='manual', path doesn't exist ⇒ should survive.
        sqlx::query(
            "INSERT INTO projects (id, name, path, language, color, branch, \
                                   dirty, ahead, behind, loc, size_bytes, \
                                   last_opened, pinned, archived, todos_count, \
                                   notes_count, time_tracked, updated_at, \
                                   discovered_at, source) \
             VALUES ('b', 'B', '/tmp/atlas-test-vanished-b', 'Rust', '#fff', '', \
                     0, 0, 0, 0, 0, NULL, 0, 0, 0, 0, '', ?, ?, 'manual')",
        )
        .bind(&now)
        .bind(&now)
        .execute(db.pool())
        .await?;

        let dropped = drop_vanished(&db).await?;
        assert_eq!(dropped, 1);

        let remaining: Vec<(String,)> = sqlx::query_as("SELECT id FROM projects ORDER BY id")
            .fetch_all(db.pool())
            .await?;
        assert_eq!(remaining, vec![("b".to_string(),)]);
        Ok(())
    }

    /// `current_version` returns the highest applied migration - for an
    #[tokio::test]
    async fn current_version_returns_highest_applied() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let v = db.current_version().await?;
        // Migrations present: 0001, 0002, 0004, 0005.
        assert_eq!(v, Some(5));
        Ok(())
    }

    /// `SyncWorker::drop` must cancel the background task. We can't
    #[tokio::test]
    async fn sync_worker_drop_cancels_cleanly() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let worker = SyncWorker::spawn(db);
        drop(worker);
        // If the drop ever started blocking or panicking, the test
        Ok(())
    }

    /// Silence unused-imports / dead-code warnings on the legacy
    #[test]
    fn legacy_helpers_are_callable() {
        let _ = queue_index;
        let _ = reindex_path;
        let _ = drop_orphans;
        let _ = project_id_for_path;
    }
}
