import {
  normalizeNoiseReduction,
  type NoiseReductionSettings,
} from "./noise-reduction";
import {
  buildAutoDuckingEnvelope,
  evaluateDuckingEnvelope,
  normalizeAutoDucking,
  normalizeSpeechAnalysis,
  projectSpeechAnalysis,
  speechAnalysisSignature,
  type AutoDuckingSettings,
  type SpeechAnalysisMetadata,
} from "./audio-ducking";
import {
  normalizeBeatAnalysis,
  type BeatAnalysisMetadata,
} from "./audio-ai";

export const PROJECT_STATE_VERSION = 2;
export const MIN_CLIP_DURATION = 1 / 120;
export const TIMELINE_EPSILON = 1e-6;

export type TrackType = "video" | "audio" | "text";
export type ClipKind = "video" | "audio" | "image" | "text";
export type TrimEdge = "start" | "end";
export type TransitionType =
  | "none"
  | "crossfade"
  | "dip-to-black"
  | "wipe-left"
  | "wipe-right"
  | "slide-up"
  | "slide-down"
  | "slide-left"
  | "slide-right"
  | "zoom-in"
  | "zoom-out"
  | "spin"
  | "flip-3d"
  | "blur"
  | "whip-left"
  | "whip-right"
  // Asset-backed graphic overlays (frame sequences in static/transitions/).
  | "fx-light-leak"
  | "fx-film-burn"
  | "fx-glitch"
  | "fx-ink"
  | "fx-brush";

export const TRANSITION_TYPES: readonly TransitionType[] = [
  "none",
  "crossfade",
  "dip-to-black",
  "wipe-left",
  "wipe-right",
  "slide-up",
  "slide-down",
  "slide-left",
  "slide-right",
  "zoom-in",
  "zoom-out",
  "spin",
  "flip-3d",
  "blur",
  "whip-left",
  "whip-right",
  "fx-light-leak",
  "fx-film-burn",
  "fx-glitch",
  "fx-ink",
  "fx-brush",
];

const TRANSITION_TYPE_SET = new Set<TransitionType>(TRANSITION_TYPES);

/** Mask-based graphic transitions reveal via luma matte, so the compositor
 * must not also fade the layer with the generic transition opacity. */
export const MASK_TRANSITION_TYPES: ReadonlySet<TransitionType> = new Set([
  "fx-ink",
  "fx-brush",
]);

export type TextAnimationType =
  | "none"
  | "fade"
  | "slide-up"
  | "slide-down"
  | "slide-left"
  | "slide-right"
  | "zoom"
  | "bounce"
  | "typewriter"
  | "flip-3d"
  | "spin";

export interface TextAnimation {
  type: TextAnimationType;
  duration: number;
}

/**
 * Per-frame text animation snapshot computed by `buildFramePlan`. Offsets are
 * fractions of the composition size so renderers stay resolution-independent.
 */
export interface TextEffectState {
  alpha: number;
  dxFrac: number;
  dyFrac: number;
  scale: number;
  /** Horizontal squash used to fake a 3D Y-axis flip (0..1). */
  scaleX: number;
  rotation: number;
  /** Typewriter reveal: 0..1 fraction of visible characters (1 = all). */
  charProgress: number;
}
export type KeyframeEasing =
  | "linear"
  | "hold"
  | "ease-in"
  | "ease-out"
  | "ease-in-out";
export type AnimatableProperty =
  | "x"
  | "y"
  | "scale"
  | "rotation"
  | "opacity"
  | "volume"
  | "riderGain"
  | "pan"
  | "shake";

export interface ClipTransform {
  x: number;
  y: number;
  scale: number;
  rotation: number;
  opacity: number;
  shake?: number;
}

export interface TimelineTrack {
  id: string;
  name: string;
  type: TrackType;
  muted: boolean;
  solo: boolean;
  locked: boolean;
  gain: number;
  pan: number;
}

export interface ClipTransition {
  type: TransitionType;
  duration: number;
}

export interface TimelineKeyframe {
  time: number;
  value: number;
  easing: KeyframeEasing;
}

export type ClipKeyframes = Partial<
  Record<AnimatableProperty, TimelineKeyframe[]>
>;

export interface TextGradient {
  from: string;
  to: string;
}

export interface TextClipStyle {
  content: string;
  fontFamily: string;
  fontSize: number;
  fontWeight: number;
  color: string;
  backgroundColor: string;
  align: "left" | "center" | "right";
  fontPath?: string;
  /** Outline. Width 0 disables the stroke. */
  strokeColor: string;
  strokeWidth: number;
  /** Shadow/glow. "transparent" color disables it. */
  shadowColor: string;
  shadowBlur: number;
  shadowOffsetX: number;
  shadowOffsetY: number;
  /** Vertical two-stop fill gradient; null uses the flat `color`. */
  gradient: TextGradient | null;
  animationIn: TextAnimation;
  animationOut: TextAnimation;
}

export interface VoiceRiderMetadata {
  model: "silero-vad-v6";
  speechCoverage: number;
  averageSpeechProbability: number;
  strongestCutDb: number;
  strongestBoostDb: number;
}


export interface TimelineClip {
  id: string;
  trackId: string;
  file: string;
  kind: ClipKind;
  start: number;
  duration: number;
  /** First source second used by the clip. */
  trimIn: number;
  /** Kept for backward compatibility with the original player. */
  videoOffset: number;
  /** Total usable source duration, or null for still/text clips. */
  sourceDuration: number | null;
  speed: number;
  volume: number;
  pan: number;
  transform: ClipTransform;
  transition: {
    in: ClipTransition;
    out: ClipTransition;
  };
  keyframes: ClipKeyframes;
  /** Local neural voice-leveling envelope; manual volume automation stays separate. */
  voiceRider: VoiceRiderMetadata | null;
  /** Cached neural speech cleanup configuration; null keeps the source untouched. */
  noiseReduction: NoiseReductionSettings | null;
  /** Reusable clip-local Silero segments; independent from Voice Rider gain. */
  speechAnalysis: SpeechAnalysisMetadata | null;
  /** Non-destructive music-ducking settings; its gain curve is derived from speech tracks. */
  autoDucking: AutoDuckingSettings | null;
  /** Source-time AI beat grid; move/trim/speed edits only re-project these markers. */
  beatAnalysis: BeatAnalysisMetadata | null;
  text: TextClipStyle | null;
  waveform: number[];
  /**
   * True once a video clip's built-in audio has been extracted to a
   * separate Voice clip. Hides audio controls for this clip instead of
   * leaving a confusing 0%-volume slider around.
   */
  audioSeparated: boolean;
}

export interface TimelineProjectState {
  version: number;
  tracks: TimelineTrack[];
  clips: TimelineClip[];
  selectedClipId: string | null;
}

export interface SerializedTimelineClip extends TimelineClip {
  path: string;
  startMs: number;
  durationMs: number;
  trimInMs: number;
}

export interface TimelineProjectSnapshot {
  version: number;
  timebase: "milliseconds";
  tracks: TimelineTrack[];
  clips: SerializedTimelineClip[];
  selectedClipId: string | null;
  durationMs: number;
}

