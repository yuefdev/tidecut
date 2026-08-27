use std::{
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{
    analysis_cancel,
    render::{configure_background_process, resolve_ffmpeg_path, RenderErrorReport},
};

const MODEL_NAME: &str = "silero-vad-v6";
const SILERO_MODEL: &[u8] = include_bytes!("../models/silero_vad.onnx");
const SAMPLE_RATE: usize = 16_000;
const FRAME_SAMPLES: usize = 512;
const CONTEXT_SAMPLES: usize = 64;
const STATE_VALUES: usize = 2 * 128;
const FRAME_MS: u64 = 32;
const START_THRESHOLD: f32 = 0.5;
const END_THRESHOLD: f32 = 0.35;
const MIN_SPEECH_MS: u64 = 250;
const MIN_SILENCE_MS: u64 = 100;
const SPEECH_PAD_MS: u64 = 30;
const RMS_WINDOW_FRAMES: usize = 6;
const ATTACK_MS: f64 = 80.0;
const RELEASE_MS: f64 = 400.0;
const MIN_GAIN_DB: f64 = -18.0;
const MAX_GAIN_DB: f64 = 12.0;
const RDP_TOLERANCE_DB: f64 = 0.35;
const MAX_OUTPUT_POINTS: usize = 4_000;
const MAX_REQUEST_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;
const STDERR_TAIL_LINES: usize = 24;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeVoiceRiderRequest {
    pub source_path: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub playback_rate: f64,
    pub base_volume: f64,
    #[serde(default)]
    pub volume_keyframes: Vec<VoiceRiderVolumeKeyframe>,
    #[serde(default = "default_target_lufs")]
    pub target_lufs: f64,
    #[serde(default = "default_true_peak_db")]
    pub true_peak_db: f64,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeSpeechActivityRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f64,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRiderVolumeKeyframe {
    pub at_ms: u64,
    pub value: f64,
    #[serde(default = "default_easing")]
    pub easing: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeVoiceRiderResponse {
    pub model: &'static str,
    pub points: Vec<VoiceRiderPoint>,
    pub speech_segments: Vec<VoiceRiderSpeechSegment>,
    pub speech_coverage: f64,
    pub average_speech_probability: f64,
    pub strongest_cut_db: f64,
    pub strongest_boost_db: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeSpeechActivityResponse {
    pub model: &'static str,
    pub duration_ms: u64,
    pub speech_segments: Vec<VoiceRiderSpeechSegment>,
    pub speech_coverage: f64,
    pub average_speech_probability: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AutomaticCleanupProfile {
    pub speech_detected: bool,
    pub speech_coverage: f64,
    pub estimated_snr_db: f64,
    pub strength: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRiderPoint {
    pub at_ms: u64,
    pub gain: f64,
    pub speech_probability: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRiderSpeechSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
struct FrameStats {
    at_ms: u64,
    end_ms: u64,
    speech_probability: f32,
    sum_squares: f64,
    sample_count: usize,
    peak: f64,
}

#[derive(Debug, Clone, Copy)]
struct EnvelopePoint {
    at_ms: u64,
    gain_db: f64,
    speech_probability: f64,
}

#[derive(Debug, Clone)]
struct ManualVolumeEnvelope {
    base_volume: f64,
    frames: Vec<VoiceRiderVolumeKeyframe>,
}

struct PcmChunk {
    samples: [f32; FRAME_SAMPLES],
    valid_samples: usize,
}

struct SileroVad {
    session: Session,
    state: Vec<f32>,
    context: [f32; CONTEXT_SAMPLES],
}

#[tauri::command]
pub async fn analyze_voice_rider(
    app: AppHandle,
    request: AnalyzeVoiceRiderRequest,
) -> Result<AnalyzeVoiceRiderResponse, RenderErrorReport> {
    validate_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());

    tauri::async_runtime::spawn_blocking(move || {
        analyze_voice_rider_with_paths(&ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        rider_error(
            "voice_rider_worker_failed",
            "AI Voice Rider analizi tamamlanamadı.",
            format!("Voice Rider worker failed: {error}"),
            true,
        )
    })?
}

#[tauri::command]
pub async fn analyze_speech_activity(
    app: AppHandle,
    request: AnalyzeSpeechActivityRequest,
) -> Result<AnalyzeSpeechActivityResponse, RenderErrorReport> {
    if analysis_cancel::is_cancelled(request.operation_id.as_deref()) {
        return Err(analysis_cancelled_error("before speech activity start"));
    }
    validate_speech_activity_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());

    tauri::async_runtime::spawn_blocking(move || {
        analyze_speech_activity_with_paths(&ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        rider_error(
            "speech_activity_worker_failed",
            "Konuşma ve sessizlik analizi tamamlanamadı.",
            format!("Speech activity worker failed: {error}"),
            true,
        )
    })?
}

fn analyze_speech_activity_with_paths(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeSpeechActivityRequest,
) -> Result<AnalyzeSpeechActivityResponse, RenderErrorReport> {
    let expected_duration_ms =
        ((request.duration_ms as f64 / request.playback_rate).round() as u64).max(1);
    let frames = decode_and_infer(
        ffmpeg_path,
        source_path,
        request.start_ms,
        request.duration_ms,
        request.playback_rate,
        expected_duration_ms,
        None,
        request.operation_id.as_deref(),
    )?;
    if frames.is_empty() {
        return Err(rider_error(
            "speech_activity_empty_audio",
            "Klipte analiz edilebilir ses akışı bulunamadı.",
            "FFmpeg produced no PCM samples for speech activity analysis",
            false,
        ));
    }
    Ok(build_speech_activity_response(
        &frames,
        expected_duration_ms,
    ))
}

pub(crate) fn analyze_automatic_cleanup_profile_with_paths(
    ffmpeg_path: &Path,
    source_path: &Path,
    start_ms: u64,
    duration_ms: u64,
) -> Result<AutomaticCleanupProfile, RenderErrorReport> {
    let frames = decode_and_infer(
        ffmpeg_path,
        source_path,
        start_ms,
        duration_ms,
        1.0,
        duration_ms.max(1),
        None,
        None,
    )?;
    if frames.is_empty() {
        return Err(rider_error(
            "audio_cleanup_empty_audio",
            "Klipte temizlenebilir bir ses akışı bulunamadı.",
            "FFmpeg produced no PCM samples for automatic cleanup analysis",
            false,
        ));
    }
    Ok(build_automatic_cleanup_profile(&frames, duration_ms.max(1)))
}

fn analyze_voice_rider_with_paths(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AnalyzeVoiceRiderRequest,
) -> Result<AnalyzeVoiceRiderResponse, RenderErrorReport> {
    let expected_duration_ms =
        ((request.duration_ms as f64 / request.playback_rate).round() as u64).max(1);
    let manual_volume = ManualVolumeEnvelope::new(request);
    let frames = decode_and_infer(
        ffmpeg_path,
        source_path,
        request.start_ms,
        request.duration_ms,
        request.playback_rate,
        expected_duration_ms,
        Some(&manual_volume),
        None,
    )?;
    if frames.is_empty() {
        return Err(rider_error(
            "voice_rider_empty_audio",
            "Klipte analiz edilebilir ses bulunamadı.",
            "FFmpeg produced no PCM samples for Voice Rider",
            false,
        ));
    }

    let actual_duration_ms = frames.last().map(|frame| frame.end_ms).unwrap_or(0);
    let analysis_duration_ms = actual_duration_ms.min(expected_duration_ms).max(1);
    let speech_segments = detect_speech_segments(&frames, analysis_duration_ms);
    if speech_segments.is_empty() {
        return Err(rider_error(
            "voice_rider_no_speech",
            "Bu klipte dengelenecek konuşma algılanmadı.",
            "Silero VAD did not find a speech segment of at least 250ms",
            false,
        ));
    }

    let envelope = extend_envelope_through_silent_tail(
        build_gain_envelope(
            &frames,
            &speech_segments,
            analysis_duration_ms,
            request.target_lufs,
            request.true_peak_db,
        ),
        analysis_duration_ms,
        expected_duration_ms,
    );
    let strongest_cut_db = envelope
        .iter()
        .map(|point| point.gain_db)
        .fold(0.0_f64, f64::min);
    let strongest_boost_db = envelope
        .iter()
        .map(|point| point.gain_db)
        .fold(0.0_f64, f64::max);
    let simplified = cap_envelope_points(
        simplify_envelope(&envelope, RDP_TOLERANCE_DB),
        MAX_OUTPUT_POINTS,
    );

    let speech_duration_ms = speech_segments
        .iter()
        .map(|segment| segment.end_ms.saturating_sub(segment.start_ms))
        .sum::<u64>();
    let (probability_sum, probability_count) = frames
        .iter()
        .filter(|frame| {
            speech_segments
                .iter()
                .any(|segment| frame.end_ms > segment.start_ms && frame.at_ms < segment.end_ms)
        })
        .fold((0.0, 0_usize), |(sum, count), frame| {
            (sum + f64::from(frame.speech_probability), count + 1)
        });

    Ok(AnalyzeVoiceRiderResponse {
        model: MODEL_NAME,
        points: simplified
            .into_iter()
            .map(|point| VoiceRiderPoint {
                at_ms: point.at_ms,
                gain: 10_f64.powf(point.gain_db / 20.0).clamp(0.0, 4.0),
                speech_probability: point.speech_probability.clamp(0.0, 1.0),
            })
            .collect(),
        speech_segments,
        speech_coverage: (speech_duration_ms as f64 / expected_duration_ms as f64).clamp(0.0, 1.0),
        average_speech_probability: if probability_count == 0 {
            0.0
        } else {
            (probability_sum / probability_count as f64).clamp(0.0, 1.0)
        },
        strongest_cut_db,
        strongest_boost_db,
    })
}

fn decode_and_infer(
    ffmpeg_path: &Path,
    source_path: &Path,
    start_ms: u64,
    duration_ms: u64,
    playback_rate: f64,
    expected_duration_ms: u64,
    manual_volume: Option<&ManualVolumeEnvelope>,
    operation_id: Option<&str>,
) -> Result<Vec<FrameStats>, RenderErrorReport> {
    let mut filters = vec![
        format!(
            "atrim=start={}:duration={}",
            format_seconds(start_ms),
            format_seconds(duration_ms)
        ),
        "asetpts=PTS-STARTPTS".to_owned(),
    ];
    filters.extend(atempo_filters(playback_rate));
    filters.extend([
        "aresample=16000".to_owned(),
        "aformat=sample_fmts=flt:sample_rates=16000:channel_layouts=mono".to_owned(),
    ]);

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
        rider_error(
            "voice_rider_ffmpeg_start_failed",
            "AI ses analizi için FFmpeg başlatılamadı.",
            format!("Failed to start {}: {error}", ffmpeg_path.display()),
            true,
        )
    })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        rider_error(
            "voice_rider_pcm_pipe_failed",
            "Ses analiz akışı açılamadı.",
            "FFmpeg stdout pipe was unavailable",
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        rider_error(
            "voice_rider_pcm_pipe_failed",
            "Ses analiz günlüğü açılamadı.",
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

    let mut vad = match SileroVad::new() {
        Ok(vad) => vad,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(error);
        }
    };
    let mut reader = BufReader::new(stdout);
    let analysis = analyze_pcm_stream(
        &mut reader,
        &mut vad,
        expected_duration_ms,
        manual_volume,
        operation_id,
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
        rider_error(
            "voice_rider_ffmpeg_wait_failed",
            "FFmpeg ses analizi tamamlanamadı.",
            error.to_string(),
            true,
        )
    })?;
    let stderr = stderr_reader.join().unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr);
    if !status.success() {
        return Err(rider_error_with_stderr(
            "voice_rider_ffmpeg_failed",
            "Klibin sesi AI analizi için çözülemedi.",
            format!("FFmpeg exited with {status}"),
            stderr_tail(&stderr_text),
        ));
    }
    analysis
}

fn analyze_pcm_stream<R: Read>(
    reader: &mut R,
    vad: &mut SileroVad,
    expected_duration_ms: u64,
    manual_volume: Option<&ManualVolumeEnvelope>,
    operation_id: Option<&str>,
) -> Result<Vec<FrameStats>, RenderErrorReport> {
    let mut frames = Vec::new();
    let mut sample_offset = 0_u64;
    let mut frame_index = 0_u64;
    while let Some(chunk) = read_pcm_chunk(reader)? {
        if frame_index % 8 == 0 && analysis_cancel::is_cancelled(operation_id) {
            return Err(analysis_cancelled_error("during Silero speech activity"));
        }
        frame_index += 1;
        let probability = vad.predict(&chunk.samples)?;
        let at_ms = sample_offset.saturating_mul(1_000) / SAMPLE_RATE as u64;
        let end_sample = sample_offset.saturating_add(chunk.valid_samples as u64);
        let end_ms = end_sample
            .saturating_mul(1_000)
            .div_ceil(SAMPLE_RATE as u64)
            .min(expected_duration_ms);
        let center_ms = at_ms.saturating_add(end_ms.saturating_sub(at_ms) / 2);
        let manual_gain = manual_volume
            .map(|envelope| envelope.evaluate(center_ms))
            .unwrap_or(1.0)
            .clamp(0.0, 4.0);
        let mut sum_squares = 0.0;
        let mut peak = 0.0_f64;
        for sample in chunk.samples.iter().take(chunk.valid_samples) {
            let value = f64::from(*sample) * manual_gain;
            sum_squares += value * value;
            peak = peak.max(value.abs());
        }
        frames.push(FrameStats {
            at_ms,
            end_ms,
            speech_probability: probability,
            sum_squares,
            sample_count: chunk.valid_samples,
            peak,
        });
        sample_offset = end_sample;
        if chunk.valid_samples < FRAME_SAMPLES || end_ms >= expected_duration_ms {
            break;
        }
    }
    Ok(frames)
}

fn read_pcm_chunk<R: Read>(reader: &mut R) -> Result<Option<PcmChunk>, RenderErrorReport> {
    let mut bytes = [0_u8; FRAME_SAMPLES * 4];
    let mut filled = 0;
    while filled < bytes.len() {
        match reader.read(&mut bytes[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(rider_error(
                    "voice_rider_pcm_read_failed",
                    "Ses örnekleri okunamadı.",
                    error.to_string(),
                    true,
                ));
            }
        }
    }
    if filled == 0 {
        return Ok(None);
    }
    if filled % 4 != 0 {
        return Err(rider_error(
            "voice_rider_pcm_invalid",
            "FFmpeg geçersiz ses örneği üretti.",
            format!("PCM byte count {filled} is not divisible by four"),
            false,
        ));
    }

    let valid_samples = filled / 4;
    let mut samples = [0.0_f32; FRAME_SAMPLES];
    for (index, target) in samples.iter_mut().take(valid_samples).enumerate() {
        let offset = index * 4;
        *target = f32::from_le_bytes(bytes[offset..offset + 4].try_into().map_err(|_| {
            rider_error(
                "voice_rider_pcm_invalid",
                "FFmpeg ses örneği ayrıştırılamadı.",
                "Failed to read four-byte PCM sample",
                false,
            )
        })?);
        if !target.is_finite() {
            return Err(rider_error(
                "voice_rider_pcm_invalid",
                "Ses akışında geçersiz örnek bulundu.",
                "PCM stream contains a non-finite f32 sample",
                false,
            ));
        }
    }
    Ok(Some(PcmChunk {
        samples,
        valid_samples,
    }))
}

impl SileroVad {
    fn new() -> Result<Self, RenderErrorReport> {
        let builder = Session::builder().map_err(model_load_error)?;
        let builder = builder.with_intra_threads(1).map_err(model_load_error)?;
        let builder = builder.with_inter_threads(1).map_err(model_load_error)?;
        let mut builder = builder
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(model_load_error)?;
        let session = builder
            .commit_from_memory(SILERO_MODEL)
            .map_err(model_load_error)?;
        Ok(Self {
            session,
            state: vec![0.0; STATE_VALUES],
            context: [0.0; CONTEXT_SAMPLES],
        })
    }

    fn predict(&mut self, samples: &[f32; FRAME_SAMPLES]) -> Result<f32, RenderErrorReport> {
        let mut input = Vec::with_capacity(CONTEXT_SAMPLES + FRAME_SAMPLES);
        input.extend_from_slice(&self.context);
        input.extend_from_slice(samples);
        let input = Tensor::from_array(([1_usize, CONTEXT_SAMPLES + FRAME_SAMPLES], input))
            .map_err(model_inference_error)?;
        let state = Tensor::from_array(([2_usize, 1, 128], self.state.clone()))
            .map_err(model_inference_error)?;
        let sample_rate = Tensor::from_array(([1_usize], vec![SAMPLE_RATE as i64]))
            .map_err(model_inference_error)?;

        let outputs = self
            .session
            .run(ort::inputs! {
                "input" => input,
                "state" => state,
                "sr" => sample_rate,
            })
            .map_err(model_inference_error)?;
        if outputs.len() < 2 {
            return Err(rider_error(
                "voice_rider_model_output_invalid",
                "AI konuşma modeli geçersiz sonuç üretti.",
                format!(
                    "Silero VAD returned {} outputs; expected two",
                    outputs.len()
                ),
                false,
            ));
        }
        let (_, probabilities) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(model_inference_error)?;
        let (_, next_state) = outputs[1]
            .try_extract_tensor::<f32>()
            .map_err(model_inference_error)?;
        let probability = *probabilities.first().ok_or_else(|| {
            rider_error(
                "voice_rider_model_output_invalid",
                "AI konuşma modeli boş sonuç üretti.",
                "Silero VAD probability tensor was empty",
                false,
            )
        })?;
        if next_state.len() != STATE_VALUES || !probability.is_finite() {
            return Err(rider_error(
                "voice_rider_model_output_invalid",
                "AI konuşma modeli geçersiz sonuç üretti.",
                format!(
                    "Silero state length was {}, probability was {probability}",
                    next_state.len()
                ),
                false,
            ));
        }
        self.state.copy_from_slice(next_state);
        self.context
            .copy_from_slice(&samples[FRAME_SAMPLES - CONTEXT_SAMPLES..]);
        Ok(probability.clamp(0.0, 1.0))
    }
}

fn model_load_error<T>(error: ort::Error<T>) -> RenderErrorReport {
    rider_error(
        "voice_rider_model_load_failed",
        "AI konuşma modeli yüklenemedi.",
        format!("Failed to load embedded Silero VAD model: {error}"),
        false,
    )
}

fn model_inference_error(error: ort::Error) -> RenderErrorReport {
    rider_error(
        "voice_rider_model_inference_failed",
        "AI konuşma analizi çalıştırılamadı.",
        error.to_string(),
        false,
    )
}

fn detect_speech_segments(frames: &[FrameStats], duration_ms: u64) -> Vec<VoiceRiderSpeechSegment> {
    let mut raw_segments = Vec::new();
    let mut speech_start = None;
    let mut silence_start = None;

    for frame in frames {
        if speech_start.is_none() {
            if frame.speech_probability >= START_THRESHOLD {
                speech_start = Some(frame.at_ms);
                silence_start = None;
            }
            continue;
        }

        if frame.speech_probability < END_THRESHOLD {
            let start = *silence_start.get_or_insert(frame.at_ms);
            if frame.end_ms.saturating_sub(start) >= MIN_SILENCE_MS {
                let speech_end = start;
                if speech_end.saturating_sub(speech_start.unwrap_or(0)) >= MIN_SPEECH_MS {
                    raw_segments.push((speech_start.unwrap_or(0), speech_end));
                }
                speech_start = None;
                silence_start = None;
            }
        } else {
            silence_start = None;
        }
    }

    if let Some(start) = speech_start {
        if duration_ms.saturating_sub(start) >= MIN_SPEECH_MS {
            raw_segments.push((start, duration_ms));
        }
    }

    let mut padded: Vec<(u64, u64)> = Vec::new();
    for (start, end) in raw_segments {
        let next = (
            start.saturating_sub(SPEECH_PAD_MS),
            end.saturating_add(SPEECH_PAD_MS).min(duration_ms),
        );
        if let Some(last) = padded.last_mut().filter(|last| next.0 <= last.1) {
            last.1 = last.1.max(next.1);
        } else {
            padded.push(next);
        }
    }

    padded
        .into_iter()
        .map(|(start_ms, end_ms)| {
            let (sum, count) = frames
                .iter()
                .filter(|frame| frame.end_ms > start_ms && frame.at_ms < end_ms)
                .fold((0.0, 0_usize), |(sum, count), frame| {
                    (sum + f64::from(frame.speech_probability), count + 1)
                });
            VoiceRiderSpeechSegment {
                start_ms,
                end_ms,
                confidence: if count == 0 {
                    0.0
                } else {
                    (sum / count as f64).clamp(0.0, 1.0)
                },
            }
        })
        .collect()
}

fn build_speech_activity_response(
    frames: &[FrameStats],
    expected_duration_ms: u64,
) -> AnalyzeSpeechActivityResponse {
    let actual_duration_ms = frames
        .last()
        .map(|frame| frame.end_ms)
        .unwrap_or(0)
        .min(expected_duration_ms);
    let speech_segments = detect_speech_segments(frames, actual_duration_ms);
    let speech_duration_ms = speech_segments
        .iter()
        .map(|segment| segment.end_ms.saturating_sub(segment.start_ms))
        .sum::<u64>();
    let average_speech_probability = if frames.is_empty() {
        0.0
    } else {
        (frames
            .iter()
            .map(|frame| f64::from(frame.speech_probability))
            .sum::<f64>()
            / frames.len() as f64)
            .clamp(0.0, 1.0)
    };
    AnalyzeSpeechActivityResponse {
        model: MODEL_NAME,
        duration_ms: expected_duration_ms,
        speech_segments,
        speech_coverage: if expected_duration_ms == 0 {
            0.0
        } else {
            (speech_duration_ms as f64 / expected_duration_ms as f64).clamp(0.0, 1.0)
        },
        average_speech_probability,
    }
}

fn build_automatic_cleanup_profile(
    frames: &[FrameStats],
    duration_ms: u64,
) -> AutomaticCleanupProfile {
    let segments = detect_speech_segments(frames, duration_ms);
    let speech_duration_ms = segments
        .iter()
        .map(|segment| segment.end_ms.saturating_sub(segment.start_ms))
        .sum::<u64>();
    let minimum_speech_ms = duration_ms.min(800).max(MIN_SPEECH_MS);
    let speech_detected = speech_duration_ms >= minimum_speech_ms;
    let speech_coverage = (speech_duration_ms as f64 / duration_ms.max(1) as f64).clamp(0.0, 1.0);

    let mut speech_levels = frames
        .iter()
        .filter(|frame| frame.speech_probability >= 0.55)
        .filter_map(frame_rms_dbfs)
        .collect::<Vec<_>>();
    let mut noise_levels = frames
        .iter()
        .filter(|frame| frame.speech_probability <= 0.20)
        .filter_map(frame_rms_dbfs)
        .collect::<Vec<_>>();
    let speech_level = percentile(&mut speech_levels, 0.5).unwrap_or(-30.0);
    // The upper quartile represents persistent fan/room noise without letting
    // long digital silence make a noisy recording appear artificially clean.
    let noise_level = percentile(&mut noise_levels, 0.75).unwrap_or(-60.0);
    let estimated_snr_db = (speech_level - noise_level).clamp(-20.0, 60.0);

    // FFmpeg arnndn's `mix` is a wet/dry blend, not an abstract intensity.
    // Values below 1.0 deliberately reintroduce the original motor/fan/noise.
    // Keep SNR as useful telemetry, but render detected speech full-wet.
    let strength: f64 = if speech_detected { 1.0 } else { 0.0 };

    AutomaticCleanupProfile {
        speech_detected,
        speech_coverage,
        estimated_snr_db,
        strength: (strength * 1_000.0).round() / 1_000.0,
    }
}

fn frame_rms_dbfs(frame: &FrameStats) -> Option<f64> {
    if frame.sample_count == 0 || frame.sum_squares <= 1e-12 {
        return None;
    }
    Some(10.0 * (frame.sum_squares / frame.sample_count as f64).log10())
}

fn percentile(values: &mut [f64], quantile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).round() as usize;
    values.get(index).copied()
}

fn build_gain_envelope(
    frames: &[FrameStats],
    speech_segments: &[VoiceRiderSpeechSegment],
    duration_ms: u64,
    target_lufs: f64,
    true_peak_db: f64,
) -> Vec<EnvelopePoint> {
    let target_rms_dbfs = target_lufs - 3.0;
    let mut points = Vec::with_capacity(frames.len() + 2);
    points.push(EnvelopePoint {
        at_ms: 0,
        gain_db: 0.0,
        speech_probability: frames
            .first()
            .map_or(0.0, |frame| f64::from(frame.speech_probability)),
    });

    let mut smoothed_gain_db = 0.0_f64;
    for (index, frame) in frames.iter().enumerate() {
        let center_ms = frame
            .at_ms
            .saturating_add(frame.end_ms.saturating_sub(frame.at_ms) / 2)
            .min(duration_ms);
        let speech_active = speech_segments
            .iter()
            .any(|segment| center_ms >= segment.start_ms && center_ms < segment.end_ms);
        let window_start = index.saturating_sub(RMS_WINDOW_FRAMES);
        let window_end = (index + RMS_WINDOW_FRAMES + 1).min(frames.len());
        let (sum_squares, sample_count, peak) = frames[window_start..window_end].iter().fold(
            (0.0, 0_usize, 0.0_f64),
            |(sum, count, peak), item| {
                (
                    sum + item.sum_squares,
                    count + item.sample_count,
                    peak.max(item.peak),
                )
            },
        );
        let rms_dbfs = if sample_count == 0 || sum_squares <= 1e-12 {
            -120.0
        } else {
            10.0 * (sum_squares / sample_count as f64).log10()
        };
        let peak_dbfs = if peak <= 1e-9 {
            -120.0
        } else {
            20.0 * peak.log10()
        };
        let mut desired_gain_db = if speech_active && rms_dbfs > -55.0 {
            (target_rms_dbfs - rms_dbfs).clamp(MIN_GAIN_DB, MAX_GAIN_DB)
        } else {
            0.0
        };
        if speech_active {
            // Sample peaks can sit slightly below the reconstructed true peak;
            // reserve another half dB before applying the requested ceiling.
            let local_peak_cap_db = true_peak_db - peak_dbfs - 0.5;
            desired_gain_db = desired_gain_db.min(local_peak_cap_db);
        }

        let time_constant = if desired_gain_db < smoothed_gain_db {
            ATTACK_MS
        } else {
            RELEASE_MS
        };
        let alpha = 1.0 - (-(FRAME_MS as f64) / time_constant).exp();
        smoothed_gain_db += alpha * (desired_gain_db - smoothed_gain_db);
        if speech_active {
            let local_peak_cap_db = true_peak_db - peak_dbfs - 0.5;
            smoothed_gain_db = smoothed_gain_db.min(local_peak_cap_db);
        } else {
            // Silence and music may retain a short release from a cut, but are
            // never lifted above their original level.
            smoothed_gain_db = smoothed_gain_db.min(0.0);
        }
        smoothed_gain_db = smoothed_gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        points.push(EnvelopePoint {
            at_ms: center_ms,
            gain_db: smoothed_gain_db,
            speech_probability: f64::from(frame.speech_probability),
        });
    }

    let speech_reaches_end = speech_segments
        .last()
        .is_some_and(|segment| segment.end_ms >= duration_ms);
    points.push(EnvelopePoint {
        at_ms: duration_ms,
        gain_db: if speech_reaches_end {
            smoothed_gain_db
        } else {
            smoothed_gain_db.min(0.0)
        },
        speech_probability: frames
            .last()
            .map_or(0.0, |frame| f64::from(frame.speech_probability)),
    });
    dedupe_envelope_points(points)
}

fn extend_envelope_through_silent_tail(
    mut points: Vec<EnvelopePoint>,
    actual_duration_ms: u64,
    expected_duration_ms: u64,
) -> Vec<EnvelopePoint> {
    if expected_duration_ms <= actual_duration_ms {
        return points;
    }

    // Container duration commonly follows video while AAC/HE-AAC ends one or
    // more packets earlier. The missing tail is silence, not a decode error.
    // Return the rider to unity over its normal release time and keep an exact
    // clip endpoint so preview and export automation stay aligned.
    let release_end_ms = actual_duration_ms
        .saturating_add(RELEASE_MS.round() as u64)
        .min(expected_duration_ms);
    points.push(EnvelopePoint {
        at_ms: release_end_ms,
        gain_db: 0.0,
        speech_probability: 0.0,
    });
    if release_end_ms < expected_duration_ms {
        points.push(EnvelopePoint {
            at_ms: expected_duration_ms,
            gain_db: 0.0,
            speech_probability: 0.0,
        });
    }
    dedupe_envelope_points(points)
}

fn simplify_envelope(points: &[EnvelopePoint], tolerance_db: f64) -> Vec<EnvelopePoint> {
    if points.len() <= 2 || tolerance_db <= 0.0 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut ranges = vec![(0_usize, points.len() - 1)];
    while let Some((start, end)) = ranges.pop() {
        if end <= start + 1 {
            continue;
        }
        let left = points[start];
        let right = points[end];
        let span = right.at_ms.saturating_sub(left.at_ms);
        let mut maximum_error = 0.0;
        let mut maximum_index = None;
        for (index, point) in points.iter().enumerate().take(end).skip(start + 1) {
            let progress = if span == 0 {
                0.0
            } else {
                point.at_ms.saturating_sub(left.at_ms) as f64 / span as f64
            };
            let interpolated = left.gain_db + (right.gain_db - left.gain_db) * progress;
            let error = (point.gain_db - interpolated).abs();
            if error > maximum_error {
                maximum_error = error;
                maximum_index = Some(index);
            }
        }
        if maximum_error > tolerance_db {
            if let Some(index) = maximum_index {
                keep[index] = true;
                ranges.push((start, index));
                ranges.push((index, end));
            }
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

fn cap_envelope_points(points: Vec<EnvelopePoint>, maximum: usize) -> Vec<EnvelopePoint> {
    if points.len() <= maximum || maximum < 2 {
        return points;
    }
    let last_index = points.len() - 1;
    let mut capped = Vec::with_capacity(maximum);
    let mut previous_index = usize::MAX;
    for slot in 0..maximum {
        let index = ((slot as f64 * last_index as f64) / (maximum - 1) as f64).round() as usize;
        if index != previous_index {
            capped.push(points[index]);
            previous_index = index;
        }
    }
    if capped.last().map(|point| point.at_ms) != points.last().map(|point| point.at_ms) {
        if capped.len() == maximum {
            capped.pop();
        }
        capped.push(points[last_index]);
    }
    capped
}

fn dedupe_envelope_points(mut points: Vec<EnvelopePoint>) -> Vec<EnvelopePoint> {
    points.sort_by_key(|point| point.at_ms);
    let mut deduped: Vec<EnvelopePoint> = Vec::with_capacity(points.len());
    for point in points {
        if let Some(previous) = deduped.last_mut().filter(|item| item.at_ms == point.at_ms) {
            *previous = point;
        } else {
            deduped.push(point);
        }
    }
    deduped
}

impl ManualVolumeEnvelope {
    fn new(request: &AnalyzeVoiceRiderRequest) -> Self {
        let mut frames = request.volume_keyframes.clone();
        // Stable sorting preserves the timeline adapter's "last keyframe wins"
        // behavior when malformed/legacy data contains duplicate timestamps.
        frames.sort_by_key(|frame| frame.at_ms);
        let mut deduped: Vec<VoiceRiderVolumeKeyframe> = Vec::with_capacity(frames.len());
        for frame in frames {
            if let Some(previous) = deduped.last_mut().filter(|item| item.at_ms == frame.at_ms) {
                *previous = frame;
            } else {
                deduped.push(frame);
            }
        }
        Self {
            base_volume: request.base_volume,
            frames: deduped,
        }
    }

    fn evaluate(&self, at_ms: u64) -> f64 {
        if self.frames.is_empty() {
            return self.base_volume;
        }
        if at_ms <= self.frames[0].at_ms {
            return self.frames[0].value;
        }
        let last = self.frames.last().expect("volume frames cannot be empty");
        if at_ms >= last.at_ms {
            return last.value;
        }
        let right_index = self.frames.partition_point(|frame| frame.at_ms <= at_ms);
        let left = &self.frames[right_index - 1];
        let right = &self.frames[right_index];
        if left.easing == "hold" || right.at_ms == left.at_ms {
            return left.value;
        }
        let progress = (at_ms - left.at_ms) as f64 / (right.at_ms - left.at_ms) as f64;
        let eased = match left.easing.as_str() {
            "ease-in" => progress * progress,
            "ease-out" => 1.0 - (1.0 - progress) * (1.0 - progress),
            "ease-in-out" if progress < 0.5 => 2.0 * progress * progress,
            "ease-in-out" => 1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0,
            _ => progress,
        };
        left.value + (right.value - left.value) * eased
    }
}

fn validate_request(request: &AnalyzeVoiceRiderRequest) -> Result<(), RenderErrorReport> {
    if request.source_path.trim().is_empty() {
        return Err(rider_error(
            "missing_source",
            "Kaynak medya seçilmedi.",
            "sourcePath is empty",
            false,
        ));
    }
    if request.duration_ms == 0 || request.duration_ms > MAX_REQUEST_DURATION_MS {
        return Err(rider_error(
            "invalid_voice_rider_duration",
            "AI Voice Rider süresi geçersiz.",
            format!(
                "durationMs must be between 1 and {MAX_REQUEST_DURATION_MS}; got {}",
                request.duration_ms
            ),
            false,
        ));
    }
    if !request.playback_rate.is_finite() || !(0.05..=16.0).contains(&request.playback_rate) {
        return Err(rider_error(
            "invalid_voice_rider_speed",
            "Klip hızı AI ses analizi için geçersiz.",
            format!("playbackRate was {}", request.playback_rate),
            false,
        ));
    }
    if !request.base_volume.is_finite() || !(0.0..=4.0).contains(&request.base_volume) {
        return Err(rider_error(
            "invalid_voice_rider_volume",
            "Klip ses seviyesi geçersiz.",
            format!("baseVolume was {}", request.base_volume),
            false,
        ));
    }
    let is_fully_muted = if request.volume_keyframes.is_empty() {
        request.base_volume == 0.0
    } else {
        request
            .volume_keyframes
            .iter()
            .all(|frame| frame.value == 0.0)
    };
    if is_fully_muted {
        return Err(rider_error(
            "voice_rider_muted",
            "Klip sesi tamamen kapalı. AI dengelemeden önce sesi açın.",
            "Base volume and every volume keyframe are zero",
            false,
        ));
    }
    if !request.target_lufs.is_finite() || !(-30.0..=-10.0).contains(&request.target_lufs) {
        return Err(rider_error(
            "invalid_voice_rider_target",
            "Hedef konuşma seviyesi geçersiz.",
            format!("targetLufs was {}", request.target_lufs),
            false,
        ));
    }
    if !request.true_peak_db.is_finite() || !(-9.0..=0.0).contains(&request.true_peak_db) {
        return Err(rider_error(
            "invalid_voice_rider_peak",
            "True-peak güvenlik hedefi geçersiz.",
            format!("truePeakDb was {}", request.true_peak_db),
            false,
        ));
    }
    if request.volume_keyframes.len() > 20_000 {
        return Err(rider_error(
            "voice_rider_too_many_keyframes",
            "Klipte analiz edilemeyecek kadar fazla ses otomasyonu var.",
            format!(
                "volumeKeyframes contains {} points",
                request.volume_keyframes.len()
            ),
            false,
        ));
    }
    let local_duration_ms =
        ((request.duration_ms as f64 / request.playback_rate).round() as u64).max(1);
    for frame in &request.volume_keyframes {
        if frame.at_ms > local_duration_ms
            || !frame.value.is_finite()
            || !(0.0..=4.0).contains(&frame.value)
            || !matches!(
                frame.easing.as_str(),
                "linear" | "hold" | "ease-in" | "ease-out" | "ease-in-out"
            )
        {
            return Err(rider_error(
                "invalid_voice_rider_keyframe",
                "Klip ses otomasyonunda geçersiz bir nokta var.",
                format!(
                    "Invalid volume keyframe at {}ms, value {}, easing '{}'",
                    frame.at_ms, frame.value, frame.easing
                ),
                false,
            ));
        }
    }
    Ok(())
}

fn validate_speech_activity_request(
    request: &AnalyzeSpeechActivityRequest,
) -> Result<(), RenderErrorReport> {
    if request.source_path.trim().is_empty() {
        return Err(rider_error(
            "missing_source",
            "Kaynak medya seçilmedi.",
            "sourcePath is empty",
            false,
        ));
    }
    if request.duration_ms == 0 || request.duration_ms > MAX_REQUEST_DURATION_MS {
        return Err(rider_error(
            "invalid_speech_activity_duration",
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
        return Err(rider_error(
            "invalid_speech_activity_range",
            "Konuşma analizi aralığı desteklenen sınırın dışında.",
            format!(
                "startMs + durationMs must not exceed {MAX_REQUEST_DURATION_MS}; got {} + {}",
                request.start_ms, request.duration_ms
            ),
            false,
        ));
    }
    if !request.playback_rate.is_finite() || !(0.05..=16.0).contains(&request.playback_rate) {
        return Err(rider_error(
            "invalid_speech_activity_speed",
            "Klip hızı konuşma analizi için geçersiz.",
            format!("playbackRate was {}", request.playback_rate),
            false,
        ));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    let path = PathBuf::from(value);
    let metadata = path.metadata().map_err(|error| {
        rider_error(
            "missing_media",
            "Kaynak medya bulunamadı. Medyayı yeniden bağlayın.",
            error.to_string(),
            true,
        )
    })?;
    if !metadata.is_file() {
        return Err(rider_error(
            "invalid_source",
            "Kaynak medya bir dosya değil.",
            format!("Not a file: {}", path.display()),
            false,
        ));
    }
    fs::canonicalize(path).map_err(|error| {
        rider_error(
            "missing_media",
            "Kaynak medya açılamadı.",
            error.to_string(),
            true,
        )
    })
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
        .map(|factor| format!("atempo={}", format_number(factor)))
        .collect()
}

fn format_seconds(duration_ms: u64) -> String {
    format!("{}.{:03}", duration_ms / 1_000, duration_ms % 1_000)
}

fn format_number(value: f64) -> String {
    let formatted = format!("{value:.6}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn default_target_lufs() -> f64 {
    -16.0
}

fn default_playback_rate() -> f64 {
    1.0
}

fn default_true_peak_db() -> f64 {
    -1.5
}

fn default_easing() -> String {
    "linear".to_owned()
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

fn rider_error(
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

fn analysis_cancelled_error(stage: &str) -> RenderErrorReport {
    rider_error(
        "smart_shorts_cancelled",
        "Akıllı Shorts analizi iptal edildi.",
        format!("Speech activity analysis cancelled {stage}"),
        false,
    )
}

fn rider_error_with_stderr(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    stderr_tail: Vec<String>,
) -> RenderErrorReport {
    RenderErrorReport {
        code: code.into(),
        user_message: user_message.into(),
        technical_message: technical_message.into(),
        retryable: false,
        stderr_tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn request() -> AnalyzeVoiceRiderRequest {
        AnalyzeVoiceRiderRequest {
            source_path: "voice.wav".to_owned(),
            start_ms: 0,
            duration_ms: 2_000,
            playback_rate: 1.0,
            base_volume: 1.0,
            volume_keyframes: Vec::new(),
            target_lufs: -16.0,
            true_peak_db: -1.5,
            ffmpeg_path: None,
        }
    }

    fn frames(probabilities: &[f32]) -> Vec<FrameStats> {
        probabilities
            .iter()
            .enumerate()
            .map(|(index, probability)| FrameStats {
                at_ms: index as u64 * FRAME_MS,
                end_ms: (index as u64 + 1) * FRAME_MS,
                speech_probability: *probability,
                sum_squares: 512.0 * 0.01,
                sample_count: 512,
                peak: 0.2,
            })
            .collect()
    }

    fn set_frame_level(frame: &mut FrameStats, dbfs: f64) {
        frame.sum_squares = frame.sample_count as f64 * 10_f64.powf(dbfs / 10.0);
        frame.peak = 10_f64.powf(dbfs / 20.0).min(1.0);
    }

    #[test]
    fn request_defaults_use_voice_loudness_targets() {
        let parsed: AnalyzeVoiceRiderRequest = serde_json::from_value(serde_json::json!({
            "sourcePath": "voice.wav",
            "startMs": 0,
            "durationMs": 1000,
            "playbackRate": 1.0,
            "baseVolume": 1.0,
            "volumeKeyframes": []
        }))
        .unwrap();
        assert_eq!(parsed.target_lufs, -16.0);
        assert_eq!(parsed.true_peak_db, -1.5);
    }

    #[test]
    fn muted_validation_follows_the_effective_manual_volume_envelope() {
        let mut input = request();
        input.base_volume = 0.0;
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "voice_rider_muted"
        );

        input.base_volume = 1.0;
        input.volume_keyframes = vec![VoiceRiderVolumeKeyframe {
            at_ms: 0,
            value: 0.0,
            easing: "linear".to_owned(),
        }];
        assert_eq!(
            validate_request(&input).unwrap_err().code,
            "voice_rider_muted"
        );

        input.base_volume = 0.0;
        input.volume_keyframes.push(VoiceRiderVolumeKeyframe {
            at_ms: 1_000,
            value: 1.0,
            easing: "linear".to_owned(),
        });
        assert!(validate_request(&input).is_ok());
    }

    #[test]
    fn speech_postprocessing_uses_hysteresis_minimum_duration_and_padding() {
        let mut probabilities = vec![0.05; 4];
        probabilities.extend([0.8; 10]);
        probabilities.extend([0.2; 3]);
        probabilities.extend([0.75; 3]);
        probabilities.extend([0.1; 4]);
        let input = frames(&probabilities);
        let segments = detect_speech_segments(&input, input.last().unwrap().end_ms);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_ms, 4 * FRAME_MS - SPEECH_PAD_MS);
        assert_eq!(segments[0].end_ms, 20 * FRAME_MS + SPEECH_PAD_MS);
        assert!(segments[0].confidence > 0.4);
    }

    #[test]
    fn short_speech_bursts_are_rejected() {
        let mut probabilities = vec![0.05; 2];
        probabilities.extend([0.9; 4]);
        probabilities.extend([0.05; 5]);
        let input = frames(&probabilities);
        assert!(detect_speech_segments(&input, input.last().unwrap().end_ms).is_empty());
    }

    #[test]
    fn speech_activity_accepts_all_silence_with_an_empty_segment_list() {
        let input = frames(&[0.01; 50]);
        let duration_ms = input.last().unwrap().end_ms;
        let response = build_speech_activity_response(&input, duration_ms);
        assert!(response.speech_segments.is_empty());
        assert_eq!(response.speech_coverage, 0.0);
        assert!(response.average_speech_probability < 0.02);
        assert_eq!(response.duration_ms, duration_ms);
    }

    #[test]
    fn automatic_cleanup_is_full_wet_for_speech_and_skips_non_speech() {
        let probabilities = [vec![0.05; 10], vec![0.92; 30], vec![0.05; 10]].concat();
        let mut clean = frames(&probabilities);
        for (index, frame) in clean.iter_mut().enumerate() {
            set_frame_level(
                frame,
                if (10..40).contains(&index) {
                    -20.0
                } else {
                    -65.0
                },
            );
        }
        let mut noisy = clean.clone();
        for (index, frame) in noisy.iter_mut().enumerate() {
            if !(10..40).contains(&index) {
                set_frame_level(frame, -25.0);
            }
        }
        let duration_ms = probabilities.len() as u64 * FRAME_MS;
        let clean_profile = build_automatic_cleanup_profile(&clean, duration_ms);
        let noisy_profile = build_automatic_cleanup_profile(&noisy, duration_ms);

        assert!(clean_profile.speech_detected);
        assert!(noisy_profile.speech_detected);
        assert_eq!(clean_profile.strength, 1.0);
        assert_eq!(noisy_profile.strength, 1.0);
        assert!(noisy_profile.estimated_snr_db < clean_profile.estimated_snr_db);

        let mut non_speech = frames(&vec![0.05; 40]);
        for frame in &mut non_speech {
            set_frame_level(frame, -12.0);
        }
        let non_speech_profile = build_automatic_cleanup_profile(&non_speech, 40 * FRAME_MS);
        assert!(!non_speech_profile.speech_detected);
        assert_eq!(non_speech_profile.strength, 0.0);
    }

    #[test]
    fn speech_activity_request_defaults_to_normal_speed() {
        let parsed: AnalyzeSpeechActivityRequest = serde_json::from_value(serde_json::json!({
            "sourcePath": "voice.wav",
            "durationMs": 1000
        }))
        .unwrap();
        assert_eq!(parsed.start_ms, 0);
        assert_eq!(parsed.playback_rate, 1.0);
        assert!(validate_speech_activity_request(&parsed).is_ok());
    }

    #[test]
    fn gain_envelope_cuts_loud_speech_boosts_quiet_speech_and_never_boosts_silence() {
        let mut input = frames(&[0.05; 60]);
        for frame in input.iter_mut().take(45).skip(4) {
            frame.speech_probability = 0.9;
        }
        for frame in input.iter_mut().take(14).skip(4) {
            frame.sum_squares = frame.sample_count as f64 * 0.25;
            frame.peak = 0.8;
        }
        for frame in input.iter_mut().take(45).skip(14) {
            frame.sum_squares = frame.sample_count as f64 * 0.0004;
            frame.peak = 0.05;
        }
        let segments = vec![VoiceRiderSpeechSegment {
            start_ms: 4 * FRAME_MS,
            end_ms: 45 * FRAME_MS,
            confidence: 0.9,
        }];
        let envelope =
            build_gain_envelope(&input, &segments, input.last().unwrap().end_ms, -16.0, -1.5);
        assert!(envelope.iter().any(|point| point.gain_db < -1.0));
        assert!(envelope.iter().any(|point| point.gain_db > 0.1));
        assert!(envelope
            .iter()
            .filter(|point| point.at_ms > 48 * FRAME_MS)
            .all(|point| point.gain_db <= 0.0));
    }

    #[test]
    fn shorter_audio_tail_returns_to_unity_and_keeps_the_clip_endpoint() {
        let original = vec![
            EnvelopePoint {
                at_ms: 0,
                gain_db: 0.0,
                speech_probability: 0.9,
            },
            EnvelopePoint {
                at_ms: 81_012,
                gain_db: -6.0,
                speech_probability: 0.8,
            },
        ];
        let extended = extend_envelope_through_silent_tail(original, 81_012, 81_128);
        assert_eq!(extended[extended.len() - 2].at_ms, 81_012);
        assert_eq!(extended.last().unwrap().at_ms, 81_128);
        assert_eq!(extended.last().unwrap().gain_db, 0.0);
        assert_eq!(extended.last().unwrap().speech_probability, 0.0);

        let unchanged = extend_envelope_through_silent_tail(extended.clone(), 81_128, 81_128);
        assert_eq!(unchanged.len(), extended.len());
    }

    #[test]
    fn rdp_simplification_stays_within_db_tolerance_and_keeps_endpoints() {
        let points = (0..100)
            .map(|index| EnvelopePoint {
                at_ms: index * 10,
                gain_db: index as f64 * 0.01 + ((index % 3) as f64 - 1.0) * 0.02,
                speech_probability: 0.8,
            })
            .collect::<Vec<_>>();
        let simplified = simplify_envelope(&points, 0.05);
        assert_eq!(simplified.first().unwrap().at_ms, 0);
        assert_eq!(simplified.last().unwrap().at_ms, 990);
        assert!(simplified.len() < points.len() / 2);

        for original in &points {
            let right = simplified.partition_point(|point| point.at_ms < original.at_ms);
            if right == 0 || right == simplified.len() {
                continue;
            }
            let left = simplified[right - 1];
            let right = simplified[right];
            let progress = (original.at_ms - left.at_ms) as f64 / (right.at_ms - left.at_ms) as f64;
            let interpolated = left.gain_db + (right.gain_db - left.gain_db) * progress;
            assert!((original.gain_db - interpolated).abs() <= 0.05 + 1e-9);
        }
    }

    #[test]
    fn hard_cap_keeps_first_and_last_envelope_points() {
        let points = (0..10_000)
            .map(|index| EnvelopePoint {
                at_ms: index,
                gain_db: index as f64 / 1_000.0,
                speech_probability: 0.5,
            })
            .collect::<Vec<_>>();
        let capped = cap_envelope_points(points, MAX_OUTPUT_POINTS);
        assert_eq!(capped.len(), MAX_OUTPUT_POINTS);
        assert_eq!(capped.first().unwrap().at_ms, 0);
        assert_eq!(capped.last().unwrap().at_ms, 9_999);
    }

    #[test]
    fn pcm_parser_reads_little_endian_f32_and_pads_the_last_frame() {
        let expected = [0.25_f32, -0.5, 1.0];
        let bytes = expected
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let chunk = read_pcm_chunk(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert_eq!(chunk.valid_samples, expected.len());
        assert_eq!(&chunk.samples[..expected.len()], &expected);
        assert!(chunk.samples[expected.len()..]
            .iter()
            .all(|sample| *sample == 0.0));
    }

    #[test]
    fn manual_volume_matches_timeline_keyframe_easing() {
        let mut request = request();
        request.volume_keyframes = vec![
            VoiceRiderVolumeKeyframe {
                at_ms: 0,
                value: 0.5,
                easing: "linear".to_owned(),
            },
            VoiceRiderVolumeKeyframe {
                at_ms: 1_000,
                value: 1.5,
                easing: "linear".to_owned(),
            },
        ];
        let envelope = ManualVolumeEnvelope::new(&request);
        assert!((envelope.evaluate(500) - 1.0).abs() < 1e-9);
        assert_eq!(envelope.evaluate(2_000), 1.5);
    }

    #[test]
    fn duplicate_manual_volume_keyframes_keep_the_last_timeline_value() {
        let mut request = request();
        request.volume_keyframes = vec![
            VoiceRiderVolumeKeyframe {
                at_ms: 500,
                value: 0.25,
                easing: "linear".to_owned(),
            },
            VoiceRiderVolumeKeyframe {
                at_ms: 500,
                value: 1.75,
                easing: "hold".to_owned(),
            },
        ];
        let envelope = ManualVolumeEnvelope::new(&request);
        assert_eq!(envelope.evaluate(500), 1.75);
    }

    #[test]
    #[ignore = "loads the real embedded ONNX Runtime model"]
    fn embedded_silero_model_runs_real_inference() {
        let mut vad = SileroVad::new().unwrap();
        let probability = vad.predict(&[0.0; FRAME_SAMPLES]).unwrap();
        assert!(probability.is_finite());
        assert!((0.0..=1.0).contains(&probability));
    }

    #[test]
    #[ignore = "set ASTRAL_VOICE_RIDER_SMOKE_SOURCE to a real speech file"]
    fn real_ffmpeg_and_silero_smoke() {
        let source = PathBuf::from(
            std::env::var("ASTRAL_VOICE_RIDER_SMOKE_SOURCE")
                .expect("ASTRAL_VOICE_RIDER_SMOKE_SOURCE must be set"),
        );
        let ffmpeg = PathBuf::from(
            std::env::var("ASTRAL_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_owned()),
        );
        let mut request = request();
        request.source_path = source.to_string_lossy().into_owned();
        request.duration_ms = std::env::var("ASTRAL_VOICE_RIDER_SMOKE_DURATION_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5_000);
        let result = analyze_voice_rider_with_paths(&ffmpeg, &source, &request).unwrap();
        eprintln!(
            "speech coverage={:.3}, avg probability={:.3}, cut={:.2} dB, boost={:.2} dB, points={}",
            result.speech_coverage,
            result.average_speech_probability,
            result.strongest_cut_db,
            result.strongest_boost_db,
            result.points.len()
        );
        assert!(!result.points.is_empty());
        assert!(!result.speech_segments.is_empty());
        assert_eq!(
            result.points.last().unwrap().at_ms,
            (request.duration_ms as f64 / request.playback_rate).round() as u64
        );
        assert!((0.0..=1.0).contains(&result.speech_coverage));
        assert!((0.0..=1.0).contains(&result.average_speech_probability));
    }
}
