import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Icon, LangDot } from "../../Icon";
import { Section, StatLine } from "./_shared";
import { useUiStore } from "../../../state/store";
import { listTags, setProjectTags } from "../../../ipc";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { Project } from "../../../types";

// Atlas - Inspector / Overview tab.

interface OverviewProps {
  project: Project;
}

export function Overview({ project }: OverviewProps) {
  const setActiveInspectorTab = useUiStore((s) => s.setActiveInspectorTab);

  return (
    <div className="p-[14px] overflow-y-auto h-full">
      <MiniHeader project={project} />

      <Section title="Git">
        <div className="flex items-center gap-2 py-[6px] border-b border-line-soft text-[12px]">
          <Icon name="branch" size={11} stroke="var(--text-dim)" />
          <span className="text-text font-mono">{project.branch || "—"}</span>
          <span className="ml-auto flex items-center gap-2 font-mono text-[11px]">
            {project.dirty > 0 && (
              <span className="text-warn" title={`${project.dirty} uncommitted`}>
                ●{project.dirty}
              </span>
            )}
            {project.ahead > 0 && (
              <span className="text-accent" title={`${project.ahead} ahead`}>
                ↑{project.ahead}
              </span>
            )}
            {project.behind > 0 && (
              <span className="text-info" title={`${project.behind} behind`}>
                ↓{project.behind}
              </span>
            )}
            {project.dirty === 0 && project.ahead === 0 && project.behind === 0 && (
              <span className="text-text-dimmer">clean</span>
            )}
          </span>
        </div>
        {project.dirty > 0 && (
          <div className="pt-[6px]">
            <button
              type="button"
              onClick={() => setActiveInspectorTab("files")}
              className="font-mono text-[10px] text-accent hover:underline"
            >
              view diff →
            </button>
          </div>
        )}
      </Section>

      <Section title="Stats">
        <StatLine label="language" value={project.language} mono={false} />
        <StatLine label="lines of code" value={project.loc.toLocaleString()} />
        <StatLine label="size on disk" value={project.size} />
        <StatLine label="tracked time" value={project.time || "—"} />
        <StatLine
          label="last opened"
          value={formatLastOpened(project.lastOpened)}
        />
      </Section>

      <TagsSection project={project} />
    </div>
  );
}