export function isTimelineProjectSnapshot(value: unknown): value is TimelineProjectSnapshot {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  if (!Array.isArray(candidate.tracks) || !Array.isArray(candidate.clips)) return false;
  if (candidate.timebase !== undefined && candidate.timebase !== "milliseconds") return false;
  if (
    candidate.selectedClipId !== undefined &&
    candidate.selectedClipId !== null &&
    typeof candidate.selectedClipId !== "string"
  ) return false;

  const trackIds = new Set<string>();
  for (const raw of candidate.tracks) {
    if (!raw || typeof raw !== "object") return false;
    const track = raw as Record<string, unknown>;
    if (
      typeof track.id !== "string" ||
      track.id.length === 0 ||
      typeof track.name !== "string" ||
      !["video", "audio", "text"].includes(String(track.type)) ||
      trackIds.has(track.id)
    ) return false;
    trackIds.add(track.id);
  }

  for (const raw of candidate.clips) {
    if (!raw || typeof raw !== "object") return false;
    const clip = raw as Record<string, unknown>;
    const hasStart =
      (typeof clip.start === "number" && Number.isFinite(clip.start)) ||
      (typeof clip.startMs === "number" && Number.isFinite(clip.startMs));
    const hasDuration =
      (typeof clip.duration === "number" && Number.isFinite(clip.duration)) ||
      (typeof clip.durationMs === "number" && Number.isFinite(clip.durationMs));
    if (
      typeof clip.id !== "string" ||
      clip.id.length === 0 ||
      typeof clip.trackId !== "string" ||
      !trackIds.has(clip.trackId) ||
      !hasStart ||
      !hasDuration ||
      (typeof clip.file !== "string" && typeof clip.path !== "string")
    ) return false;
  }
  return true;
}

export interface TimelineIssue {
  code:
    | "duplicate-id"
    | "missing-track"
    | "invalid-time"
    | "invalid-speed"
    | "source-overrun"
    | "overlap"
    | "transition-too-long"
    | "transition-overlap"
    | "invalid-transition";
  message: string;
  clipId?: string;
  trackId?: string;
}

export interface TrimOptions {
  ripple?: boolean;
}

export interface DeleteOptions {
  ripple?: boolean;
}

export interface DeleteTrackOptions {
  cascade?: boolean;
}

export interface MoveOptions {
  /**
   * `move` requires the whole destination range to be empty. `insert` opens
   * space at an edit boundary. Insert is a ripple relocation: the source
   * track closes the removed clip's slot, an adjacent target gap is consumed
   * from its left edge, and only the duration that does not fit in that gap
   * shifts the following target clips.
   */
  mode?: "move" | "insert";
  /**
   * Close the removed clip's slot on its source track for an ordinary move.
   * Insert mode always closes the source slot. The default remains `false`
   * so existing free-placement moves keep their non-ripple behavior.
   */
  rippleSource?: boolean;
}

export interface EditResult {
  state: TimelineProjectState;
  appliedDelta: number;
  affectedClipIds: string[];
}

/**
 * A normalized output-canvas region. Streamer layouts use two viewports so a
 * single source can be framed independently in the face and content areas.
 */
export interface CompositorViewport {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CompositorLayer {
  clipId: string;
  trackId: string;
  kind: "video" | "image" | "text";
  file: string;
  sourceTime: number;
  localTime: number;
  transform: ClipTransform;
  opacity: number;
  transitionType: TransitionType;
  transitionPhase: "none" | "in" | "out";
  transitionProgress: number;
  text: TextClipStyle | null;
  /** Combined enter/exit text-animation snapshot for this frame. */
  textFx?: TextEffectState | null;
  /** Optional 0..1 canvas region; absent means the full delivery canvas. */
  viewport?: CompositorViewport | null;
  /** Ephemeral preview metadata; never persisted in a project document. */
  layoutRole?: "streamer-content" | "streamer-face";
}

export interface AudioMixSource {
  clipId: string;
  trackId: string;
  file: string;
  sourceTime: number;
  localTime: number;
  playbackRate: number;
  gain: number;
  pan: number;
}

export interface ClipVolumeRegion {
  start: number;
  end: number;
  volume: number;
}

export interface FramePlan {
  time: number;
  layers: CompositorLayer[];
  audio: AudioMixSource[];
}

const DEFAULT_TRANSITION: ClipTransition = { type: "none", duration: 0 };

export function createTrack(
  id: string,
  name: string,
  type: TrackType,
  overrides: Partial<TimelineTrack> = {},
): TimelineTrack {
  return {
    id,
    name,
    type,
    muted: false,
    solo: false,
    locked: false,
    gain: 1,
    pan: 0,
    ...overrides,
  };
}

export function createClip(
  input: Pick<
    TimelineClip,
    "id" | "trackId" | "kind" | "start" | "duration"
  > &
    Partial<Omit<TimelineClip, "id" | "trackId" | "kind" | "start" | "duration">>,
): TimelineClip {
  const trimIn = finiteOr(input.trimIn ?? input.videoOffset, 0);
  const duration = Math.max(MIN_CLIP_DURATION, finiteOr(input.duration, 1));
  const speed = clamp(finiteOr(input.speed, 1), 0.05, 16);
  const noiseReduction = normalizeNoiseReduction(input.noiseReduction);
  const keyframes = normalizeKeyframes(input.keyframes);
  const voiceRider = normalizeVoiceRiderMetadata(
    input.voiceRider,
    keyframes.riderGain,
  );
  if (!voiceRider) delete keyframes.riderGain;
  const incomingTransition = normalizeTransition(input.transition?.in, duration);
  const outgoingTransition = normalizeTransition(
    input.transition?.out,
    Math.max(0, duration - incomingTransition.duration),
  );
  return {
    id: input.id,
    trackId: input.trackId,
    file: input.file ?? "",
    kind: input.kind,
    start: Math.max(0, finiteOr(input.start, 0)),
    duration,
    trimIn: Math.max(0, trimIn),
    videoOffset: Math.max(0, trimIn),
    sourceDuration:
      input.sourceDuration === null || input.sourceDuration === undefined
        ? null
        : Math.max(0, finiteOr(input.sourceDuration, 0)),
    speed,
    volume: clamp(finiteOr(input.volume, 1), 0, 4),
    pan: clamp(finiteOr(input.pan, 0), -1, 1),
    transform: {
      x: finiteOr(input.transform?.x, 0),
      y: finiteOr(input.transform?.y, 0),
      scale: clamp(finiteOr(input.transform?.scale, 1), 0.01, 100),
      rotation: finiteOr(input.transform?.rotation, 0),
      opacity: clamp(finiteOr(input.transform?.opacity, 1), 0, 1),
      shake: clamp(finiteOr(input.transform?.shake, 0), 0, 100),
    },
    transition: {
      in: incomingTransition,
      out: outgoingTransition,
    },
    keyframes,
    voiceRider,
    noiseReduction,
    speechAnalysis:
      input.kind === "audio" || input.kind === "video"
        ? normalizeSpeechAnalysis(input.speechAnalysis, {
            file: input.file ?? "",
            trimIn: Math.max(0, trimIn),
            duration,
            speed,
            noiseReduction,
          })
        : null,
    autoDucking:
      input.kind === "audio" || input.kind === "video"
        ? normalizeAutoDucking(input.autoDucking)
        : null,
    beatAnalysis:
      input.kind === "audio" || input.kind === "video"
        ? normalizeBeatAnalysis(input.beatAnalysis, input.file ?? "")
        : null,
    text: input.text ? normalizeTextStyle(input.text) : null,
    waveform: [...(input.waveform ?? [])].map((value) => clamp(value, 0, 1)),
    audioSeparated: input.audioSeparated ?? false,
  };
}

const TEXT_ANIMATION_TYPES: readonly TextAnimationType[] = [
  "none",
  "fade",
  "slide-up",
  "slide-down",
  "slide-left",
  "slide-right",
  "zoom",
  "bounce",
  "typewriter",
  "flip-3d",
  "spin",
];

function normalizeTextAnimation(
  animation: Partial<TextAnimation> | undefined,
  fallbackDuration: number,
): TextAnimation {
  const type =
    animation?.type && TEXT_ANIMATION_TYPES.includes(animation.type)
      ? animation.type
      : "none";
  return {
    type,
    duration: clamp(finiteOr(animation?.duration, fallbackDuration), 0.05, 5),
  };
}

/** Fills style defaults so clips saved before a field existed keep loading. */
export function normalizeTextStyle(
  input: Partial<TextClipStyle> | null | undefined,
): TextClipStyle {
  return {
    content: input?.content ?? "",
    fontFamily: input?.fontFamily || "Inter",
    fontSize: clamp(finiteOr(input?.fontSize, 64), 8, 512),
    fontWeight: clamp(finiteOr(input?.fontWeight, 600), 100, 900),
    color: input?.color || "#ffffff",
    backgroundColor: input?.backgroundColor || "transparent",
    align:
      input?.align === "left" || input?.align === "right" ? input.align : "center",
    ...(input?.fontPath ? { fontPath: input.fontPath } : {}),
    strokeColor: input?.strokeColor || "#000000",
    strokeWidth: clamp(finiteOr(input?.strokeWidth, 0), 0, 40),
    shadowColor: input?.shadowColor || "transparent",
    shadowBlur: clamp(finiteOr(input?.shadowBlur, 0), 0, 100),
    shadowOffsetX: clamp(finiteOr(input?.shadowOffsetX, 0), -100, 100),
    shadowOffsetY: clamp(finiteOr(input?.shadowOffsetY, 0), -100, 100),
    gradient:
      input?.gradient?.from && input.gradient.to
        ? { from: input.gradient.from, to: input.gradient.to }
        : null,
    animationIn: normalizeTextAnimation(input?.animationIn, 0.6),
    animationOut: normalizeTextAnimation(input?.animationOut, 0.6),
  };
}

export function createTextClip(
  input: Pick<TimelineClip, "id" | "trackId" | "start" | "duration"> &
    Partial<TimelineClip>,
): TimelineClip {
  return createClip({
    ...input,
    kind: "text",
    text: normalizeTextStyle(
      input.text ?? { content: "Yeni metin" },
    ),
  });
}

export function normalizeProjectState(
  state: Partial<TimelineProjectState> & {
    tracks?: Array<Partial<TimelineTrack> & Pick<TimelineTrack, "id" | "name" | "type">>;
    clips?: Array<Partial<TimelineClip> & Pick<TimelineClip, "id" | "trackId" | "start" | "duration">>;
  },
): TimelineProjectState {
  const tracks = (state.tracks ?? []).map((track) =>
    createTrack(track.id, track.name, track.type, track),
  );
  const trackTypes = new Map(tracks.map((track) => [track.id, track.type]));
  const clips = (state.clips ?? []).map((clip) => {
    const inferredKind: ClipKind =
      clip.kind ?? (trackTypes.get(clip.trackId) === "audio" ? "audio" : trackTypes.get(clip.trackId) === "text" ? "text" : "video");
    return createClip({ ...clip, kind: inferredKind });
  });

  return {
    version: PROJECT_STATE_VERSION,
    tracks,
    clips,
    selectedClipId:
      state.selectedClipId && clips.some((clip) => clip.id === state.selectedClipId)
        ? state.selectedClipId
        : null,
  };
}

export function cloneProjectState(state: TimelineProjectState): TimelineProjectState {
  return normalizeProjectState(state);
}

/** JSON-safe render/save contract. Seconds are retained for lossless UI round trips. */
export function serializeProjectState(
  state: TimelineProjectState,
): TimelineProjectSnapshot {
  const normalized = cloneProjectState(state);
  return {
    version: PROJECT_STATE_VERSION,
    timebase: "milliseconds",
    tracks: normalized.tracks.map((track) => ({ ...track })),
    clips: normalized.clips.map((clip) => ({
      ...clip,
      transform: { ...clip.transform },
      transition: {
        in: { ...clip.transition.in },
        out: { ...clip.transition.out },
      },
      keyframes: normalizeKeyframes(clip.keyframes),
      text: clip.text ? { ...clip.text } : null,
      waveform: [...clip.waveform],
      path: clip.file,
      startMs: Math.round(clip.start * 1000),
      durationMs: Math.round(clip.duration * 1000),
      trimInMs: Math.round(clip.trimIn * 1000),
    })),
    selectedClipId: normalized.selectedClipId,
    durationMs: Math.round(getTimelineDuration(normalized) * 1000),
  };
}

export function getTimelineDuration(state: TimelineProjectState): number {
  return state.clips.reduce(
    (maximum, clip) => Math.max(maximum, clip.start + clip.duration),
    0,
  );
}

export function getSourceTime(clip: TimelineClip, timelineTime: number): number {
  const localTime = clamp(timelineTime - clip.start, 0, clip.duration);
  return clip.trimIn + localTime * clip.speed;
}

/**
 * Moves a clip atomically without changing its media timing.
 *
 * Insert coordinates are interpreted in the caller's original timeline. An
 * insert first extracts the clip and ripples its source followers left. For a
 * later same-track insertion the requested edit point is mapped through that
 * extraction. Any gap adjacent to the target edit point is filled from its
 * left edge before following clips are shifted. Inserting on the same track
 * at the clip's own start or own end is a no-op.
 *
 * The returned project is a normalized clone. A rejected move throws before
 * changing either the returned clone or the caller-owned project state.
 */
export function moveClip(
  inputState: TimelineProjectState,
  clipId: string,
  targetTrackId: string,
  targetStart: number,
  options: MoveOptions = {},
): EditResult {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);

