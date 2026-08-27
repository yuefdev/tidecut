import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { normalizeTextStyle, type FramePlan } from "$lib/editor/timeline-engine";
import { CanvasCompositor } from "$lib/editor/media-runtime";
import compositorSource from "./CompositorPlayer.svelte?raw";

class FakeVideoElement {
  videoWidth = 1920;
  videoHeight = 1080;
  frame = "frame";
  drawBehavior: "paint" | "throw" = "paint";
}

class FakeImageElement {}
class FakeSvgImageElement {}
class FakeVideoFrame {}

/** Models the destructive bitmap reset required by the HTML canvas spec. */
class FakeCanvas {
  private bitmapWidth = 300;
  private bitmapHeight = 150;
  widthWriteCount = 0;
  heightWriteCount = 0;
  surface = "initial-bitmap";
  readonly context = new FakeCanvasContext(this);

  get width() {
    return this.bitmapWidth;
  }

  set width(value: number) {
    this.bitmapWidth = value;
    this.widthWriteCount += 1;
    this.surface = "cleared-by-width";
  }

  get height() {
    return this.bitmapHeight;
  }

  set height(value: number) {
    this.bitmapHeight = value;
    this.heightWriteCount += 1;
    this.surface = "cleared-by-height";
  }

  getContext() {
    return this.context;
  }
}

class FakeCanvasContext {
  fillStyle: string | CanvasGradient | CanvasPattern = "#000000";
  globalAlpha = 1;
  globalCompositeOperation: GlobalCompositeOperation = "source-over";
  imageSmoothingEnabled = true;
  imageSmoothingQuality: ImageSmoothingQuality = "low";
  fillTextCalls: string[] = [];

  constructor(private readonly canvas: FakeCanvas) {}

  save() {}
  restore() {}
  scale() {}
  translate() {}
  rotate() {}
  beginPath() {}
  rect() {}
  clip() {}
  measureText(text: string) {
    return { width: text.length * 10 } as TextMetrics;
  }
  fillText(text: string) {
    this.fillTextCalls.push(text);
  }

  fillRect() {
    this.canvas.surface = String(this.fillStyle);
  }

  drawImage(source: FakeCanvas | FakeVideoElement) {
    if (source instanceof FakeCanvas) {
      this.canvas.surface = source.surface;
      return;
    }
    if (source.drawBehavior === "throw") throw new Error("decoder frame unavailable");
    this.canvas.surface = source.frame;
  }
}

const videoPlan: FramePlan = {
  time: 1,
  audio: [],
  layers: [
    {
      clipId: "video-1",
      trackId: "v1",
      kind: "video",
      file: "video.mp4",
      sourceTime: 1,
      localTime: 1,
      transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
      opacity: 1,
      transitionType: "none",
      transitionPhase: "none",
      transitionProgress: 0,
      text: null,
    },
  ],
};

