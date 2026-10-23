# Phase 5 (Features: F1–F3, G1, G3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the top quick-win and missing features from the review: copy SSH command button on connection row (F1), CLI credentials password (F2), CLI keys export (F3), vault passphrase change (G1), and vault backup (G3).

**Architecture:**
- **F1 (Copy SSH command):** `apps/desktop/src/api.ts` exports `printCommand(id: string)` invoking Tauri command `connect(id, printOnly: true, force: false)`. `Connections.tsx` adds a copy icon button on each row that writes the returned command string to `navigator.clipboard.writeText`, showing a transient "Copied!" feedback state.
- **F2 (CLI credentials password):** `sigrun credentials password <credential> [--password-stdin] [--clear]` in `sigrun-cli`. Mirrors `sigrun connections password`: resolves credential by ID or label, reads password via `rpassword` (or stdin when `--password-stdin`, never on argv per AGENTS.md), and sends existing `Request::CredentialsSetPassword { id, password }`.
- **F3 (CLI keys export):** `sigrun keys export <key> --out <path>` in `sigrun-cli`. Resolves key by ID or name, and sends existing `Request::KeysExport { id, path }`.
- **G1 (Vault passphrase change):**
  - Core: `UnlockedVault::change_passphrase(&mut self, new_passphrase: &Secret) -> Result<()>` in `crates/sigrun-core/src/vault/store.rs`. Derives new key with `random_salt()` and pinned Argon2id parameters, re-seals payload, writes atomically at 0600, with memory rollback if persist fails.
  - IPC: `Request::VaultChangePassphrase { current_passphrase: Secret, new_passphrase: Secret }`. `host.rs` verifies `current_passphrase` against active vault (or unlocks it), calls `change_passphrase`, and updates active state.
  - CLI: `sigrun vault change-passphrase [--password-stdin]` prompting for current and new passphrases with confirmation.
  - Desktop: "Change passphrase" modal in `Settings.tsx`.
- **G3 (Vault backup):**
  - Core: `Request::VaultBackup { destination: PathBuf }`. Verifies destination is not inside `~/.ssh`, verifies destination does not already exist, and writes atomic copy of sealed `vault.json` at 0600.
  - CLI: `sigrun vault backup [--out <path>]` (defaults to `./sigrun-vault-<timestamp>.bak`).
  - Desktop: "Backup vault" button in `Settings.tsx` using native save file dialog.

**Tech Stack:** Rust (Tokio, Argon2id, XChaCha20-Poly1305, Clap, rpassword), React 19, TypeScript, Tailwind CSS, i18next.

**Spec:** `docs/superpowers/specs/2026-10-23-phase-5-features-design.md`

## Global Constraints
- Secrets live only in the vault. Never in argv.
- The vault uses Argon2id (m=65536, t=3, p=4, 16-byte salt) with XChaCha20-Poly1305 and a fresh 24-byte nonce.
- `~/.ssh` is read-only. Refuse writing backups or keys inside `~/.ssh`.
- MSRV 1.90, Rust 2021.
- All user-facing strings go through `t(...)` in all seven locales (`en, da, de, es, fr, nb, sv`).

---

### Task 1: F1 — Copy SSH Command Button on Connection Row

