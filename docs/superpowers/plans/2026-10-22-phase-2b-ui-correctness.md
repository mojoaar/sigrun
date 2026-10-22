# Phase 2b: UI Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement UI correctness and frontend hardening fixes (Findings #12, #13, #19, #29, #41) across desktop frontend, i18n bundles, and core IPC protocol.

**Architecture:** Frontend theme fallback in `themes/index.ts`, i18n key normalization in all 7 locales, backdrop click dismissal in `Connections.tsx`, failed unlock counter & redacted logging in `sigrun-core` and `VaultGate.tsx`, and typed IPC protocol for settings.

**Tech Stack:** React 19, TypeScript, Vitest, Rust 2021, Tokio, Tauri 2.

**Spec:** `docs/superpowers/specs/2026-10-22-phase-2b-ui-correctness-design.md`

## Global Constraints

- Rust 2021, MSRV 1.90. No new dependencies.
- Every user-facing UI string must be localized via `t(...)` across all 7 locales (`en`, `da`, `de`, `es`, `fr`, `nb`, `sv`).
- Logging of vault unlock attempts must be strictly redacted: never log or expose passphrases.
- Full gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` and `npm test -- --run && npm run typecheck && npm run lint && npm run build` in `apps/desktop`.

## Review Focus

1. Unknown/tampered theme name in settings should cleanly default to `"nordlys"` without throwing or locking UI.
2. In the "Add to vault" dialog, the body description text must render localized text in all 7 languages, never `"keys.addToVaultBody"`.
3. In the connection error modal, clicking the overlay backdrop or pressing Escape must close the modal without triggering "Connect anyway".
4. Repeated failed vault unlock attempts must include attempt count in error response payload without leaking passphrases.
5. Settings read and save must use typed IPC protocol without JSON-smuggling through `Response::Command`.

---

### Task 1: Theme Fallback Guard (#12)

**Files:**
- Modify: `apps/desktop/src/themes/index.ts`
- Test: `apps/desktop/src/themes/index.test.ts`

**Interfaces:**
- `applyTheme(name: ThemeName, mode: Mode): void`

- [ ] **Step 1: Write failing test in `apps/desktop/src/themes/index.test.ts`**

Add a test that passes an invalid theme string (cast as `ThemeName`) to `applyTheme` and verifies it applies `DEFAULT_THEME` (`"nordlys"`) tokens without throwing.

```ts
test("falls back to DEFAULT_THEME when given an invalid theme name", () => {
  expect(() => applyTheme("invalid_theme" as any, "dark")).not.toThrow();
  expect(document.documentElement.getAttribute("data-theme")).toBe("nordlys");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/desktop && npx vitest run src/themes/index.test.ts`
Expected: FAIL with `TypeError: Cannot read properties of undefined (reading 'dark')`

- [ ] **Step 3: Implement fallback in `apps/desktop/src/themes/index.ts`**

Update `applyTheme`:
```ts
export function applyTheme(name: ThemeName, mode: Mode): void {
  const root = document.documentElement;
  const resolvedName = name in PALETTES ? name : DEFAULT_THEME;
  const tokens = PALETTES[resolvedName][mode];
  for (const t of TOKENS) root.style.setProperty(`--color-${t}`, tokens[t]);
  root.setAttribute("data-theme", resolvedName);
  root.setAttribute("data-mode", mode);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd apps/desktop && npx vitest run src/themes/index.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/themes/index.ts apps/desktop/src/themes/index.test.ts
git commit -m "fix(desktop): fall back to default theme on invalid theme name"
```

---

### Task 2: Fix `keys.addToVaultBody` i18n Key (#13)

**Files:**
- Modify: `apps/desktop/src/i18n/{en,da,de,es,fr,nb,sv}.json`
- Test: `apps/desktop/src/views/Keys.test.tsx`

**Interfaces:**
- Consumes: translation bundles under key `"keys.addToVaultBody"`

- [ ] **Step 1: Write failing test in `apps/desktop/src/views/Keys.test.tsx`**

Add an assertion in `Keys.test.tsx` verifying that when the "Add to vault" dialog is opened, it renders the text content of `keys.addToVaultBody` rather than the literal string `"keys.addToVaultBody"`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/desktop && npx vitest run src/views/Keys.test.tsx`
Expected: FAIL or assertion mismatch

- [ ] **Step 3: Move `addToVaultBody` to `"keys"` in all 7 i18n JSON files**

In `en.json`, `da.json`, `de.json`, `es.json`, `fr.json`, `nb.json`, and `sv.json`:
Move `"addToVaultBody"` from `"vault"` section into `"keys"` section.

- [ ] **Step 4: Run test and i18n parity check**

Run: `cd apps/desktop && npx vitest run src/i18n/index.test.ts src/views/Keys.test.tsx`
Expected: PASS (all 7 locales match key parity)

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/i18n/ apps/desktop/src/views/Keys.test.tsx
git commit -m "fix(desktop): move addToVaultBody to keys namespace in all locales"
```

---

### Task 3: Unreachable Modal Backdrop Click & Escape Dismissal (#41)

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `unreachable` state in `Connections.tsx`

- [ ] **Step 1: Write failing test in `apps/desktop/src/views/Connections.test.tsx`**

Test that clicking the backdrop overlay or pressing Escape dismisses the unreachable dialog without retrying connection.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/desktop && npx vitest run src/views/Connections.test.tsx`
Expected: FAIL

- [ ] **Step 3: Implement backdrop click dismissal in `apps/desktop/src/views/Connections.tsx`**

Update the container div around the unreachable dialog:
```tsx
{unreachable && (
  <div
    className="fixed inset-0 z-20 flex items-center justify-center bg-black/50"
    onClick={(e) => {
      if (e.target === e.currentTarget) setUnreachable(null);
    }}
  >
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd apps/desktop && npx vitest run src/views/Connections.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx
git commit -m "fix(desktop): allow dismissing unreachable modal on backdrop click and Escape"
```

---

### Task 4: Surface Failed Vault Unlock Attempts (#19)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `apps/desktop/src/views/VaultGate.tsx`
- Modify: `apps/desktop/src/i18n/{en,da,de,es,fr,nb,sv}.json`
- Test: `crates/sigrun-core/src/host.rs` (unit test)
- Test: `apps/desktop/src/views/VaultGate.test.tsx`

**Interfaces:**
- Produces: `Response::Error { code: "invalid_passphrase", message: ..., payload: Some({ "attempts": u32, "backoff_ms": u64 }) }`

- [ ] **Step 1: Write failing test in `crates/sigrun-core/src/host.rs`**

Add a unit test verifying that failed `VaultUnlock` requests return a response payload containing `attempts` and `backoff_ms`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-core host::tests::failed_unlock_returns_attempt_count_in_payload`
Expected: FAIL

- [ ] **Step 3: Implement logging and payload in `crates/sigrun-core/src/host.rs`**

In `Request::VaultUnlock` error handling:
1. Print redacted log:
   ```rust
   eprintln!("sigrun-host: failed vault unlock attempt {} (backoff {}ms)", attempts, millis);
   ```
2. Return error with payload:
   ```rust
   return Ok(Response::Error {
       code: "invalid_passphrase".to_string(),
       message: e.to_string(),
       payload: Some(serde_json::json!({
           "attempts": attempts,
           "backoff_ms": millis,
       })),
   });
   ```

- [ ] **Step 4: Surface attempts in `VaultGate.tsx` and i18n files**

In `VaultGate.tsx`:
Store `failedAttempts: number | null`. If `err.payload?.attempts > 1`, display:
```tsx
{failedAttempts && failedAttempts > 1 && (
  <p role="status" className="text-xs text-muted">
    {t("vault.failedAttempts", { count: failedAttempts })}
  </p>
)}
```
Add `vault.failedAttempts` key across all 7 JSON files:
- `en`: `"Failed unlock attempts: {{count}}."`
- `da`: `"Mislykkede oplåsningsforsøg: {{count}}."`
- `de`: `"Fehlgeschlagene Entsperrversuche: {{count}}."`
- `es`: `"Intentos fallidos de desbloqueo: {{count}}."`
- `fr`: `"Tentatives de déverrouillage échouées : {{count}}."`
- `nb`: `"Mislykkede opplåsingsforsøk: {{count}}."`
- `sv`: `"Misslyckade upplåsningsförsök: {{count}}."`

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p sigrun-core` and `cd apps/desktop && npx vitest run src/views/VaultGate.test.tsx src/i18n/index.test.ts`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/sigrun-core/ apps/desktop/src/views/VaultGate.tsx apps/desktop/src/views/VaultGate.test.tsx apps/desktop/src/i18n/
git commit -m "fix(core): surface failed unlock attempts in error payload and UI"
```

---

### Task 5: Typed IPC `SettingsGet` / `SettingsSet` (#29) and Changelog

**Files:**
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/api.ts`
- Modify: `CHANGELOG.md`
- Test: `crates/sigrun-core/src/ipc/mod.rs`
- Test: `apps/desktop/src/api.test.ts`

**Interfaces:**
- Produces: `Request::SettingsGet`, `Request::SettingsSet { settings: Box<Settings> }`, `Response::Settings(Settings)`

- [ ] **Step 1: Write failing test in `crates/sigrun-core/src/ipc/mod.rs`**

Add unit test testing round-trip `SettingsGet` and `SettingsSet` via IPC handler.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-core ipc::tests::settings_roundtrip_over_ipc`
Expected: FAIL (types do not exist)

- [ ] **Step 3: Implement `SettingsGet` and `SettingsSet` in `protocol.rs` and `host.rs`**

1. In `protocol.rs`:
   Add `Request::SettingsGet`, `Request::SettingsSet { settings: Box<Settings> }`, and `Response::Settings(Settings)`.
2. In `host.rs`:
   Handle `Request::SettingsGet` -> `Ok(Response::Settings(state.store.load_settings()?))`.
   Handle `Request::SettingsSet { settings }` -> `state.store.save_settings(&settings)?; Ok(Response::Ok)`.
3. In `commands.rs`:
   `get_settings_inner` calls `call(state, Request::SettingsGet).await`.
   `save_settings_inner` calls `call(state, Request::SettingsSet { settings: Box::new(settings) }).await`.
4. In `api.ts`:
   `export const getSettings = () => call<Settings>("get_settings");` (remove `JSON.parse` and cast).
5. Update `CHANGELOG.md` with entries for Phase 2b fixes.

- [ ] **Step 4: Run full workspace gates**

Run:
`cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
`cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/ apps/desktop/ CHANGELOG.md
git commit -m "feat(core): use typed IPC protocol for settings get and save"
```
