# Sigrun v1 — Core and CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the headless Sigrun core and `sigrun` CLI: encrypted vault, key discovery/generation, connection management, SSH agent, local host IPC, and terminal launch — a working `sigrun connect` on macOS, Linux, and Windows.

**Architecture:** A Cargo workspace with `sigrun-core` (all domain logic) and `sigrun-cli` (clap binary). A long-lived `sigrun host` process owns the unlocked vault and the OpenSSH-compatible agent; the CLI is a client over a Unix socket (named pipe on Windows). Platform-specific behavior is isolated in `paths`, `terminal`, `ipc`, and `agent`; every other module is pure Rust and testable without network.

**Tech Stack:** Rust 1.85+ (edition 2021), tokio, clap 4, serde/toml, Argon2id, XChaCha20-Poly1305, zeroize, `ssh-key` 0.6 + `ssh-agent-lib` 0.6, `fs2` file locks, `tempfile`.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md`

**This is Plan 1 of 2.** Plan 2 adds the Tauri 2/React window and systray, themes, i18n, the GitHub wizard, and the installer release workflow. Plan 1 must ship a working CLI before Plan 2 starts.

## Global Constraints

- Targets: macOS, Linux, Windows. Platform-specific code only in `paths.rs`, `terminal.rs`, `ipc/`, and `agent/` transport.
- Rust 1.85+, edition 2021. Binary name is `sigrun`.
- `~/.ssh` is **read-only**: discovery only. Never create, modify, or delete anything in it.
- No plaintext secret may reach disk, a log, a crash message, `argv`, or a clipboard. Secrets live in the encrypted vault or in memory in the agent.
- Vault: Argon2id (`m_cost_kib = 65536`, `t_cost = 3`, `p_cost = 4`), 16-byte random salt, XChaCha20-Poly1305, 24-byte random nonce per write. Vault file mode `0600` on Unix. Atomic replace on every write.
- Config/data locations per spec §5: macOS `~/Library/Application Support/Sigrun/`, Linux `~/.config/sigrun/` + `~/.local/share/sigrun/`, Windows `%APPDATA%\Sigrun\`. `SIGRUN_CONFIG_DIR`, `SIGRUN_DATA_DIR`, and `SIGRUN_SSH_DIR` override them (used by tests).
- CLI text is English in v1. No network calls in any test.
- `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets` must be green.
- MIT license, Keep a Changelog, semver `0.1.0` for this plan.
- Every task ends with a commit.

## Review Focus

These are the five failure modes most likely to bite a real user. Each has a test in the task that owns the code.

1. **Wildcard SSH config entries** (`Host *`, `Host foo?`, `Host !bar`) must never appear as connections. Test in Task 8.
2. **A passphrase-protected key with a wrong or missing passphrase** must fail with a clear error and must not panic or leak key material. Tests in Tasks 7 and 10.
3. **Shell metacharacters in host/user** must not allow command injection into the launched terminal. Host and user are validated, then shell-escaped. Test in Task 9.
4. **Vault corruption or tampering** must fail closed, distinguish a wrong passphrase from a damaged vault, and never truncate the original file. Test in Task 5.
5. **Concurrent CLI calls and double host starts** must not corrupt `data.toml` or start two agents. Tests in Tasks 3 and 12.

## File Structure

```
Cargo.toml                                   workspace
rust-toolchain.toml                          stable + rustfmt + clippy
.gitignore
LICENSE                                      MIT
CHANGELOG.md                                 Keep a Changelog
README.md                                    build/run/security notes
.github/workflows/ci.yml                     fmt, clippy, test on 3 OSes
crates/sigrun-core/
  Cargo.toml
  src/lib.rs                                 public re-exports
  src/error.rs                               one Error enum
  src/paths.rs                               platform dirs + atomic file writes
  src/model.rs                               Connection, KeyMetadata, Settings, KeyView
  src/store.rs                               TOML stores + file locking
  src/vault/mod.rs                           Vault, UnlockedVault, Secret, status
  src/vault/crypto.rs                        Argon2id + XChaCha20-Poly1305
  src/vault/store.rs                         encrypted envelope + lifecycle
  src/keys/mod.rs                            re-exports
  src/keys/generate.rs                       key generation + import
  src/keys/discovery.rs                      ~/.ssh scanning
  src/ssh_config.rs                          Host block parser + adopt
  src/terminal.rs                            ssh argv + terminal specs + launch
  src/agent/mod.rs                           AgentState, vault loading, serve
  src/agent/session.rs                       ssh-agent-lib Session + raw signing
  src/ipc/mod.rs                             re-exports
  src/ipc/protocol.rs                        Request/Response, newline JSON
  src/ipc/server.rs                          HostState + request handler + listener
  src/ipc/client.rs                          request/response client
  src/host.rs                                run the headless host
  tests/                                     cross-module integration tests
crates/sigrun-cli/
  Cargo.toml
  src/main.rs                                entry point + error printing
  src/cli.rs                                 clap definitions
  src/client.rs                              socket client + passphrase prompts
  tests/cli.rs                               assert_cmd end-to-end
```

---

### Task 1: Workspace, license, changelog, and CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `LICENSE`, `CHANGELOG.md`, `README.md`
- Create: `crates/sigrun-core/Cargo.toml`, `crates/sigrun-core/src/lib.rs`
- Create: `crates/sigrun-cli/Cargo.toml`, `crates/sigrun-cli/src/main.rs`
- Create: `.github/workflows/ci.yml`
- Test: `crates/sigrun-cli/tests/cli.rs`

**Interfaces:**
- Produces: `sigrun_core::VERSION: &'static str`; binary `sigrun` with clap `--version`.
- Produces: workspace dependency table used by every later task (copy into each crate as `workspace = true`).

- [ ] **Step 1: Create the workspace and both crates**

Root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/sigrun-core", "crates/sigrun-cli"]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.85"
license = "MIT"
repository = "https://github.com/mojoaar/sigrun"

[workspace.dependencies]
sigrun-core = { path = "crates/sigrun-core" }
anyhow = "1"
argon2 = "0.5"
assert_cmd = "2"
base64 = "0.22"
chacha20poly1305 = "0.10"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
dirs = "6"
ed25519-dalek = "2"
fs2 = "0.4"
p256 = "0.13"
predicates = "3"
rand = "0.8"
rsa = "0.9"
rpassword = "7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha1 = "0.10"
sha2 = "0.10"
signature = "2"
ssh-agent-lib = "0.6"
ssh-key = { version = "0.6", features = ["ed25519", "rsa", "p256", "encryption", "getrandom"] }
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-util", "time", "sync"] }
tokio-util = { version = "0.7", features = ["rt"] }
toml = "0.8"
uuid = { version = "1", features = ["v4"] }
zeroize = { version = "1", features = ["derive"] }
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

`.gitignore`:

```
/target
node_modules/
dist/
.DS_Store
*.log
```

`sigrun-core/src/lib.rs`:

```rust
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

`sigrun-cli/src/main.rs`:

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "sigrun", version, about = "Sigrun — victory over key chaos.")]
struct Cli {}

fn main() {
    Cli::parse();
}
```

Both `Cargo.toml` files use `[package] version.workspace = true`, `edition.workspace = true`, `license.workspace = true`, and `[dependencies]` with `workspace = true` entries. `sigrun-cli` depends on `sigrun-core`, `clap`, `anyhow`, `tokio`; dev-deps `assert_cmd`, `predicates`, `tempfile`.

