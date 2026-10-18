# Sigrun — password-auth connections, delete confirmation, and landing the pending fixes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a connection an optional password that Sigrun stores in the encrypted vault and hands to `ssh` automatically (so password-only hosts are one click from the app, the tray and the CLI), fix Delete on connections and keys (the native `confirm()` is unsupported in Tauri's webview, so nothing happens today), and land the three already-written core fixes so CI goes green again.

**Architecture:** The password lives in the vault's existing but unused `Secret::Password` slot, keyed by the connection id, so it is never part of the persisted `Connection` model or `data.toml`. Two new IPC requests carry it: `ConnectionsSetPassword { id, password: Option<String> }` (set/clear) and `PasswordGet { id }` (read, for the helper). Connecting to a connection that has a stored password launches `ssh` with `SSH_ASKPASS`/`SSH_ASKPASS_REQUIRE=force` plus `SIGRUN_CONNECTION_ID`, and the **app/CLI binary doubles as the askpass helper** (a marker argument, handled before any window or CLI parse), which fetches the password from the running host over the existing socket — so the secret never appears in the environment, on a command line, or in a log. Because macOS's default terminals are driven by AppleScript (`do script "…"`) and therefore cannot inherit environment variables from the spawner, the launched command line itself is prefixed with the environment assignments it needs — which also fixes the existing latent bug where vault-backed keys only work in the argv-based terminals (Ghostty/kitty/Alacritty/WezTerm) and not in Terminal.app/iTerm.

**Tech Stack:** Rust (crates/sigrun-core: model/store/vault/ipc/host/terminal; crates/sigrun-cli: clap), Tauri 2 + React 19 + TypeScript + Tailwind + i18next, vitest, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` §6.1 (connection manager), §6.3 (provisioning), §7.6 (secret handling).

## Global Constraints

- `~/.ssh` stays read-only. No password in argv, in a command line, in `data.toml`, in a log line or in an error message. The askpass *variable names* may appear on the command line; the value never does.
- The password is stored only as `Secret::Password` in the vault, keyed by the connection id.
- Every user-facing string goes through `t(...)`; new keys land in **both** `apps/desktop/src/i18n/en.json` and `da.json` (the recursive completeness guard enforces it).
- English-only CLI output. Vault-locked means refuse with the unlock gate, on every surface.
- Gate per task: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --all-targets`, plus `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build` when frontend files change, plus `actionlint` when workflows change.
- One commit per task, message given in the task.
- Every behaviour-changing task adds a CHANGELOG entry under `## [Unreleased]` (`### Added` / `### Fixed` / `### Changed`). The version lives in exactly two places — `Cargo.toml` and `apps/desktop/package.json` — and CI's `version-sync` job keeps them in lockstep; on a `v*` tag it also requires a `## [<tag>] - <date>` heading in `CHANGELOG.md`. Nothing has been released yet, so everything here lands under `## [Unreleased]` and the first tag will be `v0.1.0`.

## Review Focus

1. A connection with a stored password and a **locked vault**: Connect refuses with the existing vault gate on all three surfaces (app, tray, CLI) and never opens a terminal.
2. A connection with a stored password **and** a key: the key is tried first, the askpass helper is not invoked until ssh asks, and connections *without* a password keep today's behaviour (ssh's own terminal prompt) exactly.
3. **Deleting a connection** clears its vault password, and deleting a **key** never touches a connection's password (the id namespaces must not collide with `KeyMetadata.id`).
4. **Terminal coverage**: a password connection works in Terminal.app and iTerm (AppleScript `do script`), not just the argv terminals — the env prefix must survive every launch path, and quoting must not break the command.
5. **Askpass helper failure**: host not running, socket missing, unknown connection id, or an empty password → exit non-zero, nothing on stdout, no secret in stderr; ssh must then fail the attempt rather than hang.

---

### Task 1: Land the pending core fixes (turns CI green)

**Files:**
- Modify: `crates/sigrun-core/src/paths.rs`, `crates/sigrun-core/src/terminal.rs` (already written and gate-green, uncommitted)

**Interfaces:**
- Consumes: nothing new. Produces: a green `main` — the pushed commit is the base every later task builds on.

