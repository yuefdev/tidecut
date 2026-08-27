use std::{
    collections::hash_map::DefaultHasher,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use zip::ZipArchive;

use crate::render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport};

pub const ENGINE_NAME: &str = "deepfilter-clearvoice";
pub const MODEL_NAME: &str = "DeepFilterNet3+MossFormer2_SE_48K";
pub const MOSSFORMER_MODEL_NAME: &str = "MossFormer2_SE_48K";
pub const AUDIO_ENHANCEMENT_PROGRESS_EVENT: &str = "audio-enhancement://progress";

const CACHE_SCHEMA: &str = "studio-voice-mossformer2-se-48k-deepfilternet3-v2";
const MAX_DURATION_MS: u64 = 4 * 60 * 60 * 1_000;
const MAX_SOURCE_RANGE_MS: u64 = 24 * 60 * 60 * 1_000;
const SAMPLE_RATE: u64 = 48_000;
const STDERR_TAIL_LINES: usize = 36;

const UV_VERSION: &str = "0.11.28";
const UV_ARCHIVE_NAME: &str = "uv-x86_64-pc-windows-msvc.zip";
const UV_ARCHIVE_URL: &str =
    "https://github.com/astral-sh/uv/releases/download/0.11.28/uv-x86_64-pc-windows-msvc.zip";
const UV_ARCHIVE_SHA256: &str = "0a23463216d09c6a72ff80ef5dc5a795f07dc1575cb84d24596c2f124a441b7b";
const MAX_UV_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
// The signed release archive is about 25.6 MB, but its pinned uv.exe expands
// to about 75.9 MB. Keep the extracted size exact instead of reusing the
// compressed-download ceiling; the archive SHA-256 above authenticates the
// bytes before extraction.
const UV_EXECUTABLE_BYTES: u64 = 75_881_984;

const PYTHON_VERSION: &str = "3.11.15";
const CLEARVOICE_VERSION: &str = "0.1.2";
const TORCH_VERSION: &str = "2.13.0+cpu";
const RUNTIME_DIRECTORY_NAME: &str = "clearvoice-0.1.2-py31115-cpu-v1";
const RUNTIME_MANIFEST_NAME: &str = ".astral-clearvoice-runtime";
const RUNNER_FILE_NAME: &str = "run_clearvoice.py";
const PROGRESS_RUNNER_FILE_NAME: &str = "run_clearvoice_progress_v1.py";

const DEEP_FILTER_VERSION: &str = "0.5.6";
const DEEP_FILTER_DIRECTORY_NAME: &str = "deep-filter-0.5.6-windows-x64";
const DEEP_FILTER_FILE_NAME: &str = "deep-filter.exe";
const DEEP_FILTER_URL: &str = "https://github.com/Rikorose/DeepFilterNet/releases/download/v0.5.6/deep-filter-0.5.6-x86_64-pc-windows-msvc.exe";
const DEEP_FILTER_BYTES: u64 = 26_912_256;
const DEEP_FILTER_SHA256: &str = "75e11fa16445f560cb6b021521ddb89e89270d13b83089705d98776f58fd7915";
// A gentle finishing pass removes residual stationary broadband noise without
// asking two neural models to attenuate the speech by their full amount.
const DEEP_FILTER_ATTENUATION_LIMIT_DB: u8 = 6;

// The neural output stays dominant while a small, time-aligned share of the
// source restores breath and consonant texture that strong separation can
// otherwise flatten. The following tone/dynamics stage is intentionally mild:
// it is voice enhancement, not a replacement for the user's final loudness
// normalization or creative EQ.
const STUDIO_MASTERING_VERSION: &str =
    "natural92-hp65-mudcut-presence-compressor-gain145-limiter-v2";
const STUDIO_ENHANCED_MIX: f64 = 0.92;
const STUDIO_ORIGINAL_MIX: f64 = 0.08;
const STUDIO_OUTPUT_GAIN: f64 = 1.45;

const MODEL_REVISION: &str = "eff8c97925c8bec812af707814b3e5d777fd4503";
const HYBRID_MODEL_REVISION: &str =
    "mossformer2-eff8c97925c8+deepfilternet3-native-0.5.6-atten6+studio-natural92-gain145-v2";
const MODEL_FILE_NAME: &str = "last_best_checkpoint.pt";
const MODEL_INDEX_FILE_NAME: &str = "last_best_checkpoint";
const MODEL_BYTES: u64 = 221_552_019;
const MODEL_SHA256: &str = "03692b9f773bbd6bb43b9c5a41f96b1e28affd66e13796b7bec66ad3d8b227c6";
const MODEL_URL: &str = "https://huggingface.co/alibabasglab/MossFormer2_SE_48K/resolve/eff8c97925c8bec812af707814b3e5d777fd4503/last_best_checkpoint.pt";

// This complete, no-deps set is the environment resolved and exercised for
// ClearVoice 0.1.2 on CPython 3.11.15/Windows x64. Keeping every package exact
// prevents a later dependency release from silently changing inference.
const LOCKED_REQUIREMENTS: &str = r#"anyio==4.14.2
audioread==3.1.0
beautifulsoup4==4.15.0
certifi==2026.6.17
cffi==2.1.0
charset-normalizer==3.4.9
clearvoice==0.1.2
click==8.4.2
colorama==0.4.6
decorator==5.3.1
einops==0.8.2
filelock==3.31.1
fsspec==2026.6.0
gdown==6.1.0
h11==0.16.0
hf-xet==1.5.2
httpcore==1.0.9
httpx==0.28.1
huggingface-hub==1.24.0
idna==3.18
jinja2==3.1.6
joblib==1.5.3
lazy-loader==0.5
librosa==0.10.2.post1
llvmlite==0.48.0
markupsafe==3.0.3
mpmath==1.3.0
msgpack==1.2.1
narwhals==2.24.0
networkx==3.6.1
numba==0.66.0
numpy==1.26.4
opencv-python==4.10.0.84
packaging==26.2
pillow==12.3.0
platformdirs==4.10.1
pooch==1.9.0
pycparser==3.0
pydub==0.25.1
pysocks==1.7.1
python-speech-features==0.6
pyyaml==6.0.3
requests==2.34.2
rotary-embedding-torch==0.8.3
scenedetect==0.6.6
scikit-learn==1.9.0
scipy==1.17.1
setuptools==83.0.0
soundfile==0.12.1
soupsieve==2.9
soxr==1.1.0
sympy==1.14.0
threadpoolctl==3.6.0
torch==2.13.0+cpu
torchaudio==2.11.0+cpu
torchinfo==1.8.0
torchvision==0.28.0+cpu
tqdm==4.69.0
typing-extensions==4.16.0
urllib3==2.7.0
yamlargparse==1.31.1
"#;

const PYTHON_RUNNER: &str = r#"import os
import sys

if len(sys.argv) != 3:
    raise SystemExit("usage: run_clearvoice.py INPUT.wav OUTPUT.wav")

input_path = os.path.abspath(sys.argv[1])
output_path = os.path.abspath(sys.argv[2])
if os.path.exists(output_path):
    raise RuntimeError("refusing to overwrite existing output")

from clearvoice import ClearVoice

voice = ClearVoice(
    task="speech_enhancement",
    model_names=["MossFormer2_SE_48K"],
)
result = voice(input_path=input_path, online_write=False)
voice.write(result, output_path=output_path)

if not os.path.isfile(output_path) or os.path.getsize(output_path) <= 44:
    raise RuntimeError("ClearVoice did not produce a usable WAV")
"#;

// ClearVoice 0.1.2 already splits long MossFormer2 inputs into overlapping
// windows. A hook on the top-level pinned model reports those real forward
// passes without replacing or approximating the upstream overlap algorithm.
// This file lives outside the heavy virtual environment so adding progress
// does not force an already verified ~1.3 GB runtime to be installed again.
const PROGRESS_PYTHON_RUNNER: &str = r#"import json
import os
import sys
import wave

if len(sys.argv) != 3:
    raise SystemExit("usage: run_clearvoice_progress_v1.py INPUT.wav OUTPUT.wav")

input_path = os.path.abspath(sys.argv[1])
output_path = os.path.abspath(sys.argv[2])
if os.path.exists(output_path):
    raise RuntimeError("refusing to overwrite existing output")

from clearvoice import ClearVoice

voice = ClearVoice(
    task="speech_enhancement",
    model_names=["MossFormer2_SE_48K"],
)
speech_model = voice.models[0]
args = speech_model.args

with wave.open(input_path, "rb") as source:
    input_frames = source.getnframes()
    input_rate = source.getframerate()

if input_rate != int(args.sampling_rate):
    raise RuntimeError(f"expected {args.sampling_rate} Hz input, got {input_rate}")

