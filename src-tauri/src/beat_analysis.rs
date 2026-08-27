use std::{
    fs::{self, File},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::UNIX_EPOCH,
};

use beat_this::{BeatThis, Model, RtenRuntime, Runtime};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::{
    analysis_cancel,
    render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport},
};

const MODEL_NAME: &str = "beat-this-final0-full-v1";
const MODEL_VERSION: &str = "beat-this-rs-1.0.0";
const MODEL_COMMIT: &str = "089b509247e6fdcec666511c0dcf0d5f39c21e73";
const MEL_MODEL_FILE: &str = "mel-spectrogram-089b5092.onnx";
const MEL_MODEL_URL: &str = "https://raw.githubusercontent.com/danigb/beat-this-rs/089b509247e6fdcec666511c0dcf0d5f39c21e73/models/mel_spectrogram.onnx";
const MEL_MODEL_SHA256: &str = "fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9";
const MEL_MODEL_MAX_BYTES: u64 = 512 * 1024;
const BEAT_MODEL_FILE: &str = "beat-this-final0-full-089b5092.onnx";
const BEAT_MODEL_URL: &str =
    "https://github.com/danigb/beat-this-rs/releases/download/model-large/beat_this.onnx";
const BEAT_MODEL_SHA256: &str = "5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02";
const BEAT_MODEL_MAX_BYTES: u64 = 96 * 1024 * 1024;
const CACHE_SCHEMA: &str = "beat-analysis-v1-source-ms";
const SAMPLE_RATE: u64 = 22_050;
const MODEL_FPS: f64 = 50.0;
const ANALYSIS_CHUNK_SECONDS: u64 = 10 * 60;
const ANALYSIS_OVERLAP_SECONDS: u64 = 5;
const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const PCM_READ_SAMPLES: usize = 16 * 1024;
const DUPLICATE_TOLERANCE_MS: u64 = 70;
const STDERR_TAIL_LINES: usize = 28;

type NativeBeatModel = <RtenRuntime as Runtime>::Model;

static ANALYZE_LOCK: Mutex<()> = Mutex::new(());
static TRACKER: OnceLock<Mutex<Option<BeatThis<NativeBeatModel>>>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeBeatsRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeBeatsResponse {
    pub bpm: Option<f64>,
    /// Absolute source-media timestamps in milliseconds.
    pub beats: Vec<u64>,
    /// One probability for every entry in `beats`.
    pub confidences: Vec<f64>,
    /// Absolute source-media downbeat timestamps in milliseconds.
    pub downbeats: Vec<u64>,
    /// One probability for every entry in `downbeats`.
    pub downbeat_confidences: Vec<f64>,
    pub analyzed_start_ms: u64,
    pub analyzed_duration_ms: u64,
    /// Model- and range-independent identity of the canonical source file.
    pub source_fingerprint: String,
    pub model: String,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Copy)]
struct BeatEvent {
    at_ms: u64,
    confidence: f64,
}

#[derive(Debug, Default)]
struct ChunkedAnalysis {
    beats: Vec<BeatEvent>,
    downbeats: Vec<BeatEvent>,
    decoded_samples: u64,
}

#[derive(Debug, Clone)]
struct ModelPaths {
    mel: PathBuf,
    beat: PathBuf,
}

#[tauri::command]
pub async fn analyze_beats(
    app: AppHandle,
    request: AnalyzeBeatsRequest,
) -> Result<AnalyzeBeatsResponse, RenderErrorReport> {
    if analysis_cancel::is_cancelled(request.operation_id.as_deref()) {
        return Err(beat_cancelled_error("before beat analysis start"));
    }
    validate_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());

    tauri::async_runtime::spawn_blocking(move || {
        let _guard = ANALYZE_LOCK.lock().map_err(|_| {
            beat_error(
                "beat_analysis_lock_failed",
                "Beat analizi başlatılamadı.",
                "Beat analysis lock was poisoned",
                true,
            )
        })?;
        analyze_blocking(&app, &ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        beat_error(
            "beat_analysis_worker_failed",
            "Beat analizi tamamlanamadı.",
            format!("Beat analysis worker failed: {error}"),
            true,
        )
    })?
}

