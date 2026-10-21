# Five More UI Languages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The app ships German, French, Spanish, Swedish and Norwegian Bokmål beside English and Danish — window, in-app Help, tray and macOS menu alike — with nothing added per language beyond one JSON file and one table entry.

**Architecture:** One `LANGUAGES` table drives the picker; each locale is one JSON bundle plus one `resources` line. Theme names stop being a per-language field on `THEMES` and become ordinary i18n keys. The completeness guard compares every locale to English with plural suffixes normalised and checks each locale's plural families against `Intl.PluralRules`, so a later Polish is legal rather than a guard rewrite. Rust translates its own strings from the same stored language, as `tray::labels` and `menu_labels` already do.

**Tech Stack:** React 19 + i18next 25 + Tailwind; Rust 2021 + Tauri 2 menu/tray APIs. No new dependency.

**Spec:** none — this extends the existing i18n design in the shipped code. The binding rules are AGENTS.md's: every user-facing string through `t(...)` in both locales today, a completeness guard, and Rust strings matched on `Settings.language` for the parts Rust draws.

## Rulings (settled 2026-10-21)

1. **Five languages, not six.** Polish is dropped until a Polish reader can review it: it is the only locale with `few`/`many` plural forms, and unreviewed Polish in a release is worse than no Polish. The guard stays plural-category aware so adding it later is one task of the same shape.
2. **Norwegian is `nb`** (Bokmål), displayed as "Norsk". Not `no` (macrolanguage) and not `nn`.
3. **Theme names move into i18n.** `THEMES` loses `label` and `labelDa`; the picker renders `t(\`theme.${name}\`)`. All four names are proper nouns and stay identical in every locale, but they now live in one mechanism instead of eight fields.
4. **Native language names in the picker, never translated:** English, Dansk, Deutsch, Français, Español, Svenska, Norsk. Order: English first, then the rest by their English names.
5. **No OS-language detection.** The app still starts in English; changing the default is not part of this.
6. **One commit per language**, so a native reviewer can reject one without touching the others.

## Global Constraints

- Every user-facing string goes through `t(...)`; a locale's bundle must contain every English key (plural forms per that locale), and no key English lacks.
- Product nouns never translated: **Sigrun**, SSH, Ghostty, iTerm, Terminal, Windows, macOS, Linux, Arch, AUR, GitHub, Nordlys, Fjord, Ravn, Skumring. Keyboard glyphs (⌘, ⌥) stay.
- No new dependency, no new Tauri capability.
- Secrets never in argv, an environment value, a log line, an error message or the persisted model. Translations must not change interpolation placeholders (`{{count}}`, `{{id}}`, `{{name}}`, `{{label}}`) — a translated placeholder is a bug.
- Every change gets a `## [Unreleased]` CHANGELOG entry. Gate: from `apps/desktop`, `npm test -- --run && npm run typecheck && npm run lint && npm run build`; from the repo root, `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --all-targets` whenever a Rust file changes (Tasks 2–6 do).
- `~/.ssh` is read-only; nothing here touches it.

## Terminology (use these words everywhere, in every locale)

| English | German | French | Spanish | Swedish | Norwegian |
| --- | --- | --- | --- | --- | --- |
| connection | Verbindung | connexion | conexión | anslutning | tilkobling |
| key | Schlüssel | clé | clave | nyckel | nøkkel |
| vault | Tresor | coffre | bóveda | valv | hvelv |
| credential | Zugangsdaten (pl.) | identifiant | credencial | inloggning | pålogging |
| password | Passwort | mot de passe | contraseña | lösenord | passord |
| passphrase | Passphrase | phrase secrète | frase de contraseña | lösenfras | passfrase |
| fingerprint | Fingerabdruck | empreinte | huella | fingeravtryck | fingeravtrykk |
| host | Host | hôte | host | värd | vert |
| user | Benutzer | utilisateur | usuario | användare | bruker |
| group | Gruppe | groupe | grupo | grupp | gruppe |
| terminal | Terminal | terminal | terminal | terminal | terminal |
| settings | Einstellungen | réglages | ajustes | inställningar | innstillinger |
| export | exportieren | exporter | exportar | exportera | eksportere |
| import | importieren | importer | importar | importera | importere |
| Install SSH key | SSH-Schlüssel installieren | installer la clé SSH | instalar la clave SSH | installera SSH-nyckel | installere SSH-nøkkel |
| lock / unlock | sperren / entsperren | verrouiller / déverrouiller | bloquear / desbloquear | lås / lås upp | lås / lås opp |

