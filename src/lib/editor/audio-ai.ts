export type SpeechSuggestionKind =
  | "silence"
  | "filler-word"
  | "possible-filler-sound"
  | "contextual-word";

export interface SpeechActivitySegment {
  startMs: number;
  endMs: number;
  confidence: number;
}

export interface TranscriptWord {
  startMs: number;
  endMs: number;
  text: string;
  confidence: number;
}

export interface SpeechSuggestion {
  id: string;
  startMs: number;
  endMs: number;
  text: string | null;
  kind: SpeechSuggestionKind;
  confidence: number;
  reason: string;
  /** Safe defaults only. Context-dependent words are never pre-selected. */
  defaultSelected: boolean;
}

export interface SpeechSuggestionOptions {
  minSilenceMs: number;
  keepBeforeSpeechMs: number;
  keepAfterSpeechMs: number;
  fillerPaddingMs: number;
  includePossibleFillerSounds: boolean;
}

export const BALANCED_SPEECH_SUGGESTIONS: SpeechSuggestionOptions = {
  minSilenceMs: 700,
  keepBeforeSpeechMs: 120,
  keepAfterSpeechMs: 160,
  fillerPaddingMs: 60,
  includePossibleFillerSounds: true,
};

const CONTEXTUAL_TURKISH_WORDS = new Set([
  "falan",
  "filan",
  "hani",
  "işte",
  "şey",
  "yani",
]);

