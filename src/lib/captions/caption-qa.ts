export type CaptionLanguage = "tr" | "en" | "unknown";

/** All caption timing fields in this module use integer or fractional milliseconds. */
export const CAPTION_TIME_UNIT = "milliseconds" as const;

export interface CaptionWord {
  id: string;
  text: string;
  startMs: number;
  endMs: number;
  confidence: number;
  speakerId?: string;
  language?: CaptionLanguage;
  edited?: boolean;
}

export interface CaptionSpeaker {
  id: string;
  label: string;
  color?: string;
  language?: Exclude<CaptionLanguage, "unknown">;
}

export interface CaptionCue {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  words: readonly CaptionWord[];
  speakerId?: string;
}

export type ConfidenceBand = "low" | "review" | "high";

export interface ConfidenceThresholds {
  lowBelow: number;
  reviewBelow: number;
}

export const DEFAULT_CONFIDENCE_THRESHOLDS: Readonly<ConfidenceThresholds> = {
  lowBelow: 0.72,
  reviewBelow: 0.86,
};

/** UI-safe defaults; consumers may replace these with theme tokens. */
export const CONFIDENCE_COLORS: Readonly<Record<ConfidenceBand, string>> = {
  low: "#ef4444",
  review: "#f59e0b",
  high: "#22c55e",
};

export type CaptionDictionaryCategory = "brand" | "proper-noun" | "term";

export interface CaptionDictionaryEntry {
  id: string;
  canonical: string;
  variants?: readonly string[];
  category: CaptionDictionaryCategory;
  language?: CaptionLanguage;
}

export interface CaptionDictionarySuggestion {
  entryId: string;
  category: CaptionDictionaryCategory;
  startWordIndex: number;
  endWordIndex: number;
  inputText: string;
  canonical: string;
  matchedForm: string;
  score: number;
}

export interface CaptionValidationIssue {
  wordIndex: number;
  field: "text" | "timing" | "confidence" | "order";
  message: string;
}

export interface WordLanguageAnnotation {
  wordId: string;
  wordIndex: number;
  language: Exclude<CaptionLanguage, "unknown">;
  source: "explicit" | "dictionary" | "lexicon" | "heuristic" | "inherited";
  isCodeSwitch: boolean;
}

export interface CaptionQaWord {
  wordId: string;
  wordIndex: number;
  confidenceBand: ConfidenceBand;
  color: string;
  needsReview: boolean;
  detectedLanguage: Exclude<CaptionLanguage, "unknown">;
  isCodeSwitch: boolean;
  dictionarySuggestion?: CaptionDictionarySuggestion;
}

export interface CaptionQaResult {
  words: CaptionQaWord[];
  dictionarySuggestions: CaptionDictionarySuggestion[];
  validationIssues: CaptionValidationIssue[];
  reviewWordCount: number;
  codeSwitchWordCount: number;
}

export interface AlignEditedTextOptions {
  cueStartMs?: number;
  cueEndMs?: number;
  speakerId?: string;
  language?: CaptionLanguage;
  editedConfidence?: number;
  idFactory?: (newWordIndex: number, text: string) => string;
}

export interface BuildSpeakerCuesOptions {
  maxGapMs?: number;
  maxDurationMs?: number;
  maxWords?: number;
  cueIdPrefix?: string;
}

export interface CaptionTranslation {
  cueId: string;
  text: string;
}

export interface BilingualCaptionCue {
  id: string;
  sourceCueId: string;
  startMs: number;
  endMs: number;
  speakerId?: string;
  sourceLanguage: Exclude<CaptionLanguage, "unknown">;
  targetLanguage: Exclude<CaptionLanguage, "unknown">;
  sourceText: string;
  translatedText: string;
  displayText: string;
  translationMissing: boolean;
}

export interface CreateBilingualCuesOptions {
  sourceLanguage?: Exclude<CaptionLanguage, "unknown">;
  targetLanguage?: Exclude<CaptionLanguage, "unknown">;
  order?: "source-first" | "translation-first";
  separator?: string;
  missingTranslation?: "source-only" | "error";
}

