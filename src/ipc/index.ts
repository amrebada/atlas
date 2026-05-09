import { invoke } from "@tauri-apps/api/core";
import type {
  Project,
  Collection,
  WatchRoot,
  Script,
  Todo,
  FileNode,
  Note,
  Pane,
  PaneId,
  PaneKind,
  Session,
  Settings,
  Template,
  EditorEntry,
  Milestone,
  MilestoneStatus,
  ExtensionReason,
  Priority,
  Routine,
  RoutineInstance,
  PlannerToday,
  PlannerState,
  SessionStartResult,
  ScoreSummary,
  ExtensionEvent,
  TimelineConfig,
  TimelineData,
} from "../types";
import type { PaneLayout } from "../features/terminal/layout";

// Discriminated-union shape mirroring Rust's `PaletteItem` in
export type PaletteItem =
  | { kind: "project"; project: Project; score: number }
  | { kind: "recent"; project: Project }
  | {
      kind: "note";
      projectId: string;
      noteId: string;
      title: string;
      snippet: string;
      score: number;
    }
  | {
      kind: "milestone";
      projectId: string;
      projectName: string;
      milestoneId: string;
      title: string;
      deadline: string;
      priority: import("../types").Priority;
      status: import("../types").MilestoneStatus;
      score: number;
    }
  | {
      kind: "routine";
      routineId: string;
      projectId: string | null;
      projectName: string | null;
      title: string;
      rrule: string;
      priority: import("../types").Priority;
      score: number;
    }
  | { kind: "action"; id: string; label: string; hint: string; keys: string[] };

// Atlas - typed IPC wrappers.

// ---------- handshake ----------

export const appVersion = () => invoke<string>("app_version");

// ---------- projects ----------

export const listProjects = () => invoke<Project[]>("projects_list");

export const getProject = (id: string) =>
  invoke<Project | null>("projects_get", { id });

export const searchProjects = (query: string, filters?: unknown) =>
  invoke<Project[]>("projects_search", { query, filters: filters ?? null });

export const seedFixtures = () => invoke<number>("projects_seed_fixtures");

/** Walk `root` (up to `depth` levels) for `.git` dirs and upsert each. */
export const discoverProjects = (root: string, depth = 3) =>
  invoke<string[]>("projects_discover", { root, depth });

export const pinProject = (id: string, pinned: boolean) =>
  invoke<void>("projects_pin", { id, pinned });
export const projectsPin = pinProject;

export const archiveProject = (id: string, archived: boolean) =>
  invoke<void>("projects_archive", { id, archived });
export const projectsArchive = archiveProject;

export const renameProject = (id: string, name: string) =>
  invoke<void>("projects_rename", { id, name });

export const setProjectTags = (id: string, tags: string[]) =>
  invoke<void>("projects_set_tags", { id, tags });

// ---------- watchers ----------

export const listWatchers = () => invoke<WatchRoot[]>("watchers_list");
export const watchersList = listWatchers;

export const addWatcher = (path: string, depth = 3) =>
  invoke<void>("watchers_add", { path, depth });

/** Stop watching `path`. When `cascade` is true, also remove every indexed
 *  project under that path from Atlas's database (files on disk are left
 *  alone). Resolves to the number of projects that were unindexed. */
export const removeWatcher = (path: string, cascade = false) =>
  invoke<number>("watchers_remove", { path, cascade });

// ---------- tags ----------

export const listTags = () => invoke<string[]>("tags_list");

export const addTag = (projectId: string, tag: string) =>
  invoke<void>("tags_add", { projectId, tag });

export const removeTag = (projectId: string, tag: string) =>
  invoke<void>("tags_remove", { projectId, tag });

// ---------- collections ----------

export const listCollections = () =>
  invoke<Collection[]>("collections_list");

export const upsertCollection = (collection: Collection) =>
  invoke<void>("collections_upsert", { collection });

export const removeCollection = (id: string) =>
  invoke<void>("collections_remove", { id });

// ---------- collections ----------

// collections CRUD + membership wrappers. D9 is landing the Rust

export const collectionsCreate = (label: string, color?: string) =>
  invoke<Collection>("collections_create", { label, color });

