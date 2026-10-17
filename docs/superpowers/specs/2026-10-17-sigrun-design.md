# Sigrun — Design

**Date:** 2026-10-17
**Status:** Draft for review
**Repo:** https://github.com/mojoaar/sigrun

> *Sigrun — victory over key chaos.*

Sigrún is a valkyrie in Norse mythology; the name means "victory run" or
"secret victory" (`sigr` = victory, `rún` = secret/rune). The "secret" root
ties to cryptography and keys. Pronounced **SIG-roon**.

---

## 1. Intent and why

SSH key work is unnecessarily hostile. Reading what keys exist, creating
new ones, getting a public key onto a server, and remembering which key
belongs to which host all happen in scattered terminal commands
(`ssh-keygen`, `ssh-copy-id`, hand-editing `~/.ssh/config`). The result is
"key chaos": keys with cryptic names, forgotten passphrases, duplicate
keys, and no single place to see connections.

Sigrun is a cross-platform desktop application that makes SSH keys and
connections a first-class, manageable thing. Instead of typing commands,
the user browses connections, clicks to connect, generates and organizes
keys, and onboards new servers — from a GUI, the system tray, or a CLI.

**The moment Sigrun exists for:** a user wants to connect to a machine or
manage a key, and instead of opening a terminal and recalling a command,
they click it in Sigrun (often straight from the systray).

**Success looks like:** a user can go from a fresh machine to a working,
key-authenticated SSH setup for their servers and GitHub without typing an
SSH command by hand, and can see every key and connection they own in one
place.

---

## 2. Goals

- Manage SSH **connections**: list, add, edit, group, and connect.
- Manage SSH **keys**: discover existing local keys, generate new ones,
  name/group them, and use them for connections.
- **Connect** by launching the user's own terminal with the correct `ssh`
  command. Sigrun is not a terminal emulator.
- **Provision** key auth onto servers (new or existing) using a
  username/password login.
- Offer **guided setup wizards** for well-known destinations, starting
  with GitHub.
- Work identically as a **GUI window**, a **systray** app, and a **CLI**,
  all over one shared core.
- Be **beautiful**, with dark/light modes and themes, and internationalized
  (**English first**, Danish second).
- Stay **local-only** and fully offline; no cloud account required.

## 3. Non-goals (v1 and deliberately out of scope)

