# Sigrun — full-app review, 2026-10-22

Ranked, deduplicated findings from nine review passes over `main` at `673c1e3`
(branch `review/full-app-audit`). Each finding states what is wrong, the risk of
fixing it, the risk of leaving it, and the effort. **Nothing here has been
fixed.** Per-pass detail lives in the sibling files (`01-code-quality.md` …
`09-docs.md`). All nine passes are complete; pass 6 (interactive UI) ran the real
React app in a stubbed-Tauri browser harness and reproduced #12 and #13 live
(screenshots in `./shots/`).

Severity key: **Critical** = security or data loss, fix before anything user-facing.
**High** = real user harm or a broken promise. **Medium** = correctness /
maintainability / polish. **Feature** = not a defect; a candidate addition.

Effort: **S** = under a day, one area. **M** = a day-ish, touches a boundary
(IPC, protocol, platform). **L** = needs design or multiple layers.

---

## Critical

### 1. IPC server serves secrets and privileged actions to any local client, unauthenticated
- **Location:** `crates/sigrun-core/src/ipc/server.rs:103-146`, `host.rs:468` (`PasswordGet`), `:482` (`KeysExport`), `:516` (`Shutdown`)
- **What:** The Unix socket is gated only by the `0700` data dir; there is no peer-credential check and no token. Any process running as the user can read every stored password (`PasswordGet`), export vault private keys (`KeysExport`), shut the host down, or launch terminals while the vault is unlocked. Unlike ssh-agent (sign-only), these disclose raw secret material.
- **Risk of fixing:** peer-uid checks are platform-specific (`getpeereid`/`SO_PEERCRED`) and must not break legitimate same-uid clients.
- **Risk of leaving:** any same-user malware walks away with every SSH password and private key in one socket call, no prompt.
- **Effort:** S — check peer uid on accept, drop mismatches before reading a byte.

### 2. No throttling/backoff on failed vault unlock over IPC
- **Location:** `crates/sigrun-core/src/host.rs:398-407`
- **What:** `VaultUnlock` runs the KDF and returns `invalid_passphrase` on failure with no counter, delay, lockout, or log — forever, at line rate. Combined with #1, a same-user process can brute-force the passphrase online at Argon2id speed, silently. (Confirmed sound: Argon2id params are the documented m=65536/t=3/p=4; a failed unlock never clobbers an unlocked vault; wrong-passphrase vs corruption errors are distinct and tested.)
- **Risk of fixing:** a hard lockout could DoS the legitimate user; must be backoff-only, never a wipe.
- **Risk of leaving:** unbounded, silent online brute force against the passphrase.
- **Effort:** S — a `failed_attempts` / `next_allowed_at` pair in `HostState` with exponential backoff capped ~30 s.

### 3. Parallel unlock attempts can exhaust memory (Argon2 × blocking pool)
- **Location:** `crates/sigrun-core/src/host.rs:400-403`
- **What:** each `VaultUnlock` runs on `spawn_blocking` (pool default 512 threads) and holds 64 MiB for Argon2; dozens of concurrent attempts allocate gigabytes and can OOM the desktop host.
- **Risk of fixing:** serializing unlocks makes a legitimate unlock wait behind a queue — acceptable, unlocks are rare.
- **Risk of leaving:** trivial same-user memory-exhaustion DoS that kills the app and drops live sessions.
- **Effort:** S — a `Semaphore(1)` around the unlock `spawn_blocking`.

### 4. IPC read loop buffers an unbounded line before the size check
- **Location:** `crates/sigrun-core/src/ipc/server.rs:111-117`; `protocol.rs:10` (`MAX_LINE` enforced only after the full line is buffered)
- **What:** `MAX_LINE` (1 MiB) is checked after the line is in memory; a client streaming bytes with no newline grows the per-connection buffer without limit. Each accepted connection is a spawned task. (Also surfaced by the bug-hunt pass.)
- **Risk of fixing:** none — legitimate requests are well under 1 MiB.
- **Risk of leaving:** same-user memory-exhaustion DoS against the host.
- **Effort:** S — `reader.take(MAX_LINE+1)` before `read_until`, error at the cap.

### 5. Host-key pre-flight fails open when `ssh-keyscan` is unavailable
- **Location:** `apps/desktop/src-tauri/src/commands.rs:363-373` (`check_trust`), `:294-315` (`probe_host_key`)
- **What:** the TOFU gate returns `Ok(())` when keyscan is missing/times out/errors, skipping the check. An attacker who drops the keyscan traffic bypasses mismatch detection; the launched `ssh` only consults the user's own `~/.ssh/known_hosts`, which knows nothing of hosts recorded only in Sigrun's store. AGENTS.md says trust "fails closed".
- **Risk of fixing:** a connect on a machine without keyscan now refuses; needs a UI path to record keys first.
- **Risk of leaving:** a MITM who suppresses the 5 s keyscan downgrades every Sigrun-known-host connect to a plain ssh prompt the user is trained to accept.
- **Effort:** S — make probe failure a `CommandError` unless an explicit `force` is passed.

