export const SPEECH_ANALYSIS_VERSION = 1 as const;
export const AUTO_DUCKING_VERSION = 1 as const;
export const AUTO_DUCKING_MODEL = "silero-vad-v6" as const;

const MIN_SEGMENT_SECONDS = 0.01;
const MERGE_SPEECH_GAP_SECONDS = 0.12;
const MAX_DUCKING_POINTS = 4_000;

export interface SpeechAnalysisSegment {
  /** Start in post-speed, clip-local seconds. */
  start: number;
  /** End in post-speed, clip-local seconds. */
  end: number;
  confidence: number;
}

export interface SpeechAnalysisMetadata {
  version: typeof SPEECH_ANALYSIS_VERSION;
  model: typeof AUTO_DUCKING_MODEL;
  /** Signature of the source range; deliberately excludes timeline position and gain. */
  signature: string;
  segments: SpeechAnalysisSegment[];
  speechCoverage: number;
  averageConfidence: number;
}

export interface AutoDuckingSettings {
  version: typeof AUTO_DUCKING_VERSION;
  enabled: boolean;
  /** Durable track references survive source-clip splits. */
  sourceTrackIds: string[];
  /** Negative attenuation in dB. -14 means the music floor is about 20%. */
  reductionDb: number;
  lookaheadMs: number;
  attackMs: number;
  holdMs: number;
  releaseMs: number;
  minConfidence: number;
}

export interface DuckingKeyframe {
  time: number;
  value: number;
  easing: "linear";
}

export interface DuckingGainKeyframeLike {
  time: number;
  value: number;
  easing?: string;
}

export interface DuckingTrackLike {
  id: string;
  type: "video" | "audio" | "text";
  muted?: boolean;
  solo?: boolean;
  gain?: number;
}

export interface SpeechSignatureClipLike {
  file?: string;
  trimIn?: number;
  duration?: number;
  speed?: number;
  noiseReduction?: {
    engine?: unknown;
    model?: unknown;
    strength?: unknown;
    highPassHz?: unknown;
    humFrequency?: unknown;
  } | null;
}

export interface DuckingClipLike extends SpeechSignatureClipLike {
  id: string;
  trackId: string;
  kind: "video" | "audio" | "image" | "text";
  start: number;
  duration: number;
  volume?: number;
  audioSeparated?: boolean;
  keyframes?: {
    volume?: DuckingGainKeyframeLike[];
    riderGain?: DuckingGainKeyframeLike[];
  };
  speechAnalysis?: SpeechAnalysisMetadata | null;
  autoDucking?: AutoDuckingSettings | null;
}

export interface DuckingProjectLike {
  tracks: readonly DuckingTrackLike[];
  clips: readonly DuckingClipLike[];
}

export interface AutoDuckingDiagnostics {
  configured: boolean;
  enabled: boolean;
  selectedSourceClipCount: number;
  validSourceClipCount: number;
  staleSourceClipIds: string[];
  speechSegmentCount: number;
}

export const DEFAULT_AUTO_DUCKING: AutoDuckingSettings = {
  version: AUTO_DUCKING_VERSION,
  enabled: true,
  sourceTrackIds: [],
  reductionDb: -14,
  lookaheadMs: 80,
  attackMs: 100,
  holdMs: 160,
  releaseMs: 650,
  minConfidence: 0.5,
};

export function normalizeAutoDucking(
  input: Partial<AutoDuckingSettings> | null | undefined,
): AutoDuckingSettings | null {
  if (!input) return null;
  const sourceTrackIds = Array.isArray(input.sourceTrackIds)
    ? [...new Set(input.sourceTrackIds.filter((id): id is string =>
        typeof id === "string" && id.trim().length > 0,
      ).map((id) => id.trim()))]
    : [];
  return {
    version: AUTO_DUCKING_VERSION,
    enabled: input.enabled !== false,
    sourceTrackIds,
    reductionDb: clamp(finiteOr(input.reductionDb, -14), -30, 0),
    lookaheadMs: Math.round(clamp(finiteOr(input.lookaheadMs, 80), 0, 500)),
    attackMs: Math.round(clamp(finiteOr(input.attackMs, 100), 10, 1_000)),
    holdMs: Math.round(clamp(finiteOr(input.holdMs, 160), 0, 1_000)),
    releaseMs: Math.round(clamp(finiteOr(input.releaseMs, 650), 50, 3_000)),
    minConfidence: clamp(finiteOr(input.minConfidence, 0.5), 0.2, 0.95),
  };
}

