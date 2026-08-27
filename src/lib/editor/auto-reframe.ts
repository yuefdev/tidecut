import type { TimelineProjectSnapshot } from "./timeline-engine";

/**
 * Pure, deterministic camera planning for tracker output. Coordinates in
 * `NormalizedTrackerSample` are normalized source coordinates; x/y identify
 * the centre of the tracked bounding box (not its top-left corner).
 *
 * The generated x/y values use Astral's timeline convention: output-canvas
 * pixels measured from the centred layer position. Scale is the multiplier on
 * top of the compositor's source-to-output `contain` fit.
 */

export const AUTO_REFRAME_ASPECTS = ["9:16", "1:1", "16:9"] as const;

export type AutoReframeAspect = (typeof AUTO_REFRAME_ASPECTS)[number];

export interface NormalizedTrackerSample {
  /** Clip-local source time. */
  timeMs: number;
  /** Bounding-box centre, normalized to source width. */
  x: number;
  /** Bounding-box centre, normalized to source height. */
  y: number;
  /** Bounding-box width, normalized to source width. */
  width: number;
  /** Bounding-box height, normalized to source height. */
  height: number;
  /** Tracker certainty in the inclusive 0..1 range. */
  confidence: number;
}

export interface AutoReframeAspectConfig {
  outputWidth: number;
  outputHeight: number;
  /** Padding on each horizontal side, relative to subject width. */
  horizontalMargin: number;
  /** Padding on each vertical side, relative to subject height. */
  verticalMargin: number;
  /** Desired empty area above the subject, as a fraction of crop height. */
  headroom: number;
  /** Maximum zoom relative to the largest crop of this aspect. */
  maxAdditionalZoom: number;
}

/** Delivery sizes and conservative editorial framing defaults. */
export const AUTO_REFRAME_ASPECT_CONFIGS: Readonly<
  Record<AutoReframeAspect, Readonly<AutoReframeAspectConfig>>
> = {
  "9:16": {
    outputWidth: 1080,
    outputHeight: 1920,
    horizontalMargin: 0.18,
    verticalMargin: 0.14,
    headroom: 0.1,
    maxAdditionalZoom: 2.15,
  },
  "1:1": {
    outputWidth: 1080,
    outputHeight: 1080,
    horizontalMargin: 0.24,
    verticalMargin: 0.18,
    headroom: 0.12,
    maxAdditionalZoom: 2,
  },
  "16:9": {
    outputWidth: 1920,
    outputHeight: 1080,
    horizontalMargin: 0.35,
    verticalMargin: 0.25,
    headroom: 0.14,
    maxAdditionalZoom: 1.6,
  },
};

export interface AutoReframePlannerOptions {
  minConfidence: number;
  /** A low-confidence box may reuse the nearest reliable box for this long. */
  maxConfidenceHoldMs: number;
  /** Longer intervals contain no trustworthy motion evidence and fail closed. */
  maxSampleIntervalMs: number;
  /** Radius of the centred robust temporal filter. */
  smoothingWindowMs: number;
  deadZoneRatioX: number;
  deadZoneRatioY: number;
  panTimeConstantMs: number;
  zoomTimeConstantMs: number;
  /** Maximum camera travel in crop widths/heights per second. */
  maxPanCropLengthsPerSecond: number;
  /** Maximum natural-log scale change per second. */
  maxZoomLogPerSecond: number;
  /** Simplifier tolerance in delivery pixels. */
  keyframePositionTolerancePx: number;
  /** Simplifier tolerance as natural-log scale error. */
  keyframeZoomTolerance: number;
  /**
   * When true, a low-confidence stretch (or an over-long sample interval) holds
   * the nearest reliable box instead of failing the whole plan. This is correct
   * for a stationary subject such as a streamer facecam, and degrades gracefully
   * (with a "held" warning) for others instead of aborting a long analysis.
   */
  holdThroughLowConfidence: boolean;
}

export const DEFAULT_AUTO_REFRAME_OPTIONS: Readonly<AutoReframePlannerOptions> = {
  minConfidence: 0.55,
  maxConfidenceHoldMs: 900,
  maxSampleIntervalMs: 1_500,
  smoothingWindowMs: 280,
  deadZoneRatioX: 0.065,
  deadZoneRatioY: 0.05,
  panTimeConstantMs: 180,
  zoomTimeConstantMs: 320,
  maxPanCropLengthsPerSecond: 1.4,
  maxZoomLogPerSecond: Math.log(1.8),
  keyframePositionTolerancePx: 3,
  keyframeZoomTolerance: 0.006,
  holdThroughLowConfidence: false,
};

export interface AutoReframeRequest {
  sourceWidth: number;
  sourceHeight: number;
  samples: readonly NormalizedTrackerSample[];
  options?: Partial<AutoReframePlannerOptions>;
}

export interface AutoReframeCropWindow {
  centerX: number;
  centerY: number;
  width: number;
  height: number;
}

export type AutoReframeTrackingState = "tracked" | "held";

export interface AutoReframeCameraKeyframe {
  timeMs: number;
  crop: AutoReframeCropWindow;
  /** Timeline translation in output-canvas pixels. */
  x: number;
  /** Timeline translation in output-canvas pixels. */
  y: number;
  /** Timeline multiplier applied after the compositor's contain fit. */
  scale: number;
  confidence: number;
  trackingState: AutoReframeTrackingState;
}

export interface AutoReframeTimelineKeyframe {
  /** Clip-local timeline time in seconds. */
  time: number;
  value: number;
  easing: "linear";
}

export interface AutoReframeVariantPlan {
  aspect: AutoReframeAspect;
  outputWidth: number;
  outputHeight: number;
  cameraKeyframes: AutoReframeCameraKeyframe[];
  /** Ready to merge into TimelineClip.keyframes. */
  timelineKeyframes: {
    x: AutoReframeTimelineKeyframe[];
    y: AutoReframeTimelineKeyframe[];
    scale: AutoReframeTimelineKeyframe[];
  };
  denseKeyframeCount: number;
  clippedTargetFrameCount: number;
  warnings: string[];
}

export interface RejectedTrackerSample {
  index: number;
  reasons: string[];
}

