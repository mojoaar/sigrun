# Phase 3a: Error Fidelity Design Spec

**Goal:** Improve error clarity and safety across remote provisioning, reachability diagnostics, and desktop UI views by eliminating remote command injection in errors, enforcing valid username constraints before Russh connection, removing misleading stderr logs, and displaying specific host error messages in the Keys and Credentials views.

## Findings Covered
- **Finding #25:** Provision hardening echoes remote stderr into UI error messages (`crates/sigrun-core/src/provision.rs`)
- **Finding #26:** Provisioning a connection with an empty user fails with a misleading auth error (`crates/sigrun-core/src/host.rs`)
- **Finding #37:** `probe_reachable`'s inconclusive warning goes to stderr that desktop users never see (`crates/sigrun-core/src/host.rs`)
- **Finding #38:** Keys and Credentials views throw away specific host error messages (`apps/desktop/src/views/Keys.tsx`, `Credentials.tsx`)

---

## 1. Remote Hardening Error Sanitization (Finding #25)

### Problem
In `crates/sigrun-core/src/provision.rs:257-277`, the hardening function parses exit code and stderr from `run(handle, &hardening_script())`:
```rust
let (code, stderr) = run(handle, &hardening_script()).await?;
let reason = stderr.trim();
```
`reason` is interpolated directly into `Error::Config(format!("sshd hardening failed: {reason}"))` for exit codes 5..=8, and also returned in `Hardening::Skipped` for exit codes 3 and 4 when non-empty. A compromised or hostile server being provisioned could return arbitrary text (up to 8 KiB) in stderr, misleading the user or forging instructions into the dialog.

### Design
1. For exit codes 5..=8 in `harden`, map each code to a fixed, descriptive string:
   - `5`: `"could not write drop-in or sshd config test failed"`
   - `6`: `"could not reload sshd service"`
   - `7`: `"password authentication remains enabled after configuration"`
   - `8`: `"hardening interrupted or rollback failed"`
2. For any other exit code `c`, return `format!("sshd hardening failed with exit code {c}")`.
3. If no exit code (`None`), return `"sshd hardening failed: server terminated session without exit status"`.
4. For `Hardening::Skipped` (codes 3 and 4), do not use `reason`:
   - `3`: `"sudo is not available"`
   - `4`: `"non-interactive sudo is required"`
5. In `install_keys` (`crates/sigrun-core/src/provision.rs:296-305`), sanitize `authorized_keys update failed (exit {code})` without echoing raw stderr from the remote server.

---

## 2. Username Validation Before Provisioning (Finding #26)

### Problem
Connections allow an empty `user` field, which standard `ssh` resolves to the local OS user. However, provisioning uses Russh password authentication (`authenticate_password(&opts.user, &opts.password)`). When `user` is empty, Russh logs in as `""`, which fails password auth on the server. The user is presented with `"password authentication failed"`, hiding the fact that no username was supplied.

### Design
In `crates/sigrun-core/src/host.rs` within `provision_connection`:
Check if `auth.user.trim().is_empty()`. If empty, return `Err(Error::InvalidInput("provisioning requires a username".into()))` before touching the vault, decrypting private keys, or attempting Russh connection.

---

## 3. Clean up `probe_reachable` Inconclusive Warning (Finding #37)

### Problem
In `crates/sigrun-core/src/host.rs:1188-1191`:
```rust
eprintln!(
    "sigrun: probe inconclusive, letting ssh try: {}",
    failure.message()
);
```
Desktop users never see stderr logs from Tauri. Inconclusive probes (timeouts, EHOSTUNREACH behind firewalls) are deliberately advisory per architecture rules—`ssh` is given the chance to connect anyway. Emitting a raw `eprintln!` here is useless noise in headless/CLI logs and invisible on the desktop.

### Design
Remove the `eprintln!` statement entirely. The advisory contract remains unchanged: if `failure.blocks()` is true (e.g. verified `ECONNREFUSED`), return `failure.into_error()`. Otherwise, proceed cleanly to SSH invocation.

---

## 4. Specific Host Error Display in Keys and Credentials (Finding #38)

### Problem
In `apps/desktop/src/views/Keys.tsx` and `Credentials.tsx`, the top-level error state is tracked as a boolean:
`const [error, setError] = useState(false);`
When an action fails (e.g. exporting a key into `~/.ssh` or saving a duplicate credential label), the error handler sets `setError(true)` and renders `<p role="alert" className="text-danger">{t("common.error")}</p>`. Specific error messages returned by the host (`e.message`) are discarded.

### Design
Adopt the pattern already used in `apps/desktop/src/views/Connections.tsx`:
1. Change `error` state from `boolean` to `string | null` initialized to `null`.
2. In `handle(err)`:
   - If `(err as CommandError)?.code === "vault_locked"`, call `onVaultLocked()`.
   - Otherwise, set `setError(e && e.code !== "internal" ? e.message : "")`.
3. In `run(promise)`: set `setError(null)` before executing.
4. In JSX render:
   `{error !== null && <p role="alert" className="text-danger">{error || t("common.error")}</p>}`.
5. In `Keys.tsx`, also handle file dialog errors (e.g. export refusal) by passing the caught error to `handle(err)` or setting `setError(e?.message ?? "")` instead of a bare `setError(true)`.

---

## Non-Goals
- Adding remote log streaming or live interactive debug consoles for remote provisioning.
- Changing `ssh` invocation fallback for general connections (an empty username in a standard connection continues to let `ssh` resolve local user).
