import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { vaultUnlock, vaultInit, type CommandError } from "../api";

type Props = { mode?: "unlock" | "create"; notice?: boolean; onDone?: () => void };

export default function VaultGate({ mode = "unlock", notice, onDone }: Props) {
  const { t } = useTranslation();
  const create = mode === "create";
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!pass) return setError(t("vault.passphraseRequired"));
    if (create && pass !== confirm) return setError(t("vault.passphraseMismatch"));
    setBusy(true);
    setError(null);
    try {
      await (create ? vaultInit(pass) : vaultUnlock(pass));
      setPass("");
      setConfirm("");
      onDone?.();
    } catch (err) {
      const code = (err as CommandError)?.code;
      setError(code === "invalid_passphrase" || code === "vault_locked" ? t("vault.wrongPassphrase") : t("common.error"));
    } finally {
      setBusy(false);
    }
  }

  const field = "w-full rounded border border-border bg-surface px-3 py-2 text-text";
  return (
    <form onSubmit={submit} className="mx-auto mt-24 flex max-w-sm flex-col gap-3">
      <h2 className="text-xl font-semibold">{t(create ? "vault.uninitialized.title" : "vault.locked.title")}</h2>
      <p className="text-muted">{t(create ? "vault.uninitialized.body" : "vault.locked.body")}</p>
      {notice && <p role="status" className="text-accent">{t("vault.locked.title")}</p>}
      <input type="password" aria-label={t("vault.passphrase")} placeholder={t("vault.passphrase")} className={field} value={pass} onChange={(e) => setPass(e.target.value)} autoFocus />
      {create && (
        <input type="password" aria-label={t("vault.passphraseConfirm")} placeholder={t("vault.passphraseConfirm")} className={field} value={confirm} onChange={(e) => setConfirm(e.target.value)} />
      )}
      {error && <p role="alert" className="text-danger">{error}</p>}
      <button type="submit" disabled={busy} className="rounded bg-accent px-3 py-2 text-bg">
        {busy ? t("vault.unlocking") : t(create ? "vault.create" : "vault.unlock")}
      </button>
    </form>
  );
}
