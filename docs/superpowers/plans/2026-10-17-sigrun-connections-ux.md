# Sigrun — Connections UX + first-time-connect plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make connections legible (groups visible), make the key-install bootstrap reachable (no dead end for disk-backed keys), tell the user *before* a terminal opens why a host is unreachable, and open iTerm in a tab instead of a window — without dropping Ghostty/Alacritty support.

**Architecture:** Two small core changes (`force` on the connect request + a TCP reachability probe; one iTerm AppleScript branch) and three frontend changes (grouped list, panel that accepts disk keys and imports them, error text that says what happened). Nothing in the vault, agent or provisioning engine changes.

**Tech Stack:** Rust (sigrun-core, russsh-free here), Tauri 2 commands, React 19 + TypeScript + Tailwind, i18next, vitest + Testing Library, clap.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` §6.1 (connection manager), §6.3 (provisioning), §8 (UI).

## Global Constraints

- `~/.ssh` stays read-only; no secret in argv, a log line, or an error message.
- Every user-facing string goes through `t(...)`; new keys land in **both** `en.json` and `da.json` (the recursive completeness guard enforces it).
- English-only output on the CLI.
- Gate per task: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --all-targets` plus, when frontend files change, `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build`; `actionlint` when workflows change.
- One commit per task, message given in the task.

## Review Focus

1. A connection with a group but **no usable key**: the group heading still renders and the panel offers the "import a key" path rather than a dead label.
2. An **encrypted disk key** selected with an empty key passphrase: the import error appears in the panel, provisioning is not attempted, and no passphrase reaches a log or message.
3. Probe **false negative** (host reachable only through `~/.ssh/config` ProxyJump/VPN): "Connect anyway" still opens the terminal, and provisioning is never blocked by the probe.
4. **iTerm not running or zero windows**: the launch still opens a window running the command (no silent no-op).
5. **Blank/whitespace group names** count as ungrouped — same rule as the tray menu.

---

### Task 0: Land the tray branch

**Files:** none (git only).

**Interfaces:** Consumes nothing; Produces a `main` that already contains `tray.rs` grouping, so Tasks 1 and 5 do not collide with it.

- [ ] **Step 1:** `git checkout main && git merge --no-ff tray-menu -m "Merge tray menu redesign and localization"` (branch is 2 commits: `704d39d`, `1640b47`).
- [ ] **Step 2:** Run the full gate on the merged result; `cargo test --all-targets` must show 11 desktop tests.
- [ ] **Step 3:** `git branch -d tray-menu`.
- [ ] **Step 4:** Report the merge commit to the human partner; do not push.

---

### Task 1: Grouped Connections list

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/components/ConnectionRow.tsx`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `Connection[]` from `listConnections` (`api.ts:6-16`, `group: string | null`).
- Produces: `export function groupConnections(conns: Connection[]): { group: string | null; items: Connection[] }[]` in `Connections.tsx` — ungrouped (`null`, `""`, whitespace-only) first, then groups sorted with `localeCompare`, items sorted by label inside a group (mirrors `tray_menu_model`).

- [ ] **Step 1: Write the failing tests** (append to `Connections.test.tsx`, reuse its existing `vi.mock("../api")` harness)

```tsx
it("groups connections under headings", async () => {
  api.listConnections.mockResolvedValue([
    { ...conn, id: "c1", label: "pve01", group: "PVE" },
    { ...conn, id: "c2", label: "web", group: null },
  ]);
  render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
  expect(await screen.findByRole("heading", { name: "PVE" })).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "Ungrouped" })).toBeNull();
});

it("treats a blank group as ungrouped", () => {
  const out = groupConnections([{ ...conn, id: "a", group: "   " }]);
  expect(out).toHaveLength(1);
  expect(out[0].group).toBeNull();
});
```

- [ ] **Step 2: Run to verify failure.** `cd apps/desktop && npm test -- --run Connections` → FAIL (`groupConnections` undefined; no heading).
- [ ] **Step 3: Implement.** Replace the flat `<ul>{conns.map(...)}</ul>` with a `<section>` per group: a `<h3>` with the group name only when `group !== null`, then a `<ul>`. Add the group chip to `ConnectionRow` next to the label (`<span className="rounded border border-border px-1 text-xs text-muted">{conn.group}</span>`) only when a group exists and no heading is shown — decision: show the chip always, headings carry the structure. Add the `onGoToKeys: () => void` prop to `ConnectionRow`/`Form` (used in Task 2).
- [ ] **Step 4: Run to verify pass.** `npm test -- --run Connections` → PASS.
- [ ] **Step 5: Commit.** `feat(desktop): show connections grouped by their group`

---

### Task 2: First-time-connect bootstrap (disk keys + honest errors)

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx` (`ProvisionPanel`, `hasVaultKey`, the grey hint)
- Modify: `apps/desktop/src/components/ConnectionRow.tsx` (hint → button)
- Modify: `apps/desktop/src/api.ts` (`importKey` returns `KeyView`)
- Modify: `apps/desktop/src/App.tsx` (pass `onGoToKeys={() => setTab("keys")}` to `Connections`)
- Modify: `apps/desktop/src/i18n/{en,da}.json`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `importKey(path: string, name: string, passphrase?: string)` — the Rust command returns `Response::Key(KeyView)` (`commands.rs:107-118`, `Response` is adjacently tagged so `call()` unwraps `value`), and `provisionConnection(id, password, keyIds, disablePasswordAuth)`.
- Produces: `export const importKey = (path, name, passphrase?) => call<KeyView>("import_key", …)`; a `KeyView` includes `{ id, name, source: "vault" }` after import.

