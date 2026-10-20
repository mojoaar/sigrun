import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { addConnection, connectForce, connectGuarded, importKey, listConnections, listKeys, provisionConnection, removeConnection, setConnectionPassword, trustHost, updateConnection, type CommandError, type Connection, type KeyView, type ProvisionOutcome } from "../api";
import ConnectionRow from "../components/ConnectionRow";
import { useDialogFocus } from "../useDialogFocus";

const field = "h-9 w-full rounded border border-border bg-bg px-2 text-text";

export type TreeNode = { name: string; path: string; items: Connection[]; children: TreeNode[] };

// A group is a `/`-separated path (`PVE/Folder 1` nests one level): same rules
// as tray_menu_model, but sorted case-insensitively for the UI.
export function groupTree(conns: Connection[]): TreeNode[] {
  const roots: TreeNode[] = [];
  const loose: Connection[] = [];
  for (const c of conns) {
    const segments = (c.group ?? "").split("/").map((s) => s.trim()).filter(Boolean);
    if (segments.length === 0) {
      loose.push(c);
      continue;
    }
    let level = roots;
    let path = "";
    let node: TreeNode | null = null;
    for (const segment of segments) {
      path = path ? `${path}/${segment}` : segment;
      node = level.find((n) => n.name === segment) ?? null;
      if (!node) {
        node = { name: segment, path, items: [], children: [] };
        level.push(node);
      }
      level = node.children;
    }
    node!.items.push(c);
  }
  const byLabel = (a: Connection, b: Connection) => a.label.localeCompare(b.label);
  const byName = (a: TreeNode, b: TreeNode) => a.name.localeCompare(b.name);
  const sortNode = (n: TreeNode) => {
    n.items.sort(byLabel);
    n.children.sort(byName);
    n.children.forEach(sortNode);
  };
  roots.sort(byName);
  roots.forEach(sortNode);
  loose.sort(byLabel);
  // Ungrouped connections lead, in a headingless node (empty name/path).
  return [
    ...(loose.length > 0 ? [{ name: "", path: "", items: loose, children: [] }] : []),
    ...roots,
  ];
}

// In-memory over the loaded list: an empty query returns the same array, so a
// cleared field does not rebuild the tree.
export function filterConnections(conns: Connection[], query: string): Connection[] {
  const q = query.trim().toLowerCase();
  if (q === "") return conns;
  return conns.filter((c) =>
    [c.label, c.user, c.host, c.group ?? "", String(c.port)].some((v) => v.toLowerCase().includes(q)),
  );
}

// Every `/`-prefix of every group, so a half-typed path matches its parent too.
export function groupPaths(conns: Connection[]): string[] {
  const paths = new Set<string>();
  for (const c of conns) {
    let path = "";
    for (const segment of (c.group ?? "").split("/").map((s) => s.trim()).filter(Boolean)) {
      path = path ? `${path}/${segment}` : segment;
      paths.add(path);
    }
  }
  return [...paths].sort();
}

export type ConnectionDraft = Omit<Connection, "port"> & { port: string };
export type FieldErrors = Partial<Record<"label" | "host" | "user" | "port", string>>;

// The launcher's set, not a stricter one: terminal::safe_host_bytes
// (crates/sigrun-core/src/terminal.rs:39) accepts exactly this and applies it
// to host and to a non-empty user alike.
const SSH_TOKEN = /^[A-Za-z0-9._-]+$/;
const PORT = /^\d+$/;

export function validateConnection(draft: ConnectionDraft): FieldErrors {
  const errors: FieldErrors = {};
  if (draft.label.trim() === "") errors.label = "connections.invalidLabel";
  if (!SSH_TOKEN.test(draft.host.trim())) errors.host = "connections.invalidHost";
  if (draft.user.trim() !== "" && !SSH_TOKEN.test(draft.user.trim())) errors.user = "connections.invalidUser";
  const port = draft.port.trim();
  if (!PORT.test(port) || Number(port) < 1 || Number(port) > 65535) errors.port = "connections.invalidPort";
  return errors;
}

