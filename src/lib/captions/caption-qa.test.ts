import { describe, expect, it } from "vitest";
import {
  CAPTION_TIME_UNIT,
  CONFIDENCE_COLORS,
  alignEditedCaptionText,
  analyzeCaptionCue,
  applyDictionarySuggestion,
  buildSpeakerCues,
  classifyWordConfidence,
  createBilingualCues,
  findDictionarySuggestions,
  markCodeSwitch,
  validateCaptionWords,
  type CaptionCue,
  type CaptionDictionaryEntry,
  type CaptionWord,
} from "./caption-qa";

function word(
  id: string,
  text: string,
  startMs: number,
  endMs: number,
  confidence = 0.95,
  extras: Partial<CaptionWord> = {},
): CaptionWord {
  return { id, text, startMs, endMs, confidence, ...extras };
}

const brandDictionary: CaptionDictionaryEntry[] = [
  {
    id: "capcut",
    canonical: "CapCut",
    variants: ["kap kat", "cap cut"],
    category: "brand",
    language: "en",
  },
  {
    id: "openai",
    canonical: "OpenAI",
    variants: ["open ay"],
    category: "proper-noun",
    language: "en",
  },
];

describe("caption confidence QA", () => {
  it("classifies low, review and high confidence with stable UI colors", () => {
    expect(CAPTION_TIME_UNIT).toBe("milliseconds");
    expect(classifyWordConfidence(0.4)).toBe("low");
    expect(classifyWordConfidence(0.8)).toBe("review");
    expect(classifyWordConfidence(0.95)).toBe("high");
    expect(CONFIDENCE_COLORS.low).toBe("#ef4444");
  });

  it("reports malformed word timing and confidence without throwing", () => {
    const issues = validateCaptionWords([
      word("w1", "", 100, 50, 1.2),
      word("w2", "sonra", 20, 80),
    ]);
    expect(issues.map((issue) => issue.field)).toEqual([
      "text",
      "timing",
      "confidence",
      "order",
    ]);
  });
});

describe("caption dictionary", () => {
  it("matches multi-word ASR variants and suggests canonical brand spelling", () => {
    const words = [
      word("w1", "Bugün", 0, 300),
      word("w2", "kap", 320, 520, 0.51),
      word("w3", "kat", 520, 700, 0.49),
      word("w4", "kullandık", 720, 1_100),
    ];

    expect(findDictionarySuggestions(words, brandDictionary)).toEqual([
      expect.objectContaining({
        entryId: "capcut",
        startWordIndex: 1,
        endWordIndex: 2,
        inputText: "kap kat",
        canonical: "CapCut",
        score: 1,
      }),
    ]);
  });

  it("applies a dictionary merge over the full old timing span", () => {
    const words = [
      word("w1", "kap", 200, 450, 0.4),
      word("w2", "kat", 450, 800, 0.45),
    ];
    const [suggestion] = findDictionarySuggestions(words, brandDictionary);
    const corrected = applyDictionarySuggestion(words, suggestion);

    expect(corrected).toHaveLength(1);
    expect(corrected[0]).toEqual(expect.objectContaining({
      text: "CapCut",
      startMs: 200,
      endMs: 800,
      confidence: 1,
      edited: true,
    }));
  });
});

describe("Turkish-English code switching", () => {
  it("uses ASR metadata before dictionary and lexical language evidence", () => {
    const annotations = markCodeSwitch(
      [
        word("w1", "Bugün", 0, 200),
        word("w2", "render", 210, 450),
        word("w3", "OpenAI", 460, 700),
        word("w4", "meeting", 710, 950, 0.9, { language: "tr" }),
      ],
      "tr",
      { dictionary: brandDictionary },
    );

    expect(annotations.map(({ language, source, isCodeSwitch }) => ({
      language,
      source,
      isCodeSwitch,
    }))).toEqual([
      { language: "tr", source: "lexicon", isCodeSwitch: false },
      { language: "en", source: "lexicon", isCodeSwitch: true },
      { language: "en", source: "dictionary", isCodeSwitch: true },
      { language: "tr", source: "explicit", isCodeSwitch: false },
    ]);
  });
});