- [ ] **Step 1: Write the failing tests**

```tsx
it("imports a selected disk key and provisions with the new vault id", async () => {
  api.listKeys.mockResolvedValue([
    { key: { ...keyMeta, id: "sha256:disk", name: "work", source: "disk", disk_path: "/home/u/.ssh/id_work", has_passphrase: false }, available: true },
  ]);
  api.importKey.mockResolvedValue({ key: { ...keyMeta, id: "k-new", name: "work", source: "vault" }, available: true });
  render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: /install/i }));
  fireEvent.click(screen.getByRole("checkbox", { name: "work" }));
  fireEvent.change(screen.getByLabelText(/password/i), { target: { value: "pw" } });
  fireEvent.click(screen.getByRole("button", { name: /install/i, selector: "button[type=submit]" }));
  await waitFor(() => expect(api.importKey).toHaveBeenCalledWith("/home/u/.ssh/id_work", "work", null));
  await waitFor(() => expect(api.provisionConnection).toHaveBeenCalledWith("c1", "pw", ["k-new"], false));
});

it("shows the host's message when provisioning fails for a non-trust reason", async () => {
  api.provisionConnection.mockRejectedValue({ code: "unreachable", message: "connection refused" });
  // …submit… then:
  expect(await screen.findByRole("alert")).toHaveTextContent("connection refused");
});
```

- [ ] **Step 2: Run to verify failure.** `npm test -- --run Connections` → FAIL (no disk-key checkbox; generic error text).
- [ ] **Step 3: Implement**
  - `diskKeys = keys.filter(k => k.key.source === "disk" && k.available)`; render them in a second `<fieldset>` with legend `t("provision.diskKeys")`, checkbox label = key name; keep the existing vault-key fieldset.
  - When any selected disk key has `has_passphrase`, render one extra field `t("provision.keyPassphrase")` (`type="password"`, `autoComplete="off"`).
  - On submit: for each selected disk id → `const v = await importKey(disk_path, name, keyPassphrase || null)`; collect `v.key.id`; then `provisionConnection(conn.id, password, [...vaultSelected, ...importedIds], disable)`. Import errors abort before provisioning and surface via the alert.
  - `hasVaultKey` → `hasUsableKey = keys.some(k => k.available)`; the row's grey note becomes a button `t("provision.importKey")` calling `onGoToKeys()`.
  - Replace `{error && <p role="alert">{t("common.error")}</p>}` with the message text: keep `t("common.error")` when `message` is empty, else show `message`.
  - i18n keys to add in both files: `provision.diskKeys`, `provision.keyPassphrase`, `provision.importKey`.
- [ ] **Step 4: Run to verify pass**, plus `npm run typecheck && npm run lint`.
- [ ] **Step 5: Commit.** `feat(desktop): install keys from disk and show why provisioning failed`

---

### Task 3: Reachability pre-check with an escape hatch

**Files:**
- Modify: `crates/sigrun-core/src/error.rs` (new `Unreachable(String)` variant, `#[error("{0}")]`)
- Modify: `crates/sigrun-core/src/ipc/protocol.rs` (`error_code` arm `"unreachable"`; `Request::ConnectionsConnect { force }` with `#[serde(default)]`)
- Modify: `crates/sigrun-core/src/terminal.rs` (`probe_reachable`)
- Modify: `crates/sigrun-core/src/host.rs` (`connections_connect` takes `force`, probes when `!print_only && !force`)
- Modify: `crates/sigrun-cli/src/cli.rs` (`--force` on `connections connect`)
- Modify: `apps/desktop/src-tauri/src/commands.rs` + `apps/desktop/src/api.ts` + `apps/desktop/src/views/*` (dialog)
- Tests: `crates/sigrun-core/src/terminal.rs`, `crates/sigrun-core/src/ipc/mod.rs`, `crates/sigrun-cli/tests/cli.rs`

**Interfaces:**
- Produces: `pub async fn probe_reachable(host: &str, port: u16, timeout: Duration) -> Result<()>` (`tokio::net::lookup_host` + `TcpStream::connect` under `tokio::time::timeout`; DNS failure → `Error::Unreachable("could not resolve <host>")`, refusal/timeout → `Error::Unreachable("connection refused on <host>:<port>")`); `Request::ConnectionsConnect { id: String, print_only: bool, force: bool }`; command `connect(id, printOnly, force)`; CLI `--force` (skips the probe only).

