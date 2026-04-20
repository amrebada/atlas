//! Project command handlers.

use crate::events;
use crate::storage::json::{atlas_file, write_json};
use crate::storage::types::{Note, Project, ProjectFilter, Script, Todo};
use crate::storage::Db;
use crate::watcher::WatcherManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// `projects.list` - return every indexed project (archived included),
#[tauri::command]
pub async fn projects_list(state: tauri::State<'_, Db>) -> Result<Vec<Project>, String> {
    let filter = ProjectFilter {
        include_archived: true,
        ..ProjectFilter::default()
    };
    state
        .list_projects(filter)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.get` - return a single project by id, or `null` if missing.
#[tauri::command]
pub async fn projects_get(
    state: tauri::State<'_, Db>,
    id: String,
) -> Result<Option<Project>, String> {
    state
        .get_project(&id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.search` - ranked FTS search across `name`, `path`, and
#[tauri::command]
pub async fn projects_search(
    state: tauri::State<'_, Db>,
    query: String,
    _filters: Option<Value>,
) -> Result<Vec<Project>, String> {
    state
        .search_projects(&query)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.seed_fixtures` - first-boot helper. Inserts the 12 prototype
#[tauri::command]
pub async fn projects_seed_fixtures(state: tauri::State<'_, Db>) -> Result<usize, String> {
    state
        .seed_fixtures()
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.discover` - walk `root` up to `depth` (default 3), upsert
#[tracing::instrument(
    level = "info",
    skip_all,
    fields(root = %root, depth = depth.unwrap_or(3)),
)]
#[tauri::command]
pub async fn projects_discover(
    state: tauri::State<'_, Db>,
    app: tauri::AppHandle,
    root: String,
    depth: Option<u8>,
) -> Result<Vec<String>, String> {
    let depth = depth.unwrap_or(3);
    let start = std::time::Instant::now();
    let result = state
        .discover_root(std::path::Path::new(&root), depth)
        .await
        .map_err(|e: anyhow::Error| e.to_string());
    match &result {
        Ok(ids) => {
            tracing::info!(
                elapsed_ms = start.elapsed().as_millis() as u64,
                new_projects = ids.len(),
                "discover complete",
            );
            // Kick off a background metrics refresh for every new project
            for id in ids.iter().cloned() {
                let db = (*state).clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    spawn_metrics_refresh(db, app, id).await;
                });
            }
        }
        Err(e) => tracing::warn!(
            elapsed_ms = start.elapsed().as_millis() as u64,
            error = %e,
            "discover failed",
        ),
    }
    result
}

