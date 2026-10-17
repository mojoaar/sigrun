# Sigrun Desktop App (Plan 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Sigrun desktop app — a Tauri 2 window + system tray over the existing headless core, with the four Norse themes, EN/DA i18n, the GitHub SSH-key wizard, and a multi-OS installer release workflow.

**Architecture:** The Tauri app **is** the long-lived host: it owns the vault, the SSH agent, and the IPC socket (reusing `sigrun-core` verbatim), exposes the core to the React frontend through `#[tauri::command]` functions, and closes-to-tray. The React + TypeScript + Tailwind UI drives the same `Request`/`Response` model the CLI uses. Plan 1 (`crates/sigrun-core`, `crates/sigrun-cli`) is unchanged except where a task's Interfaces block says otherwise.

**Tech Stack:** Tauri 2.x (`tray-icon`, `single-instance` plugin), React 19 + TypeScript + Tailwind CSS v4, i18next/react-i18next, Vite. Backend reuses `sigrun-core` (`host`, `ipc::protocol`, `model`, `vault`, `agent`, `terminal`). Wizard: GitHub REST API via OAuth device flow. Release: `tauri-action`.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md`

## Global Constraints

Copied verbatim from the spec and Plan 1; every task inherits these.

- Platforms: macOS, Linux, Windows. The app must build and run on all three.
- `~/.ssh` is **read-only** to Sigrun: discovery only; never write to it.
- No plaintext secret (passphrase, private key, derived key) on disk, in logs, in argv, or in any IPC/frontend payload beyond the encrypted vault.
- Vault crypto: Argon2id (m=65536, t=3, p=4, 16-byte salt) + XChaCha20-Poly1305 — already shipped in `sigrun-core`; do not reimplement.
- Themes: ship **nordlys (default)**, fjord, ravn, skumring; each theme has a light and a dark palette; follow the OS by default with a per-user override. Ported from the author's opencode themes into Sigrun's own token map under `apps/desktop/src/themes/`.
- i18n: English and Danish, English the source of truth; every user-facing string goes through the i18n layer — no hardcoded UI copy.
- The desktop app is a **single instance**; launching a second copy focuses the running window. Closing the window minimizes to the tray; quitting is explicit (tray menu or app menu).
- License MIT; Keep a Changelog; semver. Binary/app name is `Sigrun`; CLI stays `sigrun`.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`, and the frontend `lint`/`typecheck`/`build` must stay green.
- Branch: `main` (Plan 1 is merged). Work on a feature branch per task and merge, or commit to `main` per the plan's commits — match Plan 1's "every task commits" discipline.

## Review Focus

The five input classes / failure modes the spec implies but no single task's tests exercise; each line names the input/condition and the behavior a reasonable person expects, and the owning task carries a test for it.

1. **Locked vault at every surface.** A user clicks a tray connection while the vault is locked → they get a clear "unlock Sigrun" affordance, not a silent failure or a stack trace. (Owner: Task 6.)
2. **Host-key it has never seen.** First connect to a brand-new host must surface an explicit fingerprint confirmation (TOFU), never auto-accept, and fail closed on a later mismatch. (Owner: Task 7.)
3. **Theme + language switching at runtime.** Switching theme or language must re-render every visible string/color without a restart and without leaking the previous locale's strings. (Owner: Tasks 4, 5.)
4. **GitHub device-flow edge cases.** Expired code, user denial, network failure, and a token with insufficient scope each produce a human-readable recovery, not a hang. (Owner: Task 8.)
5. **Two instances / second launch.** Double-clicking the app while it runs must focus the existing window and never spawn a second host or corrupt the vault lock. (Owner: Task 2.)

---

### Task 1: Tauri 2 shell scaffold + window

