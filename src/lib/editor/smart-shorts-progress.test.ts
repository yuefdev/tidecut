import { describe, expect, it } from "vitest";
import {
  SmartShortsProgressEstimator,
  baselineSmartShortsStageMs,
  createSmartShortsTimingProfile,
  parseSmartShortsTimingProfile,
} from "./smart-shorts-progress";

describe("Smart Shorts estimated progress", () => {
  it("moves smoothly inside a long stage without reaching its ceiling early", () => {
    const profile = createSmartShortsTimingProfile();
    const estimator = new SmartShortsProgressEstimator(60_000, false, profile, 0);
    const expectedMs = baselineSmartShortsStageMs("speech", 60_000);

    const midway = estimator.snapshot(expectedMs / 2);
    const expected = estimator.snapshot(expectedMs);
    const overdue = estimator.snapshot(expectedMs * 2);

    expect(midway.percent).toBeGreaterThan(3);
    expect(expected.percent).toBeGreaterThan(midway.percent);
    expect(expected.percent).toBeLessThan(28);
    expect(overdue.percent).toBeGreaterThan(expected.percent);
    expect(overdue.percent).toBeLessThan(28);
    expect(overdue.overdue).toBe(true);
  });

  it("never regresses when a backend progress signal is ahead of the estimate", () => {
    const estimator = new SmartShortsProgressEstimator(
      30_000,
      false,
      createSmartShortsTimingProfile(),
      0,
    );
    estimator.observe(22.5);
    expect(estimator.snapshot(500).percent).toBe(22.5);
    estimator.begin("analysis", 1_000);
    expect(estimator.snapshot(1_000).percent).toBe(28);
  });

  it("includes optional GLM ranking in the remaining-time estimate", () => {
    const profile = createSmartShortsTimingProfile();
    const local = new SmartShortsProgressEstimator(30_000, false, profile, 0);
    const withGlm = new SmartShortsProgressEstimator(30_000, true, profile, 0);

    expect(withGlm.snapshot(0).remainingMs).toBeGreaterThan(local.snapshot(0).remainingMs);
  });

  it("learns a bounded per-machine moving average after a completed stage", () => {
    const estimator = new SmartShortsProgressEstimator(
      30_000,
      false,
      createSmartShortsTimingProfile(),
      0,
    );
    estimator.begin("analysis", 40_000);
    const learned = estimator.timingProfile().ratios.speech;

    expect(learned).toBeTypeOf("number");
    expect(learned!).toBeGreaterThan(1);
    expect(learned!).toBeLessThanOrEqual(4);
  });

  it("fails closed to a clean profile for invalid persisted data", () => {
    expect(parseSmartShortsTimingProfile("not-json")).toEqual({ version: 1, ratios: {} });
    expect(
      parseSmartShortsTimingProfile(
        JSON.stringify({ version: 1, ratios: { speech: 99, analysis: -4 } }),
      ),
    ).toEqual({ version: 1, ratios: { speech: 4, analysis: 0.35 } });
  });
});
