import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./i18n";
import "./index.css";
import { setWindowTheme } from "./api";
import { applyTheme, DEFAULT_THEME, readModeOverride, resolveMode } from "./themes";

const mode = readModeOverride();
applyTheme(DEFAULT_THEME, resolveMode(mode));
// Match the native window to the stored Light/Dark override as soon as the
// host answers, so the traffic lights are legible against the themed header.
setWindowTheme(mode ?? "system").catch((err) => console.warn("sigrun: could not set the window theme", err));

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
