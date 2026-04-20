# Atlas SQLite migrations

Forward-only, file-numbered schema evolution for `atlas.db`.

`atlas.db` is a rebuildable index. Truth lives in per-project
`.atlas/*.json`. Schema changes here never need a data
backfill from users; worst case the index is dropped and rebuilt from
disk by the sync worker (`storage/sync.rs`) or the repair IPC
(`projects.repair`).

## Numbering convention

`NNNN_short_name.sql` — four-digit zero-padded prefix, underscore, terse
snake_case description, `.sql` extension. Examples:

```
0001_init.sql
0002_watchers.sql
0004_pin_order_and_recents.sql
```

sqlx reads the numeric prefix and applies files in ascending order. Gaps
(e.g. the missing `0003`) are harmless — sqlx records whichever versions
ran, not a contiguous range. Prefer the next unused integer so future
readers can scan the directory without puzzling over skipped slots.

## DDL-only — no `PRAGMA` inside migrations

sqlx wraps every migration in a single transaction. SQLite rejects
`PRAGMA journal_mode = WAL` (and several other pragmas) mid-transaction
with `SQLITE_ERROR`. The codebase was bitten by this on the first
migration: the WAL pragma lived in `0001_init.sql` and caused every
`sqlx migrate run` to fail on a fresh DB.

Resolution: **pragmas are set per-connection in `storage/db.rs`** via
`SqliteConnectOptions` (`journal_mode`, `synchronous`, `foreign_keys`).
Migrations touch schema only:

- `CREATE TABLE` / `ALTER TABLE` / `DROP TABLE`
- `CREATE INDEX` / `DROP INDEX`
- `CREATE VIRTUAL TABLE` (FTS5)
- `INSERT` / `UPDATE` for mandatory seed rows (rare)

If a future migration really needs a pragma, set it on the connection
and add a code review note — don't put it in the `.sql`.

## No down migrations

sqlx supports paired `*.up.sql` / `*.down.sql` files. Atlas deliberately
does not use them:

- The index is rebuildable. Rolling back the schema solves no user
  problem — reinstalling the previous binary and letting it reindex
  from the per-project JSON is already the answer.
- Down migrations double the surface area and historically go stale
  (nobody runs them, so they don't get tested).

If you need to undo a previous change, **write a new forward
migration** that reverses it. For example, to drop a column added in
`0005`, add `0006_revert_foo.sql` with `ALTER TABLE … DROP COLUMN …`.

## Adding a migration

1. Drop a new file `NNNN_my_change.sql` into this directory. Use the
   next unused integer.
2. Write DDL only (see above). No pragmas.
3. `cargo check` from `src-tauri/` — sqlx compiles `sqlx::migrate!` at
   build time and will refuse to include malformed SQL.
4. `cargo test --lib` — the in-memory harness (`Db::open_in_memory`) runs
   every migration on an empty DB, so a broken file is caught without
   touching a real `atlas.db`.
5. Rebuild the app. The next start applies the new migration inside
   `Db::open` via `sqlx::migrate!("./migrations").run(&pool)`.

Nothing else to wire. The migrator tracks applied versions in
`_sqlx_migrations` automatically.

## Version introspection

`Db::current_version() -> Option<i64>` returns the highest applied
version from `_sqlx_migrations`. Used by:

- `cargo run --bin perf-check` — prints the schema version alongside
  the perf report so a regression can be pinned to a specific
  migration.
- Future drift detection — refuse to boot against a DB that's newer
  than the binary knows how to read.
