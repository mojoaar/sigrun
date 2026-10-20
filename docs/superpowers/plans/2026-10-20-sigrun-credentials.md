# Credentials Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A credential is a named identity (user plus key, password, or both) stored once and referenced by any number of connections, so rotating a password is one edit.

**Architecture:** Metadata in `data.toml` beside keys, password in the vault under the credential id. A connection references a credential (`credential_id`) or carries its own user and secret, never both. One pure `effective()` decides the identity, and every path that authenticates goes through it.

**Tech Stack:** Rust 2021 (core, CLI, Tauri shell) + React 19/TypeScript (desktop). No new dependency.

**Spec:** `docs/superpowers/specs/2026-10-20-sigrun-credentials-design.md`

## Global Constraints

- `~/.ssh` stays read-only; the only write is appending to a remote `authorized_keys` during an explicit provision.
- Secrets never appear in argv, an environment *value*, a log line, an error message, or `data.toml`. A `Secret` has no `Clone` and a redacting `Debug`.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`.
- Core errors (returned over IPC) stay English, like every existing one; the i18n keys in the spec's Messages table are for the UI that renders them.
- Every change gets a `## [Unreleased]` Keep-a-Changelog entry; one commit per task, conventional message.
- Gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`, and from `apps/desktop` `npm test -- --run && npm run typecheck && npm run lint && npm run build`.

## Review Focus

1. **One source, enforced twice.** A connection with `credential_id` *and* its own user/key/password is refused by the core save path and counted invalid on import — not merely hidden by the form.
2. **No dangling fallback.** A missing credential fails connect with an error naming the id; it never silently uses the connection's empty fields or prompts for a password.
3. **Rotation is real.** Changing one credential's password changes the prompt/askpass behaviour of every connection that references it, with no per-connection edit.
4. **Delete is safe.** A referenced credential cannot be deleted, and the refusal names the count.
5. **Nothing leaks.** The password is in the vault, keyed by credential id; `data.toml`, logs, argv and errors carry only the id and `has_password`.
6. **Own-secret connections are untouched.** Every test that exists for inline user/key/password today still passes unchanged.

---

### Task 1: Model, persistence, and the list call

**Files:**
- Modify: `crates/sigrun-core/src/model.rs`
- Modify: `crates/sigrun-core/src/store.rs` (test only, if the round-trip lives there)
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`, `lib.rs`
- Modify: `apps/desktop/src/api.ts`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `pub struct Credential { id, label, user, key_id: Option<String>, has_password: bool }`, `DataFile.credentials: Vec<Credential>`, `Connection.credential_id: Option<String>`, `Request::CredentialsList`, `Response::Credentials(Vec<Credential>)`, `credentials_list(state) -> Result<Vec<Credential>>`, command `list_credentials`, `api.listCredentials()`.

- [ ] **Step 1: Write the failing round-trip test**

In `crates/sigrun-core/src/store.rs` tests (follow the existing data round-trip test): write a `DataFile` holding one credential and one connection with `credential_id: Some("c1")`, read it back, assert both fields survive. Also assert an old file **without** the new fields still parses: deserialize a `DataFile` TOML string with only `version`, `connections`, `keys` and expect `credentials == []`, `credential_id == None`.

- [ ] **Step 2: Run it and watch it fail** — `cargo test -p sigrun-core store`. Expected: compile error, the fields do not exist.

- [ ] **Step 3: Add the model**

In `model.rs`, above `DataFile`, with `#[serde(default)]` on every field:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Credential {
    pub id: String,
    pub label: String,
    pub user: String,
    pub key_id: Option<String>,
    pub has_password: bool,
}
```

Add `#[serde(default)] pub credentials: Vec<Credential>` to `DataFile` and to its `Default` impl. Add `#[serde(default)] pub credential_id: Option<String>` to `Connection`, and to every struct literal that constructs a `Connection` (grep `Connection {` — `from_ssh_host`, the CLI builder, tests). Do not add serialization of any secret: there is none here.

- [ ] **Step 4: Add the protocol and the host handler**

`protocol.rs`: `Request::CredentialsList` and `Response::Credentials(Vec<Credential>)` — follow the adjacent-tagged `Response` convention exactly (`Response::Credentials` is a newtype over a sequence, so it must be a tagged variant, not internally tagged).

