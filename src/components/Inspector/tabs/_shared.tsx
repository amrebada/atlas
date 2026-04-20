import type { ReactNode } from "react";
import { Icon, type IconName } from "../../Icon";

// Atlas - small inspector primitives shared across tabs.

interface SectionProps {
  title: string;
  children: ReactNode;
  /** Optional right-side adornment (e.g. inline action). */
  trailing?: ReactNode;
}

export function Section({ title, children, trailing }: SectionProps) {
  return (
    <div className="mb-[18px]">
      <div className="flex items-center mb-1">
        <div className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          {title}
        </div>
        {trailing && <div className="ml-auto">{trailing}</div>}
      </div>
      {children}
    </div>
  );
}

interface EmptyProps {
  label: string;
  sub?: string | null;
  icon?: IconName;
}

export function Empty({ label, sub, icon }: EmptyProps) {
  return (
    <div className="py-[30px] text-center text-text-dimmer flex flex-col items-center gap-[10px]">
      {icon && <Icon name={icon} size={20} stroke="var(--text-dimmer)" />}
      <div className="text-[12px]">{label}</div>
      {sub && <div className="text-[11px] font-mono opacity-90">{sub}</div>}
    </div>
  );
}

/** Centered full-pane empty state used by stub tabs (Sessions/Notes/Disk). */
export function StubTab({ icon, label }: { icon: IconName; label: string }) {
  return (
    <div className="h-full flex items-center justify-center">
      <Empty label={label} icon={icon} />
    </div>
  );
}

/** Two-column k:v row used by Overview > Stats (mono right column). */
export function StatLine({
  label,
  value,
  mono = true,
}: {
  label: string;
  value: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="flex justify-between items-center py-[6px] border-b border-line-soft text-[12px]">
      <span className="text-text-dim">{label}</span>
      <span
        className={`text-text ${mono ? "font-mono" : "font-sans"}`}
        style={{ overflow: "hidden", textOverflow: "ellipsis" }}
      >
        {value}
      </span>
    </div>
  );
}
