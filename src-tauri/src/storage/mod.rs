//! Atlas storage layer.

#![allow(dead_code, unused_imports)] // consumers land in later iterations; contract is fixed now

pub mod db;
pub mod discovery;
pub mod json;
pub mod settings;
pub mod sync;
pub mod templates;
pub mod types;

use std::path::PathBuf;

pub use db::Db;
pub use discovery::{scan_root, DiscoveredRepo};
pub use types::{
    AdvancedSettings, CloneDepth, Collection, CollectionId, DiscoveryResult, EditorEntry,
    EditorsSettings, FileKind, FileNode, FullLiteral, GeneralSettings, GitPollInterval,
    GitSettings, Lang, Note, NoteId, PaletteItem, Pane, PaneId, PaneKind, PaneLayout, PaneSnapshot,
    PaneStatus, Project, ProjectFilter, ProjectId, ProjectSource, Script, ScriptGroup, ScriptId,
    Session, SessionId, SessionStatus, Settings, Template, TemplateId, Theme, Todo, TodoId,
    WatchRoot,
};

/// Bundle of long-lived resources the settings + templates commands need.
#[derive(Clone)]
pub struct AppContext {
    pub app_data_dir: PathBuf,
    pub db: Db,
}