  if (!Number.isFinite(targetStart) || targetStart < 0) {
    throw new RangeError("Clip start must be a finite non-negative number.");
  }

  const targetTrack = state.tracks.find((track) => track.id === targetTrackId);
  if (!targetTrack) throw new Error(`Unknown track: ${targetTrackId}`);
  if (targetTrack.locked) throw new Error(`Track ${targetTrack.id} is locked.`);
  if (!canPlaceClipOnTrack(clip.kind, targetTrack.type)) {
    throw new Error(
      `Clip kind ${clip.kind} is not compatible with ${targetTrack.type} track ${targetTrack.id}.`,
    );
  }

  const mode = options.mode ?? "move";
  if (mode !== "move" && mode !== "insert") {
    throw new Error(`Unknown move mode: ${String(mode)}`);
  }

  let nextStart = Math.max(0, targetStart);
  const previousStart = clip.start;

  if (mode === "insert") {
    const sourceTrackId = clip.trackId;
    const previousEnd = previousStart + clip.duration;
    if (
      sourceTrackId === targetTrack.id &&
      (Math.abs(nextStart - previousStart) <= TIMELINE_EPSILON ||
        Math.abs(nextStart - previousEnd) <= TIMELINE_EPSILON)
    ) {
      return { state, appliedDelta: 0, affectedClipIds: [] };
    }

    const affectedClipIds = new Set<string>();
    const sourceFollowers = state.clips.filter(
      (other) =>
        other.id !== clip.id &&
        other.trackId === sourceTrackId &&
        other.start >= previousEnd - TIMELINE_EPSILON,
    );
    for (const other of sourceFollowers) {
      other.start = Math.max(0, other.start - clip.duration);
      affectedClipIds.add(other.id);
    }

    // The pointer/edit point belongs to the caller's pre-extraction timeline.
    // A later point on the same track moves left with the extracted duration.
    if (
      sourceTrackId === targetTrack.id &&
      nextStart >= previousEnd - TIMELINE_EPSILON
    ) {
      nextStart = Math.max(0, nextStart - clip.duration);
    }

    const targetClips = state.clips.filter(
      (other) => other.id !== clip.id && other.trackId === targetTrack.id,
    );

    // Stabilize pointer-derived values that are only microscopically away from
    // an edit point so the inserted clip lands on the exact shared boundary.
    let nearestBoundary = nextStart;
    let nearestDistance = TIMELINE_EPSILON;
    for (const other of targetClips) {
      for (const boundary of [other.start, other.start + other.duration]) {
        const distance = Math.abs(nextStart - boundary);
        if (distance <= nearestDistance) {
          nearestBoundary = boundary;
          nearestDistance = distance;
        }
      }
    }
    nextStart = nearestBoundary;

    const containingClip = targetClips.find(
      (other) =>
        nextStart > other.start + TIMELINE_EPSILON &&
        nextStart < other.start + other.duration - TIMELINE_EPSILON,
    );
    if (containingClip) {
      throw new RangeError(
        `Insert point is inside clip ${containingClip.id} on track ${targetTrack.id}.`,
      );
    }

    const previousTargetEnd = targetClips.reduce((latest, other) => {
      const end = other.start + other.duration;
      return end <= nextStart + TIMELINE_EPSILON
        ? Math.max(latest, end)
        : latest;
    }, 0);
    const insertedStart = Math.min(nextStart, previousTargetEnd);
    const rightClips = targetClips.filter(
      (other) => other.start >= nextStart - TIMELINE_EPSILON,
    );
    const nextTargetStart = rightClips.reduce(
      (earliest, other) => Math.min(earliest, other.start),
      Number.POSITIVE_INFINITY,
    );
    const availableGap = Number.isFinite(nextTargetStart)
      ? Math.max(0, nextTargetStart - insertedStart)
      : Number.POSITIVE_INFINITY;
    const overflow = Number.isFinite(availableGap)
      ? Math.max(0, clip.duration - availableGap)
      : 0;
    const shiftedClips = overflow > TIMELINE_EPSILON ? rightClips : [];
    for (const other of shiftedClips) {
      other.start += overflow;
      affectedClipIds.add(other.id);
    }

    clip.trackId = targetTrack.id;
    clip.start = insertedStart;
    affectedClipIds.delete(clip.id);

    return {
      state,
      appliedDelta: insertedStart - previousStart,
      affectedClipIds: [clip.id, ...affectedClipIds],
    };
  }

