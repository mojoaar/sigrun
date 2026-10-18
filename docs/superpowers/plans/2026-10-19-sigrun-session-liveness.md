# Session Liveness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the GUI's "connected" dot clear when a session ends by any means — including a `SIGKILL` of the terminal's process group — by having the launched shell report its own pid and having the host watch it.

**Architecture:** The launched line sends one new IPC request, `SessionStarted`, before `ssh` runs; the host keeps `connection id → pid` in memory and, on the poll tick it already runs every 2 seconds, clears the connection's active flag for any pid that no longer exists.

**Tech Stack:** Rust 2021 (`sigrun-core`, `sigrun-cli`, Tauri 2 shell), existing IPC (newline-delimited JSON over a Unix socket), `libc` for the liveness check, React + TypeScript + Vitest for the GUI.

**Spec:** `docs/superpowers/specs/2026-10-19-sigrun-session-liveness.md`

## Global Constraints

- `~/.ssh` is read-only; the only permitted write anywhere is the explicit provisioning append to a remote `authorized_keys`.
- Secrets never appear in argv, an environment *value*, a log line, an error message, or the persisted model.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`.
- Rust 2021, MSRV 1.85; dependencies only via `[workspace.dependencies]`. `libc` is permitted here: it is already in the dependency graph transitively, so declaring it adds no compiled code.
- The gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`; plus, from `apps/desktop` when a frontend file changes: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- One commit per task with a conventional message; every behaviour change gets a `## [Unreleased]` Keep-a-Changelog entry.
- `SessionStarted` must **not** join the `last_vault_activity` `matches!` list — a session report must not defer the idle auto-lock.
- The new state is in memory only: nothing about a pid may reach `data.toml`.

## Review Focus

1. **A reap race.** The wrapper runs `<exe> … session-started <id> $$` as a *child* of the shell; if the host sweeps before that report arrives, an unncessary "session ended" is harmless, but a report that arrives *after* the sweep of an older pid must not resurrect a cleared session incorrectly — the implementation only ever sets on report and only ever clears on a dead pid, and the review must confirm no path clears a live pid or sets without a report.
2. **`exec ${SHELL:-/bin/sh}` keeps the pid.** The keep-open path must stay watched after `exec` replaces the process image, or a failed connect's shell would be treated as gone while its window is still open.
3. **Pid reuse.** A recycled pid within one 2-second tick must not hold a *cleared* connection active (it cannot, since the sweep only clears), and the reverse window is accepted and documented.
4. **The Windows path.** `cmd.exe /k` carries no report; every new code path must tolerate a connection that never reports a pid.
5. **A blocked report.** The report is an IPC call inside the user's terminal; it must be silent, must not print anything to the terminal on failure (the host may be gone), and must not delay the `ssh` launch noticeably.

---

### Task 1: The host watches a session's pid

**Files:**
- Modify: `crates/sigrun-core/src/ipc/protocol.rs` (add the request variant beside `ConnectionEnded`)
- Modify: `crates/sigrun-core/src/host.rs` (`HostState` field, two methods, the dispatch arm)
- Modify: `Cargo.toml` and `crates/sigrun-core/Cargo.toml` (`libc = "0.2"`)
- Test: `crates/sigrun-core/src/host.rs` (`mod tests`)

**Interfaces:**
- Consumes: nothing new; the existing `active` set and its `mark_active`/`clear_active`/`active_ids`.
- Produces: `Request::SessionStarted { id: String, pid: u32 }`; `HostState::report_session(&self, id: &str, pid: u32)`; `HostState::sweep_dead_sessions(&self) -> bool` (true when it changed the active set); `HostState::sessions: std::sync::Mutex<HashMap<String, u32>>`.

- [ ] **Step 1: Write the failing tests**

In `crates/sigrun-core/src/host.rs`'s test module, following the existing tempdir `state_in`/`state` helper and the `handle(...)` round-trip style:

```rust
#[test]
fn a_session_report_keeps_the_connection_active_while_its_pid_lives() {
    // Spawn a real child, report its pid, then sweep: still active.
}

#[test]
fn a_dead_pid_clears_the_connection_on_the_next_sweep() {
    // Spawn, report, kill and reap the child, then sweep -> active is gone.
}

#[test]
fn sweeping_with_no_sessions_reports_no_change() {}

#[test]
fn a_session_report_is_not_a_vault_touching_request() {
    // The idle clock must not move: mirror the existing comment/assertion style.
}
```

