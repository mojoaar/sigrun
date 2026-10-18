# Active Connections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a pulsing dot on a connection row while the `ssh` session Sigrun launched for it is still running.

**Architecture:** The launched shell line reports the end of the session to the host (`<exe> --socket <addr> connection-ended <id>`), because a fire-and-forget launch otherwise leaves the GUI guessing. The host keeps a transient in-memory `HashSet<String>` of active connection ids, exposes it as a host-filled `Connection.active` flag exactly as it already exposes `has_password`, and the desktop's 2-second poll loop emits a `connections-changed` event when that set changes; the GUI refetches and renders the dot from the flag.

**Tech Stack:** Rust 2021 (`sigrun-core`, `sigrun-cli`, Tauri 2 shell), React 19 + TypeScript + Tailwind v4, i18next (en + da).

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` (§6.1 connection manager, §7.6 no secret on a command line, §8 UI and i18n).

## Global Constraints

- `~/.ssh` is read-only. The only write is the explicit provisioning append to a remote `authorized_keys`.
- No secret in argv, an environment *value*, a log line, an error message, or the persisted model. `Secret` has no `Clone` and a redacting `Debug`.
- Every user-facing string goes through `t(...)`, with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; the recursive guard in `apps/desktop/src/i18n/index.test.ts` enforces parity.
- The gate, run before every `git commit`: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. When any file under `apps/desktop/src` changes, also run, from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- One commit per task, conventional message. Each task that changes behaviour appends its own one-line `## [Unreleased]` entry to `CHANGELOG.md` in the same commit; the README and AGENTS prose lands in Task 7.
- Rust 2021, MSRV 1.85. No new dependency.

## Review Focus

1. A second connect to a connection that is already active: the dot must stay on, and the first session ending must not clear it while the second still runs. Accepted limitation: the flag is a set of ids, so two sessions of one connection cannot be told apart — the dot clears when *either* ends.
2. A `connection-ended` id that is unknown or not active (a stale tab, a host restart, a typo): a silent no-op, never an error in the user's terminal and never an app-level error banner.
3. `data.toml` carrying `active = true` from a hand edit or a third-party tool: the host ignores it and reports its own set.
4. Vault lock while a session is running: the dot clears, and a later unlock does not bring it back.
5. The notify runs inside the user's terminal: it prints nothing, and it does not disturb the exit path or the keep-open behaviour when `ssh` fails.

---

### Task 1: `Connection.active`, host-filled and never persisted

**Files:**
- Modify: `crates/sigrun-core/src/model.rs` (the `Connection` struct, `Connection::from_ssh_host`, `Connection::managed`)
- Modify: `crates/sigrun-core/src/host.rs` (`ConnectionsAdd`, `ConnectionsUpdate`)
- Modify: `crates/sigrun-cli/src/cli.rs` (the `ConnectionsCommand::Add` connection literal)
- Modify: `apps/desktop/src-tauri/src/tray.rs` (its `Connection` test literal)
- Modify: `apps/desktop/src/api.ts` (the `Connection` type)
- Modify: `apps/desktop/src/views/Connections.tsx` (the `Form` literal)
- Test: `crates/sigrun-core/src/host.rs`, `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: nothing.
- Produces: `Connection.active: bool` — `#[serde(default)]`, host-filled on read, ignored on write, exact mirror of `has_password`. Task 2 fills it, Task 6 renders it. This field is required (not optional) in the TypeScript `Connection` type, so every literal and fixture must carry it.

- [ ] **Step 1: Write the failing tests**

In `crates/sigrun-core/src/host.rs`, next to `a_client_supplied_password_flag_is_not_persisted`, add a twin that mirrors it exactly, one test for add and one for update: a connection sent over the wire with `active: true` comes back from `ConnectionsList` with `active: false`.

```rust
#[tokio::test]
async fn a_client_supplied_active_flag_is_not_persisted() { /* mirror the password twin */ }
```

