# Phase 4: Docs Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct factual documentation inaccuracies in README, AGENTS.md, and the in-app Help view, expanding Help with keyboard shortcuts and missing CLI commands across all 7 supported locales.

**Architecture:** Update static markdown documentation (`README.md`, `AGENTS.md`), expand `Help.tsx` with a new keyboard shortcuts section and complete CLI command table, and synchronize keys across all 7 translation bundles (`en`, `da`, `de`, `es`, `fr`, `nb`, `sv`) enforcing full i18n parity.

**Tech Stack:** React 19, TypeScript, i18next, Vitest, Rust 2021, Markdown.

**Spec:** `docs/superpowers/specs/2026-10-22-phase-4-docs-sweep-design.md`

## Global Constraints

- Every user-facing string must go through `t(...)` with exact key and placeholder parity across all 7 locales in `apps/desktop/src/i18n/*.json` (`en`, `da`, `de`, `es`, `fr`, `nb`, `sv`).
- Rust MSRV 1.90, clean clippy `-D warnings`, zero formatting violations.
- Frontend tests in `apps/desktop` must pass with clean `tsc --noEmit`, ESLint, and Vite production build.

## Review Focus

- Factual accuracy: documented shortcuts and CLI commands must match actual implementation (`App.tsx`, `lib.rs`, `cli.rs`).
- Parity guard: `i18n/index.test.ts` must pass with zero missing or mismatched keys across any of the 7 languages.
- No binary path confusion: `README.md` must prescribe `-p sigrun-cli` for cargo release builds to avoid macOS binary overwrite.
- Linux tray qualification: descriptions in README and Help must accurately reflect that Linux tray icons show a context menu with an Unlock item rather than window-toggle on left-click.

---

