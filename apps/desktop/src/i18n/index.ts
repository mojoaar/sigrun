import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import en from "./en.json";
import da from "./da.json";

i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, da: { translation: da } },
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export type Language = "en" | "da";

export async function setLanguage(lng: Language): Promise<void> {
  try {
    const res = await invoke<unknown>("get_settings");
    const raw = typeof res === "string" ? res : (res as { value: string }).value;
    const settings = JSON.parse(raw);
    await invoke("save_settings", { settings: { ...settings, language: lng } });
  } catch {
    // No Tauri runtime (tests, browser) or persistence failed: still switch the UI.
  }
  await i18n.changeLanguage(lng);
}

export default i18n;