In `apps/desktop/src/views/Connections.test.tsx`, add `active: false` (or `true` where the row under test is active) to every `Connection` literal and to the `listConnections` mock, and add one assertion that the `active` field rides through the mocked row unchanged — the compiler is the real test here, so a `typecheck` failure before the edit is the expected first signal.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sigrun-core active` — expected: compile error, `no field active on type Connection`.
Run: `cd apps/desktop && npm run typecheck` — expected: errors naming each `Connection` literal that lacks `active`.

- [ ] **Step 3: Implement the field**

In `model.rs`, add to `Connection` directly after `has_password`, copying that doc comment's shape:

```rust
/// Host-filled: whether a launched session for this connection is still
/// running. Ignored on write, recomputed per response, never durable.
#[serde(default)]
pub active: bool,
```

Set `active: false` in `Connection::from_ssh_host` and `Connection::managed`, and add `active: false` to the two remaining non-test literals (`cli.rs`, `tray.rs`), the TypeScript type in `api.ts`, and the `Form` literal in `Connections.tsx`. In `host.rs`, set `connection.active = false;` beside each existing `connection.has_password = false;` in `ConnectionsAdd` and `ConnectionsUpdate`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p sigrun-core active` — expected: PASS.
Then the full gate from Global Constraints, plus the four frontend commands.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add a host-filled active flag to connections"
```

Append to `CHANGELOG.md` under `## [Unreleased]` → `### Added`: `- A host-filled `active` flag on connections, for showing which sessions are live.`

---

### Task 2: The host's active set and the `ConnectionEnded` request

**Files:**
- Modify: `crates/sigrun-core/src/protocol.rs` (`Request`)
- Modify: `crates/sigrun-core/src/host.rs` (`HostState`, its set helpers, the `handle_inner` arms for `ConnectionsList`, `VaultLock` and `ConnectionEnded`, and a `fill_active_flags` pass)
- Test: `crates/sigrun-core/src/host.rs`

**Interfaces:**
- Consumes: `Connection.active` from Task 1.
- Produces, all on `HostState`, for Tasks 3–5: `active: std::sync::Mutex<std::collections::HashSet<String>>` (public field, beside the existing `auto_locked`), `fn mark_active(&self, id: &str)`, `fn clear_active(&self, id: &str)`, `fn clear_all_active(&self)`, `fn is_active(&self, id: &str) -> bool`, `fn active_ids(&self) -> Vec<String>` (sorted, so callers can compare snapshots with `!=`). Also `Request::ConnectionEnded { id: String }` on the wire.

- [ ] **Step 1: Write the failing tests**

In `host.rs`'s test module:

```rust
#[tokio::test]
async fn listing_connections_reports_the_host_active_set_not_the_client() // mark "c1" via state.mark_active, list, assert c1 active and c2 not
#[tokio::test]
async fn connection_ended_clears_an_active_connection()                  // handle(ConnectionEnded{"c1"}), assert !is_active("c1") and Response::Ok
#[tokio::test]
async fn connection_ended_for_an_unknown_id_is_a_no_op()                 // handle(ConnectionEnded{"nope"}), assert Ok and the set untouched
#[tokio::test]
async fn locking_the_vault_clears_the_active_set()                       // mark, handle(VaultLock), assert active_ids().is_empty()
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sigrun-core connection_ended` — expected: compile error, `no variant ConnectionEnded`.

- [ ] **Step 3: Implement**

Add the field to `HostState` and initialise it in `HostState::new`. The five helpers take the lock with `self.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner())` (a poisoned set must not turn a GUI refresh into a panic) and `active_ids` returns the sorted id list.

Add the request variant in `protocol.rs`, documented as the host's own launch line reporting a session end:

```rust
/// A launched session ended. Sent by the shell line Sigrun launched, never
/// by a user; an unknown id is a no-op.
ConnectionEnded { id: String },
```

In `handle_inner`, add the arm `Request::ConnectionEnded { id } => { state.clear_active(&id); Ok(Response::Ok) }` — and **leave it out** of the `matches!` list that bumps `last_vault_activity`: it does not touch the vault, and a session ending must not defer the idle auto-lock. In the `ConnectionsList` arm, call a new `fn fill_active_flags(state: &HostState, connections: &mut [Connection])` right after `fill_password_flags`, setting `connection.active = state.is_active(&connection.id)`. In the `VaultLock` arm, call `state.clear_all_active()` beside `state.agent.clear()`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p sigrun-core` — expected: PASS. Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): track active connections and report their end"
```

CHANGELOG → `### Added`: `- Transient host-side tracking of which connections have a live session, exposed as the `active` flag.`

---

