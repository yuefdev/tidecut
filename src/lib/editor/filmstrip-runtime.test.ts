import { describe, expect, it } from "vitest";
import {
  MAX_FILMSTRIP_FRAMES,
  buildFilmstripRenderRequest,
  buildFilmstripFrameTimes,
  createFilmstripTaskQueue,
  getFilmstripFrameCount,
  mapFilmstripFrameSources,
} from "./filmstrip-runtime";
import {
  createClip,
  createTrack,
  normalizeProjectState,
  trimClip,
} from "./timeline-engine";

describe("filmstrip frame planning", () => {
  it("keeps an identical render request for fresh clips with equal scalar inputs", () => {
    const first = buildFilmstripRenderRequest("clip.mp4", 4, 1, 49);
    const cloned = buildFilmstripRenderRequest("clip.mp4", 4, 1, 80);

    expect(cloned).toBe(first);
    expect(buildFilmstripRenderRequest("clip.mp4", 3, 1, 80)).not.toBe(first);
    expect(buildFilmstripRenderRequest("clip.mp4", 4, 2, 80)).not.toBe(first);
    expect(buildFilmstripRenderRequest("clip.mp4", 4, 1, 97)).not.toBe(first);
    expect(buildFilmstripRenderRequest("other.mp4", 4, 1, 80)).not.toBe(first);
  });

  it("covers the visible clip width without opening an unbounded frame count", () => {
    expect(getFilmstripFrameCount(1)).toBe(1);
    expect(getFilmstripFrameCount(49)).toBe(2);
    expect(getFilmstripFrameCount(100_000)).toBe(MAX_FILMSTRIP_FRAMES);
  });

  it("samples a trimmed source range and stays clear of the exact EOF", () => {
    const times = buildFilmstripFrameTimes({
      videoDuration: 10,
      sourceOffset: 4,
      sourceDuration: 20,
      frameCount: 3,
    });

    expect(times).toHaveLength(3);
    expect(times[0]).toBe(4);
    expect(times[1]).toBeGreaterThan(4);
    expect(times[2]).toBeLessThan(10);
  });

  it("handles short and invalid media durations deterministically", () => {
    expect(
      buildFilmstripFrameTimes({
        videoDuration: 0.02,
        sourceOffset: 0,
        sourceDuration: 1,
        frameCount: 2,
      }),
    ).toEqual([0, 0.019]);
    expect(
      buildFilmstripFrameTimes({
        videoDuration: Number.NaN,
        sourceOffset: 0,
        sourceDuration: 1,
        frameCount: 2,
      }),
    ).toEqual([]);
  });

  it("tracks the same trimmed source window with ripple disabled or enabled", () => {
    const state = normalizeProjectState({
      tracks: [createTrack("v1", "V1", "video")],
      clips: [
        createClip({
          id: "clip",
          trackId: "v1",
          kind: "video",
          file: "clip.mp4",
          start: 2,
          duration: 4,
          trimIn: 1,
          sourceDuration: 10,
        }),
      ],
    });

    const regular = trimClip(state, "clip", "start", 1);
    const ripple = trimClip(state, "clip", "start", 1, { ripple: true });
    const regularClip = regular.state.clips[0];
    const rippleClip = ripple.state.clips[0];
    const plan = (trimIn: number, duration: number) =>
      buildFilmstripFrameTimes({
        videoDuration: 10,
        sourceOffset: trimIn,
        sourceDuration: duration,
        frameCount: 3,
      });

    expect(regularClip.start).toBe(3);
    expect(rippleClip.start).toBe(2);
    expect(plan(regularClip.trimIn, regularClip.duration)).toEqual(
      plan(rippleClip.trimIn, rippleClip.duration),
    );
    expect(plan(regularClip.trimIn, regularClip.duration)[0]).toBe(2);
  });

  it("updates the sampled tail when an end trim changes clip duration", () => {
    const before = buildFilmstripFrameTimes({
      videoDuration: 10,
      sourceOffset: 1,
      sourceDuration: 4,
      frameCount: 3,
    });
    const after = buildFilmstripFrameTimes({
      videoDuration: 10,
      sourceOffset: 1,
      sourceDuration: 3,
      frameCount: 3,
    });

    expect(before.at(-1)).toBe(5);
    expect(after.at(-1)).toBe(4);
  });

  it("fills decoder gaps from the nearest completed staging frame", () => {
    expect(
      mapFilmstripFrameSources([false, true, false, false, true]),
    ).toEqual([1, 1, 1, 4, 4]);
    expect(mapFilmstripFrameSources([false, false])).toEqual([-1, -1]);
  });
});

describe("filmstrip decoder queue", () => {
  it("limits concurrent decoder work and skips a cancelled queued task", async () => {
    const queue = createFilmstripTaskQueue(1);
    const order: string[] = [];
    let releaseFirst!: () => void;
    const firstGate = new Promise<void>((resolve) => (releaseFirst = resolve));

    const first = queue.schedule(async () => {
      order.push("first:start");
      await firstGate;
      order.push("first:end");
    });
    const cancelled = queue.schedule(async () => {
      order.push("cancelled");
    });
    const last = queue.schedule(async () => {
      order.push("last");
    });

    cancelled.cancel();
    await cancelled.done;
    await Promise.resolve();
    expect(order).toEqual(["first:start"]);

    releaseFirst();
    await Promise.all([first.done, cancelled.done, last.done]);
    expect(order).toEqual(["first:start", "first:end", "last"]);
  });

  it("aborts active decoder work before starting the next render", async () => {
    const queue = createFilmstripTaskQueue(1);
    const order: string[] = [];
    let observedAbort = false;

    const first = queue.schedule(
      (signal) =>
        new Promise<void>((resolve) => {
          order.push("first:start");
          signal.addEventListener(
            "abort",
            () => {
              observedAbort = true;
              order.push("first:abort");
              resolve();
            },
            { once: true },
          );
        }),
    );
    const next = queue.schedule(async () => {
      order.push("next");
    });

    first.cancel();
    await Promise.all([first.done, next.done]);

    expect(observedAbort).toBe(true);
    expect(order).toEqual(["first:start", "first:abort", "next"]);
  });
});
