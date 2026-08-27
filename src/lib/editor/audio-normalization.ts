import type { ClipKeyframes } from "./timeline-engine";

export interface NormalizableClipGain {
  volume: number;
  keyframes: ClipKeyframes;
}

export interface AppliedNormalizationGain {
  volume: number;
  keyframes: ClipKeyframes;
  appliedMultiplier: number;
  limited: boolean;
}

/**
 * Reserves true-peak headroom for volume automation that rises above the
 * clip's base gain. The FFmpeg analysis measures the base gain, while this
 * guard keeps louder keyframes from reintroducing clipping afterwards.
 */
export function limitGainForVolumeAutomation(
  clip: NormalizableClipGain,
  recommendedMultiplier: number,
  measuredTruePeakDb: number,
  targetTruePeakDb: number,
): number {
  const baseGain = Math.max(0, clip.volume);
  const loudestGain = Math.max(
    baseGain,
    ...(clip.keyframes.volume ?? []).map((frame) => Math.max(0, frame.value)),
  );
  if (
    baseGain <= 0 ||
    loudestGain <= baseGain ||
    !Number.isFinite(measuredTruePeakDb) ||
    !Number.isFinite(targetTruePeakDb)
  ) {
    return recommendedMultiplier;
  }

  const automationBoostDb = 20 * Math.log10(loudestGain / baseGain);
  const safeGainDb = targetTruePeakDb - measuredTruePeakDb - automationBoostDb;
  const safeMultiplier = 10 ** (safeGainDb / 20);
  return Math.max(0, Math.min(recommendedMultiplier, safeMultiplier));
}

/**
 * Applies an analysis-suggested linear multiplier without changing the shape
 * of volume automation. A shared cap keeps preview and FFmpeg render semantics
 * aligned and prevents a quiet measurement from creating unsafe gain.
 */
export function applyNormalizationGain(
  clip: NormalizableClipGain,
  recommendedMultiplier: number,
  maximumGain = 4,
): AppliedNormalizationGain {
  const volumeFrames = clip.keyframes.volume ?? [];
  const largestGain = Math.max(
    Math.max(0, clip.volume),
    ...volumeFrames.map((frame) => Math.max(0, frame.value)),
  );
  const safeRecommendation =
    Number.isFinite(recommendedMultiplier) && recommendedMultiplier >= 0
      ? recommendedMultiplier
      : 1;
  const maximumMultiplier = largestGain > 0 ? maximumGain / largestGain : 1;
  const appliedMultiplier = Math.max(
    0,
    Math.min(safeRecommendation, maximumMultiplier),
  );
  const keyframes = volumeFrames.length
    ? {
        ...clip.keyframes,
        volume: volumeFrames.map((frame) => ({
          ...frame,
          value: clamp(frame.value * appliedMultiplier, 0, maximumGain),
        })),
      }
    : clip.keyframes;

  return {
    volume: clamp(clip.volume * appliedMultiplier, 0, maximumGain),
    keyframes,
    appliedMultiplier,
    limited: appliedMultiplier + 1e-6 < safeRecommendation,
  };
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
