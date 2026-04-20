import { Icon, type IconName } from "../Icon";
import { useUiStore, type InspectorTab } from "../../state/store";

// Atlas - Inspector tab strip.

interface TabSpec {
  id: InspectorTab;
  icon: IconName;
  label: string;
}

const TABS: TabSpec[] = [
  { id: "overview", icon: "sparkle", label: "Overview" },
  { id: "files", icon: "file", label: "Files" },
  { id: "sessions", icon: "term", label: "Sessions" },
  { id: "scripts", icon: "play", label: "Scripts" },
  { id: "todos", icon: "check", label: "Todos" },
  { id: "notes", icon: "note", label: "Notes" },
  { id: "disk", icon: "hdd", label: "Disk" },
];

export function TabStrip() {
  const active = useUiStore((s) => s.activeInspectorTab);
  const setActive = useUiStore((s) => s.setActiveInspectorTab);

  return (
    <div
      role="tablist"
      aria-label="Inspector"
      className="flex border-b border-line shrink-0"
    >
      {TABS.map((t) => {
        const isActive = t.id === active;
        return (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            aria-label={t.label}
            title={t.label}
            onClick={() => setActive(t.id)}
            className="flex-1 h-8 inline-flex items-center justify-center transition-colors"
            style={{
              background: "transparent",
              color: isActive ? "var(--accent)" : "var(--text-dim)",
              boxShadow: isActive ? "inset 0 -2px 0 var(--accent)" : "none",
            }}
            onMouseEnter={(e) => {
              if (!isActive) e.currentTarget.style.color = "var(--text)";
            }}
            onMouseLeave={(e) => {
              if (!isActive) e.currentTarget.style.color = "var(--text-dim)";
            }}
          >
            <Icon
              name={t.icon}
              size={14}
              stroke={isActive ? "var(--accent)" : "currentColor"}
            />
          </button>
        );
      })}
    </div>
  );
}
