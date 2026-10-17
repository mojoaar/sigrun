# SSH Key Provisioning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user connect to a server with a password, install one or more of their Sigrun keys into the server's `authorized_keys`, verify key-only login works, and optionally disable password auth per connection.

**Architecture:** A new `provision` module in `sigrun-core` drives an in-process `russh` client: fail-closed host-key gate → password auth → idempotent `authorized_keys` install → per-key re-authentication over a second connection → optional `sshd` hardening. The existing host exposes it over IPC, so the desktop app and the CLI share one implementation.

**Tech Stack:** Rust 1.85, `russh` 0.64 (`default-features = false, features = ["ring", "rsa"]`), tokio, `ssh-key` 0.6 (vault) converted to/from OpenSSH strings for `russh`'s `ssh-key` 0.7, Tauri 2, React 19 + TypeScript, i18next.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` (§6.3 provisioning, §7.5 host-key trust, §7.6 secret handling).

## Global Constraints

- `~/.ssh` is read-only. Sigrun only writes to the **remote** `~/.ssh/authorized_keys`, and only during an explicit provision action.
- No plaintext secret on disk, in logs, or in argv: passwords arrive over the local IPC socket or stdin, are never logged, and are dropped after use.
- Host-key trust fails closed: `Unknown` → explicit confirmation; `Mismatch` → refuse before any password is sent.
- Hardening runs **only after** every selected key authenticates on its own. On any failure, restore the previous `sshd` state.
- The connection keeps **one** `key_id` for connecting; provisioning accepts a **set** of keys.
- Hardening is per-connection, default **off**, and always shown with a lockout warning.
- Existing gates stay green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --all-targets`, `npm test -- --run`, `npm run typecheck`, `npm run lint`, `actionlint`.
- English CLI; EN + DA UI strings through i18next (completeness guard enforces both files).

## Review Focus

1. **Wrong password** — a bad login password must fail with `invalid_input`/`ssh` and **never** attempt the install (Task 1).
2. **Unknown or changed host key** — provisioning refuses before sending the password; message carries `<fingerprint> <algo> <b64>` so the UI can confirm and retry (Task 1).
3. **Key installs but cannot authenticate** — hardening must not run; the outcome names the failing key (Task 2).
4. **Partial failure** — one of several keys failing must report which succeeded and leave `authorized_keys` with the valid lines intact (Task 2).
5. **Hardening breaks sshd** — a failed `sshd -t` or reload restores the drop-in and leaves password auth working (Task 3).

---

### Task 1: Trust-gated SSH client and password auth

**Files:**
- Create: `crates/sigrun-core/src/provision.rs`
- Modify: `crates/sigrun-core/src/lib.rs` (add `pub mod provision;`)
- Modify: `crates/sigrun-core/src/error.rs` (three variants)
- Modify: `crates/sigrun-core/src/ipc/protocol.rs` (`error_code` arms)
- Modify: `Cargo.toml` (workspace dep), `crates/sigrun-core/Cargo.toml`

**Interfaces:**
- Consumes: `crate::trust::{check, TrustStatus}`, `crate::error::{Error, Result}`.
- Produces:
  - `pub struct ProvisionKey { pub name: String, pub public_openssh: String, pub private_pem: String }`
  - `pub struct ProvisionOptions { pub host: String, pub port: u16, pub user: String, pub password: String, pub known_hosts: std::path::PathBuf, pub disable_password_auth: bool }`
  - `pub async fn authenticate(opts: &ProvisionOptions) -> Result<russh::client::Handle<Gate>>`
  - `pub fn normalize_public_key(raw: &str) -> Result<String>` returning `"<algo> <b64>"` with any comment stripped.
  - `Error::HostKeyUnknown(String)`, `Error::HostKeyMismatch(String)`, and `Error::Ssh(#[from] russh::Error)`; `error_code` maps them to `"host_key_unknown"`, `"host_key_mismatch"`, `"ssh"`.
  - `Gate` must implement `russh::client::Handler`; that trait requires `type Error: From<russh::Error>`, which is exactly why `Error` gains the `#[from] russh::Error` variant. Our own failures (unsupported certificate, bad password, timeout) use `Error::InvalidInput`/`Error::Config`, not `Ssh`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn normalize_public_key_strips_comments_and_rejects_garbage() {
    let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq me@laptop";
    assert_eq!(normalize_public_key(key).unwrap(), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKKEEWQJy/IdduM5HglENeKuk+5Am8lcPpdlUdvxDWqq");
    assert!(normalize_public_key("").is_err());
    assert!(normalize_public_key("ssh-ed25519").is_err());
    assert!(normalize_public_key("ssh-ed25519 bad key").is_err());
    assert!(normalize_public_key("ssh-ed25519 $()").is_err());
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(error_code(&Error::HostKeyUnknown("SHA256:x ssh-ed25519 AAAA".into())), "host_key_unknown");
    assert_eq!(error_code(&Error::HostKeyMismatch("SHA256:x ssh-ed25519 AAAA".into())), "host_key_mismatch");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sigrun-core provision::tests::normalize_public_key_strips_comments_and_rejects_garbage`
Expected: FAIL — module/function not found.

- [ ] **Step 3: Implement the gate, client, and normalization**

Add to `Cargo.toml` `[workspace.dependencies]`:
```toml
russh = { version = "0.64", default-features = false, features = ["ring", "rsa"] }
```
and `russh = { workspace = true }` in `crates/sigrun-core/Cargo.toml`.

`Gate` implements `russh::client::Handler` with `type Error = Error` and:
- `check_server_key(&mut self, key: &PublicKeyOrCertificate)`:
  - `PublicKeyOrCertificate::PublicKey { key, .. }` → `key.to_openssh()` → split into `algo` and `b64`; call `crate::trust::check(&self.known_hosts, &self.host, self.port, algo, b64)`.
  - `Trusted` → `Ok(true)`.
  - `Unknown` → `Err(Error::HostKeyUnknown(format!("{fp} {algo} {b64}")))` where `fp` comes from `crate::trust::fingerprint`.
  - `Mismatch` → `Err(Error::HostKeyMismatch(...))` with the same message shape.
  - `Certificate(_)` → `Err(Error::Config("host certificates are not supported".into()))`.
- `authenticate`: `russh::client::connect(Arc::new(Config::default()), (host, port), Gate{..})`, then `handle.authenticate_password(user, password)`; `AuthResult::Success` → `Ok(handle)`, otherwise `Err(Error::InvalidInput("password authentication failed".into()))`.

`normalize_public_key`: split once on whitespace; require exactly two fields, `algo` in `{ssh-ed25519, ssh-rsa, ecdsa-sha2-nistp256, ecdsa-sha2-nistp384, ecdsa-sha2-nistp521}`, and `b64` matching `[A-Za-z0-9+/]+={0,2}`; return `format!("{algo} {b64}")`.

Add the three `Error` variants with `#[error(...)]` messages that never include a password; map them in `error_code`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p sigrun-core provision`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/sigrun-core/Cargo.toml crates/sigrun-core/src/{lib.rs,error.rs,provision.rs} crates/sigrun-core/src/ipc/protocol.rs
git commit -m "feat(provision): trust-gated russh client with password auth"
```

---

### Task 2: Install keys into `authorized_keys` and verify each one

**Files:**
- Modify: `crates/sigrun-core/src/provision.rs`
- Test: inline `#[cfg(test)]` module

**Interfaces:**
- Consumes: Task 1's `ProvisionOptions`, `ProvisionKey`, `authenticate`, `normalize_public_key`.
- Produces:
  - `pub struct ProvisionOutcome { pub installed: Vec<String>, pub verified: Vec<String>, pub hardening: Hardening }` (derives `Debug, Clone, PartialEq, Serialize, Deserialize`)
  - `pub enum Hardening { NotRequested, Applied, Skipped(String) }` (derives `Debug, Clone, PartialEq, Serialize, Deserialize`, `#[serde(rename_all = "snake_case")]` — the wire shapes are `"not_requested"`, `"applied"`, and `{"skipped": "<reason>"}`)
  - `pub fn authorized_keys_script(keys: &[String]) -> Result<String>` — pure; returns a POSIX `sh` script.
  - `async fn run(handle: &mut russh::client::Handle<Gate>, script: &str) -> Result<(u32, String)>` — executes `sh -s` with the script on stdin and returns `(exit_status, stderr)`.
  - `pub async fn install_keys(opts: &ProvisionOptions, keys: &[ProvisionKey]) -> Result<ProvisionOutcome>`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn script_is_idempotent_and_quotes_safely() {
    let keys = vec![
        "ssh-ed25519 AAAA1111".to_string(),
        "ssh-rsa BBBB2222".to_string(),
    ];
    let s = authorized_keys_script(&keys).unwrap();
    assert!(s.contains("umask 077"));
    assert!(s.contains("chmod 700 \"$HOME/.ssh\""));
    assert!(s.contains("chmod 600 \"$HOME/.ssh/authorized_keys\""));
    assert!(s.contains("grep -qxF -- 'ssh-ed25519 AAAA1111'"));
    assert!(s.contains("grep -qxF -- 'ssh-rsa BBBB2222'"));
    assert_eq!(s.matches("authorized_keys").count(), 2);
}

#[test]
fn script_rejects_unsafe_keys() {
    assert!(authorized_keys_script(&["ssh-ed25519 AAAA'; rm -rf / #".to_string()]).is_err());
    assert!(authorized_keys_script(&[]).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sigrun-core provision::tests::script_is_idempotent_and_quotes_safely`
Expected: FAIL — `authorized_keys_script` not found.

- [ ] **Step 3: Implement install and verification**

`authorized_keys_script`: run every key through `normalize_public_key`, returning `Error::InvalidInput` if any fails or the slice is empty. Emit, in order:
```
set -e
umask 077
mkdir -p "$HOME/.ssh"
chmod 700 "$HOME/.ssh"
touch "$HOME/.ssh/authorized_keys"
chmod 600 "$HOME/.ssh/authorized_keys"
add() { grep -qxF -- "$1" "$HOME/.ssh/authorized_keys" || printf '%s\n' "$1" >> "$HOME/.ssh/authorized_keys"; }
add 'ssh-ed25519 AAAA1111'
```
Keys are normalized (`algo b64` only), so a single-quoted literal is safe; the test pins rejection of anything else.

`install_keys`:
1. `let mut handle = authenticate(opts).await?`.
2. `run(&mut handle, &authorized_keys_script(&public_keys)?)` — a helper that opens a session channel (`channel_open_session`), `exec(true, format!("sh -s"))`, writes the script, `eof()`, drains `ChannelMsg::Data`/`ExtendedData` into a capped `String` (8 KiB), and returns `(exit_status, stderr)`. Non-zero → `Error::Config(format!("authorized_keys update failed (exit {code}): {stderr}"))`.
3. For each key: parse `ssh_key::PrivateKey::from_openssh(&key.private_pem)` (vault) → `russh::keys::PrivateKey::from_openssh(&key.private_pem)` (russh's 0.7 type), wrap in `PrivateKeyWithHashAlg::new(Arc::new(k), Some(HashAlg::Sha256))` for RSA, then `russh::client::connect(...)` with the same `Gate` and `authenticate_publickey(opts.user, to)`; `Success` → push `key.name` to `verified`, else record a failure message `"<name>: key authentication failed"`.
4. If any key fails verification: return `Err(Error::Config(failures.join("; ")))` **before** hardening — the caller sees exactly which key failed, and the successfully installed lines stay in place.
5. `hardening` is `Hardening::NotRequested` when `!opts.disable_password_auth`; otherwise return `Hardening::Skipped("hardening not implemented".into())` for now (Task 3 replaces this).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p sigrun-core provision`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/provision.rs
git commit -m "feat(provision): idempotent authorized_keys install and per-key verification"
```

---

### Task 3: Opt-in password-auth hardening with rollback

**Files:**
- Modify: `crates/sigrun-core/src/provision.rs`
- Test: inline `#[cfg(test)]` module

**Interfaces:**
- Consumes: `install_keys`'s success path, `run` helper.
- Produces: `async fn harden(handle: &mut Handle<Gate>) -> Result<Hardening>`, `pub fn hardening_script() -> String`, and `pub const DROPIN: &str`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn dropin_disables_only_password_auth() {
    assert!(DROPIN.contains("PasswordAuthentication no"));
    assert!(DROPIN.contains("PubkeyAuthentication yes"));
    assert!(DROPIN.contains("KbdInteractiveAuthentication no"));
    assert!(DROPIN.contains("ChallengeResponseAuthentication no"));
}

#[test]
fn hardening_script_backs_up_and_rolls_back() {
    let s = hardening_script();
    assert!(s.contains("sshd_config.d/99-sigrun.conf"));
    assert!(s.contains("cp -p"));
    assert!(s.contains("sshd -t"));
    assert!(s.contains("grep -q '^Include /etc/ssh/sshd_config.d"));
    assert!(s.contains("rm -f"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sigrun-core provision::tests::dropin_disables_only_password_auth`
Expected: FAIL — not found.

- [ ] **Step 3: Implement hardening**

`DROPIN`:
```
# Installed by Sigrun. Remove this file to re-enable password authentication.
PasswordAuthentication no
KbdInteractiveAuthentication no
ChallengeResponseAuthentication no
PubkeyAuthentication yes
```

`hardening_script()` returns a POSIX script that:
1. `command -v sudo >/dev/null || { echo "no sudo"; exit 3; }`
2. `sudo -n true || { echo "no non-interactive sudo"; exit 4; }` — never prompt for or reuse the login password as a sudo password.
3. Backup: `sudo -n cp -p /etc/ssh/sshd_config "/etc/ssh/sshd_config.sigrun-bak.$(date +%s)"`.
4. If `grep -q '^Include /etc/ssh/sshd_config.d/\*.conf' /etc/ssh/sshd_config`, write the drop-in `sudo -n tee /etc/ssh/sshd_config.d/99-sigrun.conf >/dev/null`; otherwise append a marked block (`# BEGIN sigrun`, `# END sigrun`) to `sshd_config`.
5. `sudo -n sshd -t || { <remove drop-in / strip block>; echo "sshd config test failed"; exit 5; }`.
6. Reload: `sudo -n systemctl reload sshd || sudo -n systemctl reload ssh || sudo -n service ssh reload || { <rollback>; exit 6; }`.

Exit codes 3/4 → `Hardening::Skipped(reason)` (capability missing, not an error). 5/6 → `Error::Config` after rollback.

`harden` runs the script through the existing `run` helper on a **fresh** connection (password auth is still available at that point) and maps the exit code as above.

In `install_keys`, replace the Task 2 stub:
```rust
let hardening = if opts.disable_password_auth {
    let mut h = authenticate(opts).await?;
    harden(&mut h).await?
} else {
    Hardening::NotRequested
};
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p sigrun-core provision`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core/src/provision.rs
git commit -m "feat(provision): opt-in sshd hardening with backup and rollback"
```

---

### Task 4: Model, IPC, and host handler

**Files:**
- Modify: `crates/sigrun-core/src/model.rs` (`Connection.disable_password_auth`)
- Modify: `crates/sigrun-core/src/ipc/protocol.rs`
- Modify: `crates/sigrun-core/src/host.rs`
- Test: `crates/sigrun-core/src/ipc/mod.rs` (inline) + model default test

**Interfaces:**
- Consumes: `provision::{ProvisionKey, ProvisionOptions, ProvisionOutcome, install_keys}`, `vault::{Secret}`, `store`.
- Produces:
  - `Connection.disable_password_auth: bool` with `#[serde(default)]`, `false` in `managed`/`from_ssh_host`, and in the frontend type.
  - `Request::ConnectionsProvision { id: String, password: String, key_ids: Vec<String>, disable_password_auth: bool }`
  - `Response::Provisioned(ProvisionOutcome)`
  - `async fn connections_provision(...) -> Result<Response>` in `host.rs`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn connection_defaults_do_not_disable_password_auth() {
    let c = Connection::managed("id", "box");
    assert!(!c.disable_password_auth);
    let parsed: Connection = serde_json::from_str(
        r#"{"id":"i","label":"l","host":"h","user":"u","port":22}"#,
    ).unwrap();
    assert!(!parsed.disable_password_auth);
}
```
and, in `ipc/mod.rs`:
```rust
#[tokio::test]
async fn provisioning_unknown_connection_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_roots(dir.path(), dir.path(), dir.path().join("ssh"));
    let state = HostState::new(paths).await.unwrap();
    let resp = handle(&state, Request::ConnectionsProvision {
        id: "missing".into(),
        password: "x".into(),
        key_ids: vec![],
        disable_password_auth: false,
    }).await;
    match resp {
        Response::Error { code, .. } => assert_eq!(code, "not_found"),
        other => panic!("unexpected {other:?}"),
    }
    state.shutdown.cancel();
}

#[tokio::test]
async fn provisioning_a_locked_vault_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_roots(dir.path(), dir.path(), dir.path().join("ssh"));
    let state = HostState::new(paths).await.unwrap();
    let mut c = crate::model::Connection::managed("c1", "web");
    c.host = "example.com".into();
    c.user = "deploy".into();
    state.store.upsert_connection(c).unwrap();
    // No vault has been created or unlocked, and a key was requested.
    let resp = handle(&state, Request::ConnectionsProvision {
        id: "c1".into(),
        password: "x".into(),
        key_ids: vec!["k1".into()],
        disable_password_auth: false,
    }).await;
    match resp {
        Response::Error { code, .. } => assert_eq!(code, "vault_locked"),
        other => panic!("unexpected {other:?}"),
    }
    state.shutdown.cancel();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sigrun-core provisioning_`
Expected: FAIL — unknown variant / field.

- [ ] **Step 3: Implement model, protocol, and handler**

`connections_provision`:
1. Load data; find the connection by `id` (fall back to a unique label match like `connections_connect`).
2. `key_ids` empty → the connection's `key_id`; still empty → `Error::InvalidInput("no key selected")`.
3. Require an unlocked vault (`Error::VaultLocked` otherwise).
4. For each id: find `KeyMetadata`; `source` must be `Vault` (disk keys → `Error::InvalidInput("import the key into the vault to provision it")`); `vault.get(id)` must be `Secret::PrivateKey`.
5. Build `ProvisionKey { name: meta.name, public_openssh: meta.public_key, private_pem }`; if `passphrase` is `Some`, decrypt in `spawn_blocking` with `ssh_key::PrivateKey::from_openssh(&pem)?.decrypt(passphrase)` and re-serialize to an unencrypted PEM.
6. Build `ProvisionOptions { host, port, user, password, known_hosts: state.paths.data_dir.join("known_hosts"), disable_password_auth }`.
7. `install_keys(&opts, &keys).await` inside `tokio::time::timeout(Duration::from_secs(90), ..)`; timeout → `Error::Config("provisioning timed out")`.
8. Return `Response::Provisioned(outcome)`.

Also bump `last_vault_activity` for this request (it touches the vault) by adding it to the `matches!` list in `handle_inner`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p sigrun-core`
Expected: PASS (including the two new tests).

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core
git commit -m "feat(host): provisioning request over IPC with vault key resolution"
```

---

### Task 5: Desktop UI, commands, and translations

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs` (`provision_connection`)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (`generate_handler!`)
- Modify: `apps/desktop/src/api.ts`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/i18n/{en,da}.json`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `Request::ConnectionsProvision`, `Response::Provisioned`, `trustHost`.
- Produces: `provisionConnection(id, password, keyIds, disablePasswordAuth)`; an "Install SSH key…" flow in the connection form.

- [ ] **Step 1: Write the failing test**

Extend `Connections.test.tsx` (which already uses `fireEvent`, not `userEvent` — keep that):
```tsx
it("warns before disabling password auth", async () => {
  vi.mocked(api.listConnections).mockResolvedValue([
    { id: "c1", label: "web", host: "h", user: "u", port: 22, key_id: "k1", group: null, tags: [], source: "managed", disable_password_auth: false },
  ]);
  vi.mocked(api.listKeys).mockResolvedValue([
    { key: { id: "k1", name: "work", algorithm: "ed25519", public_key: "ssh-ed25519 AAAA", fingerprint: "SHA256:x", source: "vault", disk_path: null, has_passphrase: false, group: null, tags: [], notes: null, created_at: "" }, available: true },
  ]);
  render(<Connections onVaultLocked={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: /install ssh key/i }));
  fireEvent.click(await screen.findByLabelText(/disable password auth/i));
  expect(await screen.findByRole("alert")).toHaveTextContent(/lock you out/i);
});
```
The module mock at the top of that file must also export `provisionConnection: vi.fn()`.

- [ ] **Step 2: Run to verify failure**

Run: `cd apps/desktop && npm test -- --run Connections`
Expected: FAIL — button/label not found.

- [ ] **Step 3: Implement the command and UI**

- `commands.rs`: `provision_connection_inner(state, id, password, key_ids, disable_password_auth)` → `Response::Provisioned`; wrapper `#[tauri::command] pub async fn provision_connection(...)`; register in `generate_handler!`.
- `api.ts`: `Connection.disable_password_auth: boolean`; `ProvisionOutcome` type (`installed: string[]`, `verified: string[]`, `hardening: "not_requested" | "applied" | { skipped: string }` — matches the `Hardening` serde shape exactly); `provisionConnection`.
- `Connections.tsx`: add `disable_password_auth: false` to the `Form`'s initial `Connection` literal (TypeScript requires it once the type gains the field). Inside the connection form, an **Install SSH key…** button (enabled when at least one `Vault`-source key exists) opens an inline panel with:
  - a checkbox list of vault keys, pre-checking `key_id`;
  - a password input (`type="password"`, cleared on success and on close);
  - a **Disable password auth** checkbox bound to `c.disable_password_auth`, with `role="alert"` warning copy shown when checked;
  - submit → `provisionConnection(...)`; on `host_key_unknown`, show the existing trust dialog, then retry once after `trustHost`;
  - success → panel closes, a `role="status"` line lists installed/verified key names and, when hardening was skipped, its reason.
- i18n keys in **both** files: `provision.title`, `provision.body`, `provision.password`, `provision.selectKeys`, `provision.disablePasswordAuth`, `provision.lockoutWarning`, `provision.install`, `provision.installed`, `provision.skipped`, `provision.importFirst`.

- [ ] **Step 4: Run to verify pass**

Run: `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(desktop): install SSH keys on a server from the connection dialog"
```

---

### Task 6: CLI parity, docs, and the live-server check

**Files:**
- Modify: `crates/sigrun-cli/src/cli.rs`
- Modify: `README.md`, `CHANGELOG.md`
- Create: `crates/sigrun-core/tests/live_provision.rs` (`#[ignore]`)

**Interfaces:**
- Consumes: `Request::ConnectionsProvision`.
- Produces: `sigrun connections provision <id-or-label> [--key <id>]... [--disable-password-auth] [--password-stdin]`.

- [ ] **Step 1: Write the failing test**

In `crates/sigrun-cli/tests/cli.rs`, assert the command is wired:
```rust
#[test]
fn provision_rejects_an_empty_password() {
    assert_cmd::Command::cargo_bin("sigrun").unwrap()
        .args(["connections", "provision", "web", "--password-stdin"])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(predicates::str::contains("password"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sigrun-cli provision_rejects_an_empty_password`
Expected: FAIL — unrecognized subcommand.

- [ ] **Step 3: Implement CLI, docs, and the ignored live test**

- `ConnectionsCommand::Provision { id_or_label, key: Vec<String>, disable_password_auth: bool, password_stdin: bool }`; resolve id-or-label exactly like `Connect`; read the password with `read_passphrase` and **require** it (`--password-stdin` or a `rpassword` prompt, never argv); send the request; print installed/verified names; print a hardening line (`disabled password authentication` / `skipped: <reason>`).
- `CHANGELOG.md` `### Added`: multi-key provisioning, per-connection password-auth hardening, CLI `connections provision`.
- `README.md`: a "Install a key on a server" section covering the flow, the multi-key selection, the hardening warning and its `sudo -n` requirement, and that disk keys must be imported into the vault first.
- `tests/live_provision.rs`: `#[ignore]` test reading `SIGRUN_LIVE_HOST`, `SIGRUN_LIVE_USER`, `SIGRUN_LIVE_PASSWORD`, and a key from the vault; it provisions against a throwaway host and asserts the outcome. Document the command in the file header.

- [ ] **Step 4: Run the gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --all-targets && (cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build) && actionlint`
Expected: all clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-cli README.md CHANGELOG.md crates/sigrun-core/tests/live_provision.rs
git commit -m "feat(cli): provision connections and document key installation"
```

---

## Self-Review

- **Spec coverage:** §6.3 password-based provisioning (Tasks 2, 4), opt-in hardening with preview/confirmation (Task 3), §7.5 fail-closed host-key trust (Task 1), §7.6 no secrets in logs/argv (all), multi-key install (Task 2), add+edit entry points (Task 5), CLI parity (Task 6). GitHub/Gitea wizards remain out of scope, as agreed.
- **Step scan:** every step names a test, a command, or an exact function signature; no "handle edge cases" placeholders.
- **Type consistency:** `ProvisionKey`/`ProvisionOptions`/`ProvisionOutcome`/`Hardening` are defined once (Tasks 1-2) and consumed unchanged in Tasks 4-6; `disable_password_auth` is a `bool` on `Connection` everywhere.
- **Review Focus:** each of the five lines has a named test in the task that owns the code.
- **Proportion:** six tasks; code blocks are limited to tests and the pinned script/drop-in text.
