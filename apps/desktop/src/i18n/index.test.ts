import { describe, it, expect } from "vitest";
import i18n, { LANGUAGES, setLanguage } from "./index";
import { THEMES } from "../themes";

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
    // A code that never enters the table falls back to English rather than switching to nothing.
    await setLanguage("xx");
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

  it("every registered bundle has a LANGUAGES row", () => {
    // A bundle in resources but not the table is announced as English by <html lang>.
    expect(Object.keys(i18n.options.resources ?? {}).sort()).toEqual(LANGUAGES.map((l) => l.code).sort());
  });

  it("every theme has a label in every bundle", () => {
    for (const code of LANGUAGES.map((l) => l.code)) {
      const bundle = i18n.getResourceBundle(code, "translation") as Record<string, unknown>;
      const labelled = Object.keys((bundle.theme ?? {}) as object);
      expect({ code, missing: THEMES.filter((n) => !labelled.includes(n)) }).toEqual({ code, missing: [] });
    }
  });

  it("each locale's plural families match its own rules", () => {
    for (const { code } of LANGUAGES) {
      const bundle = i18n.getResourceBundle(code, "translation") as Record<string, unknown>;
      const creds = Object.keys((bundle.credentials ?? {}) as object);
      // Every locale needs _other; i18next does not fall back to it from another
      // category, so it must exist. _one only where the locale has that category.
      // fr/es also report "many" (10^6 connections), which no bundle covers.
      const categories = new Intl.PluralRules(code).resolvedOptions().pluralCategories;
      const required = categories.includes("one") ? ["inUse_one", "inUse_other"] : ["inUse_other"];
      expect({ code, missing: required.filter((k) => !creds.includes(k)) }).toEqual({ code, missing: [] });
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
