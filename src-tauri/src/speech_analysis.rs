use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use zip::ZipArchive;

use crate::{
    analysis_cancel::{self, CommandWaitError},
    render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport},
};

pub const SPEECH_SETUP_PROGRESS_EVENT: &str = "speech-analysis://setup-progress";

const ENGINE_NAME: &str = "whisper.cpp-v1.9.1";
const CUDA_ENGINE_NAME: &str = "whisper.cpp-v1.9.1-cuda11.8";
const MODEL_NAME: &str = "ggml-small-q5_1";
const DEFAULT_LANGUAGE: &str = "tr";
const AUTO_LANGUAGE: &str = "auto";
const CACHE_SCHEMA: &str = "speech-transcript-v4-nst-no-context";
const CLASSIFIER_SCHEMA: &str = "turkish-fillers-review-v1";

const RUNTIME_ARCHIVE_NAME: &str = "whisper-bin-x64-v1.9.1.zip";
const RUNTIME_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.1/whisper-bin-x64.zip";
const RUNTIME_SHA256: &str = "7d8be46ecd31828e1eb7a2ecdd0d6b314feafd82163038ab6092594b0a063539";
const CUDA_RUNTIME_ARCHIVE_NAME: &str = "whisper-cublas-11.8.0-bin-x64-v1.9.1.zip";
const CUDA_RUNTIME_URL: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.1/whisper-cublas-11.8.0-bin-x64.zip";
const CUDA_RUNTIME_SHA256: &str =
    "aecdce0e4d4bb758a7c72a31f3f9f19a7b6d861405fd2da743cd86398633c963";
const RUNTIME_MANIFEST_NAME: &str = ".astral-whisper-runtime";
const RUNTIME_EXECUTABLE_RELATIVE: &str = "Release/whisper-cli.exe";

const MODEL_FILE_NAME: &str = "ggml-small-q5_1.bin";
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/98aa99a0a9db05ae2342309f5096248665f7cba3/ggml-small-q5_1.bin";
const MODEL_SHA256: &str = "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb";

const MAX_RUNTIME_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CUDA_RUNTIME_ARCHIVE_BYTES: u64 = 300 * 1024 * 1024;
const MAX_MODEL_BYTES: u64 = 384 * 1024 * 1024;
const MAX_EXTRACTED_RUNTIME_BYTES: u64 = 96 * 1024 * 1024;
const MAX_EXTRACTED_CUDA_RUNTIME_BYTES: u64 = 1_200 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 256;
const MAX_REQUEST_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_JSON_BYTES: u64 = 256 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 32;

static SETUP_LOCK: Mutex<()> = Mutex::new(());
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
static CUDA_RUNTIME_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct WhisperRuntimeSpec {
    engine: &'static str,
    variant: &'static str,
    runtime_directory: &'static str,
    archive_name: &'static str,
    url: &'static str,
    sha256: &'static str,
    max_archive_bytes: u64,
    max_extracted_bytes: u64,
    display_name: &'static str,
}

struct WhisperRuntime {
    executable: PathBuf,
    engine: &'static str,
}

const CPU_RUNTIME: WhisperRuntimeSpec = WhisperRuntimeSpec {
    engine: ENGINE_NAME,
    variant: "cpu",
    runtime_directory: "whisper.cpp-v1.9.1-windows-x64",
    archive_name: RUNTIME_ARCHIVE_NAME,
    url: RUNTIME_URL,
    sha256: RUNTIME_SHA256,
    max_archive_bytes: MAX_RUNTIME_ARCHIVE_BYTES,
    max_extracted_bytes: MAX_EXTRACTED_RUNTIME_BYTES,
    display_name: "Whisper CPU çalışma motoru",
};

