# Import and export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings gains an "Import and export" section that writes connections and public key metadata to a folder or a `.zip`, and reads the same format back, skipping anything that already exists.

**Architecture:** The bundle format and all of its I/O live in `sigrun-core` as a new `bundle` module with no Tauri knowledge, so it is unit-testable against temp directories. The host exposes it over the existing IPC as two new requests, exactly like every other mutation; the desktop layer adds the official dialog plugin for the native pickers and two thin commands; the React view only picks a path and renders the summary.

**Tech Stack:** Rust 2021 (MSRV 1.85), `zip` 7.3.0 (deflate only), `tauri-plugin-dialog` 2.7.3, Tauri 2.12.4, React 19 + TypeScript, Tailwind v4, i18next (en + da).

**Spec:** `docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md`

## Global Constraints

- `~/.ssh` is read-only. The single exception (appending to a remote `authorized_keys` during an explicit provision) is untouched by this work. An export destination inside `~/.ssh` is refused.
- Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model. The bundle carries no secret: no private key material, no stored password, no vault file.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard test enforces parity.
- Dependencies only via `[workspace.dependencies]`, and no new dependency without a strong reason. Exactly two are added here, pinned to the MSRV floor: `zip = { version = "7.3.0", default-features = false, features = ["deflate-flate2-zlib-rs"] }` and `tauri-plugin-dialog = "2.7.3"`.
- The gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. When anything under `apps/desktop/src` changes, also from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- Every change gets a `## [Unreleased]` Keep-a-Changelog entry. One commit per task, conventional message.
- A Tauri plugin API the webview calls needs a grant in `apps/desktop/src-tauri/capabilities/default.json`, or the call is rejected and a swallowed promise hides it.

## Review Focus

1. A crafted bundle cannot write outside its destination, escape into `~/.ssh`, or exhaust memory — including a zip whose entry names contain `..`, an absolute path, or a symlink target.
2. Import never touches the vault and never makes a connection look like it has a stored password or a live session.
3. A connection the launcher can use survives the round trip unchanged, including one whose `user` is empty and one whose `key_id` points at a key that exists here.
4. Nothing in an export is a secret: an exported folder or zip can be read by anyone who has it, and the app says so.
5. An export that would overwrite an existing `export.json` refuses rather than clobbers.

---

### Task 1: The bundle module

**Files:**
- Create: `crates/sigrun-core/src/bundle.rs`
- Modify: `crates/sigrun-core/src/lib.rs` (register the module)
- Modify: `Cargo.toml` (workspace dependency), `crates/sigrun-core/Cargo.toml`
- Test: in-module `#[cfg(test)] mod tests` in `crates/sigrun-core/src/bundle.rs`

**Interfaces:**
- Consumes: `model::{Connection, DataFile, KeyMetadata}`.
- Produces, all `pub` in `sigrun_core::bundle`:
  - `const FORMAT_VERSION: u32 = 1;`
  - `const EXPORT_FILE: &str = "export.json";`, `const README_FILE: &str = "README.txt";`, `const KEY_DIR: &str = "keys";`
  - `struct Bundle { format: u32, exported_at: u64, connections: Vec<Connection>, keys: Vec<KeyMetadata> }` (`Serialize`, `Deserialize`, `#[serde(default)]` on the two vectors)
  - `struct ExportSummary { path: String, connections: usize, keys: usize }`
  - `struct ImportSummary { added: usize, skipped: usize, invalid: usize, unresolved_keys: usize }`
  - `fn build_bundle(data: &DataFile, now_unix: u64) -> Bundle`
  - `fn readme_text() -> String`
  - `fn write_folder(bundle: &Bundle, dir: &Path) -> Result<()>`
  - `fn write_zip(bundle: &Bundle, file: &Path) -> Result<()>`
  - `fn read_bundle(path: &Path) -> Result<Bundle>`
  - `fn merge(data: &mut DataFile, bundle: &Bundle) -> ImportSummary`
  - `fn destination_is_allowed(dest: &Path, ssh_dir: &Path) -> bool`

