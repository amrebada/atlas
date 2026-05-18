// Implementation view — epic rail, current-epic detail, history, controls.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  pilotHistory,
  pilotInterrupt,
  pilotPause,
  pilotResume,
  pilotResumeRun,
  pilotStartEpic,
  type Epic,
  type HistoryEntry,
  type PilotDetail,
} from "./ipc";
import { Btn, Card, Pill, epicTone, fmtTime } from "./parts";
import { SessionTerminal } from "./SessionTerminal";

export function EpicsView({ detail }: { detail: PilotDetail }) {
  const { path, project, epics, running, paused, paneId } = detail;

  const activeNumber = useMemo(() => {
    const active = epics.find((e) => e.status === "active");
    return (active ?? epics[0])?.number ?? null;
  }, [epics]);
  const [selected, setSelected] = useState<number | null>(null);
  const current = selected ?? activeNumber;
  const epic = epics.find((e) => e.number === current) ?? null;

  return (
    <div className="flex h-full">
      {/* Epic rail */}
      <aside className="w-60 shrink-0 overflow-y-auto border-r border-line bg-chrome">
        <div className="flex items-center gap-2 px-3 py-2.5">
          <span className="text-xs font-semibold">{project.name}</span>
          <Pill tone={project.status === "done" ? "ok" : "accent"}>
            {project.status}
          </Pill>
        </div>
        <div className="flex flex-col">
          {epics.map((e) => (
            <EpicRailItem
              key={e.number}
              epic={e}
              selected={e.number === current}
              onClick={() => setSelected(e.number)}
            />
          ))}
        </div>
      </aside>

      {/* Detail */}
      <section className="min-w-0 flex-1 overflow-y-auto">
        {epic ? (
          <EpicDetail
            path={path}
            epic={epic}
            running={running}
            paused={paused}
            paneId={paneId}
          />
        ) : (
          <p className="p-6 text-xs text-text-dimmer">No epics yet.</p>
        )}
      </section>
    </div>
  );
}

function EpicRailItem({
  epic,
  selected,
  onClick,
}: {
  epic: Epic;
  selected: boolean;
  onClick: () => void;
}) {
  const done = epic.tasks.filter((t) => t.done).length;
  return (
    <button
      onClick={onClick}
      className={
        "flex flex-col gap-1 border-b border-line-soft px-3 py-2.5 text-left " +
        (selected ? "bg-row-active" : "hover:bg-surface")
      }
    >
      <div className="flex items-center gap-2">
        <span className="font-mono text-2xs text-text-dimmer">
          {String(epic.number).padStart(2, "0")}
        </span>
        <span className="truncate text-xs font-medium">{epic.title}</span>
      </div>
      <div className="flex items-center gap-2">
        <Pill tone={epicTone(epic.status)}>{epic.status}</Pill>
        {epic.tasks.length > 0 && (
          <span className="text-2xs text-text-dimmer">
            {done}/{epic.tasks.length} tasks
          </span>
        )}
      </div>
    </button>
  );
}

function EpicDetail({
  path,
  epic,
  running,
  paused,
  paneId,
}: {
  path: string;
  epic: Epic;
  running: boolean;
  paused: boolean;
  paneId?: string | null;
}) {
  const done = epic.tasks.filter((t) => t.done).length;
  const total = epic.tasks.length;
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex items-center gap-3">
        <h1 className="text-base font-semibold">
          Epic {String(epic.number).padStart(2, "0")} — {epic.title}
        </h1>
        <Pill tone={epicTone(epic.status)}>{epic.status}</Pill>
        {paused && <Pill tone="warn">paused</Pill>}
        <span className="ml-auto text-2xs text-text-dimmer">
          {epic.iterations} iteration{epic.iterations === 1 ? "" : "s"}
        </span>
      </div>

      <p className="text-xs text-text-dim">{epic.goal}</p>
      {epic.description && (
        <p className="text-xs text-text-dimmer">{epic.description}</p>
      )}

      <Controls path={path} epic={epic} running={running} paused={paused} />

      {/* Task progress */}
      <Card className="p-4">
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-text-dim">
            Tasks
          </h2>
          <span className="text-2xs text-text-dimmer">
            {done}/{total} · {pct}%
          </span>
        </div>
        <div className="mb-3 h-1.5 overflow-hidden rounded bg-surface-2">
          <div
            className="h-full rounded bg-accent transition-all"
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="flex flex-col gap-1">
          {epic.tasks.length === 0 ? (
            <p className="text-2xs text-text-dimmer">No task list yet.</p>
          ) : (
            epic.tasks.map((t) => (
              <div key={t.id} className="flex items-center gap-2 text-xs">
                <span
                  className={
                    "flex h-3.5 w-3.5 shrink-0 items-center justify-center " +
                    "rounded-sm border text-[9px] " +
                    (t.done
                      ? "border-accent bg-accent text-accent-fg"
                      : "border-line text-transparent")
                  }
                >
                  ✓
                </span>
                <span className={t.done ? "text-text-dim line-through" : ""}>
                  {t.title}
                </span>
              </div>
            ))
          )}
        </div>
      </Card>

      {paneId ? (
        <SessionTerminal paneId={paneId} />
      ) : (
        <Card className="p-4 text-2xs text-text-dimmer">
          No live session — use the controls above to start this epic.
        </Card>
      )}

      <HistoryPanel path={path} epicNumber={epic.number} />
    </div>
  );
}