describe("scrub canvas bitmap lifetime", () => {
  const stagingCanvases: FakeCanvas[] = [];

  beforeAll(() => {
    vi.stubGlobal("HTMLVideoElement", FakeVideoElement);
    vi.stubGlobal("HTMLImageElement", FakeImageElement);
    vi.stubGlobal("SVGImageElement", FakeSvgImageElement);
    vi.stubGlobal("VideoFrame", FakeVideoFrame);
    vi.stubGlobal("document", {
      createElement: (tagName: string) => {
        if (tagName !== "canvas") throw new Error(`unexpected element: ${tagName}`);
        const canvas = new FakeCanvas();
        stagingCanvases.push(canvas);
        return canvas;
      },
    });
  });

  afterAll(() => {
    vi.unstubAllGlobals();
  });

  function createCompositor(resolutionScale = 1) {
    const visible = new FakeCanvas();
    const compositor = new CanvasCompositor(
      visible as unknown as HTMLCanvasElement,
      { width: 1920, height: 1080, resolutionScale },
    );
    const staging = stagingCanvases.at(-2)!;
    return { compositor, visible, staging };
  }

  it("does not reset either bitmap when resize receives unchanged dimensions", () => {
    const { compositor, visible, staging } = createCompositor();
    const validFrame = new FakeVideoElement();
    validFrame.frame = "decoded-frame-43";

    expect(
      compositor.render(videoPlan, () => validFrame as unknown as CanvasImageSource),
    ).toBe(true);
    expect(visible.surface).toBe("decoded-frame-43");

    const visibleWrites = [visible.widthWriteCount, visible.heightWriteCount];
    const stagingWrites = [staging.widthWriteCount, staging.heightWriteCount];
    compositor.resize(1920, 1080);

    expect([visible.widthWriteCount, visible.heightWriteCount]).toEqual(visibleWrites);
    expect([staging.widthWriteCount, staging.heightWriteCount]).toEqual(stagingWrites);
    expect(visible.surface).toBe("decoded-frame-43");
  });

  it("writes only the bitmap dimensions that really change", () => {
    const { compositor, visible, staging } = createCompositor();
    const visibleWrites = [visible.widthWriteCount, visible.heightWriteCount];
    const stagingWrites = [staging.widthWriteCount, staging.heightWriteCount];

    compositor.resize(1080, 1080);

    expect([visible.widthWriteCount, visible.heightWriteCount]).toEqual([
      visibleWrites[0] + 1,
      visibleWrites[1],
    ]);
    expect([staging.widthWriteCount, staging.heightWriteCount]).toEqual([
      stagingWrites[0] + 1,
      stagingWrites[1],
    ]);
  });

  it("rejects non-finite and non-positive resolution scales without clearing either bitmap", () => {
    const { compositor, visible, staging } = createCompositor();
    const validFrame = new FakeVideoElement();
    validFrame.frame = "decoded-frame-43";
    compositor.render(videoPlan, () => validFrame as unknown as CanvasImageSource);
    const visibleWrites = [visible.widthWriteCount, visible.heightWriteCount];
    const stagingWrites = [staging.widthWriteCount, staging.heightWriteCount];

    for (const scale of [Number.NaN, Number.POSITIVE_INFINITY, 0, -0.5]) {
      expect(() => compositor.setResolutionScale(scale)).toThrow(RangeError);
    }

    expect([visible.widthWriteCount, visible.heightWriteCount]).toEqual(visibleWrites);
    expect([staging.widthWriteCount, staging.heightWriteCount]).toEqual(stagingWrites);
    expect(visible.surface).toBe("decoded-frame-43");
  });

  it("clamps resolution scale and resizes both visible and staging canvases", () => {
    const { compositor, visible, staging } = createCompositor();

    compositor.setResolutionScale(0.01);
    expect([visible.width, visible.height]).toEqual([192, 108]);
    expect([staging.width, staging.height]).toEqual([192, 108]);

    compositor.setResolutionScale(20);
    expect([visible.width, visible.height]).toEqual([3840, 2160]);
    expect([staging.width, staging.height]).toEqual([3840, 2160]);
  });

  it("does not reset a committed frame when the normalized resolution scale is unchanged", () => {
    const { compositor, visible, staging } = createCompositor(20);
    const validFrame = new FakeVideoElement();
    validFrame.frame = "decoded-frame-43";
    compositor.render(videoPlan, () => validFrame as unknown as CanvasImageSource);
    const visibleWrites = [visible.widthWriteCount, visible.heightWriteCount];
    const stagingWrites = [staging.widthWriteCount, staging.heightWriteCount];

    compositor.setResolutionScale(3);

    expect([visible.widthWriteCount, visible.heightWriteCount]).toEqual(visibleWrites);
    expect([staging.widthWriteCount, staging.heightWriteCount]).toEqual(stagingWrites);
    expect(visible.surface).toBe("decoded-frame-43");
  });

  it("keeps the last committed bitmap when staging drawImage throws", () => {
    const { compositor, visible } = createCompositor();
    const validFrame = new FakeVideoElement();
    validFrame.frame = "decoded-frame-43";
    compositor.render(videoPlan, () => validFrame as unknown as CanvasImageSource);

    const unavailableFrame = new FakeVideoElement();
    unavailableFrame.drawBehavior = "throw";
    expect(
      compositor.render(
        { ...videoPlan, time: 2 },
        () => unavailableFrame as unknown as CanvasImageSource,
      ),
    ).toBe(false);
    expect(visible.surface).toBe("decoded-frame-43");
  });

  it("renders every line from a multiline text clip", () => {
    const { compositor, staging } = createCompositor();
    const textPlan: FramePlan = {
      time: 0,
      audio: [],
      layers: [
        {
          clipId: "text-1",
          trackId: "t1",
          kind: "text",
          file: "",
          sourceTime: 0,
          localTime: 0,
          transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
          opacity: 1,
          transitionType: "none",
          transitionPhase: "none",
          transitionProgress: 0,
          text: normalizeTextStyle({
            content: "Birinci satır\nİkinci satır",
            fontFamily: "Inter",
            fontSize: 64,
            fontWeight: 600,
            color: "#ffffff",
            backgroundColor: "transparent",
            align: "center",
          }),
        },
      ],
    };

    expect(compositor.render(textPlan, () => null)).toBe(true);
    expect(staging.context.fillTextCalls).toEqual(["Birinci satır", "İkinci satır"]);
  });
});

