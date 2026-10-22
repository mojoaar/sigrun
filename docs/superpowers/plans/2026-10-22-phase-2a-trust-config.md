# Phase 2a: Trust & Config Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Centralize the Host-Key Trust On First Use (TOFU) gate in `sigrun-core`, replace string-parsing for trust errors with structured payloads, support `Key=Value` syntax in `ssh_config`, and fix Windows path quoting for `cmd.exe`.

**Architecture:**
*   The TOFU gate will be moved from `apps/desktop/src-tauri/src/commands.rs` to `crates/sigrun-core/src/trust.rs`, making it accessible to all IPC clients (CLI, desktop).
*   Trust-related errors will use structured payloads over IPC, eliminating brittle string parsing.
*   `ssh_config` parsing will be enhanced to support `Key=Value` syntax.
*   Windows path quoting will be fixed by implementing `cmd.exe`-aware escaping.

**Tech Stack:** Rust, Tauri (desktop app), Tokio (async runtime), Serde (serialization).

**Spec:** `docs/superpowers/specs/2026-10-22-phase-2a-trust-config-design.md`

## Global Constraints

*   Follow AGENTS.md: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.
*   When frontend files change, also run `npm test -- --run && npm run typecheck && npm run lint && npm run build` in `apps/desktop`.
*   Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard enforces it.
*   A frontend `listen(...)` needs a permission: every Tauri plugin API the webview calls must be granted in `apps/desktop/src-tauri/capabilities/default.json`.
*   Tests assert real behaviour. Frontend tests use the built-in matchers and `within(...)` to disambiguate — `@testing-library/jest-dom` is deliberately absent.
*   No new dependency without a strong reason.
*   The version lives in exactly two files: `Cargo.toml` and `apps/desktop/package.json`.
*   Every change gets a Keep-a-Changelog entry under `## [Unreleased]`.
*   Branch per plan, named `plan-<n>-<topic>`, merged with `--no-ff`.
*   Never push, force-push or delete a remote branch without asking.

## Review Focus

*   The TOFU gate's interaction with the existing reachability probe: ensure the TOFU gate runs *before* the reachability probe.
*   The exact serialization format of the structured trust error payload over IPC: ensure it's an optional `payload` field on `Response::Error`.
*   The CLI's display of structured trust errors: ensure it's human-readable and actionable.
*   The precise `cmd.exe` quoting rules for Windows paths and arguments, especially with spaces and special characters.
*   The `print_only` behavior on Windows: ensure it renders POSIX-compatible output.

---

### Task 1: Centralize TOFU Gate and Structured Errors in Core

**Files:**
- Create: `crates/sigrun-core/src/trust.rs`
- Modify: `crates/sigrun-core/src/error.rs`
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Test: `crates/sigrun-core/src/trust.rs` (unit tests)
- Test: `crates/sigrun-core/src/host.rs` (integration tests for `ConnectionsConnect`)
- Test: `apps/desktop/src-tauri/src/commands.rs` (update existing tests)
- Test: `apps/desktop/src/views/Connections.test.tsx` (update existing tests)
- Test: `apps/desktop/src-tauri/src/tray.rs` (update existing tests)

**Interfaces:**
- Consumes: `crates/sigrun-core/src/model.rs` (for `Connection` struct), `crates/sigrun-core/src/error.rs` (for `Error` enum).
- Produces: `crates/sigrun-core/src/trust.rs` will expose `probe_host_key` and `evaluate_trust` functions. `crates/sigrun-core/src/error.rs` will expose `Error::HostKeyUnknown` and `Error::HostKeyMismatch` with structured payloads. `crates/sigrun-core/src/ipc/protocol.rs` will have `Response::Error` with an optional `payload` field.

- [ ] **Step 1: Write failing unit tests for `trust.rs`**

```rust
// crates/sigrun-core/src/trust.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[tokio::test]
    async fn probe_host_key_returns_key_on_success() {
        // Mock ssh-keyscan to return a known key
        // Assert that probe_host_key returns the correct key
    }

    #[tokio::test]
    async fn probe_host_key_returns_none_on_failure() {
        // Mock ssh-keyscan to fail
        // Assert that probe_host_key returns None
    }

    #[test]
    fn evaluate_trust_returns_ok_for_known_host() {
        // Provide a known host key and a matching stored key
        // Assert evaluate_trust returns Ok(())
    }

    #[test]
    fn evaluate_trust_returns_unknown_for_unknown_host() {
        // Provide an unknown host key
        // Assert evaluate_trust returns Error::HostKeyUnknown
    }

    #[test]
    fn evaluate_trust_returns_mismatch_for_mismatched_host() {
        // Provide a mismatched host key
        // Assert evaluate_trust returns Error::HostKeyMismatch
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sigrun-core --lib trust::tests`
Expected: FAIL with "function not defined" or similar.

