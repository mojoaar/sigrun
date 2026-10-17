import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { addConnection, connectForce, connectGuarded, importKey, listConnections, listKeys, provisionConnection, removeConnection, trustHost, updateConnection, type CommandError, type Connection, type KeyView, type ProvisionOutcome } from "../api";
import ConnectionRow from "../components/ConnectionRow";

const field = "h-9 w-full rounded border border-border bg-bg px-2 text-text";

// Mirrors tray_menu_model: blank groups are ungrouped, groups and labels sorted.
export function groupConnections(conns: Connection[]): { group: string | null; items: Connection[] }[] {
  const loose: Connection[] = [];
  const groups = new Map<string, Connection[]>();
  for (const c of conns) {
    const group = c.group?.trim();
    if (group) groups.set(group, [...(groups.get(group) ?? []), c]);
    else loose.push(c);
  }
  const byLabel = (a: Connection, b: Connection) => a.label.localeCompare(b.label);
  return [
    { group: null, items: loose.sort(byLabel) },
    ...[...groups].sort(([a], [b]) => a.localeCompare(b)).map(([group, items]) => ({ group, items: items.sort(byLabel) })),
  ].filter((s) => s.items.length > 0);
}

function Form({ initial, keys, onClose, onSaved, onProvision, bindProvisioned, onGoToKeys }: {
  initial: Connection | null;
  keys: KeyView[];
  onClose: () => void;
  onSaved: () => void;
  onProvision: (c: Connection) => void;
  // Lets the parent push the persisted disable-password-auth flag back in, so a
  // later Save on this still-open form cannot revert it.
  bindProvisioned: (sync: ((disablePasswordAuth: boolean) => void) | null) => void;
  onGoToKeys?: () => void;
}) {
  const { t } = useTranslation();
  const [c, setC] = useState<Connection>(
    initial ?? { id: crypto.randomUUID(), label: "", host: "", user: "", port: 22, key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false },
  );
  const [error, setError] = useState(false);
  const canProvision = keys.some((k) => k.available);
  useEffect(() => {
    bindProvisioned((disablePasswordAuth) => setC((prev) => ({ ...prev, disable_password_auth: disablePasswordAuth })));
    return () => bindProvisioned(null);
  }, [bindProvisioned]);
  async function submit(e: FormEvent) {
    e.preventDefault();
    try {
      await (initial ? updateConnection(c) : addConnection(c));
      onSaved();
    } catch {
      setError(true);
    }
  }
  return (
    <div role="dialog" aria-label={t(initial ? "connections.edit" : "connections.add")} className="fixed inset-0 flex items-center justify-center bg-black/50">
      <form onSubmit={submit} className="flex w-96 flex-col gap-2 rounded border border-border bg-surface p-4">
        <label>{t("connections.label")}<input required className={field} value={c.label} onChange={(e) => setC({ ...c, label: e.target.value })} /></label>
        <label>{t("connections.host")}<input required className={field} value={c.host} onChange={(e) => setC({ ...c, host: e.target.value })} /></label>
        <label>{t("connections.user")}<input required className={field} value={c.user} onChange={(e) => setC({ ...c, user: e.target.value })} /></label>
        <label>{t("connections.port")}<input required type="number" min={1} max={65535} className={field} value={c.port} onChange={(e) => setC({ ...c, port: Number(e.target.value) })} /></label>
        <label>{t("connections.group")}<input className={field} value={c.group ?? ""} onChange={(e) => setC({ ...c, group: e.target.value || null })} /></label>
        <label>{t("connections.key")}
          <select className={field} value={c.key_id ?? ""} onChange={(e) => setC({ ...c, key_id: e.target.value || null })}>
            <option value=""></option>
            {keys.map((k) => <option key={k.key.id} value={k.key.id}>{k.key.name}</option>)}
          </select>
        </label>
        {error && <p role="alert" className="text-danger">{t("common.error")}</p>}
        <div className="flex items-center justify-end gap-2">
          {initial && (canProvision
            ? <button type="button" className="mr-auto rounded border border-border px-2 py-1 hover:text-accent" onClick={() => onProvision(initial)}>{t("provision.install")}</button>
            : <button type="button" className="mr-auto rounded border border-border px-2 py-1 hover:text-accent" onClick={onGoToKeys}>{t("provision.importKey")}</button>)}
          <button type="button" onClick={onClose}>{t("common.cancel")}</button>
          <button type="submit" className="rounded bg-accent px-3 py-1 text-bg">{t("common.save")}</button>
        </div>
      </form>
    </div>
  );
}

