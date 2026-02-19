// ── Waveform constants ────────────────────────────────────────────────────────
// 16 bars gives a dense-but-readable spectrum at the pill width without DOM overhead.
const BAR_COUNT = 16;
const BAR_MIN_SCALE = 0.10;
const BAR_MAX_SCALE = 1.0;
// Per-bar random variation: ±25% target with 40–140 ms retarget timer makes bars feel alive.
const BAR_VARIATION_RANGE = 0.25;
const BAR_VARIATION_RETARGET_MIN_MS = 40;
const BAR_VARIATION_RETARGET_MAX_MS = 140;
// Gaussian weight (σ=0.38) gives center bars ~3× more energy than edge bars — natural shape.
const BAR_GAUSSIAN_SIGMA = 0.38;
// Tiny ambient sine wave keeps bars from being completely frozen during silence.
const BAR_AMBIENT_BREATH = 0.008;
// Mic RMS is typically 0.01–0.15; gain=14 maps normal speech to ~0.15–1.0 range.
const LEVEL_GAIN = 14.0;
// Exponent < 0.5 is heavy compression — even moderate speech pushes bars high.
const LEVEL_EXPONENT = 0.45;
const PROMPT_PROVIDER_ANTHROPIC = "anthropic";
const PROMPT_PROVIDER_OPENAI = "openai";
const PROMPT_MODEL_DEFAULTS = {
  [PROMPT_PROVIDER_ANTHROPIC]: "claude-haiku-4-5",
  [PROMPT_PROVIDER_OPENAI]: "gpt-4o-mini",
};

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function randomBetween(min, max) {
  return min + Math.random() * (max - min);
}

function normalizePromptProvider(provider) {
  return String(provider || "").toLowerCase() === PROMPT_PROVIDER_OPENAI
    ? PROMPT_PROVIDER_OPENAI
    : PROMPT_PROVIDER_ANTHROPIC;
}

function normalizePromptModeConfig(config) {
  const provider = normalizePromptProvider(config?.provider);
  const defaultModel = PROMPT_MODEL_DEFAULTS[provider] || PROMPT_MODEL_DEFAULTS[PROMPT_PROVIDER_ANTHROPIC];
  const model = String(config?.model || "").trim() || defaultModel;
  return {
    enabled: Boolean(config?.enabled),
    provider,
    model,
    api_key: String(config?.api_key || ""),
  };
}

function setPromptModeButtonState(button, enabled) {
  if (!button) return;
  button.classList.toggle("active", enabled);
  button.innerHTML = enabled ? "&#9889;" : "&#9998;";
  button.setAttribute("aria-label", enabled ? "Disable Prompt Mode" : "Enable Prompt Mode");
  button.title = enabled ? "Prompt Mode ON" : "Prompt Mode OFF";
}

let bars = [];
let barStates = [];
let pendingLevel = 0;
let smoothedLevel = 0;
let waveformAnimationFrame = 0;
let lastWaveformTimestamp = 0;
let waveformState = "idle";
let isToggleMode = false;
let widgetActionPending = false;
let promptModeActionPending = false;
let promptModeConfig = {
  enabled: false,
  provider: PROMPT_PROVIDER_ANTHROPIC,
  model: PROMPT_MODEL_DEFAULTS[PROMPT_PROVIDER_ANTHROPIC],
  api_key: "",
};

