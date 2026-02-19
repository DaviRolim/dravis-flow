import { initWidgetView } from "./widget.js";
import { initSetupView } from "./setup.js";

function getTauriApi() {
  const tauri = window.__TAURI__;

  const invokeFn =
    (typeof tauri?.core?.invoke === "function" && tauri.core.invoke) ||
    (typeof tauri?.tauri?.invoke === "function" && tauri.tauri.invoke);
  const listenFn = typeof tauri?.event?.listen === "function" ? tauri.event.listen : undefined;

  if (!invokeFn || !listenFn) {
    return null;
  }

  return { invoke: invokeFn, listen: listenFn };
}

const view = new URLSearchParams(window.location.search).get("view") || "main";

window.addEventListener("DOMContentLoaded", async () => {
  const setupEl = document.getElementById("setup");
  const widgetEl = document.getElementById("widget");
  const setupMessageEl = document.getElementById("setup-message");
  const downloadBtn = document.getElementById("download-btn");
  const hotkeyHintEl = document.getElementById("hotkey-hint");

  try {
    const tauriApi = getTauriApi();
    if (!tauriApi) {
      document.body.classList.add("view-setup");
      setupEl.classList.remove("hidden");
      widgetEl.classList.add("hidden");
      setupMessageEl.textContent =
        "Failed to initialize Tauri runtime API. Restart the app to retry.";
      hotkeyHintEl.textContent = "Hotkey unavailable until runtime is restored.";
      downloadBtn.classList.add("hidden");
      return;
    }

    const { invoke, listen } = tauriApi;
    document.body.classList.add(view === "widget" ? "view-widget" : "view-setup");

    if (view === "widget") {
      await initWidgetView(invoke, listen);
    } else {
      await initSetupView(invoke, listen);
    }
  } catch (error) {
    console.error("Frontend initialization failed:", error);
    document.body.classList.add("view-setup");
    setupEl.classList.remove("hidden");
    widgetEl.classList.add("hidden");
    setupMessageEl.textContent = `Frontend initialization failed: ${String(error)}`;
    hotkeyHintEl.textContent = "Hotkey unavailable until this error is resolved.";
    downloadBtn.classList.add("hidden");
  }
});
