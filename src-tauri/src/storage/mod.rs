//! Atlas storage layer.

#![allow(dead_code, unused_imports)] // consumers land in later iterations; contract is fixed now

pub mod db;
pub mod discovery;
pub mod json;
pub mod migrate;
pub mod planner_io;
pub mod settings;
pub mod sync;
pub mod templates;
pub mod types;

use std::path::PathBuf;

pub use db::Db;
pub use discovery::{scan_root, DiscoveredRepo};
pub use types::{
    AdvancedSettings, CloneDepth, Collection, CollectionId, DiscoveryResult, EditorEntry,
    EditorsSettings, ExtensionEvent, ExtensionReason, FileKind, FileNode, FullLiteral,
    GeneralSettings, GitPollInterval, GitSettings, Goal, Lang, Milestone, MilestoneId,
    MilestoneStatus, Note, NoteId, PaletteItem, Pane, PaneId, PaneKind, PaneLayout, PaneSnapshot,
    PaneStatus, PlannerState, Priority, Project, ProjectFilter, ProjectId, ProjectSource, Routine,
    RoutineId, RoutineInstance, RoutineInstanceId, ScoreSnapshot, Script, ScriptGroup, ScriptId,
    Session, SessionId, SessionStatus, Settings, Template, TemplateId, Theme, TimelineConfig, Todo,
    TodoId, WatchRoot,
};

/// Bundle of long-lived resources the settings + templates commands need.
#[derive(Clone)]
pub struct AppContext {
    pub app_data_dir: PathBuf,
    pub db: Db,
}
