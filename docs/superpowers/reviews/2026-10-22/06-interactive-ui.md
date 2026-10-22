# Pass 6 — Interactive UI review

Method: the real React app served by Vite in a plain Chromium (Playwright), with the
three Tauri modules aliased to stubs (`@tauri-apps/api/core` `invoke`,
`@tauri-apps/api/event` `listen`, `@tauri-apps/plugin-dialog` `open`/`save`) driving a
scripted fixture vault. This exercises the genuine component tree, state handling, i18n,
and error paths without touching the installed app or a real vault. Screenshots in
`./shots/`. A mechanical scan cross-checked every literal `t("…")` call site against
`en.json` (176 call sites, 205 keys).

## Findings

### 6.1 — Invalid `theme` in settings bricks the UI behind a false "Locked" gate (reproduced live)
- **What:** `App.refresh()` runs `applyTheme(settings.theme, …)` inside its `try`.
  `applyTheme` (`themes/index.ts:24-30`) does `PALETTES[name][mode]` with no guard. If
  `settings.toml` holds a `theme` that is not one of `nordlys|fjord|ravn|skumring`
  (hand-edited file, a removed theme, a downgrade), `PALETTES[name]` is `undefined` and
  the property read throws. The `catch` then runs `setVault("Locked")`, so the app renders
  the locked gate even though the vault is unlocked — with **no error shown anywhere**.
  Reproduced live: with `theme: "system"` the UI sat on "Vault locked" and never opened;
  switching the fixture to `theme: "nordlys"` unlocked immediately. Zero console errors.
- **Risk of fixing:** trivial — guard the palette lookup and fall back to `DEFAULT_THEME`.
- **Risk of leaving:** a single corrupt/legacy settings value renders the whole app
  unusable and looks exactly like "the vault won't unlock", sending the user down the
  wrong diagnosis. Very hard to self-serve out of.
- **Effort:** S. (This is report finding #12, now confirmed end-to-end and worth raising
  within High.)

### 6.2 — "Add to vault" dialog renders the raw i18n key `keys.addToVaultBody` (confirmed visually)
- **What:** `Keys.tsx:63` calls `t("keys.addToVaultBody")`; the key exists in **none** of
  the seven locales. The dialog body shows the literal string `keys.addToVaultBody` to the
  user (screenshot `16-addtovault-dialog.png`). Every other literal `t()` call site
  resolves — this is the only missing key in the app.
- **Risk of fixing:** none — add the sentence to all seven locales.
- **Risk of leaving:** an unpolished, confusing string on a destructive-adjacent action
  (it explains that the private key is copied into the vault). The plural-guard tests pass
  because the key is simply absent everywhere, so parity checks can't catch it.
- **Effort:** S. (Report finding #13, now visually confirmed.)

### 6.3 — Connection-error modal blocks all navigation until dismissed
- **What:** a failed connect opens a modal (`role="dialog"`, `aria-label="Host is not
  reachable"`, Cancel / "Connect anyway"). While it is open, the overlay
  (`div.fixed.inset-0`) intercepts pointer events for the entire window — the top-nav tabs
  (Keys/Credentials/Settings/Help) are unreachable by mouse until the user picks Cancel or
  Connect-anyway (screenshot `15-connect-error-modal.png`). The modal is correct and
  accessible; the only question is whether it should also close on `Escape` / outside-click
  like the other dialogs.
- **Risk of fixing:** low; must keep the explicit trust decision (don't let a stray Escape
  silently accept a host key — only Cancel, never Connect-anyway).
- **Risk of leaving:** minor — one extra click to get back to the rest of the app after a
  failed connect. Keyboard users can already tab to Cancel.
- **Effort:** S.

## Verified working (no finding)
- Locked gate → wrong-passphrase shows `role=alert` and stays locked; correct passphrase
  unlocks to the nav.
- All five tabs render; Connections groups nest (`Prod` / `Prod/EU` → EU), badges show
  (group, "Password saved", "Key work"), live dot on the active connection, discovered
  source noted. Search filters rows.
- Add/Edit connection dialog opens; port `0` and blank host are rejected inline.
- Connect failure surfaces as the accessible modal above (Cancel / Connect anyway).
- Keys, Credentials, Settings views render fully; Generate / Import forms present.
- Help view renders completely in English and Danish with no raw i18n keys; version line
  present. (Content gaps — missing shortcuts, missing `provision`/`keys`/`status` CLI rows,
  the Linux tray claim — are covered in pass 9, not re-listed here.)
- Dark mode (`prefers-color-scheme: dark`) and light both paint; no unreadable contrast seen.
- Re-lock via the header Lock button returns to the passphrase gate.
- No console errors and no page errors across the entire walk.

## Harness notes (for reproduction / re-run)
- Throwaway config `apps/desktop/vite.ui.config.ts` aliases the three Tauri modules to
  `…/T/opencode/uiharness/stubs/*.ts`; served on `:1421`. Playwright driver:
  `…/T/opencode/uiharness/run.mjs` (full walk) plus `probe8/9/10.mjs` (targeted checks).
  Both the config and the harness dir are scratch — delete after the review; nothing here
  is committed.
- Not covered by this pass: real native dialogs (file pickers are stubbed), the actual
  terminal launch, the tray menu, and Windows/Linux window chrome — those need the real
  Tauri shell, not a browser. Windows-only UI behavior remains unverified.
