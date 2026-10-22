# Phase 3b: Performance Design Spec

**Goal:** Eliminate redundant disk writes, superfluous native OS menu rebuilds, unindexed $O(N)$ view loops, and background idle polling overhead across Sigrun's core host, Tauri shell, and desktop UI.

## Findings Covered
- **Finding #21 & #34:** `KeysList` rewrites `data.toml` once per discovered key (`crates/sigrun-core/src/host.rs`, `store.rs`)
- **Finding #22:** Tray menu fully rebuilt on every window `Focused` event (`apps/desktop/src-tauri/src/tray.rs`)
- **Finding #33:** Per-row $O(\text{rows} \times \text{keys/creds})$ lookups in Connections and Credentials views (`apps/desktop/src/views/Connections.tsx`, `Credentials.tsx`)
- **Finding #40:** Supervisor tick and auto-lock watcher poll/allocate when fully idle (`crates/sigrun-core/src/host.rs`, `apps/desktop/src-tauri/src/lib.rs`)

---

## 1. Batch & Change-Gated Disk Key Sync (Findings #21 & #34)

### Problem
In `crates/sigrun-core/src/host.rs:1413`, `sync_disk_keys` calls `store.upsert_key(meta)` inside a loop over all discovered keys. Each `upsert_key` invokes `update_data`, which creates `data.lock`, locks it exclusively, reads `data.toml`, updates one key, and saves `data.toml` via atomic write (`write_atomic`).
If a user has 15 disk keys, every single `KeysList` request performs 15 independent locked file writes to `data.toml`, even when no keys have been added, modified, or removed.

### Design
1. In `crates/sigrun-core/src/store.rs`, add:
   ```rust
   pub fn upsert_keys(&self, keys: &[KeyMetadata]) -> Result<()>
   ```
2. Under a single exclusive lock on `data.lock`:
   - Load `data.toml`.
   - Track `let mut changed = false;`.
   - For each key in `keys`:
     - If the key exists with identical fields (`name`, `disk_path`, `has_passphrase`, `algorithm`, `public_key`), do nothing.
     - If the key exists with different fields, update it and set `changed = true`.
     - If the key does not exist, insert it and set `changed = true`.
   - If `changed` is true, call `self.save_data(&data)?`. If `changed` is false, skip saving entirely (0 disk writes).
3. In `crates/sigrun-core/src/host.rs` within `sync_disk_keys`:
   - Collect the generated `KeyMetadata` items into `Vec<KeyMetadata>`.
   - Call `store.upsert_keys(&metas)?` once instead of calling `store.upsert_key` in a loop.

---

## 2. Debounce Tray Menu Rebuilds & Cache Tray Model (Finding #22)

### Problem
In `apps/desktop/src-tauri/src/tray.rs:477-482`:
```rust
w.on_window_event(move |e| {
    if matches!(e, WindowEvent::Focused(true)) {
        set_title(&handle, NORMAL_TITLE);
        sync_tray(&handle, &st);
    }
});
```
Every time the window gains focus (alt-tabbing, clicking back to Sigrun), `sync_tray` is triggered. `sync_tray_now` loads settings, queries the host for connections, builds a new native OS menu model, and calls `tray.set_menu`. Mutating commands already call `sync_tray` whenever connections or vault states change. Rebuilding on every focus is completely redundant.

### Design
1. In `apps/desktop/src-tauri/src/tray.rs`:
   - In `w.on_window_event`, remove `sync_tray(&handle, &st)`. Retain `set_title(&handle, NORMAL_TITLE)` so focusing the window clears the background connection error indicator.
2. In `tray.rs:sync_tray_now`:
   - Maintain a cache of the last rendered tray model: `(bool, String, Vec<TrayEntry>)` (unlocked state, language, entry items).
   - If the new model matches the cached model, return `Ok(())` early without invoking `build_menu(app, &entries)` or `tray.set_menu(...)`.

---

## 3. $O(1)$ Indexed Lookups in Connection & Credential Row Renders (Finding #33)

### Problem
1. In `apps/desktop/src/views/Connections.tsx:595-601`, each rendered connection row calls:
   - `credentialMissing`: runs `credentials.some(...)` ($O(C)$)
   - `credentialUser`: runs `credentials.find(...)` ($O(C)$)
   - `keyName`: runs `credentials.find(...)` ($O(C)$) then `keys.find(...)` ($O(K)$)
   With $N$ connections, each render triggers $O(N \times (3C + K))$ linear search operations.
2. In `apps/desktop/src/views/Credentials.tsx:129`, each rendered credential row executes:
   - `usedBy`: runs `conns.filter(...)` ($O(N)$)
   With $C$ credentials, each render triggers $O(C \times N)$ linear filtering operations.

### Design
1. In `apps/desktop/src/views/Connections.tsx`:
   - Use `useMemo` to construct:
     - `credentialById: Map<string, Credential>` from `credentials`.
     - `keyNameById: Map<string, string>` from `keys` (mapping `key.id` to `key.name`).
   - Rewrite row helper lookups to query the maps directly in $O(1)$ time:
     - `credentialMissing(id)` -> `id !== null && !credentialById.has(id)`
     - `credentialUser(id)` -> `id !== null ? (credentialById.get(id)?.user ?? "") : ""`
     - `keyName(c)` -> lookup `c.key_id` or credential's `key_id` in `keyNameById`
2. In `apps/desktop/src/views/Credentials.tsx`:
   - Use `useMemo` to precompute `usedByCount: Map<string, number>` by iterating through `conns` once ($O(N)$).
   - In row render, retrieve `usedByCount.get(c.id) ?? 0` in $O(1)$ time.

---

## 4. Zero-Disk Idle Auto-Lock Watcher (Finding #40)

### Problem
In `crates/sigrun-core/src/host.rs:175-205`, `watch_auto_lock` wakes every 60 seconds and reads `settings.toml` from disk:
`let minutes = self.store.load_settings().map(|s| s.auto_lock_minutes).unwrap_or(0);`
It does this *before* checking whether the vault is even unlocked (`self.vault.lock().await.is_none()`). When Sigrun is locked and idle in the background, it continuously re-reads `settings.toml` from disk every minute.

### Design
1. In `HostState` (`crates/sigrun-core/src/host.rs`):
   - Add `auto_lock_minutes: std::sync::atomic::AtomicU32`, initialized in `HostState::new` via `store.load_settings().map(|s| s.auto_lock_minutes as u32).unwrap_or(0)`.
   - In `handle_inner` for `Request::SettingsSet { settings }`, update `state.auto_lock_minutes.store(settings.auto_lock_minutes as u32, std::sync::atomic::Ordering::Relaxed)`.
2. In `watch_auto_lock`:
   - Check `self.vault.lock().await.is_none()` *first*. If locked, immediately `continue` to the next sleep cycle without checking timers or reading settings.
   - If unlocked, load `self.auto_lock_minutes.load(std::sync::atomic::Ordering::Relaxed)`. If `0`, `continue`.
   - Only check `last_vault_activity` if the vault is unlocked and `minutes > 0`.
   - Results in 0 disk reads during background idle operation.
