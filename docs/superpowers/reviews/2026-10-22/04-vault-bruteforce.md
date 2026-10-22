# Pass 4 — Vault brute-force

Scope: unlock paths (`Vault::unlock` in `vault/store.rs`, `Request::VaultUnlock` dispatch in `host.rs`, IPC socket in `ipc/server.rs`, CLI `vault unlock`, desktop `vault_unlock` command). Confirmed facts first, then findings.

**Confirmed non-findings (verified, not listed below):**

- Argon2id params are exactly the documented `m=65536, t=3, p=4` with a 16-byte random salt (`crypto.rs:33-35`), pinned in code and asserted by the `params_are_pinned` test. This is the intended — and only — defense for the offline case (attacker holding `vault.json`), and it is correctly implemented. A weak offline-attacker passphrase is a user-choice problem, not a code defect.
- A failed unlock does **not** clobber an already-unlocked vault: `host.rs:400-404` runs `unlock` in `spawn_blocking` and only assigns `Some(unlocked)` after `?`, so a wrong guess never locks out a running session or zeroizes live state.
- Error separation is deliberate and tested: wrong passphrase → `InvalidPassphrase` ("invalid passphrase", code `invalid_passphrase`); payload tamper/corruption → `VaultCorrupt` (`store.rs:123-130`, test `wrong_passphrase_is_distinct_from_corruption`). The check value costs a full Argon2id run per attempt regardless of outcome, so there is no cheap pre-screen oracle.
- The Unix socket lives inside `data_dir` which `ensure_private_dir` forces to `0700` (`paths.rs:144`), and the vault file itself is `0600`. The threat model is therefore: another process running **as the same user** (or root). No cross-user attack surface on Unix.

### No throttling, backoff, or attempt counter on failed unlock over IPC
- **Area:** ipc
- **Location:** crates/sigrun-core/src/host.rs:398-407
- **What:** The `VaultUnlock` arm runs the KDF and returns `invalid_passphrase` on failure, with no failure counter, no delay, no lockout, no logging — forever, at line rate. `HostState` has no per-client or global attempt state.
- **Risk of fixing:** any counter risks DoS-ing the legitimate user (a mistyped passphrase plus a retrying client could trip a lockout); must be carefully scoped (e.g. exponential backoff only, never a hard wipe).
- **Risk of leaving:** a same-user process can brute-force the passphrase online at Argon2id speed (~1-3 attempts/sec/core — slow but unbounded, and silent: the user sees nothing).
- **Effort:** S — a small `failed_attempts`/`next_allowed_at` pair in `HostState` plus a sleep in the unlock arm.
- **Suggested fix:** add per-host exponential backoff (e.g. 2^n capped at ~30 s) after each failed `VaultUnlock`; never a hard lockout or wipe for a local vault.

### Unauthenticated IPC socket: any same-user process can hammer unlock attempts
- **Area:** ipc
- **Location:** crates/sigrun-core/src/ipc/server.rs:44-57,103-145
- **What:** The Unix listener accepts any connection that can open the socket path; there is no token, credential check (`SO_PEERCRED`), or handshake. Filesystem perms (`0700` dir) are the only gate, so any process running as the same user — a compromised app, a malicious npm postinstall — can script `vault_unlock` attempts with no user interaction.
- **Risk of fixing:** peer-credential checks are platform-specific (macOS `getpeereid`, Linux `SO_PEERCRED`, Windows named-pipe ACLs) and easy to get subtly wrong; a token file adds a secret-management problem of its own.
- **Risk of leaving:** the "user typing at a prompt" mental model is false — brute force can be fully automated by any same-user malware, at KDF speed, with zero visibility.
- **Effort:** M — `getpeereid`/`SO_PEERCRED` per platform plus tests; token auth is L.
- **Suggested fix:** on Unix, check connecting uid == own uid via peer credentials and refuse otherwise; combine with the backoff finding above rather than full token auth.

### Parallel unlock attempts can exhaust memory (Argon2 m=65536 × blocking pool)
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:400-403
- **What:** Each `VaultUnlock` runs on `tokio::task::spawn_blocking`, whose pool defaults to 512 threads, and each attempt holds 64 MiB for Argon2. Dozens of concurrent attempts from one or many connections allocate gigabytes and can OOM the host (and the desktop app is the host).
- **Risk of fixing:** serializing unlocks makes a legitimate unlock wait behind an attacker's queue — acceptable, since unlocks are rare.
- **Risk of leaving:** a trivial same-user memory-exhaustion DoS that kills the desktop app, locking the vault and dropping live sessions.
- **Effort:** S — a dedicated `tokio::sync::Semaphore(1)` (or small N) around the unlock `spawn_blocking`.
- **Suggested fix:** serialize unlock attempts behind a semaphore of 1-2 permits.

### Failed attempts are silent: no logging, no UI signal, no counter anywhere
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:398-407
- **What:** A wrong passphrase produces an error response to the caller and nothing else — no log line, no event, no persisted count. A user has no way to learn that 10,000 attempts happened overnight.
- **Risk of fixing:** logging must never include the attempted passphrase (it is itself a near-miss secret); only the count/timestamp.
- **Risk of leaving:** attacks are invisible; the user can't distinguish "I mistyped" from "something hammered my vault all night".
- **Effort:** S — one log line per failure, plus optionally surfacing a failure count in `StatusInfo`.
- **Suggested fix:** log each failed unlock (no passphrase) and consider exposing `failed_unlock_attempts_since_last_success` in `Status`.

### CLI and desktop UX give unlimited immediate retries
- **Area:** cli
- **Location:** crates/sigrun-cli/src/cli.rs:362-375; apps/desktop/src/views/VaultGate.tsx:15-32
- **What:** Neither client adds even a soft delay between attempts; `vault unlock --passphrase-stdin` in a shell loop is the intended brute-force vector. This is by design for a CLI, but it means client-side friction is zero and all protection must live in the host (see findings above).
- **Risk of fixing:** none real — clients shouldn't be the enforcement point anyway.
- **Risk of leaving:** none beyond the host findings; listed for completeness so no one "fixes" the wrong layer.
- **Effort:** S — N/A once the host enforces.
- **Suggested fix:** none client-side; enforce in the host.
