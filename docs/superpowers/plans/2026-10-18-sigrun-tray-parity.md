# Sigrun — tray parity, Lock button, and README framing

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A tray connection behaves like the app's Connect button (it shows the trust prompt for a new host and reports an unreachable one), the header Lock button stands out in red, and the README/`AGENTS.md` describe Sigrun as what it is — an SSH connection manager with key auth.

**Architecture:** One Tauri event (`connection-error`, payload `{ id, code, message, host, port }`) carries a tray connect failure into the window, where `Connections` already owns both dialogs (fingerprint trust and unreachable) and can re-run the same connect. The tray stops forcing the reachability probe off. The Lock button and the docs are independent of that.

**Tech Stack:** Tauri 2 + React 19 + TypeScript (vitest), Rust (`sigrun-core`, `sigrun-desktop`), Keep a Changelog.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` §6.1 (connection manager), §8 (UI).

## Global Constraints

- `~/.ssh` stays read-only; no secret in argv, an environment *value*, a log line, an error message or the persisted model.
- Every user-facing string goes through `t(...)`; new keys land in **both** `apps/desktop/src/i18n/en.json` and `da.json`.
- The app's own Connect path keeps working unchanged: one dialog, no duplicate prompt, the `vault-locked` gate untouched.
- Gate per task: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`, plus `cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build` whenever a frontend file changes.
- One commit per task, message given in the task; a `## [Unreleased]` CHANGELOG line per behaviour change.

## Review Focus

1. Tray click on a **never-seen host**: the window comes forward with the fingerprint dialog, accepting connects, cancelling leaves nothing armed (no stale retry closure).
2. Tray click on a **dead host**: *Host is not reachable* with **Connect anyway**, bounded by the existing 3 s probe.
3. The app's **own** Connect button is unchanged — no second dialog, no regression in the vault-locked gate.
4. The **Lock button** keeps its label, position and behaviour; only its colour changes.
5. The reframed **README/AGENTS.md** documents only shipped behaviour, and every command quoted in it still exists.

---

### Task 1: A tray connect failure reaches the window

**Files:**
- Modify: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/views/Connections.tsx`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: `connectGuarded(id)` / `connectForce(id)` from `../api`; the existing `trust` (`TrustRetry & { fp, algo, key }`) and `unreachable` (`{ message, retry }`) state and their dialogs in `Connections.tsx`; the `vault-locked` listener pattern in `App.tsx`.
- Produces: the tray emits `connection-error` with `{ id, code, message, host, port }` from its connect error arm; `App.tsx` listens and forwards it as a new optional `connectionError` prop on `Connections`, which maps `host_key_*` to the trust dialog (`run = () => connectGuarded(id)`) and `unreachable` to the unreachable dialog (`retry = () => connectForce(id)`), then calls `onConnectionErrorHandled()`.

- [ ] **Step 1: Write the failing tests** (append to `Connections.test.tsx`, reusing its `vi.mock("../api")` harness)

```tsx
it("opens the trust dialog from a tray error", async () => {
  render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} connectionError={{ id: "c1", code: "host_key_unknown", message: "SHA256:x ssh-ed25519 AAAA", host: "h", port: 22 }} onConnectionErrorHandled={() => {}} />);
  const dialog = await screen.findByRole("dialog");
  expect(dialog.textContent).toContain("SHA256:x");
  fireEvent.click(within(dialog).getByRole("button", { name: /trust and connect/i }));
  await waitFor(() => expect(api.connectGuarded).toHaveBeenCalledWith("c1"));
});