### 6. Vault `put`/`remove` diverge memory from disk when `persist` fails
- **Location:** `crates/sigrun-core/src/vault/store.rs:160-170`
- **What:** `put` mutates the in-memory payload then persists; on persist failure (disk full, permission) the secret lives in memory but not on disk and is lost at lock, while later operations in the session see the phantom entry. `remove` is the mirror: deleted entry resurrects on next unlock if persist failed.
- **Risk of fixing:** low — snapshot/restore the payload around `persist()`.
- **Risk of leaving:** after a transient disk error the session shows state the disk won't reproduce (lost new keys/passwords, resurrected deleted ones).
- **Effort:** S — revert the payload mutation on persist error.

### 7. Attacker-controlled KDF params in a tampered vault file can hang the app
- **Location:** `crates/sigrun-core/src/vault/crypto.rs:52-65` (`derive_key` trusts envelope `m/t/p`), `store.rs:108-117`
- **What:** `KdfParams` are read from vault JSON and passed to `argon2::Params::new` unchecked; values under the crate's ceiling (~4 GiB, huge iteration counts) are accepted, so a tampered vault file turns one unlock into a multi-GB allocation or near-infinite derivation. Needs write access to the data dir (self-DoS territory), but the failure is a hang, not an error.
- **Risk of fixing:** low — clamp to the pinned parameters before deriving.
- **Risk of leaving:** a corrupted/tampered envelope hangs the app on unlock instead of reporting corruption.
- **Effort:** S — reject params that don't match the pinned profile until a migration exists.

---

## High

### 8. TOFU trust gate lives in the Tauri layer; CLI connects skip it entirely
- **Location:** `apps/desktop/src-tauri/src/commands.rs:294-392` (`probe_host_key`/`evaluate_trust`/`check_trust`/`connect_guarded`) vs `crates/sigrun-core/src/host.rs:1063-1163` (`connections_connect`)
- **What:** the whole pre-flight TOFU gate (spawn keyscan, parse, decide precedence) sits in the layer AGENTS.md says "only marshals commands". The IPC `ConnectionsConnect` handler — which the CLI drives — never consults Sigrun's known_hosts. Two clients of the same host get different security postures; the layer rule is inverted for a security control. (Code-quality and security passes converge on this as one root cause.)
- **Risk of fixing:** CLI connects to previously-provisioned hosts start prompting/failing where they used to sail through.
- **Risk of leaving:** `sigrun connect` ignores the trust database the desktop maintains; a mismatch the desktop would refuse is never shown to a CLI user.
- **Effort:** M — move the gate into `sigrun-core` behind a flag so both clients share it.

### 9. Trust dialog data smuggled through an error string, re-parsed with `split(" ")`
- **Location:** `apps/desktop/src/views/Connections.tsx:515,538`; producers `provision.rs:56-64`, `commands.rs:328-336`
- **What:** `host_key_unknown`/`host_key_mismatch` encode fingerprint/algorithm/key as one space-joined `Display` string and the frontend splits it back. Any future change to the layout silently misparses — and the user would then trust the wrong key material. The app's core security UX depends on string layout across three layers.
- **Risk of fixing:** needs a typed payload on the error path end-to-end (IPC, `CommandError`, frontend); touches the tray's `connection-error` payload.
- **Risk of leaving:** a trust decision rests on string layout staying identical forever.
- **Effort:** M — add a structured `details` field to the trust error codes; drop the split.

### 10. Stale watched pid can clear a new live session and SIGHUP a recycled process group
- **Location:** `crates/sigrun-core/src/host.rs:506-509` (`ConnectionEnded` clears `active` but not the session), `136-153` (`sweep_dead_sessions`), `327-334` (`end_orphaned_session`)
- **What:** `ConnectionEnded` never calls `forget_session` although that function's doc says it goes "alongside clear_active". On a quick reconnect, if the 2 s sweep fires before the new `session-started` arrives, the dead pid clears the live session's dot and `end_orphaned_session` sends SIGHUP to whatever process group now owns the recycled pid.
- **Risk of fixing:** low — one extra map removal plus a liveness guard.
- **Risk of leaving:** the live-session dot silently disappears on a reconnect inside a poll window; a recycled pid can take an unrelated SIGHUP.
- **Effort:** S — `forget_session` in the `ConnectionEnded` handler; only signal when the pid is alive but orphaned.

### 11. `~/.ssh/config` `Key=Value` syntax silently dropped — wrong port/user on discovered hosts
- **Location:** `crates/sigrun-core/src/ssh_config.rs:38-41`
- **What:** OpenSSH accepts `Port=2222` / `User=me`. The parser reads the whole `Port=2222` token as the key, matches nothing, drops the option — a discovered host silently falls back to port 22 / the local user, so the connection goes to the wrong endpoint.
- **Risk of fixing:** low — split the first token on `=` before matching.
- **Risk of leaving:** any user whose config uses `Key=Value` gets silently wrong discovered connections.
- **Effort:** S — normalize key/value on the first `=`.

