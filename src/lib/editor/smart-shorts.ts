export interface SmartShortsWord {
  startMs: number;
  endMs: number;
  text: string;
  confidence: number;
}

export interface SmartShortsSpeechSegment {
  startMs: number;
  endMs: number;
  confidence: number;
}

export interface SmartShortsBeat {
  atMs: number;
  confidence: number;
  downbeat?: boolean;
}

export type SmartShortsAudioEventKind =
  | "laughter"
  | "giggle"
  | "cheering"
  | "applause"
  | "energy";

export interface SmartShortsAudioEvent {
  atMs: number;
  score: number;
  kind: SmartShortsAudioEventKind;
}

export interface SmartShortsLoudnessPoint {
  atMs: number;
  loudnessDb: number;
}

export interface SmartShortsNeuralLaughterWindow {
  startMs: number;
  endMs: number;
  score: number;
  kind: "laughter" | "giggle";
}

export interface SmartShortsRetentionPoint {
  atMs: number;
  score: number;
}

export interface SmartShortsInput {
  durationMs: number;
  words: readonly SmartShortsWord[];
  speechSegments: readonly SmartShortsSpeechSegment[];
  beats?: readonly SmartShortsBeat[];
  audioEvents?: readonly SmartShortsAudioEvent[];
  retention?: readonly SmartShortsRetentionPoint[];
}

export interface SmartShortsOptions {
  targetDurationMs: number;
  minDurationMs: number;
  maxDurationMs: number;
  maxCandidates: number;
  boundaryGapMs: number;
}

export interface SmartShortsCandidateSignals {
  speech: number;
  wordDensity: number;
  transcriptConfidence: number;
  hook: number;
  rhythm: number;
  audioEvent: number;
  retention: number;
}

export interface SmartShortsCandidate {
  id: string;
  startMs: number;
  endMs: number;
  score: number;
  transcript: string;
  reasons: string[];
  signals: SmartShortsCandidateSignals;
}

export interface SmartShortsRange {
  startMs: number;
  endMs: number;
}

export interface SmartShortsExternalRanking {
  candidateId: string;
  score: number;
  reason?: string;
  focusTarget?: string;
}

export const DEFAULT_SMART_SHORTS_OPTIONS: SmartShortsOptions = {
  targetDurationMs: 30_000,
  minDurationMs: 15_000,
  maxDurationMs: 45_000,
  maxCandidates: 6,
  boundaryGapMs: 700,
};

const HOOK_WORDS = new Set([
  "ama",
  "bak",
  "biliyor",
  "gerçekten",
  "inanılmaz",
  "nasıl",
  "neden",
  "şimdi",
  "sonra",
  "wait",
  "look",
  "how",
  "why",
]);

function finite(value: number, fallback = 0): number {
  return Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, minimum = 0, maximum = 1): number {
  return Math.min(maximum, Math.max(minimum, value));
}

/**
 * Converts the already-local browser waveform into sparse, clip-local energy
 * events. These are amplitude bursts, not laughter classification.
 */
export function extractWaveformEnergyEvents(
  waveform: readonly number[],
  durationMsInput: number,
  maxEvents = 12,
): SmartShortsAudioEvent[] {
  const durationMs = Math.max(1, Math.round(finite(durationMsInput, 1)));
  const values = waveform.map((value) => clamp(finite(value))).filter(Number.isFinite);
  if (values.length < 3) return [];
  const sorted = [...values].sort((left, right) => left - right);
  const quantile = (fraction: number) =>
    sorted[Math.min(sorted.length - 1, Math.max(0, Math.round((sorted.length - 1) * fraction)))] ?? 0;
  const baseline = quantile(0.5);
  const high = quantile(0.9);
  const dynamicRange = high - baseline;
  if (high < 0.08 || dynamicRange < 0.045) return [];
  const threshold = baseline + dynamicRange * 0.62;
  const candidates = values
    .map((value, index) => ({
      index,
      value,
      score: clamp((value - baseline) / Math.max(0.08, high - baseline)),
    }))
    .filter(({ index, value, score }) =>
      value >= threshold &&
      score >= 0.55 &&
      value >= (values[index - 1] ?? -1) &&
      value >= (values[index + 1] ?? -1))
    .sort((left, right) => right.score - left.score);
  const minimumGapMs = Math.max(750, (durationMs / values.length) * 3);
  const selected: SmartShortsAudioEvent[] = [];
  for (const candidate of candidates) {
    const atMs = Math.round(((candidate.index + 0.5) / values.length) * durationMs);
    if (selected.some((event) => Math.abs(event.atMs - atMs) < minimumGapMs)) continue;
    selected.push({ atMs, score: candidate.score, kind: "energy" });
    if (
      selected.length >= Math.max(1, Math.min(40, Math.round(finite(maxEvents, 12))))
    ) break;
  }
  return selected.sort((left, right) => left.atMs - right.atMs);
}