/// Background helper: refresh metrics for `project_id` and emit
pub(crate) async fn spawn_metrics_refresh(
    db: Db,
    app: tauri::AppHandle,
    project_id: String,
) {
    match db.refresh_project_metrics(&project_id).await {
        Ok(metrics) => {
            let size_str = crate::util::format_bytes(metrics.size_bytes);
            let patch = serde_json::json!({
                "loc": metrics.loc,
                "size": size_str,
                "sizeBytes": metrics.size_bytes,
            });
            if let Err(e) = events::emit_project_updated(&app, &project_id, patch) {
                tracing::warn!(
                    error = %e,
                    id = %project_id,
                    "emit project:updated (metrics) failed",
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                id = %project_id,
                "background metrics refresh failed",
            );
        }
    }
}

/// `projects.pin` - toggle the pinned bit on a project.
#[tauri::command]
pub async fn projects_pin(
    state: tauri::State<'_, Db>,
    id: String,
    pinned: bool,
) -> Result<(), String> {
    state
        .pin_project(&id, pinned)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.archive` - hide (true) or restore (false) a project from
#[tauri::command]
pub async fn projects_archive(
    state: tauri::State<'_, Db>,
    id: String,
    archived: bool,
) -> Result<(), String> {
    state
        .archive_project(&id, archived)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.rename` - change the user-visible display name. The on-disk
#[tauri::command]
pub async fn projects_rename(
    state: tauri::State<'_, Db>,
    id: String,
    name: String,
) -> Result<(), String> {
    state
        .rename_project(&id, &name)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.set_tags` - replace the full tag set for a project.
#[tauri::command]
pub async fn projects_set_tags(
    state: tauri::State<'_, Db>,
    id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    state
        .set_tags(&id, &tags)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

// =====================================================================

/// Camel-case DTO returned by `projects_refresh_metrics`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/types/rust.ts", rename_all = "camelCase")]
pub struct ProjectMetricsDto {
    #[ts(type = "number")]
    pub loc: u64,
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// Pretty-printed size, e.g. `"412 MB"`. Derived via
    pub size: String,
}

/// `projects.refresh_metrics(id)` - walk the project's directory with
#[tracing::instrument(level = "info", skip_all, fields(project_id = %id))]
#[tauri::command]
pub async fn projects_refresh_metrics(
    state: tauri::State<'_, Db>,
    id: String,
) -> Result<ProjectMetricsDto, String> {
    let metrics = state
        .refresh_project_metrics(&id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    Ok(ProjectMetricsDto {
        loc: metrics.loc,
        size_bytes: metrics.size_bytes,
        size: crate::util::format_bytes(metrics.size_bytes),
    })
}

/// `projects.reorder_pinned` - persist a new drag order for the pinned
#[tauri::command]
pub async fn projects_reorder_pinned(
    state: tauri::State<'_, Db>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    state
        .reorder_pinned(&ordered_ids)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `projects.move_to_trash` - move the project's on-disk folder to the
#[tauri::command]
pub async fn projects_move_to_trash(
    db: tauri::State<'_, Db>,
    watcher: tauri::State<'_, WatcherManager>,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    let project = db
        .get_project(&id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {id}"))?;
    let project_path = PathBuf::from(&project.path);

    // If any watcher root matches this exact path, detach it first. A
    for (root, _) in watcher.list_roots() {
        if root == project_path {
            if let Err(e) = watcher.remove_root(&root) {
                tracing::warn!(
                    error = %e,
                    path = %root.display(),
                    "remove_root failed during projects_move_to_trash",
                );
            }
            break;
        }
    }

    // `trash::delete` is blocking; drop onto the blocking pool so the
    let path_for_move = project_path.clone();
    tauri::async_runtime::spawn_blocking(move || trash::delete(&path_for_move))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| format!("move-to-trash {}: {e}", project_path.display()))?;

    // Drop the project row. `ON DELETE CASCADE` on the FK-owning tables
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(&id)
        .execute(db.pool())
        .await
        .map_err(|e| format!("delete project row {id}: {e}"))?;

    if let Err(e) = events::emit_project_removed(&app, &id) {
        tracing::warn!(error = %e, id = %id, "emit project:removed failed");
    }

    Ok(())
}

// =====================================================================

/// Outcome of a single `projects.repair` invocation. camelCase in the TS
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/types/rust.ts", rename_all = "camelCase")]
pub struct ProjectRepairReport {
    /// Number of files re-read (present OR absent). `project.json`,
    #[ts(type = "number")]
    pub files_checked: u32,
    /// Subset of `files_checked` where the on-disk contents were
    #[ts(type = "number")]
    pub files_repaired: u32,
    /// Human-readable notes for each repaired / skipped file. Surfaced in
    pub issues: Vec<String>,
}

/// `projects.repair(project_id)` - re-read every `.atlas/*.json` for a
// D8 WIP - not yet wired into `invoke_handler![..]`; silenced so the
#[allow(dead_code)]
#[tauri::command]
pub async fn projects_repair(
    db: tauri::State<'_, Db>,
    app: tauri::AppHandle,
    project_id: String,
) -> Result<ProjectRepairReport, String> {
    let project = db
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(&project.path);

    let mut report = ProjectRepairReport {
        files_checked: 0,
        files_repaired: 0,
        issues: Vec::new(),
    };

    // --- flat `.atlas/*.json` files ---

    // scripts.json - list of Script
    repair_json_list::<Script>(&project_path, "scripts", &mut report);
    // todos.json - list of Todo
    repair_json_list::<Todo>(&project_path, "todos", &mut report);
    // project.json - tolerant JSON object (no concrete type; any JSON
    repair_json_object(&project_path, "project", &mut report);
    // meta.json - same treatment as project.json; the shape is informal
    repair_json_object(&project_path, "meta", &mut report);

    // --- notes/*.json ---
    repair_notes_dir(&project_path, &mut report);

    // --- re-index into SQLite ---
    if let Err(e) = db.resync_project_fts(&project_id).await {
        tracing::warn!(error = %e, id = %project_id, "resync_project_fts failed during repair");
        report.issues.push(format!("projects_fts: {e}"));
    }

    // todos index - reload from the (possibly just-rewritten) JSON.
    match db.todos_list(&project_id).await {
        Ok(todos) => {
            if let Err(e) = db.sync_todos_index(&project_id, &todos).await {
                tracing::warn!(error = %e, id = %project_id, "sync_todos_index failed during repair");
                report.issues.push(format!("todos_fts: {e}"));
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, id = %project_id, "todos_list failed during repair");
            report.issues.push(format!("todos_list: {e}"));
        }
    }

    // notes count - walks the notes dir and writes projects.notes_count.
    if let Err(e) = db.refresh_notes_count(&project_id, &project_path).await {
        tracing::warn!(error = %e, id = %project_id, "refresh_notes_count failed during repair");
        report.issues.push(format!("refresh_notes_count: {e}"));
    }

    // Bump `updated_at` so the sync worker doesn't revisit this project
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = sqlx::query("UPDATE projects SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&project_id)
        .execute(db.pool())
        .await
    {
        tracing::warn!(error = %e, id = %project_id, "bump updated_at failed during repair");
    }

    // Toast - UI listens globally for `toast { kind, message }`.
    let message = if report.files_repaired == 0 {
        format!(
            "Project '{}' is healthy ({} files checked)",
            project.name, report.files_checked
        )
    } else {
        format!(
            "Repaired {} of {} files for '{}'",
            report.files_repaired, report.files_checked, project.name
        )
    };
    if let Err(e) = events::emit_toast(&app, "success", &message) {
        tracing::warn!(error = %e, "emit_toast failed after repair");
    }

    Ok(report)
}

/// Re-read `<project>/.atlas/<name>.json` expecting a JSON array of `T`.
#[allow(dead_code)]
fn repair_json_list<T>(project_path: &Path, name: &str, report: &mut ProjectRepairReport)
where
    T: serde::de::DeserializeOwned,
{
    report.files_checked = report.files_checked.saturating_add(1);
    let file = atlas_file(project_path, name);
    if !file.exists() {
        return;
    }
    match std::fs::read(&file) {
        Ok(bytes) => match serde_json::from_slice::<Vec<T>>(&bytes) {
            Ok(_) => {
                // Parseable - nothing to do. Don't rewrite a fine file.
            }
            Err(err) => {
                let msg = format!("{name}.json: invalid JSON ({err}); rewrote as []");
                tracing::warn!(
                    error = %err,
                    path = %file.display(),
                    "malformed .atlas/{name}.json — substituting empty list"
                );
                let empty: Vec<serde_json::Value> = Vec::new();
                if let Err(e) = write_json(&file, &empty) {
                    tracing::warn!(error = %e, path = %file.display(), "rewrite as [] failed");
                    report.issues.push(format!("{name}.json: {e}"));
                    return;
                }
                report.files_repaired = report.files_repaired.saturating_add(1);
                report.issues.push(msg);
            }
        },
        Err(err) => {
            let msg = format!("{name}.json: read failed ({err}); rewrote as []");
            tracing::warn!(
                error = %err,
                path = %file.display(),
                "unreadable .atlas/{name}.json — substituting empty list"
            );
            let empty: Vec<serde_json::Value> = Vec::new();
            if let Err(e) = write_json(&file, &empty) {
                tracing::warn!(error = %e, path = %file.display(), "rewrite as [] failed");
                report.issues.push(format!("{name}.json: {e}"));
                return;
            }
            report.files_repaired = report.files_repaired.saturating_add(1);
            report.issues.push(msg);
        }
    }
}

/// Re-read `<project>/.atlas/<name>.json` expecting a JSON object. Same
#[allow(dead_code)]
fn repair_json_object(project_path: &Path, name: &str, report: &mut ProjectRepairReport) {
    report.files_checked = report.files_checked.saturating_add(1);
    let file = atlas_file(project_path, name);
    if !file.exists() {
        return;
    }
    match std::fs::read(&file) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(
            &bytes,
        ) {
            Ok(_) => {}
            Err(err) => {
                let msg = format!("{name}.json: invalid JSON ({err}); rewrote as {{}}");
                tracing::warn!(
                    error = %err,
                    path = %file.display(),
                    "malformed .atlas/{name}.json — substituting empty object"
                );
                let empty = serde_json::Map::<String, serde_json::Value>::new();
                if let Err(e) = write_json(&file, &empty) {
                    tracing::warn!(error = %e, path = %file.display(), "rewrite as {{}} failed");
                    report.issues.push(format!("{name}.json: {e}"));
                    return;
                }
                report.files_repaired = report.files_repaired.saturating_add(1);
                report.issues.push(msg);
            }
        },
        Err(err) => {
            let msg = format!("{name}.json: read failed ({err}); rewrote as {{}}");
            tracing::warn!(
                error = %err,
                path = %file.display(),
                "unreadable .atlas/{name}.json — substituting empty object"
            );
            let empty = serde_json::Map::<String, serde_json::Value>::new();
            if let Err(e) = write_json(&file, &empty) {
                tracing::warn!(error = %e, path = %file.display(), "rewrite as {{}} failed");
                report.issues.push(format!("{name}.json: {e}"));
                return;
            }
            report.files_repaired = report.files_repaired.saturating_add(1);
            report.issues.push(msg);
        }
    }
}

/// Walk `<project>/.atlas/notes/*.json`. Every file counts toward
#[allow(dead_code)]
fn repair_notes_dir(project_path: &Path, report: &mut ProjectRepairReport) {
    let notes_dir = project_path.join(".atlas").join("notes");
    let entries = match std::fs::read_dir(&notes_dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %notes_dir.display(),
                "read_dir(notes) failed during repair"
            );
            report
                .issues
                .push(format!("notes/: read_dir failed ({err})"));
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // Only `*.json`, skip hidden (`.foo.json.tmp`) so an
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        report.files_checked = report.files_checked.saturating_add(1);

        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(err) => {
                let msg = format!(
                    "{}: read failed ({err}); rewrote as {{}}",
                    path.display()
                );
                tracing::warn!(
                    error = %err,
                    path = %path.display(),
                    "unreadable note — substituting empty object"
                );
                let empty = serde_json::Map::<String, serde_json::Value>::new();
                if let Err(e) = write_json(&path, &empty) {
                    tracing::warn!(error = %e, "rewrite note failed");
                    report.issues.push(format!("{}: {e}", path.display()));
                    continue;
                }
                report.files_repaired = report.files_repaired.saturating_add(1);
                report.issues.push(msg);
                continue;
            }
        };

        if serde_json::from_slice::<Note>(&bytes).is_ok() {
            continue;
        }

        let msg = format!("{}: invalid note JSON; rewrote as {{}}", path.display());
        tracing::warn!(
            path = %path.display(),
            "malformed note — substituting empty object"
        );
        let empty = serde_json::Map::<String, serde_json::Value>::new();
        if let Err(e) = write_json(&path, &empty) {
            tracing::warn!(error = %e, "rewrite note failed");
            report.issues.push(format!("{}: {e}", path.display()));
            continue;
        }
        report.files_repaired = report.files_repaired.saturating_add(1);
        report.issues.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut p = std::env::temp_dir();
        p.push(format!("atlas-repair-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&p).expect("create tempdir");
        p
    }

    /// A truncated todos file is rewritten as `[]` with
    #[test]
    fn repair_rewrites_truncated_todos_as_empty_list() -> anyhow::Result<()> {
        let project = unique_dir("truncated_todos");
        let atlas = project.join(".atlas");
        fs::create_dir_all(&atlas)?;
        let todos = atlas.join("todos.json");
        // Truncated JSON - unclosed object.
        fs::write(&todos, br#"{"not_valid":"#)?;
        assert!(todos.exists());

        let mut report = ProjectRepairReport {
            files_checked: 0,
            files_repaired: 0,
            issues: Vec::new(),
        };
        repair_json_list::<Todo>(&project, "todos", &mut report);

        assert!(
            report.files_repaired >= 1,
            "expected >=1 repaired, got {}",
            report.files_repaired
        );
        assert!(report.files_checked >= 1);

        // File on disk is now parseable as an empty list.
        let bytes = fs::read(&todos)?;
        let parsed: Vec<Todo> = serde_json::from_slice(&bytes)
            .expect("rewritten todos.json must parse as Vec<Todo>");
        assert!(parsed.is_empty(), "expected empty list, got {} items", parsed.len());

        fs::remove_dir_all(&project).ok();
        Ok(())
    }

    /// A healthy todos file is NOT rewritten - we should only touch
    #[test]
    fn repair_leaves_healthy_todos_alone() -> anyhow::Result<()> {
        let project = unique_dir("healthy_todos");
        let atlas = project.join(".atlas");
        fs::create_dir_all(&atlas)?;
        let todos = atlas.join("todos.json");
        let original = br#"[{"id":"t1","done":false,"text":"ship","createdAt":"2025-01-01T00:00:00Z"}]"#;
        fs::write(&todos, original)?;

        let mut report = ProjectRepairReport {
            files_checked: 0,
            files_repaired: 0,
            issues: Vec::new(),
        };
        repair_json_list::<Todo>(&project, "todos", &mut report);

        assert_eq!(report.files_repaired, 0);
        assert_eq!(report.files_checked, 1);
        // File content is unchanged.
        let now = fs::read(&todos)?;
        assert_eq!(now, original);

        fs::remove_dir_all(&project).ok();
        Ok(())
    }

    /// A missing todos file is not a problem and not a repair - missing
    #[test]
    fn repair_is_tolerant_of_missing_todos() -> anyhow::Result<()> {
        let project = unique_dir("missing_todos");
        fs::create_dir_all(project.join(".atlas"))?;

        let mut report = ProjectRepairReport {
            files_checked: 0,
            files_repaired: 0,
            issues: Vec::new(),
        };
        repair_json_list::<Todo>(&project, "todos", &mut report);

        assert_eq!(report.files_checked, 1);
        assert_eq!(report.files_repaired, 0);

        fs::remove_dir_all(&project).ok();
        Ok(())
    }

    /// `repair_notes_dir` walks `notes/*.json` and rewrites any
    #[test]
    fn repair_notes_dir_rewrites_bad_entries() -> anyhow::Result<()> {
        let project = unique_dir("notes_dir");
        let notes = project.join(".atlas").join("notes");
        fs::create_dir_all(&notes)?;

        // Good note.
        fs::write(
            notes.join("ok.json"),
            br#"{"id":"ok","title":"","body":"","pinned":false,"createdAt":"x","updatedAt":"y"}"#,
        )?;
        // Bad note - truncated.
        fs::write(notes.join("bad.json"), br#"{"truncated":"#)?;
        // Hidden temp - should be ignored entirely (not counted).
        fs::write(notes.join(".tmp.json.tmp"), br#"nope"#)?;

        let mut report = ProjectRepairReport {
            files_checked: 0,
            files_repaired: 0,
            issues: Vec::new(),
        };
        repair_notes_dir(&project, &mut report);

        // Two non-hidden json files inspected, one repaired.
        assert_eq!(report.files_checked, 2);
        assert_eq!(report.files_repaired, 1);

        fs::remove_dir_all(&project).ok();
        Ok(())
    }
}