window = int(args.sampling_rate * args.decode_window)
stride = int(window * 0.75)
give_up = (window - stride) // 2
if input_frames > args.sampling_rate * args.one_time_decode_length:
    padded_frames = input_frames
    if padded_frames < window:
        padded_frames = window
    elif padded_frames < window + stride:
        padded_frames = window + stride
    elif (padded_frames - window) % stride != 0:
        padded_frames += padded_frames - ((padded_frames - window) // stride) * stride
    total_segments = ((padded_frames - window) // stride) + 1
else:
    total_segments = 1

completed_segments = 0
def report_progress(_module, _inputs, _output):
    global completed_segments
    completed_segments += 1
    if total_segments <= 1:
        processed_frames = input_frames
    else:
        processed_frames = min(
            input_frames,
            (completed_segments - 1) * stride + window - give_up,
        )
    payload = {
        "completedSegments": min(completed_segments, total_segments),
        "totalSegments": total_segments,
        "processedMs": round(processed_frames * 1000 / input_rate),
    }
    print("ASTRAL_PROGRESS " + json.dumps(payload, separators=(",", ":")), flush=True)

hook = speech_model.model.register_forward_hook(report_progress)
try:
    result = voice(input_path=input_path, online_write=False)
finally:
    hook.remove()

voice.write(result, output_path=output_path)

if not os.path.isfile(output_path) or os.path.getsize(output_path) <= 44:
    raise RuntimeError("ClearVoice did not produce a usable WAV")
"#;

static EXPERIMENTAL_LOCK: Mutex<()> = Mutex::new(());
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalClearVoiceRequest {
    #[serde(default)]
    pub operation_id: String,
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalClearVoiceAsset {
    pub output_path: String,
    pub duration_ms: u64,
    pub cache_hit: bool,
    pub engine: &'static str,
    pub model: &'static str,
    pub model_revision: &'static str,
    pub experimental: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioEnhancementProgress {
    pub operation_id: String,
    pub sequence: u64,
    pub phase: &'static str,
    pub overall_progress_percent: Option<f64>,
    pub phase_progress_percent: Option<f64>,
    pub processed_ms: Option<u64>,
    pub total_ms: u64,
    pub message: String,
}

struct ProgressEmitter {
    app: Option<AppHandle>,
    operation_id: String,
    sequence: u64,
    total_ms: u64,
}

impl ProgressEmitter {
    fn new(app: AppHandle, operation_id: String, total_ms: u64) -> Self {
        Self {
            app: Some(app),
            operation_id,
            sequence: 0,
            total_ms,
        }
    }

    #[cfg(test)]
    fn silent(total_ms: u64) -> Self {
        Self {
            app: None,
            operation_id: "test-operation".into(),
            sequence: 0,
            total_ms,
        }
    }

    fn emit(
        &mut self,
        phase: &'static str,
        overall_progress_percent: Option<f64>,
        phase_progress_percent: Option<f64>,
        processed_ms: Option<u64>,
        message: impl Into<String>,
    ) {
        self.sequence = self.sequence.saturating_add(1);
        let payload = AudioEnhancementProgress {
            operation_id: self.operation_id.clone(),
            sequence: self.sequence,
            phase,
            overall_progress_percent: overall_progress_percent.map(|value| value.clamp(0.0, 100.0)),
            phase_progress_percent: phase_progress_percent.map(|value| value.clamp(0.0, 100.0)),
            processed_ms: processed_ms.map(|value| value.min(self.total_ms)),
            total_ms: self.total_ms,
            message: message.into(),
        };
        // Progress is a best-effort UI channel. Losing the window while a job
        // is running must not corrupt or fail the audio result itself.
        if let Some(app) = &self.app {
            let _ = app.emit(AUDIO_ENHANCEMENT_PROGRESS_EVENT, payload);
        }
    }

    fn mossformer_segment(&mut self, completed: u64, total: u64, processed_ms: u64) {
        let fraction = if total == 0 {
            0.0
        } else {
            (completed.min(total) as f64 / total as f64).clamp(0.0, 1.0)
        };
        self.emit(
            "mossformer",
            Some(8.0 + fraction * 44.0),
            Some(fraction * 100.0),
            Some(processed_ms),
            format!("MossFormer2 konuşmayı ayıklıyor · %{:.0}", fraction * 100.0),
        );
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MossFormerRunnerProgress {
    completed_segments: u64,
    total_segments: u64,
    processed_ms: u64,
}

struct RuntimePaths {
    quality_root: PathBuf,
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
            experimental_error(
                "experimental_clearvoice_directory_failed",
                "Deneysel AI çalışma klasörü hazırlanamadı.",
                format!("{}: {error}", parent.display()),
                true,
            )
        })?;
        for _ in 0..32 {
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
                    return Err(experimental_error(
                        "experimental_clearvoice_directory_failed",
                        "Deneysel AI çalışma klasörü hazırlanamadı.",
                        format!("{}: {error}", path.display()),
                        true,
                    ))
                }
            }
        }
        Err(experimental_error(
            "experimental_clearvoice_directory_failed",
            "Deneysel AI çalışma klasörü hazırlanamadı.",
            format!("Could not allocate a directory below {}", parent.display()),
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
pub async fn prepare_mossformer_test_cleanup(
    app: AppHandle,
    request: ExperimentalClearVoiceRequest,
) -> Result<ExperimentalClearVoiceAsset, RenderErrorReport> {
    validate_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());
    let operation_id = if request.operation_id.trim().is_empty() {
        format!(
            "audio-enhancement-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    } else {
        request.operation_id.clone()
    };

    tauri::async_runtime::spawn_blocking(move || {
        let mut progress = ProgressEmitter::new(app.clone(), operation_id, request.duration_ms);
        progress.emit(
            "queued",
            Some(0.0),
            None,
            None,
            "Stüdyo kalitesinde ses geliştirme sıraya alındı",
        );
        let _guard = EXPERIMENTAL_LOCK.lock().map_err(|_| {
            experimental_error(
                "experimental_clearvoice_lock_failed",
                "Stüdyo ses geliştirme başlatılamadı.",
                "Experimental ClearVoice lock was poisoned",
                true,
            )
        })?;
        prepare_blocking(&app, &ffmpeg_path, &source_path, &request, &mut progress)
    })
    .await
    .map_err(|error| {
        experimental_error(
            "experimental_clearvoice_worker_failed",
            "Stüdyo ses geliştirme tamamlanamadı.",
            format!("Experimental ClearVoice worker failed: {error}"),
            true,
        )
    })?
}

fn prepare_blocking(
    app: &AppHandle,
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &ExperimentalClearVoiceRequest,
    progress: &mut ProgressEmitter,
) -> Result<ExperimentalClearVoiceAsset, RenderErrorReport> {
    ensure_supported_platform()?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("audio-clean-experimental"))
        .map_err(|error| {
            experimental_error(
                "experimental_clearvoice_cache_failed",
                "Deneysel AI ses önbelleği bulunamadı.",
                error.to_string(),
                true,
            )
        })?;
    fs::create_dir_all(&cache_dir).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_cache_failed",
            "Deneysel AI ses önbelleği hazırlanamadı.",
            format!("{}: {error}", cache_dir.display()),
            true,
        )
    })?;
    let key = cache_key(source_path, request)?;
    let output_path = cache_dir.join(format!("{key}.studio-voice.wav"));
    if !request.force && verified_output_wav(&output_path, request.duration_ms).is_ok() {
        progress.emit(
            "completed",
            Some(100.0),
            Some(100.0),
            Some(request.duration_ms),
            "Stüdyo sesi önbellekten hazır",
        );
        return Ok(asset(output_path, request.duration_ms, true));
    }

    progress.emit(
        "runtime-setup",
        None,
        None,
        None,
        "MossFormer2 çalışma ortamı doğrulanıyor",
    );
    let runtime = ensure_runtime(app)?;
    progress.emit(
        "model-download",
        None,
        None,
        None,
        "Doğrulanmış AI modelleri hazırlanıyor",
    );
    ensure_model(&runtime.quality_root)?;
    let deep_filter = ensure_deep_filter(&runtime.quality_root)?;

    let work_parent = runtime.quality_root.join("work");
    let work = ScopedDirectory::create(&work_parent, &key)?;
    let input_path = work.path.join("input-48k-mono.wav");
    progress.emit(
        "preparing-audio",
        Some(6.0),
        None,
        Some(0),
        "Ses 48 kHz AI işlemine hazırlanıyor",
    );
    transcode_input(
        ffmpeg_path,
        source_path,
        request.start_ms,
        request.duration_ms,
        &input_path,
    )?;
    let raw_output = work.path.join("clearvoice-raw.wav");
    progress.emit(
        "mossformer",
        Some(8.0),
        Some(0.0),
        Some(0),
        "MossFormer2 konuşmayı ayıklıyor · AI aşaması 1/2",
    );
    run_clearvoice(&runtime, &input_path, &raw_output, progress)?;
    progress.emit(
        "mossformer",
        Some(52.0),
        Some(100.0),
        Some(request.duration_ms),
        "MossFormer2 tamamlandı · AI aşaması 1/2",
    );
    progress.emit(
        "deepfilter",
        Some(52.0),
        None,
        Some(0),
        "DeepFilterNet3 doğal ayrıntıları koruyarak kalıntıyı temizliyor · AI aşaması 2/2",
    );
    let deep_filtered = run_deep_filter(&deep_filter, &raw_output, &work.path)?;
    progress.emit(
        "deepfilter",
        Some(90.0),
        Some(100.0),
        Some(request.duration_ms),
        "DeepFilterNet3 tamamlandı · AI aşaması 2/2",
    );

    let studio_mastered = work.path.join("studio-mastered.wav");
    progress.emit(
        "mastering",
        Some(91.0),
        Some(0.0),
        Some(request.duration_ms),
        "Doğal ses karışımı, ton ve dinamikler dengeleniyor",
    );
    run_studio_mastering(
        ffmpeg_path,
        &deep_filtered,
        &input_path,
        request.duration_ms,
        &studio_mastered,
    )?;
    progress.emit(
        "mastering",
        Some(97.0),
        Some(100.0),
        Some(request.duration_ms),
        "Stüdyo tonlaması ve güvenli tepe sınırlama tamamlandı",
    );

    let partial_path = cache_dir.join(format!(".{key}.partial.wav"));
    if partial_path.is_file() {
        let _ = fs::remove_file(&partial_path);
    }
    let mut partial_guard = PartialFile::new(partial_path.clone());
    progress.emit(
        "finalizing",
        Some(98.0),
        None,
        Some(request.duration_ms),
        "Geliştirilmiş ses doğrulanıp sonlandırılıyor",
    );
    transcode_output(
        ffmpeg_path,
        &studio_mastered,
        request.duration_ms,
        &partial_path,
    )?;
    verified_output_wav(&partial_path, request.duration_ms)?;
    if output_path.is_file() {
        fs::remove_file(&output_path).map_err(|error| {
            experimental_error(
                "experimental_clearvoice_cache_commit_failed",
                "Deneysel AI çıktısının eski önbelleği değiştirilemedi.",
                format!("{}: {error}", output_path.display()),
                true,
            )
        })?;
    }
    fs::rename(&partial_path, &output_path).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_cache_commit_failed",
            "Deneysel AI çıktısı önbelleğe alınamadı.",
            format!(
                "{} -> {}: {error}",
                partial_path.display(),
                output_path.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    progress.emit(
        "completed",
        Some(100.0),
        Some(100.0),
        Some(request.duration_ms),
        "Stüdyo kalitesinde geliştirilmiş ses hazır",
    );
    Ok(asset(output_path, request.duration_ms, false))
}

fn ensure_supported_platform() -> Result<(), RenderErrorReport> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok(())
    } else {
        Err(experimental_error(
            "experimental_clearvoice_platform_unsupported",
            "Deneysel Studio AI motoru şu anda yalnızca Windows x64 üzerinde çalışır.",
            format!(
                "ClearVoice runtime is pinned for Windows x64; current target is {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            false,
        ))
    }
}

fn ensure_runtime(app: &AppHandle) -> Result<RuntimePaths, RenderErrorReport> {
    let quality_root = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("clearvoice-experimental"))
        .map_err(|error| {
            experimental_error(
                "experimental_clearvoice_runtime_directory_failed",
                "Deneysel Studio AI klasörü hazırlanamadı.",
                error.to_string(),
                true,
            )
        })?;
    ensure_runtime_at(quality_root)
}