Danish already ships and is not retranslated; it keeps *Login/Logins* for credential.

## Review Focus

1. **No English leaks.** In a non-English locale, no value may equal its English counterpart except the product nouns above and glyph-only strings; the Task 7 audit lists every exception and why.
2. **Placeholders survive translation.** Every `{{…}}` in English appears, spelled identically, in each locale that has the key.
3. **Plurals are grammatical, not mechanical.** German/French/Spanish/Swedish/Norwegian use `_one`/`_other` only; the guard refuses a locale whose families do not match `Intl.PluralRules`.
4. **The picker, the tray and the menu agree.** Choosing a language repaints all three without a restart (the menu already rebuilds on save).
5. **No language is a copy of Danish.** Norwegian is close enough to invite copy-paste; the reviewer checks that nothing Danish-only survives (*ikke*, *forbindelse*, *lås op*).
6. **Rust stays English-sourced.** Core errors are English; only `TrayLabels` and `MenuLabels` are translated, and their tests assert the new strings rather than being loosened to "not empty".

---

### Task 1: One language table, theme names through i18n

**Files:**
- Modify: `apps/desktop/src/i18n/index.ts`
- Modify: `apps/desktop/src/i18n/index.test.ts`
- Modify: `apps/desktop/src/i18n/en.json`, `da.json`
- Modify: `apps/desktop/src/views/Settings.tsx`
- Modify: `apps/desktop/src/themes/index.ts`
- Modify: `apps/desktop/index.html`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces in `i18n/index.ts`:
  - `export const LANGUAGES: { code: Language; name: string }[]` — English first, then Danish, German, French, Spanish, Swedish, Norwegian; each language task appends its own entry.
  - `export type Language = "en" | "da" | "de" | "fr" | "es" | "sv" | "nb"` — the union carries all seven codes from this task, but `resources` only holds the bundles that exist. A code with no bundle falls back to English through `fallbackLng`, which is the correct behaviour while a language task is in flight.
  - `export function isLanguage(value: string): value is Language`.
- `setLanguage` sets `document.documentElement.lang` next to `changeLanguage`, and falls back to `"en"` for an unknown code.
- `THEMES` becomes `Record<ThemeName, Record<string, never>>`-free: keep it as `export const THEMES: ThemeName[] = ["nordlys", "fjord", "ravn", "skumring"]` and let the picker map over it.

- [ ] **Step 1: Write the failing guard tests**

Replace the Danish-only parity test in `index.test.ts`:

```ts
const PLURAL = /_(zero|one|two|few|many|other)$/;

it("every locale covers English, with its own plural forms", () => {
  const en = flattenKeys(i18n.getResourceBundle("en", "translation")).map((k) => k.replace(PLURAL, ""));
  for (const code of LANGUAGES.map((l) => l.code)) {
    if (code === "en") continue;
    const keys = flattenKeys(i18n.getResourceBundle(code, "translation")).map((k) => k.replace(PLURAL, ""));
    expect({ code, missing: en.filter((k) => !keys.includes(k)) }).toEqual({ code, missing: [] });
    expect({ code, extra: keys.filter((k) => !en.includes(k)) }).toEqual({ code, extra: [] });
  }
});

it("each locale's plural families match its own rules", () => {
  for (const { code } of LANGUAGES) {
    const bundle = i18n.getResourceBundle(code, "translation") as Record<string, unknown>;
    const expected = new Intl.PluralRules(code).resolvedOptions().pluralCategories.map((c) => `inUse_${c}`).sort();
    const found = Object.keys((bundle.credentials ?? {}) as object).filter((k) => k.startsWith("inUse_")).sort();
    expect({ code, found }).toEqual({ code, found: expected });
  }
});

it("no locale leaves a placeholder behind", () => {
  const placeholders = (s: string) => [...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort();
  const walk = (obj: Record<string, unknown>, prefix = ""): [string, string][] =>
    Object.entries(obj).flatMap(([k, v]) =>
      v && typeof v === "object" ? walk(v as Record<string, unknown>, `${prefix}${k}.`) : [[`${prefix}${k}`, String(v)]],
    );
  const en = new Map(walk(i18n.getResourceBundle("en", "translation")));
  for (const { code } of LANGUAGES) {
    if (code === "en") continue;
    for (const [k, v] of walk(i18n.getResourceBundle(code, "translation"))) {
      expect({ code, key: k, holes: placeholders(v) }).toEqual({ code, key: k, holes: placeholders(en.get(k.replace(PLURAL, "")) ?? "") });
    }
  }
});
```