export const collectionsRename = (id: string, label: string) =>
  invoke<void>("collections_rename", { id, label });

export const collectionsUpdateColor = (id: string, color: string) =>
  invoke<void>("collections_update_color", { id, color });

export const collectionsDelete = (id: string) =>
  invoke<void>("collections_delete", { id });

export const collectionsReorder = (orderedIds: string[]) =>
  invoke<void>("collections_reorder", { orderedIds });

export const collectionsAddProject = (projectId: string, collectionId: string) =>
  invoke<void>("collections_add_project", { projectId, collectionId });

export const collectionsRemoveProject = (
  projectId: string,
  collectionId: string,
) => invoke<void>("collections_remove_project", { projectId, collectionId });

export const collectionsProjects = (collectionId: string) =>
  invoke<Project[]>("collections_projects", { collectionId });


// `.atlas/scripts.json` round-trip wrappers. Names match P3/D3's planned

export const listScripts = (projectId: string) =>
  invoke<Script[]>("scripts_list", { projectId });

export const upsertScript = (projectId: string, script: Script) =>
  invoke<void>("scripts_upsert", { projectId, script });

export const deleteScript = (projectId: string, scriptId: string) =>
  invoke<void>("scripts_delete", { projectId, scriptId });


export const listTodos = (projectId: string) =>
  invoke<Todo[]>("todos_list", { projectId });

export const upsertTodo = (projectId: string, todo: Todo) =>
  invoke<void>("todos_upsert", { projectId, todo });

export const deleteTodo = (projectId: string, todoId: string) =>
  invoke<void>("todos_delete", { projectId, todoId });

export const toggleTodo = (projectId: string, todoId: string) =>
  invoke<void>("todos_toggle", { projectId, todoId });


// `.atlas/milestones.json` round-trip wrappers. Score recompute runs
// server-side on every todo write, so the UI can read milestones and
// trust the cached `successPoints` / `failingPoints`.

export const listMilestones = (projectId: string) =>
  invoke<Milestone[]>("milestones_list", { projectId });

export const createMilestone = (projectId: string, milestone: Milestone) =>
  invoke<Milestone>("milestones_create", { projectId, milestone });

export const updateMilestone = (
  projectId: string,
  milestoneId: string,
  patch: {
    title?: string;
    description?: string;
    priority?: Priority;
    order?: number;
  },
) =>
  invoke<Milestone>("milestones_update", {
    projectId,
    milestoneId,
    title: patch.title,
    description: patch.description,
    priority: patch.priority,
    order: patch.order,
  });

export const extendMilestone = (
  projectId: string,
  milestoneId: string,
  newDeadline: string,
  reason: ExtensionReason,
  note?: string,
) =>
  invoke<Milestone>("milestones_extend", {
    projectId,
    milestoneId,
    newDeadline,
    reason,
    note,
  });

export const setMilestoneStatus = (
  projectId: string,
  milestoneId: string,
  status: MilestoneStatus,
) =>
  invoke<Milestone>("milestones_set_status", { projectId, milestoneId, status });

export const deleteMilestone = (projectId: string, milestoneId: string) =>
  invoke<boolean>("milestones_delete", { projectId, milestoneId });


// `app_data_dir/atlas/routines.json` + `routine_instances.json`. Lazy
// materialisation runs server-side on every list/instances query, so
// callers don't need to ping `routinesMaterialize` separately.

export const listRoutines = (projectId: string | null) =>
  invoke<Routine[]>("routines_list", { projectId });

export const createRoutine = (routine: Routine) =>
  invoke<Routine>("routines_create", { routine });

export const updateRoutine = (
  routineId: string,
  patch: {
    title?: string;
    description?: string;
    rrule?: string;
    priority?: Priority;
    estimate?: number;
  },
) =>
  invoke<Routine>("routines_update", {
    routineId,
    title: patch.title,
    description: patch.description,
    rrule: patch.rrule,
    priority: patch.priority,
    estimate: patch.estimate,
  });

export const deleteRoutine = (routineId: string) =>
  invoke<boolean>("routines_delete", { routineId });

export const pauseRoutine = (routineId: string, paused: boolean) =>
  invoke<Routine>("routines_pause", { routineId, paused });