### 12. Unknown `theme` in settings.toml bricks the UI behind a bogus "Locked" gate
- **Location:** `apps/desktop/src/App.tsx:36-46` (`refresh`), `apps/desktop/src/themes/index.ts:24-30` (`applyTheme`)
- **What:** `applyTheme` indexes `PALETTES[name]` with the raw persisted string; a stale/hand-edited value makes it `undefined` and throws, the catch forces `setVault("Locked")`, and the Settings view that could fix it sits behind that gate.
- **Risk of fixing:** none — fall back to the default theme when the name is unknown.
- **Risk of leaving:** one bad string strands the user on an unlock screen that can never succeed, with no in-app recovery.
- **Effort:** S — `PALETTES[name] ?? PALETTES[DEFAULT_THEME]`; keep theme failures out of the vault-state path.
- **Confirmed live (pass 6):** reproduced in the browser harness — with an invalid theme the app sat on "Vault locked" and never opened, with zero console errors; a valid theme unlocked immediately. This raises #12 from a code-read suspicion to a demonstrated failure.

### 13. `keys.addToVaultBody` used but defined in no locale — dialog renders the raw key
- **Location:** `apps/desktop/src/views/Keys.tsx:63`; the string lives at `vault.addToVaultBody` in all seven bundles
- **What:** the add-to-vault dialog calls `t("keys.addToVaultBody")` but every locale defines only `vault.addToVaultBody`; i18next falls back to the key itself, so every user sees the literal text "keys.addToVaultBody". The completeness guard compares locales against each other, so a key missing from all seven passes.
- **Risk of fixing:** none — rename the JSON entry or the call site.
- **Risk of leaving:** an untranslated internal key shown in every language on a shipped feature.
- **Effort:** S — add `addToVaultBody` under `keys` in each locale (the text already exists at `vault.addToVaultBody`); optionally extend the guard to check call sites against en.json.
- **Confirmed live (pass 6):** the "Add to vault" dialog shows the literal string `keys.addToVaultBody` as its body (screenshot `shots/16-addtovault-dialog.png`). A call-site-vs-en.json scan of all 176 `t()` calls found this is the only missing key in the app.

### 14. Windows IPC server spins forever when pipe creation keeps failing
- **Location:** `crates/sigrun-core/src/ipc/server.rs:80-99`
- **What:** `serve_windows` loops on `ServerOptions::create`; on any error it hits `continue` with no sleep and no shutdown check. A persistent failure spins at 100% CPU for the host's life, and `Shutdown` is never honoured because `select!` is only reached after a successful create. (Found by both bug-hunt and performance passes.)
- **Risk of fixing:** low — a small sleep and a shutdown check in the error arm.
- **Risk of leaving:** a wedged Windows host burns a core forever and can never be stopped via IPC.
- **Effort:** S — `Err(_) => { if shutdown.is_cancelled() { break } sleep(100ms).await }`.

### 15. Agent socket gets mode 0600; the host IPC socket gets none
- **Location:** `crates/sigrun-core/src/ipc/server.rs:44` (`UnixListener::bind`) vs `crates/sigrun-core/src/agent/mod.rs:36` (`set_permissions(addr, 0o600)`)
- **What:** the agent socket is chmod 0600 after bind; the host IPC socket's mode is whatever the umask leaves (commonly 0755). The 0700 data dir is the real barrier by default, but `sigrun host --socket <path>` lets the socket live anywhere — e.g. `/tmp`, where a 0755 socket is connectable by every local user, who can then call `PasswordGet`/`KeysExport`/`Shutdown`.
- **Risk of fixing:** none; mirrors the agent path.
- **Risk of leaving:** on a misconfigured data dir, other local users can reach the password-serving socket.
- **Effort:** S — one `set_permissions(addr, 0o600)` after bind.

### 16. Disk-key paths with spaces break `connect` on Windows
- **Location:** `crates/sigrun-core/src/terminal.rs:199-213` (`shell_escape` is POSIX), `:553-578` (Windows arm feeds `render_command` to `cmd.exe /k`)
- **What:** the Windows launch line is built with POSIX single-quote escaping, but `cmd.exe` does not strip single quotes. A disk key under `C:\Users\My User\.ssh\…` reaches ssh as broken argv pieces, so a valid connection fails to launch on Windows only.
- **Risk of fixing:** moderate — cmd argv quoting is fiddly and needs a Windows box to verify.
- **Risk of leaving:** Windows users whose profile path has a space (common "Firstname Lastname") cannot connect with disk keys.
- **Effort:** M — a cmd-aware quoting branch in the Windows arm plus a test.

### 17. `SessionStarted` accepts any pid; the sweeper signals its whole process group
- **Location:** `crates/sigrun-core/src/host.rs:512-515` (`Request::SessionStarted`), `:327-334` (`end_orphaned_session` → `kill(-pid, SIGHUP)`)
- **What:** any IPC client can register an arbitrary pid for any connection id; when the sweeper judges it orphaned it SIGHUPs the process group `-pid`. A same-user client can name a victim process-group leader it owns and get the host to hang it up. Same-user, so impact is self-DoS, but nothing verifies the report came from Sigrun's own launch line.
- **Risk of fixing:** a sanity check (reject 0/1/own pid; verify before signalling) could false-positive on exotic terminals.
- **Risk of leaving:** a same-user process can trick the host into SIGHUP-ing an unrelated process group after pid reuse.
- **Effort:** S — a few guards in `report_session`/`end_orphaned_session`.

