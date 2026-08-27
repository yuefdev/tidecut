import { describe, expect, it } from "vitest";
import {
  createClip,
  createTextClip,
  createTrack,
  serializeProjectState,
} from "$lib/editor/timeline-engine";
import { replaceTimelineMediaPaths, toRenderTimeline } from "./timeline-adapter";
import {
  createSpeechAnalysisMetadata,
  speechAnalysisSignature,
} from "$lib/editor/audio-ducking";

describe("timeline render adapter", () => {
  it("converts tracks, transforms, transitions and merged keyframes", () => {
    const clip = createClip({
      id: "video-1",
      trackId: "v1",
      file: "C:/media/source.mp4",
      kind: "video",
      start: 1.25,
      duration: 3.5,
      trimIn: 2,
      speed: 1.5,
      pan: 0.35,
      transform: { x: 12, y: -4, scale: 1.2, rotation: 8, opacity: 0.8 },
      transition: {
        in: { type: "crossfade", duration: 0.5 },
        out: { type: "dip-to-black", duration: 0.25 },
      },
      keyframes: {
        x: [{ time: 0, value: 10, easing: "linear" }],
        scale: [{ time: 0, value: 1.1, easing: "ease-in-out" }],
        pan: [{ time: 0, value: -0.4, easing: "hold" }],
        riderGain: [
          { time: 0, value: 0.65, easing: "ease-in-out" },
          { time: 1, value: 0.9, easing: "linear" },
        ],
      },
      // createClip drops riderGain keyframes unless Voice Rider metadata
      // marks the clip as processed (and an envelope needs 2+ frames); the
      // adapter must then pass them through to the export graph.
      voiceRider: {
        model: "silero-vad-v6",
        speechCoverage: 0.8,
        averageSpeechProbability: 0.9,
        strongestCutDb: -6,
        strongestBoostDb: 3,
      },
      noiseReduction: {
        engine: "rnnoise",
        model: "rnnoise-bd-v1",
        strength: 0.72,
        highPassHz: 80,
        humFrequency: 50,
      },
    });
    const snapshot = serializeProjectState({
      version: 2,
      tracks: [
        {
          ...createTrack("v1", "Video", "video"),
          muted: true,
          gain: 0.75,
          pan: -0.2,
        },
      ],
      clips: [clip],
      selectedClipId: clip.id,
    });

    const graph = toRenderTimeline(snapshot);
    expect(graph.tracks[0]).toMatchObject({
      id: "v1",
      kind: "video",
      mute: true,
      gain: 0.75,
      pan: -0.2,
    });
    expect(graph.clips[0]).toMatchObject({
      startMs: 1250,
      durationMs: 3500,
      trimInMs: 2000,
      speed: 1.5,
      pan: 0.35,
      noiseReduction: {
        engine: "rnnoise",
        model: "rnnoise-bd-v1",
        strength: 1,
        highPassHz: 70,
        humFrequency: 0,
      },
      transform: { scaleX: 1.2, scaleY: 1.2, rotationDeg: 8 },
      transition: { kind: "dissolve", inDurationMs: 500, outDurationMs: 250 },
    });
    expect(graph.clips[0].keyframes).toEqual([
      {
        atMs: 0,
        x: 10,
        scaleX: 1.1,
        scaleY: 1.1,
        pan: -0.4,
        riderGain: 0.65,
        easing: {
          x: "linear",
          scaleX: "ease-in-out",
          scaleY: "ease-in-out",
          pan: "hold",
          riderGain: "ease-in-out",
        },
      },
      {
        atMs: 1000,
        riderGain: 0.9,
        easing: { riderGain: "linear" },
      },
    ]);
  });

  it("serializes text without a media path and relinks media immutably", () => {
    const video = createClip({
      id: "v",
      trackId: "v1",
      file: "D:/missing.mp4",
      kind: "video",
      start: 0,
      duration: 4,
    });
    const baseText = createTextClip({ id: "t", trackId: "t1", start: 0, duration: 2 });
    const text = {
      ...baseText,
      text: {
        ...baseText.text!,
        content: "L'été\nİkinci satır",
        fontFamily: "Source Sans 3",
        fontSize: 84,
        fontWeight: 700,
        color: "#f4f7ffff",
        backgroundColor: "#112233cc",
        align: "right" as const,
      },
    };
    const snapshot = serializeProjectState({
      version: 2,
      tracks: [createTrack("v1", "V1", "video"), createTrack("t1", "T1", "text")],
      clips: [video, text],
      selectedClipId: null,
    });

    const graph = toRenderTimeline(snapshot);
    const renderedText = graph.clips.find((clip) => clip.id === "t");
    expect(renderedText?.path).toBeUndefined();
    expect(renderedText?.text).toEqual({
      content: "L'été\nİkinci satır",
      fontFamily: "Source Sans 3",
      fontSize: 84,
      fontWeight: 700,
      color: "#f4f7ffff",
      backgroundColor: "#112233cc",
      align: "right",
      strokeColor: "#000000",
      strokeWidth: 0,
      shadowColor: "transparent",
      shadowOffsetX: 0,
      shadowOffsetY: 0,
      animationInKind: "none",
      animationInMs: 600,
      animationOutKind: "none",
      animationOutMs: 600,
    });
    const relinked = replaceTimelineMediaPaths(
      snapshot,
      new Map([["D:/missing.mp4", "E:/found.mp4"]]),
    );
    expect(relinked.clips.find((clip) => clip.id === "v")?.file).toBe("E:/found.mp4");
    expect(snapshot.clips.find((clip) => clip.id === "v")?.file).toBe("D:/missing.mp4");
  });

  it("projects the shared ducking envelope into render-only keyframes", () => {
    const narration = createClip({
      id: "voice",
      trackId: "voice-track",
      kind: "audio",
      file: "C:/media/voice.wav",
      start: 0,
      duration: 4,
    });
    narration.speechAnalysis = createSpeechAnalysisMetadata(
      [{ start: 1, end: 2, confidence: 0.9 }],
      speechAnalysisSignature(narration),
      narration.duration,
    );
    const music = createClip({
      id: "music",
      trackId: "music-track",
      kind: "audio",
      file: "C:/media/music.wav",
      start: 0,
      duration: 4,
      autoDucking: {
        version: 1,
        enabled: true,
        sourceTrackIds: ["voice-track"],
        reductionDb: -6,
        lookaheadMs: 0,
        attackMs: 100,
        holdMs: 0,
        releaseMs: 100,
        minConfidence: 0.5,
      },
    });
    const snapshot = serializeProjectState({
      version: 2,
      tracks: [
        createTrack("voice-track", "Voice", "audio"),
        createTrack("music-track", "Music", "audio"),
      ],
      clips: [narration, music],
      selectedClipId: music.id,
    });
    const graph = toRenderTimeline(snapshot);
    const renderMusic = graph.clips.find((clip) => clip.id === music.id)!;
    const fullDuck = renderMusic.keyframes?.find((frame) => frame.atMs === 1_000);

    expect(fullDuck?.duckGain).toBeCloseTo(Math.pow(10, -6 / 20), 6);
    expect(fullDuck?.easing?.duckGain).toBe("linear");
    expect(renderMusic.keyframes?.find((frame) => frame.atMs === 900)?.duckGain)
      .toBeCloseTo(1, 6);
    expect(snapshot.clips.find((clip) => clip.id === music.id)?.keyframes)
      .not.toHaveProperty("duckGain");
  });
});