fn analyze_blocking(
    app: &AppHandle,
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeBeatsRequest,
) -> Result<AnalyzeBeatsResponse, RenderErrorReport> {
    let cache_dir = analysis_cache_dir(app)?;
    fs::create_dir_all(&cache_dir).map_err(|error| {
        beat_error(
            "beat_analysis_cache_failed",
            "Beat analizi önbelleği hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let source_fingerprint = source_fingerprint(source_path)?;
    let cache_key = analysis_cache_key(&source_fingerprint, request);
    let cache_path = cache_dir.join(format!("{cache_key}.beats.json"));
    if let Some(mut cached) = read_cached_analysis(&cache_path, request, &source_fingerprint)? {
        cached.cache_hit = true;
        return Ok(cached);
    }

    let models = ensure_models(app)?;
    let response = analyze_uncached(
        ffmpeg_path,
        source_path,
        request,
        &models,
        &source_fingerprint,
    )?;
    write_cached_analysis(&cache_path, &response)?;
    Ok(response)
}

fn analyze_uncached(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeBeatsRequest,
    models: &ModelPaths,
    source_fingerprint: &str,
) -> Result<AnalyzeBeatsResponse, RenderErrorReport> {
    with_tracker(models, |tracker| {
        let analysis = decode_and_analyze(ffmpeg_path, source_path, request, tracker)?;
        let expected_samples = duration_samples(request.duration_ms);
        if analysis.decoded_samples.saturating_add(4) < expected_samples {
            return Err(beat_error(
                "beat_analysis_short_audio",
                "Ses aralığı beklenenden kısa çözüldü.",
                format!(
                    "Expected about {expected_samples} PCM samples but decoded {}",
                    analysis.decoded_samples
                ),
                true,
            ));
        }

        let beat_times = analysis
            .beats
            .iter()
            .map(|event| request.start_ms.saturating_add(event.at_ms))
            .collect::<Vec<_>>();
        let confidences = analysis
            .beats
            .iter()
            .map(|event| event.confidence)
            .collect::<Vec<_>>();
        let downbeat_times = analysis
            .downbeats
            .iter()
            .map(|event| request.start_ms.saturating_add(event.at_ms))
            .collect::<Vec<_>>();
        let downbeat_confidences = analysis
            .downbeats
            .iter()
            .map(|event| event.confidence)
            .collect::<Vec<_>>();

        Ok(AnalyzeBeatsResponse {
            bpm: calculate_bpm(&analysis.beats),
            beats: beat_times,
            confidences,
            downbeats: downbeat_times,
            downbeat_confidences,
            analyzed_start_ms: request.start_ms,
            analyzed_duration_ms: request.duration_ms,
            source_fingerprint: source_fingerprint.to_owned(),
            model: MODEL_NAME.to_owned(),
            cache_hit: false,
        })
    })
}

fn with_tracker<T>(
    models: &ModelPaths,
    operation: impl FnOnce(&mut BeatThis<NativeBeatModel>) -> Result<T, RenderErrorReport>,
) -> Result<T, RenderErrorReport> {
    let tracker = TRACKER.get_or_init(|| Mutex::new(None));
    let mut guard = tracker.lock().map_err(|_| {
        beat_error(
            "beat_analysis_model_lock_failed",
            "AI beat modeli kullanılamadı.",
            "Beat model lock was poisoned",
            true,
        )
    })?;
    if guard.is_none() {
        let loaded = BeatThis::new(&RtenRuntime, &models.mel, &models.beat).map_err(|error| {
            beat_error(
                "beat_analysis_model_load_failed",
                "AI beat modeli yüklenemedi.",
                format!("Beat This model load failed: {error:#}"),
                true,
            )
        })?;
        *guard = Some(loaded);
    }
    operation(guard.as_mut().expect("tracker initialized above"))
}

fn decode_and_analyze<M: Model>(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeBeatsRequest,
    tracker: &mut BeatThis<M>,
) -> Result<ChunkedAnalysis, RenderErrorReport> {
    let duration = format_seconds(request.duration_ms);
    let filters = [
        format!(
            "atrim=start={}:duration={duration}",
            format_seconds(request.start_ms)
        ),
        "asetpts=PTS-STARTPTS".to_owned(),
        "aresample=22050".to_owned(),
        "aformat=sample_fmts=flt:sample_rates=22050:channel_layouts=mono".to_owned(),
        format!("apad=pad_dur={duration}"),
        format!("atrim=duration={duration}"),
        "asetpts=PTS-STARTPTS".to_owned(),
    ];
    let mut command = Command::new(ffmpeg_path);
    command
        .args([
            "-hide_banner",
            "-nostats",
            "-nostdin",
            "-loglevel",
            "error",
            "-i",
        ])
        .arg(source_path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af"])
        .arg(filters.join(","))
        .args(["-f", "f32le", "-acodec", "pcm_f32le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let mut child = command.spawn().map_err(|error| {
        beat_error(
            "beat_analysis_ffmpeg_start_failed",
            "Beat analizi için FFmpeg başlatılamadı.",
            format!("Failed to start {}: {error}", ffmpeg_path.display()),
            true,
        )
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        beat_error(
            "beat_analysis_pcm_pipe_failed",
            "Beat analizi ses akışı açılamadı.",
            "FFmpeg stdout pipe was unavailable",
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        beat_error(
            "beat_analysis_pcm_pipe_failed",
            "Beat analizi günlüğü açılamadı.",
            "FFmpeg stderr pipe was unavailable",
            true,
        )
    })?;
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_to_end(&mut bytes);
        bytes
    });

    let mut reader = BufReader::new(stdout);
    let analysis = analyze_pcm_stream(
        &mut reader,
        tracker,
        request.duration_ms,
        request.operation_id.as_deref(),
    );
    drop(reader);
    // A cancelled or failed analysis kills FFmpeg immediately and surfaces the
    // analysis error directly, so a process-exit status cannot mask cancellation.
    if analysis.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = stderr_reader.join();
        return analysis;
    }
    let status = child.wait().map_err(|error| {
        beat_error(
            "beat_analysis_ffmpeg_wait_failed",
            "FFmpeg beat analizi tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let stderr = stderr_reader.join().unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr);
    if !status.success() {
        return Err(beat_error_with_stderr(
            "beat_analysis_ffmpeg_failed",
            "Müziğin sesi beat analizi için çözülemedi.",
            format!("FFmpeg exited with {status}"),
            stderr_tail(&stderr_text),
        ));
    }
    analysis
}

fn analyze_pcm_stream<R: Read, M: Model>(
    reader: &mut R,
    tracker: &mut BeatThis<M>,
    requested_duration_ms: u64,
    operation_id: Option<&str>,
) -> Result<ChunkedAnalysis, RenderErrorReport> {
    let chunk_samples = (ANALYSIS_CHUNK_SECONDS * SAMPLE_RATE) as usize;
    let overlap_samples = (ANALYSIS_OVERLAP_SECONDS * SAMPLE_RATE) as usize;
    let advance_samples = chunk_samples - overlap_samples;
    let expected_samples = duration_samples(requested_duration_ms);
    let maximum_samples = expected_samples.saturating_add(SAMPLE_RATE);
    let mut buffer = Vec::with_capacity(chunk_samples);
    let mut offset_samples = 0_u64;
    let mut first_chunk = true;
    let mut reached_eof = false;
    let mut result = ChunkedAnalysis::default();

    loop {
        if analysis_cancel::is_cancelled(operation_id) {
            return Err(beat_cancelled_error("during beat inference"));
        }
        while buffer.len() < chunk_samples && !reached_eof {
            let wanted = (chunk_samples - buffer.len()).min(PCM_READ_SAMPLES);
            let read = read_pcm_block(reader, &mut buffer, wanted)?;
            if read == 0 {
                reached_eof = true;
            }
            if offset_samples.saturating_add(buffer.len() as u64) > maximum_samples {
                return Err(beat_error(
                    "beat_analysis_pcm_too_long",
                    "FFmpeg beklenenden uzun bir ses akışı üretti.",
                    format!(
                        "PCM exceeded requested duration: offset={offset_samples}, buffered={}",
                        buffer.len()
                    ),
                    false,
                ));
            }
        }

        if buffer.is_empty() {
            break;
        }
        let final_chunk = reached_eof;
        let local = tracker
            .analyze_audio(&buffer, SAMPLE_RATE as u32)
            .map_err(|error| {
                beat_error(
                    "beat_analysis_model_inference_failed",
                    "AI beat modeli bu sesi analiz edemedi.",
                    format!("Beat This inference failed: {error:#}"),
                    true,
                )
            })?;
        append_chunk_events(
            &mut result.beats,
            &local.beats,
            &local.beat_logits,
            offset_samples,
            buffer.len(),
            first_chunk,
            final_chunk,
        );
        append_chunk_events(
            &mut result.downbeats,
            &local.downbeats,
            &local.downbeat_logits,
            offset_samples,
            buffer.len(),
            first_chunk,
            final_chunk,
        );

        if final_chunk {
            result.decoded_samples = offset_samples.saturating_add(buffer.len() as u64);
            break;
        }
        buffer.drain(..advance_samples);
        offset_samples = offset_samples.saturating_add(advance_samples as u64);
        first_chunk = false;
    }

    merge_duplicate_events(&mut result.beats);
    merge_duplicate_events(&mut result.downbeats);
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn append_chunk_events(
    output: &mut Vec<BeatEvent>,
    timestamps_seconds: &[f32],
    logits: &[f32],
    offset_samples: u64,
    chunk_length_samples: usize,
    first_chunk: bool,
    final_chunk: bool,
) {
    let overlap_samples = ANALYSIS_OVERLAP_SECONDS * SAMPLE_RATE;
    let accept_start = if first_chunk { 0 } else { overlap_samples / 2 };
    let accept_end = if final_chunk {
        chunk_length_samples as u64
    } else {
        (chunk_length_samples as u64).saturating_sub(overlap_samples / 2)
    };

    for &seconds in timestamps_seconds {
        if !seconds.is_finite() || seconds < 0.0 {
            continue;
        }
        let local_sample = (f64::from(seconds) * SAMPLE_RATE as f64).round() as u64;
        if local_sample < accept_start || local_sample > accept_end {
            continue;
        }
        let global_sample = offset_samples.saturating_add(local_sample);
        output.push(BeatEvent {
            at_ms: samples_to_ms(global_sample),
            confidence: event_confidence(f64::from(seconds), logits),
        });
    }
}

fn merge_duplicate_events(events: &mut Vec<BeatEvent>) {
    events.sort_by_key(|event| event.at_ms);
    let mut merged: Vec<BeatEvent> = Vec::with_capacity(events.len());
    for event in events.drain(..) {
        if let Some(previous) = merged.last_mut() {
            if event.at_ms.saturating_sub(previous.at_ms) <= DUPLICATE_TOLERANCE_MS {
                if event.confidence > previous.confidence {
                    *previous = event;
                }
                continue;
            }
        }
        merged.push(event);
    }
    *events = merged;
}

fn event_confidence(seconds: f64, logits: &[f32]) -> f64 {
    if logits.is_empty() || !seconds.is_finite() {
        return 0.0;
    }
    let center = (seconds * MODEL_FPS).round().max(0.0) as usize;
    let start = center.saturating_sub(3);
    let end = center.saturating_add(4).min(logits.len());
    logits[start.min(logits.len())..end]
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(sigmoid)
        .fold(0.0, f64::max)
}

fn sigmoid(value: f32) -> f64 {
    let value = f64::from(value).clamp(-60.0, 60.0);
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exponential = value.exp();
        exponential / (1.0 + exponential)
    }
}

fn calculate_bpm(events: &[BeatEvent]) -> Option<f64> {
    let mut intervals = events
        .windows(2)
        .map(|window| window[1].at_ms.saturating_sub(window[0].at_ms))
        .filter(|interval| (100..=3_000).contains(interval))
        .collect::<Vec<_>>();
    if intervals.is_empty() {
        return None;
    }
    intervals.sort_unstable();
    let middle = intervals.len() / 2;
    let median = if intervals.len().is_multiple_of(2) {
        (intervals[middle - 1] as f64 + intervals[middle] as f64) / 2.0
    } else {
        intervals[middle] as f64
    };
    Some(60_000.0 / median)
}

fn read_pcm_block<R: Read>(
    reader: &mut R,
    output: &mut Vec<f32>,
    wanted_samples: usize,
) -> Result<usize, RenderErrorReport> {
    if wanted_samples == 0 {
        return Ok(0);
    }
    let mut bytes = vec![0_u8; wanted_samples.saturating_mul(4)];
    let mut filled = 0;
    while filled < bytes.len() {
        match reader.read(&mut bytes[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(beat_error(
                    "beat_analysis_pcm_read_failed",
                    "Beat analizi ses örneklerini okuyamadı.",
                    error.to_string(),
                    true,
                ));
            }
        }
    }
    if filled % 4 != 0 {
        return Err(beat_error(
            "beat_analysis_pcm_invalid",
            "FFmpeg geçersiz ses örnekleri üretti.",
            format!("PCM byte count {filled} is not divisible by four"),
            false,
        ));
    }
    let count = filled / 4;
    output.reserve(count);
    for chunk in bytes[..filled].chunks_exact(4) {
        let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if !value.is_finite() {
            return Err(beat_error(
                "beat_analysis_pcm_invalid",
                "Ses akışında geçersiz örnek bulundu.",
                "PCM stream contains a non-finite f32 sample",
                false,
            ));
        }
        output.push(value);
    }
    Ok(count)
}

fn ensure_models(app: &AppHandle) -> Result<ModelPaths, RenderErrorReport> {
    let directory = app
        .path()
        .app_cache_dir()
        .map(|path| path.join("audio-models").join(MODEL_VERSION))
        .map_err(|error| {
            beat_error(
                "beat_analysis_model_directory_failed",
                "AI beat modeli klasörü hazırlanamadı.",
                error.to_string(),
                true,
            )
        })?;
    fs::create_dir_all(&directory).map_err(|error| {
        beat_error(
            "beat_analysis_model_directory_failed",
            "AI beat modeli klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let mel = directory.join(MEL_MODEL_FILE);
    ensure_model_file(
        &mel,
        MEL_MODEL_URL,
        MEL_MODEL_SHA256,
        MEL_MODEL_MAX_BYTES,
        "mel",
    )?;
    let beat = directory.join(BEAT_MODEL_FILE);
    ensure_model_file(
        &beat,
        BEAT_MODEL_URL,
        BEAT_MODEL_SHA256,
        BEAT_MODEL_MAX_BYTES,
        "beat",
    )?;
    Ok(ModelPaths { mel, beat })
}

fn ensure_model_file(
    destination: &Path,
    url: &str,
    expected_sha256: &str,
    maximum_bytes: u64,
    label: &str,
) -> Result<(), RenderErrorReport> {
    if model_matches(destination, expected_sha256, maximum_bytes)? {
        return Ok(());
    }
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("beat-model.onnx");
    let partial = destination.with_file_name(format!(".{file_name}.partial"));
    if partial.is_file() {
        let _ = fs::remove_file(&partial);
    }

    let download_result = (|| {
        let mut response = ureq::get(url).call().map_err(|error| {
            beat_error(
                "beat_analysis_model_download_failed",
                "AI beat modeli indirilemedi. İnternet bağlantısını kontrol edin.",
                format!("Failed to download {label} model: {error}"),
                true,
            )
        })?;
        let mut file = File::create(&partial).map_err(|error| {
            beat_error(
                "beat_analysis_model_write_failed",
                "AI beat modeli diske yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        let downloaded = io::copy(
            &mut response
                .body_mut()
                .as_reader()
                .take(maximum_bytes.saturating_add(1)),
            &mut file,
        )
        .map_err(|error| {
            beat_error(
                "beat_analysis_model_download_failed",
                "AI beat modeli indirilemedi.",
                error.to_string(),
                true,
            )
        })?;
        if downloaded > maximum_bytes {
            return Err(beat_error(
                "beat_analysis_model_too_large",
                "AI beat modelinin güvenlik doğrulaması başarısız oldu.",
                format!("{label} model exceeded {maximum_bytes} bytes"),
                false,
            ));
        }
        file.flush().map_err(|error| {
            beat_error(
                "beat_analysis_model_write_failed",
                "AI beat modeli diske yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        file.sync_all().map_err(|error| {
            beat_error(
                "beat_analysis_model_write_failed",
                "AI beat modeli diske yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        drop(file);
        if !model_matches(&partial, expected_sha256, maximum_bytes)? {
            return Err(beat_error(
                "beat_analysis_model_checksum_failed",
                "AI beat modelinin güvenlik doğrulaması başarısız oldu.",
                format!("Expected SHA-256 {expected_sha256} for {label} model"),
                true,
            ));
        }
        if destination.is_file() {
            fs::remove_file(destination).map_err(|error| {
                beat_error(
                    "beat_analysis_model_commit_failed",
                    "AI beat modeli kurulamadı.",
                    error.to_string(),
                    true,
                )
            })?;
        }
        fs::rename(&partial, destination).map_err(|error| {
            beat_error(
                "beat_analysis_model_commit_failed",
                "AI beat modeli kurulamadı.",
                error.to_string(),
                true,
            )
        })?;
        Ok(())
    })();
    if download_result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    download_result
}

fn model_matches(
    path: &Path,
    expected_sha256: &str,
    maximum_bytes: u64,
) -> Result<bool, RenderErrorReport> {
    let metadata = match path.metadata() {
        Ok(metadata) if metadata.is_file() && metadata.len() <= maximum_bytes => metadata,
        Ok(_) | Err(_) => return Ok(false),
    };
    if metadata.len() == 0 {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|error| {
        beat_error(
            "beat_analysis_model_read_failed",
            "AI beat modeli okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            beat_error(
                "beat_analysis_model_read_failed",
                "AI beat modeli okunamadı.",
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

fn analysis_cache_dir(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join("beat-analysis"))
        .map_err(|error| {
            beat_error(
                "beat_analysis_cache_failed",
                "Beat analizi önbelleği bulunamadı.",
                error.to_string(),
                true,
            )
        })
}

fn source_fingerprint(source_path: &Path) -> Result<String, RenderErrorReport> {
    let canonical = fs::canonicalize(source_path).map_err(|error| {
        beat_error(
            "missing_media",
            "Beat analizi yapılacak medya bulunamadı.",
            error.to_string(),
            true,
        )
    })?;
    let metadata = canonical.metadata().map_err(|error| {
        beat_error(
            "missing_media",
            "Beat analizi yapılacak medya bulunamadı.",
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
    hash_field(&mut hasher, b"astral-source-fingerprint-v1");
    hash_field(&mut hasher, canonical.to_string_lossy().as_bytes());
    hash_field(&mut hasher, &metadata.len().to_le_bytes());
    hash_field(&mut hasher, &modified.to_le_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn analysis_cache_key(source_fingerprint: &str, request: &AnalyzeBeatsRequest) -> String {
    let mut hasher = Sha256::new();
    for value in [
        CACHE_SCHEMA.as_bytes(),
        MODEL_COMMIT.as_bytes(),
        MEL_MODEL_SHA256.as_bytes(),
        BEAT_MODEL_SHA256.as_bytes(),
        source_fingerprint.as_bytes(),
        &request.start_ms.to_le_bytes(),
        &request.duration_ms.to_le_bytes(),
    ] {
        hash_field(&mut hasher, value);
    }
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn read_cached_analysis(
    path: &Path,
    request: &AnalyzeBeatsRequest,
    expected_source_fingerprint: &str,
) -> Result<Option<AnalyzeBeatsResponse>, RenderErrorReport> {
    let metadata = match path.metadata() {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_CACHE_BYTES => metadata,
        Ok(_) | Err(_) => return Ok(None),
    };
    if metadata.len() == 0 {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|error| {
        beat_error(
            "beat_analysis_cache_read_failed",
            "Beat analizi önbelleği okunamadı.",
            error.to_string(),
            true,
        )
    })?;
    let Ok(response) = serde_json::from_slice::<AnalyzeBeatsResponse>(&bytes) else {
        return Ok(None);
    };
    if validate_cached_response(&response, request, expected_source_fingerprint) {
        Ok(Some(response))
    } else {
        Ok(None)
    }
}

fn write_cached_analysis(
    path: &Path,
    response: &AnalyzeBeatsResponse,
) -> Result<(), RenderErrorReport> {
    let bytes = serde_json::to_vec(response).map_err(|error| {
        beat_error(
            "beat_analysis_cache_encode_failed",
            "Beat analizi önbelleğe alınamadı.",
            error.to_string(),
            false,
        )
    })?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return Err(beat_error(
            "beat_analysis_cache_too_large",
            "Beat analizi sonucu önbelleğe alınamayacak kadar büyük.",
            format!("Serialized beat analysis used {} bytes", bytes.len()),
            false,
        ));
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("analysis.beats.json");
    let partial = path.with_file_name(format!(".{file_name}.partial"));
    if partial.is_file() {
        let _ = fs::remove_file(&partial);
    }
    let write_result = (|| {
        let mut file = File::create(&partial).map_err(|error| {
            beat_error(
                "beat_analysis_cache_write_failed",
                "Beat analizi önbelleğe yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        file.write_all(&bytes).map_err(|error| {
            beat_error(
                "beat_analysis_cache_write_failed",
                "Beat analizi önbelleğe yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        file.flush().map_err(|error| {
            beat_error(
                "beat_analysis_cache_write_failed",
                "Beat analizi önbelleğe yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        file.sync_all().map_err(|error| {
            beat_error(
                "beat_analysis_cache_write_failed",
                "Beat analizi önbelleğe yazılamadı.",
                error.to_string(),
                true,
            )
        })?;
        drop(file);
        if path.is_file() {
            fs::remove_file(path).map_err(|error| {
                beat_error(
                    "beat_analysis_cache_commit_failed",
                    "Beat analizi önbelleğe alınamadı.",
                    error.to_string(),
                    true,
                )
            })?;
        }
        fs::rename(&partial, path).map_err(|error| {
            beat_error(
                "beat_analysis_cache_commit_failed",
                "Beat analizi önbelleğe alınamadı.",
                error.to_string(),
                true,
            )
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    write_result
}

fn validate_cached_response(
    response: &AnalyzeBeatsResponse,
    request: &AnalyzeBeatsRequest,
    expected_source_fingerprint: &str,
) -> bool {
    let range_end = request.start_ms.saturating_add(request.duration_ms);
    response.model == MODEL_NAME
        && response.analyzed_start_ms == request.start_ms
        && response.analyzed_duration_ms == request.duration_ms
        && response.source_fingerprint == expected_source_fingerprint
        && response.beats.len() == response.confidences.len()
        && response.downbeats.len() == response.downbeat_confidences.len()
        && sorted_in_range(&response.beats, request.start_ms, range_end)
        && sorted_in_range(&response.downbeats, request.start_ms, range_end)
        && response
            .confidences
            .iter()
            .chain(&response.downbeat_confidences)
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        && response
            .bpm
            .is_none_or(|value| value.is_finite() && value > 0.0 && value <= 600.0)
}

fn sorted_in_range(values: &[u64], start: u64, end: u64) -> bool {
    values.iter().all(|value| *value >= start && *value <= end)
        && values.windows(2).all(|window| window[0] < window[1])
}

fn validate_request(request: &AnalyzeBeatsRequest) -> Result<(), RenderErrorReport> {
    if request.duration_ms == 0 || request.duration_ms > MAX_DURATION_MS {
        return Err(beat_error(
            "invalid_beat_analysis_duration",
            "Beat analizi yapılacak süre geçersiz.",
            format!("durationMs must be between 1 and {MAX_DURATION_MS}"),
            false,
        ));
    }
    if request.start_ms > MAX_DURATION_MS
        || request.start_ms.saturating_add(request.duration_ms) > MAX_DURATION_MS
    {
        return Err(beat_error(
            "invalid_beat_analysis_range",
            "Beat analizi aralığı desteklenen sınırın dışında.",
            format!(
                "startMs + durationMs must not exceed {MAX_DURATION_MS}; got {} + {}",
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
        return Err(beat_error(
            "missing_media",
            "Beat analizi yapılacak medya bulunamadı.",
            format!("Not a readable file: {}", path.display()),
            true,
        ));
    }
    fs::canonicalize(&path).map_err(|error| {
        beat_error(
            "missing_media",
            "Beat analizi yapılacak medya açılamadı.",
            error.to_string(),
            true,
        )
    })
}

fn duration_samples(duration_ms: u64) -> u64 {
    duration_ms.saturating_mul(SAMPLE_RATE).div_ceil(1_000)
}

fn samples_to_ms(samples: u64) -> u64 {
    samples
        .saturating_mul(1_000)
        .saturating_add(SAMPLE_RATE / 2)
        / SAMPLE_RATE
}

fn format_seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1_000, milliseconds % 1_000)
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

fn beat_error(
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

fn beat_cancelled_error(stage: &str) -> RenderErrorReport {
    beat_error(
        "smart_shorts_cancelled",
        "Akıllı Shorts analizi iptal edildi.",
        format!("Beat analysis cancelled {stage}"),
        false,
    )
}

fn beat_error_with_stderr(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    stderr_tail: Vec<String>,
) -> RenderErrorReport {
    RenderErrorReport {
        code: code.into(),
        user_message: user_message.into(),
        technical_message: technical_message.into(),
        retryable: true,
        stderr_tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn request() -> AnalyzeBeatsRequest {
        AnalyzeBeatsRequest {
            source_path: "music.wav".into(),
            start_ms: 12_000,
            duration_ms: 90_000,
            ffmpeg_path: None,
            operation_id: None,
        }
    }

    fn response() -> AnalyzeBeatsResponse {
        AnalyzeBeatsResponse {
            bpm: Some(120.0),
            beats: vec![12_250, 12_750, 13_250],
            confidences: vec![0.9, 0.8, 0.85],
            downbeats: vec![12_250],
            downbeat_confidences: vec![0.88],
            analyzed_start_ms: 12_000,
            analyzed_duration_ms: 90_000,
            source_fingerprint: "source-fingerprint".into(),
            model: MODEL_NAME.into(),
            cache_hit: false,
        }
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "astral-beat-{label}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn response_uses_stable_camel_case_json_and_parallel_confidences() {
        let value = serde_json::to_value(response()).unwrap();
        assert_eq!(value["analyzedStartMs"], 12_000);
        assert_eq!(value["analyzedDurationMs"], 90_000);
        assert_eq!(value["sourceFingerprint"], "source-fingerprint");
        assert_eq!(value["cacheHit"], false);
        assert_eq!(value["confidences"].as_array().unwrap().len(), 3);
        assert_eq!(value["downbeatConfidences"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn source_range_mapping_is_absolute_and_sample_accurate() {
        let local_sample = (2.5 * SAMPLE_RATE as f64) as u64;
        assert_eq!(12_000 + samples_to_ms(local_sample), 14_500);
        assert_eq!(duration_samples(90_000), 1_984_500);
    }

    #[test]
    fn confidence_uses_the_strongest_nearby_model_frame() {
        let mut logits = vec![-8.0_f32; 100];
        logits[51] = 3.0;
        let confidence = event_confidence(1.0, &logits);
        assert!((confidence - sigmoid(3.0)).abs() < 1e-9);
        assert!(confidence > 0.95);
    }

    #[test]
    fn bpm_uses_the_median_interval_and_ignores_impossible_gaps() {
        let events = vec![
            BeatEvent {
                at_ms: 0,
                confidence: 1.0,
            },
            BeatEvent {
                at_ms: 500,
                confidence: 1.0,
            },
            BeatEvent {
                at_ms: 1_000,
                confidence: 1.0,
            },
            BeatEvent {
                at_ms: 10_000,
                confidence: 1.0,
            },
        ];
        assert_eq!(calculate_bpm(&events), Some(120.0));
    }

    #[test]
    fn cache_json_round_trips_and_rejects_mismatched_ranges() {
        let directory = unique_test_dir("cache-json");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("analysis.json");
        write_cached_analysis(&path, &response()).unwrap();
        let cached = read_cached_analysis(&path, &request(), "source-fingerprint")
            .unwrap()
            .unwrap();
        assert_eq!(cached, response());
        let mut other = request();
        other.start_ms += 1;
        assert!(read_cached_analysis(&path, &other, "source-fingerprint")
            .unwrap()
            .is_none());
        assert!(read_cached_analysis(&path, &request(), "stale-fingerprint")
            .unwrap()
            .is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cache_key_changes_with_source_metadata_and_range() {
        let directory = unique_test_dir("cache-key");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.wav");
        fs::write(&source, b"first").unwrap();
        let first_fingerprint = source_fingerprint(&source).unwrap();
        let first = analysis_cache_key(&first_fingerprint, &request());
        let mut changed_range = request();
        changed_range.start_ms += 100;
        let second = analysis_cache_key(&first_fingerprint, &changed_range);
        assert_ne!(first, second);
        assert_eq!(first_fingerprint, source_fingerprint(&source).unwrap());
        fs::write(&source, b"a longer replacement").unwrap();
        let replacement_fingerprint = source_fingerprint(&source).unwrap();
        let third = analysis_cache_key(&replacement_fingerprint, &request());
        assert_ne!(first_fingerprint, replacement_fingerprint);
        assert_ne!(first, third);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn model_checksum_and_size_are_both_verified() {
        let directory = unique_test_dir("model-hash");
        fs::create_dir_all(&directory).unwrap();
        let model = directory.join("model.onnx");
        fs::write(&model, b"astral").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"astral"));
        assert!(model_matches(&model, &expected, 64).unwrap());
        assert!(!model_matches(&model, &expected, 3).unwrap());
        assert!(!model_matches(&model, &"0".repeat(64), 64).unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn partial_pcm_reads_are_reassembled_without_losing_samples() {
        struct TwoByteReader {
            bytes: Vec<u8>,
            offset: usize,
        }
        impl Read for TwoByteReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.offset >= self.bytes.len() {
                    return Ok(0);
                }
                let count = 2.min(buffer.len()).min(self.bytes.len() - self.offset);
                buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
                self.offset += count;
                Ok(count)
            }
        }
        let samples = [0.25_f32, -0.5, 1.0];
        let bytes = samples
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let mut reader = TwoByteReader { bytes, offset: 0 };
        let mut decoded = Vec::new();
        assert_eq!(read_pcm_block(&mut reader, &mut decoded, 3).unwrap(), 3);
        assert_eq!(decoded, samples);
    }

    #[test]
    fn validation_rejects_empty_and_overflowing_ranges() {
        let mut input = request();
        input.duration_ms = 0;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "invalid_beat_analysis_duration"
        );
        input.duration_ms = 1_000;
        input.start_ms = MAX_DURATION_MS;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "invalid_beat_analysis_range"
        );
    }

    #[test]
    #[ignore = "downloads the pinned 83 MB Beat This model and requires ASTRAL_FFMPEG_PATH"]
    fn real_beat_this_smoke_keeps_markers_through_ninety_seconds() {
        let ffmpeg = PathBuf::from(
            std::env::var_os("ASTRAL_FFMPEG_PATH")
                .expect("ASTRAL_FFMPEG_PATH must point to the downloaded FFmpeg runtime"),
        );
        assert!(ffmpeg.is_file(), "missing FFmpeg at {}", ffmpeg.display());
        // Reusing an explicitly supplied directory makes an interrupted, large-model
        // smoke test resumable without weakening production cache semantics.
        let directory = std::env::var_os("ASTRAL_BEAT_SMOKE_REUSE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| unique_test_dir("real-smoke"));
        fs::create_dir_all(&directory).unwrap();
        let mel = directory.join(MEL_MODEL_FILE);
        let beat = directory.join(BEAT_MODEL_FILE);
        ensure_model_file(
            &mel,
            MEL_MODEL_URL,
            MEL_MODEL_SHA256,
            MEL_MODEL_MAX_BYTES,
            "mel",
        )
        .unwrap();
        ensure_model_file(
            &beat,
            BEAT_MODEL_URL,
            BEAT_MODEL_SHA256,
            BEAT_MODEL_MAX_BYTES,
            "beat",
        )
        .unwrap();

        let source = directory.join("metronome-90s.wav");
        let generated = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=if(lt(mod(t\\,0.5)\\,0.06)\\,0.9*sin(2*PI*80*t)*exp(-55*mod(t\\,0.5))\\,0):s=22050:d=90",
                "-c:a",
                "pcm_s16le",
                "-loglevel",
                "error",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let input = AnalyzeBeatsRequest {
            source_path: source.to_string_lossy().into_owned(),
            start_ms: 0,
            duration_ms: 90_000,
            ffmpeg_path: None,
            operation_id: None,
        };
        let fingerprint = source_fingerprint(&source).unwrap();
        let result = analyze_uncached(
            &ffmpeg,
            &source,
            &input,
            &ModelPaths { mel, beat },
            &fingerprint,
        )
        .unwrap();
        assert!(!result.beats.is_empty(), "model returned no beat markers");
        assert!(
            result.beats.last().copied().unwrap_or_default() > 80_000,
            "beat markers stopped early: {:?}",
            result.beats.last()
        );
        assert_eq!(result.beats.len(), result.confidences.len());
        assert_eq!(result.source_fingerprint, fingerprint);
        assert_eq!(result.model, MODEL_NAME);
        fs::remove_dir_all(directory).unwrap();
    }
}
