use crate::config::model_file_path;
use crate::config::AppConfig;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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

    pub fn transcribe(&self, audio: &[f32], language: &str) -> Result<String, String> {
        if !self.model_exists() {
            return Err(format!(
                "Model file not found at {}",
                self.model_path.display()
            ));
        }

        let model = self
            .model_path
            .to_str()
            .ok_or_else(|| "invalid model path".to_string())?;

        let mut ctx_params = WhisperContextParameters::default();
        #[cfg(target_os = "macos")]
        {
            // Work around intermittent Metal backend teardown crashes on Apple Silicon.
            // CPU mode is slower but significantly more stable for dev/runtime.
            ctx_params.use_gpu(false);
        }

        let ctx = WhisperContext::new_with_params(model, ctx_params)
            .map_err(|e| format!("failed to load whisper model: {e}"))?;

        let mut state = ctx
            .create_state()
            .map_err(|e| format!("failed creating whisper state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_translate(false);
        params.set_language(Some(language));
        params.set_n_threads(4);

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
}
