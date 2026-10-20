# Native Form Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Add/Edit connection form refuses to save a connection the launcher cannot use, says which field is wrong under that field, and accepts a connection with no user.

**Architecture:** One pure `validateConnection(draft)` over a string-typed draft, called at the top of `submit()`. The form stops relying on the browser's constraint validation (which blocks with an untranslated tooltip and is invisible to jsdom) and owns the rule; `terminal::ssh_invocation` stays the authority at the injection boundary and is unchanged.

**Tech Stack:** React 19 + TypeScript + Tailwind, vitest + testing-library in `apps/desktop/src`. No Rust change.

**Spec:** `docs/superpowers/specs/2026-10-19-sigrun-form-validation-design.md`, corrected by Task 1.

## Rulings (settled 2026-10-20 — do not relitigate)

1. **The spec's Why is wrong and Task 1 rewrites it.** Native `required` / `min` / `max` *do* block the submit event: constraint validation runs before it, so `preventDefault` never gets the chance. Verified in Chromium — a `required number min=1 max=65535` field with a `preventDefault` listener accepts neither `0` (`rangeUnderflow`) nor `99999` (`rangeOverflow`) and fires no submit. Port 0/99999 and an empty label/host are already refused today, by the browser, with a native tooltip. jsdom does not implement constraint validation, which is why the suite cannot see the block.
2. **The three real holes, and therefore the justification:** the host charset is unchecked (a space or `;` is accepted and only fails at connect); the user charset is unchecked although `ssh_invocation` applies the same predicate to it (`terminal.rs:54`); and `user` is `required` (`Connections.tsx:134`) although the launcher allows empty, so a **managed connection imported with an empty user cannot be saved from Edit** — native validation blocks it. (`bundle.rs:191` accepts an empty user and `merge` keeps `source: Managed`, so the row has an Edit button.)
3. **Drop `required`, `min` and `max`.** One message style, and jsdom and the browser agree about what the form accepts.
4. **No `pub fn valid_host` in `terminal.rs`.** The form is TypeScript and cannot call Rust across IPC, so the predicate is duplicated either way; an exported function with no caller is dead weight. The Rust side keeps its existing behaviour and its injection test (`shell_metacharacters_are_rejected_not_injected`, `terminal.rs:679`). The TS predicate carries a comment naming `terminal.rs:39` as the source, and its own table test.
5. **The duplicate-label warning stays** non-blocking and unchanged.
6. **The port input becomes `type="text"` with `inputMode="numeric"`,** because the draft holds a string and "not a number" must be reachable and testable. `type="number"` would render an unparseable state as empty and hide the branch.

## Global Constraints

- `~/.ssh` is read-only. Secrets never appear in argv, an environment *value*, a log line, an error message or the persisted model.
- Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard enforces it.
- No new dependency.
- Frontend tests use the built-in matchers and `within(...)`; `@testing-library/jest-dom` is deliberately absent. Focus assertions go through `document.activeElement`.
- Every change gets a `## [Unreleased]` Keep-a-Changelog entry. One commit per task, conventional message.
- Gate, from `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. From the repo root, because Task 1 touches no code but the rule is unconditional: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`.

## Review Focus

1. **A port of 0, 99999, an empty string or `abc` is refused** with a message under the field, and `addConnection`/`updateConnection` is never called for it.
2. **A host containing a space, `/` or a shell metacharacter is refused** before it can reach `ssh` — and accepted only if `ssh_invocation` would accept it.
3. **A connection with an empty user saves.** Opening the imported empty-user connection in Edit and pressing Save calls `updateConnection`; it is not blocked and the saved user is `""`.
4. **The form is never stricter than the launcher.** The accepted set is exactly non-empty `[A-Za-z0-9._-]` for host and for a non-empty user. No length limits, no DNS lookups, no reserved-name checks.
5. **The store receives a number for the port** — never `NaN`, never a string.
6. **Trim is real.** A label or host saved with leading/trailing spaces must store the trimmed value, because that is what validation checked and what the CLI resolves.

---

### Task 1: Correct the design doc

