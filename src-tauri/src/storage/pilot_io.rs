//! Per-project Atlas Pilot JSON IO.
//!
//! Pilot state is per-project and lives under `<project>/.atlas/pilot/`:
//!
//! ```text
//! .atlas/pilot/
//!   project.json            Atlas-owned record
//!   requirements.md         skill-written (gate REQS)
//!   prd.md                  skill-written (gate PRD)
//!   epics/NN.json           skill-written (gate EPICS)
//!   epics/NN/history.jsonl  skill-appended (epic mode)
//! ```
//!
//! `project.json` and the `epics/*.json` files are atomic JSON (shared
//! `json` helpers). `history.jsonl` is append-only — one JSON object per
//! line — so it is read/written here directly rather than via `json`.

#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::storage::json::{read_json, write_json};
use crate::storage::types::{Epic, HistoryEntry, PilotProject};

// ----- path helpers -----

/// `<project>/.atlas/pilot/`.
pub fn pilot_dir(project_path: &Path) -> PathBuf {
    project_path.join(".atlas").join("pilot")
}

fn project_file(project_path: &Path) -> PathBuf {
    pilot_dir(project_path).join("project.json")
}

fn epics_dir(project_path: &Path) -> PathBuf {
    pilot_dir(project_path).join("epics")
}

fn epic_file(project_path: &Path, number: i64) -> PathBuf {
    epics_dir(project_path).join(format!("{number:02}.json"))
}

/// `<project>/.atlas/pilot/epics/NN/history.jsonl`.
pub fn history_file(project_path: &Path, number: i64) -> PathBuf {
    epics_dir(project_path)
        .join(format!("{number:02}"))
        .join("history.jsonl")
}

/// `<project>/.atlas/pilot/requirements.md` — the gate-REQS artifact.
pub fn requirements_file(project_path: &Path) -> PathBuf {
    pilot_dir(project_path).join("requirements.md")
}

/// `<project>/.atlas/pilot/prd.md` — the gate-PRD artifact.
pub fn prd_file(project_path: &Path) -> PathBuf {
    pilot_dir(project_path).join("prd.md")
}

/// True if this project is an Atlas Pilot project (has a `project.json`).
pub fn is_pilot(project_path: &Path) -> bool {
    project_file(project_path).exists()
}

// ----- project.json -----

pub fn load_project(project_path: &Path) -> anyhow::Result<Option<PilotProject>> {
    read_json(&project_file(project_path))
}

pub fn save_project(project_path: &Path, project: &PilotProject) -> anyhow::Result<()> {
    write_json(&project_file(project_path), project)
}

/// Create the `.atlas/pilot/` skeleton for a brand-new pilot project and
/// write its initial `project.json`. Idempotent on the directory.
pub fn init_pilot(project_path: &Path, project: &PilotProject) -> anyhow::Result<()> {
    fs::create_dir_all(epics_dir(project_path))?;
    save_project(project_path, project)
}

// ----- epics/NN.json -----

pub fn load_epic(project_path: &Path, number: i64) -> anyhow::Result<Option<Epic>> {
    read_json(&epic_file(project_path, number))
}

pub fn save_epic(project_path: &Path, epic: &Epic) -> anyhow::Result<()> {
    write_json(&epic_file(project_path, epic.number), epic)
}

/// Load every `epics/NN.json`, sorted by epic number. Missing dir → empty.
pub fn load_epics(project_path: &Path) -> anyhow::Result<Vec<Epic>> {
    let dir = epics_dir(project_path);
    let mut out: Vec<Epic> = Vec::new();
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(anyhow::anyhow!("read {}: {e}", dir.display())),
    };
    for entry in rd {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match read_json::<Epic>(&path) {
            Ok(Some(epic)) => out.push(epic),
            Ok(None) => {}
            Err(err) => tracing::warn!(?err, path = %path.display(), "pilot: bad epic file"),
        }
    }
    out.sort_by_key(|e| e.number);
    Ok(out)
}

// ----- epics/NN/history.jsonl -----

/// Read and parse an epic's history. Missing file → empty. Malformed
/// lines are skipped (a half-written tail line must not lose the rest).
pub fn load_history(project_path: &Path, number: i64) -> anyhow::Result<Vec<HistoryEntry>> {
    let path = history_file(project_path, number);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(anyhow::anyhow!("read {}: {e}", path.display())),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<HistoryEntry>(line) {
            Ok(entry) => out.push(entry),
            Err(err) => tracing::trace!(?err, "pilot: skipping malformed history line"),
        }
    }
    Ok(out)
}