type TrustRetry = { host: string; port: number; run: () => Promise<void> };

function ProvisionPanel({ conn, keys, onTrustError, onVaultLocked, onError, onClose, onDone }: {
  conn: Connection;
  keys: KeyView[];
  onTrustError: (err: unknown, retry: TrustRetry) => void;
  onVaultLocked: () => void;
  onError: (err: unknown) => void;
  onClose: () => void;
  onDone: (outcome: ProvisionOutcome, disablePasswordAuth: boolean) => void;
}) {
  const { t } = useTranslation();
  const vaultKeys = keys.filter((k) => k.key.source === "vault");
  const diskKeys = keys.filter((k) => k.key.source === "disk" && k.available);
  const [selected, setSelected] = useState<string[]>(
    // Only a vault key may be pre-selected: a disk key has no checkbox to uncheck it with.
    conn.key_id && vaultKeys.some((k) => k.key.id === conn.key_id) ? [conn.key_id] : [],
  );
  const [diskSelected, setDiskSelected] = useState<string[]>([]);
  const [password, setPassword] = useState("");
  const [keyPassphrase, setKeyPassphrase] = useState("");
  const [disable, setDisable] = useState(conn.disable_password_auth);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Belt and braces: a stale selection of a now-disk key must not reach the backend.
  const keyIds = selected.filter((id) => vaultKeys.some((k) => k.key.id === id));
  const needsKeyPassphrase = diskSelected.some((id) => diskKeys.find((k) => k.key.id === id)?.key.has_passphrase);
  // Disk id → vault id, so a trust retry reuses the key it already imported.
  const imported = useRef(new Map<string, string>());

  const close = () => {
    setPassword("");
    setKeyPassphrase("");
    onClose();
  };

  // Recursive on purpose: the trust retry calls the same request again once.
  async function run(): Promise<void> {
    setBusy(true);
    setError(null);
    const pw = password;
    try {
      const ids = [...keyIds];
      for (const id of diskSelected) {
        const cached = imported.current.get(id);
        if (cached) { ids.push(cached); continue; }
        const dk = diskKeys.find((k) => k.key.id === id);
        if (!dk?.key.disk_path) continue;
        const v = await importKey(dk.key.disk_path, dk.key.name, keyPassphrase || null);
        imported.current.set(id, v.key.id);
        ids.push(v.key.id);
      }
      const outcome = await provisionConnection(conn.id, pw, ids, disable);
      // A skipped or unrequested hardening leaves sshd as it was, so it must not
      // be recorded as if password auth were off.
      const persisted = outcome.hardening === "applied" ? disable : conn.disable_password_auth;
      onDone(outcome, persisted);
      // Persisted after the install is reported, so a failure here cannot hide it.
      try {
        await updateConnection({ ...conn, disable_password_auth: persisted });
      } catch (err) {
        onError(err);
      }
    } catch (err) {
      const code = (err as CommandError)?.code;
      if (code === "host_key_unknown" || code === "host_key_mismatch")
        onTrustError(err, { host: conn.host, port: conn.port, run });
      else if (code === "vault_locked") onVaultLocked();
      else setError((err as CommandError)?.message ?? "");
    } finally {
      setPassword("");
      setKeyPassphrase("");
      setBusy(false);
    }
  }

  return (
    <div role="dialog" aria-label={t("provision.title")} className="fixed inset-0 flex items-center justify-center bg-black/50">
      <form onSubmit={(e) => { e.preventDefault(); void run(); }} className="flex w-96 flex-col gap-2 rounded border border-border bg-surface p-4">
        <h3 className="font-semibold">{t("provision.title")}</h3>
        <p className="text-sm text-muted">{t("provision.body")}</p>
        <fieldset className="flex flex-col gap-1">
          <legend>{t("provision.selectKeys")}</legend>
          {vaultKeys.map((k) => (
            <label key={k.key.id} className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={selected.includes(k.key.id)}
                onChange={(e) => setSelected((s) => e.target.checked ? [...s, k.key.id] : s.filter((id) => id !== k.key.id))}
              />
              {k.key.name}
            </label>
          ))}
        </fieldset>
        {diskKeys.length > 0 && (
          <fieldset className="flex flex-col gap-1">
            <legend>{t("provision.diskKeys")}</legend>
            {diskKeys.map((k) => (
              <label key={k.key.id} className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={diskSelected.includes(k.key.id)}
                  onChange={(e) => setDiskSelected((s) => e.target.checked ? [...s, k.key.id] : s.filter((id) => id !== k.key.id))}
                />
                {k.key.name}
              </label>
            ))}
          </fieldset>
        )}
        {needsKeyPassphrase && <label>{t("provision.keyPassphrase")}<input type="password" autoComplete="off" className={field} value={keyPassphrase} onChange={(e) => setKeyPassphrase(e.target.value)} /></label>}
        <label>{t("provision.password")}<input required type="password" autoComplete="off" className={field} value={password} onChange={(e) => setPassword(e.target.value)} /></label>
        <label className="flex items-center gap-2">
          <input type="checkbox" checked={disable} onChange={(e) => setDisable(e.target.checked)} />
          {t("provision.disablePasswordAuth")}
        </label>
        {disable && <p role="alert" className="text-danger">{t("provision.lockoutWarning")}</p>}
        {error !== null && <p role="alert" className="text-danger">{error || t("common.error")}</p>}
        <div className="flex justify-end gap-2">
          <button type="button" onClick={close}>{t("common.cancel")}</button>
          <button type="submit" disabled={busy || (keyIds.length === 0 && diskSelected.length === 0)} className="rounded bg-accent px-3 py-1 text-bg">{t("provision.install")}</button>
        </div>
      </form>
    </div>
  );
}

