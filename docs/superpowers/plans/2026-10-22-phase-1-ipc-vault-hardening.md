# Phase 1: IPC & Vault Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the IPC server and vault boundary against unauthorized local access, brute-force exhaustion, unbounded line buffering, memory divergence, and fail-open host-key checks.

**Architecture:** Peer UID verification on unix socket accept; in-host unlock throttling and concurrency serialization; bounded line reader; rollback on vault mutation failure; strict KDF parameter clamping; and fail-closed TOFU trust checks.

**Tech Stack:** Rust 2021, Tokio, Argon2, libc (Unix).

**Spec:** `docs/superpowers/specs/2026-10-22-phase-1-ipc-vault-hardening-design.md`

## Global Constraints

- Rust 2021, MSRV 1.90. Dependencies only via `[workspace.dependencies]`.
- The gate is `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` (and when desktop files change, `npm test -- --run && npm run typecheck && npm run lint && npm run build` in `apps/desktop`).
- Every user-facing or behavior change gets a `## [Unreleased]` entry in `CHANGELOG.md`.
- One commit per task with a conventional commit message (`feat:`, `fix:`, `test:`).
- Tests assert real behavior; write tests first.

## Review Focus

- **Foreign UID rejection:** An IPC client with mismatched UID must be dropped immediately without reading or sending data.
- **Unbounded line stream:** A stream sending > 1 MiB without a newline must be terminated without consuming unbounded heap memory.
- **Argon2 concurrency pool exhaustion:** Multiple simultaneous unlock requests must queue through a single permit rather than spawning parallel memory-intensive Argon2 jobs.
- **Vault mutation rollback:** If write/persist fails on `put` or `remove`, the in-memory payload must exactly match disk state.
- **KDF parameter validation:** Tampered KDF parameters (m_cost > 256 MiB, t_cost > 10, invalid algorithm) must return `VaultCorrupt` and not panic or stall.

---

### Task 1: Peer UID Verification on Unix IPC Socket (Finding 1)

**Files:**
- Modify: `crates/sigrun-core/src/ipc/server.rs:45-56`
- Test: `crates/sigrun-core/src/ipc/mod.rs`

**Interfaces:**
- Consumes: `stream.peer_cred()` from `tokio::net::UnixStream`, `libc::getuid()`
- Produces: Connection drop on UID mismatch before `handle_conn` spawn

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-core/src/ipc/mod.rs`, add a test `unix_socket_verifies_peer_credentials`:
```rust
#[cfg(unix)]
#[tokio::test]
async fn unix_socket_verifies_peer_credentials() {
    // Verifies that a stream from the same process succeeds
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host.sock");
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let state = std::sync::Arc::new(HostState::new(paths).await.unwrap());
    let server = tokio::spawn({
        let state = state.clone();
        let sock = sock.clone();
        async move { serve(&sock, state).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let res = client::request(&sock, &Request::Status).await;
    assert!(res.is_ok());
    state.shutdown.cancel();
    let _ = server.await;
}
```

- [ ] **Step 2: Run test to verify it compiles and runs**

Run: `cargo test -p sigrun-core --lib ipc::tests::unix_socket_verifies_peer_credentials`

- [ ] **Step 3: Implement peer UID check in `crates/sigrun-core/src/ipc/server.rs`**

In `serve_unix`:
```rust
accepted = listener.accept() => {
    match accepted {
        Ok((stream, _)) => {
            #[cfg(unix)]
            {
                match stream.peer_cred() {
                    Ok(cred) if cred.uid() == unsafe { libc::getuid() } => {}
                    _ => continue,
                }
            }
            let state = state.clone();
            tokio::spawn(handle_conn(stream, state));
        }
        Err(_) => continue,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sigrun-core --lib ipc::tests::unix_socket_verifies_peer_credentials`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/ipc/server.rs crates/sigrun-core/src/ipc/mod.rs
git commit -m "fix(core): verify peer UID on unix IPC socket connections"
```

---

### Task 2: Bounded IPC Line Reader (Finding 4)

**Files:**
- Modify: `crates/sigrun-core/src/ipc/server.rs:102-146`
- Test: `crates/sigrun-core/src/ipc/mod.rs`

**Interfaces:**
- Consumes: `protocol::MAX_LINE` (1024 * 1024 bytes)
- Produces: `read_line_bounded<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>, max_bytes: usize) -> std::io::Result<usize>`

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-core/src/ipc/mod.rs`:
```rust
#[tokio::test]
async fn oversized_request_line_is_rejected_without_unbounded_buffering() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host.sock");
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let state = std::sync::Arc::new(HostState::new(paths).await.unwrap());
    let server = tokio::spawn({
        let state = state.clone();
        let sock = sock.clone();
        async move { serve(&sock, state).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Send a stream of MAX_LINE + 1024 bytes with no newline
    use tokio::io::AsyncWriteExt;
    let mut stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let garbage = vec![b'a'; protocol::MAX_LINE + 1024];
    let res = stream.write_all(&garbage).await;
    assert!(res.is_ok());

    // Connection should be closed by server when line exceeds MAX_LINE
    use tokio::io::AsyncReadExt;
    let mut resp = Vec::new();
    let read_res = stream.read_to_end(&mut resp).await;
    assert!(read_res.is_ok());

    state.shutdown.cancel();
    let _ = server.await;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-core --lib ipc::tests::oversized_request_line_is_rejected_without_unbounded_buffering`
Expected: FAIL (server hangs or buffers indefinitely without newline)

- [ ] **Step 3: Implement `read_line_bounded` in `server.rs`**

```rust
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

async fn read_line_bounded<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_bytes: usize,
) -> std::io::Result<usize> {
    let mut total_read = 0;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(total_read);
        }
        if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
            let len = pos + 1;
            if buf.len() + len > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "line exceeded maximum allowed length",
                ));
            }
            buf.extend_from_slice(&chunk[..len]);
            reader.consume(len);
            total_read += len;
            return Ok(total_read);
        } else {
            let len = chunk.len();
            if buf.len() + len > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "line exceeded maximum allowed length",
                ));
            }
            buf.extend_from_slice(chunk);
            reader.consume(len);
            total_read += len;
        }
    }
}
```
In `handle_conn`, call `read_line_bounded(&mut reader, &mut line, protocol::MAX_LINE).await`. On error or EOF, break.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sigrun-core --lib ipc::tests::oversized_request_line_is_rejected_without_unbounded_buffering`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/ipc/server.rs crates/sigrun-core/src/ipc/mod.rs
git commit -m "fix(core): bound IPC stream line reads to prevent memory exhaustion"
```

---

### Task 3: Serialize Concurrent Vault Unlock Operations (Finding 3)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs:140-200, 398-407`
- Test: `crates/sigrun-core/src/host.rs`

