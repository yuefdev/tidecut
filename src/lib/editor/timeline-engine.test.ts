import { describe, expect, it } from "vitest";
import {
  MIN_CLIP_DURATION,
  TIMELINE_EPSILON,
  buildFramePlan,
  createClip,
  createTextClip,
  createTrack,
  decibelsToGain,
  deleteClip,
  deleteTrack,
  evaluateKeyframes,
  generateWaveformPeaks,
  getClipVolumeRegions,
  getVerticalVolumeDragValue,
  gainToDecibels,
  getTimelineDuration,
  isTimelineProjectSnapshot,
  moveClip,
  normalizeProjectState,
  normalizeTextStyle,
  setClipSpeed,
  setClipVolumeRegion,
  setVoiceRiderEnvelope,
  removeVoiceRiderEnvelope,
  setTrackState,
  serializeProjectState,
  splitClip,
  trimClip,
  upsertKeyframe,
  validateTimeline,
  type TimelineProjectState,
} from "./timeline-engine";
import {
  createSpeechAnalysisMetadata,
  speechAnalysisSignature,
} from "./audio-ducking";

function project(): TimelineProjectState {
  return normalizeProjectState({
    tracks: [
      createTrack("v1", "V1", "video"),
      createTrack("v2", "V2", "video"),
      createTrack("a1", "A1", "audio"),
      createTrack("t1", "T1", "text"),
    ],
    clips: [
      createClip({
        id: "one",
        trackId: "v1",
        kind: "video",
        file: "one.mp4",
        start: 2,
        duration: 4,
        trimIn: 1,
        sourceDuration: 10,
      }),
      createClip({
        id: "two",
        trackId: "v1",
        kind: "video",
        file: "two.mp4",
        start: 6,
        duration: 3,
        sourceDuration: 10,
      }),
    ],
    selectedClipId: "one",
  });
}

