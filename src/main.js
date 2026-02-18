const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const view = new URLSearchParams(window.location.search).get("view") || "main";

const setupEl = document.getElementById("setup");
const widgetEl = document.getElementById("widget");
const setupMessageEl = document.getElementById("setup-message");
const downloadBtn = document.getElementById("download-btn");
const progressWrap = document.getElementById("progress-wrap");
const progressEl = document.getElementById("progress");
const progressTextEl = document.getElementById("progress-text");
const modeStatusEl = document.getElementById("mode-status");
const hotkeyHintEl = document.getElementById("hotkey-hint");
const modeButtons = Array.from(document.querySelectorAll(".mode-btn"));

const pill = document.getElementById("recording-pill");
const statusText = document.getElementById("status-text");
const waveformEl = document.getElementById("waveform");
const stopBtn = document.getElementById("stop-btn");
const cancelBtn = document.getElementById("cancel-btn");

let bars = [];
let currentMode = "hold";
let widgetActionPending = false;

function normalizeMode(mode) {
  return mode === "toggle" ? "toggle" : "hold";
}

function applyModeUI(mode, saved = true) {
  const normalized = normalizeMode(mode);
  const toggleMessage =
    normalized === "toggle"
      ? "Recording mode: Toggle recording. Press hotkey once to start, once to stop."
      : "Recording mode: Hold to talk. Hold hotkey to record, release to stop.";

  modeStatusEl.textContent = saved ? toggleMessage : "Saving recording mode...";
  hotkeyHintEl.textContent =
    normalized === "toggle"
      ? "Hotkey: Ctrl+Shift+Space (toggle on/off)"
      : "Hotkey: Ctrl+Shift+Space (hold to record)";

  modeButtons.forEach((button) => {
    const isActive = button.dataset.mode === normalized;
    button.classList.toggle("active", isActive);
    button.setAttribute("aria-checked", String(isActive));
  });
}

async function saveMode(nextMode) {
  const normalized = normalizeMode(nextMode);
  if (normalized === currentMode) {
    return;
  }

  const previousMode = currentMode;
  currentMode = normalized;
  applyModeUI(currentMode, false);

  try {
    await invoke("set_recording_mode", { mode: currentMode });
    applyModeUI(currentMode, true);
  } catch (error) {
    currentMode = previousMode;
    applyModeUI(currentMode, true);
    modeStatusEl.textContent = `Could not save mode: ${error}`;
  }
}

async function loadConfig() {
  try {
    const config = await invoke("get_config");
    currentMode = normalizeMode(config?.general?.mode || "hold");
  } catch (error) {
    currentMode = "hold";
    modeStatusEl.textContent = `Config load failed: ${error}`;
  }

  applyModeUI(currentMode, true);
}

function initBars() {
  waveformEl.innerHTML = "";
  bars = Array.from({ length: 11 }, () => {
    const bar = document.createElement("span");
    bar.className = "bar";
    waveformEl.appendChild(bar);
    return bar;
  });
}

function resetBars() {
  bars.forEach((bar) => {
    bar.style.transform = "scaleY(0.22)";
  });
}

let pendingLevel = 0;
let rafScheduled = false;

function renderLevel(level) {
  pendingLevel = level;
  if (!rafScheduled) {
    rafScheduled = true;
    requestAnimationFrame(() => {
      rafScheduled = false;
      const scaled = Math.max(0.03, Math.min(1, Number(pendingLevel || 0) * 2.6));
      bars.forEach((bar, index) => {
        const phase = Math.sin((index + 1) * 1.1) * 0.14;
        const noise = (index % 4) * 0.06;
        const height = 4 + Math.round((scaled + 0.16 + phase + noise) * 18);
        bar.style.transform = `scaleY(${Math.max(4, height) / 18})`;
      });
    });
  }
}

function setWidgetButtonsEnabled(enabled) {
  stopBtn.disabled = !enabled;
  cancelBtn.disabled = !enabled;
}

