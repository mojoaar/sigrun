# Spec Corrections and Real MSRV Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The three design docs stop contradicting the shipped code, and the declared Rust floor becomes one that can actually build the app.

**Architecture:** No product code changes. One docs commit and one metadata commit.

**Tech Stack:** Markdown, `Cargo.toml`, CI unchanged (`dtolnay/rust-toolchain@stable`).

**Spec:** The docs are the subject; the shipped code is the reference.

## What this plan does not cover

- **CLI export/import** — a missing feature, not a defect; it needs its own plan.
- **Bundle migration code** — there is no v2; v1 plus a fail-closed error *is* the story, so only the policy is written down.
- **The three search test gaps** — the list was lost with the plan workspace; they need to be named before they can be planned.

## Global Constraints

- Docs edits change no claim about behaviour that the code does not already have. If a sentence cannot be made true by editing it, say so rather than deleting the section.
- `Cargo.toml` is the only code file touched. Dependencies stay pinned (`zip ~7.3`, `tauri-plugin-dialog ~2.7`); this plan does not unpin anything.
- Every user-facing string rule, capability rule and `~/.ssh` rule is untouched.
- Gate, from the repo root: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.
- One commit per task, conventional message. No pushes.

## Review Focus

1. **No new false claim.** After Task 1, every sentence the docs assert about the code must be true at `HEAD`. The reviewer greps each edited claim against the source rather than trusting the brief.
2. **The declared MSRV is buildable.** `rust-version = "1.90"` matches the highest `rust_version` any dependency declares (measured: `muda`, `tauri`, `tauri-build`, `tauri-codegen` at 1.90; next highest is 1.89). A floor lower than the dependency graph's maximum is the defect being fixed.
3. **Three places agree.** `Cargo.toml`, `README.md` and `AGENTS.md` name the same number; `rg '1\.85' Cargo.toml README.md AGENTS.md docs/superpowers/specs/` returns nothing. The historical plans under `docs/superpowers/plans/` keep their mentions on purpose, per Step 2.
4. **The IPC spellings are the shipped ones.** `ExportBundle { path, zipped }`, `ImportBundle { path }`, `export_bundle(state, path, zipped)`, `import_bundle(state, path)`, and the helper names `build_bundle` / `read_bundle` / `merge` / `destination_is_allowed`.
5. **The migration policy is a rule, not a promise of work.** No migration code, no new module, no `FORMAT_VERSION` bump.

---

### Task 1: Line the specs up with the shipped code

**Files:**
- Modify: `docs/superpowers/specs/2026-10-19-sigrun-form-validation-design.md`
- Modify: `docs/superpowers/specs/2026-10-19-sigrun-accessibility-design.md`
- Modify: `docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md`

**Interfaces:**
- Consumes: the code as it stands after `2c5f932`.
- Produces: specs that a future plan can trust without a verification pass.

- [ ] **Step 1: Fix the two false sentences in the form-validation spec**

Replace, in `## Why`, the closing clause of the sentence:

```markdown
and the rules cover
only three fields.
```

with:

```markdown
and their rules stop at presence and range: nothing checks the host or user
charset.
```

(The next sentence, `Three fields are genuinely unprotected today:`, stays.)

Replace the port bullet in **Behaviour**:

```markdown
- The port input reads back as an empty string when it holds something that is
  not a number, so "empty" and "not a number" produce the same message.
```

with:

```markdown
- The port input holds the raw string, so an empty value and a non-number both
  produce the same message under the field instead of being coerced silently.
```

- [ ] **Step 2: Fix the five accessibility-spec nits**

In `## Why`, replace the three sentences from `Of the messages that appear` to the end of the paragraph with:

```markdown
Of the result and notice messages, only "No connections match" lacks a live
role: the import/export result, the provisioning result and the vault-locked
notice already carry `role="status"`. (The empty-list and loading strings also
change on their own, but they are list states rather than results, and this
pass leaves them alone.) The nav passes a boolean to `aria-current`, which
React renders as the string `"false"` rather than omitting the attribute. And
the macOS menu labels are English literals while the rest of the app follows
the Language setting.
```

In `### 1. Dialog focus`, replace:

```markdown
Because the shape is identical in all six places, this is one small shared hook
rather than six copies.
```

with:

```markdown
Because the wiring is the same at all six sites, this is one small shared hook
rather than six copies.
```

In **Review focus**, replace item 1 with:

```markdown
1. Every interactive control is Tab-reachable, and the one `:focus-visible`
   rule is visible on both `bg` and `surface`, in light and dark.
```

- [ ] **Step 3: Bring the import/export Interfaces block to the shipped spellings**

The stale material is the whole `## Interfaces` body, not one name. Replace
everything from the line `` `sigrun-core`: `` through the paragraph ending
`a result line.` with:

````markdown
`sigrun-core`:

```rust
pub struct ExportSummary { pub path: String, pub connections: usize, pub keys: usize }
pub struct ImportSummary {
    pub added: usize, pub skipped: usize, pub invalid: usize,
    pub unresolved_keys: usize,
}
impl HostState {
    async fn export_bundle(&self, path: &str, zipped: bool) -> Result<ExportSummary>;
    async fn import_bundle(&self, path: &str) -> Result<ImportSummary>;
}
```

Both summaries derive `Serialize`/`Deserialize` so they ride the IPC wire.
The public helpers carry the decisions a test can pin without a disk:
`build_bundle(&DataFile, now_unix: u64) -> Bundle`, `read_bundle(&Path) ->
Result<Bundle>`, `merge(&mut DataFile, &Bundle) -> ImportSummary`,
`valid_connection(&Connection) -> bool` and `destination_is_allowed(&Path,
&Path) -> bool`.

IPC: `Request::ExportBundle { path: String, zipped: bool }` →
`Response::Export(ExportSummary)`, and `Request::ImportBundle { path: String }` →
`Response::Import(ImportSummary)`. Neither touches the vault, so neither belongs
in the idle-auto-lock `last_vault_activity` list.

Desktop: `export_bundle(state, path, zipped)` and `import_bundle(state, path)`
app commands, thin wrappers over `handle`, registered in the `invoke_handler`
list. Frontend: `exportBundle(path, zipped)` and `importBundle(path)` in
`api.ts`, and a Settings section holding the four buttons, a one-line hint and
a result line.
````

Then item 5 of **left to the builder** becomes one historical line:

```markdown
5. Revised during implementation: this block now carries the shipped
   signatures and summary fields. `bundle.rs` remains the reference.
```

- [ ] **Step 4: Write the format-change rule**

Add a subsection after **Import** (before the next `##`):

```markdown
### Format changes

`export.json` carries one integer, `format`, and `FORMAT_VERSION` is `1`.
`read_bundle` refuses any other value with "this export was written by a newer
Sigrun (format N); update Sigrun to import it", so an older build fails closed
instead of misreading a newer file. When the format changes, bump
`FORMAT_VERSION` and teach `read_bundle` the old shape in the same commit —
never reinterpret an old file silently. A second version does not justify a
migration framework before it exists. `bundle.rs`'s
`an_unknown_format_is_refused_with_a_newer_sigrun_message` test pins the
refusal.
```

- [ ] **Step 5: Verify every edited claim against the code, then commit**

Run each, and fix the doc (not the claim) if it disagrees:

```bash
rg -n 'pub fn (build_bundle|read_bundle|merge|destination_is_allowed)' crates/sigrun-core/src/bundle.rs
rg -n 'fn (export_bundle|import_bundle)' crates/sigrun-core/src/host.rs apps/desktop/src-tauri/src/commands.rs
rg -n 'an_unknown_format_is_refused_with_a_newer_sigrun_message' crates/sigrun-core/src/bundle.rs
rg -n 'export_document|parse_document|ExportDocument|\{ target|\{ source' docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md
```

The last grep must return nothing.

Run the Rust gate from the repo root and quote it (docs-only change, but the rule is unconditional).

```bash
git add docs/superpowers/specs/2026-10-19-sigrun-form-validation-design.md docs/superpowers/specs/2026-10-19-sigrun-accessibility-design.md docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md
git commit -m "docs: line the specs up with what shipped"
```

---

### Task 2: Declare the MSRV the dependency graph actually needs

**Files:**
- Modify: `Cargo.toml`
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `rust-version = "1.90"` in `[workspace.package]`, inherited by all three crates via `rust-version.workspace = true`.

- [ ] **Step 1: Record the measured floor in the commit message, then change the declaration**

The measurement, reproducible with:

```bash
cargo metadata --format-version 1 | python3 -c "import json,sys; m=json.load(sys.stdin); print(max((p['rust_version'] for p in m['packages'] if p.get('rust_version')), key=lambda v:[int(x) for x in v.split('.')]))"
```

