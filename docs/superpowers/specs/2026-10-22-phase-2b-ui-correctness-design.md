# Phase 2b — UI Correctness Design

## Goal

Resolve the 5 frontend and UI-related findings from the full-app audit:
- Finding #12 (High): Fallback for invalid theme in `settings.toml`.
- Finding #13 (High): Missing `keys.addToVaultBody` i18n key in all 7 locales.
- Finding #19 (High): Surface failed vault unlock attempts and log failures securely.
- Finding #29 (Medium): Typed IPC protocol for `SettingsGet` and `SettingsSet`.
- Finding #41 (Medium): Unreachable-host connection modal backdrop click & Escape dismissal.

---

## 1. Theme Fallback Guard (#12)

### Context & Problem
In `apps/desktop/src/themes/index.ts`, `applyTheme(name: ThemeName, mode: Mode)` executes:
```ts
const tokens = PALETTES[name][mode];
```
If `name` is unrecognized (e.g. invalid string manually placed in `settings.toml`), `PALETTES[name]` is `undefined`, causing an uncaught `TypeError` during `App.refresh()`. The error is caught by `App`'s top-level error boundary which sets `vaultState("Locked")`, silently trapping the user in a bogus locked vault gate.

### Design
1. In `apps/desktop/src/themes/index.ts`:
   - Define a fallback helper or direct check:
     ```ts
     const palette = PALETTES[name] ?? PALETTES[DEFAULT_THEME];
     const resolvedName = PALETTES[name] ? name : DEFAULT_THEME;
     const tokens = palette[mode];
     for (const t of TOKENS) root.style.setProperty(`--color-${t}`, tokens[t]);
     root.setAttribute("data-theme", resolvedName);
     root.setAttribute("data-mode", mode);
     ```
2. Any invalid theme name gracefully falls back to `DEFAULT_THEME` (`"nordlys"`).

---

## 2. Missing `keys.addToVaultBody` Key in All 7 Locales (#13)

### Context & Problem
In `apps/desktop/src/views/Keys.tsx:63`, the "Add to vault" dialog renders:
```tsx
<p className="text-sm text-muted">{t("keys.addToVaultBody")}</p>
```
The translation key `addToVaultBody` currently exists under `"vault"` in the 7 i18n files (`apps/desktop/src/i18n/{en,da,de,es,fr,nb,sv}.json`), meaning `t("keys.addToVaultBody")` resolves to the literal string `"keys.addToVaultBody"`.

### Design
1. In all 7 locale files (`en.json`, `da.json`, `de.json`, `es.json`, `fr.json`, `nb.json`, `sv.json`):
   - Move or replicate `"addToVaultBody"` under `"keys": { ..., "addToVaultBody": "..." }`.
   - Remove unused `"addToVaultBody"` from `"vault"`.

---

## 3. Unreachable Modal Backdrop & Keyboard Dismissal (#41)

### Context & Problem
In `apps/desktop/src/views/Connections.tsx`, the connection failure modal is displayed when `unreachable` is not null:
```tsx
{unreachable && (
  <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
    <div role="dialog" aria-label={t("connections.unreachable")} ref={unreachableRef} className="rounded border border-border bg-surface p-3">
```
Clicking the backdrop (`bg-black/50`) does not dismiss the modal, trapping the mouse from clicking any other navigation tabs.

### Design
1. In `apps/desktop/src/views/Connections.tsx`:
   - Add backdrop click dismissal on the outer container:
     ```tsx
     <div
       className="fixed inset-0 z-20 flex items-center justify-center bg-black/50"
       onClick={(e) => {
         if (e.target === e.currentTarget) setUnreachable(null);
       }}
     >
     ```
   - Verify that `unreachableRef = useDialogFocus(unreachable !== null, () => setUnreachable(null))` ensures `Escape` invokes `setUnreachable(null)` (Cancel only, never Connect Anyway).

---

## 4. Surface Failed Vault Unlock Attempts (#19)

### Context & Problem
Repeated failed unlock attempts trigger exponential backoff inside `sigrun-core`, but:
1. No log line is emitted by the host when unlock fails.
2. The user has no feedback on how many consecutive failed attempts have occurred.

### Design
1. **Core Host Logging & Error Payload:**
   - In `crates/sigrun-core/src/host.rs` on `VaultUnlock` failure:
     - Log a redacted notice to stderr:
       ```rust
       eprintln!("sigrun-host: failed vault unlock attempt {} (backoff {}ms)", attempts, millis);
       ```
       (Never log the passphrase or length).
     - Attach `{ "attempts": attempts, "backoff_ms": millis }` to `Response::Error` when `Error::VaultInvalidPassphrase` is converted or returned.
2. **Desktop UI Notification:**
   - In `apps/desktop/src/views/VaultGate.tsx`:
     - Inspect `(err as CommandError)?.payload?.attempts`.
     - If `attempts > 1`, display an attempt counter / warning beneath the error:
       ```tsx
       {attempts > 1 && <p role="status" className="text-xs text-muted">{t("vault.failedAttempts", { count: attempts })}</p>}
       ```
   - In all 7 locale files, add `"failedAttempts"` under `"vault"`.

---

## 5. Typed Settings Wire Protocol (#29)

### Context & Problem
`commands.rs` currently implements `get_settings_inner` by serializing `Settings` to a JSON string wrapped in `Response::Command(json_string)`. In `api.ts`, `getSettings` uses `JSON.parse(await call<string>("get_settings")) as Settings`. This bypasses typed IPC protocol patterns and abuses `Response::Command`.

### Design
1. **IPC Protocol:**
   - In `crates/sigrun-core/src/ipc/protocol.rs`:
     - Add `Request::SettingsGet`
     - Add `Request::SettingsSet { settings: Box<Settings> }`
     - Add `Response::Settings(Settings)`
2. **Core Host Handler:**
   - In `crates/sigrun-core/src/host.rs`:
     - Handle `Request::SettingsGet` -> `Ok(Response::Settings(state.store.load_settings()?))`
     - Handle `Request::SettingsSet { settings }` -> `state.store.save_settings(&settings)?; Ok(Response::Ok)`
3. **Desktop Layer:**
   - In `apps/desktop/src-tauri/src/commands.rs`:
     - Update `get_settings_inner` to call `call(state, Request::SettingsGet).await`
     - Update `save_settings_inner` to call `call(state, Request::SettingsSet { settings: Box::new(settings) }).await`
   - In `apps/desktop/src/api.ts`:
     - `getSettings` calls `call<Settings>("get_settings")` directly.
