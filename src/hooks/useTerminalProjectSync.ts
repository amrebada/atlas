// Atlas - terminal-strip ↔ project sync.
//
// Auto-restore from `pane_layout_get` is disabled: saved layouts hold pane
// ids that point at PTYs from a previous session (or pre-close state), so
// hydrating them spawns ghost panes whose terminal surface is empty because
// no live PTY is attached. Terminals stay session-scoped now — opening a
// project never resurrects panes from disk.
export function useTerminalProjectSync(_projectId: string | null): void {
  // Intentionally empty. Kept as a stable hook signature so App.tsx wiring
  // doesn't churn while the saved-layout feature is being reconsidered.
}