describe("timeline edits", () => {
  it("trims the start while preserving the same source frame", () => {
    const result = trimClip(project(), "one", "start", 1);
    const clip = result.state.clips.find((item) => item.id === "one")!;
    expect(result.appliedDelta).toBe(1);
    expect(clip.start).toBe(3);
    expect(clip.duration).toBe(3);
    expect(clip.trimIn).toBe(2);
    expect(clip.videoOffset).toBe(2);
  });

  it("clamps trim handles to media bounds and minimum duration", () => {
    const left = trimClip(project(), "one", "start", -100);
    expect(left.appliedDelta).toBe(-1);
    expect(left.state.clips[0].trimIn).toBe(0);

    const right = trimClip(project(), "one", "end", -100);
    expect(right.state.clips[0].duration).toBeCloseTo(MIN_CLIP_DURATION);
  });

  it("ripples downstream clips when a trim handle changes program length", () => {
    const result = trimClip(project(), "one", "end", -1, { ripple: true });
    expect(result.state.clips.find((item) => item.id === "two")!.start).toBe(5);
    expect(result.affectedClipIds).toEqual(["one", "two"]);
  });

  it("ripple-trims a clip head without leaving a gap", () => {
    const result = trimClip(project(), "one", "start", 1, { ripple: true });
    const first = result.state.clips.find((item) => item.id === "one")!;
    const next = result.state.clips.find((item) => item.id === "two")!;
    expect(first.start).toBe(2);
    expect(first.duration).toBe(3);
    expect(first.trimIn).toBe(2);
    expect(next.start).toBe(5);
  });

  it("ripple-deletes only clips on the edited track", () => {
    const state = project();
    state.clips.push(
      createClip({
        id: "overlay",
        trackId: "v2",
        kind: "video",
        start: 8,
        duration: 1,
      }),
    );
    const result = deleteClip(state, "one", { ripple: true });
    expect(result.state.clips.find((item) => item.id === "two")!.start).toBe(2);
    expect(result.state.clips.find((item) => item.id === "overlay")!.start).toBe(8);
  });

  it("deletes an empty video or audio track without mutating caller-owned state", () => {
    const videoState = project();
    const videoResult = deleteTrack(videoState, "v2");

    expect(videoResult.tracks.map((track) => track.id)).toEqual([
      "v1",
      "a1",
      "t1",
    ]);
    expect(videoState.tracks.some((track) => track.id === "v2")).toBe(true);

    const audioState = project();
    audioState.tracks.push(createTrack("a2", "A2", "audio"));
    const audioResult = deleteTrack(audioState, "a2");
    expect(audioResult.tracks.some((track) => track.id === "a2")).toBe(
      false,
    );
  });

  it("refuses to delete text, last-of-type, and locked tracks", () => {
    expect(() => deleteTrack(project(), "t1")).toThrow(
      "Text track t1 cannot be deleted",
    );

    const singleVideo = project();
    singleVideo.tracks = singleVideo.tracks.filter((track) => track.id !== "v2");
    expect(() => deleteTrack(singleVideo, "v1", { cascade: true })).toThrow(
      "last video track",
    );
    expect(() => deleteTrack(project(), "a1")).toThrow("last audio track");

    const locked = setTrackState(project(), "v2", { locked: true });
    expect(() => deleteTrack(locked, "v2")).toThrow("locked");
  });

  it("refuses to delete a non-empty track without cascade", () => {
    const state = project();
    const before = structuredClone(state);

    expect(() => deleteTrack(state, "v1", { cascade: false })).toThrow(
      "Track v1 is not empty",
    );
    expect(state).toEqual(before);
  });

  it("cascade-deletes a track, its clips, and a selected clip on that track", () => {
    const state = project();
    const before = structuredClone(state);
    const result = deleteTrack(state, "v1", { cascade: true });

    expect(result.tracks.some((track) => track.id === "v1")).toBe(false);
    expect(result.clips).toEqual([]);
    expect(result.selectedClipId).toBeNull();
    expect(state).toEqual(before);
  });

  it("refuses destructive edits on locked tracks", () => {
    const locked = setTrackState(project(), "v1", { locked: true });
    expect(() => trimClip(locked, "one", "end", -1)).toThrow("locked");
    expect(() => deleteClip(locked, "one")).toThrow("locked");
  });

  it("moves a clip into an exact gap on the same track", () => {
    const state = project();
    state.clips.push(
      createClip({
        id: "tail",
        trackId: "v1",
        kind: "video",
        start: 12,
        duration: 2,
      }),
    );
    const result = moveClip(state, "two", "v1", 9);
    const moved = result.state.clips.find((item) => item.id === "two")!;

    expect(moved.start).toBe(9);
    expect(moved.trackId).toBe("v1");
    expect(result.appliedDelta).toBe(3);
    expect(result.affectedClipIds).toEqual(["two"]);
    expect(result.state.selectedClipId).toBe("one");
    expect(validateTimeline(result.state)).not.toEqual(
      expect.arrayContaining([expect.objectContaining({ code: "overlap" })]),
    );
  });

  it("moves video and image clips vertically between compatible tracks", () => {
    const state = project();
    const result = moveClip(state, "one", "v2", 2);
    const moved = result.state.clips.find((item) => item.id === "one")!;

    expect(moved.trackId).toBe("v2");
    expect(moved.start).toBe(2);
    expect(result.appliedDelta).toBe(0);
    expect(result.affectedClipIds).toEqual(["one"]);
    expect(result.state.selectedClipId).toBe("one");
  });

  it("optionally closes the source slot for an ordinary free-placement move", () => {
    const state = project();
    const result = moveClip(state, "one", "v2", 4, {
      rippleSource: true,
    });
    const moved = result.state.clips.find((item) => item.id === "one")!;
    const sourceFollower = result.state.clips.find((item) => item.id === "two")!;

    expect(moved).toMatchObject({ trackId: "v2", start: 4 });
    expect(sourceFollower.start).toBe(2);
    expect(result.affectedClipIds).toEqual(
      expect.arrayContaining(["one", "two"]),
    );
    expect(state.clips.find((item) => item.id === "two")!.start).toBe(6);
    expect(validateTimeline(result.state).filter((issue) => issue.code === "overlap"))
      .toEqual([]);
  });

  it("rejects overlapping moves without mutating caller-owned state", () => {
    const state = project();
    const before = structuredClone(state);

    expect(() => moveClip(state, "two", "v1", 4)).toThrow("overlap");
    expect(state).toEqual(before);

    state.clips.push(
      createClip({
        id: "overlay",
        trackId: "v2",
        kind: "video",
        start: 3,
        duration: 2,
      }),
    );
    const beforeCrossTrackMove = structuredClone(state);
    expect(() => moveClip(state, "one", "v2", 2)).toThrow("overlap");
    expect(state).toEqual(beforeCrossTrackMove);
  });

  it("validates move timing, track compatibility and both track locks", () => {
    const state = project();
    expect(() => moveClip(state, "one", "v2", -1)).toThrow(
      "finite non-negative",
    );
    expect(() => moveClip(state, "one", "v2", Number.NaN)).toThrow(
      "finite non-negative",
    );
    expect(() => moveClip(state, "one", "v2", Number.POSITIVE_INFINITY)).toThrow(
      "finite non-negative",
    );
    expect(() => moveClip(state, "one", "a1", 0)).toThrow("not compatible");

    const sourceLocked = setTrackState(state, "v1", { locked: true });
    expect(() => moveClip(sourceLocked, "one", "v2", 0)).toThrow("locked");

    const targetLocked = setTrackState(state, "v2", { locked: true });
    expect(() => moveClip(targetLocked, "one", "v2", 0)).toThrow("locked");
  });

  it("insert-edits a clip between adjacent split clips without overwriting media", () => {
    const state = splitClip(project(), "one", 4, "one-b").state;
    state.clips.push(
      createClip({
        id: "insert",
        trackId: "v2",
        kind: "video",
        start: 10,
        duration: 1,
      }),
      createClip({
        id: "source-tail",
        trackId: "v2",
        kind: "video",
        start: 11,
        duration: 1,
      }),
    );

    const result = moveClip(state, "insert", "v1", 4, { mode: "insert" });
    const byId = (id: string) => result.state.clips.find((clip) => clip.id === id)!;

    expect(byId("one").start).toBe(2);
    expect(byId("insert").start).toBe(4);
    expect(byId("insert").trackId).toBe("v1");
    expect(byId("one-b").start).toBe(5);
    expect(byId("two").start).toBe(7);
    expect(byId("source-tail").start).toBe(10);
    expect(result.affectedClipIds).toEqual(
      expect.arrayContaining(["insert", "source-tail", "one-b", "two"]),
    );
    expect(validateTimeline(result.state)).not.toEqual(
      expect.arrayContaining([expect.objectContaining({ code: "overlap" })]),
    );

    // The caller-owned project stays untouched; only the returned source
    // track closes the slot left by the extracted clip.
    expect(state.clips.find((clip) => clip.id === "insert")?.start).toBe(10);
    expect(state.clips.find((clip) => clip.id === "source-tail")?.start).toBe(11);
  });

  it("keeps ordinary moves non-destructive while insert mode opens space", () => {
    const state = splitClip(project(), "one", 4, "one-b").state;
    state.clips.push(
      createClip({
        id: "insert",
        trackId: "v2",
        kind: "video",
        start: 10,
        duration: 1,
      }),
    );

    expect(() => moveClip(state, "insert", "v1", 4)).toThrow("overlap");
    expect(moveClip(state, "insert", "v1", 4, { mode: "insert" }).state)
      .toEqual(expect.objectContaining({ clips: expect.any(Array) }));
  });

  it("rejects insertion strictly inside a target clip without mutating input", () => {
    const state = project();
    state.clips.push(
      createClip({
        id: "insert",
        trackId: "v2",
        kind: "video",
        start: 10,
        duration: 1,
      }),
    );
    const before = structuredClone(state);

    expect(() =>
      moveClip(state, "insert", "v1", 3, { mode: "insert" }),
    ).toThrow("inside clip one");
    expect(state).toEqual(before);
  });

  it("insert-edits vertically across compatible tracks at an exact boundary", () => {
    const state = project();
    state.clips.push(
      createClip({
        id: "v2-lead",
        trackId: "v2",
        kind: "video",
        start: 0,
        duration: 2,
      }),
      createClip({
        id: "v2-tail",
        trackId: "v2",
        kind: "video",
        start: 2,
        duration: 1,
      }),
    );

    const result = moveClip(state, "one", "v2", 2, { mode: "insert" });
    const byId = (id: string) => result.state.clips.find((clip) => clip.id === id)!;

    expect(byId("one")).toMatchObject({ trackId: "v2", start: 2, duration: 4 });
    expect(byId("v2-lead").start).toBe(0);
    expect(byId("v2-tail").start).toBe(6);
    expect(byId("two").start).toBe(2);
    expect(result.affectedClipIds).toEqual(
      expect.arrayContaining(["one", "two", "v2-tail"]),
    );
    expect(validateTimeline(result.state)).not.toEqual(
      expect.arrayContaining([expect.objectContaining({ code: "overlap" })]),
    );
  });

  it("maps original coordinates through extraction for later same-track inserts", () => {
    const state = project();
    state.clips.push(
      createClip({
        id: "tail",
        trackId: "v1",
        kind: "video",
        start: 9,
        duration: 2,
      }),
    );

    const result = moveClip(state, "one", "v1", 9, { mode: "insert" });
    const byId = (id: string) => result.state.clips.find((clip) => clip.id === id)!;

    expect(byId("two").start).toBe(2);
    expect(byId("one").start).toBe(5);
    expect(byId("tail").start).toBe(9);
    expect(result.appliedDelta).toBe(3);
    expect(getTimelineDuration(result.state)).toBe(11);
  });

  it("consumes a smaller target gap before shifting only the overflow", () => {
    const state = normalizeProjectState({
      tracks: [
        createTrack("v1", "V1", "video"),
        createTrack("v2", "V2", "video"),
      ],
      clips: [
        createClip({ id: "source", trackId: "v1", kind: "video", start: 0, duration: 3 }),
        createClip({ id: "source-tail", trackId: "v1", kind: "video", start: 3, duration: 2 }),
        createClip({ id: "left", trackId: "v2", kind: "video", start: 0, duration: 2 }),
        createClip({ id: "right", trackId: "v2", kind: "video", start: 3, duration: 2 }),
        createClip({ id: "tail", trackId: "v2", kind: "video", start: 5, duration: 1 }),
      ],
    });

    const result = moveClip(state, "source", "v2", 3, { mode: "insert" });
    const byId = (id: string) => result.state.clips.find((clip) => clip.id === id)!;

    expect(byId("source")).toMatchObject({ trackId: "v2", start: 2, duration: 3 });
    expect(byId("right").start).toBe(5);
    expect(byId("tail").start).toBe(7);
    expect(byId("source-tail").start).toBe(0);
    expect(result.affectedClipIds).toEqual(
      expect.arrayContaining(["source", "source-tail", "right", "tail"]),
    );
    expect(validateTimeline(result.state).filter((issue) => issue.code === "overlap"))
      .toEqual([]);
  });

  it("fills a large target gap from its left edge without shifting the right side", () => {
    const state = normalizeProjectState({
      tracks: [
        createTrack("v1", "V1", "video"),
        createTrack("v2", "V2", "video"),
      ],
      clips: [
        createClip({ id: "source", trackId: "v1", kind: "video", start: 0, duration: 3 }),
        createClip({ id: "source-tail", trackId: "v1", kind: "video", start: 3, duration: 1 }),
        createClip({ id: "left", trackId: "v2", kind: "video", start: 0, duration: 2 }),
        createClip({ id: "right", trackId: "v2", kind: "video", start: 6, duration: 2 }),
      ],
    });

    const result = moveClip(state, "source", "v2", 6, { mode: "insert" });
    const byId = (id: string) => result.state.clips.find((clip) => clip.id === id)!;

    expect(byId("source")).toMatchObject({ trackId: "v2", start: 2, duration: 3 });
    expect(byId("right").start).toBe(6);
    expect(byId("source-tail").start).toBe(0);
    expect(result.affectedClipIds).not.toContain("right");
    expect(validateTimeline(result.state).filter((issue) => issue.code === "overlap"))
      .toEqual([]);
  });

  it("treats inserting at the moved clip's own start or end as a no-op", () => {
    const state = project();
    const atStart = moveClip(state, "one", "v1", 2, { mode: "insert" });
    const atEnd = moveClip(state, "one", "v1", 6, { mode: "insert" });

    expect(atStart.state).toEqual(state);
    expect(atEnd.state).toEqual(state);
    expect(atStart.affectedClipIds).toEqual([]);
    expect(atEnd.affectedClipIds).toEqual([]);
    expect(atEnd.appliedDelta).toBe(0);
  });

  it("validates compatibility and source/target locks for insert edits", () => {
    const state = project();
    expect(() =>
      moveClip(state, "one", "a1", 0, { mode: "insert" }),
    ).toThrow("not compatible");

    const sourceLocked = setTrackState(state, "v1", { locked: true });
    expect(() =>
      moveClip(sourceLocked, "one", "v2", 0, { mode: "insert" }),
    ).toThrow("locked");

    const targetLocked = setTrackState(state, "v2", { locked: true });
    expect(() =>
      moveClip(targetLocked, "one", "v2", 0, { mode: "insert" }),
    ).toThrow("locked");
  });

  it("splits timing, trim, transitions and local keyframes correctly", () => {
    let state = upsertKeyframe(project(), "one", "opacity", {
      time: 1,
      value: 0.25,
      easing: "linear",
    });
    state = upsertKeyframe(state, "one", "opacity", {
      time: 3,
      value: 1,
      easing: "ease-out",
    });
    state.clips[0].transition.out = { type: "crossfade", duration: 0.5 };

    const result = splitClip(state, "one", 4, "one-b");
    const first = result.state.clips.find((item) => item.id === "one")!;
    const second = result.state.clips.find((item) => item.id === "one-b")!;
    expect(first.duration).toBe(2);
    expect(second.duration).toBe(2);
    expect(second.trimIn).toBe(3);
    expect(first.transition.out.type).toBe("none");
    expect(second.transition.out.type).toBe("crossfade");
    // splitKeyframes preserves the animated value at the cut with a boundary
    // keyframe at t=0, followed by the shifted original keyframe.
    expect(second.keyframes.opacity?.map((frame) => frame.time)).toEqual([0, 1]);
    expect(second.keyframes.opacity?.[1].value).toBe(1);
  });

  it("clamps non-ripple trim extensions to adjacent clips on the same track", () => {
    const state = project();
    const second = state.clips.find((item) => item.id === "two")!;
    second.start = 7;
    second.trimIn = 2;
    second.videoOffset = 2;

    const extendEnd = trimClip(state, "one", "end", 5);
    const firstAfterEnd = extendEnd.state.clips.find((item) => item.id === "one")!;
    const secondAfterEnd = extendEnd.state.clips.find((item) => item.id === "two")!;
    expect(extendEnd.appliedDelta).toBe(1);
    expect(firstAfterEnd.start + firstAfterEnd.duration).toBe(
      secondAfterEnd.start,
    );
    expect(
      validateTimeline(extendEnd.state).filter(
        (issue) => issue.code === "overlap",
      ),
    ).toEqual([]);

    const extendStart = trimClip(state, "two", "start", -5);
    const firstAfterStart = extendStart.state.clips.find((item) => item.id === "one")!;
    const secondAfterStart = extendStart.state.clips.find((item) => item.id === "two")!;
    expect(extendStart.appliedDelta).toBe(-1);
    expect(secondAfterStart.start).toBe(
      firstAfterStart.start + firstAfterStart.duration,
    );
    expect(
      validateTimeline(extendStart.state).filter(
        (issue) => issue.code === "overlap",
      ),
    ).toEqual([]);
  });

  it("keeps split neighbors adjacent for non-ripple and ripple trims", () => {
    const split = splitClip(project(), "one", 4, "one-b").state;

    const blockedEnd = trimClip(split, "one", "end", 1);
    const blockedStart = trimClip(split, "one-b", "start", -1);
    expect(blockedEnd.appliedDelta).toBe(0);
    expect(blockedStart.appliedDelta).toBe(0);
    expect(
      validateTimeline(blockedEnd.state).filter(
        (issue) => issue.code === "overlap",
      ),
    ).toEqual([]);
    expect(
      validateTimeline(blockedStart.state).filter(
        (issue) => issue.code === "overlap",
      ),
    ).toEqual([]);

    const rippled = trimClip(split, "one", "end", 1, { ripple: true });
    const first = rippled.state.clips.find((item) => item.id === "one")!;
    const second = rippled.state.clips.find((item) => item.id === "one-b")!;
    const downstream = rippled.state.clips.find((item) => item.id === "two")!;
    expect(rippled.appliedDelta).toBe(1);
    expect(second.start).toBe(first.start + first.duration);
    expect(downstream.start).toBe(second.start + second.duration);
    expect(
      validateTimeline(rippled.state).filter(
        (issue) => issue.code === "overlap",
      ),
    ).toEqual([]);
  });

  it("ripples a downstream neighbour within the shared timeline tolerance", () => {
    const state = project();
    const next = state.clips.find((item) => item.id === "two")!;
    next.start -= TIMELINE_EPSILON / 2;
    expect(
      validateTimeline(state).filter((issue) => issue.code === "overlap"),
    ).toEqual([]);

    const result = trimClip(state, "one", "end", 1, { ripple: true });
    const moved = result.state.clips.find((item) => item.id === "two")!;
    expect(moved.start).toBeCloseTo(7 - TIMELINE_EPSILON / 2, 8);
    expect(result.affectedClipIds).toContain("two");
    expect(
      validateTimeline(result.state).filter((issue) => issue.code === "overlap"),
    ).toEqual([]);
  });

  it("changes speed with source or timeline duration preservation", () => {
    const faster = setClipSpeed(project(), "one", 2, "source");
    expect(faster.state.clips[0].duration).toBe(2);

    const sameDuration = setClipSpeed(project(), "one", 2, "timeline");
    expect(sameDuration.state.clips[0].duration).toBe(4);
    expect(sameDuration.state.clips[0].speed).toBe(2);
  });
});