export default function Connections({ onVaultLocked, onGoToKeys }: { onVaultLocked: () => void; onGoToKeys: () => void }) {
  const { t } = useTranslation();
  const [conns, setConns] = useState<Connection[]>([]);
  const [keys, setKeys] = useState<KeyView[]>([]);
  const [form, setForm] = useState<{ edit: Connection | null } | null>(null);
  const [provision, setProvision] = useState<Connection | null>(null);
  const [provisioned, setProvisioned] = useState<ProvisionOutcome | null>(null);
  const [error, setError] = useState(false);
  const [trust, setTrust] = useState<(TrustRetry & { fp: string; algo: string; key: string }) | null>(null);
  const [mismatch, setMismatch] = useState(false);
  const [unreachable, setUnreachable] = useState<{ message: string; retry: () => Promise<void> } | null>(null);
  const formSync = useRef<((disablePasswordAuth: boolean) => void) | null>(null);
  const bindProvisioned = useCallback((sync: ((disablePasswordAuth: boolean) => void) | null) => {
    formSync.current = sync;
  }, []);

  const handle = useCallback((err: unknown) => {
    if ((err as CommandError)?.code === "vault_locked") onVaultLocked();
    else setError(true);
  }, [onVaultLocked]);

  const refresh = useCallback(() => {
    listConnections().then(setConns).catch(handle);
    listKeys().then(setKeys).catch(handle);
  }, [handle]);
  useEffect(refresh, [refresh]);

  const handleTrustError = useCallback((err: unknown, retry: TrustRetry) => {
    const e = err as CommandError;
    if (e?.code === "host_key_unknown") {
      const [fp, algo, key] = e.message.split(" ");
      setTrust({ ...retry, fp, algo, key });
    } else if (e?.code === "host_key_mismatch") setMismatch(true);
    else handle(err);
  }, [handle]);

  const doConnect = (c: Connection, force = false): Promise<void> => {
    setUnreachable(null);
    const attempt = force ? connectForce(c.id) : connectGuarded(c.id);
    return attempt.catch((err: unknown) => {
      const e = err as CommandError;
      if (e?.code === "unreachable")
        setUnreachable({ message: e.message, retry: () => doConnect(c, true) });
      else handleTrustError(err, { host: c.host, port: c.port, run: () => doConnect(c, force) });
    });
  };
  const accept = async () => {
    if (!trust) return;
    const { host, port, algo, key, run } = trust;
    setTrust(null);
    try {
      await trustHost(host, port, algo, key);
      await run();
    } catch (err) {
      handle(err);
    }
  };
  const openProvision = (c: Connection) => {
    setProvisioned(null);
    setMismatch(false);
    setProvision(c);
  };

  const hasUsableKey = keys.some((k) => k.available);

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">{t("connections.title")}</h2>
        <button className="rounded bg-accent px-3 py-1 text-bg" onClick={() => setForm({ edit: null })}>{t("connections.add")}</button>
      </div>
      {error && <p role="alert" className="text-danger">{t("common.error")}</p>}
      {mismatch && <p role="alert" className="text-danger">{t("trust.mismatch")}</p>}
      {provisioned && (
        <p role="status" className="text-sm">
          {t("provision.installed", { installed: provisioned.installed.join(", "), verified: provisioned.verified.join(", ") })}
          {typeof provisioned.hardening === "object" && ` ${t("provision.skipped", { reason: provisioned.hardening.skipped })}`}
        </p>
      )}
      {trust && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("trust.title")} className="rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("trust.title")}</h3>
            <p>{t("trust.body")}</p>
            <p>{t("trust.fingerprint")}: <code>{trust.fp}</code></p>
            <div className="flex gap-2">
              <button onClick={() => setTrust(null)}>{t("trust.cancel")}</button>
              <button className="rounded bg-accent px-3 py-1 text-bg" onClick={accept}>{t("trust.accept")}</button>
            </div>
          </div>
        </div>
      )}
      {unreachable && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("connections.unreachable")} className="rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("connections.unreachable")}</h3>
            <p>{unreachable.message}</p>
            <div className="flex gap-2">
              <button onClick={() => setUnreachable(null)}>{t("common.cancel")}</button>
              <button className="rounded bg-accent px-3 py-1 text-bg" onClick={() => void unreachable.retry()}>{t("connections.connectAnyway")}</button>
            </div>
          </div>
        </div>
      )}
      {conns.length === 0 && <p className="text-muted">{t("connections.empty")}</p>}
      {groupConnections(conns).map(({ group, items }) => (
        <section key={group ?? ""} className="flex flex-col gap-2">
          {group !== null && <h3 className="text-sm font-semibold text-muted">{group}</h3>}
          <ul className="flex flex-col gap-2">
            {items.map((c) => (
              <ConnectionRow
                key={c.id}
                conn={c}
                canProvision={hasUsableKey}
                onProvision={() => openProvision(c)}
                onConnect={() => { setMismatch(false); void doConnect(c); }}
                onEdit={() => setForm({ edit: c })}
                onRemove={() => { if (window.confirm(t("connections.removeConfirm"))) removeConnection(c.id).then(refresh).catch(handle); }}
                onGoToKeys={onGoToKeys}
              />
            ))}
          </ul>
        </section>
      ))}
      {form && <Form initial={form.edit} keys={keys} onClose={() => setForm(null)} onSaved={() => { setForm(null); refresh(); }} onProvision={openProvision} bindProvisioned={bindProvisioned} onGoToKeys={onGoToKeys} />}
      {provision && (
        <ProvisionPanel
          conn={provision}
          keys={keys}
          onTrustError={handleTrustError}
          onVaultLocked={onVaultLocked}
          onError={handle}
          onClose={() => setProvision(null)}
          onDone={(outcome, disablePasswordAuth) => {
            setProvision(null);
            setProvisioned(outcome);
            formSync.current?.(disablePasswordAuth);
            refresh();
          }}
        />
      )}
    </section>
  );
}
