import { describe, expect, it } from "vitest";
import audioExportDialogSource from "./AudioExportDialog.svelte?raw";

describe("AudioExportDialog", () => {
  it("offers standard standalone audio formats and a timeline insertion choice", () => {
    expect(audioExportDialogSource).toContain('id: "mp3"');
    expect(audioExportDialogSource).toContain('id: "m4a"');
    expect(audioExportDialogSource).toContain('id: "wav"');
    expect(audioExportDialogSource).toContain(
      "Ses parçası olarak zaman çizelgesine ekle",
    );
  });

  it("supports source-range probing and exports the selected source interval", () => {
    expect(audioExportDialogSource).toContain("probeMediaDuration");
    expect(audioExportDialogSource).toContain("startMs: sourceOffsetMs + rangeStartMs");
    expect(audioExportDialogSource).toContain("durationMs: selectedDurationMs");
    expect(audioExportDialogSource).toContain("startAudioExportWithListener");
    expect(audioExportDialogSource).toContain("onCompleted?.(snapshot.outputPath, addToTimeline)");
  });

  it("uses two-pass smart loudness analysis by default and forwards its measured pass", () => {
    expect(audioExportDialogSource).toContain("let normalizeAudio = $state(true)");
    expect(audioExportDialogSource).toContain("integratedLufs: -16");
    expect(audioExportDialogSource).toContain("integratedLufs: -14");
    expect(audioExportDialogSource).toContain("integratedLufs: -23");
    expect(audioExportDialogSource).toContain("loudnessAnalysis = await analyzeAudioLoudness({");
    expect(audioExportDialogSource).toContain("startMs: sourceOffsetMs + rangeStartMs");
    expect(audioExportDialogSource).toContain("durationMs: selectedDurationMs");
    expect(audioExportDialogSource).toContain(
      "const effectivePlaybackRate = normalizedPlaybackRate(playbackRate)",
    );
    expect(audioExportDialogSource).toContain("playbackRate: effectivePlaybackRate");
    expect(audioExportDialogSource).toContain("volume: normalizedVolume(volume)");
    expect(audioExportDialogSource).toContain("normalization = loudnessAnalysis.pass");
    expect(audioExportDialogSource).toMatch(
      /startAudioExportWithListener\([\s\S]*?normalization,[\s\S]*?overwrite: true/,
    );
  });

  it("preserves manual and AI gain automation in analysis and standalone export", () => {
    expect(audioExportDialogSource).toContain("volumeKeyframes?: AudioGainKeyframe[]");
    expect(audioExportDialogSource).toContain("riderGainKeyframes?: AudioGainKeyframe[]");
    expect(audioExportDialogSource).toContain(
      "automationStartMs: Math.max(0, Math.round(rangeStartMs / effectivePlaybackRate))",
    );
    expect(audioExportDialogSource.match(/\.\.\.automation,/g)).toHaveLength(2);
    expect(audioExportDialogSource).toContain("AI Voice Rider");
    expect(audioExportDialogSource).toContain("dışa aktarımda korunacak");
  });

  it("prevents overlapping work and ignores a stale analysis result", () => {
    expect(audioExportDialogSource).toContain(
      "let isWorking = $derived(isBusy || analyzingLoudness || preparingCleanup)",
    );
    expect(audioExportDialogSource).toContain(
      "if (!isCurrent(exportInitialization) || sourcePath !== exportSourcePath) return",
    );
    expect(audioExportDialogSource).toContain("if (isWorking || settingUpEngine) return");
  });
});
