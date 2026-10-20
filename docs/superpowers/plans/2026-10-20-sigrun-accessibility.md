# Accessibility and Keyboard Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every dialog can be opened, operated and dismissed from the keyboard, lands the user back where they came from, the one message that appears on its own is announced, the active nav tab carries the right semantics, and the macOS menu follows the Language setting and carries New Connection, Settings…, Import… and Export….

**Architecture:** One small `useDialogFocus(open, onClose)` hook — focus the first control on mount, Escape closes only the topmost dialog, focus returns to the opener on unmount — applied at the six `role="dialog"` sites. Everything else is a one-line semantic fix plus a Rust label table that mirrors `tray::labels`. The menu actions ride one `"menu"` event from Rust to `App`, which switches tab and hands a one-shot signal to the view that already owns the work.

**Tech Stack:** React 19 + TypeScript + Tailwind, vitest + testing-library; Rust 2021 + Tauri 2 menu API. No new dependency.

**Spec:** `docs/superpowers/specs/2026-10-19-sigrun-accessibility-design.md`, corrected by Task 1.

## Rulings (settled 2026-10-20 — do not relitigate)

1. **There are six dialogs, not four:** in `Connections.tsx` the connection form, the provision panel, the trust prompt, the unreachable prompt and the remove confirmation, plus the key remove confirmation in `Keys.tsx`. Find them by `role="dialog"` — line numbers drift as tasks land. `VaultGate` is **not** a dialog: it is the page itself, has no dismiss path, and already `autoFocus`es. Escape there would strand the user, so it is out of scope.
2. **Two of the three announcements already exist.** The import/export result (`Settings.tsx:150`) and the provisioning result (`Connections.tsx:489`) already carry `role="status"`, as does VaultGate's locked notice (`VaultGate.tsx:39`). Only `connections.noMatches` (`Connections.tsx:521`) is missing it — one line, folded into Task 3 rather than earning its own task.
3. **Backdrop-close is not existing behaviour.** No overlay has an `onClick`; the spec's review item about preserving it is deleted, and no backdrop handler is added.
4. **One `:focus-visible` rule instead of a per-theme audit.** `applyTheme` rewrites `--color-accent` on `:root` for all four themes in both modes (`themes/index.ts:29-35`), so a token-based ring follows the theme by construction. A blanket restyle is still out; this is one rule.
5. **No label sweep is expected to change anything.** `ConnectionRow`, `KeyCard`, `Settings`, `Help` and `Keys` were checked: every control has visible text or an `aria-label`, and there are no icon-only buttons. Task 3 re-verifies with a grep and reports "nothing found" rather than inventing a wrapper.
6. **No focus trap.** Escape, initial focus and restored focus are the whole contract.

## Global Constraints

- Behaviour for mouse users does not change. No `<dialog>` rewrite, no headless library, no new dependency.
- Every user-facing string goes through `t(...)` with its key in **both** locales. The macOS menu labels are the exception, following the existing `tray::labels` precedent (`tray.rs:29`): Rust-side strings, matched on `Settings.language`, because the menu is built in Rust.
- Frontend tests use the built-in matchers and `within(...)`; `@testing-library/jest-dom` is deliberately absent. Focus assertions use `document.activeElement`.
- A behaviour only desktop Tauri can exercise (the menu rebuild) is pinned by the pure label table; the rebuild itself is a partner GUI check named in Task 4.
- Every change gets a `## [Unreleased]` Keep-a-Changelog entry. One commit per task, conventional message.
- Gate: from `apps/desktop`, `npm test -- --run && npm run typecheck && npm run lint && npm run build`; from the repo root, `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` whenever a Rust file changes.

## Review Focus

1. **Escape never confirms a destructive action.** In the two remove confirmations and the vault gate, Escape means Cancel or nothing — never Delete.
2. **Escape acts on one dialog only.** During provisioning a trust prompt renders above the provision panel; one Escape must close the trust prompt and leave the panel.
3. **Focus lands somewhere useful.** Opening a dialog focuses its first control (the label field for the connection form, Cancel for the three confirmations), and closing returns focus to the element that opened it.
4. **Typing does not steal focus.** The hook's effect must run on mount only; a re-render with a new inline `onClose` must not re-focus the first field.
5. **The poll does not chatter.** The live dot keeps its `role="img"` and is not in a live region; the search field announces nothing per keystroke; `noMatches` is announced once when it appears.
6. **Only the active tab has `aria-current`,** and its value is `"page"` — not `"false"` on every tab.
7. **A menu action does what the page does.** New Connection opens the same form the Add button opens, and Import…/Export… run the same picker and the same IPC call as Settings — no second copy of the transfer logic, no second set of result strings.
8. **Menu actions are inert while the vault is locked** and never open a picker over the gate.

