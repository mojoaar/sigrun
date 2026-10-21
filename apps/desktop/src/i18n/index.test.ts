import { describe, it, expect } from "vitest";
import i18n, { LANGUAGES, setLanguage } from "./index";

const PLURAL = /_(zero|one|two|few|many|other)$/;

function flattenKeys(obj: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === "object"
      ? flattenKeys(v as Record<string, unknown>, `${prefix}${k}.`)
      : [`${prefix}${k}`],
  );
}

describe("i18n", () => {
  it("defaults to English and resolves a known key", () => {
    expect(i18n.t("app.title")).toBe("Sigrun");
  });

  it("setLanguage validates the code and keeps <html lang> in step", async () => {
    await setLanguage("da");
    expect(i18n.language).toBe("da");
    expect(document.documentElement.lang).toBe("da");
    // A code with no bundle yet falls back to English rather than switching to nothing.
    await setLanguage("de");
    expect(i18n.language).toBe("en");
    expect(document.documentElement.lang).toBe("en");
  });

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
    // English has no _other for a form a locale adds (fr _many, _zero), and its
    // _one text may legitimately drop {{count}}: prefer the same form, then _other.
    const reference = (key: string) =>
      placeholders(en.get(key) ?? en.get(`${key.replace(PLURAL, "")}_other`) ?? "");
    for (const { code } of LANGUAGES) {
      if (code === "en") continue;
      for (const [k, v] of walk(i18n.getResourceBundle(code, "translation"))) {
        expect({ code, key: k, holes: placeholders(v) }).toEqual({ code, key: k, holes: reference(k) });
      }
    }
  });
});