const DEFAULT_TURKISH_LEXICON = new Set([
  "ama",
  "ben",
  "bir",
  "biz",
  "bu",
  "bugun",
  "cok",
  "da",
  "de",
  "degil",
  "icin",
  "ile",
  "mi",
  "nasil",
  "ne",
  "sonra",
  "su",
  "ve",
  "ya",
]);

const DEFAULT_ENGLISH_LEXICON = new Set([
  "actually",
  "basically",
  "deadline",
  "download",
  "export",
  "feature",
  "feedback",
  "import",
  "meeting",
  "render",
  "review",
  "share",
  "timeline",
  "upload",
  "workflow",
]);

function assertConfidence(value: number, label = "confidence"): void {
  if (!Number.isFinite(value) || value < 0 || value > 1) {
    throw new RangeError(`${label} 0 ile 1 arasında olmalıdır.`);
  }
}

function normalizedToken(value: string): string {
  return value
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase("en-US")
    .replace(/ı/g, "i")
    .replace(/[^\p{L}\p{N}]+/gu, "");
}

function normalizedPhrase(value: string): string {
  return tokenizeCaptionText(value)
    .map(normalizedToken)
    .filter(Boolean)
    .join(" ");
}

function compactPhrase(value: string): string {
  return normalizedPhrase(value).replace(/\s+/g, "");
}

function levenshteinDistance(left: string, right: string): number {
  if (left === right) return 0;
  if (left.length === 0) return right.length;
  if (right.length === 0) return left.length;

  let previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  for (let leftIndex = 1; leftIndex <= left.length; leftIndex += 1) {
    const current = [leftIndex];
    for (let rightIndex = 1; rightIndex <= right.length; rightIndex += 1) {
      const substitution = previous[rightIndex - 1] +
        (left[leftIndex - 1] === right[rightIndex - 1] ? 0 : 1);
      current[rightIndex] = Math.min(
        previous[rightIndex] + 1,
        current[rightIndex - 1] + 1,
        substitution,
      );
    }
    previous = current;
  }
  return previous[right.length];
}

function normalizedEditDistance(left: string, right: string): number {
  const denominator = Math.max(left.length, right.length, 1);
  return levenshteinDistance(left, right) / denominator;
}

function wordDuration(word: Pick<CaptionWord, "startMs" | "endMs">): number {
  return Math.max(1, word.endMs - word.startMs);
}

function medianWordDuration(words: readonly CaptionWord[]): number {
  if (words.length === 0) return 400;
  const durations = words.map(wordDuration).sort((left, right) => left - right);
  return Math.max(80, durations[Math.floor(durations.length / 2)]);
}

function partitionInterval(startMs: number, endMs: number, count: number): Array<[number, number]> {
  if (count <= 0) return [];
  const safeEndMs = Math.max(startMs, endMs);
  const durationMs = safeEndMs - startMs;
  return Array.from({ length: count }, (_, index) => [
    Math.round(startMs + durationMs * index / count),
    Math.round(startMs + durationMs * (index + 1) / count),
  ]);
}

function dominantSpeaker(words: readonly CaptionWord[]): string | undefined {
  const counts = new Map<string, number>();
  for (const word of words) {
    if (word.speakerId) counts.set(word.speakerId, (counts.get(word.speakerId) ?? 0) + 1);
  }
  return [...counts.entries()].sort((left, right) => right[1] - left[1])[0]?.[0];
}

