# Connection Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A search field over the connection list that filters by label, user, host, port and group, so a user with many connections can find one without scrolling.

**Architecture:** One pure filter function over the already-loaded connection list, applied before the existing `groupTree` builds the tree — so the tree only ever renders what matched, and a group whose connections all filtered out disappears on its own. No IPC per keystroke: the view already holds every connection in `conns`.

**Tech Stack:** React 19 + TypeScript + Tailwind, vitest + testing-library in `apps/desktop/src`.

**Spec:** `docs/superpowers/specs/2026-10-17-sigrun-design.md` §6.1 (connection manager) and §8 (UI: accessibility required, i18n everywhere).

## Global Constraints

- `~/.ssh` is read-only. Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard test enforces it. `connections.search` already exists in both locales and is currently unused — reuse it rather than adding a duplicate.
- Rust 2021, MSRV 1.85, dependencies only via `[workspace.dependencies]`; **no new dependency** for this feature.
- Frontend tests use the built-in matchers and `within(...)`; `@testing-library/jest-dom` is deliberately absent. Rows are found with `screen.getByText(label).closest("li")`.
- Every change gets a `## [Unreleased]` Keep-a-Changelog entry. One commit per task, conventional message.
- The gate: this feature is frontend-only, so from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`, plus the Rust gate from the repo root (`cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`) if any Rust file changes.

## Review Focus

1. **The connection list is not filtered when the user expects it to be pristine.** A cleared field, or a field containing only spaces, must show every connection again — and must not be mistaken for "no matches".
2. **Filtering must not hide a connection that does match.** Matching is case-insensitive on a trimmed substring; a query of `PVE` must find a connection whose group is `pve`, and a query with a trailing space must behave like the same query without it.
3. **Match the fields a user would actually type.** label, user, host, port and group. Not tags — the UI has no tags field, so every tag list is empty and matching them would only ever be a no-op that looks like a bug when it silently cannot match.
4. **A port is a number.** Searching `22` must find a connection whose port is 22, and searching `2` must find it too, without stringifying a port differently from the way it is displayed.
5. **The two empty states stay distinct.** No connections at all keeps the existing empty message; a query that matches nothing gets its own message, or a user with connections will think their connections vanished.

---

### Task 1: Filter the connection list from a search field

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx`
- Test: `apps/desktop/src/views/Connections.test.tsx`
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing from earlier tasks (first task of the plan). `groupTree(connections: Connection[])` already exists in this file and keeps its signature.
- Produces: `export function filterConnections(connections: Connection[], query: string): Connection[]` in `apps/desktop/src/views/Connections.tsx`, and the i18n key `connections.noMatches` in both locales (`"No connections match."` / `"Ingen forbindelser matcher."`).

- [ ] **Step 1: Write the failing test for the filter**

Add to `apps/desktop/src/views/Connections.test.tsx`, using the file's existing `conn` fixture shape:

```tsx
it("filters connections by label, user, host, port and group, case-insensitively", () => {
  const rows: api.Connection[] = [
    { ...conn, id: "1", label: "Prod", user: "deploy", host: "web.example.com", port: 22, group: "PVE/Cloud" },
    { ...conn, id: "2", label: "Staging", user: "root", host: "10.0.0.5", port: 2222, group: null },
  ];
  expect(filterConnections(rows, "").map((c) => c.id)).toEqual(["1", "2"]);
  expect(filterConnections(rows, "   ").map((c) => c.id)).toEqual(["1", "2"]);
  expect(filterConnections(rows, "prod").map((c) => c.id)).toEqual(["1"]);
  expect(filterConnections(rows, "ROOT").map((c) => c.id)).toEqual(["2"]);
  expect(filterConnections(rows, "example").map((c) => c.id)).toEqual(["1"]);
  expect(filterConnections(rows, "22").map((c) => c.id)).toEqual(["1", "2"]);
  expect(filterConnections(rows, "pve").map((c) => c.id)).toEqual(["1"]);
  expect(filterConnections(rows, "nothing-here")).toEqual([]);
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd apps/desktop && npx vitest run src/views/Connections.test.tsx`
Expected: FAIL — `filterConnections` is not exported.

- [ ] **Step 3: Implement `filterConnections`**

