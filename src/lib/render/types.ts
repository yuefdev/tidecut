import type { NoiseReductionSettings } from "$lib/editor/noise-reduction";

export type ExportPreset = "9:16" | "1:1" | "16:9";
export type RenderQuality = "draft" | "standard" | "high";
export type RenderFit = "contain" | "cover";
export type RenderStatus =
  | "queued"
  | "running"
  | "cancelling"
  | "completed"
  | "cancelled"
  | "failed";

export interface RenderErrorReport {
  code: string;
  userMessage: string;
  technicalMessage: string;
  retryable: boolean;
  stderrTail: string[] | string;
}

export interface RenderJobSnapshot {
  jobId: string;
  kind: "export" | "audio_export" | "proxy";
  status: RenderStatus;
  progressPercent: number;
  encodedMs: number;
  durationMs: number;
  frame?: number;
  encodingFps?: number;
  speed?: number;
  etaMs?: number;
  outputPath: string;
  error?: RenderErrorReport;
}

export interface RenderCapabilities {
  available: boolean;
  ffmpegPath: string;
  version?: string;
  diagnostic?: RenderErrorReport;
  presets?: Array<{ preset: ExportPreset; label: string; width: number; height: number }>;
  proxyProfiles?: string[];
  proxyCacheDir?: string;
}

export interface FfmpegSetupProgress {
  stage: "starting" | "downloading" | "unpacking" | "completed";
  downloadedBytes: number;
  totalBytes: number;
  progressPercent?: number;
  message: string;
}

export interface RenderAccepted {
  job: RenderJobSnapshot;
  cacheHit: boolean;
}

export interface RenderRequest {
  sourcePath?: string;
  outputPath: string;
  durationMs: number;
  preset: ExportPreset;
  frameRate?: number;
  fit?: RenderFit;
  quality?: RenderQuality;
  includeAudio?: boolean;
  overwrite?: boolean;
  ffmpegPath?: string;
  /** JSON timeline graph consumed by the Rust filter-graph renderer. */
  timeline?: RenderTimeline;
}

/**
 * Produces a standalone audio file from a source-media range. The output
 * extension determines the container/codec: .mp3, .m4a, or .wav.
 */
export interface AudioExportRequest {
  sourcePath: string;
  outputPath: string;
  /** First source-media millisecond to include. */
  startMs: number;
  /** Source-media duration before playback-rate adjustment. */
  durationMs: number;
  /** Timeline playback rate; 1 keeps the original speed. */
  playbackRate?: number;
  /** Linear gain applied to the exported audio. */
  volume?: number;
  /** Manual clip-volume automation in exported, post-speed time. */
  volumeKeyframes?: AudioGainKeyframe[];
  /** Separate AI Voice Rider multiplier in exported, post-speed time. */
  riderGainKeyframes?: AudioGainKeyframe[];
  /** Derived music-ducking multiplier; manual volume and Rider stay untouched. */
  duckGainKeyframes?: AudioGainKeyframe[];
  /** Clip-local post-speed time where this exported subrange begins. */
  automationStartMs?: number;
  /** Measured first-pass values for deterministic EBU R128 normalization. */
  normalization?: AudioNormalizationPass;
  /** Neural speech cleanup rendered to a shared cache asset before export. */
  noiseReduction?: NoiseReductionSettings | null;
  overwrite?: boolean;
  ffmpegPath?: string;
}

export interface AudioLoudnessTarget {
  /** Integrated loudness target in LUFS. */
  integratedLufs: number;
  /** Maximum true peak in dBTP. */
  truePeakDb: number;
  /** Loudness-range target in LU. */
  loudnessRange: number;
}

export interface AudioNormalizationPass extends AudioLoudnessTarget {
  measuredIntegratedLufs: number;
  measuredTruePeakDb: number;
  measuredLoudnessRange: number;
  measuredThresholdLufs: number;
  targetOffsetDb: number;
}

