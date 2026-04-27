# Independent Terminal Theme

## Summary

Split the single Atlas theme setting into two independent knobs:

- **App theme** — `dark | light | system` (existing).
- **Terminal theme** — `dark | light | system` (new).

Each resolves independently. `system` on either side means "follow the OS
`prefers-color-scheme`". Changing the app theme no longer forces the terminal to
flip, and vice versa.

The terminal palette is the same Atlas dark/light palette as today; only the
choice of which one applies to xterm becomes independent. No new color schemes,
no ANSI palette work, no presets.

## Motivation

Today the terminal's xterm theme is read from the same CSS tokens
(`--bg`, `--text`, `--accent`, `--accent-fg`) that drive the app chrome, so the
terminal is structurally coupled to the app theme. Users who prefer a light
app surface but a dark terminal (or vice versa) cannot express that.

## Non-goals

- No full ANSI 16-color palette work. xterm keeps its built-in defaults for the
  ANSI slots.
- No named theme presets (Solarized, Dracula, etc.).
- No per-pane theme override. The setting is global across all terminal panes.
- No new "Terminal" section in Settings — the new control lives in **General**
  next to the existing Theme row.

## Architecture

### Settings shape (Rust + TS)

`GeneralSettings` (in `src-tauri/src/storage/types.rs`) gains one field:

```rust
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub menu_bar_agent: bool,
    pub default_project_location: String,
    pub theme: Theme,
    pub terminal_theme: Theme,   // NEW
}
```

`ts-rs` regenerates `src/types/rust.ts`. The hand-written
`GeneralSettings` in `src/types/index.ts` mirrors the new field
(`terminalTheme: Theme`).

**Default:** `Theme::System`. This matches the existing app-theme default and
gives existing users (whose persisted JSON lacks the field) a terminal that
follows the OS — no surprise change.

**Migration:** the settings load path already merges loaded JSON over a default
`Settings`, so missing keys deserialize to defaults. No explicit migration code
required.

### CSS tokens (`src/ui/tokens.css`)

Add a new attribute scope `[data-term-theme]` with five terminal-only tokens
mirroring today's dark/light palette values:

```css
[data-term-theme="dark"] {
  --term-bg:           oklch(0.16 0.004 260);
  --term-fg:           oklch(0.96 0.003 260);
  --term-cursor:       oklch(0.82 0.17 145);
  --term-cursor-fg:    oklch(0.16 0.004 260);
  --term-selection-bg: oklch(0.82 0.17 145 / 0.25);
}

[data-term-theme="light"] {
  --term-bg:           oklch(0.985 0.003 85);
  --term-fg:           oklch(0.22 0.004 260);
  --term-cursor:       oklch(0.62 0.17 145);
  --term-cursor-fg:    oklch(0.99 0 0);
  --term-selection-bg: oklch(0.62 0.17 145 / 0.25);
}
```

The new tokens are scoped on a separate attribute from `data-theme`, so an app
theme change does not move them, and a terminal theme change does not move the
app palette.

### Zustand store (`src/state/store.ts`)

Add:

```ts
terminalTheme: Theme;        // default "system"
setTerminalTheme: (t: Theme) => void;
```

Mirrors the existing `theme` / `setTheme` slice exactly.

### Resolution (`src/App.tsx`)

The existing effect that writes `data-theme` on `<html>` is extended to also
write `data-term-theme`. Both run through the same resolver:

```
effective(value) =
  value === "system" ? (mql.matches ? "dark" : "light") : value
```

A single `matchMedia("(prefers-color-scheme: dark)")` listener is attached when
*either* knob is set to `"system"`. Its handler recomputes and writes both
attributes.

A new `useEffect` mirrors the persisted setting into the store:

```ts
useEffect(() => {
  if (persistedSettings?.general.terminalTheme) {
    setTerminalTheme(persistedSettings.general.terminalTheme);
  }
}, [persistedSettings?.general.terminalTheme, setTerminalTheme]);
```

