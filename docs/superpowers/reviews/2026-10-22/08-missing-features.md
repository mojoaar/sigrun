# Pass 8 — Missing features

Findings rank by expected user value, highest first. Items already listed under
README "Current limitations" are marked **(documented)**.

### Vault passphrase change
- **Area:** core
- **Location:** crates/sigrun-core/src/vault/store.rs (no re-key path; `KdfParams` is pinned at create)
- **What:** There is no way to change the master passphrase. A user who suspects the passphrase leaked, or who picked a weak one at first run, must export connections (losing keys, passwords and credentials), delete the vault, re-create it, and re-import/re-provision everything. Every password manager treats passphrase rotation as table stakes.
- **Risk of fixing:** re-encrypting the payload with a fresh salt/key is simple crypto but a failed write mid-rotation must not brick the only copy of the secrets.
- **Risk of leaving:** a compromised passphrase is permanent; the only remedy is a full manual rebuild of the vault.
- **Effort:** M — one `VaultChangePassphrase` IPC (old + new passphrase), atomic re-seal through the existing `write_atomic`, CLI + Settings entry.
- **Suggested fix:** add `UnlockedVault::rekey(new_passphrase)` that re-derives with a fresh salt and re-seals through `write_atomic`; wire `vault change-passphrase` and a Settings row.

### Connection metadata: notes, custom SSH options, per-connection proxy
- **Area:** core
- **Location:** crates/sigrun-core/src/model.rs:37-74 (`Connection` has no notes/extra-options fields)
- **What:** A connection is exactly label/host/user/port/key/group/tags. There is no free-text note ("reboot before patching"), no per-connection ssh option (`-L 8080:localhost:80`, `ProxyJump bastion`, `ServerAliveInterval`), and no jump-host support. Anything beyond a plain `ssh user@host -p port` is unreachable from a managed connection — and the reachability probe already lies for ProxyJump hosts (documented). Users with bastions or port-forward workflows cannot model them at all.
- **Risk of fixing:** any free-text option field re-opens the argv/shell-injection boundary that `safe_host_bytes` currently closes; needs an allow-list or structured fields (jump host, forwards), not a raw string.
- **Risk of leaving:** users with bastions or tunneling needs — a large share of the target audience — cannot adopt Sigrun for those hosts.
- **Effort:** L — structured `proxy_jump` / `local_forwards` fields, model + form + CLI + export + probe awareness.
- **Suggested fix:** add optional `proxy_jump: Option<String>` (another connection id or host) and `options: Vec<String>` allow-listed to safe `-o` keys; render into `ssh_invocation`; skip the direct TCP probe when a jump is set.

### Full backup / disaster recovery of the vault
- **Area:** core
- **Location:** crates/sigrun-core/src/bundle.rs (export deliberately excludes secrets; README "Not a backup")
- **What:** The export is connections + public keys only, by design. The only thing that restores private keys and passwords is "the vault file plus its passphrase" — but nothing in the app helps the user copy that file anywhere, tells them where it is, or verifies a copy still opens. A disk failure loses every key and password permanently. Documented as intentional ("a transfer, not a backup"), yet backup is a reasonable expectation of a secrets vault.
- **Risk of fixing:** a one-click "Back up vault…" copies the encrypted envelope — safe since it is ciphertext — but users may mistake it for protection against a forgotten passphrase, and it slightly widens the ciphertext's exposure surface.
- **Risk of leaving:** single point of total loss; one dead laptop means re-keying every server.
- **Effort:** S — a "Back up vault file…" button that copies `vault.json` via the save dialog plus a README line; restore = drop the file back in the data dir.
- **Suggested fix:** add Settings → "Back up vault…" that copies the sealed envelope; document restore in Help. (Deliberately conflicts with nothing: the file is already ciphertext.)