`LICENSE` is the standard MIT text with `Copyright (c) 2026 Sigrun contributors`. `CHANGELOG.md` starts with `# Changelog`, the Keep a Changelog intro, and `## [Unreleased]`. `README.md` contains the tagline, a one-paragraph description, and `cargo run -p sigrun-cli -- --help`.

- [ ] **Step 2: Write the failing version test**

`crates/sigrun-cli/tests/cli.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn prints_version() {
    Command::cargo_bin("sigrun")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.1.0"));
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p sigrun-cli --test cli`
Expected: FAIL — clap has no `--version` until the derive is compiled with the test; if it already passes, confirm the binary name is `sigrun` and continue.

- [ ] **Step 4: Add CI**

`.github/workflows/ci.yml`:

```yaml
name: CI
on: [push, pull_request]
jobs:
  rust:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo test --all-targets
```

- [ ] **Step 5: Run the full check and commit**

Run: `cargo fmt --all && cargo test --all-targets && cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS.

```bash
git init
git add Cargo.toml rust-toolchain.toml .gitignore LICENSE CHANGELOG.md README.md .github crates docs
git commit -m "chore: scaffold Sigrun workspace, MIT license, changelog, CI"
```

---

### Task 2: Errors and platform paths

**Files:**
- Create: `crates/sigrun-core/src/error.rs`, `crates/sigrun-core/src/paths.rs`
- Modify: `crates/sigrun-core/src/lib.rs`
- Test: inline `#[cfg(test)]` modules in both files

**Interfaces:**
- Produces: `sigrun_core::error::{Error, Result}`.
- Produces: `Paths { config_dir, data_dir, ssh_dir }`, `Paths::resolve()`, `Paths::with_roots()`, `Paths::{settings_file, data_file, lock_file, vault_file, socket_path, agent_socket}()`.
- Produces: `ensure_private_dir(&Path) -> Result<()>`, `write_atomic(&Path, &[u8], u32) -> Result<()>`.

- [ ] **Step 1: Write the failing tests**

In `paths.rs`:

```rust
#[test]
fn with_roots_builds_expected_files() {
    let p = Paths::with_roots("/c".into(), "/d".into(), "/h/.ssh".into());
    assert_eq!(p.settings_file(), PathBuf::from("/c/settings.toml"));
    assert_eq!(p.data_file(), PathBuf::from("/d/data.toml"));
    assert_eq!(p.vault_file(), PathBuf::from("/d/vault.json"));
    assert_eq!(p.lock_file(), PathBuf::from("/d/data.lock"));
}

#[test]
fn env_overrides_are_used() {
    std::env::set_var("SIGRUN_CONFIG_DIR", "/tmp/c");
    std::env::set_var("SIGRUN_DATA_DIR", "/tmp/d");
    std::env::set_var("SIGRUN_SSH_DIR", "/tmp/ssh");
    let p = Paths::resolve().unwrap();
    assert_eq!(p.config_dir, PathBuf::from("/tmp/c"));
    assert_eq!(p.ssh_dir, PathBuf::from("/tmp/ssh"));
}

#[cfg(unix)]
#[test]
fn write_atomic_sets_mode_and_replaces() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.json");
    write_atomic(&path, b"one", 0o600).unwrap();
    write_atomic(&path, b"two", 0o600).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"two");
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core paths`
Expected: FAIL — `Paths` not found.

- [ ] **Step 3: Implement `error.rs`**

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml decode error: {0}")]
    TomlDecode(#[from] toml::de::Error),
    #[error("toml encode error: {0}")]
    TomlEncode(#[from] toml::ser::Error),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("vault is not initialized")]
    VaultUninitialized,
    #[error("vault is locked")]
    VaultLocked,
    #[error("invalid passphrase")]
    InvalidPassphrase,
    #[error("vault is corrupt: {0}")]
    VaultCorrupt(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("key error: {0}")]
    Key(String),
    #[error("agent error: {0}")]
    Agent(String),
    #[error("ipc error: {0}")]
    Ipc(String),
    #[error("another Sigrun host is already running")]
    AlreadyRunning,
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
}
pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 4: Implement `paths.rs`**

Use `dirs::config_dir()`, `dirs::data_dir()`, and `dirs::home_dir()`. App folder is `"sigrun"` on Linux and `"Sigrun"` elsewhere. `sqlite` is not used. `socket_path()` is `data_dir/host.sock` on Unix and `PathBuf::from(format!(r"\\.\pipe\sigrun-{user}"))` on Windows; `agent_socket()` is `data_dir/agent.sock` on Unix and `\\.\pipe\sigrun-agent-{user}` on Windows, where `user` is `USERNAME`/`USER` sanitized to `[A-Za-z0-9_-]`. `ensure_private_dir` creates the directory and sets `0700` on Unix. `write_atomic` writes a `tempfile::NamedTempFile` in the same directory, calls `sync_all`, sets the mode, then `persist(path)`.

- [ ] **Step 5: Run tests, format, and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core && cargo clippy -p sigrun-core --all-targets -- -D warnings`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(core): typed errors and platform paths with atomic writes"
```

---

### Task 3: Models, TOML stores, and file locking

**Files:**
- Create: `crates/sigrun-core/src/model.rs`, `crates/sigrun-core/src/store.rs`
- Modify: `crates/sigrun-core/src/lib.rs`
- Test: inline modules

**Interfaces:**
- Produces: `Connection { id, label, host, user, port, key_id, group, tags, source }`, `ConnectionSource::{Managed, Discovered}`, `Connection::managed(id, label) -> Connection`.
- Produces: `KeyMetadata { id, name, algorithm, public_key, fingerprint, source, disk_path, has_passphrase, group, tags, notes, created_at }`, `KeyMetadata::now(id, algorithm, public_key, fingerprint, source) -> KeyMetadata`, `KeySource::{Vault, Disk}`, `KeyAlgorithm::{Ed25519, Rsa, Ecdsa}`.
- Produces: `KeyView { #[serde(flatten)] key: KeyMetadata, available: bool }`.
- Produces: `DataFile { version, connections, keys }`, `Settings { version, theme, language, terminal, agent_enabled }`.
- Produces: `Store::new(Paths)`, `paths() -> &Paths`, `load_settings`, `save_settings`, `load_data`, `save_data`, `update_data`, `upsert_connection`, `remove_connection`, `upsert_key`, `remove_key`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn missing_files_return_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into()));
    assert_eq!(store.load_settings().unwrap().theme, "nordlys");
    assert!(store.load_data().unwrap().connections.is_empty());
}

#[test]
fn concurrent_writers_do_not_lose_connections() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(Store::new(Paths::with_roots(
        dir.path().into(), dir.path().into(), dir.path().into(),
    )));
    let mut handles = vec![];
    for i in 0..8 {
        let store = store.clone();
        handles.push(std::thread::spawn(move || {
            store.upsert_connection(Connection::managed(format!("c{i}"), format!("h{i}"))).unwrap();
        }));
    }
    for h in handles { h.join().unwrap(); }
    assert_eq!(store.load_data().unwrap().connections.len(), 8);
}

#[test]
fn future_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into()));
    std::fs::write(store.paths().data_file(), "version = 99\nconnections = []\nkeys = []\n").unwrap();
    assert!(matches!(store.load_data(), Err(Error::Config(_))));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core store`
Expected: FAIL — `Store` not found.

- [ ] **Step 3: Implement `model.rs`**

Use `serde` with `rename_all = "snake_case"` for enums. `Connection::managed(id, label)` creates `host = label`, `user = ""`, `port = 22`, `key_id = None`, `source = Managed`. `KeyMetadata::now()` sets `created_at = chrono::Utc::now().to_rfc3339()`. `Settings::default()` is `theme: "nordlys"`, `language: "en"`, `terminal: None`, `agent_enabled: true`. `DataFile::default()` is `version: 1`, empty vecs. Use `#[serde(default)]` on every field so future additions do not break old files.