/** Extracts sparse energy rises from clip-local EBU R128 momentary loudness. */
export function extractLoudnessEnergyEvents(
  input: readonly SmartShortsLoudnessPoint[],
  maxEvents = 16,
): SmartShortsAudioEvent[] {
  const points = input
    .map((point) => ({
      atMs: Math.max(0, Math.round(finite(point.atMs))),
      loudnessDb: finite(point.loudnessDb, -120),
    }))
    .filter((point) => point.loudnessDb > -100)
    .sort((left, right) => left.atMs - right.atMs);
  if (points.length < 3) return [];
  const sortedLevels = points
    .map((point) => point.loudnessDb)
    .sort((left, right) => left - right);
  const quantile = (fraction: number) =>
    sortedLevels[
      Math.min(
        sortedLevels.length - 1,
        Math.max(0, Math.round((sortedLevels.length - 1) * fraction)),
      )
    ] ?? -120;
  const baseline = quantile(0.5);
  const high = quantile(0.9);
  const dynamicRange = high - baseline;
  if (dynamicRange < 1.5) return [];
  const threshold = baseline + Math.max(2, dynamicRange * 0.58);
  const candidates = points
    .map((point, index) => ({
      ...point,
      score: clamp((point.loudnessDb - baseline) / Math.max(5, dynamicRange)),
      index,
    }))
    .filter(({ index, loudnessDb, score }) =>
      loudnessDb >= threshold &&
      score >= 0.55 &&
      loudnessDb >= (points[index - 1]?.loudnessDb ?? Number.NEGATIVE_INFINITY) &&
      loudnessDb >= (points[index + 1]?.loudnessDb ?? Number.NEGATIVE_INFINITY))
    .sort((left, right) => right.score - left.score);
  const selected: SmartShortsAudioEvent[] = [];
  const eventLimit = Math.max(1, Math.min(40, Math.round(finite(maxEvents, 16))));
  for (const candidate of candidates) {
    if (selected.some((event) => Math.abs(event.atMs - candidate.atMs) < 750)) continue;
    selected.push({ atMs: candidate.atMs, score: candidate.score, kind: "energy" });
    if (selected.length >= eventLimit) break;
  }
  return selected.sort((left, right) => left.atMs - right.atMs);
}

const LAUGHTER_TRANSCRIPT_HINTS = new Set([
  "laughter",
  "laughing",
  "laughs",
  "kahkaha",
  "gülme",
  "gülüşme",
  "gülüyor",
  "hahaha",
  "ahaha",
]);

/**
 * Converts neural YAMNet windows into planner events. Loudness and Whisper can
 * strengthen a neural detection, but neither is allowed to invent laughter.
 */
