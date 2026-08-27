import { describe, expect, it } from "vitest";
import {
  buildAutoDuckingEnvelope,
  createSpeechAnalysisMetadata,
  DEFAULT_AUTO_DUCKING,
  evaluateDuckingEnvelope,
  getAutoDuckingDiagnostics,
  normalizeAutoDucking,
  normalizeSpeechAnalysis,
  projectSpeechAnalysis,
  speechAnalysisSignature,
  type AutoDuckingSettings,
  type DuckingClipLike,
  type DuckingProjectLike,
} from "./audio-ducking";

function settings(overrides: Partial<AutoDuckingSettings> = {}): AutoDuckingSettings {
  return { ...DEFAULT_AUTO_DUCKING, sourceTrackIds: ["voice"], ...overrides };
}

function clip(
  input: Partial<DuckingClipLike> & Pick<DuckingClipLike, "id" | "trackId">,
): DuckingClipLike {
  return {
    kind: "audio",
    file: `${input.id}.wav`,
    start: 0,
    duration: 6,
    trimIn: 0,
    speed: 1,
    volume: 1,
    audioSeparated: false,
    speechAnalysis: null,
    autoDucking: null,
    ...input,
  };
}

function analyzed(source: DuckingClipLike, start = 1, end = 2, confidence = 0.9) {
  return {
    ...source,
    speechAnalysis: createSpeechAnalysisMetadata(
      [{ start, end, confidence }],
      speechAnalysisSignature(source),
      source.duration,
    ),
  };
}

function project(source: DuckingClipLike, target: DuckingClipLike): DuckingProjectLike {
  return {
    tracks: [
      { id: "voice", type: "audio", muted: false, solo: false, gain: 1 },
      { id: "music", type: "audio", muted: false, solo: false, gain: 1 },
    ],
    clips: [source, target],
  };
}

