use crate::config::{
    default_prompt_model, model_file_path, normalized_model_name, normalized_prompt_provider,
    save_config, AppConfig,
};
use crate::pipeline::{
    cancel_recording_inner, run_model_download, start_recording_inner, stop_recording_inner,
};
use crate::state::{with_state, AppState, ModelStatus};
use crate::whisper::WhisperEngine;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn start_recording(app: AppHandle) -> Result<(), String> {
    start_recording_inner(app).await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Result<String, String> {
    stop_recording_inner(app).await
}

#[tauri::command]
pub async fn cancel_recording(app: AppHandle) -> Result<(), String> {
    cancel_recording_inner(app).await
}

#[tauri::command]
pub fn get_status(state: State<AppState>) -> Result<String, String> {
    with_state(&state, |inner| Ok(inner.status.as_str().to_string()))
}

#[tauri::command]
pub fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    with_state(&state, |inner| Ok(inner.config.clone()))
}

#[tauri::command]
pub fn set_recording_mode(state: State<AppState>, mode: String) -> Result<AppConfig, String> {
    let normalized = crate::normalize_recording_mode(&mode)
        .ok_or_else(|| "mode must be 'hold' or 'toggle'".to_string())?;

    with_state(&state, |inner| {
        inner.config.general.mode = normalized.clone();
        inner.toggle_shortcut_held = false;
        save_config(&inner.config)?;
        Ok(inner.config.clone())
    })
}

#[tauri::command]
pub fn set_prompt_mode(
    state: State<AppState>,
    enabled: bool,
    provider: String,
    model: String,
    api_key: String,
) -> Result<AppConfig, String> {
    let normalized_provider = normalized_prompt_provider(&provider).to_string();
    let normalized_model = if model.trim().is_empty() {
        default_prompt_model(&normalized_provider).to_string()
    } else {
        model.trim().to_string()
    };

    with_state(&state, |inner| {
        inner.config.prompt_mode.enabled = enabled;
        inner.config.prompt_mode.provider = normalized_provider.clone();
        inner.config.prompt_mode.model = normalized_model.clone();
        inner.config.prompt_mode.api_key = api_key.trim().to_string();
        save_config(&inner.config)?;
        Ok(inner.config.clone())
    })
}

#[tauri::command]
pub fn set_model(state: State<AppState>, name: String) -> Result<ModelStatus, String> {
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
pub fn check_model(state: State<AppState>) -> Result<ModelStatus, String> {
    with_state(&state, |inner| {
        let engine = WhisperEngine::new(&inner.config);
        Ok(ModelStatus {
            exists: engine.model_exists(),
            path: engine.model_path().display().to_string(),
        })
    })
}

#[tauri::command]
pub fn set_dictionary_words(
    state: State<AppState>,
    words: Vec<String>,
) -> Result<AppConfig, String> {
    with_state(&state, |inner| {
        inner.config.dictionary.words = words;
        save_config(&inner.config)?;
        Ok(inner.config.clone())
    })
}

#[tauri::command]
pub fn set_dictionary_replacements(
    state: State<AppState>,
    replacements: Vec<crate::config::ReplacementEntry>,
) -> Result<AppConfig, String> {
    with_state(&state, |inner| {
        inner.config.dictionary.replacements = replacements;
        save_config(&inner.config)?;
        Ok(inner.config.clone())
    })
}

#[tauri::command]
pub async fn download_model(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
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
