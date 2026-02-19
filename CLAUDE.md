# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

DraVis Flow is a local voice-to-text desktop app built with Tauri v2 + Rust. Hold a hotkey, speak, text appears at your cursor. Uses whisper.cpp (via whisper-rs) for local transcription with Metal GPU acceleration on macOS.

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
cargo test               # Run unit tests (formatter has tests)
cargo clippy             # Lint
```

## Architecture

**Tauri v2 app** with Rust backend (`src-tauri/src/`) and plain HTML/CSS/JS frontend (`src/`). No bundler, no framework — the frontend is served directly by Tauri.

### Backend Modules (src-tauri/src/)

- **lib.rs** — App entry point, Tauri setup, hotkey handling, state machine orchestration. Contains all `#[tauri::command]` handlers and the `AppState` (Mutex-wrapped).
- **audio.rs** — Microphone capture via cpal. Handles sample format conversion (F32/I16/U16 → mono f32), resampling to 16kHz, and RMS-based silence trimming.
- **whisper.rs** — WhisperContext loading and transcription. Context is lazily loaded and cached in AppState behind a `SendWhisperCtx` wrapper (unsafe Send impl, guarded by Mutex).
- **formatter.rs** — Multi-pass rule-based text cleanup: filler removal, contraction fixing, capitalization, punctuation. Has unit tests.
- **injector.rs** — Clipboard set → Cmd+V/Ctrl+V simulation → clipboard restore. Uses CGEvent API on macOS (fallback to osascript), enigo on Windows/Linux.
- **config.rs** — TOML config at `~/.dravis-flow/config.toml`. Manages model paths, download URLs, and tilde expansion.
- **hotkey.rs** — Parses hotkey strings (e.g. "ctrl+shift+space") into Tauri's `Modifiers + Code` format.

### Frontend (src/)

Single `index.html` with two views routed by `?view=widget` query param:
- **Main view** — Config panel (recording mode, model selector, download progress). Shown when model is missing.
- **Widget view** — Floating transparent pill with 24-bar animated waveform. Always-on-top, no decorations.

### State Machine

```
idle → (hotkey press) → recording → (hotkey release/press) → processing → (paste done) → idle
```

Status changes emit `"status"` events to the frontend. Audio levels emit `"audio_level"` events every 50ms.

### Tauri Commands (frontend → backend)

`start_recording`, `stop_recording`, `cancel_recording`, `get_status`, `get_config`, `set_recording_mode`, `set_model`, `check_model`, `download_model`

### Recording Modes

- **Hold** (default): Hold hotkey to record, release to transcribe+paste
- **Toggle**: Press once to start, press again to stop+transcribe+paste

## Key Patterns

- **State access**: All mutable state goes through `with_state()` helper that locks the Mutex and handles poisoned lock errors.
- **CPU-bound work**: Transcription and text injection run via `tauri::async_runtime::spawn_blocking()` to keep the UI thread free.
- **Lazy model loading**: WhisperContext is loaded on first recording and cached for subsequent use.
- **Config persistence**: Every config change immediately writes to `~/.dravis-flow/config.toml`.

## Config & Model Paths

- Config: `~/.dravis-flow/config.toml`
- Models: `~/.dravis-flow/models/` (ggml-base.en.bin, ggml-small.en.bin, ggml-large-v3-turbo.bin)
- Models auto-download from HuggingFace (`ggerganov/whisper.cpp`)

## macOS Bundle Notes

The `tauri.conf.json` includes `NSMicrophoneUsageDescription` and `NSAccessibilityUsageDescription` in `bundle.macOS.infoPlist`. The app uses `macOSPrivateApi: true` for the transparent always-on-top widget. Text injection uses the CGEvent API (core-graphics crate) with osascript fallback.

## Prerequisites

Rust toolchain, cmake (for whisper.cpp build), and bun/npm for the Tauri CLI.
