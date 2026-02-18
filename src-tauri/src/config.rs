use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

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
                name: "base.en".to_string(),
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

pub fn model_file_path(config: &AppConfig) -> PathBuf {
    let base = models_dir(config);
    base.join("ggml-base.en.bin")
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
        fs::create_dir_all(parent).map_err(|e| format!("failed to create config parent dir: {e}"))?;
    }

    let text = toml::to_string_pretty(config).map_err(|e| format!("failed serializing config: {e}"))?;
    let mut file = fs::File::create(&cfg_path)
        .map_err(|e| format!("failed to create config file {}: {e}", cfg_path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("failed writing config: {e}"))?;
    Ok(())
}