export const listRoutineInstances = (
  routineId: string,
  from: string,
  to: string,
) => invoke<RoutineInstance[]>("routines_instances", { routineId, from, to });

export const completeRoutineInstance = (
  instanceId: string,
  completedAt?: string,
) =>
  invoke<RoutineInstance>("routines_complete_instance", {
    instanceId,
    completedAt,
  });

export const skipRoutineInstance = (instanceId: string) =>
  invoke<RoutineInstance>("routines_skip_instance", { instanceId });

export const projectedCompletion = (routineId: string) =>
  invoke<string | null>("routines_projected_completion", { routineId });


// Planner — Today view, session-start notification, pause-all.

export const plannerToday = () => invoke<PlannerToday>("planner_today");

export const plannerSessionStart = () =>
  invoke<SessionStartResult>("planner_session_start");

export const plannerPauseAll = (paused: boolean) =>
  invoke<PlannerState>("planner_pause_all", { paused });

export const plannerScoreSummary = (
  projectId: string | null,
  rangeDays: number,
) =>
  invoke<ScoreSummary>("planner_score_summary", { projectId, rangeDays });

export const plannerExtensionLog = (filter: {
  projectId?: string;
  milestoneId?: string;
  routineId?: string;
}) =>
  invoke<ExtensionEvent[]>("planner_extension_log", {
    projectId: filter.projectId,
    milestoneId: filter.milestoneId,
    routineId: filter.routineId,
  });


// Timeline — cross-project view for user-pinned projects only.

export const timelineConfigGet = () =>
  invoke<TimelineConfig>("timeline_config_get");

export const timelinePinProject = (projectId: string) =>
  invoke<TimelineConfig>("timeline_pin_project", { projectId });

export const timelineUnpinProject = (projectId: string) =>
  invoke<TimelineConfig>("timeline_unpin_project", { projectId });

export const timelineSetRange = (visibleRange: "week" | "month") =>
  invoke<TimelineConfig>("timeline_set_range", { visibleRange });

export const timelineQuery = (rangeOverride?: "week" | "month") =>
  invoke<TimelineData>("timeline_query", { rangeOverride });


// ICS export. Files land in `<app_data>/atlas/ics/`. Drag them into
// Apple Calendar / Google Calendar / Outlook from Finder.

/** Regenerate every per-project file plus `today.ics`. Returns the
 *  absolute directory path the UI should reveal. */
export const icsExportAll = () => invoke<string>("ics_export_all");

/** Regenerate one project's file. Returns the absolute file path. */
export const icsExportProject = (projectId: string) =>
  invoke<string>("ics_export_project", { projectId });

/** Reveal the ICS directory in Finder (or the platform file manager). */
export const icsRevealDir = () => invoke<void>("ics_reveal_dir");


// Returns the working-tree file list for a project. `changedOnly` keeps
export const listFiles = (projectId: string, changedOnly: boolean) =>
  invoke<FileNode[]>("files_list", { projectId, changedOnly });

// Per-file diff: HEAD vs working tree as raw strings. Consumed by the
// Inspector → Files diff modal, which feeds the strings into a JS diff
// viewer (split / inline view).
export interface FileDiff {
  path: string;
  status: string;
  oldContent: string;
  newContent: string;
  isBinary: boolean;
}

export const filesDiff = (projectId: string, path: string) =>
  invoke<FileDiff>("files_diff", { projectId, path });


// `.atlas/notes/<id>.json` round-trip wrappers. Names match D4's planned

export const listNotes = (projectId: string) =>
  invoke<Note[]>("notes_list", { projectId });

export const getNote = (projectId: string, noteId: string) =>
  invoke<Note | null>("notes_get", { projectId, noteId });

export const upsertNote = (projectId: string, note: Note) =>
  invoke<void>("notes_upsert", { projectId, note });

export const deleteNote = (projectId: string, noteId: string) =>
  invoke<void>("notes_delete", { projectId, noteId });

export const pinNote = (projectId: string, noteId: string, pinned: boolean) =>
  invoke<void>("notes_pin", { projectId, noteId, pinned });

export const searchNotes = (projectId: string, query: string) =>
  invoke<Note[]>("notes_search", { projectId, query });


