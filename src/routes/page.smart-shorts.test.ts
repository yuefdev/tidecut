import { describe, expect, it } from "vitest";
import pageSource from "./+page.svelte?raw";
import timelineSource from "$lib/components/Timeline.svelte?raw";

describe("editor Smart Shorts integration", () => {
  it("combines local timestamped speech, rhythm and momentary loudness", () => {
    expect(pageSource).toContain("analyzeClipSpeechActivity(clip)");
    expect(pageSource).toContain('language: "auto"');
    expect(pageSource).toContain("createBeatAnalysisMetadata");
    expect(pageSource).toContain("analyzeAudioEnergy");
    expect(pageSource).toContain("extractLoudnessEnergyEvents");
    expect(pageSource).toContain("extractWaveformEnergyEvents");
    expect(pageSource).toContain("analyzeLaughter");
    expect(pageSource).toContain("fuseNeuralLaughterEvents");
    expect(pageSource).toContain("planSmartShorts(");
  });

  it("keeps Fireworks optional and sends only candidate text evidence", () => {
    expect(pageSource).toContain("checkFireworksSmartShorts()");
    expect(pageSource).toContain("rankSmartShortCandidates(");
    expect(pageSource).toContain("transcript: candidate.transcript");
    expect(pageSource).toContain("evidence: candidate.reasons");
    expect(pageSource).toContain("GLM kullanılamadı; yerel sıralama korundu");
  });

  it("requires review and turns outside ranges into one forced-ripple edit", () => {
    expect(pageSource).toContain("<SmartShortsPanel");
    expect(pageSource).toContain("onCandidateSeek={handleSmartShortsCandidateSeek}");
    expect(pageSource).toContain("discardRangesOutsideHighlights(");
    expect(pageSource).toContain("{ forceRipple: true }");
    expect(timelineSource).toContain("options.forceRipple ?? rippleMode");
  });

  it("gives Smart Shorts a dedicated Inspector workspace", () => {
    const snippetStart = pageSource.indexOf("{#snippet inspectorContent()}");
    const snippetEnd = pageSource.indexOf("\n{/snippet}", snippetStart);
    const inspectorSnippet = pageSource.slice(snippetStart, snippetEnd);
    const shortsBranch = inspectorSnippet.indexOf("{:else if smartShortsStudioOpen}");
    const clipBranch = inspectorSnippet.indexOf("{:else if selectedClip}");

    expect(inspectorSnippet).toContain("Shorts");
    expect(inspectorSnippet).toContain("openSmartShortsStudioPanel");
    expect(inspectorSnippet).toContain('class:shorts-mode={smartShortsStudioOpen}');
    expect(shortsBranch).toBeGreaterThanOrEqual(0);
    expect(clipBranch).toBeGreaterThan(shortsBranch);
    expect(inspectorSnippet.indexOf("<SmartShortsPanel")).toBeGreaterThan(shortsBranch);
    expect(inspectorSnippet.indexOf("<SmartShortsPanel")).toBeLessThan(clipBranch);
    expect(pageSource.match(/<SmartShortsPanel/g) ?? []).toHaveLength(1);
  });

  it("moves advanced audio tools out of the general Clip inspector", () => {
    const snippetStart = pageSource.indexOf("{#snippet inspectorContent()}");
    const snippetEnd = pageSource.indexOf("\n{/snippet}", snippetStart);
    const inspectorSnippet = pageSource.slice(snippetStart, snippetEnd);
    const audioBranch = inspectorSnippet.indexOf("{:else if audioStudioOpen}");
    const clipBranch = inspectorSnippet.indexOf("{:else if selectedClip}");
    expect(inspectorSnippet).toContain("Ses AI");
    expect(inspectorSnippet).toContain("openAudioStudioPanel");
    expect(audioBranch).toBeGreaterThanOrEqual(0);
    expect(clipBranch).toBeGreaterThan(audioBranch);
    expect(inspectorSnippet.indexOf("<AudioEnhancementPanel")).toBeGreaterThan(audioBranch);
    expect(inspectorSnippet.indexOf("<AudioEnhancementPanel")).toBeLessThan(clipBranch);
    expect(pageSource.match(/<AudioEnhancementPanel/g) ?? []).toHaveLength(1);
  });

  it("persists streamer controls and uses the same viewport layout for export", () => {
    expect(pageSource).toContain("setClipStreamerLayout");
    expect(pageSource).toContain("streamerLayoutEnabled={selectedStreamerLayout.enabled}");
    expect(pageSource).toContain("onStreamerLayoutChange={handleStreamerLayoutChange}");
    expect(pageSource).toContain("onSelectStreamerTarget={handleSelectStreamerTarget}");
    expect(pageSource).toContain("applyStreamerLayoutToRenderTimeline(");
  });

  it("labels the actual Whisper engine returned by the backend", () => {
    expect(pageSource).toContain("transcript.engine");
    expect(pageSource).toContain('engineLabel: engineParts.join(" · ")');
  });

  it("keeps listeners recoverable and advances estimated progress without regressions", () => {
    expect(pageSource).toContain("await Promise.allSettled([");
    expect(pageSource).toContain('speechListener.status === "fulfilled"');
    expect(pageSource).toContain('laughterListener.status === "fulfilled"');
    expect(pageSource).toContain("unlisten?.();");
    expect(pageSource).toContain("unlistenLaughter?.();");
    expect(pageSource).toContain("new SmartShortsProgressEstimator(");
    expect(pageSource).toContain("setInterval(syncEstimatedProgress, 400)");
    expect(pageSource).toContain("progressEstimator.observe(percent)");
    expect(pageSource).toContain("Math.max(smartShortsProgress, snapshot.percent)");
    expect(pageSource).toContain("clearInterval(progressTimer)");
    expect(pageSource).toContain("progressRemainingMs={selectedSmartShortsBusy");
  });

  it("uses Smart Shorts-specific fallbacks for non-Error backend failures", () => {
    expect(pageSource).toContain("YAMNet kahkaha analizi tamamlanamadı.");
    expect(pageSource).toContain("Akıllı Shorts analizi tamamlanamadı.");
    expect(pageSource).toContain("GLM sıralaması tamamlanamadı.");
  });
});