  const sourceTrackId = clip.trackId;
  const previousEnd = previousStart + clip.duration;
  const rippleAffectedClipIds = new Set<string>();
  if (options.rippleSource) {
    if (
      sourceTrackId === targetTrack.id &&
      (Math.abs(nextStart - previousStart) <= TIMELINE_EPSILON ||
        Math.abs(nextStart - previousEnd) <= TIMELINE_EPSILON)
    ) {
      return { state, appliedDelta: 0, affectedClipIds: [] };
    }

    for (const other of state.clips) {
      if (
        other.id !== clip.id &&
        other.trackId === sourceTrackId &&
        other.start >= previousEnd - TIMELINE_EPSILON
      ) {
        other.start = Math.max(0, other.start - clip.duration);
        rippleAffectedClipIds.add(other.id);
      }
    }
    if (
      sourceTrackId === targetTrack.id &&
      nextStart >= previousEnd - TIMELINE_EPSILON
    ) {
      nextStart = Math.max(0, nextStart - clip.duration);
    }
  }

  const nextEnd = nextStart + clip.duration;
  const overlap = state.clips.find(
    (other) =>
      other.id !== clip.id &&
      other.trackId === targetTrack.id &&
      nextStart < other.start + other.duration - TIMELINE_EPSILON &&
      nextEnd > other.start + TIMELINE_EPSILON,
  );
  if (overlap) {
    throw new RangeError(
      `Move would overlap clip ${overlap.id} on track ${targetTrack.id}.`,
    );
  }

  const moved =
    clip.trackId !== targetTrack.id ||
    nextStart !== previousStart;
  clip.trackId = targetTrack.id;
  clip.start = nextStart;

  return {
    state,
    appliedDelta: nextStart - previousStart,
    affectedClipIds: moved
      ? [clip.id, ...rippleAffectedClipIds]
      : [...rippleAffectedClipIds],
  };
}

export function trimClip(
  inputState: TimelineProjectState,
  clipId: string,
  edge: TrimEdge,
  requestedDelta: number,
  options: TrimOptions = {},
): EditResult {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  if (!Number.isFinite(requestedDelta) || requestedDelta === 0) {
    return { state, appliedDelta: 0, affectedClipIds: [] };
  }

  const oldEnd = clip.start + clip.duration;
  let appliedDelta: number;

  const previousEnd = state.clips.reduce((latest, other) => {
    if (
      other.id === clip.id ||
      other.trackId !== clip.trackId ||
      other.start > clip.start
    ) {
      return latest;
    }
    return Math.max(latest, other.start + other.duration);
  }, Number.NEGATIVE_INFINITY);
  const nextStart = state.clips.reduce((earliest, other) => {
    if (
      other.id === clip.id ||
      other.trackId !== clip.trackId ||
      other.start < clip.start
    ) {
      return earliest;
    }
    return Math.min(earliest, other.start);
  }, Number.POSITIVE_INFINITY);

  if (edge === "start") {
    const sourceMinimum = -clip.trimIn / clip.speed;
    const trackMinimum = Number.isFinite(previousEnd)
      ? Math.min(0, previousEnd - clip.start)
      : Number.NEGATIVE_INFINITY;
    const minimumDelta = options.ripple
      ? sourceMinimum
      : Math.max(sourceMinimum, -clip.start, trackMinimum);
    const maximumDelta = clip.duration - MIN_CLIP_DURATION;
    appliedDelta = clamp(requestedDelta, minimumDelta, maximumDelta);
    clip.start += appliedDelta;
    clip.duration -= appliedDelta;
    clip.trimIn += appliedDelta * clip.speed;
    clip.videoOffset = clip.trimIn;
  } else {
    const minimumDelta = -(clip.duration - MIN_CLIP_DURATION);
    let maximumDelta =
      clip.sourceDuration === null
        ? Number.POSITIVE_INFINITY
        : Math.max(
            0,
            (clip.sourceDuration - clip.trimIn) / clip.speed - clip.duration,
          );
    if (!options.ripple && Number.isFinite(nextStart)) {
      maximumDelta = Math.min(
        maximumDelta,
        Math.max(0, nextStart - oldEnd),
      );
    }
    appliedDelta = clamp(requestedDelta, minimumDelta, maximumDelta);
    clip.duration += appliedDelta;
  }
  clampClipTransitions(clip);
  if (Math.abs(appliedDelta) >= TIMELINE_EPSILON) {
    clearVoiceRiderEnvelope(clip);
    clip.speechAnalysis = null;
  }

  const affectedClipIds = [clip.id];
  if (options.ripple && appliedDelta !== 0) {
    if (edge === "start") clip.start -= appliedDelta;
    const cutPoint = oldEnd;
    const rippleDelta = edge === "start" ? -appliedDelta : appliedDelta;
    for (const other of state.clips) {
      if (
        other.id !== clip.id &&
        other.trackId === clip.trackId &&
        other.start >= cutPoint - TIMELINE_EPSILON
      ) {
        other.start = Math.max(0, other.start + rippleDelta);
        affectedClipIds.push(other.id);
      }
    }
  }

  return { state, appliedDelta, affectedClipIds };
}

export function deleteClip(
  inputState: TimelineProjectState,
  clipId: string,
  options: DeleteOptions = {},
): EditResult {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  const end = clip.start + clip.duration;
  const affectedClipIds: string[] = [];
  state.clips = state.clips.filter((item) => item.id !== clipId);

  if (options.ripple) {
    for (const other of state.clips) {
      if (other.trackId === clip.trackId && other.start >= end - TIMELINE_EPSILON) {
        other.start = Math.max(clip.start, other.start - clip.duration);
        affectedClipIds.push(other.id);
      }
    }
  }
  if (state.selectedClipId === clipId) state.selectedClipId = null;

  return {
    state,
    appliedDelta: options.ripple ? -clip.duration : 0,
    affectedClipIds,
  };
}

