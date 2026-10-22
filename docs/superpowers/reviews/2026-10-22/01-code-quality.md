# Pass 1 — Code quality

### TOFU host-key trust logic lives in the Tauri command layer, not in core
- **Area:** tauri
- **Location:** apps/desktop/src-tauri/src/commands.rs:294-392 (`probe_host_key`, `parse_keyscan`, `key_error`, `evaluate_trust`, `check_trust`, `connect_guarded`)
- **What:** The whole pre-flight TOFU gate — spawning `ssh-keyscan`, parsing its output, deciding trusted/mismatch/unknown precedence — is implemented in the layer AGENTS.md says "only marshals commands". `sigrun-core::trust` has `check`/`record` but the policy of probing and evaluating lives here, so the CLI's `connections connect` path never runs it.
- **Risk of fixing:** Moving it into core changes when the trust check runs for CLI connects (currently CLI skips it entirely); `connect_guarded` is also called by the tray.
- **Risk of leaving:** Two front-ends with different security posture; the layer rule is violated by the most security-sensitive code in the app, inviting the next feature to follow the precedent.
- **Effort:** M — move the functions into a core module, have `connect_guarded` and the IPC `ConnectionsConnect` share it; tests in commands.rs move with them.
- **Suggested fix:** Move `probe_host_key`/`evaluate_trust` into `sigrun-core` (e.g. `trust::probe`) and call it from the host's connect handler behind a flag, or from a thin `connect_guarded` that both layers share.

