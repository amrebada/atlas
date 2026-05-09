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
pub type MilestoneId = String;
pub type RoutineId = String;
pub type RoutineInstanceId = String;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum Theme {
    Dark,
    Light,
    #[default]
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
    /// Legacy free-form due label (e.g. `"today"`, `"fri"`, ISO-8601).
    /// Preserved for backward compatibility; new writes prefer `deadline`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    pub created_at: String,
    // ---- Planner-feature fields (P1 schema; populated by P2+ flows) ----
    /// Owning project id. Optional during migration window — populated by
    /// the upsert path going forward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    /// Membership in a project milestone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub milestone_id: Option<MilestoneId>,
    /// Set when this todo was generated by a routine instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routine_instance_id: Option<RoutineInstanceId>,
    /// Importance bucket. Defaults to P2 when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Structured ISO-8601 deadline. Replaces free-form `due` in new writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    /// Estimated minutes-of-work (for the workload bar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<i64>,
    /// User explicitly pinned this todo into Today, overriding scoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_today: Option<bool>,
    /// ISO-8601 completion timestamp (set when `done` flips to true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
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
    /// Provider id (`"claude"` | `"codex"` | `"opencode"` | …).
    #[serde(default = "default_provider_id")]
    pub provider: String,
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

fn default_provider_id() -> String {
    "claude".to_string()
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
    #[serde(default)]
    pub terminal_theme: Theme,
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
    /// Embedded MCP server config (remote-control feature). Default-off,
    /// loopback-only; the user opts in from Settings → Advanced.
    #[serde(default)]
    pub mcp: McpSettings,
    /// Outbound agent that connects to the relay backend (remote-control
    /// feature). Default-off; settings here are only used if no
    /// `ATLAS_AGENT_*` env vars are set (env wins for dev override).
    #[serde(default)]
    pub agent: AgentSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct McpSettings {
    pub enabled: bool,
    #[ts(type = "number")]
    pub port: u16,
    /// Bearer token clients must send in `Authorization: Bearer …`.
    /// Empty string means the server refuses to start (no anonymous access).
    pub token: String,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8765,
            token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct AgentSettings {
    pub enabled: bool,
    /// WebSocket URL of the relay. Default points at the local stub for
    /// dev (`ws://localhost:9000/agent`). Production relay URL ships
    /// later; users can override per-device.
    pub relay_url: String,
    /// Bearer token presented to the relay on the WS upgrade. Separate
    /// from the device's signing key — the token authenticates the
    /// connection at the transport layer; signed envelopes authenticate
    /// individual messages.
    pub token: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            relay_url: "ws://localhost:9000/agent".into(),
            token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ProvidersSettings {
    /// Per-provider enable flag. Missing entries default to `true` so
    /// newly-registered providers light up automatically.
    pub enabled: std::collections::HashMap<String, bool>,
    /// Provider id used by the "+ new session" main click.
    pub default_id: String,
}

impl Default for ProvidersSettings {
    fn default() -> Self {
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("claude".into(), true);
        enabled.insert("codex".into(), false);
        enabled.insert("opencode".into(), false);
        Self {
            enabled,
            default_id: "claude".into(),
        }
    }
}

impl ProvidersSettings {
    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.get(id).copied().unwrap_or(true)
    }
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
    /// Multi-provider session config. `serde(default)` keeps older
    /// `settings.json` files loadable without a migration step.
    #[serde(default)]
    pub providers: ProvidersSettings,
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
    #[ts(rename = "milestone")]
    Milestone {
        project_id: ProjectId,
        project_name: String,
        milestone_id: MilestoneId,
        title: String,
        deadline: String,
        priority: Priority,
        status: MilestoneStatus,
        #[ts(type = "number")]
        score: f32,
    },
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "routine")]
    Routine {
        routine_id: RoutineId,
        project_id: Option<ProjectId>,
        project_name: Option<String>,
        title: String,
        rrule: String,
        priority: Priority,
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

// =============================================================
// Planner feature — milestones, routines, scoring, timeline.
// Schema introduced 2026-04-30. All fields additive: existing
// project files load without migration via serde defaults.
// =============================================================

/// Importance bucket for todos, milestones, and routines. Drives the
/// scoring weights in `score_engine` (P0=2× / P1=1.5× / P2=1× / P3=0.5×).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum Priority {
    P0,
    P1,
    #[default]
    P2,
    P3,
}

impl Priority {
    /// Multiplier applied to base points. Mirrors the constants in the
    /// design plan §4d.
    pub fn weight(self) -> f64 {
        match self {
            Self::P0 => 2.0,
            Self::P1 => 1.5,
            Self::P2 => 1.0,
            Self::P3 => 0.5,
        }
    }
}

/// Lifecycle state of a Milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum MilestoneStatus {
    Planned,
    Active,
    Done,
    Missed,
    Cancelled,
}

/// Why a deadline was extended. Determines whether the akrasia-horizon
/// cost applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "kebab-case"
)]
pub enum ExtensionReason {
    /// Auto-applied when an instance/milestone passed its deadline by 24h.
    AutoMissed,
    /// User-initiated soften within the 7-day akrasia horizon (costs points).
    UserSoften,
    /// User-initiated soften flagged as "I really mean it" (costs points).
    UserOverride,
    /// Pause-all suspended accrual; not penalised.
    Paused,
}

