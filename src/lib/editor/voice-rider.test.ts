import { describe, expect, it } from "vitest";
import { toVoiceRiderKeyframes } from "./voice-rider";

describe("toVoiceRiderKeyframes", () => {
  it("sorts, deduplicates and adds clip boundary points", () => {
    const frames = toVoiceRiderKeyframes(
      [
        { atMs: 900, gain: 0.5, speechProbability: 0.9 },
        { atMs: 200, gain: 1.4, speechProbability: 0.7 },
        { atMs: 200.4, gain: 1.2, speechProbability: 0.8 },
      ],
      1,
    );

    expect(frames).toEqual([
      { time: 0, value: 1.2, easing: "ease-in-out" },
      { time: 0.2, value: 1.2, easing: "ease-in-out" },
      { time: 0.9, value: 0.5, easing: "ease-in-out" },
      { time: 1, value: 0.5, easing: "ease-in-out" },
    ]);
  });

  it("drops invalid values and clamps unsafe gain", () => {
    const frames = toVoiceRiderKeyframes(
      [
        { atMs: 0, gain: Number.NaN, speechProbability: 0 },
        { atMs: 0, gain: 8, speechProbability: 1 },
        { atMs: 500, gain: 0.25, speechProbability: 1 },
      ],
      0.5,
    );
    expect(frames.map((frame) => frame.value)).toEqual([4, 0.25]);
  });

  it("enforces the safety cap after adding clip boundary points", () => {
    const points = Array.from({ length: 4_000 }, (_, index) => ({
      atMs: index + 1,
      gain: 1,
      speechProbability: 1,
    }));
    expect(() => toVoiceRiderKeyframes(points, 5)).toThrow(
      "güvenli keyframe sınırını aştı",
    );
  });
});
