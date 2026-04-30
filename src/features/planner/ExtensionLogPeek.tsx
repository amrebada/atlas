import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Icon } from "../../components/Icon";
import { plannerExtensionLog } from "../../ipc";
import type { ExtensionEvent } from "../../types";

const REASON_TONE: Record<string, string> = {
  "auto-missed": "var(--err, #ef4444)",
  "user-soften": "var(--warn, #f59e0b)",
  "user-override": "var(--warn, #f59e0b)",
  paused: "var(--text-dim)",
};

/** Compact rolling list of the most recent extension events across all
 *  milestones + routines. Collapsed by default so it doesn't crowd the
 *  Today modal; expanding shows the latest 5 with reason + cost. */
export function ExtensionLogPeek() {
  const [open, setOpen] = useState(false);
  const { data } = useQuery<ExtensionEvent[]>({
    queryKey: ["planner-extension-log"],
    queryFn: () => plannerExtensionLog({}),
    staleTime: 30_000,
    retry: false,
  });

  const events = (data ?? []).slice(0, 5);
  const totalCost = (data ?? []).reduce(
    (acc, e) => acc + (e.failingPointsApplied ?? 0),
    0,
  );

  if (!data || data.length === 0) return null;

  return (
    <section className="border-t border-line">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-2 px-[18px] py-[8px] hover:bg-surface-2/40"
      >
        <Icon
          name={open ? "chevron-d" : "chevron"}
          size={11}
          stroke="var(--text-dim)"
        />
        <span className="text-[10px] font-mono uppercase tracking-[0.5px] text-text-dim flex-1 text-left">
          Recent extensions ({data.length})
        </span>
        {totalCost > 0 && (
          <span className="font-mono text-[10px] text-warn">
            -{Math.round(totalCost)} pts total
          </span>
        )}
      </button>
      {open && (
        <ul className="px-[18px] pb-[10px]">
          {events.map((e, i) => (
            <li
              key={`${e.at}-${i}`}
              className="text-[11px] font-mono py-[3px] border-b border-line-soft last:border-b-0"
            >
              <div className="flex items-center gap-2">
                <span className="text-text-dim">{e.from}</span>
                <span className="text-text-dimmer">→</span>
                <span className="text-text">{e.to}</span>
                <span
                  className="ml-auto text-[10px] uppercase"
                  style={{ color: REASON_TONE[e.reason] ?? "var(--text-dim)" }}
                >
                  {e.reason}
                </span>
                {e.failingPointsApplied > 0 && (
                  <span className="text-warn">
                    -{Math.round(e.failingPointsApplied)}
                  </span>
                )}
              </div>
              {e.note && (
                <div className="text-[10px] italic text-text-dim mt-[1px]">
                  {e.note}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
