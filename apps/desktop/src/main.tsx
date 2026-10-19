import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./i18n";
import "./index.css";
import { setWindowTheme } from "./api";
import { applyTheme, DEFAULT_THEME, readModeOverride, resolveMode } from "./themes";

const mode = readModeOverride();
applyTheme(DEFAULT_THEME, resolveMode(mode));
// Point the native window at the stored Light/Dark override through a local
// Tauri command — no host round trip — so the macOS overlay title bar's traffic
// lights are legible against the themed header. Until it lands, the window keeps
// the OS appearance, so a first launch whose stored choice differs from the OS
// shows the OS glyphs for an instant.
setWindowTheme(mode ?? "system").catch((err) => console.warn("sigrun: could not set the window theme", err));

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
