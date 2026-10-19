# Terminal Tab Capability in the Open-in Setting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Settings "Open in" control only offers **New tab** for a terminal whose launch path can actually open a tab.

**Architecture:** `detect_terminals` already decides which terminal ids this build can launch on this platform. It now also reports whether that id can be asked for a tab, and the value rides the existing `TerminalInfo` wire struct, so the Settings view needs no new IPC. The launch path is not touched: the arms that ignore `open_in` keep ignoring it, and the UI stops promising what they cannot do.

**Tech Stack:** Rust (sigrun-core), React + TypeScript + Tailwind (apps/desktop), i18next.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` (§8: no user-facing text outside the i18n files, themes via CSS variables; §6.1: the user's own terminal, chosen by a setting).

## Global Constraints

- `~/.ssh` is read-only; the sole exception is an explicit provision appending to a remote `authorized_keys`.
- Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`.
- Gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`; when anything under `apps/desktop/src` changes, additionally from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- One commit per task, conventional message, plus a `## [Unreleased]` CHANGELOG entry.
- No new dependency.

## Review Focus

1. A hand-edited `open_in = "tab"` in `settings.toml` beside a terminal that cannot open one — the launch must stay correct (those arms ignore `open_in`) **and** the UI must show New window rather than silently promising a tab.
2. "System default" (`terminal: null`) — Terminal.app on macOS, the known-terminals chain on Linux, `cmd.exe`/Windows Terminal on Windows: this build can ask none of them for a tab, so the option must be disabled for that value too.
3. The enabled/disabled state must follow the host's report, never a hard-coded id list in the frontend (a user installs Ghostty and the state must change).
4. A `TerminalInfo` fixture without the new field — every mock must carry it, or `tsc` fails; do not weaken the type to optional to dodge this.
5. Changing the terminal back to a tab-capable one must re-enable the option in the same render pass.

---

### Task 1: Report each terminal's tab capability

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs` (`TerminalInfo`, `detect_terminals`, tests)
- Modify: `apps/desktop/src/api.ts` (`TerminalInfo`)
- Test: `apps/desktop/src/views/Settings.test.tsx`, `apps/desktop/src/App.test.tsx` (fixtures only)

**Interfaces:**
- Produces: `pub struct TerminalInfo { pub id: String, pub name: String, pub tabs: bool }` — `tabs` is true only for ids whose `terminal_launch_specs` arm honours `open_in == "tab"` (macOS `iterm`, `ghostty`; every id on Linux and Windows is false because those arms ignore the setting).
- Produces: a private `fn supports_tabs(platform: Platform, id: &str) -> bool` used by `detect_terminals`; `detect_terminals` keeps its signature, order and filtering.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing tests**

In `crates/sigrun-core/src/terminal.rs`'s test module. The first test must not depend on which terminals are installed on the test machine, so it drives the pure decision rather than `detect_terminals`:

```rust
#[test]
fn only_the_scriptable_mac_terminals_can_open_a_tab() {
    assert!(supports_tabs(Platform::MacOs, "iterm"));
    assert!(supports_tabs(Platform::MacOs, "ghostty"));
    for id in ["terminal", "kitty", "alacritty", "wezterm", ""] {
        assert!(!supports_tabs(Platform::MacOs, id), "{id}");
    }
    for platform in [Platform::Linux, Platform::Windows] {
        for id in ["gnome-terminal", "konsole", "wt.exe", "cmd.exe", "ghostty"] {
            assert!(!supports_tabs(platform, id), "{id}");
        }
    }
}

#[test]
fn every_detected_terminal_reports_its_tab_capability() {
    for platform in [Platform::MacOs, Platform::Linux, Platform::Windows] {
        for terminal in detect_terminals(platform) {
            assert_eq!(
                terminal.tabs,
                supports_tabs(platform, &terminal.id),
                "{}",
                terminal.id
            );
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sigrun-core supports_tabs -- --nocapture` (or `cargo test -p sigrun-core tab`)
Expected: FAIL to compile — `cannot find function supports_tabs`, `struct TerminalInfo has no field named tabs`.

- [ ] **Step 3: Implement**

`TerminalInfo` gains `pub tabs: bool` with a doc comment saying it means "this build can ask this terminal for a tab", not "this terminal has tabs" (Terminal.app has tabs and cannot be asked for one). `supports_tabs` matches on `(platform, id)` exactly as the test pins it. `detect_terminals` fills `tabs` at each construction site from `supports_tabs`.

In `apps/desktop/src/api.ts`, `TerminalInfo` becomes `{ id: string; name: string; tabs: boolean }`, and the two `listTerminals` mocks (`views/Settings.test.tsx`, `App.test.tsx`) gain a `tabs` value per entry.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
Expected: clean; the two new tests pass.
Then from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build` — typecheck fails before Step 3's fixture edit and passes after it.

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/terminal.rs apps/desktop/src/api.ts apps/desktop/src/views/Settings.test.tsx apps/desktop/src/App.test.tsx
git commit -m "feat(core): report which terminals can open a tab"
```

---

### Task 2: Only offer New tab where it can work

**Files:**
- Modify: `apps/desktop/src/views/Settings.tsx` (the "Open in" control)
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Test: `apps/desktop/src/views/Settings.test.tsx`

**Interfaces:**
- Consumes: `TerminalInfo.tabs` from Task 1.
- Produces: no exported symbols; `Settings` renders the reason line under the "Open in" select when the current terminal cannot open a tab.

- [ ] **Step 1: Write the failing tests**

In `views/Settings.test.tsx`, with `listTerminals` mocked to `[{ id: "terminal", name: "Terminal", tabs: false }, { id: "ghostty", name: "Ghostty", tabs: true }]`:

```tsx
it("only offers New tab for a terminal that can open one", async () => {
  render(<Settings />);
  const tab = (await screen.findByRole("option", { name: "New tab" })) as HTMLOptionElement;
  expect(tab.disabled).toBe(true);                       // terminal is null: System default
  fireEvent.change(screen.getByLabelText("Terminal"), { target: { value: "ghostty" } });
  expect((screen.getByRole("option", { name: "New tab" }) as HTMLOptionElement).disabled).toBe(false);
  fireEvent.change(screen.getByLabelText("Terminal"), { target: { value: "terminal" } });
  expect((screen.getByRole("option", { name: "New tab" }) as HTMLOptionElement).disabled).toBe(true);
});

it("does not keep New tab selected for a terminal that cannot open one", async () => {
  api.getSettings.mockResolvedValueOnce({ ...settings, terminal: "terminal", open_in: "tab" });
  render(<Settings />);
  await screen.findByRole("option", { name: "New tab" });
  expect((screen.getByLabelText("Open in") as HTMLSelectElement).value).toBe("window");
  expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ open_in: "window" }));
});
```

(`settings` is the file's existing fixture; keep its other fields.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm test -- --run src/views/Settings.test.tsx`
Expected: FAIL — the New tab option is not disabled, and the select still reads `tab`.