- [ ] **Step 4: Implement `store.rs`**

`load_data` checks `version == 1` and returns `Error::Config(format!("unsupported data version {version}"))` otherwise. `update_data` opens `lock_file()`, calls `fs2::FileExt::lock_exclusive`, loads, applies `f`, saves, and unlocks on drop. `save_data`/`save_settings` use `write_atomic(..., 0o600)`; settings may be `0o600` too because it can hold a terminal path. `upsert_*` replaces by `id` or pushes; `remove_*` is a no-op if absent (use `Error::NotFound` for `remove_connection` so the CLI can report it).

- [ ] **Step 5: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core && cargo clippy -p sigrun-core --all-targets -- -D warnings`
Expected: PASS, including 8 connections after concurrent writes.

```bash
git add crates/sigrun-core
git commit -m "feat(core): connection/key models and lock-safe TOML stores"
```

---

### Task 4: Vault cryptography

**Files:**
- Create: `crates/sigrun-core/src/vault/crypto.rs`
- Create: `crates/sigrun-core/src/vault/mod.rs` (module wiring only)
- Test: inline module

**Interfaces:**
- Produces: `KdfParams { algorithm, m_cost_kib, t_cost, p_cost, salt_b64 }`, `KdfParams::new([u8;16])`, `KdfParams::salt()`.
- Produces: `derive_key(&str, &KdfParams) -> Result<Zeroizing<[u8;32]>>`.
- Produces: `Sealed { nonce: [u8;24], ciphertext: Vec<u8> }`.
- Produces: `seal(&[u8;32], &[u8]) -> Result<Sealed>`, `open(&[u8;32], &[u8;24], &[u8]) -> Result<Vec<u8>>`.
- Produces: `CHECK_PLAINTEXT: &[u8] = b"sigrun-vault-v1"`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn params_are_pinned() {
    let p = KdfParams::new([7u8; 16]);
    assert_eq!((p.m_cost_kib, p.t_cost, p.p_cost), (65_536, 3, 4));
    assert_eq!(p.algorithm, "argon2id");
    assert_eq!(p.salt().unwrap(), [7u8; 16]);
}

#[test]
fn seal_open_roundtrip() {
    let key = derive_key("correct horse", &KdfParams::new([1u8; 16])).unwrap();
    let sealed = seal(&key, b"secret").unwrap();
    assert_eq!(open(&key, &sealed.nonce, &sealed.ciphertext).unwrap(), b"secret");
}

#[test]
fn wrong_key_and_tamper_fail() {
    let key = derive_key("a", &KdfParams::new([2u8; 16])).unwrap();
    let other = derive_key("b", &KdfParams::new([2u8; 16])).unwrap();
    let sealed = seal(&key, b"secret").unwrap();
    assert!(open(&other, &sealed.nonce, &sealed.ciphertext).is_err());
    let mut ct = sealed.ciphertext.clone();
    ct[0] ^= 1;
    assert!(open(&key, &sealed.nonce, &ct).is_err());
}

#[test]
fn every_seal_has_a_fresh_nonce() {
    let key = derive_key("a", &KdfParams::new([3u8; 16])).unwrap();
    assert_ne!(seal(&key, b"x").unwrap().nonce, seal(&key, b"x").unwrap().nonce);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core vault::crypto`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Use `argon2::{Algorithm, Argon2, Params, Version}` and `hash_password_into`. Map its error to `Error::Config` (it is a parameter/encoding error, not a wrong passphrase). Use `chacha20poly1305::{XChaCha20Poly1305, XNonce, KeyInit, Aead}`. Base64-encode the salt with `base64::engine::general_purpose::STANDARD`. Map decryption failure to `Error::InvalidPassphrase` at this layer; the vault layer distinguishes check failure from payload failure.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core vault::crypto`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(vault): Argon2id key derivation and XChaCha20-Poly1305 sealing"
```

---

### Task 5: Encrypted vault lifecycle

**Files:**
- Create: `crates/sigrun-core/src/vault/store.rs`
- Modify: `crates/sigrun-core/src/vault/mod.rs`
- Test: inline module

**Interfaces:**
- Produces: `Vault::new(PathBuf)`, `Vault::path() -> &Path`, `Vault::exists()`, `Vault::status()`, `Vault::create(&str)`, `Vault::unlock(&str) -> Result<UnlockedVault>`.
- Produces: `UnlockedVault::{get, put, remove, iter, lock}`; `put` and `remove` persist immediately.
- Produces: `Secret::{PrivateKey { pem, passphrase }, Password { password }}`, `VaultStatus::{Uninitialized, Locked, Unlocked}`.
- Consumes: Task 4 crypto.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn create_unlock_put_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("vault.json"));
    vault.create("pass").unwrap();
    assert_eq!(vault.status(), VaultStatus::Locked);
    let mut v = vault.unlock("pass").unwrap();
    v.put("k1", Secret::Password { password: "p".into() }).unwrap();
    drop(v);
    let v = vault.unlock("pass").unwrap();
    assert!(matches!(v.get("k1"), Some(Secret::Password { password }) if password == "p"));
}

#[test]
fn wrong_passphrase_is_distinct_from_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("vault.json"));
    vault.create("pass").unwrap();
    assert!(matches!(vault.unlock("nope"), Err(Error::InvalidPassphrase)));

    let mut envelope: serde_json::Value =
        serde_json::from_slice(&std::fs::read(vault.path()).unwrap()).unwrap();
    envelope["ciphertext"] = "AAAA".into();
    std::fs::write(vault.path(), serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert!(matches!(vault.unlock("pass"), Err(Error::VaultCorrupt(_))));
}

#[test]
fn debug_does_not_leak_secrets() {
    let s = Secret::Password { password: "hunter2".into() };
    assert!(!format!("{s:?}").contains("hunter2"));
}

#[cfg(unix)]
#[test]
fn vault_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("vault.json"));
    vault.create("pass").unwrap();
    assert_eq!(std::fs::metadata(vault.path()).unwrap().permissions().mode() & 0o777, 0o600);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core vault::store`
Expected: FAIL — `Vault` not found.

- [ ] **Step 3: Implement the envelope**

`vault.json` (plaintext JSON envelope):

```json
{
  "version": 1,
  "kdf": { "algorithm": "argon2id", "m_cost_kib": 65536, "t_cost": 3, "p_cost": 4, "salt_b64": "..." },
  "check_nonce": "...",
  "check": "...",
  "nonce": "...",
  "ciphertext": "..."
}
```

`create` refuses if the file exists (`Error::AlreadyExists`). It derives the key, seals `CHECK_PLAINTEXT` and the empty payload, and writes atomically with mode `0600`.

`unlock` reads the envelope, checks `version == 1`, derives the key, opens `check`; a failure means `InvalidPassphrase`. It then opens `ciphertext`; a failure means `VaultCorrupt`. It parses the payload as JSON; a parse failure is `VaultCorrupt`. `UnlockedVault` keeps the derived key in `Zeroizing<[u8; 32]>` and the payload in memory. Every `put`/`remove` re-seals the payload with a fresh nonce and atomically rewrites the envelope.

`Secret` derives `Serialize`, `Deserialize`, `Zeroize`, `ZeroizeOnDrop`, and implements `Debug` manually as `PrivateKey { .. }` / `Password { .. }`.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core vault`
Expected: PASS, including the distinct wrong-passphrase/corruption errors.

