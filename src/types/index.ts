// src/types/index.ts - canonical Atlas domain types.

// ---------- Branded ID types ----------

/** Stable project id - slug derived from hashing the absolute path. */
export type ProjectId = string;
/** Collection id - slug or uuid. */
export type CollectionId = string;
/** Todo id - uuid. */
export type TodoId = string;
/** Note id - uuid. */
export type NoteId = string;
/** Script id - uuid or deterministic key (e.g. "dev"). */
export type ScriptId = string;
/** Claude-Code session id - uuid, sourced from ~/.claude. */
export type SessionId = string;
/** Template id - slug ("node-ts") or uuid for user templates. */
export type TemplateId = string;
/** Terminal pane id - uuid assigned by the Rust PTY manager. */
export type PaneId = string;

// ---------- Enums ----------

export type Lang =
  | 'TypeScript'
  | 'JavaScript'
  | 'Rust'
  | 'Go'
  | 'Python'
  | 'Swift'
  | 'Kotlin'
  | 'Ruby'
  | 'Java'
  | 'C'
  | 'C++'
  | 'Other';

export type PaneKind = 'shell' | 'script' | 'claude-session';
export type PaneStatus = 'idle' | 'running' | 'active' | 'error';

export type SessionStatus = 'active' | 'idle' | 'archived';
export type FileKind = 'dir' | 'file';
export type FileStatus = 'M' | '+' | '-' | null;
export type ScriptGroup = 'run' | 'build' | 'check' | 'util';
export type Theme = 'dark' | 'light' | 'system';
export type GitPollInterval = '10s' | '30s' | '1m' | 'off';
export type CloneDepth = number | 'full';

// ---------- Core entities ----------

export interface Project {
  id: ProjectId;
  name: string;
  /** Absolute path on disk. */
  path: string;
  language: Lang;
  /** Hex color; defaults derived from language. */
  color: string;
  branch: string;
  /** Uncommitted file count. */
  dirty: number;
  ahead: number;
  behind: number;
  // Last-commit author name. `null` until the git refresh populates it
  author?: string | null;
  /** Cached LOC (periodic tokei scan). */
  loc: number;
  /** Pretty-printed source size (e.g. "412 MB"). Respects .gitignore. */
  size: string;
  sizeBytes: number;
  /** Pretty-printed on-disk size (e.g. "16.4 GB"). Full tree, no .gitignore. */
  diskSize: string;
  /** Full on-disk footprint in bytes. Always >= sizeBytes. */
  diskBytes: number;
  /** ISO-8601 timestamp, or null if never opened. */
  lastOpened: string | null;
  pinned: boolean;
  tags: string[];
  todosCount: number;
  notesCount: number;
  /** Tracked time, pretty-printed (e.g. "12h 08m"). */
  time: string;
  archived: boolean;
  collectionIds: CollectionId[];
}

export interface Collection {
  id: CollectionId;
  label: string;
  /** Hex or OKLCH color for the sidebar dot. */
  dot: string;
  order: number;
}

export interface Todo {
  id: TodoId;
  done: boolean;
  text: string;
  /** Free-form due label ("today", "fri", ISO-8601). Optional. */
  due?: string;
  /** ISO-8601. */
  createdAt: string;
}

export interface Note {
  id: NoteId;
  title: string;
  /** Tiptap-serialized HTML. */
  body: string;
  pinned: boolean;
  /** ISO-8601. */
  createdAt: string;
  /** ISO-8601. */
  updatedAt: string;
}

export interface ScriptEnvVar {
  key: string;
  default: string;
}

export interface Script {
  id: ScriptId;
  name: string;
  cmd: string;
  desc?: string;
  group: ScriptGroup;
  default?: boolean;
  icon?: string;
  /** Env vars the user defined alongside this script. Defaults are
   * applied automatically on plain "run"; the run-env modal lets the
   * user override them per-invocation before launch. */
  env_defaults?: ScriptEnvVar[];
}