**Files:**
- Modify: `docs/superpowers/specs/2026-10-19-sigrun-form-validation-design.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the corrected spec that Task 2 implements.

- [ ] **Step 1: Replace the `## Why` section body**

Keep the heading. Replace everything under it with:

```markdown
The Add/Edit connection form is the only place a connection is created, and it
currently delegates its rules to the browser. Those rules are real — constraint
validation runs before the submit event, so `required`/`min`/`max` do block a
`preventDefault` handler — but they are the wrong shape for this app: the
tooltip is the platform's language and wording, jsdom does not implement
constraint validation so the test suite cannot see them, and the rules cover
only three fields.

Three fields are genuinely unprotected today:

- **host** is accepted with a space, a slash or a shell metacharacter. The
  launcher refuses it at connect time (`terminal::ssh_invocation`), so a typo
  becomes a failed connection instead of a message under the field.
- **user** gets the same predicate as host in the launcher — non-empty and only
  `[A-Za-z0-9._-]` — and the form does not apply it.
- **user** is `required` in the form although the launcher allows empty (ssh
  then uses the local user). An imported connection with an empty user — the
  bundle format explicitly accepts one — therefore cannot be saved from Edit:
  the browser blocks the submit and there is nothing to type except a user the
  connection does not have.

The form should own one rule set, in one place, with one message style.
```

- [ ] **Step 2: Replace the rules table**

```markdown
| Field | Rule | Source |
| --- | --- | --- |
| label | non-empty after trim | `connections.label` is how the CLI resolves a connection |
| host | non-empty after trim **and** only `[A-Za-z0-9._-]` | `terminal::safe_host_bytes` (`terminal.rs:39`), applied by `ssh_invocation` |
| user | empty allowed; otherwise only `[A-Za-z0-9._-]` | `ssh_invocation` applies the same predicate to a non-empty user (`terminal.rs:54`) |
| port | parses as an integer in `1..=65535` | `ssh -p` takes exactly that |

A connection the form accepts must be one the launcher would accept; that is
the whole contract. The predicate is written twice — once in Rust for the
launcher, once in TypeScript for the form — because the form cannot call core
across the IPC boundary. Both are pinned by a table test, and the TypeScript
one names `terminal.rs` in a comment.
```

- [ ] **Step 3: Fix the Behaviour and Interfaces sections**

In **Behaviour**, keep the four bullets, and add:

```markdown
- The browser's `required`, `min` and `max` attributes come off the three
  fields. One rule set, one message style, and jsdom sees the same behaviour
  the browser does.
- The port input is `type="text"` with `inputMode="numeric"`, because the draft
  value is a string until it is validated and then parsed.
```

Replace the **Interfaces** section with:

```markdown
- `apps/desktop/src/views/Connections.tsx` gains
  `export type ConnectionDraft = Omit<Connection, "port"> & { port: string }`,
  `export type FieldErrors = Partial<Record<"label" | "host" | "user" | "port", string>>`
  (values are i18n keys, so the function stays pure) and
  `export function validateConnection(draft: ConnectionDraft): FieldErrors`
  — a pure function, so the table above is pinned by a unit test with no DOM.
- No Rust change: the launcher's predicate already exists and stays
  `pub(crate)`; the form cannot reach it through IPC, so exposing it would add
  a public function with no caller.
- New i18n keys in both locales: `connections.invalidHost`,
  `connections.invalidUser`, `connections.invalidPort`,
  `connections.invalidLabel`.
```

In **Review focus**, replace item 3 with:

```markdown
3. The Edit form must not reject a connection that is already stored and
   valid — including an imported one whose user is empty.
```

and item 4 with:

```markdown
4. Nothing the launcher accepts may become unsavable: the form's rule must not
   be stricter than `ssh_invocation`'s, and `shell_metacharacters_are_rejected_not_injected`
   in `terminal.rs` is the Rust half of the same contract.
```

- [ ] **Step 4: Run the gate and commit**

