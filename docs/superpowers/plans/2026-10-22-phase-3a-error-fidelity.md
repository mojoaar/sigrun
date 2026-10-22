# Phase 3a: Error Fidelity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve error clarity and safety across provisioning, reachability probes, and desktop UI views.

**Architecture:** Sanitize remote error messages in `provision.rs` using fixed exit-code descriptions; validate login user in `host.rs` provisioning; eliminate dead probe stderr output; update `Keys.tsx` and `Credentials.tsx` to surface specific host error messages.

**Tech Stack:** Rust (sigrun-core), React / TypeScript (apps/desktop).

**Spec:** `docs/superpowers/specs/2026-10-22-phase-3a-error-fidelity-design.md`

## Global Constraints
- Secrets must never appear in error messages or logs.
- Gate must pass for all tasks: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` and frontend checks `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- Every user-facing UI change must keep i18n parity intact across all 7 locales.
- Conventional commits for every task.

## Review Focus
- Hostile remote stderr injection into error dialogs during provisioning.
- Misleading password auth failure when username is empty.
- Probe timeouts generating noise on stderr.
- Swallowed error details in Keys view when export or removal fails.
- Swallowed error details in Credentials view when credential removal fails.

---

### Task 1: Sanitize Remote Hardening and Provisioning Error Messages (Finding #25)

**Files:**
- Modify: `crates/sigrun-core/src/provision.rs:257-310`
- Test: `crates/sigrun-core/src/provision.rs`

**Interfaces:**
- `harden(handle: &mut Handle<Gate>) -> Result<Hardening>`
- `install_keys(opts: &ProvisionOptions, keys: &[ProvisionKey]) -> Result<ProvisionOutcome>`

- [ ] **Step 1: Write the failing tests**
Add unit tests in `crates/sigrun-core/src/provision.rs` (under `mod tests`) verifying that:
1. Hardening exit codes 5, 6, 7, 8 produce expected static error messages without echoing stderr.
2. Exit codes 3 and 4 produce expected `Hardening::Skipped` messages without echoing stderr.
3. `install_keys` exit status failure does not interpolate remote stderr into `Error::Config`.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-core provision::tests`
Expected: FAIL

- [ ] **Step 3: Implement error sanitization in `crates/sigrun-core/src/provision.rs`**
Replace remote `stderr` interpolation with fixed mapped messages for exit codes 3..=8, unknown exit codes, and missing exit status.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-core provision::tests`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/provision.rs
git commit -m "fix(core): sanitize remote sshd hardening error messages"
```

---

### Task 2: Reject Empty Username Before Provisioning (Finding #26)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs:1260-1275`
- Test: `crates/sigrun-core/src/host.rs`

**Interfaces:**
- `provision_connection(state: &HostState, id: String, key_ids: Vec<String>, password: String, disable_password_auth: bool) -> Result<Response>`

- [ ] **Step 1: Write the failing test**
In `crates/sigrun-core/src/host.rs` under `mod tests`:
Add `provisioning_without_a_user_is_invalid_input`:
Verify that calling `Request::ProvisionConnection` on a connection with an empty user (`conn.user = ""` and no credential user) fails immediately with `Error::InvalidInput("provisioning requires a username")`.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-core host::tests::provisioning_without_a_user_is_invalid_input`
Expected: FAIL

- [ ] **Step 3: Implement validation in `crates/sigrun-core/src/host.rs`**
In `provision_connection`, right after resolving `auth = effective(&conn, &data.credentials)?`:
Check `if auth.user.trim().is_empty() { return Err(Error::InvalidInput("provisioning requires a username".into())); }`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-core host::tests::provisioning_without_a_user_is_invalid_input`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/host.rs
git commit -m "fix(core): reject empty username before provisioning"
```

---

### Task 3: Remove Dead `eprintln!` in Reachability Probe (Finding #37)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs:1180-1195`
- Test: `crates/sigrun-core/src/host.rs`

**Interfaces:**
- `connections_connect(state: &HostState, id: String, print_only: bool, force: bool) -> Result<Response>`

- [ ] **Step 1: Write the test / verify existing probe tests**
Verify `terminal::tests::only_proven_probe_failures_block_the_launch` and `host::tests`.

- [ ] **Step 2: Remove `eprintln!` statement**
In `crates/sigrun-core/src/host.rs`:
Delete lines 1188-1191 (`eprintln!("sigrun: probe inconclusive, letting ssh try: {}", failure.message());`).

- [ ] **Step 3: Run full core tests**
Run: `cargo test -p sigrun-core host::tests`
Expected: PASS with no stderr noise.

- [ ] **Step 4: Commit**
```bash
git add crates/sigrun-core/src/host.rs
git commit -m "fix(core): remove dead eprintln on inconclusive reachability probe"
```

---

### Task 4: Surface Specific Host Errors in Keys and Credentials Views (Finding #38)

**Files:**
- Modify: `apps/desktop/src/views/Keys.tsx`
- Modify: `apps/desktop/src/views/Credentials.tsx`
- Test: `apps/desktop/src/views/Keys.test.tsx`
- Test: `apps/desktop/src/views/Credentials.test.tsx`
- Modify: `CHANGELOG.md`

**Interfaces:**
- `Keys` component error banner displaying specific `CommandError.message`.
- `Credentials` component error banner displaying specific `CommandError.message`.

- [ ] **Step 1: Write failing frontend tests**
In `apps/desktop/src/views/Keys.test.tsx`:
Add a test asserting that when `removeKey` or `exportKey` rejects with a `CommandError` (e.g. `{ code: "invalid_input", message: "cannot export inside the SSH directory" }`), the error message `"cannot export inside the SSH directory"` is rendered in the alert element.
In `apps/desktop/src/views/Credentials.test.tsx`:
Add a test asserting that when `removeCredential` rejects with a `CommandError` (e.g. `{ code: "conflict", message: "credential is used by 2 connections" }`), that exact message is rendered in the alert element.

- [ ] **Step 2: Run frontend tests to verify failure**
Run: `cd apps/desktop && npm test -- --run`
Expected: FAIL (renders generic error text instead of specific message).

- [ ] **Step 3: Implement specific error handling in `Keys.tsx` and `Credentials.tsx`**
Update error state to `string | null` and set `setError(e && e.code !== "internal" ? e.message : "")`.
Render `{error !== null && <p role="alert" className="text-danger">{error || t("common.error")}</p>}`.
Update `CHANGELOG.md` with Keep-a-Changelog entries for Phase 3a under `## [0.5.0] - 2026-10-22` -> `### Fixed`.

- [ ] **Step 4: Run frontend and backend test suites**
Run: `npm test -- --run && npm run typecheck && npm run lint && npm run build`
Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
Expected: All tests pass.

- [ ] **Step 5: Commit**
```bash
git add apps/desktop/src/views/Keys.tsx apps/desktop/src/views/Credentials.tsx apps/desktop/src/views/Keys.test.tsx apps/desktop/src/views/Credentials.test.tsx CHANGELOG.md
git commit -m "fix(desktop): display specific host error messages in keys and credentials views"
```
