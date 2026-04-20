import { Icon, type IconName } from "../Icon";

// Atlas - Inspector tab loading / empty / error primitives.

interface TabSkeletonProps {
  // Number of placeholder rows. Default 4 feels right for the average
  rows?: number;
}

export function TabSkeleton({ rows = 4 }: TabSkeletonProps) {
  const blocks = Array.from({ length: rows });
  return (
    <div
      role="status"
      aria-label="Loading"
      aria-busy="true"
      className="p-[14px] flex flex-col gap-[8px]"
    >
      {blocks.map((_, i) => (
        <div
          key={i}
          className="animate-pulse rounded-[4px]"
          style={{
            // Vary widths slightly so the stack doesn't look like a bar chart.
            width: `${90 - (i % 3) * 8}%`,
            height: 16,
            background: "var(--surface-2)",
          }}
        />
      ))}
    </div>
  );
}

interface TabEmptyProps {
  /** Icon name from the shared icon set. Omit for a text-only empty state. */
  icon?: IconName;
  /** Short headline - e.g. "No notes yet". */
  title: string;
  /** Optional hint line - often includes a keyboard shortcut. */
  hint?: string | null;
}

export function TabEmpty({ icon, title, hint }: TabEmptyProps) {
  return (
    <div
      className="h-full flex flex-col items-center justify-center gap-[10px] px-[20px] py-[40px] text-center"
      role="status"
    >
      {icon && (
        <Icon name={icon} size={22} stroke="var(--text-dimmer)" />
      )}
      <div className="text-[12px] text-text-dim">{title}</div>
      {hint && (
        <div className="text-[11px] font-mono text-text-dimmer opacity-90">
          {hint}
        </div>
      )}
    </div>
  );
}

interface TabErrorProps {
  // Human-readable error detail. We keep this verbose because inspector
  message: string;
  /** Optional retry handler. Shown as a secondary button when provided. */
  onRetry?: () => void;
}

export function TabError({ message, onRetry }: TabErrorProps) {
  return (
    <div
      role="alert"
      className="m-[14px] p-[12px] rounded-[5px] flex flex-col gap-[8px]"
      style={{
        border: "1px solid var(--danger)",
        background: "oklch(0.66 0.19 25 / 0.10)",
        color: "var(--danger)",
      }}
    >
      <div className="flex items-start gap-[8px]">
        <span
          aria-hidden="true"
          style={{
            width: 14,
            height: 14,
            borderRadius: "50%",
            background: "var(--danger)",
            color: "var(--accent-fg)",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: 10,
            fontWeight: 700,
            flexShrink: 0,
            marginTop: 2,
          }}
        >
          !
        </span>
        <div className="flex-1 text-[12px] font-mono leading-snug break-words">
          {message}
        </div>
      </div>
      {onRetry && (
        <div className="flex justify-end">
          <button
            type="button"
            onClick={onRetry}
            className="px-[10px] py-[3px] font-mono text-[10px] rounded-[3px] font-semibold"
            style={{
              background: "var(--danger)",
              color: "var(--accent-fg)",
              border: "none",
            }}
          >
            Retry
          </button>
        </div>
      )}
    </div>
  );
}