```bash
git add crates/sigrun-core
git commit -m "feat(vault): encrypted vault lifecycle with atomic re-seal"
```

---

### Task 6: Key generation

**Files:**
- Create: `crates/sigrun-core/src/keys/mod.rs`, `crates/sigrun-core/src/keys/generate.rs`
- Modify: `crates/sigrun-core/src/lib.rs`
- Test: inline module

**Interfaces:**
- Produces: `GeneratedKey { private_pem, public_openssh, fingerprint, algorithm }`.
- Produces: `generate(KeyAlgorithm, Option<&str>) -> Result<GeneratedKey>`.
- Consumes: `KeyAlgorithm` from Task 3.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn ed25519_roundtrips_through_openssh() {
    let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
    let public = ssh_key::PublicKey::from_openssh(&k.public_openssh).unwrap();
    assert_eq!(public.fingerprint(Default::default()).to_string(), k.fingerprint);
    assert!(k.private_pem.contains("OPENSSH PRIVATE KEY"));
    assert_eq!(k.algorithm, KeyAlgorithm::Ed25519);
}

#[test]
fn passphrase_encrypts_private_key() {
    let k = generate(KeyAlgorithm::Ed25519, Some("pw")).unwrap();
    let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
    assert!(key.is_encrypted());
    assert!(key.decrypt("pw").is_ok());
    assert!(key.decrypt("wrong").is_err());
}

#[test]
fn empty_passphrase_is_treated_as_none() {
    let k = generate(KeyAlgorithm::Ed25519, Some("")).unwrap();
    assert!(!ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap().is_encrypted());
}

#[test]
fn rsa_and_ecdsa_are_supported() {
    let rsa = generate(KeyAlgorithm::Rsa, None).unwrap();
    assert!(rsa.public_openssh.starts_with("ssh-rsa "));
    let ecdsa = generate(KeyAlgorithm::Ecdsa, None).unwrap();
    assert!(ecdsa.public_openssh.starts_with("ecdsa-sha2-nistp256 "));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core keys::generate`
Expected: FAIL — `generate` not found.

- [ ] **Step 3: Implement**

Map `KeyAlgorithm` to `ssh_key::Algorithm`: `Ed25519` → `Algorithm::Ed25519`; `Rsa` → `Algorithm::Rsa { hash: None }`; `Ecdsa` → `Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 }`. Use `PrivateKey::random(&mut rand::rngs::OsRng, algorithm)`. Treat `Some("")` as `None`. If a passphrase is present, call `key.encrypt(&mut OsRng, passphrase)`. Serialize with `to_openssh(ssh_key::LineEnding::LF)`. Fingerprint with `key.public_key().fingerprint(Default::default()).to_string()`.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core keys::generate`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(keys): generate ed25519, RSA, and ECDSA keys"
```

---

### Task 7: Key discovery and import

**Files:**
- Create: `crates/sigrun-core/src/keys/discovery.rs`
- Modify: `crates/sigrun-core/src/keys/mod.rs`
- Test: inline module

**Interfaces:**
- Produces: `DiscoveredKey { path, public_key, fingerprint, algorithm, has_passphrase }`.
- Produces: `DiscoveryResult { keys: Vec<DiscoveredKey>, warnings: Vec<String> }`.
- Produces: `discover_keys(&Path) -> Result<DiscoveryResult>`, `read_key(&Path) -> Result<DiscoveredKey>`.
- Produces: `import_private_key(&Path, Option<&str>) -> Result<ImportedKey>` where `ImportedKey { private_pem, public_key, fingerprint, algorithm }` and the returned PEM is decrypted.

- [ ] **Step 1: Write the failing tests**

```rust
fn write_key(dir: &Path, name: &str, passphrase: Option<&str>) -> PathBuf {
    let k = generate(KeyAlgorithm::Ed25519, passphrase).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, k.private_pem).unwrap();
    std::fs::write(dir.join(format!("{name}.pub")), &k.public_openssh).unwrap();
    path
}

#[test]
fn discovers_plain_and_encrypted_keys() {
    let dir = tempfile::tempdir().unwrap();
    write_key(dir.path(), "id_ed25519", None);
    write_key(dir.path(), "id_work", Some("pw"));
    std::fs::write(dir.path().join("known_hosts"), "junk").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "junk").unwrap();
    let found = discover_keys(dir.path()).unwrap();
    assert_eq!(found.keys.len(), 2);
    assert!(found.keys.iter().any(|k| k.has_passphrase));
}

#[test]
fn encrypted_key_without_pub_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let k = generate(KeyAlgorithm::Ed25519, Some("pw")).unwrap();
    let path = dir.path().join("id_secret");
    std::fs::write(&path, k.private_pem).unwrap();
    assert!(matches!(read_key(&path), Err(Error::Key(_))));
}

#[test]
fn wrong_import_passphrase_is_invalid_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_key(dir.path(), "id_locked", Some("right"));
    assert!(matches!(import_private_key(&path, Some("wrong")), Err(Error::InvalidPassphrase)));
    assert!(import_private_key(&path, Some("right")).is_ok());
    assert!(matches!(import_private_key(&path, None), Err(Error::InvalidInput(_))));
}

#[test]
fn dedupes_by_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_key(dir.path(), "id_ed25519", None);
    std::fs::copy(&path, dir.path().join("copy")).unwrap();
    std::fs::copy(dir.path().join("id_ed25519.pub"), dir.path().join("copy.pub")).unwrap();
    assert_eq!(discover_keys(dir.path()).unwrap().keys.len(), 1);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core keys::discovery`
Expected: FAIL — `discover_keys` not found.

- [ ] **Step 3: Implement**

Skip directories and the names `config`, `known_hosts`, `known_hosts.old`, `authorized_keys`, `authorized_keys2`, `environment`, `rc`, and anything ending in `.pub`. For each remaining file, call `PrivateKey::read_openssh_file`. If it parses and is not encrypted, derive the public key. If it is encrypted, read the `.pub` companion (`path.with_extension("pub")`); if it is missing, push a warning and skip. Dedupe by fingerprint. Sort by path for deterministic output.

`read_key` is the single-file variant and returns `Error::Key("passphrase-protected key has no .pub companion; import it with a passphrase")` when the encrypted key has no companion.

`import_private_key`: read the file; if encrypted and no passphrase, `Error::InvalidInput("key is encrypted; a passphrase is required")`; if decryption fails, `Error::InvalidPassphrase`. Verify a `.pub` companion if present and return `Error::Key("public key does not match private key")` on mismatch. Return the decrypted PEM.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core keys::discovery`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(keys): discover ~/.ssh keys and import protected keys"
```

---

### Task 8: `~/.ssh/config` discovery and adopt

**Files:**
- Create: `crates/sigrun-core/src/ssh_config.rs`
- Modify: `crates/sigrun-core/src/lib.rs`, `crates/sigrun-core/src/model.rs`
- Test: inline module

**Interfaces:**
- Produces: `SshHost { alias, host_name, user, port, identity_file }`.
- Produces: `parse_ssh_config(&str) -> Vec<SshHost>`, `discover_ssh_config(&Path) -> Result<Vec<SshHost>>`.
- Produces: `Connection::from_ssh_host(&SshHost, String) -> Connection` with `source = Discovered`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parses_blocks_and_first_value_wins() {
    let text = r#"
Host web prod
    HostName 10.0.0.5
    HostName ignored.example
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_work

Host db
    HostName db.internal
"#;
    let hosts = parse_ssh_config(text);
    assert_eq!(hosts.len(), 3);
    let web = hosts.iter().find(|h| h.alias == "web").unwrap();
    assert_eq!(web.host_name, "10.0.0.5");
    assert_eq!(web.user.as_deref(), Some("deploy"));
    assert_eq!(web.port, Some(2222));
}

#[test]
fn wildcard_patterns_are_never_connections() {
    let text = "Host *\n  User root\n\nHost app?\n  User x\n\nHost !bad good\n  User ok\n";
    let hosts = parse_ssh_config(text);
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "good");
}

#[test]
fn comments_and_global_defaults_are_ignored() {
    let text = "# global\nUser global\n\nHost box # trailing\n  HostName box.local\n";
    let hosts = parse_ssh_config(text);
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].host_name, "box.local");
}

#[test]
fn adopt_copies_fields() {
    let h = SshHost {
        alias: "web".into(),
        host_name: "10.0.0.5".into(),
        user: Some("deploy".into()),
        port: Some(2222),
        identity_file: None,
    };
    let c = Connection::from_ssh_host(&h, "id1".into());
    assert_eq!(c.label, "web");
    assert_eq!(c.host, "10.0.0.5");
    assert_eq!(c.user, "deploy");
    assert_eq!(c.port, 2222);
    assert_eq!(c.source, ConnectionSource::Discovered);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core ssh_config`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Line parsing: strip at the first `#`; trim; skip empty. Split on whitespace; `key = tokens[0].to_ascii_lowercase()`, value = `tokens[1..].join(" ")`. Only `host` starts a block; `match` is not treated as `host` (v1). Values `hostname`, `user`, `port`, `identityfile` are recorded only if not already set in the block. Global lines before the first `Host` are ignored. A block with patterns containing `*`, `?`, or `!` contributes only its non-wildcard aliases; if none remain, the block is dropped. `HostName` defaults to the alias. Expand a leading `~` with `dirs::home_dir()`. Invalid ports are ignored.

