//! Whisper transcription engine.
//!
//! Wraps whisper-rs (whisper.cpp bindings) with Metal GPU acceleration on macOS.
//! The `WhisperContext` is pre-loaded on startup and cached in `AppState`.
//! Dictionary words are fed as a glossary in `initial_prompt` — this is style
//! conditioning (not instruction following), limited to ~224 tokens (~850 chars).

use crate::config::model_file_path;
use crate::config::AppConfig;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Sweet spot for M-series chips; higher values cause thread contention without measurable gain.
const WHISPER_N_THREADS: i32 = 4;

/// Whisper's hard limit is ~890 characters (~224 tokens). 850 leaves margin to avoid mid-word truncation.
const WHISPER_MAX_PROMPT_CHARS: usize = 850;

pub struct WhisperEngine {
    model_path: std::path::PathBuf,
}

impl WhisperEngine {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            model_path: model_file_path(config),
        }
    }

    pub fn model_exists(&self) -> bool {
        self.model_path.exists()
    }

    pub fn model_path(&self) -> &std::path::Path {
        &self.model_path
    }
}

pub fn load_context(model_path: &str) -> Result<WhisperContext, String> {
    let ctx_params = WhisperContextParameters::default();
    WhisperContext::new_with_params(model_path, ctx_params)
        .map_err(|e| format!("failed to load whisper model: {e}"))
}

/// Build the initial_prompt for Whisper conditioning.
///
/// Structure (within 224 token / ~890 char limit):
///   1. A style-setting sentence (proper caps, punctuation) — establishes output style by example
///   2. "Glossary: term1, term2, ..." — biases Whisper toward these spellings
///
/// Whisper treats this as "previous transcript context", NOT as instructions.
/// It follows the *style* of the prompt and recognizes glossary terms more accurately.
fn build_initial_prompt(dictionary_words: &[String]) -> String {
    let style = "I discussed the project requirements with the team, then reviewed the implementation details and pushed the changes.";

    if dictionary_words.is_empty() {
        return style.to_string();
    }

    let glossary = dictionary_words.join(", ");
    let prompt = format!("{style} Glossary: {glossary}");

    // Whisper hard limit: 224 tokens (~890 chars). Truncate glossary if needed.
    // Keep a safe margin — cut at WHISPER_MAX_PROMPT_CHARS to avoid mid-word truncation.
    if prompt.len() > WHISPER_MAX_PROMPT_CHARS {
        let available = WHISPER_MAX_PROMPT_CHARS - style.len() - " Glossary: ".len();
        let mut truncated = String::new();
        for word in dictionary_words {
            let next = if truncated.is_empty() {
                word.clone()
            } else {
                format!(", {word}")
            };
            if truncated.len() + next.len() > available {
                break;
            }
            truncated.push_str(&next);
        }
        format!("{style} Glossary: {truncated}")
    } else {
        prompt
    }
}

pub fn transcribe_with_ctx(
    ctx: &WhisperContext,
    audio: &[f32],
    language: &str,
    dictionary_words: &[String],
) -> Result<String, String> {
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("failed creating whisper state: {e}"))?;

    let initial_prompt = build_initial_prompt(dictionary_words);

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_translate(false);
    params.set_language(Some(language));
    params.set_n_threads(WHISPER_N_THREADS);
    params.set_initial_prompt(&initial_prompt);
    params.set_suppress_blank(true);
    params.set_suppress_non_speech_tokens(true);

    state
        .full(params, audio)
        .map_err(|e| format!("whisper inference failed: {e}"))?;

    let mut out = String::new();
    let segments = state
        .full_n_segments()
        .map_err(|e| format!("failed reading whisper segments: {e}"))?;

    for i in 0..segments {
        let segment = state
            .full_get_segment_text(i)
            .map_err(|e| format!("failed reading segment text: {e}"))?;
        out.push_str(segment.trim());
        out.push(' ');
    }

    Ok(out.trim().to_string())
}
