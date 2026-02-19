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
let vocabWords = [];
let vocabReplacements = [];
let dictErrorTimer = null;

function showDictError(dictErrorMsgEl, msg) {
  if (!dictErrorMsgEl) return;
  dictErrorMsgEl.textContent = msg;
  dictErrorMsgEl.classList.remove("hidden");
  if (dictErrorTimer) clearTimeout(dictErrorTimer);
  dictErrorTimer = setTimeout(() => {
    dictErrorMsgEl.classList.add("hidden");
    dictErrorTimer = null;
  }, 4000);
}

function renderVocabList(vocabListEl) {
  if (!vocabListEl) return;
  vocabListEl.innerHTML = "";
  vocabWords.forEach((word, index) => {
    const chip = document.createElement("span");
    chip.className = "tag-chip";
    chip.appendChild(document.createTextNode(word));
    const removeBtn = document.createElement("button");
    removeBtn.className = "tag-chip-remove";
    removeBtn.type = "button";
    removeBtn.setAttribute("aria-label", `Remove ${word}`);
    removeBtn.innerHTML = "&times;";
    removeBtn.addEventListener("click", () => removeVocabWord(invoke, vocabListEl, dictErrorMsgEl, index));
    chip.appendChild(removeBtn);
    vocabListEl.appendChild(chip);
  });
}

function renderReplacementsList(replacementsListEl) {
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
    removeBtn.addEventListener("click", () =>
      removeReplacement(invoke, replacementsListEl, dictErrorMsgEl, index),
    );
    row.appendChild(fromSpan);
    row.appendChild(arrowSpan);
    row.appendChild(toSpan);
    row.appendChild(removeBtn);
    replacementsListEl.appendChild(row);
  });
}

// Module-level references to invoke/elements, set during initSetupView.
// This avoids threading them through every render callback.
let invoke;
let dictErrorMsgEl;

async function removeVocabWord(invokeFn, vocabListEl, errorEl, index) {
  const previous = vocabWords;
  vocabWords = vocabWords.filter((_, i) => i !== index);
  renderVocabList(vocabListEl);
  try {
    await invokeFn("set_dictionary_words", { words: vocabWords });
  } catch (error) {
    vocabWords = previous;
    renderVocabList(vocabListEl);
    showDictError(errorEl, `Could not save vocabulary: ${error}`);
  }
}

async function addVocabWord(invokeFn, vocabListEl, errorEl, word) {
  const trimmed = word.trim();
  if (!trimmed || vocabWords.includes(trimmed)) return;
  const previous = vocabWords;
  vocabWords = [...vocabWords, trimmed];
  renderVocabList(vocabListEl);
  try {
    await invokeFn("set_dictionary_words", { words: vocabWords });
  } catch (error) {
    vocabWords = previous;
    renderVocabList(vocabListEl);
    showDictError(errorEl, `Could not save vocabulary: ${error}`);
  }
}

async function removeReplacement(invokeFn, replacementsListEl, errorEl, index) {
  const previous = vocabReplacements;
  vocabReplacements = vocabReplacements.filter((_, i) => i !== index);
  renderReplacementsList(replacementsListEl);
  try {
    await invokeFn("set_dictionary_replacements", { replacements: vocabReplacements });
  } catch (error) {
    vocabReplacements = previous;
    renderReplacementsList(replacementsListEl);
    showDictError(errorEl, `Could not save replacements: ${error}`);
  }
}

async function addReplacement(invokeFn, replacementsListEl, errorEl, from, to) {
  const fromTrimmed = from.trim();
  const toTrimmed = to.trim();
  if (!fromTrimmed || !toTrimmed) return;
  const previous = vocabReplacements;
  vocabReplacements = [...vocabReplacements, { from: fromTrimmed, to: toTrimmed }];
  renderReplacementsList(replacementsListEl);
  try {
    await invokeFn("set_dictionary_replacements", { replacements: vocabReplacements });
  } catch (error) {
    vocabReplacements = previous;
    renderReplacementsList(replacementsListEl);
    showDictError(errorEl, `Could not save replacements: ${error}`);
  }
}

