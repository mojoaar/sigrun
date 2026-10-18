# Open In New Window Or New Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user choose whether a connect opens a new terminal window or a new tab, for the terminals that can do it.

**Architecture:** A new `Settings.open_in` string ("window" or "tab", default "window" = today's behaviour) rides the existing settings struct from the settings view to `terminal_launch_specs`, which branches on it for the two macOS terminals that expose a tab API (Ghostty via AppleScript, iTerm via its existing script). Everything else ignores the setting deliberately, and the README says so.

**Tech Stack:** Rust 2021 (`sigrun-core`), React 19 + TypeScript + Tailwind v4, i18next (en + da), AppleScript for the macOS tab path.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` (§6.1's terminal-chosen-by-setting, §8 UI and i18n, §11's terminal heuristics left to the builder).

## Global Constraints

- `~/.ssh` is read-only. The only write is the explicit provisioning append to a remote `authorized_keys`.
- No secret in argv, an environment *value*, a log line, an error message, or the persisted model.
- Every user-facing string goes through `t(...)`, with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; the recursive guard in `apps/desktop/src/i18n/index.test.ts` enforces parity.
- The gate, run before every `git commit`: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. When any file under `apps/desktop/src` changes, also run, from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- One commit per task, conventional message. Each task that changes behaviour appends its own one-line `## [Unreleased]` entry to `CHANGELOG.md` in the same commit; README prose lands in Task 5.
- Rust 2021, MSRV 1.85. No new dependency.

## Review Focus

1. A `settings.toml` with no `open_in` key, or with a value nobody recognises (a hand edit, a future name, an empty string): the app must load and behave as "window", never fail to start.
2. macOS Terminal.app, kitty, Alacritty and WezTerm with "tab" selected: the connect must still work exactly as before — a new window — and the README must say the setting does not apply, rather than the code pretending otherwise.
3. "tab" on a machine with no terminal window open yet (the first connect after login): Ghostty must open a window, not silently do nothing.
4. A connect where `ssh` immediately fails, in tab mode: the tab must stay open showing the error, exactly as the window path does.
5. A terminal name in `Settings.terminal` that is not installed (detected on another machine, or uninstalled since): unchanged from today — the OS's launch failure still falls through to the next candidate.

---

### Task 1: `Settings.open_in`

**Files:**
- Modify: `crates/sigrun-core/src/model.rs` (`Settings`, its `Default`)
- Modify: `apps/desktop/src/api.ts` (the `Settings` type)
- Test: `crates/sigrun-core/src/model.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Settings.open_in: String`, default `"window".into()`, documented as "window" | "tab"; Task 2 reads it, Task 4 renders it.

- [ ] **Step 1: Write the failing test**

In `model.rs`'s test module:

```rust
#[test]
fn settings_default_to_opening_a_window()
    // Settings::default().open_in == "window"

#[test]
fn settings_without_an_open_in_key_still_load()
    // toml::from_str::<Settings>("version = 1\ntheme = \"nordlys\"\n") is Ok and open_in == "window"
```

The second is the guard for Review Focus 1: the struct is `#[serde(default)]`, so a missing field must not be an error.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p sigrun-core open_in` — expected: compile error, `no field open_in on type Settings`.

- [ ] **Step 3: Implement**

Add the field to `Settings` after `terminal`, a plain `String` like `theme` and `language` — deliberately not an enum, because a hand-edited value must degrade to the default rather than fail the whole settings load:

```rust
/// Where a connect opens: "window" or "tab". Anything else is "window".
pub open_in: String,
```

and `open_in: "window".into(),` to `Default`. Add `open_in: string;` to the TypeScript `Settings` type in `api.ts`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p sigrun-core` — expected: PASS. Then the full gate. The frontend typecheck will fail until Task 4 adds the field to its fixture objects, so either fold Task 4's fixture edit here or accept that `npm run build` is red until then; prefer adding `open_in: "window"` to those fixtures in this task's commit.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add the open_in setting"
```

CHANGELOG → `### Added`: `- An `open_in` setting (`"window"` or `"tab"`) for where a connect opens.`

---

### Task 2: `terminal_launch_specs` honours `open_in`, starting with iTerm

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs` (`terminal_launch_specs` signature and the iTerm branch)
- Modify: `crates/sigrun-core/src/host.rs` (`connections_connect` passes `settings.open_in.as_str()`)
- Test: `crates/sigrun-core/src/terminal.rs`

**Interfaces:**
- Consumes: `Settings.open_in` (Task 1).
- Produces: `terminal_launch_specs(platform: Platform, configured: Option<&str>, argv: &[String], env_prefix: &str, notify: Option<&str>, open_in: &str) -> Result<Vec<LaunchSpec>>` — the sixth parameter, appended after Task 3's `notify`, read as `open_in == "tab"` (anything else means a window). Task 3 branches on it too.

- [ ] **Step 1: Write the failing tests**

In `terminal.rs`:

```rust
#[test]
fn macos_iterm_opens_a_window_when_the_setting_says_so()
    // terminal_launch_specs(MacOs, Some("iterm"), &argv, "", None, "window")
    // assert specs[0].args[3] contains "create window with default profile command"
    // and does NOT contain "create tab"

#[test]
fn macos_iterm_reuses_a_tab_when_asked()
    // the existing macos_iterm_reuses_a_tab_and_still_handles_no_windows body,
    // renamed and given open_in = "tab"
```

Every other existing call site (all ~12 in `terminal.rs`) gains a trailing `"window"`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sigrun-core iterm` — expected: compile error, `this function takes 5 arguments but 6 were supplied`.

- [ ] **Step 3: Implement**

Add the parameter and branch the iTerm script: with `open_in == "window"` it is a single `create window with default profile command "{cmd}"`; with `"tab"` it keeps today's `if (count of windows) is 0 then … else …` script verbatim, because a first connect with no window open must still open one. Terminal.app's `do script` is left alone — it ignores the setting, deliberately (README, Task 5). `host.rs` passes `settings.open_in.as_str()`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p sigrun-core` — expected: PASS. Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(terminal): honour the open-in setting for iTerm"
```

---

### Task 3: Ghostty opens a tab when asked

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs` (the `Some("ghostty")` arm)
- Test: `crates/sigrun-core/src/terminal.rs`

**Interfaces:**
- Consumes: the `open_in` parameter from Task 2.
- Produces: for `open_in == "tab"`, one osascript spec; for anything else, today's `ghostty -e /bin/sh -c <line>` spec, unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn macos_ghostty_opens_a_tab_when_the_setting_says_so()
    // terminal_launch_specs(MacOs, Some("ghostty"), &argv, "", None, "tab")
    // specs[0].program == "osascript"; the script contains
    // "new tab in front window with configuration", "count of windows",
    // "new window with configuration", and "wait after command:true"

#[test]
fn macos_ghostty_keeps_the_command_line_launch_by_default()
    // open_in = "window" still yields program "ghostty" with ["-e","/bin/sh","-c",line]
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sigrun-core ghostty` — expected: FAIL, the default spec has program "ghostty", not "osascript".

- [ ] **Step 3: Implement**

In the `Some("ghostty")` arm, when `open_in == "tab"`, return a single `osascript` spec whose script creates the tab (or a window when none is open) and hands it the line as its own process:

```rust
// Ghostty's AppleScript API. `input text` does not execute in a tab this
// script creates (verified against Ghostty 1.3.1), so the tab is created
// with the line as its process instead, and `wait after command` is
// Ghostty's own keep-open. The line goes through `/bin/sh -c` for the same
// single-parse reason as the other terminals.
let line = applescript_escape(&keep_open);
let command = format!("/bin/sh -c \\\"{line}\\\"");
let script = format!(
    r#"tell application "Ghostty"
  if (count of windows) is 0 then
    new window with configuration {{command:"{command}", wait after command:true}}
  else
    new tab in front window with configuration {{command:"{command}", wait after command:true}}
  end if
end tell"#
);
return Ok(vec![LaunchSpec { program: "osascript".into(), args: vec!["-e".into(), script] }]);
```

`keep_open` never contains a double quote (`shell_escape` single-quotes), which is what makes the `\"…\"` wrapper safe: add a `shortcut:` comment saying so, and that a change to `shell_escape` breaks it.

- [ ] **Step 4: Run the tests and the gate, then verify live**

Run: `cargo test -p sigrun-core` — expected: PASS. Then the full gate.

Live check on this machine (Task 5 hands this to the user; the implementer runs the generated script once against Ghostty 1.3.1 to confirm it opens a tab and that a failing command leaves the tab open with its error visible). Note for cleanup: Ghostty blocks AppleScript `close`/`quit` while a process is running, so close the tab by hand or run the command with `wait after command:false` for the probe.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(terminal): open a Ghostty tab when the setting asks for one"
```

CHANGELOG → `### Added`: `- Ghostty and iTerm open a new tab instead of a window when the setting asks for it.`

---

### Task 4: The Settings control

**Files:**
- Modify: `apps/desktop/src/views/Settings.tsx`
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Test: `apps/desktop/src/views/Settings.test.tsx`

**Interfaces:**
- Consumes: `Settings.open_in` (Task 1) and the existing `update(patch)` / `getSettings` / `saveSettings` plumbing.
- Produces: nothing other code consumes.

- [ ] **Step 1: Write the failing tests**

In `Settings.test.tsx`: add `open_in: "window"` to the `getSettings` mock, extend the label-order assertion to the new order (Theme, Appearance, Language, Terminal, **Open in**, Auto-lock, Start minimized) and its length, and add one test that choosing "Tab" calls `saveSettings` with `open_in: "tab"`.

- [ ] **Step 2: Run it to verify it fails**

Run from `apps/desktop`: `npm test -- --run Settings` — expected: FAIL, the label order has 6 entries, or no "Open in" label.

- [ ] **Step 3: Implement**

A `<select className={field}>` immediately after the terminal picker, same shape as it:

```tsx
<label className="flex flex-col gap-1">{t("settings.openIn")}
  <select className={field} value={s.open_in} onChange={(e) => update({ open_in: e.target.value })}>
    <option value="window">{t("settings.openInWindow")}</option>
    <option value="tab">{t("settings.openInTab")}</option>
  </select>
</label>
```

i18n keys in **both** files: `settings.openIn` ("Open in" / "Åbn i"), `settings.openInWindow` ("New window" / "Nyt vindue"), `settings.openInTab` ("New tab" / "Ny fane"). The guard in `i18n/index.test.ts` fails if only one locale lands.

- [ ] **Step 4: Run the tests and the gate**

Run from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. Then `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(desktop): add the open-in-window-or-tab setting"
```

---

### Task 5: Docs

**Files:**
- Modify: `README.md`, `CHANGELOG.md`

- [ ] **Step 1: Update the prose**

`README.md`'s `### Where the terminal opens` section currently promises a new window for Ghostty ("neither exposes a tab API on macOS" — now false) and a tab-in-current-window for iTerm. Rewrite it to: the **Open in** setting chooses a new window or a new tab; **Ghostty** and **iTerm** honour it (Ghostty opening a window when none is open, so the first connect is not a no-op); **Terminal.app**, **kitty**, **Alacritty** and **WezTerm** always open a window, because they expose no reliable tab API; and one sentence that a failed connection keeps its window or tab open with the error. Also add the missing note that `open -na Ghostty.app --args --command=…` does not add a tab and is not used.

Also update the Settings bullet in the README's "What it does" list to mention the window-or-tab choice.

- [ ] **Step 2: Verify and commit**

Run the full gate one last time. Then:

```bash
git add -A
git commit -m "docs: document the open-in setting and the tab limits"
```

CHANGELOG → `### Changed`: `- Documented which terminals honour the new window-or-tab setting.`

---

## Self-Review

**Spec coverage.** §6.1's "terminal chosen by a setting" now has a second terminal preference beside it. §8 is met: the control goes through `t(...)` in both locales, is a labelled `<select>` reachable by keyboard, and needs no confirmation (it is a reversible preference, not a destructive action). §11's "terminal handling left to the builder" is where the per-terminal limits are documented rather than papered over.

**Step scan.** Each step carries one action and one checkable result. The Ghostty AppleScript is the only verbatim body, and it is verbatim because the quoting is the whole risk; the iTerm and Settings edits are signatures plus values.

**Type consistency.** `open_in` is one name across `Settings`, `settings.toml`, the TypeScript type and the i18n-adjacent select. `terminal_launch_specs`'s sixth parameter is named `open_in` in the Interfaces block, in the implementation and at every call site.

**Review Focus mapping.** (1) is `settings_without_an_open_in_key_still_load` plus the `String`-not-enum decision, and the `open_in == "tab"` comparison treating every other value as "window". (2) is the untouched Terminal/kitty/Alacritty/WezTerm arms plus the README rewrite. (3) is the `count of windows is 0` branch, asserted by `macos_ghostty_opens_a_tab_when_the_setting_says_so`. (4) is the `keep_open` line riding inside the Ghostty command unchanged and Task 3's live check. (5) is unchanged behaviour with existing tests.

**Proportion.** Five tasks for one setting and two terminal branches; no code is transcribed that a signature and a test do not already determine.

**Risk.** The Ghostty tab path's exact AppleScript quoting (a backslash-escaped double quote inside a double-quoted AppleScript string) is asserted by string tests but only truly proven by Task 3 Step 4's live check — if Ghostty's parser rejects it, the connect fails visibly rather than silently, and the fix is local to that arm. Second, whether today's `ghostty -e …` opens a fresh window when a window already exists is not settled by testing on this machine; the "window" setting preserves today's behaviour either way, so the risk is a pre-existing one the user should watch on the first live connect.
