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

const pill = document.getElementById("recording-pill");
const recDot = document.getElementById("rec-dot");
const statusText = document.getElementById("status-text");
const waveformEl = document.getElementById("waveform");

let bars = [];

function initBars() {
  waveformEl.innerHTML = "";
  bars = Array.from({ length: 7 }, () => {
    const bar = document.createElement("span");
    bar.className = "bar";
    waveformEl.appendChild(bar);
    return bar;
  });
}

function renderLevel(level) {
  const scaled = Math.max(0.05, Math.min(1, level * 2.4));
  bars.forEach((bar, i) => {
    const noise = 0.2 + ((i % 3) * 0.12);
    const h = 5 + Math.round((scaled + noise) * 14);
    bar.style.height = `${h}px`;
  });
}

function setWidgetStatus(status, message) {
  if (status === "recording") {
    recDot.classList.remove("processing");
    statusText.textContent = "Recording...";
    pill.classList.add("visible");
  } else if (status === "processing") {
    recDot.classList.add("processing");
    statusText.textContent = "Transcribing...";
    bars.forEach((bar) => {
      bar.style.height = "6px";
    });
    pill.classList.add("visible");
  } else if (status === "error") {
    recDot.classList.add("processing");
    statusText.textContent = message || "Error";
    pill.classList.add("visible");
    setTimeout(() => pill.classList.remove("visible"), 1200);
  } else {
    pill.classList.remove("visible");
  }
}

async function initSetupView() {
  setupEl.classList.remove("hidden");
  widgetEl.classList.add("hidden");

  try {
    const model = await invoke("check_model");
    if (model.exists) {
      setupMessageEl.textContent = "Model ready. You can close this window and use the hotkey.";
      downloadBtn.classList.add("hidden");
      return;
    }

    setupMessageEl.textContent = `Model missing at ${model.path}`;
    downloadBtn.classList.remove("hidden");
  } catch (error) {
    setupMessageEl.textContent = `Error checking model: ${error}`;
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

async function initWidgetView() {
  setupEl.classList.add("hidden");
  widgetEl.classList.remove("hidden");

  initBars();
  setWidgetStatus("idle");

  await listen("audio_level", (event) => {
    renderLevel(Number(event.payload || 0));
  });

  await listen("status", (event) => {
    const payload = event.payload || {};
    setWidgetStatus(payload.status || "idle", payload.message || "");
  });
}

window.addEventListener("DOMContentLoaded", async () => {
  if (view === "widget") {
    await initWidgetView();
  } else {
    await initSetupView();
  }
});