function exactTokenAnchors(
  oldWords: readonly CaptionWord[],
  newTokens: readonly string[],
): Array<[number, number]> {
  const oldTokens = oldWords.map((word) => normalizedToken(word.text));
  const normalizedNewTokens = newTokens.map(normalizedToken);
  const rows = oldWords.length + 1;
  const columns = newTokens.length + 1;
  const lengths = Array.from({ length: rows }, () => Array<number>(columns).fill(0));

  for (let oldIndex = oldWords.length - 1; oldIndex >= 0; oldIndex -= 1) {
    for (let newIndex = newTokens.length - 1; newIndex >= 0; newIndex -= 1) {
      lengths[oldIndex][newIndex] = oldTokens[oldIndex] === normalizedNewTokens[newIndex]
        ? lengths[oldIndex + 1][newIndex + 1] + 1
        : Math.max(lengths[oldIndex + 1][newIndex], lengths[oldIndex][newIndex + 1]);
    }
  }

  const anchors: Array<[number, number]> = [];
  let oldIndex = 0;
  let newIndex = 0;
  while (oldIndex < oldWords.length && newIndex < newTokens.length) {
    if (oldTokens[oldIndex] === normalizedNewTokens[newIndex]) {
      anchors.push([oldIndex, newIndex]);
      oldIndex += 1;
      newIndex += 1;
    } else if (lengths[oldIndex + 1][newIndex] >= lengths[oldIndex][newIndex + 1]) {
      oldIndex += 1;
    } else {
      newIndex += 1;
    }
  }
  return anchors;
}

export function tokenizeCaptionText(text: string): string[] {
  return text.trim().match(/\S+/gu) ?? [];
}

export function captionTextFromWords(words: readonly Pick<CaptionWord, "text">[]): string {
  return words.map((word) => word.text).join(" ");
}

export function classifyWordConfidence(
  confidence: number,
  thresholds: Partial<ConfidenceThresholds> = {},
): ConfidenceBand {
  assertConfidence(confidence);
  const resolved = { ...DEFAULT_CONFIDENCE_THRESHOLDS, ...thresholds };
  assertConfidence(resolved.lowBelow, "lowBelow");
  assertConfidence(resolved.reviewBelow, "reviewBelow");
  if (resolved.lowBelow >= resolved.reviewBelow) {
    throw new RangeError("lowBelow, reviewBelow değerinden küçük olmalıdır.");
  }
  if (confidence < resolved.lowBelow) return "low";
  if (confidence < resolved.reviewBelow) return "review";
  return "high";
}

export function validateCaptionWords(words: readonly CaptionWord[]): CaptionValidationIssue[] {
  const issues: CaptionValidationIssue[] = [];
  words.forEach((word, wordIndex) => {
    if (!word.text.trim()) {
      issues.push({ wordIndex, field: "text", message: "Kelime metni boş olamaz." });
    }
    if (
      !Number.isFinite(word.startMs) ||
      !Number.isFinite(word.endMs) ||
      word.startMs < 0 ||
      word.endMs < word.startMs
    ) {
      issues.push({
        wordIndex,
        field: "timing",
        message: "Kelime zamanları sonlu, pozitif ve artan olmalıdır.",
      });
    }
    if (!Number.isFinite(word.confidence) || word.confidence < 0 || word.confidence > 1) {
      issues.push({
        wordIndex,
        field: "confidence",
        message: "Kelime güven skoru 0 ile 1 arasında olmalıdır.",
      });
    }
    if (wordIndex > 0 && word.startMs < words[wordIndex - 1].startMs) {
      issues.push({
        wordIndex,
        field: "order",
        message: "Kelimeler başlangıç zamanına göre sıralı olmalıdır.",
      });
    }
  });
  return issues;
}