export function deleteTrack(
  inputState: TimelineProjectState,
  trackId: string,
  options: DeleteTrackOptions = {},
): TimelineProjectState {
  const state = cloneProjectState(inputState);
  const track = state.tracks.find((item) => item.id === trackId);
  if (!track) throw new Error(`Unknown track: ${trackId}`);
  if (track.locked) throw new Error(`Track ${track.id} is locked.`);
  if (track.type === "text") {
    throw new Error(`Text track ${track.id} cannot be deleted.`);
  }

  const tracksOfType = state.tracks.filter((item) => item.type === track.type);
  if (tracksOfType.length <= 1) {
    throw new Error(`Cannot delete the last ${track.type} track.`);
  }

  const affectedClipIds = state.clips
    .filter((clip) => clip.trackId === track.id)
    .map((clip) => clip.id);
  if (affectedClipIds.length > 0 && !options.cascade) {
    throw new Error(`Track ${track.id} is not empty.`);
  }

  state.tracks = state.tracks.filter((item) => item.id !== track.id);
  if (affectedClipIds.length > 0) {
    const affectedClipIdSet = new Set(affectedClipIds);
    state.clips = state.clips.filter((clip) => !affectedClipIdSet.has(clip.id));
    if (
      state.selectedClipId !== null &&
      affectedClipIdSet.has(state.selectedClipId)
    ) {
      state.selectedClipId = null;
    }
  }

  return state;
}

export function splitClip(
  inputState: TimelineProjectState,
  clipId: string,
  atTime: number,
  newClipId: string,
): EditResult {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  const originalDuration = clip.duration;
  const originalSpeechAnalysis = clip.speechAnalysis;
  const local = atTime - clip.start;
  if (local < MIN_CLIP_DURATION || local > clip.duration - MIN_CLIP_DURATION) {
    throw new RangeError("Split point must be inside the clip.");
  }

  const second = createClip({
    ...clip,
    speechAnalysis: null,
    id: newClipId,
    start: atTime,
    duration: clip.duration - local,
    trimIn: clip.trimIn + local * clip.speed,
    videoOffset: clip.trimIn + local * clip.speed,
    transition: {
      in: DEFAULT_TRANSITION,
      out: clip.transition.out,
    },
    keyframes: splitKeyframes(clip.keyframes, local, true),
  });
  clip.duration = local;
  clip.transition.out = { ...DEFAULT_TRANSITION };
  clampClipTransitions(clip);
  clip.keyframes = splitKeyframes(clip.keyframes, local, false);
  if (originalSpeechAnalysis) {
    clip.speechAnalysis = projectSpeechAnalysis(
      originalSpeechAnalysis,
      0,
      local,
      speechAnalysisSignature(clip),
    );
    second.speechAnalysis = projectSpeechAnalysis(
      originalSpeechAnalysis,
      local,
      originalDuration,
      speechAnalysisSignature(second),
    );
  }
  const index = state.clips.findIndex((item) => item.id === clipId);
  state.clips.splice(index + 1, 0, second);
  state.selectedClipId = second.id;

  return {
    state,
    appliedDelta: 0,
    affectedClipIds: [clip.id, second.id],
  };
}

export function setClipSpeed(
  inputState: TimelineProjectState,
  clipId: string,
  speed: number,
  preserve: "source" | "timeline" = "source",
  ripple = false,
): EditResult {
  if (!Number.isFinite(speed) || speed < 0.05 || speed > 16) {
    throw new RangeError("Speed must be between 0.05 and 16.");
  }
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  const oldDuration = clip.duration;
  const oldSpeed = clip.speed;
  const consumedSourceDuration = clip.duration * oldSpeed;
  clip.speed = speed;
  if (preserve === "source") {
    clip.duration = Math.max(MIN_CLIP_DURATION, consumedSourceDuration / speed);
  }
  clampClipTransitions(clip);
  if (Math.abs(speed - oldSpeed) > TIMELINE_EPSILON) {
    clearVoiceRiderEnvelope(clip);
    clip.speechAnalysis = null;
  }
  const durationDelta = clip.duration - oldDuration;
  const affectedClipIds = [clip.id];

  if (ripple && durationDelta !== 0) {
    const oldEnd = clip.start + oldDuration;
    for (const other of state.clips) {
      if (
        other.id !== clip.id &&
        other.trackId === clip.trackId &&
        other.start >= oldEnd - TIMELINE_EPSILON
      ) {
        other.start += durationDelta;
        affectedClipIds.push(other.id);
      }
    }
  }
  return { state, appliedDelta: durationDelta, affectedClipIds };
}

export function setTrackState(
  inputState: TimelineProjectState,
  trackId: string,
  updates: Partial<Pick<TimelineTrack, "muted" | "solo" | "locked" | "gain" | "pan">>,
): TimelineProjectState {
  const state = cloneProjectState(inputState);
  const track = state.tracks.find((item) => item.id === trackId);
  if (!track) throw new Error(`Unknown track: ${trackId}`);
  if (updates.muted !== undefined) track.muted = updates.muted;
  if (updates.solo !== undefined) track.solo = updates.solo;
  if (updates.locked !== undefined) track.locked = updates.locked;
  if (updates.gain !== undefined) track.gain = clamp(updates.gain, 0, 4);
  if (updates.pan !== undefined) track.pan = clamp(updates.pan, -1, 1);
  return state;
}

export function upsertKeyframe(
  inputState: TimelineProjectState,
  clipId: string,
  property: AnimatableProperty,
  keyframe: TimelineKeyframe,
): TimelineProjectState {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  const normalized: TimelineKeyframe = {
    time: clamp(finiteOr(keyframe.time, 0), 0, clip.duration),
    value: finiteOr(keyframe.value, 0),
    easing: keyframe.easing ?? "linear",
  };
  const frames = [...(clip.keyframes[property] ?? [])];
  const existingIndex = frames.findIndex(
    (item) => Math.abs(item.time - normalized.time) < TIMELINE_EPSILON,
  );
  if (existingIndex === -1) frames.push(normalized);
  else frames[existingIndex] = normalized;
  frames.sort((a, b) => a.time - b.time);
  clip.keyframes[property] = frames;
  return state;
}

export function removeKeyframe(
  inputState: TimelineProjectState,
  clipId: string,
  property: AnimatableProperty,
  time: number,
): TimelineProjectState {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  clip.keyframes[property] = (clip.keyframes[property] ?? []).filter(
    (item) => Math.abs(item.time - time) >= TIMELINE_EPSILON,
  );
  return state;
}

export function setVoiceRiderEnvelope(
  inputState: TimelineProjectState,
  clipId: string,
  metadata: VoiceRiderMetadata,
  frames: readonly TimelineKeyframe[],
): TimelineProjectState {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  if (clip.kind !== "audio" && clip.kind !== "video") {
    throw new Error("Voice Rider can only be applied to audio or video clips.");
  }
  const normalized = frames
    .map((frame) => ({
      time: clamp(finiteOr(frame.time, 0), 0, clip.duration),
      value: clamp(finiteOr(frame.value, 1), 0, 4),
      easing: frame.easing ?? "ease-in-out",
    }))
    .sort((left, right) => left.time - right.time)
    .filter(
      (frame, index, values) =>
        index === values.length - 1 ||
        Math.abs(values[index + 1].time - frame.time) >= TIMELINE_EPSILON,
    );
  if (normalized.length < 2) {
    throw new Error("Voice Rider envelope must contain at least two points.");
  }
  clip.keyframes.riderGain = normalized;
  clip.voiceRider = { ...metadata };
  return state;
}

export function removeVoiceRiderEnvelope(
  inputState: TimelineProjectState,
  clipId: string,
): TimelineProjectState {
  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  clearVoiceRiderEnvelope(clip);
  return state;
}