Add a one-line comment for the known limit: `// shortcut: Include is not followed in v1; add when a user relies on it.`

`discover_ssh_config` returns an empty vec if the file is missing.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core ssh_config`
Expected: PASS, and the wildcard test returns exactly one host.

```bash
git add crates/sigrun-core
git commit -m "feat(connections): parse ssh_config hosts and adopt them"
```

---

### Task 9: SSH command and terminal launch

**Files:**
- Create: `crates/sigrun-core/src/terminal.rs`
- Modify: `crates/sigrun-core/src/lib.rs`
- Test: inline module

**Interfaces:**
- Produces: `Platform::{MacOs, Linux, Windows}`, `Platform::current()`.
- Produces: `ssh_invocation(&Connection, Option<&KeyMetadata>) -> Result<Vec<String>>`.
- Produces: `LaunchSpec { program, args }`, `terminal_launch_specs(Platform, Option<&str>, &[String]) -> Result<Vec<LaunchSpec>>`, `launch_first(&[LaunchSpec], Option<&Path>) -> Result<()>`.
- Produces: `shell_escape(&str) -> String`, `render_command(&[String]) -> String`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn ssh_invocation_uses_port_and_disk_key() {
    let mut c = Connection::managed("id".into(), "web".into());
    c.host = "example.com".into();
    c.user = "deploy".into();
    c.port = 2222;
    let mut k = KeyMetadata::now("k".into(), KeyAlgorithm::Ed25519, "ssh-ed25519 AAAA".into(), "SHA256:x".into(), KeySource::Disk);
    k.disk_path = Some("/home/u/.ssh/id_work".into());
    assert_eq!(
        ssh_invocation(&c, Some(&k)).unwrap(),
        vec!["ssh", "deploy@example.com", "-p", "2222", "-i", "/home/u/.ssh/id_work", "-o", "IdentitiesOnly=yes"]
    );
}

#[test]
fn vault_key_is_served_by_the_agent_without_dash_i() {
    let c = Connection::managed("id".into(), "web".into());
    let k = KeyMetadata::now("k".into(), KeyAlgorithm::Ed25519, "ssh-ed25519 AAAA".into(), "SHA256:x".into(), KeySource::Vault);
    let argv = ssh_invocation(&c, Some(&k)).unwrap();
    assert!(!argv.iter().any(|a| a == "-i"));
    assert!(!argv.iter().any(|a| a == "IdentitiesOnly=yes"));
}

#[test]
fn shell_metacharacters_are_rejected_not_injected() {
    let mut c = Connection::managed("id".into(), "web".into());
    c.host = "example.com; rm -rf /".into();
    assert!(matches!(ssh_invocation(&c, None), Err(Error::InvalidInput(_))));
    let mut c = Connection::managed("id".into(), "web".into());
    c.user = "deploy$(id)".into();
    assert!(matches!(ssh_invocation(&c, None), Err(Error::InvalidInput(_))));
}

#[test]
fn escaping_quotes_posix_metacharacters() {
    assert_eq!(shell_escape("plain-host"), "plain-host");
    assert_eq!(shell_escape("a b"), "'a b'");
    assert_eq!(shell_escape("a'b"), "'a'\\''b'");
}

#[test]
fn macos_uses_terminal_app_by_default() {
    let specs = terminal_launch_specs(Platform::MacOs, None, &["ssh".into(), "u@h".into()]).unwrap();
    assert_eq!(specs[0].program, "osascript");
    assert!(specs[0].args.iter().any(|a| a.contains("Terminal")));
}

#[test]
fn linux_prefers_override_then_env_then_known_terminals() {
    let specs = terminal_launch_specs(Platform::Linux, Some("konsole"), &["ssh".into()]).unwrap();
    assert_eq!(specs[0].program, "konsole");
    assert!(specs.iter().any(|s| s.program == "gnome-terminal"));
}

#[test]
fn windows_prefers_windows_terminal() {
    let specs = terminal_launch_specs(Platform::Windows, None, &["ssh".into()]).unwrap();
    assert_eq!(specs[0].program, "wt.exe");
    assert!(specs.iter().any(|s| s.program == "cmd.exe"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core terminal`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Validate `host` with `[A-Za-z0-9._-]+` and `user` (when non-empty) with `[A-Za-z0-9._-]+`; reject anything else with `Error::InvalidInput("host contains unsupported characters")`. Reject port `0`. This is the injection boundary; `shell_escape` is the second layer.

`ssh_invocation`: `["ssh", "user@host" or "host", "-p", "<port>"]`, then `-i <disk_path> -o IdentitiesOnly=yes` only for `KeySource::Disk`. A vault-backed key adds nothing; the agent supplies it.

`terminal_launch_specs`:
- macOS: `osascript` with `tell application "Terminal" to do script "<cmd>"` and an activate line. If `configured` is `iterm`, use iTerm's `create window with default profile command "<cmd>"`. Escape `\` and `"` for AppleScript.
- Linux: ordered candidates = configured, `$TERMINAL`, `gnome-terminal`, `konsole`, `x-terminal-emulator`, `xterm`. `gnome-terminal` uses `["--", "sh", "-c", cmd]`; the others use `["-e", "sh", "-c", cmd]`. Return one spec per candidate so `launch_first` can fall back.
- Windows: `wt.exe` with `["new-tab", "--", "cmd.exe", "/k", cmd]`, then `cmd.exe` with `["/k", cmd]`. If `configured` is set, treat it as the program with `["/k", cmd]`.

`launch_first` spawns each spec with `std::process::Command`, sets `SSH_AUTH_SOCK` to the agent socket when provided, and returns the first success. Include the last spawn error in the returned `Error::Config`.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core terminal`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(connect): build ssh argv and launch the user's terminal safely"
```

---

### Task 10: SSH agent session and raw signing