- [ ] **Step 1: Add the dependency**

In the root `Cargo.toml` under `[workspace.dependencies]`:
```toml
zip = { version = "7.3.0", default-features = false, features = ["deflate-flate2-zlib-rs"] }
```
and in `crates/sigrun-core/Cargo.toml` under `[dependencies]`:
```toml
zip = { workspace = true }
```
Run `cargo build -p sigrun-core` and quote its output. If the build rejects the feature name, fall back to `features = ["deflate"]` and say so in the report.

- [ ] **Step 2: Write the failing tests**

In `bundle.rs`, `#[cfg(test)] mod tests`, using `tempfile` exactly as the existing core tests do:

```rust
#[test]
fn a_folder_round_trips_every_connection_and_public_key() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = build_bundle(&data_with_two_connections_and_one_key(), 1_700_000_000);
    write_folder(&bundle, dir.path()).unwrap();
    assert!(dir.path().join("export.json").is_file());
    assert!(dir.path().join("README.txt").is_file());
    let back = read_bundle(dir.path()).unwrap();
    assert_eq!(back.connections, bundle.connections);
    assert_eq!(back.keys, bundle.keys);
    assert_eq!(back.format, FORMAT_VERSION);
}

#[test]
fn a_zip_round_trips_the_same_bundle_as_a_folder() {
    // same fixture, write_zip, read_bundle on the file, compare to the folder result
}

#[test]
fn the_bundle_carries_no_password_and_no_active_flag() {
    // a Connection with has_password: true, active: true
    // after build_bundle both are false
}

#[test]
fn a_destination_inside_the_ssh_directory_is_refused() {
    // destination_is_allowed("/home/u/.ssh/x", "/home/u/.ssh") == false
    // destination_is_allowed("/home/u/backups", "/home/u/.ssh") == true
    // and a sibling "/home/u/.sshrc" is allowed (prefix is not a match)
}

#[test]
fn import_skips_a_connection_whose_id_already_exists() { /* added: 1, skipped: 1 */ }

#[test]
fn import_skips_an_invalid_entry_and_counts_it() {
    // a connection with an empty host -> invalid: 1, added: 0
}

#[test]
fn import_clears_the_host_filled_flags() {
    // a bundle connection with has_password/active true -> stored false
}

#[test]
fn import_counts_connections_whose_key_is_not_here() {
    // two connections, one key_id present in data.keys, one not -> unresolved_keys == 1
}

#[test]
fn a_zip_entry_that_escapes_the_archive_is_rejected() {
    // build a zip in the test whose member name is "../escape.json", read_bundle -> Err
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p sigrun-core bundle -- --nocapture`
Expected: FAIL — `cannot find function build_bundle` and friends.

- [ ] **Step 4: Implement `bundle.rs`**

The shape the tests pin: `build_bundle` clones the connections with `has_password = false` and `active = false` and clones `data.keys`; `write_folder` creates the directory, writes `export.json` (pretty JSON), `README.txt` from `readme_text()`, and one `keys/<name>.pub` per key using `key.public_key` with a sanitised file name; `write_zip` writes the same three kinds of entry through `zip::ZipWriter` with `CompressionMethod::Deflated`; `read_bundle` accepts a directory (reading `export.json`) or a file (opening the archive and reading only the `export.json` entry), and rejects an entry name that is absolute, contains `..`, or otherwise escapes the archive root; `merge` pushes each bundle connection that is not already present by `id` and whose host passes the launcher's host rule, counts the rest, forces the two host-filled flags false, and counts connections whose `key_id` is `Some` and matches no `data.keys` entry; `destination_is_allowed` canonicalises where it can and refuses any path that is equal to or inside `ssh_dir`.

In `lib.rs`, add `pub mod bundle;` in alphabetical order beside the existing modules.

