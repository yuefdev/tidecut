use std::{
    collections::{HashSet, VecDeque},
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    analysis_cancel::{self, CommandWaitError},
    render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport},
};

pub const LAUGHTER_ANALYSIS_PROGRESS_EVENT: &str = "laughter-analysis://progress";

const OUTPUT_SCHEMA: &str = "astral-yamnet-laughter-v1";
const RUNTIME_SCHEMA: &str = "astral-yamnet-runtime-v1";
const ENGINE: &str = "mediapipe-yamnet-v1";
const ENGINE_VERSION: &str = "0.10.35";
const MODEL: &str = "yamnet-audioset-521";
const RUNTIME_DIRECTORY_NAME: &str = "mediapipe-yamnet-py311-v1";
const RUNTIME_MANIFEST_NAME: &str = ".astral-yamnet-runtime.json";
const RUNNER_FILE_NAME: &str = "yamnet_laughter.py";
const PYTHON_VERSION_REQUEST: &str = "3.11";
const MODEL_FILE_NAME: &str = "yamnet-float32-4d8b4a53.tflite";
const MODEL_URL: &str = "https://storage.googleapis.com/mediapipe-models/audio_classifier/yamnet/float32/1/yamnet.tflite";
const MODEL_SHA256: &str = "4d8b4a53282dc83ef04e3e7dbc4fbc98082e34e44ed798e16c3a0cdd4c584faf";
const MODEL_BYTES: u64 = 4_126_810;
const MAX_MODEL_BYTES: u64 = 5 * 1024 * 1024;
const MAX_DURATION_MS: u64 = 4 * 60 * 60 * 1_000;
const MAX_SOURCE_RANGE_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_RUNNER_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const SAMPLE_RATE: u32 = 16_000;
const WINDOW_MS: u64 = 960;
const MERGE_GAP_MS: u64 = 600;
const DEFAULT_MIN_CONFIDENCE: f64 = 0.28;
const STRONG_SINGLE_FRAME_CONFIDENCE: f64 = 0.55;
const PROGRESS_PREFIX: &str = "ASTRAL_PROGRESS ";
const UV_HTTP_TIMEOUT_SECONDS: &str = "45";
const UV_HTTP_RETRIES: &str = "3";

// MediaPipe imports its Tasks vision helpers at package import time. These are
// exact, verified Windows/Python 3.11 wheels; sounddevice is intentionally not
// installed because this backend consumes FFmpeg PCM and never records audio.
const LOCKED_REQUIREMENTS: &[&str] = &[
    "absl-py==2.5.0",
    "certifi==2026.7.22",
    "contourpy==1.3.3",
    "cycler==0.12.1",
    "flatbuffers==25.12.19",
    "fonttools==4.63.0",
    "kiwisolver==1.5.0",
    "matplotlib==3.11.1",
    "mediapipe==0.10.35",
    "numpy==1.26.4",
    "opencv-contrib-python-headless==4.10.0.84",
    "packaging==26.2",
    "pillow==12.3.0",
    "pyparsing==3.3.2",
    "python-dateutil==2.9.0.post0",
    "six==1.17.0",
];

const RUNNER_SCRIPT: &str = include_str!("../scripts/yamnet_laughter.py");