/**
 * Deterministic local-media signature for cached VAD segments. Timeline start,
 * manual volume, Rider gain and track state are intentionally excluded: they
 * can be projected again without re-running the neural model.
 */
export function speechAnalysisSignature(clip: SpeechSignatureClipLike): string {
  const cleanup = clip.noiseReduction;
  return JSON.stringify([
    "speech-analysis-v1",
    typeof clip.file === "string" ? clip.file : "",
    finiteOr(clip.trimIn, 0),
    finiteOr(clip.duration, 0),
    finiteOr(clip.speed, 1),
    cleanup
      ? [
          cleanup.engine ?? null,
          cleanup.model ?? null,
          finiteOr(cleanup.strength, 0),
          finiteOr(cleanup.highPassHz, 0),
          finiteOr(cleanup.humFrequency, 0),
        ]
      : null,
  ]);
}

export function normalizeSpeechAnalysis(
  input: Partial<SpeechAnalysisMetadata> | null | undefined,
  clip: SpeechSignatureClipLike,
): SpeechAnalysisMetadata | null {
  if (
    !input ||
    input.version !== SPEECH_ANALYSIS_VERSION ||
    input.model !== AUTO_DUCKING_MODEL ||
    input.signature !== speechAnalysisSignature(clip)
  ) {
    return null;
  }
  return createSpeechAnalysisMetadata(
    normalizeSpeechSegments(input.segments, Math.max(0, finiteOr(clip.duration, 0))),
    input.signature,
    Math.max(0, finiteOr(clip.duration, 0)),
  );
}

/** Projects an already analyzed local range without another model run. */
export function projectSpeechAnalysis(
  input: SpeechAnalysisMetadata | null | undefined,
  rangeStart: number,
  rangeEnd: number,
  signature: string,
): SpeechAnalysisMetadata | null {
  if (!input || input.model !== AUTO_DUCKING_MODEL) return null;
  const start = Math.max(0, finiteOr(rangeStart, 0));
  const end = Math.max(start, finiteOr(rangeEnd, start));
  const duration = end - start;
  const segments = input.segments
    .map((segment) => ({
      start: clamp(segment.start, start, end) - start,
      end: clamp(segment.end, start, end) - start,
      confidence: segment.confidence,
    }))
    .filter((segment) => segment.end - segment.start >= MIN_SEGMENT_SECONDS);
  return createSpeechAnalysisMetadata(segments, signature, duration);
}

export function isSpeechAnalysisCurrent(clip: DuckingClipLike): boolean {
  return Boolean(
    clip.speechAnalysis &&
    clip.speechAnalysis.version === SPEECH_ANALYSIS_VERSION &&
    clip.speechAnalysis.model === AUTO_DUCKING_MODEL &&
    clip.speechAnalysis.signature === speechAnalysisSignature(clip),
  );
}

export function getAutoDuckingDiagnostics(
  project: DuckingProjectLike,
  target: DuckingClipLike,
): AutoDuckingDiagnostics {
  const settings = normalizeAutoDucking(target.autoDucking);
  if (!settings) {
    return {
      configured: false,
      enabled: false,
      selectedSourceClipCount: 0,
      validSourceClipCount: 0,
      staleSourceClipIds: [],
      speechSegmentCount: 0,
    };
  }
  const selectedTracks = new Set(settings.sourceTrackIds);
  const influenceStart =
    target.start - (settings.lookaheadMs + settings.attackMs) / 1_000;
  const influenceEnd =
    target.start +
    target.duration +
    (settings.holdMs + settings.releaseMs) / 1_000;
  const selected = project.clips.filter((clip) =>
    clip.id !== target.id &&
    selectedTracks.has(clip.trackId) &&
    (clip.kind === "audio" || clip.kind === "video") &&
    !clip.audioSeparated &&
    clip.start < influenceEnd &&
    clip.start + clip.duration > influenceStart,
  );
  const valid = selected.filter(isSpeechAnalysisCurrent);
  return {
    configured: true,
    enabled: settings.enabled,
    selectedSourceClipCount: selected.length,
    validSourceClipCount: valid.length,
    staleSourceClipIds: selected
      .filter((clip) => !isSpeechAnalysisCurrent(clip))
      .map((clip) => clip.id),
    speechSegmentCount: valid.reduce(
      (sum, clip) => sum + (clip.speechAnalysis?.segments.length ?? 0),
      0,
    ),
  };
}

