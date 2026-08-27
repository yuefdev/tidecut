import { describe, expect, it } from "vitest";
import {
  applyNormalizationGain,
  limitGainForVolumeAutomation,
} from "./audio-normalization";

describe("applyNormalizationGain", () => {
  it("scales base volume and every automation keyframe together", () => {
    const result = applyNormalizationGain(
      {
        volume: 0.5,
        keyframes: {
          volume: [
            { time: 0, value: 0.25, easing: "linear" },
            { time: 1, value: 0.75, easing: "ease-in-out" },
          ],
        },
      },
      2,
    );

    expect(result.volume).toBe(1);
    expect(result.keyframes.volume?.map((frame) => frame.value)).toEqual([
      0.5,
      1.5,
    ]);
    expect(result.appliedMultiplier).toBe(2);
    expect(result.limited).toBe(false);
  });

  it("caps the shared multiplier so no automated gain exceeds the engine limit", () => {
    const result = applyNormalizationGain(
      {
        volume: 1,
        keyframes: {
          volume: [{ time: 0, value: 2, easing: "linear" }],
        },
      },
      4,
    );

    expect(result.appliedMultiplier).toBe(2);
    expect(result.volume).toBe(2);
    expect(result.keyframes.volume?.[0].value).toBe(4);
    expect(result.limited).toBe(true);
  });

  it("reserves true-peak headroom for automation above the analyzed base gain", () => {
    const limited = limitGainForVolumeAutomation(
      {
        volume: 0.5,
        keyframes: {
          volume: [{ time: 1, value: 1, easing: "linear" }],
        },
      },
      2,
      -4,
      -1.5,
    );

    // The keyframe adds +6.02 dB over the analyzed base. With only 2.5 dB
    // true-peak headroom, the shared multiplier must therefore reduce gain.
    expect(20 * Math.log10(limited)).toBeCloseTo(-3.5206, 3);
  });
});
