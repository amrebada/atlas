import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon, Kbd, LangDot, type IconName } from "../../components/Icon";
import {
  homeDashboard,
  openInEditor,
  terminalOpen,
  type HomeCommit,
  type HomeDashboard,
  type HomeDay,
  type HomeProject,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import { useTerminalStore, makePane } from "../terminal/layout";
import { useGitSync } from "../git-sync/useGitSync";
import type { Project } from "../../types";

// Atlas - Home page. Cross-project activity analytics (commits, estimated
// hours, streak), what needs attention (dirty / ahead / behind), recent
// commits, and one-click shortcuts. Lives in the centre column when the
// sidebar's "Home" entry is active.

type RangeWeeks = 12 | 26 | 52;

interface HomeViewProps {
  projects: Project[];
}

export function HomeView({ projects }: HomeViewProps) {
  const [weeks, setWeeks] = useState<RangeWeeks>(12);
  const queryClient = useQueryClient();

  const { data, isLoading, isFetching, error } = useQuery<HomeDashboard>({
    queryKey: ["home", weeks],
    queryFn: () => homeDashboard(weeks * 7),
    staleTime: 60_000,
    retry: false,
  });

  const projectById = useMemo(() => {
    const m = new Map<string, Project>();
    for (const p of projects) m.set(p.id, p);
    return m;
  }, [projects]);

  return (
    <div className="h-full overflow-y-auto min-w-0">
      <div className="px-[22px] py-[18px] max-w-[1100px] mx-auto flex flex-col gap-[18px]">
        <Header
          author={data?.author ?? null}
          weeks={weeks}
          onWeeks={setWeeks}
          refreshing={isFetching}
          onRefresh={() =>
            queryClient.invalidateQueries({ queryKey: ["home"] })
          }
        />

        <QuickActions />

        {error ? (
          <div className="text-[12px] text-danger font-mono">
            Couldn&apos;t load activity: {String(error)}
          </div>
        ) : null}

        <StatTiles totals={data?.totals ?? null} loading={isLoading} />

        <Panel title="Activity" hint={`last ${weeks} weeks · commits by you`}>
          <Heatmap days={data?.days ?? []} weeks={weeks} loading={isLoading} />
        </Panel>

        <div className="grid gap-[18px]" style={{ gridTemplateColumns: "1fr 1fr" }}>
          <Panel title="When you work" hint="commits per hour · last 30 days">
            <Histogram
              values={data?.hours ?? []}
              labelFor={(i) => (i % 6 === 0 ? `${i}h` : "")}
              titleFor={(i, v) => `${pad2(i)}:00–${pad2(i)}:59 · ${v} commit${v === 1 ? "" : "s"}`}
              loading={isLoading}
            />
          </Panel>
          <Panel title="By weekday" hint="commits per weekday · last 30 days">
            <Histogram
              values={data?.weekdays ?? []}
              labelFor={(i) => WEEKDAYS[i] ?? ""}
              titleFor={(i, v) => `${WEEKDAYS_LONG[i]} · ${v} commit${v === 1 ? "" : "s"}`}
              loading={isLoading}
            />
          </Panel>
        </div>

        <AttentionPanel rows={data?.attention ?? []} projectById={projectById} />

        <div className="grid gap-[18px]" style={{ gridTemplateColumns: "3fr 2fr" }}>
          <Panel title="Recent commits" hint="newest first">
            <RecentCommits
              commits={data?.recentCommits ?? []}
              loading={isLoading}
            />
          </Panel>
          <Panel title="Active projects" hint="last 7 / 30 days">
            <ActiveProjects
              rows={data?.projects ?? []}
              projectById={projectById}
              loading={isLoading}
            />
          </Panel>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Header + quick actions

function Header({
  author,
  weeks,
  onWeeks,
  refreshing,
  onRefresh,
}: {
  author: string | null;
  weeks: RangeWeeks;
  onWeeks: (w: RangeWeeks) => void;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  const now = new Date();
  const hour = now.getHours();
  const greeting =
    hour < 5 ? "Still up" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const firstName = author?.split(/\s+/)[0] ?? null;
  const dateLabel = now.toLocaleDateString(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
  });

  return (
    <div className="flex items-end gap-[14px]">
      <div className="min-w-0">
        <div className="text-[20px] font-semibold text-text leading-tight">
          {greeting}
          {firstName ? `, ${firstName}` : ""}
        </div>
        <div className="font-mono text-[11px] text-text-dim mt-[2px]">
          {dateLabel}
          {author ? ` · commits by ${author}` : " · all authors (no git identity configured)"}
        </div>
      </div>
      <div className="flex-1" />
      <div className="flex items-center gap-1">
        {([12, 26, 52] as RangeWeeks[]).map((w) => {
          const active = w === weeks;
          return (
            <button
              key={w}
              type="button"
              onClick={() => onWeeks(w)}
              className="px-2 py-[3px] font-mono text-[11px] rounded-[4px]"
              style={{
                background: active ? "var(--surface-2)" : "transparent",
                color: active ? "var(--text)" : "var(--text-dim)",
                boxShadow: active ? "inset 0 -2px 0 var(--accent)" : "none",
              }}
            >
              {w}w
            </button>
          );
        })}
      </div>
      <button
        type="button"
        onClick={onRefresh}
        title="Refresh"
        aria-label="Refresh activity"
        className="w-6 h-6 border border-line rounded-[5px] text-text-dim inline-flex items-center justify-center hover:text-text"
        style={{ opacity: refreshing ? 0.5 : 1 }}
      >
        <Icon name="repeat" size={12} />
      </button>
    </div>
  );
}

interface QuickAction {
  id: string;
  icon: IconName;
  label: string;
  keys?: string[];
  run: () => void;
}

function QuickActions() {
  const openNewProject = useUiStore((s) => s.openNewProject);
  const openSettings = useUiStore((s) => s.openSettings);
  const setPaletteOpen = useUiStore((s) => s.setPaletteOpen);
  const setTodayOpen = useUiStore((s) => s.setTodayOpen);
  const setTimelineOpen = useUiStore((s) => s.setTimelineOpen);
  const setCollection = useUiStore((s) => s.setCollection);
  const selectedProjectId = useUiStore((s) => s.selectedProjectId);
  const pushToast = useUiStore((s) => s.pushToast);

  const actions: QuickAction[] = [
    { id: "new", icon: "plus", label: "New project", keys: ["⌘", "N"], run: () => openNewProject("new") },
    { id: "clone", icon: "clone", label: "Clone repo", keys: ["⌘", "⇧", "N"], run: () => openNewProject("clone") },
    { id: "import", icon: "import", label: "Import folder", run: () => openNewProject("import") },
    { id: "all", icon: "folder", label: "All projects", run: () => setCollection("all") },
    { id: "today", icon: "check", label: "Today", keys: ["⌘", "T"], run: () => setTodayOpen(true) },
    { id: "timeline", icon: "grid", label: "Timeline", keys: ["⌘", "⇧", "T"], run: () => setTimelineOpen(true) },
    { id: "palette", icon: "search", label: "Find anything", keys: ["⌘", "K"], run: () => setPaletteOpen(true) },
    {
      id: "editor",
      icon: "code",
      label: "Open in editor",
      keys: ["⌘", "E"],
      run: () => {
        if (!selectedProjectId) {
          pushToast("info", "Select a project first");
          return;
        }
        openInEditor(selectedProjectId).catch((err) =>
          pushToast("error", `Open failed: ${String(err)}`),
        );
      },
    },
    { id: "settings", icon: "gear", label: "Settings", keys: ["⌘", ","], run: () => openSettings("general") },
  ];

  return (
    <div
      className="grid gap-[8px]"
      style={{ gridTemplateColumns: "repeat(auto-fill, minmax(150px, 1fr))" }}
    >
      {actions.map((a) => (
        <button
          key={a.id}
          type="button"
          onClick={a.run}
          className="flex items-center gap-2 px-[10px] h-[34px] rounded-[6px] border border-line bg-surface text-text-dim hover:text-text hover:bg-row-active text-left"
        >
          <Icon name={a.icon} size={13} />
          <span className="text-[12px] flex-1 truncate">{a.label}</span>
          {a.keys && (
            <span className="flex gap-[2px]">
              {a.keys.map((k) => (
                <Kbd key={k}>{k}</Kbd>
              ))}
            </span>
          )}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Stat tiles

function StatTiles({
  totals,
  loading,
}: {
  totals: HomeDashboard["totals"] | null;
  loading: boolean;
}) {
  const tiles: Array<{ label: string; value: string; sub: string }> = totals
    ? [
        {
          label: "today",
          value: fmtHours(totals.minutesToday),
          sub: `${totals.commitsToday} commit${totals.commitsToday === 1 ? "" : "s"}`,
        },
        {
          label: "this week",
          value: fmtHours(totals.minutes7d),
          sub: `${totals.commits7d} commits · +${totals.additions7d} −${totals.deletions7d}`,
        },
        {
          label: "last 30 days",
          value: fmtHours(totals.minutes30d),
          sub: `${totals.commits30d} commits`,
        },
        {
          label: "streak",
          value: `${totals.streakDays}d`,
          sub: totals.streakDays > 0 ? "consecutive days" : "commit today to start",
        },
        {
          label: "active projects",
          value: String(totals.activeProjects7d),
          sub: "touched this week",
        },
      ]
    : [];

  return (
    <div
      className="grid gap-[8px]"
      style={{ gridTemplateColumns: "repeat(5, minmax(0, 1fr))" }}
    >
      {(loading && !totals ? Array.from({ length: 5 }) : tiles).map((t, i) => (
        <div
          key={i}
          className="rounded-[6px] border border-line bg-surface px-[12px] py-[10px] min-w-0"
        >
          <div className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
            {t ? (t as { label: string }).label : "…"}
          </div>
          <div className="text-[20px] font-semibold text-text mt-[2px] font-mono leading-tight">
            {t ? (t as { value: string }).value : "—"}
          </div>
          <div className="font-mono text-[10px] text-text-dimmer mt-[2px] truncate">
            {t ? (t as { sub: string }).sub : ""}
          </div>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Charts (inline SVG, theme-token colored)

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_LONG = [
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
  "Sunday",
];

/** GitHub-style contribution grid. One hue (accent), opacity = magnitude. */
function Heatmap({
  days,
  weeks,
  loading,
}: {
  days: HomeDay[];
  weeks: number;
  loading: boolean;
}) {
  const cell = 11;
  const gap = 3;
  const step = cell + gap;
  const labelW = 28;
  const topH = 14;

  // Align the grid so each column is a Monday→Sunday week; the first
  // column may be partial (pad with empties on the left).
  const padded = useMemo(() => {
    if (days.length === 0) return [];
    const first = new Date(days[0].date + "T00:00:00");
    const offset = (first.getDay() + 6) % 7; // Mon=0
    const blanks: Array<HomeDay | null> = Array.from({ length: offset }, () => null);
    return [...blanks, ...days];
  }, [days]);

  const cols = Math.max(weeks, Math.ceil(padded.length / 7));
  const max = Math.max(1, ...days.map((d) => d.commits));
  const width = labelW + cols * step;
  const height = topH + 7 * step;

  const monthTicks = useMemo(() => {
    const out: Array<{ x: number; label: string }> = [];
    let lastMonth = -1;
    padded.forEach((d, i) => {
      if (!d) return;
      const dt = new Date(d.date + "T00:00:00");
      const m = dt.getMonth();
      if (m !== lastMonth && dt.getDate() <= 7) {
        out.push({
          x: labelW + Math.floor(i / 7) * step,
          label: dt.toLocaleDateString(undefined, { month: "short" }),
        });
        lastMonth = m;
      }
    });
    return out;
  }, [padded]);

  if (loading && days.length === 0) {
    return <div className="h-[110px] font-mono text-[11px] text-text-dimmer">loading…</div>;
  }

  return (
    <div className="overflow-x-auto">
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="Commits per day"
        style={{ display: "block", maxWidth: "100%" }}
      >
        {monthTicks.map((t) => (
          <text
            key={`${t.label}-${t.x}`}
            x={t.x}
            y={9}
            fontSize={9}
            fontFamily="var(--mono)"
            fill="var(--text-dimmer)"
          >
            {t.label}
          </text>
        ))}
        {[0, 2, 4, 6].map((r) => (
          <text
            key={r}
            x={0}
            y={topH + r * step + cell - 2}
            fontSize={9}
            fontFamily="var(--mono)"
            fill="var(--text-dimmer)"
          >
            {WEEKDAYS[r]}
          </text>
        ))}
        {padded.map((d, i) => {
          const col = Math.floor(i / 7);
          const row = i % 7;
          const x = labelW + col * step;
          const y = topH + row * step;
          if (!d) return null;
          const level = d.commits === 0 ? 0 : 0.25 + 0.75 * Math.min(1, d.commits / max);
          return (
            <rect
              key={d.date}
              x={x}
              y={y}
              width={cell}
              height={cell}
              rx={2}
              fill={d.commits === 0 ? "var(--surface-2)" : "var(--accent)"}
              fillOpacity={d.commits === 0 ? 1 : level}
              stroke="var(--line-soft)"
              strokeWidth={0.5}
            >
              <title>
                {`${d.date} · ${d.commits} commit${d.commits === 1 ? "" : "s"}` +
                  (d.minutes > 0 ? ` · ~${fmtHours(d.minutes)}` : "") +
                  (d.projects > 1 ? ` · ${d.projects} projects` : "")}
              </title>
            </rect>
          );
        })}
      </svg>
    </div>
  );
}

/** Thin vertical bars with rounded data-ends, single series, no legend. */
function Histogram({
  values,
  labelFor,
  titleFor,
  loading,
}: {
  values: number[];
  labelFor: (i: number) => string;
  titleFor: (i: number, v: number) => string;
  loading: boolean;
}) {
  const h = 90;
  const axisH = 14;
  const width = 320;
  const n = Math.max(1, values.length);
  const slot = width / n;
  const barW = Math.max(3, Math.min(14, slot * 0.6));
  const max = Math.max(1, ...values);
  const total = values.reduce((a, b) => a + b, 0);

  if (loading && values.length === 0) {
    return <div className="h-[104px] font-mono text-[11px] text-text-dimmer">loading…</div>;
  }
  if (total === 0) {
    return (
      <div className="h-[104px] flex items-center justify-center font-mono text-[11px] text-text-dimmer">
        no commits in the last 30 days
      </div>
    );
  }

  return (
    <svg
      viewBox={`0 0 ${width} ${h + axisH}`}
      width="100%"
      height={h + axisH}
      role="img"
      aria-label="Commits histogram"
      style={{ display: "block" }}
    >
      <line
        x1={0}
        x2={width}
        y1={h + 0.5}
        y2={h + 0.5}
        stroke="var(--line)"
        strokeWidth={1}
      />
      {values.map((v, i) => {
        const bh = v === 0 ? 0 : Math.max(2, (v / max) * (h - 6));
        const x = i * slot + (slot - barW) / 2;
        const isPeak = v === max;
        return (
          <g key={i}>
            {/* Hit target wider than the mark so hover is forgiving. */}
            <rect x={i * slot} y={0} width={slot} height={h} fill="transparent">
              <title>{titleFor(i, v)}</title>
            </rect>
            {v > 0 && (
              <rect
                x={x}
                y={h - bh}
                width={barW}
                height={bh}
                rx={2}
                fill="var(--accent)"
                fillOpacity={isPeak ? 1 : 0.55}
                pointerEvents="none"
              />
            )}
            {isPeak && (
              <text
                x={x + barW / 2}
                y={Math.max(9, h - bh - 4)}
                fontSize={9}
                textAnchor="middle"
                fontFamily="var(--mono)"
                fill="var(--text-dim)"
                pointerEvents="none"
              >
                {v}
              </text>
            )}
            {labelFor(i) && (
              <text
                x={i * slot + slot / 2}
                y={h + axisH - 2}
                fontSize={9}
                textAnchor="middle"
                fontFamily="var(--mono)"
                fill="var(--text-dimmer)"
              >
                {labelFor(i)}
              </text>
            )}
          </g>
        );
      })}
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Lists

function AttentionPanel({
  rows,
  projectById,
}: {
  rows: HomeProject[];
  projectById: Map<string, Project>;
}) {
  const syncMut = useGitSync();
  const pushToast = useUiStore((s) => s.pushToast);
  const [syncingAll, setSyncingAll] = useState(false);

  const syncable = rows.filter((r) => r.ahead > 0 || r.behind > 0);

  const syncAll = async () => {
    if (syncable.length === 0) return;
    setSyncingAll(true);
    let ok = 0;
    for (const r of syncable) {
      try {
        const res = await syncMut.mutateAsync(r.projectId);
        if (res.ok) ok += 1;
      } catch {
        // per-project toast already shown by the mutation
      }
    }
    setSyncingAll(false);
    pushToast(
      ok === syncable.length ? "success" : "warn",
      `Synced ${ok}/${syncable.length} project${syncable.length === 1 ? "" : "s"}`,
    );
  };

  if (rows.length === 0) {
    return (
      <Panel title="Needs attention" hint="dirty · unpushed · behind">
        <div className="py-[10px] font-mono text-[11px] text-text-dimmer">
          Everything is committed and in sync. Nice.
        </div>
      </Panel>
    );
  }

  return (
    <Panel
      title="Needs attention"
      hint={`${rows.length} project${rows.length === 1 ? "" : "s"}`}
      trailing={
        syncable.length > 0 ? (
          <button
            type="button"
            onClick={syncAll}
            disabled={syncingAll}
            title="pull --rebase then push every project that is ahead or behind (local changes are stashed and re-applied)"
            className="inline-flex items-center gap-1 px-[8px] h-[22px] rounded-[4px] border border-line bg-surface-2 text-text-dim hover:text-text font-mono text-[10px] disabled:opacity-40"
          >
            <Icon name="sync" size={11} />
            {syncingAll ? "syncing…" : `sync ${syncable.length}`}
          </button>
        ) : null
      }
    >
      <div className="flex flex-col">
        {rows.map((r) => (
          <ProjectRow
            key={r.projectId}
            row={r}
            project={projectById.get(r.projectId) ?? null}
            syncing={syncMut.isPending && syncMut.variables === r.projectId}
            onSync={() => syncMut.mutate(r.projectId)}
            trailing={<GitBadges row={r} />}
          />
        ))}
      </div>
    </Panel>
  );
}

function ActiveProjects({
  rows,
  projectById,
  loading,
}: {
  rows: HomeProject[];
  projectById: Map<string, Project>;
  loading: boolean;
}) {
  const syncMut = useGitSync();
  if (loading && rows.length === 0) {
    return <div className="py-[10px] font-mono text-[11px] text-text-dimmer">loading…</div>;
  }
  if (rows.length === 0) {
    return (
      <div className="py-[10px] font-mono text-[11px] text-text-dimmer">
        No commits in this window yet.
      </div>
    );
  }
  return (
    <div className="flex flex-col">
      {rows.map((r) => (
        <ProjectRow
          key={r.projectId}
          row={r}
          project={projectById.get(r.projectId) ?? null}
          syncing={syncMut.isPending && syncMut.variables === r.projectId}
          onSync={() => syncMut.mutate(r.projectId)}
          trailing={
            <span className="font-mono text-[10px] text-text-dim whitespace-nowrap">
              {r.commits7d}
              <span className="text-text-dimmer"> / {r.commits30d}</span>
              {r.minutes7d > 0 && (
                <span className="text-text-dimmer"> · ~{fmtHours(r.minutes7d)}</span>
              )}
            </span>
          }
        />
      ))}
    </div>
  );
}

function ProjectRow({
  row,
  project,
  syncing,
  onSync,
  trailing,
}: {
  row: HomeProject;
  project: Project | null;
  syncing: boolean;
  onSync: () => void;
  trailing: React.ReactNode;
}) {
  const setSelectedProjectId = useUiStore((s) => s.setSelectedProjectId);
  const setCollection = useUiStore((s) => s.setCollection);
  const pushToast = useUiStore((s) => s.pushToast);

  const openTerminal = async () => {
    if (!project) return;
    try {
      const id = await terminalOpen({ kind: "shell", cwd: project.path });
      useTerminalStore.getState().addPane(
        makePane(id, "shell", project.path, project.name, {
          branch: project.branch,
          projectId: project.id,
          projectLabel: project.name,
        }),
      );
    } catch (err) {
      pushToast("error", `Open terminal failed: ${String(err)}`);
    }
  };

  return (
    <div className="group flex items-center gap-2 h-[30px] border-b border-line-soft min-w-0">
      <LangDot color={row.color} size={7} />
      <button
        type="button"
        onClick={() => setSelectedProjectId(row.projectId)}
        onDoubleClick={() => {
          setSelectedProjectId(row.projectId);
          setCollection("all");
        }}
        title={`${row.name} · click to inspect, double-click to open in the project list`}
        className="text-[12px] text-text truncate text-left hover:underline min-w-0"
      >
        {row.name}
      </button>
      {row.branch && (
        <span className="font-mono text-[10px] text-text-dimmer truncate max-w-[120px]">
          {row.branch}
        </span>
      )}
      <div className="flex-1" />
      {trailing}
      <span className="flex items-center gap-[2px] opacity-0 group-hover:opacity-100 transition-opacity">
        <IconButton
          icon="code"
          title="Open in editor"
          onClick={() =>
            openInEditor(row.projectId).catch((err) =>
              pushToast("error", `Open failed: ${String(err)}`),
            )
          }
        />
        <IconButton icon="term" title="Open terminal" onClick={openTerminal} disabled={!project} />
        <IconButton
          icon="sync"
          title="Sync: stash → pull --rebase → push → stash pop"
          onClick={onSync}
          disabled={syncing}
        />
      </span>
    </div>
  );
}

function GitBadges({ row }: { row: HomeProject }) {
  return (
    <span className="flex items-center gap-2 font-mono text-[10px]">
      {row.dirty > 0 && (
        <span className="text-warn" title={`${row.dirty} uncommitted`}>
          ●{row.dirty}
        </span>
      )}
      {row.ahead > 0 && (
        <span className="text-accent" title={`${row.ahead} ahead`}>
          ↑{row.ahead}
        </span>
      )}
      {row.behind > 0 && (
        <span className="text-info" title={`${row.behind} behind`}>
          ↓{row.behind}
        </span>
      )}
    </span>
  );
}

function RecentCommits({
  commits,
  loading,
}: {
  commits: HomeCommit[];
  loading: boolean;
}) {
  const setSelectedProjectId = useUiStore((s) => s.setSelectedProjectId);
  if (loading && commits.length === 0) {
    return <div className="py-[10px] font-mono text-[11px] text-text-dimmer">loading…</div>;
  }
  if (commits.length === 0) {
    return (
      <div className="py-[10px] font-mono text-[11px] text-text-dimmer">
        No commits by you in this window.
      </div>
    );
  }
  return (
    <div className="flex flex-col">
      {commits.map((c) => (
        <div
          key={`${c.projectId}-${c.sha}`}
          className="flex items-center gap-2 h-[30px] border-b border-line-soft min-w-0"
        >
          <LangDot color={c.projectColor} size={7} />
          <button
            type="button"
            onClick={() => setSelectedProjectId(c.projectId)}
            className="font-mono text-[10px] text-text-dim hover:text-text truncate max-w-[110px] text-left"
            title={c.projectName}
          >
            {c.projectName}
          </button>
          <span className="text-[12px] text-text truncate min-w-0" title={c.summary}>
            {c.summary || "(no message)"}
          </span>
          <div className="flex-1" />
          {(c.additions != null || c.deletions != null) && (
            <span className="font-mono text-[10px] whitespace-nowrap">
              <span style={{ color: "var(--accent)" }}>+{c.additions ?? 0}</span>{" "}
              <span className="text-danger">−{c.deletions ?? 0}</span>
            </span>
          )}
          <span className="font-mono text-[10px] text-text-dimmer whitespace-nowrap">
            {c.sha}
          </span>
          <span
            className="font-mono text-[10px] text-text-dimmer whitespace-nowrap w-[56px] text-right"
            title={new Date(c.authoredAt).toLocaleString()}
          >
            {relTime(c.authoredAt)}
          </span>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Primitives

function Panel({
  title,
  hint,
  trailing,
  children,
}: {
  title: string;
  hint?: string;
  trailing?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-[6px] border border-line bg-surface px-[14px] py-[10px] min-w-0">
      <div className="flex items-center gap-2 mb-[6px]">
        <div className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          {title}
        </div>
        {hint && <div className="font-mono text-[10px] text-text-dimmer">· {hint}</div>}
        {trailing && <div className="ml-auto">{trailing}</div>}
      </div>
      {children}
    </section>
  );
}

function IconButton({
  icon,
  title,
  onClick,
  disabled,
}: {
  icon: IconName;
  title: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={title}
      className="inline-flex items-center justify-center w-[22px] h-[20px] rounded-[3px] border border-line text-text-dim hover:text-text hover:bg-row-active disabled:opacity-40 disabled:cursor-not-allowed"
    >
      <Icon name={icon} size={11} />
    </button>
  );
}

function fmtHours(minutes: number): string {
  if (minutes <= 0) return "0h";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return `${h}h ${pad2(m)}m`;
}

function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

function relTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const diff = Date.now() - d.getTime();
  const mins = Math.floor(diff / 60_000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 14) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 9) return `${weeks}w`;
  return d.toISOString().slice(0, 10);
}