describe("AI music ducking", () => {
  it("normalizes bounded settings without duplicate source tracks", () => {
    expect(normalizeAutoDucking({
      version: 99 as 1,
      enabled: true,
      sourceTrackIds: [" voice ", "voice", ""],
      reductionDb: -99,
      lookaheadMs: 999,
      attackMs: 0,
      holdMs: -20,
      releaseMs: 9_000,
      minConfidence: 2,
    })).toEqual({
      version: 1,
      enabled: true,
      sourceTrackIds: ["voice"],
      reductionDb: -30,
      lookaheadMs: 500,
      attackMs: 10,
      holdMs: 0,
      releaseMs: 3_000,
      minConfidence: 0.95,
    });
  });

  it("creates a deterministic attack, hold and release envelope in timeline time", () => {
    const source = analyzed(clip({ id: "narration", trackId: "voice", start: 2 }));
    const target = clip({
      id: "bed",
      trackId: "music",
      autoDucking: settings(),
    });
    const envelope = buildAutoDuckingEnvelope(project(source, target), target);
    const depth = Math.pow(10, -14 / 20);

    expect(evaluateDuckingEnvelope(envelope, 2.82)).toBeCloseTo(1, 6);
    expect(evaluateDuckingEnvelope(envelope, 2.92)).toBeCloseTo(depth, 6);
    expect(evaluateDuckingEnvelope(envelope, 4.16)).toBeCloseTo(depth, 6);
    expect(evaluateDuckingEnvelope(envelope, 4.81)).toBeCloseTo(1, 6);
  });

  it("uses the minimum once for overlapping speakers instead of multiplying cuts", () => {
    const first = analyzed(clip({ id: "a", trackId: "voice", start: 0 }));
    const second = analyzed(clip({ id: "b", trackId: "voice-2", start: 0 }));
    const target = clip({
      id: "bed",
      trackId: "music",
      autoDucking: settings({ sourceTrackIds: ["voice", "voice-2"] }),
    });
    const state: DuckingProjectLike = {
      tracks: [
        { id: "voice", type: "audio", gain: 1 },
        { id: "voice-2", type: "audio", gain: 1 },
        { id: "music", type: "audio", gain: 1 },
      ],
      clips: [first, second, target],
    };
    const value = evaluateDuckingEnvelope(buildAutoDuckingEnvelope(state, target), 1.2);
    expect(value).toBeCloseTo(Math.pow(10, -14 / 20), 6);
  });

  it("reprojects cached local speech when a source moves and ignores muted sources", () => {
    const initial = analyzed(clip({ id: "narration", trackId: "voice", start: 0 }));
    const target = clip({ id: "bed", trackId: "music", autoDucking: settings() });
    const before = buildAutoDuckingEnvelope(project(initial, target), target);
    const moved = { ...initial, start: 2 };
    const after = buildAutoDuckingEnvelope(project(moved, target), target);

    expect(evaluateDuckingEnvelope(before, 1.2)).toBeLessThan(0.3);
    expect(evaluateDuckingEnvelope(after, 1.2)).toBeCloseTo(1, 6);
    expect(evaluateDuckingEnvelope(after, 3.2)).toBeLessThan(0.3);

    const muted = project(moved, target);
    muted.tracks[0].muted = true;
    expect(buildAutoDuckingEnvelope(muted, target)).toEqual([]);
  });

  it("fails stale source analysis open and reports it for export preflight", () => {
    const source = analyzed(clip({ id: "narration", trackId: "voice" }));
    const stale = { ...source, speed: 1.5 };
    const target = clip({ id: "bed", trackId: "music", autoDucking: settings() });
    const state = project(stale, target);

    expect(buildAutoDuckingEnvelope(state, target)).toEqual([]);
    expect(getAutoDuckingDiagnostics(state, target)).toMatchObject({
      configured: true,
      enabled: true,
      validSourceClipCount: 0,
      staleSourceClipIds: ["narration"],
    });
  });

  it("ignores stale clips outside the target's ducking influence window", () => {
    const nearby = analyzed(clip({ id: "nearby", trackId: "voice", start: 1 }));
    const distant = clip({ id: "distant", trackId: "voice", start: 60 });
    const target = clip({ id: "bed", trackId: "music", autoDucking: settings() });
    const base = project(nearby, target);
    const state: DuckingProjectLike = { ...base, clips: [...base.clips, distant] };

    expect(getAutoDuckingDiagnostics(state, target)).toMatchObject({
      selectedSourceClipCount: 1,
      validSourceClipCount: 1,
      staleSourceClipIds: [],
    });
  });

  it("still reports stale clips that can influence the target", () => {
    const stale = clip({ id: "nearby-stale", trackId: "voice", start: 1 });
    const target = clip({ id: "bed", trackId: "music", autoDucking: settings() });

    expect(getAutoDuckingDiagnostics(project(stale, target), target).staleSourceClipIds)
      .toEqual(["nearby-stale"]);
  });

  it("projects split speech metadata and validates its new signatures", () => {
    const source = clip({ id: "narration", trackId: "voice", duration: 6 });
    const metadata = createSpeechAnalysisMetadata(
      [
        { start: 1, end: 3, confidence: 0.9 },
        { start: 4, end: 5, confidence: 0.8 },
      ],
      speechAnalysisSignature(source),
      source.duration,
    );
    const first = { ...source, duration: 2 };
    const second = { ...source, duration: 4, trimIn: 2 };
    const firstAnalysis = projectSpeechAnalysis(
      metadata,
      0,
      2,
      speechAnalysisSignature(first),
    );
    const secondAnalysis = projectSpeechAnalysis(
      metadata,
      2,
      6,
      speechAnalysisSignature(second),
    );

    expect(firstAnalysis?.segments).toEqual([
      { start: 1, end: 2, confidence: 0.9 },
    ]);
    expect(secondAnalysis?.segments).toEqual([
      { start: 0, end: 1, confidence: 0.9 },
      { start: 2, end: 3, confidence: 0.8 },
    ]);
    expect(normalizeSpeechAnalysis(firstAnalysis, first)).not.toBeNull();
    expect(normalizeSpeechAnalysis(secondAnalysis, second)).not.toBeNull();
  });
});
