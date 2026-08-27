import { describe, expect, it } from "vitest";
import {
  buildSpeechSuggestions,
  classifyTurkishFiller,
  createBeatAnalysisMetadata,
  isBeatAnalysisRangeCurrent,
  normalizeTurkishToken,
  planBeatCuts,
  projectSourceBeats,
  type TimelineBeatMarker,
} from "./audio-ai";

describe("Turkish speech cleanup suggestions", () => {
  it("uses Turkish casing and strips punctuation", () => {
    expect(normalizeTurkishToken(" İŞTE! ")).toBe("işte");
    expect(normalizeTurkishToken("Iıİi")).toBe("ııii");
  });

  it("separates explicit fillers from contextual words", () => {
    expect(classifyTurkishFiller("eee")).toBe("filler-word");
    expect(classifyTurkishFiller("Iıı...")).toBe("filler-word");
    expect(classifyTurkishFiller("şey")).toBe("contextual-word");
    expect(classifyTurkishFiller("yani,")).toBe("contextual-word");
    expect(classifyTurkishFiller("yarın")).toBeNull();
  });

  it("keeps word edges around internal silence", () => {
    const result = buildSpeechSuggestions(
      5_000,
      [
        { startMs: 100, endMs: 1_000, confidence: 0.9 },
        { startMs: 2_500, endMs: 4_800, confidence: 0.8 },
      ],
      [],
    );
    expect(result.find((item) => item.kind === "silence")).toMatchObject({
      startMs: 1_160,
      endMs: 2_380,
      defaultSelected: true,
    });
  });

  it("never preselects contextual words", () => {
    const result = buildSpeechSuggestions(
      2_000,
      [{ startMs: 0, endMs: 2_000, confidence: 0.9 }],
      [
        { startMs: 200, endMs: 450, text: "eee", confidence: 0.92 },
        { startMs: 900, endMs: 1_200, text: "yani", confidence: 0.99 },
      ],
    );
    expect(result.find((item) => item.text === "eee")?.defaultSelected).toBe(true);
    expect(result.find((item) => item.text === "yani")?.defaultSelected).toBe(false);
  });

  it("labels short VAD islands without words as possible sounds", () => {
    const result = buildSpeechSuggestions(
      2_000,
      [{ startMs: 600, endMs: 1_050, confidence: 0.8 }],
      [],
    );
    expect(result.some((item) => item.kind === "possible-filler-sound")).toBe(true);
  });

  it("never offers the whole clip as a preselected cut when no speech is found", () => {
    const result = buildSpeechSuggestions(10_000, [], []);
    expect(result).toEqual([]);
    expect(result.some((item) => item.defaultSelected)).toBe(false);
  });
});

describe("beat projection and cut planning", () => {
  it("combines beat/downbeat responses into source-time metadata", () => {
    const metadata = createBeatAnalysisMetadata("song.wav", {
      bpm: 120,
      beats: [1_000, 1_500],
      confidences: [0.8, 0.7],
      downbeats: [1_006],
      downbeatConfidences: [0.95],
      analyzedStartMs: 500,
      analyzedDurationMs: 2_000,
      model: "beat-this-test",
      sourceFingerprint: "fingerprint",
      cacheHit: false,
    });
    expect(metadata.markers).toEqual([
      { sourceMs: 1_000, confidence: 0.95, downbeat: true },
      { sourceMs: 1_500, confidence: 0.7, downbeat: false },
    ]);
    expect(
      isBeatAnalysisRangeCurrent(
        { file: "song.wav", start: 0, duration: 1, trimIn: 1, speed: 1 },
        metadata,
      ),
    ).toBe(true);
    expect(
      isBeatAnalysisRangeCurrent(
        { file: "song.wav", start: 0, duration: 3, trimIn: 1, speed: 1 },
        metadata,
      ),
    ).toBe(false);
  });

  it("projects source markers through trim, move and speed", () => {
    const result = projectSourceBeats(
      { start: 5, duration: 4, trimIn: 12, speed: 2 },
      [
        { sourceMs: 11_000, confidence: 1, downbeat: false },
        { sourceMs: 14_000, confidence: 0.8, downbeat: true },
        { sourceMs: 20_000, confidence: 0.7, downbeat: false },
        { sourceMs: 21_000, confidence: 1, downbeat: false },
      ],
    );
    expect(result.map((item) => item.timelineTime)).toEqual([6, 9]);
  });

  it("plans downbeat and every-two-beat cuts without tiny shots", () => {
    const markers: TimelineBeatMarker[] = Array.from({ length: 8 }, (_, index) => ({
      sourceMs: index * 500,
      timelineTime: index * 0.5,
      confidence: 0.9,
      downbeat: index % 4 === 0,
    }));
    expect(
      planBeatCuts(markers, {
        mode: "downbeat",
        rangeStart: 0,
        rangeEnd: 4,
        minShotSeconds: 0.3,
      }),
    ).toEqual([2]);
    expect(
      planBeatCuts(markers, {
        mode: "every-2",
        rangeStart: 0,
        rangeEnd: 4,
        minShotSeconds: 0.3,
      }),
    ).toEqual([1, 2, 3]);
  });

  it("smart mode honors shot bounds and favors downbeats", () => {
    const markers: TimelineBeatMarker[] = Array.from({ length: 20 }, (_, index) => ({
      sourceMs: index * 500,
      timelineTime: index * 0.5,
      confidence: index % 4 === 0 ? 0.95 : 0.55,
      downbeat: index % 4 === 0,
    }));
    const cuts = planBeatCuts(markers, {
      mode: "smart",
      rangeStart: 0,
      rangeEnd: 10,
      minShotSeconds: 1.5,
      maxShotSeconds: 3,
    });
    const boundaries = [0, ...cuts, 10];
    expect(cuts.length).toBeGreaterThan(1);
    for (let index = 1; index < boundaries.length; index += 1) {
      expect(boundaries[index] - boundaries[index - 1]).toBeGreaterThanOrEqual(1.5);
      expect(boundaries[index] - boundaries[index - 1]).toBeLessThanOrEqual(3);
    }
  });
});