Use `std::process::Command::new("sleep").arg("30").spawn()` for the child and `child.kill()` + `child.wait()` to end it (reap it, or the pid lingers as a zombie and `kill(pid, 0)` still succeeds).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sigrun-core --lib -- session sweep`
Expected: compile error (`no variant SessionStarted`, no `report_session`/`sweep_dead_sessions`).

- [ ] **Step 3: Implement**

Add `SessionStarted { id: String, pid: u32 }` to `Request` with a doc comment saying the pid is the launched shell's, that the host watches it because a killed terminal group cannot report its own end, and that the field rides `rename_all = "snake_case"` for free. Place it next to `ConnectionEnded`.

In `HostState`: a `sessions: std::sync::Mutex<HashMap<String, u32>>` field (same poisoning-recovering accessor style as `active`), `report_session` inserting the pair, and:

```rust
/// Clears every connection whose watched shell is gone. True when the active
/// set changed, so the caller can emit.
pub fn sweep_dead_sessions(&self) -> bool
```

using `libc::kill(pid as i32, 0)`: treat `Ok` as alive, and `Err` with `raw_os_error() == Some(libc::ESRCH)` as gone. Document that a pid recycled inside the poll window is accepted. Add the dispatch arm `Request::SessionStarted { id, pid } => { state.report_session(&id, pid); Ok(Response::Ok) }`, **outside** the `last_vault_activity` `matches!` list, with a one-line comment saying why.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sigrun-core --lib -- session sweep`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
git add -A crates/sigrun-core Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "feat(core): watch a session's pid so a killed terminal still clears"
```

---

### Task 2: The launched line reports its pid

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs` (`SessionReports`, `launch_reports`, the launched line)
- Create: `crates/sigrun-core/src/session_started.rs` (the shared report sender, mirroring `connection_ended.rs`)
- Modify: `crates/sigrun-core/src/lib.rs` (register the module)
- Modify: `crates/sigrun-cli/src/cli.rs` (hidden `SessionStarted` subcommand + handler)
- Modify: `apps/desktop/src-tauri/src/main.rs` (the helper branch)
- Modify: `crates/sigrun-core/src/host.rs` (pass the reports)
- Test: `crates/sigrun-core/src/terminal.rs`, `crates/sigrun-cli/tests/cli.rs`, `apps/desktop/src-tauri/src/main.rs`

**Interfaces:**
- Consumes: `Request::SessionStarted` from Task 1.
- Produces: `pub struct SessionReports { pub started: String, pub ended: String }`; `pub fn launch_reports(platform: Platform, socket: &Path, connection_id: &str) -> Option<SessionReports>` (replacing `launch_notify`, `None` on Windows); `terminal_launch_specs(...)`'s fifth parameter becomes `reports: Option<&SessionReports>` — the same arity, so every call site that passes `None` is unchanged.

- [ ] **Step 1: Write the failing test**

In `terminal.rs`'s test module, extend the session-end test (or add a sibling) so it also pins the report that runs *before* the command:

```rust
// The pid is the shell's own (`$$`), so the host can watch it: a killed
// terminal group cannot report its own end.
assert!(line.contains("session-started c1 $$"), "{line}");
let started_at = line.find("session-started c1").expect("the pid is reported");
let ssh_at = line.find("ssh u@h").expect("ssh runs");
assert!(started_at < ssh_at, "{line}");
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sigrun-core --lib -- terminal::tests`
Expected: FAIL — the line has no `session-started`.

- [ ] **Step 3: Implement**

`SessionReports` with both argv fragments, built by `launch_reports` from the same `<exe> --socket <socket>` prefix `connection_ended_notify` already builds (factor the shared prefix rather than repeating it), the started report ending in `$$` expanded by the shell, the ended report unchanged. The launched line becomes:

```
trap '<ended>; exit' HUP TERM INT; <started>; {cmd}; rc=$?; {ended}; [ $rc -ne 0 ] && exec ${SHELL:-/bin/sh}; exit $rc
```

