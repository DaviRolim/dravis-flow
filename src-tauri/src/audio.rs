use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Wrapper to make cpal::Stream usable inside Mutex<InnerState>.
/// Safety: Stream is only accessed behind a Mutex, so concurrent use is impossible.
struct SendStream {
    _stream: Stream,
}
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct AudioRecorder {
    stream: Option<SendStream>,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    cached_device: Option<cpal::Device>,
    cached_config: Option<cpal::SupportedStreamConfig>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        let mut recorder = Self {
            stream: None,
            samples: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 16_000,
            cached_device: None,
            cached_config: None,
        };
        recorder.refresh_device();
        recorder
    }

    /// Re-query the default input device and config. Call when the user switches audio inputs.
    pub fn refresh_device(&mut self) {
        let host = cpal::default_host();
        match host.default_input_device() {
            Some(device) => match device.default_input_config() {
                Ok(config) => {
                    self.cached_device = Some(device);
                    self.cached_config = Some(config);
                }
                Err(e) => {
                    eprintln!("audio: failed to cache device config: {e}");
                    self.cached_device = None;
                    self.cached_config = None;
                }
            },
            None => {
                eprintln!("audio: no default input device found during cache");
                self.cached_device = None;
                self.cached_config = None;
            }
        }
    }

    pub fn start<F>(&mut self, on_level: F) -> Result<(), String>
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        if self.stream.is_some() {
            return Ok(());
        }

        // Populate cache on demand if not available at construction time
        if self.cached_device.is_none() || self.cached_config.is_none() {
            self.refresh_device();
        }

        let device = self
            .cached_device
            .as_ref()
            .ok_or_else(|| "No default microphone found".to_string())?;
        let input_cfg = self
            .cached_config
            .as_ref()
            .ok_or_else(|| "No default input config".to_string())?;

        let (channels, sample_rate, cfg) = {
            let channels = input_cfg.channels() as usize;
            let sample_rate = input_cfg.sample_rate().0;
            let cfg: cpal::StreamConfig = input_cfg.clone().into();
            (channels, sample_rate, cfg)
        };
        self.sample_rate = sample_rate;

        let shared_samples = Arc::clone(&self.samples);
        let on_level: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(on_level);
        let last_emit = Arc::new(Mutex::new(Instant::now()));

        {
            let mut lock = self
                .samples
                .lock()
                .map_err(|_| "audio sample buffer lock poisoned".to_string())?;
            lock.clear();
        }

        let err_fn = |err| eprintln!("audio stream error: {err}");

        let sample_format = input_cfg.sample_format();
        let stream = match sample_format {
            SampleFormat::F32 => {
                let samples = Arc::clone(&shared_samples);
                let level_cb = Arc::clone(&on_level);
                let emit_clock = Arc::clone(&last_emit);
                device
                    .build_input_stream(
                        &cfg,
                        move |data: &[f32], _| {
                            capture_chunk(data, channels, &samples, &level_cb, &emit_clock)
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| format!("failed to build f32 input stream: {e}"))?
            }
            SampleFormat::I16 => {
                let samples = Arc::clone(&shared_samples);
                let level_cb = Arc::clone(&on_level);
                let emit_clock = Arc::clone(&last_emit);
                device
                    .build_input_stream(
                        &cfg,
                        move |data: &[i16], _| {
                            let converted: Vec<f32> = data
                                .iter()
                                .map(|v| (*v as f32) / (i16::MAX as f32))
                                .collect();
                            capture_chunk(&converted, channels, &samples, &level_cb, &emit_clock);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| format!("failed to build i16 input stream: {e}"))?
            }
            SampleFormat::U16 => {
                let samples = Arc::clone(&shared_samples);
                let level_cb = Arc::clone(&on_level);
                let emit_clock = Arc::clone(&last_emit);
                device
                    .build_input_stream(
                        &cfg,
                        move |data: &[u16], _| {
                            let converted: Vec<f32> = data
                                .iter()
                                .map(|v| ((*v as f32) / (u16::MAX as f32)) * 2.0 - 1.0)
                                .collect();
                            capture_chunk(&converted, channels, &samples, &level_cb, &emit_clock);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| format!("failed to build u16 input stream: {e}"))?
            }
            sample => return Err(format!("unsupported sample format: {sample:?}")),
        };

        stream
            .play()
            .map_err(|e| format!("failed to start input stream: {e}"))?;

        self.stream = Some(SendStream { _stream: stream });
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Vec<f32>, String> {
        if self.stream.is_none() {
            return Ok(Vec::new());
        }

        self.stream.take();

        let recorded = self
            .samples
            .lock()
            .map_err(|_| "audio sample buffer lock poisoned".to_string())?
            .clone();

        let resampled = if self.sample_rate == 16_000 {
            recorded
        } else {
            resample_linear(&recorded, self.sample_rate, 16_000)
        };

        Ok(trim_silence(&resampled, 0.01))
    }
}

/// Trim leading and trailing silence below `threshold` RMS (per 480-sample window ≈ 30ms at 16kHz).
/// Keeps a tail padding after the last detected speech so trailing words aren't clipped.
pub(crate) fn trim_silence(samples: &[f32], threshold: f32) -> Vec<f32> {
    const WINDOW: usize = 480;
    // Keep ~0.5s of audio after the last voiced window to avoid clipping trailing words
    const TAIL_PADDING: usize = 16_000 / 2; // 8000 samples = 500ms at 16kHz

    let is_silent = |chunk: &[f32]| -> bool {
        let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
        (sum_sq / chunk.len() as f32).sqrt() < threshold
    };

    // Find first non-silent window
    let first_non_silent = samples.chunks(WINDOW).position(|w| !is_silent(w));
    let start = match first_non_silent {
        Some(i) => i * WINDOW,
        None => return Vec::new(), // all windows are silent
    };

    // Find last non-silent window, then add tail padding
    let end = samples
        .chunks(WINDOW)
        .rposition(|w| !is_silent(w))
        .map(|i| ((i + 1) * WINDOW + TAIL_PADDING).min(samples.len()))
        .unwrap_or(samples.len());

    if start >= end {
        return Vec::new();
    }

    samples[start..end].to_vec()
}

fn capture_chunk(
    input: &[f32],
    channels: usize,
    sample_buf: &Arc<Mutex<Vec<f32>>>,
    on_level: &Arc<dyn Fn(f32) + Send + Sync>,
    last_emit: &Arc<Mutex<Instant>>,
) {
    if channels == 0 {
        return;
    }

    let mut mono = Vec::with_capacity(input.len() / channels.max(1));
    for frame in input.chunks(channels) {
        if let Some(first) = frame.first() {
            mono.push(*first);
        }
    }

    if let Ok(mut lock) = sample_buf.lock() {
        lock.extend_from_slice(&mono);
    }

    let sum_sq: f32 = mono.iter().map(|v| v * v).sum();
    let rms = if mono.is_empty() {
        0.0
    } else {
        (sum_sq / mono.len() as f32).sqrt()
    };

    if let Ok(mut last) = last_emit.lock() {
        if last.elapsed() >= Duration::from_millis(50) {
            *last = Instant::now();
            (on_level)(rms.clamp(0.0, 1.0));
        }
    }
}

pub(crate) fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if input.is_empty() || in_rate == out_rate {
        return input.to_vec();
    }

    let ratio = in_rate as f64 / out_rate as f64;
    let out_len = ((input.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = (i as f64) * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;

        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_identity_when_rates_match() {
        let input: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let output = resample_linear(&input, 16_000, 16_000);
        assert_eq!(input, output);
    }

    #[test]
    fn resample_empty_input() {
        let output = resample_linear(&[], 48_000, 16_000);
        assert!(output.is_empty());
    }

    #[test]
    fn resample_2_to_1_downsample() {
        // 32kHz → 16kHz should roughly halve the length
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin()).collect();
        let output = resample_linear(&input, 32_000, 16_000);
        assert_eq!(output.len(), 500);
    }

    #[test]
    fn resample_3_to_1_downsample() {
        // 48kHz → 16kHz should produce ~1/3 the samples
        let input: Vec<f32> = (0..900).map(|i| (i as f32 * 0.01).sin()).collect();
        let output = resample_linear(&input, 48_000, 16_000);
        assert_eq!(output.len(), 300);
    }

    #[test]
    fn trim_silence_all_silent() {
        let samples = vec![0.0f32; 2000];
        let trimmed = trim_silence(&samples, 0.01);
        assert!(trimmed.is_empty());
    }

    #[test]
    fn trim_silence_preserves_non_silent_middle() {
        // Build: 480 silent + 480 loud + 480 silent
        let mut samples = vec![0.0f32; 480];
        samples.extend(vec![0.5f32; 480]);
        samples.extend(vec![0.0f32; 480]);

        let trimmed = trim_silence(&samples, 0.01);
        // Should contain the loud window plus tail padding (up to sample count limit)
        // The loud window is 480 samples, then tail padding extends into the silent region
        assert!(trimmed.len() >= 480);
        assert!(trimmed.len() <= 480 + 480); // can't exceed total trailing samples
        // First 480 samples must be the loud ones
        assert!(trimmed[..480].iter().all(|&s| s == 0.5));
    }

    #[test]
    fn trim_silence_no_trimming_when_all_loud() {
        let samples = vec![0.5f32; 960];
        let trimmed = trim_silence(&samples, 0.01);
        assert_eq!(trimmed.len(), 960);
    }
}