---

### Task 1: Correct the design doc

**Files:**
- Modify: `docs/superpowers/specs/2026-10-19-sigrun-accessibility-design.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the corrected spec that Tasks 2–4 implement.

- [ ] **Step 1: Correct the `## Why` paragraph**

Replace the sentence starting "Every dialog is an inline…" with:

```markdown
The six dialogs are built the same way — an inline `fixed inset-0` overlay with
a `role="dialog"` child and an `aria-label` — and none of them moves focus
inside when it opens, closes on Escape, or returns focus to the control that
opened it. `VaultGate` already focuses its passphrase field and is the page
itself rather than a dialog, so it is not part of this pass. Of the messages
that appear without a page change, only "No connections match" lacks a live
role: the import/export result, the provisioning result and the vault-locked
notice already carry `role="status"`.
```

- [ ] **Step 2: Correct §1 Dialog focus**

Replace "Four dialogs exist…" with "Six dialogs exist, all built the same way: the connection form, the provision panel, the trust prompt, the unreachable prompt, the connection remove confirmation (all in `Connections.tsx`), and the key remove confirmation (`Keys.tsx`)." Keep the three bullets. Replace the last paragraph with:

```markdown
Because the shape is identical in all six places, this is one small shared hook
rather than six copies. It also keeps a stack: the trust prompt renders above
the provision panel, and one Escape must close only the top one. A full focus
trap is a non-goal for now: Escape plus initial and returned focus covers the
keyboard user's actual journey, and a trap is machinery the app does not yet
need. `VaultGate` is excluded: it is the page, it already focuses its field,
and it has no dismiss path.
```

- [ ] **Step 3: Correct §2 Announcements**

Replace the first three bullets with:

```markdown
- `connections.noMatches` gets `role="status"`. The import/export result
  (`Settings.tsx:150`), the provisioning result (`Connections.tsx:451`) and the
  vault-locked notice (`VaultGate.tsx:39`) already have it; nothing to change
  there.
```

Keep the dot and search bullets as they are.

- [ ] **Step 4: Correct §3 visible focus and review focus 5**

In §3, replace the visible-focus bullet with:

```markdown
- Visible focus: one `:focus-visible` rule using the theme's accent token, so
  it follows all four themes in both modes without a per-theme audit. This is
  an addition, not a restyle: no component's classes change.
```

In **Review focus**, replace item 5 with:

```markdown
5. Mouse behaviour is unchanged. No overlay gains a backdrop-close handler —
   none has one today, and adding one would be new behaviour, not preserved
   behaviour.
```

Add to **Constraints**:

```markdown
- The macOS menu labels are Rust strings, like the tray's, because the menu is
  built in Rust; they are matched on `Settings.language` rather than `t(...)`.
```

- [ ] **Step 5: Run the gate and commit**

Run the Rust trio from the repo root (docs only). Quote the result.

```bash
git add docs/superpowers/specs/2026-10-19-sigrun-accessibility-design.md
git commit -m "docs: correct the accessibility design"
```

---

### Task 2: One dialog-focus hook, six call sites

**Files:**
- Create: `apps/desktop/src/useDialogFocus.ts`
- Create: `apps/desktop/src/useDialogFocus.test.tsx`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/views/Connections.test.tsx`
- Create: `apps/desktop/src/views/Keys.test.tsx`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the corrected spec from Task 1.
- Produces: `export function useDialogFocus(open: boolean, onClose: () => void): RefObject<HTMLDivElement | null>` in `apps/desktop/src/useDialogFocus.ts`. The returned ref goes on the element that already carries `role="dialog"`. Three dialogs in `Connections` are rendered conditionally inside one component, so the hook takes `open` and runs its effect on the false→true edge; the two dialog components that mount only when open pass `true`.

- [ ] **Step 1: Write the failing hook tests**

Create `apps/desktop/src/useDialogFocus.test.tsx`. Use a harness component with its own open state so focus return can be observed:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { useDialogFocus } from "./useDialogFocus";

function Harness({ onClose, onDidClose, name }: { onClose?: () => void; onDidClose: () => void; name: string }) {
  const [open, setOpen] = useState(false);
  const close = () => { setOpen(false); onDidClose(); onClose?.(); };
  return (
    <>
      <button onClick={() => setOpen(true)}>{`open ${name}`}</button>
      {open && <Dialog onClose={close} name={name} />}
    </>
  );
}

function Dialog({ onClose, name }: { onClose: () => void; name: string }) {
  const ref = useDialogFocus(true, onClose);
  return (
    <div role="dialog" aria-label={name} ref={ref}>
      <input aria-label="first" />
      <button onClick={onClose}>cancel</button>
    </div>
  );
}
```

