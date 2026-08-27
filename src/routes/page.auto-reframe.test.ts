import { describe, expect, it } from "vitest";
import pageSource from "./+page.svelte?raw";

describe("editor Auto Reframe integration", () => {
  it("persists the plan in settings and includes it in external undo snapshots", () => {
    expect(pageSource).toContain("autoReframe?: AutoReframeProjectState");
    expect(pageSource).toContain("autoReframe: cloneAutoReframeProjectState(autoReframeState)");
    expect(pageSource).toContain("autoReframeState = normalizeAutoReframeProjectState");
    expect(pageSource).toContain("timelineRef?.commitHistory?.()");
  });

  it("maps source tracking into speed-aware camera planning and preview", () => {
    expect(pageSource).toContain("trackerAnalysisToPlannerSamples(analysis, speed");
    expect(pageSource).toContain("baseSampleFps / speed");
    expect(pageSource).toContain("applyAutoReframeToFramePlan(");
    expect(pageSource).toContain("autoReframeRotationSupported(clip)");
  });

  it("routes independent aspect timelines into safe batch export", () => {
    expect(pageSource).toMatch(/applyAutoReframeToSnapshot\(\s*timelineSnapshot/);
    expect(pageSource).toContain("timelinesByPreset={autoReframeTimelinesByPreset}");
    expect(pageSource).toContain("autoReframePresets={autoReframeExportAspects}");
    expect(pageSource).toContain("autoReframeOpen ||");
  });

  it("keeps background analysis visible in a toast and on the source timeline clip", () => {
    expect(pageSource).toContain("autoReframeBackgroundJob = $derived.by");
    expect(pageSource).toContain('class="auto-reframe-background-toast"');
    expect(pageSource).toContain('aria-label="Arka plan akıllı kadraj ilerlemesi"');
    expect(pageSource).toContain("autoReframeJob={autoReframeBackgroundJob}");
    expect(pageSource).toContain("onAutoReframeJobOpen={openAutoReframeBackgroundJob}");
  });
});