fn ensure_runtime_at(quality_root: PathBuf) -> Result<RuntimePaths, RenderErrorReport> {
    fs::create_dir_all(&quality_root).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_directory_failed",
            "Deneysel Studio AI klasörü hazırlanamadı.",
            format!("{}: {error}", quality_root.display()),
            true,
        )
    })?;
    fs::create_dir_all(quality_root.join("temp")).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_directory_failed",
            "Deneysel Studio AI geçici çalışma klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let uv = ensure_uv(&quality_root)?;
    let runtime_root = quality_root.join("runtime");
    fs::create_dir_all(&runtime_root).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_directory_failed",
            "Deneysel Studio AI çalışma ortamı hazırlanamadı.",
            format!("{}: {error}", runtime_root.display()),
            true,
        )
    })?;
    let runtime_dir = runtime_root.join(RUNTIME_DIRECTORY_NAME);
    if runtime_is_valid(&runtime_dir) {
        let progress_runner = ensure_progress_runner(&quality_root)?;
        return Ok(runtime_paths(quality_root, runtime_dir, progress_runner));
    }

    if runtime_dir.exists() {
        remove_scoped_directory(&runtime_root, &runtime_dir)?;
    }
    let mut staging = ScopedDirectory::create(&runtime_root, "clearvoice-runtime")?;
    let requirements = staging.path.join("requirements.lock.txt");
    fs::write(&requirements, LOCKED_REQUIREMENTS).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_write_failed",
            "Deneysel Studio AI bağımlılık listesi yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    let runner = staging.path.join(RUNNER_FILE_NAME);
    fs::write(&runner, PYTHON_RUNNER).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_write_failed",
            "Deneysel Studio AI çalıştırıcısı yazılamadı.",
            error.to_string(),
            true,
        )
    })?;

    let mut venv = Command::new(&uv);
    venv.args(uv_venv_args(&staging.path));
    configure_uv_command(&mut venv, &quality_root);
    run_checked_command(
        &mut venv,
        "experimental_clearvoice_python_setup_failed",
        "Deneysel Studio AI için izole Python 3.11 kurulamadı.",
    )?;
    let python = venv_python(&staging.path);
    if !python.is_file() {
        return Err(experimental_error(
            "experimental_clearvoice_python_missing",
            "Deneysel Studio AI Python çalışma ortamı eksik kaldı.",
            format!("Expected {}", python.display()),
            true,
        ));
    }

    let mut install = Command::new(&uv);
    install.args(uv_install_args(&python, &requirements));
    configure_uv_command(&mut install, &quality_root);
    run_checked_command(
        &mut install,
        "experimental_clearvoice_dependencies_failed",
        "Deneysel Studio AI bağımlılıkları kurulamadı. İnternet bağlantısını kontrol edin.",
    )?;
    verify_python_runtime(&python, &quality_root)?;
    fs::write(
        staging.path.join(RUNTIME_MANIFEST_NAME),
        runtime_fingerprint(),
    )
    .map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_manifest_failed",
            "Deneysel Studio AI çalışma ortamı doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    fs::rename(&staging.path, &runtime_dir).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_commit_failed",
            "Deneysel Studio AI çalışma ortamı etkinleştirilemedi.",
            format!(
                "{} -> {}: {error}",
                staging.path.display(),
                runtime_dir.display()
            ),
            true,
        )
    })?;
    staging.commit();
    let progress_runner = ensure_progress_runner(&quality_root)?;
    Ok(runtime_paths(quality_root, runtime_dir, progress_runner))
}

fn runtime_paths(
    quality_root: PathBuf,
    runtime_dir: PathBuf,
    progress_runner: PathBuf,
) -> RuntimePaths {
    RuntimePaths {
        quality_root,
        python: venv_python(&runtime_dir),
        runner: progress_runner,
    }
}

fn ensure_progress_runner(quality_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let runner_root = quality_root.join("runners");
    fs::create_dir_all(&runner_root).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runner_directory_failed",
            "Deneysel Studio AI ilerleme çalıştırıcısı hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let runner = runner_root.join(PROGRESS_RUNNER_FILE_NAME);
    if fs::read_to_string(&runner).ok().as_deref() != Some(PROGRESS_PYTHON_RUNNER) {
        atomic_write(&runner, PROGRESS_PYTHON_RUNNER.as_bytes())?;
    }
    Ok(runner)
}

