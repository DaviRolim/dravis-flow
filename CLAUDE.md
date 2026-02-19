# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

DraVis Flow is a local voice-to-text desktop app built with Tauri v2 + Rust. Hold a hotkey, speak, text appears at your cursor. Uses whisper.cpp (via whisper-rs) for local transcription with Metal GPU acceleration on macOS. Optional **Prompt Mode** sends transcribed text to a cloud LLM for restructuring into well-organized prompts.

## Commands

```bash
# Development
bun install              # Install frontend deps (first time)
bun run tauri dev        # Run in dev mode (hot-reload frontend)

# Build
bun run tauri build      # Produce .app and .dmg in src-tauri/target/release/bundle/

# Rust-only commands (from src-tauri/)
cargo build              # Build Rust backend only
cargo check              # Type-check without building
cargo test --lib         # Run unit tests (IMPORTANT: --lib flag required)
cargo clippy             # Lint
```

> ⚠️ `cargo test` alone finds 0 tests due to crate-type config. Always use `cargo test --lib`.

## Architecture

**Tauri v2 app** with Rust backend (`src-tauri/src/`) and plain HTML/CSS/JS frontend (`src/`). No bundler, no framework — the frontend is served directly by Tauri.

### Backend Modules (src-tauri/src/)

| Module | LOC | Purpose | Tests |
|--------|-----|---------|-------|
| **lib.rs** | ~316 | App entry point, Tauri setup, hotkey dispatch, module registration | — |
| **state.rs** | ~102 | `AppState`, `InnerState`, `AppStatus`, `ModelStatus`, `with_state()` helper | — |
| **pipeline.rs** | ~303 | Recording pipeline: start → record → transcribe → format → [structure] → paste | — |
| **commands.rs** | ~156 | All `#[tauri::command]` handlers (thin wrappers around pipeline/state) | — |
| **app_setup.rs** | ~88 | Tray menu, window management, shortcut registration | — |
| **audio.rs** | ~349 | Mic capture via cpal, sample conversion, resampling to 16kHz, silence trimming | ✅ |
| **whisper.rs** | ~115 | WhisperContext loading, transcription, `build_initial_prompt()` with dictionary | — |
| **formatter.rs** | ~457 | Multi-pass text cleanup: filler removal, stutter, contractions, capitalization | ✅ |
| **prompt.rs** | ~206 | Cloud LLM API calls (Anthropic, OpenAI, OpenRouter) for Prompt Mode | ✅ |
| **config.rs** | ~341 | TOML config, model paths, prompt mode config, dictionary config | ✅ |
| **hotkey.rs** | ~196 | Hotkey string parsing + shortcut state machine (tap vs hold) | ✅ |
| **injector.rs** | ~185 | Clipboard paste via CGEvent API (macOS) with osascript fallback | — |

### Frontend (src/)

| File | LOC | Purpose |
|------|-----|---------|
| **main.js** | ~58 | View routing only — imports setup.js or widget.js based on `?view=widget` |
| **setup.js** | ~505 | Config panel: model selector, dictionary, prompt mode settings |
| **widget.js** | ~331 | Floating pill: waveform animation, status display, recording controls |
| **index.html** | ~135 | Both views in one HTML, toggled via CSS `.hidden` class |
| **style.css** | ~918 | Design system with CSS custom properties, animations |

### State Machine

```
idle → (hotkey) → recording → (hotkey release) → processing → [structuring] → idle
                                                      ↓
                                              (prompt mode off: skip structuring)
```

Status changes emit `"status"` events. Audio levels emit `"audio_level"` every 50ms.

### Recording Pipeline (pipeline.rs)

```
start_recording → capture audio → stop_recording:
  1. Trim silence from audio
  2. Transcribe with Whisper (+ dictionary words as initial_prompt)
  3. Format text (filler removal, capitalization, etc.)
  4. Apply dictionary replacements
  5. [If prompt mode ON] → emit "structuring" → call cloud LLM → fallback to raw on error
  6. Paste result at cursor via injector
```

