// Shared UI atoms for the Atlas Pilot window.

import type { ButtonHTMLAttributes, ReactNode } from "react";
import type { EpicStatus, PilotStatus } from "./ipc";

type BtnVariant = "primary" | "default" | "danger" | "ghost";

export function Btn({
  variant = "default",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: BtnVariant }) {
  const base =
    "inline-flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5 " +
    "text-xs font-medium transition-colors disabled:opacity-40 " +
    "disabled:cursor-not-allowed";
  const variants: Record<BtnVariant, string> = {
    primary: "bg-accent text-accent-fg hover:opacity-90",
    default: "bg-surface-2 border border-line text-text hover:border-text-dimmer",
    danger: "bg-surface-2 border border-line text-danger hover:border-danger",
    ghost: "text-text-dim hover:text-text",
  };
  return (
    <button className={`${base} ${variants[variant]} ${className}`} {...props} />
  );
}

export function Card({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`rounded-lg border border-line bg-surface ${className}`}
    >
      {children}
    </div>
  );
}

/** A small status badge. `tone` picks the colour. */
export function Pill({
  tone = "neutral",
  children,
}: {
  tone?: "neutral" | "accent" | "warn" | "info" | "danger" | "ok";
  children: ReactNode;
}) {
  const tones: Record<string, string> = {
    neutral: "bg-surface-2 text-text-dim",
    accent: "bg-surface-2 text-accent",
    warn: "bg-warn-bg text-warn",
    info: "bg-surface-2 text-info",
    danger: "bg-surface-2 text-danger",
    ok: "bg-surface-2 text-info",
  };
  return (
    <span
      className={`inline-flex items-center rounded px-1.5 py-0.5 text-2xs ` +
        `font-medium uppercase tracking-wide ${tones[tone]}`}
    >
      {children}
    </span>
  );
}

export function epicTone(status: EpicStatus): Parameters<typeof Pill>[0]["tone"] {
  switch (status) {
    case "done":
      return "ok";
    case "active":
      return "accent";
    case "interrupted":
      return "danger";
    default:
      return "neutral";
  }
}

export function pilotTone(status: PilotStatus): Parameters<typeof Pill>[0]["tone"] {
  switch (status) {
    case "done":
      return "ok";
    case "active":
      return "accent";
    default:
      return "warn";
  }
}

/** Format an ISO-8601 timestamp for the history timeline. */
export function fmtTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