export interface AudioLoudnessAnalysisRequest {
  sourcePath: string;
  startMs: number;
  durationMs: number;
  playbackRate?: number;
  volume?: number;
  volumeKeyframes?: AudioGainKeyframe[];
  riderGainKeyframes?: AudioGainKeyframe[];
  duckGainKeyframes?: AudioGainKeyframe[];
  automationStartMs?: number;
  noiseReduction?: NoiseReductionSettings | null;
  target: AudioLoudnessTarget;
  ffmpegPath?: string;
}

export interface AudioLoudnessAnalysisResult {
  pass: AudioNormalizationPass;
  /** Safe linear clip gain suggested for timeline preview/editing. */
  recommendedGain: number;
  recommendedGainDb: number;
}

export interface AudioEnergyAnalysisRequest {
  sourcePath: string;
  startMs: number;
  /** Source duration before playback-rate adjustment. */
  durationMs: number;
  playbackRate?: number;
  ffmpegPath?: string;
  operationId?: string;
}

export interface AudioEnergyPoint {
  /** Post-speed, clip-local timestamp. */
  atMs: number;
  /** FFmpeg EBU R128 momentary loudness in dB/LUFS-like units. */
  loudnessDb: number;
}

export interface AudioEnergyAnalysisResult {
  engine: "ffmpeg-ebur128-momentary-v1";
  /** Post-speed, clip-local duration. */
  durationMs: number;
  points: AudioEnergyPoint[];
}

export interface LaughterAnalysisRequest {
  sourcePath: string;
  /** First source-media millisecond to analyze. */
  startMs: number;
  /** Source duration before playback-rate adjustment. */
  durationMs: number;
  playbackRate?: number;
  /** Raw YAMNet class-score gate. Defaults to 0.28. */
  minConfidence?: number;
  ffmpegPath?: string;
  operationId?: string;
}

export type LaughterEventKind = "laughter" | "giggle";
export type YamnetLaughterLabel =
  | "Laughter"
  | "Baby laughter"
  | "Giggle"
  | "Snicker"
  | "Belly laugh"
  | "Chuckle, chortle";

export interface LaughterAnalysisEvent {
  kind: LaughterEventKind;
  label: YamnetLaughterLabel;
  /** Post-speed, clip-local start timestamp. */
  startMs: number;
  /** Post-speed, clip-local exclusive end timestamp. */
  endMs: number;
  confidence: number;
  averageConfidence: number;
  frameCount: number;
}

export interface LaughterAnalysisResult {
  engine: "mediapipe-yamnet-v1";
  engineVersion: "0.10.35";
  model: "yamnet-audioset-521";
  durationMs: number;
  sampleRate: 16000;
  windowMs: number;
  hopMs: number;
  minConfidence: number;
  events: LaughterAnalysisEvent[];
}

export interface LaughterAnalysisProgress {
  stage:
    | "preparing"
    | "model-download"
    | "runtime-setup"
    | "extracting-audio"
    | "classifying"
    | "complete";
  progressPercent: number | null;
  message: string;
}

export interface AudioGainKeyframe {
  atMs: number;
  value: number;
  easing: KeyframeEasing;
}

export type VoiceRiderInputKeyframe = AudioGainKeyframe;

export interface VoiceRiderAnalysisRequest {
  sourcePath: string;
  /** First source-media millisecond to analyze. */
  startMs: number;
  /** Source duration before playback-rate adjustment. */
  durationMs: number;
  playbackRate: number;
  /** Manual clip gain; AI gain is returned as a separate multiplier. */
  baseVolume: number;
  /** Existing manual volume automation in clip-local time. */
  volumeKeyframes?: VoiceRiderInputKeyframe[];
  noiseReduction?: NoiseReductionSettings | null;
  targetLufs?: number;
  truePeakDb?: number;
  ffmpegPath?: string;
}

export interface VoiceRiderPoint {
  atMs: number;
  gain: number;
  speechProbability: number;
}

export interface VoiceRiderSpeechSegment {
  startMs: number;
  endMs: number;
  confidence: number;
}

export interface VoiceRiderAnalysisResult {
  model: "silero-vad-v6";
  points: VoiceRiderPoint[];
  speechSegments: VoiceRiderSpeechSegment[];
  speechCoverage: number;
  averageSpeechProbability: number;
  strongestCutDb: number;
  strongestBoostDb: number;
}

