# Credentials — design

Status: approved 2026-10-20, implemented in the plan
`docs/superpowers/plans/2026-10-20-sigrun-credentials.md`.

## Why

Passwords and keys are rotated on a weekly rhythm, and today the secret lives on
each connection. Rotating one account across a dozen hosts means opening twelve
forms, and forgetting one leaves a stale secret that only fails at connect time.
The vault already stores and unlocks secrets; what is missing is a name for *who
you are* that connections can share.

## Goal

A credential is a named identity — a username plus a key, a password, or both —
defined once and referenced by any number of connections. Rotating the password
edits one credential and every connection that uses it follows.

## Non-goals

- Per-connection overrides. A connection uses a credential or its own fields, never
  a mix of the two.
- Credential folders, tags, or grouping beyond a flat list.
- Shipping credentials between machines. Export stays secret-free.
- sudo or account passwords used after login; only SSH authentication is in scope.
- Migrating today's per-connection passwords into credentials.

## Model

```rust
pub struct Credential {
    pub id: String,
    pub label: String,
    /// The login name. Required: a credential without a user is not an identity.
    pub user: String,
    pub key_id: Option<String>,
    pub has_password: bool,
}
```

A credential may carry a key, a password, or both. Both is the fallback case: the
key is offered first, the password is what `ssh` falls back to. At least one of
the two must be present — a user with no secret is a typo, not a credential.

`Connection` gains `credential_id: Option<String>` and keeps every existing field.

### The one-source rule

A connection is in exactly one of two states:

| State | Fields |
| --- | --- |
| Credential | `credential_id` set; `user` empty; `key_id` `None`; `has_password` false |
| Own | `credential_id` `None`; `user`, `key_id`, `has_password` as today |

This is enforced in `bundle::valid_connection` and in the core's save path, not
only in the form, because the CLI and an imported manifest can both produce a
hybrid. A rejected hybrid is an invalid connection, counted like any other
invalid entry on import.

### Host policy stays on the connection

`disable_password_auth` remains per connection. It describes the host, not the
identity: the same credential can be used against a host that forbids passwords
and one that does not. When it is set and the credential has no key, the
connection cannot authenticate — refused at save with its own message.

## Storage

Credential metadata lives in `data.toml` beside keys; the password lives in the
vault, sealed with everything else, keyed by the credential's id. This mirrors
the key split exactly: metadata listable while locked, secret readable only while
unlocked.

No migration. Existing connections keep their inline secrets and keep working;
a user who wants a shared identity creates a credential and points connections at
it.

## Resolution

One function decides what a connection authenticates with:

```rust
fn effective(conn: &Connection, creds: &[Credential]) -> Result<Auth>
// Auth { user, key_id, password_key: Option<String> }
```

`password_key` is the vault key to read when a password is needed: the connection's
id in the own state, the credential's id in the credential state. Keeping the
lookup out of `effective` leaves it pure and testable.

- `credential_id` set and found → `Auth` from the credential.
- `credential_id` set and missing → an error, never a fallback to the connection's
  own fields. A dangling reference is a broken connection, and saying so is the
  whole point of the reference.
- `credential_id` unset → today's path, unchanged.

Agent, askpass, provisioning and connect all go through this one function, so the
dot, the trust prompt and the password prompt cannot disagree about which identity
a session used.

## Delete

Deleting a credential referenced by N connections is refused, and the message
names N. Clearing the reference silently would turn a working connection into a
different identity without anyone asking.

## Export and import

Connections export with `credential_id`. Credentials never export: a secret does
not leave the vault. An imported connection whose credential is absent arrives
dangling: the row shows a "credential missing" badge, and connect refuses with a
message naming the missing id until the user points it at a local credential or
switches it to its own fields.

`README.txt` gains one line saying credentials are not included.

## UI

A new **Credentials** tab **after Keys** in the nav: one row per credential with
label, user, method (key, password, key + password) and a used-by count.
Add, edit and delete in the same modal shape as the connection form; editing the
password updates every user at once.

The connection form gains a source switch at the top:

- **Use a credential** — a select of credentials; user and secret fields disappear.
- **Enter my own** — today's fields, unchanged.

Switching from one to the other clears the other side's state, so a half-filled
form cannot be saved as both. A missing credential renders as a disabled option
with its id, so an imported connection can be repaired rather than guessed at.

## IPC and CLI

```text
credentials_list                 -> Vec<Credential>   // its own call, not folded into status
credentials_add    { credential } -> Ok
credentials_update { credential } -> Ok
credentials_remove { id }         -> Ok | referenced_by_connections
credentials_set_password { id, password: Option<String> } -> Ok
```

`credentials_remove` refuses with the count in the error, like connections do.

CLI: `sigrun credentials list|add|remove`, `--label`, `--user`, `--key`, and
`--password` read from stdin or a prompt — never argv. `sigrun connections add`
gains `--credential <id|label>` and keeps `--user` for the inline case, refusing
both together.

Because a label can address a credential, **labels are unique**: a duplicate is
refused at save, unlike a connection's duplicate label, which only warns. Without
that rule `--credential prod` has two answers.

## Messages

| Key | English | Danish |
| --- | --- | --- |
| `credentials.inUse_one` | This credential is used by one connection. Point it elsewhere before deleting. | Dette login bruges af én forbindelse. Flyt den først. |
| `credentials.inUse_other` | This credential is used by {{count}} connections. Point them elsewhere before deleting. | Dette login bruges af {{count}} forbindelser. Flyt dem først. |
| `credentials.missing` | Credential missing | Login mangler |
| `connections.missingCredential` | This connection uses a credential that is not here: {{id}}. Choose another, or enter your own details. | Denne forbindelse bruger et login, der ikke findes her: {{id}}. Vælg et andet, eller indtast dine egne oplysninger. |

The plural suffixes are i18next's; the completeness guard compares key sets, so
both locales carry both. If the guard cannot express them, the count leaves the
sentence and the used-by column carries the number.

## Security

- Passwords never appear in argv, an environment value, a log line, an error
  message or `data.toml`.
- The credential password is sealed under the same Argon2id + XChaCha20-Poly1305
  vault as keys, with a fresh nonce per seal.
- Listing credentials requires no unlock; reading or using a password does.
- No new Tauri capability, no new dependency.

## What is left to the builder

- Nothing open: tab order (after Keys), the two refusal messages, one
  `credentials_list` call, and `--credential` by id or label are all settled
  above.

## What success looks like

Rotate a password once, and every connection using that credential connects with
it on the next attempt, with no per-host edit and no prompt.
