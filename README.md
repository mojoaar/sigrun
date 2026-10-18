# Sigrun

Sigrun — victory over key chaos.

The name comes from Sigrún, a valkyrie in Norse mythology. It breaks into
*sigr* — "victory" — and *rún* — "secret" or "rune". A "secret victory" felt
right for a tool whose whole job is secrets: the same *rún* that runs through
cryptography and keys. Pronounced SIG-roon.

Sigrun is a cross-platform SSH connection manager. Add a host, group it — a
`/`-separated path like `PVE/Folder 1` nests — and click Connect to open `ssh`
in your terminal, from the app, the tray, or the CLI. It works the same on
macOS, Linux, and Windows.

Keys are what make those connections work. Sigrun keeps your private keys in an
encrypted local vault, generates new ones, discovers the ones already in
`~/.ssh`, and serves vault-backed keys to `ssh` through an in-process OpenSSH
agent. Installing a key on a server that only takes a password is one step too,
replacing scattered `~/.ssh` bookkeeping with one tool.

It has three faces that share one vault: a **desktop app** (window + system
tray), a **CLI** (`sigrun`), and a long-lived host that owns the vault, the
agent, and the local IPC they both talk to.

## What it does

- **Manage connections.** Add a host and group it; a `/`-separated path like
  `PVE/Folder 1` nests in the Connections list and the tray menu, and
  `~/.ssh/config` hosts are listed alongside the ones you manage.
- **Connect anywhere.** Click Connect and `ssh` opens in your terminal — from
  the app, the system tray, or `sigrun connections connect` on the CLI.
- **Keep keys in an encrypted vault.** Generate Ed25519, RSA, or ECDSA keys,
  import or discover the ones already in `~/.ssh`, and let an in-process
  OpenSSH agent serve vault keys to `ssh`.
- **Reach password-only servers.** Store a per-connection password in the vault,
  or install a key on a server with **Install SSH key…** using the password once.

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
  Left-clicking the tray icon opens the connection menu when the vault is
  unlocked, and shows the window when it is locked.
- **Themes:** four Norse themes (nordlys, fjord, ravn, skumring), each in light
  and dark, following the OS by default with an override.
- **i18n:** English and Danish.
- **Settings:** appearance, language, terminal, an idle **auto-lock** for the
  vault (default 30 minutes; 0 disables it), and "start minimized to tray"
  last.

The **terminal** for connect is auto-detected: Settings lists the launchable
terminals actually installed (Terminal, iTerm, Ghostty, kitty, Alacritty,
WezTerm on macOS; the known terminals on Linux and Windows), or "System
default" to use the OS fallback order.

## First time connecting to a server

A connection assumes a key is already installed on the server. If there is none,
install one explicitly — with **Install SSH key…** on the connection row or in
the connection's edit form. Sigrun then copies the public key into the server's
`~/.ssh/authorized_keys`, which needs the server **password once** — typed into
Sigrun, used to install the key (and, if you also disable password
authentication, to harden the server in a second session), and never stored.
Keys that already live on disk can be used for this step too; they are
**imported into the vault** as part of the same install, and `~/.ssh` itself is
never modified.

Once the terminal is open, `ssh` stays the authority on the connection: Sigrun
only hands it the argv and `SSH_AUTH_SOCK`.

### Where the terminal opens

- **iTerm** opens a **tab in the current window** (a window when none is open).
- **Terminal.app** opens a **new window on each connect** (`do script` without
  `in window`); tab reuse did not reproduce in testing.
- **Ghostty** and **Alacritty** open a **new window** (`-e`); neither exposes a
  tab API on macOS. Support for them is retained, not a fallback we intend to
  drop.

kitty and WezTerm are supported but were not tested on this machine.

## Password connections

A connection can carry a password instead of, or alongside, a key. It lives
only in the encrypted vault; on connect Sigrun writes the askpass variables
into the launched command line, so `ssh` fetches the password through Sigrun's
askpass helper and it is never typed into the terminal. Because the variables
travel on the command line they reach every macOS/Linux launch path —
Terminal.app and iTerm via `do script`, Ghostty/kitty/Alacritty/WezTerm via
`-e`, and Linux `sh -c`.

On Windows there is no askpass helper, so the launched command carries no
askpass variables and a stored password falls back to `ssh`'s own prompt. The
vault must be unlocked when the connection opens,
and deleting the connection removes the stored password once the vault is
unlocked; while the vault is locked the vault entry stays until the next
unlock. Set or clear it in the connection's edit form, or from the CLI:

```bash
sigrun connections password web                    # prompt (never argv)
printf '%s\n' "$PASSWORD" | sigrun connections password web --password-stdin
sigrun connections password web --clear
```

With the vault locked, `sigrun connections connect` cannot tell that a
password is stored and falls back to `ssh`'s own prompt; the app and the tray
refuse through the vault gate instead.

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

# Terminal 2 — add a connection and connect
sigrun vault init                          # choose a passphrase
sigrun vault unlock
sigrun connections add --label web --host example.com --user deploy
sigrun connections connect web --print     # prints the ssh command
sigrun connections connect web             # opens ssh in your terminal
sigrun status

# Then keys and passwords for the servers that need them
sigrun keys generate --name work --algorithm ed25519
sigrun keys import ~/.ssh/id_ed25519 --name personal
sigrun connections password web            # prompt (never argv)
sigrun vault lock
```

`--key` takes a key's id, not its `--name`; list ids with `sigrun keys list
--json`. Generated and imported keys get a UUID id, disk-discovered keys use
their fingerprint. Pass it when you create the connection (`sigrun connections
add … --key <key-id>`), or leave it off to let the agent and your own
`~/.ssh/config` choose the key.

## Install a key on a server

`sigrun connections provision` copies your public keys into the server's
`~/.ssh/authorized_keys`, using the server password once, and then logs back in
with each key to prove it works:

```bash
sigrun keys list --json                       # find the key ids
sigrun connections provision web --key <key-id> --key <key-id>
```

Repeat `--key` to install several keys in one run. Without `--key`, the
connection's own key is used; a connection with no key and no `--key` is an
error. The password is prompted for and never passed on argv; for scripts, pipe
it in with `--password-stdin`:

```bash
printf '%s\n' "$PASSWORD" | sigrun connections provision web --key <key-id> --password-stdin
```

Only vault keys can be provisioned: a key that lives on disk must first be
imported (`sigrun keys import ~/.ssh/id_ed25519 --name personal`), which stores
a vault-encrypted copy. Sigrun needs the private key to verify the install, and
`provision` itself never copies one out of `~/.ssh`.

The CLI has no way to record a first-seen host key. `sigrun connections
provision` therefore works only for a host Sigrun already trusts: trust it
first from the desktop app's Connect flow (where the fingerprint is shown and
confirmed), or add the line to Sigrun's `known_hosts` manually. This is a known
limitation.

`--disable-password-auth` turns password login off on the server after the keys
are verified. This will lock you out if key login fails, so make sure key login
works first.

Hardening edits `sshd_config` through `sudo -n` and reloads `sshd`, backing up
the original and rolling back if the config test or the reload fails. If
passwordless `sudo` is not available it is **skipped**, not failed — the keys
stay installed, the run reports `hardening skipped: …`, and password login is
left as it was.

## How the agent works

`sigrun host` runs an OpenSSH-compatible agent on a private socket (`agent.sock`
in Sigrun's data directory). When the vault is unlocked, every vault-backed key
is loaded into the agent and held in memory only — the plaintext never touches
disk.

`sigrun connections connect` sets `SSH_AUTH_SOCK` to that socket on the terminal
it launches, so `ssh` asks the agent to sign instead of reading a key file.
Disk-backed keys found in `~/.ssh` are the exception: they are used in place via
`-i`, and connecting never copies or re-encrypts them. Installing a key on a
server is the one step that needs a vault copy: the desktop imports the disk key
you select into the vault first.

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
- The reachability probe is a plain TCP connect, so it can report a host as
  unreachable when it is only reachable through a VPN or an `~/.ssh/config`
  ProxyJump. It runs before a terminal opens from the app's **Connect** action
  and from the tray/menu-bar connect, and offers **Connect anyway** (CLI:
  `--force`) rather than blocking. The tray runs the same checks as the app's
  Connect, so a dead host can take a few seconds (up to about 8 s) before the
  terminal opens.
- The host does not auto-start; run `sigrun host` yourself.
- Passphrase-protected keys found on disk are used with `-i` and prompt for the
  passphrase in the terminal. Importing a key into the vault is the agent-served
  path.
- Vault-backed keys require an argv-based terminal (Ghostty/kitty/Alacritty/
  WezTerm, or Linux `sh -c`). Sigrun injects `SSH_AUTH_SOCK` into the spawner's
  environment, which the AppleScript-driven Terminal.app and iTerm cannot
  inherit, so connecting from those two does not see the agent.
- RSA agent signing always uses SHA-256: ssh-key 0.6 cannot emit a legacy
  `ssh-rsa`/SHA-1 signature, so a client that only offers SHA-1 will reject
  Sigrun's signatures.

## License

MIT
