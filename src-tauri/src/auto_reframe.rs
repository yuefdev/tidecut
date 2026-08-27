use std::{
    collections::VecDeque,
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex,
    },
    thread,
    time::{Duration, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    analysis_cancel,
    render::{configure_background_process, RenderErrorReport},
};

pub const AUTO_REFRAME_PROGRESS_EVENT: &str = "auto-reframe://progress";

const TRACKER_SCHEMA: &str = "astral-auto-reframe-track-v1";
const CACHE_SCHEMA: &str = "astral-auto-reframe-cache-v2-vittrack";
const RUNTIME_SCHEMA: &str = "astral-auto-reframe-runtime-v2-vittrack";
const ENGINE: &str = "opencv-vittrack-csrt-fallback";
const CONFIDENCE_METRIC: &str = "vittrack-score-or-binary-fallback";
const RUNTIME_DIRECTORY_NAME: &str = "opencv-csrt-py311-v1";
const RUNTIME_MANIFEST_NAME: &str = ".astral-auto-reframe-runtime.json";
const RUNNER_FILE_NAME: &str = "auto_reframe_tracker.py";
const PYTHON_VERSION_REQUEST: &str = "3.11";
const OPENCV_VERSION: &str = "4.10.0";
const OPENCV_PACKAGE_VERSION: &str = "4.10.0.84";
const NUMPY_VERSION: &str = "1.26.4";
const OPENCV_LICENSE: &str = "Apache-2.0";
const NUMPY_LICENSE: &str = "BSD-3-Clause";
const VIT_MODEL_NAME: &str = "object_tracking_vittrack_2023sep.onnx";
const VIT_MODEL_VERSION: &str = "opencv-zoo-4.10.0-2023sep";
const VIT_MODEL_URL: &str = "https://github.com/opencv/opencv_zoo/raw/4.10.0/models/object_tracking_vittrack/object_tracking_vittrack_2023sep.onnx";
const VIT_MODEL_SHA256: &str = "2990f0b7cd44d92afa48cd97db6de7be113fc1d9594fddb74e2725c10478e91d";
const VIT_MODEL_LICENSE: &str = "Apache-2.0";
const MAX_VIT_MODEL_BYTES: u64 = 2 * 1024 * 1024;
const OPENCV_REQUIREMENT: &str = "opencv-contrib-python-headless==4.10.0.84";
const NUMPY_REQUIREMENT: &str = "numpy==1.26.4";
const UV_HTTP_TIMEOUT_SECONDS: &str = "45";
const UV_HTTP_RETRIES: &str = "3";
const MAX_ANALYSIS_DURATION_MS: u64 = 6 * 60 * 60 * 1_000;
const MAX_SOURCE_RANGE_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const MIN_SAMPLE_FPS: f64 = 0.5;
const MAX_SAMPLE_FPS: f64 = 12.0;
const MIN_ANALYSIS_DIMENSION: u32 = 320;
const MAX_ANALYSIS_DIMENSION: u32 = 1_920;
const MAX_TRACKER_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TRACK_SAMPLES: usize = 300_000;
const STDERR_TAIL_LINES: usize = 40;
const PROGRESS_PREFIX: &str = "ASTRAL_PROGRESS ";

const TRACKER_SCRIPT: &str = include_str!("../scripts/auto_reframe_tracker.py");

const FACECAM_SCHEMA: &str = "astral-facecam-detect-v1";
const FACECAM_CACHE_SCHEMA: &str = "astral-facecam-cache-v1";
const FACECAM_ENGINE: &str = "opencv-haar-facecam";
const FACECAM_CONFIDENCE_METRIC: &str = "haar-cluster-stability";
const FACECAM_RUNNER_FILE_NAME: &str = "facecam_detector.py";
const MIN_FACECAM_INTERVAL_MS: u64 = 250;
const MAX_FACECAM_INTERVAL_MS: u64 = 20_000;
const FACECAM_SCRIPT: &str = include_str!("../scripts/facecam_detector.py");