### Trust dialog data is smuggled through an error message string and re-parsed with `split(" ")`
- **Area:** frontend
- **Location:** apps/desktop/src/views/Connections.tsx:515 and :538 (`const [fp, algo, key] = message.split(" ")`); producers: crates/sigrun-core/src/provision.rs:56-64 and apps/desktop/src-tauri/src/commands.rs:328-336
- **What:** `host_key_unknown`/`host_key_mismatch` errors encode the fingerprint, algorithm and key as one space-joined `Display` string (error.rs:43-48 warns the prefix would "break" the UI), and the frontend splits it back apart. A base64 key cannot contain spaces, but a fingerprint string produced by a different `ssh_key` version or any future prefix silently misparses — and the user would then trust the wrong key material.
- **Risk of fixing:** Needs a typed payload on the `Response::Error`/trust path end-to-end (IPC, `CommandError`, frontend); touches the tray's `connection-error` payload too.
- **Risk of leaving:** A trust decision (the app's core security UX) depends on string layout staying identical across three layers forever; the tests pin it, but only against the current producers.
- **Effort:** M — add a structured `details` field to the error response for these two codes and read it instead of splitting.
- **Suggested fix:** Carry `{fp, algo, key}` as JSON on the `host_key_*` error path (e.g. `Response::Error { code, message, details }`) and drop the split.

### `host.rs` is a 3,154-line module holding state, dispatch, every handler, and platform process code
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:1-1366 (non-test code)
- **What:** One file contains `HostState`, the mutex/poison helpers, pid-liveness and orphan-killing `libc` code, the idle auto-lock watcher, export/import orchestration, the 25-arm request dispatcher, and a dozen per-request handlers. Individually fine; together the file does four or five distinct jobs and handlers are not findable except by scrolling.
- **Risk of fixing:** Pure reorganization; risk is mechanical (missed re-export, `pub(crate)` visibility drift), caught by the gate.
- **Risk of leaving:** Every host change touches the same 3k-line file, so unrelated diffs collide; reviewers cannot keep the invariants (vault-lock ordering, identity checks) in view at once.
- **Effort:** M — split along existing seams: `host/state.rs` (HostState + sessions), `host/dispatch.rs` (handle_inner), `host/handlers.rs`, `host/sessions.rs` (pid/sweep/orphan code).
- **Suggested fix:** Turn `host.rs` into a `host/` module directory with the four files above, no behaviour change.

### `get_settings`/`save_settings` bypass the IPC protocol and smuggle settings as a JSON string inside `Response::Command`
- **Area:** tauri
- **Location:** apps/desktop/src-tauri/src/commands.rs:187-196; consumer: apps/desktop/src/api.ts:97
- **What:** The Tauri layer reads/writes `state.store` directly (not via `Request`) and returns settings by serializing them into the `Response::Command` variant — meant for a rendered ssh command line — so the frontend must `JSON.parse(await call<string>("get_settings"))`. The typed `Settings` channel exists in `Request`/`Response` shape already; this path just doesn't use it.
- **Risk of fixing:** Adding `Request::SettingsGet`/`SettingsSave` and `Response::Settings(Settings)` variants touches the CLI-visible protocol (old hosts/new clients mismatch), so it needs a protocol-version thought.
- **Risk of leaving:** A type lie at the trust boundary: any future `Response::Command` consumer can be handed settings JSON, and the frontend's `as Settings` cast validates nothing.
- **Effort:** S — add the two Request variants and one Response variant; the handlers are three lines each.
- **Suggested fix:** Add `SettingsGet`/`SettingsSave` to the IPC protocol and delete the `Response::Command` round-trip.

### `Settings.agent_enabled` is dead: written into the model, persisted, typed in TS, never read anywhere
- **Area:** core
- **Location:** crates/sigrun-core/src/model.rs:273 (field), :287 (default `true`); surfaced in apps/desktop/src/api.ts:46
- **What:** Nothing in core, CLI, Tauri, or the frontend ever reads `agent_enabled`. The agent is always served (`host::run`/`start_host` call `agent::serve` unconditionally). It is a setting that promises a toggle that does not exist.
- **Risk of fixing:** Removing the field from `Settings` breaks old `settings.toml` only if serde isn't lenient — it is (`#[serde(default)]` on the struct), so removal is safe.
- **Risk of leaving:** A persisted no-op that a user can set and reasonably expect to do something; also widens the public wire surface for free.
- **Effort:** S — delete the field from `Settings` and the TS type.
- **Suggested fix:** Delete `agent_enabled` everywhere (one field, one default, one TS type member).

### Duplicate validation and grouping logic across three layers, kept in sync by comments
- **Area:** frontend
- **Location:** apps/desktop/src/views/Connections.tsx:83-95 (`SSH_TOKEN`/`PORT`/`validateConnection`) mirroring crates/sigrun-core/src/terminal.rs:39 (`safe_host_bytes`); group-tree logic in Connections.tsx:14-52 mirroring apps/desktop/src-tauri/src/tray.rs:99-123 (`group_path`/`GroupNode`)
- **What:** The host/user character set and the group-path tree are implemented twice each, in different languages, held together only by a comment ("same rules as tray_menu_model"). The validation mirror is at least asserted by tests; the tree grouping is not cross-checked.
- **Risk of fixing:** Extracting a shared rule means either pushing it to the host (a new IPC call for "group tree") or generating it; either is more machinery than the duplication costs today.
- **Risk of leaving:** A rules change in one layer (e.g. allowing `~` in hosts) leaves the other silently disagreeing — the form refuses what the launcher accepts, or the tray nests groups the window renders flat.
- **Effort:** S-to-M — a shared test vector would cover it without moving code.
- **Suggested fix:** Add a test pinning tray and frontend tree-building to the same fixture inputs/outputs; leave the duplication, pin the behaviour.

### Dead public API surface in core: `UnlockedVault::lock`, `Vault::status`/`exists`, `AgentState::len`/`is_empty`, `TrustStatus` predicate helpers
- **Area:** core
- **Location:** crates/sigrun-core/src/vault/store.rs:176 (`lock`), :62-72 (`exists`/`status`); crates/sigrun-core/src/agent/session.rs:55-62 (`len`/`is_empty`); crates/sigrun-core/src/trust.rs:16-25
- **What:** These are `pub` and reachable outside the crate, but non-test callers don't exist: the host locks the vault by assigning `None` (`host.rs:157`), resolves status via `vault_file().exists()` (`host.rs:593-600`), never asks the agent its length, and matches on `TrustStatus` directly. The TrustStatus helpers are used only by trust.rs's own tests.
- **Risk of fixing:** None beyond deleting lines and possibly un-deleting one if a plan lands that needs it.
- **Risk of leaving:** Each one is an invite for a new caller to take a path that bypasses the host's actual state handling (e.g. `UnlockedVault::lock` drops the key without clearing the agent or active set — calling it would be a bug).
- **Effort:** S — delete; `UnlockedVault::lock` is the only one whose doc comment argues for keeping it, and no caller honours that contract.
- **Suggested fix:** Delete the unused methods; if `lock` is kept for the future, at least mark it `#[cfg(test)]` or make it private.

### `AgentState` panics on a poisoned lock while `HostState` recovers from one
- **Area:** core
- **Location:** crates/sigrun-core/src/agent/session.rs:39,44,50,56,60,79,93 (`.expect("agent lock poisoned")`) vs crates/sigrun-core/src/host.rs:83-87 (poison-recovery with a comment)
- **What:** Two sibling modules made opposite calls on the same situation. HostState deliberately recovers ("a poisoned lock means some handler panicked while holding it; recovering keeps that from panicking every later request"); AgentState deliberately panics in the agent's request path, where one bad sign request then poisons `request_identities` for every later ssh invocation.
- **Risk of fixing:** Aligning on recovery changes failure semantics: a poisoned agent lock currently crashes the serving task loudly; recovering hides the original panic further from its cause.
- **Risk of leaving:** A single panic inside the agent session turns every subsequent ssh auth into a panic until the host restarts — worse availability behaviour than the host chose for the same class of bug.
- **Effort:** S — mirror the host's `unwrap_or_else(|p| p.into_inner())` in the six sites.
- **Suggested fix:** Recover with `into_inner()` in AgentState, matching HostState's documented choice.

### `probe_reachable`'s inconclusive warning goes to the host's stderr, which desktop users never see
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:1119-1123
- **What:** When the reachability probe fails without proving unreachability, the host logs "probe inconclusive, letting ssh try" with `eprintln!`. In the desktop app the host runs in-process with no console, so the warning is silently dropped there; only `sigrun host` users see it. The response carries nothing about it either.
- **Risk of fixing:** Surfacing it as a response field or warning list changes the `ConnectionsConnect` reply shape.
- **Risk of leaving:** Intended diagnostics vanish on the primary platform; if the probe misbehaves (firewall false-positives) there is no user-visible evidence.
- **Effort:** S — log through `log`/tracing like `ssh_agent_lib` already does, or append to a warnings vector on the Ok response.
- **Suggested fix:** Route it through the `log` facade so the desktop can capture it, or drop the message as dead code.

### `Keys.tsx` import path: a failed dialog `save()` is conflated with backend failure, and `run()`'s refresh assumes success shape
- **Area:** frontend
- **Location:** apps/desktop/src/views/Keys.tsx:31-41
- **What:** `saveToDisk` catches picker errors and sets the generic error flag, but `exportKey` failures flow through `run`, which calls `refresh()` only on success — fine — yet the generic `setError(true)` banner gives the user no reason (unlike Connections, which surfaces `e.message`). Same pattern in `Credentials.tsx:113-119` (`setError(true)` discards the host's message). The host sends specific, user-actionable messages ("cannot export inside the SSH directory", "key could not be loaded"); the Keys/Credentials views throw them away.
- **Risk of fixing:** None — the message channel already exists (`CommandError.message`); only rendering changes.
- **Risk of leaving:** Users see "Something went wrong" for errors the host carefully explained; the careful error wording on the Rust side is wasted effort.
- **Effort:** S — store `string` instead of `boolean` in the error state of both views and render it.
- **Suggested fix:** Make the error state `string | null` in Keys and Credentials and render `e.message`, as Connections already does.

### `Credential.has_password` is client-writable on `update_credential` with only a comment as defence
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:984-1000 (update path re-asserts `stored.has_password`); the same struct field is accepted from the wire in `Request::CredentialsAdd`/`CredentialsUpdate` (ipc/protocol.rs:40-47)
- **What:** `CredentialsAdd` deliberately overwrites the client-sent flag (`host.rs:958`), and `CredentialsUpdate` re-imposes the stored one (`host.rs:992`) — but the *connection* add/update path neutralises its host-derived flags by assignment while the credential path relies on remembering to do it in every handler. The invariant "clients cannot set derived flags" is enforced per-handler, not per-type.
- **Risk of fixing:** Splitting a `CredentialInput` (no `has_password`) from `Credential` touches IPC types, the CLI constructor, and the frontend type.
- **Risk of leaving:** The next handler that forgets the two-line reset (as `CredentialsUpdate` nearly does) silently persists a flag that disagrees with the vault — the exact bug `host.rs:1050` comments about.
- **Effort:** M — introduce input types without derived fields and convert at the boundary.
- **Suggested fix:** Accept `CredentialInput { label, user, key_id }` + separate password on the wire; construct `Credential` (with derived flags) host-side only.

### CLI `connections edit --key` accepts any string and the id is never resolved, unlike every other lookup
- **Area:** cli
- **Location:** crates/sigrun-cli/src/cli.rs:516-518
- **What:** `keys remove` resolves `--id-or-name` through `resolve_key` (name → id) and `connections add --credential` resolves labels, but `connections edit --key <value>` stores the raw argument as `key_id` verbatim. A user typing the key *name* (which `keys list` shows by default) stores a dangling reference the host will only report as `not_found` at connect time.
- **Risk of fixing:** Resolution needs a `KeysList` round trip; a stored name that happened to be someone's id would change behaviour (resolve id-first, so no).
- **Risk of leaving:** An easy, silent way to corrupt a connection's key reference; inconsistent with the CLI's own conventions two screens down.
- **Effort:** S — list keys, `resolve_key`, store the id.
- **Suggested fix:** Resolve `--key` through `resolve_key` before `ConnectionsUpdate`.

### `sync_disk_keys` writes on a read: `KeysList` mutates the store on every call
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:618-631 calling `sync_disk_keys` (host.rs:1302-1352)
- **What:** Listing keys triggers a full `~/.ssh` discovery and an `upsert_key` (store write, lock, atomic rewrite of `data.toml`) per discovered key, on every `KeysList` — including the desktop poll and `sync_tray`'s connection list on every focus event. Correct, but a GET-shaped request with write-shaped cost and lock contention.
- **Risk of fixing:** Caching discovery (mtime of `~/.ssh`) or splitting list/scan changes when externally-added keys appear in the UI.
- **Risk of leaving:** Every list call rewrites `data.toml`; on a big `~/.ssh` with the store lock contended by a concurrent write, listing is needlessly slow and wears the file.
- **Effort:** M — needs an explicit invalidation story (rescan on vault unlock + manual refresh).
- **Suggested fix:** Split `KeysList` (read store) from a `KeysRescan` (discovery + upsert), and have the UI trigger rescan on unlock only.
