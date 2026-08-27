import { describe, expect, it } from "vitest";
import { captionStudioFromDocument, upsertCaptionTranslation } from "./caption-studio";
import {
  buildCaptionTimelinePlan,
  CAPTION_CLIP_SOURCE_PREFIX,
  captionTextStyle,
} from "./caption-timeline";

describe("caption timeline adapter", () => {
  it("keeps overlapping cue timing by assigning separate CC lanes", () => {
    const project = captionStudioFromDocument({
      cues: [
        { id: "one", startMs: 1_000, endMs: 3_000, text: "Bir" },
        { id: "two", startMs: 2_500, endMs: 4_200, text: "İki" },
        { id: "three", startMs: 5_000, endMs: 6_000, text: "Üç" },
      ],
    });
    const plan = buildCaptionTimelinePlan(project, "16:9");

    expect(plan.tracks.map((track) => track.name)).toEqual(["CC1", "CC2"]);
    expect(plan.clips).toMatchObject([
      { trackId: "caption-qa-1", start: 1, duration: 2 },
      { trackId: "caption-qa-2", start: 2.5, duration: 1.7 },
      { trackId: "caption-qa-1", start: 5, duration: 1 },
    ]);
    expect(plan.clips.every((clip) => clip.file.startsWith(CAPTION_CLIP_SOURCE_PREFIX))).toBe(true);
  });

  it("renders one-click bilingual cue text into the generated clip", () => {
    let project = captionStudioFromDocument({
      cues: [{ id: "hello", startMs: 0, endMs: 1_500, text: "Merhaba" }],
    });
    project = upsertCaptionTranslation(project, "hello", "Hello");
    project = { ...project, bilingual: true };

    const plan = buildCaptionTimelinePlan(project, "9:16");
    expect(plan.clips[0].text?.content).toBe("Merhaba\nHello");
    expect(plan.clips[0].text?.fontSize).toBe(50);
    expect(plan.clips[0].transform.y).toBeCloseTo(614.4);
  });

  it("maps motion templates to animations supported by preview and final render", () => {
    expect(captionTextStyle("clean", "A", 2, false).animationIn.type).toBe("fade");
    expect(captionTextStyle("pop", "A", 2, false).animationIn.type).toBe("bounce");
    expect(captionTextStyle("slide", "A", 2, false).animationIn.type).toBe("slide-up");
  });
});
