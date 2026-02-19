let invoke;
let listen;

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

const setupEl = document.getElementById("setup");
const widgetEl = document.getElementById("widget");
const setupMessageEl = document.getElementById("setup-message");
const downloadBtn = document.getElementById("download-btn");
const progressWrap = document.getElementById("progress-wrap");
const progressEl = document.getElementById("progress");
const progressTextEl = document.getElementById("progress-text");
const modelStatusEl = document.getElementById("model-status");
const hotkeyHintEl = document.getElementById("hotkey-hint");

const pill = document.getElementById("recording-pill");
const waveformEl = document.getElementById("waveform");
const stopBtn = document.getElementById("stop-btn");
const cancelBtn = document.getElementById("cancel-btn");

let bars = [];
let widgetActionPending = false;
let waveformState = "idle";
let isToggleMode = false;
const BAR_COUNT = 16;
const BAR_MIN_SCALE = 0.10;
const BAR_MAX_SCALE = 1.0;
const BAR_VARIATION_RANGE = 0.25;
const BAR_VARIATION_RETARGET_MIN_MS = 40;
const BAR_VARIATION_RETARGET_MAX_MS = 140;
const BAR_GAUSSIAN_SIGMA = 0.38;
const BAR_AMBIENT_BREATH = 0.008;
// RMS from mic is typically 0.01-0.15 — amplify aggressively so speech fills bars
const LEVEL_GAIN = 14.0;
const LEVEL_EXPONENT = 0.45; // heavy compression — even moderate speech pushes bars high

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function randomBetween(min, max) {
  return min + Math.random() * (max - min);
}


const MODEL_SIZES = {
  "base.en": "142 MB",
  "small.en": "466 MB",
  "large-v3-turbo": "809 MB",
};

const MODEL_LABELS = {
  "base.en": "Base",
  "small.en": "Small",
  "large-v3-turbo": "Large Turbo",
};

let currentModel = "base.en";
const modelButtons = Array.from(document.querySelectorAll("#model-switch .mode-btn"));

let vocabWords = [];
let vocabReplacements = [];

const vocabListEl = document.getElementById("vocab-list");
const vocabInputEl = document.getElementById("vocab-input");
const vocabAddBtnEl = document.getElementById("vocab-add-btn");
const replacementsListEl = document.getElementById("replacements-list");
const replacementFromEl = document.getElementById("replacement-from-input");
const replacementToEl = document.getElementById("replacement-to-input");
const replacementAddBtnEl = document.getElementById("replacement-add-btn");

function renderVocabList() {
  if (!vocabListEl) return;
  vocabListEl.innerHTML = "";
  vocabWords.forEach((word, index) => {
    const chip = document.createElement("span");
    chip.className = "tag-chip";
    const text = document.createTextNode(word);
    chip.appendChild(text);
    const removeBtn = document.createElement("button");
    removeBtn.className = "tag-chip-remove";
    removeBtn.type = "button";
    removeBtn.setAttribute("aria-label", `Remove ${word}`);
    removeBtn.innerHTML = "&times;";
    removeBtn.addEventListener("click", () => removeVocabWord(index));
    chip.appendChild(removeBtn);
    vocabListEl.appendChild(chip);
  });
}

function renderReplacementsList() {
  if (!replacementsListEl) return;
  replacementsListEl.innerHTML = "";
  vocabReplacements.forEach((entry, index) => {
    const row = document.createElement("div");
    row.className = "replacement-row";
    const fromSpan = document.createElement("span");
    fromSpan.className = "replacement-from";
    fromSpan.textContent = entry.from;
    const arrowSpan = document.createElement("span");
    arrowSpan.className = "replacement-arrow-sep";
    arrowSpan.textContent = "→";
    const toSpan = document.createElement("span");
    toSpan.className = "replacement-to";
    toSpan.textContent = entry.to;
    const removeBtn = document.createElement("button");
    removeBtn.className = "tag-chip-remove";
    removeBtn.type = "button";
    removeBtn.setAttribute("aria-label", `Remove replacement ${entry.from}`);
    removeBtn.innerHTML = "&times;";
    removeBtn.addEventListener("click", () => removeReplacement(index));
    row.appendChild(fromSpan);
    row.appendChild(arrowSpan);
    row.appendChild(toSpan);
    row.appendChild(removeBtn);
    replacementsListEl.appendChild(row);
  });
}

async function removeVocabWord(index) {
  vocabWords = vocabWords.filter((_, i) => i !== index);
  renderVocabList();
  try {
    await invoke("set_dictionary_words", { words: vocabWords });
  } catch (error) {
    console.error("Failed to save vocabulary:", error);
  }
}