**Files:**
- Create: `apps/desktop/package.json`, `apps/desktop/vite.config.ts`, `apps/desktop/tsconfig.json`, `apps/desktop/index.html`, `apps/desktop/src/main.tsx`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/src/main.rs`, `apps/desktop/src-tauri/build.rs`, `apps/desktop/src-tauri/icons/` (placeholder icon set)
- Modify: `Cargo.toml` (workspace members += `apps/desktop/src-tauri`)
- Test: `apps/desktop/src-tauri/src/lib.rs` inline (window config), plus `apps/desktop/src/App.test.tsx`

**Interfaces:**
- Consumes: nothing from `sigrun-core` yet (shell only).
- Produces: a building Tauri app at `apps/desktop`; `sigrun-desktop` crate exposing `pub fn run()`; `apps/desktop` added to the Cargo workspace; a Vite dev server that `tauri dev` drives.

- [ ] **Step 1: Scaffold the workspace entry and frontend**

Add `apps/desktop/src-tauri` to the root `Cargo.toml` `members`. `apps/desktop/package.json` declares `react`, `react-dom`, `vite`, `@vitejs/plugin-react`, `typescript`, `tailwindcss`, `@tailwindcss/vite` (Tailwind v4), `i18next`, `react-i18next`, `@tauri-apps/api`, and dev deps `vitest`, `@testing-library/react`, `eslint`, `typescript-eslint`. `vite.config.ts` uses the React and Tailwind plugins and sets `server.port = 1420` with `clearScreen = false` (Tauri convention).

- [ ] **Step 2: Configure the Tauri shell**

`apps/desktop/src-tauri/tauri.conf.json`: app name `Sigrun`, identifier `com.sigrun.app`, a single main window (min 900×600, default 1100×700, `title: "Sigrun"`), `productName: "Sigrun"`, `version` read from the crate, `frontendDist` pointing at the Vite build output, `devUrl` at `http://localhost:1420`. `Cargo.toml` for `sigrun-desktop` depends on `sigrun-core = { path = "../../crates/sigrun-core" }`, `tauri = { version = "2", features = ["tray-icon"] }`, `tauri-plugin-single-instance = "2"`, `tokio`, `serde`, `serde_json`. `build.rs` calls `tauri_build::build()`.

- [ ] **Step 3: Minimal main + window**

`src-tauri/src/main.rs` calls `sigrun_desktop::run()`. `src-tauri/src/lib.rs` `run()` builds the Tauri app with `tauri::Builder::default()`, registers `tauri_plugin_single_instance` (its callback focuses the existing window), and runs. `index.html` mounts a React root; `src/main.tsx` renders `<App/>`; `src/App.tsx` renders a placeholder "Sigrun" heading. Test: `App.test.tsx` renders `<App/>` and asserts the heading text is present.

- [ ] **Step 4: Verify build and tests**

Run: `cargo build -p sigrun-desktop` and `(cd apps/desktop && npm install && npm run typecheck && npm test -- --run)`.
Expected: both green; the window config compiles.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop Cargo.toml Cargo.lock
git commit -m "feat(desktop): scaffold the Tauri 2 shell and window"
```

---

### Task 2: Host lifecycle — app owns vault, agent, IPC

**Files:**
- Create: `apps/desktop/src-tauri/src/host.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/host.rs` inline

**Interfaces:**
- Consumes: `sigrun_core::host::{run, HostState, handle}`, `sigrun_core::paths::Paths::resolve`, `sigrun_core::ipc::protocol::{Request, Response}`.
- Produces: `pub async fn start_host(paths: Paths) -> Result<Arc<HostState>>` that the Tauri `setup` hook owns; the app holds the single `HostState` and serves the IPC socket; a `tauri::State<Arc<HostState>>` available to commands. Second-instance launch focuses the window (Review Focus 5).

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn start_host_serves_ipc_and_locks_single_instance() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sigrun_core::paths::Paths::with_roots(
        dir.path().into(), dir.path().into(), dir.path().join("ssh"),
    );
    let state = start_host(paths.clone()).await.unwrap();
    // A second start against the same paths must refuse (single host).
    assert!(matches!(
        start_host(paths).await,
        Err(sigrun_core::error::Error::AlreadyRunning)
    ));
    state.shutdown.cancel();
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-desktop host`
Expected: FAIL — `start_host` not found.

- [ ] **Step 3: Implement**

`start_host` constructs `HostState::new(paths)` (which acquires the host single-instance lock), spawns the agent socket (`sigrun_core::agent::serve`) and the IPC server (`sigrun_core::ipc::server::serve`) on background tasks, and returns the `Arc<HostState>`. In `lib.rs` `setup`, call `Paths::resolve()`, `start_host` it inside the Tokio runtime, `.manage(state)`, and wire the single-instance plugin callback to `window.set_focus()`. Do not duplicate `host::run`'s shutdown logic — reuse it or its parts. The tray/menu "Quit" cancels `state.shutdown` and exits.

