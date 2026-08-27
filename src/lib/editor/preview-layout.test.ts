import { describe, expect, it } from "vitest";
import { fitPreviewDisplaySize } from "./preview-layout";

describe("preview display layout", () => {
  it("uses the full available width while preserving a landscape aspect", () => {
    const size = fitPreviewDisplaySize(1000, 1000, 1920, 1080, 36);

    expect(size.width).toBe(964);
    expect(size.height).toBeCloseTo(542.25);
  });

  it("uses the full available height for portrait projects", () => {
    const size = fitPreviewDisplaySize(1000, 1000, 1080, 1920, 36);

    expect(size.width).toBeCloseTo(542.25);
    expect(size.height).toBe(964);
  });

  it("can grow beyond the canvas bitmap's intrinsic dimensions", () => {
    const size = fitPreviewDisplaySize(4000, 2500, 1920, 1080, 36);

    expect(size.width).toBe(3964);
    expect(size.width).toBeGreaterThan(1920);
    expect(size.height).toBeCloseTo(2229.75);
  });

  it("returns an empty display size until the stage is measurable", () => {
    expect(fitPreviewDisplaySize(0, 0, 1920, 1080, 36)).toEqual({
      width: 0,
      height: 0,
    });
  });
});
