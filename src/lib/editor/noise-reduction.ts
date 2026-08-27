export const NOISE_REDUCTION_ENGINE = "rnnoise" as const;
export const NOISE_REDUCTION_MODEL = "rnnoise-bd-v1" as const;

export type HumFrequency = 0 | 50 | 60;

/**
 * Static, clip-level speech cleanup settings. Technical values stay in the
 * render contract, but the product exposes one automatic speech-first mode.
 */
export interface NoiseReductionSettings {
  engine: typeof NOISE_REDUCTION_ENGINE;
  model: typeof NOISE_REDUCTION_MODEL;
  /** Internal RNNoise wet/dry mix; speech cleanup is always full-wet. */
  strength: number;
  /** Internal speech-safe low-cut chosen automatically. */
  highPassHz: number;
  /** Internal mains-hum notch; zero lets RNNoise and the low-cut handle it. */
  humFrequency: HumFrequency;
}

/**
 * Full-wet RNNoise avoids mixing removed noise back in. A 70 Hz low-cut
 * removes rumble and mains fundamentals while retaining a deep speaking voice.
 */
export const DEFAULT_NOISE_REDUCTION: NoiseReductionSettings = {
  engine: NOISE_REDUCTION_ENGINE,
  model: NOISE_REDUCTION_MODEL,
  strength: 1,
  highPassHz: 70,
  humFrequency: 0,
};

export function normalizeNoiseReduction(
  value: Partial<NoiseReductionSettings> | null | undefined,
): NoiseReductionSettings | null {
  if (!value) return null;
  // Old projects may contain manual strength/filter values. Enabling cleanup
  // now consistently migrates them to the single automatic mode.
  return { ...DEFAULT_NOISE_REDUCTION };
}

export function noiseReductionSignature(
  value: NoiseReductionSettings | null | undefined,
): string {
  const normalized = normalizeNoiseReduction(value);
  return normalized
    ? [
        normalized.engine,
        normalized.model,
        normalized.strength.toFixed(3),
        normalized.highPassHz,
        normalized.humFrequency,
      ].join(":")
    : "off";
}