export function fuseNeuralLaughterEvents(
  neural: readonly SmartShortsNeuralLaughterWindow[],
  energy: readonly SmartShortsAudioEvent[] = [],
  words: readonly SmartShortsWord[] = [],
): SmartShortsAudioEvent[] {
  return neural
    .map((event) => {
      const startMs = Math.max(0, Math.round(finite(event.startMs)));
      const endMs = Math.max(startMs, Math.round(finite(event.endMs)));
      const energySupport = energy.reduce((best, candidate) => {
        if (
          candidate.kind !== "energy" ||
          candidate.atMs < startMs - 750 ||
          candidate.atMs > endMs + 750
        ) return best;
        return Math.max(best, clamp(finite(candidate.score)));
      }, 0);
      const transcriptSupport = words.some(
        (word) =>
          word.endMs >= startMs - 500 &&
          word.startMs <= endMs + 500 &&
          LAUGHTER_TRANSCRIPT_HINTS.has(normalizeToken(word.text)),
      );
      return {
        atMs: Math.round((startMs + endMs) / 2),
        score:
          Math.round(
            clamp(
              finite(event.score) + energySupport * 0.08 + (transcriptSupport ? 0.06 : 0),
            ) * 1_000_000,
          ) / 1_000_000,
        kind: event.kind,
      };
    })
    .filter((event) => event.score > 0)
    .sort((left, right) => left.atMs - right.atMs);
}

function normalizeToken(value: string): string {
  return value
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]/gu, "");
}

function overlapMs(start: number, end: number, otherStart: number, otherEnd: number): number {
  return Math.max(0, Math.min(end, otherEnd) - Math.max(start, otherStart));
}

function normalizeInput(input: SmartShortsInput) {
  const durationMs = Math.max(1, Math.round(finite(input.durationMs, 1)));
  const words = input.words
    .map((word) => ({
      startMs: clamp(Math.round(finite(word.startMs)), 0, durationMs),
      endMs: clamp(Math.round(finite(word.endMs)), 0, durationMs),
      text: String(word.text ?? "").trim(),
      confidence: clamp(finite(word.confidence)),
    }))
    .filter((word) => word.text && word.endMs > word.startMs)
    .sort((left, right) => left.startMs - right.startMs);
  const speechSegments = input.speechSegments
    .map((segment) => ({
      startMs: clamp(Math.round(finite(segment.startMs)), 0, durationMs),
      endMs: clamp(Math.round(finite(segment.endMs)), 0, durationMs),
      confidence: clamp(finite(segment.confidence)),
    }))
    .filter((segment) => segment.endMs > segment.startMs)
    .sort((left, right) => left.startMs - right.startMs);
  const beats = (input.beats ?? [])
    .map((beat) => ({
      atMs: clamp(Math.round(finite(beat.atMs)), 0, durationMs),
      confidence: clamp(finite(beat.confidence)),
      downbeat: Boolean(beat.downbeat),
    }))
    .sort((left, right) => left.atMs - right.atMs);
  const audioEvents = (input.audioEvents ?? [])
    .map((event) => ({
      atMs: clamp(Math.round(finite(event.atMs)), 0, durationMs),
      score: clamp(finite(event.score)),
      kind: event.kind,
    }))
    .sort((left, right) => left.atMs - right.atMs);
  const retention = (input.retention ?? [])
    .map((point) => ({
      atMs: clamp(Math.round(finite(point.atMs)), 0, durationMs),
      score: clamp(finite(point.score)),
    }))
    .sort((left, right) => left.atMs - right.atMs);
  return { durationMs, words, speechSegments, beats, audioEvents, retention };
}

function normalizeOptions(
  input: Partial<SmartShortsOptions>,
  durationMs: number,
): SmartShortsOptions {
  const minDurationMs = clamp(
    Math.round(finite(input.minDurationMs ?? DEFAULT_SMART_SHORTS_OPTIONS.minDurationMs)),
    1_000,
    durationMs,
  );
  const maxDurationMs = clamp(
    Math.round(finite(input.maxDurationMs ?? DEFAULT_SMART_SHORTS_OPTIONS.maxDurationMs)),
    minDurationMs,
    durationMs,
  );
  return {
    minDurationMs,
    maxDurationMs,
    targetDurationMs: clamp(
      Math.round(finite(input.targetDurationMs ?? DEFAULT_SMART_SHORTS_OPTIONS.targetDurationMs)),
      minDurationMs,
      maxDurationMs,
    ),
    maxCandidates: Math.round(
      clamp(finite(input.maxCandidates ?? DEFAULT_SMART_SHORTS_OPTIONS.maxCandidates), 1, 20),
    ),
    boundaryGapMs: Math.round(
      clamp(finite(input.boundaryGapMs ?? DEFAULT_SMART_SHORTS_OPTIONS.boundaryGapMs), 250, 3_000),
    ),
  };
}

