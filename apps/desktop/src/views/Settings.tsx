import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getSettings, saveSettings, listTerminals, type Settings as S, type TerminalInfo } from "../api";
import { setLanguage, type Language } from "../i18n";
import { applyTheme, readModeOverride, resolveMode, THEMES, writeModeOverride, type Mode, type ThemeName } from "../themes";

const field = "h-9 rounded border border-border bg-bg px-2 text-text";
export default function Settings() {
  const { t, i18n } = useTranslation();
  const [s, setS] = useState<S | null>(null);
  const [terminals, setTerminals] = useState<TerminalInfo[]>([]);
  const [mode, setMode] = useState<Mode | "system">(readModeOverride() ?? "system");

  useEffect(() => { getSettings().then(setS).catch(() => {}); }, []);
  useEffect(() => { listTerminals().then(setTerminals).catch(() => {}); }, []);
  if (!s) return <p className="text-muted">{t("common.loading")}</p>;

  const update = (patch: Partial<S>) => {
    const next = { ...s, ...patch };
    setS(next);
    saveSettings(next).catch((err) => console.warn("sigrun: could not save settings", err));
  };
  const applyMode = (m: Mode | "system") => {
    setMode(m);
    writeModeOverride(m === "system" ? null : m);
    applyTheme(s.theme as ThemeName, resolveMode(m === "system" ? null : m));
  };
  const da = i18n.language === "da";

  return (
    <section className="flex max-w-sm flex-col gap-3">
      <h2 className="text-lg font-semibold">{t("settings.title")}</h2>
      <label className="flex flex-col gap-1">{t("settings.theme")}
        <select className={field} value={s.theme} onChange={(e) => {
          applyTheme(e.target.value as ThemeName, resolveMode(mode === "system" ? null : mode));
          update({ theme: e.target.value });
        }}>
          {(Object.keys(THEMES) as ThemeName[]).map((n) => <option key={n} value={n}>{da ? THEMES[n].labelDa : THEMES[n].label}</option>)}
        </select>
      </label>
      <label className="flex flex-col gap-1">{t("settings.mode")}
        <select className={field} value={mode} onChange={(e) => applyMode(e.target.value as Mode | "system")}>
          <option value="system">{t("settings.modeSystem")}</option>
          <option value="light">{t("settings.modeLight")}</option>
          <option value="dark">{t("settings.modeDark")}</option>
        </select>
      </label>
      <label className="flex flex-col gap-1">{t("settings.language")}
        <select className={field} value={s.language} onChange={(e) => { void setLanguage(e.target.value as Language); update({ language: e.target.value }); }}>
          <option value="en">English</option>
          <option value="da">Dansk</option>
        </select>
      </label>
      <label className="flex flex-col gap-1">{t("settings.terminal")}
        <select
          className={field}
          value={terminals.some((x) => x.id === s.terminal) ? (s.terminal ?? "") : ""}
          onChange={(e) => update({ terminal: e.target.value || null })}
        >
          <option value="">{t("settings.terminalDefault")}</option>
          {terminals.map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
        </select>
      </label>
      <label className="flex flex-col gap-1">{t("settings.openIn")}
        <select className={field} value={s.open_in === "tab" ? "tab" : "window"} onChange={(e) => update({ open_in: e.target.value })}>
          <option value="window">{t("settings.openInWindow")}</option>
          <option value="tab">{t("settings.openInTab")}</option>
        </select>
      </label>
      <label className="flex flex-col gap-1">{t("settings.autoLock")}
        <select
          className={field}
          value={String(s.auto_lock_minutes)}
          onChange={(e) => update({ auto_lock_minutes: Number(e.target.value) })}
        >
          {[0, 5, 15, 30, 60, 120].map((m) => (
            <option key={m} value={m}>{m === 0 ? t("settings.autoLockNever") : t("settings.autoLockMinutes", { count: m })}</option>
          ))}
        </select>
      </label>
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={s.start_minimized} onChange={(e) => update({ start_minimized: e.target.checked })} />
        {t("settings.startMinimized")}
      </label>
    </section>
  );
}