- [ ] **Step 1: Re-verify the working tree** — `git diff --stat` shows exactly `paths.rs` and `terminal.rs`; `git diff` contains the Windows `pipe_id()`/`dir_hash()` change, the per-platform `detect_terminals_returns_launchable_terminals` assertions, and the `keep_open` (`|| exec ${SHELL:-/bin/sh}`) launch wrapper including `launches_keep_the_window_open_when_the_connection_fails`.
- [ ] **Step 2: Run the gate** — `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --all-targets && (cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build) && actionlint`. Expected: 99 Rust tests pass (2 ignored), 29 frontend tests, everything clean.
- [ ] **Step 3: Commit** — `git add crates/sigrun-core/src/paths.rs crates/sigrun-core/src/terminal.rs` then `git commit -m "fix(core): keep windows pipe names distinct per data dir, allow an empty linux terminal list, and keep the window open when ssh fails"`.
- [ ] **Step 4: Push and watch CI** — `git push` (`main` is ahead of `origin/main`), then `gh run list --limit 3` and `gh run watch` for the new run. Expected: `rust (macos-latest)`, `rust (ubuntu-latest)`, `rust (windows-latest)`, `lint-workflows`, `version-sync` all green. If Windows still fails on a pipe error, the message now names the actual failure — capture it and fix it as its own task before continuing.

### Task 2: Delete works — an in-app confirmation instead of `window.confirm`

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx` (`onRemove` at :325, state block after the `unreachable` state)
- Modify: `apps/desktop/src/views/Keys.tsx` (`onRemove` at :34)
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `removeConnection(id)` / `removeKey(id)` from `../api`, `t("connections.removeConfirm")`, `t("keys.removeConfirm")`, `t("common.delete")`, `t("common.cancel")` (all present in both language files).
- Produces: no new exports. Behaviour: Delete opens an in-app confirm card; only the card's Delete button performs the removal.

- [ ] **Step 1: Write the failing test** (append to `Connections.test.tsx`, reusing its `vi.mock("../api")` harness and the `conn` fixture)

```tsx
it("asks before removing a connection, then removes it", async () => {
  api.listConnections.mockResolvedValue([{ ...conn, id: "c1", label: "web" }]);
  render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: /delete/i }));
  expect(api.removeConnection).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /delete/i }));   // the card's confirm
  await waitFor(() => expect(api.removeConnection).toHaveBeenCalledWith("c1"));
});
```

- [ ] **Step 2: Run to verify it fails** — `cd apps/desktop && npm test -- --run Connections` → the first click calls nothing and no card appears, so the second `getByRole` throws.
- [ ] **Step 3: Implement in `Connections.tsx`** — add `const [confirmRemove, setConfirmRemove] = useState<Connection | null>(null);` after the `unreachable` state; change the row to `onRemove={() => setConfirmRemove(c)}`; insert this before `{form && <Form …}`:

```tsx
      {confirmRemove && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("common.delete")} className="rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("connections.removeConfirm")}</h3>
            <div className="flex gap-2">
              <button onClick={() => setConfirmRemove(null)}>{t("common.cancel")}</button>
              <button
                className="rounded bg-accent px-3 py-1 text-bg"
                onClick={() => {
                  const id = confirmRemove.id;
                  setConfirmRemove(null);
                  removeConnection(id).then(refresh).catch(handle);
                }}
              >
                {t("common.delete")}
              </button>
            </div>
          </div>
        </div>
      )}
```

- [ ] **Step 4: Implement the same in `Keys.tsx`** — `const [confirmRemove, setConfirmRemove] = useState<KeyView | null>(null);`, `onRemove={() => setConfirmRemove(k)}`, and the same card with `aria-label={t("keys.removeConfirm")}`, body `t("keys.removeConfirm")`, confirm running `run(removeKey(confirmRemove.key.id))`. Keep `className={field}`/`className={btn}` conventions on the card's buttons.
- [ ] **Step 5: Run to verify pass** — `npm test -- --run` → 30 passed; `npm run typecheck && npm run lint && npm run build` clean.
- [ ] **Step 6: Commit** — `git add apps/desktop/src` then `git commit -m "fix(desktop): confirm deletes in-app, since the webview ignores window.confirm"`.

### Task 3: Store a connection password in the vault (core + IPC + host)

**Files:**
- Modify: `crates/sigrun-core/src/model.rs` (a transient `has_password` flag on `Connection`)
- Modify: `crates/sigrun-core/src/ipc/protocol.rs` (`Request`/`Response` variants)
- Modify: `crates/sigrun-core/src/host.rs` (handlers, and `ConnectionsRemove` clearing the secret)
- Test: inline module test in `host.rs` (or `ipc/mod.rs`, whichever already drives `HostState`)

**Interfaces:**
- Consumes: `vault::Secret::Password { password: String }`, `UnlockedVault::{get,put,remove}`, `HostState.vault`.
- Produces: `Request::ConnectionsSetPassword { id: String, password: Option<String> } -> Response::Ok`; `Request::PasswordGet { id: String } -> Response::Password(String)`; `Connection.has_password: bool` (`#[serde(default)]`, filled by the host from the vault, so it is never persisted and never accepted from the frontend).

