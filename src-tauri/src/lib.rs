mod audio;
mod config;
mod formatter;
mod hotkey;
mod injector;
mod whisper;

use audio::AudioRecorder;
use config::{load_or_create_config, model_download_url, model_file_path, save_config, AppConfig};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use whisper::WhisperEngine;
use whisper_rs::WhisperContext;

// ── File-based logging ──────────────────────────────────────────────────

static LOG_FILE: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();
static APP_START: OnceLock<std::time::Instant> = OnceLock::new();

fn init_logging() {
    APP_START.get_or_init(std::time::Instant::now);
    let log_dir = config::config_dir();
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("dravis-flow.log");
    if let Ok(file) = File::create(&log_path) {
        let _ = LOG_FILE.set(Mutex::new(BufWriter::new(file)));
        eprintln!("logging to {}", log_path.display());
    }
}

pub(crate) fn write_log(msg: &str) {
    let elapsed = APP_START
        .get()
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    if let Some(writer) = LOG_FILE.get() {
        if let Ok(mut w) = writer.lock() {
            let _ = writeln!(w, "[{elapsed:>8.3}s] {msg}");
            let _ = w.flush();
        }
    }
}

#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        eprintln!("{}", msg);
        $crate::write_log(&msg);
    }};
}

// MODEL_URL removed — now driven by config::model_download_url()
const MIN_TRANSCRIBE_SAMPLES: usize = 16_000; // ~1s at 16 kHz
const MODE_HOLD: &str = "hold";
const MODE_TOGGLE: &str = "toggle";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // Error variant is used via as_str() for widget state
enum AppStatus {
    Idle,
    Recording,
    Processing,
    Error,
}

impl AppStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct StatusPayload {
    status: String,
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelStatus {
    exists: bool,
    path: String,
}

/// Wrapper to make WhisperContext movable across thread boundaries.
/// Safety: WhisperContext is only accessed while holding whisper_ctx Mutex,
/// guaranteeing exclusive single-threaded access at all times.
struct SendWhisperCtx(WhisperContext);
unsafe impl Send for SendWhisperCtx {}

struct AppState {
    inner_state: Mutex<InnerState>,
    whisper_ctx: Mutex<Option<SendWhisperCtx>>,
    model_ready: Arc<AtomicBool>,
}

struct InnerState {
    status: AppStatus,
    recorder: AudioRecorder,
    config: AppConfig,
    toggle_shortcut_held: bool,
    press_instant: Option<std::time::Instant>,
    toggle_active: bool,
}

impl InnerState {
    fn reset_to_idle(&mut self) {
        self.status = AppStatus::Idle;
        self.toggle_active = false;
        self.press_instant = None;
    }
}

impl AppState {
    fn new(config: AppConfig) -> Self {
        let mut config = config;
        config.general.mode = sanitize_recording_mode(&config.general.mode);

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

fn show_main_window(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
}

fn normalize_recording_mode(mode: &str) -> Option<String> {
    let normalized = mode.trim().to_lowercase();
    match normalized.as_str() {
        MODE_HOLD | MODE_TOGGLE => Some(normalized),
        _ => None,
    }
}

fn sanitize_recording_mode(mode: &str) -> String {
    normalize_recording_mode(mode).unwrap_or_else(|| MODE_HOLD.to_string())
}

fn set_widget_state(app: &AppHandle, status: &str, message: Option<String>) {
    let payload = StatusPayload {
        status: status.to_string(),
        message,
    };

    if let Some(widget) = app.get_webview_window("widget") {
        match status {
            "recording" | "processing" | "error" => {
                let _ = position_widget_window(&widget);
                let _ = widget.show();
            }
            "idle" => {
                let _ = widget.hide();
            }
            _ => {}
        }
    }

    let _ = app.emit("status", payload);
}

fn position_widget_window(window: &WebviewWindow) -> Result<(), String> {
    if let Some(monitor) = window
        .current_monitor()
        .map_err(|e| format!("monitor query failed: {e}"))?
    {
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        let window_size = window
            .outer_size()
            .map_err(|e| format!("widget size query failed: {e}"))?;

        let x_offset = ((monitor_size.width as i32 - window_size.width as i32) / 2).max(0);
        let y_offset = monitor_size.height as i32 - window_size.height as i32 - 56;
        let x = monitor_pos.x + x_offset;
        let y = (monitor_pos.y + y_offset).max(monitor_pos.y);

        window
            .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
            .map_err(|e| format!("widget position failed: {e}"))?;
    }
    Ok(())
}

fn with_state<T>(
    state: &State<AppState>,
    f: impl FnOnce(&mut InnerState) -> Result<T, String>,
) -> Result<T, String> {
    let mut lock = state
        .inner_state
        .lock()
        .map_err(|_| "app state lock poisoned".to_string())?;
    f(&mut lock)
}

async fn start_recording_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    if !state.model_ready.load(Ordering::SeqCst) {
        return Err("Model is still loading, please wait...".to_string());
    }

    let language = with_state(&state, |inner| {
        if inner.status != AppStatus::Idle {
            return Ok(None);
        }

        let model_exists = WhisperEngine::new(&inner.config).model_exists();
        if !model_exists {
            inner.reset_to_idle();
            return Err("Whisper model is missing. Download model first.".to_string());
        }

        inner.status = AppStatus::Recording;
        Ok(Some(inner.config.general.language.clone()))
    })?;

    if language.is_none() {
        return Ok(());
    }

    set_widget_state(&app, "recording", None);

    let app_for_level = app.clone();
    with_state(&state, |inner| {
        inner.recorder.start(move |level| {
            let _ = app_for_level.emit("audio_level", level);
        })
    })?;

    Ok(())
}

async fn cancel_recording_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    with_state(&state, |inner| {
        if inner.status != AppStatus::Recording {
            return Ok(());
        }

        let _ = inner.recorder.stop();
        inner.reset_to_idle();
        Ok(())
    })?;