function initBars(waveformEl) {
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

function resetBars(waveformEl) {
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

function renderLevel(level) {
  // Amplify raw RMS and compress dynamic range so speech fills the bars
  const raw = clamp(Number(level || 0), 0, 1);
  const amplified = Math.pow(clamp(raw * LEVEL_GAIN, 0, 1), LEVEL_EXPONENT);
  pendingLevel = amplified;
}

// Signal chain (per animation frame):
//   raw RMS  →  ×LEVEL_GAIN  →  clamp(0,1)  →  ^LEVEL_EXPONENT (compress)
//   →  smoothing: fast attack (×0.6 when rising), slow decay (×0.10 when falling)
//   →  per-bar Gaussian weight  →  random variation timer (±BAR_VARIATION_RANGE)
//   →  scaleY transform
//
// States:
//   recording  — live signal drives bar heights
//   processing — gentle sine-wave breath pulse (no mic input)
//   idle       — minimal ambient movement keeps the widget from looking frozen
function animateWaveform(waveformEl, timestamp) {
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

  waveformAnimationFrame = window.requestAnimationFrame((ts) => animateWaveform(waveformEl, ts));
}

function startWaveformAnimation(waveformEl) {
  if (waveformAnimationFrame) return;
  lastWaveformTimestamp = 0;
  waveformAnimationFrame = window.requestAnimationFrame((ts) => animateWaveform(waveformEl, ts));
}

function stopWaveformAnimation() {
  if (!waveformAnimationFrame) return;
  window.cancelAnimationFrame(waveformAnimationFrame);
  waveformAnimationFrame = 0;
  lastWaveformTimestamp = 0;
}

function setWidgetButtonsEnabled(stopBtn, cancelBtn, enabled) {
  stopBtn.disabled = !enabled;
  cancelBtn.disabled = !enabled;
}

function setWidgetStatus(pill, waveformEl, stopBtn, cancelBtn, status, message = "") {
  const nextStatus = status || "idle";
  pill.classList.remove("processing", "structuring", "error");

  if (nextStatus === "recording") {
    waveformState = "recording";
    if (!isToggleMode) {
      pill.classList.remove("toggle-mode");
      pill.classList.add("hold-mode");
      setWidgetButtonsEnabled(stopBtn, cancelBtn, false);
    }
    pill.classList.add("visible");
    startWaveformAnimation(waveformEl);
    return;
  }

  if (nextStatus === "processing" || nextStatus === "structuring") {
    isToggleMode = false;
    waveformState = "processing";
    pendingLevel = 0;
    setWidgetButtonsEnabled(stopBtn, cancelBtn, false);
    pill.classList.add("processing", "visible");
    if (nextStatus === "structuring") {
      pill.classList.add("structuring");
    }
    startWaveformAnimation(waveformEl);
    return;
  }

  if (nextStatus === "error") {
    console.error("Widget status error:", message || "Recording failed");
    waveformState = "idle";
    setWidgetButtonsEnabled(stopBtn, cancelBtn, false);
    stopWaveformAnimation();
    resetBars(waveformEl);
    pill.classList.add("error", "visible");
    window.setTimeout(() => {
      pill.classList.remove("visible", "processing", "error");
    }, 3000);
    return;
  }

  isToggleMode = false;
  waveformState = "idle";
  setWidgetButtonsEnabled(stopBtn, cancelBtn, false);
  stopWaveformAnimation();
  resetBars(waveformEl);
  pill.classList.remove("visible", "processing", "error");
}

async function runWidgetAction(invoke, pill, waveformEl, stopBtn, cancelBtn, commandName) {
  if (widgetActionPending) return;
  widgetActionPending = true;
  setWidgetButtonsEnabled(stopBtn, cancelBtn, false);
  try {
    await invoke(commandName);
  } catch (error) {
    setWidgetStatus(pill, waveformEl, stopBtn, cancelBtn, "error", String(error));
  } finally {
    widgetActionPending = false;
  }
}

async function togglePromptMode(invoke, promptModeBtn) {
  if (promptModeActionPending || !promptModeBtn) return;

  promptModeActionPending = true;
  promptModeBtn.disabled = true;

  const previous = { ...promptModeConfig };
  promptModeConfig.enabled = !promptModeConfig.enabled;
  setPromptModeButtonState(promptModeBtn, promptModeConfig.enabled);

  try {
    const updatedConfig = await invoke("set_prompt_mode", {
      enabled: promptModeConfig.enabled,
      provider: promptModeConfig.provider,
      model: promptModeConfig.model,
      apiKey: promptModeConfig.api_key,
    });
    promptModeConfig = normalizePromptModeConfig(updatedConfig?.prompt_mode);
    setPromptModeButtonState(promptModeBtn, promptModeConfig.enabled);
  } catch (error) {
    promptModeConfig = previous;
    setPromptModeButtonState(promptModeBtn, promptModeConfig.enabled);
    console.error("Failed to toggle Prompt Mode:", error);
  } finally {
    promptModeBtn.disabled = false;
    promptModeActionPending = false;
  }
}

export async function initWidgetView(invoke, listen) {
  const setupEl = document.getElementById("setup");
  const widgetEl = document.getElementById("widget");
  const pill = document.getElementById("recording-pill");
  const waveformEl = document.getElementById("waveform");
  const stopBtn = document.getElementById("stop-btn");
  const cancelBtn = document.getElementById("cancel-btn");
  const promptModeBtn = document.getElementById("prompt-mode-btn");

  setupEl.classList.add("hidden");
  widgetEl.classList.remove("hidden");

  initBars(waveformEl);
  setWidgetStatus(pill, waveformEl, stopBtn, cancelBtn, "idle");

  stopBtn.addEventListener("click", () =>
    runWidgetAction(invoke, pill, waveformEl, stopBtn, cancelBtn, "stop_recording"),
  );
  cancelBtn.addEventListener("click", () =>
    runWidgetAction(invoke, pill, waveformEl, stopBtn, cancelBtn, "cancel_recording"),
  );
  if (promptModeBtn) {
    promptModeBtn.addEventListener("click", () => togglePromptMode(invoke, promptModeBtn));
  }

  await Promise.all([
    listen("audio_level", (event) => {
      renderLevel(event.payload || 0);
    }),
    listen("status", (event) => {
      const payload = event.payload || {};
      setWidgetStatus(pill, waveformEl, stopBtn, cancelBtn, payload.status || "idle", payload.message || "");
    }),
    listen("toggle_mode_active", () => {
      isToggleMode = true;
      pill.classList.remove("hold-mode");
      pill.classList.add("toggle-mode");
      setWidgetButtonsEnabled(stopBtn, cancelBtn, true);
      stopBtn.tabIndex = 0;
      cancelBtn.tabIndex = 0;
    }),
    listen("model_ready", () => {
      console.log("Model pre-loaded and ready");
    }),
  ]);

  try {
    const config = await invoke("get_config");
    promptModeConfig = normalizePromptModeConfig(config?.prompt_mode);
    setPromptModeButtonState(promptModeBtn, promptModeConfig.enabled);
  } catch (error) {
    console.error("Failed to load Prompt Mode config:", error);
    promptModeConfig = normalizePromptModeConfig();
    setPromptModeButtonState(promptModeBtn, promptModeConfig.enabled);
  }

  try {
    const currentStatus = await invoke("get_status");
    setWidgetStatus(pill, waveformEl, stopBtn, cancelBtn, currentStatus || "idle");
  } catch (error) {
    console.error("Failed to sync widget status:", error);
  }
}