- [ ] **Step 1: Write the failing tests**

```rust
// terminal.rs
#[tokio::test]
async fn probe_reports_a_closed_port() {
    let err = probe_reachable("127.0.0.1", 1, Duration::from_millis(500)).await.unwrap_err();
    assert!(matches!(err, Error::Unreachable(_)));
}

// ipc/mod.rs — a refused port refuses unless forced
#[tokio::test]
async fn connect_refuses_an_unreachable_host_unless_forced() { /* conn on 127.0.0.1:1; expect code "unreachable", then force: true proceeds to the key/vault checks */ }
```
- [ ] **Step 2: Run to verify failure.** `cargo test -p sigrun-core probe_reports_a_closed_port` → FAIL (no such fn).
- [ ] **Step 3: Implement** the variant, the `error_code` arm, the probe, the request field (`#[serde(default)]` so an old client still decodes), the host check placed **after** key/vault validation and **before** `ssh_invocation`, the CLI flag, and the desktop dialog: new `connectForce(id)`; on code `unreachable` show `role="dialog"` with the message and `t("connections.connectAnyway")` → `connectForce`, plus `t("connections.unreachable")` title.
- [ ] **Step 4: Run to verify pass** (`cargo test --all-targets`, CLI test, frontend suite), plus a live CLI check: `cargo run -p sigrun-cli -- connections connect <bogus> --force --print` prints the rendered command.
- [ ] **Step 5: Commit.** `feat(connect): probe the host first and allow connecting anyway`

---

### Task 4: iTerm opens a tab (Ghostty/Alacritty unchanged)

**Files:**
- Modify: `crates/sigrun-core/src/terminal.rs` (macOS iTerm arm only)
- Test: `crates/sigrun-core/src/terminal.rs` (assert the script strings)

**Interfaces:** Consumes `applescript_escape`; Produces the iTerm spec script (verified by hand on 2026-10-17: `tell current window to create tab with default profile command "…"` created tab 2 of window id 1850):

```applescript
tell application "iTerm"
  if (count of windows) is 0 then
    create window with default profile command "<cmd>"
  else
    tell current window to create tab with default profile command "<cmd>"
  end if
end tell
```

- [ ] **Step 1: Write the failing test** (`terminal.rs`): assert the iTerm script contains `create tab with default profile` and `create window with default profile`, for `Platform::MacOs` with `configured = Some("iterm")`.
- [ ] **Step 2: Run to verify failure.** `cargo test -p sigrun-core iterm` → FAIL.
- [ ] **Step 3: Implement** the script. Leave `ghostty` (`-e`) and `alacritty` (`-e`) exactly as they are — neither has a tab API. **Live-check Terminal.app**: run `osascript -e 'tell application "Terminal" to do script "sleep 5" in window 1'` and confirm the tab count rises; if it does not (it did not on 2026-10-17), leave Terminal.app on `do script` and add the README line instead.
- [ ] **Step 4: Run to verify pass** for the cargo tests, then hand-check once in the running app with `terminal = "iterm"`.
- [ ] **Step 5: Commit.** `feat(connect): open iTerm in a tab of the current window`

---

### Task 5: Docs and translations

**Files:** Modify `README.md`, `CHANGELOG.md`, both `i18n/*.json`.

- [ ] **Step 1:** README: a "First time connecting to a server" paragraph — no key on the server yet means installing one, which needs the server password once (typed in Sigrun, never stored); disk keys are imported into the vault as part of that step; and a terminal note: iTerm opens a tab, Ghostty/Alacritty open a new window (no tab API), `ssh` itself stays the authority on the connection.
- [ ] **Step 2:** README limitation: the reachability probe can false-negative through ProxyJump/VPN — use "Connect anyway".
- [ ] **Step 3:** CHANGELOG `### Added` / `### Fixed` entries for Tasks 1-4.
- [ ] **Step 4:** Full gate, then commit: `docs: document first-time key install and connection groups`

---

## Self-Review

- **Spec coverage:** §6.1 grouping surfaced in the UI, §6.3 bootstrap reachable from the connection, §8 UI copy through i18n; nothing in the vault/agent/provisioning engine changes.
- **Step scan:** every step names a test, a command, or an exact signature/string.
- **Type consistency:** `Connection.group: string | null` and `KeyView { key, available }` unchanged; `importKey` widens its return type to `KeyView`; `force: bool` is added with a serde default so old clients keep working.
- **Review Focus:** each of the five lines has a named test in the task that owns it (Tasks 1, 2, 3, 4).
- **Known-unverified:** Terminal.app tab reuse and the probe's behaviour behind a proxy are called out in Tasks 4 and 3 rather than assumed.