### 18. Export destination TOCTOU: zip checked-then-truncated; folder export overwrites `keys/*.pub`
- **Location:** `crates/sigrun-core/src/host.rs:209-218`, `crates/sigrun-core/src/bundle.rs:121-135`, `:108-118`
- **What:** for a zip, `dest.exists()` is checked then `File::create` truncates — a swapped-in path or symlink is clobbered. For a folder export only `export.json`/`README.txt` are refused; a pre-existing `keys/` dir gets same-named `.pub` files silently overwritten.
- **Risk of fixing:** low — `create_new(true)` semantics in both writers.
- **Risk of leaving:** narrow (needs a race or pre-seeded dir); worst case is silent loss of an unrelated file the user pointed the picker at.
- **Effort:** S — `OpenOptions::create_new(true)` for the zip; refuse a destination that already contains `keys/`.

### 19. Failed unlock attempts are silent — no log, no UI signal, no counter
- **Location:** `crates/sigrun-core/src/host.rs:398-407`
- **What:** a wrong passphrase produces an error to the caller and nothing else. A user has no way to learn that 10,000 attempts happened overnight. Pairs with #2 (the fix should log, but never the passphrase).
- **Risk of fixing:** logging must never capture the attempted passphrase — only count/timestamp.
- **Risk of leaving:** attacks are invisible; the user can't tell "I mistyped" from "something hammered my vault all night".
- **Effort:** S — one redacted log line per failure; optionally expose a failure count in `Status`.

---

## Medium

### 20. Blocking sync I/O on tokio worker threads throughout the host
- **Location:** `crates/sigrun-core/src/store.rs:27-58`, `host.rs:299` (`/proc` read), `host.rs:1304` (ssh config read), `keys/discovery.rs:114` — all called inline from async handlers and the 2 s tick
- **What:** `Store` uses `std::fs` throughout and every async handler calls it inline; only the Argon2 unlock uses `spawn_blocking`. One slow filesystem stalls every IPC handler, the agent accept loop, and the supervisor tick.
- **Risk of fixing:** moving to `spawn_blocking`/`tokio::fs` touches many call sites and can change lock ordering around the store flock.
- **Risk of leaving:** the whole app feels frozen rather than one request being slow, on a network home / spinning disk / Windows AV.
- **Effort:** L — wrap the store-touching arms (or `Store`) in `spawn_blocking`; at minimum move `discover_keys` and `/proc` reads off the async threads.

### 21. `KeysList` rewrites `data.toml` once per discovered key
- **Location:** `crates/sigrun-core/src/host.rs:1302-1348` (`sync_disk_keys`), called from `keys_list` at `:619`
- **What:** every `KeysList` rescans `~/.ssh`, re-parses every private key and ssh config, then `upsert_key` per key — each upsert takes the flock, re-reads and atomically rewrites the whole file. N keys = N full read+write cycles per list, fired on every view refresh. (Performance + code-quality both flagged it; the fix is the same.)
- **Risk of fixing:** a batched `update_data` changes failure mode from "some persisted" to all-or-nothing.
- **Risk of leaving:** ~20 keys = 20 atomic TOML rewrites plus 20 private-key parses on every view switch.
- **Effort:** M — batch the upserts into one `update_data`; skip the rescan when `~/.ssh` mtime is unchanged. Splitting `KeysList` (read) from `KeysRescan` (discovery+upsert) is the cleaner shape.

### 22. Tray menu fully rebuilt on every window Focused event
- **Location:** `apps/desktop/src-tauri/src/tray.rs:465-473`, `:280-305`
- **What:** each focus gain runs `is_unlocked` + `load_settings` + a full `ConnectionsList` + a complete native menu rebuild, even when nothing changed.
- **Risk of fixing:** a dirty-flag scheme can miss an update path and leave the tray stale.
- **Risk of leaving:** alt-tabbing with a large list burns repeated disk reads and native menu rebuilds.
- **Effort:** M — gate the rebuild on a state/version token bumped by the mutating commands.

### 23. Every key mutation re-decrypts and re-parses the whole vault key set
- **Location:** `crates/sigrun-core/src/host.rs:606-616` (`reload_agent`), `agent/session.rs:199-224`
- **What:** add/import/remove one key throws away the entire agent key set and rebuilds it: every vault PEM re-parsed, every encrypted key re-decrypted, plus an O(vault × metadata) `find` per entry.
- **Risk of fixing:** incremental add/remove must keep the agent consistent with the vault on partial failure.
- **Risk of leaving:** with several passphrase-protected keys, each add/import stalls the handler for hundreds of ms of redundant decryption.
- **Effort:** S — `AgentState::insert`/`remove_key`; fall back to a full rebuild only on unlock.

