mod audio;
mod config;
mod formatter;
mod hotkey;
mod injector;
mod whisper;

use audio::AudioRecorder;
use config::{load_or_create_config, model_file_path, save_config, AppConfig};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use whisper::WhisperEngine;
use whisper_rs::WhisperContext;

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
const MIN_TRANSCRIBE_SAMPLES: usize = 16_000; // ~1s at 16 kHz
const MODE_HOLD: &str = "hold";
const MODE_TOGGLE: &str = "toggle";

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
}

struct InnerState {
    status: String,
    recorder: AudioRecorder,
    config: AppConfig,
}

impl AppState {
    fn new(config: AppConfig) -> Self {
        let mut config = config;
        config.general.mode = sanitize_recording_mode(&config.general.mode);

        Self {
            inner_state: Mutex::new(InnerState {
                status: "idle".to_string(),
                recorder: AudioRecorder::new(),
                config,
            }),
            whisper_ctx: Mutex::new(None),
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

    let language = with_state(&state, |inner| {
        if inner.status != "idle" {
            return Ok(None);
        }

        let model_exists = WhisperEngine::new(&inner.config).model_exists();
        if !model_exists {
            inner.status = "idle".to_string();
            return Err("Whisper model is missing. Download model first.".to_string());
        }

        inner.status = "recording".to_string();
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
        if inner.status != "recording" {
            return Ok(());
        }

        let _ = inner.recorder.stop();
        inner.status = "idle".to_string();
        Ok(())
    })?;

    set_widget_state(&app, "idle", None);
    Ok(())
}

async fn stop_recording_inner(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();

    let (audio, language, formatting_level, model_path_str) = with_state(&state, |inner| {
        if inner.status != "recording" {
            return Ok((Vec::new(), String::new(), String::new(), String::new()));
        }

        inner.status = "processing".to_string();
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
        ))
    })?;

    if audio.is_empty() {
        set_widget_state(&app, "idle", None);
        with_state(&state, |inner| {
            inner.status = "idle".to_string();
            Ok(())
        })?;
        return Ok(String::new());
    }

    if audio.len() < MIN_TRANSCRIBE_SAMPLES {
        eprintln!(
            "recording too short ({} samples); skipping transcription",
            audio.len()
        );
        with_state(&state, |inner| {
            inner.status = "idle".to_string();
            Ok(())
        })?;
        set_widget_state(&app, "idle", None);
        return Ok(String::new());
    }

    set_widget_state(&app, "processing", Some("Transcribing...".to_string()));
    eprintln!("pipeline: transcribing {} samples", audio.len());

    let app_clone = app.clone();
    let raw_text = tauri::async_runtime::spawn_blocking(move || {
        let state = app_clone.state::<AppState>();
        let mut ctx_lock = state
            .whisper_ctx
            .lock()
            .map_err(|_| "whisper ctx lock poisoned".to_string())?;

        if ctx_lock.is_none() {
            eprintln!("pipeline: loading whisper model (first run)");
            let ctx = whisper::load_context(&model_path_str)?;
            *ctx_lock = Some(SendWhisperCtx(ctx));
            eprintln!("pipeline: whisper model loaded and cached");
        }

        whisper::transcribe_with_ctx(&ctx_lock.as_ref().unwrap().0, &audio, &language)
    })
    .await
    .map_err(|e| format!("transcription task failed: {e}"))??;
    eprintln!("pipeline: transcription done, raw len={}", raw_text.len());

    let formatted = if formatting_level == "basic" {
        formatter::format_text(&raw_text)
    } else {
        raw_text.trim().to_string()
    };

    if formatted.trim().is_empty() {
        eprintln!("empty transcript; skipping paste");
        with_state(&state, |inner| {
            inner.status = "idle".to_string();
            Ok(())
        })?;
        set_widget_state(&app, "idle", None);
        return Ok(String::new());
    }

    eprintln!("pipeline: injecting text len={}", formatted.len());
    tauri::async_runtime::spawn_blocking({
        let text = formatted.clone();
        move || injector::paste_text(&text)
    })
    .await
    .map_err(|e| format!("injector task failed: {e}"))??;
    eprintln!("pipeline: injection done");

    with_state(&state, |inner| {
        inner.status = "idle".to_string();
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
    with_state(&state, |inner| Ok(inner.status.clone()))
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
        save_config(&inner.config)?;
        Ok(inner.config.clone())
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

#[tauri::command]
async fn download_model(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let model_path = with_state(&state, |inner| Ok(model_file_path(&inner.config)))?;

    if model_path.exists() {
        return Ok(());
    }

    if let Some(parent) = model_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create model directory {}: {e}", parent.display()))?;
    }

    let model_path_clone = model_path.clone();
    let app_clone = app.clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let mut response = reqwest::blocking::get(MODEL_URL)
            .map_err(|e| format!("model download request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "model download failed with status {}",
                response.status()
            ));
        }

        let total = response.content_length().unwrap_or(0);
        let mut file = File::create(&model_path_clone).map_err(|e| {
            format!(
                "failed creating model file {}: {e}",
                model_path_clone.display()
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
                let _ = app_clone.emit("model_download_progress", progress);
            }
        }

        let _ = app_clone.emit("model_download_progress", 1.0_f64);
        Ok(())
    })
    .await
    .map_err(|e| format!("download task failed: {e}"))??;

    Ok(())
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
    #[derive(Clone, Copy)]
    enum ShortcutAction {
        Start,
        Stop,
    }

    let action = {
        let app_state = app.state::<AppState>();
        let lock = match app_state.inner_state.lock() {
            Ok(lock) => lock,
            Err(_) => return,
        };

        let mode = sanitize_recording_mode(&lock.config.general.mode);
        match mode.as_str() {
            MODE_TOGGLE => {
                if state != ShortcutState::Pressed {
                    None
                } else if lock.status == "recording" {
                    Some(ShortcutAction::Stop)
                } else if lock.status == "idle" {
                    Some(ShortcutAction::Start)
                } else {
                    None
                }
            }
            _ => match state {
                ShortcutState::Pressed if lock.status == "idle" => Some(ShortcutAction::Start),
                ShortcutState::Released if lock.status == "recording" => Some(ShortcutAction::Stop),
                _ => None,
            },
        }
    };

    let Some(action) = action else {
        return;
    };

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let out = match action {
            ShortcutAction::Start => start_recording_inner(app_clone.clone()).await,
            ShortcutAction::Stop => stop_recording_inner(app_clone.clone()).await.map(|_| ()),
        };

        if let Err(err) = out {
            eprintln!("shortcut pipeline failed: {err}");
            set_widget_state(&app_clone, "error", Some(err.clone()));
            if let Ok(mut lock) = app_clone.state::<AppState>().inner_state.lock() {
                lock.status = "idle".to_string();
                let _ = lock.recorder.stop();
            };
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = load_or_create_config().unwrap_or_else(|e| {
        eprintln!("Config load failed, using defaults: {e}");
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
            check_model,
            download_model
        ])
        .setup(move |app| {
            build_tray(app.handle())?;

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
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { code, .. } = &event {
                eprintln!("run event: exit requested (code: {:?})", code);
            }
            if let tauri::RunEvent::Exit = event {
                eprintln!("run event: exit");
            }

            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main_window(app_handle);
            }
        });
}
