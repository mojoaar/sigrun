import { useTranslation } from "react-i18next";
import type { Connection } from "../api";

type Props = { conn: Connection; onConnect: () => void; onEdit: () => void; onRemove: () => void };

export default function ConnectionRow({ conn, onConnect, onEdit, onRemove }: Props) {
  const { t } = useTranslation();
  const btn = "rounded border border-border px-2 py-1 hover:text-accent";
  return (
    <li className="flex items-center justify-between gap-4 rounded border border-border bg-surface p-3">
      <div>
        <div className="font-medium">{conn.label}</div>
        <div className="text-sm text-muted">{conn.user}@{conn.host}:{conn.port}</div>
        {conn.source === "discovered" && <span className="text-xs text-accent">{t("connections.discovered")}</span>}
      </div>
      <div className="flex gap-2">
        <button className={btn} onClick={onConnect}>{t("connections.connect")}</button>
        {conn.source === "managed" && (
          <>
            <button className={btn} onClick={onEdit}>{t("connections.edit")}</button>
            <button className={`${btn} text-danger`} onClick={onRemove}>{t("common.delete")}</button>
          </>
        )}
      </div>
    </li>
  );
}