### 24. `Response::Password` and vault secret clones travel as plain `String`s with no zeroization
- **Location:** `crates/sigrun-core/src/host.rs:1058`, `:786`, `:1220-1221`; `crates/sigrun-core/src/askpass.rs:25-37`
- **What:** `Debug` is already redacted, so this is memory hygiene only: once a secret leaves `Secret`'s `ZeroizeOnDrop` umbrella it travels as a plain `String` through the IPC buffer, askpass return, and provision handoff. Copies freed without zeroization linger in the heap.
- **Risk of fixing:** `Zeroizing<String>` changes call-site types across the CLI, IPC client and Tauri commands.
- **Risk of leaving:** a memory dump (crash report, swap, same-user ptrace) can recover secrets shortly after use — marginal on top of the same-user socket access already possible (#1).
- **Effort:** M — thread `Zeroizing<String>` through `Response::Password` and the secret handoffs; drop the plain clones.

### 25. Provision hardening echoes remote stderr into UI error messages
- **Location:** `crates/sigrun-core/src/provision.rs:259-265`, `:170-173`
- **What:** remote stderr (attacker-influenced if the provisioned host is hostile — plausible mid-provision) is embedded in `Error::Config("sshd hardening failed: {reason}")` and rendered in the UI. Capped at 8 KiB; React escapes HTML, but a misleading message ("rollback complete") can be injected into a dialog.
- **Risk of fixing:** none; fixed exit-code messages already exist as fallbacks.
- **Risk of leaving:** a hostile server can put arbitrary text into a Sigrun error dialog.
- **Effort:** S — map exit codes 5-8 to fixed strings; drop `reason` from the user-facing error.

### 26. Provisioning a connection with an empty user fails with a misleading error
- **Location:** `crates/sigrun-core/src/host.rs:1266-1282`, `crates/sigrun-core/src/provision.rs:91-97`
- **What:** the form and `valid_connection` accept an empty user ("ssh uses the local user"), but provisioning passes it verbatim to `authenticate_password`, tries to log in as `""`, and returns "password authentication failed" — the user is told the password is wrong when the real problem is the missing login name.
- **Risk of fixing:** low — reject early with a clear message, or default to the local username.
- **Risk of leaving:** a valid configuration dead-ends in a confusing auth error at the end of a long provision flow.
- **Effort:** S — an `InvalidInput("provisioning needs a login user")` guard before connecting.

### 27. Language switch saves settings twice and can clobber a concurrent change
- **Location:** `apps/desktop/src/i18n/index.ts:49-60` (`setLanguage` reads and re-saves the whole object), `apps/desktop/src/views/Settings.tsx:116` (`onChange` also saves)
- **What:** changing language fires `setLanguage` (get → save full object) *and* `update({language})` (saves full object again); the two writes race, and a stale read can restore an old value for another field on disk while the UI shows the new one.
- **Risk of fixing:** low — have `setLanguage` only change i18n and let the view own persistence.
- **Risk of leaving:** a fast consecutive settings change can be silently reverted on disk.
- **Effort:** S — split `setLanguage` into a pure `changeLanguage` plus an explicit save from Settings.

### 28. `host.rs` is a 3,154-line module holding state, dispatch, handlers and platform code
- **Location:** `crates/sigrun-core/src/host.rs:1-1366`
- **What:** one file holds `HostState`, the mutex helpers, pid-liveness/orphan `libc` code, the auto-lock watcher, export/import orchestration, the 25-arm dispatcher and a dozen handlers — four or five distinct jobs. Handlers are findable only by scrolling; unrelated diffs collide.
- **Risk of fixing:** pure reorganization; risk is mechanical and caught by the gate.
- **Risk of leaving:** reviewers can't keep the invariants (vault-lock ordering, identity checks) in view at once.
- **Effort:** M — split into `host/{state,dispatch,handlers,sessions}.rs`, no behaviour change.

### 29. `get_settings`/`save_settings` bypass the IPC protocol, smuggling settings as a JSON string in `Response::Command`
- **Location:** `apps/desktop/src-tauri/src/commands.rs:187-196`; consumer `apps/desktop/src/api.ts:97`
- **What:** the Tauri layer reads/writes the store directly and returns settings serialized into the `Command` variant — meant for an ssh command line — so the frontend must `JSON.parse` and blind-cast. A type lie at the trust boundary; the typed `Settings` channel shape already exists but isn't used.
- **Risk of fixing:** adding `SettingsGet`/`SettingsSave` Request/Response variants touches the CLI-visible protocol (old host/new client mismatch), needs a version thought.
- **Risk of leaving:** any future `Response::Command` consumer can be handed settings JSON.
- **Effort:** S — add two Request variants and one Response variant; the handlers are three lines each.

### 30. Dead public API in core, one of which is a footgun
- **Location:** `crates/sigrun-core/src/vault/store.rs:176` (`UnlockedVault::lock`), `:62-72`; `agent/session.rs:55-62`; `trust.rs:16-25`
- **What:** these are `pub` but have no non-test callers. `UnlockedVault::lock` drops the key without clearing the agent or active set — a future caller using it directly would introduce a bug.
- **Risk of fixing:** none beyond deleting lines.
- **Risk of leaving:** an invitation to bypass the host's actual state handling.
- **Effort:** S — delete; if `lock` is kept for the future, mark it `#[cfg(test)]` or private.

### 31. `AgentState` panics on a poisoned lock while `HostState` recovers from one
- **Location:** `crates/sigrun-core/src/agent/session.rs:39,44,50,56,60,79,93` (`.expect("agent lock poisoned")`) vs `host.rs:83-87` (deliberate recovery)
- **What:** sibling modules made opposite calls. A single panic inside the agent session poisons `request_identities` for every later ssh invocation until the host restarts — worse availability than the host chose for the same class of bug.
- **Risk of fixing:** recovering hides the original panic further from its cause.
- **Risk of leaving:** one bad sign request turns every subsequent ssh auth into a panic.
- **Effort:** S — mirror the host's `unwrap_or_else(|p| p.into_inner())`.

### 32. `Settings.agent_enabled` is dead — persisted, typed, never read
- **Location:** `crates/sigrun-core/src/model.rs:273,287`; `apps/desktop/src/api.ts:46`
- **What:** nothing reads `agent_enabled`; the agent is always served. A persisted no-op a user can set and reasonably expect to do something.
- **Risk of fixing:** removing the field is safe — `#[serde(default)]` makes old `settings.toml` load fine.
- **Risk of leaving:** a toggle that does nothing; widens the wire surface for free.
- **Effort:** S — delete the field everywhere.

### 33. Per-row O(rows × keys/creds) lookups and full-list re-renders in Connections
- **Location:** `apps/desktop/src/views/Connections.tsx:574-601`; `Credentials.tsx:128`
- **What:** each rendered row linearly scans credentials/keys, no row is memoized, and a session dot toggling fires a `connections-changed` event that re-renders the whole list.
- **Risk of fixing:** `React.memo` + prebuilt `Map`s is behaviour-neutral.
- **Risk of leaving:** fine at tens of connections; quadratic-ish work at a few hundred on every session start/stop.
- **Effort:** S — build `credById`/`keyById` maps once per refresh, memoize the row, pass stable callbacks.

### 34. `sync_disk_keys` writes on a read: `KeysList` mutates the store on every call
- **Location:** `crates/sigrun-core/src/host.rs:618-631` calling `:1302-1352`
- **What:** a GET-shaped request does a full discovery + a store write per key, on every `KeysList` — including the desktop poll and the tray's connection list on every focus event. A read with write-shaped cost and lock contention.
- **Risk of fixing:** caching discovery needs an invalidation story (rescan on unlock + manual refresh).
- **Risk of leaving:** every list call rewrites `data.toml`; wears the file and contends the store lock.
- **Effort:** M — split `KeysList` (read store) from `KeysRescan` (discovery + upsert); rescan on unlock only. (Pairs with #21 — same root.)

### 35. Validation/grouping logic duplicated across three layers, held together by comments
- **Location:** `apps/desktop/src/views/Connections.tsx:83-95` mirrors `terminal.rs:39`; group-tree logic `Connections.tsx:14-52` mirrors `apps/desktop/src-tauri/src/tray.rs:99-123`
- **What:** the host/user character set and the group-path tree are each implemented twice in different languages, kept in sync only by a comment. The validation mirror is test-pinned; the tree grouping is not cross-checked.
- **Risk of fixing:** extracting a shared rule is more machinery than the duplication costs today.
- **Risk of leaving:** a rules change in one layer leaves the other silently disagreeing.
- **Effort:** S — a shared test fixture pinning tray and frontend tree-building to the same inputs/outputs.

### 36. CLI `connections edit --key` stores the argument verbatim, never resolves the name
- **Location:** `crates/sigrun-cli/src/cli.rs:516-518`
- **What:** `keys remove` resolves `--id-or-name`, but `connections edit --key <value>` stores the raw string as `key_id`. A user typing the key *name* (which `keys list` shows) stores a dangling reference reported only as `not_found` at connect time.
- **Risk of fixing:** resolution needs a `KeysList` round trip; resolve id-first so stored names that are ids are unaffected.
- **Risk of leaving:** an easy, silent way to corrupt a connection's key reference; inconsistent with the CLI's own conventions.
- **Effort:** S — resolve `--key` through `resolve_key` before `ConnectionsUpdate`.

### 37. `probe_reachable`'s inconclusive warning goes to stderr that desktop users never see
- **Location:** `crates/sigrun-core/src/host.rs:1119-1123`
- **What:** when the probe fails without proving unreachability, the host logs "probe inconclusive" via `eprintln!` — dropped in the desktop app (no console), visible only to `sigrun host` users. Intended diagnostics vanish on the primary platform.
- **Risk of fixing:** surfacing it as a response field changes the `ConnectionsConnect` reply shape.
- **Risk of leaving:** if the probe misbehaves there is no user-visible evidence.
- **Effort:** S — route through the `log` facade, or drop the message as dead code.

### 38. Keys/Credentials views throw away the host's specific error messages
- **Location:** `apps/desktop/src/views/Keys.tsx:31-41`, `Credentials.tsx:113-119`
- **What:** the host sends specific, user-actionable messages ("cannot export inside the SSH directory", "key could not be loaded"); both views store a bare `setError(true)` and render "Something went wrong". Connections already surfaces `e.message`.
- **Risk of fixing:** none — the message channel already exists; only rendering changes.
- **Risk of leaving:** the careful Rust-side error wording is wasted; users can't act on errors.
- **Effort:** S — store `string | null` in the error state of both views and render it.

### 39. `Credential.has_password` is client-writable on update with only a comment as defence
- **Location:** `crates/sigrun-core/src/host.rs:984-1000`; `ipc/protocol.rs:40-47`
- **What:** the add/update paths re-impose the derived flag by remembering to assign it in each handler; the invariant "clients cannot set derived flags" is enforced per-handler, not per-type. The next handler that forgets the reset persists a flag that disagrees with the vault.
- **Risk of fixing:** splitting a `CredentialInput` (no derived fields) from `Credential` touches IPC types, the CLI constructor and the frontend type.
- **Risk of leaving:** the exact bug `host.rs:1050` comments about, introduced by a future handler.
- **Effort:** M — accept input types without derived fields; construct `Credential` host-side only.

### 40. Supervisor tick and auto-lock watcher poll/allocate when fully idle
- **Location:** `apps/desktop/src-tauri/src/lib.rs:305-341`; `crates/sigrun-core/src/host.rs:166-196`
- **What:** every 2 s the tick runs a `Status` probe (vault mutex + fs stat) and clones the active set even when locked-and-idle; every 60 s the auto-lock watcher re-reads `settings.toml` on an async thread just to fetch `auto_lock_minutes`. Small constant waste; defeats OS idle accounting.
- **Risk of fixing:** skipping the Status probe when idle must not break CLI-side lock/unlock being picked up.
- **Risk of leaving:** negligible on a desktop, but pure waste in the common locked-idle case.
- **Effort:** S — gate the tick on a dirty flag; cache `auto_lock_minutes` refreshed by the save path.

### 41. Connection-error modal blocks all navigation until dismissed
- **Location:** `apps/desktop/src/views/Connections.tsx` — the failed-connect/trust modal is a `div.fixed.inset-0` overlay (no Escape / outside-click dismissal).
- **What:** after a failed connect an accessible modal (`role="dialog"`, `aria-label` set, Cancel / "Connect anyway") appears — correct — but its overlay intercepts pointer events for the whole window, so the nav tabs (Keys/Credentials/Settings/Help) are unreachable by mouse until the user picks an action. Other dialogs in the app close on Escape; this one does not. Found in the pass-6 browser walk (screenshot `shots/15-connect-error-modal.png`).
- **Risk of fixing:** low; keep the explicit trust decision — Escape/outside-click must map to Cancel only, never to Connect-anyway.
- **Risk of leaving:** minor — one extra click to return to the rest of the app after a failed connect. Keyboard users can already tab to Cancel.
- **Effort:** S.

---

## Docs (all S effort; fix after the code findings you choose)

### 42. README promises Ctrl+N / Ctrl+, on Windows/Linux — they don't exist there
- **Location:** `README.md:120-121`; the menu is `#[cfg(target_os = "macos")]` only (`lib.rs:151-175`). **Fix:** qualify the sentence, or implement the shortcuts cross-platform.

### 43. Six shipped locales are machine-translated and unreviewed
- **Location:** `apps/desktop/src/i18n/{da,de,fr,es,sv,nb}.json`; only `en.json` is authored. Spot-checks show "vault" rendered as the architectural/cash word (da `pengeskab`, sv `valv`, nb `hvelv`). **Fix:** a README disclaimer line + an issue for native review; full review is L.

### 44. AGENTS.md i18n rule is stale — says keys live in "both en.json and da.json"
- **Location:** AGENTS.md "Code rules" bullet 4; the guard actually enforces all seven locales. A contributor following it breaks the build. **Fix:** one-line edit to name all seven locales.

### 45. "Left-click tray icon shows the window when locked" is false on Linux
- **Location:** `README.md:130-132` and `help.groups.tray` in all locales; Linux delivers no tray click events (`tray.rs:129-131,300-302`), so the locked Linux tray leads with an Unlock item instead. **Fix:** qualify the wording in README and the seven bundles.

### 46. In-app Help omits the keyboard shortcuts entirely
- **Location:** `apps/desktop/src/views/Help.tsx` (no shortcuts section) while README documents Cmd/Ctrl+F, Cmd/Ctrl+L, Cmd+N, Cmd+,. **Fix:** add a `help.shortcuts` section in all seven bundles.

### 47. README Install section's `cargo build --release` lands the wrong binary on macOS
- **Location:** `README.md:61-65`; a bare build collides the desktop `Sigrun` onto `target/release/sigrun` on macOS's case-insensitive FS — a caveat the README itself admits 250 lines later. **Fix:** use `cargo build -p sigrun-cli --release` in Install.

### 48. Help "CLI" section lists 8 commands and omits provision/keys/status
- **Location:** `apps/desktop/src/views/Help.tsx:5-14`; omits `connections provision`, `keys generate`, `keys import`, `vault init`/`lock`, `status` — even though the same view's quickStart recommends "Install SSH key…". **Fix:** add the missing rows in all seven bundles.

### 49. README "It works the same on macOS, Linux, and Windows" overstates parity
- **Location:** `README.md:12-13`; the README's own limitations list says otherwise (no askpass on Windows, no session dot on Windows, no tabs off macOS). **Fix:** soften the sentence.

---

## Features — quick wins (ranked by value-to-effort)

These are candidate additions, not defects. Each is achievable without new
architecture. Full sketches in `07-quick-wins.md`.

| # | Feature | Area | Effort | Hook |
|---|---------|------|--------|------|
| F1 | Copy SSH command button on a connection row | frontend | S | `connect` already returns argv via `printOnly`; needs a clipboard path + grant |
| F2 | CLI `sigrun credentials password` (rotate) | cli | S | IPC `CredentialsSetPassword` exists; README documents the gap |
| F3 | CLI `sigrun keys export <id> <path>` | cli | S | IPC `KeysExport` + guardrails exist; CLI never wires it |
| F4 | CLI `sigrun credentials edit` | cli | S | `CredentialsUpdate` exists; mirror `connections edit` |
| F5 | Copy public key / fingerprint from a key card | frontend | S | `KeyMetadata` already carries both; one handler + clipboard |
| F6 | `connections edit` by label, plus `--credential` | cli | S | siblings already resolve id-or-label; `ConnectionsUpdate` takes the row |
| F7 | CLI `sigrun shutdown` | cli | S | `Request::Shutdown` exists and is handled |
| F8 | Duplicate a connection from its row | frontend | S | Form already takes `initial`; must reset `has_password` |
| F9 | Expose connection tags in the UI | frontend | S | `tags` is in the model and CLI but rendered nowhere |
| F10 | CLI `credentials list --json` | cli | S | parity with `connections list`/`keys list` |
| F11 | CLI `status` shows live session count | cli | S | `Connection.active` exists; one extra request |
| F12 | Show resolved identity in the connection form | frontend | S | data already loaded; one muted line |
| F13 | Search matches tags and credential user | frontend | S | extend the `filterConnections` field list |
| F14 | IPC `KeysUpdate` / rename a key | core | M | no rename path exists; remove destroys a generated key |
| F15 | Keyboard navigation of the filtered list | frontend | M | completes Cmd+F; needs selection state + a11y |
| F16 | CLI `connections add --clone <id>` | cli | S | scriptable duplicate; must not copy `has_password` |
| F17 | Vault auto-lock countdown in Settings | frontend | M | honest countdown needs a Status field; static hint is S |
| F18 | CLI `connections list --group` matches prefixes | cli | S | mirror the UI tree's nesting |
| F19 | Tray "Copy SSH command" per connection | tauri | M | model exists; clipboard in the tray process is the open question |

## Features — larger / missing (ranked by expected user value)

These need design. Full detail in `08-missing-features.md`. Items already in
README "Current limitations" are marked *(documented)*.

| # | Feature | Area | Effort | Note |
|---|---------|------|--------|------|
| G1 | Vault passphrase change | core | M | no re-key path; a leaked passphrase is permanent. Table-stakes for a vault |
| G2 | Connection metadata: notes, custom SSH options, ProxyJump | core | L | free-text options re-open the injection boundary; needs structured fields |
| G3 | Full backup / disaster recovery of the vault | core | S | a "Back up vault…" copies the sealed envelope; users currently get no help |
| G4 | Host-key (known_hosts) management UI | frontend | M | no list/forget; a rotated server key bricks the connection *(partially documented)* |
| G5 | Lock/unlock observability + notifications + a redacted log | tauri | M | auto-lock is silent; near-zero logging makes bug reports empty |
| G6 | ssh_config option passthrough for discovered hosts | core | L | ProxyJump/LocalForward/keepalive dropped; discovered hosts behave differently |
| G7 | Key lifecycle: change key passphrase, rotation metadata | core | M | rotation is generate + re-provision by hand, one connection at a time |
| G8 | Bulk operations / multi-select in the UI | frontend | M | 50 imported connections = 50 individual confirm dialogs |
| G9 | SFTP / non-shell ssh invocation | core | L (S for a launch template) | connect is interactive-shell only |
| G10 | Session list + "close all" / reconnect | core | M | live sessions are a dot, not a list |
| G11 | Agent lock/unlock granularity, confirm-on-use | core | L | any same-user process gets free signatures while unlocked |
| G12 | Multi-machine conflict story / change detection | docs | S | DIY-syncing the data dir silently overwrites; document the one-writer rule |
| G13 | Recovery: `vault prune`, forgot-passphrase flow | core | S (L for recovery key) | forgotten passphrase is total; orphaned secrets accumulate |
| G14 | Adopt a discovered ssh_config host as managed | core | S | data already parsed; onboarding friction for the messiest setups |
| G15 | Test connection without opening a terminal | core | M | TCP probe proves nothing about auth; reuse provision's russh path |
| G16 | Headless host auto-start (systemd/launchd/service) | cli | M | `sigrun host` must be started by hand after every reboot *(documented)* |
| G17 | Credential rotation reminders | frontend | M | depends on F2; opt-in age threshold |
| G18 | Configurable KDF params at vault init | core | M | params are pinned; offer "paranoid/standard" presets only |
| G19 | AppleScript terminals don't get the agent | core | M | vault keys silently don't work on Terminal.app/iTerm/Ghostty-tab *(documented)* |

---

## What I did not check / known limits

- **Pass 6 (interactive UI) is done.** It ran the real React app in a stubbed-Tauri
  Chromium harness (Playwright), walked every tab and the lock/unlock/error paths, and
  captured screenshots (`./shots/`). It **confirmed #12 and #13 live** and added #41
  (connection-error modal blocks navigation). It did not exercise native file pickers,
  real terminal launch, the tray menu, or OS window chrome — those need the real Tauri
  shell. See `06-interactive-ui.md`.
- No profiler was run; performance findings (#20-#24, #33, #40) are from code
  reading, not measurement. The Argon2id cost is deliberate and excluded.
- Windows behavior was reviewed from code only; no live Windows host was tested
  (#5's named-pipe ACL, #14's pipe loop, #16's quoting all assume the documented
  Win32 semantics).
- The seven non-English locales were spot-checked, not fully reviewed (#43).