- [ ] **Step 3: Implement**

`const canOpenTabs = (id: string | null) => terminals.find((t) => t.id === (id ?? ""))?.tabs ?? false;` — the `?? false` is deliberate: the empty value is "System default", which no platform's default terminal can be asked for a tab from (comment this in one line).

- The **New tab** `<option>` gets `disabled={!canOpenTabs(s.terminal)}`.
- When the select is disabled, render one muted line under it: `<p className="text-xs text-muted">{t("settings.openInUnavailable")}</p>`.
- Coerce once on load and on every terminal change, so the stored value never disagrees with the UI: on load, if `s.open_in === "tab" && !canOpenTabs(s.terminal)`, call `update({ open_in: "window" })` from the existing load effect; in the terminal `<select>`'s `onChange`, pass `{ terminal: value || null, ...(canOpenTabs(value || null) ? {} : { open_in: "window" }) }`.

Add to `en.json` `settings`: `"openInUnavailable": "This terminal cannot open a tab."`; to `da.json`: `"openInUnavailable": "Denne terminal kan ikke åbne en fane."`

- [ ] **Step 4: Run the tests and the gate**

Run: from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`
Expected: all three Settings tests plus the suite pass; the i18n completeness guard passes because both locales gained the key.
Run: `cargo test --all-targets` (nothing in Rust changed, but the gate is the gate).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/views/Settings.tsx apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json apps/desktop/src/views/Settings.test.tsx
git commit -m "feat(desktop): offer a new tab only when the terminal can open one"
```

---

### Task 3: Document the limit and the CHANGELOG

**Files:**
- Modify: `README.md` (the `### Where the terminal opens` section)
- Modify: `AGENTS.md` (the platform-rules terminal-launch sentence)
- Modify: `CHANGELOG.md`

**Interfaces:** none (prose only).

- [ ] **Step 1: Update the README**

In `### Where the terminal opens`, state that the **Open in** setting offers **New tab** only for the terminals that can be asked for one (Ghostty and iTerm on macOS) and shows **New window** alone for the rest, naming the reason for Terminal.app (its scripting dictionary exposes tabs as read-only, so no new tab can be created) and for kitty/WezTerm (their tab commands need the user's own config or a running mux). Keep the section's existing bullet voice and do not restate the per-terminal launch details already there.

- [ ] **Step 2: Update the AGENTS.md platform rule**

Extend (do not replace) the terminal-launch sentence so a future reader knows the `open_in` setting is honoured only by Ghostty and iTerm on macOS, and that the other terminals must keep ignoring it.

- [ ] **Step 3: CHANGELOG**

Under `## [Unreleased]` → `### Changed`, one bullet: the Open in setting no longer offers a new tab for terminals that cannot open one.

- [ ] **Step 4: Gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
Expected: clean (prose only; the frontend gate is not triggered).

- [ ] **Step 5: Commit**

```bash
git add README.md AGENTS.md CHANGELOG.md
git commit -m "docs: record which terminals the open-in setting can honour"
```

---

## Self-Review

- **Spec coverage:** §8's i18n rule is served by the single new key in both locales; §6.1's terminal choice is unchanged in behaviour, and the plan adds no launch path.
- **Step scan:** every step names its file, its assertion or its exact string; no step says "handle edge cases".
- **Type consistency:** `TerminalInfo.tabs` is introduced in Task 1 and consumed in Task 2 with the same name; `supports_tabs` is private to `terminal.rs` and named identically in the test.
- **Review Focus mapping:** 1 and 2 → Task 2's second test and the `?? false` rule; 3 → Task 1's `every_detected_terminal_reports_its_tab_capability` (the field follows the function, never a literal list in the UI); 4 → Task 1 Step 4's typecheck; 5 → Task 2's first test (ghostty then terminal again).
- **Proportion:** three tasks, one new field, one UI rule; no code bodies beyond the one-line capability match and the two helpers.
- **Known gap:** the disabled state for an explicitly selected id that is missing from the host's list falls back to disabled (`?? false`). The list is loaded at mount, so this is only reachable if detection fails; recorded here rather than tested.
