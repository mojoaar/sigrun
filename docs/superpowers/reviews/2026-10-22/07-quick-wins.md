# Pass 7 — Quick wins

### Copy SSH command button on a connection row
- **Area:** frontend
- **Location:** `apps/desktop/src/components/ConnectionRow.tsx:31` (button cluster); the Tauri `connect` command already accepts `print_only` and returns the argv string (`apps/desktop/src-tauri/src/commands.rs:375`), and the CLI exposes the same path as `connections connect --print`.
- **What:** A small "copy command" button per row that calls `connect` with `printOnly: true` and puts the returned `ssh …` line on the clipboard — the exact command Sigrun would launch, for pasting into scripts or docs.
- **Risk of fixing:** Needs a clipboard path (`@tauri-apps/plugin-clipboard-manager` or `navigator.clipboard`, plus a capability grant) and ~2 i18n strings ×7 locales; touching the connect command signature is avoidable by reusing `printOnly`.
- **Risk of leaving:** Users hand-retype `ssh user@host -p port` or run the CLI with `--print`.
- **Effort:** S — core and IPC already return the command; the work is one button, a clipboard write and a copied-toast string.

### CLI: `sigrun credentials password` (rotate a credential's secret)
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:208` (`CredentialsCommand` has only List/Add/Remove); the IPC variant `CredentialsSetPassword` already exists (`crates/sigrun-core/src/ipc/protocol.rs:53`) and the desktop Credentials tab uses it.
- **What:** `sigrun credentials password <id|label> [--password-stdin|--clear]`, mirroring `connections password`. README's "Current limitations" explicitly names this gap.
- **Risk of fixing:** None architectural — pure CLI wiring over an existing request, reusing `resolve_credential` and `read_passphrase`; must honour the "keyless credential cannot lose its password" host rule (it already does).
- **Risk of leaving:** Password rotation forces opening the desktop app; README keeps documenting a limitation.
- **Effort:** S — one clap variant plus ~30 lines copied from the `ConnectionsCommand::Password` arm, and a README limitations trim.

### CLI: `sigrun keys export <id|name> <path>`
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:177` (`KeysCommand` lacks Export); IPC `KeysExport { id, path }` exists (`crates/sigrun-core/src/ipc/protocol.rs:104`) and the Keys view already calls it via `exportKey` (`apps/desktop/src/api.ts:95`).
- **What:** Write a vault key's private half to a path (mode `0600`, refused inside `~/.ssh` and on existing files — all already enforced host-side), so headless/CLI users get the desktop's "Save to disk" too.
- **Risk of fixing:** Prints a private key to a user-chosen path — but the host-side guardrails already exist and the desktop already ships it; add `--yes`-style confirmation parity with `keys remove` if desired.
- **Risk of leaving:** CLI-only users cannot move a vault key out; asymmetric with `keys import`.
- **Effort:** S — one clap variant, `resolve_key`, one request, print the path.

### CLI: `sigrun credentials edit`
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:208`; IPC `CredentialsUpdate { credential }` already exists (`crates/sigrun-core/src/ipc/protocol.rs:45`) and is used by the desktop form.
- **What:** Edit a credential's label, user and key by id-or-label, mirroring `connections edit` — today a typo in a credential's user means remove + re-add, which is refused while connections use it.
- **Risk of fixing:** Must keep the one-identity-source and keyless-password invariants — both are already enforced host-side; CLI just sends the updated row.
- **Risk of leaving:** Renaming a credential that is in use is effectively impossible from the CLI.
- **Effort:** S — same shape as `ConnectionsCommand::Edit`: fetch list, resolve, patch present flags, send.

### Copy public key / fingerprint from a key card
- **Area:** frontend
- **Location:** `apps/desktop/src/components/KeyCard.tsx:21` already displays the fingerprint with `title=`; `KeyMetadata` carries `public_key` and `fingerprint` (`apps/desktop/src/api.ts:32`).
- **What:** Click the fingerprint (or a small button) to copy the full `public_key` line — the thing you paste into a server's `authorized_keys`, a GitHub deploy-key field or a ticket. Fingerprint copy is the same one-liner.
- **Risk of fixing:** Needs the clipboard grant and a couple of i18n strings; `public_key` is not secret so no security review beyond that.
- **Risk of leaving:** Users select the truncated monospace fingerprint by hand, or find the `.pub` material only via the bundle export.
- **Effort:** S — one handler and one button per card; no core or IPC change.

### `connections edit` by label, and able to change `--credential`
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:485` — Edit looks up `c.id == id` only, while `connect`, `password`, `provision` all resolve id-or-label via `resolve_connection`; Edit also lacks a `--credential` flag even though `ConnectionsUpdate` accepts the whole row.
- **What:** Resolve `id_or_label` like its sibling commands, and accept an optional `--credential` (with the existing host-side refusal when the connection still holds its own password).
- **Risk of fixing:** Low — reuses `resolve_connection` / `resolve_credential`; label ambiguity already errors cleanly.
- **Risk of leaving:** Inconsistent CLI ergonomics; moving a connection onto a credential is desktop-only.
- **Effort:** S — swap the find for `resolve_connection`, add one optional flag.