- [ ] **Step 1: Write the failing tests** (three, in the existing `HostState` test module)

```rust
#[tokio::test]
async fn setting_and_clearing_a_connection_password() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::with_roots(dir.path().into(), dir.path().into(), dir.path().into());
    let state = HostState::new(paths).await.unwrap();
    // unlock
    handle(&state, Request::VaultInit { passphrase: "pw".into() }).await;
    handle(&state, Request::VaultUnlock { passphrase: "pw".into() }).await;
    let mut c = Connection::managed("c1", "web");
    c.host = "example.com".into();
    c.user = "deploy".into();
    assert!(matches!(handle(&state, Request::ConnectionsAdd { connection: c }).await, Response::Ok));
    assert!(matches!(
        handle(&state, Request::ConnectionsSetPassword { id: "c1".into(), password: Some("s3cret".into()) }).await,
        Response::Ok
    ));
    // listing reports it without leaking the value
    match handle(&state, Request::ConnectionsList).await {
        Response::Connections(cs) => assert!(cs[0].has_password),
        other => panic!("{other:?}"),
    }
    // the secret is readable only through the dedicated request
    match handle(&state, Request::PasswordGet { id: "c1".into() }).await {
        Response::Password(p) => assert_eq!(p, "s3cret"),
        other => panic!("{other:?}"),
    }
    // clearing
    handle(&state, Request::ConnectionsSetPassword { id: "c1".into(), password: None }).await;
    assert!(matches!(handle(&state, Request::PasswordGet { id: "c1".into() }).await, Response::Error { .. }));
}

#[tokio::test]
async fn a_locked_vault_refuses_to_store_a_password() {
    // VaultInit but no VaultUnlock -> ConnectionsSetPassword is vault_locked
}

#[tokio::test]
async fn removing_a_connection_removes_its_password() {
    // after ConnectionsRemove, PasswordGet for that id is not_found
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p sigrun-core connection_password` → fails to compile (no such variants).
- [ ] **Step 3: Implement** — `model.rs`: add `#[serde(default)] pub has_password: bool` to `Connection` (documented as host-filled; `ConnectionsAdd`/`ConnectionsUpdate` clear it to `false` immediately before `upsert_connection`, so a client-supplied value is never persisted). `protocol.rs`: the two variants plus `Response::Password(String)`. `host.rs`: `ConnectionsSetPassword` requires an unlocked vault (`VaultLocked` otherwise), rejects an unknown id (`NotFound`) and an empty string (`InvalidInput("password must not be empty")`), writes/removes `Secret::Password` under the connection id; `PasswordGet` requires an unlocked vault and returns the stored value or `NotFound`; `ConnectionsList`/`ConnectionsAdd`/`ConnectionsUpdate` fill `has_password` from the vault (false when locked, which is honest); `ConnectionsRemove` removes the vault entry for that id when the vault is unlocked. Never log or echo the value.
- [ ] **Step 4: Run tests** — `cargo test -p sigrun-core` green, plus the full gate.
- [ ] **Step 5: Commit** — `git commit -am "feat(host): store a per-connection password in the vault"` 

### Task 4: The askpass helper and the env-prefixed launch