- [ ] **Step 5: Run the tests and the gate**

Run: `cargo test -p sigrun-core bundle` then the full gate.
Expected: PASS, then every gate command exits 0.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/sigrun-core/Cargo.toml crates/sigrun-core/src/bundle.rs crates/sigrun-core/src/lib.rs CHANGELOG.md
git commit -m "feat(core): add the export/import bundle format"
```

---

### Task 2: Export and import over the IPC

**Files:**
- Modify: `crates/sigrun-core/src/ipc/protocol.rs` (two requests, two responses)
- Modify: `crates/sigrun-core/src/host.rs` (the two arms, the handlers, the tests)
- Test: in-module tests in `host.rs`

**Interfaces:**
- Consumes: `bundle::{build_bundle, read_bundle, write_folder, write_zip, merge, destination_is_allowed, ExportSummary, ImportSummary}`.
- Produces:
  - `Request::ExportBundle { path: String, zipped: bool }` and `Request::ImportBundle { path: String }` (wire names `export_bundle`, `import_bundle`)
  - `Response::Export(ExportSummary)` and `Response::Import(ImportSummary)`
  - `impl HostState { async fn export_bundle(&self, path: &str, zipped: bool) -> Result<ExportSummary>; async fn import_bundle(&self, path: &str) -> Result<ImportSummary> }`

- [ ] **Step 1: Write the failing tests**

In `host.rs`'s test module, driving `handle(...)` against a tempdir host as the neighbouring tests do:

```rust
#[test]
fn exporting_writes_a_folder_the_import_can_read_back() {
    // seed one connection through ConnectionsAdd, export to a temp dir,
    // then import it into a second host whose data dir is empty:
    // added == 1, and the second host's ConnectionsList contains it
}

#[test]
fn importing_twice_adds_nothing_the_second_time() {
    // first import added == 1, second added == 0 and skipped == 1
}

#[test]
fn importing_sets_no_password_and_no_active_flag() {
    // after import, ConnectionsList reports has_password == false and active == false
}

#[test]
fn exporting_into_the_ssh_directory_is_refused() {
    // a host whose ssh_dir is a tempdir; export a path inside it -> Response::Error
}

#[test]
fn exporting_over_an_existing_export_file_is_refused() {
    // export once, export the same path again -> error, file unchanged
}

#[test]
fn a_missing_bundle_is_an_error_not_a_panic() {
    // import a path that does not exist -> Response::Error
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sigrun-core host::tests::export -- --nocapture` and `...::import -- --nocapture`
Expected: FAIL — unresolved variants and methods.

- [ ] **Step 3: Implement**

Add the two variants to `Request` and the two to `Response`. Add the two arms to `handle_inner`'s match. Add both to the `last_vault_activity` `matches!` list — unlike `ConnectionEnded`/`SessionStarted`, these read or write `data.toml` under the store lock and their result is stale if the vault changes, so they *should* defer the idle auto-lock only if that matches the list's existing intent; read the list's comment and follow it, and say in the report which way you went and why. Implement `export_bundle`: refuse a destination that `destination_is_allowed` rejects, refuse when the destination already holds `export.json`, load the data, build the bundle, then write the folder or the zip. Implement `import_bundle`: refuse a path that does not exist, read the bundle, then apply `merge` inside `store.update_data`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test --all-targets`, then the full gate.
Expected: PASS, gate exits 0.

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/ipc/protocol.rs crates/sigrun-core/src/host.rs CHANGELOG.md
git commit -m "feat(core): export and import a bundle over the IPC"
```

---

### Task 3: The native pickers and the desktop commands

**Files:**
- Modify: `Cargo.toml` (workspace dependency), `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register the plugin, add the two commands to `invoke_handler`)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (two thin commands)
- Modify: `apps/desktop/src-tauri/capabilities/default.json` (`dialog:default`)
- Modify: `apps/desktop/package.json` (`@tauri-apps/plugin-dialog`)
- Modify: `apps/desktop/src/api.ts` (two wrappers)
- Test: the existing `npm run typecheck` and `npm run build`, plus the Rust gate

