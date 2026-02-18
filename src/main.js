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
const waveformEl = document.getElementById("waveform");
const stopBtn = document.getElementById("stop-btn");
const cancelBtn = document.getElementById("cancel-btn");

let bars = [];
let currentMode = "hold";
let widgetActionPending = false;
const BAR_MIN_SCALE = 0.18;
const BAR_MAX_SCALE = 1.34;

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
  barScales = Array.from({ length: bars.length }, () => BAR_MIN_SCALE);
}

function resetBars() {
  pendingLevel = 0;
  smoothedLevel = 0;
  bars.forEach((bar, index) => {
    barScales[index] = BAR_MIN_SCALE;
    bar.style.transform = `scaleY(${BAR_MIN_SCALE})`;
  });
}

let pendingLevel = 0;
let smoothedLevel = 0;
let barScales = [];
let waveformAnimationFrame = 0;

function renderLevel(level) {
  pendingLevel = Math.max(0, Math.min(1, Number(level || 0)));
}

function animateWaveform(timestamp) {
  smoothedLevel += (pendingLevel - smoothedLevel) * 0.18;
  const energy = Math.max(0.04, Math.min(1, smoothedLevel * 2.9));
  const curvedEnergy = 1 - (1 - energy) * (1 - energy);
  const midpoint = (bars.length - 1) / 2 || 1;

  bars.forEach((bar, index) => {
    const distance = Math.abs(index - midpoint) / midpoint;
    const arch = Math.cos(distance * Math.PI * 0.5);
    const inCurve = Math.sin(timestamp * 0.012 + index * 0.62) * 0.13;
    const outCurve = Math.sin(timestamp * 0.018 - distance * 3.1) * 0.09;
    const sway = Math.sin(timestamp * 0.006 + index * 0.41) * 0.05;
    const base = BAR_MIN_SCALE + arch * 0.24;
    const lift = curvedEnergy * (0.42 + arch * 0.62);
    const targetScale = Math.max(
      BAR_MIN_SCALE,
      Math.min(BAR_MAX_SCALE, base + lift + inCurve + outCurve + sway),
    );
    const previousScale = barScales[index] ?? BAR_MIN_SCALE;
    const nextScale = previousScale + (targetScale - previousScale) * 0.34;

    barScales[index] = nextScale;
    bar.style.transform = `scaleY(${nextScale})`;
  });

  waveformAnimationFrame = window.requestAnimationFrame(animateWaveform);
}

function startWaveformAnimation() {
  if (waveformAnimationFrame) {
    return;
  }
  waveformAnimationFrame = window.requestAnimationFrame(animateWaveform);
}

function stopWaveformAnimation() {
  if (!waveformAnimationFrame) {
    return;
  }
  window.cancelAnimationFrame(waveformAnimationFrame);
  waveformAnimationFrame = 0;
}

function setWidgetButtonsEnabled(enabled) {
  stopBtn.disabled = !enabled;
  cancelBtn.disabled = !enabled;
}

function setWidgetStatus(status, message = "") {
  const nextStatus = status || "idle";
  pill.classList.remove("processing", "error");

  if (nextStatus === "recording") {
    setWidgetButtonsEnabled(true);
    pill.classList.add("visible");
    startWaveformAnimation();
    return;
  }

  if (nextStatus === "processing") {
    setWidgetButtonsEnabled(false);
    stopWaveformAnimation();
    resetBars();
    pill.classList.add("processing", "visible");
    return;
  }

  if (nextStatus === "error") {
    console.error("Widget status error:", message || "Recording failed");
    setWidgetButtonsEnabled(false);
    stopWaveformAnimation();
    resetBars();
    pill.classList.add("error", "visible");
    window.setTimeout(() => {
      pill.classList.remove("visible", "processing", "error");
    }, 1200);
    return;
  }

  setWidgetButtonsEnabled(false);
  stopWaveformAnimation();
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