must print `1.90` (from `muda`, `tauri`, `tauri-build`, `tauri-codegen`) before you write it down.

- `Cargo.toml`: `rust-version = "1.85"` → `rust-version = "1.90"`.
- `README.md:63`: `- Rust 1.85 or newer (plus Node 22+ to build the desktop app)` → `- Rust 1.90 or newer (plus Node 22+ to build the desktop app)`.
- `AGENTS.md:62`: `- Rust 2021, MSRV 1.85. Dependencies only via `[workspace.dependencies]`.` → the same line with `1.90`.
- `AGENTS.md:121`: the parenthetical `(a caret range pulls releases needing Rust 1.88/1.90)` → `(a caret range can pull a release that needs a newer toolchain)`. The `~` pins stay exactly as they are.
- `docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md:46-49` — the constraint whose premise the new floor removes:

  ```markdown
  - **MSRV 1.85 binds the dependency choice.** `tauri-plugin-dialog` must be
    **2.7.3** (2.8.x needs Rust 1.90) and `zip` must be **7.3.0** (8.x/9.x need
    1.88).
  ```

  becomes:

  ```markdown
  - **The dependency pins guard the MSRV.** The floor is Rust 1.90, so
    `tauri-plugin-dialog` stays **2.7.3** and `zip` stays **7.3.0** (8.x/9.x
    need 1.88); the pins keep a future release from raising the floor again.
  ```

- `docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md:231-234` — item 3 of **left to the builder**:

  ```markdown
  3. **The dependency pins use a `~` range.** `zip` 7.4 needs Rust 1.88 and
     `tauri-plugin-dialog` 2.8 needs 1.90; both are above the project's declared
     1.85, so a caret range would silently raise the floor. Pinned to 7.3.x and
     2.7.x.
  ```

  becomes:

  ```markdown
  3. **The dependency pins use a `~` range.** `zip` 7.4 needs Rust 1.88 and
     `tauri-plugin-dialog` 2.8 needs 1.90, so a caret range can pull a release
     that raises the floor. Pinned to 7.3.x and 2.7.x.
  ```

- [ ] **Step 2: Prove no stale number survives, then gate**

```bash
rg -n '1\.85' Cargo.toml README.md AGENTS.md docs/superpowers/specs/
```

Expected: no matches. The files under `docs/superpowers/plans/` keep their
`1.85` mentions on purpose — they record what was declared when each plan ran,
and rewriting history would make them lie about their own era. Do not touch
them. Then run the full gate from the repo root (Cargo.toml changed, so all three legs matter) and quote it:

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets
```

- [ ] **Step 3: CHANGELOG and commit**

Under `## [Unreleased]` → `### Changed`:

```markdown
- Rust 1.90 is now the minimum supported version, matching what Tauri 2.12 requires.
```

```bash
git add Cargo.toml README.md AGENTS.md docs/superpowers/specs/2026-10-19-sigrun-import-export-design.md CHANGELOG.md
git commit -m "chore: declare Rust 1.90 as the MSRV"
```

---

## Self-Review

- **Spec coverage:** every item the user approved is a step — the two false form sentences, the five accessibility nits, the import/export spellings, the migration rule, and the MSRV trio. The two declined items (CLI export/import, migration code) and the blocked item (search gaps) are named as out of scope instead of being silently absent.
- **Step scan:** Task 1's edits are replacements against exact current text, each verified in the brief's own grep. Task 2 measures the floor before writing it, so the number cannot be guessed. No step depends on another plan's output.
- **Type consistency:** the numbers and identifiers appear once each and cannot disagree — `1.90` in four places, four helper names, two IPC field sets.
- **Review Focus mapping:** 1 → Task 1 Step 5's greps; 2 → Task 2 Step 1's measurement; 3 → Task 2 Step 2's `rg`; 4 → Task 1 Step 3; 5 → the out-of-scope list plus Step 4's wording.
- **Proportion:** two commits, four files in one and three in the other, zero product code, zero dependencies.
- **Risk flagged:** raising `rust-version` changes what contributors can build with, and a future dependency bump could raise the floor again without anyone noticing — the measurement command belongs in the review, not just the commit. Unpinning `zip` or `tauri-plugin-dialog` is deliberately not attempted; the pins still protect against newer releases needing 1.91+.
