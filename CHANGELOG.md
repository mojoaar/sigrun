# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The README says how to get the `sigrun` command onto `PATH`: release binary, Arch package, or from source.

### Fixed

- Verify peer UID on Unix IPC socket connections to prevent unauthenticated local access.
- Enforce line length limits during IPC stream reading to prevent memory exhaustion from unbounded lines.
- Serialize concurrent vault unlock operations to prevent parallel Argon2 memory spikes.
- Apply exponential backoff delay on consecutive failed vault unlock attempts.
- Validate and clamp vault KDF parameters against safe upper bounds on unlock.
- Ensure vault `put` and `remove` operations roll back in-memory state if disk persistence fails.
- Fail closed during TOFU host-key verification if `ssh-keyscan` fails or is unavailable.

## [0.5.0] - 2026-10-22

### Changed

- Discovered disk keys are batch-persisted and skip file rewrites when metadata is unchanged.
- The tray menu caches its model and skips redundant native rebuilds on window focus.
- Connection and credential views use indexed O(1) maps for row metadata resolution.
- The auto-lock watcher caches settings in memory and skips disk access when the vault is locked.
- The TOFU host-key gate now runs in the host, before any connection opens, and trust errors carry a
  structured payload (`fingerprint`, `algorithm`, `key`) over IPC so clients can render the trust
  dialog from fields instead of parsing a message string.
- Settings read and save use typed IPC requests (`settings_get` / `settings_set`) and response (`settings`) rather than smuggling JSON strings.
- Removed unused agent_enabled setting and dead public core vault/agent APIs.

### Fixed

- CLI connection commands resolve key names to IDs instead of storing names verbatim.
- Agent state recovers gracefully from poisoned read/write locks instead of permanently panicking.
- Language switching in the desktop app only updates UI localization without competing with settings persistence.
- Parse `Key=Value` syntax (with optional spaces around `=`) in `~/.ssh/config`, matching OpenSSH.
- Quote arguments containing spaces the way `cmd.exe` parses them on Windows, so a key path like `C:\Users\My User\.ssh\id_rsa` reaches `ssh` as one argument.
- Fall back cleanly to the default theme (`nordlys`) if an invalid or tampered theme is configured.
- Show localized explanation instead of raw translation key `keys.addToVaultBody` when copying a disk key to the vault.
- Dismiss the unreachable connection modal on overlay backdrop click or Escape key press.
- Track and surface failed vault unlock attempt count in error responses while redacting passphrase inputs.
- `ConnectionEnded` cleans up watched sessions alongside active state to prevent stale sweep clears on quick reconnects.
- `SessionStarted` validates PIDs (rejecting <= 1, own process) and requires active connections, preventing arbitrary process group signaling.
- Windows IPC server throttles retry loops and respects shutdown when named pipe creation fails.
- Host Unix IPC socket explicitly enforces `0600` permissions.
- Bundle export uses atomic file creation (`create_new`) for `.zip` files and guards against pre-existing `keys/` directories.
- Remote sshd hardening and provisioning error messages are sanitized using fixed exit-code descriptions, preventing arbitrary remote stderr injection.
- Provisioning rejects an empty username before Russh authentication, preventing misleading password failure errors.
- Removed dead stderr logging on inconclusive reachability probe timeouts.
- Keys and Credentials views surface specific host error messages instead of generic failure notices.

## [0.4.0] - 2026-10-21

### Added

- A connection row names the key it signs in with, whether the key is its own or comes from its
  credential, next to the password badge.
- A key Sigrun found on disk can be copied into the vault from its row, with an optional passphrase,
  instead of importing the file again.
- A key in the vault can be saved to disk from its row: the save dialog picks the place, the file is
  created `0600`, an existing file is never overwritten, and `~/.ssh` is refused.
- `Cmd+F` puts the cursor in the connection search field and `Cmd+L` locks the vault, the same action
  as the Lock button. The README lists the shortcuts.
- Screenshots of the connections list, the add-connection dialog, and the keys, credentials and
  settings views, in the README.

