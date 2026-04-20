-- 0001_init.sql
-- Initial schema for Atlas. Mirrors verbatim.
--
-- Storage model: JSON-first (per-project `.atlas/*`) with SQLite as a
-- rebuildable search index. This migration establishes the index shape;
-- all queryable fields from `Project` plus FTS5 virtual tables for the
-- ⌘K palette, tag filters, and note/todo search.

-- NOTE: journal_mode=WAL and foreign_keys=ON are applied at connection
-- time via `SqliteConnectOptions` in `src/storage/db.rs`, because sqlx
-- runs every migration inside a transaction and SQLite rejects
-- `PRAGMA journal_mode = WAL` inside a transaction.

CREATE TABLE projects (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  path          TEXT NOT NULL UNIQUE,
  language      TEXT,
  color         TEXT,
  branch        TEXT,
  dirty         INTEGER DEFAULT 0,
  ahead         INTEGER DEFAULT 0,
  behind        INTEGER DEFAULT 0,
  loc           INTEGER DEFAULT 0,
  size_bytes    INTEGER DEFAULT 0,
  last_opened   TEXT,
  pinned        INTEGER DEFAULT 0,
  archived      INTEGER DEFAULT 0,
  todos_count   INTEGER DEFAULT 0,
  notes_count   INTEGER DEFAULT 0,
  time_tracked  TEXT,
  updated_at    TEXT NOT NULL
);

CREATE TABLE tags (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  tag        TEXT NOT NULL,
  PRIMARY KEY (project_id, tag)
);

CREATE TABLE collections (
  id       TEXT PRIMARY KEY,
  label    TEXT NOT NULL,
  dot      TEXT,
  ord      INTEGER NOT NULL
);

CREATE TABLE collection_members (
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, project_id)
);

CREATE VIRTUAL TABLE projects_fts USING fts5(
  name, path, tags, content='projects', content_rowid='rowid'
);

CREATE VIRTUAL TABLE notes_fts USING fts5(
  project_id UNINDEXED, note_id UNINDEXED, title, body_plain
);

CREATE VIRTUAL TABLE todos_fts USING fts5(
  project_id UNINDEXED, todo_id UNINDEXED, text
);

CREATE TABLE recents (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  opened_at  TEXT NOT NULL
);

CREATE INDEX idx_projects_pinned ON projects(pinned) WHERE pinned = 1;
CREATE INDEX idx_projects_archived ON projects(archived);
CREATE INDEX idx_projects_lastopened ON projects(last_opened DESC);
