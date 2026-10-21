# Release 0.3.0 Implementation Plan

> **For agentic workers:** execute this with the controller, not subagents — it is a release, not a feature. Steps use checkbox syntax. Every command runs from `/Users/mojoaar/Development/sigrun` unless it says otherwise.

**Goal:** Publish `v0.3.0` with the credentials feature and the five new interface languages, and leave the repository, the release page and the local install consistent with it.

**Architecture:** A version bump commit, then a tag whose CI run is the first full three-OS matrix since `2c5f932` (pushes only exercise Linux). The release workflow builds the installers and a trailing `notes` job republishes the tag's changelog section as the release body. The AUR PKGBUILD is bumped afterwards, from the published asset hashes.

**Tech Stack:** Cargo workspace, npm in `apps/desktop`, GitHub Actions (`ci.yml`, `release.yml`), `gh`.

**Spec:** none — this is release mechanics governed by AGENTS.md's Release rules.

## Why 0.3.0

`[Unreleased]` contains new features (credentials, five languages), no breaking change to stored data, and only additive bundle fields. Semver in `0.x` puts features in the minor position: **0.3.0**.

## Preconditions

- `main` clean and equal to `origin/main` (`f493efb` or later).
- Both gates green *at the commit being tagged*.
- The two `#[ignore]`d live-provisioning tests stay unrun; say so on the release page rather than implying they passed.
- No native speaker has reviewed the five translations. That is disclosed in the release notes, not hidden.

## Risks and how this plan handles them

| Risk | Handling |
| --- | --- |
| Tag CI runs macOS and Windows for the first time in ~10 commits (a push only runs Linux). A red leg after tagging means a tag whose CI never goes green — tags are immutable. | Task 2 opens a throwaway PR at the current `HEAD` **before** tagging, watches all three legs, then closes it. This is the only cheap way to run the matrix without a tag. |
| The release body could be empty (the bug `notes` job exists to prevent) — it fails the release instead. | The job's `test -s notes.md` guards it; Task 3 verifies the published body is non-empty. |
| The AUR package pins 0.2.0's URLs and sha256s; those hashes do not exist until the release assets do. | Task 4 bumps it after the release, never before. |
| A CHANGELOG section with no `## [0.3.0]` heading fails `version-sync` on the tag. | Task 1 creates that heading in the same commit as the version bump. |
| The installed app holds the host lock, so a dev build cannot start while it runs. | Task 5 quits the installed app before replacing it. |

---

### Task 1: Prepare the release commit

**Files:**
- Modify: `CHANGELOG.md`, `Cargo.toml`, `apps/desktop/package.json`, `Cargo.lock`, `apps/desktop/package-lock.json`

**Interfaces:**
- Consumes: nothing.
- Produces: a commit `chore: release 0.3.0` whose tree the tag will point at.

- [ ] **Step 1: Consolidate `## [Unreleased]` into `## [0.3.0] - 2026-10-21`**

Three defects to fix while moving the section:

1. **Two `### Changed` headings.** Merge them into one, keeping the order Added → Changed → Fixed and the Keep-a-Changelog wording.
2. **A stale bullet.** "Credentials: named identities that connections can share (model and listing; the UI follows)" is no longer true — the UI shipped in this same release. Fold it into the Credentials tab bullet: "Credentials: named identities a connection can reference instead of repeating a user, key and password."
3. **Add a `### Notes` block** at the end of the 0.3.0 section, so the release page carries it:
   - The five new translations are unreviewed by native speakers; corrections are welcome.
   - Windows keeps `ssh`'s own password prompt (no askpass path) and does not track sessions, so no live dot.
   - A bundle is not a backup: it restores connections only, and never a credential or a secret.

Leave an empty `## [Unreleased]` above it.

- [ ] **Step 2: Bump the two version files**

`Cargo.toml:6` `version = "0.2.0"` → `0.3.0`; `apps/desktop/package.json:4` → `0.3.0`. Nothing else holds the version (the CLI test reads `env!("CARGO_PKG_VERSION")`).

- [ ] **Step 3: Regenerate the lockfiles**

`cargo metadata --format-version 1 > /dev/null` updates the three workspace entries in `Cargo.lock`. For `apps/desktop/package-lock.json`, hand-edit only the two root version fields: `npm install --package-lock-only` rewrites unrelated bundled transitive entries with a newer npm and must not be committed.

- [ ] **Step 4: Update the AUR note's wording only if it names a version**

`packaging/aur/README.md` documents the bump procedure generically; leave `PKGBUILD`/`.SRCINFO` at 0.2.0 until Task 4.

- [ ] **Step 5: Run both gates and quote them**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
cd apps/desktop && npm test -- --run && npm run typecheck && npm run lint && npm run build
```

Expected: exit 0 for both; the version test in `crates/sigrun-cli/tests/cli.rs` now expects 0.3.0 from the crate itself.

- [ ] **Step 6: Commit**

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock apps/desktop/package.json apps/desktop/package-lock.json
git commit -m "chore: release 0.3.0"
git push origin main
```