/**
 * Builds the single authoritative ducking curve used by both preview and
 * render projection. No manual clip automation is mutated.
 */
export function buildAutoDuckingEnvelope(
  project: DuckingProjectLike,
  target: DuckingClipLike,
): DuckingKeyframe[] {
  const settings = normalizeAutoDucking(target.autoDucking);
  if (
    !settings?.enabled ||
    settings.sourceTrackIds.length === 0 ||
    target.duration <= 0 ||
    (target.kind !== "audio" && target.kind !== "video")
  ) {
    return [];
  }

  const intervals = collectSpeechIntervals(project, target, settings);
  if (intervals.length === 0) return [];
  const depth = clamp(Math.pow(10, settings.reductionDb / 20), 0, 1);
  if (depth >= 1 - 1e-9) return [];
  const shapes = intervals.map((interval) => ({
    attackStart: interval.start - (settings.lookaheadMs + settings.attackMs) / 1_000,
    fullStart: interval.start - settings.lookaheadMs / 1_000,
    fullEnd: interval.end + settings.holdMs / 1_000,
    releaseEnd: interval.end + (settings.holdMs + settings.releaseMs) / 1_000,
  }));

  const localTimes = [0, target.duration];
  for (const shape of shapes) {
    for (const time of [shape.attackStart, shape.fullStart, shape.fullEnd, shape.releaseEnd]) {
      if (time >= target.start && time <= target.start + target.duration) {
        localTimes.push(time - target.start);
      }
    }
  }
  for (let index = 0; index + 1 < shapes.length; index += 1) {
    const crossing = releaseAttackCrossing(shapes[index], shapes[index + 1], depth);
    if (crossing !== null && crossing >= target.start && crossing <= target.start + target.duration) {
      localTimes.push(crossing - target.start);
    }
  }

  const frames = dedupeTimes(localTimes).map((time) => ({
    time,
    value: shapes.reduce(
      (gain, shape) => Math.min(gain, shapeGain(shape, target.start + time, depth)),
      1,
    ),
    easing: "linear" as const,
  }));
  if (frames.every((frame) => frame.value >= 1 - 1e-9)) return [];
  return capDuckingPoints(simplifyDuckingPoints(frames, 0.05), MAX_DUCKING_POINTS);
}

export function evaluateDuckingEnvelope(
  frames: readonly DuckingKeyframe[],
  time: number,
): number {
  if (frames.length === 0) return 1;
  if (time <= frames[0].time) return frames[0].value;
  const last = frames[frames.length - 1];
  if (time >= last.time) return last.value;
  const rightIndex = frames.findIndex((frame) => frame.time > time);
  const left = frames[rightIndex - 1];
  const right = frames[rightIndex];
  if (right.time <= left.time) return right.value;
  const progress = clamp((time - left.time) / (right.time - left.time), 0, 1);
  return left.value + (right.value - left.value) * progress;
}

function collectSpeechIntervals(
  project: DuckingProjectLike,
  target: DuckingClipLike,
  settings: AutoDuckingSettings,
): Array<{ start: number; end: number }> {
  const selectedTracks = new Set(settings.sourceTrackIds);
  const trackById = new Map(project.tracks.map((track) => [track.id, track]));
  const hasSolo = project.tracks.some((track) => track.solo);
  const targetStart = target.start - (settings.lookaheadMs + settings.attackMs) / 1_000;
  const targetEnd = target.start + target.duration +
    (settings.holdMs + settings.releaseMs) / 1_000;
  const intervals: Array<{ start: number; end: number }> = [];

  for (const source of project.clips) {
    if (
      source.id === target.id ||
      !selectedTracks.has(source.trackId) ||
      (source.kind !== "audio" && source.kind !== "video") ||
      source.audioSeparated ||
      !isSpeechAnalysisCurrent(source)
    ) continue;
    const track = trackById.get(source.trackId);
    if (!track || track.muted || (hasSolo && !track.solo)) continue;
    const analysis = source.speechAnalysis!;
    for (const segment of analysis.segments) {
      if (segment.confidence < settings.minConfidence) continue;
      const midpoint = (segment.start + segment.end) / 2;
      if (effectiveSourceGain(source, track, midpoint) <= 0.0001) continue;
      const start = source.start + segment.start;
      const end = source.start + segment.end;
      if (end <= targetStart || start >= targetEnd) continue;
      intervals.push({ start, end });
    }
  }

  intervals.sort((left, right) => left.start - right.start || left.end - right.end);
  const merged: Array<{ start: number; end: number }> = [];
  for (const interval of intervals) {
    const previous = merged[merged.length - 1];
    if (previous && interval.start <= previous.end + MERGE_SPEECH_GAP_SECONDS) {
      previous.end = Math.max(previous.end, interval.end);
    } else {
      merged.push({ ...interval });
    }
  }
  return merged;
}