/// Append one entry to an epic's `history.jsonl`, creating the file and
/// its parent directory if needed.
pub fn append_history(
    project_path: &Path,
    number: i64,
    entry: &HistoryEntry,
) -> anyhow::Result<()> {
    let path = history_file(project_path, number);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

// ----- global pilot registry (app-data scoped) -----
//
// Pilot projects live anywhere on disk; the Pilot window needs to list them
// without depending on Atlas's main project index. A small JSON file in
// app-data tracks their paths.

const REGISTRY_FILE: &str = "pilot_projects.json";

fn registry_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(REGISTRY_FILE)
}

/// Known pilot project paths, with any that have lost their `project.json`
/// (deleted, moved) filtered out.
pub fn load_registry(app_data_dir: &Path) -> Vec<PathBuf> {
    let raw: Vec<String> = read_json(&registry_path(app_data_dir))
        .ok()
        .flatten()
        .unwrap_or_default();
    raw.into_iter()
        .map(PathBuf::from)
        .filter(|p| is_pilot(p))
        .collect()
}

/// Add a project path to the registry. Idempotent.
pub fn register(app_data_dir: &Path, project_path: &Path) -> anyhow::Result<()> {
    let mut paths = load_registry(app_data_dir);
    if !paths.iter().any(|p| p == project_path) {
        paths.push(project_path.to_path_buf());
    }
    let as_str: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    write_json(&registry_path(app_data_dir), &as_str)
}

// =====================================================================
// Tests.
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::{EpicStatus, HistoryKind, PilotGate, PilotStatus};
    use tempfile::TempDir;

    fn mk_project() -> PilotProject {
        PilotProject {
            name: "demo".into(),
            status: PilotStatus::Draft,
            gate: Some(PilotGate::Reqs),
            auto_advance: true,
            planning_session_id: None,
            created_at: "2026-05-18T00:00:00Z".into(),
        }
    }

    fn mk_epic(number: i64) -> Epic {
        Epic {
            number,
            title: format!("epic {number}"),
            goal: "ship something".into(),
            description: String::new(),
            release: Some("r1".into()),
            status: EpicStatus::Pending,
            depends_on: Vec::new(),
            tasks: Vec::new(),
            session_id: None,
            iterations: 0,
        }
    }

    #[test]
    fn project_round_trip() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        assert!(!is_pilot(dir.path()));
        assert!(load_project(dir.path())?.is_none());

        init_pilot(dir.path(), &mk_project())?;
        assert!(is_pilot(dir.path()));
        let loaded = load_project(dir.path())?.expect("present");
        assert_eq!(loaded.status, PilotStatus::Draft);
        assert!(loaded.auto_advance);
        Ok(())
    }

    #[test]
    fn epics_sorted_by_number() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        init_pilot(dir.path(), &mk_project())?;
        save_epic(dir.path(), &mk_epic(3))?;
        save_epic(dir.path(), &mk_epic(1))?;
        save_epic(dir.path(), &mk_epic(2))?;

        let epics = load_epics(dir.path())?;
        assert_eq!(
            epics.iter().map(|e| e.number).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // Zero-padded filename.
        assert!(dir.path().join(".atlas/pilot/epics/01.json").exists());
        assert_eq!(load_epic(dir.path(), 2)?.expect("present").number, 2);
        Ok(())
    }

    #[test]
    fn history_appends_one_line_per_entry() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        assert!(load_history(dir.path(), 1)?.is_empty());

        for i in 0..3 {
            append_history(
                dir.path(),
                1,
                &HistoryEntry {
                    ts: format!("2026-05-18T0{i}:00:00Z"),
                    kind: HistoryKind::Task,
                    summary: format!("task {i}"),
                    files: vec![format!("src/f{i}.rs")],
                    rationale: String::new(),
                },
            )?;
        }
        let hist = load_history(dir.path(), 1)?;
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[2].summary, "task 2");

        // A garbage tail line is skipped, not fatal.
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(history_file(dir.path(), 1))?;
        f.write_all(b"{ not json\n")?;
        assert_eq!(load_history(dir.path(), 1)?.len(), 3);
        Ok(())
    }
}