export interface AutoReframePlan {
  version: 1;
  sourceWidth: number;
  sourceHeight: number;
  startTimeMs: number;
  endTimeMs: number;
  acceptedSampleCount: number;
  reliableSampleCount: number;
  heldSampleCount: number;
  variants: Record<AutoReframeAspect, AutoReframeVariantPlan>;
  warnings: string[];
}

export type AutoReframeErrorCode =
  | "INVALID_REQUEST"
  | "INVALID_SOURCE"
  | "INVALID_OPTIONS"
  | "NO_SAMPLES"
  | "NO_VALID_SAMPLES"
  | "NO_RELIABLE_SAMPLES"
  | "UNSUPPORTED_SOURCE_ASPECT"
  | "TRACKING_GAP";

export interface AutoReframeError {
  code: AutoReframeErrorCode;
  message: string;
  details?: string[];
}

export type AutoReframeResult =
  | {
      ok: true;
      plan: AutoReframePlan;
      rejectedSamples: RejectedTrackerSample[];
    }
  | {
      ok: false;
      error: AutoReframeError;
      rejectedSamples: RejectedTrackerSample[];
    };

export type AutoReframeRequestValidation =
  | {
      ok: true;
      sourceWidth: number;
      sourceHeight: number;
      samples: NormalizedTrackerSample[];
      options: AutoReframePlannerOptions;
      rejectedSamples: RejectedTrackerSample[];
    }
  | {
      ok: false;
      error: AutoReframeError;
      rejectedSamples: RejectedTrackerSample[];
    };

interface IndexedTrackerSample {
  index: number;
  sample: NormalizedTrackerSample;
}

interface ResolvedTrackerSample extends NormalizedTrackerSample {
  trackingState: AutoReframeTrackingState;
}

interface DenseCameraPoint extends AutoReframeCameraKeyframe {
  targetClipped: boolean;
}

const BOX_EPSILON = 1e-9;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function finiteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function round(value: number, digits = 6): number {
  const multiplier = 10 ** digits;
  return Math.round(value * multiplier) / multiplier;
}

/** Returns every runtime validation issue instead of silently clamping boxes. */
export function getTrackerSampleValidationIssues(sample: unknown): string[] {
  if (!isRecord(sample)) return ["sample must be an object"];
  const issues: string[] = [];
  const numericFields = ["timeMs", "x", "y", "width", "height", "confidence"] as const;
  for (const field of numericFields) {
    if (!finiteNumber(sample[field])) issues.push(`${field} must be finite`);
  }
  if (issues.length > 0) return issues;

  const typed = sample as unknown as NormalizedTrackerSample;
  if (typed.timeMs < 0) issues.push("timeMs must be at least 0");
  if (typed.x < 0 || typed.x > 1) issues.push("x must be in 0..1");
  if (typed.y < 0 || typed.y > 1) issues.push("y must be in 0..1");
  if (typed.width <= 0 || typed.width > 1) issues.push("width must be in (0, 1]");
  if (typed.height <= 0 || typed.height > 1) issues.push("height must be in (0, 1]");
  if (typed.confidence < 0 || typed.confidence > 1) {
    issues.push("confidence must be in 0..1");
  }
  if (
    typed.width > 0 &&
    (typed.x - typed.width / 2 < -BOX_EPSILON ||
      typed.x + typed.width / 2 > 1 + BOX_EPSILON)
  ) {
    issues.push("horizontal bounding box must stay inside the source");
  }
  if (
    typed.height > 0 &&
    (typed.y - typed.height / 2 < -BOX_EPSILON ||
      typed.y + typed.height / 2 > 1 + BOX_EPSILON)
  ) {
    issues.push("vertical bounding box must stay inside the source");
  }
  return issues;
}

export function isNormalizedTrackerSample(
  sample: unknown,
): sample is NormalizedTrackerSample {
  return getTrackerSampleValidationIssues(sample).length === 0;
}

function validateOptions(
  input: unknown,
): { ok: true; options: AutoReframePlannerOptions } | { ok: false; details: string[] } {
  if (input !== undefined && !isRecord(input)) {
    return { ok: false, details: ["options must be an object"] };
  }
  const overrides = (input ?? {}) as Partial<AutoReframePlannerOptions>;
  const options: AutoReframePlannerOptions = {
    ...DEFAULT_AUTO_REFRAME_OPTIONS,
    ...overrides,
  };
  // Untrusted callers must not smuggle a non-boolean flag past the numeric checks.
  options.holdThroughLowConfidence = options.holdThroughLowConfidence === true;
  const details: string[] = [];
  const bounded = (
    key: keyof AutoReframePlannerOptions,
    minimum: number,
    maximum: number,
    minimumInclusive = true,
  ) => {
    const value = options[key];
    if (
      !finiteNumber(value) ||
      (minimumInclusive ? value < minimum : value <= minimum) ||
      value > maximum
    ) {
      details.push(
        `${key} must be ${minimumInclusive ? "in" : "greater than"} ${minimumInclusive ? `[${minimum}, ${maximum}]` : `${minimum} and at most ${maximum}`}`,
      );
    }
  };
  bounded("minConfidence", 0, 1);
  bounded("maxConfidenceHoldMs", 0, 60_000);
  bounded("maxSampleIntervalMs", 1, 60_000);
  bounded("smoothingWindowMs", 0, 10_000);
  bounded("deadZoneRatioX", 0, 0.5);
  bounded("deadZoneRatioY", 0, 0.5);
  bounded("panTimeConstantMs", 0, 10_000);
  bounded("zoomTimeConstantMs", 0, 10_000);
  bounded("maxPanCropLengthsPerSecond", 0, 100, false);
  bounded("maxZoomLogPerSecond", 0, 100, false);
  bounded("keyframePositionTolerancePx", 0, 1_000, false);
  bounded("keyframeZoomTolerance", 0, 10, false);
  return details.length > 0 ? { ok: false, details } : { ok: true, options };
}

/**
 * Validates untrusted tracker output, removes malformed samples, sorts by time,
 * and resolves duplicate timestamps by retaining the highest confidence box.
 */
