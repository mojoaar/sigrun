# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The GitHub release page now carries the tag's changelog section and is marked a pre-release, so a downloader sees what changed without opening the repository.
- Credentials: named identities that connections can share (model and listing; the UI follows).
- Credential passwords live in the vault and every connection, agent session, askpass prompt and provisioning run resolves the same identity.
- Manage credentials from the CLI, and point a connection at one.

### Fixed

- A connection can no longer mix a credential with its own user or secret.

## [0.2.0] - 2026-10-20

### Added

- The macOS menu gained a File menu (New Connection ⌘N, Import…, Export…) and a Settings… item (⌘,); both follow the Language setting.

### Changed

- Rust 1.90 is now the minimum supported version, matching what a dependency of the desktop app (Tauri 2.12) already requires.

### Fixed

- The macOS app menu (Edit, View, Window, Quit) now follows the Language setting instead of staying English.
- The active tab is marked with `aria-current="page"`, the no-matches message is announced to screen readers, and focused controls show a theme-aware focus ring.
- Every dialog now takes focus when it opens, closes on Escape without ever confirming a destructive action, and returns focus to the control that opened it.
- The connection form now refuses a host or user the launcher cannot use, says which field is wrong under that field, and allows a connection with no user.
- The dialog plugin's JavaScript package now matches the Rust crate (2.7.x), so `tauri dev` no longer warns about a version mismatch.
- New Connection (⌘N) no longer turns an open edit form into an add that overwrites the connection being edited; while another dialog is open it does nothing.

## [0.1.0] - 2026-10-19

### Added

- A search field over the connection list, matching label, user, host, port and group.
- The connection form suggests the group paths already in use and warns when a label duplicates another connection.
- A macOS About dialog naming the project, its version, its copyright and website, and the app icon, plus complete installer metadata.
- An `open_in` setting (`"window"` or `"tab"`) for where a connect opens.
- The open-in setting is editable in Settings, next to the terminal picker.
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
- Ghostty and iTerm open a new tab instead of a window when the setting asks for it.
- Per-connection passwords: stored in the vault, handed to `ssh` through an askpass helper on macOS and Linux so a password-only host never prompts in the terminal (Windows has no helper and falls back to `ssh`'s own prompt), and settable from the app or with `sigrun connections password <id-or-label> [--password-stdin] [--clear]`.
- Nested connection groups: a `/`-separated path like `PVE/Folder 1` renders as a nested submenu in the tray and a nested section in the Connections list.
- An inline help and about view in the app, describing what Sigrun does and how the pieces fit together.
- A pulsing dot on a connection row while its session is live (macOS and Linux): the host keeps the live sessions in memory, the launched command reports its own end, and the dot clears when the session closes.
- The launched line reports its own shell's pid before `ssh` runs, so the host notices a session whose terminal is killed outright (macOS and Linux).
- The export/import bundle format: connections and public key metadata written as a folder or a `.zip`, read back with zip limits and entry-name checks, and merged by id without overwriting.
- Export and import over the host IPC: writing a bundle refuses a destination inside `~/.ssh` or one already taken, and importing merges connections by id without ever touching the vault.
- The desktop side of export and import: the two native pickers (`tauri-plugin-dialog` 2.7.3, pinned to the MSRV floor, with only the `dialog:allow-open` and `dialog:allow-save` grants), the two commands, and their TypeScript wrappers.
- An Import and export section in Settings: four buttons that write a bundle to a folder or a `.zip` and read one back through the native pickers, either way reporting what was added, skipped, invalid or left unresolved, and showing the host's refusal in the section rather than an alert.
- Import and export described end to end: the four Settings buttons, the folder/`.zip` layout, the `~/.ssh` and overwrite refusals, and that a bundle is a transfer rather than a backup — it restores connections only, and holds no private key or saved password.

### Changed

- The **Open in** setting no longer offers a new tab for terminals that cannot open one: the terminal picker reports which terminals can be asked, and Settings greys out **New tab** and says so.
- Documented which terminals honour the new window-or-tab setting.
- iTerm connects honour the "Open in" setting, so the default opens a new window instead of reusing a tab.
- "Start minimized to tray" now sits at the end of Settings, after the other options.
- A connection whose `ssh` fails now drops into a shell instead of losing the error as the terminal closes.
- A tray/menu-bar connect failure now opens the trust or unreachable dialog in the window instead of failing silently.
- The tray/menu-bar connect now runs the reachability probe, so a dead host is reported before a terminal opens.
- The header Lock button stands out in danger red.
- The README and AGENTS.md now describe Sigrun as an SSH connection manager with key-based authentication; the README leads with connections and presents keys as what makes them work.
- Documented that live sessions show a dot on macOS and Linux only; the Windows launch path cannot report a session end.
- Documented how the live-session dot clears: the host watches the launched shell's own process, so closing a window or tab, a force-quit, and a crash all clear it within a tick.
- The macOS window uses an overlay title bar, so the bar takes the header's own colour; the header drags the window (the drag region is unconditional, so it drags off macOS too), and the window's native appearance follows the **Appearance** setting so the traffic lights stay legible against the themed header — on macOS that also repaints the app's own menus and About panel.

### Fixed

- Exporting refuses a chosen folder whose path reaches into `~/.ssh` through a `..` and one that already holds a `README.txt`, and the zip save dialog suggests a `.zip` name.
- Dragging the window by the header's title text now works: the drag region is `deep`, so it covers the header's descendants, not only the header element's own padding (the header's buttons still do not drag).
- The launched line's session-start report is now bounded to two seconds, so a host that accepts the socket but never answers no longer delays every connect by up to 30 seconds.
- A malformed `session-started` invocation (a missing or unparsable pid) now exits quietly instead of starting a second copy of the app.
- The host re-checks every watched shell pid on each poll tick, so a killed session clears the connected dot on the tick it is noticed rather than after the next report.
- A live session is now watched by its own shell's pid, so a terminal that kills the whole process group (closing a Ghostty tab, a force-quit) still clears the connected dot on the next tick.
- The app now declares the Tauri permission its webview needs, so the UI really receives host events: the connected dot clears within a tick of a session ending (it used to stay lit until a tab switch), and the idle auto-lock notice and a tray connect error reach the UI again.
- Closing the terminal window now clears a connection's live dot: the launched command traps `HUP`/`TERM`/`INT`, so a window close still reports the session's end instead of leaving the dot lit.
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