function intervalCoverage(
  startMs: number,
  endMs: number,
  intervals: readonly { startMs: number; endMs: number; confidence?: number }[],
): number {
  const clipped = intervals
    .map((interval) => ({
      startMs: Math.max(startMs, interval.startMs),
      endMs: Math.min(endMs, interval.endMs),
      confidence: clamp(finite(interval.confidence ?? 1)),
    }))
    .filter((interval) => interval.endMs > interval.startMs);
  const boundaries = [...new Set([
    startMs,
    endMs,
    ...clipped.flatMap((interval) => [interval.startMs, interval.endMs]),
  ])].sort((left, right) => left - right);
  let weightedCoverage = 0;
  for (let index = 1; index < boundaries.length; index += 1) {
    const segmentStart = boundaries[index - 1];
    const segmentEnd = boundaries[index];
    const midpoint = (segmentStart + segmentEnd) / 2;
    const confidence = clipped.reduce(
      (best, interval) =>
        midpoint >= interval.startMs && midpoint <= interval.endMs
          ? Math.max(best, interval.confidence)
          : best,
      0,
    );
    weightedCoverage += (segmentEnd - segmentStart) * confidence;
  }
  return clamp(weightedCoverage / Math.max(1, endMs - startMs));
}

function nearestBoundary(
  targetMs: number,
  values: readonly number[],
  toleranceMs: number,
): number {
  let best = targetMs;
  let bestDistance = toleranceMs + 1;
  for (const value of values) {
    const distance = Math.abs(value - targetMs);
    if (distance < bestDistance) {
      best = value;
      bestDistance = distance;
    }
  }
  return bestDistance <= toleranceMs ? best : targetMs;
}

function buildWindow(
  anchorMs: number,
  durationMs: number,
  options: SmartShortsOptions,
  speech: readonly SmartShortsSpeechSegment[],
): SmartShortsRange {
  const target = Math.min(options.targetDurationMs, durationMs);
  const maximumStart = Math.max(0, durationMs - target);
  let startMs = clamp(Math.round(anchorMs - target * 0.24), 0, maximumStart);
  let endMs = Math.min(durationMs, startMs + target);
  const starts = speech.map((segment) => segment.startMs);
  const ends = speech.map((segment) => segment.endMs);
  startMs = nearestBoundary(startMs, starts, 1_800);
  endMs = nearestBoundary(endMs, ends, 2_400);
  if (endMs - startMs < options.minDurationMs) {
    endMs = Math.min(durationMs, startMs + options.minDurationMs);
    startMs = Math.max(0, endMs - options.minDurationMs);
  }
  if (endMs - startMs > options.maxDurationMs) endMs = startMs + options.maxDurationMs;
  return { startMs: Math.round(startMs), endMs: Math.round(endMs) };
}

function hookScore(words: readonly SmartShortsWord[], startMs: number): number {
  const early = words.filter((word) => word.startMs < startMs + 4_500).slice(0, 18);
  if (early.length === 0) return 0;
  const lexicalHits = early.filter((word) => HOOK_WORDS.has(normalizeToken(word.text))).length;
  const questionOrExclamation = early.some((word) => /[?!…]$/.test(word.text));
  const compactOpening = early.length >= 5 && early.at(-1)!.endMs - early[0].startMs <= 4_000;
  return clamp(
    lexicalHits * 0.24 + (questionOrExclamation ? 0.34 : 0) + (compactOpening ? 0.28 : 0),
  );
}