**Interfaces:**
- Consumes: `Request::ExportBundle` / `Request::ImportBundle`.
- Produces:
  - Rust: `#[tauri::command] pub async fn export_bundle(state: St<'_>, path: String, zipped: bool) -> CmdResult` returning `Response::Export(ExportSummary)`, and `#[tauri::command] pub async fn import_bundle(state: St<'_>, path: String) -> CmdResult` returning `Response::Import(ImportSummary)`
  - TypeScript: `export const exportBundle = (path: string, zipped: boolean) => call<ExportSummary>("export_bundle", { path, zipped })` and `export const importBundle = (path: string) => call<ImportSummary>("import_bundle", { path })`, with `type ExportSummary = { path: string; connections: number; keys: number }` and `type ImportSummary = { added: number; skipped: number; invalid: number; unresolved_keys: number }`

- [ ] **Step 1: Add the plugin**

Root `[workspace.dependencies]`: `tauri-plugin-dialog = "2.7.3"`. In `apps/desktop/src-tauri/Cargo.toml`, `tauri-plugin-dialog = { workspace = true }`. In `lib.rs`'s builder chain, `.plugin(tauri_plugin_dialog::init())` before `.invoke(...)`. In `capabilities/default.json`, add `"dialog:default"` to the permissions array. In `apps/desktop/package.json`, add `"@tauri-apps/plugin-dialog": "^2"`. Run `cargo build -p sigrun-desktop` and quote it; report any version conflict with Tauri 2.12.4.

- [ ] **Step 2: Implement the two commands**

`export_bundle` and `import_bundle` follow the `open_external`/`get_settings` pattern exactly: a `*_inner` that calls `handle(&state, req)`, a `#[tauri::command]` wrapper, and one line in the `invoke_handler` list. No `app: AppHandle` is needed (nothing here changes the vault or the tray). Register both.

- [ ] **Step 3: Implement the two `api.ts` wrappers**

Beside `openExternal`, with the two types exported from the same file.

- [ ] **Step 4: Verify**

Run from `apps/desktop`: `npm run typecheck && npm run lint && npm run build`; from the root: the full Rust gate.
Expected: all exit 0. Report the versions actually resolved for both new dependencies (`cargo tree -p sigrun-desktop | grep -E "zip|dialog"`).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock apps/desktop/package.json apps/desktop/package-lock.json apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/capabilities/default.json apps/desktop/src/api.ts CHANGELOG.md
git commit -m "feat(desktop): expose export and import with native pickers"
```

---

### Task 4: The Settings section

**Files:**
- Modify: `apps/desktop/src/views/Settings.tsx`
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Test: `apps/desktop/src/views/Settings.test.tsx`

**Interfaces:**
- Consumes: `exportBundle(path, zipped)`, `importBundle(path)` from `../api`; `open`, `save` from `@tauri-apps/plugin-dialog`.
- Produces: a `settings.transfer*` key family in both locales, and a section whose four buttons call the pickers then the commands.

- [ ] **Step 1: Write the failing tests**

In `Settings.test.tsx`, mocking `@tauri-apps/plugin-dialog` (`open`, `save`) the way `App.test.tsx` mocks the event module:

```tsx
it("exports to the folder the picker returned", async () => {
  // open.mockResolvedValue("/tmp/backup"); exportBundle.mockResolvedValue({ path: "/tmp/backup", connections: 2, keys: 1 });
  // click "Export to a folder…"; expect exportBundle called with ("/tmp/backup", false)
});

it("exports to a zip when that button is used", async () => {
  // save.mockResolvedValue("/tmp/backup.zip"); click "Export to a .zip file…"
  // expect exportBundle called with ("/tmp/backup.zip", true)
});

