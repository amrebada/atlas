# Independent Terminal Theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple Atlas's terminal xterm theme from the app theme so each can be independently set to `dark | light | system`.

**Architecture:** Add a second `terminal_theme` field to `GeneralSettings` (Rust + TS). Resolve both `theme` and `terminalTheme` from `system` → OS preference inside `App.tsx`, writing two separate root attributes (`data-theme`, `data-term-theme`). Add `[data-term-theme]`-scoped CSS tokens (`--term-bg`, `--term-fg`, `--term-cursor`, `--term-cursor-fg`, `--term-selection-bg`) in `tokens.css`. `TerminalPane` reads the new tokens and live-updates open xterm instances via a `MutationObserver` watching the root attribute.

**Tech Stack:** Rust (Tauri 2, `ts-rs` for binding export), React 19 + TypeScript, Zustand, xterm.js, CSS custom properties.

**Spec:** `docs/superpowers/specs/2026-04-28-independent-terminal-theme-design.md`

**File map:**
- Modify `src-tauri/src/storage/types.rs` — add `terminal_theme: Theme` to `GeneralSettings`.
- Modify `src-tauri/src/storage/settings.rs` — default value, new test.
- Auto-regenerated `src/types/rust.ts` — via `ts-rs` `export` hook on `cargo test`.
- Modify `src/types/index.ts` — mirror `terminalTheme` on the hand-written `GeneralSettings`.
- Modify `src/state/store.ts` — add `terminalTheme` + `setTerminalTheme`.
- Modify `src/ui/tokens.css` — new `[data-term-theme="dark"|"light"]` blocks.
- Modify `src/App.tsx` — hydrate, resolve, and write `data-term-theme`.
- Modify `src/features/terminal/TerminalPane.tsx` — point `readTheme()` at `--term-*` tokens; observe attribute for live repaint.
- Modify `src/features/settings/SettingsPanel.tsx` — add "Terminal theme" select.

---

## Task 1: Add `terminal_theme` to Rust `GeneralSettings`

**Files:**
- Modify: `src-tauri/src/storage/types.rs:392-397`
- Modify: `src-tauri/src/storage/settings.rs:59-66` (defaults), add test near line 233

- [ ] **Step 1: Write the failing test in `src-tauri/src/storage/settings.rs`**

Append a new `#[tokio::test]` inside the existing `mod tests { ... }` block (just below `load_creates_defaults_on_first_read`):

```rust
#[tokio::test]
async fn default_terminal_theme_is_system() -> anyhow::Result<()> {
    let dir = unique_dir("terminal-theme-default");
    std::fs::create_dir_all(&dir)?;

    let s = load(&dir).await?;
    assert!(matches!(s.general.terminal_theme, Theme::System));

    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails (compile error first)**

```
cd src-tauri && cargo test --lib --quiet storage::settings::tests::default_terminal_theme_is_system 2>&1 | tail -20
```

Expected: compile error — `no field 'terminal_theme' on type 'GeneralSettings'`.

- [ ] **Step 3: Add the field to the struct in `src-tauri/src/storage/types.rs`**

Replace:

```rust
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub menu_bar_agent: bool,
    pub default_project_location: String,
    pub theme: Theme,
}
```

with:

```rust
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub menu_bar_agent: bool,
    pub default_project_location: String,
    pub theme: Theme,
    #[serde(default = "default_terminal_theme")]
    pub terminal_theme: Theme,
}

fn default_terminal_theme() -> Theme {
    Theme::System
}
```

`#[serde(default = ...)]` makes the field tolerant of older `settings.json` files that lack the key — they decode with `Theme::System`.

- [ ] **Step 4: Set the explicit default in `default_settings()` in `src-tauri/src/storage/settings.rs`**

Replace the `general:` block (around line 61):

```rust
        general: GeneralSettings {
            launch_at_login: true,
            menu_bar_agent: true,
            default_project_location: default_project_location(),
            theme: Theme::System,
        },
```

with:

```rust
        general: GeneralSettings {
            launch_at_login: true,
            menu_bar_agent: true,
            default_project_location: default_project_location(),
            theme: Theme::System,
            terminal_theme: Theme::System,
        },
```

- [ ] **Step 5: Run the new test (and the full settings tests) — should pass**

```
cd src-tauri && cargo test --lib --quiet storage::settings::tests 2>&1 | tail -20
```

