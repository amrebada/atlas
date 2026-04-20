import type { CSSProperties, SVGProps } from "react";

// Atlas - shared icon set.

export type IconName =
  | "folder"
  | "folder-fill"
  | "file"
  | "search"
  | "pin"
  | "pin-fill"
  | "plus"
  | "git"
  | "branch"
  | "clock"
  | "check"
  | "square"
  | "square-check"
  | "term"
  | "code"
  | "gear"
  | "list"
  | "grid"
  | "tag"
  | "trash"
  | "play"
  | "stop"
  | "clone"
  | "import"
  | "arrow-up"
  | "arrow-down"
  | "chevron"
  | "chevron-d"
  | "dot"
  | "archive"
  | "arch"
  | "copy"
  | "more"
  | "package"
  | "sparkle"
  | "cmd"
  | "note"
  | "hdd";

interface IconProps {
  name: IconName;
  size?: number;
  stroke?: string;
  strokeWidth?: number;
  fill?: string;
  className?: string;
  style?: CSSProperties;
}

export function Icon({
  name,
  size = 14,
  stroke = "currentColor",
  strokeWidth = 1.5,
  className,
  style,
}: IconProps) {
  // `flexShrink: 0` keeps icons from being squeezed by long sibling text
  const p: SVGProps<SVGSVGElement> = {
    width: size,
    height: size,
    viewBox: "0 0 16 16",
    fill: "none",
    stroke,
    strokeWidth,
    strokeLinecap: "round",
    strokeLinejoin: "round",
    className,
    style: { flexShrink: 0, ...style },
  };

  switch (name) {
    case "folder":
      return (
        <svg {...p}>
          <path d="M1.5 4.5a1 1 0 011-1h3l1.5 1.5h6a1 1 0 011 1v6a1 1 0 01-1 1h-10.5a1 1 0 01-1-1v-6.5z" />
        </svg>
      );
    case "folder-fill":
      return (
        <svg {...p} fill={stroke}>
          <path d="M1.5 4.5a1 1 0 011-1h3l1.5 1.5h6a1 1 0 011 1v6a1 1 0 01-1 1h-10.5a1 1 0 01-1-1v-6.5z" />
        </svg>
      );
    case "file":
      return (
        <svg {...p}>
          <path d="M3.5 1.5h5l4 4v9a1 1 0 01-1 1h-8a1 1 0 01-1-1v-12a1 1 0 011-1z" />
          <path d="M8.5 1.5v4h4" />
        </svg>
      );
    case "search":
      return (
        <svg {...p}>
          <circle cx="7" cy="7" r="4.5" />
          <path d="M10.5 10.5l3.5 3.5" />
        </svg>
      );
    // Pin and pin-fill share the same 16×16 polygon geometry so the two
    case "pin":
      return (
        <svg {...p}>
          <polygon
            points="13 8 11 6 11 3 12 3 12 1 4 1 4 3 5 3 5 6 3 8 3 10 7.3 10 7.3 16 8.7 16 8.7 10 13 10 13 8"
            fill="none"
          />
        </svg>
      );
    case "pin-fill":
      return (
        <svg {...p} fill={stroke}>
          <polygon points="13 8 11 6 11 3 12 3 12 1 4 1 4 3 5 3 5 6 3 8 3 10 7.3 10 7.3 16 8.7 16 8.7 10 13 10 13 8" />
        </svg>
      );
    case "plus":
      return (
        <svg {...p}>
          <path d="M8 3v10M3 8h10" />
        </svg>
      );
    case "git":
      return (
        <svg {...p}>
          <circle cx="4" cy="4" r="1.5" />
          <circle cx="4" cy="12" r="1.5" />
          <circle cx="12" cy="8" r="1.5" />
          <path d="M4 5.5v5M5.5 4h4a2 2 0 012 2v.5" />
        </svg>
      );
    case "branch":
      return (
        <svg {...p}>
          <circle cx="4" cy="3" r="1.5" />
          <circle cx="4" cy="13" r="1.5" />
          <circle cx="12" cy="7" r="1.5" />
          <path d="M4 4.5v7M5.5 3h4.5a2 2 0 012 2v.5" />
        </svg>
      );
    case "clock":
      return (
        <svg {...p}>
          <circle cx="8" cy="8" r="6" />
          <path d="M8 5v3.2l2 1.2" />
        </svg>
      );
    case "check":
      return (
        <svg {...p}>
          <path d="M3 8.5l3 3 7-7" />
        </svg>
      );
    case "square":
      return (
        <svg {...p}>
          <rect x="3" y="3" width="10" height="10" rx="2" />
        </svg>
      );
    case "square-check":
      return (
        <svg {...p}>
          <rect x="3" y="3" width="10" height="10" rx="2" />
          <path d="M6 8.5l1.5 1.5 3-3.5" />
        </svg>
      );
    case "term":
      return (
        <svg {...p}>
          <rect x="1.5" y="3" width="13" height="10" rx="1" />
          <path d="M4 7l2 1.5-2 1.5M8 10.5h3" />
        </svg>
      );
    case "code":
      return (
        <svg {...p}>
          <path d="M5.5 4.5L2 8l3.5 3.5M10.5 4.5L14 8l-3.5 3.5M9.5 3l-3 10" />
        </svg>
      );
    case "gear":
      return (
        <svg {...p}>
          <circle cx="8" cy="8" r="2" />
          <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.3 3.3l1.4 1.4M11.3 11.3l1.4 1.4M3.3 12.7l1.4-1.4M11.3 4.7l1.4-1.4" />
        </svg>
      );
    case "list":
      return (
        <svg {...p}>
          <path d="M5 4.5h9M5 8h9M5 11.5h9M2 4.5h.5M2 8h.5M2 11.5h.5" />
        </svg>
      );
    case "grid":
      return (
        <svg {...p}>
          <rect x="2.5" y="2.5" width="4" height="4" />
          <rect x="9.5" y="2.5" width="4" height="4" />
          <rect x="2.5" y="9.5" width="4" height="4" />
          <rect x="9.5" y="9.5" width="4" height="4" />
        </svg>
      );
    case "tag":
      return (
        <svg {...p}>
          <path d="M2 7.5v-4a1 1 0 011-1h4l7 7-5 5z" />
          <circle cx="5" cy="5.5" r="0.7" fill={stroke} />
        </svg>
      );
    case "trash":
      return (
        <svg {...p}>
          <path d="M2.5 4h11M6 4v-1a1 1 0 011-1h2a1 1 0 011 1v1M4 4l.5 9a1 1 0 001 1h5a1 1 0 001-1l.5-9" />
        </svg>
      );
    case "play":
      return (
        <svg {...p} fill={stroke} stroke="none">
          <path d="M4 3l9 5-9 5z" />
        </svg>
      );
    case "stop":
      return (
        <svg {...p} fill={stroke} stroke="none">
          <rect x="4" y="4" width="8" height="8" rx="1" />
        </svg>
      );
    case "clone":
      return (
        <svg {...p}>
          <rect x="5" y="5" width="9" height="9" rx="1" />
          <path d="M3 10.5V3a1 1 0 011-1h7" />
        </svg>
      );
    case "import":
      return (
        <svg {...p}>
          <path d="M8 2v8M5 7l3 3 3-3M2.5 13h11" />
        </svg>
      );
    case "arrow-up":
      return (
        <svg {...p}>
          <path d="M8 13V3M4.5 6.5L8 3l3.5 3.5" />
        </svg>
      );
    case "arrow-down":
      return (
        <svg {...p}>
          <path d="M8 3v10M4.5 9.5L8 13l3.5-3.5" />
        </svg>
      );
    case "chevron":
      return (
        <svg {...p}>
          <path d="M6 4l4 4-4 4" />
        </svg>
      );
    case "chevron-d":
      return (
        <svg {...p}>
          <path d="M4 6l4 4 4-4" />
        </svg>
      );
    case "dot":
      return (
        <svg {...p} fill={stroke} stroke="none">
          <circle cx="8" cy="8" r="2.5" />
        </svg>
      );
    case "archive":
    case "arch":
      return (
        <svg {...p}>
          <rect x="2" y="3" width="12" height="3" rx="0.5" />
          <path d="M3 6v7a1 1 0 001 1h8a1 1 0 001-1V6M6.5 9h3" />
        </svg>
      );
    case "copy":
      return (
        <svg {...p}>
          <rect x="5" y="5" width="9" height="9" rx="1" />
          <path d="M3 10.5V3a1 1 0 011-1h7" />
        </svg>
      );
    case "more":
      return (
        <svg {...p} fill={stroke} stroke="none">
          <circle cx="4" cy="8" r="1" />
          <circle cx="8" cy="8" r="1" />
          <circle cx="12" cy="8" r="1" />
        </svg>
      );
    case "package":
      return (
        <svg {...p}>
          <path d="M8 2l5.5 3v6L8 14l-5.5-3V5z" />
          <path d="M2.5 5L8 8l5.5-3M8 8v6" />
        </svg>
      );
    case "sparkle":
      return (
        <svg {...p}>
          <path d="M8 2l1.5 4.5L14 8l-4.5 1.5L8 14l-1.5-4.5L2 8l4.5-1.5z" />
        </svg>
      );
    case "cmd":
      return (
        <svg {...p}>
          <path
            d="M5.5 3.5a2 2 0 110 4h-2M10.5 3.5a2 2 0 100 4h2M5.5 12.5a2 2 0 100-4h-2M10.5 12.5a2 2 0 110-4h2M5.5 7.5h5v1h-5z"
            fill={stroke}
          />
        </svg>
      );
    case "note":
      return (
        <svg {...p}>
          <path d="M3 2.5h8l2 2v9a1 1 0 01-1 1h-9a1 1 0 01-1-1v-10a1 1 0 011-1z" />
          <path d="M5 6.5h6M5 9h6M5 11.5h4" />
        </svg>
      );
    case "hdd":
      return (
        <svg {...p}>
          <rect x="1.5" y="4" width="13" height="8" rx="1" />
          <circle cx="11.5" cy="8" r="0.8" fill={stroke} />
        </svg>
      );
    default:
      return (
        <svg {...p}>
          <circle cx="8" cy="8" r="6" />
        </svg>
      );
  }
}

/** Mac traffic-light dots (visual only, no close/minimize wiring). */
export function TrafficLights() {
  const dot = (bg: string) => (
    <span
      style={{
        width: 12,
        height: 12,
        borderRadius: "50%",
        background: bg,
        border: "0.5px solid rgba(0,0,0,0.12)",
        display: "inline-block",
      }}
    />
  );
  return (
    <div className="flex items-center gap-2">
      {dot("#ff5f57")}
      {dot("#febc2e")}
      {dot("#28c840")}
    </div>
  );
}

/** Keyboard key badge, e.g. <Kbd>⌘</Kbd><Kbd>K</Kbd>. */
export function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex min-w-[18px] h-[18px] items-center justify-center px-[5px] border border-line rounded bg-kbd-bg font-mono text-[11px] font-medium text-text-dim">
      {children}
    </span>
  );
}

/** Solid coloured dot - Lang indicator next to project rows. */
export function LangDot({
  color,
  size = 8,
}: {
  color: string;
  size?: number;
}) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        background: color,
        flexShrink: 0,
        display: "inline-block",
      }}
    />
  );
}