describe("CompositorPlayer scrub render dependencies", () => {
  it("sizes the preview from the stage instead of its intrinsic bitmap", () => {
    expect(compositorSource).toContain("fitPreviewDisplaySize(");
    expect(compositorSource).toContain("new ResizeObserver(updateStageSize)");
    expect(compositorSource).toContain("bind:this={stage}");
    expect(compositorSource).toContain(
      'style:width={`${canvasDisplaySize.width}px`}',
    );
    expect(compositorSource).toContain(
      'style:height={`${canvasDisplaySize.height}px`}',
    );
    expect(compositorSource).toContain("resolutionScale: previewQuality");
    expect(compositorSource).not.toMatch(
      /canvas\s*\{[\s\S]*?width:\s*auto;[\s\S]*?height:\s*auto;/,
    );
  });

  it("does not track the frame plan through the aspect-only resize effect", () => {
    const resizeCallIndex = compositorSource.search(/compositor\.resize\s*\(/);
    expect(resizeCallIndex, "canvas resize call").toBeGreaterThan(-1);
    const effectStart = compositorSource.lastIndexOf("$effect(() =>", resizeCallIndex);
    const effectEnd = compositorSource.indexOf("\n  $effect(() =>", resizeCallIndex);
    const resizeEffect = compositorSource.slice(effectStart, effectEnd);

    // renderFrame reads `plan`; calling it normally from this effect subscribes
    // resize() to every playhead tick and clears the canvas before seeked fires.
    const directlyTracksPlan =
      resizeEffect.includes("renderFrame()") && !resizeEffect.includes("untrack(");
    expect(directlyTracksPlan, resizeEffect).toBe(false);
  });

  it("lets a ready seeked video frame enter the render path immediately", () => {
    const directlyHandlesEveryMediaElement =
      /element\.onseeked\s*=\s*\(\)\s*=>\s*(?:\{\s*)?handleSeeked\(element\)/.test(
        compositorSource,
      );
    const directlyHandlesVideo =
      /element\.onseeked[\s\S]{0,240}if\s*\(element instanceof HTMLVideoElement\)\s*(?:\{\s*)?handleSeeked\(element\)/.test(
        compositorSource,
      );
    expect(directlyHandlesEveryMediaElement || directlyHandlesVideo).toBe(true);
  });
});