export function evaluateKeyframes(
  frames: readonly TimelineKeyframe[] | undefined,
  time: number,
  fallback: number,
): number {
  if (!frames?.length) return fallback;
  const sorted = [...frames].sort((a, b) => a.time - b.time);
  if (time <= sorted[0].time) return sorted[0].value;
  const last = sorted[sorted.length - 1];
  if (time >= last.time) return last.value;

  for (let index = 0; index < sorted.length - 1; index += 1) {
    const left = sorted[index];
    const right = sorted[index + 1];
    if (time < left.time || time > right.time) continue;
    if (left.easing === "hold") return left.value;
    const progress = (time - left.time) / (right.time - left.time);
    const eased = applyEasing(progress, left.easing);
    return left.value + (right.value - left.value) * eased;
  }
  return fallback;
}

/** Reconstructs committed lower-volume plateaus from their volume keyframes. */
export function getClipVolumeRegions(clip: TimelineClip): ClipVolumeRegion[] {
  const frames = clip.keyframes.volume;
  if (!frames || frames.length < 2) return [];

  const sorted = [...frames].sort((left, right) => left.time - right.time);
  const regions: ClipVolumeRegion[] = [];
  for (let index = 0; index < sorted.length - 1; index += 1) {
    const left = sorted[index];
    const right = sorted[index + 1];
    if (
      Math.abs(left.value - right.value) < TIMELINE_EPSILON &&
      left.value < clip.volume - 0.01 &&
      right.time - left.time > TIMELINE_EPSILON
    ) {
      regions.push({ start: left.time, end: right.time, volume: left.value });
    }
  }
  return regions;
}

/**
 * Converts the editor's linear gain multiplier to a relative decibel value.
 * 1.0 is the untouched clip level (0 dB); this is gain, not measured dBFS/LUFS.
 */
export function gainToDecibels(gain: number): number {
  const normalized = clamp(Number.isFinite(gain) ? gain : 0, 0, 4);
  return normalized === 0 ? Number.NEGATIVE_INFINITY : 20 * Math.log10(normalized);
}

/**
 * Converts a relative decibel value back to the editor's linear gain range.
 * Negative infinity is the explicit mute value, while finite values are capped
 * at the project's supported 0–400% gain range.
 */
export function decibelsToGain(decibels: number): number {
  if (decibels === Number.NEGATIVE_INFINITY) return 0;
  if (!Number.isFinite(decibels)) return 0;
  return clamp(10 ** (decibels / 20), 0, 4);
}

/** Maps a vertical pointer drag to a volume value; positive pixels mean upward. */
export function getVerticalVolumeDragValue(
  startVolume: number,
  maxVolume: number,
  upwardPixels: number,
  sensitivityPixels = 140,
): number {
  const maximum = clamp(Number.isFinite(maxVolume) ? maxVolume : 0, 0, 4);
  const start = clamp(Number.isFinite(startVolume) ? startVolume : 0, 0, maximum);
  const delta = Number.isFinite(upwardPixels) ? upwardPixels : 0;
  const sensitivity = Math.max(
    1,
    Number.isFinite(sensitivityPixels) ? sensitivityPixels : 140,
  );
  return clamp(start + (delta / sensitivity) * maximum, 0, maximum);
}

export function setClipVolumeRegion(
  inputState: TimelineProjectState,
  clipId: string,
  regionStart: number,
  regionEnd: number,
  volume: number,
  fadeDuration = 0.18,
): TimelineProjectState {
  if (
    !Number.isFinite(regionStart) ||
    !Number.isFinite(regionEnd) ||
    !Number.isFinite(volume) ||
    !Number.isFinite(fadeDuration) ||
    fadeDuration < 0
  ) {
    throw new RangeError("Volume region values must be finite and fade must be non-negative.");
  }

  const state = cloneProjectState(inputState);
  const clip = requireEditableClip(state, clipId);
  const start = clamp(Math.min(regionStart, regionEnd), 0, clip.duration);
  const end = clamp(Math.max(regionStart, regionEnd), start, clip.duration);
  if (end - start < TIMELINE_EPSILON) {
    throw new RangeError("Volume region must have a positive duration.");
  }

  const fade = Math.min(fadeDuration, (end - start) / 3);
  const fadeStart = Math.max(0, start - fade);
  const fadeEnd = Math.min(clip.duration, end + fade);
  const existingFrames = clip.keyframes.volume ?? [];
  const volumeBefore = evaluateKeyframes(existingFrames, fadeStart, clip.volume);
  const volumeAfter = evaluateKeyframes(existingFrames, fadeEnd, clip.volume);
  const frames: TimelineKeyframe[] = existingFrames
    .filter(
      (frame) =>
        frame.time < fadeStart - TIMELINE_EPSILON ||
        frame.time > fadeEnd + TIMELINE_EPSILON,
    )
    .map((frame) => ({ ...frame }));

  if (fadeStart < start - TIMELINE_EPSILON) {
    frames.push({ time: fadeStart, value: volumeBefore, easing: "linear" });
  }
  frames.push({ time: start, value: clamp(volume, 0, 4), easing: "linear" });
  frames.push({ time: end, value: clamp(volume, 0, 4), easing: "linear" });
  if (fadeEnd > end + TIMELINE_EPSILON) {
    frames.push({ time: fadeEnd, value: volumeAfter, easing: "linear" });
  }
  frames.sort((left, right) => left.time - right.time);
  clip.keyframes.volume = frames;
  return state;
}

export function buildFramePlan(
  state: TimelineProjectState,
  timelineTime: number,
): FramePlan {
  const active = state.clips.filter(
    (clip) => timelineTime >= clip.start && timelineTime < clip.start + clip.duration,
  );
  const trackOrder = new Map(state.tracks.map((track, index) => [track.id, index]));
  const hasSolo = state.tracks.some((track) => track.solo);
  const trackById = new Map(state.tracks.map((track) => [track.id, track]));

  const layers: CompositorLayer[] = active
    .filter((clip) => clip.kind === "video" || clip.kind === "image" || clip.kind === "text")
    .filter((clip) => {
      const track = trackById.get(clip.trackId);
      return !!track && !track.muted && (!hasSolo || track.solo);
    })
    .sort((a, b) => (trackOrder.get(a.trackId) ?? 0) - (trackOrder.get(b.trackId) ?? 0))
    .map((clip) => {
      const localTime = timelineTime - clip.start;
      const transition = getTransitionState(clip, localTime);
      const transform: ClipTransform = {
        x: evaluateProperty(clip, "x", localTime, clip.transform.x),
        y: evaluateProperty(clip, "y", localTime, clip.transform.y),
        scale: evaluateProperty(clip, "scale", localTime, clip.transform.scale),
        rotation: evaluateProperty(clip, "rotation", localTime, clip.transform.rotation),
        opacity: evaluateProperty(clip, "opacity", localTime, clip.transform.opacity),
        shake: evaluateProperty(clip, "shake", localTime, clip.transform.shake ?? 0),
      };
      return {
        clipId: clip.id,
        trackId: clip.trackId,
        kind: clip.kind as "video" | "image" | "text",
        file: clip.file,
        sourceTime: getSourceTime(clip, timelineTime),
        localTime,
        transform,
        opacity: clamp(
          transform.opacity *
            (transition.type === "dip-to-black" ||
            MASK_TRANSITION_TYPES.has(transition.type)
              ? 1
              : transition.opacity),
          0,
          1,
        ),
        transitionType: transition.type,
        transitionPhase: transition.phase,
        transitionProgress: transition.progress,
        text: clip.text,
        textFx: clip.kind === "text" ? getTextEffectState(clip, localTime) : null,
      };
    });

  const audio: AudioMixSource[] = active
    .filter((clip) => clip.kind === "audio" || clip.kind === "video")
    .filter((clip) => {
      const track = trackById.get(clip.trackId);
      return !!track && !track.muted && (!hasSolo || track.solo);
    })
    .map((clip) => {
      const track = trackById.get(clip.trackId)!;
      const localTime = timelineTime - clip.start;
      const duckGain = evaluateDuckingEnvelope(
        buildAutoDuckingEnvelope(state, clip),
        localTime,
      );
      return {
        clipId: clip.id,
        trackId: clip.trackId,
        file: clip.file,
        sourceTime: getSourceTime(clip, timelineTime),
        localTime,
        playbackRate: clip.speed,
        gain: clamp(
          // A separated video must stay connected to Web Audio at zero gain.
          // Otherwise its visible <video> element can bypass the mixer and play
          // the embedded source audio after the separate Voice clip is deleted.
          (clip.audioSeparated
            ? 0
            : evaluateProperty(clip, "volume", localTime, clip.volume) *
              evaluateProperty(clip, "riderGain", localTime, 1) *
              duckGain) * track.gain,
          0,
          4,
        ),
        pan: clamp(evaluateProperty(clip, "pan", localTime, clip.pan) + track.pan, -1, 1),
      };
    });

  return { time: timelineTime, layers, audio };
}