export function findDictionarySuggestions(
  words: readonly Pick<CaptionWord, "text">[],
  dictionary: readonly CaptionDictionaryEntry[],
  options: { maxDistance?: number } = {},
): CaptionDictionarySuggestion[] {
  const maxDistance = options.maxDistance ?? 0.34;
  if (!Number.isFinite(maxDistance) || maxDistance < 0 || maxDistance > 1) {
    throw new RangeError("maxDistance 0 ile 1 arasında olmalıdır.");
  }

  const candidates: CaptionDictionarySuggestion[] = [];
  for (const entry of dictionary) {
    if (!entry.id.trim() || !entry.canonical.trim()) continue;
    const forms = [entry.canonical, ...(entry.variants ?? [])]
      .filter((form, index, all) => form.trim() && all.indexOf(form) === index);

    for (const form of forms) {
      const formWordCount = Math.max(1, tokenizeCaptionText(form).length);
      const minimumWindow = Math.max(1, formWordCount - 1);
      const maximumWindow = formWordCount + 1;
      for (let startWordIndex = 0; startWordIndex < words.length; startWordIndex += 1) {
        for (let wordCount = minimumWindow; wordCount <= maximumWindow; wordCount += 1) {
          const endExclusive = startWordIndex + wordCount;
          if (endExclusive > words.length) continue;
          const inputText = words
            .slice(startWordIndex, endExclusive)
            .map((word) => word.text)
            .join(" ");
          if (inputText === entry.canonical) continue;

          const spacedDistance = normalizedEditDistance(normalizedPhrase(inputText), normalizedPhrase(form));
          const compactDistance = normalizedEditDistance(compactPhrase(inputText), compactPhrase(form));
          const distance = Math.min(spacedDistance, compactDistance);
          const normalizedInput = compactPhrase(inputText);
          if (normalizedInput.length < 3 && distance > 0) continue;
          if (distance <= maxDistance) {
            candidates.push({
              entryId: entry.id,
              category: entry.category,
              startWordIndex,
              endWordIndex: endExclusive - 1,
              inputText,
              canonical: entry.canonical,
              matchedForm: form,
              score: 1 - distance,
            });
          }
        }
      }
    }
  }

  candidates.sort((left, right) =>
    right.score - left.score ||
    (right.endWordIndex - right.startWordIndex) - (left.endWordIndex - left.startWordIndex) ||
    left.startWordIndex - right.startWordIndex,
  );

  const occupied = new Set<number>();
  const selected: CaptionDictionarySuggestion[] = [];
  for (const candidate of candidates) {
    const indices = Array.from(
      { length: candidate.endWordIndex - candidate.startWordIndex + 1 },
      (_, offset) => candidate.startWordIndex + offset,
    );
    if (indices.some((index) => occupied.has(index))) continue;
    indices.forEach((index) => occupied.add(index));
    selected.push(candidate);
  }
  return selected.sort((left, right) => left.startWordIndex - right.startWordIndex);
}

export function markCodeSwitch(
  words: readonly CaptionWord[],
  primaryLanguage: Exclude<CaptionLanguage, "unknown">,
  options: {
    dictionary?: readonly CaptionDictionaryEntry[];
    turkishLexicon?: readonly string[];
    englishLexicon?: readonly string[];
  } = {},
): WordLanguageAnnotation[] {
  const turkishLexicon = new Set([
    ...DEFAULT_TURKISH_LEXICON,
    ...(options.turkishLexicon ?? []).map(normalizedToken),
  ]);
  const englishLexicon = new Set([
    ...DEFAULT_ENGLISH_LEXICON,
    ...(options.englishLexicon ?? []).map(normalizedToken),
  ]);
  const dictionaryLanguages = new Map<number, Exclude<CaptionLanguage, "unknown">>();
  for (const entry of options.dictionary ?? []) {
    if (!entry.language || entry.language === "unknown") continue;
    for (const form of [entry.canonical, ...(entry.variants ?? [])]) {
      const formWordCount = Math.max(1, tokenizeCaptionText(form).length);
      for (let startWordIndex = 0; startWordIndex < words.length; startWordIndex += 1) {
        for (
          let wordCount = Math.max(1, formWordCount - 1);
          wordCount <= formWordCount + 1;
          wordCount += 1
        ) {
          const endExclusive = startWordIndex + wordCount;
          if (endExclusive > words.length) continue;
          const inputText = words
            .slice(startWordIndex, endExclusive)
            .map((word) => word.text)
            .join(" ");
          if (
            normalizedPhrase(inputText) === normalizedPhrase(form) ||
            compactPhrase(inputText) === compactPhrase(form)
          ) {
            for (let wordIndex = startWordIndex; wordIndex < endExclusive; wordIndex += 1) {
              dictionaryLanguages.set(wordIndex, entry.language);
            }
          }
        }
      }
    }
  }

  return words.map((word, wordIndex) => {
    const token = normalizedToken(word.text);
    let language: Exclude<CaptionLanguage, "unknown"> = primaryLanguage;
    let source: WordLanguageAnnotation["source"] = "inherited";

    if (word.language && word.language !== "unknown") {
      language = word.language;
      source = "explicit";
    } else {
      const dictionaryLanguage = dictionaryLanguages.get(wordIndex);
      if (dictionaryLanguage) {
        language = dictionaryLanguage;
        source = "dictionary";
      } else {
        const inTurkish = turkishLexicon.has(token);
        const inEnglish = englishLexicon.has(token);
        if (inTurkish !== inEnglish) {
          language = inEnglish ? "en" : "tr";
          source = "lexicon";
        } else if (/[çğıöşü]/iu.test(word.text)) {
          language = "tr";
          source = "heuristic";
        }
      }
    }

    return {
      wordId: word.id,
      wordIndex,
      language,
      source,
      isCodeSwitch: language !== primaryLanguage,
    };
  });
}

