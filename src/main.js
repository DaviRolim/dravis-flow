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
let waveformState = "idle";
const BAR_COUNT = 26;
const BAR_MIN_SCALE = 0.18;
const BAR_MAX_SCALE = 1.34;
const BAR_VARIATION_RANGE = 0.18;
const BAR_VARIATION_RETARGET_MIN_MS = 80;
const BAR_VARIATION_RETARGET_MAX_MS = 220;
const BAR_GAUSSIAN_SIGMA = 0.38;
const BAR_AMBIENT_BREATH = 0.038;

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function randomBetween(min, max) {
  return min + Math.random() * (max - min);
}

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

const MODEL_SIZES = {
  "base.en": "142 MB",
  "small.en": "466 MB",
  "large-v3-turbo": "809 MB",
};

async function loadConfig() {
  try {
    const config = await invoke("get_config");
    currentMode = normalizeMode(config?.general?.mode || "hold");
    const modelName = config?.model?.name || "base.en";
    const size = MODEL_SIZES[modelName] || "";
    downloadBtn.textContent = size ? `Download Model (${size})` : "Download Model";
  } catch (error) {
    currentMode = "hold";
    modeStatusEl.textContent = `Config load failed: ${error}`;
  }

  applyModeUI(currentMode, true);
}

function initBars() {
  waveformEl.innerHTML = "";
  bars = Array.from({ length: BAR_COUNT }, () => {
    const bar = document.createElement("span");
    bar.className = "bar";
    waveformEl.appendChild(bar);
    return bar;
  });

  const midpoint = (bars.length - 1) / 2 || 1;
  barWeights = bars.map((_, index) => {
    const normalizedOffset = (index - midpoint) / midpoint;
    const gaussian = Math.exp(
      -(normalizedOffset * normalizedOffset) / (2 * BAR_GAUSSIAN_SIGMA * BAR_GAUSSIAN_SIGMA),
    );
    return 0.18 + gaussian * 0.82;
  });
  barResponseRates = bars.map(() => randomBetween(0.3, 0.4));
  barPhaseOffsets = bars.map((_, index) => index * 0.24 + randomBetween(-0.2, 0.2));
  barVariationTargets = bars.map(() => 1);
  barVariations = bars.map(() => 1);
  barVariationTimers = bars.map(() => randomBetween(0, BAR_VARIATION_RETARGET_MAX_MS));
  barScales = Array.from({ length: bars.length }, () => BAR_MIN_SCALE);
  bars.forEach((bar) => {
    bar.style.setProperty("--bar-energy", "0");
    bar.style.transform = `scaleY(${BAR_MIN_SCALE})`;
  });
}

function resetBars() {
  pendingLevel = 0;
  smoothedLevel = 0;
  lastWaveformTimestamp = 0;
  waveformEl.style.setProperty("--waveform-level", "0");
  bars.forEach((bar, index) => {
    barScales[index] = BAR_MIN_SCALE;
    bar.style.transform = `scaleY(${BAR_MIN_SCALE})`;
    bar.style.setProperty("--bar-energy", "0");
  });
}

let pendingLevel = 0;
let smoothedLevel = 0;
let barScales = [];
let barWeights = [];
let barResponseRates = [];
let barPhaseOffsets = [];
let barVariations = [];
let barVariationTargets = [];
let barVariationTimers = [];
let waveformAnimationFrame = 0;
let lastWaveformTimestamp = 0;

function renderLevel(level) {
  pendingLevel = clamp(Number(level || 0), 0, 1);
}