### Task 3: The launched line reports the session end

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs` (a `helper_exe` helper, `env_prefix`, a new `connection_ended_notify`, `terminal_launch_specs`)
- Modify: `crates/sigrun-core/src/host.rs` (`connections_connect`)
- Test: `crates/sigrun-core/src/terminal.rs`, `crates/sigrun-core/src/host.rs`

**Interfaces:**
- Consumes: `HostState::mark_active` (Task 2); `Request::ConnectionEnded`'s CLI form (Task 4).
- Produces: `pub fn connection_ended_notify(socket: &Path, connection_id: &str) -> String`, returning the exact shell fragment Task 4's binaries must accept: `<exe> --socket <socket> connection-ended <id>`, every value shell-escaped. `terminal_launch_specs` gains a fifth parameter `notify: Option<&str>`.

- [ ] **Step 1: Write the failing tests**

In `terminal.rs`:

```rust
#[test]
fn the_launch_line_reports_the_session_end_to_the_host()
    // terminal_launch_specs(MacOs, None, &argv, "", Some("sigrun --socket /tmp/h.sock connection-ended c1"))
    // assert the line contains "connection-ended c1", that "rc=$?" precedes it,
    // and that "[ $rc -ne 0 ] && exec ${SHELL:-/bin/sh}; exit $rc" follows it.

#[test]
fn a_launch_line_without_a_notify_keeps_the_plain_keep_open_suffix()
    // notify = None still yields the exact "|| exec ${SHELL:-/bin/sh}" line.

#[test]
fn the_notify_escapes_the_socket_and_the_id()
    // connection_ended_notify(Path::new("/tmp/a b.sock"), "c 1") quotes both.
```

Update the two existing verbatim-suffix tests: `launches_keep_the_window_open_when_the_connection_fails` and `the_launch_line_carries_the_askpass_env_and_survives_every_terminal` — pass `None` for `notify` and keep asserting the old `|| exec ${SHELL:-/bin/sh}` shape, then add one case with `Some(notify)` asserting `SSH_ASKPASS=` still comes first and the keep-open chain still ends the line.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sigrun-core the_launch_line` — expected: compile error, `this function takes 4 arguments but 5 were supplied`.

- [ ] **Step 3: Implement**

Factor the existing `current_exe` lookup out of `env_prefix` into `fn helper_exe() -> String` (same fallback to `"sigrun"`) and use it in both places. Add `connection_ended_notify` beside `env_prefix`, with a doc line saying it points back at the running binary so the launched shell can report its own session and that no secret is involved. Replace the keep-open line in `terminal_launch_specs` with:

```rust
let keep_open = match notify {
    // The notify must run before the shell takes over, and `rc` keeps the
    // failure path exactly as it was: drop into a shell so the error stays
    // on screen instead of the window closing.
    Some(notify) => format!("{cmd}; rc=$?; {notify}; [ $rc -ne 0 ] && exec ${{SHELL:-/bin/sh}}; exit $rc"),
    None => format!("{cmd} || exec ${{SHELL:-/bin/sh}}"),
};
```

Every existing test call site gains a `None` (or the notify under test) as the fifth argument; the Windows arm ignores `notify` and keeps its bare command.

In `host.rs`'s `connections_connect`, build the notify and mark the session active after a successful launch:

```rust
// Windows keeps `cmd.exe /k` with no way to chain a notify, so the flag is
// only set where the launched line can clear it again.
let notify = (Platform::current() != Platform::Windows)
    .then(|| connection_ended_notify(Path::new(&state.ipc_addr), &conn.id));
let specs = terminal_launch_specs(Platform::current(), settings.terminal.as_deref(), &argv, &prefix, notify.as_deref())?;
launch_first(&specs, Some(&state.paths.agent_socket()))?;
if notify.is_some() {
    state.mark_active(&conn.id);
}
Ok(Response::Ok)
```

Add a host test `the_notify_is_built_only_where_the_launched_line_can_report_it` covering the pure decision (a `notify` of `None` on Windows, `Some` elsewhere) — the connect path itself launches a real terminal, so the launch-and-mark linkage is verified by the live check in Step 5, not by spawning a terminal in CI.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p sigrun-core` — expected: PASS. Then the full gate.

- [ ] **Step 5: Commit, then verify live**

```bash
git add -A
git commit -m "feat(terminal): report the session end from the launched command"
```

CHANGELOG → `### Added`: `- The launched command tells the host when a session ends, so the GUI can show which connections are live.`

Live check on this machine (needs the app, not CI): connect from the GUI to a reachable host, confirm `sigrun status` still works and the row shows a dot once Task 6 lands; close the `ssh` session and confirm the host reports it inactive (`sigrun connections list` — or watch the desktop log) within a second or two.

---

### Task 4: The `connection-ended` helper mode in both binaries

**Files:**
- Modify: `crates/sigrun-cli/src/cli.rs` (`Command`, the `run` dispatch, a handler)
- Modify: `apps/desktop/src-tauri/src/main.rs` (detection, argument parsing, a handler)
- Test: `crates/sigrun-cli/tests/cli.rs`, `apps/desktop/src-tauri/src/main.rs`

