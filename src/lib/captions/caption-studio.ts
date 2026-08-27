import {
  DEFAULT_CONFIDENCE_THRESHOLDS,
  alignEditedCaptionText,
  analyzeCaptionCue,
  applyDictionarySuggestion,
  buildSpeakerCues,
  captionTextFromWords,
  createBilingualCues,
  tokenizeCaptionText,
  type BilingualCaptionCue,
  type CaptionCue,
  type CaptionDictionaryEntry,
  type CaptionDictionarySuggestion,
  type CaptionLanguage,
  type CaptionSpeaker,
  type CaptionTranslation,
  type CaptionWord,
  type ConfidenceThresholds,
} from "./caption-qa";
import type {
  CaptionDocument as FormatCaptionDocument,
  CaptionFormat,
} from "./caption-formats";

export const CAPTION_STUDIO_VERSION = 1 as const;

export type CaptionTemplateId = "clean" | "pop" | "slide";
export type CaptionConfidenceMode = "word" | "unavailable";

export interface CaptionStudioProject {
  version: typeof CAPTION_STUDIO_VERSION;
  cues: CaptionCue[];
  speakers: CaptionSpeaker[];
  dictionary: CaptionDictionaryEntry[];
  translations: CaptionTranslation[];
  thresholds: ConfidenceThresholds;
  primaryLanguage: Exclude<CaptionLanguage, "unknown">;
  targetLanguage: Exclude<CaptionLanguage, "unknown">;
  bilingual: boolean;
  bilingualOrder: "source-first" | "translation-first";
  template: CaptionTemplateId;
  confidenceMode: CaptionConfidenceMode;
  sourceFormat?: CaptionFormat;
  sourceName?: string;
  /** Parsed format metadata is retained so VTT/ASS round-trips keep styles. */
  sourceDocument?: FormatCaptionDocument;
}

export interface CaptionQaSummary {
  cueCount: number;
  wordCount: number;
  reviewWordCount: number;
  lowConfidenceCount: number;
  codeSwitchWordCount: number;
  dictionarySuggestionCount: number;
  missingTranslationCount: number;
}

export interface WordTranscriptInput {
  text: string;
  startMs: number;
  endMs: number;
  confidence: number;
  speakerId?: string;
  language?: CaptionLanguage;
}

export function createEmptyCaptionStudioProject(): CaptionStudioProject {
  return {
    version: CAPTION_STUDIO_VERSION,
    cues: [],
    speakers: [],
    dictionary: [],
    translations: [],
    thresholds: { ...DEFAULT_CONFIDENCE_THRESHOLDS },
    primaryLanguage: "tr",
    targetLanguage: "en",
    bilingual: false,
    bilingualOrder: "source-first",
    template: "clean",
    confidenceMode: "unavailable",
  };
}

export function captionStudioFromDocument(
  document: FormatCaptionDocument,
  options: {
    sourceName?: string;
    primaryLanguage?: Exclude<CaptionLanguage, "unknown">;
  } = {},
): CaptionStudioProject {
  const speakerByLabel = new Map<string, CaptionSpeaker>();
  const usedCueIds = new Set<string>();
  const primaryLanguage = options.primaryLanguage ?? "tr";
  const cues = document.cues.map((sourceCue, cueIndex): CaptionCue => {
    const requestedCueId = sourceCue.id?.trim() || `cue-${cueIndex + 1}`;
    let cueId = requestedCueId;
    let duplicateIndex = 2;
    while (usedCueIds.has(cueId)) {
      cueId = `${requestedCueId}-${duplicateIndex}`;
      duplicateIndex += 1;
    }
    usedCueIds.add(cueId);
    const startMs = Math.max(0, finiteOr(sourceCue.startMs, 0));
    const endMs = Math.max(startMs + 1, finiteOr(sourceCue.endMs, startMs + 1));
    const speaker = sourceCue.speaker?.trim();
    let speakerId: string | undefined;
    if (speaker) {
      const key = speaker.toLocaleLowerCase("tr-TR");
      let entry = speakerByLabel.get(key);
      if (!entry) {
        entry = {
          id: `speaker-${speakerByLabel.size + 1}`,
          label: speaker,
          color: speakerColor(speakerByLabel.size),
          language: primaryLanguage,
        };
        speakerByLabel.set(key, entry);
      }
      speakerId = entry.id;
    }
    const tokens = tokenizeCaptionText(sourceCue.text);
    const words = distributeWords(tokens, startMs, endMs, cueId, speakerId);
    return {
      id: cueId,
      startMs,
      endMs,
      text: sourceCue.text.trim(),
      words,
      speakerId,
    };
  });

  return {
    ...createEmptyCaptionStudioProject(),
    cues,
    speakers: [...speakerByLabel.values()],
    primaryLanguage,
    sourceFormat: document.format,
    sourceName: options.sourceName,
    sourceDocument: cloneFormatDocument(document),
  };
}

