# Pass 3 — Security

### Host-key pre-flight fails open when ssh-keyscan is unavailable or blocked
- **Area:** tauri
- **Location:** apps/desktop/src-tauri/src/commands.rs:363-373 (`check_trust`), :294-315 (`probe_host_key`)
- **What:** The desktop TOFU gate skips the check entirely (`return Ok(())`) when `ssh-keyscan` is missing, times out, or errors. An attacker who can drop the keyscan traffic (or a machine without ssh-keyscan) silently bypasses mismatch detection; the launched `ssh` only consults the user's own `~/.ssh/known_hosts`, which knows nothing of hosts recorded only in Sigrun's store (e.g. provisioned servers), so the user gets a bare interactive "are you sure" prompt and likely types yes. The AGENTS.md rule says trust "fails closed on an unknown or mismatched key".
- **Risk of fixing:** a connect on a machine without ssh-keyscan now refuses; needs a UI path to record keys first.
- **Risk of leaving:** a MITM who suppresses the 5-second keyscan downgrades every Sigrun-known-host connect to a plain ssh prompt the user is trained to accept.
- **Effort:** S — make the probe failure a `CommandError` unless an explicit `force` is passed; the UI already has a trust prompt flow.
- **Suggested fix:** fail closed on probe failure (or require `force: true` to bypass), keeping the existing unknown/mismatch prompts for successful scans.

### IPC server serves secrets and privileged actions to any local client with no authentication
- **Area:** ipc
- **Location:** crates/sigrun-core/src/ipc/server.rs:103-146, crates/sigrun-core/src/host.rs:468 (`PasswordGet`), :482 (`KeysExport`), :516 (`Shutdown`)
- **What:** The Unix socket is protected only by the 0700 data directory; there is no credential check (no `SO_PEERCRED`/peer-uid verification, no token). Any process running as the user — or as anyone who can reach the socket dir — can read every stored connection/credential password (`PasswordGet`), export vault private keys to an arbitrary path (`KeysExport`), shut the host down (`Shutdown`), or launch terminals (`ConnectionsConnect`) while the vault is unlocked. Unlike ssh-agent (which only signs), `PasswordGet`/`KeysExport` disclose raw secret material.
- **Risk of fixing:** peer-credential checks break nothing for legitimate clients (same uid); a token file would need CLI/desktop plumbing.
- **Risk of leaving:** any same-user malware (or a same-user sandbox escape) walks away with every SSH password and private key in one socket call — no passphrase, no prompt.
- **Effort:** S — on Unix, check `getpeereid`/`SO_PEERCRED` uid == euid on accept (5 lines per platform); optionally document the same-user threat model.
- **Suggested fix:** verify peer uid on connection accept and drop mismatches before reading a byte.

### IPC read loop buffers an unbounded line before the size check
- **Area:** ipc
- **Location:** crates/sigrun-core/src/ipc/server.rs:111-117 (`read_until(b'\n', &mut line)`), crates/sigrun-core/src/ipc/protocol.rs:10 (`MAX_LINE`, enforced only in `decode` after the full line is in memory)
- **What:** `MAX_LINE` (1 MiB) is checked after the whole line is already buffered; a client that streams bytes without a newline grows the per-connection buffer without limit. Each accepted connection is a spawned task, so a local process can open many sockets and exhaust host memory.
- **Risk of fixing:** none — legitimate requests are well under 1 MiB.
- **Risk of leaving:** trivial same-user memory-exhaustion DoS against the desktop host (which then loses vault state / active sessions on restart).
- **Effort:** S — wrap the reader in `.take(MAX_LINE as u64 + 1)` and treat EOF-without-newline-at-cap as an error.
- **Suggested fix:** cap the read with `AsyncReadExt::take` before `read_until`, dropping the connection at the limit.

### Agent socket gets mode 0600 explicitly; the host IPC socket gets none
- **Area:** ipc
- **Location:** crates/sigrun-core/src/ipc/server.rs:44 (`UnixListener::bind(addr)`), contrast crates/sigrun-core/src/agent/mod.rs:36 (`set_permissions(addr, 0o600)`)
- **What:** The agent socket is chmod 0600 after bind; the host IPC socket's mode is whatever the process umask leaves (commonly 0755). The 0700 data directory is the real barrier in the default layout — but `sigrun host --socket <path>` (host.rs:1356) lets the socket live anywhere, e.g. `/tmp`, where a umask-0755 socket is connectable by every local user, who can then issue `PasswordGet`/`KeysExport`/`Shutdown`. Even in the default layout the missing chmod is a single layer where the agent deliberately has two.
- **Risk of fixing:** none; mirrors the agent code path.
- **Risk of leaving:** on a misconfigured data dir, other local users can reach the password-serving IPC socket.
- **Effort:** S — one `set_permissions(addr, 0o600)` after bind, as the agent already does.
- **Suggested fix:** chmod the IPC socket to 0600 immediately after `UnixListener::bind`.

