# Atlas — Project Instructions

Tauri 2 desktop app: a **React 19 + TypeScript** frontend (Vite 7) over a **Rust** core
(`src-tauri/`) that owns all filesystem, git, SQLite, PTY, and MCP/agent work. Package
manager is **pnpm 10.28.1**; Node 20; Rust stable (1.78+). See `CONTRIBUTING.md` for the
authoritative style policy.

## Build & Run
- Dev (full app): `pnpm tauri dev` — first Rust compile is 5-10 min; later builds are incremental.
- Dev (frontend only): `pnpm dev` (Vite on fixed port 1420).
- Build: `pnpm tauri build` (or `pnpm build` = `tsc && vite build` for the frontend alone).

## CI gates — run all four before pushing (CI fails on any)
- `pnpm exec tsc --noEmit` (TypeScript strict mode is the only frontend linter — no ESLint/Prettier).
- In `src-tauri/`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (warnings = errors),
  `cargo test --lib`.

## Adding an IPC command (the #1 footgun — 3 Rust steps + 1 frontend)
1. Write `#[tauri::command] pub async fn <noun_verb>(state: State<'_, Db>, ...) -> Result<T, String>`
   in `src-tauri/src/commands/<area>.rs`.
2. If it's a new file, add `pub mod <file>;` to `commands/mod.rs`.
3. Register the fully-qualified path in the `tauri::generate_handler![...]` list in `src-tauri/src/lib.rs`.
   **Skipping this compiles fine but fails at runtime with "command not found."**
4. Add a typed wrapper in `src/ipc/index.ts`: `export const myThing = (arg) => invoke<T>("my_thing", { arg })`.
   Object keys must match the Rust param names (write camelCase — Tauri maps to snake_case).
   Never call `invoke()` directly from components.

## Types (ts-rs)
- `src/types/rust.ts` is **generated** — never hand-edit. Regenerate by running `cargo test` in
  `src-tauri/` (ts-rs writes it via export tests; there is no npm codegen script). Forgetting this is a
  recurring bug. Rust types carry `#[ts(export, export_to = "../../src/types/rust.ts", rename_all = "camelCase")]`.
- `src/types/index.ts` is **hand-written** and is what `src/ipc/` imports; it parallels `rust.ts` and can
  drift — keep them in sync manually. (Most code uses `index.ts`; only the pilot window uses `rust.ts`.)

## Code conventions
- **Rust errors**: commands return `Result<T, String>` via `.map_err(|e: anyhow::Error| e.to_string())`.
  Internal code uses `anyhow::Result`; `thiserror` only at stable API boundaries. No custom command error type.
- **DB access only in `src-tauri/src/storage/`** — never call sqlx from a command handler. `sqlx` is the
  async driver for Atlas's own `atlas.db`; `rusqlite` is ONLY for read-only access to third-party tool DBs.
- **Migrations**: forward-only `src-tauri/migrations/NNNN_name.sql`, DDL only, **no PRAGMA inside migrations**
  (set pragmas on `SqliteConnectOptions` in `storage/db.rs`). Numbering gaps are fine.
- **Rust->frontend events** go through typed emitters in `src-tauri/src/events.rs` (one payload struct per
  channel, colon-namespaced names like `project:updated`). Don't `app.emit` ad-hoc from commands.
- **Rust tests** are inline `#[cfg(test)] mod tests` at the bottom of each file (no `tests/` dir — that's why
  CI uses `--lib`). Sync = `#[test]`, async/DB = `#[tokio::test]`; use `tempfile`.
- **Frontend state**: TanStack Query owns server data (query keys like `['projects']`); Zustand `useUiStore`
  owns only ephemeral UI state (selection, overlays, theme, toasts). Reconcile the cache from event hooks
  (`queryClient.setQueryData` / `invalidateQueries`). Pass `null` (not `undefined`) for `Option<T>` args.
- **Styling**: never hardcode a color. All theming is CSS variables in `src/ui/tokens.css` (OKLCH, switched
  by `data-theme`/`data-font`/`data-density` on `<html>`). Use Tailwind token classes (`text-text-dim`) or
  inline `style={{ color: 'var(--accent)' }}`. No CSS modules. Files open with a `// Atlas - ...` banner.
- **Frontend layout**: feature slices in `src/features/<kebab-case>/` (flat, named exports, NO barrel files —
  import by deep path in `App.tsx`). Shared chrome in `src/components/`. The per-project Inspector tabs live in
  `src/components/Inspector/tabs/`.

## Storage model (important nuance)
Per-project JSON under `<project>/.atlas/` is the source of truth for **content** (notes, todos, scripts,
milestones, pilot). Mutations are **write-through**: write JSON atomically first, then update the SQLite index.
BUT the `projects` table itself (path, git status, metrics, pin/archive, tags) lives ONLY in SQLite — it
rebuilds from on-disk discovery, not from a JSON mirror.

## Optional / experimental features (default-off, env-gated — don't assume they run)
- Embedded MCP server (`mcp.rs`, `ATLAS_MCP_ENABLED`), outbound atlas-agent (`agent.rs`, `ATLAS_AGENT_ENABLED`,
  Phase 4 prototype), and Atlas Pilot (`pilot/`, drives a real `claude` PTY via `<<ATLAS:*>>` sentinels).

## Project structure
```
src-tauri/src/commands/   IPC handlers (one file per domain) — registered in lib.rs
src-tauri/src/storage/    SQLite (sqlx) + per-project JSON hybrid store
src-tauri/src/{git,watcher,terminal,scripts,editors,providers}/  native/OS integration
src-tauri/src/{mcp,agent,pilot}/  optional remote-control / automation
src-tauri/src/{events,lib}.rs  event emitters; bootstrap + command registration
src/ipc/        typed invoke wrappers (the only place that calls Tauri invoke)
src/state/      Zustand UI store (useUiStore)
src/features/   feature slices; src/components/ shared chrome; src/ui/ tokens & global css
```

## Commits & releases
- **Conventional Commits**, lowercase imperative, no trailing period; meaningful scopes (`feat(terminal)`,
  `fix(mcp)`). Release commits are `release: vX.Y.Z`.
- Versioning is tag-driven: `BUMP=patch ./bump-version.sh` (syncs package.json/tauri.conf.json/Cargo.toml),
  commit, push a `vX.Y.Z` tag — `release.yml` builds/signs/notarizes and writes `latest.json` for the updater.
- **Do not commit, push, or open PRs unless explicitly asked in the current request.**
