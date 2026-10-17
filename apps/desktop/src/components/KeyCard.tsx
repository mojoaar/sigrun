import { useTranslation } from "react-i18next";
import type { KeyView } from "../api";

export default function KeyCard({ view, onRemove }: { view: KeyView; onRemove: () => void }) {
  const { t } = useTranslation();
  const k = view.key;
  return (
    <li className="flex items-start justify-between gap-4 rounded border border-border bg-surface p-3">
      <div className={view.available ? "" : "opacity-60"}>
        <div className="font-medium">{k.name} <span className="text-sm text-muted">{k.algorithm}</span></div>
        <div className="break-all font-mono text-xs text-muted">{k.fingerprint}</div>
        <span className="text-xs text-accent">{t(k.source === "disk" ? "keys.source.disk" : "keys.source.vault")}</span>
      </div>
      <button className="rounded border border-border px-2 py-1 text-danger" onClick={onRemove}>{t("common.delete")}</button>
    </li>
  );
}