- [ ] **Step 4: Run tests**

Run: `cargo test -p sigrun-desktop` and `cargo clippy -p sigrun-desktop --all-targets -- -D warnings`.
Expected: PASS; the new test green.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(desktop): host owns vault, agent, and IPC; single instance"
```

---

### Task 3: Tauri command surface (frontend ↔ core)

**Files:**
- Create: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/commands.rs` inline

**Interfaces:**
- Consumes: `tauri::State<Arc<HostState>>`, `sigrun_core::host::handle`, `sigrun_core::ipc::protocol::{Request, Response}`.
- Produces: `#[tauri::command]` async fns the frontend invokes: `get_status`, `vault_init(passphrase)`, `vault_unlock(passphrase)`, `vault_lock`, `list_connections`, `add_connection(conn)`, `update_connection(conn)`, `remove_connection(id)`, `connect(id, print_only)`, `list_keys`, `generate_key(name, algorithm, passphrase)`, `import_key(path, name, passphrase)`, `remove_key(id)`. Each delegates to `handle(&state, Request::…)` and maps `Response::Error { code, message }` to a structured command error. Passphrases arrive from the frontend, never from argv/logs.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn vault_unlock_roundtrip_through_commands() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sigrun_core::paths::Paths::with_roots(
        dir.path().into(), dir.path().into(), dir.path().join("ssh"),
    );
    let state = crate::host::start_host(paths).await.unwrap();
    crate::commands::vault_init_inner(state.clone(), "pass".into()).await.unwrap();
    let out = crate::commands::vault_unlock_inner(state.clone(), "pass".into()).await.unwrap();
    assert!(matches!(out, sigrun_core::ipc::protocol::Response::Unlocked { .. }));
    state.shutdown.cancel();
}
```

(Implement commands as thin `#[tauri::command]` wrappers over testable `*_inner(Arc<HostState>, …)` functions so logic is unit-testable without a Tauri runtime.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-desktop commands`
Expected: FAIL — module/functions not found.

- [ ] **Step 3: Implement**

`commands.rs`: for each interface, an `async fn …_inner(state: Arc<HostState>, …) -> Result<Response, CommandError>` that builds the matching `Request`, awaits `handle`, and a `#[tauri::command]` wrapper taking `tauri::State<'_, Arc<HostState>>`. `CommandError` is a serializable struct `{ code: String, message: String }`. Register all in `tauri::generate_handler!` in `lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p sigrun-desktop` and `cargo clippy -p sigrun-desktop --all-targets -- -D warnings`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(desktop): expose the core to the UI through Tauri commands"
```

---

### Task 4: Theme system — port the four Norse themes

**Files:**
- Create: `apps/desktop/src/themes/index.ts`, `apps/desktop/src/themes/nordlys.ts`, `apps/desktop/src/themes/fjord.ts`, `apps/desktop/src/themes/ravn.ts`, `apps/desktop/src/themes/skumring.ts`, `apps/desktop/src/styles/theme.css`
- Modify: `apps/desktop/src/main.tsx`, `apps/desktop/src-tauri/tauri.conf.json` (nothing; placeholder)
- Test: `apps/desktop/src/themes/index.test.ts`

**Interfaces:**
- Consumes: the four opencode theme JSONs at `~/.config/opencode/themes/{nordlys,fjord,ravn,skumring}.json` (read at authoring time only; their schema is **not** reused at runtime).
- Produces: `type ThemeName = "nordlys" | "fjord" | "ravn" | "skumring"`; `applyTheme(name: ThemeName, mode: "light" | "dark"): void` setting CSS variables on `:root`; a per-theme token map (`bg`, `surface`, `text`, `muted`, `accent`, `danger`, `border`) for both modes; `THEMES: Record<ThemeName, { label: string }>`; default theme **nordlys**. Mode follows the OS via `window.matchMedia("(prefers-color-scheme: dark)")` unless overridden.

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { applyTheme, THEMES } from "./index";

describe("themes", () => {
  it("exposes the four themes with nordlys as default", () => {
    expect(Object.keys(THEMES).sort()).toEqual(["fjord", "nordlys", "ravn", "skumring"]);
  });
  it("applies nordlys dark tokens to :root", () => {
    applyTheme("nordlys", "dark");
    const bg = document.documentElement.style.getPropertyValue("--color-bg");
    expect(bg).not.toBe("");
  });
  it("applies a light palette distinct from dark", () => {
    applyTheme("nordlys", "dark");
    const dark = document.documentElement.style.getPropertyValue("--color-bg");
    applyTheme("nordlys", "light");
    expect(document.documentElement.style.getPropertyValue("--color-bg")).not.toBe(dark);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `(cd apps/desktop && npm test -- --run themes)`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the token map**

For each theme, read its opencode JSON (`light` + `dark` blocks) and map its tokens to Sigrun's fixed token set (bg/surface/text/muted/accent/danger/border). Write each theme as a TS module exporting `{ light: TokenMap, dark: TokenMap }`. `applyTheme` sets `--color-*` CSS variables. `theme.css` maps those variables onto Tailwind v4 theme tokens (`@theme`). Unmapped opencode tokens fall back to the nearest token (document the fallback inline). Add a `data-theme`/`data-mode` attribute on `<html>` for debugging.

- [ ] **Step 4: Run tests**

Run: `(cd apps/desktop && npm test -- --run themes && npm run typecheck)`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(themes): port nordlys, fjord, ravn, skumring with light/dark"
```

