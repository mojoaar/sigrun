import { describe, it, expect } from "vitest";
import { applyTheme, resolveMode, THEMES, DEFAULT_THEME, TOKENS } from "./index";

describe("themes", () => {
  it("exposes the four themes with nordlys as default", () => {
    expect([...THEMES].sort()).toEqual(["fjord", "nordlys", "ravn", "skumring"]);
    expect(DEFAULT_THEME).toBe("nordlys");
  });
  it("applies nordlys dark tokens to :root", () => {
    applyTheme("nordlys", "dark");
    const bg = document.documentElement.style.getPropertyValue("--color-bg");
    expect(bg).not.toBe("");
  });
  it("applies a light palette distinct from dark", () => {
    applyTheme("nordlys", "dark");
    const dark = document.documentElement.style.getPropertyValue("--color-bg");
    applyTheme("nordlys", "light");
    expect(document.documentElement.style.getPropertyValue("--color-bg")).not.toBe(dark);
  });
  it("defines all seven tokens as hex strings for every theme and mode", () => {
    for (const name of THEMES) {
      for (const mode of ["light", "dark"] as const) {
        applyTheme(name, mode);
        expect(document.documentElement.dataset.theme).toBe(name);
        expect(document.documentElement.dataset.mode).toBe(mode);
        for (const t of TOKENS) {
          expect(document.documentElement.style.getPropertyValue(`--color-${t}`)).toMatch(/^#[0-9a-f]{6}$/i);
        }
      }
    }
  });
  it("resolveMode prefers the override, else the OS scheme", () => {
    expect(resolveMode("light")).toBe("light");
    window.matchMedia = ((q: string) => ({ matches: q.includes("dark") })) as typeof window.matchMedia;
    expect(resolveMode()).toBe("dark");
    expect(resolveMode(null)).toBe("dark");
  });
  it("falls back to DEFAULT_THEME when given an invalid theme name", () => {
    expect(() => applyTheme("invalid_theme" as unknown as Parameters<typeof applyTheme>[0], "dark")).not.toThrow();
    expect(document.documentElement.getAttribute("data-theme")).toBe("nordlys");
  });
});