function effectiveSourceGain(
  clip: DuckingClipLike,
  track: DuckingTrackLike,
  localTime: number,
): number {
  const manual = evaluateGainFrames(clip.keyframes?.volume, localTime, finiteOr(clip.volume, 1));
  const rider = evaluateGainFrames(clip.keyframes?.riderGain, localTime, 1);
  return Math.max(0, manual) * Math.max(0, rider) * Math.max(0, finiteOr(track.gain, 1));
}

function evaluateGainFrames(
  frames: readonly DuckingGainKeyframeLike[] | undefined,
  time: number,
  fallback: number,
): number {
  if (!frames?.length) return fallback;
  const sorted = [...frames].sort((left, right) => left.time - right.time);
  if (time <= sorted[0].time) return sorted[0].value;
  const last = sorted[sorted.length - 1];
  if (time >= last.time) return last.value;
  const rightIndex = sorted.findIndex((frame) => frame.time > time);
  const left = sorted[rightIndex - 1];
  const right = sorted[rightIndex];
  if (left.easing === "hold" || right.time <= left.time) return left.value;
  const progress = clamp((time - left.time) / (right.time - left.time), 0, 1);
  const eased = left.easing === "ease-in"
    ? progress * progress
    : left.easing === "ease-out"
      ? 1 - (1 - progress) * (1 - progress)
      : left.easing === "ease-in-out"
        ? progress < 0.5
          ? 2 * progress * progress
          : 1 - Math.pow(-2 * progress + 2, 2) / 2
        : progress;
  return left.value + (right.value - left.value) * eased;
}

interface DuckingShape {
  attackStart: number;
  fullStart: number;
  fullEnd: number;
  releaseEnd: number;
}

function shapeGain(shape: DuckingShape, time: number, depth: number): number {
  if (time <= shape.attackStart || time >= shape.releaseEnd) return 1;
  if (time < shape.fullStart) {
    const progress = (time - shape.attackStart) / (shape.fullStart - shape.attackStart);
    return 1 + (depth - 1) * clamp(progress, 0, 1);
  }
  if (time <= shape.fullEnd) return depth;
  const progress = (time - shape.fullEnd) / (shape.releaseEnd - shape.fullEnd);
  return depth + (1 - depth) * clamp(progress, 0, 1);
}

function releaseAttackCrossing(
  left: DuckingShape,
  right: DuckingShape,
  depth: number,
): number | null {
  const start = Math.max(left.fullEnd, right.attackStart);
  const end = Math.min(left.releaseEnd, right.fullStart);
  if (end <= start) return null;
  let low = start;
  let high = end;
  let lowDelta = shapeGain(left, low, depth) - shapeGain(right, low, depth);
  const highDelta = shapeGain(left, high, depth) - shapeGain(right, high, depth);
  if (Math.abs(lowDelta) < 1e-9) return low;
  if (Math.abs(highDelta) < 1e-9) return high;
  if (Math.sign(lowDelta) === Math.sign(highDelta)) return null;
  for (let iteration = 0; iteration < 32; iteration += 1) {
    const middle = (low + high) / 2;
    const delta = shapeGain(left, middle, depth) - shapeGain(right, middle, depth);
    if (Math.abs(delta) < 1e-9) return middle;
    if (Math.sign(delta) === Math.sign(lowDelta)) {
      low = middle;
      lowDelta = delta;
    } else {
      high = middle;
    }
  }
  return (low + high) / 2;
}