export function validateAutoReframeRequest(
  request: unknown,
): AutoReframeRequestValidation {
  const rejectedSamples: RejectedTrackerSample[] = [];
  if (!isRecord(request)) {
    return {
      ok: false,
      error: { code: "INVALID_REQUEST", message: "Auto-reframe request must be an object." },
      rejectedSamples,
    };
  }
  const sourceWidth = request.sourceWidth;
  const sourceHeight = request.sourceHeight;
  if (
    !finiteNumber(sourceWidth) ||
    !finiteNumber(sourceHeight) ||
    !Number.isInteger(sourceWidth) ||
    !Number.isInteger(sourceHeight) ||
    sourceWidth <= 0 ||
    sourceHeight <= 0
  ) {
    return {
      ok: false,
      error: {
        code: "INVALID_SOURCE",
        message: "Source dimensions must be positive integer pixels.",
      },
      rejectedSamples,
    };
  }

  const optionResult = validateOptions(request.options);
  if (!optionResult.ok) {
    return {
      ok: false,
      error: {
        code: "INVALID_OPTIONS",
        message: "Auto-reframe options are outside their safe ranges.",
        details: optionResult.details,
      },
      rejectedSamples,
    };
  }
  if (!Array.isArray(request.samples)) {
    return {
      ok: false,
      error: { code: "NO_SAMPLES", message: "Tracker samples are required." },
      rejectedSamples,
    };
  }
  if (request.samples.length === 0) {
    return {
      ok: false,
      error: { code: "NO_SAMPLES", message: "Tracker returned no samples." },
      rejectedSamples,
    };
  }

  const candidates: IndexedTrackerSample[] = [];
  request.samples.forEach((sample, index) => {
    const reasons = getTrackerSampleValidationIssues(sample);
    if (reasons.length > 0) {
      rejectedSamples.push({ index, reasons });
      return;
    }
    candidates.push({ index, sample: { ...(sample as NormalizedTrackerSample) } });
  });
  candidates.sort(
    (left, right) =>
      left.sample.timeMs - right.sample.timeMs ||
      right.sample.confidence - left.sample.confidence ||
      left.index - right.index,
  );

  const deduplicated: IndexedTrackerSample[] = [];
  for (const candidate of candidates) {
    const previous = deduplicated.at(-1);
    if (previous?.sample.timeMs === candidate.sample.timeMs) {
      rejectedSamples.push({
        index: candidate.index,
        reasons: ["duplicate timeMs; the highest-confidence sample was retained"],
      });
      continue;
    }
    deduplicated.push(candidate);
  }
  rejectedSamples.sort((left, right) => left.index - right.index);
  if (deduplicated.length === 0) {
    return {
      ok: false,
      error: {
        code: "NO_VALID_SAMPLES",
        message: "Tracker returned no valid in-bounds samples.",
      },
      rejectedSamples,
    };
  }
  return {
    ok: true,
    sourceWidth,
    sourceHeight,
    samples: deduplicated.map(({ sample }) => sample),
    options: optionResult.options,
    rejectedSamples,
  };
}

function fail(
  code: AutoReframeErrorCode,
  message: string,
  rejectedSamples: RejectedTrackerSample[],
  details?: string[],
): AutoReframeResult {
  return { ok: false, error: { code, message, details }, rejectedSamples };
}

function resolveConfidence(
  samples: readonly NormalizedTrackerSample[],
  options: AutoReframePlannerOptions,
):
  | { ok: true; samples: ResolvedTrackerSample[]; reliableCount: number; heldCount: number }
  | { ok: false; error: AutoReframeError } {
  const reliable = samples.map((sample) => sample.confidence >= options.minConfidence);
  const reliableCount = reliable.filter(Boolean).length;
  if (reliableCount === 0) {
    return {
      ok: false,
      error: {
        code: "NO_RELIABLE_SAMPLES",
        message: `No tracker sample reached confidence ${options.minConfidence}.`,
      },
    };
  }

  const previousReliable = new Array<number>(samples.length).fill(-1);
  const nextReliable = new Array<number>(samples.length).fill(-1);
  let latest = -1;
  for (let index = 0; index < samples.length; index += 1) {
    if (reliable[index]) latest = index;
    previousReliable[index] = latest;
  }
  latest = -1;
  for (let index = samples.length - 1; index >= 0; index -= 1) {
    if (reliable[index]) latest = index;
    nextReliable[index] = latest;
  }

  const unresolvedTimes: number[] = [];
  const output: ResolvedTrackerSample[] = samples.map((sample, index) => {
    if (reliable[index]) return { ...sample, trackingState: "tracked" };
    const before = previousReliable[index];
    const after = nextReliable[index];
    const beforeDistance =
      before >= 0 ? Math.abs(sample.timeMs - samples[before].timeMs) : Number.POSITIVE_INFINITY;
    const afterDistance =
      after >= 0 ? Math.abs(samples[after].timeMs - sample.timeMs) : Number.POSITIVE_INFINITY;
    const nearest = beforeDistance <= afterDistance ? before : after;
    const distance = Math.min(beforeDistance, afterDistance);
    if (
      nearest < 0 ||
      (!options.holdThroughLowConfidence && distance > options.maxConfidenceHoldMs)
    ) {
      unresolvedTimes.push(sample.timeMs);
      return { ...sample, trackingState: "held" };
    }
    const held = samples[nearest];
    return {
      timeMs: sample.timeMs,
      x: held.x,
      y: held.y,
      width: held.width,
      height: held.height,
      confidence: sample.confidence,
      trackingState: "held",
    };
  });
  if (unresolvedTimes.length > 0) {
    return {
      ok: false,
      error: {
        code: "TRACKING_GAP",
        message: "Tracker confidence stayed low beyond the safe hold window.",
        details: unresolvedTimes.map((timeMs) => `unresolved sample at ${round(timeMs, 3)} ms`),
      },
    };
  }

  for (let index = 1; index < output.length; index += 1) {
    const interval = output[index].timeMs - output[index - 1].timeMs;
    if (!options.holdThroughLowConfidence && interval > options.maxSampleIntervalMs) {
      return {
        ok: false,
        error: {
          code: "TRACKING_GAP",
          message: "Tracker samples contain an interval too large to interpolate safely.",
          details: [
            `${round(output[index - 1].timeMs, 3)}..${round(output[index].timeMs, 3)} ms (${round(interval, 3)} ms)`,
          ],
        },
      };
    }
  }
  return {
    ok: true,
    samples: output,
    reliableCount,
    heldCount: output.length - reliableCount,
  };
}