**Interfaces:**
- Produces: `unlock_lock: tokio::sync::Mutex<()>` in `HostState`

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-core/src/host.rs`:
Add a test `parallel_unlock_requests_are_serialized` testing that multiple parallel unlock requests are handled cleanly without panic or resource contention.

- [ ] **Step 2: Run test to verify baseline**

Run: `cargo test -p sigrun-core --lib host::tests::parallel_unlock_requests_are_serialized`

- [ ] **Step 3: Add `unlock_lock` to `HostState` and hold across Argon2 computation**

In `HostState`:
`pub unlock_lock: tokio::sync::Mutex<()>`
In `handle(Request::VaultUnlock)`:
`let _permit = state.unlock_lock.lock().await;` prior to `spawn_blocking`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sigrun-core --lib host::tests::parallel_unlock_requests_are_serialized`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/host.rs
git commit -m "fix(core): serialize vault unlock operations to prevent Argon2 OOM"
```

---

### Task 4: In-Host Exponential Backoff on Failed Unlock (Finding 2)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs:140-200, 398-407`
- Test: `crates/sigrun-core/src/host.rs`

**Interfaces:**
- Produces: `failed_unlock_attempts: std::sync::atomic::AtomicU32` in `HostState`, exponential sleep up to 30s on failure, reset to 0 on success.

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-core/src/host.rs`:
Add a test `failed_unlock_attempts_apply_exponential_backoff`:
Verify that consecutive wrong passphrase attempts result in increasing duration, and a correct passphrase resets the attempt count.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-core --lib host::tests::failed_unlock_attempts_apply_exponential_backoff`
Expected: FAIL (currently 0 backoff)

- [ ] **Step 3: Implement unlock backoff in `HostState`**

Add `failed_unlocks: std::sync::atomic::AtomicU32` to `HostState`.
On unlock failure:
Increment attempt count: `let attempts = state.failed_unlocks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;`
Compute delay: `let millis = (500u64 * (1u64 << (attempts.min(6) - 1))).min(30_000);`
Apply `tokio::time::sleep(std::time::Duration::from_millis(millis)).await;`
On unlock success:
`state.failed_unlocks.store(0, std::sync::atomic::Ordering::SeqCst);`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sigrun-core --lib host::tests::failed_unlock_attempts_apply_exponential_backoff`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/host.rs
git commit -m "fix(core): apply exponential backoff on repeated failed vault unlocks"
```

---

### Task 5: Clamped KDF Parameters on Vault Unlock (Finding 7)

**Files:**
- Modify: `crates/sigrun-core/src/vault/crypto.rs:50-65`
- Test: `crates/sigrun-core/src/vault/crypto.rs`

