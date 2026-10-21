import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { generateKey, importKey, listKeys, removeKey, type CommandError, type KeyAlgorithm, type KeyView } from "../api";
import KeyCard from "../components/KeyCard";
import { useDialogFocus } from "../useDialogFocus";

const field = "h-9 rounded border border-border bg-bg px-2 text-text";
const btn = "h-9 shrink-0 rounded bg-accent px-3 text-bg";

export default function Keys({ onVaultLocked }: { onVaultLocked: () => void }) {
  const { t } = useTranslation();
  const [keys, setKeys] = useState<KeyView[]>([]);
  const [error, setError] = useState(false);
  const [gen, setGen] = useState({ name: "", algorithm: "ed25519" as KeyAlgorithm, passphrase: "" });
  const [imp, setImp] = useState({ path: "", name: "", passphrase: "" });
  const [confirmRemove, setConfirmRemove] = useState<KeyView | null>(null);
  const removeRef = useDialogFocus(confirmRemove !== null, () => setConfirmRemove(null));
  const [adding, setAdding] = useState<KeyView | null>(null);
  const [addPassphrase, setAddPassphrase] = useState("");
  const closeAdd = () => { setAdding(null); setAddPassphrase(""); };
  const addRef = useDialogFocus(adding !== null, closeAdd);

  const handle = useCallback((err: unknown) => {
    if ((err as CommandError)?.code === "vault_locked") onVaultLocked();
    else setError(true);
  }, [onVaultLocked]);
  const refresh = useCallback(() => { listKeys().then(setKeys).catch(handle); }, [handle]);
  useEffect(refresh, [refresh]);

  const run = (p: Promise<unknown>) => { setError(false); p.then(refresh).catch(handle); };
  const onGen = (e: FormEvent) => { e.preventDefault(); run(generateKey(gen.name, gen.algorithm, gen.passphrase).then(() => setGen({ ...gen, name: "", passphrase: "" }))); };
  const onImp = (e: FormEvent) => { e.preventDefault(); run(importKey(imp.path, imp.name, imp.passphrase).then(() => setImp({ path: "", name: "", passphrase: "" }))); };

  return (
    <section className="flex flex-col gap-4">
      <h2 className="text-lg font-semibold">{t("keys.title")}</h2>
      {error && <p role="alert" className="text-danger">{t("common.error")}</p>}
      {keys.length === 0 && <p className="text-muted">{t("keys.empty")}</p>}
      <ul className="flex flex-col gap-2">
        {keys.map((k) => (
          <KeyCard
            key={k.key.id}
            view={k}
            onRemove={() => setConfirmRemove(k)}
            onAdd={k.key.source === "disk" && k.key.disk_path ? () => setAdding(k) : undefined}
          />
        ))}
      </ul>
      {adding && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("keys.addToVault")} ref={addRef} className="flex w-80 flex-col gap-2 rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("keys.addToVault")}</h3>
            <p className="text-sm text-muted">{t("keys.addToVaultBody")}</p>
            <input type="password" aria-label={t("keys.passphrase")} placeholder={t("keys.passphrase")} className={field} value={addPassphrase} onChange={(e) => setAddPassphrase(e.target.value)} />
            <div className="flex justify-end gap-2">
              <button className={field} onClick={closeAdd}>{t("common.cancel")}</button>
              <button
                className={btn}
                onClick={() => {
                  const key = adding.key;
                  const passphrase = addPassphrase;
                  closeAdd();
                  if (key.disk_path) run(importKey(key.disk_path, key.name, passphrase));
                }}
              >
                {t("keys.addToVault")}
              </button>
            </div>
          </div>
        </div>
      )}
      {confirmRemove && (
        <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/50">
          <div role="dialog" aria-label={t("keys.removeConfirm")} ref={removeRef} className="rounded border border-border bg-surface p-3">
            <h3 className="font-semibold">{t("keys.removeConfirm")}</h3>
            <div className="flex gap-2">
              <button className={field} onClick={() => setConfirmRemove(null)}>{t("common.cancel")}</button>
              <button
                className={btn}
                onClick={() => {
                  const id = confirmRemove.key.id;
                  setConfirmRemove(null);
                  run(removeKey(id));
                }}
              >
                {t("common.delete")}
              </button>
            </div>
          </div>
        </div>
      )}
      <form onSubmit={onGen} className="flex flex-wrap items-end gap-2">
        <input required aria-label={t("keys.name")} placeholder={t("keys.name")} className={field} value={gen.name} onChange={(e) => setGen({ ...gen, name: e.target.value })} />
        <select aria-label={t("keys.algorithm")} className={field} value={gen.algorithm} onChange={(e) => setGen({ ...gen, algorithm: e.target.value as KeyAlgorithm })}>
          <option value="ed25519">ed25519</option>
          <option value="rsa">rsa</option>
          <option value="ecdsa">ecdsa</option>
        </select>
        <input type="password" aria-label={t("keys.passphrase")} placeholder={t("keys.passphrase")} className={field} value={gen.passphrase} onChange={(e) => setGen({ ...gen, passphrase: e.target.value })} />
        <button type="submit" className={btn}>{t("keys.generate")}</button>
      </form>
      <form onSubmit={onImp} className="flex flex-wrap items-end gap-2">
        <input required aria-label={t("keys.path")} placeholder={t("keys.path")} className={field} value={imp.path} onChange={(e) => setImp({ ...imp, path: e.target.value })} />
        <input required aria-label={t("keys.name")} placeholder={t("keys.name")} className={field} value={imp.name} onChange={(e) => setImp({ ...imp, name: e.target.value })} />
        <input type="password" aria-label={t("keys.passphrase")} placeholder={t("keys.passphrase")} className={field} value={imp.passphrase} onChange={(e) => setImp({ ...imp, passphrase: e.target.value })} />
        <button type="submit" className={btn}>{t("keys.import")}</button>
      </form>
    </section>
  );
}