function scoreWindow(
  range: SmartShortsRange,
  input: ReturnType<typeof normalizeInput>,
): SmartShortsCandidate | null {
  const durationMs = range.endMs - range.startMs;
  if (durationMs <= 0) return null;
  const words = input.words.filter(
    (word) => word.endMs > range.startMs && word.startMs < range.endMs,
  );
  if (words.length === 0) return null;
  const speech = intervalCoverage(range.startMs, range.endMs, input.speechSegments);
  const wordDensity = clamp(words.length / Math.max(1, durationMs / 1_000) / 2.6);
  const transcriptConfidence =
    words.reduce((total, word) => total + word.confidence, 0) / words.length;
  const hook = hookScore(words, range.startMs);
  const beats = input.beats.filter(
    (beat) => beat.atMs >= range.startMs && beat.atMs <= range.endMs,
  );
  const averageBeatConfidence = beats.length
    ? beats.reduce(
        (total, beat) => total + beat.confidence * (beat.downbeat ? 1.08 : 1),
        0,
      ) / beats.length
    : 0;
  const beatSupport = clamp(beats.length / Math.max(4, (durationMs / 1_000) * 0.8));
  const rhythm = clamp(averageBeatConfidence * (0.55 + beatSupport * 0.45));
  const audioEvents = input.audioEvents.filter(
    (event) => event.atMs >= range.startMs && event.atMs <= range.endMs,
  );
  const audioEvent = clamp(
    audioEvents.reduce((best, event) => {
      const kindWeight = event.kind === "laughter" || event.kind === "giggle"
        ? 1
        : event.kind === "cheering" || event.kind === "applause"
          ? 0.86
          : 0.62;
      return Math.max(best, event.score * kindWeight);
    }, 0),
  );
  const retentionPoints = input.retention.filter(
    (point) => point.atMs >= range.startMs && point.atMs <= range.endMs,
  );
  const retention = retentionPoints.length
    ? retentionPoints.reduce((total, point) => total + point.score, 0) / retentionPoints.length
    : 0;

  const weightedSignals: Array<[number, number]> = [
    [speech, 0.31],
    [wordDensity, 0.19],
    [transcriptConfidence, 0.14],
    [hook, 0.14],
    [rhythm, 0.08],
  ];
  if (input.audioEvents.length > 0) weightedSignals.push([audioEvent, 0.18]);
  if (input.retention.length > 0) weightedSignals.push([retention, 0.14]);
  const totalWeight = weightedSignals.reduce((total, [, weight]) => total + weight, 0);
  let score = weightedSignals.reduce((total, [value, weight]) => total + value * weight, 0) /
    Math.max(0.001, totalWeight);
  if (speech < 0.38) score *= 0.72;
  if (transcriptConfidence < 0.45) score *= 0.8;

  const reasons: string[] = [];
  const laughter = audioEvents.some(
    (event) => (event.kind === "laughter" || event.kind === "giggle") && event.score >= 0.55,
  );
  const energyBurst = audioEvents.some(
    (event) => event.kind === "energy" && event.score >= 0.62,
  );
  if (laughter) reasons.push("Kahkaha/gülme olayı");
  if (energyBurst) reasons.push("Yerel ses enerjisi yükselişi");
  if (retention >= 0.66) reasons.push("Yüksek izleyici tutma sinyali");
  if (hook >= 0.5) reasons.push("Güçlü açılış cümlesi");
  if (speech >= 0.72 && wordDensity >= 0.55) reasons.push("Yoğun ve kesintisiz anlatım");
  if (rhythm >= 0.62) reasons.push("Belirgin ritmik yapı");
  if (reasons.length === 0) reasons.push("Konuşma akışı ve güven skoru");

  return {
    id: `short:${range.startMs}:${range.endMs}`,
    startMs: range.startMs,
    endMs: range.endMs,
    score: Math.round(clamp(score) * 100),
    transcript: words.map((word) => word.text).join(" ").slice(0, 1_200),
    reasons,
    signals: {
      speech,
      wordDensity,
      transcriptConfidence,
      hook,
      rhythm,
      audioEvent,
      retention,
    },
  };
}

function candidateOverlap(left: SmartShortsCandidate, right: SmartShortsCandidate): number {
  const intersection = overlapMs(left.startMs, left.endMs, right.startMs, right.endMs);
  const shorter = Math.min(left.endMs - left.startMs, right.endMs - right.startMs);
  return shorter > 0 ? intersection / shorter : 0;
}