function animateWaveform(timestamp) {
  const deltaMs = lastWaveformTimestamp ? Math.min(80, timestamp - lastWaveformTimestamp) : 16;
  lastWaveformTimestamp = timestamp;
  const seconds = timestamp * 0.001;

  const inputTarget = waveformState === "recording" ? pendingLevel : 0;
  smoothedLevel += (inputTarget - smoothedLevel) * 0.34;
  const normalizedLevel = clamp(smoothedLevel, 0, 1);

  const glowLevel =
    waveformState === "recording"
      ? normalizedLevel
      : waveformState === "processing"
        ? 0.22 + Math.sin(seconds * 0.9) * 0.06
        : 0.08;
  waveformEl.style.setProperty("--waveform-level", String(clamp(glowLevel, 0, 1)));

  bars.forEach((bar, index) => {
    barVariationTimers[index] -= deltaMs;
    if (barVariationTimers[index] <= 0) {
      barVariationTargets[index] = 1 + randomBetween(-BAR_VARIATION_RANGE, BAR_VARIATION_RANGE);
      barVariationTimers[index] = randomBetween(
        BAR_VARIATION_RETARGET_MIN_MS,
        BAR_VARIATION_RETARGET_MAX_MS,
      );
    }

    const currentVariation = barVariations[index] ?? 1;
    const targetVariation = barVariationTargets[index] ?? 1;
    const nextVariation = currentVariation + (targetVariation - currentVariation) * 0.22;
    barVariations[index] = nextVariation;

    const weight = barWeights[index] ?? 0.2;
    const phase = barPhaseOffsets[index] ?? 0;
    const ambientWave = Math.sin(seconds * 1.7 + phase) * 0.5 + 0.5;
    const ambientLift = ambientWave * BAR_AMBIENT_BREATH * weight;

    let lift = 0;
    if (waveformState === "recording") {
      const dynamicLift = normalizedLevel * (0.14 + weight * 0.94);
      lift = dynamicLift + ambientLift * (normalizedLevel < 0.04 ? 0.8 : 0.32);
    } else if (waveformState === "processing") {
      const processingWave = Math.sin(seconds * 0.92 - index * 0.42) * 0.5 + 0.5;
      lift = (0.19 + processingWave * 0.33) * (0.34 + weight * 0.82) + ambientLift * 0.48;
    } else {
      lift = ambientLift * 0.6;
    }

    const variedLift = lift * nextVariation;
    const targetScale = clamp(BAR_MIN_SCALE + variedLift, BAR_MIN_SCALE, BAR_MAX_SCALE);
    const previousScale = barScales[index] ?? BAR_MIN_SCALE;
    const responseRate = barResponseRates[index] ?? 0.34;
    const nextScale = previousScale + (targetScale - previousScale) * responseRate;

    barScales[index] = nextScale;
    bar.style.transform = `scaleY(${nextScale.toFixed(4)})`;

    const barEnergy = clamp((nextScale - BAR_MIN_SCALE) / (BAR_MAX_SCALE - BAR_MIN_SCALE), 0, 1);
    bar.style.setProperty("--bar-energy", barEnergy.toFixed(3));
  });

  waveformAnimationFrame = window.requestAnimationFrame(animateWaveform);
}

function startWaveformAnimation() {
  if (waveformAnimationFrame) {
    return;
  }
  lastWaveformTimestamp = 0;
  waveformAnimationFrame = window.requestAnimationFrame(animateWaveform);
}

function stopWaveformAnimation() {
  if (!waveformAnimationFrame) {
    return;
  }
  window.cancelAnimationFrame(waveformAnimationFrame);
  waveformAnimationFrame = 0;
  lastWaveformTimestamp = 0;
}

function setWidgetButtonsEnabled(enabled) {
  stopBtn.disabled = !enabled;
  cancelBtn.disabled = !enabled;
}

function setWidgetStatus(status, message = "") {
  const nextStatus = status || "idle";
  pill.classList.remove("processing", "error");

  if (nextStatus === "recording") {
    waveformState = "recording";
    setWidgetButtonsEnabled(true);
    pill.classList.add("visible");
    startWaveformAnimation();
    return;
  }

  if (nextStatus === "processing") {
    waveformState = "processing";
    pendingLevel = 0;
    setWidgetButtonsEnabled(false);
    pill.classList.add("processing", "visible");
    startWaveformAnimation();
    return;
  }

  if (nextStatus === "error") {
    console.error("Widget status error:", message || "Recording failed");
    waveformState = "idle";
    setWidgetButtonsEnabled(false);
    stopWaveformAnimation();
    resetBars();
    pill.classList.add("error", "visible");
    window.setTimeout(() => {
      pill.classList.remove("visible", "processing", "error");
    }, 1200);
    return;
  }

  waveformState = "idle";
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
