//! ```text

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use crate::storage::Db;

/// Directory names considered "safe to clean" - the UI lights up a
const CLEANABLE_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    ".venv",
    ".turbo",
    ".parcel-cache",
];

/// Row returned by `disk.scan`. Shape matches the `DiskEntry` interface
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct DiskEntry {
    /// Human-readable label - the top-level dir name, or `(files)` for
    pub label: String,
    /// Absolute path on disk (for the Reveal-in-Finder action).
    pub path: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Pretty-printed size, e.g. "412 MB".
    pub size: String,
    /// 0..1 share of the total.
    #[ts(type = "number")]
    pub pct: f32,
    /// True when `label` matches a well-known regeneratable directory
    pub cleanable: bool,
}

/// Aggregate shape returned by `disk.scan`. Matches
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct DiskScanResult {
    #[ts(type = "number")]
    pub total_bytes: u64,
    pub total_size: String,
    pub entries: Vec<DiskEntry>,
}

/// `disk.scan(project_id)` - return the top 10 biggest top-level
#[tracing::instrument(
    level = "info",
    skip_all,
    fields(project_id = %project_id),
)]
#[tauri::command]
pub async fn disk_scan(state: State<'_, Db>, project_id: String) -> Result<DiskScanResult, String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(project.path);

    // `ignore::WalkBuilder` is blocking. Drop onto the blocking pool so
    let start = std::time::Instant::now();
    let result = tauri::async_runtime::spawn_blocking(move || scan_tree(&project_path))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| e.to_string())?;
    tracing::info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        rows = result.entries.len(),
        total_bytes = result.total_bytes,
        "disk scan complete",
    );
    Ok(result)
}

/// `disk.clean(project_id, relative_path)` - recursively delete the
#[tauri::command]
pub async fn disk_clean(
    state: State<'_, Db>,
    project_id: String,
    relative_path: String,
) -> Result<(), String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(&project.path);

    let target = project_path.join(&relative_path);
    let project_canon = project_path
        .canonicalize()
        .map_err(|e| format!("canonicalize project path: {e}"))?;
    let target_canon = target
        .canonicalize()
        .map_err(|e| format!("canonicalize target path: {e}"))?;

    if !target_canon.starts_with(&project_canon) {
        return Err(format!(
            "refusing to clean path outside project: {}",
            relative_path
        ));
    }
    if target_canon == project_canon {
        return Err("refusing to clean the project root".into());
    }

    // `trash::delete` moves the path to the platform trash (macOS "Move to
    tauri::async_runtime::spawn_blocking(move || trash::delete(&target_canon))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| format!("move-to-trash {}: {e}", relative_path))
}

/// Walk `project_path` and sum file sizes grouped by top-level name.
fn scan_tree(project_path: &Path) -> anyhow::Result<DiskScanResult> {
    // Accumulator: top-level dir name → (absolute path, size_bytes).
    let totals: Mutex<std::collections::HashMap<String, (PathBuf, u64)>> =
        Mutex::new(std::collections::HashMap::new());

    let threads = (num_cpus()).max(1);
    let walker = WalkBuilder::new(project_path)
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        .git_global(false)
        .ignore(false)
        .parents(false)
        .follow_links(false)
        .threads(threads)
        .build_parallel();

    let project_root = project_path.to_path_buf();
    walker.run(|| {
        let project_root = project_root.clone();
        let totals = &totals;
        Box::new(move |entry_result| {
            let entry = match entry_result {
                Ok(e) => e,
                Err(err) => {
                    tracing::trace!(?err, "walk error in disk scan");
                    return ignore::WalkState::Continue;
                }
            };

            // Only files contribute bytes; dirs are implicit via their
            let file_type = match entry.file_type() {
                Some(t) => t,
                None => return ignore::WalkState::Continue,
            };
            if !file_type.is_file() {
                return ignore::WalkState::Continue;
            }

            let size = match entry.metadata() {
                Ok(m) => m.len(),
                Err(_) => return ignore::WalkState::Continue,
            };

            let rel = match entry.path().strip_prefix(&project_root) {
                Ok(r) => r,
                Err(_) => return ignore::WalkState::Continue,
            };

            let (label, abs) = match rel.components().next() {
                // First segment IS the top-level - whether the entry is
                Some(comp) => {
                    let seg = comp.as_os_str().to_string_lossy().into_owned();
                    let abs = project_root.join(&seg);
                    let remaining = rel.components().count();
                    if remaining <= 1 {
                        // Loose file at the project root.
                        ("(files)".to_string(), project_root.clone())
                    } else {
                        (seg, abs)
                    }
                }
                None => return ignore::WalkState::Continue,
            };

            if let Ok(mut map) = totals.lock() {
                let entry = map.entry(label).or_insert((abs, 0));
                entry.1 = entry.1.saturating_add(size);
            }
            ignore::WalkState::Continue
        })
    });

    let map = totals
        .into_inner()
        .map_err(|e| anyhow::anyhow!("totals mutex poisoned: {e}"))?;
    let project_total: u64 = map.values().map(|(_, n)| *n).sum();

    let mut rows: Vec<DiskEntry> = map
        .into_iter()
        .map(|(label, (path, size))| {
            // `pct` is a 0..1 share (matches the TS `DiskEntry` interface,
            let pct = if project_total == 0 {
                0.0
            } else {
                (size as f64 / project_total as f64) as f32
            };
            let cleanable = CLEANABLE_DIRS.iter().any(|c| *c == label);
            DiskEntry {
                label,
                path: path.to_string_lossy().into_owned(),
                bytes: size,
                size: format_size(size),
                pct,
                cleanable,
            }
        })
        .collect();

    rows.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    rows.truncate(10);
    Ok(DiskScanResult {
        total_bytes: project_total,
        total_size: format_size(project_total),
        entries: rows,
    })
}

