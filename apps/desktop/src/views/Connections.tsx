import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { addConnection, connect, listConnections, listKeys, removeConnection, updateConnection, type CommandError, type Connection, type KeyView } from "../api";
import ConnectionRow from "../components/ConnectionRow";

const field = "w-full rounded border border-border bg-bg px-2 py-1 text-text";

function Form({ initial, keys, onClose, onSaved }: { initial: Connection | null; keys: KeyView[]; onClose: () => void; onSaved: () => void }) {
  const { t } = useTranslation();
  const [c, setC] = useState<Connection>(
    initial ?? { id: crypto.randomUUID(), label: "", host: "", user: "", port: 22, key_id: null, group: null, tags: [], source: "managed" },
  );
  const [error, setError] = useState(false);
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
        <div className="flex justify-end gap-2">
          <button type="button" onClick={onClose}>{t("common.cancel")}</button>
          <button type="submit" className="rounded bg-accent px-3 py-1 text-bg">{t("common.save")}</button>
        </div>
      </form>
    </div>
  );
}

export default function Connections({ onVaultLocked }: { onVaultLocked: () => void }) {
  const { t } = useTranslation();
  const [conns, setConns] = useState<Connection[]>([]);
  const [keys, setKeys] = useState<KeyView[]>([]);
  const [form, setForm] = useState<{ edit: Connection | null } | null>(null);
  const [error, setError] = useState(false);

  const handle = useCallback((err: unknown) => {
    if ((err as CommandError)?.code === "vault_locked") onVaultLocked();
    else setError(true);
  }, [onVaultLocked]);

  const refresh = useCallback(() => {
    listConnections().then(setConns).catch(handle);
    listKeys().then(setKeys).catch(handle);
  }, [handle]);
  useEffect(refresh, [refresh]);

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">{t("connections.title")}</h2>
        <button className="rounded bg-accent px-3 py-1 text-bg" onClick={() => setForm({ edit: null })}>{t("connections.add")}</button>
      </div>
      {error && <p role="alert" className="text-danger">{t("common.error")}</p>}
      {conns.length === 0 && <p className="text-muted">{t("connections.empty")}</p>}
      <ul className="flex flex-col gap-2">
        {conns.map((c) => (
          <ConnectionRow
            key={c.id}
            conn={c}
            onConnect={() => connect(c.id, false).catch(handle)}
            onEdit={() => setForm({ edit: c })}
            onRemove={() => { if (window.confirm(t("connections.removeConfirm"))) removeConnection(c.id).then(refresh).catch(handle); }}
          />
        ))}
      </ul>
      {form && <Form initial={form.edit} keys={keys} onClose={() => setForm(null)} onSaved={() => { setForm(null); refresh(); }} />}
    </section>
  );
}