Also add a fifth harness that keeps the dialog mounted but toggles `open` (the shape the three conditional dialogs use):
`<div role="dialog" ref={useDialogFocus(open, onClose)}>` with `open` state controlled from outside. Escape must be ignored while `open` is false.

Tests:

1. **Focus moves in and back out.** Click `open dlg`; expect `document.activeElement` to be the `first` input; click `cancel`; expect `document.activeElement` to be the `open dlg` button.
2. **Escape closes.** Click `open dlg`, `fireEvent.keyDown(document, { key: "Escape" })`; expect the dialog to be gone and the opener focused.
3. **Only the topmost closes.** Render two dialogs at once (a harness that opens both, or two `Dialog`s inside one conditional block). Escape once → only the second's `onClose` ran. Unmount the second, Escape again → the first closes.
4. **Typing does not steal focus.** Render one dialog, focus a second text field inside it, re-render with a new inline `onClose` (e.g. `rerender` with a fresh closure), then `fireEvent.change` the field; expect `document.activeElement` to still be that field.

Run: `cd apps/desktop && npx vitest run src/useDialogFocus.test.tsx`
Expected: FAIL — the module does not exist.

- [ ] **Step 2: Implement the hook**

```ts
import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE = 'input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled])';

// Mount order, so Escape acts on the top dialog only: the trust prompt renders
// above the provision panel and one Escape must not close both.
const stack: object[] = [];

export function useDialogFocus(open: boolean, onClose: () => void): RefObject<HTMLDivElement | null> {
  const ref = useRef<HTMLDivElement>(null);
  // Kept in a ref so the effect below runs only when `open` flips: an inline
  // onClose would otherwise re-run it on every keystroke and steal focus.
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement as HTMLElement | null;
    const token = {};
    stack.push(token);
    (ref.current?.querySelector<HTMLElement>(FOCUSABLE) ?? ref.current)?.focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || stack[stack.length - 1] !== token) return;
      e.preventDefault();
      close.current();
    };
    document.addEventListener("keydown", onKey);

    return () => {
      document.removeEventListener("keydown", onKey);
      const at = stack.indexOf(token);
      if (at !== -1) stack.splice(at, 1);
      // A closed dialog's opener may have unmounted too; focus() is then a no-op.
      opener?.focus();
    };
  }, [open]);

  return ref;
}
```

If the ref target has no focusable child, focusing it needs `tabIndex={-1}` — every current dialog has at least one button, so do not add the attribute anywhere.

- [ ] **Step 3: Apply it at the six sites**

Each dialog keeps its existing markup and `role="dialog"`; only the ref and the close callback are new.

| Site | Where the ref goes | `open` | `onClose` |
| --- | --- | --- | --- |
| `Connections.tsx` — connection `Form` | the `role="dialog"` div | `true` (the component mounts only while open) | `close` (the local one that clears the password state) |
| `Connections.tsx` — `ProvisionPanel` | the `role="dialog"` div | `true` (same) | `close` (the local one) |
| `Connections.tsx` — trust | the `role="dialog"` div | `trust !== null` | `() => setTrust(null)` |
| `Connections.tsx` — unreachable | the `role="dialog"` div | `unreachable !== null` | `() => setUnreachable(null)` |
| `Connections.tsx` — connection remove | the `role="dialog"` div | `confirmRemove !== null` | `() => setConfirmRemove(null)` |
| `Keys.tsx:40` — key remove | the `role="dialog"` div | `confirmRemove !== null` | `() => setConfirmRemove(null)` |

The three conditional dialogs live inside components that render unconditionally, so their hooks are called at the top level of `Connections` (and of `Keys`) with the state check as `open` — never inside an `if`. The connection `Form` and `ProvisionPanel` are separate components that only exist while open, so their hooks sit at their own top level and pass `true`.