function setWidgetStatus(status, message = "") {
  const nextStatus = status || "idle";
  pill.classList.remove("processing", "error");

  if (nextStatus === "recording") {
    statusText.textContent = "REC";
    setWidgetButtonsEnabled(true);
    pill.classList.add("visible");
    return;
  }

  if (nextStatus === "processing") {
    statusText.textContent = "PROC";
    setWidgetButtonsEnabled(false);
    resetBars();
    pill.classList.add("processing", "visible");
    return;
  }

  if (nextStatus === "error") {
    console.error("Widget status error:", message || "Recording failed");
    statusText.textContent = "ERR";
    setWidgetButtonsEnabled(false);
    resetBars();
    pill.classList.add("error", "visible");
    window.setTimeout(() => {
      pill.classList.remove("visible", "processing", "error");
    }, 1200);
    return;
  }

  setWidgetButtonsEnabled(false);
  resetBars();
  pill.classList.remove("visible", "processing", "error");
}

async function initSetupView() {
  setupEl.classList.remove("hidden");
  widgetEl.classList.add("hidden");

  const [, modelResult] = await Promise.all([
    loadConfig(),
    invoke("check_model").catch((error) => ({ error })),
  ]);

  modeButtons.forEach((button) => {
    button.addEventListener("click", () => {
      saveMode(button.dataset.mode || "hold");
    });
  });

  if (modelResult && modelResult.error) {
    setupMessageEl.textContent = `Error checking model: ${modelResult.error}`;
  } else if (modelResult && modelResult.exists) {
    setupMessageEl.textContent = "Model ready. Close this window and use the hotkey.";
    downloadBtn.classList.add("hidden");
  } else if (modelResult) {
    setupMessageEl.textContent = `Model missing at ${modelResult.path}`;
    downloadBtn.classList.remove("hidden");
  }

  downloadBtn.addEventListener("click", async () => {
    downloadBtn.disabled = true;
    progressWrap.classList.remove("hidden");
    setupMessageEl.textContent = "Downloading model...";

    try {
      await invoke("download_model");
      setupMessageEl.textContent = "Model downloaded. Ready to transcribe.";
      downloadBtn.classList.add("hidden");
    } catch (error) {
      setupMessageEl.textContent = `Download failed: ${error}`;
      downloadBtn.disabled = false;
    }
  });

  await listen("model_download_progress", (event) => {
    const progress = Number(event.payload || 0);
    const pct = Math.round(progress * 100);
    progressEl.style.width = `${pct}%`;
    progressTextEl.textContent = `${pct}%`;
  });
}

async function runWidgetAction(commandName) {
  if (widgetActionPending) {
    return;
  }

  widgetActionPending = true;
  setWidgetButtonsEnabled(false);

  try {
    await invoke(commandName);
  } catch (error) {
    setWidgetStatus("error", String(error));
  } finally {
    widgetActionPending = false;
  }
}

async function initWidgetView() {
  setupEl.classList.add("hidden");
  widgetEl.classList.remove("hidden");

  initBars();
  setWidgetStatus("idle");

  stopBtn.addEventListener("click", () => runWidgetAction("stop_recording"));
  cancelBtn.addEventListener("click", () => runWidgetAction("cancel_recording"));

  await Promise.all([
    listen("audio_level", (event) => {
      renderLevel(event.payload || 0);
    }),
    listen("status", (event) => {
      const payload = event.payload || {};
      setWidgetStatus(payload.status || "idle", payload.message || "");
    }),
  ]);

  try {
    const currentStatus = await invoke("get_status");
    setWidgetStatus(currentStatus || "idle");
  } catch (error) {
    console.error("Failed to sync widget status:", error);
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  document.body.classList.add(view === "widget" ? "view-widget" : "view-setup");

  if (view === "widget") {
    await initWidgetView();
  } else {
    await initSetupView();
  }
});