### Task 1: README Corrections (Findings #42, #45, #47, #48, #49)

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README.md with accurate platform, build, shortcut, and translation statements**
  - Line 12-13 (#49): Replace `"It works the same on macOS, Linux, and Windows."` with `"Available on macOS, Linux, and Windows with native system integration on each platform."`
  - Line 61-65 (#47): Update install instructions to `cargo build -p sigrun-cli --release` and state that targeting `-p sigrun-cli` prevents colliding with the desktop app binary name on macOS.
  - Line 121-123 (#42): Clarify shortcuts: `"Keyboard: Cmd+F (Ctrl+F on Windows and Linux) focuses search, Cmd+L (Ctrl+L) locks the vault. On macOS, Cmd+N opens a new connection dialog and Cmd+, opens Settings."`
  - Line 131-133 (#45): Clarify tray behavior: `"Left-clicking the tray icon opens the connection menu on macOS and Windows (or shows the window when the vault is locked). On Linux, clicking the icon opens the context menu, which includes an Unlock item when locked."`
  - Line 142-143 (#49): Add note: `"Languages other than English are machine-translated; community review and improvements are welcome."`
- [ ] **Step 2: Verify formatting and syntax**
  Check markdown rendering and formatting.
- [ ] **Step 3: Commit**
  ```bash
  git add README.md
  git commit -m "docs: correct shortcuts, build command, tray behavior, and parity in README"
  ```

---

### Task 2: AGENTS.md Rule Update (Finding #44)

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: Update AGENTS.md Code rules bullet 4**
  Update line 76-78 to state:
  `"Every user-facing string goes through t(...) with its key in all seven locales in apps/desktop/src/i18n/*.json (en, da, de, es, fr, nb, sv); a recursive completeness guard enforces it."`
- [ ] **Step 2: Commit**
  ```bash
  git add AGENTS.md
  git commit -m "docs: update AGENTS.md i18n rule to reflect all seven locales"
  ```

---

### Task 3: In-App Help Keyboard Shortcuts (Finding #46)

**Files:**
- Modify: `apps/desktop/src/views/Help.tsx`
- Modify: `apps/desktop/src/views/Help.test.tsx`
- Modify: `apps/desktop/src/i18n/*.json` (all 7 locales)

- [ ] **Step 1: Write failing test in `Help.test.tsx`**
  Assert that `Help.tsx` renders a "Keyboard shortcuts" heading and describes `Cmd+F` and `Cmd+L`.
- [ ] **Step 2: Run test to verify it fails**
  `cd apps/desktop && npm test -- --run src/views/Help.test.tsx`
- [ ] **Step 3: Implement Shortcuts section in `Help.tsx` and i18n bundles**
  - Add `<section>` in `Help.tsx`:
    - `h3`: `help.shortcuts.title` ("Keyboard shortcuts")
    - `help.shortcuts.search` ("Cmd+F (Ctrl+F on Windows and Linux): Focus search field.")
    - `help.shortcuts.lock` ("Cmd+L (Ctrl+L on Windows and Linux): Lock the vault.")
    - `help.shortcuts.newConnection` ("Cmd+N (macOS): Open new connection dialog.")
    - `help.shortcuts.settings` ("Cmd+, (macOS): Open Settings.")
  - Add keys to `en.json`, `da.json`, `de.json`, `es.json`, `fr.json`, `nb.json`, and `sv.json`.
- [ ] **Step 4: Run test to verify it passes**
  `cd apps/desktop && npm test -- --run src/views/Help.test.tsx && npm test -- --run src/i18n/index.test.ts`
- [ ] **Step 5: Commit**
  ```bash
  git add apps/desktop/src/views/Help.tsx apps/desktop/src/views/Help.test.tsx apps/desktop/src/i18n/
  git commit -m "feat(desktop): add keyboard shortcuts section to help view across all locales"
  ```

---

### Task 4: In-App Help CLI Commands Expansion & Tray Linux Qualification (Findings #45 & #48)

**Files:**
- Modify: `apps/desktop/src/views/Help.tsx`
- Modify: `apps/desktop/src/views/Help.test.tsx`
- Modify: `apps/desktop/src/i18n/*.json` (all 7 locales)

- [ ] **Step 1: Write failing test in `Help.test.tsx`**
  Assert that `Help.tsx` renders commands for `sigrun connections provision`, `sigrun keys list`, `sigrun keys generate`, `sigrun vault lock`, and `sigrun status`.
- [ ] **Step 2: Run test to verify it fails**
  `cd apps/desktop && npm test -- --run src/views/Help.test.tsx`
- [ ] **Step 3: Expand `CLI` array and translation keys**
  - Add entries to `CLI` array in `Help.tsx`:
    - `"status"`
    - `"vaultInit"`
    - `"vaultLock"`
    - `"connectionsProvision"`
    - `"keysList"`
    - `"keysGenerate"`
    - `"keysImport"`
  - Add matching `help.cli.<key>.cmd` and `help.cli.<key>.desc` in all 7 locale JSON files.
  - In all 7 locale JSON files, update `help.groups.tray` to qualify Linux tray menu behavior (context menu includes Unlock item when locked).
- [ ] **Step 4: Run test to verify it passes**
  `cd apps/desktop && npm test -- --run src/views/Help.test.tsx && npm test -- --run src/i18n/index.test.ts`
- [ ] **Step 5: Commit**
  ```bash
  git add apps/desktop/src/views/Help.tsx apps/desktop/src/views/Help.test.tsx apps/desktop/src/i18n/
  git commit -m "feat(desktop): expand cli command reference and qualify linux tray behavior in help view"
  ```

---

### Task 5: Changelog & Gate Verification

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update CHANGELOG.md**
  Add entries under `## [0.5.0] - 2026-10-22` -> `### Changed`:
  - `Documented keyboard shortcuts and missing CLI commands in the in-app Help view across all seven locales.`
  - `Clarified platform-specific keyboard shortcuts, cargo build flags, and Linux tray behavior in README and Help.`
  - `Updated developer documentation in AGENTS.md to reflect completeness across all seven locales.`
- [ ] **Step 2: Run all workspace quality gates**
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets`
  - `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build`
- [ ] **Step 3: Commit**
  ```bash
  git add CHANGELOG.md
  git commit -m "docs: document phase 4 docs sweep in changelog"
  ```