async function addVocabWord(word) {
  const trimmed = word.trim();
  if (!trimmed || vocabWords.includes(trimmed)) return;
  vocabWords = [...vocabWords, trimmed];
  renderVocabList();
  try {
    await invoke("set_dictionary_words", { words: vocabWords });
  } catch (error) {
    console.error("Failed to save vocabulary:", error);
  }
}

async function removeReplacement(index) {
  vocabReplacements = vocabReplacements.filter((_, i) => i !== index);
  renderReplacementsList();
  try {
    await invoke("set_dictionary_replacements", { replacements: vocabReplacements });
  } catch (error) {
    console.error("Failed to save replacements:", error);
  }
}

async function addReplacement(from, to) {
  const fromTrimmed = from.trim();
  const toTrimmed = to.trim();
  if (!fromTrimmed || !toTrimmed) return;
  vocabReplacements = [...vocabReplacements, { from: fromTrimmed, to: toTrimmed }];
  renderReplacementsList();
  try {
    await invoke("set_dictionary_replacements", { replacements: vocabReplacements });
  } catch (error) {
    console.error("Failed to save replacements:", error);
  }
}

function applyModelUI(modelName, confirmed) {
  modelButtons.forEach((btn) => {
    const isActive = btn.dataset.model === modelName;
    btn.classList.toggle("active", isActive);
    btn.setAttribute("aria-checked", String(isActive));
  });
  const label = MODEL_LABELS[modelName] || modelName;
  const size = MODEL_SIZES[modelName] || "";
  modelStatusEl.textContent = confirmed
    ? `Current model: ${label}${size ? ` (${size})` : ""}.`
    : "Saving model...";
  downloadBtn.textContent = size ? `Download Model (${size})` : "Download Model";
}

async function saveModel(modelName) {
  if (modelName === currentModel) return;

  const previous = currentModel;
  currentModel = modelName;
  applyModelUI(modelName, false);

  try {
    const result = await invoke("set_model", { name: modelName });
    applyModelUI(modelName, true);
    // Update model status
    if (result && result.exists) {
      setupMessageEl.textContent = "Model ready. Close this window and use the hotkey.";
      downloadBtn.classList.add("hidden");
    } else {
      setupMessageEl.textContent = `Model not downloaded yet.`;
      downloadBtn.classList.remove("hidden");
    }
  } catch (error) {
    currentModel = previous;
    applyModelUI(previous, true);
    setupMessageEl.textContent = `Could not switch model: ${error}`;
  }
}

async function loadConfig() {
  try {
    const config = await invoke("get_config");
    currentModel = config?.model?.name || "base.en";
    vocabWords = config?.dictionary?.words || [];
    vocabReplacements = config?.dictionary?.replacements || [];
  } catch (error) {
    // ignore — applyModelUI will use the default
  }

  applyModelUI(currentModel, true);
  renderVocabList();
  renderReplacementsList();
}

function initBars() {
  waveformEl.innerHTML = "";
  bars = [];
  barStates = [];

  const midpoint = (BAR_COUNT - 1) / 2 || 1;
  for (let i = 0; i < BAR_COUNT; i++) {
    const el = document.createElement("span");
    el.className = "bar";
    waveformEl.appendChild(el);
    bars.push(el);

    const normalizedOffset = (i - midpoint) / midpoint;
    const gaussian = Math.exp(
      -(normalizedOffset * normalizedOffset) / (2 * BAR_GAUSSIAN_SIGMA * BAR_GAUSSIAN_SIGMA),
    );

    barStates.push({
      scale: BAR_MIN_SCALE,
      weight: 0.18 + gaussian * 0.82,
      responseRate: randomBetween(0.35, 0.55),
      phaseOffset: i * 0.24 + randomBetween(-0.2, 0.2),
      variation: 1,
      variationTarget: 1,
      variationTimer: randomBetween(0, BAR_VARIATION_RETARGET_MAX_MS),
    });

    el.style.setProperty("--bar-energy", "0");
    el.style.transform = `scaleY(${BAR_MIN_SCALE})`;
  }
}

function resetBars() {
  pendingLevel = 0;
  smoothedLevel = 0;
  lastWaveformTimestamp = 0;
  waveformEl.style.setProperty("--waveform-level", "0");
  bars.forEach((bar, index) => {
    if (barStates[index]) barStates[index].scale = BAR_MIN_SCALE;
    bar.style.transform = `scaleY(${BAR_MIN_SCALE})`;
    bar.style.setProperty("--bar-energy", "0");
  });
}

let pendingLevel = 0;
let smoothedLevel = 0;
let barStates = [];
let waveformAnimationFrame = 0;
let lastWaveformTimestamp = 0;

function renderLevel(level) {
  // Amplify raw RMS and compress dynamic range so speech fills the bars
  const raw = clamp(Number(level || 0), 0, 1);
  const amplified = Math.pow(clamp(raw * LEVEL_GAIN, 0, 1), LEVEL_EXPONENT);
  pendingLevel = amplified;
}