function normalizeSpeechSegments(
  input: unknown,
  duration: number,
): SpeechAnalysisSegment[] {
  if (!Array.isArray(input)) return [];
  const values = input
    .filter((value): value is Partial<SpeechAnalysisSegment> => Boolean(value) && typeof value === "object")
    .map((segment) => ({
      start: clamp(finiteOr(segment.start, 0), 0, duration),
      end: clamp(finiteOr(segment.end, 0), 0, duration),
      confidence: clamp(finiteOr(segment.confidence, 0), 0, 1),
    }))
    .filter((segment) => segment.end - segment.start >= MIN_SEGMENT_SECONDS)
    .sort((left, right) => left.start - right.start || left.end - right.end);
  const merged: SpeechAnalysisSegment[] = [];
  for (const segment of values) {
    const previous = merged[merged.length - 1];
    if (previous && segment.start <= previous.end) {
      const previousDuration = previous.end - previous.start;
      const segmentDuration = segment.end - segment.start;
      previous.confidence =
        (previous.confidence * previousDuration + segment.confidence * segmentDuration) /
        Math.max(MIN_SEGMENT_SECONDS, previousDuration + segmentDuration);
      previous.end = Math.max(previous.end, segment.end);
    } else {
      merged.push({ ...segment });
    }
  }
  return merged;
}

export function createSpeechAnalysisMetadata(
  segments: SpeechAnalysisSegment[],
  signature: string,
  duration: number,
): SpeechAnalysisMetadata {
  const safeDuration = Math.max(0, finiteOr(duration, 0));
  const normalized = normalizeSpeechSegments(segments, safeDuration);
  const speechSeconds = normalized.reduce((sum, segment) => sum + segment.end - segment.start, 0);
  const confidenceWeight = normalized.reduce(
    (sum, segment) => sum + segment.confidence * (segment.end - segment.start),
    0,
  );
  return {
    version: SPEECH_ANALYSIS_VERSION,
    model: AUTO_DUCKING_MODEL,
    signature: typeof signature === "string" ? signature : "",
    segments: normalized,
    speechCoverage: safeDuration > 0 ? clamp(speechSeconds / safeDuration, 0, 1) : 0,
    averageConfidence: speechSeconds > 0
      ? clamp(confidenceWeight / speechSeconds, 0, 1)
      : 0,
  };
}

function simplifyDuckingPoints(
  points: DuckingKeyframe[],
  toleranceDb: number,
): DuckingKeyframe[] {
  if (points.length <= 2) return points;
  const keep = new Array(points.length).fill(false);
  keep[0] = true;
  keep[points.length - 1] = true;
  const ranges: Array<[number, number]> = [[0, points.length - 1]];
  while (ranges.length > 0) {
    const [start, end] = ranges.pop()!;
    if (end <= start + 1) continue;
    const left = points[start];
    const right = points[end];
    const duration = right.time - left.time;
    const leftDb = gainToDb(left.value);
    const rightDb = gainToDb(right.value);
    let maximumError = 0;
    let maximumIndex = -1;
    for (let index = start + 1; index < end; index += 1) {
      const progress = duration <= 0 ? 0 : (points[index].time - left.time) / duration;
      const expected = leftDb + (rightDb - leftDb) * progress;
      const error = Math.abs(gainToDb(points[index].value) - expected);
      if (error > maximumError) {
        maximumError = error;
        maximumIndex = index;
      }
    }
    if (maximumIndex !== -1 && maximumError > toleranceDb) {
      keep[maximumIndex] = true;
      ranges.push([start, maximumIndex], [maximumIndex, end]);
    }
  }
  return points.filter((_, index) => keep[index]);
}

function capDuckingPoints(points: DuckingKeyframe[], maximum: number): DuckingKeyframe[] {
  if (points.length <= maximum) return points;
  let tolerance = 0.1;
  let simplified = points;
  while (simplified.length > maximum && tolerance <= 6.4) {
    simplified = simplifyDuckingPoints(points, tolerance);
    tolerance *= 2;
  }
  if (simplified.length <= maximum) return simplified;
  const result: DuckingKeyframe[] = [];
  const last = simplified.length - 1;
  for (let index = 0; index < maximum; index += 1) {
    result.push(simplified[Math.round(index * last / (maximum - 1))]);
  }
  return result;
}

function dedupeTimes(times: number[]): number[] {
  return times
    .filter(Number.isFinite)
    .map((time) => Math.max(0, time))
    .sort((left, right) => left - right)
    .filter((time, index, values) => index === 0 || Math.abs(time - values[index - 1]) >= 1e-7);
}

function gainToDb(value: number): number {
  return 20 * Math.log10(Math.max(value, 1e-6));
}

function finiteOr(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