**Interfaces:**
- Consumes: `Request::ConnectionEnded` (Task 2); the exact fragment from `connection_ended_notify` (Task 3).
- Produces: both binaries accept `--socket <path> connection-ended <id>` (socket optional, defaulting to `Paths::resolve().socket_path()`) and exit 0 silently, whether or not the host answers.

- [ ] **Step 1: Write the failing tests**

In `crates/sigrun-cli/tests/cli.rs` (the existing integration suite — copy the style of the nearest argument-parsing test there), assert that `connection-ended c1` parses to `Command::ConnectionEnded { id: "c1" }` and that the subcommand is hidden from `sigrun --help`.

In `apps/desktop/src-tauri/src/main.rs`, add a `#[cfg(test)] mod tests` for the pure parser:

```rust
fn connection_ended_args(args: &[String]) -> Option<(Option<PathBuf>, String)>
// ["sigrun-desktop", "--socket", "/tmp/h.sock", "connection-ended", "c1"]
//   => Some((Some("/tmp/h.sock"), "c1"))
// ["sigrun-desktop", "connection-ended", "c1"] => Some((None, "c1"))
// ["sigrun-desktop", "--socket", "/tmp/h.sock", "connection-ended"] => None (no id)
// ["sigrun-desktop"] and ["sigrun-desktop", "--foo"] => None
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sigrun-cli connection_ended` — expected: FAIL, unknown subcommand.
Run: `cargo test -p sigrun-desktop connection_ended_args` — expected: compile error. (Use the crate name from `apps/desktop/src-tauri/Cargo.toml`.)

- [ ] **Step 3: Implement**

`cli.rs`: add the variant with the same hidden-helper shape as `Askpass`:

```rust
/// Report that a launched session ended. Internal.
#[command(hide = true)]
ConnectionEnded {
    /// The connection whose session ended.
    id: String,
},
```

Dispatch it to a handler that resolves the address with the existing `resolve_addr` and sends the request, discarding the outcome:

```rust
// Runs inside the user's terminal, so it never prints and never fails: the
// host may already be gone, and that is not the user's problem.
let _ = client::request(&addr, &Request::ConnectionEnded { id }).await;
Ok(())
```

`main.rs`: handle the shape before `sigrun_desktop::run()` (right beside the existing askpass branch, for the same reason — no window, tray or host may start): if `connection_ended_args` matches, resolve the socket the same way `askpass()` does and `tauri::async_runtime::block_on(sigrun_core::ipc::client::request(...))`, discarding the result, then `std::process::exit(0)`. Both binaries must swallow the error: the CLI's `client::request` turns `Response::Error` into an `anyhow` error, so bind it with `let _ =`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p sigrun-cli && cargo test -p sigrun-desktop` — expected: PASS. Then the full gate.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: report a connection end from the CLI and the desktop binary"
```

CHANGELOG → `### Added`: `- A hidden `connection-ended` helper mode in both binaries, so a launched session can report its own end.`

---

### Task 5: The desktop emits `connections-changed`

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs` (the existing 2-second poll loop)

**Interfaces:**
- Consumes: `HostState::active_ids` (Task 2).
- Produces: the Tauri event `"connections-changed"`, emitted with no payload whenever the active set differs from the previous tick; Task 6 listens for it.

- [ ] **Step 1: Implement**

In the poll loop, beside the existing `last_unlocked` snapshot, add a `last_active = st.active_ids()` seeded before the loop and, each tick, `if last_active != st.active_ids() { let _ = handle.emit("connections-changed", ()); last_active = st.active_ids(); }`. `active_ids` is sorted precisely so this comparison is stable.

The poll loop is existing, untested glue with no unit test today; the raise and clear latency (≤ 2 s) is what Step 2 checks.

- [ ] **Step 2: Verify build and gate**

Run from `apps/desktop`: `npm run build`, then the full gate. Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(desktop): emit connections-changed when the active set changes"
```

---

### Task 6: The pulsing dot and the event listener

**Files:**
- Modify: `apps/desktop/src/components/ConnectionRow.tsx`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `Connection.active` (Task 1 and 2), the `"connections-changed"` event (Task 5).
- Produces: nothing other code consumes.

- [ ] **Step 1: Write the failing tests**

In `Connections.test.tsx`, mock `@tauri-apps/api/event` (`listen: vi.fn().mockResolvedValue(() => {})`, the same shape as `App.test.tsx`) and add:

```tsx
it("shows a connected dot on a live row and hides it otherwise")
  // listConnections returns one row with active: true and one with false;
  // expect within(row("live")).getByRole("img", { name: "Connected" }) and
  // expect within(row("idle")).queryByRole("img", { name: "Connected" }) to be null
it("refetches when the host reports a changed active set")
  // fire the registered "connections-changed" handler inside act(...)
  // (grab it from vi.mocked(listen).mock.calls, filtered by event name)
  // and expect listConnections to have been called again
```