Expected: all green, including `default_terminal_theme_is_system`. The `cargo test` invocation also triggers `ts-rs` to regenerate `src/types/rust.ts` with the new field.

- [ ] **Step 6: Verify the TS bindings were regenerated**

```
grep -n 'terminalTheme' src/types/rust.ts
```

Expected: a hit in the `GeneralSettings` line. If absent, run `cd src-tauri && cargo test --lib --quiet 2>&1 | tail -5` once more (some `ts-rs` versions only export on a full test run).

- [ ] **Step 7: Commit**

```
git add src-tauri/src/storage/types.rs src-tauri/src/storage/settings.rs src/types/rust.ts
git commit -m "feat(settings): add terminal_theme to GeneralSettings (default System)"
```

---

## Task 2: Mirror `terminalTheme` on the hand-written TS `GeneralSettings`

**Files:**
- Modify: `src/types/index.ts:254-259`

- [ ] **Step 1: Update the interface**

Replace:

```ts
export interface GeneralSettings {
  launchAtLogin: boolean;
  menuBarAgent: boolean;
  defaultProjectLocation: string;
  theme: Theme;
}
```

with:

```ts
export interface GeneralSettings {
  launchAtLogin: boolean;
  menuBarAgent: boolean;
  defaultProjectLocation: string;
  theme: Theme;
  terminalTheme: Theme;
}
```

- [ ] **Step 2: Type-check the project**

```
pnpm exec tsc --noEmit 2>&1 | tail -20
```

Expected: passes. (No call sites yet rely on the new key, so no errors.)

- [ ] **Step 3: Commit**

```
git add src/types/index.ts
git commit -m "types(settings): mirror terminalTheme on GeneralSettings"
```

---

## Task 3: Add `terminalTheme` slice to the Zustand UI store

**Files:**
- Modify: `src/state/store.ts:44-94` (state shape + setters), `src/state/store.ts:146-173` (defaults + impl)

- [ ] **Step 1: Add field to `UiState`**

In `src/state/store.ts`, find the block:

```ts
  /** Core appearance. */
  theme: Theme;
  density: Density;
  font: Font;
```

and replace with:

```ts
  /** Core appearance. */
  theme: Theme;
  terminalTheme: Theme;
  density: Density;
  font: Font;
```

- [ ] **Step 2: Add setter signature**

Find:

```ts
  setTheme: (t: Theme) => void;
  setDensity: (d: Density) => void;
```

Replace with:

```ts
  setTheme: (t: Theme) => void;
  setTerminalTheme: (t: Theme) => void;
  setDensity: (d: Density) => void;
```

- [ ] **Step 3: Add default value in the `create<UiState>(...)` call**

Find:

```ts
  theme: "dark",
  density: "dense",
```

Replace with:

```ts
  theme: "dark",
  terminalTheme: "system",
  density: "dense",
```

- [ ] **Step 4: Add setter implementation**

Find:

```ts
  setTheme: (theme) => set({ theme }),
  setDensity: (density) => set({ density }),
```

Replace with:

```ts
  setTheme: (theme) => set({ theme }),
  setTerminalTheme: (terminalTheme) => set({ terminalTheme }),
  setDensity: (density) => set({ density }),
```

- [ ] **Step 5: Type-check**

```
pnpm exec tsc --noEmit 2>&1 | tail -20
```

Expected: passes.

- [ ] **Step 6: Commit**

```
git add src/state/store.ts
git commit -m "state(ui): add terminalTheme slice"
```

---

## Task 4: Add `[data-term-theme]` CSS tokens

**Files:**
- Modify: `src/ui/tokens.css:55` (insert immediately after the `[data-theme="light"]` block)

- [ ] **Step 1: Append the two new blocks below `[data-theme="light"]`**

After the closing `}` of the `[data-theme="light"]` block (currently ending around line 55, right before `[data-font="plex"]`), insert:

```css
[data-term-theme="dark"] {
  --term-bg: oklch(0.16 0.004 260);
  --term-fg: oklch(0.96 0.003 260);
  --term-cursor: oklch(0.82 0.17 145);
  --term-cursor-fg: oklch(0.16 0.004 260);
  --term-selection-bg: oklch(0.82 0.17 145 / 0.25);
}

[data-term-theme="light"] {
  --term-bg: oklch(0.985 0.003 85);
  --term-fg: oklch(0.22 0.004 260);
  --term-cursor: oklch(0.62 0.17 145);
  --term-cursor-fg: oklch(0.99 0 0);
  --term-selection-bg: oklch(0.62 0.17 145 / 0.25);
}
```

