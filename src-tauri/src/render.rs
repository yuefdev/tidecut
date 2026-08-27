use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet, VecDeque},
    fs,
    hash::{Hash, Hasher},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};

pub const RENDER_PROGRESS_EVENT: &str = "render://progress";
pub const RENDER_COMPLETED_EVENT: &str = "render://completed";
pub const RENDER_CANCELLED_EVENT: &str = "render://cancelled";
pub const RENDER_FAILED_EVENT: &str = "render://failed";
pub const FFMPEG_SETUP_PROGRESS_EVENT: &str = "ffmpeg://setup-progress";

const STDERR_TAIL_LINES: usize = 40;
const MAX_RENDER_DURATION_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_AUDIO_AUTOMATION_POINTS: usize = 20_000;
// Bump this whenever proxy codec, pixel format, GOP or filter semantics change.
// Keeping it in the cache key prevents an older-but-valid file from being
// mistaken for a proxy produced by the current pipeline.
const PROXY_CACHE_SCHEMA_REVISION: &str = "proxy-v2-libx264-aac-yuv420p-crf28";
const REQUIRED_FFMPEG_ENCODERS: &[&str] = &["libx264", "aac"];
const REQUIRED_FFMPEG_FILTERS: &[&str] = &[
    "scale",
    "pad",
    "crop",
    "setsar",
    "fps",
    "format",
    "trim",
    "setpts",
    "overlay",
    "rotate",
    "colorchannelmixer",
    "geq",
    "fade",
    "drawtext",
    "atrim",
    "asetpts",
    "atempo",
    "aformat",
    "volume",
    "aeval",
    "pan",
    "afade",
    "adelay",
    "amix",
    "alimiter",
    "loudnorm",
    "aresample",
    "arnndn",
    "highpass",
    "bandreject",
    "apad",
    "color",
    "anullsrc",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AspectRatioPreset {
    #[serde(rename = "9:16", alias = "vertical_9_16")]
    Vertical9x16,
    #[serde(rename = "1:1", alias = "square_1_1")]
    Square1x1,
    #[serde(rename = "16:9", alias = "landscape_16_9")]
    Landscape16x9,
}

impl AspectRatioPreset {
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Vertical9x16 => (1080, 1920),
            Self::Square1x1 => (1080, 1080),
            Self::Landscape16x9 => (1920, 1080),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Vertical9x16 => "Dikey 9:16",
            Self::Square1x1 => "Kare 1:1",
            Self::Landscape16x9 => "Yatay 16:9",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasFit {
    #[default]
    Contain,
    Cover,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportQuality {
    Draft,
    #[default]
    Standard,
    High,
}

impl ExportQuality {
    fn encoder_settings(self) -> (&'static str, u8) {
        match self {
            Self::Draft => ("veryfast", 28),
            Self::Standard => ("medium", 20),
            Self::High => ("slow", 16),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProxyProfile {
    #[serde(rename = "540p")]
    P540,
    #[default]
    #[serde(rename = "720p")]
    P720,
}

impl ProxyProfile {
    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::P540 => (960, 540),
            Self::P720 => (1280, 720),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::P540 => "540p",
            Self::P720 => "720p",
        }
    }
}

fn default_frame_rate() -> u32 {
    30
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub timeline: Option<RenderTimeline>,
    pub output_path: String,
    pub duration_ms: u64,
    pub preset: AspectRatioPreset,
    #[serde(default = "default_frame_rate")]
    pub frame_rate: u32,
    #[serde(default)]
    pub fit: CanvasFit,
    #[serde(default)]
    pub quality: ExportQuality,
    #[serde(default = "default_true")]
    pub include_audio: bool,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
}

fn default_background_color() -> String {
    "#000000".into()
}

fn default_speed() -> f64 {
    1.0
}

fn default_volume() -> f64 {
    1.0
}

fn default_audio_automation_easing() -> String {
    "linear".into()
}

fn default_scale() -> f64 {
    1.0
}

fn default_opacity() -> f64 {
    1.0
}

fn default_gain() -> f64 {
    1.0
}

fn default_font_size() -> f64 {
    64.0
}

fn default_text_color() -> String {
    "#ffffff".into()
}

fn default_font_family() -> String {
    "Inter".into()
}

fn default_font_weight() -> u16 {
    600
}

fn default_text_background() -> String {
    "transparent".into()
}

fn default_text_align() -> String {
    "center".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderTimeline {
    #[serde(default)]
    pub tracks: Vec<RenderTrack>,
    #[serde(default)]
    pub clips: Vec<RenderClip>,
    #[serde(default = "default_background_color")]
    pub background_color: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderTrackKind {
    Video,
    Audio,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderTrack {
    pub id: String,
    pub kind: RenderTrackKind,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_gain")]
    pub gain: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub z_index: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderClipKind {
    Video,
    Image,
    Audio,
    Text,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderViewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderClip {
    pub id: String,
    pub track_id: String,
    pub kind: RenderClipKind,
    #[serde(default)]
    pub path: Option<String>,
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub trim_in_ms: u64,
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub audio_path: Option<String>,
    #[serde(default)]
    pub audio_trim_in_ms: Option<u64>,
    #[serde(default)]
    pub transform: RenderTransform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<RenderViewport>,
    #[serde(default)]
    pub text: Option<RenderText>,
    #[serde(default)]
    pub transition: Option<RenderTransition>,
    #[serde(default)]
    pub keyframes: Vec<RenderKeyframe>,
    #[serde(default)]
    pub z_index: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderTransform {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "default_scale")]
    pub scale_x: f64,
    #[serde(default = "default_scale")]
    pub scale_y: f64,
    #[serde(default)]
    pub rotation_deg: f64,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub shake: f64,
}

impl Default for RenderTransform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_deg: 0.0,
            opacity: 1.0,
            shake: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderText {
    pub content: String,
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f64,
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    #[serde(default = "default_text_color")]
    pub color: String,
    #[serde(default = "default_text_background")]
    pub background_color: String,
    #[serde(default = "default_text_align")]
    pub align: String,
    #[serde(default)]
    pub stroke_color: Option<String>,
    #[serde(default)]
    pub stroke_width: f64,
    #[serde(default)]
    pub shadow_color: Option<String>,
    #[serde(default)]
    pub shadow_offset_x: f64,
    #[serde(default)]
    pub shadow_offset_y: f64,
    #[serde(default)]
    pub animation_in_kind: Option<String>,
    #[serde(default)]
    pub animation_in_ms: u64,
    #[serde(default)]
    pub animation_out_kind: Option<String>,
    #[serde(default)]
    pub animation_out_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderTransitionKind {
    None,
    #[default]
    Fade,
    Dissolve,
    DipToBlack,
    WipeLeft,
    WipeRight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderTransition {
    #[serde(default)]
    pub kind: RenderTransitionKind,
    #[serde(default)]
    pub in_kind: Option<RenderTransitionKind>,
    #[serde(default)]
    pub out_kind: Option<RenderTransitionKind>,
    #[serde(default)]
    pub in_duration_ms: u64,
    #[serde(default)]
    pub out_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderKeyframe {
    pub at_ms: u64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub scale_x: Option<f64>,
    #[serde(default)]
    pub scale_y: Option<f64>,
    #[serde(default)]
    pub rotation_deg: Option<f64>,
    #[serde(default)]
    pub opacity: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub rider_gain: Option<f64>,
    #[serde(default)]
    pub duck_gain: Option<f64>,
    #[serde(default)]
    pub pan: Option<f64>,
    #[serde(default)]
    pub easing: RenderKeyframeEasing,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderKeyframeEasing {
    #[serde(default)]
    pub x: Option<KeyframeEasing>,
    #[serde(default)]
    pub y: Option<KeyframeEasing>,
    #[serde(default)]
    pub scale_x: Option<KeyframeEasing>,
    #[serde(default)]
    pub scale_y: Option<KeyframeEasing>,
    #[serde(default)]
    pub rotation_deg: Option<KeyframeEasing>,
    #[serde(default)]
    pub opacity: Option<KeyframeEasing>,
    #[serde(default)]
    pub volume: Option<KeyframeEasing>,
    #[serde(default)]
    pub rider_gain: Option<KeyframeEasing>,
    #[serde(default)]
    pub duck_gain: Option<KeyframeEasing>,
    #[serde(default)]
    pub pan: Option<KeyframeEasing>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum KeyframeEasing {
    #[default]
    Linear,
    Hold,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequest {
    pub source_path: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub profile: ProxyProfile,
    #[serde(default = "default_frame_rate")]
    pub frame_rate: u32,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub force: bool,
}

/// Exports a source media range as a standalone audio file. `duration_ms` and
/// `start_ms` are expressed in source-media time; `playback_rate` is applied
/// after trimming so a timeline clip can be exported exactly as it plays.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioAutomationKeyframe {
    pub at_ms: u64,
    pub value: f64,
    #[serde(default = "default_audio_automation_easing")]
    pub easing: String,
}

#[derive(Clone, Copy)]
struct AudioAutomationInputs<'a> {
    start_ms: u64,
    volume_keyframes: &'a [AudioAutomationKeyframe],
    rider_gain_keyframes: &'a [AudioAutomationKeyframe],
    duck_gain_keyframes: &'a [AudioAutomationKeyframe],
}

impl<'a> AudioAutomationInputs<'a> {
    fn new(
        start_ms: u64,
        volume_keyframes: &'a [AudioAutomationKeyframe],
        rider_gain_keyframes: &'a [AudioAutomationKeyframe],
        duck_gain_keyframes: &'a [AudioAutomationKeyframe],
    ) -> Self {
        Self {
            start_ms,
            volume_keyframes,
            rider_gain_keyframes,
            duck_gain_keyframes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioExportRequest {
    pub source_path: String,
    pub output_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_speed")]
    pub playback_rate: f64,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub automation_start_ms: u64,
    #[serde(default)]
    pub volume_keyframes: Vec<AudioAutomationKeyframe>,
    #[serde(default)]
    pub rider_gain_keyframes: Vec<AudioAutomationKeyframe>,
    #[serde(default)]
    pub duck_gain_keyframes: Vec<AudioAutomationKeyframe>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub normalization: Option<AudioLoudnessPass>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioLoudnessTarget {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioLoudnessAnalysisRequest {
    pub source_path: String,
    #[serde(default)]
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_speed")]
    pub playback_rate: f64,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub automation_start_ms: u64,
    #[serde(default)]
    pub volume_keyframes: Vec<AudioAutomationKeyframe>,
    #[serde(default)]
    pub rider_gain_keyframes: Vec<AudioAutomationKeyframe>,
    #[serde(default)]
    pub duck_gain_keyframes: Vec<AudioAutomationKeyframe>,
    pub target: AudioLoudnessTarget,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioLoudnessPass {
    pub integrated_lufs: f64,
    pub true_peak_db: f64,
    pub loudness_range: f64,
    pub measured_integrated_lufs: f64,
    pub measured_true_peak_db: f64,
    pub measured_loudness_range: f64,
    pub measured_threshold_lufs: f64,
    pub target_offset_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioLoudnessAnalysisResult {
    pub pass: AudioLoudnessPass,
    pub recommended_gain_db: f64,
    pub recommended_gain: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LoudnormMeasurement {
    input_i: f64,
    input_tp: f64,
    input_lra: f64,
    input_thresh: f64,
    target_offset: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioExportFormat {
    Mp3,
    M4a,
    Wav,
}

impl AudioExportFormat {
    fn from_output_path(path: &Path) -> Result<Self, RenderErrorReport> {
        match path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mp3") => Ok(Self::Mp3),
            Some("m4a") => Ok(Self::M4a),
            Some("wav") => Ok(Self::Wav),
            extension => Err(RenderErrorReport::new(
                "unsupported_audio_output",
                "Ses çıktısı MP3, M4A veya WAV olmalı.",
                format!("Unsupported audio output extension: {extension:?}"),
                false,
            )),
        }
    }

    fn encoder_args(self) -> Vec<String> {
        match self {
            Self::Mp3 => vec![
                "-c:a".into(),
                "libmp3lame".into(),
                "-b:a".into(),
                "192k".into(),
            ],
            Self::M4a => vec![
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
                "-movflags".into(),
                "+faststart".into(),
            ],
            Self::Wav => vec!["-c:a".into(), "pcm_s16le".into()],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderJobKind {
    Export,
    AudioExport,
    Proxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderJobStatus {
    Queued,
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

impl RenderJobStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderErrorReport {
    pub code: String,
    pub user_message: String,
    pub technical_message: String,
    pub retryable: bool,
    pub stderr_tail: Vec<String>,
}

impl RenderErrorReport {
    fn new(
        code: impl Into<String>,
        user_message: impl Into<String>,
        technical_message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            user_message: user_message.into(),
            technical_message: technical_message.into(),
            retryable,
            stderr_tail: Vec::new(),
        }
    }

    fn with_stderr(mut self, stderr_tail: Vec<String>) -> Self {
        self.stderr_tail = stderr_tail;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderJobSnapshot {
    pub job_id: String,
    pub kind: RenderJobKind,
    pub status: RenderJobStatus,
    pub progress_percent: f64,
    pub encoded_ms: u64,
    pub duration_ms: u64,
    pub frame: Option<u64>,
    pub encoding_fps: Option<f64>,
    pub speed: Option<f64>,
    pub eta_ms: Option<u64>,
    pub output_path: String,
    pub error: Option<RenderErrorReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderAccepted {
    pub job: RenderJobSnapshot,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPresetInfo {
    pub preset: AspectRatioPreset,
    pub label: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderCapabilities {
    pub available: bool,
    pub ffmpeg_path: String,
    pub version: Option<String>,
    pub diagnostic: Option<RenderErrorReport>,
    pub presets: Vec<RenderPresetInfo>,
    pub proxy_profiles: Vec<String>,
    pub proxy_cache_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegSetupProgress {
    pub stage: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: Option<f64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyCacheStats {
    pub cache_dir: String,
    pub file_count: u64,
    pub partial_file_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyPruneResult {
    pub before: ProxyCacheStats,
    pub after: ProxyCacheStats,
    pub removed_files: u64,
    pub reclaimed_bytes: u64,
}

struct JobControl {
    snapshot: Mutex<RenderJobSnapshot>,
    cancel_requested: AtomicBool,
    child: Mutex<Option<Child>>,
    working_path: Option<PathBuf>,
}

#[derive(Default)]
pub struct RenderManager {
    jobs: Mutex<HashMap<String, Arc<JobControl>>>,
    next_id: AtomicU64,
    ffmpeg_setup_active: AtomicBool,
}

impl RenderManager {
    fn new_job_id(&self, kind: RenderJobKind) -> String {
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_millis())
            .unwrap_or_default();
        let prefix = match kind {
            RenderJobKind::Export => "export",
            RenderJobKind::AudioExport => "audio",
            RenderJobKind::Proxy => "proxy",
        };
        format!("{prefix}-{timestamp}-{sequence}")
    }

    fn get(&self, job_id: &str) -> Option<Arc<JobControl>> {
        self.jobs.lock().ok()?.get(job_id).cloned()
    }

    fn insert(&self, job_id: String, control: Arc<JobControl>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(job_id, control);
        }
    }

    fn active_output_paths(&self) -> HashSet<PathBuf> {
        let Ok(jobs) = self.jobs.lock() else {
            return HashSet::new();
        };
        jobs.values()
            .filter_map(|control| {
                let snapshot = control.snapshot.lock().ok()?;
                (!snapshot.status.is_terminal()).then(|| {
                    let mut paths = vec![PathBuf::from(&snapshot.output_path)];
                    if let Some(working_path) = control.working_path.as_ref() {
                        paths.push(working_path.clone());
                    }
                    paths
                })
            })
            .flatten()
            .collect()
    }
}

impl Drop for RenderManager {
    fn drop(&mut self) {
        let Ok(jobs) = self.jobs.lock() else {
            return;
        };
        for control in jobs.values() {
            control.cancel_requested.store(true, Ordering::Release);
            if let Ok(mut child) = control.child.lock() {
                if let Some(child) = child.as_mut() {
                    let _ = child.kill();
                }
            }
        }
    }
}

struct JobSpec {
    kind: RenderJobKind,
    ffmpeg_path: PathBuf,
    output_path: PathBuf,
    partial_path: PathBuf,
    duration_ms: u64,
    overwrite: bool,
    args: Vec<String>,
    cleanup_paths: Vec<PathBuf>,
}

/// Owns transient render resources (currently UTF-8 drawtext payloads) and
/// removes them on every terminal path, including cancellation and failure.
struct TransientFiles(Vec<PathBuf>);

impl Drop for TransientFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Default)]
struct ProgressAccumulator {
    encoded_ms: u64,
    frame: Option<u64>,
    encoding_fps: Option<f64>,
    speed: Option<f64>,
    saw_out_time_us: bool,
}

impl ProgressAccumulator {
    fn consume(&mut self, line: &str) -> bool {
        let Some((key, value)) = line.trim().split_once('=') else {
            return false;
        };
        match key {
            "out_time_us" => {
                self.encoded_ms = value.parse::<u64>().unwrap_or_default() / 1_000;
                self.saw_out_time_us = true;
            }
            // Despite its historical name, FFmpeg reports this value in microseconds.
            "out_time_ms" if !self.saw_out_time_us => {
                self.encoded_ms = value.parse::<u64>().unwrap_or_default() / 1_000;
            }
            "frame" => self.frame = value.parse().ok(),
            "fps" => self.encoding_fps = parse_positive_number(value),
            "speed" => self.speed = parse_positive_number(value.trim_end_matches('x')),
            "progress" => return value == "continue" || value == "end",
            _ => {}
        }
        false
    }

    fn apply_to(&self, snapshot: &mut RenderJobSnapshot) {
        snapshot.encoded_ms = self.encoded_ms.min(snapshot.duration_ms);
        snapshot.progress_percent = if snapshot.duration_ms == 0 {
            0.0
        } else {
            ((snapshot.encoded_ms as f64 / snapshot.duration_ms as f64) * 100.0).min(99.9)
        };
        snapshot.frame = self.frame;
        snapshot.encoding_fps = self.encoding_fps;
        snapshot.speed = self.speed;
        snapshot.eta_ms = self.speed.and_then(|speed| {
            (speed > 0.0).then(|| {
                ((snapshot.duration_ms.saturating_sub(snapshot.encoded_ms)) as f64 / speed).round()
                    as u64
            })
        });
    }
}

fn parse_positive_number(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn ffmpeg_listing_has_component(listing: &str, component: &str) -> bool {
    listing.lines().any(|line| {
        let mut columns = line.split_whitespace();
        let Some(flags) = columns.next() else {
            return false;
        };
        let Some(name) = columns.next() else {
            return false;
        };
        name == component
            && !flags.is_empty()
            && flags
                .chars()
                .all(|character| character == '.' || character.is_ascii_uppercase())
    })
}

fn missing_ffmpeg_components(encoder_listing: &str, filter_listing: &str) -> Vec<String> {
    let mut missing = REQUIRED_FFMPEG_ENCODERS
        .iter()
        .filter(|encoder| !ffmpeg_listing_has_component(encoder_listing, encoder))
        .map(|encoder| format!("encoder:{encoder}"))
        .collect::<Vec<_>>();
    missing.extend(
        REQUIRED_FFMPEG_FILTERS
            .iter()
            .filter(|filter| !ffmpeg_listing_has_component(filter_listing, filter))
            .map(|filter| format!("filter:{filter}")),
    );
    missing
}

fn ffmpeg_output_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    text
}

fn run_ffmpeg_listing(executable: &Path, listing_flag: &str) -> Result<String, RenderErrorReport> {
    let output = Command::new(executable)
        .args(["-hide_banner", listing_flag])
        .output()
        .map_err(|error| ffmpeg_spawn_error(executable, &error))?;
    let listing = ffmpeg_output_text(&output);
    if output.status.success() {
        Ok(listing)
    } else {
        Err(RenderErrorReport::new(
            "ffmpeg_capability_probe_failed",
            "FFmpeg özellikleri doğrulanamadı.",
            if listing.trim().is_empty() {
                format!("FFmpeg {listing_flag} exited with status {}", output.status)
            } else {
                listing
            },
            true,
        ))
    }
}

fn probe_ffmpeg_capabilities(executable: &Path) -> Result<Option<String>, RenderErrorReport> {
    let output = Command::new(executable)
        .arg("-version")
        .output()
        .map_err(|error| ffmpeg_spawn_error(executable, &error))?;
    let version_output = ffmpeg_output_text(&output);
    if !output.status.success() {
        return Err(RenderErrorReport::new(
            "ffmpeg_unavailable",
            "FFmpeg çalıştırılamadı.",
            if version_output.trim().is_empty() {
                format!("FFmpeg exited with status {}", output.status)
            } else {
                version_output
            },
            true,
        ));
    }

    let version = version_output.lines().find_map(|line| {
        let line = line.trim();
        (!line.is_empty()).then(|| line.to_owned())
    });
    let encoders = run_ffmpeg_listing(executable, "-encoders")?;
    let filters = run_ffmpeg_listing(executable, "-filters")?;
    let missing = missing_ffmpeg_components(&encoders, &filters);
    if !missing.is_empty() {
        return Err(RenderErrorReport::new(
            "ffmpeg_capability_missing",
            "FFmpeg sürümü render için gerekli codec veya filtreleri içermiyor. FFmpeg motorunu yeniden kurun.",
            format!("Missing required FFmpeg components: {}", missing.join(", ")),
            true,
        ));
    }
    Ok(version)
}

#[tauri::command]
pub fn get_render_capabilities(app: AppHandle, ffmpeg_path: Option<String>) -> RenderCapabilities {
    let executable = resolve_ffmpeg_path(&app, ffmpeg_path.as_deref());
    let cache_dir = proxy_cache_dir(&app)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (available, version, diagnostic) = match probe_ffmpeg_capabilities(&executable) {
        Ok(version) => (true, version, None),
        Err(error) => (false, None, Some(error)),
    };

    RenderCapabilities {
        available,
        ffmpeg_path: executable.to_string_lossy().into_owned(),
        version,
        diagnostic,
        presets: [
            AspectRatioPreset::Vertical9x16,
            AspectRatioPreset::Square1x1,
            AspectRatioPreset::Landscape16x9,
        ]
        .into_iter()
        .map(|preset| {
            let (width, height) = preset.dimensions();
            RenderPresetInfo {
                preset,
                label: preset.label().to_owned(),
                width,
                height,
            }
        })
        .collect(),
        proxy_profiles: vec![
            ProxyProfile::P540.label().into(),
            ProxyProfile::P720.label().into(),
        ],
        proxy_cache_dir: cache_dir,
    }
}

#[tauri::command]
pub fn probe_media_duration(
    app: AppHandle,
    path: String,
    ffmpeg_path: Option<String>,
) -> Result<u64, RenderErrorReport> {
    let source = validate_source_path(&path)?;
    let executable = resolve_ffmpeg_path(&app, ffmpeg_path.as_deref());
    let output = Command::new(&executable)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(&source)
        .output()
        .map_err(|error| {
            RenderErrorReport::new(
                "ffmpeg_unavailable",
                "Medya bilgisi okunamadı. FFmpeg motorunu kurup yeniden deneyin.",
                format!("Failed to probe {}: {error}", source.display()),
                true,
            )
        })?;
    parse_ffmpeg_duration_ms(&String::from_utf8_lossy(&output.stderr)).ok_or_else(|| {
        RenderErrorReport::new(
            "media_duration_unavailable",
            "Medyanın süresi okunamadı. Dosya bozuk veya desteklenmeyen bir codec kullanıyor olabilir.",
            format!("No duration in FFmpeg probe output for {}", source.display()),
            false,
        )
    })
}

#[tauri::command]
pub async fn analyze_audio_loudness(
    app: AppHandle,
    request: AudioLoudnessAnalysisRequest,
) -> Result<AudioLoudnessAnalysisResult, RenderErrorReport> {
    validate_audio_loudness_analysis_request(&request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(&app, request.ffmpeg_path.as_deref());

    tauri::async_runtime::spawn_blocking(move || {
        run_audio_loudness_analysis(&ffmpeg_path, &source_path, &request)
    })
    .await
    .map_err(|error| {
        RenderErrorReport::new(
            "audio_loudness_worker_failed",
            "Ses normalizasyon analizi tamamlanamadı.",
            format!("Audio loudness analysis worker failed: {error}"),
            true,
        )
    })?
}

fn run_audio_loudness_analysis(
    ffmpeg_path: &Path,
    source_path: &Path,
    request: &AudioLoudnessAnalysisRequest,
) -> Result<AudioLoudnessAnalysisResult, RenderErrorReport> {
    if !probe_source_has_audio(ffmpeg_path, source_path)? {
        return Err(RenderErrorReport::new(
            "source_has_no_audio",
            "Bu kaynakta analiz edilecek bir ses akışı yok.",
            format!("No audio stream detected in {}", source_path.display()),
            false,
        ));
    }

    let mut filters = build_audio_preprocessing_filters(
        request.start_ms,
        request.duration_ms,
        request.playback_rate,
        request.volume,
        AudioAutomationInputs::new(
            request.automation_start_ms,
            &request.volume_keyframes,
            &request.rider_gain_keyframes,
            &request.duck_gain_keyframes,
        ),
    );
    filters.push(loudnorm_analysis_filter(&request.target));
    let args = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-nostdin".into(),
        "-i".into(),
        source_path.to_string_lossy().into_owned(),
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-af".into(),
        filters.join(","),
        "-f".into(),
        "null".into(),
        "-loglevel".into(),
        "info".into(),
        "-".into(),
    ];
    let mut command = Command::new(ffmpeg_path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);
    let output = command
        .output()
        .map_err(|error| ffmpeg_spawn_error(ffmpeg_path, &error))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let tail = stderr
            .lines()
            .rev()
            .take(STDERR_TAIL_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        return Err(classify_ffmpeg_error(
            &tail,
            &format!("FFmpeg loudness analysis exited with {}", output.status),
        ));
    }

    let measurement = parse_loudnorm_measurement(&stderr)?;
    let pass = AudioLoudnessPass {
        integrated_lufs: request.target.integrated_lufs,
        true_peak_db: request.target.true_peak_db,
        loudness_range: request.target.loudness_range,
        measured_integrated_lufs: measurement.input_i,
        measured_true_peak_db: measurement.input_tp,
        measured_loudness_range: measurement.input_lra,
        measured_threshold_lufs: measurement.input_thresh,
        target_offset_db: measurement.target_offset,
    };
    validate_audio_loudness_pass(&pass)?;
    let recommended_gain_db = (pass.integrated_lufs - pass.measured_integrated_lufs)
        .min(pass.true_peak_db - pass.measured_true_peak_db);
    let recommended_gain = 10_f64.powf(recommended_gain_db / 20.0);

    Ok(AudioLoudnessAnalysisResult {
        pass,
        recommended_gain_db,
        recommended_gain,
    })
}

#[tauri::command]
pub async fn ensure_ffmpeg(
    app: AppHandle,
    manager: State<'_, RenderManager>,
    force: Option<bool>,
) -> Result<RenderCapabilities, RenderErrorReport> {
    if !force.unwrap_or(false) {
        let current = get_render_capabilities(app.clone(), None);
        if current.available {
            return Ok(current);
        }
    }
    let runtime_dir = ffmpeg_runtime_dir(&app)?;
    manager
        .ffmpeg_setup_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| {
            RenderErrorReport::new(
                "ffmpeg_setup_in_progress",
                "FFmpeg kurulumu zaten devam ediyor.",
                "Concurrent ensure_ffmpeg call rejected",
                true,
            )
        })?;

    let executable = runtime_ffmpeg_path_for(&runtime_dir);
    let install_app = app.clone();
    // If a cached executable exists but capability probing failed, treat it as
    // corrupt/incompatible and replace it automatically.
    let should_force = force.unwrap_or(false) || executable.is_file();
    let install_result = tauri::async_runtime::spawn_blocking(move || {
        install_ffmpeg_runtime(&install_app, &runtime_dir, &executable, should_force)
    })
    .await;
    manager.ffmpeg_setup_active.store(false, Ordering::Release);

    let installed_path = install_result.map_err(|join_error| {
        RenderErrorReport::new(
            "ffmpeg_setup_failed",
            "FFmpeg kurulumu tamamlanamadı.",
            format!("FFmpeg setup worker failed: {join_error}"),
            true,
        )
    })??;
    let capabilities =
        get_render_capabilities(app, Some(installed_path.to_string_lossy().into_owned()));
    if capabilities.available {
        Ok(capabilities)
    } else {
        Err(capabilities.diagnostic.unwrap_or_else(|| {
            RenderErrorReport::new(
                "ffmpeg_verification_failed",
                "FFmpeg indirildi ancak çalıştırılamadı.",
                format!("Verification failed: {}", installed_path.display()),
                true,
            )
        }))
    }
}

fn install_ffmpeg_runtime(
    app: &AppHandle,
    runtime_dir: &Path,
    executable: &Path,
    force: bool,
) -> Result<PathBuf, RenderErrorReport> {
    use ffmpeg_sidecar::download::{
        download_ffmpeg_package_with_progress, ffmpeg_download_url, unpack_ffmpeg_without_extras,
        FfmpegDownloadProgressEvent,
    };

    if executable.is_file() && !force {
        return Ok(executable.to_path_buf());
    }
    fs::create_dir_all(runtime_dir).map_err(|error| {
        RenderErrorReport::new(
            "ffmpeg_setup_directory_failed",
            "FFmpeg çalışma klasörü hazırlanamadı.",
            format!("{}: {error}", runtime_dir.display()),
            true,
        )
    })?;
    if force {
        let _ = fs::remove_file(executable);
    }

    let download_url = ffmpeg_download_url().map_err(|error| {
        RenderErrorReport::new(
            "ffmpeg_platform_unsupported",
            "Bu işletim sistemi veya işlemci için otomatik FFmpeg kurulumu desteklenmiyor.",
            error.to_string(),
            false,
        )
    })?;
    emit_ffmpeg_setup_progress(
        app,
        FfmpegSetupProgress {
            stage: "starting".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: Some(0.0),
            message: "FFmpeg indirmesi başlatılıyor.".into(),
        },
    );

    let progress_app = app.clone();
    let last_progress_tick = AtomicU64::new(u64::MAX);
    let archive =
        download_ffmpeg_package_with_progress(download_url, runtime_dir, |event| match event {
            FfmpegDownloadProgressEvent::Downloading {
                downloaded_bytes,
                total_bytes,
            } => {
                let tick = downloaded_bytes
                    .saturating_mul(100)
                    .checked_div(total_bytes)
                    .unwrap_or(downloaded_bytes / (1024 * 1024));
                if last_progress_tick.swap(tick, Ordering::Relaxed) != tick {
                    emit_ffmpeg_setup_progress(
                        &progress_app,
                        FfmpegSetupProgress {
                            stage: "downloading".into(),
                            downloaded_bytes,
                            total_bytes,
                            progress_percent: (total_bytes > 0).then(|| {
                                (downloaded_bytes as f64 / total_bytes as f64 * 100.0)
                                    .clamp(0.0, 100.0)
                            }),
                            message: "FFmpeg indiriliyor.".into(),
                        },
                    );
                }
            }
            FfmpegDownloadProgressEvent::Starting => {}
            FfmpegDownloadProgressEvent::UnpackingArchive => {}
            FfmpegDownloadProgressEvent::Done => {}
        })
        .map_err(|error| {
            RenderErrorReport::new(
                "ffmpeg_download_failed",
                "FFmpeg indirilemedi. Ağ bağlantısını kontrol edin.",
                format!("{download_url}: {error}"),
                true,
            )
        })?;

    emit_ffmpeg_setup_progress(
        app,
        FfmpegSetupProgress {
            stage: "unpacking".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: None,
            message: "FFmpeg arşivi açılıyor.".into(),
        },
    );
    unpack_ffmpeg_without_extras(&archive, runtime_dir).map_err(|error| {
        RenderErrorReport::new(
            "ffmpeg_unpack_failed",
            "FFmpeg arşivi açılamadı.",
            format!("{}: {error}", archive.display()),
            true,
        )
    })?;
    if !executable.is_file() {
        return Err(RenderErrorReport::new(
            "ffmpeg_install_missing",
            "FFmpeg kurulumu tamamlandı ancak çalıştırılabilir dosya bulunamadı.",
            format!("Expected executable: {}", executable.display()),
            true,
        ));
    }
    emit_ffmpeg_setup_progress(
        app,
        FfmpegSetupProgress {
            stage: "completed".into(),
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: Some(100.0),
            message: "FFmpeg kullanıma hazır.".into(),
        },
    );
    Ok(executable.to_path_buf())
}

fn emit_ffmpeg_setup_progress(app: &AppHandle, progress: FfmpegSetupProgress) {
    let _ = app.emit(FFMPEG_SETUP_PROGRESS_EVENT, progress);
}

#[tauri::command]
pub fn start_render(
    app: AppHandle,
    manager: State<'_, RenderManager>,
    request: RenderRequest,
) -> Result<RenderAccepted, RenderErrorReport> {
    let job_id = manager.new_job_id(RenderJobKind::Export);
    let spec = build_export_spec(&app, &job_id, &request)?;
    start_job(app, manager.inner(), job_id, spec, false)
}

#[tauri::command]
pub fn start_audio_export(
    app: AppHandle,
    manager: State<'_, RenderManager>,
    request: AudioExportRequest,
) -> Result<RenderAccepted, RenderErrorReport> {
    let job_id = manager.new_job_id(RenderJobKind::AudioExport);
    let spec = build_audio_export_spec(&app, &job_id, &request)?;
    start_job(app, manager.inner(), job_id, spec, false)
}

#[tauri::command]
pub fn start_proxy(
    app: AppHandle,
    manager: State<'_, RenderManager>,
    request: ProxyRequest,
) -> Result<RenderAccepted, RenderErrorReport> {
    validate_duration_and_rate(request.duration_ms, request.frame_rate)?;
    let source_path = validate_source_path(&request.source_path)?;
    let cache_dir = proxy_cache_dir(&app)?;
    fs::create_dir_all(&cache_dir).map_err(|error| {
        RenderErrorReport::new(
            "cache_unavailable",
            "Proxy önbellek klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;

    let cache_key = proxy_cache_key(&source_path, request.profile, request.frame_rate)?;
    let output_path = cache_dir.join(format!("{cache_key}.{}.proxy.mp4", request.profile.label()));
    let job_id = manager.new_job_id(RenderJobKind::Proxy);

    if proxy_cache_hit_is_reusable(&output_path, request.force) {
        let _ = fs::OpenOptions::new()
            .write(true)
            .open(&output_path)
            .and_then(|file| file.set_modified(SystemTime::now()));
        let snapshot = RenderJobSnapshot {
            job_id: job_id.clone(),
            kind: RenderJobKind::Proxy,
            status: RenderJobStatus::Completed,
            progress_percent: 100.0,
            encoded_ms: request.duration_ms,
            duration_ms: request.duration_ms,
            frame: None,
            encoding_fps: None,
            speed: None,
            eta_ms: Some(0),
            output_path: output_path.to_string_lossy().into_owned(),
            error: None,
        };
        manager.insert(
            job_id,
            Arc::new(JobControl {
                snapshot: Mutex::new(snapshot.clone()),
                cancel_requested: AtomicBool::new(false),
                child: Mutex::new(None),
                working_path: None,
            }),
        );
        let _ = app.emit(RENDER_COMPLETED_EVENT, &snapshot);
        return Ok(RenderAccepted {
            job: snapshot,
            cache_hit: true,
        });
    }
    // A forced refresh writes to its job-specific partial path. Keep the last
    // complete proxy available until finalize_output_file atomically replaces
    // it, so cancellation or encoder failure cannot destroy a usable cache hit.
    let spec = build_proxy_spec(&app, &job_id, &request, source_path, output_path)?;
    start_job(app, manager.inner(), job_id, spec, false)
}

fn proxy_cache_hit_is_reusable(output_path: &Path, force: bool) -> bool {
    !force
        && output_path
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
}

#[tauri::command]
pub fn cancel_render(
    app: AppHandle,
    manager: State<'_, RenderManager>,
    job_id: String,
) -> Result<RenderJobSnapshot, RenderErrorReport> {
    let control = manager.get(&job_id).ok_or_else(|| {
        RenderErrorReport::new(
            "job_not_found",
            "Render görevi bulunamadı.",
            format!("Unknown render job: {job_id}"),
            false,
        )
    })?;

    let snapshot = {
        let mut snapshot = control.snapshot.lock().map_err(|_| lock_error())?;
        if snapshot.status.is_terminal() {
            return Ok(snapshot.clone());
        }
        control.cancel_requested.store(true, Ordering::Release);
        snapshot.status = RenderJobStatus::Cancelling;
        snapshot.clone()
    };
    let _ = app.emit(RENDER_PROGRESS_EVENT, &snapshot);

    if let Ok(mut child_slot) = control.child.lock() {
        if let Some(child) = child_slot.as_mut() {
            let _ = child.kill();
        }
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn get_render_job(
    manager: State<'_, RenderManager>,
    job_id: String,
) -> Result<RenderJobSnapshot, RenderErrorReport> {
    let control = manager.get(&job_id).ok_or_else(|| {
        RenderErrorReport::new(
            "job_not_found",
            "Render görevi bulunamadı.",
            format!("Unknown render job: {job_id}"),
            false,
        )
    })?;
    let snapshot = control.snapshot.lock().map_err(|_| lock_error())?.clone();
    Ok(snapshot)
}

#[tauri::command]
pub fn list_render_jobs(
    manager: State<'_, RenderManager>,
) -> Result<Vec<RenderJobSnapshot>, RenderErrorReport> {
    let jobs = manager.jobs.lock().map_err(|_| lock_error())?;
    let mut snapshots = jobs
        .values()
        .filter_map(|control| {
            control
                .snapshot
                .lock()
                .ok()
                .map(|snapshot| snapshot.clone())
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|a, b| b.job_id.cmp(&a.job_id));
    Ok(snapshots)
}

#[tauri::command]
pub fn get_proxy_cache_stats(app: AppHandle) -> Result<ProxyCacheStats, RenderErrorReport> {
    proxy_cache_stats_for(&proxy_cache_dir(&app)?)
}

#[tauri::command]
pub fn prune_proxy_cache(
    app: AppHandle,
    manager: State<'_, RenderManager>,
    max_bytes: u64,
) -> Result<ProxyPruneResult, RenderErrorReport> {
    let cache_dir = proxy_cache_dir(&app)?;
    fs::create_dir_all(&cache_dir).map_err(cache_io_error)?;
    let before = proxy_cache_stats_for(&cache_dir)?;
    let active_outputs = manager.active_output_paths();
    let mut files = proxy_files(&cache_dir)?
        .into_iter()
        .filter(|entry| !active_outputs.contains(&entry.path))
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| entry.modified);

    let mut current_bytes = before.total_bytes;
    let mut removed_files = 0;
    let mut reclaimed_bytes = 0;
    for entry in files {
        if !entry.partial && current_bytes <= max_bytes {
            continue;
        }
        if fs::remove_file(&entry.path).is_ok() {
            current_bytes = current_bytes.saturating_sub(entry.bytes);
            reclaimed_bytes += entry.bytes;
            removed_files += 1;
        }
    }
    let after = proxy_cache_stats_for(&cache_dir)?;
    Ok(ProxyPruneResult {
        before,
        after,
        removed_files,
        reclaimed_bytes,
    })
}

fn start_job(
    app: AppHandle,
    manager: &RenderManager,
    job_id: String,
    spec: JobSpec,
    cache_hit: bool,
) -> Result<RenderAccepted, RenderErrorReport> {
    let transient_files = TransientFiles(spec.cleanup_paths.clone());
    if spec.output_path.exists() && !spec.overwrite {
        return Err(RenderErrorReport::new(
            "output_exists",
            "Çıktı dosyası zaten mevcut.",
            format!("Output already exists: {}", spec.output_path.display()),
            true,
        ));
    }

    if let Some(parent) = spec.output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            RenderErrorReport::new(
                "output_unavailable",
                "Çıktı klasörü hazırlanamadı.",
                error.to_string(),
                true,
            )
        })?;
    }

    let _ = fs::remove_file(&spec.partial_path);
    let mut command = Command::new(&spec.ffmpeg_path);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| ffmpeg_spawn_error(&spec.ffmpeg_path, &error))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        RenderErrorReport::new(
            "ffmpeg_io_error",
            "Render ilerleme kanalı açılamadı.",
            "FFmpeg stdout was not piped",
            true,
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        RenderErrorReport::new(
            "ffmpeg_io_error",
            "Render hata kanalı açılamadı.",
            "FFmpeg stderr was not piped",
            true,
        )
    })?;

    let snapshot = RenderJobSnapshot {
        job_id: job_id.clone(),
        kind: spec.kind,
        status: RenderJobStatus::Running,
        progress_percent: 0.0,
        encoded_ms: 0,
        duration_ms: spec.duration_ms,
        frame: None,
        encoding_fps: None,
        speed: None,
        eta_ms: None,
        output_path: spec.output_path.to_string_lossy().into_owned(),
        error: None,
    };
    let control = Arc::new(JobControl {
        snapshot: Mutex::new(snapshot.clone()),
        cancel_requested: AtomicBool::new(false),
        child: Mutex::new(Some(child)),
        working_path: Some(spec.partial_path.clone()),
    });
    manager.insert(job_id, Arc::clone(&control));
    let _ = app.emit(RENDER_PROGRESS_EVENT, &snapshot);

    let worker_snapshot = snapshot.clone();
    thread::spawn(move || run_job(app, control, spec, stdout, stderr, transient_files));

    Ok(RenderAccepted {
        job: worker_snapshot,
        cache_hit,
    })
}

fn run_job(
    app: AppHandle,
    control: Arc<JobControl>,
    spec: JobSpec,
    stdout: impl std::io::Read,
    stderr: impl std::io::Read + Send + 'static,
    _transient_files: TransientFiles,
) {
    let stderr_tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let stderr_for_thread = Arc::clone(&stderr_tail);
    let stderr_reader = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut tail) = stderr_for_thread.lock() {
                if tail.len() == STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        }
    });

    let mut progress = ProgressAccumulator::default();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if progress.consume(&line) {
            if let Ok(mut snapshot) = control.snapshot.lock() {
                progress.apply_to(&mut snapshot);
                let payload = snapshot.clone();
                drop(snapshot);
                let _ = app.emit(RENDER_PROGRESS_EVENT, &payload);
            }
        }
    }

    let status = control
        .child
        .lock()
        .ok()
        .and_then(|mut child| child.take())
        .and_then(|mut child| child.wait().ok());
    let _ = stderr_reader.join();
    let stderr_lines = stderr_tail
        .lock()
        .map(|tail| tail.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    if control.cancel_requested.load(Ordering::Acquire) {
        let _ = fs::remove_file(&spec.partial_path);
        if let Ok(mut snapshot) = control.snapshot.lock() {
            snapshot.status = RenderJobStatus::Cancelled;
            snapshot.eta_ms = None;
            let payload = snapshot.clone();
            drop(snapshot);
            let _ = app.emit(RENDER_CANCELLED_EVENT, &payload);
        }
        return;
    }

    if status.map(|value| value.success()).unwrap_or(false) {
        if !spec
            .partial_path
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
        {
            finish_with_error(
                &app,
                &control,
                &spec,
                RenderErrorReport::new(
                    "output_missing",
                    "Kodlayıcı tamamlandı ancak çıktı dosyası oluşmadı.",
                    format!("Missing or empty output: {}", spec.partial_path.display()),
                    true,
                )
                .with_stderr(stderr_lines),
            );
            return;
        }

        let finalize_result = finalize_output_file(&spec.partial_path, &spec.output_path);
        if let Err(error) = finalize_result {
            finish_with_error(
                &app,
                &control,
                &spec,
                RenderErrorReport::new(
                    "output_finalize_failed",
                    "Render tamamlandı ancak çıktı sonlandırılamadı.",
                    error.to_string(),
                    true,
                )
                .with_stderr(stderr_lines),
            );
            return;
        }

        if let Ok(mut snapshot) = control.snapshot.lock() {
            snapshot.status = RenderJobStatus::Completed;
            snapshot.progress_percent = 100.0;
            snapshot.encoded_ms = snapshot.duration_ms;
            snapshot.eta_ms = Some(0);
            let payload = snapshot.clone();
            drop(snapshot);
            let _ = app.emit(RENDER_COMPLETED_EVENT, &payload);
        }
    } else {
        let exit_detail = status
            .and_then(|value| value.code())
            .map(|code| format!("FFmpeg exited with code {code}"))
            .unwrap_or_else(|| "FFmpeg process ended without an exit status".into());
        let error = classify_ffmpeg_error(&stderr_lines, &exit_detail);
        finish_with_error(&app, &control, &spec, error);
    }
}

fn finish_with_error(
    app: &AppHandle,
    control: &JobControl,
    spec: &JobSpec,
    error: RenderErrorReport,
) {
    let _ = fs::remove_file(&spec.partial_path);
    if let Ok(mut snapshot) = control.snapshot.lock() {
        snapshot.status = RenderJobStatus::Failed;
        snapshot.eta_ms = None;
        snapshot.error = Some(error);
        let payload = snapshot.clone();
        drop(snapshot);
        let _ = app.emit(RENDER_FAILED_EVENT, &payload);
    }
}

fn finalize_output_file(partial: &Path, output: &Path) -> std::io::Result<()> {
    if !output.exists() {
        return fs::rename(partial, output);
    }

    let file_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("render-output");
    let partial_name = partial
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("partial");
    let rollback = output.with_file_name(format!(".{file_name}.{partial_name}.replace-backup"));
    if rollback.exists() {
        fs::remove_file(&rollback)?;
    }
    fs::rename(output, &rollback)?;
    match fs::rename(partial, output) {
        Ok(()) => {
            let _ = fs::remove_file(rollback);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&rollback, output);
            Err(error)
        }
    }
}

fn build_export_spec(
    app: &AppHandle,
    job_id: &str,
    request: &RenderRequest,
) -> Result<JobSpec, RenderErrorReport> {
    validate_duration_and_rate(request.duration_ms, request.frame_rate)?;
    if let Some(timeline) = request.timeline.as_ref() {
        return build_timeline_export_spec(app, job_id, request, timeline);
    }

    let source_value = request
        .source_path
        .as_deref()
        .ok_or_else(missing_render_source_error)?;
    let source_path = validate_source_path(source_value)?;
    let output_path =
        validate_output_path(&request.output_path, std::slice::from_ref(&source_path))?;
    let partial_path = partial_output_path(&output_path, job_id)?;
    let ffmpeg_path = resolve_ffmpeg_path(app, request.ffmpeg_path.as_deref());
    let (width, height) = request.preset.dimensions();
    let filter = canvas_filter(width, height, request.frame_rate, request.fit);
    let (encoder_preset, crf) = request.quality.encoder_settings();

    let mut args = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        source_path.to_string_lossy().into_owned(),
        "-map".into(),
        "0:v:0".into(),
    ];
    if request.include_audio {
        args.extend(["-map".into(), "0:a?".into()]);
    } else {
        args.push("-an".into());
    }
    args.extend([
        "-sn".into(),
        "-dn".into(),
        "-vf".into(),
        filter,
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        encoder_preset.into(),
        "-crf".into(),
        crf.to_string(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]);
    if request.include_audio {
        args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
    }
    args.extend([
        "-movflags".into(),
        "+faststart".into(),
        "-t".into(),
        format_seconds(request.duration_ms),
        "-progress".into(),
        "pipe:1".into(),
        "-stats_period".into(),
        "0.25".into(),
        "-loglevel".into(),
        "warning".into(),
        partial_path.to_string_lossy().into_owned(),
    ]);

    Ok(JobSpec {
        kind: RenderJobKind::Export,
        ffmpeg_path,
        output_path,
        partial_path,
        duration_ms: request.duration_ms,
        overwrite: request.overwrite,
        args,
        cleanup_paths: Vec::new(),
    })
}

fn build_audio_preprocessing_filters(
    start_ms: u64,
    duration_ms: u64,
    playback_rate: f64,
    volume: f64,
    automation: AudioAutomationInputs<'_>,
) -> Vec<String> {
    let mut filters = vec![
        format!(
            "atrim=start={}:duration={}",
            format_seconds(start_ms),
            format_seconds(duration_ms)
        ),
        "asetpts=PTS-STARTPTS".into(),
    ];
    filters.extend(atempo_filters(playback_rate));
    filters.push("aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo".into());
    if automation.volume_keyframes.is_empty()
        && automation.rider_gain_keyframes.is_empty()
        && automation.duck_gain_keyframes.is_empty()
    {
        if (volume - 1.0).abs() > f64::EPSILON {
            filters.push(format!("volume={}", format_number(volume)));
        }
    } else {
        let manual_volume =
            audio_automation_expression(automation.volume_keyframes, volume, automation.start_ms);
        let rider_gain =
            audio_automation_expression(automation.rider_gain_keyframes, 1.0, automation.start_ms);
        let duck_gain =
            audio_automation_expression(automation.duck_gain_keyframes, 1.0, automation.start_ms);
        filters.push(format!(
            "volume='clip(({manual_volume})*({rider_gain})*({duck_gain}),0,4)':eval=frame"
        ));
    }
    filters
}

fn build_audio_export_filters(
    start_ms: u64,
    duration_ms: u64,
    playback_rate: f64,
    volume: f64,
    automation: AudioAutomationInputs<'_>,
    normalization: Option<&AudioLoudnessPass>,
) -> Vec<String> {
    let mut filters =
        build_audio_preprocessing_filters(start_ms, duration_ms, playback_rate, volume, automation);
    if let Some(pass) = normalization {
        filters.push(loudnorm_second_pass_filter(pass));
        // loudnorm's dynamic fallback operates at 192 kHz. Always negotiate
        // the encoded output back to the application's stable 48 kHz stereo
        // contract, regardless of whether linear mode was possible.
        filters.extend([
            "aresample=48000".into(),
            "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo".into(),
        ]);
    }
    filters
}

fn loudnorm_analysis_filter(target: &AudioLoudnessTarget) -> String {
    format!(
        "loudnorm=I={}:LRA={}:TP={}:print_format=json",
        format_number(target.integrated_lufs),
        format_number(target.loudness_range),
        format_number(target.true_peak_db)
    )
}

fn loudnorm_second_pass_filter(pass: &AudioLoudnessPass) -> String {
    format!(
        "loudnorm=I={}:LRA={}:TP={}:measured_I={}:measured_LRA={}:measured_TP={}:measured_thresh={}:offset={}:linear=true:print_format=summary",
        format_number(pass.integrated_lufs),
        format_number(pass.loudness_range),
        format_number(pass.true_peak_db),
        format_number(pass.measured_integrated_lufs),
        format_number(pass.measured_loudness_range),
        format_number(pass.measured_true_peak_db),
        format_number(pass.measured_threshold_lufs),
        format_number(pass.target_offset_db),
    )
}

fn parse_loudnorm_measurement(stderr: &str) -> Result<LoudnormMeasurement, RenderErrorReport> {
    for (start, _) in stderr.match_indices('{').rev() {
        let candidate_tail = &stderr[start..];
        let Some(end) = candidate_tail.find('}') else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&candidate_tail[..=end]) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        if ![
            "input_i",
            "input_tp",
            "input_lra",
            "input_thresh",
            "target_offset",
        ]
        .iter()
        .all(|key| object.contains_key(*key))
        {
            continue;
        }

        return Ok(LoudnormMeasurement {
            input_i: parse_loudnorm_number(object, "input_i")?,
            input_tp: parse_loudnorm_number(object, "input_tp")?,
            input_lra: parse_loudnorm_number(object, "input_lra")?,
            input_thresh: parse_loudnorm_number(object, "input_thresh")?,
            target_offset: parse_loudnorm_number(object, "target_offset")?,
        });
    }

    Err(RenderErrorReport::new(
        "audio_loudness_parse_failed",
        "Ses yüksekliği ölçümü okunamadı.",
        "FFmpeg loudnorm output did not contain a complete JSON measurement",
        true,
    ))
}

fn parse_loudnorm_number(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<f64, RenderErrorReport> {
    let value = object.get(key).and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(number) => number.trim().parse::<f64>().ok(),
        _ => None,
    });
    match value {
        Some(number) if number.is_finite() => Ok(number),
        Some(_) => Err(RenderErrorReport::new(
            "audio_loudness_unmeasurable",
            "Seçili aralık normalizasyon için ölçülebilir ses içermiyor.",
            format!("FFmpeg loudnorm returned a non-finite {key} value"),
            false,
        )),
        None => Err(RenderErrorReport::new(
            "audio_loudness_parse_failed",
            "Ses yüksekliği ölçümü okunamadı.",
            format!("FFmpeg loudnorm returned an invalid {key} value"),
            true,
        )),
    }
}

fn build_audio_export_spec(
    app: &AppHandle,
    job_id: &str,
    request: &AudioExportRequest,
) -> Result<JobSpec, RenderErrorReport> {
    validate_audio_export_request(request)?;
    let source_path = validate_source_path(&request.source_path)?;
    let ffmpeg_path = resolve_ffmpeg_path(app, request.ffmpeg_path.as_deref());
    if !probe_source_has_audio(&ffmpeg_path, &source_path)? {
        return Err(RenderErrorReport::new(
            "source_has_no_audio",
            "Bu kaynakta dışa aktarılacak bir ses akışı yok.",
            format!("No audio stream detected in {}", source_path.display()),
            false,
        ));
    }

    let (output_path, output_format) =
        validate_audio_output_path(&request.output_path, std::slice::from_ref(&source_path))?;
    let partial_path = partial_output_path(&output_path, job_id)?;
    let output_duration_ms =
        ((request.duration_ms as f64 / request.playback_rate).round() as u64).max(1);

    let audio_filters = build_audio_export_filters(
        request.start_ms,
        request.duration_ms,
        request.playback_rate,
        request.volume,
        AudioAutomationInputs::new(
            request.automation_start_ms,
            &request.volume_keyframes,
            &request.rider_gain_keyframes,
            &request.duck_gain_keyframes,
        ),
        request.normalization.as_ref(),
    );

    let mut args = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        source_path.to_string_lossy().into_owned(),
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-af".into(),
        audio_filters.join(","),
    ];
    args.extend(output_format.encoder_args());
    args.extend([
        "-t".into(),
        format_seconds(output_duration_ms),
        "-progress".into(),
        "pipe:1".into(),
        "-stats_period".into(),
        "0.25".into(),
        "-loglevel".into(),
        "warning".into(),
        partial_path.to_string_lossy().into_owned(),
    ]);

    Ok(JobSpec {
        kind: RenderJobKind::AudioExport,
        ffmpeg_path,
        output_path,
        partial_path,
        duration_ms: output_duration_ms,
        overwrite: request.overwrite,
        args,
        cleanup_paths: Vec::new(),
    })
}

fn missing_render_source_error() -> RenderErrorReport {
    RenderErrorReport::new(
        "missing_render_source",
        "Render için kaynak veya timeline verisi yok.",
        "RenderRequest requires either timeline or sourcePath",
        false,
    )
}

struct TimelineInputs {
    args: Vec<String>,
    indices: HashMap<String, usize>,
    audio_indices: HashMap<String, usize>,
    visual_paths: HashMap<String, PathBuf>,
    source_paths: Vec<PathBuf>,
    audio_clip_ids: HashSet<String>,
}

fn build_timeline_export_spec(
    app: &AppHandle,
    job_id: &str,
    request: &RenderRequest,
    timeline: &RenderTimeline,
) -> Result<JobSpec, RenderErrorReport> {
    validate_timeline(timeline, request.duration_ms)?;
    let ffmpeg_path = resolve_ffmpeg_path(app, request.ffmpeg_path.as_deref());
    let mut inputs = build_timeline_inputs(timeline, request.duration_ms)?;
    if request.include_audio {
        detect_embedded_audio_streams(&ffmpeg_path, timeline, &mut inputs)?;
    }
    let output_path = validate_output_path(&request.output_path, &inputs.source_paths)?;
    let partial_path = partial_output_path(&output_path, job_id)?;
    let (text_files, mut transient_files) = prepare_text_files(timeline, &partial_path)?;
    let args = build_timeline_command_args(request, timeline, &inputs, &text_files, &partial_path)?;
    let cleanup_paths = std::mem::take(&mut transient_files.0);

    Ok(JobSpec {
        kind: RenderJobKind::Export,
        ffmpeg_path,
        output_path,
        partial_path,
        duration_ms: request.duration_ms,
        overwrite: request.overwrite,
        args,
        cleanup_paths,
    })
}

fn prepare_text_files(
    timeline: &RenderTimeline,
    partial_path: &Path,
) -> Result<(HashMap<String, PathBuf>, TransientFiles), RenderErrorReport> {
    let parent = partial_path.parent().ok_or_else(|| {
        RenderErrorReport::new(
            "text_resource_failed",
            "Metin render kaynağı hazırlanamadı.",
            format!("Output has no parent directory: {}", partial_path.display()),
            true,
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        RenderErrorReport::new(
            "text_resource_failed",
            "Metin render klasörü hazırlanamadı.",
            error.to_string(),
            true,
        )
    })?;

    let partial_name = partial_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("render.partial");
    let mut files = HashMap::new();
    let mut transient_files = TransientFiles(Vec::new());
    for (index, clip) in timeline
        .clips
        .iter()
        .filter(|clip| clip.kind == RenderClipKind::Text)
        .enumerate()
    {
        let text = clip
            .text
            .as_ref()
            .ok_or_else(|| invalid_timeline(format!("Text clip '{}' has no payload", clip.id)))?;
        let path = parent.join(format!("{partial_name}.text-{index}.txt"));
        transient_files.0.push(path.clone());
        fs::write(&path, text.content.as_bytes()).map_err(|error| {
            RenderErrorReport::new(
                "text_resource_failed",
                "Metin render kaynağı yazılamadı.",
                format!("{}: {error}", path.display()),
                true,
            )
        })?;
        files.insert(clip.id.clone(), path);
    }
    Ok((files, transient_files))
}

fn build_timeline_command_args(
    request: &RenderRequest,
    timeline: &RenderTimeline,
    inputs: &TimelineInputs,
    text_files: &HashMap<String, PathBuf>,
    partial_path: &Path,
) -> Result<Vec<String>, RenderErrorReport> {
    let filter_complex = build_timeline_filter_complex(
        timeline,
        &inputs.indices,
        &inputs.audio_indices,
        &inputs.audio_clip_ids,
        text_files,
        request,
    )?;
    let (encoder_preset, crf) = request.quality.encoder_settings();
    let mut args = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-nostdin".into(),
        "-y".into(),
    ];
    args.extend(inputs.args.clone());
    args.extend([
        "-filter_complex".into(),
        filter_complex,
        "-map".into(),
        "[vout]".into(),
    ]);
    if request.include_audio {
        args.extend(["-map".into(), "[aout]".into()]);
    } else {
        args.push("-an".into());
    }
    args.extend([
        "-sn".into(),
        "-dn".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        encoder_preset.into(),
        "-crf".into(),
        crf.to_string(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]);
    if request.include_audio {
        args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
    }
    args.extend([
        "-movflags".into(),
        "+faststart".into(),
        "-t".into(),
        format_seconds(request.duration_ms),
        "-progress".into(),
        "pipe:1".into(),
        "-stats_period".into(),
        "0.25".into(),
        "-loglevel".into(),
        "warning".into(),
        partial_path.to_string_lossy().into_owned(),
    ]);
    Ok(args)
}

fn build_timeline_inputs(
    timeline: &RenderTimeline,
    render_duration_ms: u64,
) -> Result<TimelineInputs, RenderErrorReport> {
    let tracks = timeline
        .tracks
        .iter()
        .map(|track| (track.id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let mut args = Vec::new();
    let mut indices = HashMap::new();
    let mut audio_indices = HashMap::new();
    let mut visual_paths = HashMap::new();
    let mut source_paths = Vec::new();
    let mut audio_clip_ids = HashSet::new();
    let mut next_input_index = 0usize;

    for clip in &timeline.clips {
        if clip.start_ms >= render_duration_ms || clip.kind == RenderClipKind::Text {
            continue;
        }
        let Some(track) = tracks.get(clip.track_id.as_str()) else {
            continue;
        };
        if !track_is_active(track, &timeline.tracks) {
            continue;
        }
        let path =
            validate_source_path(clip.path.as_deref().ok_or_else(|| {
                invalid_timeline(format!("Clip '{}' has no media path", clip.id))
            })?)?;
        let input_index = next_input_index;
        next_input_index += 1;
        if clip.kind == RenderClipKind::Image {
            args.extend(["-stream_loop".into(), "-1".into()]);
        }
        args.extend(["-i".into(), path.to_string_lossy().into_owned()]);
        indices.insert(clip.id.clone(), input_index);
        visual_paths.insert(clip.id.clone(), path.clone());
        if clip.kind == RenderClipKind::Audio {
            audio_clip_ids.insert(clip.id.clone());
        }
        source_paths.push(path);
        if let Some(cleaned_path) = clip.audio_path.as_deref() {
            let cleaned_path = validate_source_path(cleaned_path)?;
            args.extend(["-i".into(), cleaned_path.to_string_lossy().into_owned()]);
            audio_indices.insert(clip.id.clone(), next_input_index);
            next_input_index += 1;
            audio_clip_ids.insert(clip.id.clone());
            source_paths.push(cleaned_path);
        }
    }
    Ok(TimelineInputs {
        args,
        indices,
        audio_indices,
        visual_paths,
        source_paths,
        audio_clip_ids,
    })
}

fn detect_embedded_audio_streams(
    ffmpeg_path: &Path,
    timeline: &RenderTimeline,
    inputs: &mut TimelineInputs,
) -> Result<(), RenderErrorReport> {
    let mut probe_cache = HashMap::<PathBuf, bool>::new();
    for clip in timeline
        .clips
        .iter()
        .filter(|clip| clip.kind == RenderClipKind::Video)
    {
        if inputs.audio_indices.contains_key(&clip.id) {
            inputs.audio_clip_ids.insert(clip.id.clone());
            continue;
        }
        let Some(_input_index) = inputs.indices.get(&clip.id).copied() else {
            continue;
        };
        let Some(path) = inputs.visual_paths.get(&clip.id) else {
            return Err(invalid_timeline(format!(
                "Clip '{}' visual path is unavailable",
                clip.id
            )));
        };
        let has_audio = match probe_cache.get(path) {
            Some(value) => *value,
            None => {
                let value = probe_source_has_audio(ffmpeg_path, path)?;
                probe_cache.insert(path.clone(), value);
                value
            }
        };
        if has_audio {
            inputs.audio_clip_ids.insert(clip.id.clone());
        }
    }
    Ok(())
}

fn probe_source_has_audio(ffmpeg_path: &Path, path: &Path) -> Result<bool, RenderErrorReport> {
    let output = Command::new(ffmpeg_path)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(path)
        .output()
        .map_err(|error| {
            RenderErrorReport::new(
                "ffmpeg_unavailable",
                "FFmpeg başlatılamadı. Render motorunu kurup yeniden deneyin.",
                format!("Failed to probe {}: {error}", path.display()),
                true,
            )
        })?;
    Ok(ffmpeg_output_has_audio_stream(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

fn ffmpeg_output_has_audio_stream(stderr: &str) -> bool {
    stderr
        .lines()
        .any(|line| line.contains("Stream #") && line.contains(" Audio:"))
}

fn parse_ffmpeg_duration_ms(stderr: &str) -> Option<u64> {
    let value = stderr.lines().find_map(|line| {
        let (_, tail) = line.split_once("Duration: ")?;
        let duration = tail.split(',').next()?.trim();
        (duration != "N/A").then_some(duration)
    })?;
    let mut parts = value.split(':');
    let hours = parts.next()?.parse::<u64>().ok()?;
    let minutes = parts.next()?.parse::<u64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() || !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(
        hours
            .saturating_mul(3_600_000)
            .saturating_add(minutes.saturating_mul(60_000))
            .saturating_add((seconds * 1_000.0).round() as u64),
    )
}

fn track_is_active(track: &RenderTrack, tracks: &[RenderTrack]) -> bool {
    if track.mute {
        return false;
    }
    let has_solo = tracks
        .iter()
        .any(|candidate| !candidate.mute && candidate.solo);
    !has_solo || track.solo
}

fn track_audio_is_active(track: &RenderTrack, tracks: &[RenderTrack]) -> bool {
    track.kind != RenderTrackKind::Text && track_is_active(track, tracks)
}

fn validate_timeline(
    timeline: &RenderTimeline,
    render_duration_ms: u64,
) -> Result<(), RenderErrorReport> {
    normalize_ffmpeg_color(&timeline.background_color)?;
    let mut track_ids = HashSet::new();
    for track in &timeline.tracks {
        if track.id.trim().is_empty() || !track_ids.insert(track.id.as_str()) {
            return Err(invalid_timeline(format!(
                "Track ids must be non-empty and unique: '{}'",
                track.id
            )));
        }
        if !track.gain.is_finite() || !(0.0..=4.0).contains(&track.gain) {
            return Err(invalid_timeline(format!(
                "Track '{}' gain must be between 0 and 4",
                track.id
            )));
        }
        if !track.pan.is_finite() || !(-1.0..=1.0).contains(&track.pan) {
            return Err(invalid_timeline(format!(
                "Track '{}' pan must be between -1 and 1",
                track.id
            )));
        }
    }

    let tracks = timeline
        .tracks
        .iter()
        .map(|track| (track.id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let mut clip_ids = HashSet::new();
    for clip in &timeline.clips {
        if clip.id.trim().is_empty() || !clip_ids.insert(clip.id.as_str()) {
            return Err(invalid_timeline(format!(
                "Clip ids must be non-empty and unique: '{}'",
                clip.id
            )));
        }
        let track = tracks.get(clip.track_id.as_str()).ok_or_else(|| {
            invalid_timeline(format!(
                "Clip '{}' references missing track '{}'",
                clip.id, clip.track_id
            ))
        })?;
        let compatible = match track.kind {
            RenderTrackKind::Video => {
                matches!(clip.kind, RenderClipKind::Video | RenderClipKind::Image)
            }
            RenderTrackKind::Audio => {
                matches!(clip.kind, RenderClipKind::Audio | RenderClipKind::Video)
            }
            RenderTrackKind::Text => clip.kind == RenderClipKind::Text,
        };
        if !compatible {
            return Err(invalid_timeline(format!(
                "Clip '{}' kind {:?} is incompatible with {:?} track '{}'",
                clip.id, clip.kind, track.kind, track.id
            )));
        }
        if clip.duration_ms == 0 {
            return Err(invalid_timeline(format!(
                "Clip '{}' duration must be greater than zero",
                clip.id
            )));
        }
        if !clip.speed.is_finite() || !(0.05..=16.0).contains(&clip.speed) {
            return Err(invalid_timeline(format!(
                "Clip '{}' speed must be between 0.05 and 16",
                clip.id
            )));
        }
        if !clip.volume.is_finite() || !(0.0..=4.0).contains(&clip.volume) {
            return Err(invalid_timeline(format!(
                "Clip '{}' volume must be between 0 and 4",
                clip.id
            )));
        }
        if !clip.pan.is_finite() || !(-1.0..=1.0).contains(&clip.pan) {
            return Err(invalid_timeline(format!(
                "Clip '{}' pan must be between -1 and 1",
                clip.id
            )));
        }
        validate_viewport(&clip.id, clip.kind, clip.viewport.as_ref())?;
        validate_transform(&clip.id, &clip.transform)?;
        validate_keyframes(clip)?;
        if clip.kind == RenderClipKind::Text {
            let text = clip.text.as_ref().ok_or_else(|| {
                invalid_timeline(format!("Text clip '{}' has no text payload", clip.id))
            })?;
            if text.content.trim().is_empty() {
                return Err(invalid_timeline(format!(
                    "Text clip '{}' content is empty",
                    clip.id
                )));
            }
            if text.content.len() > 1_000_000 || text.content.contains('\0') {
                return Err(invalid_timeline(format!(
                    "Text clip '{}' content is too large or contains a null byte",
                    clip.id
                )));
            }
            if !text.font_size.is_finite() || !(4.0..=512.0).contains(&text.font_size) {
                return Err(invalid_timeline(format!(
                    "Text clip '{}' fontSize must be between 4 and 512",
                    clip.id
                )));
            }
            if text.font_family.trim().is_empty() || text.font_family.contains('\0') {
                return Err(invalid_timeline(format!(
                    "Text clip '{}' fontFamily is invalid",
                    clip.id
                )));
            }
            if !(1..=1_000).contains(&text.font_weight) {
                return Err(invalid_timeline(format!(
                    "Text clip '{}' fontWeight must be between 1 and 1000",
                    clip.id
                )));
            }
            if !matches!(text.align.as_str(), "left" | "center" | "right") {
                return Err(invalid_timeline(format!(
                    "Text clip '{}' alignment must be left, center, or right",
                    clip.id
                )));
            }
            normalize_ffmpeg_color(&text.color)?;
            if !text.background_color.eq_ignore_ascii_case("transparent") {
                normalize_ffmpeg_color(&text.background_color)?;
            }
            if let Some(font_path) = text.font_path.as_deref() {
                if !Path::new(font_path).is_file() {
                    return Err(invalid_timeline(format!(
                        "Text clip '{}' font does not exist: {font_path}",
                        clip.id
                    )));
                }
            }
        } else if clip.start_ms < render_duration_ms && clip.path.is_none() {
            return Err(invalid_timeline(format!(
                "Media clip '{}' has no path",
                clip.id
            )));
        }
    }
    Ok(())
}

fn validate_viewport(
    id: &str,
    kind: RenderClipKind,
    viewport: Option<&RenderViewport>,
) -> Result<(), RenderErrorReport> {
    let Some(viewport) = viewport else {
        return Ok(());
    };
    if !matches!(kind, RenderClipKind::Video | RenderClipKind::Image) {
        return Err(invalid_timeline(format!(
            "Clip '{id}' can only use a viewport when it is video or image media"
        )));
    }
    let finite = [viewport.x, viewport.y, viewport.width, viewport.height]
        .into_iter()
        .all(f64::is_finite);
    let right = viewport.x + viewport.width;
    let bottom = viewport.y + viewport.height;
    if !finite
        || viewport.x < 0.0
        || viewport.y < 0.0
        || viewport.width <= 0.0
        || viewport.height <= 0.0
        || right > 1.0
        || bottom > 1.0
    {
        return Err(invalid_timeline(format!(
            "Clip '{id}' viewport must be a finite, positive rectangle inside normalized canvas bounds"
        )));
    }
    Ok(())
}

fn viewport_pixel_rect(
    id: &str,
    viewport: &RenderViewport,
    canvas_width: u32,
    canvas_height: u32,
) -> Result<(u32, u32, u32, u32), RenderErrorReport> {
    let left = (viewport.x * canvas_width as f64).round() as u32;
    let top = (viewport.y * canvas_height as f64).round() as u32;
    let right = ((viewport.x + viewport.width) * canvas_width as f64).round() as u32;
    let bottom = ((viewport.y + viewport.height) * canvas_height as f64).round() as u32;
    let viewport_width = right.saturating_sub(left);
    let viewport_height = bottom.saturating_sub(top);
    if viewport_width == 0 || viewport_height == 0 {
        return Err(invalid_timeline(format!(
            "Clip '{id}' viewport resolves to an empty output rectangle"
        )));
    }
    Ok((left, top, viewport_width, viewport_height))
}

fn validate_transform(id: &str, transform: &RenderTransform) -> Result<(), RenderErrorReport> {
    let finite = [
        transform.x,
        transform.y,
        transform.scale_x,
        transform.scale_y,
        transform.rotation_deg,
        transform.opacity,
    ]
    .into_iter()
    .all(f64::is_finite);
    if !finite
        || !(0.01..=20.0).contains(&transform.scale_x)
        || !(0.01..=20.0).contains(&transform.scale_y)
        || !(0.0..=1.0).contains(&transform.opacity)
    {
        return Err(invalid_timeline(format!(
            "Clip '{id}' has an invalid transform"
        )));
    }
    Ok(())
}

fn validate_keyframes(clip: &RenderClip) -> Result<(), RenderErrorReport> {
    for keyframe in &clip.keyframes {
        if keyframe.at_ms > clip.duration_ms {
            return Err(invalid_timeline(format!(
                "Clip '{}' keyframe at {}ms exceeds its duration",
                clip.id, keyframe.at_ms
            )));
        }
        for value in [
            keyframe.x,
            keyframe.y,
            keyframe.scale_x,
            keyframe.scale_y,
            keyframe.rotation_deg,
            keyframe.opacity,
            keyframe.volume,
            keyframe.rider_gain,
            keyframe.duck_gain,
            keyframe.pan,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() {
                return Err(invalid_timeline(format!(
                    "Clip '{}' has a non-finite keyframe value",
                    clip.id
                )));
            }
        }
        if keyframe
            .scale_x
            .is_some_and(|value| !(0.01..=20.0).contains(&value))
            || keyframe
                .scale_y
                .is_some_and(|value| !(0.01..=20.0).contains(&value))
            || keyframe
                .opacity
                .is_some_and(|value| !(0.0..=1.0).contains(&value))
            || keyframe
                .volume
                .is_some_and(|value| !(0.0..=4.0).contains(&value))
            || keyframe
                .rider_gain
                .is_some_and(|value| !(0.0..=4.0).contains(&value))
            || keyframe
                .duck_gain
                .is_some_and(|value| !(0.0..=1.0).contains(&value))
            || keyframe
                .pan
                .is_some_and(|value| !(-1.0..=1.0).contains(&value))
        {
            return Err(invalid_timeline(format!(
                "Clip '{}' has an out-of-range keyframe value",
                clip.id
            )));
        }
    }
    Ok(())
}

fn invalid_timeline(detail: impl Into<String>) -> RenderErrorReport {
    RenderErrorReport::new(
        "invalid_timeline",
        "Timeline render verisi geçersiz.",
        detail,
        false,
    )
}

#[derive(Clone, Copy)]
enum AnimatedProperty {
    X,
    Y,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
    Volume,
    RiderGain,
    DuckGain,
    Pan,
}

fn keyframe_value(keyframe: &RenderKeyframe, property: AnimatedProperty) -> Option<f64> {
    match property {
        AnimatedProperty::X => keyframe.x,
        AnimatedProperty::Y => keyframe.y,
        AnimatedProperty::ScaleX => keyframe.scale_x,
        AnimatedProperty::ScaleY => keyframe.scale_y,
        AnimatedProperty::Rotation => keyframe.rotation_deg,
        AnimatedProperty::Opacity => keyframe.opacity,
        AnimatedProperty::Volume => keyframe.volume,
        AnimatedProperty::RiderGain => keyframe.rider_gain,
        AnimatedProperty::DuckGain => keyframe.duck_gain,
        AnimatedProperty::Pan => keyframe.pan,
    }
}

fn keyframe_easing(keyframe: &RenderKeyframe, property: AnimatedProperty) -> KeyframeEasing {
    match property {
        AnimatedProperty::X => keyframe.easing.x,
        AnimatedProperty::Y => keyframe.easing.y,
        AnimatedProperty::ScaleX => keyframe.easing.scale_x,
        AnimatedProperty::ScaleY => keyframe.easing.scale_y,
        AnimatedProperty::Rotation => keyframe.easing.rotation_deg,
        AnimatedProperty::Opacity => keyframe.easing.opacity,
        AnimatedProperty::Volume => keyframe.easing.volume,
        AnimatedProperty::RiderGain => keyframe.easing.rider_gain,
        AnimatedProperty::DuckGain => keyframe.easing.duck_gain,
        AnimatedProperty::Pan => keyframe.easing.pan,
    }
    .unwrap_or_default()
}

fn keyframe_expression(
    clip: &RenderClip,
    property: AnimatedProperty,
    default_value: f64,
    absolute_time: bool,
) -> String {
    keyframe_expression_for_time(clip, property, default_value, absolute_time, "t")
}

fn keyframe_expression_for_time(
    clip: &RenderClip,
    property: AnimatedProperty,
    default_value: f64,
    absolute_time: bool,
    time_variable: &str,
) -> String {
    let points = clip
        .keyframes
        .iter()
        .filter_map(|keyframe| {
            keyframe_value(keyframe, property)
                .map(|value| (keyframe.at_ms, value, keyframe_easing(keyframe, property)))
        })
        .collect::<Vec<_>>();
    let time_offset = if absolute_time { clip.start_ms } else { 0 };
    keyframe_expression_from_points(points, default_value, time_offset, time_variable)
}

fn audio_automation_easing(value: &str) -> Option<KeyframeEasing> {
    match value {
        "linear" => Some(KeyframeEasing::Linear),
        "hold" => Some(KeyframeEasing::Hold),
        "ease-in" => Some(KeyframeEasing::EaseIn),
        "ease-out" => Some(KeyframeEasing::EaseOut),
        "ease-in-out" => Some(KeyframeEasing::EaseInOut),
        _ => None,
    }
}

fn audio_automation_expression(
    keyframes: &[AudioAutomationKeyframe],
    default_value: f64,
    automation_start_ms: u64,
) -> String {
    let points = keyframes
        .iter()
        .map(|keyframe| {
            (
                keyframe.at_ms,
                keyframe.value,
                audio_automation_easing(&keyframe.easing).unwrap_or_default(),
            )
        })
        .collect();
    let time_variable = if automation_start_ms == 0 {
        "t".to_owned()
    } else {
        format!("(t+{})", format_seconds(automation_start_ms))
    };
    keyframe_expression_from_points(points, default_value, 0, &time_variable)
}

fn keyframe_expression_from_points(
    mut points: Vec<(u64, f64, KeyframeEasing)>,
    default_value: f64,
    time_offset_ms: u64,
    time_variable: &str,
) -> String {
    points.sort_by_key(|(at_ms, _, _)| *at_ms);
    points.dedup_by(|a, b| {
        if a.0 == b.0 {
            a.1 = b.1;
            a.2 = b.2;
            true
        } else {
            false
        }
    });
    if points.is_empty() {
        return format_number(default_value);
    }
    if points.len() == 1 {
        return format_number(points[0].1);
    }

    let mut result = format_number(points.last().expect("points cannot be empty").1);
    for pair in points.windows(2).rev() {
        let (left_ms, left_value, left_easing) = pair[0];
        let (right_ms, right_value, _) = pair[1];
        let left_time = time_offset_ms.saturating_add(left_ms) as f64 / 1_000.0;
        let right_time = time_offset_ms.saturating_add(right_ms) as f64 / 1_000.0;
        let left = format_number(left_value);
        let delta = format_number(right_value - left_value);
        let progress = format!(
            "clip(({time_variable}-{})/({}),0,1)",
            format_number(left_time),
            format_number(right_time - left_time)
        );
        let eased = match left_easing {
            KeyframeEasing::Linear => progress.clone(),
            KeyframeEasing::Hold => "0".into(),
            KeyframeEasing::EaseIn => format!("pow({progress},2)"),
            KeyframeEasing::EaseOut => format!("1-pow(1-({progress}),2)"),
            KeyframeEasing::EaseInOut => {
                format!("if(lt({progress},0.5),2*pow({progress},2),1-pow(-2*({progress})+2,2)/2)")
            }
        };
        result = format!(
            "if(lt({time_variable},{}),{}+({})*({}),{})",
            format_number(right_time),
            left,
            delta,
            eased,
            result
        );
    }
    result
}

fn build_timeline_filter_complex(
    timeline: &RenderTimeline,
    input_indices: &HashMap<String, usize>,
    audio_input_indices: &HashMap<String, usize>,
    audio_clip_ids: &HashSet<String>,
    text_files: &HashMap<String, PathBuf>,
    request: &RenderRequest,
) -> Result<String, RenderErrorReport> {
    let (width, height) = request.preset.dimensions();
    let frame_rate = request.frame_rate;
    let duration_ms = request.duration_ms;
    let include_audio = request.include_audio;
    let fit = request.fit;
    let background = normalize_ffmpeg_color(&timeline.background_color)?;
    let tracks = timeline
        .tracks
        .iter()
        .map(|track| (track.id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let mut filters = vec![format!(
        "color=c={background}:s={width}x{height}:r={frame_rate}:d={},format=rgba[vbase0]",
        format_seconds(duration_ms)
    )];

    let mut layers = timeline
        .clips
        .iter()
        .enumerate()
        .filter_map(|(original_index, clip)| {
            let track = tracks.get(clip.track_id.as_str())?;
            (track.kind != RenderTrackKind::Audio
                && track_is_active(track, &timeline.tracks)
                && clip.start_ms < duration_ms)
                .then_some((track.z_index, clip.z_index, original_index, clip, *track))
        })
        .collect::<Vec<_>>();
    layers.sort_by_key(|(track_z, clip_z, original, _, _)| (*track_z, *clip_z, *original));

    let mut current_video = "vbase0".to_owned();
    for (layer_number, (_, _, _, clip, track)) in layers.into_iter().enumerate() {
        let next_video = format!("vbase{}", layer_number + 1);
        let visible_duration_ms = clip
            .duration_ms
            .min(duration_ms.saturating_sub(clip.start_ms));
        let start = clip.start_ms as f64 / 1_000.0;
        let end = (clip.start_ms + visible_duration_ms) as f64 / 1_000.0;
        let x = keyframe_expression(clip, AnimatedProperty::X, clip.transform.x, true);
        let y = keyframe_expression(clip, AnimatedProperty::Y, clip.transform.y, true);
        let shake = clip.transform.shake;
        let x_expr = if shake > 0.0 {
            format!("({x})+(sin(t*70.0)*0.7+sin(t*133.0)*0.3)*{shake}*0.35")
        } else {
            x.clone()
        };
        let y_expr = if shake > 0.0 {
            format!("({y})+(cos(t*63.0)*0.7+sin(t*115.0)*0.3)*{shake}*0.35")
        } else {
            y.clone()
        };

        if track.kind == RenderTrackKind::Text {
            let text = clip.text.as_ref().ok_or_else(|| {
                invalid_timeline(format!("Text clip '{}' has no payload", clip.id))
            })?;
            let text_file = text_files.get(&clip.id).ok_or_else(|| {
                invalid_timeline(format!("Text clip '{}' has no prepared text file", clip.id))
            })?;
            let base_scale =
                keyframe_expression(clip, AnimatedProperty::ScaleX, clip.transform.scale_x, true);
            let text_animation = text_animation_expressions(clip, visible_duration_ms);
            let scale = text_animation.scale_expression(&base_scale);
            let y_expr = text_animation.y_expression(&y_expr);
            let text_x = text_x_expression(&clip.id, &text.align, &scale, &x_expr)?;
            let mut options = vec![
                format!("textfile='{}'", escape_filter_path(text_file)),
                "expansion=none".into(),
                format!("fontsize='{}*({scale})'", format_number(text.font_size)),
                format!("fontcolor={}", normalize_ffmpeg_color(&text.color)?),
                format!("x='{text_x}'"),
                format!("y='(h-text_h)/2+({y_expr})'"),
                format!(
                    "alpha='{}'",
                    text_alpha_expression(clip, visible_duration_ms, &text_animation)
                ),
                format!(
                    "enable='between(t,{},{})'",
                    format_number(start),
                    format_number(end)
                ),
            ];
            if let Some(font_path) = text.font_path.as_deref() {
                options.insert(
                    2,
                    format!("fontfile='{}'", escape_filter_path(Path::new(font_path))),
                );
            } else {
                let font_pattern = format!(
                    "{}:style={}",
                    text.font_family,
                    font_style_for_weight(text.font_weight)
                );
                options.insert(
                    2,
                    format!("fontfile='{}'", escape_filter_value(&font_pattern)),
                );
            }
            if !text.background_color.eq_ignore_ascii_case("transparent") {
                let padding = (text.font_size * 0.2 * clip.transform.scale_x)
                    .max(8.0)
                    .round() as u32;
                options.extend([
                    "box=1".into(),
                    format!(
                        "boxcolor={}",
                        normalize_ffmpeg_color(&text.background_color)?
                    ),
                    format!("boxborderw={padding}"),
                ]);
            }
            if text.stroke_width > 0.0 {
                if let Some(stroke_color) = text.stroke_color.as_deref() {
                    let border = (text.stroke_width * clip.transform.scale_x)
                        .round()
                        .max(1.0) as u32;
                    options.extend([
                        format!("borderw={border}"),
                        format!("bordercolor={}", normalize_ffmpeg_color(stroke_color)?),
                    ]);
                }
            }
            if let Some(shadow_color) = text.shadow_color.as_deref() {
                let has_offset = text.shadow_offset_x.abs() > f64::EPSILON
                    || text.shadow_offset_y.abs() > f64::EPSILON;
                if !shadow_color.eq_ignore_ascii_case("transparent") && has_offset {
                    options.extend([
                        format!("shadowcolor={}", normalize_ffmpeg_color(shadow_color)?),
                        format!(
                            "shadowx={}",
                            (text.shadow_offset_x * clip.transform.scale_x).round() as i64
                        ),
                        format!(
                            "shadowy={}",
                            (text.shadow_offset_y * clip.transform.scale_x).round() as i64
                        ),
                    ]);
                }
            }
            filters.push(format!(
                "[{current_video}]drawtext={}[{next_video}]",
                options.join(":")
            ));
            current_video = next_video;
            continue;
        }

        let input_index = input_indices
            .get(&clip.id)
            .ok_or_else(|| invalid_timeline(format!("Clip '{}' has no prepared input", clip.id)))?;
        let source_duration = visible_duration_ms as f64 * clip.speed / 1_000.0;
        let trim = if clip.kind == RenderClipKind::Image {
            format!("trim=duration={}", format_seconds(visible_duration_ms))
        } else {
            format!(
                "trim=start={}:duration={}",
                format_seconds(clip.trim_in_ms),
                format_number(source_duration)
            )
        };
        let setpts = if clip.kind == RenderClipKind::Image {
            format!("setpts=PTS-STARTPTS+{}/TB", format_number(start))
        } else {
            format!(
                "setpts=(PTS-STARTPTS)/{}+{}/TB",
                format_number(clip.speed),
                format_number(start)
            )
        };
        let scale_x =
            keyframe_expression(clip, AnimatedProperty::ScaleX, clip.transform.scale_x, true);
        let scale_y =
            keyframe_expression(clip, AnimatedProperty::ScaleY, clip.transform.scale_y, true);
        let rotation = keyframe_expression(
            clip,
            AnimatedProperty::Rotation,
            clip.transform.rotation_deg,
            true,
        );
        let has_rotation = clip.transform.rotation_deg.abs() > f64::EPSILON
            || clip
                .keyframes
                .iter()
                .any(|keyframe| keyframe.rotation_deg.is_some());
        let clip_label = format!("vclip{layer_number}");
        let viewport_rect = clip
            .viewport
            .as_ref()
            .map(|viewport| viewport_pixel_rect(&clip.id, viewport, width, height))
            .transpose()?;
        let (scale_width, scale_height) = viewport_rect
            .map(|(_, _, viewport_width, viewport_height)| (viewport_width, viewport_height))
            .unwrap_or((width, height));
        let canvas_scale = match (fit, viewport_rect.is_some()) {
            (CanvasFit::Contain, _) => format!(
                "scale=w={scale_width}:h={scale_height}:force_original_aspect_ratio=decrease:force_divisible_by=2"
            ),
            (CanvasFit::Cover, false) => format!(
                "scale=w={width}:h={height}:force_original_aspect_ratio=increase:force_divisible_by=2,crop={width}:{height}"
            ),
            (CanvasFit::Cover, true) => format!(
                "scale=w={scale_width}:h={scale_height}:force_original_aspect_ratio=increase:force_divisible_by=2"
            ),
        };
        let mut clip_filters = vec![
            trim,
            setpts,
            canvas_scale,
            format!("scale=w='iw*({scale_x})':h='ih*({scale_y})':eval=frame"),
        ];
        if has_rotation {
            clip_filters.push(format!(
                "rotate=angle='({rotation})*PI/180':ow='hypot(iw,ih)':oh='hypot(iw,ih)':c=none"
            ));
        }
        if let Some((_, _, viewport_width, viewport_height)) = viewport_rect {
            clip_filters.push(format!(
                "crop=w={viewport_width}:h={viewport_height}:x='clip((iw-{viewport_width})/2-({x_expr}),0,max(0,iw-{viewport_width}))':y='clip((ih-{viewport_height})/2-({y_expr}),0,max(0,ih-{viewport_height}))'"
            ));
        }
        clip_filters.push("format=rgba".into());
        if clip
            .keyframes
            .iter()
            .any(|keyframe| keyframe.opacity.is_some())
        {
            let opacity = keyframe_expression_for_time(
                clip,
                AnimatedProperty::Opacity,
                clip.transform.opacity,
                true,
                "T",
            );
            clip_filters.push(format!(
                "geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*({opacity})'"
            ));
        } else {
            clip_filters.push(format!(
                "colorchannelmixer=aa={}",
                format_number(clip.transform.opacity)
            ));
        }
        append_video_transition_filters(
            &mut clip_filters,
            clip,
            visible_duration_ms,
            clip.start_ms,
        );
        filters.push(format!(
            "[{input_index}:v:0]{}[{clip_label}]",
            clip_filters.join(",")
        ));
        if let Some((viewport_x, viewport_y, _, _)) = viewport_rect {
            filters.push(format!(
                "[{current_video}][{clip_label}]overlay=x={viewport_x}:y={viewport_y}:eval=frame:eof_action=pass:repeatlast=0:shortest=0:format=auto:enable='between(t,{},{})'[{next_video}]",
                format_number(start),
                format_number(end)
            ));
        } else {
            filters.push(format!(
                "[{current_video}][{clip_label}]overlay=x='(W-w)/2+({x_expr})':y='(H-h)/2+({y_expr})':eval=frame:eof_action=pass:repeatlast=0:shortest=0:format=auto:enable='between(t,{},{})'[{next_video}]",
                format_number(start),
                format_number(end)
            ));
        }
        current_video = next_video;
    }
    filters.push(format!("[{current_video}]format=yuv420p[vout]"));

    if include_audio {
        let mut audio_labels = Vec::new();
        for (audio_number, clip) in timeline
            .clips
            .iter()
            .filter(|clip| {
                tracks.get(clip.track_id.as_str()).is_some_and(|track| {
                    audio_clip_ids.contains(&clip.id)
                        && track_audio_is_active(track, &timeline.tracks)
                        && clip.start_ms < duration_ms
                })
            })
            .enumerate()
        {
            let track = tracks[clip.track_id.as_str()];
            let input_index = input_indices.get(&clip.id).ok_or_else(|| {
                invalid_timeline(format!("Audio clip '{}' has no prepared input", clip.id))
            })?;
            let audio_input_index = audio_input_indices.get(&clip.id).unwrap_or(input_index);
            let visible_duration_ms = clip
                .duration_ms
                .min(duration_ms.saturating_sub(clip.start_ms));
            let source_duration = visible_duration_ms as f64 * clip.speed / 1_000.0;
            let volume = keyframe_expression(clip, AnimatedProperty::Volume, clip.volume, false);
            let rider_gain = keyframe_expression(clip, AnimatedProperty::RiderGain, 1.0, false);
            let duck_gain = keyframe_expression(clip, AnimatedProperty::DuckGain, 1.0, false);
            let clip_pan = keyframe_expression(clip, AnimatedProperty::Pan, clip.pan, false);
            let total_pan = format!("clip(({})+{},-1,1)", clip_pan, format_number(track.pan));
            let mut audio_filters = vec![
                format!(
                    "atrim=start={}:duration={}",
                    format_seconds(clip.audio_trim_in_ms.unwrap_or(clip.trim_in_ms)),
                    format_number(source_duration)
                ),
                "asetpts=PTS-STARTPTS".into(),
            ];
            audio_filters.extend(atempo_filters(clip.speed));
            audio_filters.extend([
                "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo".into(),
                format!(
                    "volume='clip(({volume})*({rider_gain})*({duck_gain})*{},0,4)':eval=frame",
                    format_number(track.gain)
                ),
                format!(
                    "aeval=exprs='val(0)*if(gt({0},0),1-({0}),1)|val(1)*if(lt({0},0),1+({0}),1)':c=stereo",
                    total_pan
                ),
            ]);
            append_audio_transition_filters(&mut audio_filters, clip, visible_duration_ms);
            audio_filters.push(format!("adelay={}:all=1", clip.start_ms));
            let label = format!("aclip{audio_number}");
            filters.push(format!(
                "[{audio_input_index}:a:0]{}[{label}]",
                audio_filters.join(",")
            ));
            audio_labels.push(label);
        }
        if audio_labels.is_empty() {
            filters.push(format!(
                "anullsrc=r=48000:cl=stereo,atrim=duration={}[aout]",
                format_seconds(duration_ms)
            ));
        } else {
            filters.push(format!(
                "{}amix=inputs={}:duration=longest:dropout_transition=0:normalize=0,atrim=duration={},aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,alimiter=limit=0.98[aout]",
                audio_labels
                    .iter()
                    .map(|label| format!("[{label}]"))
                    .collect::<String>(),
                audio_labels.len(),
                format_seconds(duration_ms)
            ));
        }
    }

    Ok(filters.join(";"))
}

fn append_video_transition_filters(
    filters: &mut Vec<String>,
    clip: &RenderClip,
    visible_duration_ms: u64,
    start_ms: u64,
) {
    let Some(transition) = clip.transition.as_ref() else {
        return;
    };
    let in_ms = transition.in_duration_ms.min(visible_duration_ms);
    if in_ms > 0 {
        append_video_transition_side(
            filters,
            transition.in_kind.unwrap_or(transition.kind),
            true,
            start_ms,
            in_ms,
        );
    }
    let out_ms = transition.out_duration_ms.min(visible_duration_ms);
    if out_ms > 0 {
        append_video_transition_side(
            filters,
            transition.out_kind.unwrap_or(transition.kind),
            false,
            start_ms + visible_duration_ms.saturating_sub(out_ms),
            out_ms,
        );
    }
}

fn append_video_transition_side(
    filters: &mut Vec<String>,
    kind: RenderTransitionKind,
    entering: bool,
    start_ms: u64,
    duration_ms: u64,
) {
    if kind == RenderTransitionKind::None || duration_ms == 0 {
        return;
    }
    let direction = if entering { "in" } else { "out" };
    match kind {
        RenderTransitionKind::None => {}
        RenderTransitionKind::Fade | RenderTransitionKind::Dissolve => filters.push(format!(
            "fade=t={direction}:st={}:d={}:alpha=1",
            format_seconds(start_ms),
            format_seconds(duration_ms)
        )),
        RenderTransitionKind::DipToBlack => filters.push(format!(
            "fade=t={direction}:st={}:d={}",
            format_seconds(start_ms),
            format_seconds(duration_ms)
        )),
        RenderTransitionKind::WipeLeft | RenderTransitionKind::WipeRight => {
            let progress = format!(
                "clip((T-{})/{},0,1)",
                format_seconds(start_ms),
                format_seconds(duration_ms)
            );
            let mask = match (kind, entering) {
                (RenderTransitionKind::WipeLeft, true) => {
                    format!("gte(X,W*(1-({progress})))")
                }
                (RenderTransitionKind::WipeLeft, false) => {
                    format!("gte(X,W*({progress}))")
                }
                (RenderTransitionKind::WipeRight, true) => {
                    format!("lte(X,W*({progress}))")
                }
                (RenderTransitionKind::WipeRight, false) => {
                    format!("lte(X,W*(1-({progress})))")
                }
                _ => unreachable!(),
            };
            filters.push(format!(
                "geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*({mask})'"
            ));
        }
    }
}

fn append_audio_transition_filters(
    filters: &mut Vec<String>,
    clip: &RenderClip,
    visible_duration_ms: u64,
) {
    let Some(transition) = clip.transition.as_ref() else {
        return;
    };
    let in_ms = transition.in_duration_ms.min(visible_duration_ms);
    if in_ms > 0 && transition.in_kind.unwrap_or(transition.kind) != RenderTransitionKind::None {
        filters.push(format!("afade=t=in:st=0:d={}", format_seconds(in_ms)));
    }
    let out_ms = transition.out_duration_ms.min(visible_duration_ms);
    if out_ms > 0 && transition.out_kind.unwrap_or(transition.kind) != RenderTransitionKind::None {
        filters.push(format!(
            "afade=t=out:st={}:d={}",
            format_seconds(visible_duration_ms.saturating_sub(out_ms)),
            format_seconds(out_ms)
        ));
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct TextAnimationExpressions {
    alpha_factors: Vec<String>,
    scale_factors: Vec<String>,
    y_offsets: Vec<String>,
}

impl TextAnimationExpressions {
    fn scale_expression(&self, base_scale: &str) -> String {
        if self.scale_factors.is_empty() {
            base_scale.to_owned()
        } else {
            format!("({base_scale})*({})", self.scale_factors.join("*"))
        }
    }

    fn y_expression(&self, base_y: &str) -> String {
        if self.y_offsets.is_empty() {
            base_y.to_owned()
        } else {
            format!("({base_y})+({})", self.y_offsets.join("+"))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextAnimationSide {
    In,
    Out,
}

fn text_animation_expressions(
    clip: &RenderClip,
    visible_duration_ms: u64,
) -> TextAnimationExpressions {
    let mut expressions = TextAnimationExpressions::default();
    let Some(text) = clip.text.as_ref() else {
        return expressions;
    };

    append_text_animation_expressions(
        &mut expressions,
        clip,
        visible_duration_ms,
        text.animation_in_kind.as_deref(),
        text.animation_in_ms.min(visible_duration_ms),
        TextAnimationSide::In,
    );
    append_text_animation_expressions(
        &mut expressions,
        clip,
        visible_duration_ms,
        text.animation_out_kind.as_deref(),
        text.animation_out_ms.min(visible_duration_ms),
        TextAnimationSide::Out,
    );
    expressions
}

fn append_text_animation_expressions(
    expressions: &mut TextAnimationExpressions,
    clip: &RenderClip,
    visible_duration_ms: u64,
    kind: Option<&str>,
    duration_ms: u64,
    side: TextAnimationSide,
) {
    let Some(kind) = kind.filter(|kind| *kind != "none") else {
        return;
    };
    if duration_ms == 0 {
        return;
    }

    // `progress` is the settled/visible direction on both sides: 0 -> 1 for
    // an entrance and 1 -> 0 for an exit. Keeping it clamped also makes every
    // expression neutral outside its own animation window.
    let start = clip.start_ms as f64 / 1_000.0;
    let end = (clip.start_ms + visible_duration_ms) as f64 / 1_000.0;
    let duration = format_seconds(duration_ms);
    let progress = match side {
        TextAnimationSide::In => format!("clip((t-{})/{duration},0,1)", format_number(start)),
        TextAnimationSide::Out => format!("clip(({}-t)/{duration},0,1)", format_number(end)),
    };
    let eased = format!("1-pow(1-({progress}),3)");

    match kind {
        "fade" => expressions.alpha_factors.push(eased),
        "slide-up" => {
            expressions.alpha_factors.push(eased.clone());
            expressions.y_offsets.push(format!("h*0.12*(1-({eased}))"));
        }
        "zoom" | "zoom-in" => {
            expressions
                .alpha_factors
                .push(format!("clip(({progress})*2.5,0,1)"));
            let scale = if side == TextAnimationSide::In {
                let delta = format!("(({progress})-1)");
                let back = format!("1+2.70158*pow({delta},3)+1.70158*pow({delta},2)");
                format!("0.25+0.75*({back})")
            } else {
                format!("0.25+0.75*({eased})")
            };
            expressions.scale_factors.push(scale);
        }
        "bounce" => {
            expressions
                .alpha_factors
                .push(format!("clip(({progress})*3,0,1)"));
            let scale = if side == TextAnimationSide::In {
                let bounce = ease_out_bounce_expression(&progress);
                format!("max(0.05,{bounce})")
            } else {
                format!("max(0.05,{eased})")
            };
            expressions.scale_factors.push(scale);
        }
        // Typewriter, 3D and spin still use the prior timing-preserving alpha
        // fallback until the drawtext export has a faithful implementation.
        _ => expressions.alpha_factors.push(progress),
    }
}

fn ease_out_bounce_expression(progress: &str) -> String {
    format!(
        "if(lt(({progress}),1/2.75),7.5625*pow(({progress}),2),\
         if(lt(({progress}),2/2.75),7.5625*pow(({progress})-1.5/2.75,2)+0.75,\
         if(lt(({progress}),2.5/2.75),7.5625*pow(({progress})-2.25/2.75,2)+0.9375,\
         7.5625*pow(({progress})-2.625/2.75,2)+0.984375)))"
    )
}

fn text_alpha_expression(
    clip: &RenderClip,
    visible_duration_ms: u64,
    text_animation: &TextAnimationExpressions,
) -> String {
    let opacity = keyframe_expression(
        clip,
        AnimatedProperty::Opacity,
        clip.transform.opacity,
        true,
    );
    let start = clip.start_ms as f64 / 1_000.0;
    let end = (clip.start_ms + visible_duration_ms) as f64 / 1_000.0;
    let mut factors = vec![format!("({opacity})")];

    if let Some(transition) = clip.transition.as_ref() {
        let in_ms = transition.in_duration_ms.min(visible_duration_ms);
        if in_ms > 0 && transition.in_kind.unwrap_or(transition.kind) != RenderTransitionKind::None
        {
            factors.push(format!(
                "clip((t-{})/{},0,1)",
                format_number(start),
                format_seconds(in_ms)
            ));
        }
        let out_ms = transition.out_duration_ms.min(visible_duration_ms);
        if out_ms > 0
            && transition.out_kind.unwrap_or(transition.kind) != RenderTransitionKind::None
        {
            factors.push(format!(
                "clip(({}-t)/{},0,1)",
                format_number(end),
                format_seconds(out_ms)
            ));
        }
    }

    factors.extend(text_animation.alpha_factors.iter().cloned());

    if factors.len() == 1 {
        return opacity;
    }
    factors.join("*")
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

fn normalize_ffmpeg_color(value: &str) -> Result<String, RenderErrorReport> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if matches!(hex.len(), 6 | 8) && hex.chars().all(|character| character.is_ascii_hexdigit()) {
        Ok(format!("0x{}", hex.to_ascii_uppercase()))
    } else {
        Err(invalid_timeline(format!(
            "Color must be #RRGGBB or #RRGGBBAA: '{value}'"
        )))
    }
}

fn escape_filter_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(':', "\\:")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('\n', "\\n")
        .replace('\r', "")
}

fn escape_filter_path(path: &Path) -> String {
    // Forward slashes avoid FFmpeg treating Windows path separators as filter
    // escapes. The drive colon is still escaped by the regular value encoder.
    escape_filter_value(&path.to_string_lossy().replace('\\', "/"))
}

fn font_style_for_weight(weight: u16) -> &'static str {
    match weight {
        1..=349 => "Light",
        350..=549 => "Regular",
        550..=649 => "SemiBold",
        650..=799 => "Bold",
        _ => "Black",
    }
}

fn text_x_expression(
    clip_id: &str,
    align: &str,
    scale: &str,
    x: &str,
) -> Result<String, RenderErrorReport> {
    match align {
        "left" => Ok(format!("(w-w*({scale}))/2+({x})")),
        "center" => Ok(format!("(w-text_w)/2+({x})")),
        "right" => Ok(format!("(w+w*({scale}))/2-text_w+({x})")),
        _ => Err(invalid_timeline(format!(
            "Text clip '{clip_id}' has unsupported alignment '{align}'"
        ))),
    }
}

fn format_number(value: f64) -> String {
    let formatted = format!("{value:.6}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".into()
    } else {
        trimmed.into()
    }
}

fn build_proxy_spec(
    app: &AppHandle,
    job_id: &str,
    request: &ProxyRequest,
    source_path: PathBuf,
    output_path: PathBuf,
) -> Result<JobSpec, RenderErrorReport> {
    let partial_path = partial_output_path(&output_path, job_id)?;
    let ffmpeg_path = resolve_ffmpeg_path(app, request.ffmpeg_path.as_deref());
    let (width, height) = request.profile.dimensions();
    let filter = format!(
        "scale=w={width}:h={height}:force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1,fps={}",
        request.frame_rate
    );
    let args = vec![
        "-hide_banner".into(),
        "-nostats".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        source_path.to_string_lossy().into_owned(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a?".into(),
        "-sn".into(),
        "-dn".into(),
        "-vf".into(),
        filter,
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "28".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-g".into(),
        request.frame_rate.to_string(),
        "-keyint_min".into(),
        request.frame_rate.to_string(),
        "-sc_threshold".into(),
        "0".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-t".into(),
        format_seconds(request.duration_ms),
        "-progress".into(),
        "pipe:1".into(),
        "-stats_period".into(),
        "0.25".into(),
        "-loglevel".into(),
        "warning".into(),
        partial_path.to_string_lossy().into_owned(),
    ];

    Ok(JobSpec {
        kind: RenderJobKind::Proxy,
        ffmpeg_path,
        output_path,
        partial_path,
        duration_ms: request.duration_ms,
        overwrite: true,
        args,
        cleanup_paths: Vec::new(),
    })
}

fn canvas_filter(width: u32, height: u32, frame_rate: u32, fit: CanvasFit) -> String {
    match fit {
        CanvasFit::Contain => format!(
            "scale=w={width}:h={height}:force_original_aspect_ratio=decrease:force_divisible_by=2,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps={frame_rate}"
        ),
        CanvasFit::Cover => format!(
            "scale=w={width}:h={height}:force_original_aspect_ratio=increase:force_divisible_by=2,crop={width}:{height},setsar=1,fps={frame_rate}"
        ),
    }
}

fn validate_duration_and_rate(duration_ms: u64, frame_rate: u32) -> Result<(), RenderErrorReport> {
    if duration_ms == 0 || duration_ms > MAX_RENDER_DURATION_MS {
        return Err(RenderErrorReport::new(
            "invalid_duration",
            "Render süresi geçersiz.",
            format!("durationMs must be between 1 and {MAX_RENDER_DURATION_MS}"),
            false,
        ));
    }
    if !(1..=120).contains(&frame_rate) {
        return Err(RenderErrorReport::new(
            "invalid_frame_rate",
            "Kare hızı geçersiz.",
            "frameRate must be between 1 and 120",
            false,
        ));
    }
    Ok(())
}

fn validate_audio_export_request(request: &AudioExportRequest) -> Result<(), RenderErrorReport> {
    validate_audio_processing_values(
        request.start_ms,
        request.duration_ms,
        request.playback_rate,
        request.volume,
    )?;
    validate_audio_automation(
        request.duration_ms,
        request.playback_rate,
        request.automation_start_ms,
        &request.volume_keyframes,
        &request.rider_gain_keyframes,
        &request.duck_gain_keyframes,
    )?;
    if let Some(pass) = request.normalization.as_ref() {
        validate_audio_loudness_pass(pass)?;
    }
    Ok(())
}

fn validate_audio_loudness_analysis_request(
    request: &AudioLoudnessAnalysisRequest,
) -> Result<(), RenderErrorReport> {
    validate_audio_processing_values(
        request.start_ms,
        request.duration_ms,
        request.playback_rate,
        request.volume,
    )?;
    validate_audio_automation(
        request.duration_ms,
        request.playback_rate,
        request.automation_start_ms,
        &request.volume_keyframes,
        &request.rider_gain_keyframes,
        &request.duck_gain_keyframes,
    )?;
    validate_audio_loudness_target(&request.target)
}

fn validate_audio_automation(
    source_duration_ms: u64,
    playback_rate: f64,
    automation_start_ms: u64,
    volume_keyframes: &[AudioAutomationKeyframe],
    rider_gain_keyframes: &[AudioAutomationKeyframe],
    duck_gain_keyframes: &[AudioAutomationKeyframe],
) -> Result<(), RenderErrorReport> {
    let output_duration_ms = ((source_duration_ms as f64 / playback_rate).round() as u64).max(1);
    if automation_start_ms > MAX_RENDER_DURATION_MS
        || automation_start_ms.saturating_add(output_duration_ms) > MAX_RENDER_DURATION_MS
    {
        return Err(RenderErrorReport::new(
            "invalid_audio_automation_range",
            "Ses otomasyonu aralığı geçersiz.",
            format!(
                "automationStartMs + output duration must not exceed {MAX_RENDER_DURATION_MS}; got {automation_start_ms} + {output_duration_ms}"
            ),
            false,
        ));
    }
    for (name, frames, maximum_gain) in [
        ("volumeKeyframes", volume_keyframes, 4.0),
        ("riderGainKeyframes", rider_gain_keyframes, 4.0),
        ("duckGainKeyframes", duck_gain_keyframes, 1.0),
    ] {
        if frames.len() > MAX_AUDIO_AUTOMATION_POINTS {
            return Err(RenderErrorReport::new(
                "too_many_audio_automation_points",
                "Ses otomasyonunda çok fazla nokta var.",
                format!(
                    "{name} contains {} points; maximum is {MAX_AUDIO_AUTOMATION_POINTS}",
                    frames.len()
                ),
                false,
            ));
        }
        for frame in frames {
            if frame.at_ms > MAX_RENDER_DURATION_MS
                || !frame.value.is_finite()
                || !(0.0..=maximum_gain).contains(&frame.value)
                || audio_automation_easing(&frame.easing).is_none()
            {
                return Err(RenderErrorReport::new(
                    "invalid_audio_automation_point",
                    "Ses otomasyonunda geçersiz bir nokta var.",
                    format!(
                        "Invalid {name} point at {}ms, value {}, easing '{}'",
                        frame.at_ms, frame.value, frame.easing
                    ),
                    false,
                ));
            }
        }
    }
    Ok(())
}

fn validate_audio_processing_values(
    start_ms: u64,
    duration_ms: u64,
    playback_rate: f64,
    volume: f64,
) -> Result<(), RenderErrorReport> {
    if duration_ms == 0 || duration_ms > MAX_RENDER_DURATION_MS {
        return Err(RenderErrorReport::new(
            "invalid_audio_duration",
            "Ses aralığı geçersiz.",
            format!("durationMs must be between 1 and {MAX_RENDER_DURATION_MS}"),
            false,
        ));
    }
    if start_ms > MAX_RENDER_DURATION_MS
        || start_ms.saturating_add(duration_ms) > MAX_RENDER_DURATION_MS
    {
        return Err(RenderErrorReport::new(
            "invalid_audio_range",
            "Ses aralığı desteklenen sınırın dışında.",
            format!(
                "startMs + durationMs must not exceed {MAX_RENDER_DURATION_MS}; got {} + {}",
                start_ms, duration_ms
            ),
            false,
        ));
    }
    if !playback_rate.is_finite() || !(0.05..=16.0).contains(&playback_rate) {
        return Err(RenderErrorReport::new(
            "invalid_audio_speed",
            "Ses hızı geçersiz.",
            "playbackRate must be a finite value between 0.05 and 16",
            false,
        ));
    }
    if !volume.is_finite() || !(0.0..=8.0).contains(&volume) {
        return Err(RenderErrorReport::new(
            "invalid_audio_volume",
            "Ses seviyesi geçersiz.",
            "volume must be a finite value between 0 and 8",
            false,
        ));
    }
    Ok(())
}

fn validate_audio_loudness_target(target: &AudioLoudnessTarget) -> Result<(), RenderErrorReport> {
    let valid = target.integrated_lufs.is_finite()
        && (-70.0..=-5.0).contains(&target.integrated_lufs)
        && target.true_peak_db.is_finite()
        && (-9.0..=0.0).contains(&target.true_peak_db)
        && target.loudness_range.is_finite()
        && (1.0..=50.0).contains(&target.loudness_range);
    if valid {
        Ok(())
    } else {
        Err(RenderErrorReport::new(
            "invalid_audio_loudness_target",
            "Ses normalizasyon hedefi geçersiz.",
            "integratedLufs must be -70..-5, truePeakDb -9..0, and loudnessRange 1..50",
            false,
        ))
    }
}

fn validate_audio_loudness_pass(pass: &AudioLoudnessPass) -> Result<(), RenderErrorReport> {
    validate_audio_loudness_target(&AudioLoudnessTarget {
        integrated_lufs: pass.integrated_lufs,
        true_peak_db: pass.true_peak_db,
        loudness_range: pass.loudness_range,
    })?;
    let valid = pass.measured_integrated_lufs.is_finite()
        && (-99.0..=0.0).contains(&pass.measured_integrated_lufs)
        && pass.measured_true_peak_db.is_finite()
        && (-99.0..=99.0).contains(&pass.measured_true_peak_db)
        && pass.measured_loudness_range.is_finite()
        && (0.0..=99.0).contains(&pass.measured_loudness_range)
        && pass.measured_threshold_lufs.is_finite()
        && (-99.0..=0.0).contains(&pass.measured_threshold_lufs)
        && pass.target_offset_db.is_finite()
        && (-99.0..=99.0).contains(&pass.target_offset_db);
    if valid {
        Ok(())
    } else {
        Err(RenderErrorReport::new(
            "invalid_audio_loudness_pass",
            "Ses normalizasyon ölçümü geçersiz. Sesi yeniden analiz edin.",
            "Measured loudnorm values are non-finite or outside FFmpeg's supported ranges",
            false,
        ))
    }
}

fn validate_source_path(value: &str) -> Result<PathBuf, RenderErrorReport> {
    if value.trim().is_empty() {
        return Err(RenderErrorReport::new(
            "missing_source",
            "Kaynak medya seçilmedi.",
            "sourcePath is empty",
            false,
        ));
    }
    let path = PathBuf::from(value);
    let metadata = path.metadata().map_err(|error| {
        RenderErrorReport::new(
            "missing_media",
            "Kaynak medya bulunamadı. Medyayı yeniden bağlayın.",
            error.to_string(),
            true,
        )
    })?;
    if !metadata.is_file() {
        return Err(RenderErrorReport::new(
            "invalid_source",
            "Kaynak medya bir dosya değil.",
            format!("Not a file: {}", path.display()),
            false,
        ));
    }
    fs::canonicalize(&path).map_err(|error| {
        RenderErrorReport::new(
            "missing_media",
            "Kaynak medya açılamadı.",
            error.to_string(),
            true,
        )
    })
}

fn validate_output_path(value: &str, sources: &[PathBuf]) -> Result<PathBuf, RenderErrorReport> {
    if value.trim().is_empty() {
        return Err(RenderErrorReport::new(
            "missing_output",
            "Çıktı konumu seçilmedi.",
            "outputPath is empty",
            false,
        ));
    }
    let path = PathBuf::from(value);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("mp4" | "mov")) {
        return Err(RenderErrorReport::new(
            "unsupported_output",
            "Çıktı uzantısı MP4 veya MOV olmalı.",
            format!("Unsupported output extension: {extension:?}"),
            false,
        ));
    }

    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if absolute.exists() {
        if let Ok(canonical_output) = fs::canonicalize(&absolute) {
            if sources
                .iter()
                .any(|source| paths_equal(&canonical_output, source))
            {
                return Err(same_path_error());
            }
        }
    } else if let (Some(parent), Some(file_name)) = (absolute.parent(), absolute.file_name()) {
        if let Ok(canonical_parent) = fs::canonicalize(parent) {
            let candidate = canonical_parent.join(file_name);
            if sources.iter().any(|source| paths_equal(&candidate, source)) {
                return Err(same_path_error());
            }
        }
    }
    Ok(absolute)
}

fn validate_audio_output_path(
    value: &str,
    sources: &[PathBuf],
) -> Result<(PathBuf, AudioExportFormat), RenderErrorReport> {
    if value.trim().is_empty() {
        return Err(RenderErrorReport::new(
            "missing_output",
            "Çıktı konumu seçilmedi.",
            "outputPath is empty",
            false,
        ));
    }
    let path = PathBuf::from(value);
    let format = AudioExportFormat::from_output_path(&path)?;
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if absolute.exists() {
        if let Ok(canonical_output) = fs::canonicalize(&absolute) {
            if sources
                .iter()
                .any(|source| paths_equal(&canonical_output, source))
            {
                return Err(same_path_error());
            }
        }
    } else if let (Some(parent), Some(file_name)) = (absolute.parent(), absolute.file_name()) {
        if let Ok(canonical_parent) = fs::canonicalize(parent) {
            let candidate = canonical_parent.join(file_name);
            if sources.iter().any(|source| paths_equal(&candidate, source)) {
                return Err(same_path_error());
            }
        }
    }
    Ok((absolute, format))
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

fn same_path_error() -> RenderErrorReport {
    RenderErrorReport::new(
        "source_equals_output",
        "Kaynak dosyanın üzerine render alınamaz.",
        "sourcePath and outputPath resolve to the same file",
        false,
    )
}

fn partial_output_path(output: &Path, job_id: &str) -> Result<PathBuf, RenderErrorReport> {
    let stem = output
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            RenderErrorReport::new(
                "invalid_output",
                "Çıktı dosya adı geçersiz.",
                format!("Invalid output path: {}", output.display()),
                false,
            )
        })?;
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            RenderErrorReport::new(
                "invalid_output",
                "Çıktı uzantısı geçersiz.",
                format!("Missing extension: {}", output.display()),
                false,
            )
        })?;
    Ok(output.with_file_name(format!(".{stem}.partial-{job_id}.{extension}")))
}

fn format_seconds(duration_ms: u64) -> String {
    format!("{}.{:03}", duration_ms / 1_000, duration_ms % 1_000)
}

pub(crate) fn resolve_ffmpeg_path(app: &AppHandle, explicit: Option<&str>) -> PathBuf {
    if let Some(path) = explicit.filter(|value| !value.trim().is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("ASTRAL_FFMPEG_PATH").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Ok(runtime_dir) = ffmpeg_runtime_dir(app) {
        let candidate = runtime_ffmpeg_path_for(&runtime_dir);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        let executable = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        for candidate in [
            resource_dir.join("bin").join(executable),
            resource_dir.join(executable),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    })
}

fn ffmpeg_runtime_dir(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join("ffmpeg-runtime"))
        .map_err(|error| {
            RenderErrorReport::new(
                "ffmpeg_setup_directory_failed",
                "FFmpeg çalışma klasörü bulunamadı.",
                error.to_string(),
                true,
            )
        })
}

fn runtime_ffmpeg_path_for(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    })
}

fn proxy_cache_dir(app: &AppHandle) -> Result<PathBuf, RenderErrorReport> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join("proxies"))
        .map_err(|error| {
            RenderErrorReport::new(
                "cache_unavailable",
                "Proxy önbellek konumu bulunamadı.",
                error.to_string(),
                true,
            )
        })
}

fn proxy_cache_key(
    source: &Path,
    profile: ProxyProfile,
    frame_rate: u32,
) -> Result<String, RenderErrorReport> {
    proxy_cache_key_with_schema(source, profile, frame_rate, PROXY_CACHE_SCHEMA_REVISION)
}

fn proxy_cache_key_with_schema(
    source: &Path,
    profile: ProxyProfile,
    frame_rate: u32,
    schema_revision: &str,
) -> Result<String, RenderErrorReport> {
    let metadata = source.metadata().map_err(cache_io_error)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    source.to_string_lossy().hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    schema_revision.hash(&mut hasher);
    profile.hash(&mut hasher);
    frame_rate.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

struct ProxyFile {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    partial: bool,
}

fn proxy_files(cache_dir: &Path) -> Result<Vec<ProxyFile>, RenderErrorReport> {
    if !cache_dir.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(cache_dir).map_err(cache_io_error)?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let is_complete_proxy = file_name.ends_with(".proxy.mp4");
        let is_partial_proxy = file_name.contains(".proxy.partial-")
            && file_name.ends_with(".mp4")
            && file_name.starts_with('.');
        if !is_complete_proxy && !is_partial_proxy {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_file() {
            files.push(ProxyFile {
                path,
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
                partial: is_partial_proxy,
            });
        }
    }
    Ok(files)
}

fn proxy_cache_stats_for(cache_dir: &Path) -> Result<ProxyCacheStats, RenderErrorReport> {
    let files = proxy_files(cache_dir)?;
    Ok(ProxyCacheStats {
        cache_dir: cache_dir.to_string_lossy().into_owned(),
        file_count: files.len() as u64,
        partial_file_count: files.iter().filter(|entry| entry.partial).count() as u64,
        total_bytes: files.iter().map(|entry| entry.bytes).sum(),
    })
}

fn cache_io_error(error: std::io::Error) -> RenderErrorReport {
    RenderErrorReport::new(
        "cache_io_error",
        "Proxy önbelleğine erişilemedi.",
        error.to_string(),
        true,
    )
}

fn ffmpeg_spawn_error(path: &Path, error: &std::io::Error) -> RenderErrorReport {
    let code = if error.kind() == std::io::ErrorKind::NotFound {
        "ffmpeg_not_found"
    } else {
        "ffmpeg_start_failed"
    };
    RenderErrorReport::new(
        code,
        if error.kind() == std::io::ErrorKind::NotFound {
            "FFmpeg bulunamadı. Uygulama paketine FFmpeg ekleyin veya yolunu yapılandırın."
        } else {
            "FFmpeg başlatılamadı."
        },
        format!("Failed to start {}: {error}", path.display()),
        true,
    )
}

fn classify_ffmpeg_error(stderr: &[String], exit_detail: &str) -> RenderErrorReport {
    let combined = stderr.join("\n");
    let lower = combined.to_ascii_lowercase();
    let (code, user_message, retryable) = if lower.contains("no space left on device") {
        ("disk_full", "Render için yeterli disk alanı yok.", true)
    } else if lower.contains("permission denied") || lower.contains("access is denied") {
        (
            "permission_denied",
            "Kaynak veya çıktı dosyasına erişim izni yok.",
            true,
        )
    } else if lower.contains("no such file or directory") || lower.contains("could not find file") {
        (
            "missing_media",
            "Render sırasında bir medya dosyası bulunamadı. Medyayı yeniden bağlayın.",
            true,
        )
    } else if lower.contains("invalid data found")
        || lower.contains("moov atom not found")
        || lower.contains("unsupported codec")
    {
        (
            "unsupported_media",
            "Bir medya dosyası bozuk veya desteklenmiyor.",
            false,
        )
    } else if lower.contains("unknown encoder") || lower.contains("encoder not found") {
        (
            "encoder_unavailable",
            "Gerekli video kodlayıcı bu FFmpeg sürümünde yok.",
            false,
        )
    } else if lower.contains("error initializing filter") || lower.contains("no such filter") {
        (
            "filter_failed",
            "Video işleme filtresi başlatılamadı.",
            false,
        )
    } else {
        ("ffmpeg_failed", "Render işlemi başarısız oldu.", true)
    };
    RenderErrorReport::new(code, user_message, exit_detail, retryable).with_stderr(stderr.to_vec())
}

fn lock_error() -> RenderErrorReport {
    RenderErrorReport::new(
        "render_state_unavailable",
        "Render durumu okunamadı.",
        "Render state mutex is poisoned",
        true,
    )
}

#[cfg(windows)]
pub(crate) fn configure_background_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn configure_background_process(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_test_dir(name: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("astral-render-{name}-{id}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn aspect_presets_have_exact_export_dimensions() {
        assert_eq!(AspectRatioPreset::Vertical9x16.dimensions(), (1080, 1920));
        assert_eq!(AspectRatioPreset::Square1x1.dimensions(), (1080, 1080));
        assert_eq!(AspectRatioPreset::Landscape16x9.dimensions(), (1920, 1080));
    }

    #[test]
    fn preset_wire_values_are_stable() {
        assert_eq!(
            serde_json::to_string(&AspectRatioPreset::Vertical9x16).unwrap(),
            "\"9:16\""
        );
        assert_eq!(
            serde_json::from_str::<AspectRatioPreset>("\"vertical_9_16\"").unwrap(),
            AspectRatioPreset::Vertical9x16
        );
    }

    #[test]
    fn viewport_uses_camel_case_wire_values_and_rejects_unsafe_bounds() {
        let clip: RenderClip = serde_json::from_value(serde_json::json!({
            "id": "face",
            "trackId": "video-track",
            "kind": "video",
            "path": "face.mp4",
            "startMs": 0,
            "durationMs": 1_000,
            "viewport": {
                "x": 0.0,
                "y": 0.5,
                "width": 1.0,
                "height": 0.5
            }
        }))
        .unwrap();

        let viewport = clip.viewport.as_ref().unwrap();
        assert_eq!(viewport.x, 0.0);
        assert_eq!(viewport.y, 0.5);
        assert_eq!(viewport.width, 1.0);
        assert_eq!(viewport.height, 0.5);
        assert!(validate_viewport(&clip.id, clip.kind, clip.viewport.as_ref()).is_ok());
        let wire = serde_json::to_value(&clip).unwrap();
        assert_eq!(wire["viewport"]["height"], 0.5);

        let mut outside = *viewport;
        outside.x = 0.25;
        assert_eq!(
            validate_viewport(&clip.id, clip.kind, Some(&outside))
                .unwrap_err()
                .code,
            "invalid_timeline"
        );
        let mut non_finite = *viewport;
        non_finite.y = f64::NAN;
        assert!(validate_viewport(&clip.id, clip.kind, Some(&non_finite)).is_err());
        assert!(validate_viewport(&clip.id, RenderClipKind::Audio, Some(viewport)).is_err());
    }

    #[test]
    fn viewport_filter_crops_to_its_pixel_rect_and_overlays_at_its_origin() {
        let clip: RenderClip = serde_json::from_value(serde_json::json!({
            "id": "face",
            "trackId": "video-track",
            "kind": "video",
            "path": "face.mp4",
            "startMs": 0,
            "durationMs": 1_000,
            "transform": {
                "x": 36.0,
                "y": -24.0,
                "scaleX": 2.0,
                "scaleY": 2.0
            },
            "viewport": {
                "x": 0.0,
                "y": 0.5,
                "width": 1.0,
                "height": 0.5
            }
        }))
        .unwrap();
        let timeline = RenderTimeline {
            tracks: vec![RenderTrack {
                id: "video-track".into(),
                kind: RenderTrackKind::Video,
                mute: false,
                solo: false,
                locked: false,
                gain: 1.0,
                pan: 0.0,
                z_index: 0,
            }],
            clips: vec![clip],
            background_color: "#000000".into(),
        };
        let request = RenderRequest {
            source_path: None,
            timeline: Some(timeline.clone()),
            output_path: "viewport.mp4".into(),
            duration_ms: 1_000,
            preset: AspectRatioPreset::Vertical9x16,
            frame_rate: 30,
            fit: CanvasFit::Contain,
            quality: ExportQuality::Draft,
            include_audio: false,
            overwrite: true,
            ffmpeg_path: None,
        };
        let graph = build_timeline_filter_complex(
            &timeline,
            &HashMap::from([("face".to_owned(), 0)]),
            &HashMap::new(),
            &HashSet::new(),
            &HashMap::new(),
            &request,
        )
        .unwrap();

        assert!(graph.contains(
            "scale=w=1080:h=960:force_original_aspect_ratio=decrease:force_divisible_by=2"
        ));
        assert!(graph.contains(
            "crop=w=1080:h=960:x='clip((iw-1080)/2-(36),0,max(0,iw-1080))':y='clip((ih-960)/2-(-24),0,max(0,ih-960))'"
        ));
        assert!(graph.contains("overlay=x=0:y=960:eval=frame"));
        assert!(!graph.contains("overlay=x='(W-w)/2+(36)'"));
    }

    #[test]
    fn rider_gain_uses_camel_case_wire_values_and_validates_the_export_range() {
        let clip: RenderClip = serde_json::from_value(serde_json::json!({
            "id": "voice",
            "trackId": "voice-track",
            "kind": "audio",
            "path": "voice.wav",
            "startMs": 0,
            "durationMs": 1_000,
            "keyframes": [
                {
                    "atMs": 0,
                    "riderGain": 0.5,
                    "easing": { "riderGain": "ease-in" }
                },
                { "atMs": 1_000, "riderGain": 1.5 }
            ]
        }))
        .unwrap();

        assert_eq!(clip.keyframes[0].rider_gain, Some(0.5));
        assert_eq!(
            clip.keyframes[0].easing.rider_gain,
            Some(KeyframeEasing::EaseIn)
        );
        assert!(validate_keyframes(&clip).is_ok());
        let wire = serde_json::to_value(&clip).unwrap();
        assert_eq!(wire["keyframes"][0]["riderGain"], 0.5);
        assert_eq!(wire["keyframes"][0]["easing"]["riderGain"], "ease-in");

        let expression = keyframe_expression(&clip, AnimatedProperty::RiderGain, 1.0, false);
        assert!(expression.contains("pow("));
        assert!(expression.contains("1.5"));

        let mut invalid = clip;
        invalid.keyframes[0].rider_gain = Some(4.01);
        assert_eq!(
            validate_keyframes(&invalid).unwrap_err().code,
            "invalid_timeline"
        );
    }

    #[test]
    fn duck_gain_uses_camel_case_wire_values_and_never_boosts() {
        let clip: RenderClip = serde_json::from_value(serde_json::json!({
            "id": "music",
            "trackId": "music-track",
            "kind": "audio",
            "path": "music.wav",
            "startMs": 0,
            "durationMs": 1_000,
            "keyframes": [
                {
                    "atMs": 0,
                    "duckGain": 1.0,
                    "easing": { "duckGain": "linear" }
                },
                { "atMs": 100, "duckGain": 0.2 },
                { "atMs": 1_000, "duckGain": 1.0 }
            ]
        }))
        .unwrap();

        assert_eq!(clip.keyframes[1].duck_gain, Some(0.2));
        assert_eq!(
            clip.keyframes[0].easing.duck_gain,
            Some(KeyframeEasing::Linear)
        );
        assert!(validate_keyframes(&clip).is_ok());
        let wire = serde_json::to_value(&clip).unwrap();
        assert_eq!(wire["keyframes"][1]["duckGain"], 0.2);
        let expression = keyframe_expression(&clip, AnimatedProperty::DuckGain, 1.0, false);
        assert!(expression.contains("0.2"), "{expression}");

        let mut invalid = clip;
        invalid.keyframes[1].duck_gain = Some(1.01);
        assert_eq!(
            validate_keyframes(&invalid).unwrap_err().code,
            "invalid_timeline"
        );
    }

    #[test]
    fn contain_filter_preserves_aspect_and_pads_canvas() {
        let filter = canvas_filter(1080, 1920, 30, CanvasFit::Contain);
        assert!(filter.contains("force_original_aspect_ratio=decrease"));
        assert!(filter.contains("pad=1080:1920"));
        assert!(filter.contains("setsar=1"));
        assert!(filter.ends_with("fps=30"));
    }

    #[test]
    fn cover_filter_fills_and_crops_canvas() {
        let filter = canvas_filter(1920, 1080, 60, CanvasFit::Cover);
        assert!(filter.contains("force_original_aspect_ratio=increase"));
        assert!(filter.contains("crop=1920:1080"));
        assert!(filter.ends_with("fps=60"));
    }

    #[test]
    fn audio_output_format_is_derived_from_the_destination_extension() {
        assert_eq!(
            AudioExportFormat::from_output_path(Path::new("voice.mp3")).unwrap(),
            AudioExportFormat::Mp3
        );
        assert_eq!(
            AudioExportFormat::from_output_path(Path::new("voice.M4A")).unwrap(),
            AudioExportFormat::M4a
        );
        assert_eq!(
            AudioExportFormat::from_output_path(Path::new("voice.wav")).unwrap(),
            AudioExportFormat::Wav
        );
        assert_eq!(
            AudioExportFormat::from_output_path(Path::new("voice.ogg"))
                .unwrap_err()
                .code,
            "unsupported_audio_output"
        );
    }

    fn test_loudness_pass() -> AudioLoudnessPass {
        AudioLoudnessPass {
            integrated_lufs: -16.0,
            true_peak_db: -1.5,
            loudness_range: 11.0,
            measured_integrated_lufs: -23.7,
            measured_true_peak_db: -4.2,
            measured_loudness_range: 6.1,
            measured_threshold_lufs: -33.8,
            target_offset_db: 0.2,
        }
    }

    #[test]
    fn loudness_pass_uses_the_flat_frontend_wire_shape() {
        let value = serde_json::to_value(test_loudness_pass()).unwrap();
        assert_eq!(value["integratedLufs"], -16.0);
        assert_eq!(value["measuredIntegratedLufs"], -23.7);
        assert!(value.get("target").is_none());
    }

    #[test]
    fn loudnorm_parser_selects_the_last_complete_measurement_from_noisy_stderr() {
        let stderr = r#"
ffmpeg banner {"unrelated":true}
{"input_i":"-31.00","input_tp":"-8.00","input_lra":"2.00","input_thresh":"-41.00","target_offset":"0.10"}
[Parsed_loudnorm] warning
{"input_i":"-23.70","input_tp":"-4.20","input_lra":"6.10","input_thresh":"-33.80","target_offset":"0.20"}
trailing malformed {"input_i":
"#;
        let measurement = parse_loudnorm_measurement(stderr).unwrap();
        assert_eq!(measurement.input_i, -23.7);
        assert_eq!(measurement.input_tp, -4.2);
        assert_eq!(measurement.input_lra, 6.1);
        assert_eq!(measurement.input_thresh, -33.8);
        assert_eq!(measurement.target_offset, 0.2);
    }

    #[test]
    fn loudnorm_parser_accepts_numeric_json_and_rejects_incomplete_or_silent_results() {
        let numeric = r#"{"input_i":-18.5,"input_tp":-2.1,"input_lra":3.0,"input_thresh":-28.5,"target_offset":0.05}"#;
        assert_eq!(parse_loudnorm_measurement(numeric).unwrap().input_i, -18.5);

        let incomplete = r#"{"input_i":"-18.5","input_tp":"-2.1"}"#;
        assert_eq!(
            parse_loudnorm_measurement(incomplete).unwrap_err().code,
            "audio_loudness_parse_failed"
        );

        let silent = r#"{"input_i":"-inf","input_tp":"-inf","input_lra":"0.00","input_thresh":"-70.00","target_offset":"inf"}"#;
        assert_eq!(
            parse_loudnorm_measurement(silent).unwrap_err().code,
            "audio_loudness_unmeasurable"
        );
    }

    #[test]
    fn loudnorm_passes_share_preprocessing_and_second_pass_uses_every_measurement() {
        let base = build_audio_preprocessing_filters(
            1_250,
            4_500,
            1.25,
            0.8,
            AudioAutomationInputs::new(0, &[], &[], &[]),
        );
        assert_eq!(base[0], "atrim=start=1.250:duration=4.500");
        assert_eq!(base[1], "asetpts=PTS-STARTPTS");
        assert!(base.iter().any(|filter| filter == "atempo=1.25"));
        assert!(base.iter().any(|filter| filter == "volume=0.8"));

        let target = AudioLoudnessTarget {
            integrated_lufs: -16.0,
            true_peak_db: -1.5,
            loudness_range: 11.0,
        };
        let mut analysis = base.clone();
        analysis.push(loudnorm_analysis_filter(&target));

        let mut first_pass = build_audio_preprocessing_filters(
            1_250,
            4_500,
            1.25,
            0.8,
            AudioAutomationInputs::new(0, &[], &[], &[]),
        );
        first_pass.push(loudnorm_analysis_filter(&target));
        assert_eq!(first_pass, analysis);

        let measurement = r#"{"input_i":"-18.2","input_tp":"-2.1","input_lra":"9.4","input_thresh":"-28.5","target_offset":"2.2"}"#;
        let parsed = parse_loudnorm_measurement(measurement).unwrap();
        let pass = AudioLoudnessPass {
            integrated_lufs: target.integrated_lufs,
            true_peak_db: target.true_peak_db,
            loudness_range: target.loudness_range,
            measured_integrated_lufs: parsed.input_i,
            measured_true_peak_db: parsed.input_tp,
            measured_loudness_range: parsed.input_lra,
            measured_threshold_lufs: parsed.input_thresh,
            target_offset_db: parsed.target_offset,
        };
        let second_pass = build_audio_export_filters(
            1_250,
            4_500,
            1.25,
            0.8,
            AudioAutomationInputs::new(0, &[], &[], &[]),
            Some(&pass),
        );
        assert_eq!(&second_pass[..base.len()], base.as_slice());
        for option in &[
            "measured_I=-18.2",
            "measured_TP=-2.1",
            "measured_LRA=9.4",
            "measured_thresh=-28.5",
        ] {
            assert!(
                second_pass.iter().any(|filter| filter.contains(option)),
                "missing {option}: {:?}",
                second_pass
            );
        }

        let export = build_audio_export_filters(
            1_250,
            4_500,
            1.25,
            0.8,
            AudioAutomationInputs::new(0, &[], &[], &[]),
            Some(&test_loudness_pass()),
        );
        assert_eq!(&export[..base.len()], base.as_slice());
        let loudnorm_index = export
            .iter()
            .position(|filter| filter.starts_with("loudnorm="))
            .unwrap();
        assert_eq!(export[loudnorm_index + 1], "aresample=48000");
        assert_eq!(
            export[loudnorm_index + 2],
            "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo"
        );
    }

    #[test]
    fn standalone_audio_filters_preserve_manual_and_rider_automation_with_subrange_offset() {
        let manual = vec![
            AudioAutomationKeyframe {
                at_ms: 0,
                value: 0.5,
                easing: "ease-in-out".into(),
            },
            AudioAutomationKeyframe {
                at_ms: 2_000,
                value: 1.5,
                easing: "linear".into(),
            },
        ];
        let rider = vec![
            AudioAutomationKeyframe {
                at_ms: 0,
                value: 1.2,
                easing: "hold".into(),
            },
            AudioAutomationKeyframe {
                at_ms: 2_000,
                value: 0.6,
                easing: "linear".into(),
            },
        ];
        let duck = vec![
            AudioAutomationKeyframe {
                at_ms: 0,
                value: 1.0,
                easing: "linear".into(),
            },
            AudioAutomationKeyframe {
                at_ms: 2_000,
                value: 0.25,
                easing: "linear".into(),
            },
        ];
        let filters = build_audio_preprocessing_filters(
            2_000,
            4_000,
            2.0,
            0.8,
            AudioAutomationInputs::new(1_000, &manual, &rider, &duck),
        );
        let dynamic = filters.last().unwrap();
        assert!(dynamic.starts_with("volume='clip(("), "{dynamic}");
        assert!(dynamic.contains("(t+1.000)"), "{dynamic}");
        assert!(dynamic.contains("pow("), "{dynamic}");
        assert!(dynamic.contains("0.25"), "{dynamic}");
        assert!(dynamic.ends_with(":eval=frame"), "{dynamic}");
        assert!(!filters.iter().any(|filter| filter == "volume=0.8"));

        let first_pass = {
            let mut value = filters.clone();
            value.push(loudnorm_analysis_filter(&AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: -1.5,
                loudness_range: 11.0,
            }));
            value
        };
        let second_pass = build_audio_export_filters(
            2_000,
            4_000,
            2.0,
            0.8,
            AudioAutomationInputs::new(1_000, &manual, &rider, &duck),
            Some(&test_loudness_pass()),
        );
        assert_eq!(&second_pass[..filters.len()], &first_pass[..filters.len()]);
    }

    #[test]
    fn audio_automation_wire_shape_and_validation_are_strict() {
        let parsed: AudioExportRequest = serde_json::from_value(serde_json::json!({
            "sourcePath": "voice.mp4",
            "outputPath": "voice.wav",
            "durationMs": 1000,
            "playbackRate": 1.0,
            "volume": 1.0,
            "automationStartMs": 500,
            "volumeKeyframes": [
                { "atMs": 0, "value": 0.8, "easing": "linear" },
                { "atMs": 5000, "value": 1.2, "easing": "ease-in-out" }
            ],
            "riderGainKeyframes": [
                { "atMs": 0, "value": 0.5, "easing": "hold" }
            ],
            "duckGainKeyframes": [
                { "atMs": 0, "value": 1.0, "easing": "linear" },
                { "atMs": 500, "value": 0.25, "easing": "ease-in-out" }
            ]
        }))
        .unwrap();
        assert_eq!(parsed.automation_start_ms, 500);
        // A future point is retained because it defines the easing segment
        // crossing the selected subrange's end.
        assert!(validate_audio_export_request(&parsed).is_ok());

        let invalid_duck = AudioExportRequest {
            duck_gain_keyframes: vec![AudioAutomationKeyframe {
                at_ms: 0,
                value: 1.01,
                easing: "linear".into(),
            }],
            ..parsed.clone()
        };
        assert_eq!(
            validate_audio_export_request(&invalid_duck)
                .unwrap_err()
                .code,
            "invalid_audio_automation_point"
        );

        let invalid_value = AudioExportRequest {
            rider_gain_keyframes: vec![AudioAutomationKeyframe {
                at_ms: 0,
                value: 4.01,
                easing: "linear".into(),
            }],
            ..parsed.clone()
        };
        assert_eq!(
            validate_audio_export_request(&invalid_value)
                .unwrap_err()
                .code,
            "invalid_audio_automation_point"
        );

        let invalid_easing = AudioExportRequest {
            rider_gain_keyframes: vec![AudioAutomationKeyframe {
                at_ms: 0,
                value: 1.0,
                easing: "bounce".into(),
            }],
            ..parsed.clone()
        };
        assert_eq!(
            validate_audio_export_request(&invalid_easing)
                .unwrap_err()
                .code,
            "invalid_audio_automation_point"
        );

        let too_many = AudioExportRequest {
            volume_keyframes: vec![
                AudioAutomationKeyframe {
                    at_ms: 0,
                    value: 1.0,
                    easing: "linear".into(),
                };
                MAX_AUDIO_AUTOMATION_POINTS + 1
            ],
            ..parsed.clone()
        };
        assert_eq!(
            validate_audio_export_request(&too_many).unwrap_err().code,
            "too_many_audio_automation_points"
        );

        let invalid_range = AudioExportRequest {
            automation_start_ms: MAX_RENDER_DURATION_MS,
            ..parsed
        };
        assert_eq!(
            validate_audio_export_request(&invalid_range)
                .unwrap_err()
                .code,
            "invalid_audio_automation_range"
        );
    }

    #[test]
    #[ignore = "requires the downloaded FFmpeg smoke runtime"]
    fn real_ffmpeg_standalone_audio_automation_applies_the_subrange_offset() {
        let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ffmpeg-smoke")
            .join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });
        assert!(ffmpeg.is_file(), "missing {}", ffmpeg.display());

        let rider = vec![
            AudioAutomationKeyframe {
                at_ms: 0,
                value: 0.25,
                easing: "hold".into(),
            },
            AudioAutomationKeyframe {
                at_ms: 2_000,
                value: 1.0,
                easing: "hold".into(),
            },
        ];
        let duck = vec![
            AudioAutomationKeyframe {
                at_ms: 0,
                value: 0.5,
                easing: "hold".into(),
            },
            AudioAutomationKeyframe {
                at_ms: 2_000,
                value: 1.0,
                easing: "hold".into(),
            },
        ];
        let filters = build_audio_export_filters(
            0,
            2_000,
            1.0,
            1.0,
            AudioAutomationInputs::new(1_000, &[], &rider, &duck),
            None,
        )
        .join(",");
        let output = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=48000:duration=4",
                "-af",
                &filters,
                "-ac",
                "1",
                "-ar",
                "48000",
                "-f",
                "f32le",
                "-acodec",
                "pcm_f32le",
                "-",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "FFmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let samples = output
            .stdout
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()) as f64)
            .collect::<Vec<_>>();
        assert!(samples.len() >= 95_000, "decoded {} samples", samples.len());
        let rms = |slice: &[f64]| {
            (slice.iter().map(|sample| sample * sample).sum::<f64>() / slice.len() as f64).sqrt()
        };
        let quiet = rms(&samples[4_800..43_200]);
        let loud = rms(&samples[52_800..91_200]);
        let ratio = loud / quiet;
        assert!((7.8..=8.2).contains(&ratio), "unexpected RMS ratio {ratio}");
    }

    #[test]
    fn loudness_targets_and_measurements_are_range_checked() {
        for valid in [
            AudioLoudnessTarget {
                integrated_lufs: -70.0,
                true_peak_db: -9.0,
                loudness_range: 1.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -5.0,
                true_peak_db: 0.0,
                loudness_range: 50.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: -1.5,
                loudness_range: 11.0,
            },
        ] {
            assert!(validate_audio_loudness_target(&valid).is_ok());
        }
        for invalid in [
            AudioLoudnessTarget {
                integrated_lufs: -70.1,
                true_peak_db: -1.5,
                loudness_range: 11.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -4.9,
                true_peak_db: -1.5,
                loudness_range: 11.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: -9.1,
                loudness_range: 11.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: 0.1,
                loudness_range: 11.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: -1.5,
                loudness_range: 0.9,
            },
            AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: -1.5,
                loudness_range: 50.1,
            },
            AudioLoudnessTarget {
                integrated_lufs: f64::NAN,
                true_peak_db: -1.5,
                loudness_range: 11.0,
            },
            AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: f64::INFINITY,
                loudness_range: 11.0,
            },
        ] {
            assert_eq!(
                validate_audio_loudness_target(&invalid).unwrap_err().code,
                "invalid_audio_loudness_target"
            );
        }

        let mut pass = test_loudness_pass();
        pass.measured_threshold_lufs = f64::INFINITY;
        assert_eq!(
            validate_audio_loudness_pass(&pass).unwrap_err().code,
            "invalid_audio_loudness_pass"
        );
    }

    #[test]
    #[ignore = "requires the downloaded FFmpeg smoke runtime"]
    fn real_ffmpeg_two_pass_loudness_normalization_hits_the_integrated_target() {
        let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ffmpeg-smoke")
            .join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });
        assert!(ffmpeg.is_file(), "missing {}", ffmpeg.display());
        let dir = temp_test_dir("loudnorm-smoke");
        let source = dir.join("source.wav");
        let normalized = dir.join("normalized.wav");
        let generated = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=997:sample_rate=48000:duration=3",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "fixture failed: {}",
            String::from_utf8_lossy(&generated.stderr)
        );

        let request = AudioLoudnessAnalysisRequest {
            source_path: source.to_string_lossy().into_owned(),
            start_ms: 0,
            duration_ms: 3_000,
            playback_rate: 1.0,
            volume: 1.0,
            automation_start_ms: 0,
            volume_keyframes: vec![],
            rider_gain_keyframes: vec![],
            duck_gain_keyframes: vec![],
            target: AudioLoudnessTarget {
                integrated_lufs: -16.0,
                true_peak_db: -1.5,
                loudness_range: 11.0,
            },
            ffmpeg_path: Some(ffmpeg.to_string_lossy().into_owned()),
        };
        let analysis = run_audio_loudness_analysis(&ffmpeg, &source, &request).unwrap();
        let filters = build_audio_export_filters(
            0,
            3_000,
            1.0,
            1.0,
            AudioAutomationInputs::new(0, &[], &[], &[]),
            Some(&analysis.pass),
        );
        let filter_graph = filters.join(",");
        let encoded = Command::new(&ffmpeg)
            .args(["-hide_banner", "-nostdin", "-y", "-i"])
            .arg(&source)
            .args(["-af", filter_graph.as_str(), "-c:a", "pcm_s16le"])
            .arg(&normalized)
            .output()
            .unwrap();
        assert!(
            encoded.status.success(),
            "second pass failed: {}",
            String::from_utf8_lossy(&encoded.stderr)
        );

        let verification = run_audio_loudness_analysis(&ffmpeg, &normalized, &request).unwrap();
        assert!(
            (verification.pass.measured_integrated_lufs - request.target.integrated_lufs).abs()
                <= 0.2,
            "verification: {:?}",
            verification.pass
        );
        assert!(
            verification.pass.measured_true_peak_db <= request.target.true_peak_db + 0.1,
            "verification: {:?}",
            verification.pass
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn audio_export_request_rejects_invalid_ranges_speed_and_gain() {
        let base = AudioExportRequest {
            source_path: "source.mp4".into(),
            output_path: "voice.m4a".into(),
            start_ms: 1_000,
            duration_ms: 5_000,
            playback_rate: 1.0,
            volume: 1.0,
            automation_start_ms: 0,
            volume_keyframes: vec![],
            rider_gain_keyframes: vec![],
            duck_gain_keyframes: vec![],
            overwrite: true,
            normalization: None,
            ffmpeg_path: None,
        };
        assert!(validate_audio_export_request(&base).is_ok());

        let invalid_range = AudioExportRequest {
            duration_ms: 0,
            ..base.clone()
        };
        assert_eq!(
            validate_audio_export_request(&invalid_range)
                .unwrap_err()
                .code,
            "invalid_audio_duration"
        );

        let invalid_speed = AudioExportRequest {
            playback_rate: 0.0,
            ..base.clone()
        };
        assert_eq!(
            validate_audio_export_request(&invalid_speed)
                .unwrap_err()
                .code,
            "invalid_audio_speed"
        );

        let invalid_volume = AudioExportRequest {
            volume: 8.1,
            ..base
        };
        assert_eq!(
            validate_audio_export_request(&invalid_volume)
                .unwrap_err()
                .code,
            "invalid_audio_volume"
        );
    }

    #[test]
    fn ffmpeg_stream_probe_detects_only_audio_stream_lines() {
        let with_audio = "Stream #0:0: Video: h264\n  Stream #0:1: Audio: aac, 48000 Hz";
        let without_audio = "Metadata: title=Audio: demo\n  Stream #0:0: Video: h264";
        assert!(ffmpeg_output_has_audio_stream(with_audio));
        assert!(!ffmpeg_output_has_audio_stream(without_audio));
    }

    #[test]
    fn ffmpeg_stream_probe_parses_fractional_duration() {
        let output = "Duration: 01:02:03.456, start: 0.000000, bitrate: 1200 kb/s";
        assert_eq!(parse_ffmpeg_duration_ms(output), Some(3_723_456));
        assert_eq!(parse_ffmpeg_duration_ms("Duration: N/A, start: 0"), None);
    }

    #[test]
    fn capability_listing_requires_every_pipeline_encoder_and_filter() {
        let encoders = REQUIRED_FFMPEG_ENCODERS
            .iter()
            .map(|encoder| format!(" V....D {encoder} test encoder"))
            .collect::<Vec<_>>()
            .join("\n");
        let filters = REQUIRED_FFMPEG_FILTERS
            .iter()
            .map(|filter| format!(" T.. {filter} V->V test filter"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(missing_ffmpeg_components(&encoders, &filters).is_empty());

        let without_drawtext = filters
            .lines()
            .filter(|line| !line.contains(" drawtext "))
            .collect::<Vec<_>>()
            .join("\n");
        let missing = missing_ffmpeg_components(&encoders, &without_drawtext);
        assert_eq!(missing, vec!["filter:drawtext"]);

        let without_loudnorm = filters
            .lines()
            .filter(|line| !line.contains(" loudnorm "))
            .collect::<Vec<_>>()
            .join("\n");
        let missing = missing_ffmpeg_components(&encoders, &without_loudnorm);
        assert_eq!(missing, vec!["filter:loudnorm"]);
    }

    #[test]
    fn capability_listing_does_not_accept_component_names_from_descriptions() {
        let encoders = " V....D h264_other libx264 compatibility encoder\n A..... aac AAC encoder";
        let missing = missing_ffmpeg_components(encoders, "");
        assert!(missing.contains(&"encoder:libx264".to_owned()));
    }

    #[test]
    #[ignore = "requires the downloaded FFmpeg smoke-test runtime"]
    fn real_ffmpeg_capability_probe_accepts_packaged_runtime() {
        let executable = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ffmpeg-smoke")
            .join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });
        assert!(executable.is_file(), "missing {}", executable.display());
        let version = probe_ffmpeg_capabilities(&executable).unwrap();
        assert!(version.is_some_and(|line| line.starts_with("ffmpeg version")));
    }

    #[test]
    fn progress_parser_reports_percent_speed_and_eta() {
        let mut parser = ProgressAccumulator::default();
        assert!(!parser.consume("frame=120"));
        assert!(!parser.consume("fps=48.0"));
        assert!(!parser.consume("out_time_us=5000000"));
        assert!(!parser.consume("speed=2.0x"));
        assert!(parser.consume("progress=continue"));
        let mut snapshot = RenderJobSnapshot {
            job_id: "test".into(),
            kind: RenderJobKind::Export,
            status: RenderJobStatus::Running,
            progress_percent: 0.0,
            encoded_ms: 0,
            duration_ms: 10_000,
            frame: None,
            encoding_fps: None,
            speed: None,
            eta_ms: None,
            output_path: "out.mp4".into(),
            error: None,
        };
        parser.apply_to(&mut snapshot);
        assert_eq!(snapshot.encoded_ms, 5_000);
        assert_eq!(snapshot.progress_percent, 50.0);
        assert_eq!(snapshot.frame, Some(120));
        assert_eq!(snapshot.encoding_fps, Some(48.0));
        assert_eq!(snapshot.speed, Some(2.0));
        assert_eq!(snapshot.eta_ms, Some(2_500));
    }

    #[test]
    fn partial_output_keeps_container_extension() {
        let path = partial_output_path(Path::new("C:/renders/final.mp4"), "job-42").unwrap();
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("mp4")
        );
        assert!(path.to_string_lossy().contains("partial-job-42"));
    }

    #[test]
    fn output_finalization_replaces_or_rolls_back_without_losing_existing_file() {
        let dir = temp_test_dir("finalize");
        let output = dir.join("final.mp4");
        let partial = dir.join(".final.partial-job.mp4");
        fs::write(&output, b"old").unwrap();
        fs::write(&partial, b"new").unwrap();
        finalize_output_file(&partial, &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"new");

        fs::write(&output, b"preserve-me").unwrap();
        let missing = dir.join(".missing.partial-job.mp4");
        assert!(finalize_output_file(&missing, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"preserve-me");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn validation_rejects_source_as_output() {
        let dir = temp_test_dir("same-path");
        let source = dir.join("clip.mp4");
        fs::File::create(&source)
            .unwrap()
            .write_all(b"media")
            .unwrap();
        let canonical = fs::canonicalize(&source).unwrap();
        let error =
            validate_output_path(source.to_str().unwrap(), std::slice::from_ref(&canonical))
                .unwrap_err();
        assert_eq!(error.code, "source_equals_output");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn proxy_key_changes_when_source_changes() {
        let dir = temp_test_dir("proxy-key");
        let source = dir.join("clip.mov");
        fs::File::create(&source)
            .unwrap()
            .write_all(b"first")
            .unwrap();
        let source = fs::canonicalize(source).unwrap();
        let first = proxy_cache_key(&source, ProxyProfile::P720, 30).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .unwrap()
            .write_all(b"-changed")
            .unwrap();
        let second = proxy_cache_key(&source, ProxyProfile::P720, 30).unwrap();
        assert_ne!(first, second);
        let profile_variant = proxy_cache_key(&source, ProxyProfile::P540, 30).unwrap();
        assert_ne!(second, profile_variant);
        let schema_variant =
            proxy_cache_key_with_schema(&source, ProxyProfile::P720, 30, "proxy-future").unwrap();
        assert_ne!(second, schema_variant);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn forced_proxy_refresh_keeps_previous_complete_file_until_finalization() {
        let dir = temp_test_dir("proxy-force-refresh");
        let output = dir.join("cached.720p.proxy.mp4");
        fs::write(&output, b"previous-valid-proxy").unwrap();

        assert!(proxy_cache_hit_is_reusable(&output, false));
        assert!(!proxy_cache_hit_is_reusable(&output, true));
        assert_eq!(fs::read(&output).unwrap(), b"previous-valid-proxy");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cache_stats_ignore_unrelated_files() {
        let dir = temp_test_dir("cache-stats");
        fs::write(dir.join("one.720p.proxy.mp4"), vec![0_u8; 10]).unwrap();
        fs::write(
            dir.join(".one.720p.proxy.partial-crashed-job.mp4"),
            vec![0_u8; 7],
        )
        .unwrap();
        fs::write(dir.join("notes.txt"), vec![0_u8; 50]).unwrap();
        let stats = proxy_cache_stats_for(&dir).unwrap();
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.partial_file_count, 1);
        assert_eq!(stats.total_bytes, 17);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn timeline_command_builder_composes_video_image_text_and_audio() {
        let dir = temp_test_dir("timeline-command");
        let video = dir.join("video.mp4");
        let image = dir.join("overlay.png");
        let audio = dir.join("music.wav");
        fs::write(&video, b"video").unwrap();
        fs::write(&image, b"image").unwrap();
        fs::write(&audio, b"audio").unwrap();

        let timeline = RenderTimeline {
            tracks: vec![
                RenderTrack {
                    id: "v1".into(),
                    kind: RenderTrackKind::Video,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 0,
                },
                RenderTrack {
                    id: "v2".into(),
                    kind: RenderTrackKind::Video,
                    mute: false,
                    solo: false,
                    locked: true,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 1,
                },
                RenderTrack {
                    id: "t1".into(),
                    kind: RenderTrackKind::Text,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 2,
                },
                RenderTrack {
                    id: "a1".into(),
                    kind: RenderTrackKind::Audio,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 0.8,
                    pan: -0.25,
                    z_index: 0,
                },
            ],
            clips: vec![
                RenderClip {
                    id: "video-1".into(),
                    track_id: "v1".into(),
                    kind: RenderClipKind::Video,
                    path: Some(video.to_string_lossy().into_owned()),
                    start_ms: 0,
                    duration_ms: 5_000,
                    trim_in_ms: 1_000,
                    speed: 2.0,
                    volume: 1.0,
                    pan: 0.0,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform::default(),
                    viewport: None,
                    text: None,
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::Fade,
                        in_kind: None,
                        out_kind: None,
                        in_duration_ms: 250,
                        out_duration_ms: 500,
                    }),
                    keyframes: vec![RenderKeyframe {
                        at_ms: 2_000,
                        x: Some(120.0),
                        y: None,
                        scale_x: Some(1.2),
                        scale_y: Some(1.2),
                        rotation_deg: Some(15.0),
                        opacity: Some(0.5),
                        volume: None,
                        rider_gain: None,
                        duck_gain: None,
                        pan: None,
                        easing: RenderKeyframeEasing::default(),
                    }],
                    z_index: 0,
                },
                RenderClip {
                    id: "image-1".into(),
                    track_id: "v2".into(),
                    kind: RenderClipKind::Image,
                    path: Some(image.to_string_lossy().into_owned()),
                    start_ms: 1_000,
                    duration_ms: 3_000,
                    trim_in_ms: 0,
                    speed: 1.0,
                    volume: 1.0,
                    pan: 0.0,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform {
                        x: 30.0,
                        y: -20.0,
                        scale_x: 0.5,
                        scale_y: 0.5,
                        rotation_deg: 0.0,
                        opacity: 0.75,
                        shake: 0.0,
                    },
                    viewport: None,
                    text: None,
                    transition: None,
                    keyframes: vec![],
                    z_index: 0,
                },
                RenderClip {
                    id: "text-1".into(),
                    track_id: "t1".into(),
                    kind: RenderClipKind::Text,
                    path: None,
                    start_ms: 500,
                    duration_ms: 2_000,
                    trim_in_ms: 0,
                    speed: 1.0,
                    volume: 1.0,
                    pan: 0.0,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform::default(),
                    viewport: None,
                    text: Some(RenderText {
                        content: "Astral: 'Lunar'\n%[safe]".into(),
                        font_path: None,
                        font_family: "Inter".into(),
                        font_size: 72.0,
                        font_weight: 600,
                        color: "#AABBCC".into(),
                        background_color: "#101010CC".into(),
                        align: "center".into(),
                        ..Default::default()
                    }),
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::Dissolve,
                        in_kind: None,
                        out_kind: None,
                        in_duration_ms: 250,
                        out_duration_ms: 250,
                    }),
                    keyframes: vec![],
                    z_index: 0,
                },
                RenderClip {
                    id: "audio-1".into(),
                    track_id: "a1".into(),
                    kind: RenderClipKind::Audio,
                    path: Some(audio.to_string_lossy().into_owned()),
                    start_ms: 500,
                    duration_ms: 4_000,
                    trim_in_ms: 250,
                    speed: 0.25,
                    volume: 0.5,
                    pan: 0.0,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform::default(),
                    viewport: None,
                    text: None,
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::Fade,
                        in_kind: None,
                        out_kind: None,
                        in_duration_ms: 200,
                        out_duration_ms: 400,
                    }),
                    keyframes: vec![
                        RenderKeyframe {
                            at_ms: 0,
                            x: None,
                            y: None,
                            scale_x: None,
                            scale_y: None,
                            rotation_deg: None,
                            opacity: None,
                            volume: None,
                            rider_gain: Some(0.5),
                            duck_gain: None,
                            pan: None,
                            easing: RenderKeyframeEasing {
                                rider_gain: Some(KeyframeEasing::EaseInOut),
                                ..RenderKeyframeEasing::default()
                            },
                        },
                        RenderKeyframe {
                            at_ms: 4_000,
                            x: None,
                            y: None,
                            scale_x: None,
                            scale_y: None,
                            rotation_deg: None,
                            opacity: None,
                            volume: None,
                            rider_gain: Some(1.5),
                            duck_gain: None,
                            pan: None,
                            easing: RenderKeyframeEasing::default(),
                        },
                    ],
                    z_index: 0,
                },
            ],
            background_color: "#101820".into(),
        };
        validate_timeline(&timeline, 6_000).unwrap();
        let mut inputs = build_timeline_inputs(&timeline, 6_000).unwrap();
        // The real export path populates this set through a fast FFmpeg stream probe.
        inputs.audio_clip_ids.insert("video-1".into());
        let request = RenderRequest {
            source_path: None,
            timeline: Some(timeline.clone()),
            output_path: dir.join("render.mp4").to_string_lossy().into_owned(),
            duration_ms: 6_000,
            preset: AspectRatioPreset::Vertical9x16,
            frame_rate: 30,
            fit: CanvasFit::Cover,
            quality: ExportQuality::Standard,
            include_audio: true,
            overwrite: false,
            ffmpeg_path: None,
        };
        let partial = dir.join(".render.partial-test.mp4");
        let (text_files, transient_files) = prepare_text_files(&timeline, &partial).unwrap();
        let args = build_timeline_command_args(&request, &timeline, &inputs, &text_files, &partial)
            .unwrap();
        let filter_index = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let graph = &args[filter_index + 1];

        assert_eq!(inputs.indices.len(), 3);
        assert!(args.windows(2).any(|pair| pair == ["-stream_loop", "-1"]));
        assert!(graph.contains("color=c=0x101820:s=1080x1920:r=30:d=6.000"));
        assert!(graph.contains("force_original_aspect_ratio=increase"));
        assert!(graph.contains("crop=1080:1920"));
        assert!(graph.contains("[0:v:0]trim=start=1.000:duration=10"));
        assert!(graph.contains("setpts=(PTS-STARTPTS)/2+0/TB"));
        assert!(graph.contains("geq=r='r(X,Y)'"));
        // Preview holds the first keyframe value before its timestamp; export
        // must not interpolate from the clip's base value.
        assert!(graph.contains("a='alpha(X,Y)*(0.5)'"));
        assert!(!graph.contains("if(lt(T,2)"));
        assert!(graph.contains("overlay=x="));
        let text_file = text_files.get("text-1").unwrap();
        assert_eq!(
            fs::read_to_string(text_file).unwrap(),
            "Astral: 'Lunar'\n%[safe]"
        );
        assert!(graph.contains(&format!(
            "drawtext=textfile='{}'",
            escape_filter_path(text_file)
        )));
        assert!(graph.contains("fontfile='Inter\\:style=SemiBold'"));
        assert!(graph.contains("fontsize='72*(1)'"));
        assert!(graph.contains("fontcolor=0xAABBCC"));
        assert!(graph.contains("boxcolor=0x101010CC"));
        assert!(graph.contains("boxborderw=14"));
        assert!(graph.contains("x='(w-text_w)/2+(0)'"));
        assert!(graph.contains("[2:a:0]atrim=start=0.250:duration=1"));
        assert!(graph.contains("[0:a:0]atrim=start=1.000:duration=10"));
        assert!(graph.contains("atempo=0.5,atempo=0.5"));
        assert!(graph.contains("volume='clip((0.5)*(if("));
        assert!(graph.contains(")*0.8,0,4)':eval=frame"));
        assert!(graph.contains("adelay=500:all=1"));
        assert!(graph.contains("amix=inputs=2"));
        assert!(args.windows(2).any(|pair| pair == ["-map", "[vout]"]));
        assert!(args.windows(2).any(|pair| pair == ["-map", "[aout]"]));
        assert_eq!(args.last().unwrap(), &partial.to_string_lossy());
        drop(transient_files);
        assert!(!text_file.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn text_alignment_expressions_preserve_preview_anchors() {
        assert_eq!(
            text_x_expression("title", "left", "1.5", "12").unwrap(),
            "(w-w*(1.5))/2+(12)"
        );
        assert_eq!(
            text_x_expression("title", "center", "1.5", "12").unwrap(),
            "(w-text_w)/2+(12)"
        );
        assert_eq!(
            text_x_expression("title", "right", "1.5", "12").unwrap(),
            "(w+w*(1.5))/2-text_w+(12)"
        );
        assert!(text_x_expression("title", "justify", "1", "0").is_err());
    }

    fn animated_text_clip(
        animation_in_kind: Option<&str>,
        animation_in_ms: u64,
        animation_out_kind: Option<&str>,
        animation_out_ms: u64,
    ) -> RenderClip {
        RenderClip {
            id: "caption".into(),
            track_id: "captions".into(),
            kind: RenderClipKind::Text,
            path: None,
            start_ms: 500,
            duration_ms: 2_000,
            trim_in_ms: 0,
            speed: 1.0,
            volume: 1.0,
            pan: 0.0,
            audio_path: None,
            audio_trim_in_ms: None,
            transform: RenderTransform::default(),
            viewport: None,
            text: Some(RenderText {
                content: "Merhaba world".into(),
                font_family: "Arial".into(),
                font_size: 64.0,
                font_weight: 700,
                color: "#FFFFFF".into(),
                background_color: "transparent".into(),
                align: "center".into(),
                animation_in_kind: animation_in_kind.map(str::to_owned),
                animation_in_ms,
                animation_out_kind: animation_out_kind.map(str::to_owned),
                animation_out_ms,
                ..Default::default()
            }),
            transition: None,
            keyframes: vec![],
            z_index: 0,
        }
    }

    #[test]
    fn caption_slide_up_animation_uses_canvas_relative_y_motion() {
        let clip = animated_text_clip(Some("slide-up"), 600, None, 0);
        let animation = text_animation_expressions(&clip, clip.duration_ms);

        assert_eq!(animation.alpha_factors.len(), 1);
        assert!(animation.alpha_factors[0].contains("1-pow"));
        assert!(animation.alpha_factors[0].contains("clip((t-0.5)/0.600,0,1)"));
        assert!(animation.scale_factors.is_empty());
        assert_eq!(animation.y_offsets.len(), 1);
        assert!(animation.y_offsets[0].contains("h*0.12"));
        assert!(animation.y_expression("20").contains("(20)+(h*0.12"));
    }

    #[test]
    fn caption_zoom_and_bounce_animations_drive_font_size_scale() {
        let zoom = animated_text_clip(Some("zoom"), 500, None, 0);
        let zoom_animation = text_animation_expressions(&zoom, zoom.duration_ms);
        assert_eq!(zoom_animation.scale_factors.len(), 1);
        assert!(zoom_animation.scale_factors[0].contains("2.70158"));
        assert!(zoom_animation.alpha_factors[0].contains("*2.5"));
        assert!(zoom_animation
            .scale_expression("1.25")
            .starts_with("(1.25)*(0.25+0.75"));

        let bounce = animated_text_clip(Some("bounce"), 800, None, 0);
        let bounce_animation = text_animation_expressions(&bounce, bounce.duration_ms);
        assert_eq!(bounce_animation.scale_factors.len(), 1);
        assert!(bounce_animation.scale_factors[0].contains("max(0.05"));
        assert!(bounce_animation.scale_factors[0].contains("7.5625"));
        assert!(bounce_animation.alpha_factors[0].contains("*3"));
    }

    #[test]
    fn unsupported_text_animation_keeps_the_timing_alpha_fallback() {
        let clip = animated_text_clip(Some("typewriter"), 1_000, None, 0);
        let animation = text_animation_expressions(&clip, clip.duration_ms);

        assert_eq!(animation.alpha_factors, vec!["clip((t-0.5)/1.000,0,1)"]);
        assert!(animation.scale_factors.is_empty());
        assert!(animation.y_offsets.is_empty());
    }

    #[test]
    fn drawtext_graph_embeds_caption_motion_and_scale_expressions() {
        let clip = animated_text_clip(Some("slide-up"), 600, Some("zoom"), 500);
        let timeline = RenderTimeline {
            tracks: vec![RenderTrack {
                id: "captions".into(),
                kind: RenderTrackKind::Text,
                mute: false,
                solo: false,
                locked: false,
                gain: 1.0,
                pan: 0.0,
                z_index: 0,
            }],
            clips: vec![clip],
            background_color: "#000000".into(),
        };
        let request = RenderRequest {
            source_path: None,
            timeline: Some(timeline.clone()),
            output_path: "caption-animation.mp4".into(),
            duration_ms: 3_000,
            preset: AspectRatioPreset::Vertical9x16,
            frame_rate: 30,
            fit: CanvasFit::Contain,
            quality: ExportQuality::Draft,
            include_audio: false,
            overwrite: true,
            ffmpeg_path: None,
        };
        let text_files = HashMap::from([(
            "caption".to_owned(),
            PathBuf::from("C:/tmp/caption-animation.txt"),
        )]);
        let graph = build_timeline_filter_complex(
            &timeline,
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
            &text_files,
            &request,
        )
        .unwrap();

        assert!(graph.contains("drawtext=textfile="));
        assert!(graph.contains("y='(h-text_h)/2+((0)+(h*0.12"));
        assert!(graph.contains("fontsize='64*((1)*(0.25+0.75"));
        assert!(graph.contains("alpha='(1)*"));
    }

    #[test]
    #[ignore = "requires the downloaded FFmpeg smoke runtime"]
    fn real_ffmpeg_drawtext_uses_utf8_textfile_and_typography() {
        let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ffmpeg-smoke")
            .join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });
        assert!(
            ffmpeg.is_file(),
            "downloaded FFmpeg smoke runtime is missing: {}",
            ffmpeg.display()
        );

        let dir = temp_test_dir("real-drawtext");
        let mut title = animated_text_clip(Some("bounce"), 300, Some("zoom"), 200);
        title.id = "title".into();
        title.track_id = "t1".into();
        title.start_ms = 0;
        title.duration_ms = 500;
        title.transform = RenderTransform {
            x: 24.0,
            y: -16.0,
            scale_x: 0.75,
            scale_y: 0.75,
            rotation_deg: 0.0,
            opacity: 0.9,
            shake: 0.0,
        };
        title.text = Some(RenderText {
            content: "L'été: 100% [OK]\nİkinci satır".into(),
            font_path: None,
            font_family: "Arial".into(),
            font_size: 64.0,
            font_weight: 700,
            color: "#F4F7FFFF".into(),
            background_color: "#112233CC".into(),
            align: "left".into(),
            animation_in_kind: Some("bounce".into()),
            animation_in_ms: 300,
            animation_out_kind: Some("zoom".into()),
            animation_out_ms: 200,
            ..Default::default()
        });

        let mut subtitle = animated_text_clip(Some("slide-up"), 300, Some("fade"), 200);
        subtitle.id = "subtitle".into();
        subtitle.track_id = "t1".into();
        subtitle.start_ms = 0;
        subtitle.duration_ms = 500;
        subtitle.transform.y = 160.0;

        let timeline = RenderTimeline {
            tracks: vec![RenderTrack {
                id: "t1".into(),
                kind: RenderTrackKind::Text,
                mute: false,
                solo: false,
                locked: false,
                gain: 1.0,
                pan: 0.0,
                z_index: 0,
            }],
            clips: vec![title, subtitle],
            background_color: "#000000".into(),
        };
        validate_timeline(&timeline, 500).unwrap();
        let inputs = build_timeline_inputs(&timeline, 500).unwrap();
        let request = RenderRequest {
            source_path: None,
            timeline: Some(timeline.clone()),
            output_path: dir.join("render.mp4").to_string_lossy().into_owned(),
            duration_ms: 500,
            preset: AspectRatioPreset::Square1x1,
            frame_rate: 24,
            fit: CanvasFit::Contain,
            quality: ExportQuality::Draft,
            include_audio: false,
            overwrite: true,
            ffmpeg_path: Some(ffmpeg.to_string_lossy().into_owned()),
        };
        let partial = dir.join(".render.partial-smoke.mp4");
        let (text_files, transient_files) = prepare_text_files(&timeline, &partial).unwrap();
        let text_path = text_files["title"].clone();
        let subtitle_path = text_files["subtitle"].clone();
        let args = build_timeline_command_args(&request, &timeline, &inputs, &text_files, &partial)
            .unwrap();
        let graph = &args[args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap()
            + 1];
        assert!(!graph.contains("L'été"));
        assert!(graph.contains("fontfile='Arial\\:style=Bold'"));
        assert!(graph.contains("fontsize='64*((0.75)*(max(0.05"));
        assert!(graph.contains("7.5625"));
        assert!(graph.contains("0.25+0.75"));
        assert!(graph.contains("h*0.12"));
        assert!(graph.contains("boxcolor=0x112233CC"));

        let output = Command::new(&ffmpeg).args(&args).output().unwrap();
        assert!(
            output.status.success(),
            "FFmpeg drawtext smoke failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(partial.metadata().is_ok_and(|metadata| metadata.len() > 0));
        drop(transient_files);
        assert!(!text_path.exists());
        assert!(!subtitle_path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore = "requires the downloaded FFmpeg smoke runtime"]
    fn real_ffmpeg_multilayer_compositor_mixer_pan_and_easing() {
        let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ffmpeg-smoke")
            .join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });
        assert!(
            ffmpeg.is_file(),
            "downloaded FFmpeg smoke runtime is missing: {}",
            ffmpeg.display()
        );

        let dir = temp_test_dir("real-multilayer");
        let video = dir.join("video.mp4");
        let image = dir.join("overlay.png");
        let audio = dir.join("music.wav");

        let generated_video = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=3",
                "-t",
                "3",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                "-y",
            ])
            .arg(&video)
            .output()
            .unwrap();
        assert!(
            generated_video.status.success(),
            "video fixture failed:\n{}",
            String::from_utf8_lossy(&generated_video.stderr)
        );

        let generated_image = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=0xE14D72:size=240x240:rate=1",
                "-frames:v",
                "1",
                "-update",
                "1",
                "-y",
            ])
            .arg(&image)
            .output()
            .unwrap();
        assert!(
            generated_image.status.success(),
            "image fixture failed:\n{}",
            String::from_utf8_lossy(&generated_image.stderr)
        );

        let generated_audio = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000:duration=3",
                "-c:a",
                "pcm_s16le",
                "-y",
            ])
            .arg(&audio)
            .output()
            .unwrap();
        assert!(
            generated_audio.status.success(),
            "audio fixture failed:\n{}",
            String::from_utf8_lossy(&generated_audio.stderr)
        );

        let timeline = RenderTimeline {
            tracks: vec![
                RenderTrack {
                    id: "v1".into(),
                    kind: RenderTrackKind::Video,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 0.9,
                    pan: -0.1,
                    z_index: 0,
                },
                RenderTrack {
                    id: "v2".into(),
                    kind: RenderTrackKind::Video,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 1,
                },
                RenderTrack {
                    id: "t1".into(),
                    kind: RenderTrackKind::Text,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 2,
                },
                RenderTrack {
                    id: "a1".into(),
                    kind: RenderTrackKind::Audio,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 0.65,
                    pan: 0.2,
                    z_index: 0,
                },
            ],
            clips: vec![
                RenderClip {
                    id: "video".into(),
                    track_id: "v1".into(),
                    kind: RenderClipKind::Video,
                    path: Some(video.to_string_lossy().into_owned()),
                    start_ms: 0,
                    duration_ms: 1_500,
                    trim_in_ms: 100,
                    speed: 1.0,
                    volume: 0.8,
                    pan: -0.25,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform::default(),
                    viewport: None,
                    text: None,
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::Fade,
                        in_kind: Some(RenderTransitionKind::Dissolve),
                        out_kind: Some(RenderTransitionKind::DipToBlack),
                        in_duration_ms: 180,
                        out_duration_ms: 220,
                    }),
                    keyframes: vec![
                        RenderKeyframe {
                            at_ms: 0,
                            x: Some(-80.0),
                            y: None,
                            scale_x: Some(0.9),
                            scale_y: Some(0.9),
                            rotation_deg: None,
                            opacity: Some(0.7),
                            volume: Some(0.65),
                            rider_gain: None,
                            duck_gain: None,
                            pan: Some(-0.4),
                            easing: RenderKeyframeEasing {
                                x: Some(KeyframeEasing::EaseInOut),
                                scale_x: Some(KeyframeEasing::EaseIn),
                                scale_y: Some(KeyframeEasing::EaseIn),
                                opacity: Some(KeyframeEasing::EaseOut),
                                volume: Some(KeyframeEasing::Linear),
                                pan: Some(KeyframeEasing::Hold),
                                ..RenderKeyframeEasing::default()
                            },
                        },
                        RenderKeyframe {
                            at_ms: 1_000,
                            x: Some(80.0),
                            y: None,
                            scale_x: Some(1.05),
                            scale_y: Some(1.05),
                            rotation_deg: None,
                            opacity: Some(1.0),
                            volume: Some(1.0),
                            rider_gain: None,
                            duck_gain: None,
                            pan: Some(0.4),
                            easing: RenderKeyframeEasing::default(),
                        },
                    ],
                    z_index: 0,
                },
                RenderClip {
                    id: "image".into(),
                    track_id: "v2".into(),
                    kind: RenderClipKind::Image,
                    path: Some(image.to_string_lossy().into_owned()),
                    start_ms: 250,
                    duration_ms: 1_000,
                    trim_in_ms: 0,
                    speed: 1.0,
                    volume: 1.0,
                    pan: 0.0,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform {
                        x: 180.0,
                        y: 120.0,
                        scale_x: 0.35,
                        scale_y: 0.35,
                        rotation_deg: 8.0,
                        opacity: 0.85,
                        shake: 0.0,
                    },
                    viewport: None,
                    text: None,
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::WipeLeft,
                        in_kind: Some(RenderTransitionKind::WipeLeft),
                        out_kind: Some(RenderTransitionKind::WipeRight),
                        in_duration_ms: 180,
                        out_duration_ms: 180,
                    }),
                    keyframes: vec![],
                    z_index: 1,
                },
                RenderClip {
                    id: "title".into(),
                    track_id: "t1".into(),
                    kind: RenderClipKind::Text,
                    path: None,
                    start_ms: 100,
                    duration_ms: 1_200,
                    trim_in_ms: 0,
                    speed: 1.0,
                    volume: 1.0,
                    pan: 0.0,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform {
                        x: 0.0,
                        y: -280.0,
                        scale_x: 0.8,
                        scale_y: 0.8,
                        rotation_deg: 0.0,
                        opacity: 0.95,
                        shake: 0.0,
                    },
                    viewport: None,
                    text: Some(RenderText {
                        content: "Astral'ın %100 [gerçek]\nrender testi".into(),
                        font_path: None,
                        font_family: "Arial".into(),
                        font_size: 58.0,
                        font_weight: 700,
                        color: "#FFFFFFFF".into(),
                        background_color: "#101820CC".into(),
                        align: "center".into(),
                        ..Default::default()
                    }),
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::Dissolve,
                        in_kind: Some(RenderTransitionKind::Dissolve),
                        out_kind: Some(RenderTransitionKind::Dissolve),
                        in_duration_ms: 150,
                        out_duration_ms: 150,
                    }),
                    keyframes: vec![],
                    z_index: 2,
                },
                RenderClip {
                    id: "music".into(),
                    track_id: "a1".into(),
                    kind: RenderClipKind::Audio,
                    path: Some(audio.to_string_lossy().into_owned()),
                    start_ms: 0,
                    duration_ms: 1_500,
                    trim_in_ms: 0,
                    speed: 0.8,
                    volume: 0.5,
                    pan: 0.15,
                    audio_path: None,
                    audio_trim_in_ms: None,
                    transform: RenderTransform::default(),
                    viewport: None,
                    text: None,
                    transition: Some(RenderTransition {
                        kind: RenderTransitionKind::Fade,
                        in_kind: Some(RenderTransitionKind::Fade),
                        out_kind: Some(RenderTransitionKind::Fade),
                        in_duration_ms: 180,
                        out_duration_ms: 200,
                    }),
                    keyframes: vec![
                        RenderKeyframe {
                            at_ms: 0,
                            x: None,
                            y: None,
                            scale_x: None,
                            scale_y: None,
                            rotation_deg: None,
                            opacity: None,
                            volume: Some(0.4),
                            rider_gain: None,
                            duck_gain: None,
                            pan: Some(-0.5),
                            easing: RenderKeyframeEasing {
                                volume: Some(KeyframeEasing::EaseIn),
                                pan: Some(KeyframeEasing::EaseOut),
                                ..RenderKeyframeEasing::default()
                            },
                        },
                        RenderKeyframe {
                            at_ms: 1_100,
                            x: None,
                            y: None,
                            scale_x: None,
                            scale_y: None,
                            rotation_deg: None,
                            opacity: None,
                            volume: Some(0.7),
                            rider_gain: None,
                            duck_gain: None,
                            pan: Some(0.35),
                            easing: RenderKeyframeEasing::default(),
                        },
                    ],
                    z_index: 0,
                },
            ],
            background_color: "#05080D".into(),
        };

        validate_timeline(&timeline, 1_500).unwrap();
        let mut inputs = build_timeline_inputs(&timeline, 1_500).unwrap();
        detect_embedded_audio_streams(&ffmpeg, &timeline, &mut inputs).unwrap();
        assert!(inputs.audio_clip_ids.contains("video"));
        assert!(inputs.audio_clip_ids.contains("music"));

        let request = RenderRequest {
            source_path: None,
            timeline: Some(timeline.clone()),
            output_path: dir.join("render.mp4").to_string_lossy().into_owned(),
            duration_ms: 1_500,
            preset: AspectRatioPreset::Square1x1,
            frame_rate: 24,
            fit: CanvasFit::Cover,
            quality: ExportQuality::Draft,
            include_audio: true,
            overwrite: true,
            ffmpeg_path: Some(ffmpeg.to_string_lossy().into_owned()),
        };
        let partial = dir.join(".render.partial-full-smoke.mp4");
        let (text_files, transient_files) = prepare_text_files(&timeline, &partial).unwrap();
        let text_path = text_files["title"].clone();
        let args = build_timeline_command_args(&request, &timeline, &inputs, &text_files, &partial)
            .unwrap();
        let graph = &args[args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap()
            + 1];
        assert!(graph.contains("amix=inputs=2"));
        assert!(graph.contains("aeval=exprs="));
        assert!(graph.contains("pow("));
        assert!(graph.contains("drawtext=textfile="));

        let encoded = Command::new(&ffmpeg).args(&args).output().unwrap();
        assert!(
            encoded.status.success(),
            "full FFmpeg timeline smoke failed:\n{}\nfiltergraph:\n{}",
            String::from_utf8_lossy(&encoded.stderr),
            graph
        );
        assert!(partial.metadata().is_ok_and(|metadata| metadata.len() > 0));

        let decoded = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&partial)
            .args([
                "-map", "0:v:0", "-map", "0:a:0", "-t", "0.25", "-f", "null", "-",
            ])
            .output()
            .unwrap();
        assert!(
            decoded.status.success(),
            "encoded streams could not be decoded:\n{}",
            String::from_utf8_lossy(&decoded.stderr)
        );

        let inspected = Command::new(&ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(&partial)
            .output()
            .unwrap();
        let diagnostic = String::from_utf8_lossy(&inspected.stderr);
        let duration = parse_ffmpeg_duration_ms(&diagnostic).unwrap();
        assert!(
            (1_450..=1_550).contains(&duration),
            "duration was {duration}ms"
        );
        assert!(ffmpeg_output_has_audio_stream(&diagnostic));

        drop(transient_files);
        assert!(!text_path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn solo_and_mute_tracks_are_removed_before_command_generation() {
        let dir = temp_test_dir("track-activation");
        let first = dir.join("first.wav");
        let second = dir.join("second.wav");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let timeline = RenderTimeline {
            tracks: vec![
                RenderTrack {
                    id: "a1".into(),
                    kind: RenderTrackKind::Audio,
                    mute: false,
                    solo: false,
                    locked: false,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 0,
                },
                RenderTrack {
                    id: "a2".into(),
                    kind: RenderTrackKind::Audio,
                    mute: false,
                    solo: true,
                    locked: false,
                    gain: 1.0,
                    pan: 0.0,
                    z_index: 1,
                },
            ],
            clips: vec![
                basic_audio_clip("one", "a1", &first),
                basic_audio_clip("two", "a2", &second),
            ],
            background_color: "#000000".into(),
        };
        let inputs = build_timeline_inputs(&timeline, 1_000).unwrap();
        assert_eq!(inputs.indices.len(), 1);
        assert!(!inputs.indices.contains_key("one"));
        assert!(inputs.indices.contains_key("two"));
        assert_eq!(inputs.source_paths, vec![fs::canonicalize(second).unwrap()]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn timeline_audio_uses_the_prepared_clean_asset_without_retrimming_it() {
        let dir = temp_test_dir("clean-audio-input");
        let original = dir.join("original.wav");
        let cleaned = dir.join("cleaned.wav");
        fs::write(&original, b"original").unwrap();
        fs::write(&cleaned, b"cleaned").unwrap();
        let mut clip = basic_audio_clip("voice", "a1", &original);
        clip.trim_in_ms = 2_500;
        clip.audio_path = Some(cleaned.to_string_lossy().into_owned());
        clip.audio_trim_in_ms = Some(0);
        let timeline = RenderTimeline {
            tracks: vec![RenderTrack {
                id: "a1".into(),
                kind: RenderTrackKind::Audio,
                mute: false,
                solo: false,
                locked: false,
                gain: 1.0,
                pan: 0.0,
                z_index: 0,
            }],
            clips: vec![clip],
            background_color: "#000000".into(),
        };
        let inputs = build_timeline_inputs(&timeline, 1_000).unwrap();
        let request = RenderRequest {
            source_path: None,
            timeline: None,
            output_path: dir.join("out.mp4").to_string_lossy().into_owned(),
            duration_ms: 1_000,
            preset: AspectRatioPreset::Landscape16x9,
            frame_rate: 30,
            fit: CanvasFit::Contain,
            quality: ExportQuality::Draft,
            include_audio: true,
            overwrite: true,
            ffmpeg_path: None,
        };
        let graph = build_timeline_filter_complex(
            &timeline,
            &inputs.indices,
            &inputs.audio_indices,
            &inputs.audio_clip_ids,
            &HashMap::new(),
            &request,
        )
        .unwrap();

        assert_eq!(inputs.indices["voice"], 0);
        assert_eq!(inputs.audio_indices["voice"], 1);
        assert!(graph.contains("[1:a:0]atrim=start=0.000:duration=1"));
        assert!(!graph.contains("[0:a:0]atrim=start=2.500"));
        let _ = fs::remove_dir_all(dir);
    }

    fn basic_audio_clip(id: &str, track_id: &str, path: &Path) -> RenderClip {
        RenderClip {
            id: id.into(),
            track_id: track_id.into(),
            kind: RenderClipKind::Audio,
            path: Some(path.to_string_lossy().into_owned()),
            start_ms: 0,
            duration_ms: 1_000,
            trim_in_ms: 0,
            speed: 1.0,
            volume: 1.0,
            pan: 0.0,
            audio_path: None,
            audio_trim_in_ms: None,
            transform: RenderTransform::default(),
            viewport: None,
            text: None,
            transition: None,
            keyframes: vec![],
            z_index: 0,
        }
    }

    #[test]
    fn ffmpeg_runtime_is_scoped_under_the_cache_root() {
        let root =
            Path::new("C:/Users/test/AppData/Local/com.hp.astral-lunar/cache/ffmpeg-runtime");
        let executable = runtime_ffmpeg_path_for(root);
        assert!(executable.starts_with(root));
        assert_eq!(
            executable.file_name().and_then(|value| value.to_str()),
            Some(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            })
        );
    }

    #[test]
    fn ffmpeg_errors_are_classified_for_actionable_reports() {
        let disk_full = classify_ffmpeg_error(
            &["av_interleaved_write_frame(): No space left on device".into()],
            "exit 1",
        );
        assert_eq!(disk_full.code, "disk_full");
        assert!(disk_full.retryable);

        let corrupt = classify_ffmpeg_error(
            &["Invalid data found when processing input".into()],
            "exit 1",
        );
        assert_eq!(corrupt.code, "unsupported_media");
        assert!(!corrupt.retryable);
    }
}
