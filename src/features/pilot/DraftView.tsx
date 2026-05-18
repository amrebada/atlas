// Draft (planning) view — the 3-gate planning flow.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  pilotApproveGate,
  pilotArtifactRead,
  pilotArtifactWrite,
  pilotResumeRun,
  pilotStartPlanning,
  type PilotDetail,
  type PilotGate,
} from "./ipc";
import { Btn, Card, Pill } from "./parts";
import { SessionTerminal } from "./SessionTerminal";

const STEPS: { gate: PilotGate; label: string }[] = [
  { gate: "reqs", label: "Requirements" },
  { gate: "prd", label: "PRD" },
  { gate: "epics", label: "Epics" },
];

export function DraftView({ detail }: { detail: PilotDetail }) {
  const { path, project, epics, running } = detail;
  const gate = project.gate;

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-4 overflow-y-auto p-6">
      <div className="flex items-center gap-3">
        <h1 className="text-base font-semibold">{project.name}</h1>
        <Pill tone="warn">draft</Pill>
        <span className="ml-auto text-2xs text-text-dimmer">
          {running ? "planning session live" : "planning session idle"}
        </span>
      </div>

      <GateSteps current={gate} />

      <PlanningSession detail={detail} />

      {gate === "reqs" && (
        <ArtifactGate
          path={path}
          kind="requirements"
          title="Review requirements"
          hint="Edit the captured requirements if needed, then approve to generate the PRD."
        />
      )}
      {gate === "prd" && (
        <ArtifactGate
          path={path}
          kind="prd"
          title="Review PRD"
          hint="Edit the PRD if needed, then approve to generate epics."
        />
      )}
      {gate === "epics" && (
        <EpicsGate
          path={path}
          epicCount={epics.length}
          epics={epics.map((e) => ({
            number: e.number,
            title: e.title,
            goal: e.goal,
            release: e.release,
          }))}
        />
      )}
    </div>
  );
}

/** The live planning terminal, or a control to (re)start the session. */
function PlanningSession({ detail }: { detail: PilotDetail }) {
  const [busy, setBusy] = useState(false);

  if (detail.paneId) {
    return <SessionTerminal paneId={detail.paneId} />;
  }

  const hasSession = !!detail.project.planningSessionId;
  const start = async () => {
    setBusy(true);
    try {
      if (hasSession) {
        // Prefer resuming the existing session; fall back to a fresh one.
        await pilotResumeRun(detail.path).catch(() =>
          pilotStartPlanning(detail.path),
        );
      } else {
        await pilotStartPlanning(detail.path);
      }
    } catch {
      /* surfaced via the next pilot:changed refetch */
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="flex flex-col items-center gap-3 p-6 text-center">
      <p className="text-xs text-text-dim">
        The planning session isn't running.
      </p>
      <Btn variant="primary" disabled={busy} onClick={start}>
        {busy
          ? "Starting…"
          : hasSession
            ? "Resume planning session"
            : "Start planning session"}
      </Btn>
    </Card>
  );
}

function GateSteps({ current }: { current: PilotGate | null }) {
  const idx = current ? STEPS.findIndex((s) => s.gate === current) : -1;
  return (
    <div className="flex items-center gap-2">
      {STEPS.map((s, i) => {
        const active = s.gate === current;
        const passed = idx >= 0 && i < idx;
        return (
          <div key={s.gate} className="flex items-center gap-2">
            <span
              className={
                "rounded-md px-2.5 py-1 text-2xs font-medium " +
                (active
                  ? "bg-accent text-accent-fg"
                  : passed
                    ? "bg-surface-2 text-info"
                    : "bg-surface-2 text-text-dimmer")
              }
            >
              {i + 1}. {s.label}
            </span>
            {i < STEPS.length - 1 && (
              <span className="text-text-dimmer">→</span>
            )}
          </div>
        );
      })}
    </div>
  );
}

function ArtifactGate({
  path,
  kind,
  title,
  hint,
}: {
  path: string;
  kind: "requirements" | "prd";
  title: string;
  hint: string;
}) {
  const { data, isLoading } = useQuery({
    queryKey: ["pilot-artifact", path, kind],
    queryFn: () => pilotArtifactRead(path, kind),
  });
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const value = draft ?? data ?? "";

  const save = async () => {
    setBusy(true);
    try {
      await pilotArtifactWrite(path, kind, value);
    } finally {
      setBusy(false);
    }
  };
  const approve = async () => {
    setBusy(true);
    try {
      if (draft !== null) await pilotArtifactWrite(path, kind, value);
      await pilotApproveGate(path);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="p-4">
      <h2 className="mb-1 text-sm font-semibold">{title}</h2>
      <p className="mb-3 text-2xs text-text-dimmer">{hint}</p>
      {isLoading ? (
        <p className="text-xs text-text-dimmer">Loading…</p>
      ) : (
        <textarea
          value={value}
          onChange={(e) => setDraft(e.target.value)}
          rows={16}
          className="w-full resize-y rounded-md border border-line bg-surface-2 p-3 font-mono text-xs leading-relaxed outline-none focus:border-accent"
        />
      )}
      <div className="mt-3 flex gap-2">
        <Btn disabled={busy || draft === null} onClick={save}>
          Save edits
        </Btn>
        <Btn variant="primary" disabled={busy} onClick={approve}>
          Approve & continue
        </Btn>
      </div>
    </Card>
  );
}

function EpicsGate({
  path,
  epicCount,
  epics,
}: {
  path: string;
  epicCount: number;
  epics: { number: number; title: string; goal: string; release: string | null }[];
}) {
  const [busy, setBusy] = useState(false);
  const approve = async () => {
    setBusy(true);
    try {
      await pilotApproveGate(path);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="p-4">
      <h2 className="mb-1 text-sm font-semibold">
        Review epics ({epicCount})
      </h2>
      <p className="mb-3 text-2xs text-text-dimmer">
        Approving starts implementation. To reorder or adjust an epic, edit
        its file under <code>.atlas/pilot/epics/</code> before approving.
      </p>
      <div className="flex flex-col gap-1.5">
        {epics.map((e) => (
          <div
            key={e.number}
            className="rounded-md border border-line bg-surface-2 px-3 py-2"
          >
            <div className="flex items-center gap-2">
              <span className="font-mono text-2xs text-text-dimmer">
                {String(e.number).padStart(2, "0")}
              </span>
              <span className="text-xs font-medium">{e.title}</span>
              {e.release && <Pill tone="neutral">{e.release}</Pill>}
            </div>
            <p className="mt-0.5 text-2xs text-text-dim">{e.goal}</p>
          </div>
        ))}
      </div>
      <div className="mt-3">
        <Btn variant="primary" disabled={busy || epicCount === 0} onClick={approve}>
          Approve epics & start building
        </Btn>
      </div>
    </Card>
  );
}