---

### Task 2: Prove the three-OS matrix before tagging

**Files:** none.

**Interfaces:**
- Consumes: Task 1's commit on `main`.
- Produces: confidence that `v0.3.0`'s tag run will be green, at the cost of one closed PR.

- [ ] **Step 1: Open a throwaway PR at HEAD**

```bash
git switch -c ci/three-os-matrix
git commit --allow-empty -m "ci: run the three-OS matrix"
git push -u origin ci/three-os-matrix
gh pr create --title "CI: three-OS matrix" --body "Runs the full matrix before the 0.3.0 tag. Closed without merging."
gh pr checks --watch
```

- [ ] **Step 2: Read the result**

All three `rust (…)` legs must pass. If one fails, stop and fix it on `main` (a new commit, not this branch) — a tag cannot be repaired after the fact.

- [ ] **Step 3: Close without merging**

```bash
gh pr close --delete-branch
git switch main && git branch -D ci/three-os-matrix
```

---

### Task 3: Tag, publish and verify

**Files:** none.

**Interfaces:**
- Consumes: a green matrix from Task 2 and a clean tree at Task 1's commit.
- Produces: `v0.3.0` with installers and a release body.

- [ ] **Step 1: Tag from the clean tree**

```bash
git status --short   # must print nothing
git fetch origin && git status -sb
git tag -a v0.3.0 -m "Sigrun 0.3.0"
git push origin v0.3.0
```

- [ ] **Step 2: Watch both workflows**

```bash
gh run list --limit 4
gh run watch <run-id>   # the tag's CI run
gh run watch <run-id>   # the release run
```

`CI` runs the matrix plus `version-sync` (which requires the `## [0.3.0]` heading). `Release` builds the installers and the `notes` job rewrites the release page.

- [ ] **Step 3: Verify the release page and assets**

```bash
gh release view v0.3.0 --json name,isPrerelease,body,assets
```

Expected: name `Sigrun 0.3.0`, `isPrerelease: false`, a body that starts at the 0.3.0 heading and includes the `### Notes` block, and the asset list (dmg ×2, AppImage, deb, rpm, msi, nsis setup ×2, CLI binaries ×3).

---

### Task 4: Bump the AUR package to the released assets

**Files:**
- Modify: `packaging/aur/PKGBUILD`, `packaging/aur/.SRCINFO`

**Interfaces:**
- Consumes: `v0.3.0`'s published assets.
- Produces: a PKGBUILD that builds 0.3.0 locally.

- [ ] **Step 1: Take the hashes from the release**

```bash
d=$(mktemp -d)
gh release download v0.3.0 --pattern 'Sigrun_0.3.0_amd64.AppImage' --pattern 'sigrun-x86_64-unknown-linux-gnu' --dir "$d"
shasum -a 256 "$d"/*
```

- [ ] **Step 2: Update `pkgver`, both source URLs and both sha256 sums** to match, then `makepkg --printsrcinfo > .SRCINFO` is *not* available on macOS — hand-edit `.SRCINFO` to the same values and keep the field order.

- [ ] **Step 3: Commit and push**

```bash
git add packaging/aur/PKGBUILD packaging/aur/.SRCINFO
git commit -m "build(aur): bump to 0.3.0"
git push origin main
```

If a new CHANGELOG line is wanted, it belongs under `[Unreleased]`; the package is not published to the AUR, so the release notes need no change.

---

### Task 5: Install 0.3.0 locally

**Files:** none.

- [ ] **Step 1: Build**

```bash
cd apps/desktop && npm run tauri build
```

- [ ] **Step 2: Replace the installed app**

Quit the running Sigrun first (it holds the host lock), then:

```bash
cp -R target/release/bundle/macos/Sigrun.app /Applications/
xattr -dr com.apple.quarantine /Applications/Sigrun.app
defaults read /Applications/Sigrun.app/Contents/Info CFBundleShortVersionString   # expect 0.3.0
open -a Sigrun
```

- [ ] **Step 3: Smoke-check the new surfaces**

Credentials tab lists what the CLI shows; the connection form's source switch works; Settings → Language offers seven entries; picking Deutsch switches the interface and the macOS menu; the About panel shows 0.3.0.

---

## Self-Review

- **Spec coverage:** version in exactly two files, both locks updated, `## [0.3.0]` heading present (all three are what `version-sync` checks); release page body comes from the changelog section this plan writes; AUR bumped only after assets exist; local install matches the tag.
- **Step scan:** Task 1 is the only commit before the tag; Task 2 adds no commit to `main`; Tasks 4 and 5 change nothing the tag depends on. No step requires a summary of a previous task.
- **Proportion:** two version files, one changelog section, one throwaway PR, one tag, one AUR bump. No new tooling, no workflow change, no subagents.
- **Risk flagged:** if Task 2 is skipped, a red Windows leg leaves an immutable tag with failing CI and no way to fix it except a follow-up patch release. If the `notes` job fails, the release still publishes its assets but with no body — Task 3 Step 3 catches it and `gh release edit` repairs it by hand.
