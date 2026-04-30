import { useQuery } from "@tanstack/react-query";

import { plannerScoreSummary } from "../../ipc";
import type { ScoreSummary } from "../../types";

/** Lifetime + rolling-30d success-rate dial. Renders a single SVG arc
 *  + two stat columns. Used inside the Today modal so the user has
 *  one glance-able answer to "am I shipping?". */
export function SuccessRateDial() {
  const { data } = useQuery<ScoreSummary>({
    queryKey: ["planner-score-summary", null, 30],
    queryFn: () => plannerScoreSummary(null, 30),
    staleTime: 30_000,
    retry: false,
  });

  if (!data) return null;

  const lifetime = data.lifetime.successRate;
  const rolling = data.rolling30d.successRate;
  // Ring shows the *rolling* number — that's the one users can move.
  const ringPct = clamp01(rolling);

  return (
    <section className="px-[18px] py-[12px] border-t border-line">
      <div className="text-[9px] font-mono uppercase tracking-[0.5px] text-text-dim mb-2">
        Success rate
      </div>
      <div className="flex items-center gap-4">
        <Ring pct={ringPct} />
        <div className="flex flex-col gap-1">
          <Row
            label="rolling 30d"
            rate={rolling}
            success={data.rolling30d.successPoints}
            fail={data.rolling30d.failingPoints}
          />
          <Row
            label="lifetime"
            rate={lifetime}
            success={data.lifetime.successPoints}
            fail={data.lifetime.failingPoints}
          />
        </div>
      </div>
    </section>
  );
}

function Ring({ pct }: { pct: number }) {
  const size = 64;
  const stroke = 8;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const dash = c * pct;
  const tone = colorFor(pct);

  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="shrink-0">
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke="var(--line)"
        strokeWidth={stroke}
      />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={tone}
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={`${dash} ${c - dash}`}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
      <text
        x="50%"
        y="50%"
        dominantBaseline="middle"
        textAnchor="middle"
        fontSize="14"
        fontWeight="600"
        fontFamily="ui-monospace, monospace"
        fill="var(--text)"
      >
        {Math.round(pct * 100)}%
      </text>
    </svg>
  );
}

function Row({
  label,
  rate,
  success,
  fail,
}: {
  label: string;
  rate: number;
  success: number;
  fail: number;
}) {
  return (
    <div className="flex items-center gap-2 text-[11px] font-mono">
      <span className="text-text-dim w-[80px]">{label}</span>
      <span className="font-semibold" style={{ color: colorFor(rate), minWidth: 36 }}>
        {Math.round(rate * 100)}%
      </span>
      <span className="text-text-dim">
        +{Math.round(success)} / -{Math.round(fail)}
      </span>
    </div>
  );
}

function clamp01(v: number): number {
  if (Number.isNaN(v)) return 1;
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

function colorFor(pct: number): string {
  if (pct >= 0.85) return "var(--accent)";
  if (pct >= 0.5) return "var(--warn, #f59e0b)";
  return "var(--err, #ef4444)";
}