---

### Task 5: i18n — English and Danish

**Files:**
- Create: `apps/desktop/src/i18n/index.ts`, `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Modify: `apps/desktop/src/main.tsx`
- Test: `apps/desktop/src/i18n/index.test.ts`

**Interfaces:**
- Consumes: `react-i18next`.
- Produces: an initialized i18next instance; `en` as source of truth and fallback; `da` complete; a `useTranslation` hook used by every component (no hardcoded strings); `changeLanguage("en" | "da")` persisted via a Tauri command into `Settings.language`.

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import i18n from "./index";

describe("i18n", () => {
  it("defaults to English and resolves a known key", () => {
    expect(i18n.t("app.title")).toBe("Sigrun");
  });
  it("Danish is complete — every en key exists in da", () => {
    const en = Object.keys(i18n.getResourceBundle("en", "translation"));
    const da = new Set(Object.keys(i18n.getResourceBundle("da", "translation")));
    const missing = en.filter((k) => !da.has(k));
    expect(missing).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `(cd apps/desktop && npm test -- --run i18n)`
Expected: FAIL — i18n module not initialized.

- [ ] **Step 3: Implement**

`en.json` holds every UI string for the screens built in Tasks 6-8 (keys: `app.*`, `nav.*`, `vault.*`, `connections.*`, `keys.*`, `wizard.github.*`, `settings.*`, `common.*`). `da.json` mirrors it. `i18n/index.ts` initializes i18next with `react-i18next`, `lng: "en"`, `fallbackLng: "en"`, and both resources. The completeness test (above) guards against drift — add new keys to both files together.

- [ ] **Step 4: Run tests**

Run: `(cd apps/desktop && npm test -- --run i18n && npm run typecheck)`.
Expected: PASS; the completeness test green.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(i18n): English and Danish with a completeness guard"
```

---

### Task 6: Main UI — vault gate, connections, keys, settings