/// One entry in a Milestone or Routine's `extensions` log. The shape is
/// shared between both because the surfaces are identical.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ExtensionEvent {
    /// ISO-8601 — date being moved away from.
    pub from: String,
    /// ISO-8601 — new target date.
    pub to: String,
    pub reason: ExtensionReason,
    #[ts(type = "number")]
    pub failing_points_applied: f64,
    /// ISO-8601 — when the extension was recorded.
    pub at: String,
    /// Optional free-form note ("scope grew", "external dep slipped").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Goal definition attached to a Routine. The TypeScript shape is a
/// discriminated union on `kind`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub enum Goal {
    /// Run the routine until `target` instances are completed.
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "count")]
    Count {
        #[ts(type = "number")]
        target: i64,
        /// Running tally of completed instances. Maintained by the engine.
        #[serde(default)]
        #[ts(type = "number")]
        completed: i64,
    },
    /// Run the routine until this date.
    #[serde(rename_all = "camelCase")]
    #[ts(rename = "deadline")]
    Deadline { until: String },
    /// Open-ended; the routine has no completion criterion.
    #[ts(rename = "indefinite")]
    Indefinite,
}

/// A milestone groups a set of todos under a single deadline within a
/// project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Milestone {
    pub id: MilestoneId,
    pub project_id: ProjectId,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// ISO-8601 — current target date.
    pub deadline: String,
    /// ISO-8601 — first-set target. Immutable after first save; what the
    /// "extended to X instead of Y" warning anchors to.
    pub original_deadline: String,
    pub status: MilestoneStatus,
    pub priority: Priority,
    #[ts(type = "number")]
    pub order: i64,
    /// Ordered membership; todos retain their own ordering for display.
    #[serde(default)]
    pub todo_ids: Vec<TodoId>,
    #[serde(default)]
    pub extensions: Vec<ExtensionEvent>,
    /// Running success points accrued from member todos.
    #[serde(default)]
    #[ts(type = "number")]
    pub success_points: f64,
    /// Running failing points accrued from late/missed todos and from
    /// soften-extensions inside the akrasia horizon.
    #[serde(default)]
    #[ts(type = "number")]
    pub failing_points: f64,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
}

/// A recurring task definition. Cadence is RFC 5545 RRULE; instances
/// are materialised to `RoutineInstance` records.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct Routine {
    pub id: RoutineId,
    /// Optional — `None` means a global routine spanning all projects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// RFC 5545 RRULE without the `RRULE:` prefix.
    pub rrule: String,
    /// ISO-8601 date the recurrence anchors on.
    pub start_date: String,
    pub goal: Goal,
    pub priority: Priority,
    /// Estimated minutes per instance (for workload bar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<i64>,
    pub paused: bool,
    /// ISO-8601 timestamp the pause began.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_from: Option<String>,
    #[serde(default)]
    #[ts(type = "number")]
    pub success_points: f64,
    #[serde(default)]
    #[ts(type = "number")]
    pub failing_points: f64,
    #[serde(default)]
    pub extensions: Vec<ExtensionEvent>,
    pub created_at: String,
}

/// One materialised occurrence of a Routine. Generated lazily by the
/// engine up to `MAX_HORIZON_DAYS` ahead.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct RoutineInstance {
    pub id: RoutineInstanceId,
    pub routine_id: RoutineId,
    /// ISO-8601 date the instance is due (strict / calendar-anchored).
    pub scheduled_for: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
    /// Days added to the routine's projected goal-completion date when
    /// this instance was missed.
    #[serde(default)]
    #[ts(type = "number")]
    pub extension_contribution: i64,
    #[serde(default)]
    #[ts(type = "number")]
    pub failing_points: f64,
    #[serde(default)]
    #[ts(type = "number")]
    pub success_points: f64,
}

/// Which projects appear in the cross-project timeline view. Curated by
/// the user via `TimelineProjectPicker`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct TimelineConfig {
    /// User-pinned projects. Order matters (renders top-to-bottom).
    #[serde(default)]
    pub pinned_project_ids: Vec<ProjectId>,
    /// Default visible range — `"week"` or `"month"`.
    #[serde(default = "default_visible_range")]
    pub visible_range: String,
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            pinned_project_ids: Vec::new(),
            visible_range: default_visible_range(),
        }
    }
}

fn default_visible_range() -> String {
    "month".to_string()
}

/// One snapshot of project / lifetime success rate. The score engine
/// appends a daily snapshot so charts and rolling-30d numbers exist.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ScoreSnapshot {
    /// ISO-8601 date the snapshot represents.
    pub date: String,
    #[ts(type = "number")]
    pub success_points: f64,
    #[ts(type = "number")]
    pub failing_points: f64,
    /// success_points / (success_points + failing_points), or 1.0 if both 0.
    #[ts(type = "number")]
    pub success_rate: f64,
}

/// Global planner state — pause flag, last notification bookkeeping,
/// rolling score snapshots.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct PlannerState {
    /// Pause-all is the vacation safety valve. While true, no new
    /// failing points accrue across any routine or milestone.
    #[serde(default)]
    pub paused_all: bool,
    /// ISO-8601 — when paused_all flipped on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_from: Option<String>,
    /// ISO-8601 — last time the headline notification fired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_notification_at: Option<String>,
    /// `YYYY-MM-DD` (local) — last day the session-start notification
    /// fired. Compared against today to enforce "once per day".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_session_date: Option<String>,
    #[serde(default)]
    pub score_snapshots: Vec<ScoreSnapshot>,
}