export interface SpeechActivityAnalysisRequest {
  sourcePath: string;
  startMs: number;
  /** Source-media duration before playback-rate adjustment. */
  durationMs: number;
  playbackRate?: number;
  ffmpegPath?: string;
  operationId?: string;
}

export interface SpeechActivityAnalysisResult {
  model: "silero-vad-v6";
  /** Post-speed clip-local duration. */
  durationMs: number;
  speechSegments: VoiceRiderSpeechSegment[];
  speechCoverage: number;
  averageSpeechProbability: number;
}

export interface SpeechTranscriptAnalysisRequest {
  sourcePath: string;
  startMs: number;
  /** Source-media duration before playback-rate adjustment. */
  durationMs: number;
  playbackRate?: number;
  ffmpegPath?: string;
  force?: boolean;
  /** `auto` is used by Caption QA for Turkish-English code-switch speech. */
  language?: "tr" | "auto";
  operationId?: string;
}

export interface SpeechTranscriptWord {
  text: string;
  startMs: number;
  endMs: number;
  confidence: number;
}

export interface SpeechTranscriptFillerSuggestion {
  wordIndex: number;
  text: string;
  startMs: number;
  endMs: number;
  confidence: number;
  category: "high-confidence" | "contextual";
  reviewOnly: boolean;
  reason: string;
}

export interface SpeechTranscriptAnalysisResult {
  engine: string;
  model: string;
  language: string;
  durationMs: number;
  transcript: string;
  words: SpeechTranscriptWord[];
  fillerSuggestions: SpeechTranscriptFillerSuggestion[];
  cacheHit: boolean;
}

export interface SpeechSetupProgress {
  artifact: string;
  stage: string;
  downloadedBytes: number;
  totalBytes: number;
  progressPercent?: number;
  message: string;
}

export interface BeatAnalysisRequest {
  sourcePath: string;
  startMs: number;
  durationMs: number;
  ffmpegPath?: string;
  operationId?: string;
}

export interface BeatAnalysisResult {
  bpm: number | null;
  beats: number[];
  confidences: number[];
  downbeats: number[];
  downbeatConfidences: number[];
  analyzedStartMs: number;
  analyzedDurationMs: number;
  model: string;
  sourceFingerprint: string;
  cacheHit: boolean;
}

export interface ProxyRequest {
  sourcePath: string;
  durationMs: number;
  profile?: "540p" | "720p";
  frameRate?: number;
  ffmpegPath?: string;
  force?: boolean;
}

export interface ProxyCacheStats {
  cacheDir: string;
  fileCount: number;
  partialFileCount: number;
  totalBytes: number;
}

export interface ProxyPruneResult {
  before: ProxyCacheStats;
  after: ProxyCacheStats;
  removedFiles: number;
  reclaimedBytes: number;
}

export interface RenderTimeline {
  tracks: RenderTrack[];
  clips: RenderClip[];
  backgroundColor?: string;
}

export interface RenderTrack {
  id: string;
  kind: "video" | "audio" | "text";
  mute?: boolean;
  solo?: boolean;
  locked?: boolean;
  gain?: number;
  pan?: number;
  zIndex?: number;
}

