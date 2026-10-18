# AGENTS.md

## What this is

Sigrun is a cross-platform (macOS, Linux, Windows) SSH connection manager with key-based authentication.
It keeps private keys in an encrypted local vault, generates new keys,
discovers the ones already in `~/.ssh`, serves vault-backed keys to `ssh`
through an in-process OpenSSH agent, and launches connections.

Tagline: **Sigrun — victory over key chaos.** The name comes from Sigrún, a
valkyrie in Norse mythology; it breaks into *sigr* (victory) and *rún*
(secret / rune).

Sigrun is a public open-source project under the MIT license.

## Layout

| Path | What lives there |
| --- | --- |
| `crates/sigrun-core` | All domain logic: vault, keys, agent, connections, terminal launch, trust, provisioning, IPC. |
| `crates/sigrun-cli` | The `sigrun` binary. A thin client over the host's IPC. |
| `apps/desktop` | Tauri 2 app. React + TypeScript + Tailwind in `src/`; the Rust shell in `src-tauri/`. |
| `docs/superpowers/specs/` | Design documents. |
| `docs/superpowers/plans/` | Implementation plans. |
| `.github/workflows/` | CI (`ci.yml`) and installer releases (`release.yml`). |
| `CHANGELOG.md` | Keep-a-Changelog history. |

## Architecture

The desktop app **is** the long-lived host. It owns the vault, the SSH agent and
the IPC server, enforces a single instance, and minimises to the tray on window
close. The CLI is a client that talks to that host over a Unix socket (a
Windows named pipe on Windows). All logic lives in `sigrun-core` — never in the
Tauri layer, which only marshals commands.

IPC is newline-delimited JSON. `Request` is internally tagged
(`{ "type": "connections_list", … }`); `Response` is adjacently tagged
(`{ "type": …, "value": … }`) because internally tagged enums cannot serialize
newtype variants holding a sequence.

## Security rules (non-negotiable)

- **`~/.ssh` is read-only.** Sigrun reads it and passes `-i` paths from it. The
  single explicit exception is appending to a remote `authorized_keys` during an
  explicit provision.
- **The vault** uses Argon2id (m=65536, t=3, p=4, 16-byte salt) with
  XChaCha20-Poly1305 and a fresh 24-byte nonce per seal. Writes are atomic at
  `0600` inside a `0700` directory.
- **Secrets live only in the vault.** Never in argv, a command line, an
  environment *value*, a log line, an error message or the persisted model.
  `Secret` has no `Clone` and a redacting `Debug`.
- **Host-key trust is TOFU** against Sigrun's own `known_hosts`, and fails
  closed on an unknown or mismatched key.
- **The askpass handoff** uses `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force` with
  the helper marker `SIGRUN_ASKPASS=1` **and** a non-empty
  `SIGRUN_CONNECTION_ID`. The password is never put on a command line. This is
  macOS and Linux only: the Windows launch path carries no askpass variables,
  so a stored password falls back to `ssh`'s own prompt there.

## Code rules

- Rust 2021, MSRV 1.85. Dependencies only via `[workspace.dependencies]`.
- **The gate** is `cargo fmt --all -- --check && cargo clippy --workspace
  --all-targets --all-features -- -D warnings && cargo test --all-targets`.
- When frontend files change, also run `npm test -- --run && npm run typecheck
  && npm run lint && npm run build` in `apps/desktop`. When workflows change,
  run `actionlint`.
- Every user-facing string goes through `t(...)` with its key in **both**
  `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard
  enforces it.
- Tests assert real behaviour. Frontend tests use the built-in matchers and
  `within(...)` to disambiguate — `@testing-library/jest-dom` is deliberately
  absent.
- No new dependency without a strong reason.

## Platform rules

- Windows named-pipe names derive from the user **and** a hash of the data dir,
  so two hosts with different data dirs never share a pipe. Tests that bind a
  socket pick a unique per-test name.
- Terminal launch is per platform: AppleScript for Terminal/iTerm, `-e` for
  Ghostty/kitty/Alacritty/WezTerm, `sh -c` on Linux, `cmd.exe /k` on Windows.
- On macOS and Linux the launched command reports its own end to the host when
  `ssh` exits (`connection-ended`) and then keeps the window open on failure
  with the same `|| exec ${SHELL:-/bin/sh}` chain, so the error stays on screen.
  Windows keeps the bare command with `cmd.exe /k`, which has nowhere to chain
  the report: its sessions are not tracked and no live dot appears.

## Process rules

- Design first (`docs/superpowers/specs/`), then a plan
  (`docs/superpowers/plans/`), then subagent-driven execution with a ledger:
  a fresh implementer per task, a task reviewer after each, a fix loop of at
  most five rounds, and a whole-branch review at the end.
- One commit per task, with a conventional message. Write the test first.
- Claim nothing without running the gate and quoting its output. The controller
  never fixes review findings itself.

## Release rules

- The version lives in exactly two files: `Cargo.toml` and
  `apps/desktop/package.json`. CI's `version-sync` job keeps them in lockstep
  and, on a `v*` tag, also requires a `## [<tag>]` heading in `CHANGELOG.md`.
  `Cargo.lock` records the version too, but it is regenerated, never
  hand-edited.
- Every change gets a Keep-a-Changelog entry under `## [Unreleased]`.
- Tags matching `v*` build and publish the installers.
- The first tag will be `v0.1.0`. Never tag from a dirty tree.

## Git rules

- Branch per plan, named `plan-<n>-<topic>`, merged with `--no-ff`.
- Never push, force-push or delete a remote branch without asking.