/** Converts real word-level ASR output into editable QA cues. */
export function captionStudioFromWordTranscript(
  input: readonly WordTranscriptInput[],
  options: {
    timelineOffsetMs?: number;
    sourceName?: string;
    primaryLanguage?: Exclude<CaptionLanguage, "unknown">;
    speakers?: readonly CaptionSpeaker[];
  } = {},
): CaptionStudioProject {
  const offsetMs = Math.max(0, finiteOr(options.timelineOffsetMs, 0));
  const words: CaptionWord[] = input
    .map((word, index) => ({
      id: `asr-word-${index + 1}-${makeId("word")}`,
      text: word.text.trim(),
      startMs: offsetMs + Math.max(0, finiteOr(word.startMs, 0)),
      endMs: offsetMs + Math.max(0, finiteOr(word.endMs, 0)),
      confidence: Math.max(0, Math.min(1, finiteOr(word.confidence, 0))),
      speakerId: word.speakerId,
      language: word.language,
    }))
    .filter((word) => word.text && word.endMs >= word.startMs)
    .sort((left, right) => left.startMs - right.startMs || left.endMs - right.endMs);
  const cues = buildSpeakerCues(words, {
    maxGapMs: 850,
    maxDurationMs: 5_500,
    maxWords: 12,
    cueIdPrefix: "asr-cue",
  });
  return {
    ...createEmptyCaptionStudioProject(),
    cues,
    speakers: [...(options.speakers ?? [])],
    primaryLanguage: options.primaryLanguage ?? "tr",
    sourceName: options.sourceName,
    confidenceMode: "word",
  };
}

export function normalizeCaptionStudioProject(value: unknown): CaptionStudioProject {
  const fallback = createEmptyCaptionStudioProject();
  if (!value || typeof value !== "object" || Array.isArray(value)) return fallback;
  const candidate = value as Partial<CaptionStudioProject>;
  const normalizedCues = Array.isArray(candidate.cues)
    ? candidate.cues.map(normalizeCue).filter((cue): cue is CaptionCue => cue !== null)
    : [];
  const cues = deduplicateCaptionCueIds(normalizedCues);
  const speakers = Array.isArray(candidate.speakers)
    ? candidate.speakers.filter(isCaptionSpeaker).map((speaker) => ({ ...speaker }))
    : [];
  const dictionary = Array.isArray(candidate.dictionary)
    ? candidate.dictionary.filter(isDictionaryEntry).map((entry) => ({
        ...entry,
        variants: [...(entry.variants ?? [])],
      }))
    : [];
  const translations = Array.isArray(candidate.translations)
    ? candidate.translations.filter(isTranslation).map((translation) => ({ ...translation }))
    : [];
  const lowBelow = finiteOr(candidate.thresholds?.lowBelow, fallback.thresholds.lowBelow);
  const reviewBelow = finiteOr(
    candidate.thresholds?.reviewBelow,
    fallback.thresholds.reviewBelow,
  );

  return {
    ...fallback,
    ...candidate,
    version: CAPTION_STUDIO_VERSION,
    cues,
    speakers,
    dictionary,
    translations,
    thresholds:
      lowBelow >= 0 && reviewBelow <= 1 && lowBelow < reviewBelow
        ? { lowBelow, reviewBelow }
        : { ...fallback.thresholds },
    primaryLanguage: candidate.primaryLanguage === "en" ? "en" : "tr",
    targetLanguage: candidate.targetLanguage === "tr" ? "tr" : "en",
    bilingual: Boolean(candidate.bilingual),
    bilingualOrder:
      candidate.bilingualOrder === "translation-first"
        ? "translation-first"
        : "source-first",
    template:
      candidate.template === "pop" || candidate.template === "slide"
        ? candidate.template
        : "clean",
    confidenceMode: candidate.confidenceMode === "word" ? "word" : "unavailable",
    sourceFormat:
      candidate.sourceFormat === "vtt" || candidate.sourceFormat === "ass"
        ? candidate.sourceFormat
        : candidate.sourceFormat === "srt"
          ? "srt"
          : undefined,
    sourceDocument:
      candidate.sourceDocument && typeof candidate.sourceDocument === "object"
        ? cloneFormatDocument(candidate.sourceDocument)
        : undefined,
  };
}