Run: `cd apps/desktop && npx vitest run src/i18n/index.test.ts` — fails: `LANGUAGES` is not exported.

- [ ] **Step 2: Add the table and the union**

In `i18n/index.ts`, above `init`:

```ts
export type Language = "en" | "da" | "de" | "fr" | "es" | "sv" | "nb";

// Native names, never translated. A language task adds its entry with its bundle.
export const LANGUAGES: { code: Language; name: string }[] = [
  { code: "en", name: "English" },
  { code: "da", name: "Dansk" },
];

export function isLanguage(value: string): value is Language {
  return LANGUAGES.some((l) => l.code === value);
}
```

`setLanguage` becomes:

```ts
export async function setLanguage(lng: string): Promise<void> {
  const code = isLanguage(lng) ? lng : "en";
  try { /* unchanged persist path, using code */ } catch { /* unchanged */ }
  await i18n.changeLanguage(code);
  document.documentElement.lang = code;
}
```

- [ ] **Step 3: Theme names through `t`**

`themes/index.ts`: `export const THEMES: ThemeName[] = ["nordlys", "fjord", "ravn", "skumring"];` and delete the `label`/`labelDa` record. `Settings.tsx`: drop `const da = ...` and render

```tsx
{THEMES.map((n) => <option key={n} value={n}>{t(`theme.${n}`)}</option>)}
```

`en.json` gains `"theme": { "nordlys": "Nordlys", "fjord": "Fjord", "ravn": "Ravn", "skumring": "Skumring" }`; `da.json` the same.

- [ ] **Step 4: The picker reads the table**

```tsx
<select className={field} value={s.language} onChange={(e) => { void setLanguage(e.target.value); update({ language: e.target.value }); }}>
  {LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
</select>
```

No `as Language` cast: `setLanguage` validates.

- [ ] **Step 5: `<html lang>`**

`apps/desktop/index.html` keeps `lang="en"` as the static default; `setLanguage` owns it afterwards. Do not set it in a component.

- [ ] **Step 6: Gate, CHANGELOG, commit**

Under `## [Unreleased]` → `### Changed`: "Theme names come from the interface language files, and the language picker is driven by one table."

```bash
git add apps/desktop/src/i18n/index.ts apps/desktop/src/i18n/index.test.ts apps/desktop/src/i18n/en.json apps/desktop/src/i18n/da.json apps/desktop/src/views/Settings.tsx apps/desktop/src/themes/index.ts apps/desktop/index.html CHANGELOG.md
git commit -m "feat(i18n): drive the language picker from one table"
```

---

### Tasks 2–6: One language each

Same shape five times. Order: **de, fr, es, sv, nb**. For language `XX` with display name `NAME`:

**Files:** `apps/desktop/src/i18n/XX.json` (new), `apps/desktop/src/i18n/index.ts` (import + one `resources` entry + one `LANGUAGES` entry), `apps/desktop/src-tauri/src/tray.rs`, `apps/desktop/src-tauri/src/lib.rs`, `CHANGELOG.md`.

- [ ] **Step 1:** Copy `en.json` to `XX.json` and translate every leaf, using the Terminology table. Keep `theme.*` as the proper nouns. Keep `credentials.inUse_one`/`inUse_other` and use the locale's real singular/plural (`{{count}}` stays).
- [ ] **Step 2:** Register the bundle and append the `LANGUAGES` entry in the settled order. Run `npx vitest run src/i18n` — the guard now checks the new file.
- [ ] **Step 3:** Rust. Add the arm to `tray::labels` and `menu_labels`:

```rust
"de" => TrayLabels { open: "…", quit: "…", unlock: "…", discovered: "…".into() },
```

and the same for `MenuLabels { file, new_connection, import, export, settings, edit, view, window, quit }`. Menu items keep their accelerators; only the strings change.
- [ ] **Step 4:** Extend both Rust tests with the new language's exact strings — `labels_are_localized` and `menu_labels_are_localized` assert a table, not "differs from English".
- [ ] **Step 5:** CHANGELOG under `### Added`: "German (Deutsch) in the interface, the tray and the macOS menu." (adjust the language).
- [ ] **Step 6:** Both gates, one commit: `feat(i18n): add German` (and so on).

Language-specific notes the implementer must honour:

- **German (de):** formal *Sie* nowhere — the UI speaks impersonally, as English does ("Verbindung hinzufügen", not "Fügen Sie eine Verbindung hinzu"). Compound nouns capitalised; `SSH-Schlüssel`, `MAC-Adresse` where the English says it.
- **French (fr):** *vous* only in the passphrase prompt; otherwise infinitive labels ("Ajouter une connexion"). Non-breaking space before `:` `;` `!` `?` is not required — do not introduce it.
- **Spanish (es):** neutral Spanish, infinitive labels, *tú* nowhere. `contraseña`, not `clave`, for password; `clave` is reserved for key.
- **Swedish (sv):** infinitive labels; `anslutning` for connection, never `koppling`. *du* only in the passphrase prompt.
- **Norwegian Bokmål (nb):** derived from `da.json` only as a starting point — check every string. Banned Danish forms: `ikke`→`ikke` (same), `forbindelse`→`tilkobling`, `lås op`→`lås opp`, `indstillinger`→`innstillinger`. `pålogging`, not `innlogging`.

---

### Task 7: Docs, audit and final pass

**Files:** `README.md`, `apps/desktop/src/views/Help.tsx` only if it names languages, `CHANGELOG.md`.

- [ ] **Step 1:** README: wherever it lists interface languages, name all seven. If it does not list them, say it once in *The desktop app* (themes/languages sentence).
- [ ] **Step 2:** Run the leak audit and paste the result into the report:

```bash
cd apps/desktop/src/i18n && for f in de fr es sv nb; do
  python3 - "$f.json" <<'PY'
import json,sys
en=json.load(open("en.json")); xx=json.load(open(sys.argv[1]))
def flat(o,p=""):
    for k,v in o.items():
        yield from (flat(v,p+k+".") if isinstance(v,dict) else [(p+k,v)])
same=[k for k,v in flat(xx) if flat(en).__contains__((k,v))]
print(sys.argv[1], "identical:", same)
PY
done
```

Every hit must be a product noun or a glyph. Anything else is retranslated.
- [ ] **Step 3:** Gate, CHANGELOG under `### Added` naming all five languages, commit `docs: list the interface languages`.

---

## Self-Review

- **Coverage:** plumbing (Task 1), five bundles with Rust strings (Tasks 2–6), docs and the English-leak audit (Task 7). Polish is deliberately absent with a recorded reason.
- **Guard strength:** the plural check uses `Intl.PluralRules` per locale, so a later `pl` needs `_few`/`_many` and nothing else; the placeholder check catches a translated `{{count}}`, which is the failure mode that ships silently.
- **Type consistency:** `Language` is the union of all seven codes; `resources` grows per task; `setLanguage(lng: string)` validates and falls back, so an unknown stored value cannot render raw keys.
- **Proportion:** one JSON per language, one table entry, one Rust arm pair, one test table extension. No abstraction, no new file per language beyond the bundle.
- **Risks flagged:** (a) the translations are machine-authored and unreviewed; the per-language commits let a native reviewer reject one; (b) `SetLanguage` writes `document.documentElement.lang` only on change, not on first load — the mount path must call it (Task 1 Step 2's `setLanguage` is the only path; if `App` applies the stored setting without calling it, add the attribute there); (c) German compounds can overflow the 96-wide dialogs — the review checks `Add connection` and the credential source radio at the longest label.