**Files:**
- Create: `crates/sigrun-core/src/askpass.rs` (`pub fn answer(socket: &Path, connection_id: &str) -> Result<String>`)
- Modify: `crates/sigrun-core/src/terminal.rs` (env prefix on the rendered command; `-o NumberOfPasswordPrompts=1` when a password is stored)
- Modify: `crates/sigrun-core/src/host.rs` (inject the askpass env only when a password exists)
- Modify: `crates/sigrun-cli/src/main.rs` + `cli.rs` (a hidden `askpass` subcommand), `apps/desktop/src-tauri/src/main.rs` (a `--askpass` marker handled before the GUI starts)
- Test: `crates/sigrun-core/src/terminal.rs` inline tests + one CLI integration test

**Interfaces:**
- Consumes: `ipc::client::request`, `Request::PasswordGet`.
- Produces: **`terminal_launch_specs(platform, configured, argv, env_prefix: &str)` gains a trailing parameter** (every existing call site passes `""`), and a non-empty prefix is prepended to the rendered command **inside** that function — before the `|| exec ${SHELL:-/bin/sh}` keep-open suffix — so every terminal path (AppleScript `do script`, `-e /bin/sh -c`, Linux `sh -c`) carries it; `terminal::env_prefix(socket: &Path, connection_id: &str) -> String` returns `SSH_ASKPASS=<exe> SSH_ASKPASS_REQUIRE=force SIGRUN_CONNECTION_ID=<id> SIGRUN_SOCKET=<path> ` with every value shell-escaped and one trailing space; `terminal::ssh_invocation(&Connection, Option<&KeyMetadata>)` gains `-o NumberOfPasswordPrompts=1` only when the connection has a password; `Request::ConnectionsConnect` keeps its shape; the helper subcommands `sigrun askpass` and `<app-binary> --askpass`, both calling `askpass::answer`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_launch_line_carries_the_askpass_env_and_survives_every_terminal() {
    // a Connection with has_password == true
    let argv = ssh_invocation(&conn, None).unwrap();
    let prefix = env_prefix(Path::new("/tmp/host.sock"), "c1");
    assert!(prefix.starts_with("SSH_ASKPASS="));
    assert!(prefix.contains("SSH_ASKPASS_REQUIRE=force"));
    assert!(prefix.contains("SIGRUN_CONNECTION_ID=c1"));
    for configured in [None, Some("iterm"), Some("ghostty")] {
        let specs = terminal_launch_specs(Platform::MacOs, configured, &argv, &prefix).unwrap();
        let line = specs[0].args.join(" ");
        assert!(line.contains("SSH_ASKPASS="), "{configured:?}");
        assert!(line.contains("|| exec ${SHELL:-/bin/sh}"), "{configured:?}");
    }
    // No stored password: the line is unchanged and ssh prompts as before.
    let plain = terminal_launch_specs(Platform::MacOs, None, &argv, "").unwrap();
    assert!(!plain[0].args.join(" ").contains("SSH_ASKPASS="));
}
```
and a CLI test: `sigrun askpass` with `SIGRUN_CONNECTION_ID` set, no host running → exits non-zero, prints nothing on stdout, and stderr contains no password.

- [ ] **Step 2: Verify failure**, then **Step 3: implement** — `askpass.rs` connects to `SIGRUN_SOCKET` (default `Paths::resolve().socket_path()`), sends `PasswordGet { id }`, prints the password with a trailing newline and nothing else; every failure prints a fixed English reason to stderr and returns non-zero. `host.rs::connections_connect` prefixes the command line (via the existing `render_command`) with the env assignments only when the vault holds a password for that connection; `SSH_ASKPASS` is `std::env::current_exe()` and the path is quoted with the existing `shell_escape`. Add `-o NumberOfPasswordPrompts=1` to the argv in that case so a wrong stored password cannot burn three attempts.
- [ ] **Step 4: Gate + commit** — full gate; `feat(connect): hand a stored password to ssh through an askpass helper`.

### Task 5: The desktop UI — password field, indicator, gating

**Files:**
- Modify: `apps/desktop/src/api.ts` (`setConnectionPassword`, `Connection.has_password`)
- Modify: `apps/desktop/src/views/Connections.tsx` (the `Form`'s password field + set/clear on save; a small "password" marker on the row; route `vault_locked` to the existing gate)
- Modify: `apps/desktop/src/components/ConnectionRow.tsx` (the marker)
- Modify: `apps/desktop/src/i18n/en.json` + `da.json`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `setConnectionPassword(id, password: string | null)`, `Connection.has_password`.
- Produces: saving a connection with a typed password calls `setConnectionPassword(id, password)` after the upsert; a "Remove saved password" checkbox calls `setConnectionPassword(id, null)`; the field is `type="password"`, cleared after a successful save, and never rendered from the stored value.

- [ ] **Step 1: Failing tests** — (the `Form` already generates a new connection's id with `crypto.randomUUID()` before the upsert, and `ConnectionsAdd` returns `Response::Ok` rather than an id, so that generated id is what the follow-up call must use) (a) saving a new connection with a password calls `addConnection` **and** then `setConnectionPassword(generatedId, "pw")` with that same id; (b) a row with `has_password` shows the marker, one without does not; (c) a `vault_locked` rejection from `setConnectionPassword` calls `onVaultLocked` and does not persist a password.
- [ ] **Step 2: Verify failure**, **Step 3: implement**, **Step 4: gate** (`npm test -- --run && npm run typecheck && npm run lint && npm run build` plus the Rust gate), **Step 5: commit** — `feat(desktop): set a connection's password in the vault`.