Run from the repo root: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets`. Quote the result.

```bash
git add docs/superpowers/specs/2026-10-19-sigrun-form-validation-design.md
git commit -m "docs: correct the form-validation design"
```

---

### Task 2: Validate the draft before saving

**Files:**
- Modify: `apps/desktop/src/views/Connections.tsx`
- Modify: `apps/desktop/src/views/Connections.test.tsx`
- Modify: `apps/desktop/src/i18n/en.json`, `apps/desktop/src/i18n/da.json`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the corrected spec from Task 1.
- Produces, exported from `Connections.tsx`:
  - `export type ConnectionDraft = Omit<Connection, "port"> & { port: string }`
  - `export type FieldErrors = Partial<Record<"label" | "host" | "user" | "port", string>>`
  - `export function validateConnection(draft: ConnectionDraft): FieldErrors`
- `Form`'s props and `Connection` are unchanged; `onProvision` still receives the stored `Connection`.

- [ ] **Step 1: Write the failing test for the rules**

Add to `apps/desktop/src/views/Connections.test.tsx`, beside the `groupTree` / `groupPaths` tests. Extend the existing import from `./Connections` to `Connections, { filterConnections, groupPaths, groupTree, validateConnection, type ConnectionDraft }`.

```tsx
const draft = (over: Partial<ConnectionDraft> = {}): ConnectionDraft => ({ ...conn, port: "22", ...over });

it("validates label, host, user and port the way the launcher does", () => {
  expect(validateConnection(draft())).toEqual({});
  // label
  expect(validateConnection(draft({ label: "" })).label).toBe("connections.invalidLabel");
  expect(validateConnection(draft({ label: "   " })).label).toBe("connections.invalidLabel");
  // host: the charset ssh_invocation enforces (terminal.rs:39)
  for (const host of ["", "   ", "web example.com", "web/example", "example.com; rm -rf /", "exa$mple"]) {
    expect(validateConnection(draft({ host })).host).toBe("connections.invalidHost");
  }
  for (const host of ["web", "web.example.com", "10.0.0.5", "host_1", "a-b"]) {
    expect(validateConnection(draft({ host })).host).toBeUndefined();
  }
  // user: whitespace-only is not a user the launcher refuses, it trims to empty
  expect(validateConnection(draft({ user: "" })).user).toBeUndefined();
  expect(validateConnection(draft({ user: "   " })).user).toBeUndefined();
  expect(validateConnection(draft({ user: "deploy" })).user).toBeUndefined();
  for (const user of ["de ploy", "deploy$(id)", "deploy/x"]) {
    expect(validateConnection(draft({ user })).user).toBe("connections.invalidUser");
  }
  // port: integers only, 1..=65535, both ends inclusive
  for (const port of ["", " ", "abc", "0", "-1", "1.5", "1e5", "65536"]) {
    expect(validateConnection(draft({ port })).port).toBe("connections.invalidPort");
  }
  for (const port of ["1", "22", " 22 ", "65535"]) {
    expect(validateConnection(draft({ port })).port).toBeUndefined();
  }
});

it("reports every offending field at once", () => {
  expect(validateConnection(draft({ label: "", host: "a b", user: "a;b", port: "0" }))).toEqual({
    label: "connections.invalidLabel",
    host: "connections.invalidHost",
    user: "connections.invalidUser",
    port: "connections.invalidPort",
  });
});
```

Note `"  "` for user: it trims to empty and must be accepted (it is not a user the launcher would refuse), while `"de ploy"` is refused. If the implementer finds any case above that contradicts `ssh_invocation`, fix the test, not the rule — the launcher wins.

- [ ] **Step 2: Run it to verify it fails**

Run: `cd apps/desktop && npx vitest run src/views/Connections.test.tsx`
Expected: FAIL — `validateConnection` is not exported.

- [ ] **Step 3: Implement the types and the predicate**

At module scope in `Connections.tsx`, beside `groupTree`:

```ts
export type ConnectionDraft = Omit<Connection, "port"> & { port: string };
export type FieldErrors = Partial<Record<"label" | "host" | "user" | "port", string>>;

// The launcher's set, not a stricter one: terminal::safe_host_bytes
// (crates/sigrun-core/src/terminal.rs:39) accepts exactly this and applies it
// to host and to a non-empty user alike.
const SSH_TOKEN = /^[A-Za-z0-9._-]+$/;
const PORT = /^\d+$/;