### Keyboard navigation of the filtered connection list
- **Area:** frontend
- **Location:** `apps/desktop/src/views/Connections.tsx:425` (query state) and the `renderNode` list; Cmd/Ctrl+F already focuses the search (`Connections.tsx:430`).
- **What:** ArrowDown/ArrowUp move a highlight through the flattened filtered list and Enter connects — the natural completion of Cmd+F for keyboard users.
- **Risk of fixing:** Needs a flat visible-order list (the tree is nested, so an index map over `groupTree` output) and roving `tabIndex`/aria-activedescendant to stay accessible; ~2 strings if a hint is shown.
- **Risk of leaving:** Search is mouse-dependent after typing; power users notice.
- **Effort:** M — the filtering and ordering exist; the work is selection state, key handling on the search input, and scroll-into-view.

### CLI: `sigrun shutdown`
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:29` (`Command` enum); `Request::Shutdown` exists and is handled (`crates/sigrun-core/src/ipc/protocol.rs:120`, `host.rs:516`).
- **What:** Ask the host to exit — the missing counterpart to `sigrun host`, useful for scripts and for restarting a headless host after an upgrade.
- **Risk of fixing:** One request; the only choice is output text on success.
- **Risk of leaving:** Users `pkill` the host, bypassing whatever clean shutdown does.
- **Effort:** S — a few lines following the `VaultCommand::Lock` pattern.

### Duplicate a connection from its row
- **Area:** frontend
- **Location:** `apps/desktop/src/components/ConnectionRow.tsx:36` (managed-only buttons); open the existing `Form` with `initial` prefilled but a fresh `id` and a tweaked label.
- **What:** A "Duplicate" button that opens the add form pre-filled from the row (label suffixed, e.g. "web (copy)") — the common "same host, different user/port" flow without retyping.
- **Risk of fixing:** Must not copy `has_password` truthfully — the secret is keyed by connection id, so the duplicate's `has_password` must start false (the model already separates metadata from the vault entry).
- **Risk of leaving:** Users retype near-identical connections.
- **Effort:** S — the Form already takes `initial`; only a clone-with-new-id helper and one string ×7 locales.

### Expose connection tags in the UI
- **Area:** frontend
- **Location:** `Connection.tags` is in the model (`crates/sigrun-core/src/model.rs:58`), settable via CLI `--tag` (`cli.rs:107`), serialised to the frontend (`api.ts:15`) — and rendered nowhere; the form drops them (`Connections.tsx:113` initialises `tags: []` and edit never surfaces them).
- **What:** Render tags as small badges on the row and a comma-separated input in the form; optionally let the search match them (filterConnections at `Connections.tsx:56` already takes a field list).
- **Risk of fixing:** Editing `tags` wholesale in the form is fine (CLI already replaces the vec); the only trap is losing CLI-set tags on a UI save if the field is omitted from the draft — so include it round-trip even before adding an input.
- **Risk of leaving:** A CLI-set tag is invisible in the app and silently preserved; users have no way to use a shipped model field.
- **Effort:** S — read-only badges + search match is trivial; the editable input adds one field and validation-free parsing.

### CLI: `sigrun credentials list --json`
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:726` — `connections list` and `keys list` take `--json`; credentials print a fixed table only.
- **What:** Add the `--json` flag for parity, emitting the same `Credential` vec the IPC returns.
- **Risk of fixing:** None — one branch and `serde_json::to_string_pretty`, as in the sibling arms.
- **Risk of leaving:** Scripting against credentials means parsing the table.
- **Effort:** S — minutes.