`host.rs`: `Request::CredentialsList => credentials_list(state)` in the dispatch, and

```rust
async fn credentials_list(state: &HostState) -> Result<Response> {
    let data = state.store.load_data()?;
    Ok(Response::Credentials(data.credentials))
}
```

`has_password` is stored metadata here, not derived from the vault: the list must work while locked, and the password presence is known without it.

- [ ] **Step 5: Wire the command and the client**

`commands.rs`: `list_credentials(state, app) -> Result<Vec<Credential>>` following the shape of `list_connections` (same lock-avoidance and error conversion). Register it in `lib.rs`'s `generate_handler!`. `api.ts`: `export function listCredentials(): Promise<Credential[]>` calling `invoke("list_credentials")`, plus the `Credential` type.

- [ ] **Step 6: Gate and commit**

Both gates. Under `## [Unreleased]` → `### Added`: "Credentials: named identities that connections can share (model and listing; the UI follows)."

```bash
git commit -m "feat(core): add the credential model and listing"
```

---

### Task 2: The one-source rule

**Files:**
- Modify: `crates/sigrun-core/src/bundle.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-core/src/error.rs` (only if a new variant is needed)
- Modify: `crates/sigrun-core/src/model.rs` (tests)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `Credential`, `credential_id` from Task 1.
- Produces: `pub fn credential_valid(c: &Credential, all: &[Credential]) -> bool` (non-empty label and user, at least one of key/password, label unique among `all`), and the hybrid/policy checks inside `bundle::valid_connection` and the save path.

- [ ] **Step 1: Write the failing tests**

In `bundle.rs` tests, table-driven:

```rust
// rejected: a connection that references a credential and also carries its own identity
let hybrid = Connection { credential_id: Some("c1".into()), user: "me".into(), ..base() };
assert!(!valid_connection(&hybrid));
// rejected: same, only a key
// accepted: credential_id with empty user, no key, has_password false
// accepted: own state, unchanged (user empty allowed as today)
```

And for credentials: no user → invalid; no key and no password → invalid; two credentials with the same label → the second invalid; a valid one → valid.

- [ ] **Step 2: Run, expect failure.**

- [ ] **Step 3: Implement the predicate and the checks**

`credential_valid` exactly as in Interfaces. In `valid_connection`, after the existing checks:

```rust
&& match &conn.credential_id {
    Some(_) => conn.user.is_empty() && conn.key_id.is_none() && !conn.has_password,
    None => true,
}
```

Add the same check to the core save path (`connections_add`/`connections_update` in `host.rs`) so a hybrid is refused with `Error::InvalidInput` even if a caller skips the bundle validator. Do not clear a caller's fields silently.

- [ ] **Step 4: Refuse the host-policy contradiction at save**

A connection with `credential_id` pointing at a credential that has no `key_id`, with `disable_password_auth == true`, cannot authenticate. Refuse it in `connections_add`/`connections_update` with a plain-English `Error::InvalidInput`. This needs the data file, so it belongs in the host, not in `bundle::valid_connection` (which has no credential list).

- [ ] **Step 5: Gate and commit**

Under `### Fixed`: "A connection can no longer mix a credential with its own user or secret."

```bash
git commit -m "feat(core): enforce one identity source per connection"
```

---

### Task 3: The password and the resolution