export function updateCaptionCueText(
  project: CaptionStudioProject,
  cueId: string,
  editedText: string,
): CaptionStudioProject {
  const cue = project.cues.find((item) => item.id === cueId);
  if (!cue) return project;
  const words = alignEditedCaptionText(cue.words, editedText, {
    cueStartMs: cue.startMs,
    cueEndMs: cue.endMs,
    speakerId: cue.speakerId,
    editedConfidence: 1,
    idFactory: (index) => `${cue.id}-edited-${index + 1}-${makeId("word")}`,
  });
  return replaceCue(project, cueId, {
    ...cue,
    text: editedText,
    words,
  });
}

export function applyCaptionDictionarySuggestion(
  project: CaptionStudioProject,
  cueId: string,
  suggestion: CaptionDictionarySuggestion,
): CaptionStudioProject {
  const cue = project.cues.find((item) => item.id === cueId);
  if (!cue) return project;
  const words = applyDictionarySuggestion(cue.words, suggestion, {
    cueStartMs: cue.startMs,
    cueEndMs: cue.endMs,
    speakerId: cue.speakerId,
    editedConfidence: 1,
    idFactory: (index) => `${cue.id}-dict-${index + 1}-${makeId("word")}`,
  });
  return replaceCue(project, cueId, {
    ...cue,
    text: captionTextFromWords(words),
    words,
  });
}

export function setCaptionCueSpeaker(
  project: CaptionStudioProject,
  cueId: string,
  speakerId?: string,
): CaptionStudioProject {
  const cue = project.cues.find((item) => item.id === cueId);
  if (!cue) return project;
  const normalizedSpeaker = speakerId || undefined;
  return replaceCue(project, cueId, {
    ...cue,
    speakerId: normalizedSpeaker,
    words: cue.words.map((word) => ({ ...word, speakerId: normalizedSpeaker })),
  });
}

export function upsertCaptionTranslation(
  project: CaptionStudioProject,
  cueId: string,
  text: string,
): CaptionStudioProject {
  const translations = project.translations.filter((item) => item.cueId !== cueId);
  if (text.trim()) translations.push({ cueId, text });
  return { ...project, translations };
}

export function createCaptionDictionaryEntry(input: {
  canonical: string;
  variants?: string;
  category?: CaptionDictionaryEntry["category"];
  language?: CaptionLanguage;
}): CaptionDictionaryEntry {
  const canonical = input.canonical.trim();
  if (!canonical) throw new Error("Sözlük terimi boş olamaz.");
  const variants = (input.variants ?? "")
    .split(/[,;\n]/u)
    .map((item) => item.trim())
    .filter(Boolean);
  return {
    id: makeId("dictionary"),
    canonical,
    variants,
    category: input.category ?? "proper-noun",
    language: input.language ?? "tr",
  };
}

