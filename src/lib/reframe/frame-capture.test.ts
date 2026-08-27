import { afterEach, describe, expect, it, vi } from "vitest";
import { captureVideoFrame, clampCaptureTime, fitFrameWithin } from "./frame-capture";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("auto reframe seed frame capture geometry", () => {
  it("preserves aspect ratio while limiting the analysis preview", () => {
    expect(fitFrameWithin(3840, 2160, 1280)).toEqual({ width: 1280, height: 720 });
    expect(fitFrameWithin(720, 1280, 1280)).toEqual({ width: 720, height: 1280 });
    expect(fitFrameWithin(640, 360, 1280)).toEqual({ width: 640, height: 360 });
  });

  it("keeps the requested frame inside the decodable stream range", () => {
    expect(clampCaptureTime(-1, 10)).toBe(0);
    expect(clampCaptureTime(4.25, 10)).toBe(4.25);
    expect(clampCaptureTime(10, 10)).toBeCloseTo(9.999, 6);
    expect(clampCaptureTime(Number.NaN, 10)).toBe(0);
  });

  it("fails closed for invalid frame dimensions", () => {
    expect(() => fitFrameWithin(0, 1080)).toThrow(RangeError);
    expect(() => fitFrameWithin(1920, Number.NaN)).toThrow(RangeError);
  });

  it("requests Tauri asset videos with CORS before assigning the source", async () => {
    const sourceAssignments: Array<{ source: string; crossOrigin: string | null }> = [];
    const video = {
      preload: "",
      muted: false,
      playsInline: false,
      crossOrigin: null as string | null,
      readyState: 2,
      videoWidth: 1920,
      videoHeight: 1080,
      duration: 10,
      currentTime: 0,
      set src(source: string) {
        sourceAssignments.push({ source, crossOrigin: this.crossOrigin });
      },
      pause: vi.fn(),
      removeAttribute: vi.fn(),
      load: vi.fn(),
    };
    const context = {
      imageSmoothingEnabled: false,
      imageSmoothingQuality: "low",
      drawImage: vi.fn(),
    };
    const canvas = {
      width: 0,
      height: 0,
      getContext: vi.fn(() => context),
      toDataURL: vi.fn(() => "data:image/jpeg;base64,frame"),
    };

    vi.stubGlobal("document", {
      createElement: vi.fn((tagName: string) => (tagName === "video" ? video : canvas)),
    });

    await expect(captureVideoFrame("asset://localhost/example.mp4", 0)).resolves.toMatchObject({
      dataUrl: "data:image/jpeg;base64,frame",
      sourceWidth: 1920,
      sourceHeight: 1080,
    });
    expect(sourceAssignments).toEqual([
      { source: "asset://localhost/example.mp4", crossOrigin: "anonymous" },
    ]);
    expect(context.drawImage).toHaveBeenCalledWith(video, 0, 0, 1280, 720);
  });
});
