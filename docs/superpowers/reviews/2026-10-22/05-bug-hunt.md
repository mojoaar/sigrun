# Pass 5 — Bug hunt

### Stale watched pid can clear a new live session (and SIGHUP a recycled process group)
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:506-509 (`Request::ConnectionEnded` clears only `active`), 136-153 (`sweep_dead_sessions`), 327-334 (`end_orphaned_session`)
- **What:** `ConnectionEnded` calls `clear_active` but never `forget_session`, although `forget_session`'s own doc says it goes "alongside clear_active". The dead pid stays in `sessions`; on a quick reconnect `mark_active` runs at launch, and if the 2 s sweep tick fires before the new `session-started` report arrives, `retain` sees the old dead pid and calls `clear_active(id)` — the live dot for the running session goes dark and nothing re-sets it. The same path also calls `end_orphaned_session`, which sends `SIGHUP` to the process group of whatever now owns a recycled pid, with no check that the pid is still our shell.
- **Risk of fixing:** low — one extra map removal plus a comment; sweep tests already cover the happy path.
- **Risk of leaving:** the live-session dot silently disappears for a running ssh session whenever a reconnect follows an ended one inside a poll window; a recycled pid can take an unrelated SIGHUP.
- **Effort:** S — call `forget_session` (or clear only when the pid matches) in the `ConnectionEnded` handler and skip the SIGHUP when the pid is already dead.
- **Suggested fix:** in `Request::ConnectionEnded`, also `state.forget_session(&id)`; in `sweep_dead_sessions`, only signal when the pid is alive but orphaned.

### Windows IPC server spins forever when pipe creation keeps failing
- **Area:** core
- **Location:** crates/sigrun-core/src/ipc/server.rs:80-99
- **What:** `serve_windows` loops on `ServerOptions::create`, and on any error hits `Err(_) => continue` with no sleep and no shutdown check. A persistent failure (access denied, quota) makes the accept loop spin at 100% CPU for the life of the host, and `Shutdown` is never honoured because `tokio::select!` is only reached after a successful create.
- **Risk of fixing:** low — add a small sleep and a shutdown check in the error arm; behaviour on the happy path unchanged.
- **Risk of leaving:** a wedged Windows host burns a core forever and can never be stopped via IPC.
- **Effort:** S — a few lines in the error arm.
- **Suggested fix:** `Err(_) => { if state.shutdown.is_cancelled() { break } ; tokio::time::sleep(100 ms).await }`.

### `keys.addToVaultBody` used but defined in no locale — dialog renders the raw key
- **Area:** frontend
- **Location:** apps/desktop/src/views/Keys.tsx:63; the string lives at `vault.addToVaultBody` in all seven bundles (e.g. apps/desktop/src/i18n/en.json:18)
- **What:** the add-to-vault dialog body calls `t("keys.addToVaultBody")`, but every locale only defines `vault.addToVaultBody`. i18next falls back to the key itself, so users see the literal text "keys.addToVaultBody". The recursive completeness guard compares locales against each other, so a key missing from all seven passes.
- **Risk of fixing:** none — rename the JSON section entry or the call site.
- **Risk of leaving:** every user opening the "add disk key to vault" dialog sees an untranslated internal key in every language.
- **Effort:** S — move the string to `keys.addToVaultBody` in all bundles (or repoint the call).
- **Suggested fix:** add `addToVaultBody` under `keys` in each locale file; optionally extend the guard to check `t("…")` call sites against en.json.

