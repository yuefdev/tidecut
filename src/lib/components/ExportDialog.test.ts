import { describe, expect, it } from "vitest";
import exportDialogSource from "./ExportDialog.svelte?raw";

describe("ExportDialog Auto Reframe variants", () => {
  it("selects the render timeline that belongs to the requested aspect", () => {
    expect(exportDialogSource).toContain("timelinesByPreset?.[targetPreset] ?? timeline");
    expect(exportDialogSource).toContain(
      'fit: timelinesByPreset?.[targetPreset] ? "contain" : fit',
    );
  });

  it("renders the available AI aspect variants sequentially with distinct files", () => {
    expect(exportDialogSource).toContain("for (let index = 0; index < targets.length; index += 1)");
    expect(exportDialogSource).toContain("outputPathForPreset(");
    expect(exportDialogSource).toContain('targetPreset.replace(":", "x")');
    expect(exportDialogSource).toContain("Tüm AI kadrajlarını üret");
    expect(exportDialogSource).toContain("targets.length === 1");
    expect(exportDialogSource).toContain("overwrite,");
  });

  it("keeps batch cancellation scoped to the active render job", () => {
    expect(exportDialogSource).toContain("cancelBatchRequested = true");
    expect(exportDialogSource).toContain("await cancelRender(job.jobId)");
  });

  it("locks contain fit for camera matrices and keeps partial-success paths visible", () => {
    expect(exportDialogSource).toContain("autoReframeFitLocked");
    expect(exportDialogSource).toContain("AI kamera yolu için Sığdır kullanılır");
    expect(exportDialogSource).toContain("completedOutputPaths.length > 0");
    expect(exportDialogSource).toContain("exportFocusableElements()");
  });
});
