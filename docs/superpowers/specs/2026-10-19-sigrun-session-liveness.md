# Session liveness for the connected dot

Date: 2026-10-19

## Problem

The GUI's "connected" dot is cleared by a notify the launched shell runs after
`ssh` exits. That notify is a statement inside the launched line, so it only
runs if that shell survives long enough to reach it.

Measured behaviour on macOS with Ghostty 1.3.1:

- **Closing a window** sends `SIGHUP`, which the wrapper's `trap` catches, so the
  notify runs and the dot clears.
- **Closing a tab** leaves no orphaned shell or `ssh` process (`ps` shows
  nothing), and the dot stays lit — the whole process group is killed outright,
  so no handler inside that shell can run.
- A force-quit or a crash cannot be caught either.

`SIGKILL` is not catchable, so no arrangement of traps, watchers or subshells
inside the launched shell can report the end. The signal has to come from
outside the dying process group.

## Design

The launched shell reports **its own pid** to the host once, before `ssh` runs.
The host remembers `connection id → pid` and, in the poll it already runs every
2 seconds, treats a pid that no longer exists as a finished session.

Why the shell's pid is the right signal:

- The shell **is** the session's lifetime: the terminal creates it for the
  surface and it lives exactly as long as that surface, whether or not `ssh`
  survives.
- `exec ${SHELL:-/bin/sh}` on the failure path replaces the process image and
  keeps the same pid, so the keep-open shell is still watched correctly.
- The host already ticks every 2 seconds to emit `connections-changed`, so the
  sweep costs no new machinery.

### Interface

- New IPC request `Request::SessionStarted { id: String, pid: u32 }`, sent by the
  launched line as `<exe> --socket <addr> session-started <id> $$`.
- `HostState` gains `sessions: std::sync::Mutex<HashMap<String, u32>>` beside
  `active`, with `report_session(&self, id, pid)` and
  `sweep_dead_sessions(&self) -> bool` (true when it cleared anything).
- `sweep_dead_sessions` removes every entry whose pid is gone and clears that
  connection from `active`. It is called at the top of each poll tick, before the
  `active_ids()` snapshot, so a dead session reaches the UI on the same tick.
- Liveness is `libc::kill(pid as i32, 0)`: `Ok(0)` means the process exists (or
  exists but is not ours, which for the same uid cannot happen), `Err(ESRCH)`
  means it is gone. `libc` is already in the dependency graph (tokio depends on
  it), so declaring it directly adds no compiled code.

The request is **not** a vault-touching request: it must not reset the idle
auto-lock clock.

## Edge cases

- **Pid reuse.** A pid could be recycled between the death and the next 2-second
  tick, leaving the dot lit until that unrelated process exits. The window is a
  couple of seconds; accepted and documented rather than mitigated.
- **Two sessions of one connection.** `ConnectionEnded` from either session
  clears `active` at once, and the sweep tracks only the pid of the last
  `SessionStarted`. The dot therefore clears as soon as *either* session ends,
  and can go out while its sibling is still live. That is what the notify
  already did, so the behaviour is unchanged.
- **Stale pid from a previous host run.** The map is in memory only, so a restart
  starts empty.
- **Windows.** The `cmd.exe /k` launch path carries no report, so Windows is
  unchanged: no dot.
- **A failed connect.** The notify fires when `ssh` exits, and the sweep also
  fires when the keep-open shell finally exits; both only ever *clear*, so they
  cannot disagree.

## Alternatives considered

- **Trapping more signals.** `SIGKILL` is uncatchable; measured above.
- **A watcher process started by the wrapper.** It lives in the same process
  group, so a group kill takes it too.
- **Detaching the watcher into its own session.** No `setsid(1)` on macOS, and no
  POSIX shell facility for it; would need a new dependency.
- **Watching the agent socket or a heartbeat.** More moving parts for the same
  answer, and neither notices a killed shell sooner.