Values mirror the existing `[data-theme]` palette so terminals look identical to today when both settings are aligned.

- [ ] **Step 2: Commit**

```
git add src/ui/tokens.css
git commit -m "tokens: add [data-term-theme] dark/light palette"
```

---

## Task 5: Resolve `terminalTheme` in `App.tsx`

**Files:**
- Modify: `src/App.tsx:73-103`

- [ ] **Step 1: Hydrate `terminalTheme` from persisted settings**

Locate the existing hydration block:

```ts
  const setTheme = useUiStore((s) => s.setTheme);
  const { data: persistedSettings } = useQuery<Settings>({
    queryKey: ["settings"],
    queryFn: getSettings,
    retry: false,
  });
  useEffect(() => {
    if (persistedSettings?.general.theme) {
      setTheme(persistedSettings.general.theme);
    }
  }, [persistedSettings?.general.theme, setTheme]);
```

Replace with:

```ts
  const setTheme = useUiStore((s) => s.setTheme);
  const setTerminalTheme = useUiStore((s) => s.setTerminalTheme);
  const { data: persistedSettings } = useQuery<Settings>({
    queryKey: ["settings"],
    queryFn: getSettings,
    retry: false,
  });
  useEffect(() => {
    if (persistedSettings?.general.theme) {
      setTheme(persistedSettings.general.theme);
    }
    if (persistedSettings?.general.terminalTheme) {
      setTerminalTheme(persistedSettings.general.terminalTheme);
    }
  }, [
    persistedSettings?.general.theme,
    persistedSettings?.general.terminalTheme,
    setTheme,
    setTerminalTheme,
  ]);
```

- [ ] **Step 2: Read `terminalTheme` from the store next to `theme`**

Just below `const theme = useUiStore((s) => s.theme);` (around line 48), add:

```ts
  const terminalTheme = useUiStore((s) => s.terminalTheme);
```

- [ ] **Step 3: Extend the data-attr effect to write `data-term-theme`**

Replace the existing effect:

```ts
  useEffect(() => {
    const root = document.documentElement;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const effective =
        theme === "system" ? (mql.matches ? "dark" : "light") : theme;
      root.dataset.theme = effective;
    };
    apply();
    root.dataset.density = density;
    root.dataset.font = font;
    root.style.setProperty("--sidebar-w", `${sidebarWidth}px`);
    if (theme === "system") {
      mql.addEventListener("change", apply);
      return () => mql.removeEventListener("change", apply);
    }
    return undefined;
  }, [theme, density, font, sidebarWidth]);
```

with:

```ts
  useEffect(() => {
    const root = document.documentElement;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const resolve = (t: typeof theme) =>
      t === "system" ? (mql.matches ? "dark" : "light") : t;
    const apply = () => {
      root.dataset.theme = resolve(theme);
      root.dataset.termTheme = resolve(terminalTheme);
    };
    apply();
    root.dataset.density = density;
    root.dataset.font = font;
    root.style.setProperty("--sidebar-w", `${sidebarWidth}px`);
    if (theme === "system" || terminalTheme === "system") {
      mql.addEventListener("change", apply);
      return () => mql.removeEventListener("change", apply);
    }
    return undefined;
  }, [theme, terminalTheme, density, font, sidebarWidth]);
```

- [ ] **Step 4: Type-check**

```
pnpm exec tsc --noEmit 2>&1 | tail -20
```

Expected: passes.

- [ ] **Step 5: Commit**

```
git add src/App.tsx
git commit -m "app: resolve and write data-term-theme alongside data-theme"
```

---

## Task 6: Switch `TerminalPane.readTheme()` to the new tokens + live repaint

**Files:**
- Modify: `src/features/terminal/TerminalPane.tsx:184-211` (`readTheme` body), `src/features/terminal/TerminalPane.tsx:20-180` (lifecycle wiring)

- [ ] **Step 1: Repoint `readTheme()` at the new tokens**

Replace the existing `readTheme()` function (currently reading `--bg`, `--text`, `--accent`, `--accent-fg` and a hard-coded selection color) with:

```ts
function readTheme(): import("xterm").ITheme {
  const root = document.documentElement;
  const css = getComputedStyle(root);
  const resolve = (name: string, fallback: string): string => {
    const raw = css.getPropertyValue(name).trim();
    if (!raw) return fallback;
    try {
      const probe = document.createElement("span");
      probe.style.color = raw;
      probe.style.display = "none";
      document.body.appendChild(probe);
      const out = getComputedStyle(probe).color;
      probe.remove();
      return out || fallback;
    } catch {
      return fallback;
    }
  };
  return {
    background: resolve("--term-bg", "#111111"),
    foreground: resolve("--term-fg", "#eeeeee"),
    cursor: resolve("--term-cursor", "#7c7fee"),
    cursorAccent: resolve("--term-cursor-fg", "#111111"),
    selectionBackground: resolve(
      "--term-selection-bg",
      "rgba(124, 127, 238, 0.25)",
    ),
    selectionForeground: undefined,
  };
}
```

- [ ] **Step 2: Wire a `MutationObserver` for live repaint**

Inside the `useEffect` that boots xterm (the long IIFE in `TerminalPane`), add a `MutationObserver` that updates `term.options.theme` when the root `data-term-theme` attribute changes. Locate the `tryOpen` definition (around line 61) and the variable list at the top of the effect (`let disposed = false; ... let opened = false;`).

After the existing `let resizeObs: ResizeObserver | null = null;` line, add:

```ts
    let themeObs: MutationObserver | null = null;
```

