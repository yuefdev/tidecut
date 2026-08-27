import type {
  CompositorViewport,
  FramePlan,
  TimelineClip,
  TimelineProjectSnapshot,
} from "$lib/editor/timeline-engine";
import {
  AUTO_REFRAME_ASPECTS,
  applyAutoReframeVariantToSnapshot,
  evaluateAutoReframeCameraAtMs,
  normalizeAutoReframePlan,
  type AutoReframeAspect,
  type AutoReframeCropWindow,
  type AutoReframePlan,
  type AutoReframePlannerOptions,
  type NormalizedTrackerSample,
} from "$lib/editor/auto-reframe";
import type { RenderClip, RenderKeyframe, RenderTimeline } from "$lib/render/types";
import type { AutoReframeAnalysis, NormalizedBoundingBox } from "./types";

export type AutoReframeStyle = "calm" | "natural" | "dynamic";
export type AutoReframeFraming = "auto" | "close" | "medium" | "full" | "object";

export type StreamerFacePosition = "top" | "bottom";

export interface StreamerLayoutSettings {
  enabled: boolean;
  facePosition: StreamerFacePosition;
  /** Fraction of the 9:16 canvas reserved around the face panel. */
  faceFraction: number;
  /** Static focal point used for the gameplay/screen panel. */
  contentFocusX: number;
  contentFocusY: number;
}

export const DEFAULT_STREAMER_LAYOUT: Readonly<StreamerLayoutSettings> = {
  enabled: false,
  facePosition: "top",
  faceFraction: 0.42,
  contentFocusX: 0.5,
  contentFocusY: 0.5,
};

const STREAMER_DIVIDER_RATIO = 0.006;
const STREAMER_OUTPUT_WIDTH = 1080;
const STREAMER_OUTPUT_HEIGHT = 1920;

/** One automatically detected facecam region on the clip-local timeline. */
export interface StreamerFaceTrackSample {
  /** Clip-local timeline milliseconds. */
  timeMs: number;
  /** Camera-region centre, normalized to source width. */
  centerX: number;
  /** Camera-region centre, normalized to source height. */
  centerY: number;
  /** Camera-region width as a source-width fraction. */
  width: number;
  /** Camera-region height as a source-height fraction. */
  height: number;
}

export interface ClipAutoReframeState {
  /** Invalidates the camera path after trim, duration, source, or speed edits. */
  clipSignature: string;
  aspects: AutoReframeAspect[];
  style: AutoReframeStyle;
  framing: AutoReframeFraming;
  selection: NormalizedBoundingBox;
  engine: string;
  generatedAtMs: number;
  warnings: string[];
  plan: AutoReframePlan;
  /** Optional 9:16 dual-panel delivery layout using this tracked target. */
  streamerLayout?: StreamerLayoutSettings;
  /**
   * Detected facecam boxes over time. When present, the streamer face panel
   * frames this camera rectangle directly instead of the 9:16 tracked crop.
   */
  faceTrack?: StreamerFaceTrackSample[];
}

export interface AutoReframeProjectState {
  version: 1;
  clips: Record<string, ClipAutoReframeState>;
}

