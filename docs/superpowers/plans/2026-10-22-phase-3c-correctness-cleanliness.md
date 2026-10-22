# Phase 3c (Correctness & Cleanliness) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix Medium findings #27, #30, #31, #32, #36, and #39: language switch settings race, CLI key name resolution, agent lock poison recovery, dead public core APIs, dead `agent_enabled` setting, and derived credential password verification.

**Architecture:** Refactor `setLanguage` to pure UI localization, add `resolve_key` resolution in `sigrun-cli` connection commands, recover poisoned `RwLock` in `AgentState`, remove dead APIs in `sigrun-core` and dead `agent_enabled` field, and pin `has_password` server-side derivation with tests.

**Tech Stack:** Rust 2021, TypeScript, React, Tokio, Vitest, Cargo.

**Spec:** `docs/superpowers/specs/2026-10-22-phase-3c-correctness-cleanliness-design.md`

## Global Constraints
- Gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
- Desktop gate: `npm test -- --run && npm run typecheck && npm run lint && npm run build`
- No secrets in argv, logs, or diagnostics.
- Conventional commits per task.

---

### Task 1: Decouple Language Switch from Settings Persistence (Finding #27)

**Files:**
- Modify: `apps/desktop/src/i18n/index.ts`
- Modify: `apps/desktop/src/i18n/index.test.ts`

- [ ] **Step 1: Update test in `apps/desktop/src/i18n/index.test.ts`**
Verify `setLanguage` switches language and updates `<html lang>` without calling Tauri invoke.

- [ ] **Step 2: Run frontend test to verify current state**
Run: `cd apps/desktop && npm test -- --run src/i18n/index.test.ts`

- [ ] **Step 3: Simplify `setLanguage` in `apps/desktop/src/i18n/index.ts`**
Remove `invoke("get_settings")` and `invoke("save_settings")`. Keep only:
```typescript
export async function setLanguage(lng: string): Promise<void> {
  const code = isLanguage(lng) ? lng : "en";
  await i18n.changeLanguage(code);
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cd apps/desktop && npm test -- --run src/i18n/index.test.ts`

- [ ] **Step 5: Commit**
```bash
git add apps/desktop/src/i18n/index.ts apps/desktop/src/i18n/index.test.ts
git commit -m "fix(desktop): decouple language switch from settings persistence"
```

---

### Task 2: CLI Key Name Resolution on Connections Edit and Add (Finding #36)

**Files:**
- Modify: `crates/sigrun-cli/src/cli.rs`
- Modify: `crates/sigrun-cli/tests/cli.rs`

- [ ] **Step 1: Write failing integration test in `crates/sigrun-cli/tests/cli.rs`**
Add test verifying that `connections edit --key <name>` and `connections add --key <name>` successfully resolve the key name to its key ID, and fail with clear error if ambiguous.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-cli --test cli`

- [ ] **Step 3: Implement key resolution in `ConnectionsCommand::Add` and `ConnectionsCommand::Edit`**
In `crates/sigrun-cli/src/cli.rs`:
```rust
let key_id = match key {
    Some(arg) => {
        let keys = list_keys(&addr).await?;
        Some(resolve_key(&keys, &arg)?)
    }
    None => None,
};
```
And in `ConnectionsCommand::Edit`:
```rust
if let Some(v) = key {
    let keys = list_keys(&addr).await?;
    connection.key_id = Some(resolve_key(&keys, &v)?);
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-cli --test cli`

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-cli/src/cli.rs crates/sigrun-cli/tests/cli.rs
git commit -m "fix(cli): resolve key names to IDs in connections add and edit"
```

---

### Task 3: Align `AgentState` Lock Poisoning Recovery with `HostState` (Finding #31)

**Files:**
- Modify: `crates/sigrun-core/src/agent/session.rs`

- [ ] **Step 1: Write test for poisoned lock recovery in `crates/sigrun-core/src/agent/session.rs`**
Add unit test verifying that `request_identities` and `contains_key_id` succeed and recover without panicking even after an internal write lock is poisoned.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-core agent::session::tests`

- [ ] **Step 3: Update `AgentState` lock recovery**
In `crates/sigrun-core/src/agent/session.rs`, replace `.expect("agent lock poisoned")` with `.unwrap_or_else(|p| p.into_inner())` on all `self.keys.read()` and `self.keys.write()` calls.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-core agent::session::tests`

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/agent/session.rs
git commit -m "fix(core): recover gracefully from poisoned agent state lock"
```

---

### Task 4: Remove Dead Public APIs and Unused `agent_enabled` Field (Findings #30 & #32)

**Files:**
- Modify: `crates/sigrun-core/src/vault/store.rs`
- Modify: `crates/sigrun-core/src/agent/session.rs`
- Modify: `crates/sigrun-core/src/trust.rs`
- Modify: `crates/sigrun-core/src/model.rs`
- Modify: `apps/desktop/src/api.ts`
- Modify: `apps/desktop/src/api.test.ts`
- Modify: `apps/desktop/src/App.test.tsx`
- Modify: `apps/desktop/src/views/Settings.test.tsx`

- [ ] **Step 1: Remove dead methods in core**
- `UnlockedVault::lock` and `Vault::exists` in `crates/sigrun-core/src/vault/store.rs`.
- `AgentState::len` and `AgentState::is_empty` in `crates/sigrun-core/src/agent/session.rs`.
- `TrustStatus::is_trusted`, `is_mismatch`, `needs_confirmation` in `crates/sigrun-core/src/trust.rs` (update unit tests in `trust.rs` to assert equality directly against enum variants).

- [ ] **Step 2: Remove `agent_enabled` field**
- Remove `pub agent_enabled: bool` from `Settings` in `crates/sigrun-core/src/model.rs`.
- Remove `agent_enabled: boolean` from `Settings` in `apps/desktop/src/api.ts`.
- Remove `agent_enabled` from test fixtures in `api.test.ts`, `App.test.tsx`, `Settings.test.tsx`.

- [ ] **Step 3: Run full Rust and desktop test gates**
Run: `cargo test --all-targets && cd apps/desktop && npm test -- --run`

- [ ] **Step 4: Commit**
```bash
git add crates/sigrun-core apps/desktop
git commit -m "refactor: remove dead public APIs and unused agent_enabled setting"
```

---

### Task 5: Server-Derived Invariant Verification & CHANGELOG (Finding #39)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add unit test in `crates/sigrun-core/src/host.rs`**
Add test in `host::tests` asserting that when `CredentialsAdd` receives `Credential { has_password: true, .. }` with `password: None`, the stored credential has `has_password: false`.

- [ ] **Step 2: Run test to verify it passes**
Run: `cargo test -p sigrun-core host::tests::adding_credential_ignores_client_has_password`

- [ ] **Step 3: Update `CHANGELOG.md`**
Record all Phase 3c changes under `## [0.5.0] - 2026-10-22` -> `### Fixed` and `### Changed`.

- [ ] **Step 4: Commit**
```bash
git add crates/sigrun-core/src/host.rs CHANGELOG.md
git commit -m "docs: document phase 3c correctness and cleanliness improvements"
```
