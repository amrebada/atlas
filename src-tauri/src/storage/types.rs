//! Rust mirrors of `src/types/index.ts`.

// Types here predate their consumers by several iterations. Suppress
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---------- Branded id type aliases ----------

pub type ProjectId = String;
pub type CollectionId = String;
pub type TodoId = String;
pub type NoteId = String;
pub type ScriptId = String;
pub type SessionId = String;
pub type TemplateId = String;
pub type PaneId = String;

// ---------- Enums ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum Lang {
    TypeScript,
    JavaScript,
    Rust,
    Go,
    Python,
    Swift,
    Kotlin,
    Ruby,
    Java,
    C,
    #[serde(rename = "C++")]
    #[ts(rename = "C++")]
    CPlusPlus,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "kebab-case"
)]
pub enum PaneKind {
    Shell,
    Script,
    ClaudeSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum PaneStatus {
    Idle,
    Running,
    Active,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum SessionStatus {
    Active,
    Idle,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum FileKind {
    Dir,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum ScriptGroup {
    Run,
    Build,
    Check,
    Util,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum Theme {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum GitPollInterval {
    #[serde(rename = "10s")]
    #[ts(rename = "10s")]
    TenSec,
    #[serde(rename = "30s")]
    #[ts(rename = "30s")]
    ThirtySec,
    #[serde(rename = "1m")]
    #[ts(rename = "1m")]
    OneMin,
    #[serde(rename = "off")]
    #[ts(rename = "off")]
    Off,
}

// ---------- Core entities ----------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub path: String,
    pub language: Lang,
    pub color: String,
    pub branch: String,
    #[ts(type = "number")]
    pub dirty: i64,
    #[ts(type = "number")]
    pub ahead: i64,
    #[ts(type = "number")]
    pub behind: i64,
    #[ts(type = "number")]
    pub loc: i64,
    pub size: String,
    #[ts(type = "number")]
    pub size_bytes: i64,
    /// Pretty-printed on-disk size, e.g. `"16.4 GB"`. Matches
    /// `disk_bytes` formatting via `util::format_bytes`.
    pub disk_size: String,
    /// Full on-disk footprint including files `.gitignore` hides
    /// (`node_modules`, build outputs). Always ≥ `size_bytes`.
    #[ts(type = "number")]
    pub disk_bytes: i64,
    pub last_opened: Option<String>,
    pub pinned: bool,
    pub tags: Vec<String>,
    #[ts(type = "number")]
    pub todos_count: i64,
    #[ts(type = "number")]
    pub notes_count: i64,
    pub time: String,
    pub archived: bool,
    pub collection_ids: Vec<CollectionId>,
    /// Author name of the repo's current HEAD commit, or `None` for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct Collection {
    pub id: CollectionId,
    pub label: String,
    pub dot: String,
    #[ts(type = "number")]
    pub order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Todo {
    pub id: TodoId,
    pub done: bool,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct ScriptEnvVar {
    pub key: String,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct Script {
    pub id: ScriptId,
    pub name: String,
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    pub group: ScriptGroup,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_defaults: Vec<ScriptEnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Session {
    pub id: SessionId,
    pub project_path: String,
    pub title: String,
    pub when: String,
    #[ts(type = "number")]
    pub turns: i64,
    pub duration: String,
    pub status: SessionStatus,
    pub last: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct FileNode {
    #[ts(type = "number")]
    pub depth: i64,
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    /// `Some("M")` / `Some("+")` / `Some("-")` / `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct Template {
    pub id: TemplateId,
    pub label: String,
    pub color: String,
    pub hint: String,
    pub path: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Pane {
    pub id: PaneId,
    pub kind: PaneKind,
    pub title: String,
    pub status: PaneStatus,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_id: Option<ScriptId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
}

// `PaneLayout` is the user-visible arrangement of terminal panes on a

/// The snapshot we persist per pane. Carries the user-visible hints
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct PaneSnapshot {
    pub id: String,
    /// One of "shell" | "script" | "claude-session" (mirrors `PaneKind`).
    pub kind: String,
    pub title: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_id: Option<ScriptId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
}

/// Last-known layout for a project's terminal strip. Persisted on every
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct PaneLayout {
    /// One of "tabs" | "split-v" | "split-h" | "grid". Kept as string
    pub mode: String,
    pub panes: Vec<PaneSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_pane_id: Option<String>,
}

// ---------- Settings ----------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct EditorEntry {
    pub id: String,
    pub name: String,
    pub cmd: String,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct WatchRoot {
    pub path: String,
    #[ts(type = "number")]
    pub depth: i64,
    #[ts(type = "number")]
    pub repo_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub menu_bar_agent: bool,
    pub default_project_location: String,
    pub theme: Theme,
    #[serde(default = "default_terminal_theme")]
    pub terminal_theme: Theme,
}

fn default_terminal_theme() -> Theme {
    Theme::System
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct EditorsSettings {
    pub detected: Vec<EditorEntry>,
    pub default_id: Option<String>,
}

/// Clone depth - `number | 'full'` in TS. Encoded here as a tagged enum
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum CloneDepth {
    Depth(#[ts(type = "number")] i64),
    #[serde(rename = "full")]
    Full(FullLiteral),
}

/// Literal wrapper so `untagged` can distinguish "full" from a number.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum FullLiteral {
    #[serde(rename = "full")]
    #[ts(rename = "full")]
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct GitSettings {
    pub poll_interval: GitPollInterval,
    pub show_author: bool,
    pub default_clone_depth: CloneDepth,
    pub ssh_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct AdvancedSettings {
    pub use_spotlight: bool,
    pub crash_reports: bool,
    pub shell: String,
    /// hook appends each panic payload + backtrace to
    #[serde(default)]
    pub crash_log: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct Settings {
    pub general: GeneralSettings,
    pub editors: EditorsSettings,
    pub git: GitSettings,
    pub watchers: Vec<WatchRoot>,
    pub templates: Vec<Template>,
    pub shortcuts: std::collections::HashMap<String, String>,
    pub advanced: AdvancedSettings,
}

// ---------- Palette ----------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum PaletteItem {
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "project")]
    Project {
        project: Project,
        #[ts(type = "number")]
        score: f32,
    },
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "recent")]
    Recent { project: Project },
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "note")]
    Note {
        project_id: ProjectId,
        note_id: NoteId,
        title: String,
        snippet: String,
        #[ts(type = "number")]
        score: f32,
    },
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "action")]
    Action {
        id: String,
        label: String,
        hint: String,
        keys: Vec<String>,
    },
}

// ---------- Query types ----------

/// Filter applied to `Db::list_projects`. Kept open to grow as lanes land.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFilter {
    /// Include archived projects? Default false.
    pub include_archived: bool,
    /// Limit to pinned only.
    pub pinned_only: bool,
    /// Intersect with a single tag.
    pub tag: Option<String>,
    /// Intersect with a single collection id.
    pub collection_id: Option<String>,
}

// ---------- Discovery / provenance ----------

/// Provenance tag on `projects.source`. Keeps seeded fixtures separable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSource {
    Seed,
    Discovery,
    Manual,
}

impl ProjectSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectSource::Seed => "seed",
            ProjectSource::Discovery => "discovery",
            ProjectSource::Manual => "manual",
        }
    }
}

/// Result of `Db::discover_root` - returned to UI so it can surface a
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct DiscoveryResult {
    pub root: String,
    pub new_project_ids: Vec<ProjectId>,
    #[ts(type = "number")]
    pub total_repos: i64,
}
