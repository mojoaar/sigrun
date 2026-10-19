# Connection Form: Group Suggestions and Duplicate-Label Warning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Add/Edit connection form suggests the group paths already in use while the user types one, and warns when the label duplicates another connection's.

**Architecture:** The Connections view already holds every connection in `conns`, so the form receives the derived suggestion list as a prop and no new IPC is needed. A pure `groupPaths(conns)` expands each stored group into all of its `/`-prefixes, so a partially typed path can match; the input is a plain text input with a `<datalist>`. The duplicate check is inline and non-blocking, because `connections_connect` resolves an unknown id by the **first** connection whose label matches — a duplicate label silently sends `sigrun connections connect <label>` to the wrong host.

**Tech Stack:** React + TypeScript + Tailwind, i18next, vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` (§8: accessibility and keyboard use are required; every user-facing string lives in the i18n files; §6.1: the connection manager).

## Global Constraints

- `~/.ssh` is read-only; the sole exception is an explicit provision appending to a remote `authorized_keys`.
- Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`.
- Gate: from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build` (this plan touches only `apps/desktop/src`); the Rust gate is `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.
- One commit per task, conventional message, plus a `## [Unreleased]` CHANGELOG entry.
- No new dependency.

## Review Focus

1. **The edited connection must not warn about itself** — the check excludes the connection being edited by id.
2. **Malformed group values.** `/PVE/`, `A//B`, `  ` and `null` must not produce empty or duplicated suggestions; the suggestions are a display artifact of user data, so they must never crash the form.
3. **`<datalist>` is not rendered everywhere.** WKWebView and WebView2 support it; WebKitGTK's support is unreliable. The group field must stay a working plain text input where nothing appears.
4. **Label matching is exact.** The CLI's fallback compares labels for equality, so the warning must be exact too — no case folding, no trimming beyond what the field itself stores.
5. **The list follows the data.** After a connection is added or removed and the view refreshes, the suggestions must reflect the new set (the props come from `conns`, so this follows automatically — the test pins it).

---

### Task 1: Suggest group paths and warn on a duplicate label

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx` (exported `groupPaths`, the `Form` props and two fields)
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Modify: `CHANGELOG.md`
- Test: `apps/desktop/src/views/Connections.test.tsx`

**Interfaces:**
- Produces: `export function groupPaths(connections: Connection[]): string[]` — every distinct `/`-prefix of every non-empty `group`, deduplicated and sorted, with empty segments dropped (`/PVE/` → `PVE`).
- Produces: `Form` gains props `groups: string[]` and `labels: string[]` (the existing labels, excluding the connection being edited).
- Consumes: the existing `conns` state in `Connections` for both props; the existing `field` class constant for the inputs.

- [ ] **Step 1: Write the failing tests**

In `views/Connections.test.tsx`:

```tsx
it("suggests every prefix of the group paths already in use", () => {
  expect(groupPaths([
    { ...conn, id: "a", group: "PVE/Folder 1" },
    { ...conn, id: "b", group: "PVE" },
    { ...conn, id: "c", group: "/Cloud//EU/" },
    { ...conn, id: "d", group: null },
    { ...conn, id: "e", group: "   " },
  ])).toEqual(["Cloud", "Cloud/EU", "PVE", "PVE/Folder 1"]);
});

it("offers the known group paths on the group field", async () => {
  api.listConnections.mockResolvedValueOnce([{ ...conn, id: "a", group: "PVE/Folder 1" }]);
  render(<Connections onVaultLocked={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
  const group = screen.getByLabelText("Group") as HTMLInputElement;
  expect(group.getAttribute("list")).toBe("sigrun-groups");
  const options = Array.from(document.querySelectorAll("#sigrun-groups option")).map((o) => o.getAttribute("value"));
  expect(options).toEqual(["PVE", "PVE/Folder 1"]);
});

it("warns when the label duplicates another connection, but not about itself", async () => {
  api.listConnections.mockResolvedValueOnce([{ ...conn, id: "a", label: "prod" }]);
  render(<Connections onVaultLocked={() => {}} />);
  fireEvent.click(await screen.findByRole("button", { name: "Add connection" }));
  const label = screen.getByLabelText("Label") as HTMLInputElement;
  fireEvent.change(label, { target: { value: "prod" } });
  expect(screen.getByText("Another connection already uses this label.")).toBeTruthy();
  fireEvent.change(label, { target: { value: "prod-2" } });
  expect(screen.queryByText("Another connection already uses this label.")).toBeNull();
});
```

Use the file's existing fixtures and helpers; `conn` already carries every field.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm test -- --run src/views/Connections.test.tsx`
Expected: FAIL — `groupPaths` is not exported; the group input has no `list`; no warning text.

- [ ] **Step 3: Implement `groupPaths`**

Export it beside `groupTree`. Split each group on `/`, drop empty segments (which also handles a leading, trailing or doubled separator), accumulate the prefixes in order for that group, then deduplicate and sort the union. A `None` or whitespace-only group contributes nothing. One comment line on why prefixes are expanded (a half-typed path must match).

- [ ] **Step 4: Wire the two helpers into the form**

- `Connections` passes `groups={groupPaths(conns)}` and `labels={conns.filter((c) => c.id !== editingId).map((c) => c.label)}` (derive the editing id from the same state the form already receives).
- The group input gains `list="sigrun-groups"`; render one `<datalist id="sigrun-groups">` with a `<option value={path} key={path} />` per path. Keep the input a plain text input.
- When the trimmed label is a non-empty member of `labels`, render one inline warning next to the label field: `<p className="text-xs text-danger">{t("connections.duplicateLabel")}</p>`. It must not block saving.
- i18n: `en.json` `connections.duplicateLabel` = `"Another connection already uses this label."`; `da.json` = `"En anden forbindelse bruger allerede denne etiket."`

- [ ] **Step 5: Run the tests and the gate**

Run: from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`
Expected: the three new tests and the whole suite pass; the i18n completeness guard passes because both locales gained the key.
Run: `cargo test --all-targets` (unchanged Rust, but the gate is the gate).

- [ ] **Step 6: CHANGELOG**

Under `## [Unreleased]` → `### Added`, one bullet: the connection form suggests the group paths already in use and warns when a label duplicates another connection.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json CHANGELOG.md
git commit -m "feat(desktop): suggest group paths and warn on duplicate labels"
```

---

## Self-Review

- **Spec coverage:** §8's i18n rule is served by the one new key in both locales and §8's accessibility by leaving the field a native input with a native suggestion list; no host behaviour changes.
- **Step scan:** the pure function's exact output is pinned by an assertion that also covers the malformed inputs of Review Focus 2; the UI steps name the test ids and the exact strings.
- **Type consistency:** `groupPaths(Connection[])` is defined and consumed in the same task; `groups`/`labels` are named identically in the props and the call site.
- **Review Focus mapping:** 1 → the third test's second half plus the id-filter in Step 4; 2 → the first test's `/Cloud//EU/`, `null` and whitespace fixtures; 3 → the plain text input (no custom widget that would break where `datalist` is absent); 4 → exact membership against the stored labels; 5 → the props derive from `conns`, which `refresh()` reloads (the second test's `mockResolvedValueOnce` proves the prop path).
- **Proportion:** one task, one pure function, two form fields, one key. No new IPC, no new module.
- **Known gap:** `datalist` popup behaviour itself is not asserted (jsdom renders options, not the popup), so Review Focus 3's WebKitGTK caveat is untested by construction — the field degrades to a plain input there.
