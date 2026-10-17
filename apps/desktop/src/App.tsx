import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { getSettings, getStatus, vaultLock, type VaultState } from "./api";
import i18n from "./i18n";
import { applyTheme, DEFAULT_THEME, readModeOverride, resolveMode, type ThemeName } from "./themes";
import Connections from "./views/Connections";
import Keys from "./views/Keys";
import Settings from "./views/Settings";
import VaultGate from "./views/VaultGate";
import ErrorBoundary from "./components/ErrorBoundary";

type Tab = "connections" | "keys" | "settings";
const TABS: Tab[] = ["connections", "keys", "settings"];

export default function App() {
  const { t } = useTranslation();
  const [vault, setVault] = useState<VaultState | null>(null);
  const [tab, setTab] = useState<Tab>("connections");
  const [notice, setNotice] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [s, settings] = await Promise.all([getStatus(), getSettings().catch(() => null)]);
      applyTheme((settings?.theme as ThemeName) ?? DEFAULT_THEME, resolveMode(readModeOverride()));
      await i18n.changeLanguage(settings?.language ?? "en");
      const v = String(s.vault).toLowerCase();
      setVault(v === "unlocked" ? "Unlocked" : v === "uninitialized" ? "Uninitialized" : "Locked");
    } catch {
      setVault("Locked");
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    let un: (() => void) | undefined;
    let gone = false;
    try {
      listen("vault-locked", () => { setNotice(true); setVault("Locked"); })
        .then((f) => { if (gone) f(); else un = f; })
        .catch(() => {});
    } catch { /* not running under Tauri */ }
    return () => { gone = true; un?.(); };
  }, []);

  const locked = () => { setNotice(true); void refresh(); };
  const done = () => { setNotice(false); void refresh(); };

  return (
    <div className="min-h-screen bg-bg text-text">
      <header className="flex items-center justify-between border-b border-border p-3">
        <h1 className="flex items-center gap-2 text-xl font-semibold">
          <img src="/app-icon.png" alt="" className="h-7 w-7 rounded" />
          {t("app.title")}
        </h1>
        {vault === "Unlocked" && (
          <nav className="flex items-center gap-3">
            {TABS.map((n) => (
              <button key={n} aria-current={tab === n} className={tab === n ? "text-accent" : "text-muted"} onClick={() => setTab(n)}>{t(`nav.${n}`)}</button>
            ))}
            <button className="rounded border border-border px-2 py-1" onClick={() => vaultLock().then(refresh)}>{t("vault.lock")}</button>
          </nav>
        )}
      </header>
      <main className="p-4">
        {vault === null && <p className="text-muted">{t("common.loading")}</p>}
        {vault === "Uninitialized" && <VaultGate mode="create" onDone={done} />}
        {vault === "Locked" && <VaultGate notice={notice} onDone={done} />}
        {vault === "Unlocked" && tab === "connections" && (
          <ErrorBoundary key="connections"><Connections onVaultLocked={locked} /></ErrorBoundary>
        )}
        {vault === "Unlocked" && tab === "keys" && (
          <ErrorBoundary key="keys"><Keys onVaultLocked={locked} /></ErrorBoundary>
        )}
        {vault === "Unlocked" && tab === "settings" && (
          <ErrorBoundary key="settings"><Settings /></ErrorBoundary>
        )}
      </main>
    </div>
  );
}