it("opens the unreachable dialog from a tray error", async () => {
  render(<Connections onVaultLocked={() => {}} onGoToKeys={() => {}} connectionError={{ id: "c1", code: "unreachable", message: "connection refused on h:22", host: "h", port: 22 }} onConnectionErrorHandled={() => {}} />);
  const dialog = await screen.findByRole("dialog");
  expect(dialog.textContent).toContain("connection refused on h:22");
  fireEvent.click(within(dialog).getByRole("button", { name: /connect anyway/i }));
  await waitFor(() => expect(api.connectForce).toHaveBeenCalledWith("c1"));
});
```

- [ ] **Step 2: Run to verify failure.** `cd apps/desktop && npm test -- --run Connections` → FAIL (no `connectionError` prop, no dialog).

- [ ] **Step 3: Implement.** `Connections` accepts the optional `connectionError`/`onConnectionErrorHandled` props and, in a `useEffect`, sets the existing state: for a `host_key_*` code split `message` on spaces into `fp`/`algo`/`key` and set `trust = { host, port, fp, algo, key, run: () => connectGuarded(id) }`; for `unreachable` set `unreachable = { message, retry: () => connectForce(id) }`; then call `onConnectionErrorHandled()`. `App.tsx` adds `listen("connection-error", …)` beside its `vault-locked` listener, stores the payload in state and passes it to `Connections`, clearing it in the handler. `tray.rs`'s connect error arm emits `connection-error` with the connection's `id`, the error `code`, its `message`, and the connection's `host`/`port`, keeping `show_main` and the attention title as they are.

- [ ] **Step 4: Run the gate** (Rust + frontend) → green.

- [ ] **Step 5: Commit** — `git add -A apps/desktop && git commit -m "feat(desktop): surface tray connect failures in the window"`

---

### Task 2: The tray runs the reachability probe

**Files:**
- Modify: `apps/desktop/src-tauri/src/tray.rs`

**Interfaces:**
- Consumes: `connect_guarded(state, id, print_only, force)`.
- Produces: the tray calls it with `force = false`, so a dead host returns `unreachable` and Task 1's event shows *Host is not reachable* in the window; a helper `fn tray_connect_force() -> bool { false }` exists so the argument is testable.

- [ ] **Step 1: Write the failing test** — in `tray.rs`'s inline test module:

```rust
#[test]
fn tray_connects_run_the_reachability_probe() {
    assert!(!tray_connect_force(), "the tray must not skip the reachability probe");
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test -p sigrun-desktop tray` → FAIL (`tray_connect_force` undefined).

- [ ] **Step 3: Implement.** Add `fn tray_connect_force() -> bool { false }` with a comment explaining that a tray click is no longer a "just connect" action — a dead host must be reported in the window, at the cost of up to the 3 s probe — and use it at the tray's `connect_guarded` call site.

- [ ] **Step 4: Run the gate** → green.

- [ ] **Step 5: Commit** — `git commit -am "feat(desktop): probe the host before a tray connect"`

---

### Task 3: The Lock button stands out

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Test: `apps/desktop/src/App.test.tsx`

**Interfaces:**
- Produces: the header Lock button's class becomes `rounded border border-danger px-2 py-1 text-danger hover:bg-danger hover:text-bg`; its label, key and handler are unchanged.

- [ ] **Step 1: Write the failing test** — in `App.test.tsx`, with the existing api mock:

```tsx
it("styles the lock button as a danger action", async () => {
  render(<App />);
  const lock = await screen.findByRole("button", { name: /lock/i });
  expect(lock.className).toContain("text-danger");
});
```

- [ ] **Step 2: Run to verify failure.** `npm test -- --run App` → FAIL.

- [ ] **Step 3: Implement.** Apply the class to the Lock button in the header.

- [ ] **Step 4: Run the gate** → green.

- [ ] **Step 5: Commit** — `git commit -am "feat(desktop): make the lock button stand out"`

---

### Task 4: Reframe the README around connection management

**Files:**
- Modify: `README.md`, `AGENTS.md`, `CHANGELOG.md`

**Interfaces:**
- Produces: docs only; no code, no i18n.

- [ ] **Step 1: Rewrite the README opening.** Lead with what Sigrun *is*: an SSH connection manager — add a host, group it (`PVE/Folder 1`), click Connect and it opens in your terminal, from the app, the tray or the CLI — then present keys as what makes those connections work (encrypted vault, generate/import/discover, the agent serving vault keys to `ssh`, installing a key on a server with a password). Reorder the feature bullets and the quick start to the same order (add a connection → connect → then keys and password). Keep the existing password-connections, groups, terminal and first-time-connect sections.

- [ ] **Step 2: Align `AGENTS.md`.** Its "What this is" first sentence becomes "Sigrun is a cross-platform (macOS, Linux, Windows) SSH connection manager with key-based authentication." — nothing else changes.

- [ ] **Step 3: CHANGELOG.** Add a `### Changed` line under `## [Unreleased]`: the README and AGENTS.md now describe Sigrun as a connection manager with key auth.

- [ ] **Step 4: Verify the claims.** `grep -n "sigrun " README.md` — every command quoted still exists (`sigrun host`, `status`, `vault …`, `keys …`, `connections …`); no version bump.

- [ ] **Step 5: Run the gate** (Rust only; docs) → green.

- [ ] **Step 6: Commit** — `git add README.md AGENTS.md CHANGELOG.md && git commit -m "docs: reframe the README around connection management"`

---

## Self-Review

- **Spec coverage:** §6.1's tray/connection surface (Tasks 1–2), §8's UI polish (Task 3) and the product framing (Task 4). No task implements anything the spec does not already promise.
- **Step scan:** every step names a test, a command with an expected result, or an exact prop/signature; no "handle edge cases".
- **Type consistency:** `connection-error` carries `{ id, code, message, host, port }` in both the tray emitter (Task 1) and the frontend consumer; `connectionError`/`onConnectionErrorHandled` are optional props so existing render sites keep compiling.
- **Review Focus:** line 1 → Task 1's trust test; line 2 → Task 1's unreachable test plus Task 2's force assertion; line 3 → the existing app Connect tests must stay green; line 4 → Task 3's class test; line 5 → Task 4's claim check.
- **Proportion:** four small tasks, three of them one-file changes; the only cross-cutting piece is the event payload, pinned above.
- **Risk flagged now:** if the window is hidden when the tray emits the event, the event may arrive before the webview has listeners; Task 1 keeps `show_main` first (the Plan 5 `vault-locked` precedent), and the manual check below is the proof.