### Host-key (known_hosts) management UI
- **Area:** frontend
- **Location:** crates/sigrun-core/src/trust.rs (store exists; no list/forget IPC, no UI surface)
- **What:** Once a host key is trusted, the user can never see the list, never revoke one entry, and never pre-seed one from the CLI (documented: the CLI cannot record a first-seen host key, so `provision` against a new host fails until the desktop app has trusted it). A mismatch banner says "changed key" but offers no path to inspect the old vs new fingerprint side by side or to forget the stale entry.
- **Risk of fixing:** a forget/replace action is a security-sensitive surface; it must show fingerprints and require explicit confirmation, not become an "auto-update known_hosts" button.
- **Risk of leaving:** a legitimately rotated server key bricks the connection until the user hand-edits Sigrun's `known_hosts`; headless/CLI-only users cannot provision a new host at all.
- **Effort:** M — `TrustList`/`TrustForget` IPC + a Settings section, plus `sigrun trust add <host>` doing a `ssh-keyscan`-style probe with a fingerprint prompt.
- **Suggested fix:** expose list/forget over IPC and render a "Trusted hosts" list in Settings; add `sigrun connections trust <id>` that probes, shows the fingerprint, and records on confirm.

### Vault auto-lock / unlock observability and notifications
- **Area:** tauri
- **Location:** crates/sigrun-core/src/host.rs:166-196 (auto-lock sets a flag; nothing notifies the user)
- **What:** When the idle auto-lock fires, the app silently re-gates to the passphrase screen; the tray menu just stops working. There is no notification ("vault locked after 30 min idle"), no countdown, no visible idle state, and no log anywhere of lock/unlock events. Combined with zero logging (only one `log::error!` in the whole workspace), the user is blind to everything the host does: agent reload warnings, probe decisions, session sweeps, IPC errors all vanish.
- **Risk of fixing:** a log file must never capture secrets — the codebase is careful, but a logging framework invites a future leak; needs a redaction rule and a bounded file.
- **Risk of leaving:** every failure mode is invisible; bug reports from users contain nothing actionable.
- **Effort:** M — tray notification on auto-lock (S) plus a rotating, redacted `sigrun.log` with vault/agent/session events (M).
- **Suggested fix:** emit a Tauri notification on auto-lock; add `log`+`simplelog`-style file logging at the host with a documented "no secrets" lint pass.

### Per-connection SSH runtime options: keepalive, compression, jump from ssh_config
- **Area:** core
- **Location:** crates/sigrun-core/src/ssh_config.rs:53 (wildcards/negations dropped), terminal.rs `ssh_invocation` (no `-o` passthrough)
- **What:** Discovered `~/.ssh/config` hosts lose `ProxyJump`, `LocalForward`, `ServerAliveInterval`, etc. — the parser keeps only alias/hostname/user/port/identityfile (Include not followed is documented). A managed connection cannot request keepalive or compression either. A user migrating from a real ssh config finds half their hosts connect differently under Sigrun.
- **Risk of fixing:** parsing more of ssh_config grows a real grammar (Match blocks, includes, first-value-wins) — a maintenance commitment, not a one-off.
- **Risk of leaving:** discovered hosts are second-class: they appear in the list but silently behave differently than `ssh alias` in a terminal.
- **Effort:** L — full ssh_config semantics plus an option model that survives export.
- **Suggested fix:** track upstream `ssh-config` crate or extend the parser for the common directives; pass safe ones through as `-o` args for discovered connections.

### Key lifecycle: comment, expiry/rotation metadata, change key passphrase
- **Area:** core
- **Location:** crates/sigrun-core/src/keys (no rename beyond name; no passphrase change for a vault key; `notes` field exists in model but is never surfaced)
- **What:** A vault key's passphrase cannot be changed (the PEM is re-stored only at import/generate), a generated key's comment is fixed, and although `KeyMetadata.notes` and `created_at` exist, the UI shows neither and there is no "this key is 2 years old, rotate it" affordance. Rotating a key means generate + re-provision every server by hand, one connection at a time.
- **Risk of fixing:** low crypto risk (re-encrypting a PEM inside the vault), but a "provision everywhere" bulk action needs a fleet-wide confirmation story.
- **Risk of leaving:** keys age silently; rotation is painful enough that users won't do it.
- **Effort:** M — change-passphrase IPC is small; the rotation workflow (new key → provision to every connection using the old one → swap references) is the bulk of it.
- **Suggested fix:** add `keys rekey <id>` (re-wrap the stored PEM) and a "rotate" action that generates a successor and offers to provision it to all connections referencing the old key.

