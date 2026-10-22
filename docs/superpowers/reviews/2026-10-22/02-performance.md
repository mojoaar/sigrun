# Pass 2 — Performance

### KeysList rewrites data.toml once per discovered key
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:1302-1348 (`sync_disk_keys`), called from `keys_list` at host.rs:619
- **What:** Every `KeysList` request rescans all of `~/.ssh` (reading and parsing every private key file via `read_openssh_file`), re-parses `~/.ssh/config`, then calls `store.upsert_key` per key — and each upsert takes the flock, re-reads data.toml, and atomically rewrites the whole file. N keys = N full read+parse+write cycles per list, and KeysList fires on every Keys, Connections and Credentials view refresh.
- **Risk of fixing:** A single batched `update_data` changes the failure mode from "some keys persisted" to all-or-nothing; tests around per-key upsert need adjusting.
- **Risk of leaving:** A user with ~20 keys pays 20 atomic TOML rewrites plus 20 private-key parses on every view switch; the UI stalls on slow disks and the store file churns constantly.
- **Effort:** M — batch the upserts into one `update_data`, consider caching discovery by dir mtime.
- **Suggested fix:** Collect all disk-key metadata, do one `store.update_data` that upserts the whole set, and skip the rescan when `~/.ssh`'s mtime is unchanged.

### Blocking sync I/O on tokio worker threads everywhere in the host
- **Area:** core
- **Location:** crates/sigrun-core/src/store.rs:27-58 (`load_settings`/`load_data`/`save_data`), host.rs:299 (`/proc/<pid>/stat` read in `parent_of`), host.rs:1304 (`read_to_string` of ssh config), keys/discovery.rs:114 (`read_dir`) — all called directly from async handlers and the 2 s supervisor tick
- **What:** `Store` uses `std::fs` throughout and every async request handler (`ConnectionsList`, `keys_list`, `connections_connect`, …) calls it inline; the Linux session sweep also does a synchronous `/proc` read per pid per tick on the runtime thread. Only the Argon2id unlock path uses `spawn_blocking`.
- **Risk of fixing:** Moving to `spawn_blocking` or `tokio::fs` touches many call sites and can subtly change lock-ordering behaviour around the store flock.
- **Risk of leaving:** One slow filesystem (network home, spinning disk, antivirus on Windows) stalls every IPC handler, the agent socket accept loop, and the supervisor tick behind a single blocking read; the whole app feels frozen rather than one request being slow.
- **Effort:** L — the store is sync by design (flock + read-modify-write); wrapping the top handlers in `spawn_blocking` is mechanical but wide.
- **Suggested fix:** Wrap `handle_inner`'s store-touching arms (or `Store` itself) in `spawn_blocking`; at minimum move `discover_keys` and the `/proc` reads off the async threads.

### Tray menu fully rebuilt on every window Focused event
- **Area:** tauri
- **Location:** apps/desktop/src-tauri/src/tray.rs:465-473 (`on_window_event` → `sync_tray`), tray.rs:280-305 (`sync_tray_now`)
- **What:** Each window focus gain runs `is_unlocked` (an IPC-style `handle(Status)` with vault mutex + fs stat), `load_settings` (fs read + TOML parse), a full `ConnectionsList` (data.toml read, vault flag fill per connection), and a complete native menu rebuild — even when nothing changed.
- **Risk of fixing:** A dirty-flag or change-token scheme can miss an update path and leave the tray stale; needs a test that CLI-driven changes still propagate.
- **Risk of leaving:** Alt-tabbing back and forth with a large connection list burns repeated disk reads and native menu rebuilds; on Linux/Windows focus churn is user-visible lag in the tray.
- **Effort:** M — gate the rebuild on a state/version token bumped by the mutating commands.
- **Suggested fix:** Compare a cheap fingerprint (connection count + max updated marker, or an explicit dirty flag set by add/update/remove/lock/unlock) before calling `sync_tray_now` from the focus handler.

### Every key mutation re-decrypts and re-parses the whole vault key set
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:606-616 (`reload_agent`), called from `keys_generate` (682), `keys_import` (730), `keys_remove` (754); agent/session.rs:199-224 (`vault_keys`)
- **What:** Adding, importing or removing one key throws away the entire agent key set and rebuilds it: every vault-held PEM is re-parsed and every passphrase-encrypted key is re-decrypted (bcrypt-ish cost per key), plus `vault_keys` does an O(vault × metadata) `find` per entry.
- **Risk of fixing:** Incremental add/remove must keep the agent consistent with the vault on partial failure (e.g. an unparseable new key must not evict the working set).
- **Risk of leaving:** With several passphrase-protected keys, each key add/import stalls the IPC handler for hundreds of milliseconds of redundant decryption and briefly replaces the whole agent key list.
- **Effort:** S — `reload_agent` could take the single changed key (or removed id) and patch `AgentState` instead of rebuilding.
- **Suggested fix:** Add `AgentState::insert`/`remove_key` and only fall back to a full rebuild on unlock.

