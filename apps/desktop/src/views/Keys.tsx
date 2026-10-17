import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { generateKey, importKey, listKeys, removeKey, type CommandError, type KeyAlgorithm, type KeyView } from "../api";
import KeyCard from "../components/KeyCard";

const field = "rounded border border-border bg-bg px-2 py-1 text-text";

export default function Keys({ onVaultLocked }: { onVaultLocked: () => void }) {
  const { t } = useTranslation();
  const [keys, setKeys] = useState<KeyView[]>([]);
  const [error, setError] = useState(false);
  const [gen, setGen] = useState({ name: "", algorithm: "ed25519" as KeyAlgorithm, passphrase: "" });
  const [imp, setImp] = useState({ path: "", name: "", passphrase: "" });

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
          <KeyCard key={k.key.id} view={k} onRemove={() => { if (window.confirm(t("keys.removeConfirm"))) run(removeKey(k.key.id)); }} />
        ))}
      </ul>
      <form onSubmit={onGen} className="flex flex-wrap items-end gap-2">
        <input required aria-label={t("keys.name")} placeholder={t("keys.name")} className={field} value={gen.name} onChange={(e) => setGen({ ...gen, name: e.target.value })} />
        <select aria-label={t("keys.algorithm")} className={field} value={gen.algorithm} onChange={(e) => setGen({ ...gen, algorithm: e.target.value as KeyAlgorithm })}>
          <option value="ed25519">ed25519</option>
          <option value="rsa">rsa</option>
          <option value="ecdsa">ecdsa</option>
        </select>
        <input type="password" aria-label={t("keys.passphrase")} placeholder={t("keys.passphrase")} className={field} value={gen.passphrase} onChange={(e) => setGen({ ...gen, passphrase: e.target.value })} />
        <button type="submit" className="rounded bg-accent px-3 py-1 text-bg">{t("keys.generate")}</button>
      </form>
      <form onSubmit={onImp} className="flex flex-wrap items-end gap-2">
        <input required aria-label={t("keys.path")} placeholder={t("keys.path")} className={field} value={imp.path} onChange={(e) => setImp({ ...imp, path: e.target.value })} />
        <input required aria-label={t("keys.name")} placeholder={t("keys.name")} className={field} value={imp.name} onChange={(e) => setImp({ ...imp, name: e.target.value })} />
        <input type="password" aria-label={t("keys.passphrase")} placeholder={t("keys.passphrase")} className={field} value={imp.passphrase} onChange={(e) => setImp({ ...imp, passphrase: e.target.value })} />
        <button type="submit" className="rounded bg-accent px-3 py-1 text-bg">{t("keys.import")}</button>
      </form>
    </section>
  );
}
