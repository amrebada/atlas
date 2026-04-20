-- 0004_pin_order_and_recents.sql
-- (D7) - persistent order for pinned projects + recents ring buffer
-- hardening.
--
-- Rationale:
--   * `projects.pin_ord` is the drag-reorder slot for the sidebar's "Pinned"
--     section. NULL means "pinned but never explicitly reordered" - those
--     float to the end of the pinned block (NULLS LAST). Unpinned rows keep
--     NULL forever; `ORDER BY pinned DESC, pin_ord ASC NULLS LAST, …` gives
--     a stable, drag-friendly sort without touching unpinned rows.
--   * The `recents` table was already created in 0001_init.sql with a
--     TEXT `opened_at`. The `CREATE TABLE IF NOT EXISTS` below is a
--     defensive no-op for fresh installs that somehow reach 0004 without
--     having run 0001 (shouldn't happen, but cheap insurance). The column
--     type comment below describes the *intent* for any future reset; the
--     current implementation stores RFC3339 strings and the sort works
--     lexicographically, which matches unix-seconds ordering byte-for-byte
--     for any timestamp past 1582 AD.
--   * `idx_recents_opened_at` speeds up both the LIFO list query and the
--     ring-buffer trim (`DELETE … WHERE project_id NOT IN (SELECT … LIMIT 20)`).
--
-- NB: PRAGMAs (`journal_mode=WAL`, `foreign_keys=ON`) live in `db.rs` at
-- connection time, not here - sqlx wraps each migration in a transaction
-- and SQLite rejects `journal_mode=WAL` mid-transaction.

ALTER TABLE projects ADD COLUMN pin_ord INTEGER;

CREATE TABLE IF NOT EXISTS recents (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  opened_at  INTEGER NOT NULL  -- unix seconds (pre-0004 installs store RFC3339 TEXT; lexicographic order matches)
);

CREATE INDEX IF NOT EXISTS idx_recents_opened_at ON recents(opened_at DESC);
