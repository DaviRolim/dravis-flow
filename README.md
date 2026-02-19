# DraVis Flow

Local voice-to-text desktop app built with Tauri v2 + Rust. Hold a hotkey, speak, text appears at your cursor. Optionally restructure speech into well-organized prompts via cloud LLM.

## Features

- 🎙️ **Hold-to-record** — Global hotkey (`Ctrl+Shift+Space`), toggle mode optional
- 🔒 **100% local transcription** — Whisper.cpp via whisper-rs, Metal GPU acceleration on macOS
- 📋 **Auto-paste** — Text injected at cursor position via clipboard
- ⚡ **Prompt Mode** — Cloud LLM restructures speech into organized prompts (Anthropic / OpenAI / OpenRouter)
- 📖 **Dictionary** — Custom vocabulary for Whisper + post-transcription replacements
- 🎨 **Floating widget** — Transparent always-on-top pill with waveform animation
- 📥 **Auto model download** — First-run download from HuggingFace

## Prerequisites

- **Rust** — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **CMake** — `brew install cmake` (macOS) or `apt install cmake` (Linux)
- **Bun** (or npm) — `curl -fsSL https://bun.sh/install | bash`

## Quick Start

```bash
bun install            # Install frontend deps
bun run tauri dev      # Run in dev mode
```

## Configuration

Config lives at `~/.dravis-flow/config.toml`. Created automatically on first run.

```toml
[general]
language = "en"
hotkey = "ctrl+shift+space"
mode = "hold"                    # "hold" or "toggle"

[model]
name = "base.en"                 # "base.en", "small.en", or "large-v3-turbo"
path = "~/.dravis-flow/models/"

[formatting]
level = "basic"

[dictionary]
words = ["Bun", "Tauri", "Rust", "SvelteKit"]  # Whisper vocabulary hints

[[dictionary.replacements]]
from = "dravis"
to = "DraVis"

[prompt_mode]
enabled = false
provider = "openrouter"          # "anthropic", "openai", or "openrouter"
model = "anthropic/claude-3.5-haiku"
anthropic_key = ""               # Keys stored per provider
openai_key = ""
openrouter_key = "sk-or-..."
```

### Whisper Models

| Model | Size | Speed (M4) | Quality |
|-------|------|------------|---------|
| `base.en` | 142 MB | ~0.3s | Good for short dictation |
| `small.en` | 466 MB | ~0.7s | Better accuracy |
| `large-v3-turbo` | 809 MB | ~1-1.5s | Best quality, recommended |

Models auto-download from HuggingFace on first use. Change via the config panel or `config.toml`.

### Prompt Mode

Sends transcribed text to a cloud LLM to restructure into a clean, first-person prompt with markdown sections. Supports:

- **Anthropic** (direct API) — default model: `claude-haiku-4-5`
- **OpenAI** (direct API) — default model: `gpt-4o-mini`
- **OpenRouter** (any model) — default model: `anthropic/claude-3.5-haiku`

API keys are stored per provider — switching providers doesn't lose your keys. Toggle via the config panel.

**Fallback**: If the API call fails for any reason, the raw formatted text is pasted instead. Transcription is never lost.

### Recording Modes

- **Hold** (default): Hold the hotkey to record, release to transcribe and paste
- **Toggle**: Tap once to start recording, tap again to stop and paste

## Build

```bash
bun run tauri build    # Produces .app and .dmg in src-tauri/target/release/bundle/
```

## Development

```bash
# Run tests (--lib flag required)
cd src-tauri && cargo test --lib

# Type-check
cargo check

# Lint
cargo clippy
```

## Architecture

See [CLAUDE.md](./CLAUDE.md) for detailed architecture documentation.

## License

MIT