function applyModelUI(modelButtons, modelStatusEl, downloadBtn, modelName, confirmed) {
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

async function saveModel(invokeFn, modelButtons, modelStatusEl, setupMessageEl, downloadBtn, modelName) {
  if (modelName === currentModel) return;

  const previous = currentModel;
  currentModel = modelName;
  applyModelUI(modelButtons, modelStatusEl, downloadBtn, modelName, false);

  try {
    const result = await invokeFn("set_model", { name: modelName });
    applyModelUI(modelButtons, modelStatusEl, downloadBtn, modelName, true);
    if (result && result.exists) {
      setupMessageEl.textContent = "Model ready. Close this window and use the hotkey.";
      downloadBtn.classList.add("hidden");
    } else {
      setupMessageEl.textContent = `Model not downloaded yet.`;
      downloadBtn.classList.remove("hidden");
    }
  } catch (error) {
    currentModel = previous;
    applyModelUI(modelButtons, modelStatusEl, downloadBtn, previous, true);
    setupMessageEl.textContent = `Could not switch model: ${error}`;
  }
}

async function loadConfig(invokeFn, modelButtons, modelStatusEl, downloadBtn, vocabListEl, replacementsListEl) {
  try {
    const config = await invokeFn("get_config");
    currentModel = config?.model?.name || "base.en";
    vocabWords = config?.dictionary?.words || [];
    vocabReplacements = config?.dictionary?.replacements || [];
  } catch (_) {
    // ignore — applyModelUI will use the default
  }

  applyModelUI(modelButtons, modelStatusEl, downloadBtn, currentModel, true);
  renderVocabList(vocabListEl);
  renderReplacementsList(replacementsListEl);
}

export async function initSetupView(invokeFn, listen) {
  invoke = invokeFn;

  const setupEl = document.getElementById("setup");
  const widgetEl = document.getElementById("widget");
  const setupMessageEl = document.getElementById("setup-message");
  const downloadBtn = document.getElementById("download-btn");
  const progressWrap = document.getElementById("progress-wrap");
  const progressEl = document.getElementById("progress");
  const progressTextEl = document.getElementById("progress-text");
  const modelStatusEl = document.getElementById("model-status");
  const hotkeyHintEl = document.getElementById("hotkey-hint");
  const modelButtons = Array.from(document.querySelectorAll("#model-switch .mode-btn"));
  const vocabListEl = document.getElementById("vocab-list");
  const vocabInputEl = document.getElementById("vocab-input");
  const vocabAddBtnEl = document.getElementById("vocab-add-btn");
  const replacementsListEl = document.getElementById("replacements-list");
  const replacementFromEl = document.getElementById("replacement-from-input");
  const replacementToEl = document.getElementById("replacement-to-input");
  const replacementAddBtnEl = document.getElementById("replacement-add-btn");
  dictErrorMsgEl = document.getElementById("dict-error-msg");

  setupEl.classList.remove("hidden");
  widgetEl.classList.add("hidden");

  const [, modelResult] = await Promise.all([
    loadConfig(invokeFn, modelButtons, modelStatusEl, downloadBtn, vocabListEl, replacementsListEl),
    invokeFn("check_model").catch((error) => ({ error })),
  ]);

  modelButtons.forEach((button) => {
    button.addEventListener("click", () => {
      saveModel(invokeFn, modelButtons, modelStatusEl, setupMessageEl, downloadBtn, button.dataset.model || "base.en");
    });
  });

  if (vocabAddBtnEl && vocabInputEl) {
    vocabAddBtnEl.addEventListener("click", () => {
      addVocabWord(invokeFn, vocabListEl, dictErrorMsgEl, vocabInputEl.value);
      vocabInputEl.value = "";
    });
    vocabInputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        addVocabWord(invokeFn, vocabListEl, dictErrorMsgEl, vocabInputEl.value);
        vocabInputEl.value = "";
      }
    });
  }

  if (replacementAddBtnEl && replacementFromEl && replacementToEl) {
    replacementAddBtnEl.addEventListener("click", () => {
      addReplacement(invokeFn, replacementsListEl, dictErrorMsgEl, replacementFromEl.value, replacementToEl.value);
      replacementFromEl.value = "";
      replacementToEl.value = "";
    });
    replacementToEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        addReplacement(invokeFn, replacementsListEl, dictErrorMsgEl, replacementFromEl.value, replacementToEl.value);
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
      await invokeFn("download_model");
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
    progressWrap.setAttribute("aria-valuenow", pct);
  });

  // Unused in setup view but must satisfy the hint element reference.
  if (hotkeyHintEl) {
    // hint is static HTML; no dynamic update needed in the happy path
  }
}
