use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::{
    render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport},
    voice_rider::{analyze_automatic_cleanup_profile_with_paths, AutomaticCleanupProfile},
};

const ENGINE_NAME: &str = "rnnoise";
const MODEL_NAME: &str = "rnnoise-bd-v1";
const MODEL_FILE_NAME: &str = "rnnoise-bd-v1.rnnn";
const MODEL_URL: &str = "https://raw.githubusercontent.com/GregorR/rnnoise-models/3eee541a283fd3b8f81b85b1748e3b9ccbefa04d/beguiling-drafter-2018-08-30/bd.rnnn";
const MODEL_SHA256: &str = "ae3f7411e1e6a884f839a4a145c394408398f09854dbc1216ee02faafc98a17b";
// Bump this whenever the rendered filter graph changes. Otherwise a clip can
// report the new analysis while still playing an older, weaker cached WAV.
const CACHE_SCHEMA: &str = "rnnoise-clean-v4-dialogue-clarity";
const AUTO_HIGH_PASS_HZ: u32 = 70;
// RNNoise does the heavy non-stationary cleanup. Keep the residual FFT pass
// conservative so consonants and room tone do not turn into a muffled,
// underwater voice, then restore a small amount of dialogue presence.
const AUTO_SPECTRAL_DENOISE: &str = "afftdn=nr=7:nf=-55:tn=1:tr=1:ad=0.75:gs=12:nl=average";
const DIALOGUE_BODY_EQ: &str = "equalizer=f=280:t=q:w=1.1:g=-1.5";
const DIALOGUE_PRESENCE_EQ: &str = "equalizer=f=3200:t=q:w=0.9:g=1.8";
const DIALOGUE_AIR_EQ: &str = "treble=f=6500:t=q:w=0.7:g=0.8";
const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_MODEL_BYTES: u64 = 2 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 28;

