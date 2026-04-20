import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import { useUiStore } from "../../../state/store";
import { diskClean, diskScan, type DiskEntry, type DiskScanResult } from "../../../ipc";
import type { Project } from "../../../types";

// Atlas - Inspector / Disk tab.

interface DiskProps {
  project: Project;
}

// Sensible default palette for stacked bar segments. Re-used if the
const SEGMENT_COLORS = [
  "oklch(0.72 0.14 25)",
  "oklch(0.75 0.13 75)",
  "oklch(0.78 0.14 135)",
  "oklch(0.74 0.13 185)",
  "oklch(0.70 0.13 235)",
  "oklch(0.66 0.15 285)",
  "oklch(0.72 0.14 315)",
  "oklch(0.68 0.13 355)",
  "oklch(0.64 0.11 55)",
  "oklch(0.60 0.09 155)",
];

function formatExcess(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"] as const;
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

export function Disk({ project }: DiskProps) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const [confirmPath, setConfirmPath] = useState<string | null>(null);

  const { data, isLoading, error, refetch, isFetching } =
    useQuery<DiskScanResult>({
      queryKey: ["disk", project.id],
      queryFn: () => diskScan(project.id),
      staleTime: 30_000,
      retry: false,
    });

  const clean = useMutation({
    mutationFn: (path: string) => diskClean(project.id, path),
    onSuccess: (_res, path) => {
      queryClient.invalidateQueries({ queryKey: ["disk", project.id] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", `Cleaned ${path}`);
    },
    onError: (err) => pushToast("error", `Clean failed: ${String(err)}`),
  });

  // Trim to top 10 for the bar; the row list below shows everything.
  const stacked = useMemo(() => {
    if (!data) return [] as Array<DiskEntry & { color: string }>;
    return data.entries
      .slice(0, 10)
      .map((e, i) => ({ ...e, color: e.color || SEGMENT_COLORS[i % SEGMENT_COLORS.length] }));
  }, [data]);

  const colored = useMemo(() => {
    if (!data) return [] as Array<DiskEntry & { color: string }>;
    return data.entries.map((e, i) => ({
      ...e,
      color: e.color || SEGMENT_COLORS[i % SEGMENT_COLORS.length],
    }));
  }, [data]);

  // `project.sizeBytes` is the gitignored (source) total — the disk scan
  // walks everything. The delta is whatever `.gitignore` hides:
  // `node_modules`, `target`, build output, caches. Surfacing it here
  // makes the Disk tab self-explanatory for users coming from the list
  // who wondered why a "50 MB" project showed 16 GB of disk use.
  const totalDiskBytes = data?.totalBytes ?? 0;
  const excessBytes = Math.max(0, totalDiskBytes - project.sizeBytes);
  const showSplit = !!data && excessBytes > 50 * 1024 * 1024;

  return (
    <div className="p-[14px] overflow-y-auto h-full">
      <div className="flex items-center mb-[4px] gap-2">
        <span className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          Disk usage
        </span>
        <span
          className="font-mono text-[11px] text-text ml-2"
          title={data ? `${data.totalBytes} bytes` : "—"}
        >
          {data?.totalSize ?? "—"}
        </span>
        <div className="flex-1" />
        <button
          type="button"
          onClick={() => refetch()}
          disabled={isFetching}
          title="Rescan disk usage"
          aria-label="Rescan disk usage"
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] text-text-dim border border-line rounded-[3px] hover:text-text disabled:opacity-50"
        >
          <Icon name="arrow-up" size={9} stroke="currentColor" />
          {isFetching ? "scanning…" : "rescan"}
        </button>
      </div>

      {showSplit && (
        <div className="font-mono text-[10px] text-text-dim mb-[12px]">
          <span className="text-text-dimmer">source</span>{" "}
          <span className="text-text">{project.size}</span>
          <span className="text-text-dimmer"> · </span>
          <span className="text-text-dimmer">gitignored</span>{" "}
          <span style={{ color: "var(--warn, #d97757)" }}>
            {formatExcess(excessBytes)}
          </span>
          <span className="text-text-dimmer">
            {" "}(node_modules, build outputs, caches)
          </span>
        </div>
      )}
      {!showSplit && <div className="mb-[8px]" />}

      {/* Stacked bar */}
      {stacked.length > 0 && (
        <div
          style={{
            display: "flex",
            height: 8,
            borderRadius: 4,
            overflow: "hidden",
            marginBottom: 16,
            background: "var(--surface-2)",
          }}
        >
          {stacked.map((e) => (
            <div
              key={e.path}
              title={`${e.label}: ${e.size} (${Math.round(e.pct * 100)}%)`}
              style={{
                width: `${Math.max(e.pct * 100, 0.5)}%`,
                background: e.color,
              }}
            />
          ))}
        </div>
      )}

      {isLoading && !data && <TabSkeleton rows={5} />}

      {error && (
        <TabError
          message={error instanceof Error ? error.message : String(error)}
          onRetry={() => void refetch()}
        />
      )}

      {!isLoading && !error && (!data || data.entries.length === 0) && (
        <TabEmpty
          icon="hdd"
          title="Nothing to scan yet"
          hint="Run a build or pull dependencies to populate this view"
        />
      )}

      {colored.map((e) => (
        <DiskRow
          key={e.path}
          entry={e}
          onClean={() => setConfirmPath(e.path)}
          cleaning={clean.isPending && clean.variables === e.path}
        />
      ))}

      {confirmPath && (
        <ConfirmDialog
          path={confirmPath}
          onCancel={() => setConfirmPath(null)}
          onConfirm={() => {
            clean.mutate(confirmPath);
            setConfirmPath(null);
          }}
        />
      )}
    </div>
  );
}

function DiskRow({
  entry,
  onClean,
  cleaning,
}: {
  entry: DiskEntry & { color: string };
  onClean: () => void;
  cleaning: boolean;
}) {
  const pct = Math.max(0, Math.min(1, entry.pct));
  return (
    <div
      className="flex items-center gap-[10px] py-[8px] px-[6px] rounded-[4px] mb-[3px]"
      style={{ background: "var(--surface-2)" }}
    >
      <span
        style={{
          width: 10,
          height: 10,
          borderRadius: 2,
          background: entry.color,
          flexShrink: 0,
        }}
      />
      <div className="flex-1 min-w-0">
        <div
          className="text-[12px] text-text font-mono truncate"
          title={entry.path}
        >
          {entry.label || entry.path}
        </div>
        <div
          style={{
            marginTop: 4,
            height: 3,
            borderRadius: 2,
            background: "var(--line-soft)",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              width: `${pct * 100}%`,
              height: "100%",
              background: entry.color,
            }}
          />
        </div>
      </div>
      <span
        className="font-mono text-[11px] text-text-dim flex-shrink-0"
        style={{ minWidth: 72, textAlign: "right" }}
      >
        {entry.size}
      </span>
      {entry.cleanable ? (
        <button
          type="button"
          onClick={onClean}
          disabled={cleaning}
          title={`Move ${entry.path} to Trash`}
          className="inline-flex items-center gap-[4px] px-[8px] py-[3px] font-mono text-[10px] rounded-[3px] flex-shrink-0"
          style={{
            background: "transparent",
            border: "1px solid var(--line)",
            color: "var(--danger)",
          }}
        >
          <Icon name="trash" size={10} stroke="var(--danger)" />
          {cleaning ? "cleaning…" : "clean"}
        </button>
      ) : (
        <span style={{ width: 64, flexShrink: 0 }} />
      )}
    </div>
  );
}

function ConfirmDialog({
  path,
  onCancel,
  onConfirm,
}: {
  path: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div
      onClick={onCancel}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 410,
        background: "rgba(0,0,0,0.45)",
        backdropFilter: "blur(3px)",
        WebkitBackdropFilter: "blur(3px)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Confirm move to Trash"
        style={{
          width: 420,
          padding: 20,
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          boxShadow: "0 20px 60px rgba(0,0,0,0.4)",
          color: "var(--text)",
        }}
      >
        <div
          style={{
            fontSize: 13,
            fontWeight: 600,
            marginBottom: 8,
          }}
        >
          Move to Trash?
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-dim)",
            marginBottom: 14,
            fontFamily: "var(--mono)",
            wordBreak: "break-all",
          }}
        >
          This will move <span style={{ color: "var(--text)" }}>{path}</span>{" "}
          to the system Trash. You can restore it from Finder.
        </div>
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button
            type="button"
            onClick={onCancel}
            style={{
              padding: "6px 12px",
              fontSize: 12,
              background: "transparent",
              border: "1px solid var(--line)",
              borderRadius: 5,
              color: "var(--text)",
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            style={{
              padding: "6px 14px",
              fontSize: 12,
              background: "var(--danger)",
              color: "white",
              border: "none",
              borderRadius: 5,
              cursor: "pointer",
              fontWeight: 600,
            }}
          >
            Move to Trash
          </button>
        </div>
      </div>
    </div>
  );
}