/**
 * Aligns edited tokens against exact old-token anchors. Exact tokens retain their
 * IDs/timings; replacements retain one-to-one timings and split/merge edits share
 * the complete replaced span.
 */
export function alignEditedCaptionText(
  oldWords: readonly CaptionWord[],
  editedText: string,
  options: AlignEditedTextOptions = {},
): CaptionWord[] {
  const validationIssues = validateCaptionWords(oldWords);
  if (validationIssues.length > 0) {
    throw new Error(`Geçersiz caption kelimeleri: ${validationIssues[0].message}`);
  }
  const editedConfidence = options.editedConfidence ?? 1;
  assertConfidence(editedConfidence, "editedConfidence");
  const newTokens = tokenizeCaptionText(editedText);
  if (newTokens.length === 0) return [];

  const anchors = exactTokenAnchors(oldWords, newTokens);
  const sentinels: Array<[number, number]> = [
    [-1, -1],
    ...anchors,
    [oldWords.length, newTokens.length],
  ];
  const result = new Array<CaptionWord>(newTokens.length);
  const usedIds = new Set(oldWords.map((word) => word.id));
  let generatedIdCounter = 1;

  const createId = (newWordIndex: number, text: string): string => {
    const requested = options.idFactory?.(newWordIndex, text);
    if (requested) {
      if (usedIds.has(requested)) throw new Error(`Tekrarlanan caption kelime kimliği: ${requested}`);
      usedIds.add(requested);
      return requested;
    }
    let candidate = `edited-word-${generatedIdCounter}`;
    while (usedIds.has(candidate)) {
      generatedIdCounter += 1;
      candidate = `edited-word-${generatedIdCounter}`;
    }
    generatedIdCounter += 1;
    usedIds.add(candidate);
    return candidate;
  };

  const inferredDuration = medianWordDuration(oldWords);
  for (let anchorIndex = 0; anchorIndex < sentinels.length - 1; anchorIndex += 1) {
    const [previousOldIndex, previousNewIndex] = sentinels[anchorIndex];
    const [nextOldIndex, nextNewIndex] = sentinels[anchorIndex + 1];
    const oldStart = previousOldIndex + 1;
    const oldEnd = nextOldIndex;
    const newStart = previousNewIndex + 1;
    const newEnd = nextNewIndex;
    const oldBlock = oldWords.slice(oldStart, oldEnd);
    const newBlock = newTokens.slice(newStart, newEnd);

    if (newBlock.length > 0) {
      let intervals: Array<[number, number]>;
      if (oldBlock.length > 0 && oldBlock.length === newBlock.length) {
        intervals = oldBlock.map((word) => [word.startMs, word.endMs]);
      } else if (oldBlock.length > 0) {
        intervals = partitionInterval(
          oldBlock[0].startMs,
          oldBlock[oldBlock.length - 1].endMs,
          newBlock.length,
        );
      } else {
        const leftWord = previousOldIndex >= 0 ? oldWords[previousOldIndex] : undefined;
        const rightWord = nextOldIndex < oldWords.length ? oldWords[nextOldIndex] : undefined;
        let insertionStart: number;
        let insertionEnd: number;
        if (leftWord && rightWord && rightWord.startMs > leftWord.endMs) {
          insertionStart = leftWord.endMs;
          insertionEnd = rightWord.startMs;
        } else if (leftWord) {
          insertionStart = leftWord.endMs;
          insertionEnd = Math.min(
            options.cueEndMs ?? Number.POSITIVE_INFINITY,
            insertionStart + inferredDuration * newBlock.length,
          );
          if (!Number.isFinite(insertionEnd) || insertionEnd <= insertionStart) {
            insertionEnd = insertionStart + newBlock.length;
          }
        } else if (rightWord) {
          insertionEnd = rightWord.startMs;
          insertionStart = Math.max(
            options.cueStartMs ?? 0,
            insertionEnd - inferredDuration * newBlock.length,
          );
          if (insertionEnd <= insertionStart) insertionEnd = insertionStart + newBlock.length;
        } else {
          insertionStart = Math.max(0, options.cueStartMs ?? 0);
          insertionEnd = Math.max(
            insertionStart + newBlock.length,
            options.cueEndMs ?? insertionStart + inferredDuration * newBlock.length,
          );
        }
        intervals = partitionInterval(insertionStart, insertionEnd, newBlock.length);
      }

      const speakerId = options.speakerId ?? dominantSpeaker(oldBlock) ??
        oldWords[previousOldIndex]?.speakerId ?? oldWords[nextOldIndex]?.speakerId;
      newBlock.forEach((text, blockIndex) => {
        const newWordIndex = newStart + blockIndex;
        const replacedWord = oldBlock.length === newBlock.length ? oldBlock[blockIndex] : undefined;
        result[newWordIndex] = {
          id: replacedWord?.id ?? createId(newWordIndex, text),
          text,
          startMs: intervals[blockIndex][0],
          endMs: intervals[blockIndex][1],
          confidence: editedConfidence,
          speakerId: options.speakerId ?? replacedWord?.speakerId ?? speakerId,
          language: options.language,
          edited: true,
        };
      });
    }

    if (nextOldIndex < oldWords.length && nextNewIndex < newTokens.length) {
      const oldWord = oldWords[nextOldIndex];
      const newText = newTokens[nextNewIndex];
      const textChanged = oldWord.text !== newText;
      result[nextNewIndex] = {
        ...oldWord,
        text: newText,
        confidence: textChanged ? editedConfidence : oldWord.confidence,
        language: textChanged ? options.language : oldWord.language,
        edited: textChanged || oldWord.edited,
      };
    }
  }
  return result;
}

