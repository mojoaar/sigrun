import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { exportBundle, getSettings, importBundle, saveSettings, setWindowTheme, listTerminals, type CommandError, type Settings as S, type TerminalInfo } from "../api";
import { setLanguage, type Language } from "../i18n";
import { applyTheme, readModeOverride, resolveMode, THEMES, writeModeOverride, type Mode, type ThemeName } from "../themes";

const field = "h-9 rounded border border-border bg-bg px-2 text-text";
const btn = "h-9 shrink-0 rounded bg-accent px-3 text-bg";
// The empty id is "System default": no platform's default terminal can be asked
// for a tab, so a missing entry means false.
const canOpenTabs = (list: TerminalInfo[], id: string | null) =>
  list.find((t) => t.id === (id ?? ""))?.tabs ?? false;
export default function Settings() {
  const { t, i18n } = useTranslation();
  const [s, setS] = useState<S | null>(null);
  const [terminals, setTerminals] = useState<TerminalInfo[]>([]);
  const [mode, setMode] = useState<Mode | "system">(readModeOverride() ?? "system");
  const [result, setResult] = useState("");
  const [transferError, setTransferError] = useState<string | null>(null);

  useEffect(() => {
    // Both must land together: the stored open_in is only valid once we know
    // which terminals can open a tab. A failed terminal list must not block
    // the pane, so it degrades to none.
    Promise.all([getSettings(), listTerminals().catch(() => [] as TerminalInfo[])])
      .then(([next, list]) => {
        setTerminals(list);
        if (next.open_in === "tab" && !canOpenTabs(list, next.terminal)) {
          const fixed = { ...next, open_in: "window" };
          setS(fixed);
          saveSettings(fixed).catch((err) => console.warn("sigrun: could not save settings", err));
        } else setS(next);
      })
      .catch(() => {});
  }, []);
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
    setWindowTheme(m).catch((err) => console.warn("sigrun: could not set the window theme", err));
  };
  const da = i18n.language === "da";
  const zipFilter = { name: t("settings.transferZipFilter"), extensions: ["zip"] };
  // A cancelled picker resolves to null and must do nothing at all.
  const transfer = (op: () => Promise<string | null>) => {
    setResult("");
    setTransferError(null);
    op()
      .then((message) => { if (message !== null) setResult(message); })
      .catch((err) => {
        console.warn("sigrun: the transfer failed", err);
        setTransferError((err as CommandError)?.message ?? "");
      });
  };
  const exportTo = (zipped: boolean) => transfer(async () => {
    const path = zipped
      ? await save({ defaultPath: "sigrun-export", title: t("settings.transferExportZip"), filters: [zipFilter] })
      : await open({ directory: true, title: t("settings.transferExportFolder") });
    if (path === null) return null;
    const done = await exportBundle(path, zipped);
    return t("settings.transferExported", { connections: done.connections, keys: done.keys });
  });
  const importFrom = (zipped: boolean) => transfer(async () => {
    const path = zipped
      ? await open({ filters: [zipFilter], title: t("settings.transferImportZip") })
      : await open({ directory: true, title: t("settings.transferImportFolder") });
    if (path === null) return null;
    const done = await importBundle(path);
    return t("settings.transferImported", {
      added: done.added, skipped: done.skipped, invalid: done.invalid, unresolved: done.unresolved_keys,
    });
  });

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
          onChange={(e) => {
            const id = e.target.value || null;
            update({ terminal: id, ...(canOpenTabs(terminals, id) ? {} : { open_in: "window" }) });
          }}
        >
          <option value="">{t("settings.terminalDefault")}</option>
          {terminals.map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
        </select>
      </label>
      <label className="flex flex-col gap-1">{t("settings.openIn")}
        <select className={field} value={s.open_in === "tab" ? "tab" : "window"} onChange={(e) => update({ open_in: e.target.value })}>
          <option value="window">{t("settings.openInWindow")}</option>
          <option value="tab" disabled={!canOpenTabs(terminals, s.terminal)}>{t("settings.openInTab")}</option>
        </select>
      </label>
      {!canOpenTabs(terminals, s.terminal) && <p className="text-xs text-muted">{t("settings.openInUnavailable")}</p>}
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
      <div className="flex flex-col gap-2 border-t border-border pt-3">
        <h3 className="font-semibold">{t("settings.transferTitle")}</h3>
        <p className="text-xs text-muted">{t("settings.transferBody")}</p>
        <div className="flex flex-wrap gap-2">
          <button type="button" className={btn} onClick={() => exportTo(false)}>{t("settings.transferExportFolder")}</button>
          <button type="button" className={btn} onClick={() => exportTo(true)}>{t("settings.transferExportZip")}</button>
          <button type="button" className={btn} onClick={() => importFrom(false)}>{t("settings.transferImportFolder")}</button>
          <button type="button" className={btn} onClick={() => importFrom(true)}>{t("settings.transferImportZip")}</button>
        </div>
        <p role="status" className="text-xs text-muted">{result}</p>
        {transferError !== null && <p role="alert" className="text-xs text-danger">{transferError || t("common.error")}</p>}
      </div>
    </section>
  );
}
