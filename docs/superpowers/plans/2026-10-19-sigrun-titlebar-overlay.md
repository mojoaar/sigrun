# Overlay Title Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the macOS title bar show the app's own colour instead of the system's grey, and make the window's appearance follow the app's theme setting.

**Architecture:** Two Tauri window facts, not webview tricks. The window is configured with an overlay title bar (`titleBarStyle: "Overlay"` plus `hiddenTitle`), so the system stops painting a bar and the webview extends under it; the app's existing header therefore *is* the visible bar, coloured by the theme's CSS variables. Separately, the window's native appearance is set from `Settings.theme` so the traffic-light glyphs stay legible against that header.

**Tech Stack:** Tauri 2 (Rust shell in `apps/desktop/src-tauri`), React 19 + TypeScript + Tailwind in `apps/desktop/src`.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` §8 (UI: beautiful by default, themes via CSS variables, i18n everywhere, accessibility required).

## Global Constraints

- `~/.ssh` is read-only. Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard test enforces it.
- Rust 2021, MSRV 1.85, dependencies only via `[workspace.dependencies]`; **no new dependency** for this feature.
- Frontend tests use the built-in matchers and `within(...)`; `@testing-library/jest-dom` is deliberately absent.
- A frontend `listen(...)` needs a permission in `apps/desktop/src-tauri/capabilities/default.json` or it is rejected and silently swallowed. This feature adds no `listen`.
- Every change gets a `## [Unreleased]` Keep-a-Changelog entry. One commit per task, conventional message.
- The gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`; when anything under `apps/desktop/src` changes, additionally from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.

## Review Focus

1. **Window dragging still works and the controls still click.** With `data-tauri-drag-region` on the header, a click on the Lock button or a nav tab must not start a window drag, and a click on empty header space must drag the window. Tauri honours the attribute only when the attributed element is the event target, so this must be verified in a running app; the plan's own test only pins the attribute's presence.
2. **The traffic lights stay legible in every combination.** macOS paints them according to the *window's* appearance, not the webview's CSS. A dark app theme over a light window appearance gives dark glyphs on a dark header.
3. **Nothing changes off macOS.** Linux and Windows have no overlay concept here and must keep their existing decorations and layout, with no stray left inset.
4. **No white flash.** Before the webview paints, the window shows its background colour; it must not be a colour the app never shows.
5. **The window title disappearing.** `hiddenTitle` removes the system's title text; the app's own header must actually name the app, or the window becomes anonymous in the window list.

---

### Task 1: Overlay the title bar on macOS

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (the `main` window entry)
- Modify: `apps/desktop/src/App.tsx` (the shell div and header)
- Test: `apps/desktop/src/App.test.tsx`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing from earlier tasks (first task of the plan).
- Produces: `export function isMacTitlebar(): boolean` from `apps/desktop/src/App.tsx`; the header element carries `data-tauri-drag-region` and, when `isMacTitlebar()` is true, the left-inset class.

- [ ] **Step 1: Confirm the pinned field names before writing them**

Read the pinned Tauri version in `Cargo.lock`, then confirm in the crate sources (or `https://schema.tauri.app/config/2` for that version) that a window accepts:
- `titleBarStyle` with the variant spelled exactly `"Overlay"`,
- `hiddenTitle` (boolean),
- `backgroundColor` (a CSS colour string).

Record in the report the file and line you read each name from. If any name differs from the above, use the real name and say so in the report — do not guess.

- [ ] **Step 2: Write the failing test**

Add to `apps/desktop/src/App.test.tsx`:

```tsx
it("gives the header a drag region and a macOS inset", () => {
  render(<App />);
  const header = document.querySelector("header") as HTMLElement;
  expect(header.getAttribute("data-tauri-drag-region")).not.toBeNull();
  expect(header.className).toContain("pl-20");
});
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cd apps/desktop && npx vitest run src/App.test.tsx`
Expected: FAIL — `data-tauri-drag-region` is null and the class list has no `pl-20`.

- [ ] **Step 4: Implement the window config**

In `apps/desktop/src-tauri/tauri.conf.json`, on the `main` window entry, add the overlay style, the hidden title, and a background colour taken from the theme's `--color-bg` value in `apps/desktop/src/styles/theme.css` (copy the literal, noting in the report which theme's value you used — `backgroundColor` cannot read a CSS variable).

- [ ] **Step 5: Implement `isMacTitlebar` and the header attributes**

In `apps/desktop/src/App.tsx`:

```tsx
/** The overlay title bar is macOS-only, so only macOS gets the traffic-light inset. */
export function isMacTitlebar(): boolean {
  return navigator.userAgent.includes("Mac");
}
```

Put `data-tauri-drag-region` on the shell's `<header>` element and add `pl-20` to its class list only when `isMacTitlebar()` is true, keeping the existing classes (`flex items-center justify-between border-b border-border p-3`). Do not put the attribute on any `<button>` or on a container that only exists to hold buttons.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd apps/desktop && npx vitest run src/App.test.tsx`
Expected: PASS.

- [ ] **Step 7: Run the gate**