// Shape returned by `sessions_resume_info` - the command + argv + cwd a
export interface ResumeInfo {
  sessionId: string;
  provider: string;
  command: string;
  args: string[];
  cwd: string;
}

export const listSessions = (projectId: string, provider?: string) =>
  invoke<Session[]>("sessions_list", {
    projectId,
    provider: provider ?? null,
  });

export const sessionResumeInfo = (sessionId: string, provider?: string) =>
  invoke<ResumeInfo>("sessions_resume_info", {
    sessionId,
    provider: provider ?? null,
  });

export interface ProviderInfo {
  id: string;
  label: string;
  binaryName: string;
  available: boolean;
  enabled: boolean;
  isDefault: boolean;
}

export interface ProviderInvocation {
  provider: string;
  command: string;
  args: string[];
  cwd: string;
}

export const listProviders = () => invoke<ProviderInfo[]>("providers_list");

export const providerNewInvocation = (providerId: string, projectId: string) =>
  invoke<ProviderInvocation>("providers_new_invocation", {
    providerId,
    projectId,
  });


// Tauri commands. The wrappers below exist so U5 can compile and

export const getSettings = () => invoke<Settings>("settings_get");

export const setSettings = (
  patch: Partial<Settings> | Record<string, unknown>,
) => invoke<Settings>("settings_set", { patch });


export const listTemplates = () => invoke<Template[]>("templates_list");

export const upsertTemplate = (template: Template) =>
  invoke<void>("templates_upsert", { template });

export const removeTemplate = (id: string) =>
  invoke<void>("templates_remove", { id });


export const paletteQuery = (query: string, limit?: number) =>
  invoke<PaletteItem[]>("palette_query", { query, limit: limit ?? null });

export const pushRecent = (projectId: string) =>
  invoke<void>("recents_push", { projectId });

export const listRecents = (limit?: number) =>
  invoke<Project[]>("recents_list", { limit: limit ?? null });


export const detectEditors = () => invoke<EditorEntry[]>("editors_detect");

export const openInEditor = (projectId: string, editorId?: string) =>
  invoke<void>("editors_open_project", {
    projectId,
    editorId: editorId ?? null,
  });

export const revealInFinder = (projectId: string) =>
  invoke<void>("editors_reveal", { projectId });


// Spawn one-or-more scripts in the project's cwd. Returns pane ids in the
export const scriptsRun = (projectId: string, scriptIds: string[]) =>
  invoke<string[]>("scripts_run", { projectId, scriptIds });

/** Single script invocation with user-edited env overrides. */
export interface ScriptInvocation {
  scriptId: string;
  env: Array<[string, string]>;
}

/** Spawn scripts carrying per-invocation env overrides (from the
 *  run-env modal). Falls back identical to {@link scriptsRun} when
 *  `env` is empty. */
export const scriptsRunWithEnv = (
  projectId: string,
  invocations: ScriptInvocation[],
) => invoke<string[]>("scripts_run_with_env", { projectId, invocations });


/** Request shape for `terminal_open`. */
export interface TerminalOpenRequest {
  kind: PaneKind;
  cwd: string;
  command?: string;
  args?: string[];
  /** Extra env pairs merged on top of the inherited parent env. */
  env?: Array<[string, string]>;
}

export const terminalOpen = (req: TerminalOpenRequest) =>
  invoke<string>("terminal_open", { req });

export const terminalWrite = (paneId: PaneId, data: string) =>
  invoke<void>("terminal_write", { paneId, data });

export const terminalResize = (paneId: PaneId, cols: number, rows: number) =>
  invoke<void>("terminal_resize", { paneId, cols, rows });

export const terminalClose = (paneId: PaneId) =>
  invoke<void>("terminal_close", { paneId });

export const terminalList = () => invoke<Pane[]>("terminal_list");


// One row of `disk_scan`. `pct` is 0..1 of total; `cleanable` marks the
export interface DiskEntry {
  /** Path relative to the project root. */
  path: string;
  /** Human label shown in the row (defaults to `path`). */
  label: string;
  bytes: number;
  /** Pretty-printed size (e.g. "412 MB"). */
  size: string;
  /** 0..1 share of the total. */
  pct: number;
  /** Tint used for the stacked bar + row swatch. */
  color: string;
  /** Whether this row is safe to delete / moves to Trash. */
  cleanable: boolean;
}

