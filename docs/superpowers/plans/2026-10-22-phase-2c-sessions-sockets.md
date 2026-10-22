# Phase 2c: Sessions & Sockets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve Findings #10, #17, #14, #15, and #18 from the full application review, hardening process tracking, IPC socket creation/permissions, and export bundle file safety.

**Architecture:** Harden host session registration and sweeping in `host.rs`, set mode `0600` on the Unix IPC listener in `ipc/server.rs`, add backoff and shutdown handling in Windows named pipe creation in `ipc/server.rs`, and use atomic `create_new(true)` with directory checks in `bundle.rs` and `host.rs`.

**Tech Stack:** Rust (Tokio, standard library filesystem & process APIs, libc on Unix).

**Spec:** `docs/superpowers/specs/2026-10-22-phase-2c-sessions-sockets-design.md`

## Global Constraints

- MSRV 1.90, Rust 2021.
- No new external dependencies without strong justification.
- Workspace gates: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` and `npm test -- --run && npm run typecheck && npm run lint && npm run build` in `apps/desktop`.
- Conventional commit messages per task.
- Secrets never exposed or logged.

## Review Focus

- PID validation must prevent registration of PID 0, 1, or host process PID.
- Sweeper must only signal alive processes that are confirmed orphaned (`parent <= 1`).
- `ConnectionEnded` must reliably clean up both active set and session map.
- Unix IPC socket must have mode `0600` immediately after bind.
- Zip export must fail if destination already exists without truncating.

---

### Task 1: Session Lifecycle Hardening and PID Validation

**Files:**
- Modify: `crates/sigrun-core/src/host.rs`
- Test: `crates/sigrun-core/src/host.rs`

**Interfaces:**
- `Request::ConnectionEnded { id }`
- `Request::SessionStarted { id, pid }`
- `HostState::forget_session(&self, id: &str)`
- `HostState::sweep_dead_sessions(&self) -> bool`

- [ ] **Step 1: Write failing tests for session cleanup and PID validation**
Add tests in `crates/sigrun-core/src/host.rs`:
1. `connection_ended_forgets_session`: Verifies handling `Request::ConnectionEnded { id }` removes `id` from `session_map`.
2. `session_started_rejects_invalid_pids_and_inactive_connections`: Verifies `SessionStarted` fails with `Error::InvalidInput` for `pid == 0`, `pid == 1`, `pid == std::process::id()`, and when `!state.is_active(&id)`.
3. `dead_non_orphaned_session_is_not_signaled`: Verifies `sweep_dead_sessions` cleans up dead sessions without sending SIGHUP.

- [ ] **Step 2: Run tests to verify they fail**
Run: `cargo test -p sigrun-core host::tests::connection_ended_forgets_session`
Expected: FAIL

- [ ] **Step 3: Implement session cleanup and validation in `crates/sigrun-core/src/host.rs`**
1. In `handle_inner` for `Request::ConnectionEnded { id }`:
   Call `state.forget_session(&id);` alongside `state.clear_active(&id);`.
2. In `handle_inner` for `Request::SessionStarted { id, pid }`:
   Validate `state.is_active(&id)` and `pid > 1 && pid != std::process::id()`. If invalid, return `Err(Error::InvalidInput(...))`.
3. In `sweep_dead_sessions`:
   Only call `end_orphaned_session(*pid)` if `session_is_orphaned(*pid)`.
4. In `end_orphaned_session`:
   Guard with `if pid > 1 && pid != std::process::id()`.

- [ ] **Step 4: Run tests to verify they pass**
Run: `cargo test -p sigrun-core host::tests`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/host.rs
git commit -m "fix(core): harden session tracking and validate session started pids"
```

---

### Task 2: Unix Host IPC Socket Permissions

**Files:**
- Modify: `crates/sigrun-core/src/ipc/server.rs`
- Test: `crates/sigrun-core/src/ipc/mod.rs`

**Interfaces:**
- `serve_unix(addr: &Path, state: Arc<HostState>) -> Result<()>`

- [ ] **Step 1: Write failing test for IPC socket permissions**
In `crates/sigrun-core/src/ipc/mod.rs`:
Add test `unix_socket_has_mode_0600` asserting that a bound Unix IPC socket file has permissions mode `0o600`.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-core ipc::tests::unix_socket_has_mode_0600`
Expected: FAIL

- [ ] **Step 3: Implement mode `0o600` setting on socket in `crates/sigrun-core/src/ipc/server.rs`**
Immediately after `tokio::net::UnixListener::bind(addr)?`:
```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(addr, std::fs::Permissions::from_mode(0o600))?;
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-core ipc::tests::unix_socket_has_mode_0600`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/ipc/server.rs crates/sigrun-core/src/ipc/mod.rs
git commit -m "fix(core): set mode 0600 on host unix ipc socket"
```