export interface Session {
  id: SessionId;
  projectPath: string;
  title: string;
  /** ISO-8601. */
  when: string;
  turns: number;
  /** Pretty-printed ("47m"). */
  duration: string;
  status: SessionStatus;
  /** Snippet of the last user/assistant line. */
  last: string;
  model: string;
  branch?: string;
}

export interface FileNode {
  depth: number;
  name: string;
  path: string;
  kind: FileKind;
  status?: FileStatus;
  /** e.g. "+42 -18". */
  delta?: string;
}

export interface Template {
  id: TemplateId;
  label: string;
  color: string;
  hint: string;
  path: string;
  builtin: boolean;
}

export interface Pane {
  id: PaneId;
  kind: PaneKind;
  title: string;
  status: PaneStatus;
  cwd: string;
  branch?: string;
  scriptId?: ScriptId;
  sessionId?: SessionId;
  // Owning project id (optional - legacy panes without one are treated as
  projectId?: ProjectId;
  // Human-readable project label (name, not path). Stored on the pane
  projectLabel?: string;
  // Command + args the pane was spawned with. Set for `script` panes
  command?: string;
  args?: string[];
}


// Subset of `Pane` that can be serialized between runs. Live PTY
export interface PaneSnapshot {
  id: string;
  // One of `PaneKind` values; stored as a string so unknown kinds on
  kind: string;
  title: string;
  cwd: string;
  scriptId?: ScriptId;
  sessionId?: SessionId;
}

// Last-known layout for a project's terminal strip. Persisted at
export interface PaneLayout {
  // One of `'tabs' | 'split-v' | 'split-h' | 'grid'`; kept as string
  mode: string;
  panes: PaneSnapshot[];
  activePaneId?: string;
}

// Body for `templates.create_project`. Fields are all explicit so the
export interface CreateProjectParams {
  name: string;
  // Absolute path to the parent directory. Must be inside $HOME (or a
  parent: string;
  templateId: TemplateId;
  initGit: boolean;
  createEnv: boolean;
  /** Optional editor id; the caller is responsible for launching. */
  openInEditor?: string;
}

// ---------- Settings ----------

export interface EditorEntry {
  id: string;
  name: string;
  cmd: string;
  present: boolean;
}

export interface WatchRoot {
  path: string;
  depth: number;
  repoCount: number;
}


// Result of `projects.discover(root, depth?)` - returned so the UI can
export interface DiscoveryResult {
  root: string;
  newProjectIds: ProjectId[];
  totalRepos: number;
}

// Partial `Project` patch used by streaming `project:updated` events and
export type ProjectPatch = Partial<Project> & { id: ProjectId };

// IPC-facing shape for `settings.watchers.list` / `.add`. Mirrors the
export interface WatchRootDto {
  path: string;
  depth: number;
  repoCount: number;
}

export interface GeneralSettings {
  launchAtLogin: boolean;
  menuBarAgent: boolean;
  defaultProjectLocation: string;
  theme: Theme;
}

export interface EditorsSettings {
  detected: EditorEntry[];
  defaultId: string | null;
}

export interface GitSettings {
  pollInterval: GitPollInterval;
  showAuthor: boolean;
  defaultCloneDepth: CloneDepth;
  sshKey: string;
}

export interface AdvancedSettings {
  useSpotlight: boolean;
  crashReports: boolean;
  shell: string;
}

export interface Settings {
  general: GeneralSettings;
  editors: EditorsSettings;
  git: GitSettings;
  watchers: WatchRoot[];
  templates: Template[];
  shortcuts: Record<string, string>;
  advanced: AdvancedSettings;
}


// Discriminated union returned by `palette.query(query, limit?)`.
export type PaletteItem =
  | { kind: 'project'; project: Project; score: number }
  | { kind: 'recent'; project: Project }
  | {
      kind: 'note';
      projectId: ProjectId;
      noteId: NoteId;
      title: string;
      snippet: string;
      score: number;
    }
  | { kind: 'action'; id: string; label: string; hint: string; keys: string[] };