The stack follows effect order: a conditionally rendered child (`ProvisionPanel`) registers before its parent's hook, and the trust prompt's hook lives in the parent, so trust is on top — which is exactly how the overlays paint (`z-20` over the panel). Do not reorder the hooks.

- [ ] **Step 4: Wire the behaviour tests**

In `Connections.test.tsx`:

1. **Add dialog.** Click "Add connection"; expect `document.activeElement` to be the Label input. Press Escape; expect the dialog gone and `api.addConnection` not called, and focus back on the "Add connection" button.
2. **Remove confirmation.** Click the row's "Delete"; expect focus on the dialog's Cancel button. Press Escape; expect the dialog gone and `api.removeConnection` not called.
3. **Trust above provision.** Open a row's provision panel ("Install SSH key" with an available vault key), make `api.provisionConnection` reject with `{ code: "host_key_unknown", message: "SHA256:abc ssh-ed25519 AAAA" }`, and submit. With the trust prompt open, press Escape once: the trust prompt is gone and the provision panel is still there.

In the new `Keys.test.tsx`, mocking `../api` the way `Connections.test.tsx` does (`listKeys` returning one vault key, `removeKey: vi.fn()`): click the key's Delete, expect focus on Cancel, press Escape, expect the dialog gone, `removeKey` not called, and focus back on the Delete button.

Run: `cd apps/desktop && npx vitest run src/useDialogFocus.test.tsx src/views/Connections.test.tsx src/views/Keys.test.tsx`. Expected: PASS.

- [ ] **Step 5: Run the gate**

From `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. Quote the output.

- [ ] **Step 6: Add the CHANGELOG entry and commit**

Under `## [Unreleased]` → `### Fixed`:

```markdown
- Every dialog now takes focus when it opens, closes on Escape without ever confirming a destructive action, and returns focus to the control that opened it.
```

```bash
git add apps/desktop/src/useDialogFocus.ts apps/desktop/src/useDialogFocus.test.tsx apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/views/Keys.test.tsx CHANGELOG.md
git commit -m "feat(desktop): give dialogs keyboard focus and Escape"
```

---

### Task 3: Semantics that are one line each

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/index.css`
- Modify: `apps/desktop/src/App.test.tsx`
- Modify: `apps/desktop/src/views/Connections.test.tsx`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing from Task 2 (independent one-liners).
- Produces: no new exported symbols.

- [ ] **Step 1: Write the failing tests**

In `App.test.tsx`, following the existing Unlocked setup (`vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", … })`):

```tsx
test("marks only the active tab with aria-current", async () => {
  render(<App />);
  const nav = await screen.findByRole("navigation");
  // App applies the persisted language, which this suite's settings mock sets
  // to Danish, so match either label.
  const connectionsTab = () => within(nav).getByRole("button", { name: /connections|forbindelser/i });
  const keysTab = () => within(nav).getByRole("button", { name: /keys|nøgler/i });
  expect(connectionsTab().getAttribute("aria-current")).toBe("page");
  expect(keysTab().getAttribute("aria-current")).toBeNull();
  fireEvent.click(keysTab());
  expect(keysTab().getAttribute("aria-current")).toBe("page");
  expect(connectionsTab().getAttribute("aria-current")).toBeNull();
});
```

In `Connections.test.tsx`, extend the existing no-matches test (the one that types `zzz`): assert the element is found by role: `expect(screen.getByRole("status").textContent).toBe("No connections match.")` — or `within(screen.getByRole("status"))`, matching the file's style. If another `role="status"` is on screen in that test (the provisioning result), scope it to the no-matches text instead.

- [ ] **Step 2: Run them to verify they fail**

Run: `cd apps/desktop && npx vitest run src/App.test.tsx src/views/Connections.test.tsx`
Expected: FAIL — `aria-current` is `"false"` on every tab, and no element has the status role for no-matches.

- [ ] **Step 3: Make the three changes**

- `App.tsx`: `aria-current={tab === n ? "page" : undefined}`.
- `Connections.tsx`, the `connections.noMatches` line (grep for it): `<p role="status" className="text-sm text-muted">`. Leave the `connections.empty` line alone.
- `apps/desktop/src/index.css`, after the two imports:

```css
@layer base {
  /* Theme tokens are rewritten on :root by applyTheme, so this follows every
     theme and mode without per-theme rules. */
  :focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
}
```

- [ ] **Step 4: The label sweep (expect no change)**

Run: `rg -n '<button|<select|<input' apps/desktop/src --glob '!*.test.tsx'` and check every hit for visible text or an `aria-label`/wrapped `<label>`. Record in the report either the file:line of anything unnamed that was fixed, or "nothing unnamed found". Do not add wrappers speculatively.

- [ ] **Step 5: Run the tests, add the CHANGELOG line**

Run: `cd apps/desktop && npx vitest run src/App.test.tsx src/views/Connections.test.tsx`, then the full gate (`npm test -- --run && npm run typecheck && npm run lint && npm run build`). Quote it.

Under `## [Unreleased]` → `### Fixed`:

```markdown
- The active tab is marked with `aria-current="page"`, the no-matches message is announced to screen readers, and focused controls show a theme-aware focus ring.
```

Commit:

```bash
git add apps/desktop/src/App.tsx apps/desktop/src/App.test.tsx apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/index.css CHANGELOG.md
git commit -m "feat(desktop): announce the empty search and mark the active tab"
```

---

### Task 4: The macOS menu follows Language

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing from Tasks 2–3.
- Produces: `pub struct MenuLabels { pub edit: String, pub view: String, pub window: String, pub quit: String }` and `pub fn menu_labels(language: &str) -> MenuLabels` in `lib.rs`, both compiled on **every** platform so the test runs on all three CI legs (only `setup_macos_menu` stays `#[cfg(target_os = "macos")]`).
- `pub(crate) fn setup_macos_menu(handle: &tauri::AppHandle, language: &str) -> tauri::Result<()>` — signature changes from `&tauri::App` to `&tauri::AppHandle` plus a language.

- [ ] **Step 1: Write the failing test**

At the end of `lib.rs`:

```rust
#[cfg(test)]
mod menu_tests {
    use super::menu_labels;

    #[test]
    fn menu_labels_are_localized() {
        let en = menu_labels("en");
        assert_eq!((en.edit.as_str(), en.view.as_str(), en.window.as_str(), en.quit.as_str()), ("Edit", "View", "Window", "Quit Sigrun"));
        let da = menu_labels("da");
        assert_eq!((da.edit.as_str(), da.view.as_str(), da.window.as_str(), da.quit.as_str()), ("Rediger", "Vis", "Vindue", "Afslut Sigrun"));
        // An unknown language falls back to English, like the tray.
        assert_eq!(menu_labels("kl").edit, "Edit");
    }
}
```

Run: `cargo test -p sigrun-desktop menu_labels_are_localized` (use the crate name from `apps/desktop/src-tauri/Cargo.toml`). Expected: FAIL — `menu_labels` does not exist.

- [ ] **Step 2: Add the table**

Beside `about_metadata`, outside any `cfg`:

```rust
/// The macOS menu strings Sigrun writes itself. Predefined items (Services,
/// Hide, Cut/Copy/Paste, Fullscreen) keep the strings macOS supplies in the
/// system language; the product name is not translated. Matches `tray::labels`
/// in shape and fallback.
pub struct MenuLabels {
    pub edit: String,
    pub view: String,
    pub window: String,
    pub quit: String,
}

pub fn menu_labels(language: &str) -> MenuLabels {
    match language {
        "da" => MenuLabels {
            edit: "Rediger".into(),
            view: "Vis".into(),
            window: "Vindue".into(),
            quit: "Afslut Sigrun".into(),
        },
        _ => MenuLabels {
            edit: "Edit".into(),
            view: "View".into(),
            window: "Window".into(),
            quit: "Quit Sigrun".into(),
        },
    }
}
```

- [ ] **Step 3: Take the labels in `setup_macos_menu`**

Change the signature to `pub(crate) fn setup_macos_menu(handle: &tauri::AppHandle, language: &str) -> tauri::Result<()>`, drop the local `let handle = app.handle();`, and build the submenus from `menu_labels(language)`: the `with_id(..., "Window")` title, `SubmenuBuilder::new(handle, "Edit")`, `..., "View"`, and the quit title. If `SubmenuBuilder::new` needs a `&str`, pass `&labels.edit` — if it needs an owned `String`, `.as_str()`; follow what the compiler accepts, do not clone unnecessarily.