function Controls({
  path,
  epic,
  running,
  paused,
}: {
  path: string;
  epic: Epic;
  running: boolean;
  paused: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch {
      /* surfaced via the next refetch */
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      {running ? (
        <>
          <Btn
            disabled={busy || paused}
            onClick={() => run(() => pilotPause(path))}
          >
            ⏸ Pause
          </Btn>
          <Btn
            disabled={busy || !paused}
            onClick={() => run(() => pilotResume(path))}
          >
            ▶ Resume
          </Btn>
          <Btn
            variant="danger"
            disabled={busy}
            onClick={() => run(() => pilotInterrupt(path))}
          >
            ⎋ Interrupt
          </Btn>
          <span className="text-2xs text-text-dimmer">
            {paused
              ? "paused — resumes at the next task"
              : "running"}
          </span>
        </>
      ) : epic.status === "interrupted" ? (
        <>
          <Btn
            variant="primary"
            disabled={busy}
            onClick={() => run(() => pilotResumeRun(path))}
          >
            ▶ Resume session
          </Btn>
          <Btn
            disabled={busy}
            onClick={() => run(() => pilotStartEpic(path, epic.number))}
          >
            ↻ Restart epic
          </Btn>
        </>
      ) : epic.status === "pending" ? (
        <Btn
          variant="primary"
          disabled={busy}
          onClick={() => run(() => pilotStartEpic(path, epic.number))}
        >
          ▶ Start epic
        </Btn>
      ) : (
        <span className="text-2xs text-text-dimmer">epic complete</span>
      )}
    </div>
  );
}

function HistoryPanel({
  path,
  epicNumber,
}: {
  path: string;
  epicNumber: number;
}) {
  const { data: history = [] } = useQuery<HistoryEntry[]>({
    queryKey: ["pilot-history", path, epicNumber],
    queryFn: () => pilotHistory(path, epicNumber),
    refetchInterval: 4_000,
  });

  return (
    <Card className="p-4">
      <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-dim">
        History
      </h2>
      {history.length === 0 ? (
        <p className="text-2xs text-text-dimmer">
          No history entries yet — they appear as the epic progresses.
        </p>
      ) : (
        <div className="flex flex-col gap-2.5">
          {[...history].reverse().map((h, i) => (
            <div key={i} className="flex gap-2.5 text-xs">
              <span className="mt-0.5 shrink-0">
                <Pill tone={h.kind === "epic" ? "ok" : "neutral"}>
                  {h.kind}
                </Pill>
              </span>
              <div className="min-w-0">
                <div className="flex items-baseline gap-2">
                  <span className="font-medium">{h.summary}</span>
                  <span className="text-2xs text-text-dimmer">
                    {fmtTime(h.ts)}
                  </span>
                </div>
                {h.rationale && (
                  <p className="text-2xs text-text-dim">{h.rationale}</p>
                )}
                {h.files.length > 0 && (
                  <p className="truncate font-mono text-2xs text-text-dimmer">
                    {h.files.join(", ")}
                  </p>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}