### Windows named-pipe accept loop spins hot on persistent create failure
- **Area:** core
- **Location:** crates/sigrun-core/src/ipc/server.rs:80-98
- **What:** `ServerOptions::new().create(name)` failing (e.g. a stale pipe owned by a dead process, or a permission error) hits `Err(_) => continue` with no sleep, retrying in a tight loop for the host's lifetime.
- **Risk of fixing:** A backoff delays legitimate re-creation by milliseconds; must not mask the "already running" error the host lock file is meant to produce first.
- **Risk of leaving:** A rare but real failure mode pins a CPU core at 100% for as long as the app runs, invisible except as fan noise and battery drain.
- **Effort:** S — one `tokio::time::sleep` (or capped exponential backoff) in the error arm.
- **Suggested fix:** Sleep ~100 ms (with shutdown cancellation) before retrying `create`, and log the error once.

### connect_guarded loads the whole connection list to read one host/port
- **Area:** tauri
- **Location:** apps/desktop/src-tauri/src/commands.rs:376-392 (`connect_guarded`)
- **What:** Every in-window and tray connect first runs a full `ConnectionsList` — data.toml read + parse plus a vault `get` per connection to fill `has_password` — only to `.find` the one connection's host/port for the trust probe; then `connect_inner` loads data.toml all over again.
- **Risk of fixing:** A narrower host/port lookup path must not bypass the vault-flag fill that `ConnectionsList` performs, if anything else relies on it.
- **Risk of leaving:** Doubles the disk work per connect and adds a vault lock round trip per stored connection; noticeable only with many connections or a slow disk.
- **Effort:** S — add a `Request::ConnectionGet { id }`-style lookup or read the row from `store.load_data()` directly in `connect_guarded`.
- **Suggested fix:** Replace `list_connections_inner` with a direct `store.load_data()` find of the single connection.

### Supervisor tick polls Status and allocates even when fully idle
- **Area:** tauri
- **Location:** apps/desktop/src-tauri/src/lib.rs:305-341
- **What:** Every 2 s, forever: `sweep_dead_sessions` (fine), `handle(Status)` (vault mutex + `vault_file().exists()` fs stat), and `active_ids()` (clone + sort of the set) — even when the vault is locked, no session has ever run, and nothing can have changed.
- **Risk of fixing:** Skipping the Status probe when locked-and-idle must not break CLI-side state changes being picked up (the CLI can lock/unlock the vault under the app).
- **Risk of leaving:** Constant small wakeups: a mutex, an fs stat and an allocation every 2 s for the app's lifetime; negligible on a desktop, but it defeats OS idle accounting and is pure waste in the common locked-idle case.
- **Effort:** S — short-circuit the tick when `sessions` is empty and the vault state matches the last tick, relying on the mutating commands' own `sync_tray` calls.
- **Suggested fix:** Only call `is_unlocked` when a dirty flag (set by lock/unlock/auto-lock) or the active-set check demands it.

### Auto-lock watcher re-reads settings.toml every 60 s
- **Area:** core
- **Location:** crates/sigrun-core/src/host.rs:166-196 (`watch_auto_lock`)
- **What:** Each tick does a blocking `load_settings()` (fs read + TOML parse) on the async task just to fetch `auto_lock_minutes`, a value that changes only when the user saves settings.
- **Risk of fixing:** Caching the value needs an invalidation hook from `save_settings`; a stale cache could delay an auto-lock change by up to a minute (or until next activity) — acceptable but must be documented.
- **Risk of leaving:** Trivial waste and another sync fs read on a tokio thread; also means a corrupt settings file logs nothing and silently disables auto-lock via `unwrap_or(0)`.
- **Effort:** S — cache `auto_lock_minutes` in an `AtomicU64` updated by the settings save path.
- **Suggested fix:** Read the setting once at start and refresh it from the `save_settings` command instead of polling the file.

### Per-row O(rows × keys/creds) lookups and full-list re-renders
- **Area:** frontend
- **Location:** apps/desktop/src/views/Connections.tsx:574-580 (`credentialMissing`/`credentialUser`/`keyName` per row), 587-601 (every `ConnectionRow` re-created per render); Credentials.tsx:128 (`usedBy` filters all connections per row)
- **What:** Each rendered row linearly scans `credentials` and `keys` (two or three `.find` passes per row per render), and no row component is memoized, so any refresh (including the `connections-changed` event that fires when a session's dot toggles) re-renders the entire list.
- **Risk of fixing:** `React.memo` plus prebuilt `Map`s is behaviour-neutral; the only risk is stale rows if a prop is accidentally mutated in place.
- **Risk of leaving:** Fine at tens of connections; at a few hundred, every session start/stop event re-renders the whole tree with quadratic-ish lookup work.
- **Effort:** S — build `Map`s from credentials/keys once per refresh and wrap `ConnectionRow` in `React.memo`.
- **Suggested fix:** Precompute `credById`/`keyById` maps in the view, memoize the row component, and pass stable callbacks.

### App.refresh re-applies theme and language unconditionally
- **Area:** frontend
- **Location:** apps/desktop/src/App.tsx:36-46
- **What:** Every `refresh()` (lock, unlock, vault-locked event, manual lock button) runs `applyTheme` and `await i18n.changeLanguage(...)` even when neither setting changed; `changeLanguage` notifies every `t()` consumer and re-renders the tree.
- **Risk of fixing:** None real — compare with the previously applied values first.
- **Risk of leaving:** An unnecessary full-tree re-render on each lock/unlock cycle; cosmetic.
- **Effort:** S — keep last-applied theme/language in refs and skip when unchanged.
- **Suggested fix:** Memoize the applied `(theme, language)` pair and only call `applyTheme`/`changeLanguage` on change.