At the setup call site (`lib.rs:137`), pass the language from the managed state:

```rust
#[cfg(target_os = "macos")]
{
    let language = app.state::<std::sync::Arc<sigrun_core::host::HostState>>()
        .store
        .load_settings()
        .map(|s| s.language)
        .unwrap_or_else(|_| "en".to_string());
    setup_macos_menu(app.handle(), &language)?;
}
```

(`app.manage(state)` runs above it, so the state is available; `tray.rs` reads the same field the same way.)

- [ ] **Step 4: Rebuild the menu when the setting is saved**

In `commands.rs::save_settings`, capture the language **before** the settings value is moved into `save_settings_inner` (it takes `settings` by value, so `settings.language` is not available afterwards):

```rust
let language = settings.language.clone();
let out = save_settings_inner(st.clone(), settings).await;
// The tray labels follow Settings.language.
crate::tray::sync_tray(&app, &st);
// The macOS menu labels follow it too. A failed save must not repaint the
// menu with a language that was not stored.
#[cfg(target_os = "macos")]
if matches!(out, Ok(Response::Ok)) {
    if let Err(e) = crate::setup_macos_menu(&app, &language) {
        eprintln!("sigrun: could not rebuild the menu: {e}");
    }
}
out
```

If `set_menu` from a command thread turns out to be refused on macOS, wrap the call in `app.run_on_main_thread(move || { ... })` — the tray already calls `set_menu` from a spawned task, so try the direct call first and record what happened in the report.

- [ ] **Step 5: Run the gate and commit**

From the repo root: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. Also run the frontend gate from `apps/desktop` because the CHANGELOG and no frontend file changed — state that the frontend is untouched instead if that is the case.

Under `## [Unreleased]` → `### Fixed`:

```markdown
- The macOS app menu (Edit, View, Window, Quit) now follows the Language setting instead of staying English.
```

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/commands.rs CHANGELOG.md
git commit -m "feat(desktop): localize the macOS menu"
```

**Partner check (not a subagent step):** with the dev app running, switch Settings → Language to Danish and confirm the menu bar reads Rediger / Vis / Vindue / Afslut Sigrun without a restart, then switch back. Record the result in the ledger.

---

### Task 5: File and Settings menu actions

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/App.tsx`, `apps/desktop/src/App.test.tsx`
- Modify: `apps/desktop/src/views/Connections.tsx`, `apps/desktop/src/views/Connections.test.tsx`
- Modify: `apps/desktop/src/views/Settings.tsx`, `apps/desktop/src/views/Settings.test.tsx`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `menu_labels` and `setup_macos_menu(handle, language)` from Task 4.
- Produces:
  - `MenuLabels` gains `file`, `new_connection`, `import`, `export`, `settings`.
  - The app emits one event, `"menu"`, whose payload is the menu id: `"new-connection" | "settings" | "import" | "export"`.
  - `Connections` gains optional `openNew?: boolean` and `onNewOpened?: () => void`.
  - `Settings` gains optional `transferRequest?: "import" | "export" | null` and `onTransferDone?: () => void`.

- [ ] **Step 1: Extend the failing Rust test**

In `menu_tests`:

```rust
#[test]
fn menu_labels_name_the_file_menu() {
    let en = menu_labels("en");
    assert_eq!(
        (en.file.as_str(), en.new_connection.as_str(), en.import.as_str(), en.export.as_str(), en.settings.as_str()),
        ("File", "New Connection", "Import…", "Export…", "Settings…")
    );
    let da = menu_labels("da");
    assert_eq!(
        (da.file.as_str(), da.new_connection.as_str(), da.import.as_str(), da.export.as_str(), da.settings.as_str()),
        ("Filer", "Ny forbindelse", "Importér…", "Eksportér…", "Indstillinger…")
    );
}
```

Run: `cargo test -p <desktop-crate> menu_labels`. Expected: FAIL — the fields do not exist.

- [ ] **Step 2: Add the items**

In `MenuLabels` and both arms of `menu_labels`, add the five fields with the strings above.

In `setup_macos_menu`, after the edit menu:

```rust
let new_connection = MenuItemBuilder::with_id("new-connection", &labels.new_connection)
    .accelerator("CmdOrCtrl+N")
    .build(handle)?;
let import = MenuItemBuilder::with_id("import", &labels.import).build(handle)?;
let export = MenuItemBuilder::with_id("export", &labels.export).build(handle)?;
let file_menu = SubmenuBuilder::new(handle, &labels.file)
    .item(&new_connection)
    .separator()
    .item(&import)
    .item(&export)
    .build()?;
let settings = MenuItemBuilder::with_id("settings", &labels.settings)
    .accelerator("CmdOrCtrl+,")
    .build(handle)?;
```