### Bulk operations and multi-select in the UI
- **Area:** frontend
- **Location:** apps/desktop/src/views/Connections.tsx (one row action at a time)
- **What:** No multi-select, no bulk delete/move-to-group, no duplicate-connection action, no drag between groups. Cleaning up after an import of 50 connections is 50 individual confirm dialogs.
- **Risk of fixing:** bulk delete needs a stronger confirmation (count, maybe type-to-confirm); otherwise low.
- **Risk of leaving:** large inventories are tedious enough to push users back to editing `data.toml` by hand.
- **Effort:** M — checkbox selection model, bulk toolbar, one bulk IPC (`ConnectionsRemoveMany`) to keep it transactional.
- **Suggested fix:** add row checkboxes + "Delete selected / Move to group…" toolbar; a single host request applying the set.

### SFTP / file transfer and non-shell ssh invocation
- **Area:** core
- **Location:** crates/sigrun-core/src/terminal.rs (`ssh_invocation` is hard-coded to interactive `ssh`)
- **What:** Connect always opens an interactive shell. There is no "Open SFTP here" (sftp or a file manager), no "run this one command" action, and no way to reuse a connection's identity for `scp`/`rsync` from the CLI (`--print` exists, which is the workaround). Users managing servers frequently want a file browser for the same host.
- **Risk of fixing:** an SFTP browser is a whole new surface (listing, transfer, progress); the cheap version (`sftp` in a terminal, or `--print` variants for scp/rsync) is small.
- **Risk of leaving:** users keep a second tool for files, weakening the "one place for keys and connections" promise.
- **Effort:** L for a real browser; S for `sftp`/`scp`/`rsync` launch templates reusing `ssh_invocation`.
- **Suggested fix:** add a per-row "SFTP" action that launches the same env/identity with `sftp` instead of `ssh`; defer a graphical browser.

### Session list and "close all" / reconnect
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:43-49 (`active`/`sessions` are per-connection booleans, not a session list)
- **What:** Live sessions are a dot, not a list: no session start time, no remote user/host summary line, no way to see all open sessions at once, no "reconnect" or "open a second session" affordance from a session view, and no way to kill a stuck session from the app (the sweep only clears dead pids). Two concurrent sessions of one connection are tracked as one (documented).
- **Risk of fixing:** per-session tracking multiplies the pid-watching state; killing a session sends signals to user processes, so a wrong pid recycle hurts.
- **Risk of leaving:** the user cannot answer "what am I connected to right now?" from the app that opened the sessions.
- **Effort:** M — sessions keyed by (connection, pid) with a sessions list IPC, a Sessions view, and a kill action through the existing SIGHUP path.
- **Suggested fix:** store `Vec<Session{id, pid, started_at}>` per connection; render a Sessions panel; add "end session" sending SIGHUP to the group as `end_orphaned_session` already does.

### Agent: lock/unlock granularity and confirmation on use
- **Area:** core
- **Location:** crates/sigrun-core/src/agent/session.rs (keys load en masse; no per-key constraint or confirm hook)
- **What:** Unlocking the vault loads every vault key into the agent at once and serves any signature request from any local process that can reach the socket (mode 0600, but every user process qualifies). There is no per-key loading, no "confirm each use" extension, and no `ssh-add -c`-style confirmation — the stock agent-socket threat model with no mitigation beyond the file mode.
- **Risk of fixing:** confirmation needs a UI prompt wired from the agent's sign path (async, cross-process) — real complexity; per-key loading touches connect's assumption that the agent has everything.
- **Risk of leaving:** any malware running as the user gets free signatures for every vault key while the vault is unlocked, with no user-visible signal.
- **Effort:** L — confirm-on-use requires an agent extension + a desktop prompt + timeout handling.
- **Suggested fix:** start with per-connection scoping at launch (agent already supports key selection via `contains_key_id`; serve only the resolved key per launched session is not possible on one socket, so document), then evaluate the confirm extension behind a setting.