static PREPARE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AudioCleanupEngine {
    Rnnoise,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AudioCleanupModel {
    RnnoiseBdV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioCleanupSettings {
    pub engine: AudioCleanupEngine,
    pub model: AudioCleanupModel,
    pub strength: f64,
    #[serde(default)]
    pub high_pass_hz: u32,
    #[serde(default)]
    pub hum_frequency: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCleanupRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    pub settings: AudioCleanupSettings,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCleanupAsset {
    pub output_path: String,
    pub duration_ms: u64,
    pub cache_hit: bool,
    pub engine: &'static str,
    pub model: &'static str,
    pub applied: bool,
    pub speech_coverage: f64,
    pub estimated_snr_db: f64,
    pub strength: f64,
}

#[tauri::command]
pub async fn prepare_audio_cleanup(
    app: AppHandle,
    request: AudioCleanupRequest,
) -> Result<AudioCleanupAsset, RenderErrorReport> {
    validate_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = PREPARE_LOCK.lock().map_err(|_| {
            cleanup_error(
                "audio_cleanup_lock_failed",
                "AI ses temizleme başlatılamadı.",
                "Audio cleanup preparation lock was poisoned",
                true,
            )
        })?;
        prepare_blocking(&app, &ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        cleanup_error(
            "audio_cleanup_worker_failed",
            "AI ses temizleme tamamlanamadı.",
            format!("Audio cleanup worker failed: {error}"),
            true,
        )
    })?
}

fn prepare_blocking(
    app: &AppHandle,
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AudioCleanupRequest,
) -> Result<AudioCleanupAsset, RenderErrorReport> {
    let model_path = ensure_model(app)?;
    let cache_dir = cleanup_cache_dir(app)?;
    fs::create_dir_all(&cache_dir).map_err(|error| {
        cleanup_error(
            "audio_cleanup_cache_failed",
            "AI ses önbelleği hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let cache_key = cleanup_cache_key(source_path, request)?;
    let output_path = cache_dir.join(format!("{cache_key}.clean.wav"));
    let profile = analyze_automatic_cleanup_profile_with_paths(
        ffmpeg_path,
        source_path,
        request.start_ms,
        request.duration_ms,
    )?;
    if reusable_cache_file(&output_path) {
        return Ok(asset(output_path, request.duration_ms, true, profile));
    }

    let partial_path = cache_dir.join(format!(".{cache_key}.partial.wav"));
    if partial_path.is_file() {
        let _ = fs::remove_file(&partial_path);
    }
    let filters = cleanup_filters(request, &model_path, profile);
    let mut command = Command::new(ffmpeg_path);
    command
        .args(["-hide_banner", "-nostats", "-nostdin", "-y", "-i"])
        .arg(source_path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af"])
        .arg(filters.join(","))
        .args([
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
            "-loglevel",
            "warning",
        ])
        .arg(&partial_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let output = command.output().map_err(|error| {
        cleanup_error(
            "audio_cleanup_ffmpeg_failed",
            "AI ses motoru başlatılamadı.",
            format!("Failed to start {}: {error}", ffmpeg_path.display()),
            true,
        )
    })?;
    if !output.status.success() || !reusable_cache_file(&partial_path) {
        let _ = fs::remove_file(&partial_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail = stderr
            .lines()
            .rev()
            .take(STDERR_TAIL_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        return Err(cleanup_error_with_stderr(
            "audio_cleanup_ffmpeg_failed",
            "AI gürültü azaltma sesi işleyemedi.",
            format!("FFmpeg exited with {}", output.status),
            true,
            tail,
        ));
    }
    if output_path.is_file() {
        let _ = fs::remove_file(&output_path);
    }
    fs::rename(&partial_path, &output_path).map_err(|error| {
        cleanup_error(
            "audio_cleanup_cache_commit_failed",
            "Temizlenen ses önbelleğe alınamadı.",
            error.to_string(),
            true,
        )
    })?;
    Ok(asset(output_path, request.duration_ms, false, profile))
}

fn cleanup_filters(
    request: &AudioCleanupRequest,
    model_path: &Path,
    profile: AutomaticCleanupProfile,
) -> Vec<String> {
    let duration = seconds(request.duration_ms);
    let mut filters = vec![
        format!(
            "atrim=start={}:duration={duration}",
            seconds(request.start_ms)
        ),
        "asetpts=PTS-STARTPTS".into(),
        "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo".into(),
    ];
    if profile.speech_detected {
        filters.push(format!("highpass=f={AUTO_HIGH_PASS_HZ}:p=2"));
        filters.extend([
            format!(
                "arnndn=m='{}':mix={}",
                escape_filter_path(model_path),
                number(profile.strength)
            ),
            // RNNoise removes the non-stationary bulk. The conservative,
            // tracking FFT pass then suppresses residual fan/motor/hiss in
            // quiet windows without using a hard gate that clips word tails.
            AUTO_SPECTRAL_DENOISE.into(),
            DIALOGUE_BODY_EQ.into(),
            DIALOGUE_PRESENCE_EQ.into(),
            DIALOGUE_AIR_EQ.into(),
            "alimiter=limit=0.98:latency=1".into(),
        ]);
    }
    filters.extend([
        format!("apad=pad_dur={duration}"),
        format!("atrim=duration={duration}"),
        "asetpts=PTS-STARTPTS".into(),
    ]);
    filters
}

fn ensure_model(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    let model_dir = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("audio-models"))
        .map_err(|error| {
            cleanup_error(
                "audio_cleanup_model_directory_failed",
                "AI model klasörü hazırlanamadı.",
                error.to_string(),
                true,
            )
        })?;
    fs::create_dir_all(&model_dir).map_err(|error| {
        cleanup_error(
            "audio_cleanup_model_directory_failed",
            "AI model klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let model_path = model_dir.join(MODEL_FILE_NAME);
    if model_matches(&model_path)? {
        return Ok(model_path);
    }
    let partial_path = model_dir.join(format!(".{MODEL_FILE_NAME}.partial"));
    if partial_path.is_file() {
        let _ = fs::remove_file(&partial_path);
    }
    let mut response = ureq::get(MODEL_URL).call().map_err(|error| {
        cleanup_error(
            "audio_cleanup_model_download_failed",
            "AI gürültü azaltma modeli indirilemedi. İnternet bağlantısını kontrol edin.",
            error.to_string(),
            true,
        )
    })?;
    let mut file = File::create(&partial_path).map_err(|error| {
        cleanup_error(
            "audio_cleanup_model_write_failed",
            "AI modeli diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    let downloaded_bytes = io::copy(
        &mut response
            .body_mut()
            .as_reader()
            .take(MAX_MODEL_BYTES.saturating_add(1)),
        &mut file,
    )
    .map_err(|error| {
        cleanup_error(
            "audio_cleanup_model_download_failed",
            "AI gürültü azaltma modeli indirilemedi.",
            error.to_string(),
            true,
        )
    })?;
    if downloaded_bytes > MAX_MODEL_BYTES {
        drop(file);
        let _ = fs::remove_file(&partial_path);
        return Err(cleanup_error(
            "audio_cleanup_model_too_large",
            "AI modelinin güvenlik doğrulaması başarısız oldu.",
            format!("Model download exceeded {MAX_MODEL_BYTES} bytes"),
            false,
        ));
    }
    file.flush().ok();
    file.sync_all().ok();
    drop(file);
    if !model_matches(&partial_path)? {
        let _ = fs::remove_file(&partial_path);
        return Err(cleanup_error(
            "audio_cleanup_model_checksum_failed",
            "AI modelinin güvenlik doğrulaması başarısız oldu.",
            format!("Expected SHA-256 {MODEL_SHA256}"),
            true,
        ));
    }
    if model_path.is_file() {
        let _ = fs::remove_file(&model_path);
    }
    fs::rename(&partial_path, &model_path).map_err(|error| {
        cleanup_error(
            "audio_cleanup_model_commit_failed",
            "AI modeli kurulamadı.",
            error.to_string(),
            true,
        )
    })?;
    Ok(model_path)
}

fn model_matches(path: &Path) -> Result<bool, RenderErrorReport> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|error| {
        cleanup_error(
            "audio_cleanup_model_read_failed",
            "AI modeli okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            cleanup_error(
                "audio_cleanup_model_read_failed",
                "AI modeli okunamadı.",
                error.to_string(),
                true,
            )
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()) == MODEL_SHA256)
}

fn cleanup_cache_dir(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join("audio-clean"))
        .map_err(|error| {
            cleanup_error(
                "audio_cleanup_cache_failed",
                "AI ses önbelleği bulunamadı.",
                error.to_string(),
                true,
            )
        })
}

fn cleanup_cache_key(
    source_path: &Path,
    request: &AudioCleanupRequest,
) -> Result<String, RenderErrorReport> {
    let metadata = source_path.metadata().map_err(|error| {
        cleanup_error(
            "missing_media",
            "Kaynak medya bulunamadı. Medyayı yeniden bağlayın.",
            error.to_string(),
            true,
        )
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    CACHE_SCHEMA.hash(&mut hasher);
    MODEL_SHA256.hash(&mut hasher);
    source_path.to_string_lossy().hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    request.start_ms.hash(&mut hasher);
    request.duration_ms.hash(&mut hasher);
    request.settings.strength.to_bits().hash(&mut hasher);
    request.settings.high_pass_hz.hash(&mut hasher);
    request.settings.hum_frequency.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn reusable_cache_file(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 44)
}

fn validate_request(request: &AudioCleanupRequest) -> Result<(), RenderErrorReport> {
    if request.duration_ms == 0 || request.duration_ms > MAX_DURATION_MS {
        return Err(cleanup_error(
            "invalid_audio_cleanup_duration",
            "Temizlenecek ses aralığı geçersiz.",
            format!("durationMs must be between 1 and {MAX_DURATION_MS}"),
            false,
        ));
    }
    if request.start_ms > MAX_DURATION_MS
        || request.start_ms.saturating_add(request.duration_ms) > MAX_DURATION_MS
    {
        return Err(cleanup_error(
            "invalid_audio_cleanup_range",
            "Temizlenecek ses aralığı desteklenen sınırın dışında.",
            format!(
                "startMs + durationMs must not exceed {MAX_DURATION_MS}; got {} + {}",
                request.start_ms, request.duration_ms
            ),
            false,
        ));
    }
    if !request.settings.strength.is_finite() || !(0.1..=1.0).contains(&request.settings.strength) {
        return Err(cleanup_error(
            "invalid_audio_cleanup_strength",
            "AI gürültü azaltma miktarı geçersiz.",
            "strength must be a finite value between 0.1 and 1",
            false,
        ));
    }
    if request.settings.high_pass_hz != 0 && !(20..=200).contains(&request.settings.high_pass_hz) {
        return Err(cleanup_error(
            "invalid_audio_cleanup_high_pass",
            "Alt frekans temizliği geçersiz.",
            "highPassHz must be 0 or between 20 and 200",
            false,
        ));
    }
    if !matches!(request.settings.hum_frequency, 0 | 50 | 60) {
        return Err(cleanup_error(
            "invalid_audio_cleanup_hum",
            "Uğultu frekansı geçersiz.",
            "humFrequency must be 0, 50, or 60",
            false,
        ));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    let path = PathBuf::from(value);
    if value.trim().is_empty() || !path.is_file() {
        return Err(cleanup_error(
            "missing_media",
            "Temizlenecek kaynak medya bulunamadı.",
            format!("Not a readable file: {}", path.display()),
            true,
        ));
    }
    fs::canonicalize(&path).map_err(|error| {
        cleanup_error(
            "missing_media",
            "Temizlenecek kaynak medya açılamadı.",
            error.to_string(),
            true,
        )
    })
}

fn escape_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace('\'', "\\'")
        .replace(':', "\\:")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn seconds(duration_ms: u64) -> String {
    format!("{}.{:03}", duration_ms / 1_000, duration_ms % 1_000)
}

fn number(value: f64) -> String {
    let formatted = format!("{value:.6}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn asset(
    output_path: PathBuf,
    duration_ms: u64,
    cache_hit: bool,
    profile: AutomaticCleanupProfile,
) -> AudioCleanupAsset {
    AudioCleanupAsset {
        output_path: output_path.to_string_lossy().into_owned(),
        duration_ms,
        cache_hit,
        engine: ENGINE_NAME,
        model: MODEL_NAME,
        applied: profile.speech_detected,
        speech_coverage: profile.speech_coverage,
        estimated_snr_db: profile.estimated_snr_db,
        strength: profile.strength,
    }
}

fn cleanup_error(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    retryable: bool,
) -> RenderErrorReport {
    RenderErrorReport {
        code: code.into(),
        user_message: user_message.into(),
        technical_message: technical_message.into(),
        retryable,
        stderr_tail: Vec::new(),
    }
}

fn cleanup_error_with_stderr(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    retryable: bool,
    stderr_tail: Vec<String>,
) -> RenderErrorReport {
    RenderErrorReport {
        code: code.into(),
        user_message: user_message.into(),
        technical_message: technical_message.into(),
        retryable,
        stderr_tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn request() -> AudioCleanupRequest {
        AudioCleanupRequest {
            source_path: "voice.wav".into(),
            start_ms: 1_000,
            duration_ms: 90_000,
            settings: AudioCleanupSettings {
                engine: AudioCleanupEngine::Rnnoise,
                model: AudioCleanupModel::RnnoiseBdV1,
                strength: 1.0,
                high_pass_hz: 70,
                hum_frequency: 0,
            },
            ffmpeg_path: None,
        }
    }

    fn speech_profile(strength: f64) -> AutomaticCleanupProfile {
        AutomaticCleanupProfile {
            speech_detected: true,
            speech_coverage: 0.6,
            estimated_snr_db: 14.0,
            strength,
        }
    }

    #[test]
    fn settings_use_the_stable_camel_case_wire_contract() {
        let wire = serde_json::to_value(&request().settings).unwrap();
        assert_eq!(wire["engine"], "rnnoise");
        assert_eq!(wire["model"], "rnnoise-bd-v1");
        assert_eq!(wire["highPassHz"], 70);
        assert_eq!(wire["humFrequency"], 0);
    }

    #[test]
    fn filter_chain_is_neural_and_preserves_requested_duration() {
        let filters = cleanup_filters(
            &request(),
            Path::new("C:\\models\\voice.rnnn"),
            speech_profile(1.0),
        );
        let joined = filters.join(",");
        assert!(joined.contains("highpass=f=70:p=2"));
        assert!(!joined.contains("bandreject="));
        assert!(joined.contains("arnndn=m='C\\:/models/voice.rnnn':mix=1"));
        assert!(joined.contains(AUTO_SPECTRAL_DENOISE));
        assert!(joined.contains(DIALOGUE_PRESENCE_EQ));
        assert!(joined.contains(DIALOGUE_AIR_EQ));
        assert!(joined.contains("apad=pad_dur=90.000,atrim=duration=90.000"));
        assert!(joined.contains("alimiter=limit=0.98:latency=1"));
        assert!(joined.find("arnndn=").unwrap() < joined.find("afftdn=").unwrap());
        assert!(joined.find("afftdn=").unwrap() < joined.find("equalizer=").unwrap());
        assert!(joined.find("treble=").unwrap() < joined.find("alimiter=").unwrap());
    }

    #[test]
    fn automatic_cleanup_leaves_non_speech_audio_out_of_the_neural_filter() {
        let filters = cleanup_filters(
            &request(),
            Path::new("C:\\models\\voice.rnnn"),
            AutomaticCleanupProfile {
                speech_detected: false,
                speech_coverage: 0.0,
                estimated_snr_db: 0.0,
                strength: 0.0,
            },
        )
        .join(",");
        assert!(!filters.contains("arnndn="));
        assert!(!filters.contains("afftdn="));
        assert!(!filters.contains("highpass="));
        assert!(!filters.contains("alimiter="));
        assert!(filters.contains("atrim=start=1.000:duration=90.000"));
    }

    #[test]
    fn validation_rejects_unbounded_or_mislabeled_settings() {
        let mut input = request();
        input.settings.strength = f64::NAN;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "invalid_audio_cleanup_strength"
        );
        input.settings.strength = 0.5;
        input.settings.hum_frequency = 55;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "invalid_audio_cleanup_hum"
        );
    }

    #[test]
    #[ignore = "downloads the pinned RNNoise model and requires ASTRAL_FFMPEG_PATH"]
    fn real_rnnoise_filter_preserves_ninety_second_duration() {
        let ffmpeg = PathBuf::from(
            std::env::var_os("ASTRAL_FFMPEG_PATH")
                .expect("ASTRAL_FFMPEG_PATH must point to the downloaded FFmpeg runtime"),
        );
        assert!(ffmpeg.is_file(), "missing FFmpeg at {}", ffmpeg.display());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "astral-rnnoise-smoke-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let model = dir.join(MODEL_FILE_NAME);
        let mut response = ureq::get(MODEL_URL).call().unwrap();
        let mut model_file = File::create(&model).unwrap();
        io::copy(&mut response.body_mut().as_reader(), &mut model_file).unwrap();
        drop(model_file);
        assert!(
            model_matches(&model).unwrap(),
            "pinned model checksum changed"
        );

        let source = dir.join("noisy-90s.wav");
        let generated = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=220:sample_rate=48000:duration=90",
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=color=white:amplitude=0.08:sample_rate=48000:duration=90",
                "-filter_complex",
                "[0:a][1:a]amix=inputs=2:normalize=0",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                "-loglevel",
                "error",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let mut input = request();
        input.start_ms = 0;
        input.duration_ms = 90_000;
        let cleaned = dir.join("clean-90s.wav");
        let processed = Command::new(&ffmpeg)
            .args(["-hide_banner", "-nostdin", "-y", "-i"])
            .arg(&source)
            .args(["-map", "0:a:0", "-af"])
            .arg(cleanup_filters(&input, &model, speech_profile(1.0)).join(","))
            .args([
                "-ar",
                "48000",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                "-loglevel",
                "error",
            ])
            .arg(&cleaned)
            .status()
            .unwrap();
        assert!(processed.success());

        let probe = Command::new(&ffmpeg)
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(&cleaned)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&probe.stderr);
        assert!(
            stderr.contains("Duration: 00:01:30.00"),
            "cleaned duration changed:\n{stderr}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore = "requires a speech fixture with a noise-only first 1.5s plus FFmpeg"]
    fn real_automatic_speech_cleanup_reduces_measured_noise() {
        let ffmpeg = PathBuf::from(
            std::env::var_os("ASTRAL_FFMPEG_PATH")
                .expect("ASTRAL_FFMPEG_PATH must point to ffmpeg.exe"),
        );
        let source = PathBuf::from(
            std::env::var_os("ASTRAL_AUDIO_CLEANUP_SMOKE_SOURCE")
                .expect("ASTRAL_AUDIO_CLEANUP_SMOKE_SOURCE must point to the noisy speech WAV"),
        );
        let duration_ms = std::env::var("ASTRAL_AUDIO_CLEANUP_SMOKE_DURATION_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(15_000);
        let profile =
            analyze_automatic_cleanup_profile_with_paths(&ffmpeg, &source, 0, duration_ms).unwrap();
        assert!(
            profile.speech_detected,
            "Silero did not detect fixture speech"
        );
        assert_eq!(profile.strength, 1.0, "speech cleanup must be full-wet");

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "astral-auto-denoise-smoke-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let model = dir.join(MODEL_FILE_NAME);
        let mut response = ureq::get(MODEL_URL).call().unwrap();
        let mut model_file = File::create(&model).unwrap();
        io::copy(&mut response.body_mut().as_reader(), &mut model_file).unwrap();
        drop(model_file);
        assert!(model_matches(&model).unwrap());

        let mut input = request();
        input.source_path = source.to_string_lossy().into_owned();
        input.start_ms = 0;
        input.duration_ms = duration_ms;
        let cleaned = dir.join("cleaned.wav");
        let status = Command::new(&ffmpeg)
            .args(["-hide_banner", "-nostdin", "-y", "-i"])
            .arg(&source)
            .args(["-map", "0:a:0", "-af"])
            .arg(cleanup_filters(&input, &model, profile).join(","))
            .args([
                "-ar",
                "48000",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                "-loglevel",
                "error",
            ])
            .arg(&cleaned)
            .status()
            .unwrap();
        assert!(status.success());

        let noise_before = mean_volume_db(&ffmpeg, &source, 0.0, 1.5);
        let noise_after = mean_volume_db(&ffmpeg, &cleaned, 0.0, 1.5);
        let speech_after = mean_volume_db(&ffmpeg, &cleaned, 2.2, 8.0);
        let reduction_db = noise_before - noise_after;
        println!(
            "auto cleanup: speech={:.1}%, snr={:.1} dB, strength={:.3}, noise reduction={:.1} dB, cleaned speech={:.1} dBFS",
            profile.speech_coverage * 100.0,
            profile.estimated_snr_db,
            profile.strength,
            reduction_db,
            speech_after,
        );
        assert!(
            reduction_db >= 3.0,
            "noise reduction was only {reduction_db:.1} dB"
        );
        assert!(speech_after > -50.0, "cleaned speech became inaudible");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn mean_volume_db(ffmpeg: &Path, source: &Path, start: f64, duration: f64) -> f64 {
        let output = Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-ss",
                &format!("{start:.3}"),
                "-t",
            ])
            .arg(format!("{duration:.3}"))
            .args(["-i"])
            .arg(source)
            .args(["-af", "volumedetect", "-f", "null", "-"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        stderr
            .lines()
            .find_map(|line| {
                line.split("mean_volume:")
                    .nth(1)?
                    .split_whitespace()
                    .next()?
                    .parse::<f64>()
                    .ok()
            })
            .unwrap_or_else(|| panic!("mean_volume missing from FFmpeg output:\n{stderr}"))
    }
}