pub(crate) fn ensure_uv(quality_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let tools_root = quality_root.join("tools");
    let tool_dir = tools_root.join(format!("uv-{UV_VERSION}-windows-x64"));
    let executable = tool_dir.join("uv.exe");
    let manifest = tool_dir.join(".astral-uv-runtime");
    if executable.is_file()
        && fs::read_to_string(&manifest).is_ok_and(|value| value.trim() == UV_ARCHIVE_SHA256)
    {
        return Ok(executable);
    }

    fs::create_dir_all(&tools_root).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_directory_failed",
            "Deneysel Studio AI kurulum aracı klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if tool_dir.exists() {
        remove_scoped_directory(&tools_root, &tool_dir)?;
    }
    let downloads = quality_root.join("downloads");
    fs::create_dir_all(&downloads).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_download_directory_failed",
            "Deneysel Studio AI indirme klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let archive_path = downloads.join(UV_ARCHIVE_NAME);
    download_verified_file(
        UV_ARCHIVE_URL,
        UV_ARCHIVE_SHA256,
        None,
        &archive_path,
        MAX_UV_ARCHIVE_BYTES,
        "experimental_clearvoice_uv_download_failed",
        "Deneysel Studio AI kurulum aracı indirilemedi.",
    )?;

    let mut staging = ScopedDirectory::create(&tools_root, "uv-runtime")?;
    extract_uv(&archive_path, &staging.path.join("uv.exe"))?;
    fs::write(staging.path.join(".astral-uv-runtime"), UV_ARCHIVE_SHA256).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_manifest_failed",
            "Deneysel Studio AI kurulum aracı doğrulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    fs::rename(&staging.path, &tool_dir).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_commit_failed",
            "Deneysel Studio AI kurulum aracı etkinleştirilemedi.",
            error.to_string(),
            true,
        )
    })?;
    staging.commit();
    Ok(tool_dir.join("uv.exe"))
}

fn extract_uv(archive_path: &Path, destination: &Path) -> Result<(), RenderErrorReport> {
    let archive_file = File::open(archive_path).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_archive_failed",
            "Deneysel Studio AI kurulum arşivi açılamadı.",
            error.to_string(),
            true,
        )
    })?;
    let mut archive = ZipArchive::new(BufReader::new(archive_file)).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_archive_failed",
            "Deneysel Studio AI kurulum arşivi geçersiz.",
            error.to_string(),
            false,
        )
    })?;
    let mut uv_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            experimental_error(
                "experimental_clearvoice_uv_archive_failed",
                "Deneysel Studio AI kurulum arşivi okunamadı.",
                error.to_string(),
                false,
            )
        })?;
        let is_uv = entry.is_file()
            && Path::new(entry.name())
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("uv.exe"));
        if is_uv {
            if uv_index.replace(index).is_some() {
                return Err(experimental_error(
                    "experimental_clearvoice_uv_archive_failed",
                    "Deneysel Studio AI kurulum arşivi güvenli değil.",
                    "Archive contains more than one uv.exe",
                    false,
                ));
            }
        }
    }
    let index = uv_index.ok_or_else(|| {
        experimental_error(
            "experimental_clearvoice_uv_archive_failed",
            "Deneysel Studio AI kurulum arşivi eksik.",
            "Archive does not contain uv.exe",
            false,
        )
    })?;
    let mut entry = archive.by_index(index).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_archive_failed",
            "Deneysel Studio AI kurulum arşivi okunamadı.",
            error.to_string(),
            false,
        )
    })?;
    if entry.size() != UV_EXECUTABLE_BYTES {
        return Err(experimental_error(
            "experimental_clearvoice_uv_archive_failed",
            "Deneysel Studio AI kurulum aracı boyut doğrulamasını geçemedi.",
            format!(
                "Unexpected uv.exe size: {}; expected {UV_EXECUTABLE_BYTES}",
                entry.size()
            ),
            false,
        ));
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| {
            experimental_error(
                "experimental_clearvoice_uv_extract_failed",
                "Deneysel Studio AI kurulum aracı çıkarılamadı.",
                error.to_string(),
                true,
            )
        })?;
    io::copy(&mut entry, &mut output).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_uv_extract_failed",
            "Deneysel Studio AI kurulum aracı çıkarılamadı.",
            error.to_string(),
            true,
        )
    })?;
    output.flush().ok();
    output.sync_all().ok();
    Ok(())
}

fn ensure_model(quality_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let model_dir = quality_root.join("checkpoints").join(MOSSFORMER_MODEL_NAME);
    fs::create_dir_all(&model_dir).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_model_directory_failed",
            "Deneysel Studio AI model klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let model_path = model_dir.join(MODEL_FILE_NAME);
    download_verified_file(
        MODEL_URL,
        MODEL_SHA256,
        Some(MODEL_BYTES),
        &model_path,
        MODEL_BYTES,
        "experimental_clearvoice_model_download_failed",
        "Deneysel Studio AI modeli indirilemedi. Yaklaşık 222 MB model için internet bağlantısını kontrol edin.",
    )?;
    let index_path = model_dir.join(MODEL_INDEX_FILE_NAME);
    let expected_index = format!("{MODEL_FILE_NAME}\n");
    if fs::read_to_string(&index_path).ok().as_deref() != Some(expected_index.as_str()) {
        atomic_write(&index_path, expected_index.as_bytes())?;
    }
    Ok(model_path)
}

fn ensure_deep_filter(quality_root: &Path) -> Result<PathBuf, RenderErrorReport> {
    let tool_dir = quality_root.join("tools").join(DEEP_FILTER_DIRECTORY_NAME);
    fs::create_dir_all(&tool_dir).map_err(|error| {
        experimental_error(
            "experimental_deepfilter_directory_failed",
            "DeepFilterNet3 çalışma klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let executable = tool_dir.join(DEEP_FILTER_FILE_NAME);
    download_verified_file(
        DEEP_FILTER_URL,
        DEEP_FILTER_SHA256,
        Some(DEEP_FILTER_BYTES),
        &executable,
        DEEP_FILTER_BYTES,
        "experimental_deepfilter_download_failed",
        "DeepFilterNet3 motoru indirilemedi. İnternet bağlantısını kontrol edin.",
    )?;
    Ok(executable)
}

#[allow(clippy::too_many_arguments)]
fn download_verified_file(
    url: &str,
    expected_sha256: &str,
    exact_bytes: Option<u64>,
    destination: &Path,
    max_bytes: u64,
    error_code: &str,
    user_message: &str,
) -> Result<(), RenderErrorReport> {
    if verified_file(destination, expected_sha256, exact_bytes)? {
        return Ok(());
    }
    if destination.is_file() {
        fs::remove_file(destination).map_err(|error| {
            experimental_error(
                error_code,
                user_message,
                format!("Could not replace {}: {error}", destination.display()),
                true,
            )
        })?;
    }
    let partial_path = sibling_with_suffix(destination, ".partial");
    if partial_path.is_file() {
        let _ = fs::remove_file(&partial_path);
    }
    let mut partial_guard = PartialFile::new(partial_path.clone());
    let mut response = ureq::get(url).call().map_err(|error| {
        experimental_error(error_code, user_message, format!("{url}: {error}"), true)
    })?;
    let content_length = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if content_length.is_some_and(|bytes| bytes > max_bytes)
        || exact_bytes
            .is_some_and(|expected| content_length.is_some_and(|actual| actual != expected))
    {
        return Err(experimental_error(
            error_code,
            "Deneysel Studio AI indirmesinin boyut doğrulaması başarısız oldu.",
            format!("Unexpected Content-Length {content_length:?} for {url}"),
            false,
        ));
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial_path)
        .map_err(|error| {
            experimental_error(
                error_code,
                user_message,
                format!("{}: {error}", partial_path.display()),
                true,
            )
        })?;
    let copied = io::copy(
        &mut response
            .body_mut()
            .as_reader()
            .take(max_bytes.saturating_add(1)),
        &mut file,
    )
    .map_err(|error| experimental_error(error_code, user_message, error.to_string(), true))?;
    if copied > max_bytes || exact_bytes.is_some_and(|expected| copied != expected) {
        return Err(experimental_error(
            error_code,
            "Deneysel Studio AI indirmesinin boyut doğrulaması başarısız oldu.",
            format!("Downloaded {copied} bytes; expected {exact_bytes:?}, max {max_bytes}"),
            false,
        ));
    }
    file.flush().ok();
    file.sync_all().ok();
    drop(file);
    if !verified_file(&partial_path, expected_sha256, exact_bytes)? {
        return Err(experimental_error(
            error_code,
            "Deneysel Studio AI indirmesinin güvenlik doğrulaması başarısız oldu.",
            format!("Expected SHA-256 {expected_sha256}"),
            false,
        ));
    }
    fs::rename(&partial_path, destination).map_err(|error| {
        experimental_error(
            error_code,
            user_message,
            format!(
                "{} -> {}: {error}",
                partial_path.display(),
                destination.display()
            ),
            true,
        )
    })?;
    partial_guard.commit();
    Ok(())
}

fn verified_file(
    path: &Path,
    expected_sha256: &str,
    exact_bytes: Option<u64>,
) -> Result<bool, RenderErrorReport> {
    let metadata = match path.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(experimental_error(
                "experimental_clearvoice_file_read_failed",
                "Deneysel Studio AI dosyası okunamadı.",
                format!("{}: {error}", path.display()),
                true,
            ))
        }
    };
    if exact_bytes.is_some_and(|expected| metadata.len() != expected) {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_file_read_failed",
            "Deneysel Studio AI dosyası okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            experimental_error(
                "experimental_clearvoice_file_read_failed",
                "Deneysel Studio AI dosyası okunamadı.",
                error.to_string(),
                true,
            )
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()) == expected_sha256)
}

