# Sigrun 0.1.0 Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Sigrun v0.1.0 as an unsigned GitHub pre-release whose installers build, whose metadata is right, and whose README tells a stranger how to get from download to a connected shell.

**Architecture:** No new features and no new module. Three implementer tasks change files (version/CHANGELOG, About copy across its three surfaces, the README front door); three release steps are performed by the controller in this session because they act on the repository or the machine rather than on code (`gh repo edit`, the packaging dry run, the tag and the release). The split is deliberate: a subagent must never push, tag or publish.

**Tech Stack:** unchanged (Rust 2021 workspace, Tauri 2.12.4 desktop app, React 19 + TypeScript + Tailwind v4, GitHub Actions release workflow, `gh` CLI).

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` plus the release and git rules in `AGENTS.md` — this plan implements those rules, so it argues from them rather than restating them.

## Global Constraints

- The version lives in exactly two files: `Cargo.toml` (workspace `[workspace.package]` version) and `apps/desktop/package.json`. CI's `version-sync` job keeps them in lockstep and, on a `v*` tag, also requires a `## [<tag>]` heading in `CHANGELOG.md`. `Cargo.lock` records the version too but is regenerated, never hand-edited.
- Keep-a-Changelog: every change has an entry under `## [Unreleased]`; the release renames that heading and opens a fresh empty `Unreleased`.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; the recursive completeness guard test enforces parity.
- Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model — the release adds no code, so nothing here may introduce one.
- The gate, quoted not claimed: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`; whenever anything under `apps/desktop/src` changes, also from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
- One commit per task, conventional message. **Never tag from a dirty tree.**
- The copyright holder is **mojoaar** (the partner's decision), matching `LICENSE` after Task 2 fixes it.

## Review Focus

1. The three version strings (`Cargo.toml`, `apps/desktop/package.json`, `Cargo.lock`) and the `## [0.1.0]` heading must agree — a mismatch fails CI only at tag time, i.e. after the release looks done.
2. The macOS About panel renders only name/version/copyright/credits, so the website only ever appears on the copyright line; copy changed in one surface but not the others is the most likely drift.
3. The tag fires the three-OS installer matrix for the first time in this repository's history — the dry run must happen first, and its output must be recorded, not assumed.
4. A stranger's first five minutes: clone-free download, the "unidentified developer" dialog on macOS, and SmartScreen on Windows must each have a one-line answer in the README.
5. The GitHub metadata step mutates the live repository; it must be re-readable afterwards (`gh repo view`) so a silent failure is visible.

---

### Task 1: Version and changelog lockstep

**Files:**
- Modify: `CHANGELOG.md` (rename the `## [Unreleased]` heading, open a fresh empty one)
- Modify: `Cargo.toml` (workspace version → `0.1.0`)
- Modify: `apps/desktop/package.json` (`version` → `0.1.0`)
- Regenerate: `Cargo.lock` (by running cargo, never by editing)

**Interfaces:**
- Consumes: nothing.
- Produces: `0.1.0` in both version files, a `## [0.1.0] - 2026-10-19` heading holding every existing `Unreleased` entry, and a fresh empty `## [Unreleased]` above it. Tasks 5 and 6 depend on this exact heading text.

- [ ] **Step 1: Read the current top of the changelog and the two version files**

Run: `sed -n '1,40p' CHANGELOG.md; grep -n '^version' Cargo.toml; grep -n '"version"' apps/desktop/package.json`
Expected: the `## [Unreleased]` heading with its `### Added`/`### Changed`/`### Fixed` sections, and any current version strings.

- [ ] **Step 2: Rename the heading and open a fresh section**

`## [Unreleased]` becomes:

```markdown
## [Unreleased]

## [0.1.0] - 2026-10-19
```

with every existing entry left inside `0.1.0`, in its existing order. The new `Unreleased` is empty (no subsections).

- [ ] **Step 3: Set both versions to `0.1.0`**

`Cargo.toml`'s workspace version and `apps/desktop/package.json`'s `version`. Then let cargo rewrite the lock file rather than editing it: `cargo metadata --format-version 1 > /dev/null`.

- [ ] **Step 4: Verify the triad agrees**

```bash
grep -n '^version' Cargo.toml
grep -n '"version"' apps/desktop/package.json
grep -n '^version' Cargo.lock | head -3
grep -n '^## \[0.1.0\]' CHANGELOG.md
```