export function applyDictionarySuggestion(
  words: readonly CaptionWord[],
  suggestion: CaptionDictionarySuggestion,
  options: AlignEditedTextOptions = {},
): CaptionWord[] {
  if (
    suggestion.startWordIndex < 0 ||
    suggestion.endWordIndex < suggestion.startWordIndex ||
    suggestion.endWordIndex >= words.length
  ) {
    throw new RangeError("Sözlük önerisinin kelime aralığı geçersiz.");
  }
  const editedTokens = [
    ...words.slice(0, suggestion.startWordIndex).map((word) => word.text),
    ...tokenizeCaptionText(suggestion.canonical),
    ...words.slice(suggestion.endWordIndex + 1).map((word) => word.text),
  ];
  return alignEditedCaptionText(words, editedTokens.join(" "), options);
}

export function buildSpeakerCues(
  words: readonly CaptionWord[],
  options: BuildSpeakerCuesOptions = {},
): CaptionCue[] {
  const validationIssues = validateCaptionWords(words);
  if (validationIssues.length > 0) {
    throw new Error(`Geçersiz caption kelimeleri: ${validationIssues[0].message}`);
  }
  if (words.length === 0) return [];
  const maxGapMs = options.maxGapMs ?? 900;
  const maxDurationMs = options.maxDurationMs ?? 6_000;
  const maxWords = options.maxWords ?? 14;
  if (maxGapMs < 0 || maxDurationMs <= 0 || maxWords <= 0) {
    throw new RangeError("Cue bölümleme sınırları pozitif olmalıdır.");
  }

  const groups: CaptionWord[][] = [];
  let current: CaptionWord[] = [];
  for (const word of words) {
    const previous = current[current.length - 1];
    const first = current[0];
    const shouldSplit = Boolean(previous) && (
      previous.speakerId !== word.speakerId ||
      word.startMs - previous.endMs > maxGapMs ||
      word.endMs - first.startMs > maxDurationMs ||
      current.length >= maxWords
    );
    if (shouldSplit) {
      groups.push(current);
      current = [];
    }
    current.push({ ...word });
  }
  if (current.length > 0) groups.push(current);

  const prefix = options.cueIdPrefix ?? "speaker-cue";
  return groups.map((group, index) => ({
    id: `${prefix}-${index + 1}`,
    startMs: group[0].startMs,
    endMs: group[group.length - 1].endMs,
    text: captionTextFromWords(group),
    words: group,
    speakerId: group[0].speakerId,
  }));
}