fn atomic_write(destination: &Path, contents: &[u8]) -> Result<(), RenderErrorReport> {
    let partial_path = sibling_with_suffix(destination, ".partial");
    if partial_path.is_file() {
        let _ = fs::remove_file(&partial_path);
    }
    let mut guard = PartialFile::new(partial_path.clone());
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial_path)
        .map_err(|error| {
            experimental_error(
                "experimental_clearvoice_file_write_failed",
                "Deneysel Studio AI dosyası yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
    file.write_all(contents).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_file_write_failed",
            "Deneysel Studio AI dosyası yazılamadı.",
            error.to_string(),
            true,
        )
    })?;
    file.flush().ok();
    file.sync_all().ok();
    drop(file);
    if destination.is_file() {
        fs::remove_file(destination).map_err(|error| {
            experimental_error(
                "experimental_clearvoice_file_write_failed",
                "Deneysel Studio AI dosyası güncellenemedi.",
                error.to_string(),
                true,
            )
        })?;
    }
    fs::rename(&partial_path, destination).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_file_write_failed",
            "Deneysel Studio AI dosyası etkinleştirilemedi.",
            error.to_string(),
            true,
        )
    })?;
    guard.commit();
    Ok(())
}

fn configure_uv_command(command: &mut Command, quality_root: &Path) {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(
            "SYSTEMROOT",
            std::env::var_os("SYSTEMROOT").unwrap_or_default(),
        )
        .env("TEMP", quality_root.join("temp"))
        .env("TMP", quality_root.join("temp"))
        .env("UV_CACHE_DIR", quality_root.join("uv-cache"))
        .env("UV_PYTHON_INSTALL_DIR", quality_root.join("python"))
        .env("UV_MANAGED_PYTHON", "1")
        .env("UV_PYTHON_DOWNLOADS", "automatic")
        .env("UV_NO_PROJECT", "1")
        .env("UV_NO_CONFIG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process(command);
}

fn uv_venv_args(destination: &Path) -> Vec<OsString> {
    vec![
        "venv".into(),
        "--python".into(),
        PYTHON_VERSION.into(),
        "--managed-python".into(),
        "--allow-existing".into(),
        "--no-project".into(),
        "--no-config".into(),
        destination.as_os_str().to_owned(),
    ]
}

fn uv_install_args(python: &Path, requirements: &Path) -> Vec<OsString> {
    vec![
        "pip".into(),
        "install".into(),
        "--python".into(),
        python.as_os_str().to_owned(),
        "--requirements".into(),
        requirements.as_os_str().to_owned(),
        "--no-deps".into(),
        "--torch-backend".into(),
        "cpu".into(),
        "--default-index".into(),
        "https://pypi.org/simple".into(),
        "--no-config".into(),
    ]
}

fn verify_python_runtime(python: &Path, quality_root: &Path) -> Result<(), RenderErrorReport> {
    let verification = format!(
        "import importlib.metadata as m, sys, torch; assert sys.version_info[:3] == (3, 11, 15); assert m.version('clearvoice') == '{CLEARVOICE_VERSION}'; assert torch.__version__ == '{TORCH_VERSION}'; assert torch.version.cuda is None; print('astral-clearvoice-runtime-ok')"
    );
    let mut command = Command::new(python);
    command.args(["-I", "-c", &verification]);
    configure_python_command(&mut command, quality_root);
    run_checked_command(
        &mut command,
        "experimental_clearvoice_runtime_verification_failed",
        "Deneysel Studio AI çalışma ortamı doğrulanamadı.",
    )?;
    Ok(())
}

fn run_deep_filter(
    executable: &Path,
    input_path: &Path,
    work_dir: &Path,
) -> Result<PathBuf, RenderErrorReport> {
    if !verified_file(executable, DEEP_FILTER_SHA256, Some(DEEP_FILTER_BYTES))? {
        return Err(experimental_error(
            "experimental_deepfilter_checksum_failed",
            "DeepFilterNet3 motor doğrulaması başarısız oldu.",
            format!(
                "{} did not match {DEEP_FILTER_SHA256}",
                executable.display()
            ),
            false,
        ));
    }
    let output_dir = work_dir.join("deepfilter-output");
    fs::create_dir_all(&output_dir).map_err(|error| {
        experimental_error(
            "experimental_deepfilter_output_directory_failed",
            "DeepFilterNet3 çıktı klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let file_name = input_path.file_name().ok_or_else(|| {
        experimental_error(
            "experimental_deepfilter_input_invalid",
            "DeepFilterNet3 giriş dosyasını okuyamadı.",
            input_path.display().to_string(),
            false,
        )
    })?;
    // The official native v0.5.6 executable preserves the input filename in
    // its separate output directory (the Python CLI uses a model suffix).
    let output_path = output_dir.join(file_name);
    let mut command = Command::new(executable);
    command
        .arg("-D")
        .args([
            "--atten-lim-db",
            &DEEP_FILTER_ATTENUATION_LIMIT_DB.to_string(),
            "--output-dir",
        ])
        .arg(&output_dir)
        .arg(input_path)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    run_checked_command(
        &mut command,
        "experimental_deepfilter_inference_failed",
        "DeepFilterNet3 geniş bant gürültüyü işleyemedi.",
    )?;
    if !output_path.metadata().is_ok_and(|value| value.len() > 44) {
        return Err(experimental_error(
            "experimental_deepfilter_output_missing",
            "DeepFilterNet3 geçerli bir ses üretemedi.",
            format!("Missing or empty output: {}", output_path.display()),
            true,
        ));
    }
    Ok(output_path)
}

fn run_studio_mastering(
    ffmpeg: &Path,
    enhanced: &Path,
    original: &Path,
    duration_ms: u64,
    destination: &Path,
) -> Result<(), RenderErrorReport> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-nostats", "-nostdin", "-y", "-i"])
        .arg(enhanced)
        .arg("-i")
        .arg(original)
        .arg("-filter_complex")
        .arg(studio_mastering_filter(duration_ms))
        .args([
            "-map",
            "[studio]",
            "-vn",
            "-sn",
            "-dn",
            "-ar",
            "48000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-loglevel",
            "warning",
        ])
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let output = command.output().map_err(|error| {
        experimental_error(
            "studio_voice_mastering_failed",
            "Stüdyo tonlaması uygulanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !output.status.success() || !destination.metadata().is_ok_and(|value| value.len() > 44) {
        return Err(experimental_error_with_stderr(
            "studio_voice_mastering_failed",
            "Stüdyo tonlaması ve doğal ses karışımı tamamlanamadı.",
            format!("FFmpeg exited with {}", output.status),
            true,
            tail_lines(&output.stderr),
        ));
    }
    Ok(())
}

fn studio_mastering_filter(duration_ms: u64) -> String {
    let samples = duration_samples(duration_ms);
    format!(
        "[0:a]aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=mono,volume={STUDIO_ENHANCED_MIX}[enhanced];\
[1:a]aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=mono,highpass=f=65,volume={STUDIO_ORIGINAL_MIX}[natural];\
[enhanced][natural]amix=inputs=2:duration=first:dropout_transition=0:normalize=0,\
highpass=f=65,equalizer=f=180:t=q:w=0.8:g=-1.2,equalizer=f=3200:t=q:w=1.0:g=1.6,equalizer=f=9000:t=q:w=0.8:g=0.7,\
acompressor=threshold=0.18:ratio=2:attack=15:release=180:makeup=1.12:knee=2.5:detection=rms:link=average,\
volume={STUDIO_OUTPUT_GAIN},alimiter=limit=0.891251:attack=5:release=80:level=false:latency=1,apad=whole_len={samples},atrim=end_sample={samples},asetpts=PTS-STARTPTS[studio]"
    )
}

fn run_clearvoice(
    runtime: &RuntimePaths,
    input_path: &Path,
    output_path: &Path,
    progress: &mut ProgressEmitter,
) -> Result<(), RenderErrorReport> {
    let model_path = runtime
        .quality_root
        .join("checkpoints")
        .join(MOSSFORMER_MODEL_NAME)
        .join(MODEL_FILE_NAME);
    if !verified_file(&model_path, MODEL_SHA256, Some(MODEL_BYTES))? {
        return Err(experimental_error(
            "experimental_clearvoice_model_checksum_failed",
            "Deneysel Studio AI model doğrulaması başarısız oldu.",
            format!("{} did not match {MODEL_SHA256}", model_path.display()),
            false,
        ));
    }
    let mut command = Command::new(&runtime.python);
    command
        .args([OsStr::new("-I"), runtime.runner.as_os_str()])
        .arg(input_path)
        .arg(output_path)
        .current_dir(&runtime.quality_root);
    configure_python_command(&mut command, &runtime.quality_root);
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command.spawn().map_err(|error| {
        experimental_error(
            "experimental_clearvoice_inference_failed",
            "Deneysel Studio AI sesi işleyemedi. Bu deneysel model bazı kayıtlarda çalışmayabilir.",
            format!("Failed to start {program}: {error}"),
            true,
        )
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        experimental_error(
            "experimental_clearvoice_inference_failed",
            "Deneysel Studio AI ilerlemesi okunamadı.",
            "ClearVoice stdout pipe was unavailable",
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        experimental_error(
            "experimental_clearvoice_inference_failed",
            "Deneysel Studio AI tanı çıktısı okunamadı.",
            "ClearVoice stderr pipe was unavailable",
            true,
        )
    })?;
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_to_end(&mut bytes);
        bytes
    });
    let mut stdout_tail = Vec::new();
    let mut stdout_error = None;
    for line_result in BufReader::new(stdout).lines() {
        let line = match line_result {
            Ok(line) => line,
            Err(error) => {
                stdout_error = Some(error);
                break;
            }
        };
        if let Some(payload) = line.strip_prefix("ASTRAL_PROGRESS ") {
            if let Ok(update) = serde_json::from_str::<MossFormerRunnerProgress>(payload) {
                progress.mossformer_segment(
                    update.completed_segments,
                    update.total_segments,
                    update.processed_ms,
                );
                continue;
            }
        }
        stdout_tail.push(line);
        if stdout_tail.len() > STDERR_TAIL_LINES {
            stdout_tail.remove(0);
        }
    }
    if let Some(error) = stdout_error {
        let _ = child.kill();
        let _ = child.wait();
        let _ = stderr_reader.join();
        return Err(experimental_error(
            "experimental_clearvoice_inference_failed",
            "Deneysel Studio AI ilerlemesi okunamadı.",
            error.to_string(),
            true,
        ));
    }
    let status = child.wait().map_err(|error| {
        experimental_error(
            "experimental_clearvoice_inference_failed",
            "Deneysel Studio AI işlemi tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        let mut combined = tail_lines(&stderr_bytes);
        combined.extend(stdout_tail);
        if combined.len() > STDERR_TAIL_LINES {
            combined = combined.split_off(combined.len() - STDERR_TAIL_LINES);
        }
        return Err(experimental_error_with_stderr(
            "experimental_clearvoice_inference_failed",
            "Deneysel Studio AI sesi işleyemedi. Bu deneysel model bazı kayıtlarda çalışmayabilir.",
            format!("{program} exited with {status}"),
            true,
            combined,
        ));
    }
    if !output_path.metadata().is_ok_and(|value| value.len() > 44) {
        return Err(experimental_error(
            "experimental_clearvoice_output_missing",
            "Deneysel Studio AI geçerli bir ses üretemedi.",
            format!("Missing or empty output: {}", output_path.display()),
            true,
        ));
    }
    Ok(())
}

fn configure_python_command(command: &mut Command, quality_root: &Path) {
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .clamp(1, 8)
        .to_string();
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(
            "SYSTEMROOT",
            std::env::var_os("SYSTEMROOT").unwrap_or_default(),
        )
        .env("TEMP", quality_root.join("temp"))
        .env("TMP", quality_root.join("temp"))
        .env("HF_HOME", quality_root.join("huggingface-cache"))
        .env("TORCH_HOME", quality_root.join("torch-cache"))
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .env("OMP_NUM_THREADS", &threads)
        .env("MKL_NUM_THREADS", &threads)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process(command);
}

fn run_checked_command(
    command: &mut Command,
    code: &str,
    user_message: &str,
) -> Result<Output, RenderErrorReport> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command.output().map_err(|error| {
        experimental_error(
            code,
            user_message,
            format!("Failed to start {program}: {error}"),
            true,
        )
    })?;
    if output.status.success() {
        return Ok(output);
    }
    let stderr = tail_lines(&output.stderr);
    let stdout = tail_lines(&output.stdout);
    let mut combined = stderr;
    combined.extend(stdout);
    if combined.len() > STDERR_TAIL_LINES {
        combined = combined.split_off(combined.len() - STDERR_TAIL_LINES);
    }
    Err(experimental_error_with_stderr(
        code,
        user_message,
        format!("{program} exited with {}", output.status),
        true,
        combined,
    ))
}

fn transcode_input(
    ffmpeg: &Path,
    source: &Path,
    start_ms: u64,
    duration_ms: u64,
    destination: &Path,
) -> Result<(), RenderErrorReport> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-nostats", "-nostdin", "-y", "-i"])
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af"])
        .arg(input_filter(start_ms, duration_ms))
        .args([
            "-ar",
            "48000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-loglevel",
            "warning",
        ])
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let output = command.output().map_err(|error| {
        experimental_error(
            "experimental_clearvoice_ffmpeg_failed",
            "Deneysel Studio AI için ses hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !output.status.success() || !destination.metadata().is_ok_and(|value| value.len() > 44) {
        return Err(experimental_error_with_stderr(
            "experimental_clearvoice_ffmpeg_failed",
            "Deneysel Studio AI için 48 kHz mono ses hazırlanamadı.",
            format!("FFmpeg exited with {}", output.status),
            true,
            tail_lines(&output.stderr),
        ));
    }
    Ok(())
}

fn transcode_output(
    ffmpeg: &Path,
    source: &Path,
    duration_ms: u64,
    destination: &Path,
) -> Result<(), RenderErrorReport> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-nostats", "-nostdin", "-y", "-i"])
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af"])
        .arg(output_filter(duration_ms))
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
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let output = command.output().map_err(|error| {
        experimental_error(
            "experimental_clearvoice_ffmpeg_failed",
            "Deneysel Studio AI çıktısı sonlandırılamadı.",
            error.to_string(),
            true,
        )
    })?;
    if !output.status.success() {
        return Err(experimental_error_with_stderr(
            "experimental_clearvoice_ffmpeg_failed",
            "Deneysel Studio AI çıktısı 48 kHz WAV biçimine dönüştürülemedi.",
            format!("FFmpeg exited with {}", output.status),
            true,
            tail_lines(&output.stderr),
        ));
    }
    Ok(())
}

fn input_filter(start_ms: u64, duration_ms: u64) -> String {
    let samples = duration_samples(duration_ms);
    format!(
        "atrim=start={}:duration={},asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=s16:sample_rates=48000:channel_layouts=mono,apad=whole_len={samples},atrim=end_sample={samples},asetpts=PTS-STARTPTS",
        seconds(start_ms),
        seconds(duration_ms),
    )
}

fn output_filter(duration_ms: u64) -> String {
    let samples = duration_samples(duration_ms);
    format!(
        "aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,alimiter=limit=0.98:latency=1,apad=whole_len={samples},atrim=end_sample={samples},asetpts=PTS-STARTPTS"
    )
}

fn duration_samples(duration_ms: u64) -> u64 {
    duration_ms.saturating_mul(SAMPLE_RATE) / 1_000
}

fn verified_output_wav(path: &Path, duration_ms: u64) -> Result<(), RenderErrorReport> {
    let expected_data_bytes = duration_samples(duration_ms).saturating_mul(4);
    let mut file = File::open(path).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_output_invalid",
            "Deneysel Studio AI çıktısı okunamadı.",
            format!("{}: {error}", path.display()),
            true,
        )
    })?;
    let mut riff = [0_u8; 12];
    file.read_exact(&mut riff).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_output_invalid",
            "Deneysel Studio AI geçerli bir WAV üretmedi.",
            error.to_string(),
            true,
        )
    })?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(experimental_error(
            "experimental_clearvoice_output_invalid",
            "Deneysel Studio AI geçerli bir WAV üretmedi.",
            "Missing RIFF/WAVE signature",
            true,
        ));
    }
    let mut format = None;
    let mut data_bytes = None;
    loop {
        let mut header = [0_u8; 8];
        match file.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => {
                return Err(experimental_error(
                    "experimental_clearvoice_output_invalid",
                    "Deneysel Studio AI WAV çıktısı okunamadı.",
                    error.to_string(),
                    true,
                ))
            }
        }
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64;
        if &header[0..4] == b"fmt " {
            if size < 16 {
                break;
            }
            let mut fmt = [0_u8; 16];
            file.read_exact(&mut fmt).map_err(|error| {
                experimental_error(
                    "experimental_clearvoice_output_invalid",
                    "Deneysel Studio AI WAV biçimi okunamadı.",
                    error.to_string(),
                    true,
                )
            })?;
            format = Some((
                u16::from_le_bytes(fmt[0..2].try_into().unwrap()),
                u16::from_le_bytes(fmt[2..4].try_into().unwrap()),
                u32::from_le_bytes(fmt[4..8].try_into().unwrap()),
                u16::from_le_bytes(fmt[14..16].try_into().unwrap()),
            ));
            file.seek(SeekFrom::Current((size - 16 + (size & 1)) as i64))
                .map_err(|error| {
                    experimental_error(
                        "experimental_clearvoice_output_invalid",
                        "Deneysel Studio AI WAV biçimi okunamadı.",
                        error.to_string(),
                        true,
                    )
                })?;
        } else if &header[0..4] == b"data" {
            data_bytes = Some(size);
            break;
        } else {
            file.seek(SeekFrom::Current((size + (size & 1)) as i64))
                .map_err(|error| {
                    experimental_error(
                        "experimental_clearvoice_output_invalid",
                        "Deneysel Studio AI WAV çıktısı okunamadı.",
                        error.to_string(),
                        true,
                    )
                })?;
        }
    }
    if format != Some((1, 2, 48_000, 16)) || data_bytes != Some(expected_data_bytes) {
        return Err(experimental_error(
            "experimental_clearvoice_output_duration_mismatch",
            "Deneysel Studio AI çıktısının süresi veya biçimi doğrulanamadı.",
            format!(
                "Expected PCM s16 stereo 48k with {expected_data_bytes} data bytes; got format={format:?}, data={data_bytes:?}"
            ),
            true,
        ));
    }
    Ok(())
}