static RUNTIME_LOCK: Mutex<()> = Mutex::new(());
static MODEL_LOCK: Mutex<()> = Mutex::new(());
static ANALYSIS_LOCK: Mutex<()> = Mutex::new(());
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeAutoReframeRequest {
    pub source_path: String,
    #[serde(default, alias = "trimStartMs")]
    pub start_ms: u64,
    pub duration_ms: u64,
    /// Milliseconds relative to `start_ms`, not an absolute media timestamp.
    pub seed_time_ms: u64,
    pub selection: NormalizedBoundingBox,
    #[serde(default = "default_sample_fps")]
    pub sample_fps: f64,
    #[serde(default = "default_analysis_max_dimension")]
    pub analysis_max_dimension: u32,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AutoReframeTrackSample {
    /// Milliseconds relative to the requested `start_ms`.
    pub time_ms: u64,
    pub bbox: NormalizedBoundingBox,
    /// ViTTrack exposes a tracking score; the CSRT fallback reports binary success.
    /// This is a tracker-specific signal, not a calibrated probability.
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AutoReframeAnalysis {
    pub engine: String,
    pub engine_version: String,
    pub confidence_metric: String,
    pub source_width: u32,
    pub source_height: u32,
    pub source_fps: Option<f64>,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub seed_time_ms: u64,
    pub sample_fps: f64,
    pub analysis_max_dimension: u32,
    pub selection: NormalizedBoundingBox,
    pub samples: Vec<AutoReframeTrackSample>,
    pub warnings: Vec<String>,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReframeRuntimeStatus {
    pub ready: bool,
    pub engine: String,
    pub runtime_dir: String,
    pub python_path: Option<String>,
    pub python_version: Option<String>,
    pub opencv_version: Option<String>,
    pub numpy_version: Option<String>,
    pub packages: Vec<AutoReframeRuntimePackage>,
    pub uv_available: bool,
    pub diagnostic: Option<RenderErrorReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReframeRuntimePackage {
    pub name: String,
    pub version: String,
    pub declared_license: String,
    pub free_and_open_source: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReframeProgress {
    pub operation: String,
    pub stage: String,
    pub progress_percent: Option<f64>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactTrackerOutput {
    schema: String,
    request_key: String,
    engine: String,
    engine_version: String,
    source_width: u32,
    source_height: u32,
    source_fps: Option<f64>,
    confidence_metric: String,
    samples: Vec<CompactTrackSample>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CompactTrackSample {
    t: u64,
    b: [f64; 4],
    c: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonProgress {
    stage: String,
    progress_percent: f64,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    schema: String,
    runner_sha256: String,
    python_version: String,
    opencv_version: String,
    numpy_version: String,
}

#[derive(Debug, Clone)]
struct RuntimePaths {
    root: PathBuf,
    python: PathBuf,
    runner: PathBuf,
    manifest: RuntimeManifest,
}

#[derive(Debug, Clone)]
struct SourceFingerprint {
    canonical_path: PathBuf,
    bytes: u64,
    modified_seconds: u64,
    modified_nanos: u32,
}

#[derive(Debug, Clone)]
struct ValidatedRequest {
    request: AnalyzeAutoReframeRequest,
    source: SourceFingerprint,
}

struct ScopedDirectory {
    path: PathBuf,
    committed: bool,
}

impl ScopedDirectory {
    fn create(parent: &Path, label: &str) -> Result<Self, RenderErrorReport> {
        fs::create_dir_all(parent).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_runtime_directory_failed",
                "Yerel takip motoru klasörü hazırlanamadı.",
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
                    return Err(auto_reframe_error(
                        "auto_reframe_runtime_directory_failed",
                        "Yerel takip motoru çalışma klasörü hazırlanamadı.",
                        error.to_string(),
                        true,
                    ))
                }
            }
        }
        Err(auto_reframe_error(
            "auto_reframe_runtime_directory_failed",
            "Yerel takip motoru çalışma klasörü hazırlanamadı.",
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

fn default_sample_fps() -> f64 {
    6.0
}

fn default_analysis_max_dimension() -> u32 {
    960
}

#[tauri::command]
pub fn check_auto_reframe_runtime(
    app: AppHandle,
) -> Result<AutoReframeRuntimeStatus, RenderErrorReport> {
    let data_root = auto_reframe_data_root(&app)?;
    Ok(runtime_status(&app, &data_root))
}

#[tauri::command]
pub async fn setup_auto_reframe_runtime(
    app: AppHandle,
    force: Option<bool>,
) -> Result<AutoReframeRuntimeStatus, RenderErrorReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let data_root = auto_reframe_data_root(&app)?;
        ensure_vit_model(&app, &data_root)?;
        let runtime = ensure_runtime_at(&app, &data_root, force.unwrap_or(false))?;
        Ok(status_from_runtime(&app, &runtime))
    })
    .await
    .map_err(|error| {
        auto_reframe_error(
            "auto_reframe_setup_task_failed",
            "Yerel takip motoru kurulumu tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?
}

#[tauri::command]
pub async fn analyze_auto_reframe(
    app: AppHandle,
    request: AnalyzeAutoReframeRequest,
) -> Result<AutoReframeAnalysis, RenderErrorReport> {
    tauri::async_runtime::spawn_blocking(move || analyze_blocking(&app, request))
        .await
        .map_err(|error| {
            auto_reframe_error(
                "auto_reframe_analysis_task_failed",
                "Yerel nesne takibi tamamlanamadı.",
                error.to_string(),
                true,
            )
        })?
}

fn analyze_blocking(
    app: &AppHandle,
    request: AnalyzeAutoReframeRequest,
) -> Result<AutoReframeAnalysis, RenderErrorReport> {
    let validated = validate_request(request)?;
    let _analysis_guard = ANALYSIS_LOCK.lock().map_err(|_| {
        auto_reframe_error(
            "auto_reframe_analysis_lock_failed",
            "Yerel takip motoru şu anda kullanılamıyor.",
            "Auto-reframe analysis lock was poisoned",
            true,
        )
    })?;

    let cache_root = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("auto-reframe").join("analysis"))
        .map_err(|error| {
            auto_reframe_error(
                "auto_reframe_cache_directory_failed",
                "Yerel takip önbelleği açılamadı.",
                error.to_string(),
                true,
            )
        })?;
    fs::create_dir_all(&cache_root).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_cache_directory_failed",
            "Yerel takip önbelleği hazırlanamadı.",
            format!("{}: {error}", cache_root.display()),
            true,
        )
    })?;

    let cache_key = analysis_cache_key(&validated);
    let cache_path = cache_root.join(format!("{cache_key}.track.json"));
    if !validated.request.force && cache_path.is_file() {
        match parse_tracker_file(&cache_path, &validated.request, &cache_key) {
            Ok(mut cached) => {
                cached.cache_hit = true;
                emit_progress(
                    app,
                    "analysis",
                    "cache-hit",
                    Some(100.0),
                    "Önceden hesaplanan yerel nesne takibi kullanıldı.",
                );
                return Ok(cached);
            }
            Err(_) => {
                // A corrupt cache is reconstructable and must never block analysis.
                let _ = fs::remove_file(&cache_path);
            }
        }
    }

    let data_root = auto_reframe_data_root(app)?;
    let vit_model = ensure_vit_model(app, &data_root)?;
    let runtime = ensure_runtime_at(app, &data_root, false)?;
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let partial_path = cache_root.join(format!(
        ".{cache_key}.partial-{}-{sequence}.json",
        std::process::id()
    ));
    if partial_path.exists() {
        return Err(auto_reframe_error(
            "auto_reframe_cache_collision",
            "Yerel takip geçici çıktısı hazırlanamadı.",
            partial_path.display().to_string(),
            true,
        ));
    }
    let mut partial_guard = PartialFile::new(partial_path.clone());

    emit_progress(
        app,
        "analysis",
        "starting",
        Some(0.0),
        "Yerel ViT nesne takibi başlatılıyor; gerekirse CSRT güvenli geri dönüşü kullanılacak.",
    );
    run_tracker(
        app,
        &runtime,
        &vit_model,
        &validated,
        &cache_key,
        &partial_path,
    )?;
    let mut response = parse_tracker_file(&partial_path, &validated.request, &cache_key)?;
    response.cache_hit = false;

    if cache_path.is_file() {
        fs::remove_file(&cache_path).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_cache_replace_failed",
                "Yerel takip önbelleği güncellenemedi.",
                format!("{}: {error}", cache_path.display()),
                true,
            )
        })?;
    }
    fs::rename(&partial_path, &cache_path).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_cache_commit_failed",
            "Yerel takip sonucu önbelleğe alınamadı.",
            format!(
                "{} -> {}: {error}",
                partial_path.display(),
                cache_path.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    emit_progress(
        app,
        "analysis",
        "complete",
        Some(100.0),
        "Yerel nesne takibi tamamlandı.",
    );
    Ok(response)
}

fn validate_request(
    request: AnalyzeAutoReframeRequest,
) -> Result<ValidatedRequest, RenderErrorReport> {
    validate_request_values(&request)?;
    let source = fingerprint_source(&request.source_path)?;
    Ok(ValidatedRequest { request, source })
}

fn fingerprint_source(source_path: &str) -> Result<SourceFingerprint, RenderErrorReport> {
    if source_path.len() > 32_767 || source_path.contains('\0') {
        return Err(invalid_request("Video dosyası yolu geçersiz."));
    }
    let requested_path = PathBuf::from(source_path);
    let canonical_path = fs::canonicalize(&requested_path).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_source_missing",
            "Takip edilecek video dosyası bulunamadı.",
            format!("{}: {error}", requested_path.display()),
            false,
        )
    })?;
    let metadata = canonical_path.metadata().map_err(|error| {
        auto_reframe_error(
            "auto_reframe_source_unreadable",
            "Takip edilecek video dosyası okunamadı.",
            format!("{}: {error}", canonical_path.display()),
            false,
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(auto_reframe_error(
            "auto_reframe_source_invalid",
            "Takip kaynağı geçerli ve boş olmayan bir video dosyası olmalıdır.",
            canonical_path.display().to_string(),
            false,
        ));
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    Ok(SourceFingerprint {
        canonical_path,
        bytes: metadata.len(),
        modified_seconds: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
}

fn validate_request_values(request: &AnalyzeAutoReframeRequest) -> Result<(), RenderErrorReport> {
    if request.duration_ms == 0 || request.duration_ms > MAX_ANALYSIS_DURATION_MS {
        return Err(invalid_request(format!(
            "Takip süresi 1 ms ile {} saat arasında olmalıdır.",
            MAX_ANALYSIS_DURATION_MS / 3_600_000
        )));
    }
    let end_ms = request
        .start_ms
        .checked_add(request.duration_ms)
        .ok_or_else(|| invalid_request("Video zaman aralığı sayı sınırını aşıyor."))?;
    if end_ms > MAX_SOURCE_RANGE_MS {
        return Err(invalid_request(
            "Video zaman aralığı desteklenen sınırı aşıyor.",
        ));
    }
    if request.seed_time_ms >= request.duration_ms {
        return Err(invalid_request(
            "Hedef seçim zamanı analiz aralığının içinde olmalıdır.",
        ));
    }
    if !request.sample_fps.is_finite()
        || !(MIN_SAMPLE_FPS..=MAX_SAMPLE_FPS).contains(&request.sample_fps)
    {
        return Err(invalid_request(format!(
            "Takip örnek hızı {MIN_SAMPLE_FPS} ile {MAX_SAMPLE_FPS} FPS arasında olmalıdır."
        )));
    }
    if !(MIN_ANALYSIS_DIMENSION..=MAX_ANALYSIS_DIMENSION).contains(&request.analysis_max_dimension)
    {
        return Err(invalid_request(format!(
            "Analiz boyutu {MIN_ANALYSIS_DIMENSION} ile {MAX_ANALYSIS_DIMENSION} piksel arasında olmalıdır."
        )));
    }
    validate_normalized_box(request.selection, true)
}

fn validate_normalized_box(
    bbox: NormalizedBoundingBox,
    enforce_selection_minimum: bool,
) -> Result<(), RenderErrorReport> {
    let values = [bbox.x, bbox.y, bbox.width, bbox.height];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(invalid_request(
            "Nesne seçim kutusu sonlu sayılardan oluşmalıdır.",
        ));
    }
    let minimum = if enforce_selection_minimum {
        0.005
    } else {
        0.0
    };
    if bbox.x < 0.0
        || bbox.y < 0.0
        || bbox.width <= minimum
        || bbox.height <= minimum
        || bbox.x + bbox.width > 1.0 + 1e-6
        || bbox.y + bbox.height > 1.0 + 1e-6
    {
        return Err(invalid_request(
            "Nesne seçim kutusu görüntü içinde normalize edilmiş geçerli bir alan olmalıdır.",
        ));
    }
    Ok(())
}

fn analysis_cache_key(validated: &ValidatedRequest) -> String {
    analysis_cache_key_with_runner(validated, &tracker_script_sha256())
}

fn analysis_cache_key_with_runner(validated: &ValidatedRequest, runner_sha256: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_SCHEMA.as_bytes());
    hasher.update(runner_sha256.as_bytes());
    hasher.update(validated.source.canonical_path.to_string_lossy().as_bytes());
    hasher.update(validated.source.bytes.to_le_bytes());
    hasher.update(validated.source.modified_seconds.to_le_bytes());
    hasher.update(validated.source.modified_nanos.to_le_bytes());
    hasher.update(validated.request.start_ms.to_le_bytes());
    hasher.update(validated.request.duration_ms.to_le_bytes());
    hasher.update(validated.request.seed_time_ms.to_le_bytes());
    hasher.update(validated.request.sample_fps.to_bits().to_le_bytes());
    hasher.update(validated.request.analysis_max_dimension.to_le_bytes());
    for value in [
        validated.request.selection.x,
        validated.request.selection.y,
        validated.request.selection.width,
        validated.request.selection.height,
    ] {
        hasher.update(value.to_bits().to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn parse_tracker_file(
    path: &Path,
    request: &AnalyzeAutoReframeRequest,
    expected_key: &str,
) -> Result<AutoReframeAnalysis, RenderErrorReport> {
    let metadata = path
        .metadata()
        .map_err(|error| tracker_output_error(format!("{}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TRACKER_JSON_BYTES {
        return Err(tracker_output_error(format!(
            "Unexpected tracker output size: {} bytes",
            metadata.len()
        )));
    }
    let file = File::open(path)
        .map_err(|error| tracker_output_error(format!("{}: {error}", path.display())))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_TRACKER_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| tracker_output_error(error.to_string()))?;
    if bytes.len() as u64 > MAX_TRACKER_JSON_BYTES {
        return Err(tracker_output_error("Tracker JSON exceeded the size limit"));
    }
    parse_tracker_output(&bytes, request, expected_key)
}

fn parse_tracker_output(
    bytes: &[u8],
    request: &AnalyzeAutoReframeRequest,
    expected_key: &str,
) -> Result<AutoReframeAnalysis, RenderErrorReport> {
    let output: CompactTrackerOutput = serde_json::from_slice(bytes)
        .map_err(|error| tracker_output_error(format!("Invalid tracker JSON: {error}")))?;
    if output.schema != TRACKER_SCHEMA
        || output.request_key != expected_key
        || output.engine != ENGINE
        || output.confidence_metric != CONFIDENCE_METRIC
    {
        return Err(tracker_output_error(
            "Tracker schema, request key, engine, or confidence metric mismatch",
        ));
    }
    if output.engine_version.is_empty() || output.engine_version.len() > 64 {
        return Err(tracker_output_error("Invalid OpenCV version string"));
    }
    if output.source_width == 0
        || output.source_height == 0
        || output.source_width > 32_768
        || output.source_height > 32_768
    {
        return Err(tracker_output_error("Invalid source dimensions"));
    }
    if output
        .source_fps
        .is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 1_000.0)
    {
        return Err(tracker_output_error("Invalid source FPS"));
    }
    let request_bound =
        (request.duration_ms as f64 / 1_000.0 * request.sample_fps).ceil() as usize + 4;
    if output.samples.is_empty()
        || output.samples.len() > request_bound
        || output.samples.len() > MAX_TRACK_SAMPLES
    {
        return Err(tracker_output_error(format!(
            "Unexpected tracker sample count: {} (limit {})",
            output.samples.len(),
            request_bound.min(MAX_TRACK_SAMPLES)
        )));
    }
    if output.warnings.len() > 16
        || output
            .warnings
            .iter()
            .any(|warning| warning.is_empty() || warning.len() > 512)
    {
        return Err(tracker_output_error("Invalid tracker warnings"));
    }

    let mut samples = Vec::with_capacity(output.samples.len());
    let mut previous_time = None;
    let mut saw_seed = false;
    for sample in output.samples {
        if sample.t >= request.duration_ms
            || previous_time.is_some_and(|previous| sample.t <= previous)
        {
            return Err(tracker_output_error(
                "Tracker timestamps are outside the request or not strictly increasing",
            ));
        }
        let bbox = NormalizedBoundingBox {
            x: sample.b[0],
            y: sample.b[1],
            width: sample.b[2],
            height: sample.b[3],
        };
        validate_normalized_box(bbox, false)
            .map_err(|error| tracker_output_error(error.technical_message))?;
        if !sample.c.is_finite() || !(0.0..=1.0).contains(&sample.c) {
            return Err(tracker_output_error("Invalid tracker confidence signal"));
        }
        saw_seed |= sample.t == request.seed_time_ms;
        previous_time = Some(sample.t);
        samples.push(AutoReframeTrackSample {
            time_ms: sample.t,
            bbox,
            confidence: sample.c,
        });
    }
    if !saw_seed {
        return Err(tracker_output_error(
            "Tracker output omitted the seed sample",
        ));
    }

    Ok(AutoReframeAnalysis {
        engine: output.engine,
        engine_version: output.engine_version,
        confidence_metric: output.confidence_metric,
        source_width: output.source_width,
        source_height: output.source_height,
        source_fps: output.source_fps,
        start_ms: request.start_ms,
        duration_ms: request.duration_ms,
        seed_time_ms: request.seed_time_ms,
        sample_fps: request.sample_fps,
        analysis_max_dimension: request.analysis_max_dimension,
        selection: request.selection,
        samples,
        warnings: output.warnings,
        cache_hit: false,
    })
}

fn run_tracker(
    app: &AppHandle,
    runtime: &RuntimePaths,
    vit_model: &Path,
    validated: &ValidatedRequest,
    cache_key: &str,
    output_path: &Path,
) -> Result<(), RenderErrorReport> {
    let request = &validated.request;
    let mut command = Command::new(&runtime.python);
    command
        .arg("-I")
        .arg(&runtime.runner)
        .arg("--model")
        .arg(vit_model)
        .arg("--source")
        .arg(&validated.source.canonical_path)
        .arg("--start-ms")
        .arg(request.start_ms.to_string())
        .arg("--duration-ms")
        .arg(request.duration_ms.to_string())
        .arg("--seed-time-ms")
        .arg(request.seed_time_ms.to_string())
        .arg("--box")
        .arg(request.selection.x.to_string())
        .arg(request.selection.y.to_string())
        .arg(request.selection.width.to_string())
        .arg(request.selection.height.to_string())
        .arg("--sample-fps")
        .arg(request.sample_fps.to_string())
        .arg("--max-dimension")
        .arg(request.analysis_max_dimension.to_string())
        .arg("--request-key")
        .arg(cache_key)
        .arg("--output")
        .arg(output_path)
        .current_dir(&runtime.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_python_command(&mut command, &runtime.root);
    configure_background_process(&mut command);

    let mut child = command.spawn().map_err(|error| {
        auto_reframe_error(
            "auto_reframe_tracker_start_failed",
            "Yerel takip motoru başlatılamadı.",
            error.to_string(),
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        auto_reframe_error(
            "auto_reframe_tracker_pipe_failed",
            "Yerel takip motorunun çıktısı okunamadı.",
            "Tracker stderr pipe was unavailable",
            true,
        )
    })?;
    let mut stderr_tail = VecDeque::with_capacity(STDERR_TAIL_LINES);
    for line in BufReader::new(stderr).lines() {
        let line = line.map_err(|error| {
            auto_reframe_error(
                "auto_reframe_tracker_pipe_failed",
                "Yerel takip motorunun çıktısı okunamadı.",
                error.to_string(),
                true,
            )
        })?;
        if let Some(raw) = line.strip_prefix(PROGRESS_PREFIX) {
            if raw.len() <= 4_096 {
                if let Ok(progress) = serde_json::from_str::<PythonProgress>(raw) {
                    if progress.progress_percent.is_finite()
                        && progress.stage.len() <= 64
                        && progress.message.len() <= 512
                    {
                        emit_progress(
                            app,
                            "analysis",
                            &progress.stage,
                            Some(progress.progress_percent.clamp(0.0, 100.0)),
                            &progress.message,
                        );
                        continue;
                    }
                }
            }
        }
        if stderr_tail.len() == STDERR_TAIL_LINES {
            stderr_tail.pop_front();
        }
        stderr_tail.push_back(line);
    }
    let status = child.wait().map_err(|error| {
        auto_reframe_error(
            "auto_reframe_tracker_wait_failed",
            "Yerel takip motoru tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !status.success() {
        let mut report = auto_reframe_error(
            "auto_reframe_tracking_failed",
            "Seçilen nesne bu videoda yerel ViT/CSRT motoruyla takip edilemedi.",
            format!("Tracker exited with status {status}"),
            true,
        );
        report.stderr_tail = stderr_tail.into_iter().collect();
        return Err(report);
    }
    if !output_path.is_file() {
        return Err(tracker_output_error(format!(
            "Tracker did not create {}",
            output_path.display()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectFacecamRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub interval_ms: Option<u64>,
    #[serde(default = "default_analysis_max_dimension")]
    pub analysis_max_dimension: u32,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacecamDetection {
    pub engine: String,
    pub engine_version: String,
    pub confidence_metric: String,
    pub source_width: u32,
    pub source_height: u32,
    pub source_fps: Option<f64>,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub interval_ms: u64,
    pub analysis_max_dimension: u32,
    pub samples: Vec<AutoReframeTrackSample>,
    pub warnings: Vec<String>,
    pub cache_hit: bool,
}

#[tauri::command]
pub async fn detect_facecam(
    app: AppHandle,
    request: DetectFacecamRequest,
) -> Result<FacecamDetection, RenderErrorReport> {
    tauri::async_runtime::spawn_blocking(move || detect_facecam_blocking(&app, request))
        .await
        .map_err(|error| {
            auto_reframe_error(
                "facecam_detect_task_failed",
                "Yayıncı kamerası algılama tamamlanamadı.",
                error.to_string(),
                true,
            )
        })?
}

fn facecam_cancelled_error() -> RenderErrorReport {
    auto_reframe_error(
        "smart_shorts_cancelled",
        "Yayıncı kamerası algılama iptal edildi.",
        "Facecam detection was cancelled",
        false,
    )
}

fn detect_facecam_blocking(
    app: &AppHandle,
    request: DetectFacecamRequest,
) -> Result<FacecamDetection, RenderErrorReport> {
    if analysis_cancel::is_cancelled(request.operation_id.as_deref()) {
        return Err(facecam_cancelled_error());
    }
    if request.duration_ms == 0 || request.duration_ms > MAX_ANALYSIS_DURATION_MS {
        return Err(invalid_request(format!(
            "Algılama süresi 1 ms ile {} saat arasında olmalıdır.",
            MAX_ANALYSIS_DURATION_MS / 3_600_000
        )));
    }
    let end_ms = request
        .start_ms
        .checked_add(request.duration_ms)
        .ok_or_else(|| invalid_request("Video zaman aralığı sayı sınırını aşıyor."))?;
    if end_ms > MAX_SOURCE_RANGE_MS {
        return Err(invalid_request(
            "Video zaman aralığı desteklenen sınırı aşıyor.",
        ));
    }
    if !(MIN_ANALYSIS_DIMENSION..=MAX_ANALYSIS_DIMENSION).contains(&request.analysis_max_dimension)
    {
        return Err(invalid_request(format!(
            "Analiz boyutu {MIN_ANALYSIS_DIMENSION} ile {MAX_ANALYSIS_DIMENSION} piksel arasında olmalıdır."
        )));
    }
    // Long clips scan more sparsely so a 40-minute video stays around 600 seeks.
    let interval_ms = request
        .interval_ms
        .unwrap_or_else(|| (request.duration_ms / 600).max(1_000))
        .clamp(MIN_FACECAM_INTERVAL_MS, MAX_FACECAM_INTERVAL_MS);
    let source = fingerprint_source(&request.source_path)?;

    let _analysis_guard = ANALYSIS_LOCK.lock().map_err(|_| {
        auto_reframe_error(
            "auto_reframe_analysis_lock_failed",
            "Yerel takip motoru şu anda kullanılamıyor.",
            "Auto-reframe analysis lock was poisoned",
            true,
        )
    })?;

    let cache_root = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("auto-reframe").join("facecam"))
        .map_err(|error| {
            auto_reframe_error(
                "auto_reframe_cache_directory_failed",
                "Yerel algılama önbelleği açılamadı.",
                error.to_string(),
                true,
            )
        })?;
    fs::create_dir_all(&cache_root).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_cache_directory_failed",
            "Yerel algılama önbelleği hazırlanamadı.",
            format!("{}: {error}", cache_root.display()),
            true,
        )
    })?;
    let cache_key = facecam_cache_key(&source, &request, interval_ms);
    let cache_path = cache_root.join(format!("{cache_key}.detect.json"));
    if !request.force && cache_path.is_file() {
        match parse_facecam_file(&cache_path, &request, interval_ms, &cache_key) {
            Ok(mut cached) => {
                cached.cache_hit = true;
                emit_progress(
                    app,
                    "analysis",
                    "cache-hit",
                    Some(100.0),
                    "Önceden hesaplanan yayıncı kamerası konumu kullanıldı.",
                );
                return Ok(cached);
            }
            Err(_) => {
                let _ = fs::remove_file(&cache_path);
            }
        }
    }

    let data_root = auto_reframe_data_root(app)?;
    let runtime = ensure_runtime_at(app, &data_root, false)?;
    let detector = ensure_facecam_script(&runtime)?;
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let partial_path = cache_root.join(format!(
        ".{cache_key}.partial-{}-{sequence}.json",
        std::process::id()
    ));
    if partial_path.exists() {
        return Err(auto_reframe_error(
            "auto_reframe_cache_collision",
            "Yerel algılama geçici çıktısı hazırlanamadı.",
            partial_path.display().to_string(),
            true,
        ));
    }
    let mut partial_guard = PartialFile::new(partial_path.clone());

    emit_progress(
        app,
        "analysis",
        "starting",
        Some(0.0),
        "Yayıncı kamerası yerel yüz algılamayla aranıyor…",
    );
    run_facecam_detector(
        app,
        &runtime,
        &detector,
        &source.canonical_path,
        &request,
        interval_ms,
        &cache_key,
        &partial_path,
    )?;
    let mut response = parse_facecam_file(&partial_path, &request, interval_ms, &cache_key)?;
    response.cache_hit = false;

    if cache_path.is_file() {
        fs::remove_file(&cache_path).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_cache_replace_failed",
                "Yerel algılama önbelleği güncellenemedi.",
                format!("{}: {error}", cache_path.display()),
                true,
            )
        })?;
    }
    fs::rename(&partial_path, &cache_path).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_cache_commit_failed",
            "Yayıncı kamerası sonucu önbelleğe alınamadı.",
            format!(
                "{} -> {}: {error}",
                partial_path.display(),
                cache_path.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    emit_progress(
        app,
        "analysis",
        "complete",
        Some(100.0),
        "Yayıncı kamerası algılama tamamlandı.",
    );
    Ok(response)
}

fn facecam_cache_key(
    source: &SourceFingerprint,
    request: &DetectFacecamRequest,
    interval_ms: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FACECAM_CACHE_SCHEMA.as_bytes());
    hasher.update(facecam_script_sha256().as_bytes());
    hasher.update(source.canonical_path.to_string_lossy().as_bytes());
    hasher.update(source.bytes.to_le_bytes());
    hasher.update(source.modified_seconds.to_le_bytes());
    hasher.update(source.modified_nanos.to_le_bytes());
    hasher.update(request.start_ms.to_le_bytes());
    hasher.update(request.duration_ms.to_le_bytes());
    hasher.update(interval_ms.to_le_bytes());
    hasher.update(request.analysis_max_dimension.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

fn facecam_script_sha256() -> String {
    format!("{:x}", Sha256::digest(FACECAM_SCRIPT.as_bytes()))
}

/// Writes the detector runner next to the tracker runner. The runtime manifest
/// only tracks the tracker script, so the detector is re-verified by content.
fn ensure_facecam_script(runtime: &RuntimePaths) -> Result<PathBuf, RenderErrorReport> {
    let path = runtime.root.join(FACECAM_RUNNER_FILE_NAME);
    let current = fs::read(&path).ok();
    if current.as_deref() != Some(FACECAM_SCRIPT.as_bytes()) {
        fs::write(&path, FACECAM_SCRIPT.as_bytes()).map_err(|error| {
            auto_reframe_error(
                "facecam_detect_script_failed",
                "Yerel yüz algılama betiği hazırlanamadı.",
                format!("{}: {error}", path.display()),
                true,
            )
        })?;
    }
    Ok(path)
}

#[allow(clippy::too_many_arguments)]
fn run_facecam_detector(
    app: &AppHandle,
    runtime: &RuntimePaths,
    detector: &Path,
    source: &Path,
    request: &DetectFacecamRequest,
    interval_ms: u64,
    cache_key: &str,
    output_path: &Path,
) -> Result<(), RenderErrorReport> {
    let mut command = Command::new(&runtime.python);
    command
        .arg("-I")
        .arg(detector)
        .arg("--source")
        .arg(source)
        .arg("--start-ms")
        .arg(request.start_ms.to_string())
        .arg("--duration-ms")
        .arg(request.duration_ms.to_string())
        .arg("--interval-ms")
        .arg(interval_ms.to_string())
        .arg("--max-dimension")
        .arg(request.analysis_max_dimension.to_string())
        .arg("--request-key")
        .arg(cache_key)
        .arg("--output")
        .arg(output_path)
        .current_dir(&runtime.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_python_command(&mut command, &runtime.root);
    configure_background_process(&mut command);

    let mut child = command.spawn().map_err(|error| {
        auto_reframe_error(
            "facecam_detect_start_failed",
            "Yerel yüz algılama motoru başlatılamadı.",
            error.to_string(),
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        auto_reframe_error(
            "facecam_detect_pipe_failed",
            "Yerel yüz algılama çıktısı okunamadı.",
            "Detector stderr pipe was unavailable",
            true,
        )
    })?;
    // Reading through a channel keeps cancellation responsive: the loop wakes on
    // a timeout even when the detector emits no output for a while.
    let (line_sender, line_receiver) = mpsc::channel::<io::Result<String>>();
    let reader_thread = thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });

    let operation_id = request.operation_id.as_deref();
    let mut stderr_tail: VecDeque<String> = VecDeque::with_capacity(STDERR_TAIL_LINES);
    let mut cancelled = false;
    loop {
        if !cancelled && analysis_cancel::is_cancelled(operation_id) {
            cancelled = true;
            let _ = child.kill();
        }
        match line_receiver.recv_timeout(Duration::from_millis(150)) {
            Ok(Ok(line)) => {
                if let Some(raw) = line.strip_prefix(PROGRESS_PREFIX) {
                    if !cancelled && raw.len() <= 4_096 {
                        if let Ok(progress) = serde_json::from_str::<PythonProgress>(raw) {
                            if progress.progress_percent.is_finite()
                                && progress.stage.len() <= 64
                                && progress.message.len() <= 512
                            {
                                emit_progress(
                                    app,
                                    "analysis",
                                    &progress.stage,
                                    Some(progress.progress_percent.clamp(0.0, 100.0)),
                                    &progress.message,
                                );
                            }
                        }
                    }
                    continue;
                }
                if stderr_tail.len() == STDERR_TAIL_LINES {
                    stderr_tail.pop_front();
                }
                stderr_tail.push_back(line);
            }
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    let _ = reader_thread.join();
    let status = child.wait().map_err(|error| {
        auto_reframe_error(
            "facecam_detect_wait_failed",
            "Yerel yüz algılama motoru tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if cancelled {
        return Err(facecam_cancelled_error());
    }
    if !status.success() {
        if stderr_tail
            .iter()
            .any(|line| line.contains("FACECAM_NOT_FOUND"))
        {
            return Err(auto_reframe_error(
                "facecam_not_found",
                "Videoda sabit bir yayıncı kamerası bulunamadı. Facecam görünmüyorsa hedefi elle de seçebilirsiniz.",
                "Facecam detector found no stable face cluster",
                false,
            ));
        }
        let mut report = auto_reframe_error(
            "facecam_detect_failed",
            "Yayıncı kamerası bu videoda yerel yüz algılamayla bulunamadı.",
            format!("Facecam detector exited with status {status}"),
            true,
        );
        report.stderr_tail = stderr_tail.into_iter().collect();
        return Err(report);
    }
    if !output_path.is_file() {
        return Err(tracker_output_error(format!(
            "Facecam detector did not create {}",
            output_path.display()
        )));
    }
    Ok(())
}

fn parse_facecam_file(
    path: &Path,
    request: &DetectFacecamRequest,
    interval_ms: u64,
    expected_key: &str,
) -> Result<FacecamDetection, RenderErrorReport> {
    let metadata = path
        .metadata()
        .map_err(|error| tracker_output_error(format!("{}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TRACKER_JSON_BYTES {
        return Err(tracker_output_error(format!(
            "Unexpected facecam output size: {} bytes",
            metadata.len()
        )));
    }
    let file = File::open(path)
        .map_err(|error| tracker_output_error(format!("{}: {error}", path.display())))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_TRACKER_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| tracker_output_error(error.to_string()))?;
    if bytes.len() as u64 > MAX_TRACKER_JSON_BYTES {
        return Err(tracker_output_error("Facecam JSON exceeded the size limit"));
    }
    parse_facecam_output(&bytes, request, interval_ms, expected_key)
}

fn parse_facecam_output(
    bytes: &[u8],
    request: &DetectFacecamRequest,
    interval_ms: u64,
    expected_key: &str,
) -> Result<FacecamDetection, RenderErrorReport> {
    let output: CompactTrackerOutput = serde_json::from_slice(bytes)
        .map_err(|error| tracker_output_error(format!("Invalid facecam JSON: {error}")))?;
    if output.schema != FACECAM_SCHEMA
        || output.request_key != expected_key
        || output.engine != FACECAM_ENGINE
        || output.confidence_metric != FACECAM_CONFIDENCE_METRIC
    {
        return Err(tracker_output_error(
            "Facecam schema, request key, engine, or confidence metric mismatch",
        ));
    }
    if output.engine_version.is_empty() || output.engine_version.len() > 64 {
        return Err(tracker_output_error("Invalid OpenCV version string"));
    }
    if output.source_width == 0
        || output.source_height == 0
        || output.source_width > 32_768
        || output.source_height > 32_768
    {
        return Err(tracker_output_error("Invalid source dimensions"));
    }
    if output
        .source_fps
        .is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 1_000.0)
    {
        return Err(tracker_output_error("Invalid source FPS"));
    }
    let request_bound = (request.duration_ms / interval_ms.max(1)) as usize + 4;
    if output.samples.is_empty()
        || output.samples.len() > request_bound
        || output.samples.len() > MAX_TRACK_SAMPLES
    {
        return Err(tracker_output_error(format!(
            "Unexpected facecam sample count: {} (limit {})",
            output.samples.len(),
            request_bound.min(MAX_TRACK_SAMPLES)
        )));
    }
    if output.warnings.len() > 16
        || output
            .warnings
            .iter()
            .any(|warning| warning.is_empty() || warning.len() > 512)
    {
        return Err(tracker_output_error("Invalid facecam warnings"));
    }

    let mut samples = Vec::with_capacity(output.samples.len());
    let mut previous_time = None;
    for sample in output.samples {
        if sample.t >= request.duration_ms
            || previous_time.is_some_and(|previous| sample.t <= previous)
        {
            return Err(tracker_output_error(
                "Facecam timestamps are outside the request or not strictly increasing",
            ));
        }
        let bbox = NormalizedBoundingBox {
            x: sample.b[0],
            y: sample.b[1],
            width: sample.b[2],
            height: sample.b[3],
        };
        validate_normalized_box(bbox, false)
            .map_err(|error| tracker_output_error(error.technical_message))?;
        if !sample.c.is_finite() || !(0.0..=1.0).contains(&sample.c) {
            return Err(tracker_output_error("Invalid facecam confidence signal"));
        }
        previous_time = Some(sample.t);
        samples.push(AutoReframeTrackSample {
            time_ms: sample.t,
            bbox,
            confidence: sample.c,
        });
    }

    Ok(FacecamDetection {
        engine: output.engine,
        engine_version: output.engine_version,
        confidence_metric: output.confidence_metric,
        source_width: output.source_width,
        source_height: output.source_height,
        source_fps: output.source_fps,
        start_ms: request.start_ms,
        duration_ms: request.duration_ms,
        interval_ms,
        analysis_max_dimension: request.analysis_max_dimension,
        samples,
        warnings: output.warnings,
        cache_hit: false,
    })
}

fn auto_reframe_data_root(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("auto-reframe"))
        .map_err(|error| {
            auto_reframe_error(
                "auto_reframe_data_directory_failed",
                "Yerel takip motoru veri klasörü açılamadı.",
                error.to_string(),
                true,
            )
        })
}

fn runtime_status(app: &AppHandle, data_root: &Path) -> AutoReframeRuntimeStatus {
    let runtime_dir = data_root.join("runtime").join(RUNTIME_DIRECTORY_NAME);
    if vit_model_is_valid(data_root).unwrap_or(false) {
        if let Some(runtime) = valid_runtime(&runtime_dir) {
            return status_from_runtime(app, &runtime);
        }
    }
    let uv_available = resolve_uv(app, data_root).is_some();
    let diagnostic = if uv_available {
        Some(auto_reframe_error(
            "auto_reframe_runtime_setup_required",
            "Ücretsiz yerel takip motoru ilk kullanımda kurulmalıdır.",
            "Pinned OpenCV ViT tracker model or virtual environment is not ready",
            true,
        ))
    } else {
        Some(auto_reframe_error(
            "auto_reframe_uv_missing",
            "Ücretsiz yerel takip motorunu kurmak için uv bulunamadı.",
            "Install uv or complete the existing Studio AI runtime setup, then retry",
            true,
        ))
    };
    AutoReframeRuntimeStatus {
        ready: false,
        engine: ENGINE.into(),
        runtime_dir: runtime_dir.to_string_lossy().into_owned(),
        python_path: None,
        python_version: None,
        opencv_version: None,
        numpy_version: None,
        packages: runtime_packages(),
        uv_available,
        diagnostic,
    }
}

fn status_from_runtime(app: &AppHandle, runtime: &RuntimePaths) -> AutoReframeRuntimeStatus {
    AutoReframeRuntimeStatus {
        ready: true,
        engine: ENGINE.into(),
        runtime_dir: runtime.root.to_string_lossy().into_owned(),
        python_path: Some(runtime.python.to_string_lossy().into_owned()),
        python_version: Some(runtime.manifest.python_version.clone()),
        opencv_version: Some(runtime.manifest.opencv_version.clone()),
        numpy_version: Some(runtime.manifest.numpy_version.clone()),
        packages: runtime_packages(),
        uv_available: resolve_uv(app, runtime.root.parent().unwrap_or(&runtime.root)).is_some(),
        diagnostic: None,
    }
}

fn runtime_packages() -> Vec<AutoReframeRuntimePackage> {
    vec![
        AutoReframeRuntimePackage {
            name: "opencv-contrib-python-headless".into(),
            version: OPENCV_PACKAGE_VERSION.into(),
            declared_license: OPENCV_LICENSE.into(),
            free_and_open_source: true,
        },
        AutoReframeRuntimePackage {
            name: "numpy".into(),
            version: NUMPY_VERSION.into(),
            declared_license: NUMPY_LICENSE.into(),
            free_and_open_source: true,
        },
        AutoReframeRuntimePackage {
            name: "opencv-zoo-vittrack".into(),
            version: VIT_MODEL_VERSION.into(),
            declared_license: VIT_MODEL_LICENSE.into(),
            free_and_open_source: true,
        },
    ]
}

fn vit_model_path(data_root: &Path) -> PathBuf {
    data_root.join("models").join(VIT_MODEL_NAME)
}

fn vit_model_is_valid(data_root: &Path) -> Result<bool, RenderErrorReport> {
    let path = vit_model_path(data_root);
    let metadata = match path.metadata() {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_VIT_MODEL_BYTES => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(auto_reframe_error(
                "auto_reframe_model_read_failed",
                "ViT takip modeli doğrulanamadı.",
                format!("{}: {error}", path.display()),
                true,
            ))
        }
    };
    if metadata.len() == 0 {
        return Ok(false);
    }
    let mut file = File::open(&path).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_model_read_failed",
            "ViT takip modeli doğrulanamadı.",
            format!("{}: {error}", path.display()),
            true,
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_model_read_failed",
                "ViT takip modeli doğrulanamadı.",
                error.to_string(),
                true,
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()) == VIT_MODEL_SHA256)
}

fn ensure_vit_model(app: &AppHandle, data_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let _model_guard = MODEL_LOCK.lock().map_err(|_| {
        auto_reframe_error(
            "auto_reframe_model_lock_failed",
            "ViT takip modeli şu anda hazırlanamadı.",
            "Auto-reframe model lock was poisoned",
            true,
        )
    })?;
    let model_dir = data_root.join("models");
    fs::create_dir_all(&model_dir).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_model_directory_failed",
            "ViT takip modeli klasörü hazırlanamadı.",
            format!("{}: {error}", model_dir.display()),
            true,
        )
    })?;
    let destination = vit_model_path(data_root);
    if vit_model_is_valid(data_root)? {
        return Ok(destination);
    }
    if destination.is_file() {
        fs::remove_file(&destination).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_model_replace_failed",
                "ViT takip modelinin eski kopyası yenilenemedi.",
                format!("{}: {error}", destination.display()),
                true,
            )
        })?;
    }
    let partial_path = model_dir.join(format!(".{VIT_MODEL_NAME}.partial"));
    if partial_path.is_file() {
        fs::remove_file(&partial_path).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_model_replace_failed",
                "ViT takip modelinin yarım indirmesi temizlenemedi.",
                format!("{}: {error}", partial_path.display()),
                true,
            )
        })?;
    }
    let mut partial_guard = PartialFile::new(partial_path.clone());
    emit_progress(
        app,
        "setup",
        "downloading-vit-model",
        Some(1.0),
        "Apache-2.0 lisanslı OpenCV ViT takip modeli indiriliyor.",
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(45)))
        .build()
        .into();
    let mut response = agent.get(VIT_MODEL_URL).call().map_err(|error| {
        auto_reframe_error(
            "auto_reframe_model_download_failed",
            "ViT takip modeli indirilemedi; internet bağlantısını kontrol edin.",
            format!("{VIT_MODEL_URL}: {error}"),
            true,
        )
    })?;
    let total_bytes = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if total_bytes > MAX_VIT_MODEL_BYTES {
        return Err(auto_reframe_error(
            "auto_reframe_model_too_large",
            "ViT takip modelinin boyut doğrulaması başarısız oldu.",
            format!("Content-Length {total_bytes} exceeded {MAX_VIT_MODEL_BYTES}"),
            false,
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
        .map_err(|error| {
            auto_reframe_error(
                "auto_reframe_model_write_failed",
                "ViT takip modeli diske yazılamadı.",
                format!("{}: {error}", partial_path.display()),
                true,
            )
        })?;
    let mut writer = BufWriter::new(file);
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded_bytes = 0_u64;
    let mut last_tick = u64::MAX;
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_model_download_failed",
                "ViT takip modeli indirilemedi.",
                error.to_string(),
                true,
            )
        })?;
        if read == 0 {
            break;
        }
        downloaded_bytes = downloaded_bytes.saturating_add(read as u64);
        if downloaded_bytes > MAX_VIT_MODEL_BYTES {
            return Err(auto_reframe_error(
                "auto_reframe_model_too_large",
                "ViT takip modelinin boyut doğrulaması başarısız oldu.",
                format!("Download exceeded {MAX_VIT_MODEL_BYTES} bytes"),
                false,
            ));
        }
        writer.write_all(&buffer[..read]).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_model_write_failed",
                "ViT takip modeli diske yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        hasher.update(&buffer[..read]);
        let tick = if total_bytes > 0 {
            downloaded_bytes.saturating_mul(100) / total_bytes
        } else {
            downloaded_bytes / (64 * 1024)
        };
        if tick != last_tick {
            last_tick = tick;
            let percent = if total_bytes > 0 {
                1.0 + (downloaded_bytes as f64 / total_bytes as f64 * 3.0).clamp(0.0, 3.0)
            } else {
                2.0
            };
            emit_progress(
                app,
                "setup",
                "downloading-vit-model",
                Some(percent),
                "OpenCV ViT takip modeli indiriliyor.",
            );
        }
    }
    writer.flush().map_err(|error| {
        auto_reframe_error(
            "auto_reframe_model_write_failed",
            "ViT takip modeli diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    writer.get_ref().sync_all().map_err(|error| {
        auto_reframe_error(
            "auto_reframe_model_write_failed",
            "ViT takip modeli diske yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    drop(writer);
    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != VIT_MODEL_SHA256 {
        return Err(auto_reframe_error(
            "auto_reframe_model_checksum_failed",
            "ViT takip modelinin güvenlik doğrulaması başarısız oldu.",
            format!("Expected {VIT_MODEL_SHA256}, got {actual_sha256}"),
            false,
        ));
    }
    fs::rename(&partial_path, &destination).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_model_commit_failed",
            "ViT takip modeli etkinleştirilemedi.",
            format!(
                "{} -> {}: {error}",
                partial_path.display(),
                destination.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    emit_progress(
        app,
        "setup",
        "vit-model-ready",
        Some(4.0),
        "OpenCV ViT takip modeli doğrulandı.",
    );
    Ok(destination)
}

fn ensure_runtime_at(
    app: &AppHandle,
    data_root: &Path,
    force: bool,
) -> Result<RuntimePaths, RenderErrorReport> {
    let _runtime_guard = RUNTIME_LOCK.lock().map_err(|_| {
        auto_reframe_error(
            "auto_reframe_runtime_lock_failed",
            "Yerel takip motoru kurulumu şu anda kullanılamıyor.",
            "Auto-reframe runtime lock was poisoned",
            true,
        )
    })?;
    let runtime_parent = data_root.join("runtime");
    let runtime_dir = runtime_parent.join(RUNTIME_DIRECTORY_NAME);
    if !force {
        if let Some(runtime) = valid_runtime(&runtime_dir) {
            return Ok(runtime);
        }
        if let Some(runtime) = try_upgrade_runtime_in_place(data_root, &runtime_dir) {
            emit_progress(
                app,
                "setup",
                "upgraded-runner",
                Some(100.0),
                "Mevcut OpenCV ortamı ViT takip motoruna yükseltildi.",
            );
            return Ok(runtime);
        }
    }

    fs::create_dir_all(data_root.join("temp")).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runtime_directory_failed",
            "Yerel takip motoru geçici klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    fs::create_dir_all(&runtime_parent).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runtime_directory_failed",
            "Yerel takip motoru çalışma klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let uv = if let Some(uv) = resolve_uv(app, data_root) {
        uv
    } else {
        emit_progress(
            app,
            "setup",
            "downloading-installer",
            Some(2.0),
            "Doğrulanmış ücretsiz kurulum aracı indiriliyor.",
        );
        let quality_root = app
            .path()
            .app_cache_dir()
            .map(|path| path.join("clearvoice-experimental"))
            .map_err(|error| {
                auto_reframe_error(
                    "auto_reframe_uv_directory_failed",
                    "Ücretsiz yerel takip motorunun kurulum klasörü açılamadı.",
                    error.to_string(),
                    true,
                )
            })?;
        crate::clearvoice_experimental::ensure_uv(&quality_root).map_err(|error| {
            let mut report = auto_reframe_error(
                "auto_reframe_uv_download_failed",
                "Ücretsiz yerel takip motorunun doğrulanmış kurulum aracı indirilemedi.",
                error.technical_message,
                true,
            );
            report.stderr_tail = error.stderr_tail;
            report
        })?
    };

    emit_progress(
        app,
        "setup",
        "creating-environment",
        Some(5.0),
        "İzole Python ortamı hazırlanıyor.",
    );
    let mut staging = ScopedDirectory::create(&runtime_parent, "opencv-csrt-runtime")?;
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
    run_checked_command(
        &mut venv,
        "auto_reframe_python_setup_failed",
        "Yerel takip motoru için ücretsiz Python ortamı kurulamadı.",
    )?;
    let python = venv_python(&staging.path);
    if !python.is_file() {
        return Err(auto_reframe_error(
            "auto_reframe_python_missing",
            "Yerel takip motorunun Python ortamı eksik kaldı.",
            format!("Expected {}", python.display()),
            true,
        ));
    }

    emit_progress(
        app,
        "setup",
        "installing-dependencies",
        Some(35.0),
        "Ücretsiz OpenCV ViT/CSRT ve NumPy paketleri kuruluyor.",
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
        .arg("https://pypi.org/simple")
        .arg(NUMPY_REQUIREMENT)
        .arg(OPENCV_REQUIREMENT);
    configure_uv_command(&mut install, data_root);
    run_checked_command(
        &mut install,
        "auto_reframe_dependencies_failed",
        "Ücretsiz OpenCV takip paketleri kurulamadı. İnternet bağlantısını kontrol edin.",
    )?;

    emit_progress(
        app,
        "setup",
        "verifying",
        Some(85.0),
        "Yerel takip motoru doğrulanıyor.",
    );
    let (python_version, opencv_version, numpy_version) =
        verify_python_runtime(&python, data_root)?;
    let runner = staging.path.join(RUNNER_FILE_NAME);
    fs::write(&runner, TRACKER_SCRIPT.as_bytes()).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runner_write_failed",
            "Yerel takip çalıştırıcısı hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let manifest = RuntimeManifest {
        schema: RUNTIME_SCHEMA.into(),
        runner_sha256: tracker_script_sha256(),
        python_version,
        opencv_version,
        numpy_version,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runtime_manifest_failed",
            "Yerel takip motoru doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    fs::write(staging.path.join(RUNTIME_MANIFEST_NAME), manifest_bytes).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runtime_manifest_failed",
            "Yerel takip motoru doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;

    if runtime_dir.exists() {
        remove_scoped_runtime_directory(&runtime_parent, &runtime_dir)?;
    }
    fs::rename(&staging.path, &runtime_dir).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runtime_commit_failed",
            "Yerel takip motoru etkinleştirilemedi.",
            format!(
                "{} -> {}: {error}",
                staging.path.display(),
                runtime_dir.display()
            ),
            true,
        )
    })?;
    staging.commit();
    let runtime = valid_runtime(&runtime_dir).ok_or_else(|| {
        auto_reframe_error(
            "auto_reframe_runtime_verification_failed",
            "Yerel takip motoru kuruldu ancak doğrulanamadı.",
            runtime_dir.display().to_string(),
            true,
        )
    })?;
    emit_progress(
        app,
        "setup",
        "ready",
        Some(100.0),
        "Ücretsiz yerel OpenCV ViT takip motoru hazır.",
    );
    Ok(runtime)
}

fn valid_runtime(runtime_dir: &Path) -> Option<RuntimePaths> {
    let python = venv_python(runtime_dir);
    let runner = runtime_dir.join(RUNNER_FILE_NAME);
    if !python.is_file() || !runner.is_file() {
        return None;
    }
    if fs::read(&runner).ok()?.as_slice() != TRACKER_SCRIPT.as_bytes() {
        return None;
    }
    let manifest: RuntimeManifest =
        serde_json::from_slice(&fs::read(runtime_dir.join(RUNTIME_MANIFEST_NAME)).ok()?).ok()?;
    if manifest.schema != RUNTIME_SCHEMA
        || manifest.runner_sha256 != tracker_script_sha256()
        || manifest.opencv_version != OPENCV_VERSION
        || manifest.numpy_version != NUMPY_VERSION
    {
        return None;
    }
    Some(RuntimePaths {
        root: runtime_dir.to_path_buf(),
        python,
        runner,
        manifest,
    })
}

fn try_upgrade_runtime_in_place(data_root: &Path, runtime_dir: &Path) -> Option<RuntimePaths> {
    let python = venv_python(runtime_dir);
    if !python.is_file() {
        return None;
    }
    let (python_version, opencv_version, numpy_version) =
        verify_python_runtime(&python, data_root).ok()?;
    let runner = runtime_dir.join(RUNNER_FILE_NAME);
    fs::write(&runner, TRACKER_SCRIPT.as_bytes()).ok()?;
    let manifest = RuntimeManifest {
        schema: RUNTIME_SCHEMA.into(),
        runner_sha256: tracker_script_sha256(),
        python_version,
        opencv_version,
        numpy_version,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).ok()?;
    fs::write(runtime_dir.join(RUNTIME_MANIFEST_NAME), manifest_bytes).ok()?;
    valid_runtime(runtime_dir)
}

fn tracker_script_sha256() -> String {
    format!("{:x}", Sha256::digest(TRACKER_SCRIPT.as_bytes()))
}

fn verify_python_runtime(
    python: &Path,
    data_root: &Path,
) -> Result<(String, String, String), RenderErrorReport> {
    let script = format!(
        "import json,platform,cv2,numpy; assert cv2.__version__ == '{OPENCV_VERSION}'; assert numpy.__version__ == '{NUMPY_VERSION}'; csrt=getattr(cv2,'TrackerCSRT_create',None) or getattr(getattr(cv2,'legacy',None),'TrackerCSRT_create',None); vit=getattr(cv2,'TrackerVit_create',None); vit_params=getattr(cv2,'TrackerVit_Params',None); assert callable(csrt); assert callable(vit); assert callable(vit_params); vit_params(); tracker=csrt(); frame=numpy.zeros((64,64,3),dtype=numpy.uint8); initialized=tracker.init(frame,(8,8,24,24)); assert initialized is not False; print(json.dumps([platform.python_version(),cv2.__version__,numpy.__version__]))"
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
        auto_reframe_error(
            "auto_reframe_runtime_verification_failed",
            "Yerel takip motoru doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !output.status.success() {
        let mut report = auto_reframe_error(
            "auto_reframe_runtime_verification_failed",
            "Yerel takip motoru doğrulanamadı.",
            format!("Python verification exited with {}", output.status),
            true,
        );
        report.stderr_tail = stderr_tail(&output.stderr);
        return Err(report);
    }
    if output.stdout.len() > 4_096 {
        return Err(auto_reframe_error(
            "auto_reframe_runtime_verification_failed",
            "Yerel takip motoru doğrulanamadı.",
            "Python verification output exceeded 4096 bytes",
            false,
        ));
    }
    let versions: [String; 3] =
        serde_json::from_slice(trim_ascii(&output.stdout)).map_err(|error| {
            auto_reframe_error(
                "auto_reframe_runtime_verification_failed",
                "Yerel takip motoru sürümleri doğrulanamadı.",
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

fn resolve_uv(app: &AppHandle, data_root: &Path) -> Option<PathBuf> {
    if let Some(path) = find_on_path(if cfg!(windows) { "uv.exe" } else { "uv" }) {
        return Some(path);
    }
    let cache_dir = app.path().app_cache_dir().ok()?;
    let tools_dir = cache_dir.join("clearvoice-experimental").join("tools");
    let entries = fs::read_dir(tools_dir).ok()?;
    for entry in entries.flatten().take(32) {
        let candidate = entry
            .path()
            .join(if cfg!(windows) { "uv.exe" } else { "uv" });
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let local_candidate = data_root
        .join("tools")
        .join(if cfg!(windows) { "uv.exe" } else { "uv" });
    local_candidate.is_file().then_some(local_candidate)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
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

fn run_checked_command(
    command: &mut Command,
    code: &str,
    user_message: &str,
) -> Result<(), RenderErrorReport> {
    let output = command
        .output()
        .map_err(|error| auto_reframe_error(code, user_message, error.to_string(), true))?;
    if output.status.success() {
        return Ok(());
    }
    let mut report = auto_reframe_error(
        code,
        user_message,
        format!("Command exited with {}", output.status),
        true,
    );
    report.stderr_tail = stderr_tail(&output.stderr);
    Err(report)
}

fn stderr_tail(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .rev()
        .take(STDERR_TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(str::to_owned)
        .collect()
}

fn venv_python(runtime_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        runtime_dir.join("Scripts").join("python.exe")
    } else {
        runtime_dir.join("bin").join("python")
    }
}

fn remove_scoped_runtime_directory(
    runtime_parent: &Path,
    runtime_dir: &Path,
) -> Result<(), RenderErrorReport> {
    if runtime_dir.parent() != Some(runtime_parent)
        || runtime_dir.file_name().and_then(|value| value.to_str()) != Some(RUNTIME_DIRECTORY_NAME)
    {
        return Err(auto_reframe_error(
            "auto_reframe_runtime_scope_failed",
            "Yerel takip motorunun eski sürümü güvenle kaldırılamadı.",
            format!(
                "Refusing to remove {} outside {}",
                runtime_dir.display(),
                runtime_parent.display()
            ),
            false,
        ));
    }
    fs::remove_dir_all(runtime_dir).map_err(|error| {
        auto_reframe_error(
            "auto_reframe_runtime_replace_failed",
            "Yerel takip motorunun eski sürümü güncellenemedi.",
            format!("{}: {error}", runtime_dir.display()),
            true,
        )
    })
}

fn emit_progress(
    app: &AppHandle,
    operation: &str,
    stage: &str,
    progress_percent: Option<f64>,
    message: &str,
) {
    let _ = app.emit(
        AUTO_REFRAME_PROGRESS_EVENT,
        AutoReframeProgress {
            operation: operation.into(),
            stage: stage.into(),
            progress_percent: progress_percent.map(|value| value.clamp(0.0, 100.0)),
            message: message.into(),
        },
    );
}

fn invalid_request(message: impl Into<String>) -> RenderErrorReport {
    auto_reframe_error(
        "auto_reframe_request_invalid",
        message,
        "Auto-reframe request validation failed",
        false,
    )
}

fn tracker_output_error(technical_message: impl Into<String>) -> RenderErrorReport {
    auto_reframe_error(
        "auto_reframe_output_invalid",
        "Yerel takip motoru geçerli bir sonuç üretmedi.",
        technical_message,
        true,
    )
}

fn auto_reframe_error(
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

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "astral-auto-reframe-{label}-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn request(source_path: &Path) -> AnalyzeAutoReframeRequest {
        AnalyzeAutoReframeRequest {
            source_path: source_path.to_string_lossy().into_owned(),
            start_ms: 1_000,
            duration_ms: 10_000,
            seed_time_ms: 4_000,
            selection: NormalizedBoundingBox {
                x: 0.25,
                y: 0.2,
                width: 0.3,
                height: 0.5,
            },
            sample_fps: 6.0,
            analysis_max_dimension: 960,
            force: false,
        }
    }

    fn valid_tracker_json(key: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": TRACKER_SCHEMA,
            "requestKey": key,
            "engine": ENGINE,
            "engineVersion": OPENCV_VERSION,
            "sourceWidth": 1920,
            "sourceHeight": 1080,
            "sourceFps": 30.0,
            "confidenceMetric": CONFIDENCE_METRIC,
            "samples": [
                {"t": 0, "b": [0.2, 0.2, 0.3, 0.5], "c": 1.0},
                {"t": 4000, "b": [0.25, 0.2, 0.3, 0.5], "c": 1.0},
                {"t": 9999, "b": [0.3, 0.2, 0.3, 0.5], "c": 0.0}
            ],
            "warnings": ["manual review"]
        }))
        .unwrap()
    }

    #[test]
    fn request_validation_rejects_bad_ranges_and_boxes() {
        let source = PathBuf::from("unused.mp4");
        let mut value = request(&source);
        value.seed_time_ms = value.duration_ms;
        assert_eq!(
            validate_request_values(&value).unwrap_err().code,
            "auto_reframe_request_invalid"
        );

        value.seed_time_ms = 0;
        value.selection.x = 0.9;
        value.selection.width = 0.2;
        assert!(validate_request_values(&value).is_err());

        value.selection.x = 0.2;
        value.selection.width = 0.3;
        value.sample_fps = f64::NAN;
        assert!(validate_request_values(&value).is_err());
    }

    #[test]
    fn cache_key_changes_with_source_metadata_and_request() {
        let directory = unique_test_dir("cache-key");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.mp4");
        fs::write(&source, b"first").unwrap();

        let first = validate_request(request(&source)).unwrap();
        let first_key = analysis_cache_key(&first);
        assert_ne!(
            analysis_cache_key_with_runner(&first, "runner-sha-a"),
            analysis_cache_key_with_runner(&first, "runner-sha-b")
        );
        let mut changed_request = request(&source);
        changed_request.selection.x = 0.35;
        let second = validate_request(changed_request).unwrap();
        assert_ne!(first_key, analysis_cache_key(&second));

        fs::write(&source, b"replacement-with-a-different-size").unwrap();
        let replacement = validate_request(request(&source)).unwrap();
        assert_ne!(first_key, analysis_cache_key(&replacement));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn compact_tracker_json_parses_without_opencv() {
        let source = PathBuf::from("unused.mp4");
        let request = request(&source);
        let result =
            parse_tracker_output(&valid_tracker_json("cache-key"), &request, "cache-key").unwrap();
        assert_eq!(result.samples.len(), 3);
        assert_eq!(result.samples[1].time_ms, request.seed_time_ms);
        assert_eq!(result.confidence_metric, CONFIDENCE_METRIC);
        assert!(!result.cache_hit);
    }

    #[test]
    fn tracker_json_rejects_wrong_key_and_unsafe_values() {
        let source = PathBuf::from("unused.mp4");
        let request = request(&source);
        assert!(parse_tracker_output(&valid_tracker_json("wrong"), &request, "expected").is_err());

        let invalid = serde_json::to_vec(&serde_json::json!({
            "schema": TRACKER_SCHEMA,
            "requestKey": "expected",
            "engine": ENGINE,
            "engineVersion": OPENCV_VERSION,
            "sourceWidth": 1920,
            "sourceHeight": 1080,
            "sourceFps": 30.0,
            "confidenceMetric": CONFIDENCE_METRIC,
            "samples": [{"t": 4000, "b": [0.9, 0.2, 0.3, 0.5], "c": 1.0}],
            "warnings": []
        }))
        .unwrap();
        assert!(parse_tracker_output(&invalid, &request, "expected").is_err());

        let mut endpoint: serde_json::Value =
            serde_json::from_slice(&valid_tracker_json("expected")).unwrap();
        endpoint["samples"][2]["t"] = serde_json::json!(request.duration_ms);
        assert!(parse_tracker_output(
            &serde_json::to_vec(&endpoint).unwrap(),
            &request,
            "expected"
        )
        .is_err());
    }

    #[test]
    fn uv_commands_have_bounded_network_policy() {
        let mut command = Command::new("uv");
        configure_uv_command(&mut command, Path::new("auto-reframe-test-root"));
        let env_value = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| key.to_string_lossy() == name)
                .and_then(|(_, value)| value)
                .map(|value| value.to_string_lossy().into_owned())
        };
        assert_eq!(
            env_value("UV_HTTP_TIMEOUT").as_deref(),
            Some(UV_HTTP_TIMEOUT_SECONDS)
        );
        assert_eq!(
            env_value("UV_HTTP_RETRIES").as_deref(),
            Some(UV_HTTP_RETRIES)
        );
    }

    #[test]
    fn vit_model_rejects_unverified_bytes() {
        let directory = unique_test_dir("vit-model-hash");
        let model_directory = directory.join("models");
        fs::create_dir_all(&model_directory).unwrap();
        fs::write(
            model_directory.join(VIT_MODEL_NAME),
            b"not-the-pinned-model",
        )
        .unwrap();

        assert!(!vit_model_is_valid(&directory).unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn runtime_packages_disclose_neural_model_and_licenses() {
        let packages = runtime_packages();
        assert_eq!(packages.len(), 3);
        assert!(packages.iter().all(|package| package.free_and_open_source));
        assert!(packages.iter().any(|package| {
            package.name == "opencv-zoo-vittrack"
                && package.version == VIT_MODEL_VERSION
                && package.declared_license == VIT_MODEL_LICENSE
        }));
    }
}