Then, immediately after the `tryOpen()` first-call site that succeeds (search for the place where `opened = true;` is set inside `tryOpen`, or the `tryOpen()` call right after `term.open(...)`'s success path — practically: at the end of the boot IIFE *after* `tryOpen()` is invoked for the first time), insert:

```ts
      themeObs = new MutationObserver((records) => {
        for (const r of records) {
          if (r.attributeName === "data-term-theme" && term) {
            term.options.theme = readTheme();
            return;
          }
        }
      });
      themeObs.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-term-theme"],
      });
```

If the existing IIFE structure is unclear, place the observer setup right before the closing `})();` of the boot IIFE (the function body that opens the terminal) — at this point `term` is non-null and the pane is alive.

Then in the cleanup (the `return () => { ... }` of the same `useEffect`), add a disconnect line near where `resizeObs?.disconnect()` is called:

```ts
      themeObs?.disconnect();
```

If a `resizeObs?.disconnect()` line does not exist, add the disconnect alongside the other cleanup operations (`dataUnlisten?.()`, `exitUnlisten?.()`, `term?.dispose()`).

- [ ] **Step 3: Type-check**

```
pnpm exec tsc --noEmit 2>&1 | tail -20
```

Expected: passes.

- [ ] **Step 4: Commit**

```
git add src/features/terminal/TerminalPane.tsx
git commit -m "terminal: read from --term-* tokens and live-update on data-term-theme change"
```

---

## Task 7: Add the "Terminal theme" select to General settings

**Files:**
- Modify: `src/features/settings/SettingsPanel.tsx:215-275`

- [ ] **Step 1: Pull `setTerminalTheme` into `GeneralSection`**

Find:

```ts
  const setTheme = useUiStore((s) => s.setTheme);
```

Replace with:

```ts
  const setTheme = useUiStore((s) => s.setTheme);
  const setTerminalTheme = useUiStore((s) => s.setTerminalTheme);
```

- [ ] **Step 2: Mirror the store update inside the mutation `onSuccess`**

Find:

```ts
    onSuccess: (_data, patch) => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (patch.theme) setTheme(patch.theme);
    },
```

Replace with:

```ts
    onSuccess: (_data, patch) => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (patch.theme) setTheme(patch.theme);
      if (patch.terminalTheme) setTerminalTheme(patch.terminalTheme);
    },
```

- [ ] **Step 3: Add the new SettingsRow immediately after the existing "Theme" row**

Find the existing row:

```tsx
      <SettingsRow label="Theme">
        <select
          value={general?.theme ?? "system"}
          onChange={(e) =>
            mutation.mutate({
              theme: e.target.value as Settings["general"]["theme"],
            })
          }
          style={SELECT_STYLE}
        >
          <option value="dark">dark</option>
          <option value="light">light</option>
          <option value="system">match system</option>
        </select>
      </SettingsRow>
    </div>
  );
}
```

Replace with (note: the trailing `</div>` and `}` of the function are preserved):

```tsx
      <SettingsRow label="Theme">
        <select
          value={general?.theme ?? "system"}
          onChange={(e) =>
            mutation.mutate({
              theme: e.target.value as Settings["general"]["theme"],
            })
          }
          style={SELECT_STYLE}
        >
          <option value="dark">dark</option>
          <option value="light">light</option>
          <option value="system">match system</option>
        </select>
      </SettingsRow>
      <SettingsRow
        label="Terminal theme"
        hint="Independent from the app theme — pick a different look for shells."
      >
        <select
          value={general?.terminalTheme ?? "system"}
          onChange={(e) =>
            mutation.mutate({
              terminalTheme: e.target
                .value as Settings["general"]["terminalTheme"],
            })
          }
          style={SELECT_STYLE}
        >
          <option value="dark">dark</option>
          <option value="light">light</option>
          <option value="system">match system</option>
        </select>
      </SettingsRow>
    </div>
  );
}
```

- [ ] **Step 4: Type-check**

```
pnpm exec tsc --noEmit 2>&1 | tail -20
```

Expected: passes.

- [ ] **Step 5: Commit**

```
git add src/features/settings/SettingsPanel.tsx
git commit -m "settings: add Terminal theme select to General"
```

---

## Task 8: Manual verification

The change is visual and threads through Tauri IPC, xterm runtime, and matchMedia — too many dynamic paths to cover with unit tests in this codebase. Run the dev app and walk the spec's six-case verification grid.

- [ ] **Step 1: Build the Rust side once to surface any backend regressions**

```
cd src-tauri && cargo build --quiet 2>&1 | tail -10
```

Expected: clean build (warnings ok, errors not).

- [ ] **Step 2: Boot the app**

```
pnpm tauri dev
```

Wait until the Atlas window opens.

- [ ] **Step 3: Walk the verification matrix**

Open Settings → General. Open at least one terminal pane (`Ctrl + ` `` ` `` or click the terminal strip).

For each case, change the corresponding select(s) and confirm the visible state. The existing terminal pane MUST repaint without losing scrollback (try `seq 1 200` then change theme — the buffer should still scroll).

| # | App theme | Terminal theme | Expected                                                      |
|---|-----------|----------------|---------------------------------------------------------------|
| 1 | dark      | dark           | Identical to today (single dark surface).                     |
| 2 | light     | dark           | Chrome light, terminal stays dark, no flicker.                |
| 3 | dark      | light          | Chrome dark, terminal flips light immediately, scrollback ok. |
| 4 | system    | system         | OS toggle switches both at once.                              |
| 5 | dark      | system         | OS toggle flips only the terminal.                            |
| 6 | restart   | restart        | Both selections persist across an app restart.                |

For case 4 / 5, change the OS appearance via System Settings → Appearance.

- [ ] **Step 4: Sanity-check the on-disk JSON**

```
python3 -c "import json; print(json.load(open('$HOME/Library/Application Support/atlas/settings.json'))['general'])"
```

Expected: shows both `theme` and `terminalTheme` keys.

- [ ] **Step 5: Stop the dev server**

`Ctrl+C` in the `pnpm tauri dev` terminal.

- [ ] **Step 6: No commit needed for this task** (all earlier tasks already committed). If any defect was discovered, fix in a follow-up commit referencing the case number.

---

## Self-review notes

- **Spec coverage:** every section of `docs/superpowers/specs/2026-04-28-independent-terminal-theme-design.md` maps to a task — Settings shape (Task 1, 2), Zustand (Task 3), CSS tokens (Task 4), App.tsx resolution (Task 5), Terminal wiring + live repaint (Task 6), Settings UI (Task 7), manual verification (Task 8).
- **Migration:** covered by `#[serde(default = ...)]` in Task 1 step 3 — old JSON files lacking `terminalTheme` decode with `Theme::System`, no explicit migration code path required.
- **Type consistency:** `terminal_theme` (Rust snake_case) ↔ `terminalTheme` (TS camelCase via `serde(rename_all = "camelCase")` already on `GeneralSettings`). The `setTerminalTheme` setter name and signature `(t: Theme) => void` are the same in store, App, and SettingsPanel.
- **No placeholders:** every step contains the exact code or command to run. Manual verification (Task 8) is acknowledged explicitly because there is no unit-test framework on the frontend in this repo.
