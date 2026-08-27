import { describe, expect, it } from "vitest";
import {
  DEFAULT_NOISE_REDUCTION,
  noiseReductionSignature,
  normalizeNoiseReduction,
} from "./noise-reduction";

describe("automatic noise reduction settings", () => {
  it("keeps disabled clips disabled and exposes one speech-first mode", () => {
    expect(normalizeNoiseReduction(null)).toBeNull();
    expect(DEFAULT_NOISE_REDUCTION).toEqual({
      engine: "rnnoise",
      model: "rnnoise-bd-v1",
      strength: 1,
      highPassHz: 70,
      humFrequency: 0,
    });
  });

  it("migrates old or corrupt technical controls to automatic mode", () => {
    expect(
      normalizeNoiseReduction({
        strength: 99,
        highPassHz: 200,
        humFrequency: 60,
      }),
    ).toEqual(DEFAULT_NOISE_REDUCTION);
  });

  it("uses one deterministic cache signature for every enabled legacy value", () => {
    const migrated = normalizeNoiseReduction({
      ...DEFAULT_NOISE_REDUCTION,
      strength: 0.1,
    })!;
    expect(migrated).toEqual(DEFAULT_NOISE_REDUCTION);
    expect(noiseReductionSignature(migrated)).toContain("1.000:70:0");
    expect(noiseReductionSignature(null)).toBe("off");
  });
});