### SessionStarted accepts any pid and the sweeper signals its whole process group
- **Area:** ipc
- **Location:** crates/sigrun-core/src/host.rs:512-515 (`Request::SessionStarted`), :327-334 (`end_orphaned_session` → `libc::kill(-(pid), SIGHUP)`)
- **What:** Any IPC client can register an arbitrary pid for any connection id; when the sweeper later judges it orphaned (parent pid <= 1) it sends SIGHUP to the *process group* `-pid`. A same-user client can name a victim process-group leader it owns (or a recycled pid that became one) and get the host to hang it up. Same-user, so impact is limited to self-DoS, but the report is meant to come only from Sigrun's own launch line and nothing verifies that.
- **Risk of fixing:** a sanity check (pid != 1, pid != own pid, maybe verify the pid's command line contains the helper) could false-positive on exotic terminals.
- **Risk of leaving:** same-user process can trick the host into SIGHUP-ing an unrelated process group after pid reuse.
- **Effort:** S — a few guards in `report_session`/`end_orphaned_session`; no protocol change.
- **Suggested fix:** reject reserved pids (0, 1, own pid) at `report_session`, and before signalling check the pid was registered by a session that also sent `session-started` from the expected socket — at minimum verify `kill(pid,0)` ownership matches before the group SIGHUP.

### CLI connect path has no host-key trust gate at all
- **Area:** cli
- **Location:** crates/sigrun-core/src/host.rs:1063-1163 (`connections_connect`), vs apps/desktop/src-tauri/src/commands.rs:376-392 (`connect_guarded`)
- **What:** The TOFU check against Sigrun's known_hosts lives only in the Tauri layer's `connect_guarded`; the IPC `ConnectionsConnect` handler — which the CLI drives — never consults Sigrun's store. Two clients of the same host get different security postures, and the "logic lives in sigrun-core, never in the Tauri layer" rule is inverted for a security control. The launched ssh still checks the user's own `~/.ssh/known_hosts`, but nothing checks Sigrun's.
- **Risk of fixing:** CLI connects to previously provisioned (Sigrun-known but ssh-unknown) hosts start prompting/failing where they used to sail through.
- **Risk of leaving:** `sigrun connect` ignores the trust database the desktop app maintains; a mismatch the desktop would refuse is never shown to a CLI user.
- **Effort:** M — move the keyscan gate into `sigrun-core` (host.rs) behind a flag so both clients share it; russh-based probing would drop the ssh-keyscan dependency.
- **Suggested fix:** run the trust check inside `connections_connect` (core), keeping the desktop's prompt flow driven by the same error codes.

### Provision hardening echoes remote stderr into error messages shown in the UI
- **Area:** core
- **Location:** crates/sigrun-core/src/provision.rs:259-265 (`harden`), :170-173 (`capped_push`)
- **What:** Remote stderr (attacker-influenced if the provisioned host is hostile — which is plausible mid-provision, before keys are installed) is embedded in `Error::Config("sshd hardening failed: {reason}")` and rendered by the UI. Capped at 8 KiB, but terminal escape sequences in that text reach the desktop renderer as plain text (React escapes HTML, so this is cosmetic) and, more relevantly, a misleading message ("rollback complete") could be injected.
- **Risk of fixing:** none; the fixed exit-code messages already exist as fallbacks.
- **Risk of leaving:** a hostile server can put arbitrary text (including fake success/failure instructions) into a Sigrun error dialog.
- **Effort:** S — use the exit code to pick a fixed message and drop `reason` from the error.
- **Suggested fix:** map exit codes 5-8 to fixed strings; log stderr nowhere user-facing.

### `Response::Password(String)` and vault secret clones are plain `String`s with no zeroization
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:1058 (`password.clone()` into `Response::Password`), :786, :1220-1221 (pem/passphrase clones), crates/sigrun-core/src/askpass.rs:25-37 (password returned as `String`)
- **What:** `Debug` is already redacted (manual impl in protocol.rs), so this is memory hygiene only: once a secret leaves `Secret`'s `ZeroizeOnDrop` umbrella it travels as a plain `String` — through the IPC encode buffer, the askpass return value, and provision's `Zeroizing` wrapping is applied *after* an intermediate plain clone (`Zeroizing::new(pem.clone())`). Copies freed without zeroization linger in the heap until reuse.
- **Risk of fixing:** `Zeroizing<String>` serializes transparently with serde but changes call-site types across the CLI, IPC client and Tauri commands.
- **Risk of leaving:** a memory dump (crash report, swap, ptrace by same user) can recover passwords/private keys shortly after use — marginal on top of the same-user socket access already possible.
- **Effort:** M — thread `Zeroizing<String>` through `Response::Password`, askpass, and the provision/import secret handoffs; drop the plain intermediate clones.
- **Suggested fix:** make `Response::Password(Zeroizing<String>)` and clone vault secrets straight into `Zeroizing`, never into a bare `String`.