export function bilingualCaptionCues(
  project: CaptionStudioProject,
): BilingualCaptionCue[] {
  return createBilingualCues(project.cues, project.translations, {
    sourceLanguage: project.primaryLanguage,
    targetLanguage: project.targetLanguage,
    order: project.bilingualOrder,
    missingTranslation: "source-only",
  });
}

export function captionDisplayCues(project: CaptionStudioProject): Array<{
  id: string;
  startMs: number;
  endMs: number;
  speakerId?: string;
  text: string;
  translationMissing: boolean;
}> {
  if (!project.bilingual) {
    return project.cues.map((cue) => ({
      id: cue.id,
      startMs: cue.startMs,
      endMs: cue.endMs,
      speakerId: cue.speakerId,
      text: cue.text || captionTextFromWords(cue.words),
      translationMissing: false,
    }));
  }
  return bilingualCaptionCues(project).map((cue) => ({
    id: cue.sourceCueId,
    startMs: cue.startMs,
    endMs: cue.endMs,
    speakerId: cue.speakerId,
    text: cue.displayText,
    translationMissing: cue.translationMissing,
  }));
}

export function captionQaSummary(project: CaptionStudioProject): CaptionQaSummary {
  let wordCount = 0;
  let reviewWordCount = 0;
  let lowConfidenceCount = 0;
  let codeSwitchWordCount = 0;
  let dictionarySuggestionCount = 0;
  for (const cue of project.cues) {
    const result = analyzeCaptionCue(cue, {
      primaryLanguage: project.primaryLanguage,
      dictionary: project.dictionary,
      thresholds: project.thresholds,
    });
    wordCount += cue.words.length;
    reviewWordCount += project.confidenceMode === "word"
      ? result.reviewWordCount
      : result.words.filter((word) => Boolean(word.dictionarySuggestion)).length;
    lowConfidenceCount += project.confidenceMode === "word"
      ? result.words.filter((word) => word.confidenceBand === "low").length
      : 0;
    codeSwitchWordCount += result.codeSwitchWordCount;
    dictionarySuggestionCount += result.dictionarySuggestions.length;
  }
  const translated = new Set(
    project.translations.filter((item) => item.text.trim()).map((item) => item.cueId),
  );
  return {
    cueCount: project.cues.length,
    wordCount,
    reviewWordCount,
    lowConfidenceCount,
    codeSwitchWordCount,
    dictionarySuggestionCount,
    missingTranslationCount: project.cues.filter((cue) => !translated.has(cue.id)).length,
  };
}

export function captionDocumentFromStudio(
  project: CaptionStudioProject,
  options: { bilingual?: boolean; format?: CaptionFormat } = {},
): FormatCaptionDocument {
  const source = project.sourceDocument
    ? cloneFormatDocument(project.sourceDocument)
    : { cues: [] };
  const originals = new Map(
    source.cues.map((cue, index) => [cue.id || project.cues[index]?.id || `cue-${index + 1}`, cue]),
  );
  const display = options.bilingual
    ? new Map(bilingualCaptionCues(project).map((cue) => [cue.sourceCueId, cue.displayText]))
    : new Map<string, string>();
  const speakerLabels = new Map(project.speakers.map((speaker) => [speaker.id, speaker.label]));
  return {
    ...source,
    format: options.format ?? project.sourceFormat ?? "srt",
    cues: project.cues.map((cue, index) => ({
      ...(originals.get(cue.id) ?? source.cues[index] ?? {}),
      id: cue.id,
      startMs: cue.startMs,
      endMs: cue.endMs,
      text: display.get(cue.id) ?? cue.text ?? captionTextFromWords(cue.words),
      speaker: cue.speakerId ? speakerLabels.get(cue.speakerId) : undefined,
    })),
  };
}