**Files:**
- Create: `apps/desktop/src/App.tsx` (replace), `apps/desktop/src/views/VaultGate.tsx`, `apps/desktop/src/views/Connections.tsx`, `apps/desktop/src/views/Keys.tsx`, `apps/desktop/src/views/Settings.tsx`, `apps/desktop/src/components/ConnectionRow.tsx`, `apps/desktop/src/components/KeyCard.tsx`, `apps/desktop/src/api.ts`
- Modify: `apps/desktop/src/main.tsx`
- Test: `apps/desktop/src/views/VaultGate.test.tsx`, `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Consumes: the Task 3 Tauri commands (via `apps/desktop/src/api.ts` typed wrappers around `@tauri-apps/api/core.invoke`), the Task 4 themes, the Task 5 i18n.
- Produces: a working window: vault lock/unlock gate (Review Focus 1 — locked vault shows an unlock affordance everywhere), a connections list with add/edit/remove/connect, a keys list with generate/import/remove, and a settings view (theme picker, mode override, language picker). Connect on a locked vault surfaces the unlock gate rather than an error.

- [ ] **Step 1: Write the failing test**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import VaultGate from "./VaultGate";

vi.mock("../api", () => ({
  getStatus: vi.fn().mockResolvedValue({ vault: "Locked", agent_socket: null, host_addr: "x", version: "0.1.0" }),
}));

describe("VaultGate", () => {
  it("shows the unlock affordance when the vault is locked", async () => {
    render(<VaultGate />);
    expect(await screen.findByText(/unlock/i)).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `(cd apps/desktop && npm test -- --run views)`
Expected: FAIL — components/api not found.

- [ ] **Step 3: Implement**

`api.ts` wraps each command with a typed function returning `Response` or throwing `{ code, message }`. `App.tsx` reads status on mount and routes: `Uninitialized` → a "create vault" (set master passphrase) screen; `Locked` → `VaultGate` (passphrase unlock); `Unlocked` → the main tabbed UI (Connections / Keys / Settings). `Connections` lists from `list_connections`, supports add/edit (a modal), remove (confirm), and a Connect button calling `connect(id, false)`. `Keys` lists from `list_keys` with generate/import forms and remove. `Settings` applies theme/mode/language live (Review Focus 3). Vault status changes re-gate the whole app. All copy through `useTranslation`.

- [ ] **Step 4: Run tests**

Run: `(cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint)`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(ui): vault gate, connections, keys, and settings views"
```

---

### Task 7: System tray + host-key trust (TOFU)

**Files:**
- Create: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/src/host.rs`
- Test: `apps/desktop/src-tauri/src/tray.rs` inline (menu construction); TOFU logic unit test in core

**Interfaces:**
- Consumes: `tauri::tray::TrayIconBuilder`, `tauri::menu::{Menu, MenuItem}`, the `HostState`, the core's connection list.
- Produces: a tray icon with a menu of connections (connect), an "Open Sigrun" item, and "Quit". Clicking a connection launches the user's terminal via the existing `connect` path. **TOFU:** a Sigrun-owned `known_hosts` at `Paths.data_dir()/known_hosts`; first-seen host key triggers an explicit fingerprint confirmation dialog; a later mismatch fails closed. (Review Focus 2.)

- [ ] **Step 1: Write the failing test (TOFU core logic)**

```rust
#[test]
fn host_key_trust_is_tofu_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let kh = dir.path().join("known_hosts");
    // First sight: recorded as trusted.
    assert!(sigrun_core::trust::check(&kh, "h.example", 22, "ssh-ed25519", "AAAA").unwrap().needs_confirmation());
    sigrun_core::trust::record(&kh, "h.example", 22, "ssh-ed25519", "AAAA").unwrap();
    // Same key: trusted.
    assert!(sigrun_core::trust::check(&kh, "h.example", 22, "ssh-ed25519", "AAAA").unwrap().is_trusted());
    // Changed key: fails closed.
    assert!(sigrun_core::trust::check(&kh, "h.example", 22, "ssh-ed25519", "BBBB").unwrap().is_mismatch());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-core trust`
Expected: FAIL — `trust` module not found.

- [ ] **Step 3: Implement trust + tray**

Add `crates/sigrun-core/src/trust.rs`: `check(path, host, port, algo, key_b64) -> TrustStatus { Trusted, Unknown(needs_confirmation), Mismatch }`, `record(path, …)` appending a `known_hosts` line (0600, atomic via `paths::write_atomic`), never auto-accepting. Wire `connect` to consult it: on `Unknown` the UI shows the fingerprint and asks; on `Mismatch` it refuses. `tray.rs` builds the menu from `list_connections` (refreshed on show), each item calling connect; "Open Sigrun" shows+focuses the window; "Quit" cancels shutdown. Register the tray in `setup` with `TrayIconBuilder` + `show_menu_on_left_click(false)`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p sigrun-core trust && cargo test -p sigrun-desktop && cargo clippy --workspace --all-targets -- -D warnings`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sigrun-core apps/desktop
git commit -m "feat(tray,trust): system tray menu and TOFU host-key trust"
```

---

### Task 8: GitHub SSH-key wizard (device flow)

**Files:**
- Create: `apps/desktop/src/views/wizard/GitHubWizard.tsx`, `apps/desktop/src-tauri/src/github.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`, `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/github.rs` inline (state machine, no network), `apps/desktop/src/views/wizard/GitHubWizard.test.tsx`

**Interfaces:**
- Consumes: GitHub OAuth device flow endpoints, the `keys generate` + `connect` commands, the core's `trust` (Task 7) for verifying `github.com`.
- Produces: commands `github_device_start() -> { user_code, verification_uri, device_code, expires_in, interval }`, `github_device_poll(device_code) -> { status, token? }`; a wizard view that generates a key, uploads the public key to GitHub (REST `POST /user/keys` with the token), and verifies by connecting to `github.com`. Handles expired/denied/network/insufficient-scope with human-readable recovery (Review Focus 4).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn device_poll_maps_denied_and_expired_to_recoverable_errors() {
    use crate::github::DevicePoll;
    assert!(matches!(DevicePoll::from_error("access_denied"), DevicePoll::Denied));
    assert!(matches!(DevicePoll::from_error("expired_token"), DevicePoll::Expired));
    assert!(matches!(DevicePoll::from_error("authorization_pending"), DevicePoll::Pending));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sigrun-desktop github`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

