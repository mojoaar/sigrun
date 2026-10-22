# Phase 3b: Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate redundant disk writes, superfluous native OS menu rebuilds, unindexed $O(N)$ view loops, and background idle polling overhead across Sigrun's core host, Tauri shell, and desktop UI.

**Architecture:**
- Batch discovered disk key metadata writes in `Store::upsert_keys` and gate persistence on actual changes, turning $N$ sequential file locks into 0 writes when unchanged and 1 write when altered.
- Remove redundant tray menu sync from window `Focused` events and cache the built tray model to avoid rebuilding native OS menus.
- Precompute memoized `$O(1)$` lookup maps for connection and credential row rendering.
- Cache `auto_lock_minutes` in an atomic on `HostState` and check lock state first to avoid reading `settings.toml` from disk during idle periods.

**Tech Stack:** Rust 2021 (Tokio, toml, atomic), Tauri v2 (tray, window events), React (TypeScript, useMemo).

**Spec:** `docs/superpowers/specs/2026-10-22-phase-3b-perf-design.md`

## Global Constraints
- Gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` and `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- Dependencies: Only via `[workspace.dependencies]`. No new dependencies.
- Security: Secrets live only in the vault. `~/.ssh` is strictly read-only.
- Conventional commits: One commit per task.

## Review Focus
- Unchanged keys during disk sync must not update or touch `data.toml` file mtime.
- Window focus must continue to clear `ATTENTION_TITLE` to `NORMAL_TITLE` without stalling on tray rebuilds.
- Connection rows with missing credentials or matching keys must display identical information before and after map optimizations.
- Changing `auto_lock_minutes` in settings must immediately reflect in the cached atomic without requiring host restart.
- Idle loop must wake on shutdown cancellation immediately without sleeping full timeouts.

---

### Task 1: Batch & Change-Gated Disk Key Sync (Findings #21 & #34)

**Files:**
- Modify: `crates/sigrun-core/src/store.rs`
- Modify: `crates/sigrun-core/src/host.rs:1410-1430`
- Test: `crates/sigrun-core/src/store.rs` (tests module)
- Test: `crates/sigrun-core/src/host.rs` (tests module)

**Interfaces:**
- Consumes: `KeyMetadata`, `Store::paths()`, `Store::load_data()`, `Store::save_data()`
- Produces: `pub fn upsert_keys(&self, keys: &[KeyMetadata]) -> Result<()>` on `Store`

