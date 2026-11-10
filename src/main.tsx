import React from "react";
import ReactDOM from "react-dom/client";
import { ApplicationEntry } from "./ApplicationEntry";
import { applyThemePreference, readStoredThemePreference } from "./lib/theme";
import { registerPanelCloseHook } from "./lib/transport";
import "virtual:uno.css";
import "./styles.css";

// Apply the last known theme before the first paint so a dark preference never
// flashes the light background on startup.
applyThemePreference(readStoredThemePreference() ?? "system");

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
void registerPanelCloseHook(root).catch((error) => {
  console.error("Unable to register panel close cleanup", error);
});
root.render(
  <React.StrictMode>
    <ApplicationEntry />
  </React.StrictMode>,
);
