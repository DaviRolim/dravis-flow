use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

pub const MODEL_BASE_EN: &str = "base.en";
pub const MODEL_SMALL_EN: &str = "small.en";
pub const MODEL_LARGE_V3_TURBO: &str = "large-v3-turbo";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub language: String,
    pub hotkey: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormattingConfig {
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub model: ModelConfig,
    pub formatting: FormattingConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig {
                language: "en".to_string(),
                hotkey: "ctrl+shift+space".to_string(),
                mode: "hold".to_string(),
            },
            model: ModelConfig {
                name: MODEL_BASE_EN.to_string(),
                path: "~/.dravis-flow/models/".to_string(),
            },
            formatting: FormattingConfig {
                level: "basic".to_string(),
            },
        }
    }
}

pub fn config_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dravis-flow")
}

pub fn models_dir(config: &AppConfig) -> PathBuf {
    let path = config.model.path.trim();
    if let Some(stripped) = path.strip_prefix("~/") {
        return home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(stripped);
    }
    PathBuf::from(path)
}

pub fn canonical_model_name(name: &str) -> Option<&'static str> {
    match name.trim() {
        MODEL_BASE_EN => Some(MODEL_BASE_EN),
        MODEL_SMALL_EN => Some(MODEL_SMALL_EN),
        MODEL_LARGE_V3_TURBO => Some(MODEL_LARGE_V3_TURBO),
        _ => None,
    }
}

pub fn normalized_model_name(name: &str) -> &'static str {
    canonical_model_name(name).unwrap_or(MODEL_BASE_EN)
}

pub fn model_filename(model_name: &str) -> &'static str {
    match normalized_model_name(model_name) {
        MODEL_SMALL_EN => "ggml-small.en.bin",
        MODEL_LARGE_V3_TURBO => "ggml-large-v3-turbo.bin",
        _ => "ggml-base.en.bin",
    }
}

#[cfg(test)]
pub fn model_download_url(model_name: &str) -> &'static str {
    match normalized_model_name(model_name) {
        MODEL_SMALL_EN => {
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"
        }
        MODEL_LARGE_V3_TURBO => {
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
        }
        _ => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
    }
}

pub fn model_file_path(config: &AppConfig) -> PathBuf {
    let base = models_dir(config);
    base.join(model_filename(&config.model.name))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_or_create_config() -> Result<AppConfig, String> {
    let cfg_dir = config_dir();
    fs::create_dir_all(&cfg_dir).map_err(|e| format!("failed to create config dir: {e}"))?;

    let cfg_path = config_path();
    if !cfg_path.exists() {
        let default_cfg = AppConfig::default();
        save_config(&default_cfg)?;
        return Ok(default_cfg);
    }

    let content = fs::read_to_string(&cfg_path)
        .map_err(|e| format!("failed reading config {}: {e}", cfg_path.display()))?;
    toml::from_str::<AppConfig>(&content)
        .map_err(|e| format!("failed parsing config {}: {e}", cfg_path.display()))
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let cfg_path = config_path();
    if let Some(parent) = cfg_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create config parent dir: {e}"))?;
    }

    let text =
        toml::to_string_pretty(config).map_err(|e| format!("failed serializing config: {e}"))?;
    let mut file = fs::File::create(&cfg_path)
        .map_err(|e| format!("failed to create config file {}: {e}", cfg_path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("failed writing config: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_model_names_to_expected_files_and_urls() {
        assert_eq!(model_filename(MODEL_BASE_EN), "ggml-base.en.bin");
        assert_eq!(model_filename(MODEL_SMALL_EN), "ggml-small.en.bin");
        assert_eq!(model_filename(MODEL_LARGE_V3_TURBO), "ggml-large-v3-turbo.bin");

        assert_eq!(
            model_download_url(MODEL_BASE_EN),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
        );
        assert_eq!(
            model_download_url(MODEL_SMALL_EN),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"
        );
        assert_eq!(
            model_download_url(MODEL_LARGE_V3_TURBO),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
        );
    }

    #[test]
    fn config_toml_roundtrip_keeps_model_name() {
        let mut config = AppConfig::default();
        config.model.name = MODEL_SMALL_EN.to_string();
        let encoded = toml::to_string_pretty(&config).expect("serialize config");
        let decoded = toml::from_str::<AppConfig>(&encoded).expect("parse config");
        assert_eq!(decoded.model.name, MODEL_SMALL_EN);
    }
}