/// Human-readable IEC-ish byte formatter. Keeps one decimal for the
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut idx = 0usize;
    while val >= 1024.0 && idx < UNITS.len() - 1 {
        val /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{val:.0} {}", UNITS[idx])
    } else {
        format!("{val:.1} {}", UNITS[idx])
    }
}

/// Conservative core-count estimate. We don't depend on the `num_cpus`
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().max(2) / 2)
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("{prefix}-{ns}"));
        fs::create_dir_all(&p).expect("create tempdir");
        p
    }

    /// A small fixture: `node_modules/a.js` (200B), `src/main.rs` (100B),
    #[test]
    fn scan_tree_groups_by_top_level() -> anyhow::Result<()> {
        let root = tempdir("atlas_disk_scan");
        fs::create_dir_all(root.join("node_modules"))?;
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("node_modules/a.js"), vec![b'x'; 200])?;
        fs::write(root.join("src/main.rs"), vec![b'x'; 100])?;
        fs::write(root.join("README.md"), vec![b'x'; 50])?;

        let result = scan_tree(&root)?;
        assert!(!result.entries.is_empty());
        assert_eq!(result.total_bytes, 350);
        assert!(result.total_size.contains("350"));

        // Map by label so the assertions survive iteration-order churn.
        let by_label: std::collections::HashMap<_, _> = result
            .entries
            .iter()
            .map(|r| (r.label.clone(), r.clone()))
            .collect();

        let nm = by_label.get("node_modules").expect("node_modules row");
        assert_eq!(nm.bytes, 200);
        assert!(nm.cleanable);

        let src = by_label.get("src").expect("src row");
        assert_eq!(src.bytes, 100);
        assert!(!src.cleanable);

        let loose = by_label.get("(files)").expect("(files) row");
        assert_eq!(loose.bytes, 50);

        // Total pct rounds to 1.0 ± 0.01 (0..1 share, not 0..100).
        let total: f32 = result.entries.iter().map(|r| r.pct).sum();
        assert!((total - 1.0).abs() < 0.01, "pct sum was {total}");

        // Top row is node_modules.
        assert_eq!(result.entries[0].label, "node_modules");

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// Empty project: no panic, empty vec.
    #[test]
    fn scan_tree_empty() -> anyhow::Result<()> {
        let root = tempdir("atlas_disk_empty");
        let result = scan_tree(&root)?;
        assert!(result.entries.is_empty());
        assert_eq!(result.total_bytes, 0);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn format_size_rounds_sanely() {
        assert_eq!(super::format_size(0), "0 B");
        assert_eq!(super::format_size(512), "512 B");
        assert_eq!(super::format_size(2048), "2.0 KB");
        assert_eq!(super::format_size(1_572_864), "1.5 MB");
    }
}
