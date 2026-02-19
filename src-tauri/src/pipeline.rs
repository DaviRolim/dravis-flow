use crate::config::model_file_path;
use crate::state::{with_state, AppState, AppStatus, SendWhisperCtx};
use crate::{dlog, set_widget_state};
use crate::{formatter, prompt, whisper};
use std::fs::{self, File};
use std::io::{Read, Write};
use tauri::{AppHandle, Emitter, Manager};
use whisper::WhisperEngine;

/// Minimum samples required to attempt transcription (~1s at 16 kHz).
const MIN_TRANSCRIBE_SAMPLES: usize = 16_000;

pub async fn start_recording_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    if !state.model_ready.load(std::sync::atomic::Ordering::SeqCst) {
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

pub async fn cancel_recording_inner(app: AppHandle) -> Result<(), String> {
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

pub async fn stop_recording_inner(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();

    let (
        audio,
        language,
        formatting_level,
        model_path_str,
        dict_words,
        dict_replacements,
        prompt_mode_enabled,
        prompt_mode_provider,
        prompt_mode_model,
        prompt_mode_api_key,
    ) = with_state(&state, |inner| {
        if inner.status != AppStatus::Recording {
            return Ok((
                Vec::new(),
                String::new(),
                String::new(),
                String::new(),
                Vec::new(),
                Vec::new(),
                false,
                String::new(),
                String::new(),
                String::new(),
            ));
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
            inner.config.prompt_mode.enabled,
            inner.config.prompt_mode.provider.clone(),
            inner.config.prompt_mode.model.clone(),
            inner.config.prompt_mode.api_key.clone(),
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

        whisper::transcribe_with_ctx(
            &ctx_lock.as_ref().unwrap().0,
            &audio,
            &language,
            &dict_words,
        )
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

    let mut output_text = formatted;
    if prompt_mode_enabled && !prompt_mode_api_key.trim().is_empty() {
        set_widget_state(
            &app,
            "structuring",
            Some("Structuring prompt...".to_string()),
        );

        match prompt::structure_prompt(
            &output_text,
            &prompt_mode_provider,
            &prompt_mode_model,
            &prompt_mode_api_key,
        )
        .await
        {
            Ok(structured) if !structured.trim().is_empty() => {
                dlog!(
                    "pipeline: prompt structuring done, len={}",
                    structured.len()
                );
                output_text = structured;
            }
            Ok(_) => {
                dlog!("pipeline: prompt structuring returned empty text, falling back");
            }
            Err(err) => {
                dlog!("pipeline: prompt structuring failed, falling back: {err}");
            }
        }
    }

    // Hide the widget BEFORE pasting so the target app regains focus.
    // Without this, Cmd+V goes to the widget window (which has focus if
    // the user clicked its stop button) instead of the intended app.
    if let Some(widget) = app.get_webview_window("widget") {
        let _ = widget.hide();
    }
    // Give the target app time to regain focus after the widget hides
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    dlog!("pipeline: injecting text len={}", output_text.len());
    tauri::async_runtime::spawn_blocking({
        let text = output_text.clone();
        move || crate::injector::paste_text(&text)
    })
    .await
    .map_err(|e| format!("injector task failed: {e}"))??;
    dlog!("pipeline: injection done");

    with_state(&state, |inner| {
        inner.reset_to_idle();
        Ok(())
    })?;

    set_widget_state(&app, "idle", None);
    Ok(output_text)
}

pub async fn run_model_download(
    app: AppHandle,
    model_path: std::path::PathBuf,
    model_name: String,
) -> Result<(), String> {
    use tauri::Emitter;

    if let Some(parent) = model_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create model directory {}: {e}", parent.display()))?;
    }

    let url = crate::config::model_download_url(&model_name);

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
        let mut file = File::create(&model_path)
            .map_err(|e| format!("failed creating model file {}: {e}", model_path.display()))?;

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
