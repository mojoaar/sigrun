import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./en.json";
import da from "./da.json";
import de from "./de.json";
import fr from "./fr.json";
import es from "./es.json";
import sv from "./sv.json";
import nb from "./nb.json";

export type Language = "en" | "da" | "de" | "fr" | "es" | "sv" | "nb";

// Native names, never translated. A language task adds its entry with its bundle.
export const LANGUAGES: { code: Language; name: string }[] = [
  { code: "en", name: "English" },
  { code: "da", name: "Dansk" },
  { code: "de", name: "Deutsch" },
  { code: "fr", name: "Français" },
  { code: "es", name: "Español" },
  { code: "sv", name: "Svenska" },
  { code: "nb", name: "Norsk bokmål" },
];

export function isLanguage(value: string): value is Language {
  return LANGUAGES.some((l) => l.code === value);
}

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    da: { translation: da },
    de: { translation: de },
    fr: { translation: fr },
    es: { translation: es },
    sv: { translation: sv },
    nb: { translation: nb },
  },
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

// One owner for <html lang>, so the persisted startup path keeps it in step too.
i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = isLanguage(lng) ? lng : "en";
});

export async function setLanguage(lng: string): Promise<void> {
  const code = isLanguage(lng) ? lng : "en";
  await i18n.changeLanguage(code);
}

export default i18n;
