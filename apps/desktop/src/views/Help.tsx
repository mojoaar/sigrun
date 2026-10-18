import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getStatus, openExternal } from "../api";

const CLI = ["host", "vaultUnlock", "connectionsAdd", "connectionsConnect", "connectionsPassword"] as const;

const LINKS = [
  { url: "https://johansen.foo", label: "help.about.website" },
  { url: "https://github.com/mojoaar/sigrun", label: "help.about.github" },
];

export default function Help() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => { getStatus().then((s) => setVersion(s.version)).catch(() => {}); }, []);

  return (
    <section className="flex max-w-2xl flex-col gap-4">
      <h2 className="text-lg font-semibold">{t("help.title")}</h2>
      <p className="text-muted">{t("help.intro")}</p>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.quickStart.title")}</h3>
        <ol className="ml-5 list-decimal text-muted">
          <li>{t("help.quickStart.vault")}</li>
          <li>{t("help.quickStart.key")}</li>
          <li>{t("help.quickStart.connection")}</li>
          <li>{t("help.quickStart.connect")}</li>
        </ol>
        <p className="text-muted">{t("help.quickStart.provision")}</p>
      </section>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.groups.title")}</h3>
        <p className="text-muted">{t("help.groups.nesting")}</p>
        <p className="text-muted">{t("help.groups.tray")}</p>
      </section>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.keys.title")}</h3>
        <p className="text-muted">{t("help.keys.vault")}</p>
        <p className="text-muted">{t("help.keys.disk")}</p>
      </section>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.settings.title")}</h3>
        <p className="text-muted">{t("help.settings.summary")}</p>
      </section>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.cli.title")}</h3>
        <dl className="flex flex-col gap-1">
          {CLI.map((k) => (
            <div key={k} className="flex gap-2">
              <dt><code className="rounded bg-surface px-1">{t(`help.cli.${k}.cmd`)}</code></dt>
              <dd className="text-muted">{t(`help.cli.${k}.desc`)}</dd>
            </div>
          ))}
        </dl>
      </section>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.data.title")}</h3>
        <p className="text-muted">{t("help.data.dir")}</p>
        <p className="text-muted">{t("help.data.ssh")}</p>
      </section>

      <section className="flex flex-col gap-1">
        <h3 className="font-semibold">{t("help.about.title")}</h3>
        {version && <p className="text-muted">{t("help.about.version", { version })}</p>}
        <p className="text-muted">{t("help.about.author")}</p>
        <div className="flex gap-3">
          {LINKS.map(({ url, label }) => (
            <button key={url} type="button" className="text-accent underline" onClick={() => { void openExternal(url).catch(() => {}); }}>
              {t(label)}
            </button>
          ))}
        </div>
        <p className="text-muted">{t("help.about.license")}</p>
      </section>
    </section>
  );
}