export function createBilingualCues(
  sourceCues: readonly CaptionCue[],
  translations: readonly CaptionTranslation[],
  options: CreateBilingualCuesOptions = {},
): BilingualCaptionCue[] {
  const translationByCueId = new Map<string, string>();
  for (const translation of translations) {
    if (translationByCueId.has(translation.cueId)) {
      throw new Error(`Aynı cue için birden fazla çeviri var: ${translation.cueId}`);
    }
    translationByCueId.set(translation.cueId, translation.text.trim());
  }

  const sourceLanguage = options.sourceLanguage ?? "tr";
  const targetLanguage = options.targetLanguage ?? "en";
  const order = options.order ?? "source-first";
  const separator = options.separator ?? "\n";
  const missingTranslation = options.missingTranslation ?? "source-only";

  return sourceCues.map((cue) => {
    const sourceText = cue.text.trim() || captionTextFromWords(cue.words);
    const translatedText = translationByCueId.get(cue.id) ?? "";
    const translationMissing = translatedText.length === 0;
    if (translationMissing && missingTranslation === "error") {
      throw new Error(`Çevirisi eksik cue: ${cue.id}`);
    }
    const displayText = translationMissing
      ? sourceText
      : order === "source-first"
        ? `${sourceText}${separator}${translatedText}`
        : `${translatedText}${separator}${sourceText}`;
    return {
      id: `${cue.id}-bilingual`,
      sourceCueId: cue.id,
      startMs: cue.startMs,
      endMs: cue.endMs,
      speakerId: cue.speakerId,
      sourceLanguage,
      targetLanguage,
      sourceText,
      translatedText,
      displayText,
      translationMissing,
    };
  });
}

export function analyzeCaptionCue(
  cue: CaptionCue,
  options: {
    primaryLanguage?: Exclude<CaptionLanguage, "unknown">;
    dictionary?: readonly CaptionDictionaryEntry[];
    thresholds?: Partial<ConfidenceThresholds>;
    turkishLexicon?: readonly string[];
    englishLexicon?: readonly string[];
  } = {},
): CaptionQaResult {
  const validationIssues = validateCaptionWords(cue.words);
  const dictionarySuggestions = findDictionarySuggestions(
    cue.words,
    options.dictionary ?? [],
  );
  const languageAnnotations = markCodeSwitch(cue.words, options.primaryLanguage ?? "tr", {
    dictionary: options.dictionary,
    turkishLexicon: options.turkishLexicon,
    englishLexicon: options.englishLexicon,
  });

  const words = cue.words.map((word, wordIndex): CaptionQaWord => {
    const confidenceBand = Number.isFinite(word.confidence) && word.confidence >= 0 && word.confidence <= 1
      ? classifyWordConfidence(word.confidence, options.thresholds)
      : "low";
    const dictionarySuggestion = dictionarySuggestions.find((suggestion) =>
      wordIndex >= suggestion.startWordIndex && wordIndex <= suggestion.endWordIndex,
    );
    const language = languageAnnotations[wordIndex];
    return {
      wordId: word.id,
      wordIndex,
      confidenceBand,
      color: CONFIDENCE_COLORS[confidenceBand],
      needsReview: confidenceBand !== "high" || Boolean(dictionarySuggestion),
      detectedLanguage: language.language,
      isCodeSwitch: language.isCodeSwitch,
      dictionarySuggestion,
    };
  });

  return {
    words,
    dictionarySuggestions,
    validationIssues,
    reviewWordCount: words.filter((word) => word.needsReview).length,
    codeSwitchWordCount: words.filter((word) => word.isCodeSwitch).length,
  };
}
