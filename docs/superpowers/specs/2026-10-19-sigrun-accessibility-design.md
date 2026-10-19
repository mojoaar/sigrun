# Sigrun accessibility and keyboard pass — design

**Status:** approved in conversation 2026-10-19, pending plan.

## Why

The app is keyboard-hostile in places and silent in others. Every dialog is an
inline `fixed inset-0` div with `role="dialog"` and an `aria-label`: **none of
them** moves focus inside when it opens (only `VaultGate` sets `autoFocus`),
none closes on Escape, and none returns focus to the control that opened it.
Messages that appear without a page change — "No connections match", the
import/export result, the vault-locked notice — are plain text, so a screen
reader never hears them. The nav marks the active tab with
`aria-current={active}`, which React renders as the string `"false"` rather
than omitting the attribute. And the macOS menu labels are English literals
while the rest of the app follows the Language setting.

## Goal

Everything in the app can be reached and understood without a mouse, and
everything that changes on its own is announced once.

## The four slices

This is one design with four plans, in this order:

1. **Dialog focus** — the shared behaviour every dialog lacks.
2. **Announcements** — the live regions for messages that appear on their own.
3. **Nav and labels** — the semantics, and any control that has no name.
4. **Native menu labels** — the macOS menu follows the Language setting.

### 1. Dialog focus

Four dialogs exist, all built the same way: the connection form, the trust
prompt, the remove confirmation, and the vault gate (which already focuses its
input). Each gains:

- focus moving to the first meaningful control on open (the dialog's input, or
  its first button when it has none);
- Escape closing it, equivalent to pressing Cancel — never equivalent to
  confirming a destructive action;
- focus returning to the control that opened it when it closes.

Because the shape is identical in all four places, this is one small shared
hook rather than four copies. A full focus trap is a non-goal for now: Escape
plus initial and returned focus covers the keyboard user's actual journey, and
a trap is machinery the app does not yet need.

### 2. Announcements

- `connections.noMatches` gets `role="status"`.
- The Import/export result line gets `role="status"`.
- The vault-locked notice in the header gets `role="status"`.
- The pulsing dot keeps its existing `role="img"` with an `aria-label`, and is
  **not** put in a live region: a list whose rows change on a two-second poll
  would announce continuously. The dot is information when you navigate to it,
  not an interruption.
- The connection search field must not announce on every keystroke; the tree it
  filters is not a live region.

### 3. Nav and labels

- `aria-current={active ? "page" : undefined}` on the nav buttons, so the
  attribute is present only on the active one.
- A sweep of every view for a control with no accessible name — an icon-only
  button, a select with no label — and a name added where one is missing. The
  form's fields are already labelled; the dialog `aria-label`s already exist.
- Visible focus: the app relies on the browser's default focus ring today. The
  plan verifies it is actually visible against `bg`, `surface` and every theme,
  and adds an explicit style only where it is not — no blanket restyle.

### 4. Native menu labels

The macOS app menu is built once in `setup`, so its labels are frozen at
startup: "Sigrun", "Edit", "View", "Window", "Quit Sigrun". Making them follow
the Language setting means rebuilding the menu when the language changes. The
predefined items (Services, Hide, Cut/Copy/Paste) keep the strings macOS
supplies in the system language — that is the platform's behaviour and matching
it is a non-goal.

## Constraints

- No new dependency. No `<dialog>` rewrite, no headless UI library: the
  existing markup and Tailwind utilities are enough.
- Tests use the built-in matchers and `within(...)`; `@testing-library/jest-dom`
  is deliberately absent, so focus assertions go through
  `document.activeElement`.
- Behaviour for mouse users does not change.

## What is left to the builder

- The exact shape of the shared hook (its name and whether it returns props or
  takes a ref).
- Whether the announce-able messages use `role="status"` or `aria-live="polite"`
  — they are equivalent for these cases; pick one and use it everywhere.
- How much of the focus-ring verification becomes a test versus a manual check.
- Whether slice 4 rebuilds the whole menu or swaps only the labels.

## Review focus

1. Every interactive control is Tab-reachable and shows a visible focus
   indicator against all four themes, in light and dark.
2. A keyboard user can open, operate and dismiss every dialog without a mouse,
   and lands back on the control they came from.
3. Escape never confirms a destructive action (the remove confirmation and the
   vault gate both matter here).
4. A message that appears on its own is announced exactly once — and the
   two-second connection poll does not turn into a stream of announcements.
5. Mouse behaviour is unchanged, including that the dialogs still close on
   backdrop click where they do today.