- [ ] **Step 1: Write failing tests for `Store::upsert_keys`**
In `crates/sigrun-core/src/store.rs` tests module:
Write `upsert_keys_batches_and_skips_write_when_unchanged`:
- Call `store.upsert_keys(&[key1.clone(), key2.clone()])`. Assert both keys present.
- Record the modified time of `data.toml`.
- Call `store.upsert_keys(&[key1.clone(), key2.clone()])` with identical keys.
- Assert that `data.toml` was not modified (mtime unchanged).
- Call `store.upsert_keys(&[modified_key1])` and assert the update persisted.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-core store::tests::upsert_keys`
Expected: FAIL (method `upsert_keys` not found)

- [ ] **Step 3: Implement `Store::upsert_keys` and update `sync_disk_keys`**
In `crates/sigrun-core/src/store.rs`:
```rust
pub fn upsert_keys(&self, new_keys: &[KeyMetadata]) -> Result<()> {
    if new_keys.is_empty() {
        return Ok(());
    }
    ensure_private_dir(&self.paths.data_dir)?;
    let lock = File::create(self.paths.lock_file())?;
    lock.lock_exclusive()?;
    let outcome = (|| {
        let mut data = self.load_data()?;
        let mut changed = false;
        for key in new_keys {
            match data.keys.iter_mut().find(|k| k.id == key.id) {
                Some(existing) => {
                    if existing != key {
                        *existing = key.clone();
                        changed = true;
                    }
                }
                None => {
                    data.keys.push(key.clone());
                    changed = true;
                }
            }
        }
        if changed {
            self.save_data(&data)?;
        }
        Ok(())
    })();
    drop(lock);
    outcome
}
```
In `crates/sigrun-core/src/host.rs` in `sync_disk_keys`:
Collect all generated `meta` into `Vec<KeyMetadata>` and call `store.upsert_keys(&metas)?` once before building `views`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-core store::tests && cargo test -p sigrun-core host::tests`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/store.rs crates/sigrun-core/src/host.rs
git commit -m "fix(core): batch disk key upserts and avoid unnecessary writes"
```

---

### Task 2: Eliminate Tray Menu Rebuild on Focus & Cache Tray Model (Finding #22)

**Files:**
- Modify: `apps/desktop/src-tauri/src/tray.rs`
- Test: `apps/desktop/src-tauri/src/tray.rs` (tests module)

**Interfaces:**
- Consumes: `tray_menu_model`, `build_menu`, `TRAY_ID`, `NORMAL_TITLE`
- Produces: Cached tray menu model skipping redundant `tray.set_menu` calls

- [ ] **Step 1: Write test for cached tray menu model**
In `apps/desktop/src-tauri/src/tray.rs`:
Write a unit test `tray_menu_model_equality` asserting that `TrayEntry` equality correctly identifies unchanged menu lists and differences when connections or unlocked status change.

- [ ] **Step 2: Run test to verify it fails/passes**
Run: `cargo test -p sigrun-desktop tray::tests::tray_menu_model_equality`

- [ ] **Step 3: Update `tray.rs` to remove focus sync and cache menu model**
In `apps/desktop/src-tauri/src/tray.rs`:
1. In `w.on_window_event`:
   Remove `sync_tray(&handle, &st);` from `WindowEvent::Focused(true)`, keeping `set_title(&handle, NORMAL_TITLE)`.
2. In `sync_tray_now`:
   Maintain a static or mutex-guarded `LAST_TRAY_MODEL: Mutex<Option<(bool, String, Vec<TrayEntry>)>>`.
   Compute `(unlocked, language.clone(), entries.clone())`.
   If `*last == Some(current)`, return `Ok(())` early without calling `build_menu` or `tray.set_menu`.
   Otherwise, update `*last = Some(current)` and apply to tray.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-desktop tray::tests`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add apps/desktop/src-tauri/src/tray.rs
git commit -m "fix(desktop): avoid rebuilding tray menu on focus and cache model"
```

---

### Task 3: Precompute $O(1)$ Maps for UI Row Renders (Finding #33)

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/views/Credentials.tsx`
- Test: `apps/desktop/src/views/Connections.test.tsx`
- Test: `apps/desktop/src/views/Credentials.test.tsx`

**Interfaces:**
- Consumes: `credentials`, `keys`, `conns`
- Produces: `credentialById`, `keyNameById`, `usedByCount` maps via `useMemo`

- [ ] **Step 1: Write tests verifying row lookups and counts**
In `apps/desktop/src/views/Connections.test.tsx`:
Add a test verifying that connection row displaying adopted credentials and direct keys continues to correctly show the key name and missing credential indicator.
In `apps/desktop/src/views/Credentials.test.tsx`:
Add a test asserting that credential rows render correct `usedBy` counts matching connection associations.

- [ ] **Step 2: Run tests to verify existing behavior**
Run: `npm test -- --run src/views/Connections.test.tsx src/views/Credentials.test.tsx`
Expected: PASS