with the `None` arm byte-identical to today's `{cmd} || exec ${SHELL:-/bin/sh}`. `session_started.rs` mirrors `connection_ended.rs`: resolve the socket (explicit `--socket` → `SIGRUN_SOCKET` → `Paths`), send the request, swallow every error (the report runs inside the user's terminal, which may outlive the host), print nothing. Wire the hidden `session-started <id> <pid>` subcommand in `cli.rs` and the early-exit branch in `main.rs` (after the askpass branch).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sigrun-core --lib -- terminal` and `cargo test -p sigrun-cli`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
git add -A crates/sigrun-core crates/sigrun-cli apps/desktop/src-tauri CHANGELOG.md
git commit -m "feat(terminal): report the session's pid so the host can watch it"
```

---

### Task 3: The desktop sweeps dead sessions each tick

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs` (the poll loop)

**Interfaces:**
- Consumes: `HostState::sweep_dead_sessions` from Task 1.
- Produces: nothing; the existing `connections-changed` emit carries the result.

- [ ] **Step 1: Implement**

Call `st.sweep_dead_sessions();` at the top of each tick, before the `active_ids()` snapshot is computed, beside the existing `auto_locked` check, with a short comment on why (a killed terminal group cannot report its own end).

- [ ] **Step 2: Run the gate and commit**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
git add apps/desktop/src-tauri/src/lib.rs
git commit -m "fix(desktop): sweep dead sessions so the dot clears after a kill"
```

No test is added: the loop only exists inside a running Tauri app, and a hollow test was ruled worse than none. Task 4's docs carry the manual check.

---

### Task 4: Documentation

**Files:**
- Modify: `README.md` (the live-session dot's limitation entry)
- Modify: `AGENTS.md` (the platform rule and the `connection-ended` sentence)

**Interfaces:**
- Consumes: the shipped behaviour from Tasks 1–3.
- Produces: nothing.

- [ ] **Step 1: Rewrite the limitation**

The dot is cleared by the host watching the launched shell's process, so closing a window, closing a tab, force-quitting the terminal and crashing all clear it within about two seconds. What remains: a pid recycled within that window (rare, accepted), two sessions of one connection clearing when the last watched shell dies, and Windows, which reports nothing and therefore shows no dot.

- [ ] **Step 2: Run the gate and commit**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
git add README.md AGENTS.md CHANGELOG.md
git commit -m "docs: describe how the live dot clears"
```

---

## Self-Review

**Spec coverage.** Every design point in the spec has a task: the request and host state (Task 1), the report and the launched line (Task 2), the sweep on the poll tick (Task 3), the limitation text (Task 4). The spec's "not vault-touching", "nothing on disk" and "in memory only" constraints are each pinned by a Task 1 assertion or comment.

**Step scan.** No step says "handle edge cases" or names a symbol no task defines. The two places the plan deliberately leaves the implementer a choice — which `libc` call shape and whether the sweep lives in `HostState` or a free function — are both forced by the Interfaces blocks.

**Type consistency.** `report_session`/`sweep_dead_sessions` names match across Tasks 1 and 3; `SessionReports`/`launch_reports` match across Task 2's two files; the fifth parameter of `terminal_launch_specs` keeps its arity, so the eighteen existing call sites stay valid.

**Review Focus mapping.** (1) the reap race is Task 1's tests plus the Review-Focus note; (2) the `exec` pid continuity is Task 2's `$$` report (a pid is not re-issued by `exec`) and Task 1's sweep semantics; (3) pid reuse is Task 1's documented acceptance; (4) the Windows path is `launch_reports` returning `None` on `Platform::Windows`, asserted in Task 2; (5) the blocked/silent report is Task 2's swallow-every-error implementation, asserted by the CLI's silence tests.

**Risk flagged.** `libc` is a new *declared* dependency. It is already in the lock file via tokio, so nothing new compiles, but the spec records the reason and the plan states it as a Global Constraint rather than smuggling it in. If a stricter reading is preferred, `/bin/kill -0` needs no dependency but is not reliably on `PATH` on Linux — worth one line from the reviewer if they disagree.
