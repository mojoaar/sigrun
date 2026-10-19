# About Sigrun: Native Dialog and Installer Metadata Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The macOS app menu's **About Sigrun** shows the author, the website and the licence alongside the name and version, and the installer metadata that ships with the bundle is filled in.

**Architecture:** The app defines no menu today, so macOS shows Tauri's default submenus and its About panel carries only the product name and version. `setup` gains a macOS-gated menu whose About item carries explicit `AboutMetadata`; the standard submenus are carried over explicitly, because setting a menu replaces the default one and an absent Edit submenu breaks copy and paste in the app's own forms. Windows and Linux keep showing no menu bar. The remaining work is `tauri.conf.json`'s empty bundle metadata, which the DMG/installer reads.

**Tech Stack:** Rust (Tauri 2 `tauri::menu`), JSON config.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` (§8: beautiful by default; the app is the host, the shell only marshals).

## Global Constraints

- `~/.ssh` is read-only; the sole exception is an explicit provision appending to a remote `authorized_keys`.
- Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model.
- No new dependency.
- Gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. Nothing under `apps/desktop/src` changes in this plan, so the frontend gate is not triggered — unless a task changes it, in which case it is.
- One commit per task, conventional message, plus a `## [Unreleased]` CHANGELOG entry.

## Review Focus

1. **Copy and paste.** Setting a menu replaces Tauri's default. If the Edit submenu is not carried over explicitly, ⌘C/⌘V stops working in every form in the app — this is the change's main regression risk, and it is invisible to the test suite.
2. **Quit.** The tray's quit handler cancels the host's shutdown token before exiting. A predefined Quit item that bypasses that path leaves the IPC server and agent un-torn-down on exit; the menu must use the same handler the tray uses.
3. **Other platforms.** Windows and Linux show no menu bar today; the block must be gated so they still show none.
4. **Version drift.** The dialog must read the version the bundle ships (`CARGO_PKG_VERSION`), not a literal that drifts from `Cargo.toml`/`package.json`.
5. **A metadata field the pinned Tauri version does not have.** The field list in this plan came from current documentation, which may be newer than the version in `Cargo.lock`; the implementer verifies against the pinned version before writing code.
6. **`licenseFile`.** The repository has no `LICENSE` file and the bundler errors on a missing path, so `licenseFile` must not be set.

---

### Task 1: A macOS About that carries the author and the links

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs` (a `#[cfg(target_os = "macos")]` menu in `setup`)
- Modify: `apps/desktop/src-tauri/src/tray.rs` (make the quit path callable from the menu, if it is not already `pub`)
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (`bundle`)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `fn about_metadata() -> tauri::menu::AboutMetadata` in `lib.rs` — pure, so it is unit-testable without an app: name `Sigrun`, version `env!("CARGO_PKG_VERSION")`, authors `["mojoaar"]`, website `https://johansen.foo`, license `MIT`, copyright `Copyright © 2026 mojoaar`, comments from the bundle's short description.
- Consumes: the tray's existing quit routine (cancel the shutdown token, then `app.exit(0)`), reused verbatim for the menu's Quit item.

- [ ] **Step 1: Verify the menu API against the pinned version**

Read the installed Tauri version from `Cargo.lock`, then confirm from its docs (docs.rs for that exact version) the names and types of: `tauri::menu::AboutMetadata` and each of its fields; the constructor for a predefined About item taking a metadata value; the submenu/menu builders used to assemble the app submenu plus an Edit submenu (undo/redo/separator/cut/copy/paste/select all) and a Window submenu; and how a built menu is installed (`app.set_menu`). Write the confirmed names into the task report. Do not code from memory or from this plan's prose.

- [ ] **Step 2: Write the failing test**

In `apps/desktop/src-tauri/src/lib.rs`'s test module:

```rust
#[test]
fn the_about_metadata_names_the_project_its_author_and_its_licence() {
    let meta = about_metadata();
    assert_eq!(meta.name.as_deref(), Some("Sigrun"));
    assert_eq!(meta.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(meta.authors.as_deref(), Some(["mojoaar".to_string()].as_slice()));
    assert_eq!(meta.website.as_deref(), Some("https://johansen.foo"));
    assert_eq!(meta.license.as_deref(), Some("MIT"));
    assert_eq!(meta.copyright.as_deref(), Some("Copyright © 2026 mojoaar"));
}
```

Adjust the field names to the ones Step 1 confirmed; keep every asserted value exactly as written here.

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p sigrun-desktop the_about_metadata`
Expected: FAIL to compile — `cannot find function about_metadata`.

- [ ] **Step 4: Implement the metadata and the menu**

- `about_metadata()` returns the struct the test pins.
- In `setup`, after the state is managed, build the menu **only** on macOS (`#[cfg(target_os = "macos")]` around the block, so Windows and Linux keep no menu bar): the app submenu (About with the metadata, separator, services, hide, hide others, show all, separator, and a **Quit item wired to the tray's quit routine** — not a bare predefined quit), the Edit submenu (undo, redo, separator, cut, copy, paste, select all), and the Window submenu (minimize, close). Install it.
- If the tray's quit routine is not reachable, make it `pub(crate)` and call it; do not duplicate its body.

- [ ] **Step 5: Fill the bundle metadata**

In `tauri.conf.json`'s `bundle`: add `publisher: "mojoaar"`, `homepage: "https://johansen.foo"`, `copyright: "Copyright © 2026 mojoaar"`. Do **not** add `licenseFile`. Keep `shortDescription`, `longDescription`, `category`, `icon` and `targets` as they are.

- [ ] **Step 6: Run the test and the gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
Expected: clean; the new test passes.
Then confirm the config still parses: `cargo build -p sigrun-desktop` (or `cargo test -p sigrun-desktop`, which builds the same `tauri.conf.json` through `tauri-build`). A bad key fails the build.

- [ ] **Step 7: Manual check (do not skip; the suite cannot see it)**

From `apps/desktop`: `npm run tauri dev`. Confirm the app menu's **About Sigrun** shows the name, version, author, website and MIT; confirm ⌘C/⌘V still work in the Add-connection form; confirm Quit from the menu exits cleanly. Record what you saw in the report. If a menu item is missing or copy/paste is broken, fix it before committing.

- [ ] **Step 8: Commit**

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/tray.rs apps/desktop/src-tauri/tauri.conf.json CHANGELOG.md
git commit -m "feat(desktop): carry the author and links in the About dialog"
```

---

## Self-Review

- **Spec coverage:** §8's "beautiful by default" is served by making the native About truthful and the installer metadata complete; no behaviour of the host changes.
- **Step scan:** Step 1 exists because the plan cannot pin a third-party API it has not verified; Steps 2–4 give the test, the signature and the exact values; Step 7 is the only check that can see the regression in Review Focus 1.
- **Proportion:** one task, one pure function, one menu block, three config keys. The plan is deliberately short because the deliverable is small; the length sits in the checks.
- **Review Focus mapping:** 1 and 2 → Step 7 (manual) and the explicit Edit/Quit instructions in Step 4; 3 → the `cfg` gate; 4 → the test's `CARGO_PKG_VERSION` assertion; 5 → Step 1; 6 → Step 5's prohibition.
- **Known gap:** the menu itself has no automated test. A Tauri menu needs an app runtime, and enabling `tauri`'s `test` feature for one assertion about static structure would buy little; the pure metadata function carries the test, and Step 7 carries the menu. This is recorded, not hidden.
