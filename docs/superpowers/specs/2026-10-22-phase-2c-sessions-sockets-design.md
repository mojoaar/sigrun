# Phase 2c: Sessions & Sockets Design Spec

**Goal:** Resolve Findings #10, #17, #14, #15, and #18 from the full application review, hardening process tracking, IPC socket creation/permissions, and export bundle file safety.

## 1. Problem Statement & Motivation

During the full application review, several edge cases in process lifecycle management, IPC transport, and export filesystem operations were identified:
- **Finding #10:** `ConnectionEnded` clears a connection from the `active` set, but fails to call `forget_session`. On quick reconnects, the background sweeper seeing an old dead PID will mistakenly clear the newly live session and could signal recycled PIDs. Furthermore, `sweep_dead_sessions` calls `end_orphaned_session(*pid)` even when `!pid_is_alive(*pid)`.
- **Finding #17:** `SessionStarted` accepts any arbitrary PID for any connection ID without verifying if the connection is active or if the PID is valid. An arbitrary same-user client could register a victim process ID, and if that process ever matches orphan heuristics, Sigrun signals its process group.
- **Finding #14:** The Windows IPC server `serve_windows` enters a tight, unthrottled loop if `ServerOptions::create` returns an error, causing 100% CPU usage and ignoring shutdown signals.
- **Finding #15:** The host Unix IPC socket does not explicitly set `0o600` permissions (unlike the SSH agent socket), leaving its security reliant entirely on directory umask if bound to shared paths like `/tmp`.
- **Finding #18:** Exporting to a `.zip` checks `dest.exists()` before calling `File::create`, which is prone to TOCTOU truncation. Exporting to a directory only checks `export.json` and `README.txt`, failing to check if a `keys/` directory already exists and overwriting `.pub` files.

## 2. Detailed Technical Design

### Finding #10 & #17: Session Lifecycle & PID Validation (`host.rs`)
1. **Forget Session on End:** In `host.rs::handle_inner` for `Request::ConnectionEnded { id }`:
   ```rust
   state.clear_active(&id);
   state.forget_session(&id);
   ```
2. **Selective Orphan Signaling:** In `sweep_dead_sessions`:
   Only call `end_orphaned_session(*pid)` if `session_is_orphaned(*pid)`. If `!pid_is_alive(*pid)`, do not send a signal.
3. **PID & State Validation in `SessionStarted`:**
   In `Request::SessionStarted { id, pid }`:
   - Verify `state.is_active(&id)`: a session cannot register unless the connection was launched and marked active.
   - Verify `pid > 1 && pid != std::process::id()` (and on Unix, not `libc::getpid() as u32`).
   - If invalid, return `Error::InvalidInput`.
4. **Defensive Signal Guard:** In `end_orphaned_session(pid)`:
   Ensure `pid > 1 && pid != std::process::id()` before issuing `libc::kill`.

### Finding #14: Windows Named Pipe Create Backoff (`server.rs`)
In `crates/sigrun-core/src/ipc/server.rs` within `serve_windows`:
```rust
let server = match ServerOptions::new().create(name) {
    Ok(server) => server,
    Err(_) => {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
        }
    }
};
```

### Finding #15: Unix IPC Socket Permissions (`server.rs`)
In `crates/sigrun-core/src/ipc/server.rs` within `serve_unix`:
Immediately after `let listener = tokio::net::UnixListener::bind(addr)?;`:
```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(addr, std::fs::Permissions::from_mode(0o600))?;
}
```

### Finding #18: Export Atomic Creation & Overwrite Protection (`host.rs`, `bundle.rs`)
1. **Directory Check:** In `crates/sigrun-core/src/host.rs` within `export_bundle`:
   Include `bundle::KEY_DIR` in the existence check:
   ```rust
   [bundle::EXPORT_FILE, bundle::README_FILE, bundle::KEY_DIR]
       .iter()
       .any(|name| dest.join(name).exists())
   ```
2. **Atomic Zip Creation:** In `crates/sigrun-core/src/bundle.rs` within `write_zip`:
   Replace `File::create(file)` with:
   ```rust
   let file = std::fs::OpenOptions::new()
       .write(true)
       .create_new(true)
       .open(file)
       .map_err(|e| {
           if e.kind() == std::io::ErrorKind::AlreadyExists {
               Error::AlreadyExists(file.display().to_string())
           } else {
               Error::Io(e)
           }
       })?;
   ```

## 3. Testing Strategy
- Unit test in `host.rs` asserting `forget_session` is called when `ConnectionEnded` is handled.
- Unit test in `host.rs` verifying `sweep_dead_sessions` does not signal a non-orphaned dead PID.
- Unit test in `host.rs` verifying `SessionStarted` rejects `pid <= 1`, own PID, and inactive connections.
- Unit test in `server.rs` (or `ipc::tests`) verifying unix IPC socket file mode is `0o600`.
- Unit test in `bundle.rs` / `host.rs` asserting `write_zip` fails if destination already exists, and folder export refuses when `keys/` already exists.