- An in-app terminal emulator. Sigrun launches the user's terminal.
- Cross-device sync of connections or keys.
- A hosted/cloud service, account system, or telemetry.
- Being an `sshd` configuration manager. (Optional hardening is a
  user-enabled extra, not the product's purpose.)
- Mobile apps.
- Automatic discovery/scanning of the network for hosts.

## 4. Anti-goals (failure conditions)

Even if it technically works, Sigrun fails if it:

- **Corrupts, overwrites, or silently mutates** the user's existing
  `~/.ssh` (keys, `config`, `known_hosts`, `authorized_keys` are read-only
  from Sigrun's perspective, except for the explicit provisioning action
  on a remote server).
- **Writes a secret in plaintext** to a log, file, crash report, or
  clipboard history. Private keys and passwords live only in the encrypted
  vault or an in-memory agent.
- **Requires an internet connection** to manage local keys/connections.
- **Locks a user out** of a server by changing `sshd` settings without an
  explicit, previewed, opt-in action.
- Regresses to a worse experience than the terminal for the common path
  (connect, list keys, generate a key).

---

## 5. Platforms, stack, and architecture

### Platforms

macOS, Linux, and Windows. This constrains every non-trivial choice
(systray, keychain, terminal launching, file paths).

### Stack

| Layer | Choice | Notes |
|---|---|---|
| Core | **Rust** | All SSH, key, vault, config, and wizard logic. |
| Desktop shell | **Tauri 2.x** | Native window + system tray; Rust backend. |
| Frontend | **React + TypeScript + Tailwind CSS** | Theming and component flexibility. |
| i18n | **i18next / react-i18next** | English first, Danish second. |
| CLI | **Rust + `clap`** | Binary named `sigrun`. |

Rationale for Tauri over Electron: much smaller binaries, Rust core is
where the sensitive logic belongs, and Tauri 2 has first-class system-tray
support. Frontend framework is my call; React has the strongest
theming/i18n ecosystem.

### Workspace layout

A single Cargo workspace:

```
sigrun/
  Cargo.toml                 # workspace
  crates/
    sigrun-core/             # library: all domain logic
    sigrun-cli/              # `sigrun` CLI binary
  apps/
    desktop/                 # Tauri app (Rust shell + React frontend)
      src-tauri/
      src/                   # React frontend
  docs/
    superpowers/specs/
  .github/workflows/
  CHANGELOG.md
  LICENSE
```

`sigrun-core` is the single source of truth. The CLI links it directly.
The desktop app exposes operations as Tauri commands that call into it.
No domain logic lives in the GUI or CLI layers.

### Process model

- The **desktop app is the long-lived host**. It owns the vault, the SSH
  agent, and a local IPC server, and it hosts the window and tray. It
  enforces a **single instance**; closing the window minimizes to the tray
  rather than quitting by default.
- The **CLI is a client**: it talks to the running host over a local
  channel (Unix domain socket on macOS/Linux, named pipe on Windows). If
  no host is running, it may start a headless core process on demand or
  ask the user to launch the desktop app — the builder picks the lighter
  option.
- Because all surfaces share one host, there is one unlocked vault and one
  agent at a time.

### State and data locations

Non-secret config and the encrypted vault live in the platform config/data
directory (`dirs`-style resolution):

- macOS: `~/Library/Application Support/Sigrun/`
- Linux: `~/.config/sigrun/` and `~/.local/share/sigrun/`
- Windows: `%APPDATA%\Sigrun\`

---

## 6. Features

### 6.1 Connection manager

- A connection is
  `{ id, label, host, user, port, keyRef?, group?, tags?, source }`, where
  `source` is `managed` (created in Sigrun) or `discovered` (parsed from
  `~/.ssh/config`).
- List, search, add, edit, delete, and group connections.
- **Discovered hosts:** Sigrun parses `~/.ssh/config` and shows `Host`
  entries as read-only connections, so hosts already configured locally
  are visible. The user may *adopt* one into a managed connection (which
  copies its current host/user/port/identity); Sigrun never writes back to
  `~/.ssh/config`.
- **Connect:** resolves the SSH command (`ssh user@host -p port -i key`)
  and launches the user's default terminal already running it. Sigrun does
  not force `-F /dev/null`, so the user's own `ssh_config` still applies
  wherever Sigrun gives no explicit argument.
- Terminal resolution is platform-specific:
  - macOS: user's configured terminal (Terminal.app / iTerm2), via a
    setting; default to the system default.
  - Windows: Windows Terminal if present, else PowerShell/cmd.
  - Linux: `$TERMINAL`, then common emulators (`gnome-terminal`,
    `konsole`, `x-terminal-emulator`), with a user override setting.
- The systray lists connections; selecting one performs Connect.

### 6.2 Key management

- **Discover** keys in `~/.ssh` (and `%USERPROFILE%\.ssh` on Windows):
  `id_ed25519`, `id_rsa`, `id_ecdsa`, and `*.pub`, plus keys referenced in
  `~/.ssh/config`. Read-only.
- **Generate** keypairs. Default **ed25519**; RSA and ECDSA available.
  Passphrase optional but recommended.
- **Organize:** user-facing name, notes, group/tags, and an association to
  one or more connections.
- **Two sources of key material:**
  - *Disk-backed* keys are the ones discovered in `~/.ssh`. They are used
    in place (`-i ~/.ssh/...`); Sigrun never copies or alters them.
  - *Vault-backed* keys are generated in or imported into Sigrun. They are
    stored only in the encrypted vault and are never written to `~/.ssh`
    by default (an opt-in export is a future feature).
- **SSH agent:** Sigrun runs its own OpenSSH-compatible agent
  (`SSH_AUTH_SOCK` on macOS/Linux, named pipe on Windows). When the vault
  is unlocked, vault-backed keys are loaded into the agent, so any `ssh`
  the user launches can use them without a plaintext key file ever
  touching disk. Disk-backed keys can also be loaded into the agent with
  their passphrases so the user doesn't retype them.
- Import a key into the vault by copying it; the original on disk is left
  untouched.

### 6.3 Provisioning (v2)

Flow for a server where the user currently has password auth:

1. User enters host, port, username, and password.
2. Sigrun connects over SSH with the password.
3. Sigrun appends the selected public key to the remote
   `~/.ssh/authorized_keys` (creating `~/.ssh` with `0700` and the file
   with `0600` if needed). Idempotent: it detects an existing identical key
   and does nothing.
4. Sigrun optionally verifies the new key works (opens a second connection
   using key auth) before reporting success.
5. The connection is saved with the key attached.

**Password-auth hardening is OFF by default.** A configurable option can,
after an explicit preview and confirmation, make additional `sshd`
changes (e.g. disable password auth). Because this can lock a user out, it
is never part of the default provisioning step.

### 6.4 Guided setup wizards

Wizards walk a user through adding their public key to a service and
verifying it.

- **GitHub** (v1): guide the user to create a token with the minimum
  `admin:public_key` scope (or use the service's OAuth device flow), then
  add the key via the API and verify the SSH connection.
- **Gitea, Forgejo, Codeberg** (v2): the same wizard shape, parameterized
  by **instance URL** (Codeberg is a hosted Forgejo instance; self-hosted
  Gitea/Forgejo work by entering their URL).
- Wizards must degrade to clear manual instructions when a token/API path
  is unavailable.

---

## 7. Security model

### 7.1 Encrypted vault

- On first run the user sets a **master passphrase**. The app derives a
  key with **Argon2id** (memory-hard parameters tuned to a desktop
  baseline) and encrypts the vault with an AEAD (e.g.
  XChaCha20-Poly1305 or AES-256-GCM).
- The vault holds **private keys, passwords, and passphrases**. Derived
  key material and plaintext secrets are zeroized after use.
- The vault must be **unlocked** once per host process start (the desktop
  app or a headless core process). **v1 is passphrase-only**: there is no
  silent keychain storage. OS-keychain convenience unlock and fingerprint
  unlock arrive in v2 (§7.2).

### 7.2 Fingerprint unlock

- Fingerprint is a **convenience layer**, never the root of trust.
  Biometrics are not a reproducible secret, so they cannot derive
  encryption keys directly.
- The vault key (or a wrapping key) is stored in OS secure storage, and
  the biometric check authorizes release:
  - macOS: Keychain with Touch ID / Secure Enclave access control.
  - Windows: Credential Manager / DPAPI with Windows Hello.
  - Linux: best-effort via `fprintd` + libsecret; may fall back to the
    passphrase where unsupported.
- If biometric unlock fails or is unavailable, the passphrase always
  works. The user can disable biometric unlock entirely.

### 7.3 What is encrypted vs plaintext

| Data | Where |
|---|---|
| Private keys, passwords, passphrases | Encrypted vault only |
| Public keys, labels, hosts, users, ports, groups, settings | Plain config |

### 7.4 `~/.ssh` handling

- Read-only discovery of keys, `config`, and `known_hosts`.
- Never written to by Sigrun, with the single exception of the explicit
  remote provisioning action (`authorized_keys` **on the server**).
- Optional "export to `~/.ssh/config`" is a future opt-in, never automatic.

### 7.5 Host-key trust

- Sigrun keeps its **own** `known_hosts` store; it does not silently rely
  on the user's.
- First contact is **trust-on-first-use with an explicit confirmation**:
  the user sees the fingerprint and must accept it. Sigrun never
  auto-accepts a host key.
- A later key mismatch **fails closed** with a loud warning; it is not
  overridable in a single click.
- Well-known services (GitHub, Gitea/Forgejo/Codeberg) may have published
  host keys pre-seeded to skip the first-connect prompt, but a mismatch
  still fails closed.

### 7.6 Secret handling at connect time

- No plaintext key file is ever materialized: vault-backed keys are served
  by the in-memory agent, not written to a temp file.
- Passwords and passphrases are never passed as command-line arguments
  (visible in `ps`); they go through the agent or an askpass helper.
- The launched terminal inherits `SSH_AUTH_SOCK` so `ssh` can reach the
  Sigrun agent.

---

## 8. UI/UX

- **Beautiful by default**: a considered visual identity (Norse-themed
  name/brand), not a stock component demo.
- **Theme set** — four Norse-named themes shipped in the repo
  (`apps/desktop/src/themes/`): **nordlys (default)**, fjord, ravn,
  skumring. Palettes are ported from the author's opencode TUI themes
  (`~/.config/opencode/themes/*.json`) into Sigrun's own token map; the
  opencode theme schema itself is not reusable.
- Each theme carries both a light and a dark palette, so the app follows
  the OS mode by default, with a per-user light/dark override.
- Theming via CSS variables so themes swap cleanly; the choice persists
  per user.
- **i18n**: English first, Danish second. All user-facing strings in
  locale files; no hard-coded UI text. (CLI text is English for v1.)
- Systray menu is fast and shallow: connection list at the top level,
  open-app and quit below.
- Accessibility: keyboard navigation, focus states, sufficient contrast in
  both modes.
- Destructive/risky actions (delete key, provision, hardening) require
  confirmation and show a clear preview of what will change.

---

## 9. Release engineering

- **Semantic versioning**; releases tagged `vMAJOR.MINOR.PATCH`.
- **Changelog** in [Keep a Changelog](https://keepachangelog.com) format,
  maintained with the release.
- **CI** (on PR/push): `cargo fmt --check`, `cargo clippy`, `cargo test`,
  frontend lint + typecheck + build.
- **Release** (on `v*` tag): build and bundle installers for macOS
  (Apple Silicon + Intel), Linux (deb/rpm/AppImage), and Windows
  (NSIS/MSI), then attach them to a GitHub Release.
- **Auto-update** is a later addition; it needs signing certificates
  (macOS notarization, Windows code signing). Until then, users download
  installers from the GitHub Release.

---

## 10. Scope by release

### v1

- Workspace scaffold, CI, license, changelog, release workflow.
- Encrypted vault with master passphrase.
- Key discovery in `~/.ssh`; key generation (ed25519 default); the Sigrun
  SSH agent serving vault-backed keys.
- Connection manager (add/edit/delete/group), including discovered
  `~/.ssh/config` hosts.
- Launch terminal + connect from GUI and systray.
- CLI: list/connect connections; list/generate keys.
- GitHub setup wizard.
- GUI with the four themes (nordlys default) and light/dark; i18n
  English + Danish.

### v2

- Server provisioning (password → key auth) with opt-in hardening.
- Fingerprint unlock (macOS/Windows solid; Linux best-effort).
- Gitea / Forgejo / Codeberg wizards.
- Auto-update with signed builds.
- Optional `~/.ssh/config` export.

---

## 11. Left to the builder

The design review raised five gaps; they are resolved as follows and
recorded in the sections above: key usage via a Sigrun SSH agent (§6.2,
§7.6), the host/client process model (§5), `~/.ssh/config` discovery
(§6.1), host-key trust (§7.5), and passphrase-only unlock in v1 (§7.1).

These remaining items are safe for a capable builder to decide without
changing what gets built:

- Exact SSH engine: pure-Rust `russh` is the intended default for
  provisioning/verification; the "open terminal and connect" path shells
  out to the user's own `ssh`. Refine only if a platform gap appears.
- Vault file format and Argon2 parameter defaults.
- Config file format (TOML vs JSON) and schema/migration strategy.
- Terminal detection heuristics per platform and the override setting.
- GitHub wizard's exact auth path (PAT vs OAuth device flow), preferring
  the least-privilege option.
- Frontend state management and component library details.
- Test strategy (unit tests in core, integration where SSH behavior is
  mockable, a small end-to-end smoke test per platform).
- Auto-update signing setup timing.

---

## 12. Open questions for the product owner

None outstanding. Decisions made during design:

- License: **MIT**.
- Sync: **local-only**.
- v1 scope: as in §10.
- Provisioning hardening: **off by default, configurable**.
- Wizard order after GitHub: **Gitea → Forgejo → Codeberg**.
- Key usage: **Sigrun SSH agent** (no plaintext key files).
- Unlock in v1: **master passphrase only**.
