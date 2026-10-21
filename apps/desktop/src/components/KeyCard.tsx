import { useTranslation } from "react-i18next";
import type { KeyView } from "../api";

export default function KeyCard({ view, onRemove, onAdd }: { view: KeyView; onRemove: () => void; onAdd?: () => void }) {
  const { t } = useTranslation();
  const k = view.key;
  return (
    <li className="flex h-16 items-center justify-between gap-4 rounded border border-border bg-surface p-3">
      <div className={`min-w-0 ${view.available ? "" : "opacity-60"}`}>
        <div className="truncate font-medium">{k.name} <span className="text-sm text-muted">{k.algorithm}</span></div>
        <div className="truncate font-mono text-xs text-muted" title={k.fingerprint}>{k.fingerprint}</div>
        <span className="text-xs text-accent">{t(k.source === "disk" ? "keys.source.disk" : "keys.source.vault")}</span>
      </div>
      <div className="flex shrink-0 gap-2">
        {onAdd && <button className="rounded border border-border px-2 py-1 hover:text-accent" onClick={onAdd}>{t("keys.addToVault")}</button>}
        <button className="rounded border border-border px-2 py-1 text-danger" onClick={onRemove}>{t("common.delete")}</button>
      </div>
    </li>
  );
}
