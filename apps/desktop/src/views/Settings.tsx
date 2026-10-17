import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getSettings, saveSettings, type Settings as S } from "../api";
import { setLanguage, type Language } from "../i18n";
import { applyTheme, resolveMode, THEMES, type Mode, type ThemeName } from "../themes";

const field = "rounded border border-border bg-surface px-2 py-1 text-text";
const MODE_KEY = "sigrun.mode";

export function storedMode(): Mode | null {
  const m = localStorage.getItem(MODE_KEY);
  return m === "light" || m === "dark" ? m : null;
}

export default function Settings() {
  const { t, i18n } = useTranslation();
  const [s, setS] = useState<S | null>(null);
  const [mode, setMode] = useState<Mode | "system">(storedMode() ?? "system");

  useEffect(() => { getSettings().then(setS).catch(() => {}); }, []);
  if (!s) return <p className="text-muted">{t("common.loading")}</p>;

  const update = (patch: Partial<S>) => {
    const next = { ...s, ...patch };
    setS(next);
    saveSettings(next).catch(() => {});
  };
  const applyMode = (m: Mode | "system") => {
    setMode(m);
    if (m === "system") localStorage.removeItem(MODE_KEY);
    else localStorage.setItem(MODE_KEY, m);
    applyTheme(s.theme as ThemeName, resolveMode(m === "system" ? null : m));
  };
  const da = i18n.language === "da";

  return (
    <section className="flex max-w-sm flex-col gap-3">
      <h2 className="text-lg font-semibold">{t("settings.title")}</h2>
      <label className="flex flex-col">{t("settings.theme")}
        <select className={field} value={s.theme} onChange={(e) => {
          applyTheme(e.target.value as ThemeName, resolveMode(mode === "system" ? null : mode));
          update({ theme: e.target.value });
        }}>
          {(Object.keys(THEMES) as ThemeName[]).map((n) => <option key={n} value={n}>{da ? THEMES[n].labelDa : THEMES[n].label}</option>)}
        </select>
      </label>
      <label className="flex flex-col">{t("settings.mode")}
        <select className={field} value={mode} onChange={(e) => applyMode(e.target.value as Mode | "system")}>
          <option value="system">{t("settings.modeSystem")}</option>
          <option value="light">{t("settings.modeLight")}</option>
          <option value="dark">{t("settings.modeDark")}</option>
        </select>
      </label>
      <label className="flex flex-col">{t("settings.language")}
        <select className={field} value={s.language} onChange={(e) => { void setLanguage(e.target.value as Language); update({ language: e.target.value }); }}>
          <option value="en">English</option>
          <option value="da">Dansk</option>
        </select>
      </label>
      <label className="flex flex-col">{t("settings.terminal")}
        <input className={field} value={s.terminal ?? ""} onChange={(e) => update({ terminal: e.target.value || null })} />
      </label>
    </section>
  );
}
