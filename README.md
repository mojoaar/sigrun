# Sigrun

Sigrun — victory over key chaos.

The name comes from Sigrún, a valkyrie in Norse mythology. It breaks into
*sigr* — "victory" — and *rún* — "secret" or "rune". A "secret victory" felt
right for a tool whose whole job is secrets: the same *rún* that runs through
cryptography and keys. Pronounced SIG-roon.

Sigrun is a cross-platform SSH key and connection manager. It keeps your private
keys in an encrypted local vault, generates new keys, discovers the ones already
in `~/.ssh`, serves vault-backed keys to `ssh` through an in-process OpenSSH
agent, and launches connections — replacing scattered `~/.ssh` bookkeeping with
one tool that works the same on macOS, Linux, and Windows.

It has three faces that share one vault: a **desktop app** (window + system
tray), a **CLI** (`sigrun`), and a long-lived host that owns the vault, the
agent, and the local IPC they both talk to.

## Requirements

- Rust 1.85 or newer (plus Node 22+ to build the desktop app)
- macOS, Linux, or Windows

## The desktop app

The app is the long-lived host: it owns the vault, agent, and IPC, runs as a
single instance, and closing the window minimizes it to the tray — quitting is
explicit, from the tray menu.

```bash
cd apps/desktop && npm run tauri dev
```

On first run it asks you to create the vault (choose a master passphrase). From
there you can manage connections and keys, and connect — which opens your
terminal with `ssh` running.

- **Tray:** a connection menu to connect directly, plus Open and Quit.
  Left-clicking the tray icon shows or hides the window.
- **Themes:** four Norse themes (nordlys, fjord, ravn, skumring), each in light
  and dark, following the OS by default with an override.
- **i18n:** English and Danish.
- **Settings:** appearance, language, terminal, "start minimized to tray", and
  an idle **auto-lock** for the vault (default 30 minutes; 0 disables it).

The **terminal** for connect is auto-detected: Settings lists the launchable
terminals actually installed (Terminal, iTerm, Ghostty, kitty, Alacritty,
WezTerm on macOS; the known terminals on Linux and Windows), or "System
default" to use the OS fallback order.

## Install

Build from source:

```bash
cargo build --release
```

The `sigrun` binary lands at `target/release/sigrun`. Installers (macOS, Linux,
Windows) are produced by pushing a `v*` tag.

## Quick start (CLI)

The CLI talks to the running host (the desktop app, or a headless `sigrun
host`). To run headless, start the host in its own terminal — it owns the
vault, the agent, and the IPC socket every other `sigrun` command talks to.

```bash
# Terminal 1 — the long-lived host (leave it running)
sigrun host

# Terminal 2 — configure and connect
sigrun vault init                          # choose a passphrase
sigrun vault unlock
sigrun keys generate --name work --algorithm ed25519
sigrun keys import ~/.ssh/id_ed25519 --name personal
sigrun connections add --label web --host example.com --user deploy --key <key-id>
sigrun connections connect web --print     # prints the ssh command
sigrun connections connect web             # opens ssh in your terminal
sigrun status
sigrun vault lock
```

`--key` takes a key's id, not its `--name`; list ids with `sigrun keys list
--json`. Generated and imported keys get a UUID id, disk-discovered keys use
their fingerprint. Leave `--key` off to let the agent and your own
`~/.ssh/config` choose the key.

## How the agent works

`sigrun host` runs an OpenSSH-compatible agent on a private socket (`agent.sock`
in Sigrun's data directory). When the vault is unlocked, every vault-backed key
is loaded into the agent and held in memory only — the plaintext never touches
disk.

`sigrun connections connect` sets `SSH_AUTH_SOCK` to that socket on the terminal
it launches, so `ssh` asks the agent to sign instead of reading a key file.
Disk-backed keys found in `~/.ssh` are the exception: they are used in place via
`-i`, because Sigrun never copies or re-encrypts them.

## Security notes

- `~/.ssh` is read-only to Sigrun. Discovery scans it; Sigrun never writes,
  modifies, or deletes anything there.
- The vault is encrypted with Argon2id (key derivation) and XChaCha20-Poly1305
  (authenticated encryption). It is written at `0600` with an atomic replace.
- The vault auto-locks after an idle timeout (default 30 minutes), clearing the
  agent; it can be changed or disabled in Settings.
- No telemetry, no cloud, no network calls. Everything stays on your machine.

## Current limitations

- Sigrun's host-key (TOFU) prompt only gates whether Sigrun launches the terminal.
  The `ssh` inside that terminal still verifies the host against your own
  `~/.ssh/known_hosts`; Sigrun does not override it. The pre-flight probe is
  skipped if `ssh-keyscan` is missing or fails.
- `Include` is not followed when parsing `ssh_config`.
- The host does not auto-start; run `sigrun host` yourself.
- Passphrase-protected keys found on disk are used with `-i` and prompt for the
  passphrase in the terminal. Importing a key into the vault is the agent-served
  path.
- RSA agent signing always uses SHA-256: ssh-key 0.6 cannot emit a legacy
  `ssh-rsa`/SHA-1 signature, so a client that only offers SHA-1 will reject
  Sigrun's signatures.

## License

MIT