export function createEmptyAutoReframeProjectState(): AutoReframeProjectState {
  return { version: 1, clips: {} };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isAspect(value: unknown): value is AutoReframeAspect {
  return typeof value === "string" && AUTO_REFRAME_ASPECTS.includes(value as AutoReframeAspect);
}

function normalizeSelection(value: unknown): NormalizedBoundingBox | null {
  if (!isRecord(value)) return null;
  const x = value.x;
  const y = value.y;
  const width = value.width;
  const height = value.height;
  if (
    typeof x !== "number" ||
    typeof y !== "number" ||
    typeof width !== "number" ||
    typeof height !== "number" ||
    ![x, y, width, height].every(Number.isFinite) ||
    x < 0 ||
    y < 0 ||
    width <= 0 ||
    height <= 0 ||
    x + width > 1.000001 ||
    y + height > 1.000001
  ) return null;
  return { x, y, width, height };
}

function normalizeStyle(value: unknown): AutoReframeStyle {
  return value === "calm" || value === "dynamic" ? value : "natural";
}

function normalizeFraming(value: unknown): AutoReframeFraming {
  return value === "close" ||
      value === "medium" ||
      value === "full" ||
      value === "object"
    ? value
    : "auto";
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

export function normalizeStreamerLayout(value: unknown): StreamerLayoutSettings | undefined {
  if (!isRecord(value)) return undefined;
  const finiteOr = (candidate: unknown, fallback: number) =>
    typeof candidate === "number" && Number.isFinite(candidate) ? candidate : fallback;
  return {
    enabled: value.enabled === true,
    facePosition: value.facePosition === "bottom" ? "bottom" : "top",
    faceFraction: clamp(
      finiteOr(value.faceFraction, DEFAULT_STREAMER_LAYOUT.faceFraction),
      0.3,
      0.6,
    ),
    contentFocusX: clamp(
      finiteOr(value.contentFocusX, DEFAULT_STREAMER_LAYOUT.contentFocusX),
      0,
      1,
    ),
    contentFocusY: clamp(
      finiteOr(value.contentFocusY, DEFAULT_STREAMER_LAYOUT.contentFocusY),
      0,
      1,
    ),
  };
}

const MAX_FACE_TRACK_SAMPLES = 100_000;

function normalizeFaceTrack(value: unknown): StreamerFaceTrackSample[] | undefined {
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_FACE_TRACK_SAMPLES) {
    return undefined;
  }
  const samples: StreamerFaceTrackSample[] = [];
  for (const raw of value) {
    if (!isRecord(raw)) return undefined;
    const timeMs = raw.timeMs;
    const centerX = raw.centerX;
    const centerY = raw.centerY;
    const width = raw.width;
    const height = raw.height;
    if (
      typeof timeMs !== "number" ||
      typeof centerX !== "number" ||
      typeof centerY !== "number" ||
      typeof width !== "number" ||
      typeof height !== "number" ||
      ![timeMs, centerX, centerY, width, height].every(Number.isFinite) ||
      timeMs < 0 ||
      width <= 0 ||
      width > 1 ||
      height <= 0 ||
      height > 1 ||
      centerX - width / 2 < -1e-6 ||
      centerX + width / 2 > 1 + 1e-6 ||
      centerY - height / 2 < -1e-6 ||
      centerY + height / 2 > 1 + 1e-6
    ) {
      return undefined;
    }
    const previous = samples.at(-1);
    if (previous && timeMs <= previous.timeMs) return undefined;
    samples.push({ timeMs, centerX, centerY, width, height });
  }
  return samples;
}

/** Treat project JSON as untrusted and drop only corrupt/stale-looking entries. */
export function normalizeAutoReframeProjectState(
  value: unknown,
): AutoReframeProjectState {
  const normalized = createEmptyAutoReframeProjectState();
  if (!isRecord(value) || value.version !== 1 || !isRecord(value.clips)) return normalized;
  for (const [clipId, rawEntry] of Object.entries(value.clips)) {
    if (!clipId || !isRecord(rawEntry) || typeof rawEntry.clipSignature !== "string") continue;
    const plan = normalizeAutoReframePlan(rawEntry.plan);
    const selection = normalizeSelection(rawEntry.selection);
    if (!plan || !selection || !Array.isArray(rawEntry.aspects)) continue;
    const aspects = [...new Set(rawEntry.aspects.filter(isAspect))];
    if (aspects.length === 0) continue;
    const streamerLayout = normalizeStreamerLayout(rawEntry.streamerLayout);
    const faceTrack = normalizeFaceTrack(rawEntry.faceTrack);
    normalized.clips[clipId] = {
      clipSignature: rawEntry.clipSignature,
      aspects,
      style: normalizeStyle(rawEntry.style),
      framing: normalizeFraming(rawEntry.framing),
      selection,
      engine: typeof rawEntry.engine === "string" ? rawEntry.engine : "opencv-csrt",
      generatedAtMs:
        typeof rawEntry.generatedAtMs === "number" && Number.isFinite(rawEntry.generatedAtMs)
          ? Math.max(0, rawEntry.generatedAtMs)
          : 0,
      warnings: Array.isArray(rawEntry.warnings)
        ? rawEntry.warnings.filter((warning): warning is string => typeof warning === "string")
        : [],
      plan,
      ...(streamerLayout ? { streamerLayout } : {}),
      ...(faceTrack ? { faceTrack: faceTrack.map((sample) => ({ ...sample })) } : {}),
    };
  }
  return normalized;
}

export function cloneAutoReframeProjectState(
  state: AutoReframeProjectState,
): AutoReframeProjectState {
  return normalizeAutoReframeProjectState(state);
}

export function autoReframeClipSignature(
  clip: Pick<
    TimelineClip,
    "file" | "trimIn" | "duration" | "speed" | "transform" | "keyframes"
  >,
): string {
  const number = (value: number) => (Number.isFinite(value) ? value.toFixed(6) : "invalid");
  return JSON.stringify([
    clip.file,
    number(clip.trimIn),
    number(clip.duration),
    number(clip.speed),
    number(clip.transform.rotation),
    (clip.keyframes.rotation ?? []).map((frame) => [
      number(frame.time),
      number(frame.value),
      frame.easing,
    ]),
  ]);
}

export function autoReframeRotationSupported(
  clip: Pick<TimelineClip, "transform" | "keyframes">,
): boolean {
  return Math.abs(clip.transform.rotation) < 1e-6 &&
    (clip.keyframes.rotation ?? []).every((frame) => Math.abs(frame.value) < 1e-6);
}

export function plannerOptionsForStyle(
  style: AutoReframeStyle,
): Partial<AutoReframePlannerOptions> {
  if (style === "calm") {
    return {
      smoothingWindowMs: 520,
      deadZoneRatioX: 0.09,
      deadZoneRatioY: 0.07,
      panTimeConstantMs: 360,
      zoomTimeConstantMs: 600,
      maxPanCropLengthsPerSecond: 0.8,
      maxZoomLogPerSecond: Math.log(1.35),
    };
  }
  if (style === "dynamic") {
    return {
      smoothingWindowMs: 140,
      deadZoneRatioX: 0.035,
      deadZoneRatioY: 0.03,
      panTimeConstantMs: 90,
      zoomTimeConstantMs: 180,
      maxPanCropLengthsPerSecond: 2.5,
      maxZoomLogPerSecond: Math.log(2.5),
    };
  }
  return {};
}

/** Converts backend top-left boxes/source time into planner centre boxes/timeline time. */
export function trackerAnalysisToPlannerSamples(
  analysis: Pick<AutoReframeAnalysis, "samples">,
  playbackRate: number,
  framing: AutoReframeFraming,
): NormalizedTrackerSample[] {
  const safeRate = Number.isFinite(playbackRate) && playbackRate > 0 ? playbackRate : 1;
  const framingFactor: Record<AutoReframeFraming, number> = {
    auto: 1,
    close: 0.65,
    medium: 0.86,
    full: 1.35,
    object: 0.78,
  };
  const factor = framingFactor[framing];
  return analysis.samples.map((sample) => {
    const centerX = sample.bbox.x + sample.bbox.width / 2;
    const centerY = sample.bbox.y + sample.bbox.height / 2;
    // Keep expanded framing boxes valid even when the tracked subject touches an edge.
    const width = Math.max(
      1e-6,
      Math.min(sample.bbox.width * factor, centerX * 2, (1 - centerX) * 2),
    );
    const height = Math.max(
      1e-6,
      Math.min(sample.bbox.height * factor, centerY * 2, (1 - centerY) * 2),
    );
    return {
      timeMs: sample.timeMs / safeRate,
      x: centerX,
      y: centerY,
      width,
      height,
      confidence: sample.confidence,
    };
  });
}

export function isAutoReframeEntryCurrent(
  clip: TimelineClip,
  entry: ClipAutoReframeState | null | undefined,
): entry is ClipAutoReframeState {
  return Boolean(entry && entry.clipSignature === autoReframeClipSignature(clip));
}

export function setClipAutoReframe(
  state: AutoReframeProjectState,
  clipId: string,
  entry: ClipAutoReframeState,
): AutoReframeProjectState {
  return normalizeAutoReframeProjectState({
    version: 1,
    clips: { ...state.clips, [clipId]: entry },
  });
}

/** Persists streamer layout settings without changing the tracked camera plan. */
export function setClipStreamerLayout(
  state: AutoReframeProjectState,
  clip: TimelineClip,
  settings: StreamerLayoutSettings,
): AutoReframeProjectState {
  const entry = state.clips[clip.id];
  if (!isAutoReframeEntryCurrent(clip, entry)) return state;
  const streamerLayout = normalizeStreamerLayout(settings);
  if (!streamerLayout) return state;
  return setClipAutoReframe(state, clip.id, {
    ...entry,
    aspects: streamerLayout.enabled
      ? [...new Set<AutoReframeAspect>([...entry.aspects, "9:16"])]
      : entry.aspects,
    streamerLayout,
  });
}

export function removeClipAutoReframe(
  state: AutoReframeProjectState,
  clipId: string,
): AutoReframeProjectState {
  const clips = { ...state.clips };
  delete clips[clipId];
  return { version: 1, clips };
}

export interface StreamerLayoutViewports {
  content: CompositorViewport;
  face: CompositorViewport;
}

export function streamerLayoutViewports(
  settings: StreamerLayoutSettings,
): StreamerLayoutViewports {
  const faceFraction = clamp(settings.faceFraction, 0.3, 0.6);
  const halfGap = STREAMER_DIVIDER_RATIO / 2;
  if (settings.facePosition === "bottom") {
    const contentHeight = 1 - faceFraction - halfGap;
    const faceY = 1 - faceFraction + halfGap;
    return {
      content: { x: 0, y: 0, width: 1, height: contentHeight },
      face: {
        x: 0,
        y: faceY,
        width: 1,
        height: 1 - faceY,
      },
    };
  }
  const contentY = faceFraction + halfGap;
  return {
    face: { x: 0, y: 0, width: 1, height: faceFraction - halfGap },
    content: {
      x: 0,
      y: contentY,
      width: 1,
      height: 1 - contentY,
    },
  };
}

function viewportNormalizedCropAspect(
  viewport: CompositorViewport,
  sourceWidth: number,
  sourceHeight: number,
): number {
  const viewportAspect =
    (viewport.width * STREAMER_OUTPUT_WIDTH) /
    (viewport.height * STREAMER_OUTPUT_HEIGHT);
  return viewportAspect * (sourceHeight / sourceWidth);
}

function constrainCropToSource(crop: AutoReframeCropWindow): AutoReframeCropWindow {
  const width = clamp(crop.width, 1e-6, 1);
  const height = clamp(crop.height, 1e-6, 1);
  return {
    centerX: clamp(crop.centerX, width / 2, 1 - width / 2),
    centerY: clamp(crop.centerY, height / 2, 1 - height / 2),
    width,
    height,
  };
}

function maximalCropAtFocus(
  normalizedAspect: number,
  focusX: number,
  focusY: number,
): AutoReframeCropWindow {
  const width = normalizedAspect <= 1 ? normalizedAspect : 1;
  const height = normalizedAspect <= 1 ? 1 : 1 / normalizedAspect;
  return constrainCropToSource({ centerX: focusX, centerY: focusY, width, height });
}

/** Re-aspects a tracked crop while keeping its full tracked region visible. */
function trackedCropForViewport(
  source: AutoReframeCropWindow,
  normalizedAspect: number,
): AutoReframeCropWindow {
  let width = Math.max(source.width, source.height * normalizedAspect);
  let height = width / normalizedAspect;
  if (height < source.height) {
    height = source.height;
    width = height * normalizedAspect;
  }
  const oversize = Math.max(width, height, 1);
  width /= oversize;
  height /= oversize;
  return constrainCropToSource({
    centerX: source.centerX,
    centerY: source.centerY,
    width,
    height,
  });
}

function round(value: number): number {
  return Math.round(value * 1_000_000) / 1_000_000;
}

function cropToViewportTransform(
  crop: AutoReframeCropWindow,
  sourceWidth: number,
  sourceHeight: number,
  viewport: CompositorViewport,
): { x: number; y: number; scale: number } {
  const outputWidth = viewport.width * STREAMER_OUTPUT_WIDTH;
  const outputHeight = viewport.height * STREAMER_OUTPUT_HEIGHT;
  const containScale = Math.min(outputWidth / sourceWidth, outputHeight / sourceHeight);
  const cropToViewportScale = outputWidth / (crop.width * sourceWidth);
  return {
    x: round(-(crop.centerX - 0.5) * sourceWidth * cropToViewportScale),
    y: round(-(crop.centerY - 0.5) * sourceHeight * cropToViewportScale),
    scale: round(cropToViewportScale / containScale),
  };
}

type StreamerPanelTransform = { x: number; y: number; scale: number };

function faceTransformForCrop(
  entry: ClipAutoReframeState,
  viewport: CompositorViewport,
  crop: AutoReframeCropWindow,
): StreamerPanelTransform {
  const trackedCrop = trackedCropForViewport(
    crop,
    viewportNormalizedCropAspect(
      viewport,
      entry.plan.sourceWidth,
      entry.plan.sourceHeight,
    ),
  );
  return cropToViewportTransform(
    trackedCrop,
    entry.plan.sourceWidth,
    entry.plan.sourceHeight,
    viewport,
  );
}

/**
 * Frames one detected camera rectangle at the face viewport's aspect with a
 * small breathing margin, so the panel shows the webcam overlay tightly
 * instead of the much looser 9:16 tracked crop.
 */
function faceTrackCropForViewport(
  sample: StreamerFaceTrackSample,
  viewport: CompositorViewport,
  sourceWidth: number,
  sourceHeight: number,
): AutoReframeCropWindow {
  const aspect = viewportNormalizedCropAspect(viewport, sourceWidth, sourceHeight);
  let width = Math.max(sample.width * 1.12, sample.height * 1.12 * aspect);
  let height = width / aspect;
  if (width > 1) {
    width = 1;
    height = width / aspect;
  }
  if (height > 1) {
    height = 1;
    width = height * aspect;
  }
  return constrainCropToSource({
    centerX: sample.centerX,
    centerY: sample.centerY,
    width,
    height,
  });
}

function faceTrackTransform(
  entry: ClipAutoReframeState,
  viewport: CompositorViewport,
  sample: StreamerFaceTrackSample,
): StreamerPanelTransform {
  const crop = faceTrackCropForViewport(
    sample,
    viewport,
    entry.plan.sourceWidth,
    entry.plan.sourceHeight,
  );
  return cropToViewportTransform(
    crop,
    entry.plan.sourceWidth,
    entry.plan.sourceHeight,
    viewport,
  );
}

function evaluateFaceTrackTransformAtMs(
  entry: ClipAutoReframeState,
  faceTrack: readonly StreamerFaceTrackSample[],
  viewport: CompositorViewport,
  localTimeMs: number,
): StreamerPanelTransform | null {
  if (faceTrack.length === 0) return null;
  const first = faceTrack[0];
  const last = faceTrack.at(-1)!;
  if (localTimeMs <= first.timeMs) return faceTrackTransform(entry, viewport, first);
  if (localTimeMs >= last.timeMs) return faceTrackTransform(entry, viewport, last);
  for (let index = 0; index < faceTrack.length - 1; index += 1) {
    const left = faceTrack[index];
    const right = faceTrack[index + 1];
    if (localTimeMs < left.timeMs || localTimeMs > right.timeMs) continue;
    const leftTransform = faceTrackTransform(entry, viewport, left);
    if (localTimeMs === left.timeMs) return leftTransform;
    const rightTransform = faceTrackTransform(entry, viewport, right);
    if (localTimeMs === right.timeMs) return rightTransform;
    const progress = (localTimeMs - left.timeMs) / (right.timeMs - left.timeMs);
    const interpolate = (start: number, end: number) => start + (end - start) * progress;
    return {
      x: interpolate(leftTransform.x, rightTransform.x),
      y: interpolate(leftTransform.y, rightTransform.y),
      scale: interpolate(leftTransform.scale, rightTransform.scale),
    };
  }
  return null;
}

/**
 * Export interpolates the derived viewport transforms, not the source crop
 * dimensions. Mirror that here because converting an interpolated crop to a
 * scale is nonlinear and would otherwise make preview zoom drift from export.
 */
function evaluateFaceTransformAtMs(
  entry: ClipAutoReframeState,
  viewport: CompositorViewport,
  localTimeMs: number,
): StreamerPanelTransform | null {
  if (entry.faceTrack && entry.faceTrack.length > 0) {
    return evaluateFaceTrackTransformAtMs(entry, entry.faceTrack, viewport, localTimeMs);
  }
  const frames = entry.plan.variants["9:16"].cameraKeyframes;
  if (frames.length === 0) return null;
  const first = frames[0];
  const last = frames.at(-1)!;
  if (localTimeMs <= first.timeMs) {
    return faceTransformForCrop(entry, viewport, first.crop);
  }
  if (localTimeMs >= last.timeMs) {
    return faceTransformForCrop(entry, viewport, last.crop);
  }
  for (let index = 0; index < frames.length - 1; index += 1) {
    const left = frames[index];
    const right = frames[index + 1];
    if (localTimeMs < left.timeMs || localTimeMs > right.timeMs) continue;
    const leftTransform = faceTransformForCrop(entry, viewport, left.crop);
    const rightTransform = faceTransformForCrop(entry, viewport, right.crop);
    const progress = (localTimeMs - left.timeMs) / (right.timeMs - left.timeMs);
    const interpolate = (start: number, end: number) => start + (end - start) * progress;
    return {
      x: interpolate(leftTransform.x, rightTransform.x),
      y: interpolate(leftTransform.y, rightTransform.y),
      scale: interpolate(leftTransform.scale, rightTransform.scale),
    };
  }
  return null;
}

function streamerTransforms(
  entry: ClipAutoReframeState,
  localTimeMs: number,
): {
  viewports: StreamerLayoutViewports;
  content: { x: number; y: number; scale: number };
  face: { x: number; y: number; scale: number };
} | null {
  const settings = entry.streamerLayout;
  if (!settings?.enabled) return null;
  const viewports = streamerLayoutViewports(settings);
  const face = evaluateFaceTransformAtMs(entry, viewports.face, localTimeMs);
  if (!face) return null;
  const { sourceWidth, sourceHeight } = entry.plan;
  const contentCrop = maximalCropAtFocus(
    viewportNormalizedCropAspect(viewports.content, sourceWidth, sourceHeight),
    settings.contentFocusX,
    settings.contentFocusY,
  );
  return {
    viewports,
    content: cropToViewportTransform(contentCrop, sourceWidth, sourceHeight, viewports.content),
    face,
  };
}

/** Applies the selected delivery camera without mutating the master timeline. */
export function applyAutoReframeToFramePlan(
  framePlan: FramePlan,
  snapshot: TimelineProjectSnapshot | null,
  state: AutoReframeProjectState,
  aspect: AutoReframeAspect,
): FramePlan {
  if (!snapshot || framePlan.layers.length === 0) return framePlan;
  const clips = new Map(snapshot.clips.map((clip) => [clip.id, clip]));
  let changed = false;
  const layers = framePlan.layers.flatMap((layer) => {
    const clip = clips.get(layer.clipId);
    const entry = state.clips[layer.clipId];
    if (!clip || !isAutoReframeEntryCurrent(clip, entry) || !entry.aspects.includes(aspect)) {
      return [layer];
    }
    if (aspect === "9:16" && clip.kind === "video" && entry.streamerLayout?.enabled) {
      const layout = streamerTransforms(entry, Math.max(0, layer.localTime * 1_000));
      if (!layout) return [layer];
      changed = true;
      return [
        {
          ...layer,
          viewport: layout.viewports.content,
          layoutRole: "streamer-content" as const,
          transform: { ...layer.transform, ...layout.content },
        },
        {
          ...layer,
          viewport: layout.viewports.face,
          layoutRole: "streamer-face" as const,
          transform: { ...layer.transform, ...layout.face },
        },
      ];
    }
    const camera = evaluateAutoReframeCameraAtMs(
      entry.plan.variants[aspect],
      Math.max(0, layer.localTime * 1_000),
    );
    if (!camera) return [layer];
    changed = true;
    return [{
      ...layer,
      transform: {
        ...layer.transform,
        x: camera.x,
        y: camera.y,
        scale: camera.scale,
      },
    }];
  });
  return changed ? { ...framePlan, layers } : framePlan;
}

/** Builds one ephemeral delivery timeline while keeping the project master intact. */
export function applyAutoReframeToSnapshot(
  snapshot: TimelineProjectSnapshot,
  state: AutoReframeProjectState,
  aspect: AutoReframeAspect,
): TimelineProjectSnapshot {
  let result = snapshot;
  for (const clip of snapshot.clips) {
    const entry = state.clips[clip.id];
    if (!isAutoReframeEntryCurrent(clip, entry) || !entry.aspects.includes(aspect)) continue;
    // Dual-panel delivery is represented after timeline adaptation because a
    // project snapshot cannot express two independently framed copies safely.
    if (aspect === "9:16" && entry.streamerLayout?.enabled) continue;
    result = applyAutoReframeVariantToSnapshot(result, clip.id, entry.plan.variants[aspect]);
  }
  return result;
}

function withoutCameraKeyframe(frame: RenderKeyframe): RenderKeyframe {
  const { x: _x, y: _y, scaleX: _scaleX, scaleY: _scaleY, ...rest } = frame;
  if (!frame.easing) return rest;
  const {
    x: _easeX,
    y: _easeY,
    scaleX: _easeScaleX,
    scaleY: _easeScaleY,
    ...easing
  } = frame.easing;
  return { ...rest, ...(Object.keys(easing).length ? { easing } : {}) };
}

function withoutAudioKeyframe(frame: RenderKeyframe): RenderKeyframe {
  const {
    volume: _volume,
    riderGain: _riderGain,
    duckGain: _duckGain,
    pan: _pan,
    ...rest
  } = frame;
  if (!frame.easing) return rest;
  const {
    volume: _easeVolume,
    riderGain: _easeRider,
    duckGain: _easeDuck,
    pan: _easePan,
    ...easing
  } = frame.easing;
  return { ...rest, ...(Object.keys(easing).length ? { easing } : {}) };
}

function hasAnimatedValues(frame: RenderKeyframe): boolean {
  return Object.keys(frame).some((key) => key !== "atMs" && key !== "easing");
}

function baseLayoutClip(
  clip: RenderClip,
  role: "content" | "face",
  viewport: CompositorViewport,
  transform: { x: number; y: number; scale: number },
): RenderClip {
  const visualFrames = (clip.keyframes ?? [])
    .map(withoutCameraKeyframe)
    .map((frame) => (role === "face" ? withoutAudioKeyframe(frame) : frame))
    .filter(hasAnimatedValues);
  return {
    ...clip,
    id: `${clip.id}::streamer-${role}`,
    viewport,
    transform: {
      ...clip.transform,
      x: transform.x,
      y: transform.y,
      scaleX: transform.scale,
      scaleY: transform.scale,
    },
    keyframes: visualFrames,
    ...(role === "face"
      ? {
          volume: 0,
          pan: 0,
          audioPath: undefined,
          audioTrimInMs: undefined,
          noiseReduction: null,
        }
      : {}),
  };
}

function addFaceCameraKeyframes(
  clip: RenderClip,
  entry: ClipAutoReframeState,
  viewport: CompositorViewport,
): RenderClip {
  const frames = new Map((clip.keyframes ?? []).map((frame) => [frame.atMs, frame]));
  // Preview and export must interpolate identically, so the exported keyframes
  // come from the same per-sample transforms evaluateFaceTransformAtMs uses.
  const timedTransforms: Array<{ atMs: number; transform: StreamerPanelTransform }> =
    entry.faceTrack && entry.faceTrack.length > 0
      ? entry.faceTrack.map((sample) => ({
          atMs: Math.max(0, Math.round(sample.timeMs)),
          transform: faceTrackTransform(entry, viewport, sample),
        }))
      : entry.plan.variants["9:16"].cameraKeyframes.map((camera) => ({
          atMs: Math.max(0, Math.round(camera.timeMs)),
          transform: faceTransformForCrop(entry, viewport, camera.crop),
        }));
  for (const { atMs, transform } of timedTransforms) {
    const current = frames.get(atMs) ?? { atMs };
    frames.set(atMs, {
      ...current,
      x: transform.x,
      y: transform.y,
      scaleX: transform.scale,
      scaleY: transform.scale,
      easing: {
        ...current.easing,
        x: "linear",
        y: "linear",
        scaleX: "linear",
        scaleY: "linear",
      },
    });
  }
  return { ...clip, keyframes: [...frames.values()].sort((a, b) => a.atMs - b.atMs) };
}

/**
 * Expands eligible 9:16 video clips into content + tracked-face render layers.
 * The face duplicate is always silent, so the source audio is mixed exactly once.
 */
export function applyStreamerLayoutToRenderTimeline(
  timeline: RenderTimeline,
  snapshot: TimelineProjectSnapshot,
  state: AutoReframeProjectState,
  aspect: AutoReframeAspect,
): RenderTimeline {
  if (aspect !== "9:16") return timeline;
  const clips = new Map(snapshot.clips.map((clip) => [clip.id, clip]));
  let changed = false;
  const renderClips = timeline.clips.flatMap((renderClip) => {
    const sourceClip = clips.get(renderClip.id);
    const entry = sourceClip ? state.clips[sourceClip.id] : undefined;
    if (
      !sourceClip ||
      sourceClip.kind !== "video" ||
      renderClip.kind !== "video" ||
      !isAutoReframeEntryCurrent(sourceClip, entry) ||
      !entry.aspects.includes("9:16") ||
      !entry.streamerLayout?.enabled
    ) return [renderClip];
    const initial = streamerTransforms(entry, 0);
    if (!initial) return [renderClip];
    changed = true;
    const content = baseLayoutClip(
      renderClip,
      "content",
      initial.viewports.content,
      initial.content,
    );
    const face = addFaceCameraKeyframes(
      baseLayoutClip(renderClip, "face", initial.viewports.face, initial.face),
      entry,
      initial.viewports.face,
    );
    return [content, face];
  });
  return changed ? { ...timeline, clips: renderClips } : timeline;
}

export function getAutoReframeExportAspects(
  snapshot: TimelineProjectSnapshot | null,
  state: AutoReframeProjectState,
): AutoReframeAspect[] {
  if (!snapshot) return [];
  const available = new Set<AutoReframeAspect>();
  for (const clip of snapshot.clips) {
    const entry = state.clips[clip.id];
    if (!isAutoReframeEntryCurrent(clip, entry)) continue;
    entry.aspects.forEach((aspect) => available.add(aspect));
  }
  return AUTO_REFRAME_ASPECTS.filter((aspect) => available.has(aspect));
}
