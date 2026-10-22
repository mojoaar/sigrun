import nordlys from "./nordlys";
import fjord from "./fjord";
import ravn from "./ravn";
import skumring from "./skumring";

export type TokenMap = { bg: string; surface: string; text: string; muted: string; accent: string; danger: string; border: string };
export type ThemeName = "nordlys" | "fjord" | "ravn" | "skumring";
export type Mode = "light" | "dark";

export const TOKENS = ["bg", "surface", "text", "muted", "accent", "danger", "border"] as const;

export const DEFAULT_THEME: ThemeName = "nordlys";

export const THEMES: ThemeName[] = ["nordlys", "fjord", "ravn", "skumring"];

const PALETTES: Record<ThemeName, { light: TokenMap; dark: TokenMap }> = { nordlys, fjord, ravn, skumring };

export function resolveMode(override?: Mode | null): Mode {
  if (override) return override;
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(name: ThemeName, mode: Mode): void {
  const root = document.documentElement;
  const resolvedName = name in PALETTES ? name : DEFAULT_THEME;
  const tokens = PALETTES[resolvedName][mode];
  for (const t of TOKENS) root.style.setProperty(`--color-${t}`, tokens[t]);
  root.setAttribute("data-theme", resolvedName);
  root.setAttribute("data-mode", mode);
}

const MODE_KEY = "sigrun.mode";

export function readModeOverride(): Mode | null {
  try {
    const m = localStorage.getItem(MODE_KEY);
    return m === "light" || m === "dark" ? m : null;
  } catch {
    return null;
  }
}

export function writeModeOverride(m: Mode | null): void {
  try {
    if (m) localStorage.setItem(MODE_KEY, m);
    else localStorage.removeItem(MODE_KEY);
  } catch {
    // storage unavailable: override just won't persist
  }
}
