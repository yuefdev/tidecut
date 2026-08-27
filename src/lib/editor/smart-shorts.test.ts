import { describe, expect, it } from "vitest";
import {
  discardRangesOutsideHighlights,
  extractLoudnessEnergyEvents,
  extractWaveformEnergyEvents,
  fuseNeuralLaughterEvents,
  mergeExternalSmartShortsRankings,
  planSmartShorts,
  type SmartShortsInput,
} from "./smart-shorts";

function spokenInput(durationMs = 90_000): SmartShortsInput {
  const words = Array.from({ length: 180 }, (_, index) => ({
    startMs: index * 450,
    endMs: index * 450 + 320,
    text: index === 70 ? "İnanılmaz!" : `kelime${index}`,
    confidence: 0.9,
  }));
  return {
    durationMs,
    words,
    speechSegments: [
      { startMs: 0, endMs: 38_000, confidence: 0.92 },
      { startMs: 41_000, endMs: 84_000, confidence: 0.9 },
    ],
    beats: Array.from({ length: 30 }, (_, index) => ({
      atMs: 30_000 + index * 1_000,
      confidence: 0.82,
      downbeat: index % 4 === 0,
    })),
  };
}

describe("smart shorts planner", () => {
  it("returns bounded, non-overlapping ranked candidates", () => {
    const candidates = planSmartShorts(spokenInput(), {
      targetDurationMs: 25_000,
      minDurationMs: 15_000,
      maxDurationMs: 35_000,
      maxCandidates: 3,
    });
    expect(candidates.length).toBeGreaterThan(0);
    expect(candidates.length).toBeLessThanOrEqual(3);
    for (const candidate of candidates) {
      expect(candidate.startMs).toBeGreaterThanOrEqual(0);
      expect(candidate.endMs).toBeLessThanOrEqual(90_000);
      expect(candidate.endMs - candidate.startMs).toBeGreaterThanOrEqual(15_000);
      expect(candidate.endMs - candidate.startMs).toBeLessThanOrEqual(35_000);
      expect(candidate.score).toBeGreaterThanOrEqual(0);
      expect(candidate.score).toBeLessThanOrEqual(100);
    }
    for (let left = 0; left < candidates.length; left += 1) {
      for (let right = left + 1; right < candidates.length; right += 1) {
        const intersection = Math.max(
          0,
          Math.min(candidates[left].endMs, candidates[right].endMs) -
            Math.max(candidates[left].startMs, candidates[right].startMs),
        );
        const shorter = Math.min(
          candidates[left].endMs - candidates[left].startMs,
          candidates[right].endMs - candidates[right].startMs,
        );
        expect(intersection / shorter).toBeLessThan(0.45);
      }
    }
  });

  it("promotes a strong local laughter signal and explains it honestly", () => {
    const input = spokenInput();
    input.audioEvents = [{ atMs: 62_000, score: 0.96, kind: "laughter" }];
    const candidates = planSmartShorts(input, { maxCandidates: 4 });
    expect(candidates[0].startMs).toBeLessThanOrEqual(62_000);
    expect(candidates[0].endMs).toBeGreaterThanOrEqual(62_000);
    expect(candidates[0].reasons).toContain("Kahkaha/gülme olayı");
  });

  it("does not invent highlights without timestamped speech", () => {
    expect(
      planSmartShorts({ durationMs: 60_000, words: [], speechSegments: [] }),
    ).toEqual([]);
  });

  it("turns local waveform bursts into honest energy events", () => {
    const events = extractWaveformEnergyEvents(
      [0.08, 0.1, 0.12, 0.92, 0.15, 0.11, 0.78, 0.1, 0.09, 0.13],
      10_000,
    );
    expect(events.length).toBeGreaterThan(0);
    expect(events.every((event) => event.kind === "energy")).toBe(true);
    expect(events.some((event) => event.atMs >= 3_000 && event.atMs <= 4_000)).toBe(true);
  });

  it("extracts timestamped rises from real momentary loudness points", () => {
    const levels = [-31, -30, -29, -14, -28, -27, -13, -29, -30, -28];
    const events = extractLoudnessEnergyEvents(
      levels.map((loudnessDb, index) => ({ atMs: index * 1_000, loudnessDb })),
    );
    expect(events.map((event) => event.atMs)).toEqual([3_000, 6_000]);
    expect(events.every((event) => event.kind === "energy")).toBe(true);
  });

  it("lets energy and Whisper support neural laughter without inventing it", () => {
    const energy = [{ atMs: 1_500, score: 0.9, kind: "energy" as const }];
    const words = [{
      startMs: 1_100,
      endMs: 1_700,
      text: "[LAUGHTER]",
      confidence: 0.8,
    }];
    expect(fuseNeuralLaughterEvents([], energy, words)).toEqual([]);
    const [event] = fuseNeuralLaughterEvents(
      [{ startMs: 1_000, endMs: 1_960, score: 0.5, kind: "laughter" }],
      energy,
      words,
    );
    expect(event).toEqual({ atMs: 1_480, score: 0.632, kind: "laughter" });
  });

  it("lets an external judge reorder only existing local candidates", () => {
    const candidates = planSmartShorts(spokenInput(), { maxCandidates: 3 });
    const last = candidates.at(-1)!;
    const merged = mergeExternalSmartShortsRankings(candidates, [
      {
        candidateId: last.id,
        score: 100,
        reason: "Net bir payoff var",
        focusTarget: "konuşan kişi",
      },
      { candidateId: "invented", score: 100, reason: "uydurma" },
    ]);
    expect(merged).toHaveLength(candidates.length);
    expect(merged.find((candidate) => candidate.id === last.id)?.reasons[0]).toContain("GLM notu");
    expect(merged.some((candidate) => candidate.id === "invented")).toBe(false);
  });

  it("converts selected highlights into discard ranges for one undoable ripple edit", () => {
    expect(
      discardRangesOutsideHighlights(100_000, [
        { startMs: 10_000, endMs: 30_000 },
        { startMs: 25_000, endMs: 40_000 },
        { startMs: 70_000, endMs: 90_000 },
      ]),
    ).toEqual([
      { startMs: 0, endMs: 10_000 },
      { startMs: 40_000, endMs: 70_000 },
      { startMs: 90_000, endMs: 100_000 },
    ]);
  });
});