describe("timing-preserving text edits", () => {
  it("keeps exact anchors and gives a merged correction the replaced span", () => {
    const original = [
      word("w1", "Merhaba", 0, 400),
      word("w2", "kap", 500, 760, 0.4),
      word("w3", "kat", 760, 1_000, 0.42),
      word("w4", "dünyası", 1_050, 1_450),
    ];
    const edited = alignEditedCaptionText(original, "Merhaba CapCut dünyası");

    expect(edited.map((item) => [item.text, item.startMs, item.endMs])).toEqual([
      ["Merhaba", 0, 400],
      ["CapCut", 500, 1_000],
      ["dünyası", 1_050, 1_450],
    ]);
    expect(edited[0].id).toBe("w1");
    expect(edited[2].id).toBe("w4");
    expect(edited[1]).toEqual(expect.objectContaining({ confidence: 1, edited: true }));
  });

  it("retains ID and timing for a one-to-one correction", () => {
    const [corrected] = alignEditedCaptionText(
      [word("asr-7", "yanlıs", 120, 620, 0.31, { speakerId: "speaker-a" })],
      "yanlış",
    );
    expect(corrected).toEqual({
      id: "asr-7",
      text: "yanlış",
      startMs: 120,
      endMs: 620,
      confidence: 1,
      speakerId: "speaker-a",
      language: undefined,
      edited: true,
    });
  });

  it("places inserted words in the real gap without moving old anchors", () => {
    const original = [
      word("w1", "Bugün", 0, 300),
      word("w2", "geldik", 500, 800),
    ];
    const edited = alignEditedCaptionText(original, "Bugün erken geldik");

    expect(edited.map((item) => [item.text, item.startMs, item.endMs])).toEqual([
      ["Bugün", 0, 300],
      ["erken", 300, 500],
      ["geldik", 500, 800],
    ]);
    expect(edited[0].id).toBe("w1");
    expect(edited[2].id).toBe("w2");
  });

  it("partitions an old word span when a correction splits it into two words", () => {
    const edited = alignEditedCaptionText(
      [word("w1", "OpenAI", 100, 700, 0.6)],
      "Open AI",
    );
    expect(edited.map((item) => [item.text, item.startMs, item.endMs])).toEqual([
      ["Open", 100, 400],
      ["AI", 400, 700],
    ]);
    expect(edited.every((item) => item.confidence === 1 && item.edited)).toBe(true);
  });
});

describe("speaker cues and bilingual output", () => {
  const speakerWords = [
    word("w1", "Merhaba", 0, 300, 0.95, { speakerId: "a" }),
    word("w2", "Ece", 320, 600, 0.95, { speakerId: "a" }),
    word("w3", "Selam", 650, 950, 0.95, { speakerId: "b" }),
    word("w4", "tekrar", 2_000, 2_300, 0.95, { speakerId: "b" }),
  ];

  it("splits cues on speaker changes and long pauses", () => {
    const cues = buildSpeakerCues(speakerWords, { maxGapMs: 500, cueIdPrefix: "qa" });
    expect(cues.map((cue) => ({
      id: cue.id,
      speakerId: cue.speakerId,
      text: cue.text,
      timing: [cue.startMs, cue.endMs],
    }))).toEqual([
      { id: "qa-1", speakerId: "a", text: "Merhaba Ece", timing: [0, 600] },
      { id: "qa-2", speakerId: "b", text: "Selam", timing: [650, 950] },
      { id: "qa-3", speakerId: "b", text: "tekrar", timing: [2_000, 2_300] },
    ]);
  });

  it("builds two-line bilingual cues without changing source timing or speaker", () => {
    const [sourceCue] = buildSpeakerCues(speakerWords.slice(0, 2), { cueIdPrefix: "qa" });
    const [bilingual] = createBilingualCues(
      [sourceCue],
      [{ cueId: sourceCue.id, text: "Hello Ece" }],
    );

    expect(bilingual).toEqual(expect.objectContaining({
      sourceCueId: "qa-1",
      startMs: 0,
      endMs: 600,
      speakerId: "a",
      sourceLanguage: "tr",
      targetLanguage: "en",
      displayText: "Merhaba Ece\nHello Ece",
      translationMissing: false,
    }));
  });

  it("marks missing translations explicitly or rejects them in strict mode", () => {
    const cue: CaptionCue = {
      id: "cue-1",
      startMs: 0,
      endMs: 500,
      text: "Merhaba",
      words: [word("w1", "Merhaba", 0, 500)],
    };
    expect(createBilingualCues([cue], [])[0]).toEqual(expect.objectContaining({
      displayText: "Merhaba",
      translationMissing: true,
    }));
    expect(() => createBilingualCues([cue], [], { missingTranslation: "error" }))
      .toThrow("Çevirisi eksik cue");
  });
});

describe("integrated caption QA", () => {
  it("combines confidence, dictionary and code-switch signals per word", () => {
    const words = [
      word("w1", "Bugün", 0, 250),
      word("w2", "open", 260, 500, 0.5),
      word("w3", "ay", 500, 650, 0.55),
      word("w4", "meeting", 660, 1_000, 0.91),
    ];
    const result = analyzeCaptionCue(
      {
        id: "cue-qa",
        startMs: 0,
        endMs: 1_000,
        text: "Bugün open ay meeting",
        words,
      },
      { dictionary: brandDictionary, primaryLanguage: "tr" },
    );

    expect(result.dictionarySuggestions).toEqual([
      expect.objectContaining({ entryId: "openai", startWordIndex: 1, endWordIndex: 2 }),
    ]);
    expect(result.words[1]).toEqual(expect.objectContaining({
      confidenceBand: "low",
      color: CONFIDENCE_COLORS.low,
      needsReview: true,
    }));
    expect(result.words[3]).toEqual(expect.objectContaining({
      detectedLanguage: "en",
      isCodeSwitch: true,
    }));
    expect(result.words.slice(1, 3).every((word) => word.isCodeSwitch)).toBe(true);
    expect(result.reviewWordCount).toBe(2);
    expect(result.codeSwitchWordCount).toBe(3);
  });
});
