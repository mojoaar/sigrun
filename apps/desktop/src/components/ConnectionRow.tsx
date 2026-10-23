import { useTranslation } from "react-i18next";
import type { Connection } from "../api";

type Props = {
  conn: Connection;
  credentialMissing: boolean;
  credentialUser?: string;
  keyName?: string;
  canProvision: boolean;
  copied?: boolean;
  onProvision: () => void;
  onConnect: () => void;
  onCopyCommand?: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onGoToKeys?: () => void;
};

export default function ConnectionRow({ conn, credentialMissing, credentialUser, keyName, canProvision, copied, onProvision, onConnect, onCopyCommand, onEdit, onRemove, onGoToKeys }: Props) {
  const { t } = useTranslation();
  const btn = "rounded border border-border px-2 py-1 hover:text-accent";
  // A credential-mode connection has no user of its own; the credential holds it.
  const user = conn.user || credentialUser;
  return (
    <li className="flex items-center justify-between gap-4 rounded border border-border bg-surface p-3">
      <div>
        <div className="flex items-center gap-2 font-medium">
          {conn.active && (
            <span
              role="img"
              aria-label={t("connections.active")}
              className="h-2 w-2 rounded-full bg-accent animate-pulse motion-reduce:animate-none"
            />
          )}
          {conn.label}
          {conn.group?.trim() && <span className="rounded border border-border px-1 text-xs text-muted">{conn.group}</span>}
          {conn.has_password && <span className="rounded border border-border px-1 text-xs text-muted">{t("connections.hasPassword")}</span>}
          {keyName && <span className="rounded border border-border px-1 text-xs text-muted">{t("connections.usesKey", { name: keyName })}</span>}
        </div>
        <div className="text-sm text-muted">{user ? `${user}@` : ""}{conn.host}:{conn.port}</div>
        {credentialMissing && <p className="text-xs text-danger">{t("connections.missingCredential", { id: conn.credential_id })}</p>}
        {conn.source === "discovered" && <span className="text-xs text-accent">{t("connections.discovered")}</span>}
      </div>
      <div className="flex items-center gap-2">
        <button className="rounded bg-accent px-2 py-1 font-medium text-bg" onClick={onConnect}>{t("connections.connect")}</button>
        {onCopyCommand && (
          <button
            type="button"
            className={btn}
            onClick={onCopyCommand}
            title={t(copied ? "connections.commandCopied" : "connections.copyCommand")}
            aria-label={t(copied ? "connections.commandCopied" : "connections.copyCommand")}
          >
            {copied ? "✓" : t("connections.copyCommand")}
          </button>
        )}
        {canProvision
          ? <button className={btn} onClick={onProvision}>{t("provision.install")}</button>
          : <button className={btn} onClick={onGoToKeys}>{t("provision.importKey")}</button>}
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