- [ ] **Step 3: Implement `trust.rs` and update `error.rs` and `ipc/protocol.rs`**

*   Move `probe_host_key` and `evaluate_trust` from `apps/desktop/src-tauri/src/commands.rs` to `crates/sigrun-core/src/trust.rs`.
*   Define `Error::HostKeyUnknown` and `Error::HostKeyMismatch` in `crates/sigrun-core/src/error.rs` with structured fields.
*   Update `Response::Error` in `crates/sigrun-core/src/ipc/protocol.rs` to include an optional `payload: Option<serde_json::Value>` field.
*   Implement `From<Error> for Response` to populate the `payload` for trust errors.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sigrun-core --lib trust::tests`
Expected: PASS

- [ ] **Step 5: Update `host.rs` to use the new TOFU gate**

*   In `crates/sigrun-core/src/host.rs`, modify `Request::ConnectionsConnect` handler to call `trust::evaluate_trust` before launching the connection.
*   If `evaluate_trust` returns an error, return the structured error directly.
*   Ensure `force: true` bypasses the TOFU gate.
*   Ensure the TOFU gate runs *before* the reachability probe.

- [ ] **Step 6: Write failing integration tests for `host.rs`**

```rust
// crates/sigrun-core/src/host.rs
#[cfg(test)]
mod tests {
    // ... existing tests ...

    #[tokio::test]
    async fn connections_connect_refuses_unknown_host_key_without_force() {
        // Simulate a request to connect to an unknown host without force
        // Assert that the response is Error::HostKeyUnknown
    }

    #[tokio::test]
    async fn connections_connect_allows_unknown_host_key_with_force() {
        // Simulate a request to connect to an unknown host with force
        // Assert that the connection proceeds (mock terminal launch)
    }

    #[tokio::test]
    async fn connections_connect_refuses_mismatched_host_key_without_force() {
        // Simulate a request to connect to a mismatched host without force
        // Assert that the response is Error::HostKeyMismatch
    }

    #[tokio::test]
    async fn connections_connect_allows_mismatched_host_key_with_force() {
        // Simulate a request to connect to a mismatched host with force
        // Assert that the connection proceeds (mock terminal launch)
    }
}
```

- [ ] **Step 7: Run tests to verify they fail**

Run: `cargo test -p sigrun-core --lib host::tests`
Expected: FAIL

- [ ] **Step 8: Implement `host.rs` changes**

*   Integrate the `trust::evaluate_trust` call into the `ConnectionsConnect` handler.

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p sigrun-core --lib host::tests`
Expected: PASS

- [ ] **Step 10: Update `apps/desktop/src-tauri/src/commands.rs`**

*   Remove the old TOFU gate logic from `commands.rs`.
*   Update `connect_guarded` to simply forward `ConnectionsConnect` to the IPC server.
*   Update error handling to deserialize structured trust errors from the IPC response.
*   Update `apps/desktop/src-tauri/src/tray.rs` to handle the new structured payload in `connection-error`.

- [ ] **Step 11: Update `apps/desktop/src/views/Connections.tsx`**

*   Modify error handling to parse the structured trust error payload directly from the IPC response.
*   Update the trust dialog to use the structured data.
*   Ensure the `ConnectionError` type in `Connections.tsx` is updated.

- [ ] **Step 12: Update frontend tests**

*   Update `apps/desktop/src/views/Connections.test.tsx` and `apps/desktop/src/App.test.tsx` to reflect the new structured error payloads.
*   Update `apps/desktop/src-tauri/src/tray.rs` tests to reflect the new `connection-error` payload shape.

- [ ] **Step 13: Run frontend tests**

Run: `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build`
Expected: PASS

- [ ] **Step 14: Commit**

```bash
git add crates/sigrun-core/src/trust.rs crates/sigrun-core/src/error.rs crates/sigrun-core/src/ipc/protocol.rs crates/sigrun-core/src/host.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/tray.rs apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/App.test.tsx apps/desktop/src-tauri/src/tray.rs
git commit -m "feat(core): Centralize TOFU gate and use structured trust errors"
```

---

### Task 2: Enhance SSH Config Parser for `Key=Value` Syntax