Run from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`, then from the repo root: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.
Expected: all exit 0. Quote the output in the report.

- [ ] **Step 8: Add the CHANGELOG entry and commit**

Under `## [Unreleased]` → `### Changed`, one line: the macOS window uses an overlay title bar, so the app's own colour shows in it.

```bash
git add apps/desktop/src-tauri/tauri.conf.json apps/desktop/src/App.tsx apps/desktop/src/App.test.tsx CHANGELOG.md
git commit -m "feat(desktop): use an overlay title bar on macOS"
```

---

### Task 2: Make the window appearance follow the theme setting

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/src/lib.rs` (unit test beside the existing `about_metadata` test)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1's overlay window.
- Produces: `pub fn window_theme(setting: &str) -> Option<tauri::Theme>` in `apps/desktop/src-tauri/src/lib.rs` — `Some(Dark)` for `"dark"`, `Some(Light)` for `"light"`, `None` (follow the system) for anything else, which is what `Settings.theme`'s `"system"` default means.

- [ ] **Step 1: Write the failing test**

Add beside the existing `about_metadata` test in `apps/desktop/src-tauri/src/lib.rs`:

```rust
#[test]
fn the_window_theme_follows_the_setting() {
    assert_eq!(window_theme("dark"), Some(tauri::Theme::Dark));
    assert_eq!(window_theme("light"), Some(tauri::Theme::Light));
    assert_eq!(window_theme("system"), None);
    assert_eq!(window_theme("nordlys"), None);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p sigrun-desktop --lib window_theme`
Expected: FAIL — `cannot find function window_theme`.

- [ ] **Step 3: Implement the mapping and apply it**

Add `window_theme` as above (a plain `match`, no I/O). Apply it in two places: at startup, after the window exists and the settings are read (in the same `setup` that builds the menu and tray), and on every settings save, so that changing Light/Dark in Settings repaints the traffic lights immediately. Get the window with `app.get_webview_window("main")` and call `set_theme(window_theme(&settings.theme))`; discard the result with `let _ =` if the window is not up yet, and say in the report whether you needed a helper so both call sites share one code path.

If the pinned Tauri version names these differently (`set_theme` / `Theme`), use the real names and record where you verified them.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p sigrun-desktop --lib window_theme`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

Run the full gate from the repo root (`fmt`, `clippy -D warnings`, `cargo test --all-targets`) and from `apps/desktop` (`npm test -- --run && npm run typecheck && npm run lint && npm run build`). Quote the output.

Under `## [Unreleased]` → `### Changed`: the window's appearance follows the Light/Dark setting so the traffic lights stay legible.

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/commands.rs CHANGELOG.md
git commit -m "feat(desktop): follow the theme setting in the window appearance"
```

---

### Task 3: Document the overlay bar and its limits

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: documentation only.

- [ ] **Step 1: README**

In the macOS-facing appearance material, state: the window uses an overlay title bar on macOS, so the app's own header colour is the bar; the window's appearance (and therefore the traffic lights) follows the Light/Dark setting; Windows and Linux keep their own decorations. Keep the README's existing voice and do not restate the per-terminal launch rules.

- [ ] **Step 2: AGENTS.md**

Add one line to the platform rules beside the existing window rules: the macOS window uses an overlay title bar, the header carries `data-tauri-drag-region`, and the window appearance follows `Settings.theme`. Compressed neighbouring style.

- [ ] **Step 3: Verify every claim against the code**

For each sentence you write, name in the report the file and line that makes it true (`tauri.conf.json`'s window entry, `App.tsx`'s header, `lib.rs`'s `window_theme`). Do not write a claim you have not read the code for.

- [ ] **Step 4: Run the gate and commit**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` (docs only, so the frontend gate does not apply unless you touched a frontend file).

```bash
git add README.md AGENTS.md CHANGELOG.md
git commit -m "docs: describe the overlay title bar"
```

---

## Self-Review

- **Spec coverage:** §8's "beautiful by default" and "themes via CSS variables" is what this delivers — the bar becomes the app's colour instead of a system grey. Accessibility (§8) is the reason Task 2 exists: unreadable traffic lights are an accessibility failure, not a cosmetic one.
- **Step scan:** every step names a file, an exact value or a command with expected output. The one judgement left open (the literal background colour) names where to read it from.
- **Type consistency:** `isMacTitlebar()` (Task 1) and `window_theme(&str) -> Option<tauri::Theme>` (Task 2) are each defined once and used by name in the task that creates them.
- **Review Focus mapping:** 1 → Task 1 Step 7's manual note plus the attribute test; 2 → Task 2's test and its two apply sites; 3 → Task 1 Step 5's conditional inset; 4 → Task 1 Step 4's background colour; 5 → Task 1 Step 5 (the header keeps naming the app) and Task 3's documentation.
- **Proportion:** three tasks, no new abstraction, no dependency. The only new code is a one-line platform check and a `match`.
- **Risk flagged:** everything visible here needs a running GUI. The manual check is the human partner's, and the report must say so rather than imply the tests saw it.
