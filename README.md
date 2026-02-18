# DraVis Flow

Local, cross-platform voice-to-text desktop app with Tauri v2 + Rust.

## Features

- Global hotkey hold-to-record (`Ctrl+Shift+Space` by default)
- Local microphone capture with `cpal` (in-memory PCM)
- Local Whisper transcription with `whisper-rs`
- Text post-formatting + paste injection into active app
- System tray app with floating recording widget
- First-run model download from Hugging Face

## Run

1. Install prerequisites for Tauri v2 + Rust toolchain.
2. Install frontend deps:

```bash
npm install
```

3. Run desktop app:

```bash
cargo tauri dev
```

## Config

Config file is created at `~/.dravis-flow/config.toml`:

```toml
[general]
language = "en"
hotkey = "ctrl+shift+space"
mode = "hold"

[model]
name = "base.en"
path = "~/.dravis-flow/models/"

[formatting]
level = "basic"
```