**Files:**
- Create: `crates/sigrun-core/src/agent/mod.rs`, `crates/sigrun-core/src/agent/session.rs`
- Modify: `crates/sigrun-core/src/lib.rs`
- Test: inline module

**Interfaces:**
- Produces: `AgentKey { key_id, private_key: ssh_key::PrivateKey, comment }`.
- Produces: `AgentState` (cloneable) with `new`, `replace(Vec<AgentKey>)`, `clear`, `contains_key_id(&str) -> bool`, `len()`.
- Produces: `SigrunAgent` implementing `ssh_agent_lib::agent::Session`.
- Produces: `sign_raw(&PrivateKey, flags: u32, data: &[u8]) -> Result<Signature>`.
- Produces: `vault_keys(&UnlockedVault, &[KeyMetadata]) -> (Vec<AgentKey>, Vec<String>)`; warnings are human-readable and contain no secret material.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn identities_are_listed() {
    let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
    let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
    let state = AgentState::new();
    state.replace(vec![AgentKey { key_id: "k1".into(), private_key: key, comment: "k1".into() }]);
    assert!(state.contains_key_id("k1"));
    assert_eq!(state.len(), 1);
}

#[test]
fn ed25519_signature_verifies() {
    let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
    let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
    let sig = sign_raw(&key, 0, b"hello").unwrap();
    let public = ssh_key::PublicKey::from_openssh(&k.public_openssh).unwrap();
    let ed = match public.key_data() {
        ssh_key::public::KeyData::Ed25519(p) => p,
        _ => panic!("wrong type"),
    };
    let seed: &[u8; 32] = ed.as_ref().try_into().unwrap();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(seed).unwrap();
    let bytes: [u8; 64] = sig.as_bytes().try_into().unwrap();
    assert!(vk.verify_strict(b"hello", &ed25519_dalek::Signature::from_bytes(&bytes)).is_ok());
}

#[test]
fn rsa_and_ecdsa_signatures_verify() {
    for alg in [KeyAlgorithm::Rsa, KeyAlgorithm::Ecdsa] {
        let k = generate(alg, None).unwrap();
        let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
        assert!(sign_raw(&key, 0, b"hello").is_ok(), "{alg:?}");
    }
}

#[tokio::test]
async fn request_for_an_unknown_key_errors() {
    let state = AgentState::new();
    let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
    let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
    let mut agent = SigrunAgent { state };
    let req = ssh_agent_lib::proto::SignRequest {
        credential: key.public_key().key_data().clone().into(),
        data: b"x".to_vec(),
        flags: 0,
    };
    assert!(ssh_agent_lib::agent::Session::sign(&mut agent, req).await.is_err());
}

#[test]
fn encrypted_vault_key_is_decrypted_for_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("vault.json"));
    vault.create("pass").unwrap();
    let mut v = vault.unlock("pass").unwrap();
    let k = generate(KeyAlgorithm::Ed25519, Some("keypw")).unwrap();
    v.put("k1", Secret::PrivateKey { pem: k.private_pem, passphrase: Some("keypw".into()) }).unwrap();
    let meta = KeyMetadata::now("k1".into(), KeyAlgorithm::Ed25519, k.public_openssh, k.fingerprint, KeySource::Vault);
    let (keys, warnings) = vault_keys(&v, &[meta]);
    assert_eq!(keys.len(), 1);
    assert!(warnings.is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core agent`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`AgentState` holds `Arc<std::sync::RwLock<Vec<AgentKey>>>`.

`sign_raw` matches `private_key.key_data()`:
- `KeypairData::Ed25519` → `ed25519_dalek::SigningKey` from the 32-byte seed; `ssh_key::Signature::new(Algorithm::Ed25519, sig.to_bytes().to_vec())`.
- `KeypairData::Rsa` → rebuild `rsa::RsaPrivateKey` with `from_components(n, e, d, primes)` using `keypair.public.n`, `keypair.public.e`, `keypair.private.d`, and `keypair.private.primes` as big-endian bytes; pick SHA-512 when `flags & ssh_agent_lib::proto::signature::RSA_SHA2_512 != 0`, SHA-256 when `RSA_SHA2_256`, otherwise SHA-1; return `Algorithm::Rsa { hash: Some(...) }`.
- `KeypairData::Ecdsa` → P-256 only; `p256::ecdsa::SigningKey` from the scalar; sign `data`; concatenate `r || s` (64 bytes) and return `Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 }`.
- Anything else → `Error::Agent("unsupported key type")`.

`SigrunAgent::request_identities` maps each key to `ssh_agent_lib::proto::Identity { credential: key.private_key.public_key().key_data().clone().into(), comment }`. `sign` looks up the key by public-key equality, returning `AgentError::other("identity not found")` otherwise. Do not implement add/remove; the defaults return "unsupported command".

`vault_keys` iterates vault secrets. For each `Secret::PrivateKey`, parse the PEM; if encrypted, decrypt with the stored passphrase (a missing or wrong passphrase produces a warning, not a failure); push `AgentKey` with the metadata name as comment. Skip secrets without matching metadata. Warnings must read like `key "work" could not be loaded: invalid passphrase` and must never contain PEM text.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core agent`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(agent): in-memory SSH agent with ed25519, RSA, and ECDSA signing"
```

---

### Task 11: Agent socket transport

**Files:**
- Modify: `crates/sigrun-core/src/agent/mod.rs`
- Test: inline module, `#[cfg(unix)]` and `#[cfg(windows)]` variants

**Interfaces:**
- Produces: `serve(addr: &Path, state: AgentState) -> Result<tokio::task::JoinHandle<()>>`.
- Consumes: Task 10 `SigrunAgent`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(unix)]
#[tokio::test]
async fn serves_identities_over_a_unix_socket() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let addr = dir.path().join("agent.sock");
    let state = AgentState::new();
    let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
    let key = ssh_key::PrivateKey::from_openssh(&k.private_pem).unwrap();
    state.replace(vec![AgentKey { key_id: "k1".into(), private_key: key, comment: "k1".into() }]);

    let handle = serve(&addr, state).await.unwrap();
    assert_eq!(std::fs::metadata(&addr).unwrap().permissions().mode() & 0o777, 0o600);

    let stream = tokio::net::UnixStream::connect(&addr).await.unwrap();
    let mut client = ssh_agent_lib::client::AgentClient::new(stream);
    let ids = client.request_identities().await.unwrap();
    assert_eq!(ids.len(), 1);
    handle.abort();
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core agent::tests::serves_identities`
Expected: FAIL — `serve` not found.

- [ ] **Step 3: Implement**

Unix: remove a stale socket file, `UnixListener::bind`, set permissions `0600`, then spawn `ssh_agent_lib::agent::listen(listener, SigrunAgent { state })`. Windows: `NamedPipeListener::bind(addr.as_os_str())` with the `sigrun-agent-<user>` pipe from `Paths`, then the same `listen`. Map `AgentError` to `Error::Agent`. Return the spawned task handle so the host can abort it on shutdown.

The host sets `SSH_AUTH_SOCK` to this address for every launched terminal.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core agent`
Expected: PASS on the host platform; the Windows variant is exercised in CI.

```bash
git add crates/sigrun-core
git commit -m "feat(agent): serve the OpenSSH agent protocol on a private socket"
```

---

### Task 12: IPC protocol and headless host

**Files:**
- Create: `crates/sigrun-core/src/ipc/mod.rs`, `protocol.rs`, `server.rs`, `client.rs`
- Create: `crates/sigrun-core/src/host.rs`
- Modify: `crates/sigrun-core/src/lib.rs`
- Test: inline module + integration test

