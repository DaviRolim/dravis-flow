//! Application state types.
//!
//! `AppState` is the top-level Tauri-managed state. All mutable fields live in
//! `InnerState` behind a Mutex, accessed via [`with_state`]. The `WhisperContext`
//! gets its own Mutex (`SendWhisperCtx`) since transcription is CPU-heavy and
//! shouldn't block state reads.

use crate::audio::AudioRecorder;
use crate::config::AppConfig;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use tauri::State;
use whisper_rs::WhisperContext;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // Error variant is used via as_str() for widget state
pub enum AppStatus {
    Idle,
    Recording,
    Processing,
    Error,
}

impl AppStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub exists: bool,
    pub path: String,
}

/// Wrapper to make WhisperContext movable across thread boundaries.
/// Safety: WhisperContext is only accessed while holding the whisper_ctx Mutex,
/// guaranteeing exclusive single-threaded access at all times.
pub struct SendWhisperCtx(pub WhisperContext);
unsafe impl Send for SendWhisperCtx {}

pub struct AppState {
    pub inner_state: Mutex<InnerState>,
    pub whisper_ctx: Mutex<Option<SendWhisperCtx>>,
    pub model_ready: Arc<AtomicBool>,
}

pub struct InnerState {
    pub status: AppStatus,
    pub recorder: AudioRecorder,
    pub config: AppConfig,
    pub toggle_shortcut_held: bool,
    pub press_instant: Option<std::time::Instant>,
    pub toggle_active: bool,
}

impl InnerState {
    pub fn reset_to_idle(&mut self) {
        self.status = AppStatus::Idle;
        self.toggle_active = false;
        self.press_instant = None;
    }
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let mut config = config;
        config.general.mode = crate::sanitize_recording_mode(&config.general.mode);

        Self {
            inner_state: Mutex::new(InnerState {
                status: AppStatus::Idle,
                recorder: AudioRecorder::new(),
                config,
                toggle_shortcut_held: false,
                press_instant: None,
                toggle_active: false,
            }),
            whisper_ctx: Mutex::new(None),
            model_ready: Arc::new(AtomicBool::new(true)),
        }
    }
}

/// Lock `AppState::inner_state`, run `f`, and return its result.
/// Handles poisoned locks with a consistent error message.
pub fn with_state<T>(
    state: &State<AppState>,
    f: impl FnOnce(&mut InnerState) -> Result<T, String>,
) -> Result<T, String> {
    let mut lock = state
        .inner_state
        .lock()
        .map_err(|_| "app state lock poisoned".to_string())?;
    f(&mut lock)
}