export function generateWaveformPeaks(
  channels: readonly Float32Array[],
  bucketCount: number,
): number[] {
  if (!Number.isInteger(bucketCount) || bucketCount <= 0) {
    throw new RangeError("bucketCount must be a positive integer.");
  }
  const sampleCount = channels.reduce(
    (maximum, channel) => Math.max(maximum, channel.length),
    0,
  );
  if (sampleCount === 0 || channels.length === 0) {
    return Array.from({ length: bucketCount }, () => 0);
  }

  const peaks: number[] = [];
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const start = Math.floor((bucket * sampleCount) / bucketCount);
    const end = Math.max(start + 1, Math.floor(((bucket + 1) * sampleCount) / bucketCount));
    let peak = 0;
    for (const channel of channels) {
      for (let sample = start; sample < Math.min(end, channel.length); sample += 1) {
        peak = Math.max(peak, Math.abs(channel[sample] ?? 0));
      }
    }
    peaks.push(clamp(peak, 0, 1));
  }
  return peaks;
}

export function validateTimeline(state: TimelineProjectState): TimelineIssue[] {
  const issues: TimelineIssue[] = [];
  const ids = new Set<string>();
  const trackIds = new Set(state.tracks.map((track) => track.id));
  for (const item of [...state.tracks, ...state.clips]) {
    if (ids.has(item.id)) {
      issues.push({ code: "duplicate-id", message: `Duplicate id: ${item.id}` });
    }
    ids.add(item.id);
  }

  for (const clip of state.clips) {
    if (!trackIds.has(clip.trackId)) {
      issues.push({
        code: "missing-track",
        message: `Clip ${clip.id} references missing track ${clip.trackId}.`,
        clipId: clip.id,
      });
    }
    if (clip.start < 0 || clip.duration < MIN_CLIP_DURATION) {
      issues.push({ code: "invalid-time", message: `Invalid timing on ${clip.id}.`, clipId: clip.id });
    }
    if (clip.speed < 0.05 || clip.speed > 16) {
      issues.push({ code: "invalid-speed", message: `Invalid speed on ${clip.id}.`, clipId: clip.id });
    }
    if (
      clip.sourceDuration !== null &&
      clip.trimIn + clip.duration * clip.speed > clip.sourceDuration + TIMELINE_EPSILON
    ) {
      issues.push({ code: "source-overrun", message: `${clip.id} exceeds source media.`, clipId: clip.id });
    }
    for (const transition of [clip.transition.in, clip.transition.out]) {
      if (!TRANSITION_TYPE_SET.has(transition.type)) {
        issues.push({
          code: "invalid-transition",
          message: `Unknown transition on ${clip.id}.`,
          clipId: clip.id,
        });
      }
      if (transition.duration > clip.duration + TIMELINE_EPSILON) {
        issues.push({
          code: "transition-too-long",
          message: `Transition is longer than ${clip.id}.`,
          clipId: clip.id,
        });
      }
    }
    if (
      clip.transition.in.duration + clip.transition.out.duration >
      clip.duration + TIMELINE_EPSILON
    ) {
      issues.push({
        code: "transition-overlap",
        message: `Transition envelopes overlap on ${clip.id}.`,
        clipId: clip.id,
      });
    }
  }

  for (const track of state.tracks) {
    const ordered = state.clips
      .filter((clip) => clip.trackId === track.id)
      .sort((a, b) => a.start - b.start);
    for (let index = 1; index < ordered.length; index += 1) {
      const previous = ordered[index - 1];
      const current = ordered[index];
      if (current.start < previous.start + previous.duration - TIMELINE_EPSILON) {
        issues.push({
          code: "overlap",
          message: `${previous.id} overlaps ${current.id} on ${track.id}.`,
          clipId: current.id,
          trackId: track.id,
        });
      }
    }
  }
  return issues;
}

function requireEditableClip(
  state: TimelineProjectState,
  clipId: string,
): TimelineClip {
  const clip = state.clips.find((item) => item.id === clipId);
  if (!clip) throw new Error(`Unknown clip: ${clipId}`);
  const track = state.tracks.find((item) => item.id === clip.trackId);
  if (!track) throw new Error(`Unknown track: ${clip.trackId}`);
  if (track.locked) throw new Error(`Track ${track.id} is locked.`);
  return clip;
}

function canPlaceClipOnTrack(kind: ClipKind, trackType: TrackType): boolean {
  if (kind === "video" || kind === "image") return trackType === "video";
  if (kind === "audio") return trackType === "audio";
  return trackType === "text";
}

function normalizeTransition(
  transition: Partial<ClipTransition> | undefined,
  maximumDuration = Number.POSITIVE_INFINITY,
): ClipTransition {
  const type = TRANSITION_TYPE_SET.has(transition?.type as TransitionType)
    ? transition!.type as TransitionType
    : "none";
  if (type === "none") return { ...DEFAULT_TRANSITION };
  const maximum = Number.isFinite(maximumDuration)
    ? Math.max(0, maximumDuration)
    : Number.POSITIVE_INFINITY;
  return {
    type,
    duration: Math.min(
      maximum,
      Math.max(0, finiteOr(transition?.duration, 0)),
    ),
  };
}

function clampClipTransitions(clip: TimelineClip): void {
  const incoming = normalizeTransition(clip.transition.in, clip.duration);
  clip.transition = {
    in: incoming,
    out: normalizeTransition(
      clip.transition.out,
      Math.max(0, clip.duration - incoming.duration),
    ),
  };
}

function normalizeKeyframes(keyframes: ClipKeyframes | undefined): ClipKeyframes {
  const normalized: ClipKeyframes = {};
  if (!keyframes) return normalized;
  for (const property of Object.keys(keyframes) as AnimatableProperty[]) {
    normalized[property] = (keyframes[property] ?? [])
      .map((frame) => ({
        time: Math.max(0, finiteOr(frame.time, 0)),
        value:
          property === "riderGain"
            ? clamp(finiteOr(frame.value, 1), 0, 4)
            : finiteOr(frame.value, 0),
        easing: frame.easing ?? "linear",
      }))
      .sort((a, b) => a.time - b.time);
  }
  return normalized;
}

function normalizeVoiceRiderMetadata(
  metadata: VoiceRiderMetadata | null | undefined,
  frames: readonly TimelineKeyframe[] | undefined,
): VoiceRiderMetadata | null {
  if (metadata?.model !== "silero-vad-v6" || !frames || frames.length < 2) {
    return null;
  }
  return {
    model: "silero-vad-v6",
    speechCoverage: clamp(finiteOr(metadata.speechCoverage, 0), 0, 1),
    averageSpeechProbability: clamp(
      finiteOr(metadata.averageSpeechProbability, 0),
      0,
      1,
    ),
    strongestCutDb: Math.min(0, finiteOr(metadata.strongestCutDb, 0)),
    strongestBoostDb: Math.max(0, finiteOr(metadata.strongestBoostDb, 0)),
  };
}