### Task 6: CLI parity, docs, and the manual check

**Files:**
- Modify: `crates/sigrun-cli/src/cli.rs` (a dedicated `connections password` subcommand; `connections add`/`edit` take no password flag)
- Modify: `README.md`, `CHANGELOG.md`
- Test: `crates/sigrun-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `Request::ConnectionsSetPassword`.
- Produces: `sigrun connections password <id-or-label> [--password-stdin] [--clear]`, reading the password the same way the provisioning command does (never argv).

- [ ] **Step 1: Failing test** — `connections password bogus --password-stdin` with an empty stdin line fails with `password must not be empty`, before any host call.
- [ ] **Step 2–3: implement**, **Step 4: gate**, **Step 5: commit** — `feat(cli): set and clear a connection password`.
- [ ] **Step 6: Docs + the one thing tests cannot prove** — README: a "Password connections" paragraph (stored in the vault, used only by ssh through the askpass helper, requires an unlocked vault, and Delete removes it); the terminal note already explains new window vs tab; CHANGELOG `### Added`. Then the manual check on macOS: add a password connection to a host that accepts passwords, connect from the app **in Terminal.app and in iTerm** (both AppleScript paths) and from the tray, confirm no password prompt appears, then Delete the connection and confirm the vault entry is gone (`sigrun keys list` must not show it; `git grep` the data dir for the password and find nothing).

---

### Task 7: Nested groups — `PVE/Folder 1` becomes a folder under `PVE`

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx` (grouping + rendering)
- Modify: `apps/desktop/src-tauri/src/tray.rs` (menu build)
- Test: `apps/desktop/src/views/Connections.test.tsx` and `tray.rs`'s inline tests

**Interfaces:**
- Consumes: `Connection.group: Option<String>`, which is now read as a `/`-separated path — no model change, no migration (an existing `"PVE"` keeps working).
- Produces: `export function groupTree(conns: Connection[]): TreeNode[]` where `type TreeNode = { name: string; path: string; items: Connection[]; children: TreeNode[] }`; segments are trimmed and empty ones dropped (`"PVE//Folder 1/"` → `["PVE", "Folder 1"]`, so `"PVE"` and `" PVE/Folder 1 "` land in one tree); a connection whose group is exactly a node's path sits on that node; roots and children sort case-insensitively with `localeCompare`; ungrouped connections stay a headingless first section.

- [ ] **Step 1: Write the failing tests**

```tsx
it("nests group paths like PVE/Folder 1", async () => {
  api.listConnections.mockResolvedValueOnce([
    { ...conn, id: "c1", label: "pve01", group: "PVE" },
    { ...conn, id: "c2", label: "web1", group: "PVE/Folder 1" },
    { ...conn, id: "c3", label: "web2", group: "PVE/Folder 2" },
  ]);
  render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
  expect(await screen.findByRole("heading", { name: "PVE" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Folder 1" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Folder 2" })).toBeTruthy();
  expect(screen.getByText("pve01")).toBeTruthy();
});

