//! Filesystem watcher pipeline.

mod classifier;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self as std_mpsc, Sender as StdSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use serde_json::json;
use tauri::AppHandle;

use crate::events;
use crate::git;
use crate::storage::db::project_id_for_path;
use crate::storage::Db;

pub use classifier::{classify, EventKind};

const DEBOUNCE_MS: u64 = 250;

/// Minimum interval between consecutive `project:updated` emits for the
const COALESCE_MS: u128 = 2_000;

/// State owned by a single watched root.
struct Root {
    #[allow(dead_code)] // kept alive so the notify thread isn't dropped
    debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
    depth: u8,
}

/// Shared mutable state for the dispatcher. Kept behind an `Arc<Mutex<>>`
struct Inner {
    roots: HashMap<PathBuf, Root>,
    /// `repo_root → last emit timestamp` - per-repo coalesce tracker.
    last_emit: HashMap<PathBuf, Instant>,
}

/// Public handle. Cloning is cheap - it just bumps an `Arc` count.
#[derive(Clone)]
pub struct WatcherManager {
    inner: Arc<Mutex<Inner>>,
    app: AppHandle,
    db: Db,
    tx: StdSender<RootEvent>,
    pool: Arc<rayon::ThreadPool>,
}

/// Message pushed onto the shared dispatcher channel. We tag events with
struct RootEvent {
    result: DebounceEventResult,
}

impl WatcherManager {
    /// Build a manager + spawn the dispatcher thread. Creating the manager
    pub fn new(app: AppHandle, db: Db) -> anyhow::Result<Self> {
        let parallelism = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(2);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(parallelism)
            .thread_name(|i| format!("atlas-git-{i}"))
            .build()
            .map_err(|e| anyhow::anyhow!("rayon pool: {e}"))?;

        let (tx, rx) = std_mpsc::channel::<RootEvent>();
        let inner = Arc::new(Mutex::new(Inner {
            roots: HashMap::new(),
            last_emit: HashMap::new(),
        }));

        let mgr = WatcherManager {
            inner: inner.clone(),
            app: app.clone(),
            db: db.clone(),
            tx,
            pool: Arc::new(pool),
        };

        let dispatcher = mgr.clone();
        std::thread::Builder::new()
            .name("atlas-watcher-dispatch".into())
            .spawn(move || dispatcher.run_dispatcher(rx))
            .map_err(|e| anyhow::anyhow!("spawn dispatcher: {e}"))?;

        Ok(mgr)
    }