**Interfaces:**
- Produces: `Request`, `Response`, `StatusInfo`, `encode/decode`.
- Produces: `HostState { paths, store, vault, agent, agent_socket, shutdown, lock_file: std::fs::File }` (the lock file stays alive for the host's lifetime); `handle(&HostState, Request) -> Response` (async); `serve(&Path, Arc<HostState>) -> Result<()>`.
- Produces: `client::request(&Path, &Request) -> Result<Response>`.
- Produces: `sync_disk_keys(&Store, &Path) -> Result<Vec<KeyView>>`.
- Produces: `error_code(&Error) -> &'static str`.
- Produces: `host::run(Paths, Option<PathBuf>) -> Result<()>`; the second argument overrides the IPC socket path.
- Consumes: Tasks 3, 5, 7, 8, 9, 10, 11.

**Request/Response shape:**

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    VaultInit { passphrase: String },
    VaultUnlock { passphrase: String },
    VaultLock,
    ConnectionsList,
    ConnectionsAdd { connection: Connection },
    ConnectionsUpdate { connection: Connection },
    ConnectionsRemove { id: String },
    ConnectionsConnect { id: String, print_only: bool },
    KeysList,
    KeysGenerate { name: String, algorithm: KeyAlgorithm, passphrase: Option<String> },
    KeysImport { path: PathBuf, name: String, passphrase: Option<String> },
    KeysRemove { id: String },
    Shutdown,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Status(StatusInfo),
    Connections(Vec<Connection>),
    Keys(Vec<KeyView>),
    Key(KeyView),
    Command(String),
    Unlocked { warnings: Vec<String> },
    Error { code: String, message: String },
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn status_and_vault_roundtrip_over_ipc() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host.sock");
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let state = std::sync::Arc::new(HostState::new(paths).await.unwrap());
    let server = tokio::spawn({
        let state = state.clone();
        let sock = sock.clone();
        async move { serve(&sock, state).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let r = client::request(&sock, &Request::VaultInit { passphrase: "pass".into() }).await.unwrap();
    assert!(matches!(r, Response::Ok));
    let r = client::request(&sock, &Request::VaultUnlock { passphrase: "pass".into() }).await.unwrap();
    assert!(matches!(r, Response::Unlocked { .. }));

    state.shutdown.cancel();
    let _ = server.await;
}

#[tokio::test]
async fn second_host_refuses_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let first = std::sync::Arc::new(HostState::new(paths.clone()).await.unwrap());
    assert!(matches!(HostState::new(paths).await, Err(Error::AlreadyRunning)));
    first.shutdown.cancel();
}

#[tokio::test]
async fn sync_includes_keys_referenced_outside_the_ssh_dir() {
    let dir = tempfile::tempdir().unwrap();
    let ssh = dir.path().join("ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let outside = dir.path().join("id_external");
    let k = generate(KeyAlgorithm::Ed25519, None).unwrap();
    std::fs::write(&outside, &k.private_pem).unwrap();
    std::fs::write(dir.path().join("id_external.pub"), &k.public_openssh).unwrap();
    std::fs::write(
        ssh.join("config"),
        format!("Host ext\n  HostName ext.example\n  IdentityFile {}\n", outside.display()),
    ).unwrap();
    let store = Store::new(Paths::with_roots(dir.path().into(), dir.path().into(), ssh.clone()));
    let views = sync_disk_keys(&store, &ssh).unwrap();
    assert!(views.iter().any(|v| v.key.disk_path.as_deref() == Some(outside.as_path())));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core ipc`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `protocol.rs`**

Newline-delimited JSON: `encode` appends `\n`; `decode` parses one line. Reject lines over 1 MiB. `StatusInfo { version, vault: VaultStatus, agent_socket: Option<String>, host_addr: String }`.

- [ ] **Step 4: Implement `HostState` and `handle`**

`HostState::new(paths)`:
1. `ensure_private_dir` for config and data.
2. Acquire `paths.lock_file()` with `fs2::try_lock_exclusive`; on failure return `Error::AlreadyRunning`. Hold the file handle in the state.
3. Create `Store`; `vault = tokio::sync::Mutex::new(None)`; `agent = AgentState::new()`.
4. If `vault.exists()`, leave it locked; otherwise status is `Uninitialized`.

`handle`:
- `Status`: report `VaultStatus` and the agent socket.
- `VaultInit`: fail if exists; `spawn_blocking` create; return `Ok`.
- `VaultUnlock`: `spawn_blocking` unlock; on success `vault_keys(...)` and `agent.replace(keys)`; return `Unlocked { warnings }`.
- `VaultLock`: take the vault, drop it, `agent.clear()`.
- `ConnectionsList`: `store.load_data().connections`.
- `ConnectionsAdd/Update/Remove`: `store.upsert_connection` / `remove_connection`.
- `KeysList`: call `sync_disk_keys(&store, &paths.ssh_dir)`: run `discover_keys`, add `identity_file` paths from `parse_ssh_config(&ssh_dir/config)` when they live outside the scanned directory (deduped by fingerprint), upsert metadata by `disk_path`/fingerprint, and return `KeyView`s where `available` is true only if the file still exists.
- `KeysGenerate`: require unlocked vault; `keys::generate`; store `Secret::PrivateKey`; insert `KeyMetadata` with `source = Vault`; reload agent keys; return `Key`.
- `KeysImport`: require unlocked vault; `import_private_key`; store decrypted PEM; insert metadata; reload agent; return `Key`.
- `KeysRemove`: remove metadata; if `source = Vault`, remove the secret and reload agent.
- `ConnectionsConnect`: load the connection and settings; resolve `key_id`; if the key is vault-backed and the vault is locked, return `Error::VaultLocked`; if it is vault-backed and absent from the agent, reload agent keys and retry once. Build `ssh_invocation`. If `print_only`, return `Command(render_command(&argv))`. Otherwise resolve `terminal_launch_specs(Platform::current(), settings.terminal.as_deref(), &argv)` and `launch_first(&specs, Some(&paths.agent_socket()))`.
- `Shutdown`: cancel `state.shutdown` and return `Ok`.
- Any internal error becomes `Response::Error { code, message }` via `error_code(&Error)`. Define `error_code` in `protocol.rs` as a `match` returning `"vault_uninitialized"`, `"vault_locked"`, `"invalid_passphrase"`, `"vault_corrupt"`, `"not_found"`, `"already_exists"`, `"invalid_input"`, `"invalid_config"`, `"already_running"`, `"ipc"`, and `"internal"`. Never put secret values in `message`.

- [ ] **Step 5: Implement `server.rs` and `client.rs`**

`serve`: bind `UnixListener` (remove a stale socket after a failed connect) or a Windows named pipe; accept in a loop with `tokio::select!` on `state.shutdown.cancelled()`. Each connection reads lines, decodes, calls `handle`, writes the encoded response, and repeats until EOF. On shutdown, remove the Unix socket.

`client::request`: connect with a 2-second timeout, write one line, read one line with a 30-second timeout (Argon2 can take ~200 ms), decode. Translate connection refusal into `Error::Ipc("Sigrun host is not running; start it with: sigrun host")`.

- [ ] **Step 6: Implement `host.rs`**

```rust
pub async fn run(paths: Paths, socket: Option<PathBuf>) -> Result<()> {
    let state = std::sync::Arc::new(HostState::new(paths.clone()).await?);
    let agent = agent::serve(&paths.agent_socket(), state.agent.clone()).await?;
    let addr = socket.unwrap_or_else(|| paths.socket_path());
    let result = ipc::server::serve(&addr, state.clone()).await;
    agent.abort();
    state.agent.clear();
    result
}
```

- [ ] **Step 7: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-core ipc && cargo clippy -p sigrun-core --all-targets -- -D warnings`
Expected: PASS.

```bash
git add crates/sigrun-core
git commit -m "feat(host): headless host, IPC protocol, vault/key/connection handlers"
```

---

### Task 13: `sigrun` CLI

**Files:**
- Create: `crates/sigrun-cli/src/cli.rs`, `crates/sigrun-cli/src/client.rs`
- Modify: `crates/sigrun-cli/src/main.rs`
- Test: `crates/sigrun-cli/tests/cli.rs`

**Interfaces:**
- Produces: commands `host`, `status`, `vault init|unlock|lock`, `connections list|add|edit|remove|connect`, `keys list|generate|import|remove`.
- Global `--socket <path>`; `--json` on list commands; `--print` on connect; `--yes` on destructive commands.
- Passphrases come from an interactive prompt or `--passphrase-stdin` (one line on stdin). Never from `argv`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn status_without_a_host_explains_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("sigrun").unwrap()
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .arg("--socket").arg(dir.path().join("missing.sock"))
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("sigrun host"));
}

#[test]
fn end_to_end_host_vault_key_and_connection() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host.sock");
    let mut host = std::process::Command::new(assert_cmd::cargo::cargo_bin("sigrun"))
        .env("SIGRUN_CONFIG_DIR", dir.path())
        .env("SIGRUN_DATA_DIR", dir.path())
        .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
        .args(["host", "--socket"]).arg(&sock)
        .spawn().unwrap();
    wait_for(|| sock.exists());

    let run = |args: &[&str], stdin: &str| {
        let mut cmd = Command::cargo_bin("sigrun").unwrap();
        cmd.env("SIGRUN_CONFIG_DIR", dir.path())
            .env("SIGRUN_DATA_DIR", dir.path())
            .env("SIGRUN_SSH_DIR", dir.path().join("ssh"))
            .arg("--socket").arg(&sock).args(args)
            .write_stdin(stdin);
        cmd
    };

    run(&["vault", "init", "--passphrase-stdin"], "pass\n").assert().success();
    run(&["vault", "unlock", "--passphrase-stdin"], "pass\n").assert().success();
    run(&["keys", "generate", "--name", "work", "--algorithm", "ed25519", "--passphrase-stdin"], "\n")
        .assert().success().stdout(predicate::str::contains("ssh-ed25519"));
    run(&["connections", "add", "--label", "web", "--host", "example.com", "--user", "deploy"], "")
        .assert().success();
    run(&["connections", "add", "--label", "db", "--host", "db.example.com", "--user", "deploy", "--group", "prod"], "")
        .assert().success();
    run(&["connections", "list", "--json"], "")
        .assert().success().stdout(predicate::str::contains("\"web\""));
    run(&["connections", "list", "--search", "web", "--json"], "")
        .assert().success()
        .stdout(predicate::str::contains("\"web\"").and(predicate::str::contains("\"db\"").not()));
    run(&["connections", "list", "--group", "prod", "--json"], "")
        .assert().success().stdout(predicate::str::contains("\"db\""));
    run(&["connections", "connect", "web", "--print"], "")
        .assert().success().stdout(predicate::str::contains("ssh deploy@example.com -p 22"));

    host.kill().unwrap();
    host.wait().unwrap();
}
```

`wait_for` is a helper polling for up to 5 seconds.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-cli`
Expected: FAIL — subcommands are missing.

- [ ] **Step 3: Implement clap and the client**

`Cli` has `#[arg(long, global = true)] socket: Option<PathBuf>` and a required subcommand. `host` calls `sigrun_core::host::run(Paths::resolve()?, cli.socket.clone()).await`; the global `--socket` overrides only the IPC address. `status` prints `host: running`, `vault: <status>`, `agent: <path>`. Lists print aligned columns; with `--json`, `serde_json::to_string_pretty`.

`connections list` accepts `--search <term>` (case-insensitive match on label, host, or user) and `--group <name>`; `add` and `edit` accept `--group`. `connections connect <id-or-label>` resolves the argument by exact `id` first, then by a unique case-insensitive `label`; an ambiguous label is an `invalid_input` error listing the matching ids.

`read_passphrase(stdin: bool, confirm: bool)` uses `rpassword::prompt_password` and, for init, prompts twice and compares. With `--passphrase-stdin`, read one line and trim the trailing newline; an empty line means `None` for key passphrases and is invalid for the master passphrase.

Map `Response::Error { code, message }` to an `anyhow!` error prefixed with `code`. `connect` without `--print` reports success and leaves the terminal open.

- [ ] **Step 4: Run tests and commit**

Run: `cargo fmt --all && cargo test -p sigrun-cli && cargo clippy -p sigrun-cli --all-targets -- -D warnings`
Expected: PASS.

```bash
git add crates/sigrun-cli
git commit -m "feat(cli): sigrun host, vault, keys, connections, and connect commands"
```

---

### Task 14: End-to-end hardening, docs, and release metadata

**Files:**
- Modify: `README.md`, `CHANGELOG.md`
- Create: `crates/sigrun-core/tests/end_to_end.rs`
- Test: full workspace

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the failing cross-module test**

```rust
#[tokio::test]
async fn vault_to_agent_to_connection_flow() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let store = Store::new(paths.clone());
    let vault = Vault::new(paths.vault_file());
    vault.create("pass").unwrap();
    let mut v = vault.unlock("pass").unwrap();

    let generated = keys::generate(KeyAlgorithm::Ed25519, None).unwrap();
    v.put("k1", Secret::PrivateKey { pem: generated.private_pem, passphrase: None }).unwrap();
    let meta = KeyMetadata::now(
        "k1".into(), KeyAlgorithm::Ed25519, generated.public_openssh,
        generated.fingerprint, KeySource::Vault,
    );
    store.upsert_key(meta.clone()).unwrap();

    let (agent_keys, warnings) = agent::vault_keys(&v, &[meta]);
    assert!(warnings.is_empty());
    assert_eq!(agent_keys.len(), 1);

    let mut conn = Connection::managed("c1".into(), "web".into());
    conn.host = "example.com".into();
    conn.user = "deploy".into();
    conn.key_id = Some("k1".into());
    store.upsert_connection(conn.clone()).unwrap();

    let argv = terminal::ssh_invocation(&conn, store.load_data().unwrap().keys.first()).unwrap();
    assert_eq!(argv, vec!["ssh", "deploy@example.com", "-p", "22"]);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core --test end_to_end`
Expected: FAIL until the test is wired to the public re-exports; add the missing `pub use` lines in `lib.rs`.

- [ ] **Step 3: Update README and CHANGELOG**

README sections: what Sigrun is; install from source (`cargo build --release`); quick start (`sigrun host`, `sigrun vault init`, `sigrun keys generate`, `sigrun connections add`, `sigrun connect --print`); how the agent works (`SSH_AUTH_SOCK`); security notes (`~/.ssh` read-only, vault, no telemetry); current limitations (headless only; desktop app in Plan 2; no `Include` in `ssh_config`; no auto-start of the host — run `sigrun host`; protected disk-backed keys use `-i` and prompt in the terminal, while importing a key into the vault is the agent-served path). CHANGELOG `## [Unreleased]` lists the features added by these tasks.

- [ ] **Step 4: Run the full gate**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets`
Expected: PASS on macOS, Linux, and Windows CI.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md crates/sigrun-core/tests
git commit -m "test: end-to-end vault/agent/connection flow and v0.1 docs"
```