Expected: `0.1.0` in all three version files and exactly one `## [0.1.0]` heading. This is the same comparison CI's `version-sync` job makes.

- [ ] **Step 5: Run the gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`
Expected: exit 0, no failures.

- [ ] **Step 6: Commit**

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock apps/desktop/package.json
git commit -m "chore: release 0.1.0"
```

### Task 2: The About copy, on all three surfaces

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (`bundle.shortDescription`, `bundle.longDescription`)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (`about_metadata`'s copyright string and its test)
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json` (`help.about.*`)
- Modify: `LICENSE` (holder → `mojoaar`)
- Test: the existing `about_metadata` unit test in `lib.rs`; `apps/desktop/src/views/Help.test.tsx`

**Interfaces:**
- Consumes: the approved copy, verbatim: short — `SSH connections and keys in a local encrypted vault, served to ssh through a built-in agent.`
- Produces: that sentence as `bundle.shortDescription` and as the in-app `help.about.tagline`; a longer paragraph as `bundle.longDescription` (three sentences: what it stores, how `ssh` gets the key, that nothing leaves the machine); the copyright string `Copyright © 2026 mojoaar · https://johansen.foo` unchanged in shape (the website rides this line because it is the only macOS-rendered field that can carry it).

- [ ] **Step 1: Write the failing assertions first**

In `lib.rs`'s `about_metadata` test, assert the short description reached the metadata (or, if the struct exposes no such field, assert the copyright line is unchanged and add a test that reads `tauri.conf.json` and asserts `bundle.shortDescription` equals the approved sentence verbatim). In `Help.test.tsx`, assert the rendered About section contains the tagline.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sigrun-desktop --lib about` and, from `apps/desktop`, `npm test -- --run Help`
Expected: FAIL on the new assertions.

- [ ] **Step 3: Apply the copy**

Edit `tauri.conf.json`'s two description fields, the `help.about` strings in **both** locales (the tagline key plus any wording the paragraph replaces), and `LICENSE`'s holder line. Keep the Danish text natural, not a word-for-word transplant.

- [ ] **Step 4: Run the gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`, then from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`.
Expected: exit 0 everywhere, including the i18n parity guard.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/src/lib.rs apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json LICENSE apps/desktop/src/views/Help.test.tsx
git commit -m "docs: sharpen the About copy and the licence holder"
```

### Task 3: The README front door

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: the shipped behaviour only — no claim may go in that a test does not already back.
- Produces: sections in this order: one-paragraph *what it is* (the tagline plus who it is for), *Install* (the published installer links, which do not exist until Task 6 — link the releases page, `https://github.com/mojoaar/sigrun/releases/latest`, and say 0.1.0 is an unsigned pre-release with one line each for macOS's "unidentified developer" dialog and Windows' SmartScreen), *The first five minutes* (create the vault, generate or import a key, add a connection, connect), *Requirements*, and the existing honest limitations.

- [ ] **Step 1: Write the install and first-five-minutes sections**

Keep the existing feature list and limitations. Every UI step must name the control as it is labelled in the app (Create vault, Generate key, Add connection, Connect). Do not invent a download URL for an artifact name that does not exist yet; point at the releases page.

- [ ] **Step 2: Verify every claim against the shipped code**

For each sentence that describes behaviour, name the file it comes from in the commit message body; for each UI label, grep the locale file to confirm the label text.

Run: `grep -n '"install"\|"create"\|"generate"\|"connect"' apps/desktop/src/i18n/en.json | head -20`
Expected: the labels the README names.

- [ ] **Step 3: Run the gate**

Run: `cargo fmt --all -- --check && cargo test --all-targets` (docs-only change, so the full clippy leg is optional but cheap).
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: make the README a front door"
```

### Task 4: GitHub repository metadata (controller-performed)

**Files:** none — this mutates repository settings, not the tree.

- [ ] **Step 1: Confirm the account and the current state**

Run: `gh auth status && gh repo view mojoaar/sigrun --json description,homepageUrl,repositoryTopics`
Expected: authenticated as `mojoaar` and the current (probably empty) metadata.

- [ ] **Step 2: Set description, website and topics**

```bash
gh repo edit mojoaar/sigrun \
  --description "SSH connections and keys in a local encrypted vault, served to ssh through a built-in agent." \
  --homepage "https://johansen.foo" \
  --add-topic ssh --add-topic ssh-agent --add-topic ssh-client --add-topic key-management \
  --add-topic vault --add-topic tauri --add-topic rust --add-topic macos --add-topic linux \
  --add-topic windows --add-topic argon2 --add-topic xchacha20 --add-topic desktop-app --add-topic security-tools
```

- [ ] **Step 3: Re-read to prove it applied**

Run: `gh repo view mojoaar/sigrun --json description,homepageUrl,repositoryTopics`
Expected: the description, the URL and the topics, exactly as set. A silent failure here is the whole reason this step exists.

### Task 5: Packaging dry run (controller-performed)

**Files:** none — this proves the bundle, it does not change it. Fix-forward in a new commit only if it fails.

- [ ] **Step 1: Build the app the way the release workflow will**

Run, from `apps/desktop`: `npm run tauri build 2>&1 | tail -30`
Expected: a `.app` and a `.dmg` under `src-tauri/target/release/bundle/`, exit 0. **This is the first time the packaging path has run in this repository** — the release workflow has never been triggered, so a failure here is the cheapest possible time to find it.

- [ ] **Step 2: Inspect what was produced**

Run: `ls -la apps/desktop/src-tauri/target/release/bundle/dmg apps/desktop/src-tauri/target/release/bundle/macos`
Expected: a `.dmg` whose name carries `0.1.0`, and an `.app` carrying the icon and the bundle metadata set in Task 2.

- [ ] **Step 3: Record the dry run**

Paste the tail of the build output and the `ls` result into the plan's report file (`.superpowers/sdd/2026-10-19-sigrun-release/dry-run.md`), including the artifact sizes. Do not commit build output.

### Task 6: Tag and pre-release (controller-performed)

**Files:** none — this publishes.

- [ ] **Step 1: Confirm the preconditions**

Run: `git status --short && git log --oneline -1 && grep -n '^## \[0.1.0\]' CHANGELOG.md && git ls-remote --tags origin | tail -3`
Expected: a **clean** tree, the version commit at HEAD, exactly one `0.1.0` heading, and no existing `v0.1.0` tag. Do not proceed on a dirty tree.

- [ ] **Step 2: Push main, then tag and push the tag**

```bash
git push origin main
git tag -a v0.1.0 -m "Sigrun v0.1.0"
git push origin v0.1.0
```

- [ ] **Step 3: Watch the release workflow**

Run: `gh run list --workflow release.yml --limit 3`
Expected: a run triggered by the tag, covering the three-OS matrix. Report its conclusion; if it fails, the failure is the next task, not a reason to re-tag.

- [ ] **Step 4: Create the pre-release with notes from the changelog**

```bash
gh release create v0.1.0 --prerelease --title "Sigrun 0.1.0" --notes-file /tmp/sigrun-0.1.0-notes.md
```

where the notes file holds the `## [0.1.0]` entries verbatim plus a short paragraph naming the known limits: unsigned (macOS "unidentified developer", Windows SmartScreen), Windows has no password prompt and no live connected dot, a bundle is not a backup, and the two live provisioning tests were not run against this build.

- [ ] **Step 5: Prove the release is visible**

Run: `gh release view v0.1.0 --json isPrerelease,name,tagName,assets`
Expected: `isPrerelease: true` and, once the workflow finishes, the installer assets attached.

## Self-Review

- **Spec coverage:** version lockstep (Task 1), About copy on all three surfaces plus the licence holder (Task 2), the README front door including the unsigned guidance (Task 3), GitHub metadata (Task 4), the packaging proof the release workflow has never had (Task 5), and the tag/pre-release (Task 6). The AGENTS.md release rules are the spec here; nothing in them is left unimplemented.
- **Deliberate split:** Tasks 4, 5 and 6 are controller-performed because they mutate repository settings, the local machine or the published tag — a subagent must never push, tag or publish. Tasks 1–3 are ordinary implementer tasks with the usual per-task review.
- **Known gap, stated rather than hidden:** the two `#[ignore]`d live provisioning tests need a throwaway host with `SIGRUN_LIVE_HOST`/`USER`/`PASSWORD`/`KEY`; if the partner has not run them by Task 6, the release notes say so instead of implying they passed. The stored-password GUI checks likewise remain the partner's.
- **Review Focus mapping:** 1 → Task 1 Step 4; 2 → Task 2 Steps 3–4 (both locales and the panel's field list); 3 → Task 5 Steps 1–3; 4 → Task 3 Step 1; 5 → Task 4 Step 3.