### CLI: `status` shows live session count
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:336` (`status` prints host/vault/agent); `Connection.active` already exists and `ConnectionsList` is a cheap extra request.
- **What:** Print `sessions: N active` (and optionally which labels) under `sigrun status`, so headless users see what the pulsing dot shows in the app.
- **Risk of fixing:** One extra IPC round-trip per `status` call; on Windows the number is always 0 (sessions untracked) — worth a parenthetical or nothing.
- **Risk of leaving:** No CLI visibility into live sessions.
- **Effort:** S — one `ConnectionsList` request and a println.

### Show a connection's resolved identity in the edit form
- **Area:** frontend
- **Location:** `apps/desktop/src/views/Connections.tsx:577` — `keyName` already resolves the credential's key for the row; the form itself shows only the credential dropdown without stating which user/key that credential carries.
- **What:** Under the credential picker, one muted line: "Signs in as deploy with key work" — read from the already-loaded `credentials` and `keys` arrays.
- **Risk of fixing:** Pure render of loaded data plus ~1 string ×7 locales.
- **Risk of leaving:** Users pick a credential blind and find out at connect time.
- **Effort:** S — a conditional `<p>` reusing `methodKey`-style logic.

### CLI: `sigrun keys generate --public-key-only` style stdout clarity
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:679` — generate prints only the public key line, import prints the same; the table form of `keys list` truncates nothing but also doesn't show the public key.
- **What:** `sigrun keys list --json` already carries `public_key`; add a `keys show <id|name>` that prints just the public key line for a known key (the common "give me the .pub contents" step without a JSON parse).
- **Risk of fixing:** None — a filtered lookup and a println over `KeysList`.
- **Risk of leaving:** Users `keys list --json | jq` for a routine operation.
- **Effort:** S — mirrors `resolve_key` + one print.

### Tray/menu: "Copy SSH command" next to each connection
- **Area:** tauri
- **Location:** the tray menu builder (crates/sigrun-core tray_menu_model + `apps/desktop/src-tauri/src/lib.rs` menu wiring); `connect_inner` with `print_only` already produces the string.
- **What:** A submenu item per connection that copies the launch command instead of opening a terminal.
- **Risk of fixing:** Tauri menu APIs plus a clipboard call inside Rust (`arboard` would be a new dependency — the stronger reason bar may push this to the webview via an event, which complicates it); cap at M.
- **Risk of leaving:** Tray users can't get the command without the CLI.
- **Effort:** M — model exists but the clipboard story in the tray process is the open question.

### Search should match tags and credential user
- **Area:** frontend
- **Location:** `filterConnections` (`apps/desktop/src/views/Connections.tsx:56`) matches label/user/host/port/group but not `c.tags`, and not the credential's user (rows display `credentialUser` but search ignores it).
- **What:** Add `tags` and the resolved credential user to the matched field list.
- **Risk of fixing:** Needs the credentials list passed into the filter (already loaded in the view) or a resolved-user precompute; one line each otherwise.
- **Risk of leaving:** Searching a tag set via CLI or a shared sign-in name finds nothing.
- **Effort:** S — extend the `.some(...)` array.

### IPC: `KeysUpdate` / rename a key
- **Area:** core
- **Location:** no rename path exists — `protocol.rs` has `KeysGenerate/Import/Remove/Export` only; a typo in a key name means remove + re-import (losing vault residency for generated keys: remove destroys the private half).
- **What:** `KeysUpdate { id, name }` (metadata only, vault untouched) plus a CLI `keys rename` and an inline rename in the Keys view.
- **Risk of fixing:** New IPC variant + store write; must keep name-uniqueness rules if any exist (none enforced today beyond display).
- **Risk of leaving:** Renaming a generated key is impossible without regenerating and re-provisioning it everywhere.
- **Effort:** M — touches protocol, host handler, CLI and UI; still small per layer.

### CLI: `connections add --clone <id|label>`
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:436` (Add arm).
- **What:** Seed a new connection from an existing one, then apply any given flags — the scriptable version of the UI duplicate button.
- **Risk of fixing:** Must not copy `has_password` (secret is per-id) and must refuse to clone `source: discovered` rows, which are read-only.
- **Risk of leaving:** Users script an add from scratch.
- **Effort:** S — fetch, clone, patch, send; shares resolve helpers.

### Show vault auto-lock countdown / last-unlock in Settings
- **Area:** frontend
- **Location:** `apps/desktop/src/views/Settings.tsx` auto-lock field; the host knows the idle deadline (auto_lock_minutes) but exposes no remaining time.
- **What:** A muted "locks in ~N min" line while unlocked — or skip the countdown and just confirm the setting took effect.
- **Risk of fixing:** A live countdown needs a new status field (IPC change) or polling; the static confirmation is string-only.
- **Risk of leaving:** Users are surprised by the vault gate mid-work.
- **Effort:** M for the honest countdown (StatusInfo change), S for the static hint.

### `connections list --group` should match group prefixes
- **Area:** cli
- **Location:** `crates/sigrun-cli/src/cli.rs:412` — `group_ok` requires exact equality (`c.group.as_deref() == Some(g)`), while the UI's tree nests `PVE/Folder 1` under `PVE`.
- **What:** Match `g` against the connection's group or any of its `/`-prefixes, so `--group PVE` lists the whole subtree like the app does.
- **Risk of fixing:** One helper mirroring `groupPaths` prefix logic; changes existing filter semantics (strictly widens matches).
- **Risk of leaving:** CLI group filtering disagrees with the visual grouping.
- **Effort:** S — a `starts_with`-with-segment-boundary check.
