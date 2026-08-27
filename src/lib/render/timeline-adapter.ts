import type {
  AnimatableProperty,
  KeyframeEasing,
  TimelineProjectSnapshot,
} from "$lib/editor/timeline-engine";
import {
  buildAutoDuckingEnvelope,
  type DuckingKeyframe,
} from "$lib/editor/audio-ducking";
import type {
  RenderClip,
  RenderKeyframe,
  RenderTimeline,
  RenderTransition,
  RenderTransitionKind,
} from "./types";

export function toRenderTimeline(
  snapshot: TimelineProjectSnapshot,
): RenderTimeline {
  const trackOrder = new Map(
    snapshot.tracks.map((track, index) => [track.id, index]),
  );

  return {
    backgroundColor: "#000000",
    tracks: snapshot.tracks.map((track, index) => ({
      id: track.id,
      kind: track.type,
      mute: track.muted,
      solo: track.solo,
      locked: track.locked,
      gain: track.gain,
      pan: track.pan,
      zIndex: index,
    })),
    clips: snapshot.clips.map((clip) => {
      const renderClip: RenderClip = {
        id: clip.id,
        trackId: clip.trackId,
        kind: clip.kind,
        path: clip.kind === "text" ? undefined : clip.file,
        startMs: clip.startMs,
        durationMs: clip.durationMs,
        trimInMs: clip.trimInMs,
        speed: clip.speed,
        volume: clip.volume,
        pan: clip.pan,
        noiseReduction: clip.audioSeparated ? null : clip.noiseReduction,
        transform: {
          x: clip.transform.x,
          y: clip.transform.y,
          scaleX: clip.transform.scale,
          scaleY: clip.transform.scale,
          rotationDeg: clip.transform.rotation,
          opacity: clip.transform.opacity,
          shake: clip.transform.shake,
        },
        transition: toRenderTransition(clip.transition),
        keyframes: flattenKeyframes(
          clip.keyframes,
          buildAutoDuckingEnvelope(snapshot, clip),
        ),
        zIndex: trackOrder.get(clip.trackId) ?? 0,
      };

      if (clip.text) {
        renderClip.text = {
          content: clip.text.content,
          fontFamily: clip.text.fontFamily,
          fontSize: clip.text.fontSize,
          fontWeight: clip.text.fontWeight,
          color: clip.text.color,
          backgroundColor: clip.text.backgroundColor,
          align: clip.text.align,
          ...(clip.text.fontPath ? { fontPath: clip.text.fontPath } : {}),
          strokeColor: clip.text.strokeColor,
          strokeWidth: clip.text.strokeWidth,
          shadowColor: clip.text.shadowColor,
          shadowOffsetX: clip.text.shadowOffsetX,
          shadowOffsetY: clip.text.shadowOffsetY,
          animationInKind: clip.text.animationIn.type,
          animationInMs: Math.round(clip.text.animationIn.duration * 1000),
          animationOutKind: clip.text.animationOut.type,
          animationOutMs: Math.round(clip.text.animationOut.duration * 1000),
        };
      }
      return renderClip;
    }),
  };
}

function toRenderTransition(
  transition: TimelineProjectSnapshot["clips"][number]["transition"],
): RenderTransition | undefined {
  const inKind = mapTransitionKind(transition.in.type);
  const outKind = mapTransitionKind(transition.out.type);
  const inDurationMs = inKind === "none"
    ? 0
    : Math.max(0, Math.round(transition.in.duration * 1000));
  const outDurationMs = outKind === "none"
    ? 0
    : Math.max(0, Math.round(transition.out.duration * 1000));
  if (inDurationMs === 0 && outDurationMs === 0) return undefined;
  return {
    kind: inKind !== "none" ? inKind : outKind,
    inKind,
    outKind,
    inDurationMs,
    outDurationMs,
  };
}

function mapTransitionKind(
  type: TimelineProjectSnapshot["clips"][number]["transition"]["in"]["type"],
): RenderTransitionKind {
  switch (type) {
    case "none":
      return "none";
    case "crossfade":
      return "dissolve";
    case "dip-to-black":
      return "dip_to_black";
    case "wipe-left":
      return "wipe_left";
    case "wipe-right":
      return "wipe_right";
    // Motion and graphic-overlay transitions are preview-side effects; the
    // ffmpeg pipeline exports them as a dissolve so timing still matches.
    case "slide-up":
    case "slide-down":
    case "slide-left":
    case "slide-right":
    case "zoom-in":
    case "zoom-out":
    case "spin":
    case "flip-3d":
    case "blur":
    case "whip-left":
    case "whip-right":
    case "fx-light-leak":
    case "fx-film-burn":
    case "fx-glitch":
    case "fx-ink":
    case "fx-brush":
      return "dissolve";
  }
}

function flattenKeyframes(
  keyframes: TimelineProjectSnapshot["clips"][number]["keyframes"],
  ducking: readonly DuckingKeyframe[] = [],
): RenderKeyframe[] {
  const frames = new Map<number, RenderKeyframe>();
  for (const [property, values] of Object.entries(keyframes) as Array<
    [
      AnimatableProperty,
      Array<{ time: number; value: number; easing: KeyframeEasing }>,
    ]
  >) {
    for (const value of values) {
      const atMs = Math.max(0, Math.round(value.time * 1000));
      const frame = frames.get(atMs) ?? { atMs };
      assignKeyframeValue(frame, property, value.value);
      assignKeyframeEasing(frame, property, value.easing);
      frames.set(atMs, frame);
    }
  }
  for (const value of ducking) {
    const atMs = Math.max(0, Math.round(value.time * 1000));
    const frame = frames.get(atMs) ?? { atMs };
    frame.duckGain = value.value;
    frame.easing = { ...frame.easing, duckGain: value.easing };
    frames.set(atMs, frame);
  }
  return [...frames.values()].sort((left, right) => left.atMs - right.atMs);
}

function assignKeyframeValue(
  frame: RenderKeyframe,
  property: AnimatableProperty,
  value: number,
) {
  switch (property) {
    case "x":
    case "y":
    case "opacity":
    case "volume":
    case "riderGain":
    case "pan":
      frame[property] = value;
      break;
    case "scale":
      frame.scaleX = value;
      frame.scaleY = value;
      break;
    case "rotation":
      frame.rotationDeg = value;
      break;
  }
}

function assignKeyframeEasing(
  frame: RenderKeyframe,
  property: AnimatableProperty,
  easing: KeyframeEasing,
) {
  const values = frame.easing ?? {};
  switch (property) {
    case "x":
    case "y":
    case "opacity":
    case "volume":
    case "riderGain":
    case "pan":
      values[property] = easing;
      break;
    case "scale":
      values.scaleX = easing;
      values.scaleY = easing;
      break;
    case "rotation":
      values.rotationDeg = easing;
      break;
  }
  frame.easing = values;
}

export function replaceTimelineMediaPaths(
  snapshot: TimelineProjectSnapshot,
  replacements: ReadonlyMap<string, string>,
): TimelineProjectSnapshot {
  return {
    ...snapshot,
    tracks: snapshot.tracks.map((track) => ({ ...track })),
    clips: snapshot.clips.map((clip) => ({
      ...clip,
      file: replacements.get(clip.file) ?? clip.file,
    })),
  };
}