Export it beside `groupTree`. Trim the query; if it is empty, return the input unchanged (same array, so no needless re-render). Otherwise lowercase it once and keep a connection when any of these lowercased values contains it: `label`, `user`, `host`, `group` (`""` when `null`) and `port.to_string()`. Do not add a matching path that a field cannot use.

- [ ] **Step 4: Add the search field and wire it in**

In the `Connections` component, add `const [query, setQuery] = useState("")` beside the existing state, render a search input in the same row as the "Add connection" button, and build the tree from `groupTree(filterConnections(conns, query))`.

The input: `className={field}`-style consistency with the file's existing inputs, `type="search"`, `value={query}`, `onChange={(e) => setQuery(e.target.value)}`, `placeholder={t("connections.search")}`, and an `aria-label={t("connections.search")}` so it is reachable by name where a placeholder is not. Leave the Refresh/Add controls and their order intact.

- [ ] **Step 5: Add the no-matches state**

Where the tree currently renders, show the new line instead when `query.trim() !== ""` and the filtered list is empty:

```tsx
{conns.length > 0 && query.trim() !== "" && filtered.length === 0 && (
  <p className="text-sm text-muted">{t("connections.noMatches")}</p>
)}
```

The existing empty state for `conns.length === 0` stays exactly as it is (same condition, same string). Add `noMatches` to **both** `en.json` and `da.json` under `connections`, with the values named above.

- [ ] **Step 6: Write and run the component test**

Add a test that renders the view with two connections, types `prod` into the search field (`screen.getByLabelText("Search connections")`), and asserts the `Prod` row is present and the `Staging` row is gone; then clears the field and asserts both are back. Add a second that types `zzz` and asserts the `connections.noMatches` text appears.

Run: `cd apps/desktop && npx vitest run src/views/Connections.test.tsx`
Expected: PASS.

- [ ] **Step 7: Run the gate**

Run from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. Quote the output in the report. If you changed no Rust file, say so.

- [ ] **Step 8: Add the CHANGELOG entry and commit**

Under `## [Unreleased]` → `### Added`: a search field over the connection list matching label, user, host, port and group.

```bash
git add apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json CHANGELOG.md
git commit -m "feat(desktop): search the connection list"
```

---

### Task 2: Document the search field

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1.
- Produces: documentation only.

- [ ] **Step 1: README**

In the connections material, add one sentence: the connection list can be filtered by typing in the search field, which matches the label, user, host, port and group. Keep the README's existing voice; do not restate unrelated rules.

- [ ] **Step 2: Verify the claim against the code**

Name in the report the file and line of each field the sentence claims to match (`filterConnections` in `Connections.tsx`). If the implementation matches a different set of fields, fix the sentence, not the code.

- [ ] **Step 3: Run the gate and commit**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` (docs only).

```bash
git add README.md CHANGELOG.md
git commit -m "docs: describe the connection search"
```

---

## Self-Review

- **Spec coverage:** §6.1 asks for a connection manager that stays usable as the list grows; §8 requires the controls to be reachable by name, which is why the field carries an `aria-label` and not only a placeholder.
- **Step scan:** the field set is pinned in the test, the empty-query behaviour is pinned twice (empty and whitespace-only), and the two empty states are distinguished in the code step and in Review Focus 5. The one thing left to the implementer is the input's exact class string, which the file already has as a constant to reuse.
- **Type consistency:** `filterConnections(connections, query)` is defined once, imported by no other module, and used in this task's own tests and in the view. `groupTree` keeps its signature so the tray and the group chips are unaffected.
- **Review Focus mapping:** 1 → Step 1's `""` and `"   "` cases plus Step 6's clearing case; 2 → Step 1's `"prod"`, `"ROOT"` and `"pve"` cases; 3 → Step 3's field list; 4 → Step 1's `"22"` case; 5 → Step 5's distinct condition.
- **Proportion:** one pure function, one input, one line of copy, one wire-up. No new file, no new dependency, no IPC.
- **Risk flagged:** the `"22"` case deliberately matches both rows in the fixture (port 22 and port 2222), which is the honest behaviour of a substring search over a port; the test asserts that rather than a stricter rule the code does not implement.