it("treats extra or blank separators as one level", () => {
  const tree = groupTree([{ ...conn, id: "a", group: " PVE//Folder 1/ " }]);
  expect(tree).toHaveLength(1);
  expect(tree[0].name).toBe("PVE");
  expect(tree[0].children[0].name).toBe("Folder 1");
});
```

- [ ] **Step 2: Run to verify failure** — `cd apps/desktop && npm test -- --run Connections` → FAIL (`groupTree` undefined, no `Folder 1` heading).

- [ ] **Step 3: Implement** — replace `groupConnections` with `groupTree` and delete `groupConnections` outright (the tray is Rust and never imports it; update its two tests to the new function), render nested `<section>`s one level indented (`pl-3` on children), and build the same nesting in `tray.rs`: a `Submenu` per node with the node's own items first, then its children's submenus. The existing tray test `groups_discovered_and_loose_connections` stays; add one nested case.

- [ ] **Step 4: Run to verify pass** — `npm test -- --run` and `cargo test -p sigrun-desktop` green.

- [ ] **Step 5: Commit** — `feat(desktop): nest group paths like PVE/Folder 1 in the list and the tray`

---

### Task 8: Start minimized is the last Settings option

**Files:**
- Modify: `apps/desktop/src/views/Settings.tsx`

**Interfaces:**
- Consumes: the existing `settings.startMinimized` key (present in both `en.json` and `da.json` — no translation work).
- Produces: nothing new; DOM order only.

- [ ] **Step 1: Move the checkbox**

In `Settings.tsx`, move the existing block
```tsx
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={s.start_minimized} onChange={(e) => update({ start_minimized: e.target.checked })} />
        {t("settings.startMinimized")}
      </label>
```
so it renders **after** the auto-lock `<label>` (last control in the `<section>`). Keep the markup, class names and key unchanged; do not touch anything else — this is a pure reorder (Theme, Mode, Language, Terminal, Auto-lock, Start minimized).

- [ ] **Step 2: Verify**

Run: `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build`
Expected: all clean. No new test: this changes no logic, and a DOM-order assertion would need a whole new `Settings` test harness with an api mock — not worth it for a reorder.

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/src/views/Settings.tsx
git commit -m "feat(desktop): list start minimized last in settings"
```

---

### Task 9: Inline help and about

**Files:**
- Create: `apps/desktop/src/views/Help.tsx`
- Modify: `apps/desktop/src/App.tsx` (nav + route), `apps/desktop/src/api.ts`, `apps/desktop/src-tauri/src/commands.rs` (allowlisted `open_external`), both `i18n/*.json`
- Test: `apps/desktop/src/views/Help.test.tsx`, a validation test in `commands.rs`

**Interfaces:**
- Consumes: `getStatus()` (already returns `{ version, vault, agent_socket, host_addr }`) for the version, and a new `openExternal(url)` in `api.ts` over `#[tauri::command] open_external(url: String)`.
- Produces: a fourth nav tab (`nav.help` / "Help" / "Hjælp") rendering: what Sigrun is (one paragraph, with the Sigrún name gloss — *sigr* victory, *rún* secret), a quick start (create or unlock the vault → import or generate a key → add a connection → connect; installing a key on a server with a password), connections and groups (including `PVE/Folder 1` nesting and what a tray left-click does when locked vs unlocked), keys (vault vs disk-backed), a short settings summary, a five-command CLI cheat sheet, where data lives (`~/Library/Application Support/Sigrun`-style data dir, `~/.ssh` read-only), and an About block: version, author `mojoaar`, links to `https://johansen.foo` and `https://github.com/mojoaar/sigrun`, MIT.

- [ ] **Step 1: Write the failing test**

```tsx
it("shows the version, the links, and opens them through the host", async () => {
  api.getStatus.mockResolvedValue({ version: "0.1.0", vault: "Unlocked", agent_socket: null, host_addr: "x" });
  render(<Help />);
  expect(await screen.findByText(/0\.1\.0/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /johansen\.foo/ }));
  expect(api.openExternal).toHaveBeenCalledWith("https://johansen.foo");
});
```

- [ ] **Step 2: Run to verify failure.** `cd apps/desktop && npm test -- --run Help` → FAIL (no such module).