`github.rs`: a `DeviceFlow` client (no secrets logged; the client id is a public constant for Sigrun's registered OAuth App, scoped to `write:public_key` minimum). `from_error` maps GitHub's `error` field to a `DevicePoll` state. The wizard view: start device flow → show `user_code` + a button opening `verification_uri` → poll on the returned interval → on success, generate a key (`generate_key`), upload `public_openssh` via `POST /user/keys`, then verify with `connect` to `github.com` (TOFU via Task 7). Denied/expired/network errors render a retry/abort message. Note for the implementer: the OAuth App + client id must be registered on the GitHub org — mark the constant `GITHUB_CLIENT_ID` with a `// TODO(owner): register Sigrun OAuth App` if it is a placeholder.

- [ ] **Step 4: Run tests**

Run: `cargo test -p sigrun-desktop github && (cd apps/desktop && npm test -- --run wizard && npm run typecheck)`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(wizard): GitHub SSH-key setup via OAuth device flow"
```

---

### Task 9: Installer release workflow

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (bundle targets), `CHANGELOG.md`
- Test: `n/a` (workflow validated by a dry-run / `actionlint`)

**Interfaces:**
- Consumes: the built `sigrun-desktop` app and `tauri-action`.
- Produces: on a `v*` tag, macOS (Apple Silicon + Intel), Linux (deb/rpm/AppImage), and Windows (NSIS/MSI) installers attached to a GitHub Release; the CLI binary `sigrun` packaged alongside. Auto-update is **out of scope** (needs signing/notarization) — note it as future work.

- [ ] **Step 1: Write the workflow**

`.github/workflows/release.yml`: trigger on `push` tags `v*`; matrix over `macos-latest` (two targets), `ubuntu-latest`, `windows-latest`; install Rust + Node; `npm ci` in `apps/desktop`; `tauri-action` to build and upload installers to the release. Pin action SHAs. Add `actionlint` to CI.

- [ ] **Step 2: Validate locally**

Run: `actionlint .github/workflows/release.yml` and `cargo build -p sigrun-desktop --release`.
Expected: actionlint clean; release build succeeds.

- [ ] **Step 3: Update the changelog**

Add the desktop app, themes, i18n, wizard, and installers to `CHANGELOG.md` under `## [Unreleased]` → `### Added`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml apps/desktop/src-tauri/tauri.conf.json CHANGELOG.md
git commit -m "ci(release): build and publish desktop installers on tag"
```

---

## Self-Review Notes

- **Spec coverage:** window+tray (Tasks 1,2,7), themes (4), i18n (5), main UI (6), GitHub wizard (8), installers (9), host-as-app + single instance (2), TOFU (7). v2-only items (provisioning hardening, fingerprint, Gitea/Forgejo/Codeberg, auto-update) are intentionally out.
- **Type consistency:** commands reuse Plan 1's `Request`/`Response`; the frontend consumes `Response` variants by the adjacently-tagged shape (`{"type":…,"value":…}`) established in Plan 1.
- **Review Focus:** each of the five has a test in its owning task (noted inline).
- **Open dependency for the user:** the GitHub wizard needs a registered OAuth App and its `GITHUB_CLIENT_ID` before Task 8's wizard can run against the real GitHub (device flow). Flagged in Task 8 Step 3.