export interface DiskScanResult {
  totalBytes: number;
  totalSize: string;
  entries: DiskEntry[];
}

export const diskScan = (projectId: string) =>
  invoke<DiskScanResult>("disk_scan", { projectId });

export const diskClean = (projectId: string, relativePath: string) =>
  invoke<void>("disk_clean", { projectId, relativePath });


export const paneLayoutGet = (projectId: string) =>
  invoke<PaneLayout | null>("pane_layout_get", { projectId });

export const paneLayoutSave = (projectId: string, layout: PaneLayout) =>
  invoke<void>("pane_layout_save", { projectId, layout });


// Params for `templates_create_project`. Mirrors the prototype New-Project
export interface CreateProjectParams {
  name: string;
  /** Absolute parent directory; project folder is created inside it. */
  parent: string;
  templateId: string;
  initGit: boolean;
  createEnv: boolean;
  openInEditor: string | null;
}

/** Returns the new project's id. */
export const createProjectFromTemplate = (params: CreateProjectParams) =>
  invoke<string>("templates_create_project", { params });


// wrappers. Bodies land in the P7/D7 Rust lanes; U7 depends on the

/** One branch entry returned by `git_branch_list`. */
export interface BranchInfo {
  name: string;
  isHead: boolean;
  isRemote: boolean;
}

// Read-only preview of a branch checkout. The v1 Rust implementation does
export interface GitCheckoutPreview {
  branch: string;
  filesWouldChange: number;
  isDirty: boolean;
  warning: string | null;
}

export const gitBranchList = (projectId: string) =>
  invoke<BranchInfo[]>("git_branch_list", { projectId });

export const gitCheckout = (projectId: string, branch: string) =>
  invoke<GitCheckoutPreview>("git_checkout", { projectId, branch });

// Shell-mediated mutating git action (commit / stash / push). Mirrors
export interface GitActionResult {
  ok: boolean;
  stdout: string;
  stderr: string;
}

export const gitCommit = (projectId: string, message: string) =>
  invoke<GitActionResult>("git_commit", { projectId, message });

export const gitStash = (projectId: string, message: string) =>
  invoke<GitActionResult>("git_stash", { projectId, message });

export const gitPush = (projectId: string) =>
  invoke<GitActionResult>("git_push", { projectId });

export const projectsMoveToTrash = (id: string) =>
  invoke<void>("projects_move_to_trash", { id });

// ---------- D7 ----------

// Persist the new pinned order to Rust. Called after `onDragEnd` in the
export const projectsReorderPinned = (orderedIds: string[]) =>
  invoke<void>("projects_reorder_pinned", { orderedIds });

// naming alias over the iter-5 `listRecents` wrapper. Kept so U7 +
export const recentsList = (limit = 20) =>
  invoke<Project[]>("recents_list", { limit });

export const recentsPush = (projectId: string) =>
  invoke<void>("recents_push", { projectId });

// ---------- system ----------

// Snapshot of the host's filesystem capacity, as returned by
export interface SystemDiskUsage {
  totalBytes: number;
  freeBytes: number;
  usedBytes: number;
  total: string;
  free: string;
  used: string;
  mountPoint: string;
  pctUsed: number;
}

export const systemDiskUsage = () =>
  invoke<SystemDiskUsage>("system_disk_usage");

// ── MCP remote-control feature ─────────────────────────────────────────
export const resolveMcpApproval = (id: string, approve: boolean) =>
  invoke<void>("mcp_approval_resolve", { id, approve });

import type { PairingInfo, PairEnvelope } from "../types";
export const agentPairingInfo = () =>
  invoke<PairingInfo>("agent_pairing_info");

/** Build a freshly-signed pair envelope for QR display. The envelope
 *  is good for ~60s before the relay rejects it for staleness — call
 *  this each time the QR modal opens (and ideally on a refresh tick). */
export const agentPairEnvelope = () =>
  invoke<PairEnvelope>("agent_pair_envelope");