    /// Begin watching `path` at the given recursion depth hint. `depth`
    #[tracing::instrument(
        level = "info",
        skip(self),
        fields(path = %path.display(), depth),
    )]
    pub fn add_root(&self, path: PathBuf, depth: u8) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        let canonical = canonicalize_within_home(&path)?;

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("watcher state poisoned"))?;
        if inner.roots.contains_key(&canonical) {
            tracing::debug!(path = %canonical.display(), "watch root already active");
            return Ok(());
        }

        let tx_clone = self.tx.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(DEBOUNCE_MS),
            None,
            move |res: DebounceEventResult| {
                // If the receiver is gone, silently drop - app is tearing
                let _ = tx_clone.send(RootEvent { result: res });
            },
        )
        .map_err(|e| anyhow::anyhow!("create debouncer for {}: {e}", canonical.display()))?;

        debouncer
            .watcher()
            .watch(&canonical, notify::RecursiveMode::Recursive)
            .map_err(|e| anyhow::anyhow!("watch {}: {e}", canonical.display()))?;

        // NB: we intentionally do NOT call `debouncer.cache().add_root(…)` here.

        inner
            .roots
            .insert(canonical.clone(), Root { debouncer, depth });
        tracing::info!(
            path = %canonical.display(),
            depth,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "watch root added",
        );
        Ok(())
    }

    /// Stop watching `path`. No-op if the root wasn't active.
    pub fn remove_root(&self, path: &Path) -> anyhow::Result<()> {
        let canonical = canonicalize_within_home(path).unwrap_or_else(|_| path.to_path_buf());
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("watcher state poisoned"))?;
        if inner.roots.remove(&canonical).is_some() {
            tracing::info!(path = %canonical.display(), "watch root removed");
        }
        Ok(())
    }

    /// Queue a git-status scan for every indexed project. Used on boot and
    pub fn refresh_all_git_status(&self) {
        let db = self.db.clone();
        let manager = self.clone();
        // Spawn on the rayon pool so the DB read doesn't block a tokio task.
        self.pool.spawn(move || {
            let paths =
                match tauri::async_runtime::block_on(async move { db.all_project_paths().await }) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "Db::all_project_paths failed");
                        return;
                    }
                };
            tracing::info!(count = paths.len(), "queueing initial git-status refresh");
            for path in paths {
                manager.spawn_git_status(path);
            }
        });
    }

    /// Variant that emits `discovery:progress` events keyed by `root`
    pub fn refresh_git_status_under(&self, root: PathBuf) {
        let db = self.db.clone();
        let manager = self.clone();
        let app = self.app.clone();
        let root_str = root.to_string_lossy().into_owned();
        self.pool.spawn(move || {
            let paths = match tauri::async_runtime::block_on(async { db.all_project_paths().await })
            {
                Ok(all) => all
                    .into_iter()
                    .filter(|p| p.starts_with(&root) || p == &root)
                    .collect::<Vec<_>>(),
                Err(e) => {
                    tracing::warn!(error = %e, "Db::all_project_paths failed");
                    return;
                }
            };

            let total = paths.len();
            let _ = events::emit_discovery_progress(
                &app,
                &root_str,
                events::DiscoveryPhase::GitStatus,
                None,
                0,
                Some(total),
            );

            if total == 0 {
                let _ = events::emit_discovery_progress(
                    &app,
                    &root_str,
                    events::DiscoveryPhase::Done,
                    None,
                    0,
                    Some(0),
                );
                return;
            }

            let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for path in paths {
                let app = app.clone();
                let db = db.clone();
                let done = done.clone();
                let root_str = root_str.clone();
                let path_for_display = path.to_string_lossy().into_owned();
                manager.pool.spawn(move || {
                    let status = match git::read_status(&path) {
                        Ok(Some(s)) => Some(s),
                        _ => None,
                    };
                    let id = project_id_for_path(&path);
                    if let Some(ref s) = status {
                        let _ = events::emit_git_status(&app, &id, s);
                        if let Err(e) = tauri::async_runtime::block_on(async {
                            db.apply_git_status(&id, s).await
                        }) {
                            tracing::warn!(error = %e, id = %id, "apply_git_status");
                        }
                    }
                    let n = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    let _ = events::emit_discovery_progress(
                        &app,
                        &root_str,
                        events::DiscoveryPhase::GitStatus,
                        Some(&path_for_display),
                        n,
                        Some(total),
                    );
                    if n == total {
                        let _ = events::emit_discovery_progress(
                            &app,
                            &root_str,
                            events::DiscoveryPhase::Done,
                            None,
                            total,
                            Some(total),
                        );
                    }
                });
            }
        });
    }

    /// Snapshot of the active roots. Used by the `watchers.list` IPC.
    pub fn list_roots(&self) -> Vec<(PathBuf, u8)> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        let mut out: Vec<_> = inner
            .roots
            .iter()
            .map(|(p, r)| (p.clone(), r.depth))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    // ---- dispatcher ----

    #[tracing::instrument(level = "info", name = "watcher.dispatcher", skip_all)]
    fn run_dispatcher(self, rx: std_mpsc::Receiver<RootEvent>) {
        tracing::info!("watcher dispatcher online");
        while let Ok(RootEvent { result }) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(errs) => {
                    for e in errs {
                        tracing::warn!(error = ?e, "watcher error");
                    }
                    continue;
                }
            };

            // Snapshot roots once per batch so classification is consistent.
            let roots: Vec<PathBuf> = match self.inner.lock() {
                Ok(inner) => inner.roots.keys().cloned().collect(),
                Err(_) => continue,
            };

            // Span covers one debounce batch; keep it at debug so
            let span = tracing::debug_span!(
                "watcher.batch",
                batch_size = events.len(),
                roots = roots.len(),
            );
            let _enter = span.enter();

            for ev in events {
                let is_dir = matches!(
                    ev.event.kind,
                    notify::EventKind::Create(notify::event::CreateKind::Folder)
                        | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
                );

                for p in &ev.event.paths {
                    let kind = classify(p, &roots, is_dir);
                    self.dispatch(kind);
                }
            }
        }
        tracing::info!("watcher dispatcher exiting");
    }

    fn dispatch(&self, kind: EventKind) {
        match kind {
            EventKind::Ignored => {}
            EventKind::GitMetadata { repo_root } => {
                self.spawn_git_status(repo_root);
            }
            EventKind::AtlasJson { repo_root, .. } => {
                // bumps. Project id comes from D2's `project_id_for_path`
                let id = project_id_for_path(&repo_root);
                if let Err(e) = events::emit_project_updated(&self.app, &id, json!({})) {
                    tracing::warn!(error = %e, "emit project:updated (atlas-json)");
                }
            }
            EventKind::PackageJson { repo_root } => {
                // TODO: wire to the script parser. For now just
                tracing::debug!(root = %repo_root.display(), "package.json changed (iter-3 TODO)");
                let id = project_id_for_path(&repo_root);
                if let Err(e) = events::emit_project_updated(&self.app, &id, json!({})) {
                    tracing::warn!(error = %e, "emit project:updated (package-json)");
                }
            }
            EventKind::SourceFile { repo_root } => {
                self.coalesced_dirty_bump(repo_root);
            }
            EventKind::NewDirectory { path } => {
                self.spawn_discovery_probe(path);
            }
        }
    }

    /// Emit `project:updated` with an empty patch at most once per
    fn coalesced_dirty_bump(&self, repo_root: PathBuf) {
        let now = Instant::now();
        let mut should_emit = false;
        if let Ok(mut inner) = self.inner.lock() {
            let fresh = inner
                .last_emit
                .get(&repo_root)
                .map(|t| now.duration_since(*t).as_millis() >= COALESCE_MS)
                .unwrap_or(true);
            if fresh {
                inner.last_emit.insert(repo_root.clone(), now);
                should_emit = true;
            }
        }
        if should_emit {
            let id = project_id_for_path(&repo_root);
            if let Err(e) = events::emit_project_updated(&self.app, &id, json!({})) {
                tracing::warn!(error = %e, "emit project:updated (dirty-bump)");
            }
        }
    }

    /// Run `git::read_status` on the rayon pool, then hand the result to D2
    fn spawn_git_status(&self, repo_root: PathBuf) {
        let app = self.app.clone();
        let db = self.db.clone();
        self.pool.spawn(move || {
            let status = match git::read_status(&repo_root) {
                Ok(Some(s)) => s,
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        repo = %repo_root.display(),
                        "git::read_status failed",
                    );
                    return;
                }
            };

            let id = project_id_for_path(&repo_root);
            if let Err(e) = events::emit_git_status(&app, &id, &status) {
                tracing::warn!(error = %e, "emit git:status");
            }

            // Persist into the index.
            let db_for_call = db.clone();
            let id_for_call = id.clone();
            let status_for_call = status.clone();
            if let Err(e) = tauri::async_runtime::block_on(async move {
                db_for_call
                    .apply_git_status(&id_for_call, &status_for_call)
                    .await
            }) {
                tracing::warn!(
                    error = %e,
                    id = %id,
                    "Db::apply_git_status failed",
                );
            }
        });
    }

    /// A new directory appeared under a watch root. Probe for `.git` and,
    fn spawn_discovery_probe(&self, path: PathBuf) {
        let db = self.db.clone();
        let app = self.app.clone();
        let manager = self.clone();
        self.pool.spawn(move || {
            if !git::is_git_repo(&path) {
                return;
            }

            // `discover_root(path, 0)` scans just this single dir (depth 0
            let db_for_call = db.clone();
            let path_for_call = path.clone();
            let new_ids = match tauri::async_runtime::block_on(async move {
                db_for_call.discover_root(&path_for_call, 0).await
            }) {
                Ok(ids) => ids,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %path.display(),
                        "Db::discover_root failed",
                    );
                    return;
                }
            };

            for id in &new_ids {
                if let Ok(Some(project)) = tauri::async_runtime::block_on({
                    let db = db.clone();
                    let id = id.clone();
                    async move { db.get_project(&id).await }
                }) {
                    if let Err(e) = events::emit_project_discovered(&app, &project) {
                        tracing::warn!(error = %e, "emit project:discovered");
                    }
                }
            }

            // Prime the git status for the newly discovered repo so the
            manager.spawn_git_status(path);
        });
    }
}