In `app_menu`, put `.item(&settings).separator()` between `.about(...)` and `.separator().services()`, so the order is About, Settings…, Services, Hide…, Quit — the macOS convention.

Build the menu as `&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu]`.

- [ ] **Step 3: Emit one event from the app menu**

On the `tauri::Builder` in `run()`, before `.setup(...)`:

```rust
// The app menu is Rust-side; the webview owns the views, so the action
// travels as one event and App decides what it means.
.on_menu_event(|app, event| {
    let id = event.id().as_ref();
    if matches!(id, "new-connection" | "settings" | "import" | "export") {
        let _ = app.emit("menu", id);
    }
})
```

`tauri::Emitter` is already imported for the existing `emit` calls; add it if the compiler asks.

- [ ] **Step 4: Route it in `App`**

Add `useRef` to the React import, and:

```ts
const [newConnection, setNewConnection] = useState(false);
const [transfer, setTransfer] = useState<"import" | "export" | null>(null);
// The listener is registered once; a ref lets it see the current gate state.
const vaultRef = useRef(vault);
vaultRef.current = vault;
```

Inside the existing `try` in the listener effect, beside the two `listen` calls and following the same `gone`/`unX` pattern:

```ts
let unMenu: (() => void) | undefined;
listen<string>("menu", (e) => {
  if (vaultRef.current !== "Unlocked") return;
  switch (e.payload) {
    case "new-connection": setTab("connections"); setNewConnection(true); break;
    case "settings": setTab("settings"); break;
    case "import": setTab("settings"); setTransfer("import"); break;
    case "export": setTab("settings"); setTransfer("export"); break;
  }
}).then((f) => { if (gone) f(); else unMenu = f; })
  .catch((err) => console.warn("sigrun: could not listen for menu actions", err));
```

and `unMenu?.()` in the cleanup. Pass the two props:

```tsx
<Connections … openNew={newConnection} onNewOpened={() => setNewConnection(false)} />
…
<Settings transferRequest={transfer} onTransferDone={() => setTransfer(null)} />
```

- [ ] **Step 5: Consume the signal in `Connections` and `Settings`**

`Connections` — add the two optional props to its signature and, after the `form` state:

```ts
useEffect(() => {
  if (!openNew) return;
  setForm({ edit: null });
  onNewOpened?.();
}, [openNew]);
```

`Settings` — add the two optional props, then **after** `exportTo` and `importFrom` are defined:

```ts
// The menu hands over a one-shot request; the ref keeps the latest handlers
// out of the effect's dependencies so it runs on the request edge only.
const runTransfer = useRef<(what: "import" | "export") => void>(() => {});
const transferDone = useRef(onTransferDone);
transferDone.current = onTransferDone;
runTransfer.current = (what) => (what === "export" ? exportTo(true) : importFrom(true));
useEffect(() => {
  if (!transferRequest) return;
  runTransfer.current(transferRequest);
  transferDone.current?.();
}, [transferRequest]);
```

`exportTo(true)` / `importFrom(true)` are the zip variants — the menu has one Import… and one Export…, and the folder variants stay on the page.

- [ ] **Step 6: Tests**

`Connections.test.tsx`:

```tsx
it("opens the add form when the menu asks for a new connection", async () => {
  const opened = vi.fn();
  render(<Connections openNew onNewOpened={opened} onVaultLocked={() => {}} onGoToKeys={() => {}} />);
  expect(await screen.findByRole("dialog", { name: "Add connection" })).toBeTruthy();
  expect(opened).toHaveBeenCalled();
});
```

`Settings.test.tsx` — pass the prop and assert the existing path is reused, not reimplemented:

```tsx
it("runs the export from a menu request", async () => {
  saveMock.mockResolvedValueOnce("/tmp/out.zip");
  vi.mocked(api.exportBundle).mockResolvedValueOnce({ connections: 2, keys: 1 });
  const done = vi.fn();
  render(<Settings transferRequest="export" onTransferDone={done} />);
  await waitFor(() => expect(saveMock).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "sigrun-export.zip" })));
  expect(api.exportBundle).toHaveBeenCalledWith("/tmp/out.zip", true);
  expect(done).toHaveBeenCalled();
});
```

