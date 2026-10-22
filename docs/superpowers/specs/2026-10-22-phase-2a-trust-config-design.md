# Phase 2a — Trust & Config Hardening Design

This document outlines the design for addressing the high-severity findings related to trust evaluation, SSH config parsing, and Windows path quoting, as part of the "Phase 2a: trust & config" plan. These changes aim to centralize security logic, improve robustness, and enhance cross-platform compatibility.

## Findings Addressed

*   **#8: TOFU trust gate lives in the Tauri layer; CLI connects skip it entirely.**
    *   **Problem:** The Host-Key Trust On First Use (TOFU) check, which verifies server host keys against Sigrun's internal `known_hosts` store, is implemented in the Tauri commands layer. The CLI, which interacts directly with the `sigrun-core` IPC server, bypasses this crucial security check entirely.
    *   **Solution:** Migrate the TOFU gate logic into `sigrun-core`'s `host.rs` (or a dedicated module it calls). The `ConnectionsConnect` IPC request will then inherently include this check.
*   **#9: Trust dialog data smuggled through an error string, re-parsed with `split(" ")`.**
    *   **Problem:** When a host key is unknown or mismatched, the `host_key_unknown` or `host_key_mismatch` error code is returned with fingerprint, algorithm, and key material concatenated into a single `String` message, requiring the frontend to re-parse it with `split(" ")`. This is brittle and prone to silent misparsing if the string layout changes.
    *   **Solution:** Define a structured payload for trust-related errors using `serde` enums/structs, ensuring type safety and explicit data transfer across IPC and to the frontend.
*   **#11: `~/.ssh/config` `Key=Value` syntax silently dropped — wrong port/user on discovered hosts.**
    *   **Problem:** OpenSSH configuration files allow `Key=Value` syntax (e.g., `Port=2222`). Sigrun's `ssh_config` parser currently only handles `Key Value` (space-separated) syntax, causing `Key=Value` entries to be ignored and leading to incorrect default ports or usernames for discovered hosts.
    *   **Solution:** Enhance the `ssh_config` parser to correctly interpret `Key=Value` pairs.
*   **#16: Disk-key paths with spaces break `connect` on Windows.**
    *   **Problem:** When launching SSH connections on Windows, disk-key paths (e.g., `C:\Users\My User\.ssh\id_rsa`) containing spaces are incorrectly quoted using POSIX single-quote escaping, which `cmd.exe` does not process as expected. This leads to connection failures.
    *   **Solution:** Implement a `cmd.exe`-aware quoting mechanism for Windows launch commands.

## Architectural Changes

1.  **Centralized TOFU Gate:**
    *   The `probe_host_key` and `evaluate_trust` functions (currently in `apps/desktop/src-tauri/src/commands.rs`) will be moved to a suitable location within `crates/sigrun-core/src/`. A new module, `crates/sigrun-core/src/trust.rs`, will be created to encapsulate this logic.
    *   The `Request::ConnectionsConnect` handler in `sigrun-core/src/host.rs` will call this centralized TOFU gate *before* attempting to launch the SSH command.
    *   The Tauri command layer (`apps/desktop/src-tauri/src/commands.rs`) will then simply forward the `ConnectionsConnect` request to the IPC server and interpret any trust-related errors returned by the core.
    *   **CLI Trust-Accept Flow:** For this phase, the CLI trust-accept flow (e.g., a `sigrun trust` subcommand) is **out of scope**. The CLI will report the structured error and suggest using `--force` (with appropriate warnings) or the desktop app to manage trust.
    *   **`force` Semantics:** After migration, `ConnectionsConnect { force: true }` will bypass both the TOFU gate and the reachability probe, consistent with the desktop's "Connect anyway" button. The CLI `--force` flag's warning message will be updated to reflect this.
    *   **Probe Ordering:** The TOFU gate will be executed *before* the existing TCP reachability probe. This ensures that security checks are prioritized.

2.  **Structured Trust Error Payload:**
    *   Modify `crates/sigrun-core/src/error.rs` to define a structured error type for host key issues, e.g., `Error::HostKeyUnknown { host: String, port: u16, fingerprint: String, algorithm: String, key_base64: String }` and `Error::HostKeyMismatch { ... }`. The `fingerprint` will be computed host-side.
    *   The IPC protocol (`crates/sigrun-core/src/ipc/protocol.rs`) will be updated to include an optional `payload` field in `Response::Error` to serialize these structured errors. This allows for future structured error types without adding new `Response` variants.
    *   The `ConnectionsConnect` handler will return these structured errors when the TOFU gate fails.
    *   The CLI and desktop frontend will be updated to deserialize and display these structured errors directly, eliminating `split(" ")` parsing.
    *   **Mismatch Details:** The `HostKeyMismatch` payload will include both the *stored* fingerprint and the *presented* fingerprint.
    *   **CLI Display:** The CLI will format the structured error payload into a human-readable message for the user, including the fingerprints and suggestions for resolution.
    *   **Tray Payload Shape:** The `connection-error` event in the tray will be updated to carry the structured payload.

3.  **SSH Config Parser Enhancement:**
    *   The `crates/sigrun-core/src/ssh_config.rs` module's parsing logic will be updated to split key-value pairs on both spaces and the first equals sign (`=`). This will handle `Port=2222`, `Port = 2222`, `Port= 2222`, and `IdentityFile=~/.ssh/a b` (quoted value with space).
    *   Comment stripping will occur after tokenization to correctly handle `#` within `Key=Value` values.
    *   `Host` lines with `=` will be tested to ensure correct pattern extraction.

4.  **Windows Path Quoting:**
    *   A new function, likely within `crates/sigrun-core/src/terminal.rs` or a utility module, will provide `cmd.exe`-compatible string quoting. This function will handle the entire rendered command string.
    *   The Windows-specific launch logic in `crates/sigrun-core/src/terminal.rs` will use this new quoting mechanism for paths and arguments.
    *   **`print_only` Behavior:** When `print_only` is true, the command will be rendered in a POSIX-compatible format regardless of the target platform, maximizing its utility for users copying to various shells.

## Implementation Plan (High-Level)

1.  **Spec:** Create this design document.
2.  **Plan:** Detail tasks for each finding, including tests and specific code changes.
3.  **Execution:**
    *   Implement TOFU gate migration to `sigrun-core` with structured errors.
    *   Update `ssh_config` parser.
    *   Implement Windows path quoting.
    *   Add comprehensive unit and integration tests for all changes.
4.  **Review:** Task-scoped reviews and a final whole-branch review.
5.  **Documentation:** Update `CHANGELOG.md` and relevant `AGENTS.md` sections.

## Testing Strategy

*   **Unit Tests:** For `ssh_config` parsing, `cmd_escape` utility, and the new TOFU gate logic in isolation.
*   **Integration Tests:** Crucially, tests that simulate `ConnectionsConnect` requests via IPC for both CLI and desktop contexts, verifying that the TOFU gate is applied consistently and structured errors are returned correctly. Test scenarios for unknown, mismatched, and known host keys.
*   **Frontend Tests:** Existing frontend tests mocking `host_key_unknown` messages will be updated to reflect the new structured payload.
*   **Windows-specific Tests:** Manual verification or automated tests on a Windows environment for path quoting.
*   **End-to-End Tests:** Existing end-to-end tests should continue to pass.

## Rollback Plan

Revert the commits associated with this phase.