### Changed

- Installing a key on a server can reuse the password already saved on the connection or its
  credential: leave the field empty. A failed install names the host and port instead of the bare ssh
  error.
- The install confirmation names the key once and clears itself after five seconds.

### Fixed

- A key held in the vault works in a terminal started through AppleScript (Terminal.app, iTerm,
  Ghostty tabs). The agent socket travels in the command line, because that shell inherits nothing
  from Sigrun, so ssh no longer falls back to asking for a password.
- Terminal launching: one window instead of a blank one plus the session, for Terminal.app, iTerm and
  Ghostty; iTerm types the line into the session instead of exec'ing it; Ghostty's "open in a new
  window" opens a window rather than a tab; a terminal installed as an app bundle is found without
  being on `PATH`; and the AppleScript terminals clear the echoed command line.
- Closing a connection's terminal window closes it instead of leaving it hanging, and a session whose
  terminal vanished without a hangup no longer leaves a pulsing dot and a stranded ssh behind.
- Connecting no longer refuses a host the reachability probe cannot classify. Only a name that does
  not resolve, a refused port and a timeout block the launch; any other error is logged and ssh has
  the last word.
- A host that offers several key types no longer looks like a changed host key. An unseen type is a
  first-use prompt; only a different key of a known type fails closed.
- The key picker in the connection form names its empty choice "No key" instead of showing a blank
  row.
- The README and the in-app Help describe moving a key between the vault and disk.

## [0.3.0] - 2026-10-21

### Added

- The GitHub release page now carries the tag's changelog section, so a downloader sees what changed without opening the repository.
- Credentials: named identities a connection can reference instead of repeating a user, key and password.
- Credential passwords live in the vault and every connection, agent session, askpass prompt and provisioning run resolves the same identity.
- Manage credentials from the CLI, and point a connection at one.
- A Credentials tab, and a connection form that can use one instead of its own details.
- An AUR `sigrun-bin` PKGBUILD (`packaging/aur`) that repackages the released AppImage and CLI binary for Arch, kept in the repository for local builds and not yet published to the AUR.
- German (Deutsch), French (Français), Spanish (Español), Swedish (Svenska) and Norwegian Bokmål (Norsk bokmål) in the interface, the tray and the macOS menu.

### Changed

- Theme names come from the interface language files, and the language picker is driven by one table.
- `sigrun credentials add` stores the credential and seals its password in one step.
- Export carries the credential reference, never the credential; an imported connection shows a missing-credential badge until it is repaired.
- GitHub releases are published as normal releases rather than pre-releases.

### Fixed

- The Add/Edit connection and Install-key dialogs scroll inside the window instead of being clipped when the window is short, and the window opens taller (820px) by default.
- A credential-backed connection shows the credential's user in the list, and switching one with a saved password reports the removal as a status line rather than an error.
- The Connect button on a connection row is the filled accent button, so the main action stands out from Install, Edit and Delete.
- Removing the saved password from a credential with no key is refused by the app and the host, so a connection can no longer resolve to an identity with neither a key nor a password.
- `sigrun connections add --credential` refuses a value that matches no credential instead of storing the typed text as a dangling id.
- The `--key` help now points at `sigrun keys list --json`, which prints the ids it asks for.
- Saving a connection whose credential no longer exists now refuses instead of storing a dangling reference and deleting its saved password.
- A connection that switches to a credential no longer loses its saved password before a switch the host will refuse, and says so when the password had to be removed first.
- A connection can no longer mix a credential with its own user or secret.
- The README, the in-app Help view and AGENTS.md now document credentials: the tab, one sign-in source per connection, the vault split, the CLI commands, and that credentials never travel in an export.

### Notes

- The five new translations have not been read by native speakers yet; corrections are welcome.
- On Windows there is no askpass helper and sessions are not tracked, so a stored password falls back to `ssh`'s own prompt and no live dot appears.
- A bundle is not a backup: it restores connections only, and never a private key, a stored password or a credential. The vault file plus its passphrase is what restores those.

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
