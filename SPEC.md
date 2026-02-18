# 🎙️ DraVis Flow — Spec v2

**Local voice-to-text for Mac + Windows. Hold a hotkey, speak, text appears at your cursor.**

---

## ✅ Decisions Locked

| Decision | Choice |
|----------|--------|
| **Name** | DraVis Flow (Davi + Jarvis) |
| **Hotkey** | Hold-to-record default, toggle mode via config |
| **Formatting** | Rule-based (Level 1, zero latency). LLM post-processing = Phase 2 |
| **UI Framework** | Tauri v2 (cross-platform: Mac + Windows + Linux) |
| **Model** | Adaptive — `small.en` (~500MB) on Mac, `base.en` (~150MB) on Windows. Auto-detect. |
| **Language** | English only for MVP. Architecture ready for pt-BR. |

---

## What is this?

A local, fast, unlimited voice-to-text tool. Press a hotkey, speak, text appears wherever your cursor is. Like Wispr Flow, but free, open-source, and ours.

**Why build it:** Wispr Flow free plan is running out. Whisper.cpp runs perfectly on Apple Silicon and modern PCs. No cloud dependency = no latency, no limits, no cost.

---

## Wispr Flow vs DraVis Flow

| | Wispr Flow | DraVis Flow |
|---|---|---|
| Transcription | Cloud-based | Local (whisper.cpp) |
| Cost | Free plan with limits ⚠️ | Unlimited, forever free ✅ |
| Latency | ~500ms+ (network) | ~100-200ms (local) |
| Formatting | Smart Formatting ✅ | Rule-based (LLM in Phase 2) |
| Backtrack | ✅ | Possible via re-process |
| Dictionary | ✅ | Custom vocabulary file |
| Platforms | Mac only | Mac + Windows (Tauri) → Android Phase 2 |
| Source | Closed | Open source (yours) |

---

## Architecture

```
[User presses hotkey]
    ↓
Hotkey Listener (global keyboard hook — rdev)
    ↓ start recording
Audio Capture (cpal — 16kHz mono PCM)
    ↓ raw PCM chunks
VAD (Voice Activity Detection — optional, silero-vad)
    ↓ speech segments
Whisper.cpp (whisper-rs, base.en model)
    ↓ raw text
Post-Processor (punctuation, formatting, casing)
    ↓ formatted text
Text Injector (clipboard + simulate Cmd+V / Ctrl+V)
    ↓
[Text appears at cursor]
```

### Key Components

1. **Hotkey Listener** — Global shortcut (default: `Ctrl+Shift+Space`). **Hold to record, release to transcribe** (default). Toggle mode available via config. Uses `rdev` crate.