    set_widget_state(&app, "idle", None);
    Ok(())
}

async fn stop_recording_inner(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();

    let (audio, language, formatting_level, model_path_str, dict_words, dict_replacements) = with_state(&state, |inner| {
        if inner.status != AppStatus::Recording {
            return Ok((Vec::new(), String::new(), String::new(), String::new(), Vec::new(), Vec::new()));
        }

        inner.status = AppStatus::Processing;
        let samples = inner.recorder.stop()?;
        let model_path = model_file_path(&inner.config)
            .to_str()
            .ok_or_else(|| "invalid model path".to_string())?
            .to_string();
        Ok((
            samples,
            inner.config.general.language.clone(),
            inner.config.formatting.level.clone(),
            model_path,
            inner.config.dictionary.words.clone(),
            inner.config.dictionary.replacements.clone(),
        ))
    })?;

    if audio.is_empty() {
        set_widget_state(&app, "idle", None);
        with_state(&state, |inner| {
            inner.reset_to_idle();
            Ok(())
        })?;
        return Ok(String::new());
    }

    if audio.len() < MIN_TRANSCRIBE_SAMPLES {
        dlog!(
            "recording too short ({} samples); skipping transcription",
            audio.len()
        );
        with_state(&state, |inner| {
            inner.reset_to_idle();
            Ok(())
        })?;
        set_widget_state(&app, "idle", None);
        return Ok(String::new());
    }

    set_widget_state(&app, "processing", Some("Transcribing...".to_string()));
    dlog!("pipeline: transcribing {} samples", audio.len());

    let app_clone = app.clone();
    let raw_text = tauri::async_runtime::spawn_blocking(move || {
        let state = app_clone.state::<AppState>();
        let mut ctx_lock = state
            .whisper_ctx
            .lock()
            .map_err(|_| "whisper ctx lock poisoned".to_string())?;

        if ctx_lock.is_none() {
            dlog!("pipeline: loading whisper model (first run)");
            let ctx = whisper::load_context(&model_path_str)?;
            *ctx_lock = Some(SendWhisperCtx(ctx));
            dlog!("pipeline: whisper model loaded and cached");
        }

        whisper::transcribe_with_ctx(&ctx_lock.as_ref().unwrap().0, &audio, &language, &dict_words)
    })
    .await
    .map_err(|e| format!("transcription task failed: {e}"))??;
    dlog!("pipeline: transcription done, raw len={}", raw_text.len());

    let formatted = if formatting_level == "basic" {
        let text = formatter::format_text(&raw_text);
        formatter::apply_replacements(&text, &dict_replacements)
    } else {
        let text = raw_text.trim().to_string();
        formatter::apply_replacements(&text, &dict_replacements)
    };

    if formatted.trim().is_empty() {
        dlog!("empty transcript; skipping paste");
        with_state(&state, |inner| {
            inner.reset_to_idle();
            Ok(())
        })?;
        set_widget_state(&app, "idle", None);
        return Ok(String::new());
    }

    // Hide the widget BEFORE pasting so the target app regains focus.
    // Without this, Cmd+V goes to the widget window (which has focus if
    // the user clicked its stop button) instead of the intended app.
    if let Some(widget) = app.get_webview_window("widget") {
        let _ = widget.hide();
    }
    // Give the target app time to regain focus after the widget hides
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    dlog!("pipeline: injecting text len={}", formatted.len());
    tauri::async_runtime::spawn_blocking({
        let text = formatted.clone();
        move || injector::paste_text(&text)
    })
    .await
    .map_err(|e| format!("injector task failed: {e}"))??;
    dlog!("pipeline: injection done");

    with_state(&state, |inner| {
        inner.reset_to_idle();
        Ok(())
    })?;

    set_widget_state(&app, "idle", None);
    Ok(formatted)
}

#[tauri::command]
async fn start_recording(app: AppHandle) -> Result<(), String> {
    start_recording_inner(app).await
}

#[tauri::command]
async fn stop_recording(app: AppHandle) -> Result<String, String> {
    stop_recording_inner(app).await
}

#[tauri::command]
async fn cancel_recording(app: AppHandle) -> Result<(), String> {
    cancel_recording_inner(app).await
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Result<String, String> {
    with_state(&state, |inner| Ok(inner.status.as_str().to_string()))
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    with_state(&state, |inner| Ok(inner.config.clone()))
}

#[tauri::command]
fn set_recording_mode(state: State<AppState>, mode: String) -> Result<AppConfig, String> {
    let normalized = normalize_recording_mode(&mode)
        .ok_or_else(|| "mode must be 'hold' or 'toggle'".to_string())?;

    with_state(&state, |inner| {
        inner.config.general.mode = normalized.clone();
        inner.toggle_shortcut_held = false;
        save_config(&inner.config)?;
        Ok(inner.config.clone())
    })
}

#[tauri::command]
fn set_model(state: State<AppState>, name: String) -> Result<ModelStatus, String> {
    use config::normalized_model_name;

    let model_name = normalized_model_name(&name).to_string();

    // Update config and invalidate cached WhisperContext
    with_state(&state, |inner| {
        inner.config.model.name = model_name.clone();
        save_config(&inner.config)?;
        Ok(())
    })?;

    // Clear cached context so next transcription loads the new model
    if let Ok(mut ctx_lock) = state.whisper_ctx.lock() {
        *ctx_lock = None;
    }

    // Return model status for the new model
    with_state(&state, |inner| {
        let engine = WhisperEngine::new(&inner.config);
        Ok(ModelStatus {
            exists: engine.model_exists(),
            path: engine.model_path().display().to_string(),
        })
    })
}

#[tauri::command]
fn check_model(state: State<AppState>) -> Result<ModelStatus, String> {
    with_state(&state, |inner| {
        let engine = WhisperEngine::new(&inner.config);
        Ok(ModelStatus {
            exists: engine.model_exists(),
            path: engine.model_path().display().to_string(),
        })
    })
}

async fn run_model_download(
    app: AppHandle,
    model_path: std::path::PathBuf,
    model_name: String,
) -> Result<(), String> {
    if let Some(parent) = model_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create model directory {}: {e}", parent.display()))?;
    }

    let url = model_download_url(&model_name);

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let mut response = reqwest::blocking::get(url)
            .map_err(|e| format!("model download request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "model download failed with status {}",
                response.status()
            ));
        }