**Files:**
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-core/src/terminal.rs` (only if the invocation takes a user it no longer reads)
- Modify: `crates/sigrun-core/src/agent/*` (only if a key id is read from the connection directly)
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `Request::CredentialsSetPassword { id, password: Option<String> }`, `credentials_set_password`, the `credentials.set_password` command and `api.setCredentialPassword`, and

```rust
pub struct Auth { pub user: String, pub key_id: Option<String>, pub password_key: Option<String> }
pub fn effective(conn: &Connection, creds: &[Credential]) -> Result<Auth>; // Error::InvalidInput when the id is missing
```

- [ ] **Step 1: Write the failing tests**

`effective` (pure, in `model.rs` or `host.rs` beside it):

- own connection with user/key/password → `Auth { user, key_id, password_key: Some(conn.id) }`.
- own connection with no password → `password_key: None`.
- credential with both → `password_key: Some(cred.id)`, user and key from the credential.
- credential missing from the list → `Err` whose message contains the id.
- credential with a password but no key → key `None`, password key set.

`credentials_set_password` (host test, unlocked vault): stores, `has_password` flips true in the next `credentials_list`, `None` clears it and flips false. Empty string is refused, like `connections_set_password`.

- [ ] **Step 2: Run, expect failure.**

- [ ] **Step 3: Store the password**

Mirror `connections_set_password` exactly: `vault.put(&id, Secret::Password { password })` on `Some`, remove on `None`, empty refused, vault-locked refused with the existing error, and `has_password` written into `data.toml` in the same update. Never log it.

- [ ] **Step 4: Route every authentication through `effective`**

Grep for the reads that decide identity: `conn.user`, `.key_id`, `password_get`, the askpass env handoff, and `connections_provision`. Replace each with `effective(conn, &data.credentials)?`, then read the password from the vault under `password_key`. The askpass helper keeps receiving `SIGRUN_CONNECTION_ID`; the host resolves the credential at prompt time, so the helper needs no change.

Do not change what an own-secret connection does: same user, same key, same password key (its own id). The whole existing test suite is the proof.

- [ ] **Step 5: Gate and commit**

Under `### Added`: "Credential passwords live in the vault and every connection, agent session, askpass prompt and provisioning run resolves the same identity."

```bash
git commit -m "feat(core): resolve credentials for connect, agent and askpass"
```

---

### Task 4: Delete refusal and the CLI

**Files:**
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-cli/src/cli.rs`, `main.rs` (or wherever the command enum lives)
- Modify: `apps/desktop/src-tauri/src/commands.rs`, `lib.rs`, `apps/desktop/src/api.ts`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `credentials_add/update/remove`, commands `add_credential`/`update_credential`/`remove_credential`, `api.addCredential/updateCredential/removeCredential`, and `sigrun credentials list|add|remove` with `--label --user --key --password-stdin`, plus `sigrun connections add --credential <id|label>`.

- [ ] **Step 1: Failing tests**

- Removing a credential referenced by two connections fails with a message containing `2`, and the credential is still there.
- Removing an unreferenced one succeeds and removes its vault entry.
- `credential_valid` refuses a duplicate label (already covered in Task 2) — the *save* path must too, so the CLI cannot create two `prod`s.
- CLI parse test: `connections add` with both `--credential` and `--user` is refused; `--credential prod` resolves a label, `--credential <id>` resolves an id.

- [ ] **Step 2: Implement add/update/remove**

`add` assigns an id when empty (`crypto`-free: reuse whatever id scheme connections use), validates with `credential_valid` against the stored list, and persists metadata only. `update` keeps the id, validates the same way, and does not touch the password. `remove` counts references across `data.connections` and refuses with `Error::InvalidInput(format!("credential {id} is used by {n} connections"))` — the UI renders the localized count from its own list; this message is the backstop.

- [ ] **Step 3: CLI**

`sigrun credentials list` (label, user, method, used-by), `add --label --user [--key]` reading the password from stdin when `--password-stdin` is passed, `remove <id|label>`. `connections add` gains `--credential <id|label>` and refuses it together with `--user`; resolving a label is a linear scan shared with the credential commands, not a second copy.

- [ ] **Step 4: Gate and commit**

Under `### Added`: "Manage credentials from the CLI, and point a connection at one."

```bash
git commit -m "feat(cli): manage credentials"
```

---

### Task 5: The Credentials tab and the form switch

**Files:**
- Create: `apps/desktop/src/views/Credentials.tsx`
- Create: `apps/desktop/src/views/Credentials.test.tsx`
- Modify: `apps/desktop/src/App.tsx`, `App.test.tsx`
- Modify: `apps/desktop/src/views/Connections.tsx`, `Connections.test.tsx`
- Modify: `apps/desktop/src/i18n/en.json`, `da.json`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `listCredentials`, `addCredential`, `updateCredential`, `removeCredential`, `setCredentialPassword`, `Connection.credential_id`.
- Produces: `<Credentials onVaultLocked={…} />`, a `"credentials"` tab between `"keys"` and `"settings"`, and `Connections`'s source switch.

- [ ] **Step 1: Failing UI tests**

`Credentials.test.tsx` (mock `../api` like `Connections.test.tsx`): lists a credential with its method and used-by; opens the add dialog, saves with a key only, and calls `addCredential` once; refuses an empty label/user with the same message pattern as the connection form; the delete dialog refuses while referenced (button disabled, `credentials.inUse_*` text shown) and succeeds at zero.

`Connections.test.tsx`: opening the form shows the source switch; choosing a credential hides the user/secret fields and saves `credential_id` with `user: ""`; switching back to "enter my own" **clears** `credential_id` (assert the saved payload has `credential_id: null`); a connection whose credential is missing shows `connections.missingCredential` and the select offers the id as a disabled option.

`App.test.tsx`: the nav shows Credentials after Keys (bilingual matcher, the file renders Danish).

- [ ] **Step 2: Build the tab**

`Credentials.tsx` follows `Keys.tsx`'s shape: list, add/edit modal, remove confirmation, using `useDialogFocus` (already present) and the existing field styles. Method is derived: key only, password only, key + password. Used-by counts `connections.filter(c => c.credential_id === id).length`; pass `connections` in as a prop rather than fetching twice.

- [ ] **Step 3: The source switch**

In `Form`, add `const [source, setSource] = useState<"own" | "credential">(initial?.credential_id ? "credential" : "own")`. Choosing a credential sets `draft.credential_id` and blanks `user`, `key_id` and `has_password`; choosing own sets `credential_id: null`. `validateConnection` grows one rule: in credential mode, the user/host/port checks apply to the credential's values, so the user field is not validated at all and `host`/`port` still are. A missing id is a disabled `<option>` carrying the id, so the select cannot silently repoint.

- [ ] **Step 4: i18n**

Add `nav.credentials`, the credentials view's labels, the method labels, and the four keys from the spec (both locales). The plural keys are `credentials.inUse_one` / `credentials.inUse_other`; if the completeness guard rejects the suffixes, fall back to one sentence without a number and put the count in the used-by column — say which you did in the report.

- [ ] **Step 5: Gate and commit**

From `apps/desktop`, the full frontend gate. Under `### Added`: "A Credentials tab, and a connection form that can use one instead of its own details."

```bash
git commit -m "feat(desktop): add the credentials view"
```

---

### Task 6: Export, import, and the missing case

**Files:**
- Modify: `crates/sigrun-core/src/bundle.rs`
- Modify: `crates/sigrun-core/src/host.rs` (README text lives with the export writer)
- Modify: `apps/desktop/src/views/Connections.tsx` (badge only)
- Modify: `crates/sigrun-core/src/bundle.rs` tests
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `credential_id`, `effective`.
- Produces: no new public API; the bundle carries `credential_id` and never a credential.

- [ ] **Step 1: Failing tests**

- An export of a connection with a credential includes `credential_id` and **no** key or password from that credential.
- `README.txt` contains the line "Credentials are not included".
- Importing such a bundle leaves the connection dangling: it is stored (id and label valid), `credential_id` intact, and connecting it returns the message naming the id.
- A credential never appears in `export.json` even when it exists locally.

- [ ] **Step 2: Make the writer explicit**

`build_bundle` copies `credential_id` with the connection (it already clones the struct — assert it in a test rather than adding code). Add the README line. Add nothing else: a credential must not be reachable from the serializer.

- [ ] **Step 3: The dangling path**

`effective` already errors on a missing id (Task 3); confirm the connect path surfaces that message rather than a generic failure, and that no prompt is attempted. Add the badge in `Connections.tsx`: when `credential_id` is set and absent from `listCredentials`, render `connections.missingCredential` with the id.

- [ ] **Step 4: Gate and commit**

Under `### Changed`: "Export carries the credential reference, never the credential; an imported connection shows a missing-credential badge until it is repaired."

```bash
git commit -m "feat(core): keep credentials out of the bundle"
```

---

## Self-Review

- **Spec coverage:** model, one-source rule, storage split, resolution, delete, export/import, UI, IPC/CLI, messages and security each map to a task. The non-goals are untouched: no overrides, no folders, no migration, no sudo.
- **Order:** each task compiles alone. Task 3 changes behaviour only after Tasks 1–2 have made the data legal, so the existing suite is a real regression check.
- **Risk:** Task 3 is the wide one — it touches connect, askpass, agent and provisioning. If the grep finds a read that cannot go through `effective` (for example a path that has no `DataFile` in scope), stop and report it rather than threading a new parameter through the call stack.
- **Judgement left to implementers:** the exact id scheme for a new credential (match connections), and where `effective` lives (beside `Connection` is the default).