function replaceCue(
  project: CaptionStudioProject,
  cueId: string,
  cue: CaptionCue,
): CaptionStudioProject {
  return {
    ...project,
    cues: project.cues.map((item) => (item.id === cueId ? cue : item)),
  };
}

function distributeWords(
  tokens: readonly string[],
  startMs: number,
  endMs: number,
  cueId: string,
  speakerId?: string,
) {
  const duration = Math.max(1, endMs - startMs);
  return tokens.map((text, index) => ({
    id: `${cueId}-word-${index + 1}`,
    text,
    startMs: Math.round(startMs + (duration * index) / Math.max(1, tokens.length)),
    endMs: Math.round(startMs + (duration * (index + 1)) / Math.max(1, tokens.length)),
    confidence: 1,
    speakerId,
  }));
}

function normalizeCue(value: unknown): CaptionCue | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const cue = value as Partial<CaptionCue>;
  if (!cue.id || !Number.isFinite(cue.startMs) || !Number.isFinite(cue.endMs)) return null;
  const startMs = Math.max(0, cue.startMs as number);
  const endMs = Math.max(startMs + 1, cue.endMs as number);
  const words = Array.isArray(cue.words)
    ? cue.words.filter((word) =>
        Boolean(
          word &&
            typeof word.id === "string" &&
            typeof word.text === "string" &&
            Number.isFinite(word.startMs) &&
            Number.isFinite(word.endMs) &&
            Number.isFinite(word.confidence),
        ),
      ).map((word) => ({ ...word }))
    : [];
  const text = typeof cue.text === "string" ? cue.text : captionTextFromWords(words);
  return {
    id: cue.id,
    startMs,
    endMs,
    text,
    words,
    speakerId: typeof cue.speakerId === "string" ? cue.speakerId : undefined,
  };
}

function deduplicateCaptionCueIds(cues: readonly CaptionCue[]): CaptionCue[] {
  const cueIds = new Set<string>();
  return cues.map((cue, cueIndex) => {
    const cueId = claimUniqueId(cue.id, cueIds, `cue-${cueIndex + 1}`);
    const wordIds = new Set<string>();
    const words = cue.words.map((word, wordIndex) => ({
      ...word,
      id: claimUniqueId(word.id, wordIds, `${cueId}-word-${wordIndex + 1}`),
    }));
    return cueId === cue.id && words.every((word, index) => word.id === cue.words[index]?.id)
      ? cue
      : { ...cue, id: cueId, words };
  });
}

function claimUniqueId(rawId: string, usedIds: Set<string>, fallback: string): string {
  const base = rawId || fallback;
  let candidate = base;
  let suffix = 2;
  while (usedIds.has(candidate)) {
    candidate = `${base}-${suffix}`;
    suffix += 1;
  }
  usedIds.add(candidate);
  return candidate;
}

function isCaptionSpeaker(value: unknown): value is CaptionSpeaker {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const speaker = value as Partial<CaptionSpeaker>;
  return typeof speaker.id === "string" && typeof speaker.label === "string";
}

function isDictionaryEntry(value: unknown): value is CaptionDictionaryEntry {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const entry = value as Partial<CaptionDictionaryEntry>;
  return (
    typeof entry.id === "string" &&
    typeof entry.canonical === "string" &&
    (entry.category === "brand" || entry.category === "proper-noun" || entry.category === "term")
  );
}

function isTranslation(value: unknown): value is CaptionTranslation {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const translation = value as Partial<CaptionTranslation>;
  return typeof translation.cueId === "string" && typeof translation.text === "string";
}

function cloneFormatDocument(document: FormatCaptionDocument): FormatCaptionDocument {
  return JSON.parse(JSON.stringify(document)) as FormatCaptionDocument;
}

function finiteOr(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function speakerColor(index: number): string {
  return ["#65d7b2", "#79a8ff", "#d59cff", "#ffbd70", "#ff7f91"][index % 5];
}

function makeId(prefix: string): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}
