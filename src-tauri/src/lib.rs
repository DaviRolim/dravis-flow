mod app_setup;
mod audio;
mod commands;
mod config;
mod formatter;
mod hotkey;
mod injector;
mod pipeline;
mod state;
mod whisper;

// Re-export commands so tauri::generate_handler! can find their __cmd__* macros at crate root.
pub use commands::*;

use app_setup::{build_tray, position_widget_window, show_main_window};
use config::{load_or_create_config, model_file_path};
use state::{AppState, AppStatus, SendWhisperCtx, StatusPayload};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use whisper::WhisperEngine;

// ── File-based logging ──────────────────────────────────────────────────

static LOG_FILE: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();
static APP_START: OnceLock<std::time::Instant> = OnceLock::new();

pub(crate) fn init_logging() {
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

const MODE_HOLD: &str = "hold";
const MODE_TOGGLE: &str = "toggle";

pub(crate) fn normalize_recording_mode(mode: &str) -> Option<String> {
    let normalized = mode.trim().to_lowercase();
    match normalized.as_str() {
        MODE_HOLD | MODE_TOGGLE => Some(normalized),
        _ => None,
    }
}

pub(crate) fn sanitize_recording_mode(mode: &str) -> String {
    normalize_recording_mode(mode).unwrap_or_else(|| MODE_HOLD.to_string())
}

pub(crate) fn set_widget_state(app: &AppHandle, status: &str, message: Option<String>) {
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

// ── Shortcut event handler ──────────────────────────────────────────────

fn handle_shortcut_event(app: &AppHandle, state: ShortcutState) {
    // How it works (WisprFlow-style dual mode):
    // - Hold the hotkey (>= 300 ms): push-to-talk — release stops recording.
    // - Quick tap (< 300 ms): toggle mode — tap again to stop.
    // The pure decision logic lives in hotkey::resolve_shortcut_action (unit-tested).

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

        let pressed = matches!(state, ShortcutState::Pressed);
        let is_idle = lock.status == AppStatus::Idle;
        let is_recording = lock.status == AppStatus::Recording;
        let held_ms = lock.press_instant.map(|t| t.elapsed().as_millis());

        let action = hotkey::resolve_shortcut_action(
            pressed,
            is_idle,
            is_recording,
            lock.toggle_shortcut_held,
            lock.toggle_active,
            held_ms,
        );

        // Apply state mutations that must happen inside the lock.
        if pressed {
            if !lock.toggle_shortcut_held {
                lock.toggle_shortcut_held = true;
                if matches!(action, Some(hotkey::ShortcutAction::Stop)) && lock.toggle_active {
                    lock.toggle_active = false;
                } else if matches!(action, Some(hotkey::ShortcutAction::Start)) {
                    lock.press_instant = Some(std::time::Instant::now());
                }
            }
        } else {
            lock.toggle_shortcut_held = false;
            if matches!(action, Some(hotkey::ShortcutAction::ToggleActivated)) {
                lock.toggle_active = true;
            }
        }

        action
    };

    let Some(action) = action else {
        return;
    };

    if matches!(action, hotkey::ShortcutAction::ToggleActivated) {
        let _ = app.emit("toggle_mode_active", ());
        return;
    }

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let out = match action {
            hotkey::ShortcutAction::Start => {
                pipeline::start_recording_inner(app_clone.clone()).await
            }
            hotkey::ShortcutAction::Stop => {
                pipeline::stop_recording_inner(app_clone.clone())
                    .await
                    .map(|_| ())
            }
            hotkey::ShortcutAction::ToggleActivated => unreachable!(),
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

// ── App entry point ─────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = load_or_create_config().unwrap_or_else(|e| {
        dlog!("Config load failed, using defaults: {e}");
        config::AppConfig::default()
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
            download_model,
            set_dictionary_words,
            set_dictionary_replacements
        ])
        .setup(move |app| {
            init_logging();
            build_tray(app.handle())?;

            // Prompt for Accessibility permission on first launch so CGEvent paste works.
            // On subsequent launches where permission is already granted this is a no-op.
            #[cfg(target_os = "macos")]
            {
                injector::request_accessibility_permission();
                dlog!(
                    "startup: accessibility trusted = {}",
                    injector::is_accessibility_trusted()
                );
            }

            let shortcut_str = hotkey::config_combo_to_shortcut(&hotkey_combo)
                .map_err(|e| format!("invalid hotkey config: {e}"))?;
            let shortcut: tauri_plugin_global_shortcut::Shortcut = shortcut_str
                .parse()
                .map_err(|e| format!("failed to parse shortcut '{shortcut_str}': {e}"))?;
            app.global_shortcut()
                .register(shortcut)
                .map_err(|e| {
                    format!("failed to register global shortcut '{shortcut_str}': {e}")
                })?;

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
