# Sigrun form validation — design

**Status:** approved in conversation 2026-10-19, pending plan.

## Why

Before this pass, the Add/Edit connection form — the only place a connection is
created — delegated its rules to the browser. Those rules were real —
constraint validation ran before the submit event, so `required`/`min`/`max`
did block a `preventDefault` handler — but they were the wrong shape for this
app: the tooltip was the platform's language and wording, jsdom does not
implement constraint validation so the test suite could not see them, and their
rules stopped at presence and range: nothing checked the host or user charset.

Three field rules were genuinely unprotected before this pass:

- **host** was accepted with a space, a slash or a shell metacharacter. The
  launcher refuses it at connect time (`terminal::ssh_invocation`), so a typo
  became a failed connection instead of a message under the field.
- **user** got the same predicate as host in the launcher — non-empty and only
  `[A-Za-z0-9._-]` — and the form did not apply it.
- **user** was `required` in the form although the launcher allowed empty (ssh
  then uses the local user). An imported connection with an empty user — the
  bundle format explicitly accepts one — therefore could not be saved from Edit:
  the browser blocked the submit and there was nothing to type except a user the
  connection did not have.

The form should own one rule set, in one place, with one message style.

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
| host | non-empty after trim **and** only `[A-Za-z0-9._-]` | `terminal::safe_host_bytes` (`terminal.rs:39`), applied by `ssh_invocation` |
| user | empty allowed; otherwise only `[A-Za-z0-9._-]` | `ssh_invocation` applies the same predicate to a non-empty user (`terminal.rs:54`) |
| port | parses as an integer in `1..=65535` | `ssh -p` takes exactly that |

A connection the form accepts must be one the launcher would accept; that is
the whole contract. The predicate is written twice — once in Rust for the
launcher, once in TypeScript for the form — because the form cannot call core
across the IPC boundary. Both are pinned by a table test, and the TypeScript
one names `terminal.rs` in a comment.

## Behaviour

- `submit()` validates first. On failure it does **not** call the backend,
  shows the message under each offending field, marks those fields
  `aria-invalid`, points at the message with `aria-describedby`, and moves
  focus to the first one.
- Messages are one line each, next to the field, in the existing
  `text-xs text-danger` style.
- The port input holds the raw string, so an empty value and a non-number both
  produce the same message under the field instead of being coerced silently.
- Values are trimmed before saving, matching what the validation checked.
- The browser's `required`, `min` and `max` attributes come off the four
  fields. One rule set, one message style, and jsdom sees the same behaviour
  the browser does.
- The port input is `type="text"` with `inputMode="numeric"`, because the draft
  value is a string until it is validated and then parsed.

## Interfaces

- `apps/desktop/src/views/Connections.tsx` gains
  `export type ConnectionDraft = Omit<Connection, "port"> & { port: string }`,
  `export type FieldErrors = Partial<Record<"label" | "host" | "user" | "port", string>>`
  (values are i18n keys, so the function stays pure) and
  `export function validateConnection(draft: ConnectionDraft): FieldErrors`
  — a pure function, so the table above is pinned by a unit test with no DOM.
- No Rust change: the launcher's predicate already exists and stays
  `pub(crate)`; the form cannot reach it through IPC, so exposing it would add
  a public function with no caller.
- New i18n keys in both locales: `connections.invalidHost`,
  `connections.invalidUser`, `connections.invalidPort`,
  `connections.invalidLabel`.

## What is left to the builder

- The exact wording of the four messages.
- Whether the invalid state also tints the field's border (my default: yes,
  `border-danger`, since colour is not the only signal — the message and
  `aria-invalid` carry it too).

## Review focus

1. A port of `0`, `99999`, or a non-numeric value must be refused with a
   message, not saved.
2. A host containing a space, a slash or a shell metacharacter must be refused
   before it can reach `ssh`.
3. The Edit form must not reject a connection that is already stored and
   valid — including an imported one whose user is empty.
4. Nothing the launcher accepts may become unsavable: the form's rule must not
   be stricter than `ssh_invocation`'s, and `shell_metacharacters_are_rejected_not_injected`
   in `terminal.rs` is the Rust half of the same contract.