describe("AI Voice Rider envelope", () => {
  const metadata = {
    model: "silero-vad-v6" as const,
    speechCoverage: 0.72,
    averageSpeechProbability: 0.86,
    strongestCutDb: -7,
    strongestBoostDb: 4,
  };

  it("multiplies a separate rider envelope without overwriting manual volume", () => {
    const base = project();
    const manual = upsertKeyframe(base, "one", "volume", {
      time: 0,
      value: 0.8,
      easing: "linear",
    });
    const ridden = setVoiceRiderEnvelope(manual, "one", metadata, [
      { time: 0, value: 0.5, easing: "ease-in-out" },
      { time: 4, value: 1.25, easing: "ease-in-out" },
    ]);

    expect(ridden.clips[0].keyframes.volume?.[0].value).toBe(0.8);
    expect(ridden.clips[0].keyframes.riderGain).toHaveLength(2);
    expect(buildFramePlan(ridden, 2).audio[0].gain).toBeCloseTo(0.4);
    expect(buildFramePlan(ridden, 5.999).audio[0].gain).toBeCloseTo(1);
  });

  it("replaces AI points on rerun and removes them without touching manual automation", () => {
    const manual = upsertKeyframe(project(), "one", "volume", {
      time: 1,
      value: 0.7,
      easing: "linear",
    });
    const first = setVoiceRiderEnvelope(manual, "one", metadata, [
      { time: 0, value: 0.5, easing: "linear" },
      { time: 4, value: 0.5, easing: "linear" },
    ]);
    const second = setVoiceRiderEnvelope(first, "one", metadata, [
      { time: 0, value: 1.2, easing: "linear" },
      { time: 4, value: 0.8, easing: "linear" },
    ]);
    expect(second.clips[0].keyframes.riderGain?.map((frame) => frame.value)).toEqual([
      1.2,
      0.8,
    ]);

    const removed = removeVoiceRiderEnvelope(second, "one");
    expect(removed.clips[0].keyframes.riderGain).toBeUndefined();
    expect(removed.clips[0].keyframes.volume?.[0].value).toBe(0.7);
    expect(removed.clips[0].voiceRider).toBeNull();
  });

  it("invalidates an analyzed envelope after source trim or speed changes", () => {
    const ridden = setVoiceRiderEnvelope(project(), "one", metadata, [
      { time: 0, value: 1, easing: "linear" },
      { time: 4, value: 0.6, easing: "linear" },
    ]);
    expect(trimClip(ridden, "one", "start", 0.5).state.clips[0].voiceRider).toBeNull();
    expect(setClipSpeed(ridden, "one", 1.5).state.clips[0].voiceRider).toBeNull();
  });

  it("keeps the AI gain continuous when a ridden clip is split", () => {
    const ridden = setVoiceRiderEnvelope(project(), "one", metadata, [
      { time: 0, value: 0.5, easing: "ease-in-out" },
      { time: 4, value: 1.5, easing: "ease-in-out" },
    ]);
    const split = splitClip(ridden, "one", 4, "second").state;
    const first = split.clips.find((clip) => clip.id === "one")!;
    const second = split.clips.find((clip) => clip.id === "second")!;

    expect(first.voiceRider).toEqual(metadata);
    expect(second.voiceRider).toEqual(metadata);
    expect(evaluateKeyframes(first.keyframes.riderGain, first.duration, 1)).toBeCloseTo(1);
    expect(evaluateKeyframes(second.keyframes.riderGain, 0, 1)).toBeCloseTo(1);
  });

  it("drops orphan AI metadata or gain points while normalizing saved clips", () => {
    const orphanPoints = createClip({
      id: "points",
      trackId: "a1",
      kind: "audio",
      start: 0,
      duration: 2,
      keyframes: {
        riderGain: [
          { time: 0, value: 0.8, easing: "linear" },
          { time: 2, value: 1.2, easing: "linear" },
        ],
      },
    });
    const orphanMetadata = createClip({
      id: "metadata",
      trackId: "a1",
      kind: "audio",
      start: 0,
      duration: 2,
      voiceRider: metadata,
    });

    expect(orphanPoints.keyframes.riderGain).toBeUndefined();
    expect(orphanPoints.voiceRider).toBeNull();
    expect(orphanMetadata.keyframes.riderGain).toBeUndefined();
    expect(orphanMetadata.voiceRider).toBeNull();
  });
});