### Multi-machine: no conflict story when the vault travels
- **Area:** docs
- **Location:** n/a
- **What:** The README's transfer story is connections-only, and the vault file is the single source of secrets — but nothing says what happens when a user copies `vault.json` + `data.toml` to two machines and edits both (adds a connection on each). The next manual copy silently overwrites one side. There is no merge, no sync, no "vault changed on disk" detection. This is a deliberate non-goal (no cloud), but the multi-machine path users will actually take — syncing the data dir with Syncthing/Dropbox — is unaddressed and unsafe-by-silence. (Listing this explicitly because it conflicts with "no cloud": the fix is documentation + change detection, not sync.)
- **Risk of fixing:** a "file changed under us" check can false-positive on legitimate restore flows; documentation is cheap but easy to ignore.
- **Risk of leaving:** users who DIY-sync lose data with no warning; the first data-loss report will be about this.
- **Effort:** S — document the one-writer rule and add an mtime/fingerprint check on the data files before write ("reloaded, changed on disk").
- **Suggested fix:** store an mtime/hash with each in-memory load and refuse (or reload) on write when it drifted; README section on multi-machine.

### Recovery: forgotten passphrase and orphaned secrets
- **Area:** core
- **Location:** crates/sigrun-core/src/vault/store.rs
- **What:** Forgetting the master passphrase is total and silent — no hint, no recovery key, no escrow option, not even a "wipe and start over" command (the user must find and delete the data dir by hand). Relatedly, deleting a connection while the vault is locked leaves the password sealed in the vault forever (documented behavior), with no "prune orphaned secrets" command to reclaim them.
- **Risk of fixing:** any recovery mechanism (recovery key, escrow) weakens the single-passphrase model and adds crypto surface; a prune command needs to prove an id is referenced nowhere (connections + credentials).
- **Risk of leaving:** forgotten passphrase = lose everything with no guided path; orphaned secrets accumulate invisibly.
- **Effort:** S for `vault reset` + `vault prune` (list/remove secrets with no referencing row); L for an optional recovery key.
- **Suggested fix:** add `sigrun vault prune` (delete vault entries no connection/credential/key references) and a "forgot passphrase" flow that wipes the vault file after confirmation.

### Import from other tools / ssh config adoption
- **Area:** core
- **Location:** crates/sigrun-core/src/ssh_config.rs (read-only listing; no "adopt this host" action)
- **What:** Discovered `~/.ssh/config` hosts are listed but read-only — there is no "Import as managed connection" button that copies alias/host/user/port/identityfile into a managed row (with the key optionally imported to the vault). Nor any import from other managers' formats. The migration path is retyping.
- **Risk of fixing:** low; the data is already parsed, adoption is a copy + optional key import.
- **Risk of leaving:** onboarding friction for exactly the users with the messiest existing setups.
- **Effort:** S — one "Adopt" action on a discovered row → pre-filled add form; CLI `connections adopt <alias>`.
- **Suggested fix:** add an Adopt button that opens the connection form pre-filled from the SshHost, with an optional "import IdentityFile into vault" checkbox.

