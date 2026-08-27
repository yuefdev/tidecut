import { describe, expect, it } from "vitest";
import {
  applyTransportSeek,
  playbackStartTime,
  resolveTransportShortcut,
} from "./transport-shortcuts";

describe("transport shortcuts", () => {
  it("toggles playback once and consumes held-key repeats", () => {
    expect(resolveTransportShortcut({ code: "Space" })).toEqual({
      kind: "toggle-playback",
    });
    expect(resolveTransportShortcut({ code: "Space", repeat: true })).toEqual({
      kind: "consume",
    });
    expect(resolveTransportShortcut({ code: "KeyK", repeat: true })).toEqual({
      kind: "consume",
    });
  });

  it("uses one second, five seconds and one frame for arrow seeking", () => {
    expect(resolveTransportShortcut({ code: "ArrowLeft" })).toEqual({
      kind: "seek-relative",
      deltaSeconds: -1,
    });
    expect(
      resolveTransportShortcut({ code: "ArrowRight", shiftKey: true }),
    ).toEqual({ kind: "seek-relative", deltaSeconds: 5 });
    expect(
      resolveTransportShortcut({ code: "ArrowLeft", altKey: true }),
    ).toEqual({ kind: "seek-relative", deltaSeconds: -1 / 30 });
  });

  it("keeps operating-system modifiers and IME input untouched", () => {
    expect(
      resolveTransportShortcut({ code: "ArrowLeft", ctrlKey: true }),
    ).toBeNull();
    expect(
      resolveTransportShortcut({ code: "Space", metaKey: true }),
    ).toBeNull();
    expect(
      resolveTransportShortcut({ code: "ArrowRight", isComposing: true }),
    ).toBeNull();
  });

  it("clamps relative and edge seeks to the real media duration", () => {
    expect(
      applyTransportSeek(0.25, 10, {
        kind: "seek-relative",
        deltaSeconds: -1,
      }),
    ).toBe(0);
    expect(
      applyTransportSeek(9, 10, {
        kind: "seek-relative",
        deltaSeconds: 5,
      }),
    ).toBe(10);
    expect(
      applyTransportSeek(4, 10, { kind: "seek-edge", edge: "end" }),
    ).toBe(10);
  });

  it("restarts playback from zero when the playhead is at the end", () => {
    expect(playbackStartTime(10, 10)).toBe(0);
    expect(playbackStartTime(9.99, 10)).toBe(0);
    expect(playbackStartTime(4, 10)).toBe(4);
  });
});