/** Normalized output-canvas rectangle used to constrain one visual layer. */
export interface RenderViewport {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RenderClip {
  id: string;
  trackId: string;
  kind: "video" | "image" | "audio" | "text";
  path?: string;
  startMs: number;
  durationMs: number;
  trimInMs?: number;
  speed?: number;
  volume?: number;
  pan?: number;
  noiseReduction?: NoiseReductionSettings | null;
  /** Ephemeral cache asset; never persisted in the project document. */
  audioPath?: string;
  audioTrimInMs?: number;
  transform?: RenderTransform;
  viewport?: RenderViewport | null;
  text?: RenderText;
  transition?: RenderTransition;
  keyframes?: RenderKeyframe[];
  zIndex?: number;
}

export interface RenderTransform {
  x?: number;
  y?: number;
  scaleX?: number;
  scaleY?: number;
  rotationDeg?: number;
  opacity?: number;
  shake?: number;
}

export interface RenderText {
  content: string;
  fontPath?: string;
  fontFamily?: string;
  fontSize?: number;
  fontWeight?: number;
  color?: string;
  backgroundColor?: string;
  align?: "left" | "center" | "right";
  /** Outline; width 0 disables. */
  strokeColor?: string;
  strokeWidth?: number;
  /** Shadow; "transparent" disables. Export ignores blur (ffmpeg drawtext). */
  shadowColor?: string;
  shadowOffsetX?: number;
  shadowOffsetY?: number;
  /** Text enter/exit animations; export approximates them as alpha fades. */
  animationInKind?: string;
  animationInMs?: number;
  animationOutKind?: string;
  animationOutMs?: number;
}

export interface RenderTransition {
  /** Legacy/default kind used when a side-specific kind is omitted. */
  kind?: RenderTransitionKind;
  inKind?: RenderTransitionKind;
  outKind?: RenderTransitionKind;
  inDurationMs?: number;
  outDurationMs?: number;
}

export type RenderTransitionKind =
  | "none"
  | "fade"
  | "dissolve"
  | "dip_to_black"
  | "wipe_left"
  | "wipe_right";

export interface RenderKeyframe {
  atMs: number;
  x?: number;
  y?: number;
  scaleX?: number;
  scaleY?: number;
  rotationDeg?: number;
  opacity?: number;
  volume?: number;
  riderGain?: number;
  duckGain?: number;
  pan?: number;
  easing?: RenderKeyframeEasing;
}

export interface RenderKeyframeEasing {
  x?: KeyframeEasing;
  y?: KeyframeEasing;
  scaleX?: KeyframeEasing;
  scaleY?: KeyframeEasing;
  rotationDeg?: KeyframeEasing;
  opacity?: KeyframeEasing;
  volume?: KeyframeEasing;
  riderGain?: KeyframeEasing;
  duckGain?: KeyframeEasing;
  pan?: KeyframeEasing;
}

export type KeyframeEasing =
  | "linear"
  | "hold"
  | "ease-in"
  | "ease-out"
  | "ease-in-out";

export interface AudioCleanupRequest {
  sourcePath: string;
  startMs: number;
  durationMs: number;
  settings: NoiseReductionSettings;
  ffmpegPath?: string;
}

export interface AudioCleanupAsset {
  outputPath: string;
  durationMs: number;
  cacheHit: boolean;
  engine: "rnnoise";
  model: "rnnoise-bd-v1";
  applied: boolean;
  speechCoverage: number;
  estimatedSnrDb: number;
  strength: number;
}

export interface MossFormerTestCleanupRequest {
  operationId: string;
  sourcePath: string;
  startMs: number;
  durationMs: number;
  ffmpegPath?: string;
  force?: boolean;
}

export type AudioEnhancementProgressPhase =
  | "queued"
  | "runtime-setup"
  | "model-download"
  | "preparing-audio"
  | "deepfilter"
  | "mossformer"
  | "mastering"
  | "finalizing"
  | "completed";

export interface AudioEnhancementProgress {
  operationId: string;
  sequence: number;
  phase: AudioEnhancementProgressPhase;
  overallProgressPercent: number | null;
  phaseProgressPercent: number | null;
  processedMs: number | null;
  totalMs: number | null;
  message: string;
}

export interface MossFormerTestCleanupAsset {
  outputPath: string;
  durationMs: number;
  cacheHit: boolean;
  engine: "deepfilter-clearvoice";
  model: "DeepFilterNet3+MossFormer2_SE_48K";
  modelRevision: string;
  experimental: true;
}

export const EXPORT_PRESETS: Record<
  ExportPreset,
  { width: number; height: number; label: string; detail: string }
> = {
  "9:16": {
    width: 1080,
    height: 1920,
    label: "Dikey",
    detail: "1080 × 1920",
  },
  "1:1": {
    width: 1080,
    height: 1080,
    label: "Kare",
    detail: "1080 × 1080",
  },
  "16:9": {
    width: 1920,
    height: 1080,
    label: "Yatay",
    detail: "1920 × 1080",
  },
};