export function validateConnection(draft: ConnectionDraft): FieldErrors {
  const errors: FieldErrors = {};
  if (draft.label.trim() === "") errors.label = "connections.invalidLabel";
  if (!SSH_TOKEN.test(draft.host.trim())) errors.host = "connections.invalidHost";
  if (draft.user.trim() !== "" && !SSH_TOKEN.test(draft.user.trim())) errors.user = "connections.invalidUser";
  const port = draft.port.trim();
  if (!PORT.test(port) || Number(port) < 1 || Number(port) > 65535) errors.port = "connections.invalidPort";
  return errors;
}
```

Values are i18n keys: the function stays free of `t`, and the component renders `t(key)`.

- [ ] **Step 4: Switch the form state to a draft**

In `Form`:

- `const [c, setC] = useState<Connection>` becomes
  `const [draft, setDraft] = useState<ConnectionDraft>(() => initial ? { ...initial, port: String(initial.port) } : { id: crypto.randomUUID(), label: "", host: "", user: "", port: "22", key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false })`.
- Add `const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});` beside the existing `error`.
- Rename the state and **every** reference: `c` → `draft`, `setC` → `setDraft`, including the `bindProvisioned` effect at line 103 (`setDraft((prev) => ({ ...prev, disable_password_auth: disablePasswordAuth }))`), which is easy to miss because it is not a field handler. `tsc` will catch any survivor.
- The port input loses `required type="number" min={1} max={65535}` and becomes
  `<input inputMode="numeric" className={...} value={draft.port} onChange={(e) => setDraft((prev) => ({ ...prev, port: e.target.value }))} />`.
  `Number(e.target.value)` goes away.
- `duplicateLabel` reads `draft.label`.

- [ ] **Step 5: Validate in `submit()` and save the coerced connection**

```ts
async function submit(e: FormEvent) {
  e.preventDefault();
  const found = validateConnection(draft);
  setFieldErrors(found);
  if (Object.keys(found).length > 0) return;
  // The launcher takes a number; validation already proved the string parses.
  const connection: Connection = {
    ...draft,
    label: draft.label.trim(),
    host: draft.host.trim(),
    user: draft.user.trim(),
    port: Number(draft.port),
  };
  try {
    await (initial ? updateConnection(connection) : addConnection(connection));
    if (password) await setConnectionPassword(connection.id, password);
    else if (removePassword) await setConnectionPassword(connection.id, null);
    setPassword("");
    setRemovePassword(false);
    onSaved();
  } catch (err) {
    const e = err as CommandError;
    if (e?.code === "vault_locked") onVaultLocked();
    else setError(e?.message ?? "");
  }
}
```

Everything after the try is unchanged; only the two backend calls take `connection` instead of `c`.

- [ ] **Step 6: Render the messages and move focus**

Give the form a ref: `const formRef = useRef<HTMLFormElement>(null);` and `<form ref={formRef} …>`.

Add one effect:

```ts
useEffect(() => {
  if (Object.keys(fieldErrors).length === 0) return;
  formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
}, [fieldErrors]);
```

For each of label, host, user, port, directly after its `</label>` (the same placement as the existing duplicate-label `<p>`):

```tsx
{fieldErrors.label && <p id="err-label" className="text-xs text-danger">{t(fieldErrors.label)}</p>}
```

and on the input:

```tsx
className={fieldErrors.label ? `${field} border-danger!` : field}
aria-invalid={fieldErrors.label ? true : undefined}
aria-describedby={fieldErrors.label ? "err-label" : undefined}
```

`border-danger!` uses Tailwind v4's trailing important marker: `field` already carries `border-border`, and two `border-*` utilities in one class list resolve by stylesheet order, not attribute order, so the important marker is what makes the tint reliable.

Use ids `err-label`, `err-host`, `err-user`, `err-port`. Drop `required` from label, host and user. Do not touch the provisioning panel's password `required` — that is a different form and out of scope.

- [ ] **Step 7: Add the four i18n keys to both locales**

`en.json` under `connections`:

```json
"invalidLabel": "A label is required.",
"invalidHost": "Use only letters, digits, dots, underscores and hyphens.",
"invalidUser": "Use only letters, digits, dots, underscores and hyphens, or leave it empty.",
"invalidPort": "Enter a port between 1 and 65535.",
```

`da.json` under `connections`:

```json
"invalidLabel": "En etiket er påkrævet.",
"invalidHost": "Brug kun bogstaver, tal, punktum, understregning og bindestreg.",
"invalidUser": "Brug kun bogstaver, tal, punktum, understregning og bindestreg — eller lad feltet være tomt.",
"invalidPort": "Angiv en port mellem 1 og 65535.",
```

- [ ] **Step 8: Component tests**

Add three to `Connections.test.tsx`, each following the file's existing open-the-dialog pattern (`fireEvent.click(await screen.findByRole("button", { name: "Add connection" }))`, then `await screen.findByRole("dialog", { name: "Add connection" })`; fields found with `within(dialog).getByLabelText("Port")`).

1. **Refuses a bad port and focuses it.** Fill label/host, set port to `"0"`, click Save. Assert `api.addConnection` was not called, the text `"Enter a port between 1 and 65535."` is in the dialog, the port input has `aria-invalid="true"` and `aria-describedby="err-port"`, and `document.activeElement` is the port input.
2. **Refuses a host with a space, then saves a padded valid one.** Set host to `"web example.com"` with a valid port and Save. Assert `addConnection` was not called and `connections.invalidHost`'s English text is shown. Then set label to `"  Prod  "`, host to `"  web.example.com  "` and Save again. Assert `addConnection` was called once, with `label: "Prod"`, `host: "web.example.com"`, and `port` a number equal to `22` (`typeof call.port === "number"`).
3. **Saves an imported connection with an empty user.** Resolve `listConnections` with `{ ...conn, id: "imp", label: "Imported", user: "", source: "managed" }`, click the Edit button inside that row (`screen.getByText("Imported").closest("li")` then `within(row).getByRole("button", { name: "Edit connection" })`), press Save without touching a field, and assert `api.updateConnection` was called once with `user: ""` and `port: 22` as a number. This is the regression the `required` attribute caused.

Run: `cd apps/desktop && npx vitest run src/views/Connections.test.tsx`. Expected: PASS.

- [ ] **Step 9: Run the gate**

From `apps/desktop`: `npm test -- --run && npm run typecheck && npm run lint && npm run build`. Quote the output in the report. No Rust file changed — say so.

- [ ] **Step 10: Add the CHANGELOG entry and commit**

Under `## [Unreleased]` → `### Fixed`:

