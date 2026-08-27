import { describe, expect, it } from "vitest";
import { createClip, createTrack, type TimelineProjectState } from "./timeline-engine";
import { deleteClipRanges, splitVisualsAtTimes } from "./timeline-batch-edits";

function project(): TimelineProjectState {
  return {
    version: 2,
    tracks: [
      createTrack("v1", "V1", "video"),
      createTrack("a1", "A1", "audio"),
    ],
    clips: [
      createClip({
        id: "voice",
        trackId: "a1",
        file: "voice.wav",
        kind: "audio",
        start: 1,
        duration: 10,
        sourceDuration: 10,
      }),
      createClip({
        id: "after",
        trackId: "a1",
        file: "after.wav",
        kind: "audio",
        start: 11,
        duration: 2,
        sourceDuration: 2,
      }),
      createClip({
        id: "video",
        trackId: "v1",
        file: "video.mp4",
        kind: "video",
        start: 0,
        duration: 10,
        sourceDuration: 10,
      }),
    ],
    selectedClipId: "voice",
  };
}

describe("deleteClipRanges", () => {
  it("removes merged ranges while preserving source offsets", () => {
    let id = 0;
    const result = deleteClipRanges(
      project(),
      "voice",
      [
        { start: 2, end: 3 },
        { start: 2.8, end: 4 },
        { start: 6, end: 7 },
      ],
      { idFactory: () => `new-${++id}` },
    );
    const pieces = result.state.clips
      .filter((clip) => clip.trackId === "a1" && clip.id !== "after")
      .sort((a, b) => a.start - b.start);
    expect(pieces.map((clip) => [clip.start, clip.duration, clip.trimIn])).toEqual([
      [1, 2, 0],
      [5, 2, 4],
      [8, 3, 7],
    ]);
    expect(result.removedDuration).toBe(3);
  });

  it("packs kept pieces and downstream clips when ripple is enabled", () => {
    let id = 0;
    const result = deleteClipRanges(
      project(),
      "voice",
      [
        { start: 2, end: 3 },
        { start: 6, end: 8 },
      ],
      { ripple: true, idFactory: () => `new-${++id}` },
    );
    const pieces = result.state.clips
      .filter((clip) => clip.trackId === "a1")
      .sort((a, b) => a.start - b.start);
    expect(pieces.map((clip) => [clip.start, clip.duration])).toEqual([
      [1, 2],
      [3, 3],
      [6, 2],
      [8, 2],
    ]);
  });

  it("keeps other tracks untouched", () => {
    const result = deleteClipRanges(project(), "voice", [{ start: 2, end: 4 }], {
      ripple: true,
      idFactory: () => "piece",
    });
    expect(result.state.clips.find((clip) => clip.id === "video")?.start).toBe(0);
  });

  it("snaps sub-frame tail leftovers to the clip boundary and reports real removal", () => {
    const result = deleteClipRanges(project(), "voice", [{ start: 2, end: 9.999 }], {
      idFactory: () => "tail-piece",
    });
    const pieces = result.state.clips.filter(
      (clip) => clip.trackId === "a1" && clip.id !== "after",
    );
    expect(pieces.map((clip) => [clip.start, clip.duration, clip.trimIn])).toEqual([
      [1, 2, 0],
    ]);
    expect(result.removedDuration).toBeCloseTo(8, 6);
  });

  it("snaps sub-frame head leftovers and keeps the exact source offset", () => {
    const result = deleteClipRanges(project(), "voice", [{ start: 0.001, end: 4 }], {
      idFactory: () => "head-piece",
    });
    const pieces = result.state.clips.filter(
      (clip) => clip.trackId === "a1" && clip.id !== "after",
    );
    expect(pieces.map((clip) => [clip.start, clip.duration, clip.trimIn])).toEqual([
      [5, 6, 4],
    ]);
    expect(result.removedDuration).toBeCloseTo(4, 6);
  });

  it("reports a no-op when a proposed range is shorter than one frame", () => {
    const result = deleteClipRanges(project(), "voice", [{ start: 2, end: 2.001 }]);
    expect(result.removedDuration).toBe(0);
    expect(result.cutCount).toBe(0);
  });
});

describe("splitVisualsAtTimes", () => {
  it("makes all beat cuts in one returned state and ignores boundaries", () => {
    let id = 0;
    const result = splitVisualsAtTimes(project(), [0, 2, 4, 4.005, 9.95, 12], {
      idFactory: () => `cut-${++id}`,
      minimumSegmentSeconds: 0.1,
    });
    expect(result.cutCount).toBe(2);
    expect(
      result.state.clips
        .filter((clip) => clip.trackId === "v1")
        .sort((a, b) => a.start - b.start)
        .map((clip) => [clip.start, clip.duration]),
    ).toEqual([
      [0, 2],
      [2, 2],
      [4, 6],
    ]);
  });
});