function finite(value: number | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

export function normalizeTurkishToken(text: string): string {
  return text
    .normalize("NFKC")
    .toLocaleLowerCase("tr-TR")
    .replace(/[^\p{L}]/gu, "");
}

export function classifyTurkishFiller(
  text: string,
): "filler-word" | "contextual-word" | null {
  const token = normalizeTurkishToken(text);
  if (!token) return null;
  if (CONTEXTUAL_TURKISH_WORDS.has(token)) return "contextual-word";
  if (
    /^(?:e{2,}|ı{2,}|i{2,}|ıh+m*|ih+m*|h+m+|öh+ö*m*)$/u.test(token)
  ) {
    return "filler-word";
  }
  return null;
}

function normalizeSpeechSegments(
  segments: readonly SpeechActivitySegment[],
  durationMs: number,
): SpeechActivitySegment[] {
  return segments
    .map((segment) => ({
      startMs: clamp(Math.round(finite(segment.startMs)), 0, durationMs),
      endMs: clamp(Math.round(finite(segment.endMs)), 0, durationMs),
      confidence: clamp(finite(segment.confidence), 0, 1),
    }))
    .filter((segment) => segment.endMs > segment.startMs)
    .sort((left, right) => left.startMs - right.startMs)
    .reduce<SpeechActivitySegment[]>((merged, segment) => {
      const previous = merged.at(-1);
      if (!previous || segment.startMs > previous.endMs) {
        merged.push({ ...segment });
      } else {
        previous.endMs = Math.max(previous.endMs, segment.endMs);
        previous.confidence = Math.max(previous.confidence, segment.confidence);
      }
      return merged;
    }, []);
}

function normalizeWords(
  words: readonly TranscriptWord[],
  durationMs: number,
): TranscriptWord[] {
  return words
    .map((word) => ({
      startMs: clamp(Math.round(finite(word.startMs)), 0, durationMs),
      endMs: clamp(Math.round(finite(word.endMs)), 0, durationMs),
      text: String(word.text ?? "").trim(),
      confidence: clamp(finite(word.confidence), 0, 1),
    }))
    .filter((word) => word.text && word.endMs > word.startMs)
    .sort((left, right) => left.startMs - right.startMs);
}

function suggestionId(
  kind: SpeechSuggestionKind,
  startMs: number,
  endMs: number,
  text: string | null,
): string {
  const token = text ? normalizeTurkishToken(text).slice(0, 20) : "gap";
  return `${kind}:${startMs}:${endMs}:${token}`;
}

function addSilenceSuggestion(
  output: SpeechSuggestion[],
  startMs: number,
  endMs: number,
  confidence: number,
  minimumMs: number,
) {
  if (endMs - startMs < minimumMs) return;
  output.push({
    id: suggestionId("silence", startMs, endMs, null),
    startMs,
    endMs,
    text: null,
    kind: "silence",
    confidence: clamp(confidence, 0, 1),
    reason: "Silero VAD bu aralıkta konuşma bulmadı; kelime kenarları korundu.",
    defaultSelected: true,
  });
}

/**
 * Builds review-only edit suggestions. It never mutates media and intentionally
 * keeps semantic Turkish discourse markers out of the default selection.
 */
export function buildSpeechSuggestions(
  durationMsInput: number,
  speechInput: readonly SpeechActivitySegment[],
  wordInput: readonly TranscriptWord[],
  options: SpeechSuggestionOptions = BALANCED_SPEECH_SUGGESTIONS,
): SpeechSuggestion[] {
  const durationMs = Math.max(1, Math.round(finite(durationMsInput, 1)));
  const speech = normalizeSpeechSegments(speechInput, durationMs);
  const words = normalizeWords(wordInput, durationMs);
  const output: SpeechSuggestion[] = [];
  const minimumCutMs = Math.max(80, Math.round(options.minSilenceMs));

  if (speech.length > 0) {
    addSilenceSuggestion(
      output,
      0,
      Math.max(0, speech[0].startMs - options.keepBeforeSpeechMs),
      speech[0].confidence,
      minimumCutMs,
    );
    for (let index = 0; index < speech.length - 1; index += 1) {
      const left = speech[index];
      const right = speech[index + 1];
      addSilenceSuggestion(
        output,
        Math.min(durationMs, left.endMs + options.keepAfterSpeechMs),
        Math.max(0, right.startMs - options.keepBeforeSpeechMs),
        Math.min(left.confidence, right.confidence),
        minimumCutMs,
      );
    }
    const last = speech.at(-1)!;
    addSilenceSuggestion(
      output,
      Math.min(durationMs, last.endMs + options.keepAfterSpeechMs),
      durationMs,
      last.confidence,
      minimumCutMs,
    );
  }

  for (const word of words) {
    const kind = classifyTurkishFiller(word.text);
    if (!kind) continue;
    const startMs = Math.max(0, word.startMs - options.fillerPaddingMs);
    const endMs = Math.min(durationMs, word.endMs + options.fillerPaddingMs);
    const contextual = kind === "contextual-word";
    output.push({
      id: suggestionId(kind, startMs, endMs, word.text),
      startMs,
      endMs,
      text: word.text,
      kind,
      confidence: clamp(word.confidence * (contextual ? 0.65 : 1), 0, 1),
      reason: contextual
        ? "Bu sözcük bağlama göre anlamlı olabilir; yalnız dinleyip onaylarsanız kesin."
        : "Whisper zaman kodu ve Türkçe dolgu sözlüğü eşleşti.",
      defaultSelected: !contextual && word.confidence >= 0.72,
    });
  }

  if (options.includePossibleFillerSounds) {
    for (const segment of speech) {
      const length = segment.endMs - segment.startMs;
      if (length < 180 || length > 1_200) continue;
      const hasWord = words.some(
        (word) => word.endMs > segment.startMs && word.startMs < segment.endMs,
      );
      if (hasWord) continue;
      output.push({
        id: suggestionId(
          "possible-filler-sound",
          segment.startMs,
          segment.endMs,
          null,
        ),
        startMs: segment.startMs,
        endMs: segment.endMs,
        text: null,
        kind: "possible-filler-sound",
        confidence: clamp(segment.confidence * 0.65, 0, 1),
        reason: "Silero konuşma benzeri bir ses gördü fakat Whisper sözcük üretmedi.",
        defaultSelected: false,
      });
    }
  }

  return output.sort(
    (left, right) => left.startMs - right.startMs || left.endMs - right.endMs,
  );
}

export interface SourceBeatMarker {
  sourceMs: number;
  confidence: number;
  downbeat: boolean;
}

export const BEAT_ANALYSIS_VERSION = 1 as const;

export interface BeatAnalysisMetadata {
  version: typeof BEAT_ANALYSIS_VERSION;
  model: string;
  sourcePath: string;
  sourceFingerprint: string;
  analyzedStartMs: number;
  analyzedDurationMs: number;
  bpm: number | null;
  markers: SourceBeatMarker[];
}

export interface AnalyzeBeatsResult {
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

export function createBeatAnalysisMetadata(
  sourcePath: string,
  result: AnalyzeBeatsResult,
): BeatAnalysisMetadata {
  const downbeats = result.downbeats.map((sourceMs, index) => ({
    sourceMs,
    confidence: clamp(finite(result.downbeatConfidences[index], 0), 0, 1),
  }));
  const markers = result.beats.map((sourceMs, index) => {
    const matchingDownbeat = downbeats.find(
      (downbeat) => Math.abs(downbeat.sourceMs - sourceMs) <= 70,
    );
    return {
      sourceMs: Math.max(0, Math.round(finite(sourceMs))),
      confidence: clamp(
        Math.max(
          finite(result.confidences[index], 0),
          matchingDownbeat?.confidence ?? 0,
        ),
        0,
        1,
      ),
      downbeat: Boolean(matchingDownbeat),
    };
  });
  for (const downbeat of downbeats) {
    if (markers.some((marker) => Math.abs(marker.sourceMs - downbeat.sourceMs) <= 70)) {
      continue;
    }
    markers.push({ ...downbeat, downbeat: true });
  }
  markers.sort((left, right) => left.sourceMs - right.sourceMs);
  return {
    version: BEAT_ANALYSIS_VERSION,
    model: String(result.model || "beat-this"),
    sourcePath,
    sourceFingerprint: String(result.sourceFingerprint || ""),
    analyzedStartMs: Math.max(0, Math.round(finite(result.analyzedStartMs))),
    analyzedDurationMs: Math.max(1, Math.round(finite(result.analyzedDurationMs, 1))),
    bpm: result.bpm === null ? null : Math.max(0, finite(result.bpm)),
    markers,
  };
}

export function normalizeBeatAnalysis(
  input: Partial<BeatAnalysisMetadata> | null | undefined,
  sourcePath: string,
): BeatAnalysisMetadata | null {
  if (
    !input ||
    input.version !== BEAT_ANALYSIS_VERSION ||
    input.sourcePath !== sourcePath ||
    typeof input.model !== "string" ||
    typeof input.sourceFingerprint !== "string" ||
    !Array.isArray(input.markers)
  ) return null;
  const analyzedStartMs = Math.max(0, Math.round(finite(input.analyzedStartMs)));
  const analyzedDurationMs = Math.max(1, Math.round(finite(input.analyzedDurationMs, 1)));
  const markers = input.markers
    .map((marker) => ({
      sourceMs: Math.max(0, Math.round(finite(marker.sourceMs))),
      confidence: clamp(finite(marker.confidence), 0, 1),
      downbeat: Boolean(marker.downbeat),
    }))
    .filter(
      (marker) =>
        marker.sourceMs >= analyzedStartMs &&
        marker.sourceMs <= analyzedStartMs + analyzedDurationMs,
    )
    .sort((left, right) => left.sourceMs - right.sourceMs);
  return {
    version: BEAT_ANALYSIS_VERSION,
    model: input.model,
    sourcePath,
    sourceFingerprint: input.sourceFingerprint,
    analyzedStartMs,
    analyzedDurationMs,
    bpm:
      input.bpm === null || input.bpm === undefined
        ? null
        : Math.max(0, finite(input.bpm)),
    markers,
  };
}

export function isBeatAnalysisRangeCurrent(
  clip: BeatProjectionClip & { file: string },
  analysis: BeatAnalysisMetadata | null | undefined,
): boolean {
  if (!analysis || analysis.sourcePath !== clip.file) return false;
  const currentStart = clip.trimIn * 1_000;
  const currentEnd = currentStart + clip.duration * clip.speed * 1_000;
  const analyzedEnd = analysis.analyzedStartMs + analysis.analyzedDurationMs;
  return (
    currentStart >= analysis.analyzedStartMs - 1 &&
    currentEnd <= analyzedEnd + 1
  );
}

export interface TimelineBeatMarker extends SourceBeatMarker {
  timelineTime: number;
}

export interface BeatProjectionClip {
  start: number;
  duration: number;
  trimIn: number;
  speed: number;
}

/** Keeps beat metadata in source time so move/trim/speed edits cannot stale it. */
export function projectSourceBeats(
  clip: BeatProjectionClip,
  markers: readonly SourceBeatMarker[],
): TimelineBeatMarker[] {
  const speed = finite(clip.speed, 1) > 0 ? clip.speed : 1;
  const sourceStartMs = Math.max(0, finite(clip.trimIn) * 1_000);
  const sourceEndMs = sourceStartMs + Math.max(0, finite(clip.duration)) * speed * 1_000;
  return markers
    .filter(
      (marker) =>
        finite(marker.sourceMs, -1) >= sourceStartMs &&
        finite(marker.sourceMs, -1) <= sourceEndMs,
    )
    .map((marker) => ({
      sourceMs: Math.round(marker.sourceMs),
      confidence: clamp(finite(marker.confidence), 0, 1),
      downbeat: Boolean(marker.downbeat),
      timelineTime:
        finite(clip.start) + (marker.sourceMs - sourceStartMs) / (speed * 1_000),
    }))
    .sort((left, right) => left.timelineTime - right.timelineTime);
}

export type BeatCutMode = "downbeat" | "every-2" | "every-beat" | "smart";

export interface BeatCutPlanOptions {
  mode: BeatCutMode;
  rangeStart: number;
  rangeEnd: number;
  minShotSeconds?: number;
  maxShotSeconds?: number;
}

function dedupeTimelineMarkers(
  input: readonly TimelineBeatMarker[],
): TimelineBeatMarker[] {
  const result: TimelineBeatMarker[] = [];
  for (const marker of [...input].sort((a, b) => a.timelineTime - b.timelineTime)) {
    const previous = result.at(-1);
    if (previous && Math.abs(previous.timelineTime - marker.timelineTime) < 0.02) {
      if (marker.confidence > previous.confidence || marker.downbeat) {
        result[result.length - 1] = { ...marker };
      }
    } else {
      result.push({ ...marker });
    }
  }
  return result;
}

function smartBeatCuts(
  markers: readonly TimelineBeatMarker[],
  start: number,
  end: number,
  minimum: number,
  maximum: number,
): number[] {
  const candidates = markers.filter(
    (marker) => marker.timelineTime > start && marker.timelineTime < end,
  );
  const nodes = [
    { time: start, score: 0 },
    ...candidates.map((marker) => ({
      time: marker.timelineTime,
      score: marker.confidence + (marker.downbeat ? 0.45 : 0),
    })),
    { time: end, score: 0 },
  ];
  const target = clamp((minimum + maximum) / 2, minimum, maximum);
  const best = Array.from({ length: nodes.length }, () => Number.NEGATIVE_INFINITY);
  const previous = Array.from({ length: nodes.length }, () => -1);
  best[0] = 0;

  for (let right = 1; right < nodes.length; right += 1) {
    for (let left = 0; left < right; left += 1) {
      const distance = nodes[right].time - nodes[left].time;
      const isFinal = right === nodes.length - 1;
      if (distance < minimum || (!isFinal && distance > maximum)) continue;
      if (isFinal && distance > maximum * 1.35) continue;
      const rhythmPenalty = Math.abs(distance - target) / Math.max(target, 0.001);
      const value = best[left] + nodes[right].score - rhythmPenalty * 0.28;
      if (value > best[right]) {
        best[right] = value;
        previous[right] = left;
      }
    }
  }

  if (previous.at(-1)! < 0) return [];
  const path: number[] = [];
  let cursor = nodes.length - 1;
  while (cursor > 0) {
    cursor = previous[cursor];
    if (cursor > 0) path.push(nodes[cursor].time);
  }
  return path.reverse();
}

/** Produces cut suggestions only; applying them remains an explicit undoable action. */
export function planBeatCuts(
  markerInput: readonly TimelineBeatMarker[],
  options: BeatCutPlanOptions,
): number[] {
  const start = finite(options.rangeStart);
  const end = Math.max(start, finite(options.rangeEnd, start));
  const minimum = clamp(finite(options.minShotSeconds, 0.35), 0.1, 30);
  const maximum = Math.max(minimum, finite(options.maxShotSeconds, 4));
  const allMarkers = dedupeTimelineMarkers(markerInput);
  const cadenceMarkers =
    options.mode === "downbeat"
      ? allMarkers.filter((marker) => marker.downbeat)
      : options.mode === "every-2"
        ? allMarkers.filter((_marker, index) => index % 2 === 0)
        : allMarkers;
  const markers = cadenceMarkers.filter(
    (marker) =>
      marker.timelineTime >= start + minimum &&
      marker.timelineTime <= end - minimum,
  );
  if (options.mode === "smart") {
    return smartBeatCuts(markers, start, end, minimum, maximum);
  }

  const cuts: number[] = [];
  let previous = start;
  for (const marker of markers) {
    if (marker.timelineTime - previous < minimum) continue;
    if (end - marker.timelineTime < minimum) continue;
    cuts.push(marker.timelineTime);
    previous = marker.timelineTime;
  }
  return cuts;
}