function median(values: readonly number[]): number {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

function robustTemporalValue(
  neighbours: readonly ResolvedTrackerSample[],
  centre: ResolvedTrackerSample,
  field: "x" | "y" | "width" | "height",
  windowMs: number,
): number {
  if (windowMs <= 0 || neighbours.length <= 1) return centre[field];
  const medianValue = median(neighbours.map((sample) => sample[field]));
  const mad = median(neighbours.map((sample) => Math.abs(sample[field] - medianValue)));
  const minimumTolerance = field === "x" || field === "y" ? 0.006 : 0.01;
  const tolerance = Math.max(minimumTolerance, mad * 4.4478);
  const inliers = neighbours.filter(
    (sample) => Math.abs(sample[field] - medianValue) <= tolerance,
  );
  let weightedTotal = 0;
  let weightTotal = 0;
  for (const sample of inliers) {
    const distance = Math.abs(sample.timeMs - centre.timeMs);
    const temporalWeight = Math.exp(-distance / Math.max(1, windowMs));
    const confidenceWeight =
      sample.trackingState === "tracked" ? Math.max(0.2, sample.confidence) : 0.15;
    const weight = temporalWeight * confidenceWeight;
    weightedTotal += sample[field] * weight;
    weightTotal += weight;
  }
  return weightTotal > 0 ? weightedTotal / weightTotal : medianValue;
}

function smoothTrackerSamples(
  samples: readonly ResolvedTrackerSample[],
  windowMs: number,
): ResolvedTrackerSample[] {
  let windowStart = 0;
  let windowEnd = 0;
  return samples.map((sample) => {
    while (
      windowStart < samples.length &&
      samples[windowStart].timeMs < sample.timeMs - windowMs
    ) {
      windowStart += 1;
    }
    windowEnd = Math.max(windowEnd, windowStart);
    while (
      windowEnd < samples.length &&
      samples[windowEnd].timeMs <= sample.timeMs + windowMs
    ) {
      windowEnd += 1;
    }
    const neighbours = samples.slice(windowStart, windowEnd);
    const width = clamp(
      robustTemporalValue(neighbours, sample, "width", windowMs),
      1e-6,
      1,
    );
    const height = clamp(
      robustTemporalValue(neighbours, sample, "height", windowMs),
      1e-6,
      1,
    );
    return {
      ...sample,
      width,
      height,
      x: clamp(
        robustTemporalValue(neighbours, sample, "x", windowMs),
        width / 2,
        1 - width / 2,
      ),
      y: clamp(
        robustTemporalValue(neighbours, sample, "y", windowMs),
        height / 2,
        1 - height / 2,
      ),
    };
  });
}

function aspectGeometry(
  sourceWidth: number,
  sourceHeight: number,
  config: AutoReframeAspectConfig,
) {
  const outputAspect = config.outputWidth / config.outputHeight;
  const normalizedAspect = outputAspect * (sourceHeight / sourceWidth);
  const maxHeight = Math.min(1, 1 / normalizedAspect);
  const containScale = Math.min(
    config.outputWidth / sourceWidth,
    config.outputHeight / sourceHeight,
  );
  const coverScale = Math.max(
    config.outputWidth / sourceWidth,
    config.outputHeight / sourceHeight,
  );
  const coverTimelineScale = coverScale / containScale;
  return {
    normalizedAspect,
    maxHeight,
    maxWidth: maxHeight * normalizedAspect,
    coverTimelineScale,
    maxAdditionalZoom: Math.max(
      1,
      Math.min(config.maxAdditionalZoom, 20 / coverTimelineScale),
    ),
  };
}

function desiredCrop(
  sample: ResolvedTrackerSample,
  sourceWidth: number,
  sourceHeight: number,
  config: AutoReframeAspectConfig,
): AutoReframeCropWindow & { targetClipped: boolean } {
  const geometry = aspectGeometry(sourceWidth, sourceHeight, config);
  const requiredWidth = sample.width * (1 + config.horizontalMargin * 2);
  const requiredHeight = sample.height * (1 + config.verticalMargin * 2);
  const minimumHeight = geometry.maxHeight / geometry.maxAdditionalZoom;
  const unconstrainedHeight = Math.max(
    minimumHeight,
    requiredHeight,
    requiredWidth / geometry.normalizedAspect,
  );
  const height = Math.min(geometry.maxHeight, unconstrainedHeight);
  const width = height * geometry.normalizedAspect;
  const subjectTop = sample.y - sample.height / 2;
  const preferredY = subjectTop + height * (0.5 - config.headroom);
  const centerX = clamp(sample.x, width / 2, 1 - width / 2);
  const centerY = clamp(preferredY, height / 2, 1 - height / 2);
  return {
    centerX,
    centerY,
    width,
    height,
    targetClipped:
      requiredWidth > geometry.maxWidth + BOX_EPSILON ||
      requiredHeight > geometry.maxHeight + BOX_EPSILON,
  };
}

function lowPassAlpha(deltaMs: number, timeConstantMs: number): number {
  if (timeConstantMs <= 0) return 1;
  return 1 - Math.exp(-Math.max(0, deltaMs) / timeConstantMs);
}

function moveOutsideDeadZone(current: number, desired: number, radius: number): number {
  const delta = desired - current;
  if (Math.abs(delta) <= radius) return current;
  return desired - Math.sign(delta) * radius;
}

function clampDelta(value: number, maximumMagnitude: number): number {
  return clamp(value, -maximumMagnitude, maximumMagnitude);
}

function keepSubjectVisible(
  proposedCenter: number,
  subjectCenter: number,
  subjectSize: number,
  cropSize: number,
): number {
  const sourceMinimum = cropSize / 2;
  const sourceMaximum = 1 - cropSize / 2;
  const visibilityMinimum = subjectCenter + subjectSize / 2 - cropSize / 2;
  const visibilityMaximum = subjectCenter - subjectSize / 2 + cropSize / 2;
  const minimum = Math.max(sourceMinimum, visibilityMinimum);
  const maximum = Math.min(sourceMaximum, visibilityMaximum);
  // An unusually large subject cannot fit this aspect. Centre it as closely as
  // source bounds allow; the variant warning will explicitly expose the loss.
  if (minimum > maximum) return clamp(subjectCenter, sourceMinimum, sourceMaximum);
  return clamp(proposedCenter, minimum, maximum);
}

function toTimelineTransform(
  crop: AutoReframeCropWindow,
  sourceWidth: number,
  sourceHeight: number,
  outputWidth: number,
  outputHeight: number,
): { x: number; y: number; scale: number } {
  const containScale = Math.min(outputWidth / sourceWidth, outputHeight / sourceHeight);
  const cropToCanvasScale = outputWidth / (crop.width * sourceWidth);
  const timelineScale = cropToCanvasScale / containScale;
  return {
    x: round(-(crop.centerX - 0.5) * sourceWidth * cropToCanvasScale),
    y: round(-(crop.centerY - 0.5) * sourceHeight * cropToCanvasScale),
    scale: round(timelineScale),
  };
}

function buildDenseCameraPoints(
  samples: readonly ResolvedTrackerSample[],
  sourceWidth: number,
  sourceHeight: number,
  config: AutoReframeAspectConfig,
  options: AutoReframePlannerOptions,
): DenseCameraPoint[] {
  const geometry = aspectGeometry(sourceWidth, sourceHeight, config);
  const desired = samples.map((sample) =>
    desiredCrop(sample, sourceWidth, sourceHeight, config),
  );
  const smoothedCrops: AutoReframeCropWindow[] = [];

  desired.forEach((target, index) => {
    if (index === 0) {
      smoothedCrops.push({
        centerX: target.centerX,
        centerY: target.centerY,
        width: target.width,
        height: target.height,
      });
      return;
    }
    const previous = smoothedCrops[index - 1];
    const deltaMs = samples[index].timeMs - samples[index - 1].timeMs;
    const deltaSeconds = deltaMs / 1_000;

    const zoomAlpha = lowPassAlpha(deltaMs, options.zoomTimeConstantMs);
    const desiredLogHeight = Math.log(target.height);
    const previousLogHeight = Math.log(previous.height);
    const zoomDelta = clampDelta(
      (desiredLogHeight - previousLogHeight) * zoomAlpha,
      options.maxZoomLogPerSecond * deltaSeconds,
    );
    const minimumHeight = geometry.maxHeight / geometry.maxAdditionalZoom;
    const height = clamp(
      Math.exp(previousLogHeight + zoomDelta),
      minimumHeight,
      geometry.maxHeight,
    );
    const width = height * geometry.normalizedAspect;

    const deadZoneX = width * options.deadZoneRatioX;
    const deadZoneY = height * options.deadZoneRatioY;
    const outsideX = moveOutsideDeadZone(previous.centerX, target.centerX, deadZoneX);
    const outsideY = moveOutsideDeadZone(previous.centerY, target.centerY, deadZoneY);
    const panAlpha = lowPassAlpha(deltaMs, options.panTimeConstantMs);
    const maximumX = options.maxPanCropLengthsPerSecond * width * deltaSeconds;
    const maximumY = options.maxPanCropLengthsPerSecond * height * deltaSeconds;
    const proposedCenterX = clamp(
      previous.centerX + clampDelta((outsideX - previous.centerX) * panAlpha, maximumX),
      width / 2,
      1 - width / 2,
    );
    const proposedCenterY = clamp(
      previous.centerY + clampDelta((outsideY - previous.centerY) * panAlpha, maximumY),
      height / 2,
      1 - height / 2,
    );
    const centerX = keepSubjectVisible(
      proposedCenterX,
      samples[index].x,
      samples[index].width,
      width,
    );
    const centerY = keepSubjectVisible(
      proposedCenterY,
      samples[index].y,
      samples[index].height,
      height,
    );
    smoothedCrops.push({ centerX, centerY, width, height });
  });

  return smoothedCrops.map((crop, index) => {
    const transform = toTimelineTransform(
      crop,
      sourceWidth,
      sourceHeight,
      config.outputWidth,
      config.outputHeight,
    );
    return {
      timeMs: samples[index].timeMs,
      crop: {
        centerX: round(crop.centerX),
        centerY: round(crop.centerY),
        width: round(crop.width),
        height: round(crop.height),
      },
      ...transform,
      confidence: round(samples[index].confidence),
      trackingState: samples[index].trackingState,
      targetClipped:
        desired[index].targetClipped ||
        crop.width + BOX_EPSILON <
          samples[index].width * (1 + config.horizontalMargin * 2) ||
        crop.height + BOX_EPSILON <
          samples[index].height * (1 + config.verticalMargin * 2),
    };
  });
}

function interpolationError(
  point: DenseCameraPoint,
  left: DenseCameraPoint,
  right: DenseCameraPoint,
  options: AutoReframePlannerOptions,
): number {
  const duration = right.timeMs - left.timeMs;
  const progress = duration > 0 ? (point.timeMs - left.timeMs) / duration : 0;
  const expectedX = left.x + (right.x - left.x) * progress;
  const expectedY = left.y + (right.y - left.y) * progress;
  const expectedScale = left.scale + (right.scale - left.scale) * progress;
  return Math.max(
    Math.abs(point.x - expectedX) / options.keyframePositionTolerancePx,
    Math.abs(point.y - expectedY) / options.keyframePositionTolerancePx,
    Math.abs(Math.log(point.scale / expectedScale)) / options.keyframeZoomTolerance,
  );
}

function simplifyCameraPoints(
  points: readonly DenseCameraPoint[],
  options: AutoReframePlannerOptions,
): AutoReframeCameraKeyframe[] {
  if (points.length <= 1) return points.map(({ targetClipped: _targetClipped, ...point }) => point);
  const keep = new Set<number>([0, points.length - 1]);
  const ranges: Array<[number, number]> = [[0, points.length - 1]];
  while (ranges.length > 0) {
    const [start, end] = ranges.pop()!;
    let largestError = 1;
    let largestIndex = -1;
    for (let index = start + 1; index < end; index += 1) {
      const error = interpolationError(points[index], points[start], points[end], options);
      if (error > largestError) {
        largestError = error;
        largestIndex = index;
      }
    }
    if (largestIndex >= 0) {
      keep.add(largestIndex);
      ranges.push([start, largestIndex], [largestIndex, end]);
    }
  }
  const simplified = [...keep]
    .sort((left, right) => left - right)
    .map((index) => {
      const { targetClipped: _targetClipped, ...point } = points[index];
      return point;
    });
  const first = simplified[0];
  const effectivelyStatic = simplified.every(
    (point) =>
      Math.abs(point.x - first.x) <= options.keyframePositionTolerancePx &&
      Math.abs(point.y - first.y) <= options.keyframePositionTolerancePx &&
      Math.abs(Math.log(point.scale / first.scale)) <= options.keyframeZoomTolerance,
  );
  return effectivelyStatic ? [first] : simplified;
}

function timelineProperty(
  keyframes: readonly AutoReframeCameraKeyframe[],
  property: "x" | "y" | "scale",
): AutoReframeTimelineKeyframe[] {
  return keyframes.map((keyframe) => ({
    time: round(keyframe.timeMs / 1_000),
    value: keyframe[property],
    easing: "linear",
  }));
}

function buildVariant(
  aspect: AutoReframeAspect,
  samples: readonly ResolvedTrackerSample[],
  sourceWidth: number,
  sourceHeight: number,
  options: AutoReframePlannerOptions,
): AutoReframeVariantPlan {
  const config = AUTO_REFRAME_ASPECT_CONFIGS[aspect];
  const dense = buildDenseCameraPoints(
    samples,
    sourceWidth,
    sourceHeight,
    config,
    options,
  );
  const cameraKeyframes = simplifyCameraPoints(dense, options);
  const clippedTargetFrameCount = dense.filter((point) => point.targetClipped).length;
  const warnings: string[] = [];
  if (samples.some((sample) => sample.trackingState === "held")) {
    warnings.push("Low-confidence tracker samples were held on the nearest reliable box.");
  }
  if (clippedTargetFrameCount > 0) {
    warnings.push(
      `${clippedTargetFrameCount} frame(s) could not preserve every requested subject margin within source bounds.`,
    );
  }
  return {
    aspect,
    outputWidth: config.outputWidth,
    outputHeight: config.outputHeight,
    cameraKeyframes,
    timelineKeyframes: {
      x: timelineProperty(cameraKeyframes, "x"),
      y: timelineProperty(cameraKeyframes, "y"),
      scale: timelineProperty(cameraKeyframes, "scale"),
    },
    denseKeyframeCount: dense.length,
    clippedTargetFrameCount,
    warnings,
  };
}

/**
 * Builds independent 9:16, 1:1 and 16:9 camera paths from one tracker pass.
 * The function never falls back to a centre crop when tracking is unusable.
 */
export function planAutoReframe(request: AutoReframeRequest): AutoReframeResult {
  const validation = validateAutoReframeRequest(request);
  if (!validation.ok) {
    return {
      ok: false,
      error: validation.error,
      rejectedSamples: validation.rejectedSamples,
    };
  }
  const unsupportedAspect = AUTO_REFRAME_ASPECTS.find(
    (aspect) =>
      aspectGeometry(
        validation.sourceWidth,
        validation.sourceHeight,
        AUTO_REFRAME_ASPECT_CONFIGS[aspect],
      ).coverTimelineScale > 20,
  );
  if (unsupportedAspect) {
    return fail(
      "UNSUPPORTED_SOURCE_ASPECT",
      `Source aspect cannot fill ${unsupportedAspect} within the renderer's safe scale limit.`,
      validation.rejectedSamples,
    );
  }
  const confidence = resolveConfidence(validation.samples, validation.options);
  if (!confidence.ok) {
    return fail(
      confidence.error.code,
      confidence.error.message,
      validation.rejectedSamples,
      confidence.error.details,
    );
  }
  const smoothedSamples = smoothTrackerSamples(
    confidence.samples,
    validation.options.smoothingWindowMs,
  );
  const variants = Object.fromEntries(
    AUTO_REFRAME_ASPECTS.map((aspect) => [
      aspect,
      buildVariant(
        aspect,
        smoothedSamples,
        validation.sourceWidth,
        validation.sourceHeight,
        validation.options,
      ),
    ]),
  ) as Record<AutoReframeAspect, AutoReframeVariantPlan>;
  const warnings = [...new Set(AUTO_REFRAME_ASPECTS.flatMap((aspect) => variants[aspect].warnings))];
  return {
    ok: true,
    rejectedSamples: validation.rejectedSamples,
    plan: {
      version: 1,
      sourceWidth: validation.sourceWidth,
      sourceHeight: validation.sourceHeight,
      startTimeMs: smoothedSamples[0].timeMs,
      endTimeMs: smoothedSamples.at(-1)!.timeMs,
      acceptedSampleCount: smoothedSamples.length,
      reliableSampleCount: confidence.reliableCount,
      heldSampleCount: confidence.heldCount,
      variants,
      warnings,
    },
  };
}

function copyCameraKeyframe(
  keyframe: AutoReframeCameraKeyframe,
): AutoReframeCameraKeyframe {
  return { ...keyframe, crop: { ...keyframe.crop } };
}

function planIssueCollector() {
  const issues: string[] = [];
  return {
    issues,
    add(issue: string) {
      // Persisted project data is untrusted. Keep diagnostics useful without
      // allowing a corrupt, enormous plan to flood the UI.
      if (issues.length < 100) issues.push(issue);
    },
  };
}

/** Strict persisted-plan validation used by project hydration. */
export function getAutoReframePlanValidationIssues(value: unknown): string[] {
  const collector = planIssueCollector();
  const { add } = collector;
  if (!isRecord(value)) return ["plan must be an object"];
  if (value.version !== 1) add("version must be 1");
  const sourceWidth = value.sourceWidth;
  const sourceHeight = value.sourceHeight;
  if (!finiteNumber(sourceWidth) || !Number.isInteger(sourceWidth) || sourceWidth <= 0) {
    add("sourceWidth must be a positive integer");
  }
  if (!finiteNumber(sourceHeight) || !Number.isInteger(sourceHeight) || sourceHeight <= 0) {
    add("sourceHeight must be a positive integer");
  }
  for (const field of [
    "startTimeMs",
    "endTimeMs",
    "acceptedSampleCount",
    "reliableSampleCount",
    "heldSampleCount",
  ] as const) {
    if (!finiteNumber(value[field]) || value[field] < 0) add(`${field} must be non-negative`);
  }
  if (
    finiteNumber(value.startTimeMs) &&
    finiteNumber(value.endTimeMs) &&
    value.endTimeMs < value.startTimeMs
  ) {
    add("endTimeMs must not precede startTimeMs");
  }
  for (const field of ["acceptedSampleCount", "reliableSampleCount", "heldSampleCount"] as const) {
    if (finiteNumber(value[field]) && !Number.isInteger(value[field])) {
      add(`${field} must be an integer`);
    }
  }
  if (
    finiteNumber(value.acceptedSampleCount) &&
    finiteNumber(value.reliableSampleCount) &&
    finiteNumber(value.heldSampleCount) &&
    value.reliableSampleCount + value.heldSampleCount !== value.acceptedSampleCount
  ) {
    add("reliableSampleCount plus heldSampleCount must equal acceptedSampleCount");
  }
  if (!Array.isArray(value.warnings) || value.warnings.some((warning) => typeof warning !== "string")) {
    add("warnings must be a string array");
  }
  if (!isRecord(value.variants)) {
    add("variants must be an object");
    return collector.issues;
  }

  for (const aspect of AUTO_REFRAME_ASPECTS) {
    const variant = value.variants[aspect];
    const path = `variants.${aspect}`;
    if (!isRecord(variant)) {
      add(`${path} must be an object`);
      continue;
    }
    const config = AUTO_REFRAME_ASPECT_CONFIGS[aspect];
    if (variant.aspect !== aspect) add(`${path}.aspect must be ${aspect}`);
    if (variant.outputWidth !== config.outputWidth || variant.outputHeight !== config.outputHeight) {
      add(`${path} output dimensions do not match the aspect preset`);
    }
    if (
      !finiteNumber(variant.denseKeyframeCount) ||
      !Number.isInteger(variant.denseKeyframeCount) ||
      variant.denseKeyframeCount < 1
    ) {
      add(`${path}.denseKeyframeCount must be a positive integer`);
    }
    if (
      !finiteNumber(variant.clippedTargetFrameCount) ||
      !Number.isInteger(variant.clippedTargetFrameCount) ||
      variant.clippedTargetFrameCount < 0
    ) {
      add(`${path}.clippedTargetFrameCount must be a non-negative integer`);
    }
    if (
      finiteNumber(variant.denseKeyframeCount) &&
      finiteNumber(variant.clippedTargetFrameCount) &&
      variant.clippedTargetFrameCount > variant.denseKeyframeCount
    ) {
      add(`${path}.clippedTargetFrameCount exceeds denseKeyframeCount`);
    }
    if (
      !Array.isArray(variant.warnings) ||
      variant.warnings.some((warning) => typeof warning !== "string")
    ) {
      add(`${path}.warnings must be a string array`);
    }
    if (!Array.isArray(variant.cameraKeyframes) || variant.cameraKeyframes.length === 0) {
      add(`${path}.cameraKeyframes must be a non-empty array`);
      continue;
    }
    const cameraKeyframes = variant.cameraKeyframes;
    if (cameraKeyframes.length > 100_000) {
      add(`${path}.cameraKeyframes exceeds the safety limit`);
      continue;
    }
    if (
      finiteNumber(variant.denseKeyframeCount) &&
      cameraKeyframes.length > variant.denseKeyframeCount
    ) {
      add(`${path}.cameraKeyframes cannot exceed denseKeyframeCount`);
    }
    let previousTime = -1;
    cameraKeyframes.forEach((rawKeyframe, index) => {
      const keyPath = `${path}.cameraKeyframes[${index}]`;
      if (!isRecord(rawKeyframe)) {
        add(`${keyPath} must be an object`);
        return;
      }
      for (const field of ["timeMs", "x", "y", "scale", "confidence"] as const) {
        if (!finiteNumber(rawKeyframe[field])) add(`${keyPath}.${field} must be finite`);
      }
      if (finiteNumber(rawKeyframe.timeMs)) {
        if (rawKeyframe.timeMs < 0) add(`${keyPath}.timeMs must be non-negative`);
        if (rawKeyframe.timeMs <= previousTime) add(`${keyPath}.timeMs must be strictly increasing`);
        if (
          finiteNumber(value.startTimeMs) &&
          finiteNumber(value.endTimeMs) &&
          (rawKeyframe.timeMs < value.startTimeMs || rawKeyframe.timeMs > value.endTimeMs)
        ) {
          add(`${keyPath}.timeMs must stay inside the plan time range`);
        }
        previousTime = rawKeyframe.timeMs;
      }
      if (finiteNumber(rawKeyframe.scale) && rawKeyframe.scale <= 0) {
        add(`${keyPath}.scale must be positive`);
      }
      if (finiteNumber(rawKeyframe.scale) && rawKeyframe.scale > 20) {
        add(`${keyPath}.scale exceeds the renderer safety limit`);
      }
      if (
        finiteNumber(rawKeyframe.confidence) &&
        (rawKeyframe.confidence < 0 || rawKeyframe.confidence > 1)
      ) {
        add(`${keyPath}.confidence must be in 0..1`);
      }
      if (rawKeyframe.trackingState !== "tracked" && rawKeyframe.trackingState !== "held") {
        add(`${keyPath}.trackingState is invalid`);
      }
      if (!isRecord(rawKeyframe.crop)) {
        add(`${keyPath}.crop must be an object`);
        return;
      }
      const crop = rawKeyframe.crop;
      for (const field of ["centerX", "centerY", "width", "height"] as const) {
        if (!finiteNumber(crop[field])) add(`${keyPath}.crop.${field} must be finite`);
      }
      if (
        finiteNumber(crop.width) &&
        finiteNumber(crop.height) &&
        finiteNumber(crop.centerX) &&
        finiteNumber(crop.centerY)
      ) {
        if (crop.width <= 0 || crop.height <= 0 || crop.width > 1 || crop.height > 1) {
          add(`${keyPath}.crop dimensions must be in (0, 1]`);
        }
        if (
          crop.centerX - crop.width / 2 < -1e-6 ||
          crop.centerX + crop.width / 2 > 1 + 1e-6 ||
          crop.centerY - crop.height / 2 < -1e-6 ||
          crop.centerY + crop.height / 2 > 1 + 1e-6
        ) {
          add(`${keyPath}.crop must stay inside source bounds`);
        }
        if (
          finiteNumber(sourceWidth) &&
          finiteNumber(sourceHeight) &&
          sourceWidth > 0 &&
          sourceHeight > 0
        ) {
          const expectedRatio =
            (config.outputWidth / config.outputHeight) * (sourceHeight / sourceWidth);
          if (Math.abs(crop.width / crop.height - expectedRatio) > 1e-4) {
            add(`${keyPath}.crop does not match the variant aspect`);
          }
          if (
            finiteNumber(rawKeyframe.x) &&
            finiteNumber(rawKeyframe.y) &&
            finiteNumber(rawKeyframe.scale)
          ) {
            const expectedTransform = toTimelineTransform(
              crop as unknown as AutoReframeCropWindow,
              sourceWidth,
              sourceHeight,
              config.outputWidth,
              config.outputHeight,
            );
            if (
              Math.abs(rawKeyframe.x - expectedTransform.x) > 0.02 ||
              Math.abs(rawKeyframe.y - expectedTransform.y) > 0.02 ||
              Math.abs(rawKeyframe.scale - expectedTransform.scale) > 1e-5
            ) {
              add(`${keyPath} timeline transform does not match its crop`);
            }
          }
        }
      }
    });

    if (!isRecord(variant.timelineKeyframes)) {
      add(`${path}.timelineKeyframes must be an object`);
      continue;
    }
    if (
      finiteNumber(value.acceptedSampleCount) &&
      finiteNumber(variant.denseKeyframeCount) &&
      variant.denseKeyframeCount !== value.acceptedSampleCount
    ) {
      add(`${path}.denseKeyframeCount must equal acceptedSampleCount`);
    }
    for (const property of ["x", "y", "scale"] as const) {
      const frames = variant.timelineKeyframes[property];
      if (!Array.isArray(frames) || frames.length !== cameraKeyframes.length) {
        add(`${path}.timelineKeyframes.${property} must mirror cameraKeyframes`);
        continue;
      }
      frames.forEach((rawFrame, index) => {
        if (!isRecord(rawFrame)) {
          add(`${path}.timelineKeyframes.${property}[${index}] must be an object`);
          return;
        }
        const camera = cameraKeyframes[index];
        if (
          !isRecord(camera) ||
          !finiteNumber(rawFrame.time) ||
          !finiteNumber(rawFrame.value) ||
          rawFrame.easing !== "linear"
        ) {
          add(`${path}.timelineKeyframes.${property}[${index}] is invalid`);
          return;
        }
        if (
          finiteNumber(camera.timeMs) &&
          finiteNumber(camera[property]) &&
          (Math.abs(rawFrame.time - camera.timeMs / 1_000) > 1e-6 ||
            Math.abs(rawFrame.value - camera[property]) > 1e-6)
        ) {
          add(`${path}.timelineKeyframes.${property}[${index}] does not match its camera keyframe`);
        }
      });
    }
  }
  return collector.issues;
}

export function isAutoReframePlan(value: unknown): value is AutoReframePlan {
  return getAutoReframePlanValidationIssues(value).length === 0;
}

/** Returns an isolated, canonical copy or null for unsafe persisted data. */
export function normalizeAutoReframePlan(value: unknown): AutoReframePlan | null {
  if (!isAutoReframePlan(value)) return null;
  const variants = Object.fromEntries(
    AUTO_REFRAME_ASPECTS.map((aspect) => {
      const source = value.variants[aspect];
      const cameraKeyframes = source.cameraKeyframes.map(copyCameraKeyframe);
      return [
        aspect,
        {
          ...source,
          cameraKeyframes,
          timelineKeyframes: {
            x: timelineProperty(cameraKeyframes, "x"),
            y: timelineProperty(cameraKeyframes, "y"),
            scale: timelineProperty(cameraKeyframes, "scale"),
          },
          warnings: [...source.warnings],
        },
      ];
    }),
  ) as Record<AutoReframeAspect, AutoReframeVariantPlan>;
  return {
    ...value,
    variants,
    warnings: [...value.warnings],
  };
}

/** Evaluates the exact linear camera state used by Timeline keyframes. */
export function evaluateAutoReframeCameraAtMs(
  variant: AutoReframeVariantPlan,
  timeMs: number,
): AutoReframeCameraKeyframe | null {
  if (!finiteNumber(timeMs) || variant.cameraKeyframes.length === 0) return null;
  const frames = variant.cameraKeyframes;
  const first = frames[0];
  const last = frames.at(-1)!;
  if (timeMs <= first.timeMs) return copyCameraKeyframe(first);
  if (timeMs >= last.timeMs) return copyCameraKeyframe(last);
  for (let index = 0; index < frames.length - 1; index += 1) {
    const left = frames[index];
    const right = frames[index + 1];
    if (timeMs < left.timeMs || timeMs > right.timeMs) continue;
    if (timeMs === left.timeMs) return copyCameraKeyframe(left);
    if (timeMs === right.timeMs) return copyCameraKeyframe(right);
    const progress = (timeMs - left.timeMs) / (right.timeMs - left.timeMs);
    const interpolate = (start: number, end: number) => start + (end - start) * progress;
    return {
      timeMs,
      crop: {
        centerX: interpolate(left.crop.centerX, right.crop.centerX),
        centerY: interpolate(left.crop.centerY, right.crop.centerY),
        width: interpolate(left.crop.width, right.crop.width),
        height: interpolate(left.crop.height, right.crop.height),
      },
      x: interpolate(left.x, right.x),
      y: interpolate(left.y, right.y),
      scale: interpolate(left.scale, right.scale),
      confidence: interpolate(left.confidence, right.confidence),
      trackingState:
        left.trackingState === "held" || right.trackingState === "held" ? "held" : "tracked",
    };
  }
  return null;
}

/**
 * Applies one generated camera path immutably. All non-camera automation (for
 * example opacity, rotation, volume, riderGain and pan) remains byte-for-byte
 * represented by the original keyframe arrays.
 */
export function applyAutoReframeVariantToSnapshot(
  snapshot: TimelineProjectSnapshot,
  clipId: string,
  variant: AutoReframeVariantPlan,
): TimelineProjectSnapshot {
  if (!snapshot.clips.some((clip) => clip.id === clipId)) {
    throw new Error(`Cannot apply auto-reframe: clip '${clipId}' was not found.`);
  }
  const x = timelineProperty(variant.cameraKeyframes, "x");
  const y = timelineProperty(variant.cameraKeyframes, "y");
  const scale = timelineProperty(variant.cameraKeyframes, "scale");
  return {
    ...snapshot,
    clips: snapshot.clips.map((clip) =>
      clip.id === clipId
        ? {
            ...clip,
            keyframes: { ...clip.keyframes, x, y, scale },
          }
        : clip,
    ),
  };
}