- [ ] **Step 2: Run to verify it fails**

Run from `apps/desktop`: `npm test -- --run Connections` — expected: FAIL, no element with role img named "Connected".

- [ ] **Step 3: Implement**

`ConnectionRow.tsx`, as the first child of the label flex div, so the dot leads the label and the chips:

```tsx
{conn.active && (
  <span
    role="img"
    aria-label={t("connections.active")}
    className="h-2 w-2 rounded-full bg-accent animate-pulse motion-reduce:animate-none"
  />
)}
```

`Connections.tsx`: in the same effect that owns `refresh`, subscribe with the `gone` guard and the `.catch(() => {})` shape `App.tsx` already uses, and return the unlistener from cleanup:

```tsx
listen("connections-changed", () => refresh()).then((f) => { if (gone) f(); else unChanged = f; }).catch(() => {});
```

so the dot clears within a tick of the session ending. In `doConnect`, refresh on success so the dot appears on the click rather than up to 2 s later: keep the existing `.catch(...)` chain and insert `.then(() => { refresh(); })` before it.

i18n: add `"active": "Connected"` to `connections` in `en.json` and `"active": "Forbundet"` in `da.json` — the completeness guard fails if only one lands.

- [ ] **Step 4: Run the tests and the gate**

Run from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. Then `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(desktop): show a pulsing dot on connected rows"
```

CHANGELOG → `### Added`: `- A pulsing dot on a connection row while its session is running.`

---

### Task 7: Docs

**Files:**
- Modify: `README.md`, `AGENTS.md`, `CHANGELOG.md`

- [ ] **Step 1: Update the prose**

`AGENTS.md` platform rules currently say every macOS/Linux launch ends with `|| exec ${SHELL:-/bin/sh}`; that is now the no-notify shape. Reword to: the launched command reports its own end to the host and then keeps the window open on failure with the same `exec` chain, and add one sentence that Windows keeps the bare `cmd.exe /k` command, so a session there is not tracked and the dot does not appear.

`README.md`: in the feature list (the "Connect anywhere" bullet) or beside it, one sentence that a live session shows a pulsing dot in the app, and that on Windows sessions are not tracked.

- [ ] **Step 2: Verify and commit**

Run the full gate one last time. Then:

```bash
git add -A
git commit -m "docs: document the active-connection dot"
```

CHANGELOG → `### Changed`: `- Documented that live sessions show a dot on macOS and Linux only; the Windows launch path cannot report a session end.`

---

## Self-Review

**Spec coverage.** §6.1's connection manager gains observable session state without changing how `ssh` is launched. §7.6 is untouched: the notify carries a connection id and the socket path, no secret, and it only ever appears where `env_prefix` already appears. §8: the dot has an accessible name, `motion-reduce:animate-none` honours reduced motion, and its label passes through `t(...)` in both locales.

**Step scan.** Every step names one action and one checkable result. Two steps contain code the implementer could not otherwise determine (the notify line's exact shape, the launch-and-mark ordering) and are verbatim for that reason; everything else is a signature plus values.

**Type consistency.** `mark_active`/`clear_active`/`clear_all_active`/`is_active`/`active_ids` are declared once in Task 2's Interfaces block and used with those names in Tasks 3 and 5. `connection_ended_notify(socket, connection_id)` is declared in Task 3 and its output shape is what Task 4 parses. `Connection.active` is one field name across Rust, the wire, and TypeScript.

**Review Focus mapping.** (1) is covered by Task 2's set semantics and asserted indirectly by `connection_ended_clears_an_active_connection`; the multi-session limit is documented rather than tested, because the flag cannot express it. (2) is `connection_ended_for_an_unknown_id_is_a_no_op` and Task 4's swallow-the-error rule. (3) is `a_client_supplied_active_flag_is_not_persisted` plus `listing_connections_reports_the_host_active_set_not_the_client`. (4) is `locking_the_vault_clears_the_active_set`. (5) is Task 3's keep-open assertions plus the live check.

**Proportion.** Seven tasks, one file or interface per task; the launch-line string is the only verbatim body, and it is the safety-critical one.

**Risk.** The one piece of shipped behaviour no test here can pin is `connections_connect` actually calling `mark_active` after a successful launch — a real launch opens a terminal, so CI must not do it. Task 3 Step 5's live check plus the whole-branch review cover it, and a regression there shows up as a dot that never appears.
