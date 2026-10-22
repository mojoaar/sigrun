import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { addCredential, listConnections, listCredentials, listKeys, removeCredential, setCredentialPassword, updateCredential, type CommandError, type Connection, type Credential, type KeyView } from "../api";
import { useDialogFocus } from "../useDialogFocus";

const field = "h-9 w-full rounded border border-border bg-bg px-2 text-text";
const btn = "h-9 shrink-0 rounded bg-accent px-3 text-bg";

// A credential is key, password, or both; the row names whichever it has.
function methodKey(c: Credential): string {
  if (!c.key_id) return "credentials.method.password";
  return c.has_password ? "credentials.method.keyPassword" : "credentials.method.key";
}

type Draft = { label: string; user: string; key_id: string | null; password: string; removePassword: boolean };
type FieldErrors = Partial<Record<"label" | "user" | "key", string>>;

function Form({ initial, keys, onClose, onSaved, onVaultLocked }: {
  initial: Credential | null;
  keys: KeyView[];
  onClose: () => void;
  onSaved: () => void;
  onVaultLocked: () => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<Draft>(() => initial
    ? { label: initial.label, user: initial.user, key_id: initial.key_id, password: "", removePassword: false }
    : { label: "", user: "", key_id: null, password: "", removePassword: false });
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [error, setError] = useState<string | null>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const close = () => { setDraft((d) => ({ ...d, password: "" })); onClose(); };
  const dialogRef = useDialogFocus(true, close);
  useEffect(() => {
    if (Object.keys(fieldErrors).length === 0) return;
    formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
  }, [fieldErrors]);
  async function submit(e: FormEvent) {
    e.preventDefault();
    const found: FieldErrors = {};
    if (draft.label.trim() === "") found.label = "credentials.invalidLabel";
    if (draft.user.trim() === "") found.user = "credentials.invalidUser";
    // A credential with no key is a password-only identity; removing that last
    // secret would leave it unable to authenticate.
    const keepsPassword = draft.password !== "" || (initial?.has_password === true && !draft.removePassword);
    if (draft.key_id === null && !keepsPassword) found.key = "credentials.invalidKey";
    setFieldErrors(found);
    if (Object.keys(found).length > 0) return;
    const credential: Credential = {
      // The host assigns ids; it refuses one supplied by the caller.
      id: initial?.id ?? "",
      label: draft.label.trim(),
      user: draft.user.trim(),
      key_id: draft.key_id,
      has_password: initial?.has_password ?? draft.password !== "",
    };
    try {
      if (!initial) {
        await addCredential(credential, draft.password || null);
      } else {
        // Metadata first: the secret is stored against a row that exists.
        await updateCredential(credential);
        if (draft.password) await setCredentialPassword(credential.id, draft.password);
        else if (draft.removePassword) await setCredentialPassword(credential.id, null);
      }
      setDraft((d) => ({ ...d, password: "" }));
      onSaved();
    } catch (err) {
      const e = err as CommandError;
      if (e?.code === "vault_locked") onVaultLocked();
      else setError(e?.message ?? "");
    }
  }
  return (
    <div role="dialog" aria-label={t(initial ? "credentials.edit" : "credentials.add")} ref={dialogRef} className="fixed inset-0 flex items-center justify-center bg-black/50">
      <form ref={formRef} onSubmit={submit} className="flex w-96 flex-col gap-2 rounded border border-border bg-surface p-4">
        <label>{t("credentials.label")}<input className={fieldErrors.label ? `${field} border-danger!` : field} aria-invalid={fieldErrors.label ? true : undefined} aria-describedby={fieldErrors.label ? "err-label" : undefined} value={draft.label} onChange={(e) => setDraft({ ...draft, label: e.target.value })} /></label>
        {fieldErrors.label && <p id="err-label" className="text-xs text-danger">{t(fieldErrors.label)}</p>}
        <label>{t("credentials.user")}<input className={fieldErrors.user ? `${field} border-danger!` : field} aria-invalid={fieldErrors.user ? true : undefined} aria-describedby={fieldErrors.user ? "err-user" : undefined} value={draft.user} onChange={(e) => setDraft({ ...draft, user: e.target.value })} /></label>
        {fieldErrors.user && <p id="err-user" className="text-xs text-danger">{t(fieldErrors.user)}</p>}
        <label>{t("credentials.key")}
          <select className={field} aria-invalid={fieldErrors.key ? true : undefined} aria-describedby={fieldErrors.key ? "err-key" : undefined} value={draft.key_id ?? ""} onChange={(e) => setDraft({ ...draft, key_id: e.target.value || null })}>
            <option value=""></option>
            {keys.map((k) => <option key={k.key.id} value={k.key.id}>{k.key.name}</option>)}
          </select>
        </label>
        {fieldErrors.key && <p id="err-key" className="text-xs text-danger">{t(fieldErrors.key)}</p>}
        <label>{t("credentials.password")}<input type="password" autoComplete="off" className={field} value={draft.password} onChange={(e) => setDraft({ ...draft, password: e.target.value })} /></label>
        <p className="text-sm text-muted">{t("credentials.passwordHint")}</p>
        {initial?.has_password && (
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={draft.removePassword} onChange={(e) => setDraft({ ...draft, removePassword: e.target.checked })} />
            {t("credentials.removeSavedPassword")}
          </label>
        )}
        {error !== null && <p role="alert" className="text-danger">{error || t("common.error")}</p>}
        <div className="flex justify-end gap-2">
          <button type="button" onClick={close}>{t("common.cancel")}</button>
          <button type="submit" className="rounded bg-accent px-3 py-1 text-bg">{t("common.save")}</button>
        </div>
      </form>
    </div>
  );
}

export default function Credentials({ onVaultLocked }: { onVaultLocked: () => void }) {
  const { t } = useTranslation();
  const [creds, setCreds] = useState<Credential[]>([]);
  const [conns, setConns] = useState<Connection[]>([]);
  const [keys, setKeys] = useState<KeyView[]>([]);
  const [form, setForm] = useState<{ edit: Credential | null } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<Credential | null>(null);
  const [error, setError] = useState<string | null>(null);
  const removeRef = useDialogFocus(confirmRemove !== null, () => setConfirmRemove(null));

  const handle = useCallback((err: unknown) => {
    const e = err as CommandError | undefined;
    if (e?.code === "vault_locked") onVaultLocked();
    else setError(e && e.code !== "internal" ? e.message : "");
  }, [onVaultLocked]);
  const refresh = useCallback(() => {
    // The used-by count is read from the connections, never fetched separately.
    listCredentials().then(setCreds).catch(handle);
    listConnections().then(setConns).catch(handle);
    listKeys().then(setKeys).catch(handle);
  }, [handle]);
  useEffect(refresh, [refresh]);

  const usedByCount = useMemo(() => {
    const counts = new Map<string, number>();
    for (const c of conns) {
      if (c.credential_id) {
        counts.set(c.credential_id, (counts.get(c.credential_id) ?? 0) + 1);
      }
    }
    return counts;
  }, [conns]);

  const usedBy = (id: string) => usedByCount.get(id) ?? 0;
  const rows = [...creds].sort((a, b) => a.label.localeCompare(b.label));

  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-semibold">{t("credentials.title")}</h2>
        <button className="shrink-0 rounded bg-accent px-3 py-1 text-bg" onClick={() => setForm({ edit: null })}>{t("credentials.add")}</button>
      </div>
      {error !== null && <p role="alert" className="text-danger">{error || t("common.error")}</p>}
      {creds.length === 0 && <p className="text-muted">{t("credentials.empty")}</p>}
      <ul className="flex flex-col gap-2">
        {rows.map((c) => (
          <li key={c.id} className="flex items-center justify-between gap-4 rounded border border-border bg-surface p-3">
            <div className="min-w-0">
              <div className="truncate font-medium">{c.label} <span className="text-sm text-muted">{c.user}</span></div>
              <div className="text-sm text-muted">
                <span>{t(methodKey(c))}</span>
                <span className="px-1">·</span>
                <span>{t("credentials.usedBy", { count: usedBy(c.id) })}</span>
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <button className="rounded border border-border px-2 py-1 hover:text-accent" onClick={() => setForm({ edit: c })}>{t("credentials.edit")}</button>
              <button className="rounded border border-border px-2 py-1 text-danger" onClick={() => setConfirmRemove(c)}>{t("common.delete")}</button>
            </div>
          </li>
        ))}
      </ul>
      {form && <Form key={form.edit?.id ?? "new"} initial={form.edit} keys={keys} onClose={() => setForm(null)} onSaved={() => { setForm(null); refresh(); }} onVaultLocked={onVaultLocked} />}
      {confirmRemove && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("credentials.removeConfirm")} ref={removeRef} className="rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("credentials.removeConfirm")}</h3>
            {usedBy(confirmRemove.id) > 0 && (
              <p role="alert" className="text-sm text-danger">{t("credentials.inUse", { count: usedBy(confirmRemove.id) })}</p>
            )}
            <div className="flex gap-2">
              <button className={field} onClick={() => setConfirmRemove(null)}>{t("common.cancel")}</button>
              <button
                className={btn}
                disabled={usedBy(confirmRemove.id) > 0}
                onClick={() => {
                  const id = confirmRemove.id;
                  setConfirmRemove(null);
                  setError(null);
                  removeCredential(id).then(refresh).catch(handle);
                }}
              >
                {t("common.delete")}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