### Unknown `theme` in settings.toml bricks the UI behind a bogus "Locked" gate
- **Area:** frontend
- **Location:** apps/desktop/src/App.tsx:36-46 (`refresh`), apps/desktop/src/themes/index.ts:24-30 (`applyTheme`)
- **What:** `applyTheme` indexes `PALETTES[name]` with the raw persisted string; a hand-edited or stale settings.toml (e.g. `theme = "midnight"`) makes it `undefined` and throws a TypeError. The catch in `refresh` then forces `setVault("Locked")`, so the app shows the unlock gate forever — and the Settings view that could fix the value sits behind that gate.
- **Risk of fixing:** none — fall back to DEFAULT_THEME when the name is unknown.
- **Risk of leaving:** one bad string in settings.toml strands the user on an unlock screen that can never succeed, with no in-app recovery.
- **Effort:** S — a `PALETTES[name] ?? PALETTES[DEFAULT_THEME]` guard (and ideally don't map theme errors to the vault gate).
- **Suggested fix:** validate the theme name in `applyTheme` and default; keep theme failures out of the vault-state path.

### Disk-key paths with spaces break `connect` on Windows
- **Area:** core
- **Location:** crates/sigrun-core/src/terminal.rs:199-213 (`shell_escape` is POSIX), 553-578 (Windows arm feeds `render_command` output to `cmd.exe /k`)
- **What:** the Windows launch line is built with POSIX single-quote escaping, but `cmd.exe` does not strip single quotes. A disk key under e.g. `C:\Users\My User\.ssh\id_ed25519` reaches ssh as several broken argv pieces, so a perfectly valid connection fails to launch on Windows only.
- **Risk of fixing:** moderate — Windows argv quoting for `cmd.exe` is fiddly and needs a Windows test box to verify.
- **Risk of leaving:** Windows users whose profile path contains a space (common with "Firstname Lastname" accounts) cannot connect with disk keys.
- **Effort:** M — a cmd-aware quoting branch in the Windows arm plus a test.
- **Suggested fix:** in the `Platform::Windows` arm, quote argv elements with double quotes/`\^` escaping instead of `shell_escape`.

### `~/.ssh/config` `Key=Value` syntax is silently dropped — wrong port/user on discovered hosts
- **Area:** core
- **Location:** crates/sigrun-core/src/ssh_config.rs:38-41 (`tokens.next()` as key, rest joined as value)
- **What:** OpenSSH accepts `Port=2222` / `User=me` (equals syntax). The parser reads the whole `Port=2222` token as the key, matches nothing, and drops the option, so a discovered host silently falls back to port 22 or the local user — the connection then goes to the wrong endpoint or prompts the wrong user.
- **Risk of fixing:** low — split the first token on `=` before matching; existing whitespace parsing is unaffected.
- **Risk of leaving:** any user whose config uses `Key=Value` gets silently wrong discovered connections (wrong port is the nasty one).
- **Effort:** S — normalize `key`/`value` by splitting on the first `=` when present.
- **Suggested fix:** `let (key, value) = match line.split_once('=') { Some((k, v)) => (k.trim().to_lowercase(), v.trim().to_string()), None => … }`.

### Vault `put`/`remove` diverge memory from disk when `persist` fails
- **Area:** core
- **Location:** crates/sigrun-core/src/vault/store.rs:160-170
- **What:** `put` inserts into the in-memory payload and *then* persists; if `persist` fails (disk full, permission), the secret lives in memory but not on disk and is silently lost at lock — while the caller got an error, later operations in the same session still see the phantom entry. `remove` has the mirror problem: the entry is gone in memory, and if persist fails the secret resurrects on the next unlock.
- **Risk of fixing:** low — persist first from a cloned payload, or roll the in-memory change back on failure.
- **Risk of leaving:** after a transient disk error, the session shows state the disk will not reproduce (lost new keys/passwords, resurrected deleted ones).
- **Effort:** S — on persist error, revert the payload mutation before returning the error.
- **Suggested fix:** snapshot/restore the payload around `persist()` in `put` and `remove`.

### Unbounded `read_until` on the IPC socket lets a local client grow host memory without limit
- **Area:** core
- **Location:** crates/sigrun-core/src/ipc/server.rs:111-117
- **What:** `handle_conn` reads with `read_until(b'\n', &mut line)` and only enforces `MAX_LINE` after the line is complete, inside `decode`. A client that streams bytes without a newline grows the buffer forever — a local denial of service against the host (the socket sits in a 0700 dir, so it is the same user, but a crashed/misbehaving client can still do it).
- **Risk of fixing:** low — switch to `take(MAX_LINE + 1)` or check `line.len()` per read.
- **Risk of leaving:** a runaway same-user process can OOM the host; cheap to close.
- **Effort:** S — cap the buffered line during the read loop.
- **Suggested fix:** use `reader.take(MAX_LINE as u64 + 1).read_until(...)` and error when the cap is hit.

### Export destination TOCTOU: zip path checked, then truncated; folder export overwrites pre-existing `keys/*.pub`
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:209-218 (existence check), crates/sigrun-core/src/bundle.rs:121-135 (`File::create` truncates), 108-118 (`write_folder` overwrites)
- **What:** for a zip, `dest.exists()` is checked, then `File::create` opens with truncation — a path swapped in between (or a symlink) is clobbered. For a folder export only `export.json`/`README.txt` are refused; a pre-existing `keys/` directory gets its same-named `.pub` files silently overwritten.
- **Risk of fixing:** low — `create_new(true)` for the zip, and refuse a non-empty `keys/` dir (or per-file create_new) for the folder.
- **Risk of leaving:** narrow: needs a race or a pre-seeded directory; worst case is silent loss of an unrelated file the user pointed the picker at.
- **Effort:** S — swap to `create_new` semantics in both writers.
- **Suggested fix:** `OpenOptions::create_new(true)` for the zip; in `write_folder`, error if the destination directory already contains `keys/`.

### Language switch saves settings twice and can clobber a concurrent change
- **Area:** frontend
- **Location:** apps/desktop/src/i18n/index.ts:49-60 (`setLanguage` reads and re-saves the whole settings object), apps/desktop/src/views/Settings.tsx:116 (`onChange` also calls `update({ language })`)
- **What:** changing the language fires `setLanguage`, which does `get_settings` → `save_settings({...read, language})`, *and* `update({ language })`, which saves the full current object again. The two writes race; if the `setLanguage` read lands before another in-flight `update` (theme, terminal) wrote, its later save restores the stale value for that field on disk while the UI shows the new one.
- **Risk of fixing:** low — have `setLanguage` only change i18n and let the view own persistence, or make it merge onto the latest state.
- **Risk of leaving:** a fast consecutive settings change can be silently reverted on disk; confusing but self-heals on next save.
- **Effort:** S — drop the persistence from `setLanguage` when a caller already saves.
- **Suggested fix:** split `setLanguage` into a pure `changeLanguage` plus an explicit save from Settings.

### Provisioning a connection with an empty user always fails with a misleading error
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:1266-1282, crates/sigrun-core/src/provision.rs:91-97
- **What:** the form and `valid_connection` accept an empty user ("ssh uses the local user"), but provisioning passes `auth.user` verbatim to `authenticate_password`, so it tries to log in as `""` and returns "password authentication failed". The user is told the password is wrong when the real problem is the missing login name.
- **Risk of fixing:** low — reject early with a clear message, or default to the local username.
- **Risk of leaving:** a valid configuration dead-ends in a confusing auth error at the end of a long provision flow.
- **Effort:** S — an `InvalidInput("provisioning needs a login user")` guard before connecting.
- **Suggested fix:** in `connections_provision`, refuse when `auth.user` is empty.

### Attacker-controlled KDF parameters in a tampered vault file can exhaust memory/time at unlock
- **Area:** core
- **Location:** crates/sigrun-core/src/vault/crypto.rs:52-65 (`derive_key` trusts envelope `m_cost_kib`/`t_cost`/`p_cost`), crates/sigrun-core/src/vault/store.rs:108-117
- **What:** `KdfParams` are read from the vault JSON and passed to `argon2::Params::new` unchecked; values below the argon2 crate's ceiling (up to ~4 GiB memory, huge iteration counts) are accepted, so a tampered vault file turns one unlock attempt into a multi-GB allocation or a near-infinite derivation. Requires write access to the data dir, so it is self-DoS territory, but the failure mode is a hang, not an error.
- **Risk of fixing:** low — clamp to the pinned parameters (with forward-compat headroom) before deriving.
- **Risk of leaving:** a corrupted/tampered envelope hangs the app on unlock instead of reporting corruption.
- **Effort:** S — validate `m/t/p` against sane bounds in `derive_key` and error as `VaultCorrupt`.
- **Suggested fix:** reject params that don't match the pinned Argon2id profile (m=65536, t=3, p=4) until a migration exists.
