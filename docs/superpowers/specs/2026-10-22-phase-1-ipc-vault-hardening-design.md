# Phase 1: IPC & Vault Hardening — Design

Status: approved 2026-10-22 for Phase 1 of full-app review.
Implements: Findings 1, 2, 3, 4, 5, 6, 7 from `docs/superpowers/reviews/2026-10-22/report.md`.

## Why

The full-app review identified seven Critical security findings in the IPC and vault boundary:
1. The IPC server accepts connections from any local process with no peer identity verification.
2. The IPC server allows unlimited rapid unlock attempts without throttling or backoff.
3. Concurrent unlock attempts execute Argon2id in parallel, creating memory exhaustion (OOM) risks.
4. The IPC stream buffers lines without a bound prior to detecting newlines, allowing memory exhaustion via unbounded inputs.
5. The pre-flight host-key check fails open when `ssh-keyscan` is missing or fails, bypassing TOFU prompts.
6. Vault mutations (`put` and `remove`) update in-memory state before persistence succeeds, diverging memory from disk on I/O error.
7. Vault file KDF parameters are parsed and used without range clamping, allowing tampered files to cause multi-gigabyte allocations or execution hangs.

Together, these represent the primary security surface of Sigrun: socket access, rate limiting, resource bounds, cryptographic bounds, and state consistency.

## Goal

Harden the IPC and vault boundary against local privilege abuse, denial of service (memory/CPU exhaustion), rate-limit bypass, fail-open trust bypass, and state divergence.

## Non-goals

- Remote network authentication (Sigrun IPC is strictly local host-to-client).
- Shifting the TOFU trust gate entirely into `sigrun-core` (reserved for Phase 2a / Finding 8).
- UI changes or redesigns (all Phase 1 fixes are in core domain logic and IPC layers).

## Detailed Design

### 1. IPC Peer Authentication (Finding 1)

**Location:** `crates/sigrun-core/src/ipc/server.rs`

- **Unix:** On `listener.accept()`, query `stream.peer_cred()`.
  - Extract the peer's UID: `cred.uid()`.
  - Compare against the host process's UID: `unsafe { libc::getuid() }`.
  - If peer UID does not match our UID, immediately drop the connection without reading or processing any bytes.
  - If `stream.peer_cred()` returns an error, drop the connection.
- **Windows:** Named pipe path already includes `std::process::id()` and user data directory hash, but server options should ensure default named pipe security attributes restrict connection to the current user context.

### 2. In-Host Unlock Throttling (Finding 2)

**Location:** `crates/sigrun-core/src/host.rs`

- In `HostState`, track failed unlock attempts:
  - `failed_unlock_attempts: std::sync::atomic::AtomicU32` (initialized to 0).
- When `Request::VaultUnlock` is received:
  - If unlock fails (e.g. `Error::InvalidPassphrase`):
    - Increment `failed_unlock_attempts`.
    - Calculate backoff delay: `base_delay * 2^(attempts - 1)`, where `base_delay = 500ms`, capped at a maximum of `30s`.
    - Apply `tokio::time::sleep(delay).await` before returning the error response to the client.
  - If unlock succeeds:
    - Reset `failed_unlock_attempts.store(0, std::sync::atomic::Ordering::SeqCst)`.
    - Return `Response::Unlocked`.
- This ensures any client attempting a brute-force attack over IPC experiences exponential backoff directly enforced by the host.

### 3. Concurrency Permit / Serialization for Vault Unlock KDF (Finding 3)

**Location:** `crates/sigrun-core/src/host.rs`

- In `HostState`, maintain an unlock lock:
  - `unlock_lock: tokio::sync::Mutex<()>`
- When `Request::VaultUnlock` is processed:
  - Acquire `let _permit = state.unlock_lock.lock().await;` before spawning the blocking task that executes `Vault::new(vault_path).unlock(&passphrase)`.
- Holding this permit across the KDF derivation ensures at most one Argon2id computation runs at any given moment. Parallel requests queue cleanly on the mutex rather than allocating parallel 64 MiB Argon2id memory blocks.