**Files:**
- Modify: `crates/sigrun-core/src/ssh_config.rs`
- Test: `crates/sigrun-core/src/ssh_config.rs` (unit tests)

**Interfaces:**
- Consumes: None
- Produces: Updated `ssh_config` parsing logic.

- [ ] **Step 1: Write failing unit tests for `ssh_config.rs`**

```rust
// crates/sigrun-core/src/ssh_config.rs
#[cfg(test)]
mod tests {
    // ... existing tests ...

    #[test]
    fn parses_key_value_syntax() {
        let config = "Host=example.com\n  Port=2222\n  User=testuser";
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[0].user, Some("testuser".to_string()));
    }

    #[test]
    fn parses_key_value_with_spaces_around_equals() {
        let config = "Host = example.com\n  Port = 2222\n  User = testuser";
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts[0].port, Some(2222));
        assert_eq!(hosts[0].user, Some("testuser".to_string()));
    }

    #[test]
    fn parses_key_value_with_quoted_value_containing_spaces() {
        let config = "Host=example.com\n  IdentityFile=\"~/.ssh/my key.pem\"";
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts[0].identity_file, Some("~/.ssh/my key.pem".to_string()));
    }

    #[test]
    fn comment_stripping_handles_hash_in_key_value() {
        let config = "Host=example.com # comment\n  IdentityFile=~/.ssh/id#2";
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts[0].identity_file, Some("~/.ssh/id#2".to_string()));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sigrun-core --lib ssh_config::tests`
Expected: FAIL

- [ ] **Step 3: Implement `ssh_config.rs` changes**

*   Modify the parsing logic in `crates/sigrun-core/src/ssh_config.rs` to correctly split key-value pairs on both spaces and the first equals sign (`=`).
*   Ensure comment stripping happens after tokenization.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sigrun-core --lib ssh_config::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/ssh_config.rs
git commit -m "feat(core): Support Key=Value syntax in ssh_config parser"
```

---

### Task 3: Implement Windows Path Quoting

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs`
- Test: `crates/sigrun-core/src/terminal.rs` (unit tests)

**Interfaces:**
- Consumes: None
- Produces: Updated Windows path quoting logic.

- [ ] **Step 1: Write failing unit tests for `terminal.rs`**

```rust
// crates/sigrun-core/src/terminal.rs
#[cfg(test)]
mod tests {
    // ... existing tests ...

    #[test]
    fn cmd_escape_quotes_path_with_spaces() {
        let path = "C:\\Users\\My User\\Documents\\file.txt";
        assert_eq!(cmd_escape(path), "\"C:\\Users\\My User\\Documents\\file.txt\"");
    }

    #[test]
    fn cmd_escape_escapes_internal_double_quotes() {
        let path = "C:\\Program Files\\My \"App\"\\app.exe";
        assert_eq!(cmd_escape(path), "\"C:\\Program Files\\My \"\"App\"\"\\app.exe\"");
    }

    #[test]
    fn cmd_escape_handles_empty_string() {
        assert_eq!(cmd_escape(""), "\"\"");
    }

    #[test]
    fn windows_launch_line_uses_cmd_escape_for_disk_key() {
        // Simulate a Windows launch with a disk key path containing spaces
        // Assert that the rendered command string uses cmd_escape for the path
    }

    #[test]
    fn print_only_renders_posix_compatible_output_on_windows() {
        // Simulate a print_only request on Windows
        // Assert that the output is POSIX-compatible
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sigrun-core --lib terminal::tests`
Expected: FAIL

- [ ] **Step 3: Implement `terminal.rs` changes**

*   Implement a `cmd_escape` utility function in `crates/sigrun-core/src/terminal.rs` that handles `cmd.exe` quoting rules (double quotes, doubling internal double quotes).
*   Modify the Windows launch logic in `crates/sigrun-core/src/terminal.rs` to use `cmd_escape` for all arguments and paths.
*   Ensure `print_only` still renders POSIX-compatible output.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sigrun-core --lib terminal::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/terminal.rs
git commit -m "feat(core): Implement cmd.exe-aware quoting for Windows paths"
```

---

### Task 4: Update Documentation and Changelog

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: None
- Produces: Updated documentation.

- [ ] **Step 1: Update `CHANGELOG.md`**

*   Add entries under `## [Unreleased]` for all changes in Phase 2a.

- [ ] **Step 2: Update `AGENTS.md`**

*   Update the i18n rule to reflect the correct number of locales (seven).

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md AGENTS.md
git commit -m "docs: Update changelog and AGENTS.md for Phase 2a"
```