```markdown
- The connection form now refuses a host or user the launcher cannot use, says which field is wrong under that field, and allows a connection with no user.
```

```bash
git add apps/desktop/src/views/Connections.tsx apps/desktop/src/views/Connections.test.tsx apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json CHANGELOG.md
git commit -m "feat(desktop): validate the connection form before saving"
```

---

## Self-Review

- **Spec coverage:** host and user charset, the port range, the empty-user allowance, trimming, `aria-invalid`/`aria-describedby` and first-invalid focus are each a step and each a test. The spec's async-validation, duplicate-label and group/key/password non-goals are untouched.
- **Step scan:** the rule table is pinned twice (pure test, component test); the empty-user regression is a component test; trimming and numeric coercion are asserted in the same test that saves. The one judgement call left to the implementer is the exact Tailwind class order for `border-danger`, which the file's existing constants already demonstrate.
- **Type consistency:** `ConnectionDraft` differs from `Connection` only in `port`; the save path is the single place the conversion happens; `FieldErrors` values are keys, and `t()` is called only in render.
- **Review Focus mapping:** 1 → Step 1's port table and Step 8.1; 2 → Step 1's host table and Step 8.2; 3 → Step 8.3; 4 → Step 3's comment plus the Rust test named in Task 1 Step 3; 5 → Step 8.2's `typeof` assertion; 6 → Step 8.2's padded values.
- **Proportion:** one pure function, a state type change, four error paragraphs, four i18n pairs, no new file, no dependency, no Rust change.
- **Risk flagged:** removing `required` means a completely empty form now submits once and shows four messages instead of the browser's tooltip on one field. That is the intended single-style behaviour, and Step 1's "reports every offending field at once" test pins it.