impl std::fmt::Debug for WatcherManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let roots = self.list_roots();
        f.debug_struct("WatcherManager")
            .field("roots", &roots)
            .finish()
    }
}

// ---- helpers ----

/// Canonicalize `path` and reject anything that escapes the user's home
fn canonicalize_within_home(path: &Path) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| anyhow::anyhow!("canonicalize {}: {e}", path.display()))?;

    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        if let Ok(home_canonical) = std::fs::canonicalize(&home_path) {
            if !canonical.starts_with(&home_canonical) {
                // Explicitly allow tmp dirs for testing - macOS temp dirs
                let allowed_tmp_prefixes: [&str; 4] = [
                    "/tmp",
                    "/private/tmp",
                    "/var/folders",
                    "/private/var/folders",
                ];
                let is_tmp = allowed_tmp_prefixes
                    .iter()
                    .any(|p| canonical.starts_with(p));
                if !is_tmp {
                    return Err(anyhow::anyhow!(
                        "watch root {} escapes $HOME via symlink",
                        canonical.display()
                    ));
                }
            }
        }
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_window_matches_prd() {
        assert_eq!(COALESCE_MS, 2_000);
    }

    #[test]
    fn debounce_window_matches_prd() {
        assert_eq!(DEBOUNCE_MS, 250);
    }
}