function animateWaveform(timestamp) {
  const deltaMs = lastWaveformTimestamp ? Math.min(80, timestamp - lastWaveformTimestamp) : 16;
  lastWaveformTimestamp = timestamp;
  const seconds = timestamp * 0.001;

  const inputTarget = waveformState === "recording" ? pendingLevel : 0;
  // Snappy attack, fast decay — bars collapse quickly when voice stops
  const attackRate = inputTarget > smoothedLevel ? 0.6 : 0.10;
  smoothedLevel += (inputTarget - smoothedLevel) * attackRate;
  const normalizedLevel = clamp(smoothedLevel, 0, 1);

  const glowLevel =
    waveformState === "recording"
      ? normalizedLevel
      : waveformState === "processing"
        ? 0.22 + Math.sin(seconds * 0.9) * 0.06
        : 0.08;
  waveformEl.style.setProperty("--waveform-level", String(clamp(glowLevel, 0, 1)));

  bars.forEach((bar, index) => {
    const state = barStates[index];

    state.variationTimer -= deltaMs;
    if (state.variationTimer <= 0) {
      state.variationTarget = 1 + randomBetween(-BAR_VARIATION_RANGE, BAR_VARIATION_RANGE);
      state.variationTimer = randomBetween(
        BAR_VARIATION_RETARGET_MIN_MS,
        BAR_VARIATION_RETARGET_MAX_MS,
      );
    }

    state.variation += (state.variationTarget - state.variation) * 0.22;

    const ambientWave = Math.sin(seconds * 1.7 + state.phaseOffset) * 0.5 + 0.5;
    const ambientLift = ambientWave * BAR_AMBIENT_BREATH * state.weight;

    let lift = 0;
    if (waveformState === "recording") {
      const dynamicLift = normalizedLevel * (0.3 + state.weight * 0.7);
      // When quiet, almost no ambient movement — bars stay flat
      const quietBlend = normalizedLevel < 0.05 ? 0.15 : 0.05;
      lift = dynamicLift + ambientLift * quietBlend;
    } else if (waveformState === "processing") {
      const pulse = Math.sin(seconds * 2.4) * 0.5 + 0.5;
      lift = pulse * 0.12 * state.weight;
    } else {
      lift = ambientLift * 0.5;
    }

    const variedLift = lift * state.variation;
    const targetScale = clamp(BAR_MIN_SCALE + variedLift, BAR_MIN_SCALE, BAR_MAX_SCALE);
    const nextScale = state.scale + (targetScale - state.scale) * state.responseRate;

    state.scale = nextScale;
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
    // Don't override toggle mode — the toggle_mode_active event may have
    // already switched us to toggle mode before this status event arrives.
    if (!isToggleMode) {
      pill.classList.remove("toggle-mode");
      pill.classList.add("hold-mode");
      setWidgetButtonsEnabled(false);
    }
    pill.classList.add("visible");
    startWaveformAnimation();
    return;
  }

  if (nextStatus === "processing") {
    isToggleMode = false;
    waveformState = "processing";
    pendingLevel = 0;
    setWidgetButtonsEnabled(false);
    pill.classList.add("processing", "visible");
    // Keep animation for gentle breathing pulse
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
    }, 3000);
    return;
  }

  isToggleMode = false;
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

  modelButtons.forEach((button) => {
    button.addEventListener("click", () => {
      saveModel(button.dataset.model || "base.en");
    });
  });

  if (vocabAddBtnEl && vocabInputEl) {
    vocabAddBtnEl.addEventListener("click", () => {
      addVocabWord(vocabInputEl.value);
      vocabInputEl.value = "";
    });
    vocabInputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        addVocabWord(vocabInputEl.value);
        vocabInputEl.value = "";
      }
    });
  }

  if (replacementAddBtnEl && replacementFromEl && replacementToEl) {
    replacementAddBtnEl.addEventListener("click", () => {
      addReplacement(replacementFromEl.value, replacementToEl.value);
      replacementFromEl.value = "";
      replacementToEl.value = "";
    });
    replacementToEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        addReplacement(replacementFromEl.value, replacementToEl.value);
        replacementFromEl.value = "";
        replacementToEl.value = "";
      }
    });
  }

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
    listen("toggle_mode_active", () => {
      isToggleMode = true;
      pill.classList.remove("hold-mode");
      pill.classList.add("toggle-mode");
      setWidgetButtonsEnabled(true);
      stopBtn.tabIndex = 0;
      cancelBtn.tabIndex = 0;
    }),
    listen("model_ready", () => {
      console.log("Model pre-loaded and ready");
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

    ({ invoke, listen } = tauriApi);
    document.body.classList.add(view === "widget" ? "view-widget" : "view-setup");

    if (view === "widget") {
      await initWidgetView();
    } else {
      await initSetupView();
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
