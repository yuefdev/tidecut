import { describe, expect, it } from "vitest";
import {
  applyAutoReframeToFramePlan,
  applyAutoReframeToSnapshot,
  applyStreamerLayoutToRenderTimeline,
  autoReframeClipSignature,
  autoReframeRotationSupported,
  createEmptyAutoReframeProjectState,
  getAutoReframeExportAspects,
  normalizeAutoReframeProjectState,
  plannerOptionsForStyle,
  setClipAutoReframe,
  setClipStreamerLayout,
  streamerLayoutViewports,
  trackerAnalysisToPlannerSamples,
} from "./project-state";
import { planAutoReframe } from "$lib/editor/auto-reframe";
import {
  createClip,
  createTrack,
  serializeProjectState,
} from "$lib/editor/timeline-engine";
import { toRenderTimeline } from "$lib/render/timeline-adapter";

function fixture() {
  const clip = createClip({
    id: "clip-1",
    trackId: "v1",
    file: "C:/media/source.mp4",
    kind: "video",
    start: 0,
    duration: 2,
    trimIn: 0,
    sourceDuration: 2,
  });
  const snapshot = serializeProjectState({
    version: 2,
    tracks: [createTrack("v1", "V1", "video")],
    clips: [clip],
    selectedClipId: clip.id,
  });
  const planned = planAutoReframe({
    sourceWidth: 1920,
    sourceHeight: 1080,
    samples: [
      { timeMs: 0, x: 0.25, y: 0.5, width: 0.15, height: 0.4, confidence: 1 },
      { timeMs: 1_000, x: 0.5, y: 0.5, width: 0.15, height: 0.4, confidence: 1 },
      { timeMs: 2_000, x: 0.75, y: 0.5, width: 0.15, height: 0.4, confidence: 1 },
    ],
  });
  if (!planned.ok) throw new Error(planned.error.message);
  const state = setClipAutoReframe(createEmptyAutoReframeProjectState(), clip.id, {
    clipSignature: autoReframeClipSignature(clip),
    aspects: ["9:16", "1:1"],
    style: "natural",
    framing: "auto",
    selection: { x: 0.175, y: 0.3, width: 0.15, height: 0.4 },
    engine: "opencv-csrt",
    generatedAtMs: 1,
    warnings: [],
    plan: planned.plan,
  });
  return { clip, snapshot, state };
}

