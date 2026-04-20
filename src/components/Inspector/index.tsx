import { useMutation } from "@tanstack/react-query";
import { Icon, LangDot } from "../Icon";
import { useUiStore } from "../../state/store";
import { openInEditor, revealInFinder, terminalOpen } from "../../ipc";
import { useTerminalStore, makePane } from "../../features/terminal/layout";
import type { Project } from "../../types";
import { TabStrip } from "./TabStrip";
import { Overview } from "./tabs/Overview";
import { Files } from "./tabs/Files";
import { Sessions } from "./tabs/Sessions";
import { Scripts } from "./tabs/Scripts";
import { Todos } from "./tabs/Todos";
import { Notes } from "./tabs/Notes";
import { Disk } from "./tabs/Disk";

interface InspectorProps {
  project: Project | null;
}

// Atlas - inspector panel.

export function Inspector({ project }: InspectorProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  const activeTab = useUiStore((s) => s.activeInspectorTab);

  const openMut = useMutation({
    mutationFn: (id: string) => openInEditor(id),
    onError: (e) => pushToast("error", `Open in editor failed: ${String(e)}`),
  });
  const revealMut = useMutation({
    mutationFn: (id: string) => revealInFinder(id),
    onError: (e) => pushToast("error", `Reveal failed: ${String(e)}`),
  });

  if (!project) {
    return (
      <aside className="border-l border-line bg-chrome flex items-center justify-center">
        <div className="flex flex-col items-center gap-2 text-text-dimmer">
          <Icon name="sparkle" size={20} stroke="var(--text-dimmer)" />
          <span className="text-xs">Select a project to see details</span>
        </div>
      </aside>
    );
  }

  return (
    <aside className="border-l border-line bg-chrome flex flex-col overflow-hidden min-h-0">
      {/* Project header. */}
      <div className="p-[14px_16px_12px] border-b border-line shrink-0">
        <div className="flex items-center gap-2 mb-[6px]">
          <LangDot color={project.color} size={10} />
          <span className="font-semibold text-[14px] flex-1 truncate">
            {project.name}
          </span>
          {project.pinned && (
            <span
              title="Pinned"
              className="inline-flex items-center justify-center"
            >
              <Icon name="pin-fill" size={12} stroke="var(--accent)" />
            </span>
          )}
        </div>
        <div className="text-[11px] font-mono text-text-dim mb-[10px] truncate">
          {project.path}
        </div>
        <div className="flex gap-[6px]">
          <button
            onClick={() => openMut.mutate(project.id)}
            disabled={openMut.isPending}
            title="Open in editor (⌘E)"
            aria-label="Open in editor"
            className="flex-1 inline-flex items-center justify-center gap-[6px] h-[26px] px-[10px] bg-accent text-accent-fg rounded-[5px] font-semibold text-xs hover:opacity-90 disabled:opacity-60"
          >
            <Icon name="code" size={12} stroke="var(--accent-fg)" />
            {openMut.isPending ? "Opening…" : "Open in editor"}
          </button>
          <button
            onClick={async () => {
              try {
                const id = await terminalOpen({
                  kind: "shell",
                  cwd: project.path,
                });
                useTerminalStore
                  .getState()
                  .addPane(
                    makePane(id, "shell", project.path, project.name, {
                      branch: project.branch,
                      projectId: project.id,
                      projectLabel: project.name,
                    }),
                  );
              } catch (err) {
                // eslint-disable-next-line no-console
                console.error("[atlas] terminal_open (shell) failed:", err);
                pushToast("error", `Open terminal failed: ${String(err)}`);
              }
            }}
            title="Open terminal"
            aria-label="Open terminal"
            className="w-[26px] h-[26px] inline-flex items-center justify-center bg-surface-2 border border-line rounded-[5px] text-text-dim hover:text-text"
          >
            <Icon name="term" size={12} />
          </button>
          <button
            onClick={() => revealMut.mutate(project.id)}
            disabled={revealMut.isPending}
            title="Reveal in Finder"
            aria-label="Reveal in Finder"
            className="w-[26px] h-[26px] inline-flex items-center justify-center bg-surface-2 border border-line rounded-[5px] text-text-dim hover:text-text disabled:opacity-60"
          >
            <Icon name="folder" size={12} />
          </button>
        </div>
      </div>

      <TabStrip />

      {/* Tab body — owns its own scroll so the header + tabs stay pinned. */}
      <div className="flex-1 min-h-0 overflow-hidden">
        {activeTab === "overview" && <Overview project={project} />}
        {activeTab === "files" && <Files project={project} />}
        {activeTab === "sessions" && <Sessions project={project} />}
        {activeTab === "scripts" && <Scripts project={project} />}
        {activeTab === "todos" && <Todos project={project} />}
        {activeTab === "notes" && <Notes project={project} />}
        {activeTab === "disk" && <Disk project={project} />}
      </div>
    </aside>
  );
}