### Terminal wiring (`src/features/terminal/TerminalPane.tsx`)

`readTheme()` switches its `resolve()` calls from the app tokens to the new
dedicated terminal tokens:

```ts
return {
  background:          resolve("--term-bg",        "#111111"),
  foreground:          resolve("--term-fg",        "#eeeeee"),
  cursor:              resolve("--term-cursor",    "#7c7fee"),
  cursorAccent:        resolve("--term-cursor-fg", "#111111"),
  selectionBackground: resolve("--term-selection-bg", "rgba(124,127,238,0.25)"),
  selectionForeground: undefined,
};
```

**Live repaint of open panes.** A `MutationObserver` on
`document.documentElement` watching `attributes` (filtered to `data-term-theme`)
fires after `App.tsx` writes the new attribute. Its callback re-reads tokens
and assigns `term.options.theme = readTheme()`, which causes xterm to repaint
in place. Scrollback, the PTY connection, and the FitAddon are all untouched.

The observer is created once per `TerminalPane` after `term.open()` succeeds
and is disconnected in the same cleanup that disposes the terminal. One
observer per pane.

**Why an observer rather than a store subscription:** the CSS tokens only have
their new values *after* the browser has applied the new attribute's rules.
Reading from the Zustand store directly could fire one tick early, before
`getComputedStyle` returns fresh values. The observer fires after the
attribute write, guaranteeing freshness.

### Settings UI (`src/features/settings/SettingsPanel.tsx`)

In `GeneralSection`, immediately below the existing "Theme" row, add:

```tsx
<SettingsRow
  label="Terminal theme"
  hint="Independent from the app theme — pick a different look for shells."
>
  <select
    value={general?.terminalTheme ?? "system"}
    onChange={(e) =>
      mutation.mutate({
        terminalTheme: e.target.value as Settings["general"]["terminalTheme"],
      })
    }
    style={SELECT_STYLE}
  >
    <option value="dark">dark</option>
    <option value="light">light</option>
    <option value="system">match system</option>
  </select>
</SettingsRow>
```

`mutation`'s `onSuccess` already invalidates the settings query and calls
`setTheme(patch.theme)` when present. Add the symmetric line:

```ts
if (patch.terminalTheme) setTerminalTheme(patch.terminalTheme);
```

## Data flow

```
User picks "light" in "Terminal theme" select
        │
        ▼
SettingsPanel mutation.mutate({ terminalTheme: "light" })
        │
        ├──► Tauri setSettings → JSON on disk
        │
        └──► onSuccess: setTerminalTheme("light")  (Zustand)
                    │
                    ▼
        App.tsx effect: root.dataset.termTheme = "light"
                    │
                    ▼
        CSS rules switch: --term-bg, --term-fg, … repoint
                    │
                    ▼
        MutationObserver in each TerminalPane fires
                    │
                    ▼
        term.options.theme = readTheme()  → xterm repaints
```

## Testing

Manual verification (no automated tests added; this is a visual-only change):

1. App = dark, Terminal = dark → identical to today.
2. App = light, Terminal = dark → xterm stays dark while chrome turns light.
3. App = dark, Terminal = light → xterm flips to light immediately on the
   toggle, with no pane recreation, no scrollback loss.
4. Both = system, OS toggled to light → both flip simultaneously.
5. App = dark fixed, Terminal = system, OS toggled to light → only xterm flips.
6. Restart app → both selections persist via the existing settings JSON path.

The existing Rust tests in `storage/settings.rs` already cover round-tripping a
`Settings` value; one new assertion confirms the default `terminal_theme` is
`Theme::System`.

## Out of scope / explicit YAGNI

- Per-pane theme. The setting is global. If a user later wants per-pane
  overrides, that's a separate feature.
- A "Terminal" section in Settings. The control lives in General; promoting it
  to its own section can wait until there are >1 terminal-specific settings.
- ANSI palette customization, scheme presets, font size split. None of these
  are required by the user-stated goal.
