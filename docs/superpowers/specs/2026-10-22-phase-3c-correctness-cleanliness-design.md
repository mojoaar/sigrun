# Phase 3c: Correctness & Cleanliness Design Spec

**Goal:** Resolve remaining Medium findings (#27, #30, #31, #32, #36, #39) from the full-app review, focusing on correctness, interface safety, and dead code cleanup across frontend, core, and CLI.

## Scope & Findings Addressed

1. **Finding #27 (UI):** Language switch saves settings twice and can clobber concurrent changes.
   - `setLanguage(lng)` in `apps/desktop/src/i18n/index.ts` should only validate and switch the i18next language, keeping `<html lang>` updated.
   - `Settings.tsx`'s existing `onChange` handler continues to call `update({ language: e.target.value })`, which owns persistence of `settings.toml`.
   - Prevents stale reads in `setLanguage` from racing with other setting changes.

2. **Finding #36 (CLI):** CLI `connections edit --key <val>` and `connections add --key <val>` store key argument verbatim without name resolution.
   - When `--key` is provided in `sigrun connections edit` or `sigrun connections add`, fetch keys via `list_keys(&addr).await?` and resolve via `resolve_key(&keys, &arg)?`.
   - If ambiguous, returns an error. If matched, stores the canonical UUID key ID.

3. **Finding #31 (Core):** `AgentState` panics on a poisoned lock while `HostState` recovers from one.
   - In `crates/sigrun-core/src/agent/session.rs`, replace `.expect("agent lock poisoned")` with `.unwrap_or_else(|p| p.into_inner())` on all `self.keys` read/write locks.
   - Guarantees that a transient panic during one SSH key sign operation does not permanently brick the in-process agent for all future SSH authentication attempts.

4. **Finding #30 & #32 (Core & Model):** Dead public APIs in core and dead `agent_enabled` setting.
   - In `crates/sigrun-core/src/vault/store.rs`, remove dead public method `UnlockedVault::lock(self)` (which drops the key without clearing agent or active connection state). Remove unused `Vault::exists`.
   - In `crates/sigrun-core/src/agent/session.rs`, remove unused `AgentState::len` and `AgentState::is_empty`.
   - In `crates/sigrun-core/src/trust.rs`, remove unused `TrustStatus::is_trusted`, `is_mismatch`, `needs_confirmation`.
   - In `crates/sigrun-core/src/model.rs`, remove `pub agent_enabled: bool` from `Settings`.
   - In `apps/desktop/src/api.ts`, remove `agent_enabled: boolean` from `Settings` type and update test fixtures.

5. **Finding #39 (Core):** Enforce and verify that `Credential.has_password` is strictly server-derived.
   - Add regression tests verifying that a client sending `Credential { has_password: true, .. }` to `CredentialsAdd` with `password: None` results in stored `has_password: false`.
   - Document all changes in `CHANGELOG.md`.

## Non-Goals & Invariants
- No changes to Argon2 parameters or vault cryptographic structure.
- No changes to wire protocol serialization tags.
- Compatibility with existing `settings.toml` on disk: serde ignores `agent_enabled = true` on load due to `#[serde(default)]`.