        let total = response.content_length().unwrap_or(0);
        let mut file = File::create(&model_path).map_err(|e| {
            format!(
                "failed creating model file {}: {e}",
                model_path.display()
            )
        })?;

        let mut downloaded: u64 = 0;
        let mut buf = [0u8; 16 * 1024];

        loop {
            let read = response
                .read(&mut buf)
                .map_err(|e| format!("failed reading model stream: {e}"))?;
            if read == 0 {
                break;
            }

            file.write_all(&buf[..read])
                .map_err(|e| format!("failed writing model file: {e}"))?;

            downloaded += read as u64;
            if total > 0 {
                let progress = (downloaded as f64 / total as f64).clamp(0.0, 1.0);
                let _ = app.emit("model_download_progress", progress);
            }
        }

        let _ = app.emit("model_download_progress", 1.0_f64);
        Ok(())
    })
    .await
    .map_err(|e| format!("download task failed: {e}"))??;

    Ok(())
}

#[tauri::command]
async fn download_model(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let (model_path, model_name) = with_state(&state, |inner| {
        Ok((
            model_file_path(&inner.config),
            inner.config.model.name.clone(),
        ))
    })?;

    if model_path.exists() {
        return Ok(());
    }

    run_model_download(app, model_path, model_name).await
}

fn build_tray(app: &AppHandle) -> Result<(), String> {
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)
        .map_err(|e| format!("failed creating settings menu item: {e}"))?;
    let about = MenuItem::with_id(app, "about", "About", true, None::<&str>)
        .map_err(|e| format!("failed creating about menu item: {e}"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| format!("failed creating quit menu item: {e}"))?;

    let menu = Menu::with_items(app, &[&settings, &about, &quit])
        .map_err(|e| format!("failed creating tray menu: {e}"))?;

    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .map_err(|e| format!("failed loading tray icon: {e}"))?;

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                show_main_window(app);
            }
            "about" => {
                let _ = app.emit(
                    "status",
                    StatusPayload {
                        status: "idle".to_string(),
                        message: Some("DraVis Flow - local voice transcription".to_string()),
                    },
                );
                show_main_window(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| format!("failed creating tray icon: {e}"))?;

    Ok(())
}

