import { describe, expect, it } from "vitest";
import { parseAss, serializeAss, type CaptionDocument } from "./caption-formats";
import {
  bilingualCaptionCues,
  captionDisplayCues,
  captionDocumentFromStudio,
  captionQaSummary,
  captionStudioFromDocument,
  captionStudioFromWordTranscript,
  normalizeCaptionStudioProject,
  updateCaptionCueText,
  upsertCaptionTranslation,
} from "./caption-studio";

describe("caption studio model", () => {
  it("creates word timing without pretending subtitle imports contain confidence", () => {
    const project = captionStudioFromDocument({
      format: "srt",
      cues: [{ id: "1", startMs: 1_000, endMs: 2_000, text: "Merhaba OpenAI" }],
    });

    expect(project.confidenceMode).toBe("unavailable");
    expect(project.cues[0].words).toMatchObject([
      { text: "Merhaba", startMs: 1_000, endMs: 1_500 },
      { text: "OpenAI", startMs: 1_500, endMs: 2_000 },
    ]);

    const summary = captionQaSummary({
      ...project,
      cues: project.cues.map((cue) => ({
        ...cue,
        words: cue.words.map((word) => ({ ...word, confidence: 0.1 })),
      })),
    });
    expect(summary).toMatchObject({ reviewWordCount: 0, lowConfidenceCount: 0 });
  });

  it("keeps exact anchor timings while an edited span is repartitioned", () => {
    const project = captionStudioFromDocument({
      cues: [{ id: "cue", startMs: 100, endMs: 1_000, text: "Bugün OpenAI geldi" }],
    });
    const edited = updateCaptionCueText(project, "cue", "Bugün Open AI geldi");

    expect(edited.cues[0].words.map((word) => word.text)).toEqual([
      "Bugün",
      "Open",
      "AI",
      "geldi",
    ]);
    expect(edited.cues[0].words[0]).toMatchObject(project.cues[0].words[0]);
    expect(edited.cues[0].words.at(-1)).toMatchObject(project.cues[0].words.at(-1)!);
    expect(edited.cues[0].startMs).toBe(100);
    expect(edited.cues[0].endMs).toBe(1_000);
  });

  it("builds one-click bilingual display while reporting missing translations", () => {
    let project = captionStudioFromDocument({
      cues: [
        { id: "a", startMs: 0, endMs: 1_000, text: "Merhaba" },
        { id: "b", startMs: 1_000, endMs: 2_000, text: "Dünya" },
      ],
    });
    project = upsertCaptionTranslation(project, "a", "Hello");
    project = { ...project, bilingual: true };

    expect(bilingualCaptionCues(project)[0].displayText).toBe("Merhaba\nHello");
    expect(captionDisplayCues(project).map((cue) => cue.text)).toEqual([
      "Merhaba\nHello",
      "Dünya",
    ]);
    expect(captionQaSummary(project).missingTranslationCount).toBe(1);
  });

  it("keeps ASS styles and speaker metadata through the QA document adapter", () => {
    const source = `[Script Info]\nTitle: QA\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Brand,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,20,20,60,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Brand,Ayşe,0,0,0,,Merhaba`;
    const parsed = parseAss(source);
    const studio = captionStudioFromDocument(parsed);
    const output = parseAss(serializeAss(captionDocumentFromStudio(studio, { format: "ass" })));

    expect(output.styles?.some((style) => style.name === "Brand")).toBe(true);
    expect(output.cues[0]).toMatchObject({ style: "Brand", speaker: "Ayşe", text: "Merhaba" });
  });

  it("deduplicates imported cue ids and rejects malformed persisted cues", () => {
    const source: CaptionDocument = {
      cues: [
        { id: "same", startMs: 0, endMs: 100, text: "Bir" },
        { id: "same", startMs: 100, endMs: 200, text: "İki" },
      ],
    };
    const project = captionStudioFromDocument(source);
    expect(project.cues.map((cue) => cue.id)).toEqual(["same", "same-2"]);

    const firstWord = project.cues[0].words[0];
    const normalized = normalizeCaptionStudioProject({
      ...project,
      cues: [
        {
          ...project.cues[0],
          id: "persisted",
          words: [
            { ...firstWord, id: "persisted-word" },
            { ...firstWord, id: "persisted-word", text: "tekrar" },
          ],
        },
        { ...project.cues[1], id: "persisted" },
        { id: "bad", startMs: "x", endMs: 2 },
      ],
    });
    expect(normalized.cues).toHaveLength(2);
    expect(normalized.cues.map((cue) => cue.id)).toEqual(["persisted", "persisted-2"]);
    expect(normalized.cues[0].words.map((word) => word.id)).toEqual([
      "persisted-word",
      "persisted-word-2",
    ]);
  });

  it("turns real ASR confidence and clip-local times into timeline QA cues", () => {
    const project = captionStudioFromWordTranscript(
      [
        { text: "Bugün", startMs: 0, endMs: 300, confidence: 0.97 },
        { text: "export", startMs: 320, endMs: 620, confidence: 0.61 },
      ],
      { timelineOffsetMs: 5_000, sourceName: "röportaj.mp4" },
    );

    expect(project.confidenceMode).toBe("word");
    expect(project.cues[0]).toMatchObject({ startMs: 5_000, endMs: 5_620 });
    expect(project.cues[0].words[1]).toMatchObject({ text: "export", confidence: 0.61 });
    expect(captionQaSummary(project)).toMatchObject({ lowConfidenceCount: 1, codeSwitchWordCount: 1 });
  });
});
