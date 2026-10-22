# Phase 4 — Docs Sweep Design Spec

**Date:** 2026-10-22  
**Status:** Approved  
**Author:** AI Agent & Maintainer  
**Scope:** Findings #42, #44, #45, #46, #47, #48, #49 from `docs/superpowers/reviews/2026-10-22/report.md` (Finding #43 native-speaker review deferred)

---

## 1. Intent and Why

During the full-app audit, several documentation claims and references were found to be inaccurate, misleading, or incomplete:
- The README overstated platform parity, claimed macOS-only menu shortcuts (`Cmd+N`, `Cmd+,`) worked with `Ctrl` on Windows and Linux, misdescribed Linux tray click behavior, and gave a build command (`cargo build --release`) that collides the desktop app with the CLI binary on macOS.
- `AGENTS.md` stated that translations live in "both en.json and da.json", which breaks builds since 7 locales are enforced by the test suite.
- The in-app `Help.tsx` view lacked a section for keyboard shortcuts and omitted critical CLI commands (`connections provision`, `keys generate`, `keys import`, `keys list`, `vault init`, `vault lock`, `status`).
- Six locales are machine-translated and should carry an explicit disclaimer in the README inviting native-speaker reviews.

This phase aligns all documentation, developer instructions, and in-app Help with reality across all 7 supported locales.

---

## 2. Goals, Non-Goals, Anti-Goals

### Goals
- Correct all factual inaccuracies in `README.md` (shortcuts, tray, cargo build target, parity phrasing, translation disclaimer).
- Update `AGENTS.md` to reference all seven supported locales (`en`, `da`, `de`, `es`, `fr`, `nb`, `sv`).
- Add a "Keyboard shortcuts" section to `Help.tsx` and all 7 translation bundles.
- Expand the "Command line" section in `Help.tsx` and all 7 translation bundles with the missing core commands.
- Qualify tray click descriptions in `Help.tsx` (`help.groups.tray`) across all 7 locales to reflect Linux tray behavior.
- Ensure all frontend and backend gates pass cleanly with strict key parity across all 7 locales.

### Non-Goals
- Full professional native-speaker translation audit for all 6 non-English languages (Finding #43 remains deferred as external work).
- Implementing new native window menus on Windows/Linux (the focus is accurate documentation of existing behavior).

### Anti-Goals
- Inventing new undocumented CLI commands or shortcuts.
- Introducing unlocalized or hardcoded UI strings.

---

## 3. Detailed Changes

### 3.1 README Corrections
1. **Platform Parity (#49):**
   - Replace `"It works the same on macOS, Linux, and Windows."` with:
     `"Available on macOS, Linux, and Windows with native system integration on each platform."`
2. **Install from Source (#47):**
   - Change `cargo build --release` to `cargo build -p sigrun-cli --release` in the Install section, explaining that building the CLI package directly avoids binary name collision with the desktop app on macOS.
3. **Keyboard Shortcuts (#42):**
   - Update the desktop app keyboard section to clearly distinguish macOS menu bar shortcuts from cross-platform window shortcuts:
     `"Keyboard: Cmd+F (Ctrl+F on Windows/Linux) searches connections, Cmd+L (Ctrl+L) locks the vault. On macOS, Cmd+N opens a new connection and Cmd+, opens Settings."`
4. **Tray Behavior (#45):**
   - Clarify the tray description:
     `"Left-clicking the tray icon opens the connection menu on macOS and Windows (or shows the window when the vault is locked). On Linux, left-click shows the context menu, which includes an Unlock item when locked."`
5. **Localization & Machine Translations (#49 / #43):**
   - Add a note under i18n acknowledging that German, French, Spanish, Swedish, Danish, and Norwegian Bokmål are machine-translated, and welcome community contributions and corrections.

### 3.2 AGENTS.md Rule Update (#44)
- Update "Code rules" bullet 4 in `AGENTS.md`:
  `"Every user-facing string goes through t(...) with its key in all seven locales in apps/desktop/src/i18n/*.json (en, da, de, es, fr, nb, sv); a recursive completeness guard enforces it."`

### 3.3 In-App Help: Keyboard Shortcuts (#46)
- Add a new section in `Help.tsx` before or after Settings:
  - Title: `help.shortcuts.title` ("Keyboard shortcuts")
  - `help.shortcuts.search` ("Cmd+F (Ctrl+F on Windows and Linux): Focus search field.")
  - `help.shortcuts.lock` ("Cmd+L (Ctrl+L on Windows and Linux): Lock the vault.")
  - `help.shortcuts.newConnection` ("Cmd+N (macOS): Open new connection dialog.")
  - `help.shortcuts.settings` ("Cmd+, (macOS): Open Settings.")
- Add corresponding keys to all 7 locale files with natural translations.

### 3.4 In-App Help: CLI Reference Expansion (#48)
- Expand `CLI` array in `Help.tsx`:
  - `host`
  - `status` (new: "Display vault, agent, and connection status.")
  - `vaultInit` (new: "Initialize a new vault.")
  - `vaultLock` (new: "Lock the vault.")
  - `vaultUnlock`
  - `connectionsAdd`
  - `connectionsConnect`
  - `connectionsPassword`
  - `connectionsProvision` (new: "Install a vault key on a remote host.")
  - `keysList` (new: "List vault and discovered keys.")
  - `keysGenerate` (new: "Generate a new SSH key.")
  - `keysImport` (new: "Import an existing key from disk into the vault.")
  - `credentialsList`
  - `credentialsAdd`
  - `credentialsRemove`
- Add cmd and desc keys to `help.cli.*` in all 7 locale files.

### 3.5 In-App Help: Tray Linux Qualification (#45)
- Update `help.groups.tray` in all 7 locale files to mention that on Linux, the tray menu provides an "Unlock" option when the vault is locked.

---

## 4. Verification Plan

1. **Unit and Parity Tests:**
   - `npm test -- --run` in `apps/desktop`:
     - `src/i18n/index.test.ts` enforces 100% key and placeholder parity across all 7 locales.
     - `src/views/Help.test.tsx` tests that the shortcuts section and new CLI commands render properly.
   - `npm run typecheck`, `npm run lint`, and `npm run build` pass cleanly.
2. **Rust Quality Gate:**
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo test --all-targets`
3. **Documentation Consistency:**
   - Cross-check that all documented CLI commands match actual CLI subcommands and flags.