2. **Audio Capture** — `cpal` crate for cross-platform audio. Records at 16kHz mono (Whisper's expected format). Buffers in memory, no disk I/O.

3. **VAD (Voice Activity Detection)** — Optional. Silero VAD trims silence at start/end. Prevents sending silence to Whisper.

4. **Whisper Engine** — `whisper-rs` (Rust bindings for whisper.cpp). Model: `ggml-base.en` (~150MB, fast) or `ggml-small.en` (~500MB, more accurate). Runs on CPU; M4 handles base model in ~200ms for 10s of audio.

5. **Post-Processor** — Rule-based text formatting:
   - Capitalize first letter of sentences
   - Add periods at natural pauses / Whisper segment boundaries
   - Handle "i" → "I", basic contractions
   - Trim whitespace

6. **Text Injector** — Copies formatted text to clipboard, simulates Cmd+V (Mac) or Ctrl+V (Windows), restores original clipboard. Uses `arboard` + `enigo` crates.

---

## UI — Recording Widget

Minimal floating widget. Appears when recording, disappears when done. Tauri v2 webview — cross-platform.

### States

**IDLE** — No widget visible. App lives in system tray / menu bar (🎙️ icon).

**RECORDING** — Floating pill appears at bottom-center of screen:
- Red pulsing dot + animated 7-bar audio waveform + "Recording..." text
- Semi-transparent dark background with frosted glass effect
- ~220px wide, ~44px tall
- Fade in (200ms)

**PROCESSING** — Same pill:
- Yellow/amber dot
- "Transcribing..." text
- Bars stop animating
- Fades out when text is pasted

### UI Details
- **Framework:** Tauri v2 webview — cross-platform, transparent, always-on-top, no title bar
- **Position:** Bottom-center of screen (configurable)
- **Audio wave:** Real-time visualization of mic input volume. 7-bar equalizer. Bars scale with audio amplitude.
- **Animations:** Fade in/out (200ms). Smooth bar transitions.

---

## Hotkey Behavior

### Option A: Hold-to-Record ✅ (default)
- Hold `Ctrl+Shift+Space` → recording starts
- Release → recording stops → transcribe → paste
- Simple mental model: "hold to talk"
- Natural for short dictations

### Option B: Toggle (config option)
- Press once → start recording
- Press again → stop → transcribe → paste
- Better for long dictations

---

## Formatting

### Level 1: Rule-based (zero latency) ✅ MVP
- Capitalize first word of sentences
- Add periods at natural pauses
- Basic contractions and common patterns
- Good enough for casual messages

### Level 2: LLM-powered (Phase 2)
- Send raw text to local Ollama or Claude API
- Much better for professional text, documentation
- Adds ~200-500ms

---

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Language | **Rust** |
| App shell | **Tauri v2** (Mac + Windows + Linux) |
| Audio capture | `cpal` (cross-platform audio) |
| Whisper engine | `whisper-rs` (whisper.cpp bindings) |
| Hotkey | `rdev` (global keyboard events) |
| GUI widget | Tauri webview (HTML/CSS/JS) |
| Text injection | `arboard` (clipboard) + `enigo` (key simulation) |
| VAD (optional) | `silero-vad` or simple energy threshold |
| Config | TOML file (`~/.dravis-flow/config.toml`) |
| Model | **Adaptive:** `ggml-small.en` on Mac, `ggml-base.en` on Windows |

---

## Config File

```toml
# ~/.dravis-flow/config.toml

[general]
language = "en"              # "en", "pt", "auto"
hotkey = "ctrl+shift+space"  # configurable
mode = "hold"                # "hold" or "toggle"

[model]
name = "base.en"             # "base.en", "small.en", "medium.en"
path = "~/.dravis-flow/models/"

[ui]
position = "bottom-center"   # "bottom-center", "near-cursor"
theme = "dark"               # "dark", "light", "auto"
show_waveform = true

[formatting]
level = "basic"              # "basic" (rule-based), "smart" (LLM)

[clipboard]
restore_after_paste = true
paste_delay_ms = 50
```

---

## Phase 1 Scope — MVP ✅

### In Scope
- Global hotkey (hold-to-record)
- Audio capture → local Whisper transcription
- Floating pill widget with audio waveform
- Auto-paste into active application
- System tray icon (start/stop, settings)
- Rule-based formatting (Level 1)
- Config file for hotkey, model, formatting
- Model auto-download on first run

### Phase 2 (Later)
- LLM-powered smart formatting (Level 2)
- Custom dictionary / vocabulary
- Voice commands ("new line", "period", "select all")
- Multi-language support (Portuguese + English)
- Usage stats (words transcribed, time saved)
- Android companion app

---

## Build Plan

| Phase | Task | Est. Time |
|-------|------|-----------|
| 1 | Scaffold + audio capture | 1-2h |
| 2 | Whisper integration | 1-2h |
| 3 | Text injection | 1h |
| 4 | UI widget (Tauri webview) | 1-2h |
| 5 | Polish + config | 1h |
| **Total** | | **5-8 hours** |

---

## File Structure

```
dravis-flow/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs          # Tauri app entry point
│   │   ├── audio.rs         # Audio capture (cpal)
│   │   ├── whisper.rs       # Whisper engine wrapper
│   │   ├── hotkey.rs        # Global hotkey listener
│   │   ├── injector.rs      # Text injection (clipboard + paste)
│   │   ├── formatter.rs     # Rule-based text formatting
│   │   └── config.rs        # Config file handling
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── index.html           # Recording widget UI
│   ├── style.css            # Widget styles
│   └── main.js              # Widget logic + Tauri events
├── package.json
├── SPEC.md                  # This file
└── README.md
```

---

*DraVis Flow — Built by Davi 🎩*
