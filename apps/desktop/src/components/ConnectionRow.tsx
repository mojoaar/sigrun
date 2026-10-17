import { useTranslation } from "react-i18next";
import type { Connection } from "../api";

type Props = { conn: Connection; canProvision: boolean; onProvision: () => void; onConnect: () => void; onEdit: () => void; onRemove: () => void; onGoToKeys?: () => void };

export default function ConnectionRow({ conn, canProvision, onProvision, onConnect, onEdit, onRemove }: Props) {
  const { t } = useTranslation();
  const btn = "rounded border border-border px-2 py-1 hover:text-accent";
  return (
    <li className="flex items-center justify-between gap-4 rounded border border-border bg-surface p-3">
      <div>
        <div className="font-medium">
          {conn.label}
          {conn.group?.trim() && <span className="rounded border border-border px-1 text-xs text-muted">{conn.group}</span>}
        </div>
        <div className="text-sm text-muted">{conn.user}@{conn.host}:{conn.port}</div>
        {conn.source === "discovered" && <span className="text-xs text-accent">{t("connections.discovered")}</span>}
      </div>
      <div className="flex items-center gap-2">
        <button className={btn} onClick={onConnect}>{t("connections.connect")}</button>
        {canProvision
          ? <button className={btn} onClick={onProvision}>{t("provision.install")}</button>
          : <span className="text-sm text-muted">{t("provision.importFirst")}</span>}
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
