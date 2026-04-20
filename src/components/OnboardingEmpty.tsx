import { Icon, Kbd } from "./Icon";
import { useUiStore } from "../state/store";

// Atlas - onboarding empty state.
export function OnboardingEmpty() {
  const openSettings = useUiStore((s) => s.openSettings);
  const openNewProject = useUiStore((s) => s.openNewProject);

  return (
    <div
      className="flex-1 flex items-center justify-center p-10 font-sans text-text"
      style={{ fontFamily: "var(--sans)" }}
    >
      <div style={{ width: 520, textAlign: "center" }}>
        {/* Hero icon — dashed frame + accent folder + plus badge. Mirrors
            the prototype so visual regressions across iters stay at zero. */}
        <div
          className="mx-auto mb-[22px] flex items-center justify-center relative"
          style={{
            width: 72,
            height: 72,
            border: "1px dashed var(--line)",
            borderRadius: 14,
          }}
        >
          <Icon name="folder" size={30} stroke="var(--accent)" />
          <div
            className="absolute flex items-center justify-center"
            style={{
              right: -6,
              bottom: -6,
              width: 22,
              height: 22,
              borderRadius: "50%",
              background: "var(--accent)",
              color: "var(--accent-fg)",
            }}
          >
            <Icon name="plus" size={12} stroke="currentColor" />
          </div>
        </div>

        <h2
          className="text-[22px] font-semibold"
          style={{ margin: "0 0 8px", letterSpacing: -0.3 }}
        >
          Welcome to Atlas
        </h2>
        <div
          className="text-[13px] text-text-dim"
          style={{ marginBottom: 26, lineHeight: 1.55 }}
        >
          A quiet home for all your local repos. Atlas watches a folder, picks
          up git projects, and gives you one keystroke to get back into any of
          them.
        </div>

        <div
          className="grid mb-[26px]"
          style={{ gridTemplateColumns: "1fr 1fr", gap: 10 }}
        >
          <OnbCard
            icon="folder"
            title="Add a watch folder"
            hint="Point Atlas at ~/code — we'll scan for .git and keep watching for new repos."
            primary
            onClick={() => openSettings("watchers")}
          />
          <OnbCard
            icon="plus"
            title="New project"
            hint="Start from a template: Node · Rust · Python · Go"
            onClick={() => openNewProject("new")}
          />
          <OnbCard
            icon="clone"
            title="Clone from URL"
            hint="Paste any https or ssh git URL"
            onClick={() => openNewProject("clone")}
          />
          <OnbCard
            icon="import"
            title="Import folder"
            hint="Point at a single folder you already have"
            onClick={() => openNewProject("import")}
          />
        </div>

        <div
          className="flex items-center justify-center gap-[14px] text-[11px] text-text-dim"
          style={{ fontFamily: "var(--mono)" }}
        >
          <span>
            press <Kbd>⌘</Kbd>
            <Kbd>K</Kbd> any time
          </span>
        </div>
      </div>
    </div>
  );
}

// One action card in the onboarding grid. `primary` swaps the border and
function OnbCard({
  icon,
  title,
  hint,
  primary,
  onClick,
}: {
  icon: React.ComponentProps<typeof Icon>["name"];
  title: string;
  hint: string;
  primary?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="text-left cursor-pointer"
      style={{
        padding: 16,
        border: "1px solid " + (primary ? "var(--accent)" : "var(--line)"),
        background: primary ? "var(--row-active)" : "var(--surface)",
        borderRadius: 8,
        color: "var(--text)",
        fontFamily: "inherit",
      }}
    >
      <div
        className="flex items-center"
        style={{ gap: 8, marginBottom: 6 }}
      >
        <Icon
          name={icon}
          size={14}
          stroke={primary ? "var(--accent)" : "var(--text-dim)"}
        />
        <span style={{ fontSize: 13, fontWeight: 500 }}>{title}</span>
      </div>
      <div
        className="text-text-dim"
        style={{ fontSize: 11, lineHeight: 1.5 }}
      >
        {hint}
      </div>
    </button>
  );
}
