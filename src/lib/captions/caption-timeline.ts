import {
  createTextClip,
  createTrack,
  normalizeTextStyle,
  type TextClipStyle,
  type TimelineClip,
  type TimelineTrack,
} from "$lib/editor/timeline-engine";
import {
  captionDisplayCues,
  type CaptionStudioProject,
  type CaptionTemplateId,
} from "./caption-studio";

export const CAPTION_CLIP_SOURCE_PREFIX = "astral-caption:";

export interface CaptionTimelinePlan {
  tracks: TimelineTrack[];
  clips: TimelineClip[];
}

/**
 * Turns the canonical QA model into generated text clips. Overlapping speaker
 * cues are assigned to deterministic CC lanes so no timing is truncated.
 */
export function buildCaptionTimelinePlan(
  project: CaptionStudioProject,
  aspect: "9:16" | "1:1" | "16:9",
): CaptionTimelinePlan {
  const displayCues = [...captionDisplayCues(project)].sort(
    (left, right) => left.startMs - right.startMs || left.endMs - right.endMs,
  );
  if (displayCues.length === 0) return { tracks: [], clips: [] };

  const laneEnds: number[] = [];
  const laneByCueIndex: number[] = [];
  displayCues.forEach((cue, cueIndex) => {
    let lane = laneEnds.findIndex((endMs) => endMs <= cue.startMs);
    if (lane === -1) lane = laneEnds.length;
    laneEnds[lane] = cue.endMs;
    laneByCueIndex[cueIndex] = lane;
  });
  const tracks = laneEnds.map((_, lane) =>
    createTrack(`caption-qa-${lane + 1}`, `CC${lane + 1}`, "text"),
  );
  const canvasHeight = aspect === "9:16" ? 1920 : 1080;
  const verticalOffset = canvasHeight * 0.32;
  const clips = displayCues.map((cue, index) => {
    const lane = laneByCueIndex[index];
    const duration = Math.max(1 / 120, (cue.endMs - cue.startMs) / 1000);
    return createTextClip({
      id: `caption-qa-${index + 1}-${safeCaptionId(cue.id)}`,
      trackId: tracks[lane].id,
      file: `${CAPTION_CLIP_SOURCE_PREFIX}${cue.id}`,
      start: cue.startMs / 1000,
      duration,
      transform: {
        x: 0,
        y: verticalOffset - lane * Math.max(82, canvasHeight * 0.06),
        scale: 1,
        rotation: 0,
        opacity: 1,
      },
      text: captionTextStyle(project.template, cue.text, duration, project.bilingual),
    });
  });
  return { tracks, clips };
}

export function captionTextStyle(
  template: CaptionTemplateId,
  content: string,
  duration: number,
  bilingual: boolean,
): TextClipStyle {
  const fontSize = bilingual ? 50 : 62;
  if (template === "pop") {
    return normalizeTextStyle({
      content,
      fontFamily: "Inter",
      fontSize: fontSize + 7,
      fontWeight: 900,
      color: "#FFE27AFF",
      backgroundColor: "transparent",
      align: "center",
      strokeColor: "#101010FF",
      strokeWidth: 7,
      shadowColor: "#00000099",
      shadowBlur: 18,
      shadowOffsetX: 0,
      shadowOffsetY: 8,
      animationIn: { type: "bounce", duration: Math.min(0.38, duration / 2) },
      animationOut: { type: "fade", duration: Math.min(0.16, duration / 3) },
    });
  }
  if (template === "slide") {
    return normalizeTextStyle({
      content,
      fontFamily: "Inter",
      fontSize,
      fontWeight: 800,
      color: "#FFFFFFFF",
      backgroundColor: "#111817D9",
      align: "center",
      strokeColor: "#000000FF",
      strokeWidth: 2,
      shadowColor: "#00000080",
      shadowBlur: 12,
      shadowOffsetX: 0,
      shadowOffsetY: 5,
      animationIn: { type: "slide-up", duration: Math.min(0.32, duration / 2) },
      animationOut: { type: "fade", duration: Math.min(0.16, duration / 3) },
    });
  }
  return normalizeTextStyle({
    content,
    fontFamily: "Inter",
    fontSize,
    fontWeight: 750,
    color: "#FFFFFFFF",
    backgroundColor: "#101010CC",
    align: "center",
    strokeColor: "#000000FF",
    strokeWidth: 2,
    shadowColor: "#00000080",
    shadowBlur: 10,
    shadowOffsetX: 0,
    shadowOffsetY: 4,
    animationIn: { type: "fade", duration: Math.min(0.16, duration / 3) },
    animationOut: { type: "fade", duration: Math.min(0.16, duration / 3) },
  });
}

function safeCaptionId(value: string): string {
  return value.replace(/[^a-zA-Z0-9_-]+/g, "-").slice(0, 48) || "cue";
}
