//! Global planner-feature JSON IO.
//!
//! Routines and routine instances are *not* per-project — a routine
//! can span projects, and the materialiser benefits from one canonical
//! file to read/write. We follow the same pattern as `storage/settings`:
//! free functions taking `app_data_dir: &Path`, atomic writes via the
//! shared `json` helpers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::storage::json::{read_json, write_json};
use crate::storage::types::{PlannerState, Routine, RoutineInstance, TimelineConfig};

const ROUTINES_FILE: &str = "routines.json";
const ROUTINE_INSTANCES_FILE: &str = "routine_instances.json";
const TIMELINE_CONFIG_FILE: &str = "timeline_config.json";
const PLANNER_STATE_FILE: &str = "planner_state.json";

fn routines_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ROUTINES_FILE)
}
fn instances_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ROUTINE_INSTANCES_FILE)
}
fn timeline_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(TIMELINE_CONFIG_FILE)
}
fn planner_state_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PLANNER_STATE_FILE)
}

// ----- routines -----

pub fn load_routines(app_data_dir: &Path) -> anyhow::Result<Vec<Routine>> {
    Ok(read_json::<Vec<Routine>>(&routines_path(app_data_dir))?.unwrap_or_default())
}

pub fn save_routines(app_data_dir: &Path, routines: &[Routine]) -> anyhow::Result<()> {
    write_json(&routines_path(app_data_dir), routines)
}

pub fn upsert_routine(app_data_dir: &Path, routine: &Routine) -> anyhow::Result<()> {
    let mut all = load_routines(app_data_dir)?;
    if let Some(slot) = all.iter_mut().find(|r| r.id == routine.id) {
        *slot = routine.clone();
    } else {
        all.push(routine.clone());
    }
    save_routines(app_data_dir, &all)
}

pub fn delete_routine(app_data_dir: &Path, routine_id: &str) -> anyhow::Result<bool> {
    let mut all = load_routines(app_data_dir)?;
    let before = all.len();
    all.retain(|r| r.id != routine_id);
    let removed = all.len() != before;
    if removed {
        save_routines(app_data_dir, &all)?;
        // Also drop the routine's instances so we don't keep an orphan
        // history pointing at a routine that no longer exists.
        let mut insts = load_instances(app_data_dir)?;
        insts.retain(|i| i.routine_id != routine_id);
        save_instances(app_data_dir, &insts)?;
    }
    Ok(removed)
}

// ----- routine instances -----

pub fn load_instances(app_data_dir: &Path) -> anyhow::Result<Vec<RoutineInstance>> {
    Ok(read_json::<Vec<RoutineInstance>>(&instances_path(app_data_dir))?.unwrap_or_default())
}

pub fn save_instances(app_data_dir: &Path, instances: &[RoutineInstance]) -> anyhow::Result<()> {
    write_json(&instances_path(app_data_dir), instances)
}

pub fn upsert_instances(
    app_data_dir: &Path,
    new_or_updated: &[RoutineInstance],
) -> anyhow::Result<()> {
    if new_or_updated.is_empty() {
        return Ok(());
    }
    let mut all = load_instances(app_data_dir)?;
    for inst in new_or_updated {
        if let Some(slot) = all.iter_mut().find(|i| i.id == inst.id) {
            *slot = inst.clone();
        } else {
            all.push(inst.clone());
        }
    }
    save_instances(app_data_dir, &all)
}

// ----- timeline config -----

pub fn load_timeline_config(app_data_dir: &Path) -> anyhow::Result<TimelineConfig> {
    Ok(read_json::<TimelineConfig>(&timeline_path(app_data_dir))?.unwrap_or_default())
}

pub fn save_timeline_config(app_data_dir: &Path, cfg: &TimelineConfig) -> anyhow::Result<()> {
    write_json(&timeline_path(app_data_dir), cfg)
}

// ----- planner state -----

pub fn load_planner_state(app_data_dir: &Path) -> anyhow::Result<PlannerState> {
    Ok(read_json::<PlannerState>(&planner_state_path(app_data_dir))?.unwrap_or_default())
}

pub fn save_planner_state(app_data_dir: &Path, state: &PlannerState) -> anyhow::Result<()> {
    write_json(&planner_state_path(app_data_dir), state)
}

// =====================================================================
// Tests.
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::{Goal, Priority};
    use tempfile::TempDir;

    fn mk_routine(id: &str) -> Routine {
        Routine {
            id: id.into(),
            project_id: None,
            title: format!("routine-{id}"),
            description: None,
            rrule: "FREQ=DAILY".into(),
            start_date: "2026-05-01".into(),
            goal: Goal::Indefinite,
            priority: Priority::P2,
            estimate: None,
            paused: false,
            paused_from: None,
            success_points: 0.0,
            failing_points: 0.0,
            extensions: Vec::new(),
            created_at: "2026-04-30T00:00:00Z".into(),
        }
    }

    #[test]
    fn routines_round_trip() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        assert!(load_routines(dir.path())?.is_empty());

        upsert_routine(dir.path(), &mk_routine("a"))?;
        upsert_routine(dir.path(), &mk_routine("b"))?;
        let listed = load_routines(dir.path())?;
        assert_eq!(listed.len(), 2);

        // Update preserves order.
        let mut b2 = mk_routine("b");
        b2.title = "updated".into();
        upsert_routine(dir.path(), &b2)?;
        let listed = load_routines(dir.path())?;
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1].title, "updated");

        // Delete returns true and drops instances too.
        upsert_instances(
            dir.path(),
            &[RoutineInstance {
                id: "i1".into(),
                routine_id: "a".into(),
                scheduled_for: "2026-05-01".into(),
                done_at: None,
                skipped: None,
                extension_contribution: 0,
                failing_points: 0.0,
                success_points: 0.0,
            }],
        )?;
        assert!(delete_routine(dir.path(), "a")?);
        let listed = load_routines(dir.path())?;
        assert_eq!(listed.len(), 1);
        let insts = load_instances(dir.path())?;
        assert!(insts.is_empty());

        // Delete missing → false, no error.
        assert!(!delete_routine(dir.path(), "ghost")?);
        Ok(())
    }

    #[test]
    fn timeline_config_defaults_when_missing() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let cfg = load_timeline_config(dir.path())?;
        assert!(cfg.pinned_project_ids.is_empty());
        assert_eq!(cfg.visible_range, "month");
        Ok(())
    }
}