### Connection test without opening a terminal
- **Area:** core
- **Location:** crates/sigrun-core/src/terminal.rs (`probe_reachable` is TCP-only; provision's verify path exists but is unreachable without installing)
- **What:** The only ways to validate a connection are "open a terminal and watch ssh" or a TCP ping that proves nothing about auth. There is no "Test connection" that actually authenticates (like provision's verify step) and reports "key accepted / password accepted / host key trusted" without installing anything. Debugging a new server means reading ssh's error in a terminal window.
- **Risk of fixing:** reuses the russh path from provision; a failed auth attempt can lock accounts under aggressive fail2ban setups, so it must be an explicit user action.
- **Risk of leaving:** misconfigurations surface only as a failed terminal window; no way to distinguish "host down" from "key rejected" from the app.
- **Effort:** M — a `ConnectionsTest` IPC doing trust-check + auth attempt with the resolved identity, reporting structured outcomes.
- **Suggested fix:** add `sigrun connections test <id>` and a per-row Test action calling a russh auth probe (no command exec).

### Headless server / remote host operation
- **Area:** cli
- **Location:** crates/sigrun-cli/src/cli.rs:31 (`sigrun host` runs in a terminal; documented "does not auto-start")
- **What:** The host must be started by hand (`sigrun host`) or by launching the desktop app — there is no systemd user unit, launchd agent, or Windows service shipped or generated, and no guidance for running headless on a server (documented). The CLI-first user story in the README begins with "leave a terminal running".
- **Risk of fixing:** packaging service units for three platforms is release-engineering surface; an auto-started host holding an unlocked vault changes the security posture users may not expect.
- **Risk of leaving:** CLI users get a broken-pipe experience after every reboot until they remember to start the host.
- **Effort:** M — `sigrun host install` that writes a systemd --user unit / launchd plist / scheduled task, with docs; the host itself needs no changes.
- **Suggested fix:** add `sigrun host install|uninstall` generating the platform service unit; keep auto-lock defaults so a booted host is locked until unlocked.

### Credential expiry/rotation reminders and password visibility rules
- **Area:** frontend
- **Location:** apps/desktop/src/views/Credentials.tsx (list/add/remove only; documented that rotation is app-only)
- **What:** Credentials have no `rotated_at`, no expiry hint, and no way to see *which* connections break before deleting one beyond a count. Combined with the documented CLI gap (`credentials` has no `password` subcommand), rotating a shared password from a headless setup is impossible, and there's no prompt to rotate after N days.
- **Risk of fixing:** rotation reminders are UI noise if wrong; needs an opt-in age threshold.
- **Risk of leaving:** shared passwords rot; the headless CLI user cannot rotate at all.
- **Effort:** S for CLI `credentials password <id>` (the IPC `CredentialsSetPassword` already exists); M for metadata + reminders.
- **Suggested fix:** add `sigrun credentials password <id> [--stdin|--clear]` first (one IPC call); optionally `rotated_at` metadata with a Settings threshold.

### Configurable security posture: KDF params, auto-lock per machine, screenshot privacy
- **Area:** core
- **Location:** crates/sigrun-core/src/vault/crypto.rs:30 (KDF params pinned), model.rs:266-292 (settings surface)
- **What:** Argon2id parameters are hard-coded, auto-lock is global with fixed presets, and there is no switch for screen-shield behaviors (hide window on screen share, clear clipboard after copy — moot since there's no copy-password feature, which is itself absent). Power users on slow machines (RPi) or high-threat users cannot tune the KDF up or down at vault creation.
- **Risk of fixing:** exposing KDF params invites users to weaken their vault; must keep safe minimums.
- **Risk of leaving:** low — defaults are sane; this is a flexibility gap, not a hole.
- **Effort:** M — KDF params are already stored per-vault (`KdfParams` in the envelope), so this is a create-time option + validation.
- **Suggested fix:** offer "paranoid/standard" at vault init mapping to two pinned parameter sets; leave runtime tuning out.

### AppleScript terminals don't get the agent (feature gap with a fix path)
- **Area:** core
- **Location:** crates/sigrun-core/src/terminal.rs (`agent_prefix` rides the command string; README documents the AppleScript gap)
- **What:** Documented: Terminal.app, iTerm and Ghostty-tab launches don't see `SSH_AUTH_SOCK`, so vault keys silently don't work in the three most default macOS paths (Terminal.app is the factory default). The env is written into the command string already, so a follow-on fix (e.g. writing a per-session env file the launched shell sources, or exporting inside the `do script` text before `clear`) is an engineering task, not an architecture change.
- **Risk of fixing:** any fix that puts the socket path into the typed `do script` text must keep the redaction/escaping discipline already in place.
- **Risk of leaving:** the default macOS user experience is "vault keys don't work" — the central feature fails on the default terminal.
- **Effort:** M — the launched line already runs through a shell; prepend `export SSH_AUTH_SOCK=…` into the typed text (it's the `write text`/`do script` payload, which is exactly where `clear` already lives).
- **Suggested fix:** include the env assignments in the AppleScript-typed command line itself rather than relying on process-env inheritance.
