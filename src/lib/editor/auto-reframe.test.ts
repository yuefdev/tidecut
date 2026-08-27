import { describe, expect, it } from "vitest";
import {
  AUTO_REFRAME_ASPECTS,
  applyAutoReframeVariantToSnapshot,
  evaluateAutoReframeCameraAtMs,
  normalizeAutoReframePlan,
  planAutoReframe,
  type NormalizedTrackerSample,
} from "./auto-reframe";
import type { TimelineProjectSnapshot } from "./timeline-engine";

function sample(
  timeMs: number,
  x: number,
  overrides: Partial<NormalizedTrackerSample> = {},
): NormalizedTrackerSample {
  return {
    timeMs,
    x,
    y: 0.5,
    width: 0.16,
    height: 0.58,
    confidence: 0.94,
    ...overrides,
  };
}

function successful(samples: readonly NormalizedTrackerSample[]) {
  const result = planAutoReframe({ sourceWidth: 1920, sourceHeight: 1080, samples });
  expect(result.ok).toBe(true);
  if (!result.ok) throw new Error(result.error.message);
  return result;
}

describe("auto-reframe camera planner", () => {
  it("absorbs stationary tracker jitter and emits a sparse stable path", () => {
    const result = successful([
      sample(0, 0.5),
      sample(100, 0.504, { y: 0.498 }),
      sample(200, 0.497, { y: 0.503 }),
      sample(300, 0.502, { y: 0.499 }),
      sample(400, 0.499, { y: 0.501 }),
    ]);

    for (const aspect of AUTO_REFRAME_ASPECTS) {
      const variant = result.plan.variants[aspect];
      expect(variant.cameraKeyframes.length).toBeLessThanOrEqual(2);
      expect(variant.denseKeyframeCount).toBe(5);
      const xs = variant.cameraKeyframes.map((keyframe) => keyframe.x);
      expect(Math.max(...xs) - Math.min(...xs)).toBeLessThan(3.1);
    }
  });

  it("follows a moving target with bounded, directionally correct camera motion", () => {
    const result = successful([
      sample(0, 0.25),
      sample(300, 0.34),
      sample(600, 0.45),
      sample(900, 0.57),
      sample(1_200, 0.68),
      sample(1_500, 0.76),
    ]);
    const vertical = result.plan.variants["9:16"];
    expect(vertical.cameraKeyframes.length).toBeGreaterThan(1);
    expect(vertical.cameraKeyframes.at(-1)!.x).toBeLessThan(
      vertical.cameraKeyframes[0].x,
    );
    for (let index = 1; index < vertical.cameraKeyframes.length; index += 1) {
      expect(vertical.cameraKeyframes[index].timeMs).toBeGreaterThan(
        vertical.cameraKeyframes[index - 1].timeMs,
      );
      expect(Number.isFinite(vertical.cameraKeyframes[index].scale)).toBe(true);
    }
  });

  it("keeps every crop inside source bounds for a subject near the edge", () => {
    const result = successful([
      sample(0, 0.91, { width: 0.14 }),
      sample(250, 0.92, { width: 0.14 }),
      sample(500, 0.93, { width: 0.12 }),
    ]);

    for (const aspect of AUTO_REFRAME_ASPECTS) {
      for (const keyframe of result.plan.variants[aspect].cameraKeyframes) {
        expect(keyframe.crop.centerX - keyframe.crop.width / 2).toBeGreaterThanOrEqual(
          -1e-6,
        );
        expect(keyframe.crop.centerX + keyframe.crop.width / 2).toBeLessThanOrEqual(
          1 + 1e-6,
        );
        expect(keyframe.crop.centerY - keyframe.crop.height / 2).toBeGreaterThanOrEqual(
          -1e-6,
        );
        expect(keyframe.crop.centerY + keyframe.crop.height / 2).toBeLessThanOrEqual(
          1 + 1e-6,
        );
      }
    }
  });

  it("produces genuinely different crop geometry and contain-based scales per aspect", () => {
    const result = successful([sample(0, 0.5)]);
    const vertical = result.plan.variants["9:16"].cameraKeyframes[0];
    const square = result.plan.variants["1:1"].cameraKeyframes[0];
    const wide = result.plan.variants["16:9"].cameraKeyframes[0];

    expect(vertical.crop.width / vertical.crop.height).toBeCloseTo(0.31640625, 5);
    expect(square.crop.width / square.crop.height).toBeCloseTo(0.5625, 5);
    expect(wide.crop.width / wide.crop.height).toBeCloseTo(1, 5);
    expect(vertical.scale).toBeGreaterThan(square.scale);
    expect(square.scale).toBeGreaterThan(wide.scale);
    expect(vertical.x).toBeCloseTo(0, 5);
  });

  it("holds a brief confidence dip without following its untrusted box", () => {
    const result = successful([
      sample(0, 0.32),
      sample(300, 0.78, { confidence: 0.2 }),
      sample(600, 0.36),
    ]);

    expect(result.plan.heldSampleCount).toBe(1);
    expect(result.plan.warnings.join(" ")).toContain("Low-confidence");
    expect(result.plan.variants["9:16"].denseKeyframeCount).toBe(3);
    const horizontalTravel = result.plan.variants["9:16"].cameraKeyframes.map(
      (keyframe) => Math.abs(keyframe.x),
    );
    expect(Math.max(...horizontalTravel)).toBeLessThan(900);
  });

  it("rejects malformed samples while retaining valid tracker evidence", () => {
    const result = planAutoReframe({
      sourceWidth: 1920,
      sourceHeight: 1080,
      samples: [
        sample(0, 0.5),
        sample(100, Number.NaN),
        sample(200, 0.99, { width: 0.2 }),
        sample(300, 0.52),
      ],
    });

    expect(result.ok).toBe(true);
    expect(result.rejectedSamples).toHaveLength(2);
    if (result.ok) expect(result.plan.acceptedSampleCount).toBe(2);
  });

  it("fails closed when there are no samples or no reliable samples", () => {
    const empty = planAutoReframe({ sourceWidth: 1920, sourceHeight: 1080, samples: [] });
    expect(empty.ok).toBe(false);
    if (!empty.ok) expect(empty.error.code).toBe("NO_SAMPLES");

    const unreliable = planAutoReframe({
      sourceWidth: 1920,
      sourceHeight: 1080,
      samples: [sample(0, 0.5, { confidence: 0.2 }), sample(200, 0.5, { confidence: 0.4 })],
    });
    expect(unreliable.ok).toBe(false);
    if (!unreliable.ok) expect(unreliable.error.code).toBe("NO_RELIABLE_SAMPLES");
  });

  it("fails closed when low confidence outlives the configured hold window", () => {
    const result = planAutoReframe({
      sourceWidth: 1920,
      sourceHeight: 1080,
      samples: [sample(0, 0.4), sample(1_200, 0.75, { confidence: 0.1 })],
      options: { maxConfidenceHoldMs: 500 },
    });

    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error.code).toBe("TRACKING_GAP");
  });

  it("holds through a low-confidence gap instead of failing when leniency is on", () => {
    const samples = [
      sample(0, 0.3),
      sample(1_000, 0.7, { confidence: 0.1 }),
      sample(2_000, 0.32),
    ];
    const strict = planAutoReframe({
      sourceWidth: 1920,
      sourceHeight: 1080,
      samples,
      options: { maxConfidenceHoldMs: 500 },
    });
    expect(strict.ok).toBe(false);
    if (!strict.ok) expect(strict.error.code).toBe("TRACKING_GAP");

    const lenient = planAutoReframe({
      sourceWidth: 1920,
      sourceHeight: 1080,
      samples,
      options: { maxConfidenceHoldMs: 500, holdThroughLowConfidence: true },
    });
    expect(lenient.ok).toBe(true);
    if (lenient.ok) {
      expect(lenient.plan.reliableSampleCount).toBe(2);
      expect(lenient.plan.heldSampleCount).toBe(1);
      // The held frame adopts a reliable neighbour's box rather than aborting.
      expect(lenient.plan.variants["9:16"].warnings.join(" ")).toContain("held");
    }
  });

  it("normalizes persisted plans and evaluates their camera path at milliseconds", () => {
    const result = successful([
      sample(0, 0.3),
      sample(300, 0.42),
      sample(600, 0.58),
      sample(900, 0.7),
    ]);
    const normalized = normalizeAutoReframePlan(JSON.parse(JSON.stringify(result.plan)));
    expect(normalized).not.toBeNull();
    const variant = normalized!.variants["9:16"];
    const left = variant.cameraKeyframes[0];
    const right = variant.cameraKeyframes.at(-1)!;
    const middleTime = (left.timeMs + right.timeMs) / 2;
    const evaluated = evaluateAutoReframeCameraAtMs(variant, middleTime);

    expect(evaluated).not.toBeNull();
    expect(evaluated!.x).toBeGreaterThan(Math.min(left.x, right.x));
    expect(evaluated!.x).toBeLessThan(Math.max(left.x, right.x));
    expect(normalizeAutoReframePlan({ version: 1 })).toBeNull();
  });

  it("applies only camera keyframes to a snapshot and preserves other automation", () => {
    const result = successful([sample(0, 0.35), sample(300, 0.55), sample(600, 0.7)]);
    const preservedOpacity = [{ time: 0, value: 0.8, easing: "ease-in" as const }];
    const preservedVolume = [{ time: 0, value: 0.7, easing: "linear" as const }];
    const clip = {
      id: "video-1",
      trackId: "video",
      keyframes: {
        x: [{ time: 0, value: 99, easing: "linear" as const }],
        opacity: preservedOpacity,
        volume: preservedVolume,
      },
    };
    const snapshot = {
      version: 2,
      timebase: "milliseconds",
      tracks: [],
      clips: [clip],
      selectedClipId: "video-1",
      durationMs: 600,
    } as unknown as TimelineProjectSnapshot;
    const applied = applyAutoReframeVariantToSnapshot(
      snapshot,
      "video-1",
      result.plan.variants["9:16"],
    );

    expect(applied).not.toBe(snapshot);
    expect(applied.clips[0]).not.toBe(clip);
    expect(applied.clips[0].keyframes.opacity).toBe(preservedOpacity);
    expect(applied.clips[0].keyframes.volume).toBe(preservedVolume);
    expect(applied.clips[0].keyframes.x).toEqual(
      result.plan.variants["9:16"].timelineKeyframes.x,
    );
    expect(clip.keyframes.x[0].value).toBe(99);
  });
});
