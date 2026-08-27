import { describe, expect, it } from "vitest";
import { applyRelinkResolutions } from "./media";
import { createProjectDocument, isProjectDocument } from "./types";

describe("project media helpers", () => {
  it("relinks media pool and opaque timeline paths without mutating input", () => {
    const document = createProjectDocument({
      now: 10,
      projectId: "project-1",
      timeline: {
        clips: [{ file: "D:\\offline\\clip.mp4" }],
      },
    });
    document.media = [
      {
        id: "asset-1",
        sourcePath: "D:\\offline\\clip.mp4",
        fileName: "clip.mp4",
        kind: "video",
        sizeBytes: 42,
        quickHash: "sha256:test",
        availability: "missing",
      },
    ];

    const relinked = applyRelinkResolutions(document, [
      {
        reference: {
          assetId: "asset-1",
          path: "D:/offline/clip.mp4",
        },
        replacementPath: "E:/media/clip.mp4",
        confidence: 100,
        reason: "quick-hash",
      },
    ]);

    expect(relinked.media[0].sourcePath).toBe("E:/media/clip.mp4");
    expect(relinked.media[0].availability).toBe("online");
    expect(relinked.timeline.clips[0].file).toBe("E:/media/clip.mp4");
    expect(document.media[0].sourcePath).toBe("D:\\offline\\clip.mp4");
  });

  it("validates the stable project envelope while leaving timeline generic", () => {
    const document = createProjectDocument({ timeline: ["any", "json"] });
    expect(isProjectDocument(document)).toBe(true);
    expect(isProjectDocument({ ...document, schemaVersion: 99 })).toBe(false);

    const empty = createProjectDocument();
    expect(isProjectDocument(JSON.parse(JSON.stringify(empty)))).toBe(true);
  });
});