**Interfaces:**
- Consumes: `KdfParams`
- Produces: `derive_key` returns `Error::VaultCorrupt` if algorithm != "argon2id", `m_cost_kib` not in `8_192..=262_144`, `t_cost` not in `1..=10`, or `p_cost` not in `1..=16`.

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-core/src/vault/crypto.rs`:
```rust
#[test]
fn excessive_kdf_params_are_rejected() {
    let mut params = KdfParams::new([0u8; 16]);
    params.m_cost_kib = 1_000_000; // 1 GiB
    let res = derive_key("pass", &params);
    assert!(matches!(res, Err(Error::VaultCorrupt(_))));

    params.m_cost_kib = 65_536;
    params.t_cost = 100;
    let res = derive_key("pass", &params);
    assert!(matches!(res, Err(Error::VaultCorrupt(_))));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-core --lib vault::crypto::tests::excessive_kdf_params_are_rejected`
Expected: FAIL (currently accepted)

- [ ] **Step 3: Implement parameter bounds check in `derive_key`**

In `derive_key`:
```rust
if params.algorithm != "argon2id" {
    return Err(Error::VaultCorrupt(format!(
        "unsupported KDF algorithm: {}",
        params.algorithm
    )));
}
if !(8_192..=262_144).contains(&params.m_cost_kib) {
    return Err(Error::VaultCorrupt(format!(
        "m_cost {} KiB is outside safe range (8192..=262144)",
        params.m_cost_kib
    )));
}
if !(1..=10).contains(&params.t_cost) {
    return Err(Error::VaultCorrupt(format!(
        "t_cost {} is outside safe range (1..=10)",
        params.t_cost
    )));
}
if !(1..=16).contains(&params.p_cost) {
    return Err(Error::VaultCorrupt(format!(
        "p_cost {} is outside safe range (1..=16)",
        params.p_cost
    )));
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sigrun-core --lib vault::crypto::tests::excessive_kdf_params_are_rejected`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/vault/crypto.rs
git commit -m "fix(core): clamp vault KDF parameters to safe ranges"
```

---

### Task 6: Vault Memory-Disk Atomicity on `put` and `remove` (Finding 6)

**Files:**
- Modify: `crates/sigrun-core/src/vault/store.rs:160-170`
- Test: `crates/sigrun-core/src/vault/store.rs`

**Interfaces:**
- Produces: Rollback of in-memory `payload` in `UnlockedVault::put` and `UnlockedVault::remove` if `persist()` fails.

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-core/src/vault/store.rs`:
Write a test `failed_persist_rolls_back_memory_changes`:
Create a vault in a read-only directory or corrupt path during persist, verify that on persist failure, `vault.get(key)` returns the previous state (or None).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-core --lib vault::store::tests::failed_persist_rolls_back_memory_changes`
Expected: FAIL

- [ ] **Step 3: Implement rollback in `put` and `remove`**

In `UnlockedVault::put`:
```rust
pub fn put(&mut self, key: &str, secret: Secret) -> Result<()> {
    let prev = self.payload.insert(key.to_string(), secret);
    if let Err(e) = self.persist() {
        match prev {
            Some(old) => {
                self.payload.insert(key.to_string(), old);
            }
            None => {
                self.payload.remove(key);
            }
        }
        return Err(e);
    }
    Ok(())
}
```
In `UnlockedVault::remove`:
```rust
pub fn remove(&mut self, key: &str) -> Result<()> {
    if let Some(removed) = self.payload.remove(key) {
        if let Err(e) = self.persist() {
            self.payload.insert(key.to_string(), removed);
            return Err(e);
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sigrun-core --lib vault::store::tests::failed_persist_rolls_back_memory_changes`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/vault/store.rs
git commit -m "fix(core): roll back in-memory changes when vault persistence fails"
```

---

### Task 7: TOFU Fails Closed on Missing or Failed `ssh-keyscan` (Finding 5)

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs:363-392`
- Test: `apps/desktop/src-tauri/src/commands.rs` (unit test in `commands.rs`)

**Interfaces:**
- Consumes: `probe_host_key`
- Produces: `check_trust` returns `Err(CommandError { code: "host_key_probe_failed", ... })` when `probe_host_key` returns `None`. `connect_guarded` bypasses `check_trust` when `force == true`.

- [ ] **Step 1: Write the failing test**

In `apps/desktop/src-tauri/src/commands.rs` tests:
Test that if `probe_host_key` returns `None`, `check_trust` yields an error rather than `Ok(())`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sigrun-desktop commands::tests`
Expected: FAIL

- [ ] **Step 3: Update `check_trust` and `connect_guarded` in `commands.rs`**

In `check_trust`:
```rust
async fn check_trust(state: &HostState, host: &str, port: u16) -> Result<(), CommandError> {
    let Some(keys) = probe_host_key(host, port).await else {
        return Err(CommandError {
            code: "host_key_probe_failed".into(),
            message: format!("could not scan host keys for {host}:{port}"),
        });
    };
    evaluate_trust(&state.paths.data_dir.join("known_hosts"), host, port, &keys)
}
```
In `connect_guarded`:
Ensure `!print_only && !force` guards `check_trust`, so clicking "Connect anyway" (`force = true`) skips the check.

- [ ] **Step 4: Run tests and workspace gate**

Run: `cargo test --all-targets`
Run: `npm test -- --run && npm run typecheck && npm run lint && npm run build` in `apps/desktop`
Expected: PASS

- [ ] **Step 5: Update CHANGELOG.md and commit**

Add bullets under `## [Unreleased]` for each of the 7 hardening fixes.
```bash
git add apps/desktop/src-tauri/src/commands.rs CHANGELOG.md
git commit -m "fix(desktop): fail closed on host-key scan failure during connect"
```