function MiniHeader({ project }: { project: Project }) {
  // Strip the user home prefix for visual density - same trick as ProjectList.
  const shortPath = project.path.replace(/^\/Users\/[^/]+\//, "~/");
  return (
    <div className="flex items-center gap-2 mb-[14px] text-[11px]">
      <LangDot color={project.color} size={8} />
      <span className="text-text font-medium">{project.name}</span>
      <span
        className="text-text-dim font-mono truncate"
        title={project.path}
      >
        {shortPath}
      </span>
    </div>
  );
}

function TagsSection({ project }: { project: Project }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const [draft, setDraft] = useState("");
  const [adding, setAdding] = useState(false);
  const [activeSuggestion, setActiveSuggestion] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const { data: allTags = [] } = useQuery<string[]>({
    queryKey: ["tags"],
    queryFn: listTags,
    staleTime: 30_000,
    retry: false,
  });

  // Reset draft when project changes - avoids carrying half-typed text
  useEffect(() => {
    setDraft("");
    setAdding(false);
  }, [project.id]);

  const writeTags = async (next: string[]) => {
    const prev = project.tags;
    // Optimistic merge into `['projects']` so the row re-renders immediately.
    queryClient.setQueryData<Project[]>(["projects"], (old) =>
      old
        ? old.map((p) => (p.id === project.id ? { ...p, tags: next } : p))
        : old,
    );
    try {
      await setProjectTags(project.id, next);
      // Refresh the global tag list so new tags show in autocomplete
      queryClient.invalidateQueries({ queryKey: ["tags"] });
    } catch (err) {
      // Roll back the optimistic patch.
      queryClient.setQueryData<Project[]>(["projects"], (old) =>
        old
          ? old.map((p) => (p.id === project.id ? { ...p, tags: prev } : p))
          : old,
      );
      pushToast(
        "error",
        `Couldn't save tags: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  };

  // Suggestions are tags from `listTags()` that:
  const suggestions = useMemo(() => {
    const q = draft.trim().replace(/^#/, "").toLowerCase();
    const owned = new Set(project.tags);
    return allTags
      .filter((t) => !owned.has(t))
      .filter((t) => (q ? t.toLowerCase().includes(q) : true))
      .slice(0, 8);
  }, [allTags, draft, project.tags]);

  // Clamp active suggestion when the list changes under us.
  useEffect(() => {
    if (activeSuggestion >= suggestions.length) setActiveSuggestion(0);
  }, [suggestions.length, activeSuggestion]);

  const commit = (tagRaw?: string) => {
    const tag = (tagRaw ?? draft).trim().replace(/^#/, "");
    if (!tag) {
      setAdding(false);
      setDraft("");
      return;
    }
    if (project.tags.includes(tag)) {
      setDraft("");
      return;
    }
    void writeTags([...project.tags, tag]);
    setDraft("");
    setActiveSuggestion(0);
  };

  const remove = (tag: string) => {
    void writeTags(project.tags.filter((t) => t !== tag));
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (suggestions.length > 0 && activeSuggestion < suggestions.length) {
        commit(suggestions[activeSuggestion]);
      } else {
        commit();
      }
    } else if (e.key === "ArrowDown" && suggestions.length > 0) {
      e.preventDefault();
      setActiveSuggestion((i) => (i + 1) % suggestions.length);
    } else if (e.key === "ArrowUp" && suggestions.length > 0) {
      e.preventDefault();
      setActiveSuggestion(
        (i) => (i - 1 + suggestions.length) % suggestions.length,
      );
    } else if (e.key === "Escape") {
      e.preventDefault();
      setDraft("");
      setAdding(false);
    }
  };

  return (
    <Section title="Tags">
      <div className="flex flex-wrap gap-1 py-[4px]">
        {project.tags.length === 0 && !adding && (
          <span className="font-mono text-[10px] text-text-dimmer">
            No tags
          </span>
        )}
        {project.tags.map((t) => (
          <TagChip key={t} tag={t} onRemove={() => remove(t)} />
        ))}
        {adding ? (
          <div className="relative">
            <input
              ref={inputRef}
              autoFocus
              value={draft}
              onChange={(e) => {
                setDraft(e.target.value);
                setActiveSuggestion(0);
              }}
              onBlur={() => {
                // Delay so onMouseDown on a suggestion row fires first.
                setTimeout(() => {
                  commit();
                }, 120);
              }}
              onKeyDown={onKey}
              placeholder="tag name"
              className="bg-bg border border-accent rounded-[3px] px-[6px] py-[1px] outline-none text-text font-mono text-[10px] w-[110px]"
            />
            {suggestions.length > 0 && (
              <div
                className="absolute top-full left-0 mt-1 py-[2px] rounded-[4px] z-20"
                style={{
                  background: "var(--surface)",
                  border: "1px solid var(--line)",
                  minWidth: 140,
                  maxHeight: 200,
                  overflowY: "auto",
                  boxShadow:
                    "0 6px 14px rgba(0,0,0,0.3), 0 2px 4px rgba(0,0,0,0.15)",
                }}
              >
                {suggestions.map((t, i) => (
                  <div
                    key={t}
                    // Use mousedown + preventDefault so the input keeps focus
                    onMouseDown={(e) => {
                      e.preventDefault();
                      commit(t);
                    }}
                    onMouseEnter={() => setActiveSuggestion(i)}
                    className="px-[8px] py-[3px] font-mono text-[10px] cursor-pointer"
                    style={{
                      background:
                        i === activeSuggestion
                          ? "var(--row-active)"
                          : "transparent",
                      color: "var(--text)",
                    }}
                  >
                    #{t}
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : (
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="px-[7px] py-[2px] rounded-[3px] border border-dashed border-line font-mono text-[10px] text-text-dimmer hover:text-text-dim"
          >
            + add tag
          </button>
        )}
      </div>
    </Section>
  );
}

function TagChip({ tag, onRemove }: { tag: string; onRemove: () => void }) {
  return (
    <span className="group inline-flex items-center gap-[4px] pl-[7px] pr-[5px] py-[1px] rounded-[3px] bg-surface-2 border border-line font-mono text-[10px] text-text-dim">
      #{tag}
      <button
        type="button"
        onClick={onRemove}
        title={`Remove #${tag}`}
        aria-label={`Remove tag ${tag}`}
        className="opacity-0 group-hover:opacity-100 text-text-dimmer hover:text-danger transition-opacity"
      >
        ×
      </button>
    </span>
  );
}

function formatLastOpened(iso: string | null): string {
  if (!iso) return "never";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  const hours = Math.floor(diffMs / 3_600_000);
  if (hours < 1) return "just now";
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks}w ago`;
  return d.toISOString().slice(0, 10);
}
