// Embeds the live Claude session's PTY in the Pilot window — the same
// xterm component the main Atlas window uses for terminal panes.

import { TerminalPane } from "../terminal/TerminalPane";
import type { Pane } from "../../types";

export function SessionTerminal({ paneId }: { paneId: string }) {
  // TerminalPane only reads `pane.id`; the rest is a harmless stub.
  const pane = {
    id: paneId,
    kind: "claude-session",
    title: "Claude session",
    status: "active",
    cwd: "",
  } as Pane;

  return (
    <div className="h-[460px] overflow-hidden rounded-lg border border-line bg-bg">
      {/* key remounts xterm cleanly when the epic (and pane) changes */}
      <TerminalPane key={paneId} pane={pane} />
    </div>
  );
}
