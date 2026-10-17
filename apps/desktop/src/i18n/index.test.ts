import { describe, it, expect } from "vitest";
import i18n from "./index";

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
  it("Danish is complete — every en leaf key exists in da, and vice versa", () => {
    const en = flattenKeys(i18n.getResourceBundle("en", "translation"));
    const da = flattenKeys(i18n.getResourceBundle("da", "translation"));
    expect(en.filter((k) => !da.includes(k))).toEqual([]);
    expect(da.filter((k) => !en.includes(k))).toEqual([]);
  });
});