- [ ] **Step 3: Implement memoized maps in `Connections.tsx` and `Credentials.tsx`**
In `apps/desktop/src/views/Connections.tsx`:
```typescript
const credentialById = useMemo(
  () => new Map(credentials.map((c) => [c.id, c])),
  [credentials]
);
const keyNameById = useMemo(
  () => new Map(keys.map((k) => [k.key.id, k.key.name])),
  [keys]
);

const credentialMissing = (id: string | null) => id !== null && !credentialById.has(id);
const credentialUser = (id: string | null) => (id !== null ? credentialById.get(id)?.user ?? "" : "");
const keyName = (c: Connection) => {
  const keyId = c.key_id ?? (c.credential_id ? credentialById.get(c.credential_id)?.key_id : null);
  return (keyId ? keyNameById.get(keyId) : "") ?? "";
};
```
In `apps/desktop/src/views/Credentials.tsx`:
```typescript
const usedByCount = useMemo(() => {
  const counts = new Map<string, number>();
  for (const c of conns) {
    if (c.credential_id) {
      counts.set(c.credential_id, (counts.get(c.credential_id) ?? 0) + 1);
    }
  }
  return counts;
}, [conns]);

const usedBy = (id: string) => usedByCount.get(id) ?? 0;
```

- [ ] **Step 4: Run tests to verify it passes**
Run: `npm test -- --run src/views/Connections.test.tsx src/views/Credentials.test.tsx && npm run typecheck && npm run lint`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Credentials.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/views/Credentials.test.tsx
git commit -m "perf(desktop): precompute O(1) maps for connection and credential rows"
```

---

### Task 4: Zero-Disk Idle Auto-Lock Watcher (Finding #40)

**Files:**
- Modify: `crates/sigrun-core/src/host.rs`
- Test: `crates/sigrun-core/src/host.rs` (tests module)

**Interfaces:**
- Consumes: `Store::load_settings()`, `Request::SettingsSet`, `HostState`
- Produces: `auto_lock_minutes: AtomicU32` cached on `HostState`

- [ ] **Step 1: Write failing test for cached auto-lock minutes**
In `crates/sigrun-core/src/host.rs` tests module:
Write `auto_lock_minutes_atomic_updates_on_settings_save`:
- Create host state with default settings (auto_lock_minutes = 0). Assert cached atomic is 0.
- Dispatch `Request::SettingsSet` with `auto_lock_minutes = 15`.
- Assert cached atomic is now 15 without having to read from disk.

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p sigrun-core host::tests::auto_lock_minutes_atomic`
Expected: FAIL (field not present on `HostState`)

- [ ] **Step 3: Implement atomic cache and lock-first check in `watch_auto_lock`**
In `crates/sigrun-core/src/host.rs`:
1. In `HostState`: add `auto_lock_minutes: std::sync::atomic::AtomicU32`.
   Initialize in `HostState::new`:
   ```rust
   let auto_lock_minutes = std::sync::atomic::AtomicU32::new(
       store.load_settings().map(|s| s.auto_lock_minutes as u32).unwrap_or(0),
   );
   ```
2. In `handle_inner` under `Request::SettingsSet`:
   ```rust
   state.auto_lock_minutes.store(settings.auto_lock_minutes as u32, std::sync::atomic::Ordering::Relaxed);
   ```
3. In `watch_auto_lock`:
   Check `if self.vault.lock().await.is_none() { continue; }` first.
   Then read `let minutes = self.auto_lock_minutes.load(std::sync::atomic::Ordering::Relaxed);`.
   If `minutes == 0 { continue; }`.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p sigrun-core host::tests`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add crates/sigrun-core/src/host.rs
git commit -m "perf(core): cache auto-lock minutes and skip disk reads when idle"
```

---

### Task 5: Documentation & Changelog

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update CHANGELOG.md**
Under `## [0.5.0] - 2026-10-22` -> `### Changed`:
Add bullets:
- `Discovered disk keys are batch-persisted and skip file rewrites when metadata is unchanged.`
- `The tray menu caches its model and skips redundant native rebuilds on window focus.`
- `Connection and credential views use indexed O(1) maps for row metadata resolution.`
- `The auto-lock watcher caches settings in memory and skips disk access when the vault is locked.`

- [ ] **Step 2: Run full gates**
```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build
```

- [ ] **Step 3: Commit**
```bash
git add CHANGELOG.md
git commit -m "docs: document phase 3b performance improvements"
```
