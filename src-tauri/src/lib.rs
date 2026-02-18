mod audio;
mod config;
mod formatter;
mod hotkey;
mod injector;
mod whisper;

use audio::AudioRecorder;
use config::{AppConfig, load_or_create_config, model_file_path};
use hotkey::HotkeyAction;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use whisper::WhisperEngine;

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

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

struct AppState {
    inner_state: Mutex<InnerState>,
}

struct InnerState {
    status: String,
    recorder: AudioRecorder,
    config: AppConfig,
}

impl AppState {
    fn new(config: AppConfig) -> Self {
        Self {
            inner_state: Mutex::new(InnerState {
                status: "idle".to_string(),
                recorder: AudioRecorder::new(),
                config,
            }),
        }
    }
}

fn set_widget_state(app: &AppHandle, status: &str, message: Option<String>) {
    let payload = StatusPayload {
        status: status.to_string(),
        message,
    };

    let _ = app.emit("status", payload);

    if let Some(widget) = app.get_webview_window("widget") {
        match status {
            "recording" | "processing" | "error" => {
                let _ = position_widget_window(&widget);
                let _ = widget.show();
                let _ = widget.set_focus();
            }
            "idle" => {
                let _ = widget.hide();
            }
            _ => {}
        }
    }
}

fn position_widget_window(window: &WebviewWindow) -> Result<(), String> {
    if let Some(monitor) = window
        .current_monitor()
        .map_err(|e| format!("monitor query failed: {e}"))?
    {
        let size = monitor.size();
        let x = (size.width as f64 / 2.0) - 110.0;
        let y = size.height as f64 - 120.0;
        window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .map_err(|e| format!("widget position failed: {e}"))?;
    }
    Ok(())
}

fn with_state<T>(state: &State<AppState>, f: impl FnOnce(&mut InnerState) -> Result<T, String>) -> Result<T, String> {
    let mut lock = state
        .inner_state
        .lock()
        .map_err(|_| "app state lock poisoned".to_string())?;
    f(&mut lock)
}

async fn start_recording_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let language = with_state(&state, |inner| {
        if inner.status == "recording" {
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

async fn stop_recording_inner(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();

    let (audio, language, formatting_level, config_clone) = with_state(&state, |inner| {
        if inner.status != "recording" {
            return Ok((Vec::new(), String::new(), String::new(), inner.config.clone()));
        }

        inner.status = "processing".to_string();
        let samples = inner.recorder.stop()?;
        Ok((
            samples,
            inner.config.general.language.clone(),
            inner.config.formatting.level.clone(),
            inner.config.clone(),
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

    set_widget_state(&app, "processing", Some("Transcribing...".to_string()));

    let raw_text = tauri::async_runtime::spawn_blocking(move || {
        let engine = WhisperEngine::new(&config_clone);
        engine.transcribe(&audio, &language)
    })
    .await
    .map_err(|e| format!("transcription task failed: {e}"))??;

    let formatted = if formatting_level == "basic" {
        formatter::format_text(&raw_text)
    } else {
        raw_text.trim().to_string()
    };

    tauri::async_runtime::spawn_blocking({
        let text = formatted.clone();
        move || injector::paste_text(&text)
    })
    .await
    .map_err(|e| format!("injector task failed: {e}"))??;

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
fn get_status(state: State<AppState>) -> Result<String, String> {
    with_state(&state, |inner| Ok(inner.status.clone()))
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    with_state(&state, |inner| Ok(inner.config.clone()))
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
        let mut file = File::create(&model_path_clone)
            .map_err(|e| format!("failed creating model file {}: {e}", model_path_clone.display()))?;

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
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "settings" => {
                    if let Some(main) = app.get_webview_window("main") {
                        let _ = main.show();
                        let _ = main.set_focus();
                    }
                }
                "about" => {
                    let _ = app.emit(
                        "status",
                        StatusPayload {
                            status: "idle".to_string(),
                            message: Some("DraVis Flow - local voice transcription".to_string()),
                        },
                    );
                    if let Some(main) = app.get_webview_window("main") {
                        let _ = main.show();
                        let _ = main.set_focus();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }
        })
        .build(app)
        .map_err(|e| format!("failed creating tray icon: {e}"))?;

    Ok(())
}

fn start_hotkey_listener(app: AppHandle, combo: String) {
    let app_for_callback = app.clone();
    let result = hotkey::start_listener(&combo, move |action| {
        let app_clone = app_for_callback.clone();
        tauri::async_runtime::spawn(async move {
            let out = match action {
                HotkeyAction::Pressed => start_recording_inner(app_clone.clone()).await,
                HotkeyAction::Released => stop_recording_inner(app_clone.clone()).await.map(|_| ()),
            };

            if let Err(err) = out {
                set_widget_state(&app_clone, "error", Some(err.clone()));
                if let Ok(mut lock) = app_clone.state::<AppState>().inner_state.lock() {
                    lock.status = "idle".to_string();
                    let _ = lock.recorder.stop();
                };
            }
        });
    });

    if let Err(err) = result {
        eprintln!("hotkey listener start failed: {err}");
    }
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
        .manage(AppState::new(config.clone()))
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            get_status,
            get_config,
            check_model,
            download_model
        ])
        .setup(move |app| {
            build_tray(app.handle())?;
            start_hotkey_listener(app.handle().clone(), hotkey_combo.clone());

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
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