function clearVoiceRiderEnvelope(clip: TimelineClip) {
  const { riderGain: _riderGain, ...manualKeyframes } = clip.keyframes;
  clip.keyframes = manualKeyframes;
  clip.voiceRider = null;
}

function splitKeyframes(
  keyframes: ClipKeyframes,
  splitAt: number,
  secondHalf: boolean,
): ClipKeyframes {
  const result: ClipKeyframes = {};
  for (const property of Object.keys(keyframes) as AnimatableProperty[]) {
    const frames = [...(keyframes[property] ?? [])].sort(
      (left, right) => left.time - right.time,
    );
    if (frames.length === 0) continue;
    const boundaryValue = evaluateKeyframes(frames, splitAt, frames[0].value);
    const boundaryEasing =
      [...frames].reverse().find((frame) => frame.time <= splitAt)?.easing ??
      frames[0].easing;
    const splitFrames = frames
      .filter((frame) => (secondHalf ? frame.time >= splitAt : frame.time <= splitAt))
      .map((frame) => ({ ...frame, time: secondHalf ? frame.time - splitAt : frame.time }));
    const boundaryTime = secondHalf ? 0 : splitAt;
    if (!splitFrames.some((frame) => Math.abs(frame.time - boundaryTime) < TIMELINE_EPSILON)) {
      splitFrames.push({
        time: boundaryTime,
        value: boundaryValue,
        easing: boundaryEasing,
      });
      splitFrames.sort((left, right) => left.time - right.time);
    }
    result[property] = splitFrames;
  }
  return result;
}

function evaluateProperty(
  clip: TimelineClip,
  property: AnimatableProperty,
  localTime: number,
  fallback: number,
): number {
  return evaluateKeyframes(clip.keyframes[property], localTime, fallback);
}

function getTransitionState(
  clip: TimelineClip,
  localTime: number,
): {
  type: TransitionType;
  phase: "none" | "in" | "out";
  progress: number;
  opacity: number;
} {
  const incoming = clip.transition.in;
  if (incoming.type !== "none" && incoming.duration > 0 && localTime < incoming.duration) {
    const progress = clamp(localTime / incoming.duration, 0, 1);
    return { type: incoming.type, phase: "in", progress, opacity: progress };
  }
  const outgoing = clip.transition.out;
  const timeRemaining = clip.duration - localTime;
  if (outgoing.type !== "none" && outgoing.duration > 0 && timeRemaining <= outgoing.duration) {
    const progress = clamp(1 - timeRemaining / outgoing.duration, 0, 1);
    return { type: outgoing.type, phase: "out", progress, opacity: 1 - progress };
  }
  return { type: "none", phase: "none", progress: 1, opacity: 1 };
}

function easeOutCubic(x: number): number {
  return 1 - Math.pow(1 - clamp(x, 0, 1), 3);
}

function easeInCubic(x: number): number {
  const t = clamp(x, 0, 1);
  return t * t * t;
}

function easeOutBack(x: number): number {
  const t = clamp(x, 0, 1);
  const c1 = 1.70158;
  const c3 = c1 + 1;
  return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
}

function easeOutBounce(x: number): number {
  const t = clamp(x, 0, 1);
  const n1 = 7.5625;
  const d1 = 2.75;
  if (t < 1 / d1) return n1 * t * t;
  if (t < 2 / d1) {
    const u = t - 1.5 / d1;
    return n1 * u * u + 0.75;
  }
  if (t < 2.5 / d1) {
    const u = t - 2.25 / d1;
    return n1 * u * u + 0.9375;
  }
  const u = t - 2.625 / d1;
  return n1 * u * u + 0.984375;
}

const NEUTRAL_TEXT_FX: TextEffectState = {
  alpha: 1,
  dxFrac: 0,
  dyFrac: 0,
  scale: 1,
  scaleX: 1,
  rotation: 0,
  charProgress: 1,
};

/**
 * Enter animations run on raw progress p (0→1 after the clip starts); exit
 * animations mirror them with q (0→1 approaching the clip end). Both sides
 * are combined multiplicatively so short clips can overlap enter and exit.
 */
function applyTextAnimationSide(
  fx: TextEffectState,
  type: TextAnimationType,
  rawProgress: number,
  side: "in" | "out",
): void {
  // p: 1 = fully settled/visible for both sides.
  const p = side === "in" ? rawProgress : 1 - rawProgress;
  const eased = side === "in" ? easeOutCubic(p) : 1 - easeInCubic(1 - p);
  const slide = 1 - eased;
  switch (type) {
    case "fade":
      fx.alpha *= eased;
      break;
    case "slide-up":
      fx.alpha *= eased;
      fx.dyFrac += 0.12 * slide;
      break;
    case "slide-down":
      fx.alpha *= eased;
      fx.dyFrac -= 0.12 * slide;
      break;
    case "slide-left":
      fx.alpha *= eased;
      fx.dxFrac += 0.14 * slide;
      break;
    case "slide-right":
      fx.alpha *= eased;
      fx.dxFrac -= 0.14 * slide;
      break;
    case "zoom":
      fx.alpha *= clamp(p * 2.5, 0, 1);
      fx.scale *= side === "in" ? 0.25 + 0.75 * easeOutBack(p) : 0.25 + 0.75 * eased;
      break;
    case "bounce":
      fx.alpha *= clamp(p * 3, 0, 1);
      fx.scale *= side === "in" ? Math.max(0.05, easeOutBounce(p)) : Math.max(0.05, eased);
      break;
    case "typewriter":
      fx.charProgress = Math.min(fx.charProgress, clamp(p, 0, 1));
      break;
    case "flip-3d":
      fx.alpha *= clamp(p * 2, 0, 1);
      fx.scaleX *= Math.max(0.02, Math.cos(((1 - eased) * Math.PI) / 2));
      break;
    case "spin":
      fx.alpha *= eased;
      fx.scale *= 0.2 + 0.8 * eased;
      fx.rotation += (1 - eased) * (side === "in" ? -180 : 180);
      break;
    default:
      break;
  }
}

function getTextEffectState(
  clip: TimelineClip,
  localTime: number,
): TextEffectState | null {
  const style = clip.text;
  if (!style) return null;
  const animIn = style.animationIn;
  const animOut = style.animationOut;
  const inActive =
    animIn.type !== "none" && animIn.duration > 0 && localTime < animIn.duration;
  const timeRemaining = clip.duration - localTime;
  const outActive =
    animOut.type !== "none" &&
    animOut.duration > 0 &&
    timeRemaining <= animOut.duration;
  if (!inActive && !outActive) return null;

  const fx: TextEffectState = { ...NEUTRAL_TEXT_FX };
  if (inActive) {
    applyTextAnimationSide(
      fx,
      animIn.type,
      clamp(localTime / animIn.duration, 0, 1),
      "in",
    );
  }
  if (outActive) {
    applyTextAnimationSide(
      fx,
      animOut.type,
      clamp(1 - timeRemaining / animOut.duration, 0, 1),
      "out",
    );
  }
  fx.alpha = clamp(fx.alpha, 0, 1);
  fx.charProgress = clamp(fx.charProgress, 0, 1);
  return fx;
}

function applyEasing(value: number, easing: KeyframeEasing): number {
  const x = clamp(value, 0, 1);
  switch (easing) {
    case "hold":
      return 0;
    case "ease-in":
      return x * x;
    case "ease-out":
      return 1 - (1 - x) * (1 - x);
    case "ease-in-out":
      return x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2;
    default:
      return x;
  }
}

function finiteOr(value: number | undefined, fallback: number): number {
  return value !== undefined && Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