fn handle_shortcut_event(app: &AppHandle, state: ShortcutState) {
    // How it works (WisprFlow-style dual mode):
    // - Hold the hotkey (>= 300 ms): push-to-talk — release stops recording.
    // - Quick tap (< 300 ms): toggle mode — tap again to stop.
    const HOLD_THRESHOLD_MS: u128 = 300;

    #[derive(Clone, Copy)]
    enum ShortcutAction {
        Start,
        Stop,
        ToggleActivated,
    }

    let action = {
        let app_state = app.state::<AppState>();
        let mut lock = match app_state.inner_state.lock() {
            Ok(lock) => lock,
            Err(_) => return,
        };

        // Clean up stale toggle state if recording ended externally (e.g. stop button).
        if lock.toggle_active && lock.status != AppStatus::Recording {
            lock.toggle_active = false;
            lock.press_instant = None;
        }

        match state {
            ShortcutState::Pressed => {
                // Ignore key-repeat events while the key is physically held.
                if lock.toggle_shortcut_held {
                    None
                } else {
                    lock.toggle_shortcut_held = true;
                    if lock.toggle_active {
                        // We're in toggle mode — stop on this press.
                        lock.toggle_active = false;
                        Some(ShortcutAction::Stop)
                    } else if lock.status == AppStatus::Idle {
                        lock.press_instant = Some(std::time::Instant::now());
                        Some(ShortcutAction::Start)
                    } else {
                        None
                    }
                }
            }
            ShortcutState::Released => {
                lock.toggle_shortcut_held = false;
                if lock.status == AppStatus::Recording && !lock.toggle_active {
                    let held_ms = lock
                        .press_instant
                        .map(|t| t.elapsed().as_millis())
                        .unwrap_or(HOLD_THRESHOLD_MS + 1);
                    if held_ms >= HOLD_THRESHOLD_MS {
                        // Held long enough: push-to-talk — stop now.
                        Some(ShortcutAction::Stop)
                    } else {
                        // Quick tap: switch to toggle mode, keep recording.
                        lock.toggle_active = true;
                        Some(ShortcutAction::ToggleActivated)
                    }
                } else {
                    None
                }
            }
        }
    };

    let Some(action) = action else {
        return;
    };

    if matches!(action, ShortcutAction::ToggleActivated) {
        let _ = app.emit("toggle_mode_active", ());
        return;
    }

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let out = match action {
            ShortcutAction::Start => start_recording_inner(app_clone.clone()).await,
            ShortcutAction::Stop => stop_recording_inner(app_clone.clone()).await.map(|_| ()),
            ShortcutAction::ToggleActivated => unreachable!(),
        };

        if let Err(err) = out {
            dlog!("shortcut pipeline failed: {err}");
            set_widget_state(&app_clone, "error", Some(err.clone()));
            if let Ok(mut lock) = app_clone.state::<AppState>().inner_state.lock() {
                lock.reset_to_idle();
                lock.toggle_shortcut_held = false;
                let _ = lock.recorder.stop();
            };
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = load_or_create_config().unwrap_or_else(|e| {
        dlog!("Config load failed, using defaults: {e}");
        AppConfig::default()
    });

    let hotkey_combo = config.general.hotkey.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    let _ = shortcut;
                    handle_shortcut_event(app, event.state);
                })
                .build(),
        )
        .manage(AppState::new(config.clone()))
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            cancel_recording,
            get_status,
            get_config,
            set_recording_mode,
            set_model,
            check_model,
            download_model
        ])
        .setup(move |app| {
            init_logging();
            build_tray(app.handle())?;

            // Prompt for Accessibility permission on first launch so CGEvent paste works.
            // On subsequent launches where permission is already granted this is a no-op.
            #[cfg(target_os = "macos")]
            {
                injector::request_accessibility_permission();
                dlog!("startup: accessibility trusted = {}", injector::is_accessibility_trusted());
            }

            let shortcut_str = hotkey::config_combo_to_shortcut(&hotkey_combo)
                .map_err(|e| format!("invalid hotkey config: {e}"))?;
            let shortcut: tauri_plugin_global_shortcut::Shortcut = shortcut_str
                .parse()
                .map_err(|e| format!("failed to parse shortcut '{shortcut_str}': {e}"))?;
            app.global_shortcut()
                .register(shortcut)
                .map_err(|e| format!("failed to register global shortcut '{shortcut_str}': {e}"))?;

            if let Some(widget) = app.get_webview_window("widget") {
                let _ = widget.hide();
                let _ = position_widget_window(&widget);
            }

            let state = app.state::<AppState>();
            let needs_model = state
                .inner_state
                .lock()
                .map(|s| !WhisperEngine::new(&s.config).model_exists())
                .unwrap_or(true);

            if needs_model {
                show_main_window(app.handle());
            } else {
                // Pre-load WhisperContext in background so first recording is instant.
                // Block recording until pre-load finishes to avoid 30s mutex contention.
                state.model_ready.store(false, Ordering::SeqCst);
                let app_handle = app.handle().clone();
                let model_ready = state.model_ready.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = app_handle.state::<AppState>();
                    let model_path = {
                        let inner = state.inner_state.lock().ok();
                        inner.map(|s| model_file_path(&s.config).display().to_string())
                    };
                    if let Some(path) = model_path {
                        dlog!("startup: pre-loading whisper model from {path}");
                        match whisper::load_context(&path) {
                            Ok(ctx) => {
                                if let Ok(mut lock) = state.whisper_ctx.lock() {
                                    *lock = Some(SendWhisperCtx(ctx));
                                    dlog!("startup: whisper model pre-loaded and cached");
                                }
                                model_ready.store(true, Ordering::SeqCst);
                                let _ = app_handle.emit("model_ready", ());
                            }
                            Err(e) => {
                                dlog!("startup: failed to pre-load model: {e}");
                                // Allow recording to try loading inline
                                model_ready.store(true, Ordering::SeqCst);
                            }
                        }
                    } else {
                        model_ready.store(true, Ordering::SeqCst);
                    }
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { code, .. } = &event {
                dlog!("run event: exit requested (code: {:?})", code);
            }
            if let tauri::RunEvent::Exit = event {
                dlog!("run event: exit");
            }

            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main_window(app_handle);
            }
        });
}
