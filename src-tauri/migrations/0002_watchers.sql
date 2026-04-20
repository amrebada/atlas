-- 0002_watchers.sql
-- (D2) - watcher persistence + discovery provenance columns.
--
-- Rationale:
--   * `watchers` is the authoritative list of configured watch roots; P2's
--     boot-up reconstructs notify-rs subscriptions from this table.
--   * `projects.source` distinguishes seeded fixtures from real discoveries
--     so the `seed_fixtures` path can stay idempotent and non-destructive
--     once a watcher is configured (the seeds never mix with real repos).
--   * `projects.discovered_at` lets the UI show "added on" in onboarding
--     and gives us a tiebreaker when the same repo is discovered twice.
--
-- NB: PRAGMAs are set per-connection in `db.rs`, not here - sqlx runs each
-- migration in a transaction and SQLite rejects `journal_mode=WAL` inside
-- a transaction (same constraint as 0001_init.sql).

CREATE TABLE watchers (
  path       TEXT PRIMARY KEY,
  depth      INTEGER NOT NULL DEFAULT 3,
  added_at   TEXT NOT NULL
);

CREATE INDEX idx_watchers_path ON watchers(path);

-- Provenance on projects. Existing rows get `source='seed'` by the DEFAULT
-- clause; discovery always inserts with `source='discovery'`, manual
-- additions (+ New Project modal) use `source='manual'`.
ALTER TABLE projects ADD COLUMN discovered_at TEXT;
ALTER TABLE projects ADD COLUMN source TEXT NOT NULL DEFAULT 'seed';