### 4. Bounded IPC Line Reader (Finding 4)

**Location:** `crates/sigrun-core/src/ipc/server.rs`

- Replace `reader.read_until(b'\n', &mut line)` with a bounded line reader function `read_line_bounded`:
  - `async fn read_line_bounded<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>, max_bytes: usize) -> Result<usize>`
  - Loops over `reader.fill_buf().await?`:
    - Checks if `b'\n'` is in the available buffer chunk.
    - If found: appends bytes up to `\n` to `buf`. If `buf.len() > max_bytes`, returns an error (`Error::Ipc("request line exceeded maximum allowed length")`). Consumes the buffer and returns `Ok(bytes_read)`.
    - If not found: checks if `buf.len() + chunk.len() > max_bytes`. If so, returns an error without buffering further. Otherwise appends chunk to `buf`, consumes chunk, and loops.
    - On EOF (0 bytes available from `fill_buf`): returns `Ok(0)`.
- If the line exceeds `MAX_LINE` (1 MiB), the server returns an error response and closes the connection, completely preventing memory exhaustion from unbounded input.

### 5. TOFU Fails Closed on Missing or Failed `ssh-keyscan` (Finding 5)

**Location:** `apps/desktop/src-tauri/src/commands.rs`

- In `check_trust`:
  - Current behavior: if `probe_host_key(host, port).await` returns `None`, logs a warning and returns `Ok(())` (fail open).
  - New behavior: if `probe_host_key(host, port).await` returns `None`, return `Err(CommandError { code: "host_key_probe_failed".into(), message: format!("could not scan host keys for {host}:{port}") })`.
- In `connect_guarded`:
  - When `force` is `true` (user explicitly clicks "Connect anyway" in the UI after a prompt or error), bypass `check_trust`, preserving user agency while ensuring all automated or unprompted paths fail closed.

### 6. Vault Memory-Disk Atomicity on `put` and `remove` (Finding 6)

**Location:** `crates/sigrun-core/src/vault/store.rs`

- In `UnlockedVault::put(&mut self, key: &str, secret: Secret) -> Result<()>`:
  - Insert new secret and retain the previous entry: `let prev = self.payload.insert(key.to_string(), secret);`
  - Call `self.persist()`.
  - If `persist` fails:
    - Roll back `self.payload`: if `prev` was `Some(s)`, re-insert `prev`; if `None`, remove `key`.
    - Return the persist error.
  - If `persist` succeeds, return `Ok(())`.
- In `UnlockedVault::remove(&mut self, key: &str) -> Result<()>`:
  - If `key` is present, remove it and retain the removed secret: `let removed = self.payload.remove(key);`
  - If `removed.is_some()`:
    - Call `self.persist()`.
    - If `persist` fails:
      - Roll back `self.payload`: re-insert `removed.unwrap()` into `self.payload`.
      - Return the persist error.
  - Return `Ok(())`.

### 7. Clamped KDF Parameters on Vault Unlock (Finding 7)

**Location:** `crates/sigrun-core/src/vault/crypto.rs`

- In `derive_key(passphrase: &str, params: &KdfParams) -> Result<Zeroizing<[u8; 32]>>`:
  - Validate `params.algorithm == "argon2id"`, else return `Error::VaultCorrupt("unsupported KDF algorithm")`.
  - Validate memory cost: `params.m_cost_kib >= 8_192 && params.m_cost_kib <= 262_144` (min 8 MiB, max 256 MiB; standard is 65,536 = 64 MiB).
  - Validate time cost: `params.t_cost >= 1 && params.t_cost <= 10` (standard is 3).
  - Validate parallelism: `params.p_cost >= 1 && params.p_cost <= 16` (standard is 4).
  - If any parameter falls outside these bounds, reject with `Error::VaultCorrupt("KDF parameters exceed safe security bounds")`.
- This ensures any tampered vault file cannot consume gigabytes of RAM or loop indefinitely on derivation.