describe("AI music ducking projection", () => {
  function duckingProject(): TimelineProjectState {
    const narration = createClip({
      id: "narration",
      trackId: "voice",
      kind: "audio",
      file: "voice.wav",
      start: 0,
      duration: 4,
    });
    narration.speechAnalysis = createSpeechAnalysisMetadata(
      [{ start: 1, end: 3, confidence: 0.9 }],
      speechAnalysisSignature(narration),
      narration.duration,
    );
    const music = createClip({
      id: "music",
      trackId: "music-track",
      kind: "audio",
      file: "music.wav",
      start: 0,
      duration: 4,
      volume: 0.8,
      keyframes: {
        volume: [
          { time: 0, value: 0.8, easing: "linear" },
          { time: 4, value: 0.8, easing: "linear" },
        ],
        riderGain: [
          { time: 0, value: 0.5, easing: "linear" },
          { time: 4, value: 0.5, easing: "linear" },
        ],
      },
      voiceRider: {
        model: "silero-vad-v6",
        speechCoverage: 1,
        averageSpeechProbability: 0.9,
        strongestCutDb: -6,
        strongestBoostDb: 0,
      },
      autoDucking: {
        version: 1,
        enabled: true,
        sourceTrackIds: ["voice"],
        reductionDb: -6,
        lookaheadMs: 0,
        attackMs: 100,
        holdMs: 0,
        releaseMs: 100,
        minConfidence: 0.5,
      },
    });
    return normalizeProjectState({
      tracks: [
        createTrack("voice", "Voice", "audio"),
        createTrack("music-track", "Music", "audio", { gain: 0.5 }),
      ],
      clips: [narration, music],
      selectedClipId: music.id,
    });
  }

  it("multiplies derived duck gain without changing manual volume or Rider", () => {
    const state = duckingProject();
    const music = state.clips.find((clip) => clip.id === "music")!;
    const manualBefore = structuredClone(music.keyframes.volume);
    const riderBefore = structuredClone(music.keyframes.riderGain);
    const expectedDuck = Math.pow(10, -6 / 20);

    expect(buildFramePlan(state, 1.5).audio.find((source) => source.clipId === "music")?.gain)
      .toBeCloseTo(0.8 * 0.5 * expectedDuck * 0.5, 6);
    expect(music.keyframes.volume).toEqual(manualBefore);
    expect(music.keyframes.riderGain).toEqual(riderBefore);

    music.autoDucking = null;
    expect(buildFramePlan(state, 1.5).audio.find((source) => source.clipId === "music")?.gain)
      .toBeCloseTo(0.8 * 0.5 * 0.5, 6);
    expect(music.keyframes.volume).toEqual(manualBefore);
    expect(music.keyframes.riderGain).toEqual(riderBefore);
  });

  it("splits cached local speech while keeping target duck settings independent", () => {
    const state = duckingProject();
    const splitVoice = splitClip(state, "narration", 2, "narration-b").state;
    const first = splitVoice.clips.find((clip) => clip.id === "narration")!;
    const second = splitVoice.clips.find((clip) => clip.id === "narration-b")!;
    expect(first.speechAnalysis?.segments).toEqual([
      { start: 1, end: 2, confidence: 0.9 },
    ]);
    expect(second.speechAnalysis?.segments).toEqual([
      { start: 0, end: 1, confidence: 0.9 },
    ]);

    const splitMusic = splitClip(state, "music", 2, "music-b").state;
    expect(splitMusic.clips.find((clip) => clip.id === "music")?.autoDucking)
      .toEqual(splitMusic.clips.find((clip) => clip.id === "music-b")?.autoDucking);
    expect(splitMusic.clips.find((clip) => clip.id === "music")?.autoDucking)
      .not.toBe(splitMusic.clips.find((clip) => clip.id === "music-b")?.autoDucking);
  });

  it("invalidates source analysis after trim or speed but saves valid metadata", () => {
    const state = duckingProject();
    expect(trimClip(state, "narration", "end", -0.5).state.clips[0].speechAnalysis)
      .toBeNull();
    expect(setClipSpeed(state, "narration", 1.25).state.clips[0].speechAnalysis)
      .toBeNull();

    const parsed = JSON.parse(JSON.stringify(serializeProjectState(state)));
    expect(parsed.clips.find((clip: { id: string }) => clip.id === "narration").speechAnalysis)
      .toMatchObject({ model: "silero-vad-v6", speechCoverage: 0.5 });
    expect(parsed.clips.find((clip: { id: string }) => clip.id === "music").autoDucking)
      .toMatchObject({ reductionDb: -6, sourceTrackIds: ["voice"] });
  });
});

