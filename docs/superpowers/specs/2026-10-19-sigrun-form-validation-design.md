# Sigrun form validation — design

**Status:** approved in conversation 2026-10-19, pending plan.

## Why

The Add/Edit connection form trusts the browser. Its only custom check is the
duplicate-label warning; everything else is an HTML `required`, `min` and
`max` attribute — and those do **not** block a React `onSubmit` that calls
`preventDefault`, which is exactly what this form does. So a port of `0`, a
port of `99999`, or a host with a space in it are all accepted, stored, and
only surface later as a failed connect. A typo should be answered where it was
typed.

## Goal

The form refuses to save a connection the launcher cannot use, and says which
field is wrong, under that field, before anything is written.

## Non-goals

- No async validation, no DNS or reachability check (Connect already probes,
  and a host that is right but currently down is not an invalid host).
- No change to the duplicate-label behaviour: it stays a non-blocking warning,
  because a duplicate label is only ambiguous for the CLI's label form.
- No validation of group, key or password: all three are legitimately empty.

## The rules, and where they come from

The launcher is the authority, so the form mirrors it rather than inventing
stricter rules:

| Field | Rule | Source |
| --- | --- | --- |
| label | non-empty after trim | `connections.label` is how the CLI resolves a connection |
| host | non-empty after trim **and** only `[A-Za-z0-9._-]` | the check `terminal::ssh_invocation` already applies before it builds the command line |
| port | parses as an integer in `1..=65535` | `ssh -p` takes exactly that |
| user | empty allowed | `ssh user@host` falls back to the local user |

A connection the form accepts must be one the launcher would accept; that is
the whole contract.

## Behaviour

- `submit()` validates first. On failure it does **not** call the backend,
  shows the message under each offending field, marks those fields
  `aria-invalid`, points at the message with `aria-describedby`, and moves
  focus to the first one.
- Messages are one line each, next to the field, in the existing
  `text-xs text-danger` style.
- The port input reads back as an empty string when it holds something that is
  not a number, so "empty" and "not a number" produce the same message.
- Values are trimmed before saving, matching what the validation checked.

## Interfaces

- `crates/sigrun-core/src/terminal.rs` gains
  `pub fn valid_host(host: &str) -> bool`, the existing predicate extracted from
  `ssh_invocation` so one rule serves both the launcher and (through the
  command that already reaches core) any future caller. `ssh_invocation` keeps
  its behaviour exactly.
- `apps/desktop/src/views/Connections.tsx` gains
  `export type FieldErrors = Partial<Record<"label" | "host" | "port", string>>`
  and `export function validateConnection(connection: Connection): FieldErrors`
  — a pure function, so the table above is pinned by a unit test with no DOM.
- New i18n keys in both locales: `connections.invalidHost`,
  `connections.invalidPort`, `connections.invalidLabel`.

## What is left to the builder

- The exact wording of the three messages.
- Whether the invalid state also tints the field's border (my default: yes,
  `border-danger`, since colour is not the only signal — the message and
  `aria-invalid` carry it too).
- Whether to keep the browser's `required` attributes as well (my default:
  keep them off, so the user sees one consistent message rather than the
  browser's tooltip in one case and ours in another).

## Review focus

1. A port of `0`, `99999`, or a non-numeric value must be refused with a
   message, not saved.
2. A host containing a space, a slash or a shell metacharacter must be refused
   before it can reach `ssh`.
3. The Edit form must not reject a connection that is already stored and
   valid — including a disk-discovered one whose user is empty.
4. Nothing the launcher accepts may become unsavable: the form's rule must not
   be stricter than `ssh_invocation`'s.