- [ ] **Step 3: Implement.** `Help.tsx` (plain read-only view, `useTranslation`, version from `getStatus()`); register the tab in `App.tsx`'s nav; `open_external` validates against a fixed allowlist (`https://johansen.foo`, `https://github.com/mojoaar/sigrun` — anything else → `Error::InvalidInput("unsupported url")`) and spawns `open` / `xdg-open` / `cmd /c start` per `Platform::current()`; **no new dependency**. All copy through `t(...)`, with real Danish in `da.json`.

- [ ] **Step 4: Run the gate.** `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --all-targets && (cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build)` → all clean, including `open_external_rejects_a_foreign_url`.

- [ ] **Step 5: Commit.** `feat(desktop): add an inline help and about view`

---

### Task 10: Changelog and semver discipline

**Files:**
- Modify: `CHANGELOG.md`, `Cargo.toml`, `apps/desktop/package.json`

**Interfaces:**
- Consumes: nothing from the code — this task audits the tree the other tasks leave behind.
- Produces: a `## [Unreleased]` section that lists every behaviour change this plan lands, and a version that is identical in `Cargo.toml` and `apps/desktop/package.json` (CI's `version-sync` job fails the build if they drift, and on a `v*` tag it also requires a `## [<tag>]` heading in `CHANGELOG.md`).

- [ ] **Step 1: Audit the changelog against the diff**

```bash
git log --oneline a252e05..HEAD          # a252e05 is the plan-doc commit this branch was cut from
git diff a252e05..HEAD --stat
```
Every task that changed behaviour must appear once under `## [Unreleased]`:
`### Added` — password connections (stored in the vault, handed to ssh by the askpass helper), nested group paths, the inline help/about view, `sigrun connections password`.
`### Fixed` — Delete on connections and keys (the webview ignores `window.confirm`), Windows named-pipe collisions between hosts with different data dirs, the window closing before a failed `ssh` printed its error, the Linux terminal-detection test.
`### Changed` — Start minimized moved to the end of Settings; the launched command now drops into a shell after a failed connection.
Missing or duplicated bullets are fixed here; nothing unlisted in `git diff` is left out.

- [ ] **Step 2: Verify the version pairing**

Run: `grep -m1 '^version' Cargo.toml && grep -m1 '"version"' apps/desktop/package.json`
Expected: the same version, `0.1.0`. Nothing in this plan is a breaking change, and nothing has been released yet, so the first tag stays `v0.1.0`.

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md Cargo.toml apps/desktop/package.json
git commit -m "docs(changelog): record the password, groups, help and fix work"
```

- [ ] **Step 4 (only when you actually cut a release): rename the unreleased section and tag**

```bash
# rename "## [Unreleased]" to "## [0.1.0] - <today>" (Keep a Changelog),
# then, with the version already 0.1.0 in both files:
git tag v0.1.0 && git push origin main --tags
```
Expected: the release workflow builds the installers; `version-sync` passes because the tag, `Cargo.toml` and the new `## [0.1.0]` heading agree. Do **not** tag from a dirty tree, and do not bump to `0.2.0` before 0.1.0 has ever shipped.

---

### Task 11: `AGENTS.md` — the project and the rules we work by

**Files:**
- Create: `AGENTS.md`

**Interfaces:**
- Consumes nothing; produces the document an agent (or a new contributor) reads before touching the repo.

- [ ] **Step 1: Write `AGENTS.md` at the repo root** with these sections, each stating what the code actually does today:

  - **What this is.** Sigrun, an SSH key and connection manager for macOS, Linux and Windows; the tagline; the Sigrún name gloss (*sigr* victory, *rún* secret); MIT; public.
  - **Layout.** `crates/sigrun-core` (all domain logic), `crates/sigrun-cli` (the `sigrun` binary), `apps/desktop` (Tauri 2 + React 19 + TypeScript + Tailwind v4 in `src/`, Rust shell in `src-tauri/`), `docs/superpowers/specs/`, `docs/superpowers/plans/`, `.github/workflows/`, `CHANGELOG.md`.
  - **Architecture.** The desktop app *is* the host: it owns the vault, the SSH agent and the IPC server, enforces single instance and minimises to the tray on close; the CLI is a client over the Unix socket / Windows named pipe; all logic lives in `sigrun-core`, never in the Tauri layer. IPC is newline-delimited JSON: `Request` is internally tagged, `Response` adjacently tagged (`{ "type": …, "value": … }`).
  - **Security rules (non-negotiable).** `~/.ssh` is read-only (reads and `-i` only; the single exception is appending `authorized_keys` during an explicit provision). The vault uses Argon2id (m=65536, t=3, p=4, 16-byte salt) with XChaCha20-Poly1305 and a fresh 24-byte nonce per seal, written atomically at `0600` inside a `0700` directory. Secrets live only in the vault: never in argv, a command line, the environment, a log line, an error message or the persisted model; `Secret` has no `Clone` and a redacting `Debug`. Host-key trust is TOFU against Sigrun's own `known_hosts` and fails closed.
  - **Code rules.** Rust 2021, MSRV 1.85, dependencies only via `[workspace.dependencies]`. The gate is `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --all-targets`, plus the frontend gate when TS changes. Every user-facing string goes through `t(...)` and lands in **both** `en.json` and `da.json` (a recursive completeness test enforces it). Tests assert real behaviour; frontend tests use built-in matchers and `within(...)` to disambiguate. No new dependency without a strong reason.
  - **Platform rules.** Windows named pipes are derived from the user **and** a hash of the data dir (two data dirs must never share a pipe); tests that bind a socket pick a unique per-test name. Terminal launch is per platform (AppleScript for Terminal/iTerm, `-e` for Ghostty/kitty/Alacritty/WezTerm, `sh -c` on Linux, `cmd.exe /k` on Windows) and every launched command ends in `|| exec ${SHELL:-/bin/sh}` so a failed `ssh` keeps its error on screen.
  - **Process rules.** Design first (`docs/superpowers/specs/`), then a plan (`docs/superpowers/plans/`), then subagent-driven execution: a fresh implementer per task, a task reviewer after each, a fix loop of at most five rounds, a whole-branch review at the end, progress in `.superpowers/sdd/<plan>/progress.md`. One commit per task, conventional message. Test first, then implement. Claim nothing without running the gate and quoting its output. The controller never fixes findings itself.
  - **Release rules.** The version lives in exactly two places (`Cargo.toml`, `apps/desktop/package.json`) and CI's `version-sync` job keeps them in lockstep; every change gets a Keep-a-Changelog entry under `## [Unreleased]`; tags matching `v*` trigger the installer workflow; the first tag is `v0.1.0`; never tag from a dirty tree.
  - **Git rules.** Branch per plan (`plan-<n>-<topic>`), merged with `--no-ff`; never push, force-push or delete a remote branch without asking.

- [ ] **Step 2: Verify every claim against the repo before committing** — grep the `version-sync` job, the Argon2/ChaCha constants in `crates/sigrun-core/src/vault/crypto.rs`, `pipe_id`/`dir_hash` in `paths.rs`, the terminal arms in `terminal.rs`, and the i18n guard — and correct anything the document overstates.

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md
git commit -m "docs: add AGENTS.md with the project layout and working rules"
```

---

## Self-Review

- **Spec coverage:** §6.1 connection manager (password field, Delete, nested groups, help view) — Tasks 2, 3, 5, 6, 7, 9; §6.3 provisioning unchanged but now benefits from a stored password; §7.6 secret handling — the password never leaves the vault except over the 0600 socket to the helper (Tasks 3, 4). Password-only connections already worked through ssh's own prompt; this plan adds storage and one-click connect.
- **Step scan:** every step names a test, a command with an expected result, or an exact signature; no "handle edge cases".
- **Type consistency:** `ConnectionsSetPassword { id, password: Option<String> }` and `PasswordGet { id }` are defined once (Task 3) and consumed unchanged in Tasks 4-6; `has_password` is a host-filled `bool` on `Connection` everywhere.
- **Review Focus:** each of the five lines has a named test in the owning task (locked vault: Task 3 + the UI's Task 5; key-first: Task 4's argv test; id namespaces: Task 3's remove test; terminal coverage: Task 4's per-terminal assertions; helper failure: Task 4's CLI test).
- **Proportion:** ten tasks; the code blocks are tests and the pinned probes and string shapes.
- **Risk I am flagging now:** macOS Terminal.app/iTerm get the environment through the command line, not through the spawner's environment — if that turns out not to work for a given terminal, the password feature degrades to ssh's own prompt there (never to a wrong password), and the plan's manual check will show it.