describe("compositor and mixer planning", () => {
  it("returns all active visual layers in track order", () => {
    const state = project();
    state.clips.push(
      createClip({
        id: "top",
        trackId: "v2",
        kind: "image",
        file: "overlay.png",
        start: 2,
        duration: 4,
      }),
      createTextClip({
        id: "title",
        trackId: "t1",
        start: 2,
        duration: 4,
      }),
    );
    const plan = buildFramePlan(state, 3);
    expect(plan.layers.map((layer) => layer.clipId)).toEqual(["one", "top", "title"]);
  });

  it("applies transition opacity and keyframed transforms", () => {
    let state = project();
    state.clips[0].transition.in = { type: "crossfade", duration: 2 };
    state = upsertKeyframe(state, "one", "x", {
      time: 0,
      value: 0,
      easing: "linear",
    });
    state = upsertKeyframe(state, "one", "x", {
      time: 2,
      value: 100,
      easing: "linear",
    });
    const layer = buildFramePlan(state, 3).layers[0];
    expect(layer.opacity).toBeCloseTo(0.5);
    expect(layer.transform.x).toBeCloseTo(50);
    expect(layer.transitionType).toBe("crossfade");
  });

  it("produces text animation snapshots for enter and exit", () => {
    const state = normalizeProjectState({
      tracks: [createTrack("t1", "T1", "text")],
      clips: [
        createTextClip({
          id: "title",
          trackId: "t1",
          start: 0,
          duration: 5,
          text: normalizeTextStyle({
            content: "Merhaba",
            animationIn: { type: "zoom", duration: 1 },
            animationOut: { type: "fade", duration: 1 },
          }),
        }),
      ],
    });

    const entering = buildFramePlan(state, 0.2).layers[0];
    expect(entering.textFx).not.toBeNull();
    expect(entering.textFx!.scale).toBeLessThan(1);
    expect(entering.textFx!.alpha).toBeLessThan(1);

    const settled = buildFramePlan(state, 2.5).layers[0];
    expect(settled.textFx).toBeNull();

    const exiting = buildFramePlan(state, 4.8).layers[0];
    expect(exiting.textFx).not.toBeNull();
    expect(exiting.textFx!.alpha).toBeLessThan(1);
    expect(exiting.textFx!.scale).toBe(1);
  });

  it("honors mute/solo and combines clip + track gain and pan", () => {
    let state = project();
    state.clips.push(
      createClip({
        id: "music",
        trackId: "a1",
        kind: "audio",
        file: "music.wav",
        start: 2,
        duration: 4,
        volume: 0.5,
        pan: 0.25,
      }),
    );
    state = setTrackState(state, "a1", { solo: true, gain: 0.5, pan: -0.5 });
    const plan = buildFramePlan(state, 3);
    expect(plan.layers).toHaveLength(0);
    expect(plan.audio).toHaveLength(1);
    expect(plan.audio[0].gain).toBeCloseTo(0.25);
    expect(plan.audio[0].pan).toBeCloseTo(-0.25);
  });

  it("keeps separated video audio routed through a silent mixer strip", () => {
    const state = project();
    state.clips[0] = createClip({
      ...state.clips[0],
      audioSeparated: true,
      volume: 1,
      speed: 1.5,
    });

    const plan = buildFramePlan(state, 3);
    const source = plan.audio.find((item) => item.clipId === "one");

    expect(source).toBeDefined();
    expect(source?.gain).toBe(0);
    expect(source?.playbackRate).toBe(1.5);
  });
});