**Files:**
- Modify: `apps/desktop/src/api.ts`
- Modify: `apps/desktop/src/api.test.ts`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/views/Connections.test.tsx`
- Modify: `apps/desktop/src/i18n/*.json` (all 7 locales)

- [ ] In `apps/desktop/src/api.ts`, add `printCommand = (id: string) => call<string>("connect", { id, printOnly: true, force: false })`.
- [ ] In `apps/desktop/src/views/Connections.tsx`, add a copy button next to Connect on connection row with `t("connections.copyCommand")`.
- [ ] On click, call `api.printCommand(c.id)` and write to `navigator.clipboard.writeText(cmd)`. Show transient "copied" state (e.g. checkmark icon / tooltip) for 2 seconds.
- [ ] Add i18n keys `connections.copyCommand` and `connections.commandCopied` across all 7 locales (`da, de, en, es, fr, nb, sv`) with full parity.
- [ ] Update `Connections.test.tsx` to assert click invokes `printCommand` and writes to clipboard.

---

### Task 2: F2 & F3 — CLI Credentials Password & Keys Export

**Files:**
- Modify: `crates/sigrun-cli/src/cli.rs`
- Modify: `crates/sigrun-cli/tests/cli.rs`

- [ ] In `crates/sigrun-cli/src/cli.rs`, add `CredentialsCommand::Password { credential: String, password_stdin: bool, clear: bool }`.
- [ ] Resolve credential ID or label; prompt via `read_passphrase("password: ", password_stdin, false)` unless `--clear`.
- [ ] Send `Request::CredentialsSetPassword { id, password }` to host IPC.
- [ ] In `crates/sigrun-cli/src/cli.rs`, add `KeysCommand::Export { key: String, out: PathBuf }`.
- [ ] Resolve key ID or name using `resolve_key`, send `Request::KeysExport { id, path: out }` to host IPC.
- [ ] Add integration tests in `crates/sigrun-cli/tests/cli.rs` verifying `sigrun credentials password` (set & clear) and `sigrun keys export`.

---

### Task 3: G1 — Vault Passphrase Change in Core and CLI

**Files:**
- Modify: `crates/sigrun-core/src/vault/store.rs`
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-cli/src/cli.rs`
- Test: `crates/sigrun-core/src/vault/store.rs`
- Test: `crates/sigrun-cli/tests/cli.rs`

- [ ] In `crates/sigrun-core/src/vault/store.rs`, implement `UnlockedVault::change_passphrase(&mut self, new_passphrase: &Secret) -> Result<()>`.
- [ ] In `crates/sigrun-core/src/ipc/protocol.rs`, add `Request::VaultChangePassphrase { current_passphrase: Secret, new_passphrase: Secret }`.
- [ ] In `crates/sigrun-core/src/host.rs`, handle `Request::VaultChangePassphrase`: verify current passphrase unlocks the store, invoke `change_passphrase`, update memory cache, reset unlock backoff.
- [ ] In `crates/sigrun-cli/src/cli.rs`, add `VaultCommand::ChangePassphrase { password_stdin: bool }` prompting for current passphrase, new passphrase, and confirmation.
- [ ] Add unit test in `store.rs` and CLI integration test in `tests/cli.rs`.

---

### Task 4: G3 — Vault Backup in Core and CLI

**Files:**
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-cli/src/cli.rs`
- Test: `crates/sigrun-core/src/host.rs`
- Test: `crates/sigrun-cli/tests/cli.rs`

- [ ] In `crates/sigrun-core/src/ipc/protocol.rs`, add `Request::VaultBackup { destination: PathBuf }`.
- [ ] In `crates/sigrun-core/src/host.rs`, implement `vault_backup`:
  - Check destination is not inside `~/.ssh`.
  - Check destination does not already exist (`create_new(true)` semantics).
  - Copy sealed `paths.vault_file()` to destination atomically at mode 0600.
- [ ] In `crates/sigrun-cli/src/cli.rs`, add `VaultCommand::Backup { out: Option<PathBuf> }`, defaulting to `./sigrun-vault-<timestamp>.bak`.
- [ ] Add unit test in `host.rs` and integration test in `tests/cli.rs`.

---

### Task 5: Desktop UI for Vault Passphrase Change & Backup (G1 & G3)

**Files:**
- Modify: `apps/desktop/src/api.ts`
- Modify: `apps/desktop/src/api.test.ts`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/views/Settings.tsx`
- Modify: `apps/desktop/src/views/Settings.test.tsx`
- Modify: `apps/desktop/src/i18n/*.json` (all 7 locales)

- [ ] In `commands.rs` and `api.ts`, add `vaultChangePassphrase(currentPassphrase, newPassphrase)` and `vaultBackup(destination)`.
- [ ] In `Settings.tsx`, add a "Vault" section with:
  - "Change passphrase" button opening modal: inputs for Current Passphrase, New Passphrase, Confirm New Passphrase, with validation and error display.
  - "Backup vault" button calling Tauri `save` dialog and sending destination to `vaultBackup`.
- [ ] Add translations for all new modal and button strings in all 7 locales (`da, de, en, es, fr, nb, sv`) with full parity.
- [ ] Add unit tests in `Settings.test.tsx`.

---

### Task 6: Documentation and CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `README.md`
- Modify: `apps/desktop/src/views/Help.tsx`
- Modify: `apps/desktop/src/i18n/*.json` (all 7 locales)

- [ ] Document all new features in `CHANGELOG.md` under `## [0.5.0] - 2026-10-22` -> `### Added`, keeping all audit fixes and top features bundled for the upcoming 0.5.0 release.
- [ ] Update `README.md` with the new CLI commands (`sigrun credentials password`, `sigrun keys export`, `sigrun vault change-passphrase`, `sigrun vault backup`).
- [ ] Update `Help.tsx` CLI command reference across all 7 locales.
- [ ] Verify full workspace gates: `cargo fmt`, `clippy -D warnings`, `cargo test --all-targets`, `npm test`, `typecheck`, `lint`, and `build`.
