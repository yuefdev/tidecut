import { describe, expect, it } from "vitest";
import autoReframeDialogSource from "./AutoReframeDialog.svelte?raw";

describe("AutoReframeDialog", () => {
  it("exposes a controlled local-analysis contract with normalized target bounds", () => {
    expect(autoReframeDialogSource).toContain("export interface NormalizedBoundingBox");
    expect(autoReframeDialogSource).toContain("export interface AutoReframeDialogProps");
    expect(autoReframeDialogSource).toContain("targetBox?: NormalizedBoundingBox | null");
    expect(autoReframeDialogSource).toContain("seedFrameUrl: string");
    expect(autoReframeDialogSource).toContain("sourceWidth: number");
    expect(autoReframeDialogSource).toContain("sourceHeight: number");
    expect(autoReframeDialogSource).toContain("durationMs: number");
    expect(autoReframeDialogSource).toContain("seedTimeMs?: number");
    expect(autoReframeDialogSource).toContain("busy?: boolean");
    expect(autoReframeDialogSource).toContain("onTargetBoxChange?: (box: NormalizedBoundingBox | null) => void");
    expect(autoReframeDialogSource).toContain("onAnalyze: (request: AutoReframeRequest)");
    expect(autoReframeDialogSource).toContain("onApply: (payload: AutoReframeApplyPayload)");
    expect(autoReframeDialogSource).toContain("onCancel: () => void | Promise<void>");
  });

  it("supports pointer and keyboard target selection on the real seed frame", () => {
    expect(autoReframeDialogSource).toContain("src={seedFrameUrl}");
    expect(autoReframeDialogSource).toContain("onpointerdown={beginTargetDrag}");
    expect(autoReframeDialogSource).toContain("onpointermove={updateTargetDrag}");
    expect(autoReframeDialogSource).toContain("onpointerup={finishTargetDrag}");
    expect(autoReframeDialogSource).toContain("getBoundingClientRect()");
    expect(autoReframeDialogSource).toContain("(event.clientX - bounds.left) / bounds.width");
    expect(autoReframeDialogSource).toContain("handleFrameKeydown");
    expect(autoReframeDialogSource).toContain('event.key === "Enter"');
    expect(autoReframeDialogSource).toContain('event.key === "Delete"');
    expect(autoReframeDialogSource).toContain("targetOverlayStyle");
  });

  it("offers all output ratios, movement styles, and person/object framings", () => {
    for (const aspect of ['id: "9:16"', 'id: "1:1"', 'id: "16:9"']) {
      expect(autoReframeDialogSource).toContain(aspect);
    }
    for (const style of ['id: "calm"', 'id: "natural"', 'id: "dynamic"']) {
      expect(autoReframeDialogSource).toContain(style);
    }
    for (const framing of [
      'id: "auto"',
      'id: "close"',
      'id: "medium"',
      'id: "full"',
      'id: "object"',
    ]) {
      expect(autoReframeDialogSource).toContain(framing);
    }
    expect(autoReframeDialogSource).toContain("En az bir çıktı oranı seçin.");
    expect(autoReframeDialogSource).toContain("Ücretsiz yerel analiz");
  });

  it("renders setup, progress, errors, and the low-confidence result summary", () => {
    expect(autoReframeDialogSource).toContain('export type AutoReframeAnalysisStatus = "idle" | "preparing" | "analyzing" | "completed" | "error"');
    expect(autoReframeDialogSource).toContain('role="progressbar"');
    expect(autoReframeDialogSource).toContain('aria-label="Akıllı kadraj analiz ilerlemesi"');
    expect(autoReframeDialogSource).toContain('class="error-card" role="alert"');
    expect(autoReframeDialogSource).toContain("lowConfidenceCount");
    expect(autoReframeDialogSource).toContain("DÜŞÜK GÜVEN");
    expect(autoReframeDialogSource).toContain("Zaman çizelgesine uygula");
  });

  it("is modal, traps/restores focus, handles Escape, and can detach a busy analysis", () => {
    expect(autoReframeDialogSource).toContain('role="dialog"');
    expect(autoReframeDialogSource).toContain('aria-modal="true"');
    expect(autoReframeDialogSource).toContain("handleWindowKeydown");
    expect(autoReframeDialogSource).toContain('event.key === "Escape"');
    expect(autoReframeDialogSource).toContain('event.key !== "Tab"');
    expect(autoReframeDialogSource).toContain("focusableElements()");
    expect(autoReframeDialogSource).toContain("previouslyFocused");
    expect(autoReframeDialogSource).toContain("Pencereyi kapatırsanız analiz arka planda sürer");
    expect(autoReframeDialogSource).toContain("Arka planda sürdür");
    expect(autoReframeDialogSource).toContain("if (isBusy || analysisStatus !== \"completed\") return");
    expect(autoReframeDialogSource).toContain("busy || externalAnalysisBusy");
  });

  it("passes source metadata and chosen settings to analysis and apply callbacks", () => {
    expect(autoReframeDialogSource).toMatch(
      /return \{[\s\S]*?targetBox: \{ \.\.\.currentTarget \}[\s\S]*?aspects: \[\.\.\.selectedAspectIds\][\s\S]*?style: framingStyle[\s\S]*?framing: subjectFraming/,
    );
    expect(autoReframeDialogSource).toContain("width: Math.max(1, Math.round(sourceWidth))");
    expect(autoReframeDialogSource).toContain("height: Math.max(1, Math.round(sourceHeight))");
    expect(autoReframeDialogSource).toContain("durationMs: Math.max(1, Math.round(durationMs))");
    expect(autoReframeDialogSource).toContain("seedTimeMs: clamp(Math.round(seedTimeMs)");
    expect(autoReframeDialogSource).toContain("await onAnalyze(request)");
    expect(autoReframeDialogSource).toContain("await onApply({ request, result })");
  });
});
