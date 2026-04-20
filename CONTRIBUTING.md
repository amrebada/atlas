# Contributing to Atlas

Thanks for your interest in contributing. This document covers the practical details:
development setup, coding style, and the PR workflow.

## Development setup

1. Install prerequisites:
   - Rust stable (1.78+, via `rustup`)
   - Node.js 20+ and pnpm
   - Platform Tauri deps: see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)

2. Clone and install:

   ```bash
   git clone https://github.com/amrebada/atlas.git
   cd atlas
   pnpm install
   ```

3. Run the dev build:

   ```bash
   pnpm tauri dev
   ```

   The first Rust compile is slow (5-10 min). Subsequent edits are incremental and the
   frontend hot-reloads via Vite.

## Repository layout

- `src-tauri/` - Rust core. Owns everything that touches the filesystem, spawns processes,
  or reads git repos.
- `src/` - React + TypeScript frontend. Talks to Rust exclusively through the typed
  wrappers in `src/ipc/`.
- `src-tauri/migrations/` - forward-only SQLite migrations. Name files `NNNN_description.sql`.

## Coding style

- Rust:
  - `cargo fmt` before committing.
  - `cargo clippy --all-targets` should be clean.
  - Prefer `anyhow::Result` for fallible internal code; use `thiserror` only for errors that
    cross a stable API boundary.
  - Database work goes in `src-tauri/src/storage/`; never call `sqlx` from a command handler
    directly.

- TypeScript:
  - `tsc --noEmit` should pass.
  - No `any`. If you need to escape a type, use `unknown` plus a narrow.
  - IPC call sites go through `src/ipc/` wrappers; never `invoke()` from a component.
  - Domain types live in `src/types/`; the Rust side mirrors them via `ts-rs`.

- Comments:
  - Write short comments only where the *why* is not obvious from the code.
  - Skip multi-paragraph module doc blocks. A single sentence is enough.

## Tests

- `cargo test --lib` should pass (170+ unit + integration tests).
- `pnpm build` should succeed (runs `tsc` + `vite build`).

Fast feedback loop while iterating:

```bash
cd src-tauri && cargo test --lib -- --test-threads=1
```

## Pull requests

- Branch from `main`.
- Keep PRs focused. A bug fix + an unrelated refactor should be two PRs.
- Write a PR description that explains *why*, not just *what*.
- Link the issue, if any.
- CI runs `cargo test`, `cargo clippy`, and `pnpm build` on macOS, Linux, and Windows.

## Reporting bugs

Open an issue with:

- What you expected to happen.
- What actually happened.
- Steps to reproduce.
- OS + Atlas version (`About` panel in Settings).
- Relevant log output from `~/Library/Application Support/atlas/crash.log` (macOS) or the
  equivalent on your platform, if the app crashed.

## License

Contributions are accepted under the terms of the [MIT License](./LICENSE). By opening a
pull request you agree to license your contribution under the same terms.