static RUNTIME_LOCK: Mutex<()> = Mutex::new(());
static MODEL_LOCK: Mutex<()> = Mutex::new(());
static ANALYSIS_LOCK: Mutex<()> = Mutex::new(());
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeLaughterRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f64,
    #[serde(default)]
    pub min_confidence: Option<f64>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaughterEvent {
    pub kind: &'static str,
    pub label: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f64,
    pub average_confidence: f64,
    pub frame_count: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeLaughterResponse {
    pub engine: &'static str,
    pub engine_version: &'static str,
    pub model: &'static str,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub window_ms: u64,
    pub hop_ms: u64,
    pub min_confidence: f64,
    pub events: Vec<LaughterEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaughterAnalysisProgress {
    pub stage: String,
    pub progress_percent: Option<f64>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerOutput {
    schema: String,
    sample_rate: u32,
    window_ms: u64,
    hop_ms: u64,
    duration_ms: u64,
    frames: Vec<RunnerFrame>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerFrame {
    t: u64,
    s: f64,
    l: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerProgress {
    progress_percent: f64,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    schema: String,
    runner_sha256: String,
    python_version: String,
    mediapipe_version: String,
    numpy_version: String,
}

#[derive(Debug, Clone)]
struct RuntimePaths {
    root: PathBuf,
    python: PathBuf,
    runner: PathBuf,
}

struct ScopedDirectory {
    path: PathBuf,
    committed: bool,
}

impl ScopedDirectory {
    fn create(parent: &Path, label: &str) -> Result<Self, RenderErrorReport> {
        fs::create_dir_all(parent).map_err(|error| {
            laughter_error(
                "laughter_runtime_directory_failed",
                "Kahkaha algılama klasörü hazırlanamadı.",
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
                Ok(()) => {
                    return Ok(Self {
                        path,
                        committed: false,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(laughter_error(
                        "laughter_runtime_directory_failed",
                        "Kahkaha algılama çalışma klasörü hazırlanamadı.",
                        error.to_string(),
                        true,
                    ))
                }
            }
        }
        Err(laughter_error(
            "laughter_runtime_directory_failed",
            "Kahkaha algılama çalışma klasörü hazırlanamadı.",
            "Could not allocate a unique staging directory",
            true,
        ))
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ScopedDirectory {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct PartialFile {
    path: PathBuf,
    committed: bool,
}

impl PartialFile {
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

impl Drop for PartialFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[tauri::command]
pub async fn analyze_laughter(
    app: AppHandle,
    request: AnalyzeLaughterRequest,
) -> Result<AnalyzeLaughterResponse, RenderErrorReport> {
    validate_request(&request)?;
    let source = validate_source_path(&request.source_path)?;
    let ffmpeg = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());
    tauri::async_runtime::spawn_blocking(move || analyze_blocking(&app, &ffmpeg, &source, &request))
        .await
        .map_err(|error| {
            laughter_error(
                "laughter_worker_failed",
                "Kahkaha analizi tamamlanamadı.",
                format!("Laughter analysis worker failed: {error}"),
                true,
            )
        })?
}

fn analyze_blocking(
    app: &AppHandle,
    ffmpeg: &Path,
    source: &Path,
    request: &AnalyzeLaughterRequest,
) -> Result<AnalyzeLaughterResponse, RenderErrorReport> {
    let _analysis_guard = ANALYSIS_LOCK.lock().map_err(|_| {
        laughter_error(
            "laughter_analysis_lock_failed",
            "Kahkaha motoru şu anda kullanılamıyor.",
            "Laughter analysis lock was poisoned",
            true,
        )
    })?;
    ensure_not_cancelled(request, "before setup")?;
    let data_root = laughter_data_root(app)?;
    emit_progress(
        app,
        "preparing",
        Some(2.0),
        "Yerel YAMNet kahkaha motoru hazırlanıyor.",
    );
    let model = ensure_model(app, &data_root)?;
    ensure_not_cancelled(request, "after model setup")?;
    let runtime = ensure_runtime(app, &data_root)?;
    ensure_not_cancelled(request, "after runtime setup")?;
    let expected_duration_ms = post_speed_duration_ms(request.duration_ms, request.playback_rate);
    let mut work = ScopedDirectory::create(&data_root.join("temp"), "laughter-analysis")?;
    let pcm_path = work.path.join("clip-16k-mono.f32");
    let classifier_output_path = work.path.join("yamnet-result.json");

    emit_progress(
        app,
        "extracting-audio",
        Some(70.0),
        "Klip sesi 16 kHz mono analize hazırlanıyor.",
    );
    extract_pcm(ffmpeg, source, request, &pcm_path)?;
    emit_progress(
        app,
        "classifying",
        Some(78.0),
        "YAMNet kahkaha ve gülme sınıflarını tarıyor.",
    );
    let output = run_classifier(
        app,
        &runtime,
        &model,
        &pcm_path,
        expected_duration_ms,
        &classifier_output_path,
        request.operation_id.as_deref(),
    )?;
    let min_confidence = request.min_confidence.unwrap_or(DEFAULT_MIN_CONFIDENCE);
    let response = response_from_runner(output, min_confidence)?;
    // The work directory has no persistent output; mark it committed only to
    // avoid a second removal attempt after the explicit cleanup.
    fs::remove_dir_all(&work.path).map_err(|error| {
        laughter_error(
            "laughter_temp_cleanup_failed",
            "Kahkaha analizi tamamlandı ancak geçici ses temizlenemedi.",
            format!("{}: {error}", work.path.display()),
            true,
        )
    })?;
    work.commit();
    emit_progress(
        app,
        "complete",
        Some(100.0),
        "Yerel kahkaha analizi tamamlandı.",
    );
    Ok(response)
}

fn response_from_runner(
    output: RunnerOutput,
    min_confidence: f64,
) -> Result<AnalyzeLaughterResponse, RenderErrorReport> {
    validate_runner_output(&output)?;
    let events = merge_laughter_frames(&output.frames, output.duration_ms, min_confidence);
    Ok(AnalyzeLaughterResponse {
        engine: ENGINE,
        engine_version: ENGINE_VERSION,
        model: MODEL,
        duration_ms: output.duration_ms,
        sample_rate: output.sample_rate,
        window_ms: output.window_ms,
        hop_ms: output.hop_ms,
        min_confidence,
        events,
    })
}

fn validate_runner_output(output: &RunnerOutput) -> Result<(), RenderErrorReport> {
    if output.schema != OUTPUT_SCHEMA
        || output.sample_rate != SAMPLE_RATE
        || output.window_ms != WINDOW_MS
        || !(300..=1_200).contains(&output.hop_ms)
        || output.duration_ms == 0
    {
        return Err(output_error(
            "YAMNet output contract did not match the pinned model",
        ));
    }
    let max_frames = (output.duration_ms / 250).saturating_add(32) as usize;
    if output.frames.is_empty() || output.frames.len() > max_frames {
        return Err(output_error(format!(
            "Unexpected YAMNet frame count: {} (limit {max_frames})",
            output.frames.len()
        )));
    }
    let allowed = laughter_labels();
    let mut previous_time = None;
    for frame in &output.frames {
        if frame.t >= output.duration_ms
            || previous_time.is_some_and(|previous| frame.t <= previous)
            || !frame.s.is_finite()
            || !(0.0..=1.0).contains(&frame.s)
            || !allowed.contains(frame.l.as_str())
        {
            return Err(output_error("YAMNet returned an invalid frame"));
        }
        previous_time = Some(frame.t);
    }
    Ok(())
}

fn merge_laughter_frames(
    frames: &[RunnerFrame],
    duration_ms: u64,
    min_confidence: f64,
) -> Vec<LaughterEvent> {
    #[derive(Debug)]
    struct PendingEvent {
        start_ms: u64,
        end_ms: u64,
        peak: f64,
        sum: f64,
        count: u32,
        label: String,
    }

    fn finish(event: PendingEvent) -> Option<LaughterEvent> {
        if event.count < 2 && event.peak < STRONG_SINGLE_FRAME_CONFIDENCE {
            return None;
        }
        let kind = if matches!(event.label.as_str(), "Giggle" | "Snicker") {
            "giggle"
        } else {
            "laughter"
        };
        Some(LaughterEvent {
            kind,
            label: event.label,
            start_ms: event.start_ms,
            end_ms: event.end_ms,
            confidence: round_score(event.peak),
            average_confidence: round_score(event.sum / f64::from(event.count)),
            frame_count: event.count,
        })
    }

    let mut events = Vec::new();
    let mut current: Option<PendingEvent> = None;
    for frame in frames.iter().filter(|frame| frame.s >= min_confidence) {
        let frame_end = frame.t.saturating_add(WINDOW_MS).min(duration_ms);
        let can_merge = current
            .as_ref()
            .is_some_and(|event| frame.t <= event.end_ms.saturating_add(MERGE_GAP_MS));
        if !can_merge {
            if let Some(event) = current.take().and_then(finish) {
                events.push(event);
            }
            current = Some(PendingEvent {
                start_ms: frame.t,
                end_ms: frame_end,
                peak: frame.s,
                sum: frame.s,
                count: 1,
                label: frame.l.clone(),
            });
            continue;
        }
        if let Some(event) = current.as_mut() {
            event.end_ms = event.end_ms.max(frame_end);
            event.sum += frame.s;
            event.count = event.count.saturating_add(1);
            if frame.s > event.peak {
                event.peak = frame.s;
                event.label.clone_from(&frame.l);
            }
        }
    }
    if let Some(event) = current.and_then(finish) {
        events.push(event);
    }
    events
}

fn ensure_runtime(app: &AppHandle, data_root: &Path) -> Result<RuntimePaths, RenderErrorReport> {
    let _guard = RUNTIME_LOCK.lock().map_err(|_| {
        laughter_error(
            "laughter_runtime_lock_failed",
            "Kahkaha motoru kurulumu şu anda kullanılamıyor.",
            "YAMNet runtime lock was poisoned",
            true,
        )
    })?;
    let runtime_parent = data_root.join("runtime");
    let runtime_dir = runtime_parent.join(RUNTIME_DIRECTORY_NAME);
    if let Some(runtime) = valid_runtime(&runtime_dir) {
        return Ok(runtime);
    }
    fs::create_dir_all(data_root.join("temp")).map_err(runtime_directory_error)?;
    fs::create_dir_all(&runtime_parent).map_err(runtime_directory_error)?;

    emit_progress(
        app,
        "runtime-setup",
        Some(8.0),
        "İzole MediaPipe ortamı hazırlanıyor.",
    );
    let quality_root = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("clearvoice-experimental"))
        .map_err(|error| {
            laughter_error(
                "laughter_uv_directory_failed",
                "Yerel AI kurulum aracı klasörü açılamadı.",
                error.to_string(),
                true,
            )
        })?;
    let uv = crate::clearvoice_experimental::ensure_uv(&quality_root).map_err(|error| {
        let mut report = laughter_error(
            "laughter_uv_download_failed",
            "Yerel AI kurulum aracı indirilemedi.",
            error.technical_message,
            true,
        );
        report.stderr_tail = error.stderr_tail;
        report
    })?;
    let mut staging = ScopedDirectory::create(&runtime_parent, "yamnet-runtime")?;
    let mut venv = Command::new(&uv);
    venv.args([
        OsString::from("venv"),
        OsString::from("--python"),
        OsString::from(PYTHON_VERSION_REQUEST),
        OsString::from("--managed-python"),
        OsString::from("--allow-existing"),
        OsString::from("--no-project"),
        OsString::from("--no-config"),
        staging.path.as_os_str().to_owned(),
    ]);
    configure_uv_command(&mut venv, data_root);
    run_checked(
        &mut venv,
        "laughter_python_setup_failed",
        "Kahkaha motoru için Python ortamı kurulamadı.",
    )?;
    let python = venv_python(&staging.path);
    if !python.is_file() {
        return Err(laughter_error(
            "laughter_python_missing",
            "Kahkaha motorunun Python ortamı eksik kaldı.",
            format!("Expected {}", python.display()),
            true,
        ));
    }

    emit_progress(
        app,
        "runtime-setup",
        Some(28.0),
        "MediaPipe YAMNet bağımlılıkları kuruluyor.",
    );
    let mut install = Command::new(&uv);
    install
        .arg("pip")
        .arg("install")
        .arg("--python")
        .arg(&python)
        .arg("--no-deps")
        .arg("--no-config")
        .arg("--default-index")
        .arg("https://pypi.org/simple");
    for requirement in LOCKED_REQUIREMENTS {
        install.arg(requirement);
    }
    configure_uv_command(&mut install, data_root);
    run_checked(
        &mut install,
        "laughter_dependencies_failed",
        "MediaPipe YAMNet paketleri kurulamadı. İnternet bağlantısını kontrol edin.",
    )?;

    emit_progress(
        app,
        "runtime-setup",
        Some(62.0),
        "MediaPipe YAMNet ortamı doğrulanıyor.",
    );
    let (python_version, mediapipe_version, numpy_version) = verify_runtime(&python, data_root)?;
    let runner = staging.path.join(RUNNER_FILE_NAME);
    fs::write(&runner, RUNNER_SCRIPT.as_bytes()).map_err(|error| {
        laughter_error(
            "laughter_runner_write_failed",
            "Kahkaha sınıflandırıcısı hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let manifest = RuntimeManifest {
        schema: RUNTIME_SCHEMA.into(),
        runner_sha256: runner_sha256(),
        python_version,
        mediapipe_version,
        numpy_version,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        laughter_error(
            "laughter_runtime_manifest_failed",
            "Kahkaha motoru doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    fs::write(staging.path.join(RUNTIME_MANIFEST_NAME), manifest_bytes).map_err(|error| {
        laughter_error(
            "laughter_runtime_manifest_failed",
            "Kahkaha motoru doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;

    if runtime_dir.exists() {
        remove_scoped_runtime(&runtime_parent, &runtime_dir)?;
    }
    fs::rename(&staging.path, &runtime_dir).map_err(|error| {
        laughter_error(
            "laughter_runtime_commit_failed",
            "Kahkaha motoru etkinleştirilemedi.",
            format!(
                "{} -> {}: {error}",
                staging.path.display(),
                runtime_dir.display()
            ),
            true,
        )
    })?;
    staging.commit();
    valid_runtime(&runtime_dir).ok_or_else(|| {
        laughter_error(
            "laughter_runtime_verification_failed",
            "Kahkaha motoru kuruldu ancak doğrulanamadı.",
            runtime_dir.display().to_string(),
            true,
        )
    })
}

fn valid_runtime(runtime_dir: &Path) -> Option<RuntimePaths> {
    let python = venv_python(runtime_dir);
    let runner = runtime_dir.join(RUNNER_FILE_NAME);
    if !python.is_file()
        || !runner.is_file()
        || fs::read(&runner).ok()?.as_slice() != RUNNER_SCRIPT.as_bytes()
    {
        return None;
    }
    let manifest: RuntimeManifest =
        serde_json::from_slice(&fs::read(runtime_dir.join(RUNTIME_MANIFEST_NAME)).ok()?).ok()?;
    if manifest.schema != RUNTIME_SCHEMA
        || manifest.runner_sha256 != runner_sha256()
        || manifest.mediapipe_version != ENGINE_VERSION
        || manifest.numpy_version != "1.26.4"
    {
        return None;
    }
    Some(RuntimePaths {
        root: runtime_dir.to_owned(),
        python,
        runner,
    })
}

fn verify_runtime(
    python: &Path,
    data_root: &Path,
) -> Result<(String, String, String), RenderErrorReport> {
    let script = format!(
        "import json,platform,mediapipe,numpy; from mediapipe.tasks import python as mp_python; from mediapipe.tasks.python import audio; from mediapipe.tasks.python.components.containers import AudioData; assert mediapipe.__version__ == '{ENGINE_VERSION}'; assert numpy.__version__ == '1.26.4'; print(json.dumps([platform.python_version(),mediapipe.__version__,numpy.__version__]))"
    );
    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_python_command(&mut command, data_root);
    configure_background_process(&mut command);
    let output = command.output().map_err(|error| {
        laughter_error(
            "laughter_runtime_verification_failed",
            "MediaPipe YAMNet ortamı doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !output.status.success() {
        return Err(error_with_stderr(
            "laughter_runtime_verification_failed",
            "MediaPipe YAMNet ortamı doğrulanamadı.",
            format!("Python verification exited with {}", output.status),
            true,
            stderr_tail(&output.stderr),
        ));
    }
    let versions: [String; 3] =
        serde_json::from_slice(trim_ascii(&output.stdout)).map_err(|error| {
            laughter_error(
                "laughter_runtime_verification_failed",
                "MediaPipe YAMNet sürümleri doğrulanamadı.",
                error.to_string(),
                false,
            )
        })?;
    Ok((
        versions[0].clone(),
        versions[1].clone(),
        versions[2].clone(),
    ))
}

fn ensure_model(app: &AppHandle, data_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let _guard = MODEL_LOCK.lock().map_err(|_| {
        laughter_error(
            "laughter_model_lock_failed",
            "YAMNet modeli şu anda hazırlanamadı.",
            "YAMNet model lock was poisoned",
            true,
        )
    })?;
    let model_dir = data_root.join("models");
    fs::create_dir_all(&model_dir).map_err(|error| {
        laughter_error(
            "laughter_model_directory_failed",
            "YAMNet model klasörü hazırlanamadı.",
            format!("{}: {error}", model_dir.display()),
            true,
        )
    })?;
    let destination = model_dir.join(MODEL_FILE_NAME);
    if model_is_valid(&destination)? {
        return Ok(destination);
    }
    if destination.is_file() {
        fs::remove_file(&destination).map_err(|error| {
            laughter_error(
                "laughter_model_replace_failed",
                "YAMNet modelinin eski kopyası yenilenemedi.",
                format!("{}: {error}", destination.display()),
                true,
            )
        })?;
    }
    let partial = model_dir.join(format!(".{MODEL_FILE_NAME}.partial"));
    if partial.is_file() {
        fs::remove_file(&partial).map_err(|error| {
            laughter_error(
                "laughter_model_replace_failed",
                "YAMNet modelinin yarım indirmesi temizlenemedi.",
                format!("{}: {error}", partial.display()),
                true,
            )
        })?;
    }
    let mut partial_guard = PartialFile::new(partial.clone());
    emit_progress(
        app,
        "model-download",
        Some(4.0),
        "Resmî MediaPipe YAMNet modeli indiriliyor.",
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(45)))
        .build()
        .into();
    let mut response = agent.get(MODEL_URL).call().map_err(|error| {
        laughter_error(
            "laughter_model_download_failed",
            "YAMNet modeli indirilemedi; internet bağlantısını kontrol edin.",
            format!("{MODEL_URL}: {error}"),
            true,
        )
    })?;
    let content_length = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if content_length > MAX_MODEL_BYTES {
        return Err(laughter_error(
            "laughter_model_too_large",
            "YAMNet model boyutu doğrulanamadı.",
            format!("Content-Length {content_length} exceeded {MAX_MODEL_BYTES}"),
            false,
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|error| {
            laughter_error(
                "laughter_model_write_failed",
                "YAMNet modeli diske yazılamadı.",
                format!("{}: {error}", partial.display()),
                true,
            )
        })?;
    let mut writer = BufWriter::new(file);
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            laughter_error(
                "laughter_model_download_failed",
                "YAMNet modeli indirilemedi.",
                error.to_string(),
                true,
            )
        })?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        if bytes > MAX_MODEL_BYTES {
            return Err(laughter_error(
                "laughter_model_too_large",
                "YAMNet model boyutu doğrulanamadı.",
                format!("Download exceeded {MAX_MODEL_BYTES} bytes"),
                false,
            ));
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read]).map_err(|error| {
            laughter_error(
                "laughter_model_write_failed",
                "YAMNet modeli diske yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
    }
    writer.flush().map_err(|error| {
        laughter_error(
            "laughter_model_write_failed",
            "YAMNet modeli diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    writer.get_ref().sync_all().map_err(|error| {
        laughter_error(
            "laughter_model_write_failed",
            "YAMNet modeli diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    drop(writer);
    let actual_sha = format!("{:x}", hasher.finalize());
    if bytes != MODEL_BYTES || actual_sha != MODEL_SHA256 {
        return Err(laughter_error(
            "laughter_model_checksum_failed",
            "YAMNet modelinin güvenlik doğrulaması başarısız oldu.",
            format!("Expected {MODEL_BYTES} bytes/{MODEL_SHA256}, got {bytes} bytes/{actual_sha}"),
            false,
        ));
    }
    fs::rename(&partial, &destination).map_err(|error| {
        laughter_error(
            "laughter_model_commit_failed",
            "YAMNet modeli etkinleştirilemedi.",
            format!(
                "{} -> {}: {error}",
                partial.display(),
                destination.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    Ok(destination)
}

fn model_is_valid(path: &Path) -> Result<bool, RenderErrorReport> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(laughter_error(
                "laughter_model_read_failed",
                "YAMNet modeli okunamadı.",
                error.to_string(),
                true,
            ))
        }
    };
    if !metadata.is_file() || metadata.len() != MODEL_BYTES {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|error| {
        laughter_error(
            "laughter_model_read_failed",
            "YAMNet modeli okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(|error| {
        laughter_error(
            "laughter_model_read_failed",
            "YAMNet modeli doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    Ok(format!("{:x}", hasher.finalize()) == MODEL_SHA256)
}

fn extract_pcm(
    ffmpeg: &Path,
    source: &Path,
    request: &AnalyzeLaughterRequest,
    output: &Path,
) -> Result<(), RenderErrorReport> {
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-ss",
        ])
        .arg(seconds(request.start_ms))
        .arg("-t")
        .arg(seconds(request.duration_ms))
        .arg("-i")
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn"]);
    let filters = atempo_filters(request.playback_rate);
    if !filters.is_empty() {
        command.arg("-af").arg(filters.join(","));
    }
    command
        .args([
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
        ])
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let child = command.spawn().map_err(|error| {
        laughter_error(
            "laughter_ffmpeg_start_failed",
            "Kahkaha analizi için FFmpeg başlatılamadı.",
            format!("{}: {error}", ffmpeg.display()),
            true,
        )
    })?;
    let result = match analysis_cancel::wait_for_output(child, request.operation_id.as_deref()) {
        Ok(output) => output,
        Err(CommandWaitError::Cancelled) => {
            return Err(cancelled_error("during FFmpeg extraction"));
        }
        Err(CommandWaitError::Io(error)) => {
            return Err(laughter_error(
                "laughter_ffmpeg_wait_failed",
                "Kahkaha analizi için FFmpeg tamamlanamadı.",
                error.to_string(),
                true,
            ));
        }
    };
    if !result.status.success() {
        return Err(error_with_stderr(
            "laughter_ffmpeg_failed",
            "Klip sesi kahkaha analizi için hazırlanamadı.",
            format!("FFmpeg exited with {}", result.status),
            true,
            stderr_tail(&result.stderr),
        ));
    }
    let bytes = output.metadata().map(|value| value.len()).unwrap_or(0);
    if bytes == 0 || bytes % 4 != 0 {
        return Err(laughter_error(
            "laughter_audio_empty",
            "Klipte kahkaha analizi için kullanılabilir ses bulunamadı.",
            format!("Unexpected float32 PCM size: {bytes}"),
            false,
        ));
    }
    Ok(())
}

fn run_classifier(
    app: &AppHandle,
    runtime: &RuntimePaths,
    model: &Path,
    pcm: &Path,
    duration_ms: u64,
    output_path: &Path,
    operation_id: Option<&str>,
) -> Result<RunnerOutput, RenderErrorReport> {
    let mut command = Command::new(&runtime.python);
    command
        .arg("-I")
        .arg(&runtime.runner)
        .arg("--model")
        .arg(model)
        .arg("--audio")
        .arg(pcm)
        .arg("--duration-ms")
        .arg(duration_ms.to_string())
        .arg("--output")
        .arg(output_path)
        .current_dir(&runtime.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_python_command(&mut command, &runtime.root);
    configure_background_process(&mut command);
    let mut child = command.spawn().map_err(|error| {
        laughter_error(
            "laughter_classifier_start_failed",
            "Yerel YAMNet kahkaha motoru başlatılamadı.",
            error.to_string(),
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        laughter_error(
            "laughter_classifier_pipe_failed",
            "Yerel YAMNet kahkaha motorunun ilerlemesi okunamadı.",
            "YAMNet stderr pipe was unavailable",
            true,
        )
    })?;
    let mut tail = VecDeque::with_capacity(32);
    let mut pipe_error = None;
    for line in BufReader::new(stderr).lines() {
        if analysis_cancel::is_cancelled(operation_id) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(cancelled_error("during YAMNet classification"));
        }
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                pipe_error = Some(laughter_error(
                    "laughter_classifier_pipe_failed",
                    "Yerel YAMNet kahkaha motorunun ilerlemesi okunamadı.",
                    error.to_string(),
                    true,
                ));
                break;
            }
        };
        if let Some(payload) = line.strip_prefix(PROGRESS_PREFIX) {
            if let Ok(progress) = serde_json::from_str::<RunnerProgress>(payload) {
                if progress.progress_percent.is_finite()
                    && (0.0..=100.0).contains(&progress.progress_percent)
                    && !progress.message.is_empty()
                    && progress.message.len() <= 256
                {
                    emit_progress(
                        app,
                        "classifying",
                        Some(78.0 + progress.progress_percent * 0.2),
                        "YAMNet kahkaha pencerelerini sınıflandırıyor.",
                    );
                }
            }
            continue;
        }
        if tail.len() == 32 {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    if let Some(error) = pipe_error {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let status = child.wait().map_err(|error| {
        laughter_error(
            "laughter_classifier_wait_failed",
            "Yerel YAMNet kahkaha motoru tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !status.success() {
        return Err(error_with_stderr(
            "laughter_classifier_failed",
            "Yerel YAMNet kahkaha analizi tamamlanamadı.",
            format!("YAMNet runner exited with {status}"),
            true,
            tail.into_iter().collect(),
        ));
    }
    let metadata = output_path
        .metadata()
        .map_err(|error| output_error(format!("{}: {error}", output_path.display())))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_RUNNER_OUTPUT_BYTES as u64
    {
        return Err(output_error(format!(
            "Unexpected YAMNet JSON size: {}",
            metadata.len()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(output_path)
        .and_then(|file| {
            file.take(MAX_RUNNER_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| output_error(format!("{}: {error}", output_path.display())))?;
    if bytes.len() > MAX_RUNNER_OUTPUT_BYTES {
        return Err(output_error("YAMNet JSON exceeded the size limit"));
    }
    serde_json::from_slice(trim_ascii(&bytes))
        .map_err(|error| output_error(format!("Invalid YAMNet JSON: {error}")))
}

fn validate_request(request: &AnalyzeLaughterRequest) -> Result<(), RenderErrorReport> {
    let min_confidence = request.min_confidence.unwrap_or(DEFAULT_MIN_CONFIDENCE);
    if request.duration_ms == 0
        || request.duration_ms > MAX_DURATION_MS
        || request
            .start_ms
            .checked_add(request.duration_ms)
            .is_none_or(|end| end > MAX_SOURCE_RANGE_MS)
        || !request.playback_rate.is_finite()
        || !(0.1..=16.0).contains(&request.playback_rate)
        || !min_confidence.is_finite()
        || !(0.05..=0.95).contains(&min_confidence)
    {
        return Err(laughter_error(
            "laughter_request_invalid",
            "Kahkaha analizi için klip aralığı, hız veya hassasiyet geçersiz.",
            format!(
                "startMs={}, durationMs={}, playbackRate={}, minConfidence={min_confidence}",
                request.start_ms, request.duration_ms, request.playback_rate
            ),
            false,
        ));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(laughter_error(
            "laughter_source_missing",
            "Kahkaha analizi için kaynak medya seçilmedi.",
            "sourcePath was empty or contained NUL",
            false,
        ));
    }
    let path = fs::canonicalize(trimmed).map_err(|error| {
        laughter_error(
            "laughter_source_unavailable",
            "Kaynak medya kahkaha analizi için açılamadı.",
            format!("{trimmed}: {error}"),
            false,
        )
    })?;
    if !path.is_file() {
        return Err(laughter_error(
            "laughter_source_invalid",
            "Kahkaha analizi kaynağı bir dosya değil.",
            path.display().to_string(),
            false,
        ));
    }
    Ok(path)
}

fn laughter_data_root(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("laughter-analysis"))
        .map_err(|error| {
            laughter_error(
                "laughter_data_directory_failed",
                "Kahkaha algılama veri klasörü açılamadı.",
                error.to_string(),
                true,
            )
        })
}

fn laughter_labels() -> HashSet<&'static str> {
    [
        "Laughter",
        "Baby laughter",
        "Giggle",
        "Snicker",
        "Belly laugh",
        "Chuckle, chortle",
    ]
    .into_iter()
    .collect()
}

fn atempo_filters(mut speed: f64) -> Vec<String> {
    let mut filters = Vec::new();
    while speed > 2.0 {
        filters.push("atempo=2".to_owned());
        speed /= 2.0;
    }
    while speed < 0.5 {
        filters.push("atempo=0.5".to_owned());
        speed /= 0.5;
    }
    if (speed - 1.0).abs() > 0.000_001 {
        filters.push(format!("atempo={speed:.8}"));
    }
    filters
}

fn post_speed_duration_ms(duration_ms: u64, playback_rate: f64) -> u64 {
    ((duration_ms as f64 / playback_rate).round() as u64).max(1)
}

fn seconds(duration_ms: u64) -> String {
    format!("{:.6}", duration_ms as f64 / 1_000.0)
}

fn round_score(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn runner_sha256() -> String {
    format!("{:x}", Sha256::digest(RUNNER_SCRIPT.as_bytes()))
}

fn venv_python(runtime_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        runtime_dir.join("Scripts").join("python.exe")
    } else {
        runtime_dir.join("bin").join("python")
    }
}

fn remove_scoped_runtime(parent: &Path, target: &Path) -> Result<(), RenderErrorReport> {
    if target.parent() != Some(parent)
        || target.file_name().and_then(|name| name.to_str()) != Some(RUNTIME_DIRECTORY_NAME)
    {
        return Err(laughter_error(
            "laughter_runtime_scope_failed",
            "Kahkaha motorunun eski kurulumu güvenle değiştirilemedi.",
            target.display().to_string(),
            false,
        ));
    }
    fs::remove_dir_all(target).map_err(|error| {
        laughter_error(
            "laughter_runtime_replace_failed",
            "Kahkaha motorunun eski kurulumu değiştirilemedi.",
            format!("{}: {error}", target.display()),
            true,
        )
    })
}

fn configure_uv_command(command: &mut Command, data_root: &Path) {
    configure_minimal_environment(command, data_root);
    command
        .env("UV_CACHE_DIR", data_root.join("uv-cache"))
        .env("UV_PYTHON_INSTALL_DIR", data_root.join("python"))
        .env("UV_MANAGED_PYTHON", "1")
        .env("UV_PYTHON_DOWNLOADS", "automatic")
        .env("UV_HTTP_TIMEOUT", UV_HTTP_TIMEOUT_SECONDS)
        .env("UV_HTTP_RETRIES", UV_HTTP_RETRIES)
        .env("UV_NO_PROJECT", "1")
        .env("UV_NO_CONFIG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process(command);
}

fn configure_python_command(command: &mut Command, data_root: &Path) {
    configure_minimal_environment(command, data_root);
    command.env("PYTHONNOUSERSITE", "1");
}

fn configure_minimal_environment(command: &mut Command, data_root: &Path) {
    command
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap_or_default())
        .env("SYSTEMROOT", env::var_os("SYSTEMROOT").unwrap_or_default())
        .env("WINDIR", env::var_os("WINDIR").unwrap_or_default())
        .env("TEMP", data_root.join("temp"))
        .env("TMP", data_root.join("temp"));
}

fn run_checked(
    command: &mut Command,
    code: &'static str,
    user_message: &'static str,
) -> Result<(), RenderErrorReport> {
    let output = command
        .output()
        .map_err(|error| laughter_error(code, user_message, error.to_string(), true))?;
    if output.status.success() {
        return Ok(());
    }
    Err(error_with_stderr(
        code,
        user_message,
        format!("Command exited with {}", output.status),
        true,
        stderr_tail(&output.stderr),
    ))
}

fn runtime_directory_error(error: io::Error) -> RenderErrorReport {
    laughter_error(
        "laughter_runtime_directory_failed",
        "Kahkaha motoru klasörü hazırlanamadı.",
        error.to_string(),
        true,
    )
}

fn output_error(message: impl Into<String>) -> RenderErrorReport {
    laughter_error(
        "laughter_output_invalid",
        "YAMNet kahkaha motoru geçersiz bir sonuç üretti.",
        message,
        false,
    )
}

fn ensure_not_cancelled(
    request: &AnalyzeLaughterRequest,
    stage: &str,
) -> Result<(), RenderErrorReport> {
    if analysis_cancel::is_cancelled(request.operation_id.as_deref()) {
        return Err(cancelled_error(stage));
    }
    Ok(())
}

fn cancelled_error(stage: &str) -> RenderErrorReport {
    laughter_error(
        "smart_shorts_cancelled",
        "Akıllı Shorts analizi iptal edildi.",
        format!("Laughter analysis cancelled {stage}"),
        false,
    )
}

fn laughter_error(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    retryable: bool,
) -> RenderErrorReport {
    error_with_stderr(code, user_message, technical_message, retryable, Vec::new())
}

fn error_with_stderr(
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

fn stderr_tail(bytes: &[u8]) -> Vec<String> {
    let value = String::from_utf8_lossy(bytes);
    let lines = value.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(32)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn emit_progress(app: &AppHandle, stage: &str, percent: Option<f64>, message: &str) {
    let _ = app.emit(
        LAUGHTER_ANALYSIS_PROGRESS_EVENT,
        LaughterAnalysisProgress {
            stage: stage.to_owned(),
            progress_percent: percent,
            message: message.to_owned(),
        },
    );
}

fn default_playback_rate() -> f64 {
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: u64, s: f64, label: &str) -> RunnerFrame {
        RunnerFrame {
            t,
            s,
            l: label.to_owned(),
        }
    }

    #[test]
    fn adjacent_neural_frames_become_one_timestamped_event() {
        let events = merge_laughter_frames(
            &[
                frame(1_000, 0.34, "Laughter"),
                frame(1_480, 0.71, "Belly laugh"),
                frame(1_960, 0.52, "Laughter"),
                frame(5_000, 0.10, "Giggle"),
            ],
            8_000,
            0.28,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].start_ms, 1_000);
        assert_eq!(events[0].end_ms, 2_920);
        assert_eq!(events[0].label, "Belly laugh");
        assert_eq!(events[0].confidence, 0.71);
        assert_eq!(events[0].frame_count, 3);
        assert_eq!(events[0].kind, "laughter");
    }

    #[test]
    fn weak_single_frame_is_rejected_but_strong_chuckle_survives() {
        let events = merge_laughter_frames(
            &[
                frame(0, 0.31, "Laughter"),
                frame(4_000, 0.62, "Chuckle, chortle"),
            ],
            6_000,
            0.28,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].start_ms, 4_000);
        assert_eq!(events[0].end_ms, 4_960);
    }

    #[test]
    fn giggle_labels_are_preserved_as_giggle_events() {
        let events = merge_laughter_frames(
            &[frame(500, 0.4, "Giggle"), frame(980, 0.44, "Snicker")],
            3_000,
            0.28,
        );
        assert_eq!(events[0].kind, "giggle");
    }

    #[test]
    fn loudness_is_not_an_input_to_the_neural_event_merger() {
        let events = merge_laughter_frames(
            &[frame(0, 0.0, "Laughter"), frame(480, 0.0, "Laughter")],
            2_000,
            0.28,
        );
        assert!(events.is_empty());
    }

    #[test]
    fn runner_contract_rejects_unknown_audioset_labels() {
        let output = RunnerOutput {
            schema: OUTPUT_SCHEMA.into(),
            sample_rate: SAMPLE_RATE,
            window_ms: WINDOW_MS,
            hop_ms: 480,
            duration_ms: 1_000,
            frames: vec![frame(0, 0.5, "Explosion")],
        };
        assert_eq!(
            validate_runner_output(&output).unwrap_err().code,
            "laughter_output_invalid"
        );
    }

    #[test]
    fn playback_rate_maps_to_post_speed_timeline_time() {
        assert_eq!(post_speed_duration_ms(30_000, 2.0), 15_000);
        assert_eq!(post_speed_duration_ms(30_000, 0.5), 60_000);
    }

    #[test]
    fn model_and_runner_are_content_pinned() {
        assert_eq!(MODEL_BYTES, 4_126_810);
        assert_eq!(MODEL_SHA256.len(), 64);
        assert!(MODEL_URL.contains("/float32/1/"));
        assert_eq!(runner_sha256().len(), 64);
        assert!(LOCKED_REQUIREMENTS.contains(&"mediapipe==0.10.35"));
        assert!(LOCKED_REQUIREMENTS.contains(&"numpy==1.26.4"));
    }
}
