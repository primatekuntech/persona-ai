/// Audio transcription via whisper.cpp (whisper-rs crate).
/// Runs entirely inside `tokio::task::spawn_blocking`.
///
/// # Build note
/// The `whisper-rs` crate requires cmake + C++ toolchain (compiles whisper.cpp).
/// Enable with: `cargo build --features whisper`.
/// Without the feature, `transcribe()` returns an error immediately.
use std::path::Path;

const MAX_DURATION_SEC: f64 = 6.0 * 3600.0; // 6 hours

pub struct Transcriber {
    model_path: std::path::PathBuf,
}

impl Transcriber {
    /// Load the whisper model.
    pub fn new(model_path: &Path) -> Result<Self, anyhow::Error> {
        if !model_path.exists() {
            return Err(anyhow::anyhow!(
                "Whisper model not found at: {}",
                model_path.display()
            ));
        }
        Ok(Self {
            model_path: model_path.to_path_buf(),
        })
    }

    /// Transcribe an audio file to text.
    /// Returns `(transcript_text, duration_sec)`.
    /// `progress_cb` is called with values 0..=100 as transcription progresses.
    pub fn transcribe(
        &self,
        audio_path: &Path,
        progress_cb: impl Fn(i16),
    ) -> Result<(String, i32), anyhow::Error> {
        // Step 1: Probe duration with ffprobe
        let duration_sec = probe_duration(audio_path)?;
        if duration_sec > MAX_DURATION_SEC {
            return Err(anyhow::anyhow!(
                "audio_too_long: duration {:.0}s exceeds {:.0}s limit",
                duration_sec,
                MAX_DURATION_SEC
            ));
        }

        // Step 2: Convert to 16 kHz mono WAV via ffmpeg
        let tmp_dir = tempfile::TempDir::new()?;
        let wav_path = tmp_dir.path().join("audio.wav");
        let ffmpeg_status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                audio_path.to_str().unwrap_or_default(),
                "-ar",
                "16000",
                "-ac",
                "1",
                "-t",
                "18000", // hard cap 5h per security spec
                "-f",
                "wav",
                wav_path.to_str().unwrap_or_default(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;

        if !ffmpeg_status.success() {
            return Err(anyhow::anyhow!("ffmpeg conversion failed"));
        }

        progress_cb(10);

        // Step 3: Run whisper.
        //
        // TODO: Uncomment when building with `--features whisper`.
        // The whisper-rs 0.11 API (approximate):
        //
        //   use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};
        //   let ctx = WhisperContext::new_with_params(
        //       self.model_path.to_str().unwrap_or_default(),
        //       WhisperContextParameters::default(),
        //   )?;
        //   let mut state = ctx.create_state()?;
        //   let samples = read_wav_samples(&wav_path)?;
        //   let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        //   params.set_language(Some("en"));
        //   state.full(params, &samples)?;
        //   let n_segments = state.full_n_segments()?;
        //   let mut transcript = String::new();
        //   for i in 0..n_segments {
        //       transcript.push_str(&state.full_get_segment_text(i)?);
        //       progress_cb((10 + (i as f32 / n_segments as f32 * 90.0) as i16).min(99));
        //   }
        //   progress_cb(100);
        //   return Ok((transcript, duration_sec as i32));

        let _ = &self.model_path; // suppress unused warning
        let _ = wav_path;
        Err(anyhow::anyhow!(
            "Whisper not enabled — build with --features whisper and ensure model files exist at /data/models/"
        ))
    }
}

/// Run ffprobe to get the duration in seconds.
fn probe_duration(path: &Path) -> Result<f64, anyhow::Error> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            path.to_str().unwrap_or_default(),
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("ffprobe failed on audio file"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = json["streams"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No streams in ffprobe output"))?;

    for stream in streams {
        if let Some(dur_str) = stream["duration"].as_str() {
            if let Ok(dur) = dur_str.parse::<f64>() {
                return Ok(dur);
            }
        }
    }

    Err(anyhow::anyhow!(
        "Could not determine audio duration from ffprobe output"
    ))
}