and the mirror for `"import"` (`openMock` resolves a path, `api.importBundle` resolves `{ added: 1, skipped: 0, invalid: 0, unresolved_keys: 0 }`).

`App.test.tsx` — two tests, following the `vault-locked` handler pattern, so no test renders `App` twice:

```tsx
const menuHandler = () => vi.mocked(listen).mock.calls.filter(([e]) => e === "menu").at(-1)![1] as (e: { payload: string }) => void;

test("a menu action does nothing while the vault is locked", async () => {
  render(<App />);
  await screen.findByRole("button", { name: /lås op|unlock/i });
  act(() => menuHandler()({ payload: "new-connection" }));
  act(() => menuHandler()({ payload: "export" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.queryByRole("heading", { name: /settings|indstillinger/i })).toBeNull();
});

test("the menu opens the add form when unlocked", async () => {
  vi.mocked(getStatus).mockResolvedValueOnce({ vault: "Unlocked", agent_socket: null, host_addr: "x", version: "0.1.0" });
  render(<App />);
  await screen.findByRole("navigation");
  act(() => menuHandler()({ payload: "new-connection" }));
  expect(await screen.findByRole("dialog", { name: /add connection|tilføj forbindelse/i })).toBeTruthy();
});
```

- [ ] **Step 7: Run the gates**

From `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. From the root: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. Quote both.

- [ ] **Step 8: CHANGELOG and commit**

Under `## [Unreleased]` → `### Added`:

```markdown
- The macOS menu gained a File menu (New Connection ⌘N, Import…, Export…) and a Settings… item (⌘,); both follow the Language setting.
```

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src/App.tsx apps/desktop/src/App.test.tsx apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/views/Settings.tsx apps/desktop/src/views/Settings.test.tsx CHANGELOG.md
git commit -m "feat(desktop): add file and settings menu actions"
```

**Partner check (not a subagent step):** with the dev app running and unlocked, use each of the four items — New Connection opens the form, Settings… switches tab, Export… opens the save picker and reports a result, Import… opens the open picker — then lock the vault and confirm all four do nothing. Confirm ⌘N and ⌘, work with focus in a text field. Record the result in the ledger.

Items stay enabled while locked; making them visibly disabled means syncing vault state into the Rust menu on every lock and unlock, which is machinery this pass does not need.

---

## Self-Review

- **Spec coverage:** dialog focus, Escape, focus return and the stack (Task 2); the one missing live region (Task 3); `aria-current` and the focus ring (Task 3); the macOS menu (Task 4); and the File/Settings actions (Task 5, added at your request on 2026-10-20 — new behaviour, not part of the accessibility spec, and it gets no spec edit). The spec's VaultGate and backdrop-close items are deleted with reasons in Task 1, not silently dropped.
- **Step scan:** every behaviour has a failing test first, except the focus ring (CSS cannot be asserted in jsdom — the partner GUI pass covers it) and the menu rebuild (only a running macOS app can; the label table is tested on all three OSes).
- **Type consistency:** one hook signature, returned ref typed `RefObject<HTMLDivElement | null>` and placed on the existing dialog div; `MenuLabels` is plain owned strings so both call sites can borrow.
- **Review Focus mapping:** 1 → Task 2 Step 4.2 and the Keys test; 2 → hook test 3 and Task 2 Step 4.3; 3 → hook test 1 and both component tests; 4 → hook test 4; 5 → Task 3 leaves the dot and the search field alone and only adds `noMatches`; 6 → Task 3 Step 1.
- **Proportion:** one 20-line hook, six refs, three one-liners, one Rust label table, and four menu items driven by one event. No component restructure, no dependency, no trap, no second copy of the transfer logic.
- **Risk flagged:** Task 2's hook goes at the `Connections` top level for three conditional dialogs; if the implementer places a hook inside an `if`, React's rules-of-hooks lint will catch it, but the reviewer should confirm all six are unconditional. Closing two dialogs at once (vault lock while a trust prompt is open) can leave focus on `body`, because the inner restore targets an unmounted opener; that is recorded rather than engineered around. Task 5 adds an app-level `on_menu_event` while the tray already has one — the ids do not overlap, but only a running app proves the app menu reaches the new handler, which is why it has a partner check rather than a unit test.
