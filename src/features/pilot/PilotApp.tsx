// Root component for the Atlas Pilot window.

import { useEffect, useState } from "react";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useUiStore } from "../../state/store";
import { getSettings } from "../../ipc";
import {
  onPilotChanged,
  pilotCreate,
  pilotGet,
  pilotList,
  type PilotDetail,
  type PilotSummary,
} from "./ipc";
import { Btn, Card, Pill, pilotTone } from "./parts";
import { DraftView } from "./DraftView";
import { EpicsView } from "./EpicsView";

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 5_000, retry: false } },
});

/**
 * Apply Atlas's theming the same way the main window does — `data-theme`,
 * `data-term-theme`, `data-density`, `data-font` on `<html>`. Without this
 * `--sans` is undefined and the window renders in a serif fallback.
 */
function usePilotTheme() {
  const theme = useUiStore((s) => s.theme);
  const terminalTheme = useUiStore((s) => s.terminalTheme);
  const density = useUiStore((s) => s.density);
  const font = useUiStore((s) => s.font);
  const setTheme = useUiStore((s) => s.setTheme);
  const setTerminalTheme = useUiStore((s) => s.setTerminalTheme);

  // Follow the user's persisted theme-mode selection.
  useEffect(() => {
    getSettings()
      .then((s) => {
        if (s?.general?.theme) setTheme(s.general.theme);
        if (s?.general?.terminalTheme)
          setTerminalTheme(s.general.terminalTheme);
      })
      .catch(() => {});
  }, [setTheme, setTerminalTheme]);

  useEffect(() => {
    const root = document.documentElement;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const resolve = (t: typeof theme) =>
      t === "system" ? (mql.matches ? "dark" : "light") : t;
    const apply = () => {
      root.dataset.theme = resolve(theme);
      root.dataset.termTheme = resolve(terminalTheme);
    };
    apply();
    root.dataset.density = density;
    root.dataset.font = font;
    if (theme === "system" || terminalTheme === "system") {
      mql.addEventListener("change", apply);
      return () => mql.removeEventListener("change", apply);
    }
    return undefined;
  }, [theme, terminalTheme, density, font]);
}

export default function PilotApp() {
  usePilotTheme();
  return (
    <QueryClientProvider client={queryClient}>
      <PilotShell />
    </QueryClientProvider>
  );
}

function PilotShell() {
  const [selected, setSelected] = useState<string | null>(null);
  const qc = useQueryClient();

  // Refetch on orchestrator events.
  useEffect(() => {
    let un: (() => void) | undefined;
    onPilotChanged(() => {
      qc.invalidateQueries({ queryKey: ["pilot-list"] });
      qc.invalidateQueries({ queryKey: ["pilot-detail"] });
      qc.invalidateQueries({ queryKey: ["pilot-history"] });
    }).then((f) => {
      un = f;
    });
    return () => un?.();
  }, [qc]);

  return (
    <div className="flex h-screen flex-col bg-bg text-text">
      <header
        data-tauri-drag-region
        className="flex h-11 shrink-0 items-center gap-3 border-b border-line bg-chrome pl-[80px] pr-4"
      >
        <span className="text-[13px] font-semibold tracking-tight">
          Atlas Pilot
        </span>
        {selected && (
          <Btn variant="ghost" onClick={() => setSelected(null)}>
            ← All projects
          </Btn>
        )}
        <span className="ml-auto text-2xs text-text-dimmer">
          automated project lifecycle
        </span>
      </header>
      <main className="min-h-0 flex-1 overflow-hidden">
        {selected ? (
          <ProjectDetail path={selected} />
        ) : (
          <ProjectList onOpen={setSelected} />
        )}
      </main>
    </div>
  );
}

function ProjectList({ onOpen }: { onOpen: (path: string) => void }) {
  const { data: projects = [], isLoading } = useQuery({
    queryKey: ["pilot-list"],
    queryFn: pilotList,
  });

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-5 overflow-y-auto p-6">
      <NewProjectForm onCreated={onOpen} />
      <div>
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-dim">
          Projects
        </h2>
        {isLoading ? (
          <p className="text-xs text-text-dimmer">Loading…</p>
        ) : projects.length === 0 ? (
          <p className="text-xs text-text-dimmer">
            No pilot projects yet — create one above.
          </p>
        ) : (
          <div className="flex flex-col gap-1.5">
            {projects.map((p: PilotSummary) => (
              <button
                key={p.path}
                onClick={() => onOpen(p.path)}
                className="flex items-center gap-3 rounded-lg border border-line bg-surface px-3.5 py-3 text-left hover:border-text-dimmer"
              >
                <span className="font-medium">{p.name}</span>
                <Pill tone={pilotTone(p.status)}>{p.status}</Pill>
                <span className="ml-auto truncate text-2xs text-text-dimmer">
                  {p.path}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function NewProjectForm({ onCreated }: { onCreated: (path: string) => void }) {
  const [name, setName] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === "string") setParent(dir);
  };

  const create = async () => {
    if (!name.trim() || !parent) return;
    setBusy(true);
    setError(null);
    try {
      const path = await pilotCreate(parent, name.trim());
      onCreated(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="p-4">
      <h2 className="mb-3 text-xs font-semibold uppercase tracking-wide text-text-dim">
        New pilot project
      </h2>
      <div className="flex flex-col gap-2.5">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Project name"
          className="rounded-md border border-line bg-surface-2 px-3 py-2 text-sm outline-none focus:border-accent"
        />
        <div className="flex items-center gap-2">
          <Btn onClick={pickFolder}>Choose parent folder…</Btn>
          <span className="truncate text-2xs text-text-dimmer">
            {parent ?? "no folder selected"}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Btn
            variant="primary"
            disabled={busy || !name.trim() || !parent}
            onClick={create}
          >
            {busy ? "Creating…" : "Create & start planning"}
          </Btn>
          {error && <span className="text-2xs text-danger">{error}</span>}
        </div>
        <p className="text-2xs text-text-dimmer">
          Atlas creates the folder, runs <code>git init</code>, and starts a
          gated planning session (grill-me → PRD → epics).
        </p>
      </div>
    </Card>
  );
}

function ProjectDetail({ path }: { path: string }) {
  const { data, isLoading, error } = useQuery<PilotDetail>({
    queryKey: ["pilot-detail", path],
    queryFn: () => pilotGet(path),
    refetchInterval: 4_000,
  });

  if (isLoading)
    return <p className="p-6 text-xs text-text-dimmer">Loading project…</p>;
  if (error || !data)
    return <p className="p-6 text-xs text-danger">{String(error)}</p>;

  return data.project.status === "draft" ? (
    <DraftView detail={data} />
  ) : (
    <EpicsView detail={data} />
  );
}