### Prompt Mode

Three providers supported: **Anthropic**, **OpenAI**, **OpenRouter** (OpenAI-compatible).
- Per-provider API keys stored separately (switching providers doesn't lose keys)
- Toggled via config panel (not widget — avoids focus stealing)
- Falls back to raw formatted text on ANY error — never loses transcription
- System prompt produces first-person structured prompts with adaptive sections

### Tauri Commands (frontend → backend)

`start_recording`, `stop_recording`, `cancel_recording`, `get_status`, `get_config`, `set_recording_mode`, `set_model`, `check_model`, `download_model`, `set_prompt_mode`, `set_dictionary_words`, `set_dictionary_replacements`

> **Tauri camelCase rule**: Rust `snake_case` params become `camelCase` in JS invoke calls. E.g. `api_key` in Rust → `apiKey` in JS.

### Recording Modes

- **Hold** (default): Hold hotkey to record, release to transcribe+paste
- **Toggle**: Tap once to start, tap again to stop+transcribe+paste

## Key Patterns

- **State access**: All mutable state through `with_state()` (locks Mutex, handles poisoned locks).
- **CPU-bound work**: Transcription and injection run via `tauri::async_runtime::spawn_blocking()`.
- **WhisperContext pre-loaded on startup** (not lazy — avoids first-recording delay).
- **Config persistence**: Every config change immediately writes to `~/.dravis-flow/config.toml`.
- **Dictionary**: Words fed as Whisper `initial_prompt` glossary (style conditioning, 224 token limit). Post-transcription replacements applied after formatting.
- **Widget non-focusable**: `focus: false` in tauri.conf.json prevents stealing focus from target app.
- **Paste**: CGEvent API (core-graphics crate) for macOS. Clipboard save → set text → Cmd+V → restore.
- **Error handling**: Consistent `Result<T, String>` with context messages throughout.

## Config & Model Paths

- **Config**: `~/.dravis-flow/config.toml`
- **Models**: `~/.dravis-flow/models/` (ggml-base.en.bin, ggml-small.en.bin, ggml-large-v3-turbo.bin)
- Models auto-download from HuggingFace (`ggerganov/whisper.cpp`)

### Changing the Prompt Mode Model

Edit `~/.dravis-flow/config.toml`:
```toml
[prompt_mode]
enabled = true
provider = "openrouter"        # "anthropic", "openai", or "openrouter"
model = "anthropic/claude-3.5-haiku"  # any model the provider supports
openrouter_key = "sk-or-..."
```

Or use the config panel in the app (provider + API key — model defaults per provider).

### Named Constants Reference

| Constant | Value | Rationale |
|----------|-------|-----------|
| `WHISPER_N_THREADS` | 4 | Sweet spot for M-series; higher causes thread contention |
| `WHISPER_MAX_PROMPT_CHARS` | 850 | Whisper hard limit ~890 chars; 850 leaves margin |
| `SILENCE_RMS_THRESHOLD` | 0.01 | Background noise floor for silence trimming |
| `CLIPBOARD_SYNC_DELAY_MS` | 50 | Pasteboard sync wait after clipboard set |
| `PASTE_SETTLE_DELAY_MS` | 100 | Wait for target app to process paste event |
| `KEY_EVENT_DELAY_MS` | 10 | CGEvent key-down to key-up gap |

## macOS Bundle Notes

`tauri.conf.json` includes `NSMicrophoneUsageDescription` and `NSAccessibilityUsageDescription`. Uses `macOSPrivateApi: true` for the transparent always-on-top widget.

## Prerequisites

Rust toolchain, cmake (for whisper.cpp build), and bun/npm for the Tauri CLI.

## Git Push from Mac Mini

If credential helper hangs:
```bash
git config credential.helper '!gh auth git-credential'
GIT_TERMINAL_PROMPT=0 git push
```