fn validate_request(request: &ExperimentalClearVoiceRequest) -> Result<(), RenderErrorReport> {
    if request.operation_id.len() > 160 || request.operation_id.contains(['\r', '\n']) {
        return Err(experimental_error(
            "invalid_audio_enhancement_operation_id",
            "Hibrit AI işlem kimliği geçersiz.",
            "operationId must be at most 160 bytes and contain no line breaks",
            false,
        ));
    }
    if request.duration_ms == 0 || request.duration_ms > MAX_DURATION_MS {
        return Err(experimental_error(
            "invalid_experimental_clearvoice_duration",
            "Deneysel Studio AI için seçilen ses süresi geçersiz.",
            format!("durationMs must be between 1 and {MAX_DURATION_MS}"),
            false,
        ));
    }
    if request.start_ms > MAX_SOURCE_RANGE_MS
        || request.start_ms.saturating_add(request.duration_ms) > MAX_SOURCE_RANGE_MS
    {
        return Err(experimental_error(
            "invalid_experimental_clearvoice_range",
            "Deneysel Studio AI için seçilen ses aralığı geçersiz.",
            format!(
                "startMs + durationMs must not exceed {MAX_SOURCE_RANGE_MS}; got {} + {}",
                request.start_ms, request.duration_ms
            ),
            false,
        ));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    let path = PathBuf::from(value);
    if value.trim().is_empty() || !path.is_file() {
        return Err(experimental_error(
            "missing_media",
            "Deneysel Studio AI ile temizlenecek medya bulunamadı.",
            format!("Not a readable file: {}", path.display()),
            true,
        ));
    }
    fs::canonicalize(&path).map_err(|error| {
        experimental_error(
            "missing_media",
            "Deneysel Studio AI kaynak medyasını açamadı.",
            error.to_string(),
            true,
        )
    })
}

fn cache_key(
    source_path: &Path,
    request: &ExperimentalClearVoiceRequest,
) -> Result<String, RenderErrorReport> {
    let metadata = source_path.metadata().map_err(|error| {
        experimental_error(
            "missing_media",
            "Kaynak medya bulunamadı.",
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
    MODEL_REVISION.hash(&mut hasher);
    MODEL_SHA256.hash(&mut hasher);
    DEEP_FILTER_VERSION.hash(&mut hasher);
    DEEP_FILTER_SHA256.hash(&mut hasher);
    DEEP_FILTER_ATTENUATION_LIMIT_DB.hash(&mut hasher);
    STUDIO_MASTERING_VERSION.hash(&mut hasher);
    STUDIO_ENHANCED_MIX.to_bits().hash(&mut hasher);
    STUDIO_ORIGINAL_MIX.to_bits().hash(&mut hasher);
    STUDIO_OUTPUT_GAIN.to_bits().hash(&mut hasher);
    PROGRESS_PYTHON_RUNNER.hash(&mut hasher);
    source_path.to_string_lossy().hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    request.start_ms.hash(&mut hasher);
    request.duration_ms.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn runtime_is_valid(runtime_dir: &Path) -> bool {
    venv_python(runtime_dir).is_file()
        && fs::read_to_string(runtime_dir.join(RUNNER_FILE_NAME))
            .is_ok_and(|value| value == PYTHON_RUNNER)
        && fs::read_to_string(runtime_dir.join("requirements.lock.txt"))
            .is_ok_and(|value| value == LOCKED_REQUIREMENTS)
        && fs::read_to_string(runtime_dir.join(RUNTIME_MANIFEST_NAME))
            .is_ok_and(|value| value.trim() == runtime_fingerprint())
}

fn runtime_fingerprint() -> String {
    let mut hasher = Sha256::new();
    for value in [
        UV_VERSION,
        UV_ARCHIVE_SHA256,
        PYTHON_VERSION,
        CLEARVOICE_VERSION,
        TORCH_VERSION,
        LOCKED_REQUIREMENTS,
        PYTHON_RUNNER,
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn venv_python(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("Scripts").join("python.exe")
}

fn remove_scoped_directory(root: &Path, target: &Path) -> Result<(), RenderErrorReport> {
    if target.parent() != Some(root) || target == root {
        return Err(experimental_error(
            "experimental_clearvoice_scoped_remove_refused",
            "Deneysel Studio AI eski çalışma ortamını güvenle yenileyemedi.",
            format!(
                "Refused to remove {} outside direct root {}",
                target.display(),
                root.display()
            ),
            false,
        ));
    }
    fs::remove_dir_all(target).map_err(|error| {
        experimental_error(
            "experimental_clearvoice_runtime_replace_failed",
            "Deneysel Studio AI eski çalışma ortamını yenileyemedi.",
            format!("{}: {error}", target.display()),
            true,
        )
    })
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clearvoice".into());
    path.with_file_name(format!("{name}{suffix}"))
}

fn seconds(value_ms: u64) -> String {
    format!("{}.{:03}", value_ms / 1_000, value_ms % 1_000)
}

fn tail_lines(bytes: &[u8]) -> Vec<String> {
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

fn asset(output_path: PathBuf, duration_ms: u64, cache_hit: bool) -> ExperimentalClearVoiceAsset {
    ExperimentalClearVoiceAsset {
        output_path: output_path.to_string_lossy().into_owned(),
        duration_ms,
        cache_hit,
        engine: ENGINE_NAME,
        model: MODEL_NAME,
        model_revision: HYBRID_MODEL_REVISION,
        experimental: true,
    }
}

fn experimental_error(
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

fn experimental_error_with_stderr(
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

    fn request() -> ExperimentalClearVoiceRequest {
        ExperimentalClearVoiceRequest {
            operation_id: "test-operation".into(),
            source_path: "voice.wav".into(),
            start_ms: 1_250,
            duration_ms: 81_016,
            ffmpeg_path: None,
            force: false,
        }
    }

    #[test]
    fn model_and_runtime_are_immutably_pinned() {
        assert_eq!(MODEL_REVISION, "eff8c97925c8bec812af707814b3e5d777fd4503");
        assert_eq!(MODEL_BYTES, 221_552_019);
        assert_eq!(UV_EXECUTABLE_BYTES, 75_881_984);
        assert_eq!(DEEP_FILTER_BYTES, 26_912_256);
        assert_eq!(DEEP_FILTER_ATTENUATION_LIMIT_DB, 6);
        assert_eq!(STUDIO_ENHANCED_MIX + STUDIO_ORIGINAL_MIX, 1.0);
        assert!(HYBRID_MODEL_REVISION.contains("studio-natural92-gain145-v2"));
        assert!(UV_EXECUTABLE_BYTES > MAX_UV_ARCHIVE_BYTES);
        assert_eq!(MODEL_SHA256.len(), 64);
        assert_eq!(UV_ARCHIVE_SHA256.len(), 64);
        assert_eq!(DEEP_FILTER_SHA256.len(), 64);
        assert!(MODEL_URL.contains(MODEL_REVISION));
        assert!(LOCKED_REQUIREMENTS.contains("clearvoice==0.1.2\n"));
        assert!(LOCKED_REQUIREMENTS.contains("torch==2.13.0+cpu\n"));
        assert!(LOCKED_REQUIREMENTS
            .lines()
            .all(|line| line.is_empty() || line.contains("==")));
    }

    #[test]
    fn studio_mastering_is_gentle_bounded_and_duration_locked() {
        let filter = studio_mastering_filter(1_500);
        assert!(filter.contains("volume=0.92[enhanced]"));
        assert!(filter.contains("volume=0.08[natural]"));
        assert!(filter.contains("equalizer=f=3200"));
        assert!(filter.contains("acompressor="));
        assert!(filter.contains("volume=1.45,alimiter=limit=0.891251"));
        assert!(filter.contains("level=false"));
        assert!(filter.contains("apad=whole_len=72000"));
        assert!(filter.contains("atrim=end_sample=72000"));
    }

    #[test]
    #[ignore = "requires the pinned uv release archive or network access"]
    fn real_uv_release_archive_extracts_and_runs() {
        let quality_root = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_ROOT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_ROOT to the ClearVoice cache root");
        let executable = ensure_uv(&quality_root).expect("pinned uv archive should extract");
        let output = Command::new(&executable)
            .arg("--version")
            .output()
            .expect("extracted uv.exe should start");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(UV_VERSION));
    }

    #[test]
    #[ignore = "downloads and installs the pinned ClearVoice CPU runtime"]
    fn real_clearvoice_runtime_installs_and_verifies() {
        let quality_root = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_ROOT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_ROOT to the ClearVoice cache root");
        let runtime = ensure_runtime_at(quality_root).expect("ClearVoice runtime should install");
        assert!(runtime.python.is_file());
        assert!(runtime.runner.is_file());
        verify_python_runtime(&runtime.python, &runtime.quality_root)
            .expect("installed ClearVoice runtime should verify");
    }

    #[test]
    #[ignore = "downloads and verifies the pinned MossFormer2 checkpoint"]
    fn real_mossformer_model_downloads_and_verifies() {
        let quality_root = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_ROOT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_ROOT to the ClearVoice cache root");
        let model = ensure_model(&quality_root).expect("MossFormer2 model should download");
        assert!(verified_file(&model, MODEL_SHA256, Some(MODEL_BYTES)).unwrap());
    }

    #[test]
    #[ignore = "runs real MossFormer2 CPU inference on a supplied WAV"]
    fn real_mossformer_runtime_inference() {
        let quality_root = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_ROOT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_ROOT to the ClearVoice cache root");
        let input = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_INPUT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_INPUT to a 48 kHz mono WAV");
        let runtime = ensure_runtime_at(quality_root).expect("ClearVoice runtime should verify");
        ensure_model(&runtime.quality_root).expect("MossFormer2 model should verify");
        let work = ScopedDirectory::create(
            &runtime.quality_root.join("temp"),
            "mossformer-inference-smoke",
        )
        .expect("smoke work directory should be created");
        let output = work.path.join("clearvoice-smoke.wav");
        let mut progress = ProgressEmitter::silent(request().duration_ms);
        run_clearvoice(&runtime, &input, &output, &mut progress)
            .expect("MossFormer2 inference should run");
        assert!(output.metadata().is_ok_and(|value| value.len() > 44));
    }

    #[test]
    #[ignore = "downloads and runs the pinned native DeepFilterNet3 executable"]
    fn real_deepfilter_runtime_inference() {
        let quality_root = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_ROOT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_ROOT to the ClearVoice cache root");
        let input = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_INPUT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_INPUT to a 48 kHz mono WAV");
        let executable = ensure_deep_filter(&quality_root).expect("DeepFilterNet3 should install");
        let work = ScopedDirectory::create(&quality_root.join("temp"), "deepfilter-smoke")
            .expect("smoke work directory should be created");
        let output = run_deep_filter(&executable, &input, &work.path)
            .expect("DeepFilterNet3 inference should run");
        assert!(output.metadata().is_ok_and(|value| value.len() > 44));
    }

    #[test]
    #[ignore = "runs the pinned MossFormer2, DeepFilterNet3 and studio mastering pipeline"]
    fn real_hybrid_runtime_inference() {
        let quality_root = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_ROOT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_ROOT to the ClearVoice cache root");
        let input = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_INPUT")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_INPUT to a 48 kHz mono WAV");
        let ffmpeg = std::env::var_os("ASTRAL_CLEARVOICE_SMOKE_FFMPEG")
            .map(PathBuf::from)
            .expect("set ASTRAL_CLEARVOICE_SMOKE_FFMPEG to ffmpeg.exe");
        let duration_ms = std::env::var("ASTRAL_CLEARVOICE_SMOKE_DURATION_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_500);
        let runtime = ensure_runtime_at(quality_root).expect("ClearVoice runtime should verify");
        ensure_model(&runtime.quality_root).expect("MossFormer2 model should verify");
        let deep_filter =
            ensure_deep_filter(&runtime.quality_root).expect("DeepFilterNet3 should verify");
        let work = ScopedDirectory::create(&runtime.quality_root.join("temp"), "hybrid-smoke")
            .expect("smoke work directory should be created");
        let mossformer_output = work.path.join("mossformer-smoke.wav");
        let mut progress = ProgressEmitter::silent(request().duration_ms);
        run_clearvoice(&runtime, &input, &mossformer_output, &mut progress)
            .expect("MossFormer2 inference should run");
        let output = run_deep_filter(&deep_filter, &mossformer_output, &work.path)
            .expect("DeepFilterNet3 should finish the MossFormer2 output");
        let studio_output = work.path.join("studio-mastering-smoke.wav");
        run_studio_mastering(&ffmpeg, &output, &input, duration_ms, &studio_output)
            .expect("studio mastering should finish the neural output");
        assert!(
            progress.sequence > 1,
            "long MossFormer input should report multiple real windows"
        );
        assert!(output.metadata().is_ok_and(|value| value.len() > 44));
        assert!(studio_output.metadata().is_ok_and(|value| value.len() > 44));
    }

    #[test]
    fn uv_commands_only_use_managed_pinned_python_and_cpu_packages() {
        let venv = uv_venv_args(Path::new("C:\\cache\\runtime"));
        let values = venv
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(values
            .windows(2)
            .any(|pair| pair == ["--python", "3.11.15"]));
        assert!(values.iter().any(|value| value == "--managed-python"));
        assert!(values.iter().any(|value| value == "--allow-existing"));
        assert!(values.iter().any(|value| value == "--no-project"));

        let install = uv_install_args(
            Path::new("C:\\cache\\runtime\\Scripts\\python.exe"),
            Path::new("C:\\cache\\requirements.lock.txt"),
        );
        let values = install
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(values
            .windows(2)
            .any(|pair| pair == ["--torch-backend", "cpu"]));
        assert!(values.iter().any(|value| value == "--no-deps"));
        assert!(values.iter().any(|value| value == "--no-config"));
    }

    #[test]
    fn ffmpeg_filters_trim_then_force_an_exact_sample_count() {
        let input = input_filter(1_250, 81_016);
        assert!(input.starts_with("atrim=start=1.250:duration=81.016"));
        assert!(input.contains("sample_rates=48000:channel_layouts=mono"));
        assert!(input.contains("apad=whole_len=3888768"));
        assert!(input.contains("atrim=end_sample=3888768"));

        let output = output_filter(81_016);
        assert!(output.contains("sample_rates=48000:channel_layouts=stereo"));
        assert!(output.contains("apad=whole_len=3888768"));
        assert!(output.contains("atrim=end_sample=3888768"));
    }

    #[test]
    fn output_wav_validator_rejects_wrong_duration() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "astral-clearvoice-wav-{}-{stamp}.wav",
            std::process::id()
        ));
        write_pcm_wav(&path, 48);
        verified_output_wav(&path, 1).unwrap();
        assert_eq!(
            verified_output_wav(&path, 2).unwrap_err().code,
            "experimental_clearvoice_output_duration_mismatch"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn validation_bounds_experimental_work() {
        let mut input = request();
        input.duration_ms = 0;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "invalid_experimental_clearvoice_duration"
        );
        input.duration_ms = 1;
        input.start_ms = MAX_SOURCE_RANGE_MS;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "invalid_experimental_clearvoice_range"
        );
    }

    #[test]
    fn wire_asset_truthfully_marks_the_engine_experimental() {
        let wire = serde_json::to_value(asset(PathBuf::from("clean.wav"), 1_000, true)).unwrap();
        assert_eq!(wire["engine"], "deepfilter-clearvoice");
        assert_eq!(wire["model"], "DeepFilterNet3+MossFormer2_SE_48K");
        assert_eq!(wire["modelRevision"], HYBRID_MODEL_REVISION);
        assert_eq!(wire["experimental"], true);
        assert_eq!(wire["cacheHit"], true);
    }

    fn write_pcm_wav(path: &Path, samples: u32) {
        let data_bytes = samples * 4;
        let riff_size = 36 + data_bytes;
        let mut file = File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&riff_size.to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&48_000_u32.to_le_bytes()).unwrap();
        file.write_all(&192_000_u32.to_le_bytes()).unwrap();
        file.write_all(&4_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        file.write_all(&vec![0_u8; data_bytes as usize]).unwrap();
    }
}