function Form({ initial, keys, groups, labels, onClose, onSaved, onProvision, bindProvisioned, onGoToKeys, onVaultLocked }: {
  initial: Connection | null;
  keys: KeyView[];
  groups: string[];
  labels: string[];
  onClose: () => void;
  onSaved: () => void;
  onProvision: (c: Connection) => void;
  // Lets the parent push the persisted disable-password-auth flag back in, so a
  // later Save on this still-open form cannot revert it.
  bindProvisioned: (sync: ((disablePasswordAuth: boolean) => void) | null) => void;
  onGoToKeys?: () => void;
  onVaultLocked: () => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<ConnectionDraft>(() => initial ? { ...initial, port: String(initial.port) } : { id: crypto.randomUUID(), label: "", host: "", user: "", port: "22", key_id: null, group: null, tags: [], source: "managed", disable_password_auth: false, has_password: false, active: false });
  // Only ever the freshly typed value: the stored password is never read back.
  const [password, setPassword] = useState("");
  const [removePassword, setRemovePassword] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const formRef = useRef<HTMLFormElement>(null);
  const canProvision = keys.some((k) => k.available);
  // Non-blocking: two connections sharing a label make `connections connect
  // <label>` resolve to whichever the host lists first.
  const duplicateLabel = draft.label.trim() !== "" && labels.includes(draft.label.trim());
  useEffect(() => {
    bindProvisioned((disablePasswordAuth) => setDraft((prev) => ({ ...prev, disable_password_auth: disablePasswordAuth })));
    return () => bindProvisioned(null);
  }, [bindProvisioned]);
  useEffect(() => {
    if (Object.keys(fieldErrors).length === 0) return;
    formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
  }, [fieldErrors]);
  const close = () => {
    setPassword("");
    setRemovePassword(false);
    onClose();
  };
  const dialogRef = useDialogFocus(true, close);
  async function submit(e: FormEvent) {
    e.preventDefault();
    const found = validateConnection(draft);
    setFieldErrors(found);
    if (Object.keys(found).length > 0) return;
    // The launcher takes a number; validation already proved the string parses.
    const connection: Connection = {
      ...draft,
      label: draft.label.trim(),
      host: draft.host.trim(),
      user: draft.user.trim(),
      port: Number(draft.port),
    };
    try {
      // The connection must exist before its password: the id is generated here
      // (`ConnectionsAdd` never returns one).
      await (initial ? updateConnection(connection) : addConnection(connection));
      if (password) await setConnectionPassword(connection.id, password);
      else if (removePassword) await setConnectionPassword(connection.id, null);
      setPassword("");
      setRemovePassword(false);
      onSaved();
    } catch (err) {
      const e = err as CommandError;
      if (e?.code === "vault_locked") onVaultLocked();
      else setError(e?.message ?? "");
    }
  }
  return (
    <div role="dialog" aria-label={t(initial ? "connections.edit" : "connections.add")} ref={dialogRef} className="fixed inset-0 flex items-center justify-center bg-black/50">
      <form ref={formRef} onSubmit={submit} className="flex w-96 flex-col gap-2 rounded border border-border bg-surface p-4">
        <label>{t("connections.label")}<input className={fieldErrors.label ? `${field} border-danger!` : field} aria-invalid={fieldErrors.label ? true : undefined} aria-describedby={fieldErrors.label ? "err-label" : undefined} value={draft.label} onChange={(e) => setDraft({ ...draft, label: e.target.value })} /></label>
        {fieldErrors.label && <p id="err-label" className="text-xs text-danger">{t(fieldErrors.label)}</p>}
        {duplicateLabel && <p role="alert" className="text-xs text-danger">{t("connections.duplicateLabel")}</p>}
        <label>{t("connections.host")}<input className={fieldErrors.host ? `${field} border-danger!` : field} aria-invalid={fieldErrors.host ? true : undefined} aria-describedby={fieldErrors.host ? "err-host" : undefined} value={draft.host} onChange={(e) => setDraft({ ...draft, host: e.target.value })} /></label>
        {fieldErrors.host && <p id="err-host" className="text-xs text-danger">{t(fieldErrors.host)}</p>}
        <label>{t("connections.user")}<input className={fieldErrors.user ? `${field} border-danger!` : field} aria-invalid={fieldErrors.user ? true : undefined} aria-describedby={fieldErrors.user ? "err-user" : undefined} value={draft.user} onChange={(e) => setDraft({ ...draft, user: e.target.value })} /></label>
        {fieldErrors.user && <p id="err-user" className="text-xs text-danger">{t(fieldErrors.user)}</p>}
        <label>{t("connections.port")}<input inputMode="numeric" className={fieldErrors.port ? `${field} border-danger!` : field} aria-invalid={fieldErrors.port ? true : undefined} aria-describedby={fieldErrors.port ? "err-port" : undefined} value={draft.port} onChange={(e) => setDraft((prev) => ({ ...prev, port: e.target.value }))} /></label>
        {fieldErrors.port && <p id="err-port" className="text-xs text-danger">{t(fieldErrors.port)}</p>}
        <label>{t("connections.group")}<input className={field} list="sigrun-groups" value={draft.group ?? ""} onChange={(e) => setDraft({ ...draft, group: e.target.value || null })} />
          <datalist id="sigrun-groups">{groups.map((path) => <option value={path} key={path} />)}</datalist>
        </label>
        <label>{t("connections.key")}
          <select className={field} value={draft.key_id ?? ""} onChange={(e) => setDraft({ ...draft, key_id: e.target.value || null })}>
            <option value=""></option>
            {keys.map((k) => <option key={k.key.id} value={k.key.id}>{k.key.name}</option>)}
          </select>
        </label>
        <label>{t("connections.password")}<input type="password" autoComplete="off" className={field} value={password} onChange={(e) => setPassword(e.target.value)} /></label>
        <p className="text-sm text-muted">{t("connections.passwordHint")}</p>
        {initial?.has_password && (
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={removePassword} onChange={(e) => setRemovePassword(e.target.checked)} />
            {t("connections.removeSavedPassword")}
          </label>
        )}
        {error !== null && <p role="alert" className="text-danger">{error || t("common.error")}</p>}
        <div className="flex items-center justify-end gap-2">
          {initial && (canProvision
            ? <button type="button" className="mr-auto rounded border border-border px-2 py-1 hover:text-accent" onClick={() => onProvision(initial)}>{t("provision.install")}</button>
            : <button type="button" className="mr-auto rounded border border-border px-2 py-1 hover:text-accent" onClick={onGoToKeys}>{t("provision.importKey")}</button>)}
          <button type="button" onClick={close}>{t("common.cancel")}</button>
          <button type="submit" className="rounded bg-accent px-3 py-1 text-bg">{t("common.save")}</button>
        </div>
      </form>
    </div>
  );
}

type TrustRetry = { host: string; port: number; run: () => Promise<void> };

// A connect failure raised outside this view (a tray click), carried in so the
// same dialog the in-window Connect would show can open here.
export type ConnectionError = { id: string; code: string; message: string; host: string; port: number };

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
  const dialogRef = useDialogFocus(true, close);

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
    <div role="dialog" aria-label={t("provision.title")} ref={dialogRef} className="fixed inset-0 flex items-center justify-center bg-black/50">
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

export default function Connections({ onVaultLocked, onGoToKeys, connectionError, onConnectionErrorHandled }: {
  onVaultLocked: () => void;
  onGoToKeys: () => void;
  connectionError?: ConnectionError | null;
  onConnectionErrorHandled?: () => void;
}) {
  const { t } = useTranslation();
  const [conns, setConns] = useState<Connection[]>([]);
  const [query, setQuery] = useState("");
  const [keys, setKeys] = useState<KeyView[]>([]);
  const [form, setForm] = useState<{ edit: Connection | null } | null>(null);
  const [provision, setProvision] = useState<Connection | null>(null);
  const [provisioned, setProvisioned] = useState<ProvisionOutcome | null>(null);
  const [error, setError] = useState(false);
  const [trust, setTrust] = useState<(TrustRetry & { fp: string; algo: string; key: string }) | null>(null);
  const [mismatch, setMismatch] = useState(false);
  const [unreachable, setUnreachable] = useState<{ message: string; retry: () => Promise<void> } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<Connection | null>(null);
  const formSync = useRef<((disablePasswordAuth: boolean) => void) | null>(null);
  const bindProvisioned = useCallback((sync: ((disablePasswordAuth: boolean) => void) | null) => {
    formSync.current = sync;
  }, []);
  const trustRef = useDialogFocus(trust !== null, () => setTrust(null));
  const unreachableRef = useDialogFocus(unreachable !== null, () => setUnreachable(null));
  const removeRef = useDialogFocus(confirmRemove !== null, () => setConfirmRemove(null));

  const handle = useCallback((err: unknown) => {
    if ((err as CommandError)?.code === "vault_locked") onVaultLocked();
    else setError(true);
  }, [onVaultLocked]);

  const refresh = useCallback(() => {
    listConnections().then(setConns).catch(handle);
    listKeys().then(setKeys).catch(handle);
  }, [handle]);
  useEffect(refresh, [refresh]);

  // The host only emits when its live set changes, so the dot clears within a
  // tick of a session ending rather than on the next poll.
  useEffect(() => {
    let unChanged: (() => void) | undefined;
    let gone = false;
    try {
      listen("connections-changed", () => refresh())
        .then((f) => { if (gone) f(); else unChanged = f; })
        .catch((err) => console.warn("sigrun: could not listen for connections-changed", err));
    } catch { /* not running under Tauri */ }
    return () => { gone = true; unChanged?.(); };
  }, [refresh]);

  // A tray connect that failed (untrusted or unreachable host) opens the same
  // dialog the in-window Connect would, then the host clears the carried error.
  // A changed host key fails closed: it shows the mismatch banner, never an
  // accept, so the changed key is not recorded.
  useEffect(() => {
    if (!connectionError) return;
    const { id, code, message, host, port } = connectionError;
    // A stale in-window banner must not sit behind a tray dialog.
    setError(false);
    setMismatch(false);
    if (code === "host_key_unknown") {
      const [fp, algo, key] = message.split(" ");
      setTrust({ host, port, fp, algo, key, run: () => connectGuarded(id) });
    } else if (code === "host_key_mismatch") {
      setMismatch(true);
    } else if (code === "unreachable") {
      setUnreachable({
        message,
        retry: () => {
          setUnreachable(null);
          return connectForce(id).catch(handle);
        },
      });
    }
    onConnectionErrorHandled?.();
  }, [connectionError, onConnectionErrorHandled, handle]);

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
    return attempt.then(() => { refresh(); }).catch((err: unknown) => {
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
  const filtered = filterConnections(conns, query);

  const renderNode = (node: TreeNode) => (
    <section key={node.path} className="flex flex-col gap-2">
      {node.name !== "" && <h3 className="text-sm font-semibold text-muted">{node.name}</h3>}
      {node.items.length > 0 && (
        <ul className="flex flex-col gap-2">
          {node.items.map((c) => (
            <ConnectionRow
              key={c.id}
              conn={c}
              canProvision={hasUsableKey}
              onProvision={() => openProvision(c)}
              onConnect={() => { setMismatch(false); void doConnect(c); }}
              onEdit={() => setForm({ edit: c })}
              onRemove={() => setConfirmRemove(c)}
              onGoToKeys={onGoToKeys}
            />
          ))}
        </ul>
      )}
      {node.children.length > 0 && <div className="flex flex-col gap-2 pl-3">{node.children.map(renderNode)}</div>}
    </section>
  );

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-semibold">{t("connections.title")}</h2>
        <div className="flex w-full max-w-sm items-center gap-2">
          <input
            type="search"
            className={field}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("connections.search")}
            aria-label={t("connections.search")}
          />
          <button className="shrink-0 rounded bg-accent px-3 py-1 text-bg" onClick={() => setForm({ edit: null })}>{t("connections.add")}</button>
        </div>
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
          <div role="dialog" aria-label={t("trust.title")} ref={trustRef} className="rounded border border-border bg-surface p-3">
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
          <div role="dialog" aria-label={t("connections.unreachable")} ref={unreachableRef} className="rounded border border-border bg-surface p-3">
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
      {conns.length > 0 && query.trim() !== "" && filtered.length === 0 && (
        <p className="text-sm text-muted">{t("connections.noMatches")}</p>
      )}
      {groupTree(filtered).map(renderNode)}
      {confirmRemove && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("common.delete")} ref={removeRef} className="rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("connections.removeConfirm")}</h3>
            <div className="flex gap-2">
              <button onClick={() => setConfirmRemove(null)}>{t("common.cancel")}</button>
              <button
                className="rounded bg-accent px-3 py-1 text-bg"
                onClick={() => {
                  const id = confirmRemove.id;
                  setConfirmRemove(null);
                  removeConnection(id).then(refresh).catch(handle);
                }}
              >
                {t("common.delete")}
              </button>
            </div>
          </div>
        </div>
      )}
      {form && <Form initial={form.edit} keys={keys} groups={groupPaths(conns)} labels={conns.filter((x) => x.id !== form.edit?.id).map((x) => x.label.trim())} onClose={() => setForm(null)} onSaved={() => { setForm(null); refresh(); }} onProvision={openProvision} bindProvisioned={bindProvisioned} onGoToKeys={onGoToKeys} onVaultLocked={onVaultLocked} />}
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
