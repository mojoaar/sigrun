# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rust workspace with the `sigrun-core` domain library and the `sigrun` CLI, plus CI for build, lint, and test.
- Encrypted vault: Argon2id key derivation, XChaCha20-Poly1305 sealed envelope, `0600` atomic writes, and a lockable in-memory lifecycle.
- Key generation for Ed25519, RSA, and ECDSA.
- Key discovery and import from `~/.ssh`, including passphrase-protected private keys.
- An in-process OpenSSH-compatible SSH agent with Ed25519, RSA, and ECDSA signing, served over a private socket.
- Connection manager with `ssh_config` host discovery and adoption.
- Terminal launch: build a validated `ssh` argv and open it in the user's terminal with `SSH_AUTH_SOCK`.
- The `sigrun` command-line interface (`host`, `vault`, `keys`, `connections`, `status`).
- A headless host that owns the vault, agent, and store and serves requests over local IPC.
- Tauri 2 desktop app with a main window and system tray.
- Four Norse themes, each with light and dark variants.
- English and Danish i18n.
- Desktop host lifecycle: the app owns the vault, agent, and IPC, and runs as a single instance.
- Tauri command surface exposing the host requests to the UI.
- System tray connection menu.
- TOFU host-key trust with a Sigrun-owned `known_hosts`.
- Multi-OS installer releases (macOS, Linux, Windows) built and published on `v*` tags.
- Raven line-art tray icon (macOS template) and app-icon set; tray left-click toggles the window.
- A "Start minimized to tray" setting.
- An idle auto-lock for the vault (default 30 minutes; configurable, 0 disables it).
- Auto-detected terminal picker in Settings, listing the launchable terminals installed on the machine (Terminal, iTerm, Ghostty, kitty, Alacritty, WezTerm on macOS; the known terminals on Linux/Windows).
- Multi-key provisioning: install several vault keys on a connection over password authentication, verifying each key by logging in with it.
- Per-connection password-auth hardening (`--disable-password-auth`), with backup and rollback and a no-sudo skip.
- The CLI `connections provision` subcommand, with repeatable `--key` and `--password-stdin`.
- Tray connect menu groups connections by their group, lists `~/.ssh/config` hosts under "Discovered", and shows no connections while the vault is locked.
- The Connections list shows one section per group (the tray menu groups connections too).
- Installing a key on a server (desktop) can use disk-backed keys, importing them into the vault as part of the step.
- The app's Connect action reports an unreachable host before a terminal opens, with "Connect anyway"; the tray/menu-bar connect runs the same check.
- iTerm opens a tab in the current window.
- Per-connection passwords: stored in the vault, handed to `ssh` through an askpass helper so a password-only host never prompts in the terminal, and settable from the app or with `sigrun connections password <id-or-label> [--password-stdin] [--clear]`.
- Nested connection groups: a `/`-separated path like `PVE/Folder 1` renders as a nested submenu in the tray and a nested section in the Connections list.
- An inline help and about view in the app, describing what Sigrun does and how the pieces fit together.

### Changed

- "Start minimized to tray" now sits at the end of Settings, after the other options.
- A connection whose `ssh` fails now drops into a shell instead of losing the error as the terminal closes.
- A tray/menu-bar connect failure now opens the trust or unreachable dialog in the window instead of failing silently.
- The tray/menu-bar connect now runs the reachability probe, so a dead host is reported before a terminal opens.
- The header Lock button stands out in danger red.
- The README and AGENTS.md now describe Sigrun as an SSH connection manager with key-based authentication; the README leads with connections and presents keys as what makes them work.

### Fixed

- A disk-backed key with no vault key no longer hides the install action (the grey hint is now a button that opens Keys).
- A failed key install now shows the host's own error message instead of a generic one.
- Keys view and the connection key-select no longer blank the app (the flattened `KeyView` wire shape is re-nested; views are wrapped in an error boundary).
- Void commands (`vault lock`, `vault init`, removals, host trust) now actually run — the response unwrap no longer rejects a payload-less `Ok`.
- Clicking the dock icon with the window hidden now reopens it on macOS.
- Tray labels follow the app language instead of always being English, and tray left-click shows the app when the vault is locked instead of toggling the window.
- The dock shows `Sigrun`, not `sigrun-desktop`.
- Deleting a connection or a key now asks in-app instead of calling `window.confirm`, which the webview ignores (so Delete silently did nothing).
- Windows named pipes are derived from the user and a hash of the data dir, so two hosts with different data dirs no longer collide on the same pipe name.
- A terminal no longer closes before a failed `ssh` has printed its error.
- The Linux terminal-detection test no longer fails on a host with no known terminal installed.
- `has_password` now survives the IPC wire round trip, so the saved-password state is correct after a reload.

### Notes

- Auto-update is not implemented; it requires code signing and notarization.