const CUDA_RUNTIME: WhisperRuntimeSpec = WhisperRuntimeSpec {
    engine: CUDA_ENGINE_NAME,
    variant: "cuda",
    runtime_directory: "whisper.cpp-v1.9.1-windows-x64-cuda11.8",
    archive_name: CUDA_RUNTIME_ARCHIVE_NAME,
    url: CUDA_RUNTIME_URL,
    sha256: CUDA_RUNTIME_SHA256,
    max_archive_bytes: MAX_CUDA_RUNTIME_ARCHIVE_BYTES,
    max_extracted_bytes: MAX_EXTRACTED_CUDA_RUNTIME_BYTES,
    display_name: "Whisper NVIDIA GPU çalışma motoru",
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeSpeechTranscriptRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f64,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeSpeechTranscriptResponse {
    pub engine: String,
    pub model: String,
    pub language: String,
    pub duration_ms: u64,
    pub transcript: String,
    pub words: Vec<SpeechWord>,
    pub filler_suggestions: Vec<FillerSuggestion>,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeechWord {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FillerCategory {
    HighConfidence,
    Contextual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FillerSuggestion {
    pub word_index: usize,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f64,
    pub category: FillerCategory,
    pub review_only: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechSetupProgress {
    pub artifact: String,
    pub stage: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: Option<f64>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct WhisperJson {
    #[serde(default)]
    result: WhisperResult,
    #[serde(default)]
    transcription: Vec<WhisperSegment>,
}

struct ParsedWhisperTranscript {
    language: String,
    words: Vec<SpeechWord>,
}

#[derive(Debug, Default, Deserialize)]
struct WhisperResult {
    #[serde(default)]
    language: String,
}

#[derive(Debug, Deserialize)]
struct WhisperSegment {
    #[serde(default)]
    offsets: WhisperOffsets,
    #[serde(default)]
    text: String,
    #[serde(default)]
    tokens: Vec<WhisperToken>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct WhisperOffsets {
    #[serde(default)]
    from: u64,
    #[serde(default)]
    to: u64,
}

#[derive(Debug, Deserialize)]
struct WhisperToken {
    #[serde(default)]
    text: String,
    #[serde(default)]
    offsets: Option<WhisperOffsets>,
    #[serde(default)]
    p: Option<f64>,
}

#[derive(Debug)]
struct WordBuilder {
    text: String,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    weighted_probability: f64,
    probability_weight: usize,
}

impl WordBuilder {
    fn new() -> Self {
        Self {
            text: String::new(),
            start_ms: None,
            end_ms: None,
            weighted_probability: 0.0,
            probability_weight: 0,
        }
    }

    fn push(&mut self, token: &WhisperToken, clean_text: &str) {
        self.text.push_str(clean_text);
        if let Some(offsets) = token.offsets {
            self.start_ms = Some(
                self.start_ms
                    .map_or(offsets.from, |current| current.min(offsets.from)),
            );
            self.end_ms = Some(
                self.end_ms
                    .map_or(offsets.to, |current| current.max(offsets.to)),
            );
        }
        if let Some(probability) = token.p.filter(|value| value.is_finite()) {
            let weight = clean_text.chars().count().max(1);
            self.weighted_probability += probability.clamp(0.0, 1.0) * weight as f64;
            self.probability_weight += weight;
        }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

struct PartialFileGuard {
    path: PathBuf,
    committed: bool,
}

impl PartialFileGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for PartialFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn create(parent: &Path, label: &str) -> Result<Self, RenderErrorReport> {
        fs::create_dir_all(parent).map_err(|error| {
            speech_error(
                "speech_temp_directory_failed",
                "Konuşma analizi çalışma klasörü hazırlanamadı.",
                format!("{}: {error}", parent.display()),
                true,
            )
        })?;
        for _ in 0..16 {
            let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".{label}.partial-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(speech_error(
                        "speech_temp_directory_failed",
                        "Konuşma analizi çalışma klasörü hazırlanamadı.",
                        format!("{}: {error}", path.display()),
                        true,
                    ));
                }
            }
        }
        Err(speech_error(
            "speech_temp_directory_failed",
            "Konuşma analizi çalışma klasörü hazırlanamadı.",
            format!(
                "Could not allocate a unique directory under {}",
                parent.display()
            ),
            true,
        ))
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[tauri::command]
pub async fn analyze_speech_transcript(
    app: AppHandle,
    request: AnalyzeSpeechTranscriptRequest,
) -> Result<AnalyzeSpeechTranscriptResponse, RenderErrorReport> {
    ensure_not_cancelled(&request, "before transcript analysis")?;
    validate_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());

    tauri::async_runtime::spawn_blocking(move || {
        analyze_speech_transcript_blocking(&app, &ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        speech_error(
            "speech_analysis_worker_failed",
            "AI konuşma analizi tamamlanamadı.",
            format!("Speech analysis worker failed: {error}"),
            true,
        )
    })?
}

fn analyze_speech_transcript_blocking(
    app: &AppHandle,
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeSpeechTranscriptRequest,
) -> Result<AnalyzeSpeechTranscriptResponse, RenderErrorReport> {
    let expected_duration_ms = post_speed_duration_ms(request.duration_ms, request.playback_rate);
    let cache_root = speech_cache_root(app)?;
    let transcript_dir = cache_root.join("transcripts");
    fs::create_dir_all(&transcript_dir).map_err(|error| {
        speech_error(
            "speech_cache_directory_failed",
            "Konuşma analizi önbelleği hazırlanamadı.",
            format!("{}: {error}", transcript_dir.display()),
            true,
        )
    })?;
    let cache_key = transcript_cache_key(source_path, request)?;
    let cache_path = transcript_dir.join(format!("{cache_key}.json"));
    if !request.force {
        if let Some(mut cached) = read_cached_response(&cache_path)? {
            cached.cache_hit = true;
            return Ok(cached);
        }
    }

    let (mut whisper_runtime, model_path) = {
        let _guard = SETUP_LOCK.lock().map_err(|_| {
            speech_error(
                "speech_setup_lock_failed",
                "AI konuşma motoru hazırlanamadı.",
                "Speech setup lock was poisoned",
                true,
            )
        })?;
        ensure_supported_platform()?;
        let whisper_runtime = ensure_whisper_runtime(app, &cache_root)?;
        let model_path = ensure_whisper_model(app, &cache_root)?;
        (whisper_runtime, model_path)
    };
    ensure_not_cancelled(request, "after Whisper setup")?;

    let work_dir = TempDirectory::create(&cache_root.join("work"), &cache_key)?;
    let wav_path = work_dir.path.join("speech.wav");
    transcode_post_speed_wav(ffmpeg_path, source_path, request, &wav_path)?;
    let output_prefix = work_dir.path.join("whisper-result");
    if let Err(gpu_error) = run_whisper_cli(
        &whisper_runtime.executable,
        &model_path,
        &wav_path,
        &output_prefix,
        &request.language,
        request.operation_id.as_deref(),
    ) {
        if whisper_runtime.engine != CUDA_ENGINE_NAME {
            return Err(gpu_error);
        }
        CUDA_RUNTIME_FAILED.store(true, Ordering::Relaxed);
        emit_setup_progress(
            app,
            SpeechSetupProgress {
                artifact: "runtime-cuda".into(),
                stage: "fallback".into(),
                downloaded_bytes: 0,
                total_bytes: 0,
                progress_percent: None,
                message: format!(
                    "GPU motoru bu sistemde başlatılamadı; CPU ile yeniden deneniyor. ({})",
                    gpu_error.user_message
                ),
            },
        );
        let cpu_runtime = {
            let _guard = SETUP_LOCK.lock().map_err(|_| {
                speech_error(
                    "speech_setup_lock_failed",
                    "AI konuşma motoru hazırlanamadı.",
                    "Speech setup lock was poisoned during CUDA fallback",
                    true,
                )
            })?;
            ensure_whisper_runtime_variant(app, &cache_root, CPU_RUNTIME)?
        };
        let _ = fs::remove_file(output_prefix.with_extension("json"));
        if let Err(mut cpu_error) = run_whisper_cli(
            &cpu_runtime.executable,
            &model_path,
            &wav_path,
            &output_prefix,
            &request.language,
            request.operation_id.as_deref(),
        ) {
            cpu_error.technical_message = format!(
                "CUDA attempt failed: {}; CPU fallback failed: {}",
                gpu_error.technical_message, cpu_error.technical_message
            );
            return Err(cpu_error);
        }
        whisper_runtime = cpu_runtime;
    }
    let json_path = output_prefix.with_extension("json");
    let parsed =
        parse_whisper_json_file_for_language(&json_path, expected_duration_ms, &request.language)?;
    let words = parsed.words;
    let transcript = transcript_from_words(&words);
    let filler_suggestions = classify_fillers(&words);
    let response = AnalyzeSpeechTranscriptResponse {
        engine: whisper_runtime.engine.to_owned(),
        model: MODEL_NAME.to_owned(),
        language: parsed.language,
        duration_ms: expected_duration_ms,
        transcript,
        words,
        filler_suggestions,
        cache_hit: false,
    };
    write_cached_response(&cache_path, &response, request.force)?;
    Ok(response)
}

fn ensure_supported_platform() -> Result<(), RenderErrorReport> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok(())
    } else {
        Err(speech_error(
            "speech_platform_unsupported",
            "Bu cihaz için yerel AI konuşma motoru henüz desteklenmiyor.",
            format!(
                "whisper.cpp sidecar is pinned for Windows x64; current target is {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            false,
        ))
    }
}

fn ensure_whisper_runtime(
    app: &AppHandle,
    cache_root: &Path,
) -> Result<WhisperRuntime, RenderErrorReport> {
    let device_preference = std::env::var("ASTRAL_WHISPER_DEVICE")
        .unwrap_or_else(|_| "auto".into())
        .trim()
        .to_ascii_lowercase();
    let cuda_allowed = should_use_cuda(&device_preference, nvidia_cuda_available())
        && !CUDA_RUNTIME_FAILED.load(Ordering::Relaxed);
    if cuda_allowed {
        match ensure_whisper_runtime_variant(app, cache_root, CUDA_RUNTIME) {
            Ok(runtime) => return Ok(runtime),
            Err(error) => emit_setup_progress(
                app,
                SpeechSetupProgress {
                    artifact: "runtime-cuda".into(),
                    stage: "fallback".into(),
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    progress_percent: None,
                    message: format!(
                        "GPU motoru hazırlanamadı; güvenli CPU motoruna dönülüyor. ({})",
                        error.user_message
                    ),
                },
            ),
        }
    }
    ensure_whisper_runtime_variant(app, cache_root, CPU_RUNTIME)
}

fn should_use_cuda(device_preference: &str, nvidia_available: bool) -> bool {
    nvidia_available && !matches!(device_preference, "cpu" | "off" | "disabled")
}

fn nvidia_cuda_available() -> bool {
    let mut command = Command::new("nvidia-smi");
    command.args(["--query-gpu=name", "--format=csv,noheader"]);
    configure_background_process(&mut command);
    command.output().is_ok_and(|output| {
        output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
    })
}

fn ensure_whisper_runtime_variant(
    app: &AppHandle,
    cache_root: &Path,
    spec: WhisperRuntimeSpec,
) -> Result<WhisperRuntime, RenderErrorReport> {
    let downloads_dir = cache_root.join("downloads");
    let runtime_root = cache_root.join("runtime");
    let runtime_dir = runtime_root.join(spec.runtime_directory);
    let executable = runtime_dir.join(RUNTIME_EXECUTABLE_RELATIVE);
    let manifest = runtime_dir.join(RUNTIME_MANIFEST_NAME);
    if executable.is_file()
        && fs::read_to_string(&manifest).is_ok_and(|value| value.trim() == spec.sha256)
    {
        return Ok(WhisperRuntime {
            executable,
            engine: spec.engine,
        });
    }

    fs::create_dir_all(&downloads_dir).map_err(|error| {
        speech_error(
            "speech_runtime_directory_failed",
            "AI konuşma motoru klasörü hazırlanamadı.",
            format!("{}: {error}", downloads_dir.display()),
            true,
        )
    })?;
    fs::create_dir_all(&runtime_root).map_err(|error| {
        speech_error(
            "speech_runtime_directory_failed",
            "AI konuşma motoru klasörü hazırlanamadı.",
            format!("{}: {error}", runtime_root.display()),
            true,
        )
    })?;
    let archive_path = downloads_dir.join(spec.archive_name);
    download_verified_file(
        app,
        &format!("runtime-{}", spec.variant),
        spec.url,
        spec.sha256,
        &archive_path,
        spec.max_archive_bytes,
        spec.display_name,
    )?;

    emit_setup_progress(
        app,
        SpeechSetupProgress {
            artifact: format!("runtime-{}", spec.variant),
            stage: "extracting".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: None,
            message: format!("{} güvenli biçimde kuruluyor.", spec.display_name),
        },
    );
    let staging = TempDirectory::create(&runtime_root, &format!("whisper-{}", spec.variant))?;
    safe_unpack_runtime(&archive_path, &staging.path, spec.max_extracted_bytes)?;
    let staged_executable = staging.path.join(RUNTIME_EXECUTABLE_RELATIVE);
    if !staged_executable.is_file() {
        return Err(speech_error(
            "speech_runtime_executable_missing",
            "AI konuşma motoru arşivi eksik veya geçersiz.",
            format!(
                "Expected {} after extracting {}",
                staged_executable.display(),
                archive_path.display()
            ),
            false,
        ));
    }
    fs::write(staging.path.join(RUNTIME_MANIFEST_NAME), spec.sha256).map_err(|error| {
        speech_error(
            "speech_runtime_manifest_failed",
            "AI konuşma motoru kurulamadı.",
            error.to_string(),
            true,
        )
    })?;
    if runtime_dir.exists() {
        remove_scoped_runtime_dir(&runtime_root, &runtime_dir)?;
    }
    fs::rename(&staging.path, &runtime_dir).map_err(|error| {
        speech_error(
            "speech_runtime_commit_failed",
            "AI konuşma motoru kurulamadı.",
            format!(
                "{} -> {}: {error}",
                staging.path.display(),
                runtime_dir.display()
            ),
            true,
        )
    })?;
    std::mem::forget(staging);

    emit_setup_progress(
        app,
        SpeechSetupProgress {
            artifact: format!("runtime-{}", spec.variant),
            stage: "ready".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: Some(100.0),
            message: format!("{} hazır.", spec.display_name),
        },
    );
    Ok(WhisperRuntime {
        executable: runtime_dir.join(RUNTIME_EXECUTABLE_RELATIVE),
        engine: spec.engine,
    })
}

fn ensure_whisper_model(app: &AppHandle, cache_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let model_dir = cache_root.join("models");
    fs::create_dir_all(&model_dir).map_err(|error| {
        speech_error(
            "speech_model_directory_failed",
            "AI konuşma modeli klasörü hazırlanamadı.",
            format!("{}: {error}", model_dir.display()),
            true,
        )
    })?;
    let model_path = model_dir.join(MODEL_FILE_NAME);
    download_verified_file(
        app,
        "model",
        MODEL_URL,
        MODEL_SHA256,
        &model_path,
        MAX_MODEL_BYTES,
        "Türkçe Whisper modeli",
    )?;
    Ok(model_path)
}

fn download_verified_file(
    app: &AppHandle,
    artifact: &str,
    url: &str,
    expected_sha256: &str,
    destination: &Path,
    max_bytes: u64,
    display_name: &str,
) -> Result<(), RenderErrorReport> {
    if sha256_matches(destination, expected_sha256)? {
        return Ok(());
    }
    if destination.is_file() {
        fs::remove_file(destination).map_err(|error| {
            speech_error(
                "speech_download_replace_failed",
                "AI konuşma dosyasının eski kopyası kaldırılamadı.",
                format!("{}: {error}", destination.display()),
                true,
            )
        })?;
    }
    let partial_path = path_with_suffix(destination, ".partial");
    if partial_path.is_file() {
        let _ = fs::remove_file(&partial_path);
    }
    let mut partial_guard = PartialFileGuard::new(partial_path.clone());
    emit_setup_progress(
        app,
        SpeechSetupProgress {
            artifact: artifact.into(),
            stage: "starting".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: Some(0.0),
            message: format!("{display_name} indirilmeye hazırlanıyor."),
        },
    );

    let mut response = ureq::get(url).call().map_err(|error| {
        speech_error(
            "speech_download_failed",
            "AI konuşma dosyası indirilemedi. İnternet bağlantısını kontrol edin.",
            format!("{url}: {error}"),
            true,
        )
    })?;
    let total_bytes = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if total_bytes > max_bytes {
        return Err(speech_error(
            "speech_download_too_large",
            "AI konuşma dosyasının boyut doğrulaması başarısız oldu.",
            format!("Content-Length {total_bytes} exceeds {max_bytes} for {url}"),
            false,
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
        .map_err(|error| {
            speech_error(
                "speech_download_write_failed",
                "AI konuşma dosyası diske yazılamadı.",
                format!("{}: {error}", partial_path.display()),
                true,
            )
        })?;
    let mut writer = BufWriter::new(file);
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded_bytes = 0_u64;
    let mut last_progress_tick = u64::MAX;
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            speech_error(
                "speech_download_failed",
                "AI konuşma dosyası indirilemedi.",
                error.to_string(),
                true,
            )
        })?;
        if read == 0 {
            break;
        }
        downloaded_bytes = downloaded_bytes.saturating_add(read as u64);
        if downloaded_bytes > max_bytes {
            return Err(speech_error(
                "speech_download_too_large",
                "AI konuşma dosyasının boyut doğrulaması başarısız oldu.",
                format!("Download exceeded {max_bytes} bytes for {url}"),
                false,
            ));
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read]).map_err(|error| {
            speech_error(
                "speech_download_write_failed",
                "AI konuşma dosyası diske yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        let tick = if total_bytes > 0 {
            downloaded_bytes.saturating_mul(100) / total_bytes
        } else {
            downloaded_bytes / (1024 * 1024)
        };
        if tick != last_progress_tick {
            last_progress_tick = tick;
            emit_setup_progress(
                app,
                SpeechSetupProgress {
                    artifact: artifact.into(),
                    stage: "downloading".into(),
                    downloaded_bytes,
                    total_bytes,
                    progress_percent: (total_bytes > 0).then(|| {
                        (downloaded_bytes as f64 / total_bytes as f64 * 100.0).clamp(0.0, 100.0)
                    }),
                    message: format!("{display_name} indiriliyor."),
                },
            );
        }
    }
    writer.flush().map_err(|error| {
        speech_error(
            "speech_download_write_failed",
            "AI konuşma dosyası diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    writer.get_ref().sync_all().map_err(|error| {
        speech_error(
            "speech_download_write_failed",
            "AI konuşma dosyası diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    drop(writer);
    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(speech_error(
            "speech_download_checksum_failed",
            "AI konuşma dosyasının güvenlik doğrulaması başarısız oldu.",
            format!("Expected SHA-256 {expected_sha256}, got {actual_sha256} from {url}"),
            true,
        ));
    }
    fs::rename(&partial_path, destination).map_err(|error| {
        speech_error(
            "speech_download_commit_failed",
            "AI konuşma dosyası kurulamadı.",
            format!(
                "{} -> {}: {error}",
                partial_path.display(),
                destination.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    emit_setup_progress(
        app,
        SpeechSetupProgress {
            artifact: artifact.into(),
            stage: "verified".into(),
            downloaded_bytes,
            total_bytes,
            progress_percent: Some(100.0),
            message: format!("{display_name} doğrulandı."),
        },
    );
    Ok(())
}

fn safe_unpack_runtime(
    archive_path: &Path,
    destination: &Path,
    max_extracted_bytes: u64,
) -> Result<(), RenderErrorReport> {
    let archive_file = File::open(archive_path).map_err(|error| {
        speech_error(
            "speech_runtime_archive_read_failed",
            "AI konuşma motoru arşivi okunamadı.",
            format!("{}: {error}", archive_path.display()),
            true,
        )
    })?;
    let mut archive = ZipArchive::new(BufReader::new(archive_file)).map_err(|error| {
        speech_error(
            "speech_runtime_archive_invalid",
            "AI konuşma motoru arşivi geçersiz.",
            error.to_string(),
            false,
        )
    })?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(speech_error(
            "speech_runtime_archive_invalid",
            "AI konuşma motoru arşivi güvenlik sınırını aşıyor.",
            format!(
                "Archive contains {} entries; maximum is {MAX_ARCHIVE_ENTRIES}",
                archive.len()
            ),
            false,
        ));
    }
    let mut extracted_bytes = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            speech_error(
                "speech_runtime_archive_invalid",
                "AI konuşma motoru arşivi okunamadı.",
                error.to_string(),
                false,
            )
        })?;
        let relative_path = safe_archive_relative_path(entry.name())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(speech_error(
                "speech_runtime_archive_unsafe",
                "AI konuşma motoru arşivi güvenli değil.",
                format!("Symbolic link entry rejected: {}", entry.name()),
                false,
            ));
        }
        extracted_bytes = extracted_bytes.saturating_add(entry.size());
        if extracted_bytes > max_extracted_bytes {
            return Err(speech_error(
                "speech_runtime_archive_too_large",
                "AI konuşma motoru arşivi güvenlik sınırını aşıyor.",
                format!("Uncompressed archive exceeds {max_extracted_bytes} bytes"),
                false,
            ));
        }
        let output_path = destination.join(&relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(|error| {
                speech_error(
                    "speech_runtime_extract_failed",
                    "AI konuşma motoru arşivi açılamadı.",
                    format!("{}: {error}", output_path.display()),
                    true,
                )
            })?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                speech_error(
                    "speech_runtime_extract_failed",
                    "AI konuşma motoru arşivi açılamadı.",
                    format!("{}: {error}", parent.display()),
                    true,
                )
            })?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(|error| {
                speech_error(
                    "speech_runtime_extract_failed",
                    "AI konuşma motoru arşivi açılamadı.",
                    format!("{}: {error}", output_path.display()),
                    false,
                )
            })?;
        let declared_size = entry.size();
        let copied = io::copy(
            &mut entry.by_ref().take(declared_size.saturating_add(1)),
            &mut output,
        )
        .map_err(|error| {
            speech_error(
                "speech_runtime_extract_failed",
                "AI konuşma motoru arşivi açılamadı.",
                format!("{}: {error}", output_path.display()),
                true,
            )
        })?;
        if copied != declared_size {
            return Err(speech_error(
                "speech_runtime_archive_invalid",
                "AI konuşma motoru arşivi eksik veya geçersiz.",
                format!(
                    "Entry {} declared {} bytes but yielded {copied}",
                    entry.name(),
                    declared_size
                ),
                false,
            ));
        }
    }
    Ok(())
}

fn safe_archive_relative_path(name: &str) -> Result<PathBuf, RenderErrorReport> {
    if name.is_empty()
        || name.contains('\0')
        || name.contains('\\')
        || name.starts_with('/')
        || name.as_bytes().get(1) == Some(&b':')
    {
        return Err(unsafe_archive_path_error(name));
    }
    let path = Path::new(name);
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) if !value.to_string_lossy().contains(':') => clean.push(value),
            Component::Normal(_) => return Err(unsafe_archive_path_error(name)),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(unsafe_archive_path_error(name));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(unsafe_archive_path_error(name));
    }
    Ok(clean)
}

fn unsafe_archive_path_error(name: &str) -> RenderErrorReport {
    speech_error(
        "speech_runtime_archive_unsafe",
        "AI konuşma motoru arşivi güvenli değil.",
        format!("Unsafe ZIP entry path rejected: {name:?}"),
        false,
    )
}

fn remove_scoped_runtime_dir(root: &Path, target: &Path) -> Result<(), RenderErrorReport> {
    if target == root || !target.starts_with(root) {
        return Err(speech_error(
            "speech_runtime_scope_invalid",
            "AI konuşma motoru klasörü güvenle yenilenemedi.",
            format!(
                "Refusing to remove {} outside {}",
                target.display(),
                root.display()
            ),
            false,
        ));
    }
    fs::remove_dir_all(target).map_err(|error| {
        speech_error(
            "speech_runtime_replace_failed",
            "AI konuşma motorunun eski kopyası kaldırılamadı.",
            format!("{}: {error}", target.display()),
            true,
        )
    })
}

fn transcode_post_speed_wav(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeSpeechTranscriptRequest,
    wav_path: &Path,
) -> Result<(), RenderErrorReport> {
    let expected_duration_ms = post_speed_duration_ms(request.duration_ms, request.playback_rate);
    let expected_seconds = seconds(expected_duration_ms);
    let mut filters = vec![
        format!(
            "atrim=start={}:duration={}",
            seconds(request.start_ms),
            seconds(request.duration_ms)
        ),
        "asetpts=PTS-STARTPTS".to_owned(),
    ];
    filters.extend(atempo_filters(request.playback_rate));
    filters.extend([
        "aresample=16000".to_owned(),
        "aformat=sample_fmts=s16:sample_rates=16000:channel_layouts=mono".to_owned(),
        format!("apad=pad_dur={expected_seconds}"),
        format!("atrim=duration={expected_seconds}"),
        "asetpts=PTS-STARTPTS".to_owned(),
    ]);

    let mut command = Command::new(ffmpeg_path);
    command
        .args([
            "-hide_banner",
            "-nostats",
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-i",
        ])
        .arg(source_path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af"])
        .arg(filters.join(","))
        .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(wav_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let output = command.output().map_err(|error| {
        speech_error(
            "speech_ffmpeg_start_failed",
            "Konuşma analizi için FFmpeg başlatılamadı.",
            format!("Failed to start {}: {error}", ffmpeg_path.display()),
            true,
        )
    })?;
    if !output.status.success() || !reusable_wav(wav_path) {
        let _ = fs::remove_file(wav_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(speech_error_with_stderr(
            "speech_ffmpeg_failed",
            "Klibin sesi konuşma analizi için hazırlanamadı.",
            format!("FFmpeg exited with {}", output.status),
            true,
            stderr_tail(&stderr),
        ));
    }
    Ok(())
}

fn run_whisper_cli(
    whisper_path: &Path,
    model_path: &Path,
    wav_path: &Path,
    output_prefix: &Path,
    language: &str,
    operation_id: Option<&str>,
) -> Result<(), RenderErrorReport> {
    let threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
        .clamp(2, 8);
    let mut command = Command::new(whisper_path);
    command
        .arg("--model")
        .arg(model_path)
        .arg("--file")
        .arg(wav_path)
        .args(whisper_decode_args(language))
        .arg(threads.to_string())
        .arg("--output-file")
        .arg(output_prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let child = command.spawn().map_err(|error| {
        speech_error(
            "speech_whisper_start_failed",
            "Yerel AI konuşma motoru başlatılamadı.",
            format!("Failed to start {}: {error}", whisper_path.display()),
            true,
        )
    })?;
    let output = match analysis_cancel::wait_for_output(child, operation_id) {
        Ok(output) => output,
        Err(CommandWaitError::Cancelled) => {
            return Err(cancelled_error("during Whisper inference"));
        }
        Err(CommandWaitError::Io(error)) => {
            return Err(speech_error(
                "speech_whisper_wait_failed",
                "Yerel AI konuşma motoru tamamlanamadı.",
                error.to_string(),
                true,
            ));
        }
    };
    let json_path = output_prefix.with_extension("json");
    if !output.status.success() || !json_path.is_file() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(speech_error_with_stderr(
            "speech_whisper_failed",
            "AI konuşma analizi sesi çözemedi.",
            format!(
                "{} exited with {}; expected {}",
                whisper_path.display(),
                output.status,
                json_path.display()
            ),
            true,
            stderr_tail(&stderr),
        ));
    }
    Ok(())
}

fn whisper_decode_args(language: &str) -> [&str; 11] {
    [
        "--language",
        language,
        "--output-json-full",
        "--max-len",
        "1",
        "--split-on-word",
        // Motor, müzik ve kalabalık gibi ses açıklamalarını konuşma diye yazma.
        "--suppress-nst",
        // Bir yanlış ses açıklamasının sonraki pencerelere döngü halinde taşınmasını önle.
        "--max-context",
        "0",
        "--no-prints",
        "--threads",
    ]
}

#[cfg(test)]
fn parse_whisper_json_file(
    json_path: &Path,
    duration_ms: u64,
) -> Result<Vec<SpeechWord>, RenderErrorReport> {
    Ok(parse_whisper_json_file_for_language(json_path, duration_ms, DEFAULT_LANGUAGE)?.words)
}

fn parse_whisper_json_file_for_language(
    json_path: &Path,
    duration_ms: u64,
    requested_language: &str,
) -> Result<ParsedWhisperTranscript, RenderErrorReport> {
    let metadata = json_path.metadata().map_err(|error| {
        speech_error(
            "speech_json_missing",
            "AI konuşma motoru sonuç dosyası üretmedi.",
            format!("{}: {error}", json_path.display()),
            true,
        )
    })?;
    if metadata.len() > MAX_JSON_BYTES {
        return Err(speech_error(
            "speech_json_too_large",
            "AI konuşma sonucu güvenlik sınırını aşıyor.",
            format!("JSON result is {} bytes", metadata.len()),
            false,
        ));
    }
    let bytes = fs::read(json_path).map_err(|error| {
        speech_error(
            "speech_json_read_failed",
            "AI konuşma sonucu okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    parse_whisper_json_for_language(&bytes, duration_ms, requested_language)
}

#[cfg(test)]
fn parse_whisper_json(json: &[u8], duration_ms: u64) -> Result<Vec<SpeechWord>, RenderErrorReport> {
    Ok(parse_whisper_json_for_language(json, duration_ms, DEFAULT_LANGUAGE)?.words)
}

fn parse_whisper_json_for_language(
    json: &[u8],
    duration_ms: u64,
    requested_language: &str,
) -> Result<ParsedWhisperTranscript, RenderErrorReport> {
    let parsed: WhisperJson = serde_json::from_slice(json).map_err(|error| {
        speech_error(
            "speech_json_invalid",
            "AI konuşma motoru geçersiz sonuç üretti.",
            error.to_string(),
            false,
        )
    })?;
    if requested_language != AUTO_LANGUAGE
        && !parsed.result.language.is_empty()
        && parsed.result.language != DEFAULT_LANGUAGE
        && parsed.result.language != "turkish"
    {
        return Err(speech_error(
            "speech_language_mismatch",
            "AI konuşma motoru Türkçe sonuç üretemedi.",
            format!("whisper.cpp reported language {:?}", parsed.result.language),
            true,
        ));
    }
    let detected_language = if parsed.result.language.is_empty() {
        requested_language.to_owned()
    } else {
        parsed.result.language.clone()
    };
    let mut words = Vec::new();
    for segment in parsed.transcription {
        words.extend(words_from_segment(&segment, duration_ms));
    }
    words = spoken_words_only(words);
    words.sort_by_key(|word| (word.start_ms, word.end_ms));
    for word in &mut words {
        word.start_ms = word.start_ms.min(duration_ms);
        word.end_ms = word.end_ms.max(word.start_ms).min(duration_ms);
        word.confidence = word.confidence.clamp(0.0, 1.0);
    }
    Ok(ParsedWhisperTranscript {
        language: detected_language,
        words,
    })
}

fn words_from_segment(segment: &WhisperSegment, duration_ms: u64) -> Vec<SpeechWord> {
    let segment_start = segment.offsets.from.min(duration_ms);
    let segment_end = segment.offsets.to.max(segment_start).min(duration_ms);
    let mut builders = Vec::new();
    let mut current = WordBuilder::new();
    for token in &segment.tokens {
        let raw = token.text.as_str();
        let clean = raw.trim();
        if clean.is_empty() || is_special_token_text(clean) {
            continue;
        }
        let starts_new_word = raw.chars().next().is_some_and(char::is_whitespace)
            && !current.is_empty()
            && clean.chars().any(char::is_alphanumeric);
        if starts_new_word {
            builders.push(current);
            current = WordBuilder::new();
        }
        current.push(token, clean);
    }
    if !current.is_empty() {
        builders.push(current);
    }

    if builders.is_empty() {
        builders = segment
            .text
            .split_whitespace()
            .filter(|text| !is_special_token_text(text))
            .map(|text| {
                let mut builder = WordBuilder::new();
                builder.text = text.to_owned();
                builder
            })
            .collect();
    }
    let count = builders.len().max(1) as u64;
    builders
        .into_iter()
        .enumerate()
        .filter_map(|(index, builder)| {
            let text = builder.text.trim().to_owned();
            if text.is_empty() || is_special_token_text(&text) {
                return None;
            }
            let fallback_start = segment_start.saturating_add(
                segment_end
                    .saturating_sub(segment_start)
                    .saturating_mul(index as u64)
                    / count,
            );
            let fallback_end = segment_start.saturating_add(
                segment_end
                    .saturating_sub(segment_start)
                    .saturating_mul(index as u64 + 1)
                    / count,
            );
            let start_ms = builder.start_ms.unwrap_or(fallback_start).min(duration_ms);
            let end_ms = builder
                .end_ms
                .unwrap_or(fallback_end)
                .max(start_ms)
                .min(duration_ms);
            Some(SpeechWord {
                text,
                start_ms,
                end_ms,
                confidence: if builder.probability_weight == 0 {
                    0.0
                } else {
                    builder.weighted_probability / builder.probability_weight as f64
                },
            })
        })
        .collect()
}

fn is_special_token_text(text: &str) -> bool {
    (text.starts_with("<|") && text.ends_with("|>"))
        || (text.starts_with('[') && text.ends_with(']'))
}

/// Keeps only dictated speech. Whisper can emit accessibility-style sound
/// descriptions such as `(kalabalik)`, `[muzik]`, or `♪ alkis ♪`; those are
/// useful in closed captions but should not become editable spoken subtitles.
fn spoken_words_only(words: Vec<SpeechWord>) -> Vec<SpeechWord> {
    let mut output = Vec::with_capacity(words.len());
    let mut round_depth = 0_u32;
    let mut square_depth = 0_u32;
    let mut curly_depth = 0_u32;
    let mut music_description = false;
    let mut emphasized_description = false;

    for mut word in words {
        let mut spoken = String::with_capacity(word.text.len());
        for character in word.text.chars() {
            match character {
                '♪' | '♫' => {
                    music_description = !music_description;
                }
                '*' if !music_description => {
                    emphasized_description = !emphasized_description;
                }
                '(' if !music_description && !emphasized_description => {
                    round_depth = round_depth.saturating_add(1)
                }
                ')' if !music_description && !emphasized_description && round_depth > 0 => {
                    round_depth -= 1
                }
                '[' if !music_description && !emphasized_description => {
                    square_depth = square_depth.saturating_add(1)
                }
                ']' if !music_description && !emphasized_description && square_depth > 0 => {
                    square_depth -= 1
                }
                '{' if !music_description && !emphasized_description => {
                    curly_depth = curly_depth.saturating_add(1)
                }
                '}' if !music_description && !emphasized_description && curly_depth > 0 => {
                    curly_depth -= 1
                }
                _ if !music_description
                    && !emphasized_description
                    && round_depth == 0
                    && square_depth == 0
                    && curly_depth == 0 =>
                {
                    spoken.push(character);
                }
                _ => {}
            }
        }

        let spoken = spoken.trim();
        if spoken.chars().any(char::is_alphanumeric) {
            word.text = spoken.to_owned();
            output.push(word);
        }
    }
    output
}

fn transcript_from_words(words: &[SpeechWord]) -> String {
    let mut transcript = String::new();
    for word in words {
        let text = word.text.trim();
        if text.is_empty() {
            continue;
        }
        let punctuation_only = text.chars().all(|value| !value.is_alphanumeric());
        if !transcript.is_empty() && !punctuation_only {
            transcript.push(' ');
        }
        transcript.push_str(text);
    }
    transcript
}

fn classify_fillers(words: &[SpeechWord]) -> Vec<FillerSuggestion> {
    words
        .iter()
        .enumerate()
        .filter_map(|(word_index, word)| {
            let normalized = normalize_turkish_word(&word.text);
            let (category, reason) = if is_high_confidence_filler(&normalized) {
                (
                    FillerCategory::HighConfidence,
                    "Sözcüksel olmayan Türkçe dolgu sesi; kesmeden önce dinleyin.",
                )
            } else if is_contextual_filler(&normalized) {
                (
                    FillerCategory::Contextual,
                    "Bağlama göre anlamlı olabilen söylem belirteci; otomatik kesilmez.",
                )
            } else {
                return None;
            };
            Some(FillerSuggestion {
                word_index,
                text: word.text.clone(),
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                confidence: word.confidence,
                category,
                review_only: true,
                reason: reason.to_owned(),
            })
        })
        .collect()
}

fn normalize_turkish_word(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            'I' => vec!['ı'],
            'İ' => vec!['i'],
            other => other.to_lowercase().collect::<Vec<_>>(),
        })
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn is_high_confidence_filler(value: &str) -> bool {
    let repeated_vowel = (2..=10).contains(&value.chars().count())
        && (value.chars().all(|character| character == 'e')
            || value.chars().all(|character| character == 'ı'));
    repeated_vowel
        || matches!(
            value,
            "ıh" | "ıhm"
                | "ııhm"
                | "hım"
                | "hımm"
                | "hmm"
                | "hmmm"
                | "mmm"
                | "eeem"
                | "ehm"
                | "uh"
                | "um"
        )
}

fn is_contextual_filler(value: &str) -> bool {
    matches!(value, "yani" | "şey" | "işte" | "hani" | "falan" | "filan")
}

fn read_cached_response(
    cache_path: &Path,
) -> Result<Option<AnalyzeSpeechTranscriptResponse>, RenderErrorReport> {
    if !cache_path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(cache_path).map_err(|error| {
        speech_error(
            "speech_cache_read_failed",
            "Konuşma analizi önbelleği okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    match serde_json::from_slice(&bytes) {
        Ok(response) => Ok(Some(response)),
        Err(_) => {
            let _ = fs::remove_file(cache_path);
            Ok(None)
        }
    }
}

fn write_cached_response(
    cache_path: &Path,
    response: &AnalyzeSpeechTranscriptResponse,
    replace_existing: bool,
) -> Result<(), RenderErrorReport> {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let partial_path = path_with_suffix(
        cache_path,
        &format!(".partial-{}-{unique}", std::process::id()),
    );
    let mut guard = PartialFileGuard::new(partial_path.clone());
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
        .map_err(|error| {
            speech_error(
                "speech_cache_write_failed",
                "Konuşma analizi önbelleğe yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, response).map_err(|error| {
        speech_error(
            "speech_cache_write_failed",
            "Konuşma analizi önbelleğe yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    writer.flush().map_err(|error| {
        speech_error(
            "speech_cache_write_failed",
            "Konuşma analizi önbelleğe yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    writer.get_ref().sync_all().map_err(|error| {
        speech_error(
            "speech_cache_write_failed",
            "Konuşma analizi önbelleğe yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    drop(writer);
    if replace_existing && cache_path.is_file() {
        fs::remove_file(cache_path).map_err(|error| {
            speech_error(
                "speech_cache_replace_failed",
                "Eski konuşma analizi önbelleği yenilenemedi.",
                error.to_string(),
                true,
            )
        })?;
    }
    match fs::rename(&partial_path, cache_path) {
        Ok(()) => {
            guard.commit();
            Ok(())
        }
        Err(_error) if cache_path.is_file() => Ok(()),
        Err(error) => Err(speech_error(
            "speech_cache_commit_failed",
            "Konuşma analizi önbelleğe alınamadı.",
            error.to_string(),
            true,
        )),
    }
}

fn transcript_cache_key(
    source_path: &Path,
    request: &AnalyzeSpeechTranscriptRequest,
) -> Result<String, RenderErrorReport> {
    let metadata = source_path.metadata().map_err(|error| {
        speech_error(
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
    let mut hasher = Sha256::new();
    for value in [
        CACHE_SCHEMA.to_owned(),
        CLASSIFIER_SCHEMA.to_owned(),
        ENGINE_NAME.to_owned(),
        MODEL_SHA256.to_owned(),
        source_path.to_string_lossy().into_owned(),
        metadata.len().to_string(),
        modified.to_string(),
        request.start_ms.to_string(),
        request.duration_ms.to_string(),
        request.playback_rate.to_bits().to_string(),
        request.language.clone(),
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn speech_cache_root(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join("speech-analysis"))
        .map_err(|error| {
            speech_error(
                "speech_cache_directory_failed",
                "Konuşma analizi önbelleği bulunamadı.",
                error.to_string(),
                true,
            )
        })
}

fn validate_request(request: &AnalyzeSpeechTranscriptRequest) -> Result<(), RenderErrorReport> {
    if request.duration_ms == 0 || request.duration_ms > MAX_REQUEST_DURATION_MS {
        return Err(speech_error(
            "invalid_speech_duration",
            "Konuşma analizi süresi geçersiz.",
            format!(
                "durationMs must be between 1 and {MAX_REQUEST_DURATION_MS}; got {}",
                request.duration_ms
            ),
            false,
        ));
    }
    if request.start_ms > MAX_REQUEST_DURATION_MS
        || request.start_ms.saturating_add(request.duration_ms) > MAX_REQUEST_DURATION_MS
    {
        return Err(speech_error(
            "invalid_speech_range",
            "Konuşma analizi aralığı desteklenen sınırın dışında.",
            format!(
                "startMs + durationMs must not exceed {MAX_REQUEST_DURATION_MS}; got {} + {}",
                request.start_ms, request.duration_ms
            ),
            false,
        ));
    }
    if !request.playback_rate.is_finite() || !(0.05..=16.0).contains(&request.playback_rate) {
        return Err(speech_error(
            "invalid_speech_speed",
            "Klip hızı konuşma analizi için geçersiz.",
            format!("playbackRate was {}", request.playback_rate),
            false,
        ));
    }
    if request.language != DEFAULT_LANGUAGE && request.language != AUTO_LANGUAGE {
        return Err(speech_error(
            "invalid_speech_language",
            "Konuşma analizi dili desteklenmiyor.",
            format!("language must be tr or auto; got {:?}", request.language),
            false,
        ));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    let path = PathBuf::from(value);
    if value.trim().is_empty() || !path.is_file() {
        return Err(speech_error(
            "missing_media",
            "Konuşma analizi yapılacak medya bulunamadı.",
            format!("Not a readable file: {}", path.display()),
            true,
        ));
    }
    fs::canonicalize(&path).map_err(|error| {
        speech_error(
            "missing_media",
            "Konuşma analizi yapılacak medya açılamadı.",
            error.to_string(),
            true,
        )
    })
}

fn post_speed_duration_ms(duration_ms: u64, playback_rate: f64) -> u64 {
    ((duration_ms as f64 / playback_rate).round() as u64).max(1)
}

fn atempo_filters(speed: f64) -> Vec<String> {
    let mut remaining = speed;
    let mut factors = Vec::new();
    while remaining > 2.0 + f64::EPSILON {
        factors.push(2.0);
        remaining /= 2.0;
    }
    while remaining < 0.5 - f64::EPSILON {
        factors.push(0.5);
        remaining /= 0.5;
    }
    factors.push(remaining);
    factors
        .into_iter()
        .map(|factor| format!("atempo={}", number(factor)))
        .collect()
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

fn default_playback_rate() -> f64 {
    1.0
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_owned()
}

fn reusable_wav(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 44)
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn sha256_matches(path: &Path, expected_sha256: &str) -> Result<bool, RenderErrorReport> {
    if !path.is_file() {
        return Ok(false);
    }
    let file = File::open(path).map_err(|error| {
        speech_error(
            "speech_checksum_read_failed",
            "AI konuşma dosyası doğrulanamadı.",
            format!("{}: {error}", path.display()),
            true,
        )
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            speech_error(
                "speech_checksum_read_failed",
                "AI konuşma dosyası doğrulanamadı.",
                error.to_string(),
                true,
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()) == expected_sha256)
}

fn emit_setup_progress(app: &AppHandle, progress: SpeechSetupProgress) {
    let _ = app.emit(SPEECH_SETUP_PROGRESS_EVENT, progress);
}

fn stderr_tail(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .rev()
        .take(STDERR_TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(str::to_owned)
        .collect()
}

fn ensure_not_cancelled(
    request: &AnalyzeSpeechTranscriptRequest,
    stage: &str,
) -> Result<(), RenderErrorReport> {
    if analysis_cancel::is_cancelled(request.operation_id.as_deref()) {
        return Err(cancelled_error(stage));
    }
    Ok(())
}

fn cancelled_error(stage: &str) -> RenderErrorReport {
    speech_error(
        "smart_shorts_cancelled",
        "Akıllı Shorts analizi iptal edildi.",
        format!("Speech analysis cancelled {stage}"),
        false,
    )
}

fn speech_error(
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

fn speech_error_with_stderr(
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

    fn temp_directory(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "astral-speech-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn request() -> AnalyzeSpeechTranscriptRequest {
        AnalyzeSpeechTranscriptRequest {
            source_path: "voice.mp4".into(),
            start_ms: 1_000,
            duration_ms: 90_000,
            playback_rate: 1.25,
            ffmpeg_path: None,
            force: false,
            language: DEFAULT_LANGUAGE.into(),
            operation_id: None,
        }
    }

    #[test]
    fn whisper_runtime_prefers_cuda_only_when_nvidia_is_available() {
        assert!(should_use_cuda("auto", true));
        assert!(should_use_cuda("cuda", true));
        assert!(!should_use_cuda("auto", false));
        assert!(!should_use_cuda("cuda", false));
        assert!(!should_use_cuda("cpu", true));
        assert!(!should_use_cuda("disabled", true));
    }

    #[test]
    fn checksum_accepts_exact_bytes_and_rejects_mutation() {
        let dir = temp_directory("checksum");
        let path = dir.join("artifact.bin");
        fs::write(&path, b"astral-lunar").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"astral-lunar"));
        assert!(sha256_matches(&path, &expected).unwrap());
        fs::write(&path, b"astral-lunar!").unwrap();
        assert!(!sha256_matches(&path, &expected).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn zip_slip_paths_are_rejected_before_joining_destination() {
        for unsafe_name in [
            "../escape.exe",
            "Release/../../escape.exe",
            "/absolute.exe",
            "C:\\absolute.exe",
        ] {
            assert_eq!(
                safe_archive_relative_path(unsafe_name).unwrap_err().code,
                "speech_runtime_archive_unsafe"
            );
        }
        assert_eq!(
            safe_archive_relative_path("Release/whisper-cli.exe").unwrap(),
            PathBuf::from("Release/whisper-cli.exe")
        );
    }

    #[test]
    fn malicious_zip_cannot_write_outside_the_staging_directory() {
        use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

        let root = temp_directory("zip-slip");
        let archive_path = root.join("malicious.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("../escape.exe", options).unwrap();
        writer.write_all(b"must not escape").unwrap();
        writer.finish().unwrap();

        let destination = root.join("staging");
        fs::create_dir(&destination).unwrap();
        let error = safe_unpack_runtime(&archive_path, &destination, MAX_EXTRACTED_RUNTIME_BYTES)
            .unwrap_err();
        assert_eq!(error.code, "speech_runtime_archive_unsafe");
        assert!(!root.join("escape.exe").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parser_reads_v191_json_full_word_offsets_and_probabilities() {
        let json = br#"{
            "result": {"language": "tr"},
            "transcription": [{
                "offsets": {"from": 100, "to": 760},
                "text": " yani geldim",
                "tokens": [
                    {"text": " yani", "offsets": {"from": 100, "to": 390}, "id": 1, "p": 0.92, "t_dtw": -1},
                    {"text": " geldim", "offsets": {"from": 400, "to": 760}, "id": 2, "p": 0.81, "t_dtw": -1}
                ]
            }]
        }"#;
        let words = parse_whisper_json(json, 1_000).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "yani");
        assert_eq!((words[0].start_ms, words[0].end_ms), (100, 390));
        assert!((words[0].confidence - 0.92).abs() < 1e-9);
        assert_eq!(words[1].text, "geldim");
        assert_eq!((words[1].start_ms, words[1].end_ms), (400, 760));
    }

    #[test]
    fn parser_falls_back_to_segment_offsets_and_clamps_duration() {
        let json = r#"{
            "result": {"language": "tr"},
            "transcription": [{
                "offsets": {"from": 900, "to": 1400},
                "text": " merhaba dünya",
                "tokens": []
            }]
        }"#;
        let words = parse_whisper_json(json.as_bytes(), 1_000).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!((words[0].start_ms, words[0].end_ms), (900, 950));
        assert_eq!((words[1].start_ms, words[1].end_ms), (950, 1_000));
    }

    #[test]
    fn parser_keeps_only_spoken_words_and_removes_sound_descriptions() {
        let json = r#"{
            "result": {"language": "tr"},
            "transcription": [{
                "offsets": {"from": 0, "to": 3000},
                "text": " *Kalabalık şarkı* merhaba (kalabalik sesi) bugun [muzik] geldim \u266a alkis \u266a",
                "tokens": []
            }]
        }"#;
        let words = parse_whisper_json(json.as_bytes(), 3_000).unwrap();
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["merhaba", "bugun", "geldim"]
        );
        assert_eq!(transcript_from_words(&words), "merhaba bugun geldim");
    }

    #[test]
    fn spoken_word_filter_preserves_speech_joined_to_an_annotation() {
        let words = vec![SpeechWord {
            text: "Merhaba,(kalabalik)".into(),
            start_ms: 0,
            end_ms: 500,
            confidence: 0.9,
        }];
        let filtered = spoken_words_only(words);
        assert_eq!(filtered[0].text, "Merhaba,");
    }

    #[test]
    fn whisper_arguments_suppress_non_speech_and_disable_hallucination_context() {
        let arguments = whisper_decode_args(AUTO_LANGUAGE);
        assert!(arguments.contains(&"--suppress-nst"));
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--max-context", "0"]));
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--language", AUTO_LANGUAGE]));
    }

    #[test]
    fn auto_language_accepts_english_for_turkish_code_switch_workflows() {
        let json = br#"{
            "result": {"language": "en"},
            "transcription": [{
                "offsets": {"from": 0, "to": 500},
                "text": " export workflow",
                "tokens": []
            }]
        }"#;
        let parsed = parse_whisper_json_for_language(json, 1_000, AUTO_LANGUAGE).unwrap();
        assert_eq!(parsed.language, "en");
        assert_eq!(parsed.words.len(), 2);
        assert!(parse_whisper_json(json, 1_000).is_err());
    }

    #[test]
    fn invalid_json_is_reported_as_structured_render_error() {
        assert_eq!(
            parse_whisper_json(b"not-json", 1_000).unwrap_err().code,
            "speech_json_invalid"
        );
    }

    #[test]
    fn turkish_fillers_are_review_only_and_context_is_separate() {
        let words = vec![
            SpeechWord {
                text: "III...".into(),
                start_ms: 0,
                end_ms: 250,
                confidence: 0.88,
            },
            SpeechWord {
                text: "Yani,".into(),
                start_ms: 300,
                end_ms: 600,
                confidence: 0.93,
            },
            SpeechWord {
                text: "geldim".into(),
                start_ms: 650,
                end_ms: 900,
                confidence: 0.97,
            },
        ];
        let suggestions = classify_fillers(&words);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].category, FillerCategory::HighConfidence);
        assert_eq!(suggestions[1].category, FillerCategory::Contextual);
        assert!(suggestions.iter().all(|item| item.review_only));
    }

    #[test]
    fn speed_mapping_matches_post_speed_timeline_duration() {
        assert_eq!(post_speed_duration_ms(90_000, 1.5), 60_000);
        assert_eq!(post_speed_duration_ms(90_000, 0.5), 180_000);
        let filters = atempo_filters(8.0);
        assert_eq!(filters, ["atempo=2", "atempo=2", "atempo=2"]);
    }

    #[test]
    fn request_defaults_are_stable_for_the_client_contract() {
        let parsed: AnalyzeSpeechTranscriptRequest = serde_json::from_value(serde_json::json!({
            "sourcePath": "voice.mp4",
            "durationMs": 1000
        }))
        .unwrap();
        assert_eq!(parsed.playback_rate, 1.0);
        assert_eq!(parsed.start_ms, 0);
        assert!(!parsed.force);
        assert_eq!(parsed.language, DEFAULT_LANGUAGE);
        assert!(validate_request(&request()).is_ok());
    }

    #[test]
    #[ignore = "requires real FFmpeg, whisper.cpp v1.9.1, model, and Turkish speech paths"]
    fn real_whisper_sidecar_smoke() {
        let ffmpeg = PathBuf::from(
            std::env::var_os("ASTRAL_FFMPEG_PATH")
                .expect("ASTRAL_FFMPEG_PATH must point to ffmpeg.exe"),
        );
        let whisper = PathBuf::from(
            std::env::var_os("ASTRAL_WHISPER_CLI_PATH")
                .expect("ASTRAL_WHISPER_CLI_PATH must point to whisper-cli.exe"),
        );
        let model = PathBuf::from(
            std::env::var_os("ASTRAL_WHISPER_MODEL_PATH")
                .expect("ASTRAL_WHISPER_MODEL_PATH must point to ggml-small-q5_1.bin"),
        );
        let source = PathBuf::from(
            std::env::var_os("ASTRAL_SPEECH_SMOKE_SOURCE")
                .expect("ASTRAL_SPEECH_SMOKE_SOURCE must point to Turkish speech"),
        );
        let dir = temp_directory("real-smoke");
        let wav = dir.join("speech.wav");
        let mut input = request();
        input.source_path = source.to_string_lossy().into_owned();
        input.start_ms = 0;
        input.duration_ms = 10_000;
        input.playback_rate = 1.0;
        transcode_post_speed_wav(&ffmpeg, &source, &input, &wav).unwrap();
        let prefix = dir.join("result");
        run_whisper_cli(&whisper, &model, &wav, &prefix, DEFAULT_LANGUAGE, None).unwrap();
        let words = parse_whisper_json_file(&prefix.with_extension("json"), 10_000).unwrap();
        assert!(!words.is_empty());
        assert!(words.iter().all(|word| word.end_ms <= 10_000));
        fs::remove_dir_all(dir).unwrap();
    }
}