it("reports what an import did", async () => {
  // importBundle.mockResolvedValue({ added: 3, skipped: 1, invalid: 0, unresolved_keys: 2 });
  // expect the summary line to read "3 added, 1 skipped" and to mention the 2 unresolved keys
});

it("says nothing when the picker is cancelled", async () => {
  // open.mockResolvedValue(null); click; expect importBundle not called and no summary line
});

it("shows an error when the command refuses", async () => {
  // exportBundle.mockRejectedValue({ code: "internal", message: "…" });
  // expect the error line and no crash
});
```

Keep the existing label-order assertion passing: the new section uses a heading and buttons, **no `<label>`**, so the seven labels it counts are unchanged. Say so in the report.

- [ ] **Step 2: Run them to verify they fail**

Run from `apps/desktop`: `npm test -- --run Settings`
Expected: FAIL — the buttons do not exist.

- [ ] **Step 3: Implement the section**

Append below the existing controls, before the closing of the panel: a `<h3>` heading, one short paragraph of explanation, the four buttons (each `className={btn}`-shaped as the view's other buttons), and a `role="status"` result line plus a `role="alert"` error line, both empty until used. `open({ directory: true, title })` for the folder pickers, `save({ defaultPath: "sigrun-export", filters })` for the zip export, `open({ filters })` for the zip import. Every picker result is checked for `null` (cancelled) before any command call. Add the keys to both locales.

- [ ] **Step 4: Run the frontend gate**

Run from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
Expected: all exit 0, the order test still sees seven labels.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/views/Settings.tsx apps/desktop/src/views/Settings.test.tsx apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json CHANGELOG.md
git commit -m "feat(desktop): add the import and export section"
```

---

### Task 5: Documentation

**Files:**
- Modify: `README.md`, `AGENTS.md`, `CHANGELOG.md`

- [ ] **Step 1: README**

Add an "Import and export" entry to the feature list and, in the Settings section, what the four buttons do; a *Not a backup* paragraph stating plainly that the export carries connections and public keys but no private key material and no stored passwords, so the vault file plus the passphrase is the only thing that can restore a key — and that an exported folder or zip is readable by anyone who has it.

- [ ] **Step 2: AGENTS.md**

One Platform-rules bullet: the bundle format, the two dependencies pinned to the MSRV floor, the `dialog:default` grant, and that the destination must never be inside `~/.ssh`.

- [ ] **Step 3: CHANGELOG**

One `### Added` bullet under `## [Unreleased]`.

- [ ] **Step 4: Gate and commit**

Run the full Rust gate (docs only, so no frontend gate) then:
```bash
git add README.md AGENTS.md CHANGELOG.md
git commit -m "docs: document import and export"
```

---

## Self-Review

- **Spec coverage:** the four buttons, the format, the two pinned dependencies, the `dialog:default` grant, the `~/.ssh` refusal, the overwrite refusal, skip-on-conflict, the forced flags, the zip limits and the summary counts each map to a step above. The key-metadata import (the one deviation flagged in the spec) is Task 1's `merge` and Task 2's import arm.
- **Review Focus:** 1 → Task 1 Step 2's escaping test and the limits; 2 → Task 2's flags test; 3 → Task 1's round-trip test with an empty `user`; 4 → Task 5's README paragraph plus Task 1's no-secret test; 5 → Task 2's overwrite test.
- **Type consistency:** `ExportSummary`/`ImportSummary` are defined once in Task 1, serialized through `Response::Export`/`Response::Import` in Task 2, re-exported as TypeScript types in Task 3 and read in Task 4 — the field names are `path`/`connections`/`keys` and `added`/`skipped`/`invalid`/`unresolved_keys` in all four places.
- **Risk:** the file-name sanitisation for `keys/<name>.pub` is the one place a key's `name` (arbitrary user text) meets the filesystem; Task 1 must refuse or rewrite a name containing a path separator, and its test should cover it.