describe("keyframes, waveform and validation", () => {
  it("converts clip gain to a clear relative decibel scale", () => {
    expect(gainToDecibels(1)).toBe(0);
    expect(gainToDecibels(2)).toBeCloseTo(6.0206, 3);
    expect(gainToDecibels(0.5)).toBeCloseTo(-6.0206, 3);
    expect(gainToDecibels(0)).toBe(Number.NEGATIVE_INFINITY);

    expect(decibelsToGain(0)).toBe(1);
    expect(decibelsToGain(6.0206)).toBeCloseTo(2, 3);
    expect(decibelsToGain(Number.NEGATIVE_INFINITY)).toBe(0);
    expect(decibelsToGain(99)).toBe(4);
  });

  it("maps an upward region drag to a clamped volume increase", () => {
    expect(getVerticalVolumeDragValue(0.35, 1, 35)).toBeCloseTo(0.6);
    expect(getVerticalVolumeDragValue(0.35, 1, -200)).toBe(0);
    expect(getVerticalVolumeDragValue(0.35, 1, 200)).toBe(1);
  });

  it("interpolates supported easing modes", () => {
    const linear = evaluateKeyframes(
      [
        { time: 0, value: 0, easing: "linear" },
        { time: 2, value: 10, easing: "linear" },
      ],
      1,
      -1,
    );
    const hold = evaluateKeyframes(
      [
        { time: 0, value: 3, easing: "hold" },
        { time: 2, value: 10, easing: "linear" },
      ],
      1,
      -1,
    );
    expect(linear).toBe(5);
    expect(hold).toBe(3);
  });

  it("ducks a selected audio region with short linear fades", () => {
    const original = project();
    const state = setClipVolumeRegion(original, "one", 1, 3, 0.25, 0.2);
    const frames = state.clips.find((clip) => clip.id === "one")!.keyframes
      .volume!;

    expect(frames).toEqual([
      { time: 0.8, value: 1, easing: "linear" },
      { time: 1, value: 0.25, easing: "linear" },
      { time: 3, value: 0.25, easing: "linear" },
      { time: 3.2, value: 1, easing: "linear" },
    ]);
    expect(evaluateKeyframes(frames, 2, 1)).toBe(0.25);
    expect(getClipVolumeRegions(state.clips[0])).toEqual([
      { start: 1, end: 3, volume: 0.25 },
    ]);
    expect(original.clips[0].keyframes.volume).toBeUndefined();
  });

  it("rejects zero-length volume regions and locked tracks", () => {
    expect(() => setClipVolumeRegion(project(), "one", 2, 2, 0.25)).toThrow(
      "positive duration",
    );

    const locked = project();
    locked.tracks.find((track) => track.id === "v1")!.locked = true;
    expect(() => setClipVolumeRegion(locked, "one", 1, 2, 0.25)).toThrow(
      "locked",
    );
  });

  it("extracts deterministic multi-channel waveform peaks", () => {
    const peaks = generateWaveformPeaks(
      [new Float32Array([0, 0.25, -0.5, 0]), new Float32Array([0.1, 0, 0, 1])],
      2,
    );
    expect(peaks).toEqual([0.25, 1]);
  });

  it("reports overlaps and source overruns", () => {
    const state = project();
    state.clips[1].start = 5;
    state.clips[0].sourceDuration = 4;
    const codes = validateTimeline(state).map((issue) => issue.code);
    expect(codes).toContain("overlap");
    expect(codes).toContain("source-overrun");
  });

  it("exports a JSON-safe millisecond render contract", () => {
    const state = project();
    state.clips[0].noiseReduction = {
      engine: "rnnoise",
      model: "rnnoise-bd-v1",
      strength: 0.72,
      highPassHz: 80,
      humFrequency: 50,
    };
    const snapshot = serializeProjectState(state);
    const parsed = JSON.parse(JSON.stringify(snapshot));
    expect(parsed.timebase).toBe("milliseconds");
    expect(parsed.clips[0].startMs).toBe(2000);
    expect(parsed.clips[0].durationMs).toBe(4000);
    expect(parsed.clips[0].trimInMs).toBe(1000);
    expect(parsed.clips[0].path).toBe("one.mp4");
    expect(parsed.clips[0].noiseReduction).toEqual({
      engine: "rnnoise",
      model: "rnnoise-bd-v1",
      strength: 1,
      highPassHz: 70,
      humFrequency: 0,
    });
    expect(parsed.tracks[0]).toMatchObject({ muted: false, gain: 1, pan: 0 });
  });

  it("migrates legacy cleanup controls to automatic mode and preserves it across a split", () => {
    const state = project();
    state.clips[0].noiseReduction = {
      engine: "rnnoise",
      model: "rnnoise-bd-v1",
      strength: 4,
      highPassHz: 9,
      humFrequency: 60,
    };
    const normalized = normalizeProjectState(state);
    expect(normalized.clips[0].noiseReduction).toMatchObject({
      strength: 1,
      highPassHz: 70,
      humFrequency: 0,
    });
    const split = splitClip(normalized, "one", 4, "one-b");
    expect(split.state.clips[0].noiseReduction).toEqual(
      split.state.clips[1].noiseReduction,
    );
    expect(split.state.clips[0].noiseReduction).not.toBe(
      split.state.clips[1].noiseReduction,
    );
  });

  it("preserves source-time beat metadata across split and JSON round trips", () => {
    const state = project();
    state.clips[0].beatAnalysis = {
      version: 1,
      model: "beat-this-test",
      sourcePath: state.clips[0].file,
      sourceFingerprint: "fixture",
      analyzedStartMs: 0,
      analyzedDurationMs: 10_000,
      bpm: 120,
      markers: [
        { sourceMs: 2_000, confidence: 0.9, downbeat: true },
        { sourceMs: 4_000, confidence: 0.8, downbeat: false },
      ],
    };
    const split = splitClip(state, "one", 4, "one-b");
    expect(split.state.clips[0].beatAnalysis).toEqual(
      split.state.clips[1].beatAnalysis,
    );
    expect(split.state.clips[0].beatAnalysis).not.toBe(
      split.state.clips[1].beatAnalysis,
    );
    const restored = normalizeProjectState(
      JSON.parse(JSON.stringify(serializeProjectState(split.state))),
    );
    expect(restored.clips[1].beatAnalysis?.markers).toHaveLength(2);
  });

  it("accepts serialized timeline snapshots and rejects structurally unsafe project data", () => {
    const snapshot = serializeProjectState(project());
    expect(isTimelineProjectSnapshot(snapshot)).toBe(true);

    expect(
      isTimelineProjectSnapshot({ ...snapshot, timebase: "seconds" }),
    ).toBe(false);
    expect(
      isTimelineProjectSnapshot({
        ...snapshot,
        tracks: [...snapshot.tracks, { ...snapshot.tracks[0] }],
      }),
    ).toBe(false);
    expect(
      isTimelineProjectSnapshot({
        ...snapshot,
        clips: [{ ...snapshot.clips[0], trackId: "missing-track" }],
      }),
    ).toBe(false);
    expect(
      isTimelineProjectSnapshot({
        ...snapshot,
        clips: [{ ...snapshot.clips[0], start: Number.NaN, startMs: Number.NaN }],
      }),
    ).toBe(false);
  });

  it("normalizes transition types and keeps the two envelopes inside the clip", () => {
    const clip = createClip({
      id: "transition-safe",
      trackId: "v1",
      kind: "video",
      start: 0,
      duration: 4,
      transition: {
        in: { type: "crossfade", duration: 3 },
        out: { type: "wipe-left", duration: 3 },
      },
    });
    expect(clip.transition.in.duration).toBe(3);
    expect(clip.transition.out.duration).toBe(1);

    const unknown = createClip({
      id: "transition-unknown",
      trackId: "v1",
      kind: "video",
      start: 0,
      duration: 4,
      transition: {
        in: { type: "not-a-transition" as never, duration: 1 },
        out: { type: "none", duration: 5 },
      },
    });
    expect(unknown.transition).toEqual({
      in: { type: "none", duration: 0 },
      out: { type: "none", duration: 0 },
    });
  });

  it("reclamps transition envelopes after trim, speed and split edits", () => {
    const trimmedState = project();
    trimmedState.clips[0].transition = {
      in: { type: "crossfade", duration: 1.5 },
      out: { type: "wipe-left", duration: 1.5 },
    };
    const trimmed = trimClip(trimmedState, "one", "end", -2).state.clips[0];
    expect(trimmed.duration).toBe(2);
    expect(trimmed.transition.in.duration + trimmed.transition.out.duration).toBe(2);

    const spedState = project();
    spedState.clips[0].transition = {
      in: { type: "crossfade", duration: 1.5 },
      out: { type: "wipe-left", duration: 1.5 },
    };
    const sped = setClipSpeed(spedState, "one", 4, "source").state.clips[0];
    expect(sped.duration).toBe(1);
    expect(sped.transition.in.duration + sped.transition.out.duration).toBe(1);

    const splitState = project();
    splitState.clips[0].transition = {
      in: { type: "crossfade", duration: 1 },
      out: { type: "wipe-left", duration: 3 },
    };
    const split = splitClip(splitState, "one", 5, "transition-second").state;
    const first = split.clips.find((clip) => clip.id === "one")!;
    const second = split.clips.find((clip) => clip.id === "transition-second")!;
    expect(first.transition.out).toEqual({ type: "none", duration: 0 });
    expect(second.duration).toBe(1);
    expect(second.transition.out.duration).toBe(1);
    expect(validateTimeline(split).filter((issue) => issue.code.startsWith("transition"))).toEqual([]);
  });
});