---

### Task 3: Windows Named Pipe Creation Backoff

**Files:**
- Modify: `crates/sigrun-core/src/ipc/server.rs`

**Interfaces:**
- `serve_windows(addr: &Path, state: Arc<HostState>) -> Result<()>`

- [ ] **Step 1: Review Windows named pipe loop in `crates/sigrun-core/src/ipc/server.rs`**
Inspect `serve_windows` error handling on `ServerOptions::new().create(name)`.

- [ ] **Step 2: Implement backoff and shutdown check on error**
Update the error match arm:
```rust
let server = match ServerOptions::new().create(name) {
    Ok(server) => server,
    Err(_) => {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
        }
    }
};
```

- [ ] **Step 3: Run cargo check and clippy**
Run: `cargo check --all-targets && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: PASS

- [ ] **Step 4: Commit**
```bash
git add crates/sigrun-core/src/ipc/server.rs
git commit -m "fix(core): add backoff and shutdown check on windows pipe creation error"
```

---

### Task 4: Export TOCTOU and Overwrite Safety

**Files:**
- Modify: `crates/sigrun-core/src/bundle.rs`, `crates/sigrun-core/src/host.rs`
- Test: `crates/sigrun-core/src/bundle.rs`, `crates/sigrun-core/src/host.rs`

**Interfaces:**
- `bundle::write_zip(bundle: &Bundle, file: &Path) -> Result<()>`
- `host::export_bundle(&self, path: &str, zipped: bool) -> Result<ExportSummary>`

- [ ] **Step 1: Write failing tests for export overwrite protection**
In `crates/sigrun-core/src/bundle.rs` or `crates/sigrun-core/src/host.rs`:
1. `write_zip_fails_if_destination_file_exists`: Asserts `write_zip` returns `Error::AlreadyExists` if the zip file already exists, rather than truncating it.
2. `export_folder_refuses_existing_keys_dir`: Asserts `export_bundle` refuses with `Error::AlreadyExists` if `keys/` already exists in destination.

- [ ] **Step 2: Run tests to verify they fail**
Run: `cargo test -p sigrun-core bundle::tests::write_zip_fails_if_destination_file_exists`
Expected: FAIL

- [ ] **Step 3: Implement atomic zip creation and keys directory pre-check**
1. In `crates/sigrun-core/src/bundle.rs`:
   Update `write_zip` to use `std::fs::OpenOptions::new().write(true).create_new(true).open(file)` and map `ErrorKind::AlreadyExists` to `Error::AlreadyExists(file.display().to_string())`.
2. In `crates/sigrun-core/src/host.rs`:
   Update `export_bundle` to check `[bundle::EXPORT_FILE, bundle::README_FILE, bundle::KEY_DIR]`.

- [ ] **Step 4: Run tests to verify they pass**
Run: `cargo test -p sigrun-core bundle::tests && cargo test -p sigrun-core host::tests::export`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/bundle.rs crates/sigrun-core/src/host.rs
git commit -m "fix(core): prevent export toctou file truncation and keys dir clobbering"
```

---

### Task 5: Documentation and CHANGELOG Updates

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add entries under `## [0.5.0] - 2026-10-22` in `CHANGELOG.md`**
Under `### Fixed`:
- `ConnectionEnded` now cleans up watched sessions alongside active state to prevent stale sweep clears on quick reconnects.
- `SessionStarted` rejects invalid PIDs (0, 1, own process) and inactive connections, preventing arbitrary process group signaling.
- Windows IPC server throttles retry loops and respects shutdown when named pipe creation fails.
- Host Unix IPC socket explicitly enforces `0600` permissions.
- Bundle export uses atomic file creation for `.zip` files and guards against pre-existing `keys/` directories.

- [ ] **Step 2: Verify all project gates**
Run:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --all-targets`
- `npm test -- --run`
- `npm run typecheck`
- `npm run lint`
- `npm run build`

- [ ] **Step 3: Commit**
```bash
git add CHANGELOG.md
git commit -m "docs: document phase 2c sessions and sockets hardening"
```