export function planSmartShorts(
  rawInput: SmartShortsInput,
  rawOptions: Partial<SmartShortsOptions> = {},
): SmartShortsCandidate[] {
  const input = normalizeInput(rawInput);
  const options = normalizeOptions(rawOptions, input.durationMs);
  if (input.words.length === 0) return [];

  const anchors = new Set<number>([input.words[0]?.startMs ?? 0]);
  for (let index = 1; index < input.words.length; index += 1) {
    const previous = input.words[index - 1];
    const word = input.words[index];
    if (word.startMs - previous.endMs >= options.boundaryGapMs || /[?!…]$/.test(previous.text)) {
      anchors.add(word.startMs);
    }
  }
  for (const event of input.audioEvents) anchors.add(event.atMs);
  for (const point of input.retention) {
    if (point.score >= 0.65) anchors.add(point.atMs);
  }
  const gridStep = Math.max(5_000, Math.round(options.targetDurationMs / 2));
  for (let atMs = 0; atMs < input.durationMs; atMs += gridStep) anchors.add(atMs);

  const uniqueWindows = new Map<string, SmartShortsRange>();
  for (const anchor of anchors) {
    const window = buildWindow(anchor, input.durationMs, options, input.speechSegments);
    uniqueWindows.set(`${window.startMs}:${window.endMs}`, window);
  }
  const scored = [...uniqueWindows.values()]
    .map((range) => scoreWindow(range, input))
    .filter((candidate): candidate is SmartShortsCandidate => Boolean(candidate))
    .sort((left, right) => right.score - left.score || left.startMs - right.startMs);

  const selected: SmartShortsCandidate[] = [];
  for (const candidate of scored) {
    if (selected.some((current) => candidateOverlap(current, candidate) >= 0.45)) continue;
    selected.push(candidate);
    if (selected.length >= options.maxCandidates) break;
  }
  return selected;
}

/** Keeps local scoring authoritative while allowing a transcript-only judge to
 * reorder supplied candidates. Unknown external IDs cannot create moments. */
export function mergeExternalSmartShortsRankings(
  candidates: readonly SmartShortsCandidate[],
  rankings: readonly SmartShortsExternalRanking[],
): SmartShortsCandidate[] {
  const byId = new Map(rankings.map((ranking) => [ranking.candidateId, ranking]));
  return candidates
    .map((candidate) => {
      const ranking = byId.get(candidate.id);
      if (!ranking || !Number.isFinite(ranking.score)) return candidate;
      const reasons = [...candidate.reasons];
      const reason = String(ranking.reason ?? "").trim();
      const focusTarget = String(ranking.focusTarget ?? "").trim();
      if (reason) reasons.unshift(`GLM notu: ${reason.slice(0, 320)}`);
      if (focusTarget) reasons.push(`Kadraj inceleme notu: ${focusTarget.slice(0, 120)}`);
      return {
        ...candidate,
        score: Math.round(candidate.score * 0.55 + clamp(ranking.score, 0, 100) * 0.45),
        reasons,
      };
    })
    .sort((left, right) => right.score - left.score || left.startMs - right.startMs);
}

export function discardRangesOutsideHighlights(
  durationMsInput: number,
  highlights: readonly SmartShortsRange[],
): SmartShortsRange[] {
  const durationMs = Math.max(1, Math.round(finite(durationMsInput, 1)));
  const keep = highlights
    .map((range) => ({
      startMs: clamp(Math.round(finite(range.startMs)), 0, durationMs),
      endMs: clamp(Math.round(finite(range.endMs)), 0, durationMs),
    }))
    .filter((range) => range.endMs > range.startMs)
    .sort((left, right) => left.startMs - right.startMs)
    .reduce<SmartShortsRange[]>((merged, range) => {
      const previous = merged.at(-1);
      if (!previous || range.startMs > previous.endMs) merged.push({ ...range });
      else previous.endMs = Math.max(previous.endMs, range.endMs);
      return merged;
    }, []);
  if (keep.length === 0) return [];
  const discard: SmartShortsRange[] = [];
  let cursor = 0;
  for (const range of keep) {
    if (range.startMs > cursor) discard.push({ startMs: cursor, endMs: range.startMs });
    cursor = Math.max(cursor, range.endMs);
  }
  if (cursor < durationMs) discard.push({ startMs: cursor, endMs: durationMs });
  return discard;
}