describe("auto-reframe project state", () => {
  it("drops malformed persisted entries", () => {
    expect(normalizeAutoReframeProjectState({ version: 1, clips: { bad: {} } })).toEqual({
      version: 1,
      clips: {},
    });
  });

  it("overrides preview camera only for a selected aspect", () => {
    const { snapshot, state } = fixture();
    const plan = {
      time: 1,
      layers: [{
        clipId: "clip-1",
        trackId: "v1",
        kind: "video" as const,
        file: "C:/media/source.mp4",
        sourceTime: 1,
        localTime: 1,
        transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
        opacity: 1,
        transitionType: "none" as const,
        transitionPhase: "none" as const,
        transitionProgress: 0,
        text: null,
      }],
      audio: [],
    };
    const vertical = applyAutoReframeToFramePlan(plan, snapshot, state, "9:16");
    expect(vertical.layers[0].transform.scale).toBeGreaterThan(1);
    expect(applyAutoReframeToFramePlan(plan, snapshot, state, "16:9")).toBe(plan);
  });

  it("creates an ephemeral export snapshot and invalidates after a speed edit", () => {
    const { snapshot, state } = fixture();
    const vertical = applyAutoReframeToSnapshot(snapshot, state, "9:16");
    expect(vertical).not.toBe(snapshot);
    expect(vertical.clips[0].keyframes.x?.length).toBeGreaterThan(0);
    expect(snapshot.clips[0].keyframes.x).toBeUndefined();

    const changed = {
      ...snapshot,
      clips: [{ ...snapshot.clips[0], speed: 2 }],
    };
    expect(applyAutoReframeToSnapshot(changed, state, "9:16")).toBe(changed);
    expect(getAutoReframeExportAspects(changed, state)).toEqual([]);
  });

  it("builds two independently framed 9:16 preview panels from one tracked clip", () => {
    const { clip, snapshot, state } = fixture();
    const layoutState = setClipStreamerLayout(state, clip, {
      enabled: true,
      facePosition: "top",
      faceFraction: 0.42,
      contentFocusX: 0.5,
      contentFocusY: 0.5,
    });
    const plan = {
      time: 1,
      layers: [{
        clipId: clip.id,
        trackId: clip.trackId,
        kind: "video" as const,
        file: clip.file,
        sourceTime: 1,
        localTime: 1,
        transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
        opacity: 1,
        transitionType: "none" as const,
        transitionPhase: "none" as const,
        transitionProgress: 0,
        text: null,
      }],
      audio: [{
        clipId: clip.id,
        trackId: clip.trackId,
        file: clip.file,
        sourceTime: 1,
        localTime: 1,
        playbackRate: 1,
        gain: 1,
        pan: 0,
      }],
    };

    const vertical = applyAutoReframeToFramePlan(plan, snapshot, layoutState, "9:16");
    expect(vertical.layers).toHaveLength(2);
    expect(vertical.layers.map((layer) => layer.layoutRole)).toEqual([
      "streamer-content",
      "streamer-face",
    ]);
    expect(vertical.layers[0].viewport?.y).toBeGreaterThan(0.42);
    expect(vertical.layers[1].viewport).toMatchObject({ x: 0, y: 0, width: 1 });
    expect(vertical.layers[0].transform.scale).toBeGreaterThan(1);
    expect(vertical.audio).toBe(plan.audio);
    expect(applyAutoReframeToFramePlan(plan, snapshot, layoutState, "1:1").layers).toHaveLength(1);
  });

  it("expands streamer delivery into a silent tracked-face render layer", () => {
    const { clip, snapshot, state } = fixture();
    const layoutState = setClipStreamerLayout(state, clip, {
      enabled: true,
      facePosition: "bottom",
      faceFraction: 0.45,
      contentFocusX: 0.5,
      contentFocusY: 0.5,
    });
    const timeline = toRenderTimeline(snapshot);
    timeline.clips[0].audioPath = "C:/cache/clean.wav";
    timeline.clips[0].audioTrimInMs = 125;
    timeline.clips[0].keyframes = [
      {
        atMs: 500,
        volume: 0.8,
        riderGain: 1.1,
        duckGain: 0.7,
        pan: -0.2,
        opacity: 0.9,
        easing: {
          volume: "linear",
          riderGain: "linear",
          duckGain: "linear",
          pan: "linear",
        },
      },
    ];
    const result = applyStreamerLayoutToRenderTimeline(
      timeline,
      snapshot,
      layoutState,
      "9:16",
    );

    expect(result).not.toBe(timeline);
    expect(result.clips.map((item) => item.id)).toEqual([
      "clip-1::streamer-content",
      "clip-1::streamer-face",
    ]);
    const [content, face] = result.clips;
    expect(content.viewport?.y).toBe(0);
    expect(face.viewport?.y).toBeGreaterThan(0.5);
    expect(content.volume).toBe(1);
    expect(content.audioPath).toBe("C:/cache/clean.wav");
    expect(content.audioTrimInMs).toBe(125);
    expect(face.volume).toBe(0);
    expect(face.audioPath).toBeUndefined();
    expect(face.audioTrimInMs).toBeUndefined();
    expect(face.keyframes?.some((frame) => frame.x !== undefined)).toBe(true);
    expect(face.keyframes?.some((frame) =>
      frame.volume !== undefined ||
      frame.riderGain !== undefined ||
      frame.duckGain !== undefined ||
      frame.pan !== undefined
    )).toBe(false);
    expect(content.keyframes?.some((frame) => frame.volume === 0.8)).toBe(true);
    expect(timeline.clips).toHaveLength(1);
    expect(applyAutoReframeToSnapshot(snapshot, layoutState, "9:16")).toBe(snapshot);
  });

  it("frames the face panel from detected facecam boxes when a faceTrack exists", () => {
    const { clip, snapshot, state } = fixture();
    const layoutSettings = {
      enabled: true,
      facePosition: "top" as const,
      faceFraction: 0.42,
      contentFocusX: 0.5,
      contentFocusY: 0.5,
    };
    const planOnlyState = setClipStreamerLayout(state, clip, layoutSettings);
    const trackedState = setClipStreamerLayout(
      setClipAutoReframe(state, clip.id, {
        ...state.clips[clip.id],
        faceTrack: [
          { timeMs: 0, centerX: 0.8, centerY: 0.25, width: 0.22, height: 0.36 },
          { timeMs: 2_000, centerX: 0.2, centerY: 0.25, width: 0.22, height: 0.36 },
        ],
      }),
      clip,
      layoutSettings,
    );
    expect(trackedState.clips[clip.id].faceTrack).toHaveLength(2);

    const layerAt = (localTime: number) => ({
      time: localTime,
      layers: [{
        clipId: clip.id,
        trackId: clip.trackId,
        kind: "video" as const,
        file: clip.file,
        sourceTime: localTime,
        localTime,
        transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
        opacity: 1,
        transitionType: "none" as const,
        transitionPhase: "none" as const,
        transitionProgress: 0,
        text: null,
      }],
      audio: [],
    });
    const faceLayer = (
      framed: ReturnType<typeof applyAutoReframeToFramePlan>,
    ) => framed.layers.find((layer) => layer.layoutRole === "streamer-face")!;

    // A right-side facecam pans the source left (negative x) and vice versa.
    const start = faceLayer(applyAutoReframeToFramePlan(layerAt(0), snapshot, trackedState, "9:16"));
    const end = faceLayer(applyAutoReframeToFramePlan(layerAt(2), snapshot, trackedState, "9:16"));
    expect(start.transform.x).toBeLessThan(0);
    expect(end.transform.x).toBeGreaterThan(0);

    // The detected camera rectangle is framed tighter than the loose 9:16 crop.
    const loose = faceLayer(applyAutoReframeToFramePlan(layerAt(0), snapshot, planOnlyState, "9:16"));
    expect(start.transform.scale).toBeGreaterThan(loose.transform.scale);

    // Export emits one face keyframe per detected sample.
    const rendered = applyStreamerLayoutToRenderTimeline(
      toRenderTimeline(snapshot),
      snapshot,
      trackedState,
      "9:16",
    );
    const faceClip = rendered.clips.find((item) => item.id.endsWith("::streamer-face"))!;
    expect(faceClip.keyframes?.filter((frame) => frame.x !== undefined)).toHaveLength(2);

    // Corrupt persisted tracks are dropped without dropping the entry.
    const corrupted = normalizeAutoReframeProjectState({
      version: 1,
      clips: {
        [clip.id]: {
          ...trackedState.clips[clip.id],
          faceTrack: [
            { timeMs: 5, centerX: 0.5, centerY: 0.5, width: 0.2, height: 0.2 },
            { timeMs: 5, centerX: 0.5, centerY: 0.5, width: 0.2, height: 0.2 },
          ],
        },
      },
    });
    expect(corrupted.clips[clip.id]).toBeDefined();
    expect(corrupted.clips[clip.id].faceTrack).toBeUndefined();
  });

  it("keeps moving face-panel preview transforms aligned with export interpolation", () => {
    const { clip, snapshot, state } = fixture();
    const sourceEntry = state.clips[clip.id];
    const sourceVariant = sourceEntry.plan.variants["9:16"];
    const normalizedAspect = (1080 / 1920) * (1080 / 1920);
    const cameraKeyframes = sourceVariant.cameraKeyframes.map((frame, index) => {
      const height = index % 2 === 0 ? 0.4 : 0.8;
      const width = height * normalizedAspect;
      return {
        ...frame,
        crop: { centerX: 0.5, centerY: 0.5, width, height },
        x: 0,
        y: 0,
        scale: Math.round((1 / width) * 1_000_000) / 1_000_000,
      };
    });
    const timelineProperty = (property: "x" | "y" | "scale") =>
      cameraKeyframes.map((frame) => ({
        time: Math.round((frame.timeMs / 1_000) * 1_000_000) / 1_000_000,
        value: frame[property],
        easing: "linear" as const,
      }));
    const tracked = setClipAutoReframe(createEmptyAutoReframeProjectState(), clip.id, {
      ...sourceEntry,
      plan: {
        ...sourceEntry.plan,
        variants: {
          ...sourceEntry.plan.variants,
          "9:16": {
            ...sourceVariant,
            cameraKeyframes,
            timelineKeyframes: {
              x: timelineProperty("x"),
              y: timelineProperty("y"),
              scale: timelineProperty("scale"),
            },
          },
        },
      },
      streamerLayout: {
        enabled: true,
        facePosition: "top",
        faceFraction: 0.42,
        contentFocusX: 0.5,
        contentFocusY: 0.5,
      },
    });
    const exported = applyStreamerLayoutToRenderTimeline(
      toRenderTimeline(snapshot),
      snapshot,
      tracked,
      "9:16",
    );
    expect(tracked.clips[clip.id]?.streamerLayout?.enabled).toBe(true);
    expect(exported.clips).toHaveLength(2);
    const faceFrames = (exported.clips[1].keyframes ?? []).filter(
      (frame) => frame.scaleX !== undefined,
    );
    const pairIndex = faceFrames.findIndex((frame, index) => {
      const next = faceFrames[index + 1];
      return Boolean(
        next &&
        next.atMs > frame.atMs + 1 &&
        Math.abs((next.scaleX ?? 0) - (frame.scaleX ?? 0)) > 0.02,
      );
    });
    expect(pairIndex).toBeGreaterThanOrEqual(0);
    const left = faceFrames[pairIndex];
    const right = faceFrames[pairIndex + 1];
    const midpointMs = (left.atMs + right.atMs) / 2;
    const progress = (midpointMs - left.atMs) / (right.atMs - left.atMs);
    const expectedScale = left.scaleX! + (right.scaleX! - left.scaleX!) * progress;
    const basePlan = {
      time: midpointMs / 1_000,
      layers: [{
        clipId: clip.id,
        trackId: clip.trackId,
        kind: "video" as const,
        file: clip.file,
        sourceTime: midpointMs / 1_000,
        localTime: midpointMs / 1_000,
        transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
        opacity: 1,
        transitionType: "none" as const,
        transitionPhase: "none" as const,
        transitionProgress: 0,
        text: null,
      }],
      audio: [],
    };
    const preview = applyAutoReframeToFramePlan(
      basePlan,
      snapshot,
      tracked,
      "9:16",
    );
    const previewFace = preview.layers.find((layer) => layer.layoutRole === "streamer-face");
    expect(previewFace?.transform.scale).toBeCloseTo(expectedScale, 5);
  });

  it("normalizes saved streamer layout bounds and leaves a visible divider", () => {
    const { clip, state } = fixture();
    const normalized = setClipStreamerLayout(state, clip, {
      enabled: true,
      facePosition: "bottom",
      faceFraction: 5,
      contentFocusX: -4,
      contentFocusY: 9,
    });
    const settings = normalized.clips[clip.id].streamerLayout!;
    expect(settings).toMatchObject({
      enabled: true,
      facePosition: "bottom",
      faceFraction: 0.6,
      contentFocusX: 0,
      contentFocusY: 1,
    });
    const restored = normalizeAutoReframeProjectState(
      JSON.parse(JSON.stringify(normalized)),
    );
    expect(restored.clips[clip.id].streamerLayout).toEqual(settings);
    const viewports = streamerLayoutViewports(settings);
    expect(viewports.content.height).toBeLessThan(viewports.face.y);
    expect(viewports.face.y + viewports.face.height).toBeCloseTo(1);
  });

  it("maps backend top-left/source-time samples to centre/timeline-time samples", () => {
    const [sample] = trackerAnalysisToPlannerSamples({
      samples: [{
        timeMs: 1_000,
        bbox: { x: 0.2, y: 0.3, width: 0.2, height: 0.4 },
        confidence: 1,
      }],
    }, 2, "close");
    expect(sample.timeMs).toBe(500);
    expect(sample.x).toBeCloseTo(0.3);
    expect(sample.y).toBeCloseTo(0.5);
    expect(sample.width).toBeCloseTo(0.13);
    expect(sample.height).toBeCloseTo(0.26);
    expect(plannerOptionsForStyle("dynamic").panTimeConstantMs).toBeLessThan(
      plannerOptionsForStyle("calm").panTimeConstantMs!,
    );
  });

  it("invalidates a plan when rotation changes and rejects rotated camera geometry", () => {
    const { clip } = fixture();
    const rotated = {
      ...clip,
      transform: { ...clip.transform, rotation: 90 },
    };
    expect(autoReframeRotationSupported(clip)).toBe(true);
    expect(autoReframeRotationSupported(rotated)).toBe(false);
    expect(autoReframeClipSignature(rotated)).not.toBe(autoReframeClipSignature(clip));
  });
});
