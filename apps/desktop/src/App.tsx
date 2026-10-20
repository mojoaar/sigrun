import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { getSettings, getStatus, vaultLock, type VaultState } from "./api";
import i18n from "./i18n";
import { applyTheme, DEFAULT_THEME, readModeOverride, resolveMode, type ThemeName } from "./themes";
import Connections, { type ConnectionError } from "./views/Connections";
import Credentials from "./views/Credentials";
import Help from "./views/Help";
import Keys from "./views/Keys";
import Settings from "./views/Settings";
import VaultGate from "./views/VaultGate";
import ErrorBoundary from "./components/ErrorBoundary";

type Tab = "connections" | "keys" | "credentials" | "settings" | "help";
const TABS: Tab[] = ["connections", "keys", "credentials", "settings", "help"];

/** The overlay title bar is macOS-only, so only macOS gets the traffic-light inset. */
export function isMacTitlebar(): boolean {
  return navigator.userAgent.includes("Mac");
}

export default function App() {
  const { t } = useTranslation();
  const [vault, setVault] = useState<VaultState | null>(null);
  const [tab, setTab] = useState<Tab>("connections");
  const [notice, setNotice] = useState(false);
  const [connectionError, setConnectionError] = useState<ConnectionError | null>(null);
  const [newConnection, setNewConnection] = useState(false);
  const [transfer, setTransfer] = useState<"import" | "export" | null>(null);
  // The menu listener is registered once; the ref lets it see the current gate
  // state instead of the value captured on mount.
  const vaultRef = useRef(vault);
  vaultRef.current = vault;

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
    let unLocked: (() => void) | undefined;
    let unError: (() => void) | undefined;
    let unMenu: (() => void) | undefined;
    let gone = false;
    try {
      listen("vault-locked", () => { setNotice(true); setVault("Locked"); })
        .then((f) => { if (gone) f(); else unLocked = f; })
        .catch((err) => console.warn("sigrun: could not listen for vault-locked", err));
      listen<ConnectionError>("connection-error", (e) => { setConnectionError(e.payload); setTab("connections"); })
        .then((f) => { if (gone) f(); else unError = f; })
        .catch((err) => console.warn("sigrun: could not listen for connection-error", err));
      // A menu action is inert while locked: no view behind VaultGate can open.
      listen<string>("menu", (e) => {
        if (vaultRef.current !== "Unlocked") return;
        switch (e.payload) {
          case "new-connection": setTab("connections"); setNewConnection(true); break;
          case "settings": setTab("settings"); break;
          case "import": setTab("settings"); setTransfer("import"); break;
          case "export": setTab("settings"); setTransfer("export"); break;
        }
      })
        .then((f) => { if (gone) f(); else unMenu = f; })
        .catch((err) => console.warn("sigrun: could not listen for menu actions", err));
    } catch { /* not running under Tauri */ }
    return () => { gone = true; unLocked?.(); unError?.(); unMenu?.(); };
  }, []);

  const locked = () => { setNotice(true); void refresh(); };
  const done = () => { setNotice(false); void refresh(); };

  return (
    <div className="min-h-screen bg-bg text-text">
      <header
        data-tauri-drag-region="deep"
        className={`flex items-center justify-between border-b border-border p-3${isMacTitlebar() ? " pl-20" : ""}`}
      >
        <h1 className="text-xl font-semibold">{t("app.title")}</h1>
        {vault === "Unlocked" && (
          <nav className="flex items-center gap-3">
            {TABS.map((n) => (
              <button key={n} aria-current={tab === n ? "page" : undefined} className={tab === n ? "text-accent" : "text-muted"} onClick={() => setTab(n)}>{t(`nav.${n}`)}</button>
            ))}
            <button className="rounded border border-danger px-2 py-1 text-danger hover:bg-danger hover:text-bg" onClick={() => vaultLock().then(refresh)}>{t("vault.lock")}</button>
          </nav>
        )}
      </header>
      <main className="p-4">
        {vault === null && <p className="text-muted">{t("common.loading")}</p>}
        {vault === "Uninitialized" && <VaultGate mode="create" onDone={done} />}
        {vault === "Locked" && <VaultGate notice={notice} onDone={done} />}
        {vault === "Unlocked" && tab === "connections" && (
          <ErrorBoundary key="connections"><Connections onVaultLocked={locked} onGoToKeys={() => setTab("keys")} connectionError={connectionError} onConnectionErrorHandled={() => setConnectionError(null)} openNew={newConnection} onNewOpened={() => setNewConnection(false)} /></ErrorBoundary>
        )}
        {vault === "Unlocked" && tab === "keys" && (
          <ErrorBoundary key="keys"><Keys onVaultLocked={locked} /></ErrorBoundary>
        )}
        {vault === "Unlocked" && tab === "credentials" && (
          <ErrorBoundary key="credentials"><Credentials onVaultLocked={locked} /></ErrorBoundary>
        )}
        {vault === "Unlocked" && tab === "settings" && (
          <ErrorBoundary key="settings"><Settings transferRequest={transfer} onTransferDone={() => setTransfer(null)} /></ErrorBoundary>
        )}
        {vault === "Unlocked" && tab === "help" && (
          <ErrorBoundary key="help"><Help /></ErrorBoundary>
        )}
      </main>
    </div>
  );
}
