export type CaptionFormat = "srt" | "vtt" | "ass";

export interface CaptionDocument {
  format?: CaptionFormat;
  cues: CaptionCue[];
  title?: string;
  styles?: AssStyle[];
  vtt?: VttDocumentMetadata;
  ass?: AssDocumentMetadata;
}

export interface CaptionCue {
  id?: string;
  startMs: number;
  endMs: number;
  text: string;
  speaker?: string;
  style?: string;
  settings?: string;
  vtt?: VttCueMetadata;
  ass?: AssCueMetadata;
}

export interface VttDocumentMetadata {
  header?: string;
  headerLines?: string[];
  blocks?: VttBlock[];
}

export interface VttBlock {
  type: "NOTE" | "STYLE" | "REGION";
  text: string;
}

export interface VttCueMetadata {
  classes?: string[];
  voiceClasses?: string[];
}

export interface AssDocumentMetadata {
  scriptInfo?: Record<string, string>;
  styleFormat?: string[];
  eventFormat?: string[];
}

export interface AssStyle {
  name: string;
  fontName?: string;
  fontSize?: number;
  primaryColor?: string;
  secondaryColor?: string;
  outlineColor?: string;
  backColor?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikeOut?: boolean;
  scaleX?: number;
  scaleY?: number;
  spacing?: number;
  angle?: number;
  borderStyle?: number;
  outline?: number;
  shadow?: number;
  alignment?: number;
  marginL?: number;
  marginR?: number;
  marginV?: number;
  encoding?: number;
  /** Original and non-standard ASS style fields, keyed by their Format name. */
  fields?: Record<string, string>;
}

export interface AssCueMetadata {
  eventType?: "Dialogue" | "Comment";
  layer?: number;
  marked?: number;
  marginL?: number;
  marginR?: number;
  marginV?: number;
  effect?: string;
  animations?: AssAnimation[];
  overrides?: AssTextStyleOverride;
  /**
   * Original ASS event text. It is retained for an explicitly requested
   * lossless export, but is not trusted by the safe serializer by default.
   */
  rawText?: string;
  fields?: Record<string, string>;
}

export type AssAnimation =
  | {
      kind: "fade";
      fadeInMs: number;
      fadeOutMs: number;
    }
  | {
      kind: "move";
      fromX: number;
      fromY: number;
      toX: number;
      toY: number;
      startMs?: number;
      endMs?: number;
    }
  | {
      kind: "position";
      x: number;
      y: number;
    };

export interface AssTextStyleOverride {
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikeOut?: boolean;
  fontName?: string;
  fontSize?: number;
  primaryColor?: string;
  outlineColor?: string;
  outline?: number;
  shadow?: number;
  alignment?: number;
}

export interface SerializeTextOptions {
  newline?: "\n" | "\r\n";
}

export interface SerializeAssOptions extends SerializeTextOptions {
  /**
   * Reuses parsed inline ASS override markup only when it still decodes to the
   * cue's current text. Leave disabled for user-edited or untrusted captions.
   */
  preserveRawText?: boolean;
}

const ASS_STYLE_FORMAT = [
  "Name",
  "Fontname",
  "Fontsize",
  "PrimaryColour",
  "SecondaryColour",
  "OutlineColour",
  "BackColour",
  "Bold",
  "Italic",
  "Underline",
  "StrikeOut",
  "ScaleX",
  "ScaleY",
  "Spacing",
  "Angle",
  "BorderStyle",
  "Outline",
  "Shadow",
  "Alignment",
  "MarginL",
  "MarginR",
  "MarginV",
  "Encoding",
] as const;

const ASS_EVENT_FORMAT = [
  "Layer",
  "Start",
  "End",
  "Style",
  "Name",
  "MarginL",
  "MarginR",
  "MarginV",
  "Effect",
  "Text",
] as const;

const DEFAULT_ASS_STYLE: Required<Omit<AssStyle, "fields">> = {
  name: "Default",
  fontName: "Arial",
  fontSize: 48,
  primaryColor: "&H00FFFFFF",
  secondaryColor: "&H000000FF",
  outlineColor: "&H00000000",
  backColor: "&H64000000",
  bold: false,
  italic: false,
  underline: false,
  strikeOut: false,
  scaleX: 100,
  scaleY: 100,
  spacing: 0,
  angle: 0,
  borderStyle: 1,
  outline: 2,
  shadow: 0,
  alignment: 2,
  marginL: 30,
  marginR: 30,
  marginV: 30,
  encoding: 1,
};

function normalizeLines(input: string): string[] {
  return input.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n").split("\n");
}

function normalizeText(input: string): string {
  return input.replace(/\r\n?/g, "\n");
}

function parseClock(value: string, allowShort = false): number | null {
  const normalized = value.trim().replace(",", ".");
  const parts = normalized.split(":");
  if ((!allowShort && parts.length !== 3) || (allowShort && parts.length < 2)) {
    return null;
  }
  if (parts.length !== 2 && parts.length !== 3) return null;

  const secondsPart = parts.at(-1);
  const minutesPart = parts.at(-2);
  const hoursPart = parts.length === 3 ? parts[0] : "0";
  if (!secondsPart || !minutesPart || hoursPart === undefined) return null;
  if (!/^\d+$/.test(hoursPart) || !/^\d{1,2}$/.test(minutesPart)) return null;

  const secondMatch = /^(\d{1,2})(?:\.(\d{1,3}))?$/.exec(secondsPart);
  if (!secondMatch) return null;
  const hours = Number(hoursPart);
  const minutes = Number(minutesPart);
  const seconds = Number(secondMatch[1]);
  if (minutes > 59 || seconds > 59) return null;

  const milliseconds = Number((secondMatch[2] ?? "").padEnd(3, "0"));
  return hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + milliseconds;
}

function pad(value: number, width: number): string {
  return Math.trunc(value).toString().padStart(width, "0");
}

function checkedMilliseconds(value: number, cueIndex: number, field: "startMs" | "endMs"): number {
  if (!Number.isFinite(value) || value < 0) {
    throw new RangeError(`Cue ${cueIndex + 1} has an invalid ${field}: ${String(value)}`);
  }
  return Math.round(value);
}

function checkedCueTimes(cue: CaptionCue, cueIndex: number): [number, number] {
  const startMs = checkedMilliseconds(cue.startMs, cueIndex, "startMs");
  const endMs = checkedMilliseconds(cue.endMs, cueIndex, "endMs");
  if (endMs < startMs) {
    throw new RangeError(`Cue ${cueIndex + 1} ends before it starts`);
  }
  return [startMs, endMs];
}

function formatSrtClock(milliseconds: number): string {
  const hours = Math.floor(milliseconds / 3_600_000);
  const minutes = Math.floor((milliseconds % 3_600_000) / 60_000);
  const seconds = Math.floor((milliseconds % 60_000) / 1_000);
  const millis = milliseconds % 1_000;
  return `${pad(hours, 2)}:${pad(minutes, 2)}:${pad(seconds, 2)},${pad(millis, 3)}`;
}

function formatVttClock(milliseconds: number): string {
  return formatSrtClock(milliseconds).replace(",", ".");
}

function parseTimingLine(line: string, allowShort: boolean): {
  startMs: number;
  endMs: number;
  settings: string;
} | null {
  const match = /^\s*(\S+)\s+-->\s+(\S+)(?:\s+(.*?))?\s*$/.exec(line);
  if (!match) return null;
  const startMs = parseClock(match[1], allowShort);
  const endMs = parseClock(match[2], allowShort);
  if (startMs === null || endMs === null || endMs < startMs) return null;
  return { startMs, endMs, settings: match[3]?.trim() ?? "" };
}

export function parseSrt(input: string): CaptionDocument {
  const lines = normalizeLines(input);
  const cues: CaptionCue[] = [];
  let index = 0;

  while (index < lines.length) {
    while (index < lines.length && !lines[index].trim()) index += 1;
    if (index >= lines.length) break;

    let id: string | undefined;
    let timing = parseTimingLine(lines[index], false);
    if (!timing && index + 1 < lines.length) {
      const nextTiming = parseTimingLine(lines[index + 1], false);
      if (nextTiming) {
        id = lines[index].trim();
        timing = nextTiming;
        index += 1;
      }
    }

    if (!timing) {
      index += 1;
      continue;
    }

    index += 1;
    const textLines: string[] = [];
    while (index < lines.length && lines[index].trim() !== "") {
      textLines.push(lines[index]);
      index += 1;
    }
    cues.push({
      ...(id ? { id } : {}),
      startMs: timing.startMs,
      endMs: timing.endMs,
      text: textLines.join("\n"),
    });
  }

  return { format: "srt", cues };
}

export function serializeSrt(
  document: CaptionDocument,
  options: SerializeTextOptions = {},
): string {
  const newline = options.newline ?? "\r\n";
  const blocks = document.cues.map((cue, index) => {
    const [startMs, endMs] = checkedCueTimes(cue, index);
    const id = cue.id?.trim() || String(index + 1);
    const text = normalizeText(cue.text);
    return [
      id.replace(/[\r\n]+/g, " "),
      `${formatSrtClock(startMs)} --> ${formatSrtClock(endMs)}`,
      text,
    ].join(newline);
  });
  return `${blocks.join(`${newline}${newline}`)}${blocks.length ? newline : ""}`;
}

function parseVttBlock(lines: string[], index: number): { block: VttBlock; next: number } | null {
  const match = /^(NOTE|STYLE|REGION)(?:[ \t]+(.*))?$/.exec(lines[index]);
  if (!match) return null;
  const blockLines = [match[2] ?? ""];
  index += 1;
  while (index < lines.length && lines[index].trim() !== "") {
    blockLines.push(lines[index]);
    index += 1;
  }
  if (!blockLines[0]) blockLines.shift();
  return {
    block: { type: match[1] as VttBlock["type"], text: blockLines.join("\n") },
    next: index,
  };
}

function unescapeVttAnnotation(value: string): string {
  return value
    .replace(/&gt;/gi, ">")
    .replace(/&lt;/gi, "<")
    .replace(/&amp;/gi, "&")
    .trim();
}

function unwrapVttCueText(text: string): {
  text: string;
  speaker?: string;
  style?: string;
  metadata?: VttCueMetadata;
} {
  let content = text;
  let speaker: string | undefined;
  let style: string | undefined;
  let classes: string[] | undefined;
  let voiceClasses: string[] | undefined;

  const voiceMatch = /^<v((?:\.[A-Za-z0-9_-]+)*)(?:\s+([^>]*))?>([\s\S]*?)(?:<\/v>)?$/.exec(
    content,
  );
  if (voiceMatch) {
    voiceClasses = voiceMatch[1].split(".").filter(Boolean);
    speaker = voiceMatch[2] ? unescapeVttAnnotation(voiceMatch[2]) : undefined;
    content = voiceMatch[3];
  }

  const classMatch = /^<c((?:\.[A-Za-z0-9_-]+)+)>([\s\S]*?)<\/c>$/.exec(content);
  if (classMatch) {
    classes = classMatch[1].split(".").filter(Boolean);
    style = classes[0];
    content = classMatch[2];
  }

  const metadata = classes?.length || voiceClasses?.length ? { classes, voiceClasses } : undefined;
  return {
    text: content,
    ...(speaker ? { speaker } : {}),
    ...(style ? { style } : {}),
    ...(metadata ? { metadata } : {}),
  };
}

export function parseWebVtt(input: string): CaptionDocument {
  const lines = normalizeLines(input);
  if (!/^WEBVTT(?:[ \t].*)?$/.test(lines[0]?.trim() ?? "")) {
    throw new SyntaxError("WebVTT input must begin with WEBVTT");
  }

  const header = lines[0].trim().slice("WEBVTT".length).trim();
  const headerLines: string[] = [];
  const blocks: VttBlock[] = [];
  const cues: CaptionCue[] = [];
  let index = 1;
  while (index < lines.length && lines[index].trim() !== "") {
    headerLines.push(lines[index]);
    index += 1;
  }

  while (index < lines.length) {
    while (index < lines.length && lines[index].trim() === "") index += 1;
    if (index >= lines.length) break;

    const parsedBlock = parseVttBlock(lines, index);
    if (parsedBlock) {
      blocks.push(parsedBlock.block);
      index = parsedBlock.next;
      continue;
    }

    let id: string | undefined;
    let timing = parseTimingLine(lines[index], true);
    if (!timing && index + 1 < lines.length) {
      const nextTiming = parseTimingLine(lines[index + 1], true);
      if (nextTiming) {
        id = lines[index];
        timing = nextTiming;
        index += 1;
      }
    }
    if (!timing) {
      index += 1;
      continue;
    }

    index += 1;
    const textLines: string[] = [];
    while (index < lines.length && lines[index].trim() !== "") {
      textLines.push(lines[index]);
      index += 1;
    }
    const unwrapped = unwrapVttCueText(textLines.join("\n"));
    cues.push({
      ...(id ? { id } : {}),
      startMs: timing.startMs,
      endMs: timing.endMs,
      text: unwrapped.text,
      ...(unwrapped.speaker ? { speaker: unwrapped.speaker } : {}),
      ...(unwrapped.style ? { style: unwrapped.style } : {}),
      ...(timing.settings ? { settings: timing.settings } : {}),
      ...(unwrapped.metadata ? { vtt: unwrapped.metadata } : {}),
    });
  }

  return {
    format: "vtt",
    cues,
    vtt: {
      ...(header ? { header } : {}),
      ...(headerLines.length ? { headerLines } : {}),
      ...(blocks.length ? { blocks } : {}),
    },
  };
}

export const parseVtt = parseWebVtt;

function safeVttClasses(classes: string[] | undefined): string[] {
  return (classes ?? []).filter((value) => /^[A-Za-z0-9_-]+$/.test(value));
}

function escapeVttAnnotation(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/[\r\n]+/g, " ")
    .trim();
}

function wrapVttCueText(cue: CaptionCue): string {
  let text = normalizeText(cue.text);
  const classes = safeVttClasses(cue.vtt?.classes ?? (cue.style ? [cue.style] : undefined));
  if (classes.length) text = `<c.${classes.join(".")}>${text}</c>`;

  if (cue.speaker || cue.vtt?.voiceClasses?.length) {
    const voiceClasses = safeVttClasses(cue.vtt?.voiceClasses);
    const classSuffix = voiceClasses.length ? `.${voiceClasses.join(".")}` : "";
    const speaker = cue.speaker ? ` ${escapeVttAnnotation(cue.speaker)}` : "";
    text = `<v${classSuffix}${speaker}>${text}</v>`;
  }
  return text;
}

export function serializeWebVtt(
  document: CaptionDocument,
  options: SerializeTextOptions = {},
): string {
  const newline = options.newline ?? "\n";
  const header = document.vtt?.header?.replace(/[\r\n]+/g, " ").trim();
  const output: string[] = [`WEBVTT${header ? ` ${header}` : ""}`];
  if (document.vtt?.headerLines?.length) {
    output.push(...document.vtt.headerLines.map((line) => line.replace(/[\r\n]+/g, " ")));
  }
  output.push("");

  for (const block of document.vtt?.blocks ?? []) {
    const text = normalizeText(block.text);
    output.push(block.type + (block.type === "NOTE" && text ? ` ${text.split("\n")[0]}` : ""));
    if (text && block.type !== "NOTE") output.push(...text.split("\n"));
    if (text.includes("\n") && block.type === "NOTE") output.push(...text.split("\n").slice(1));
    output.push("");
  }

  document.cues.forEach((cue, index) => {
    const [startMs, endMs] = checkedCueTimes(cue, index);
    if (cue.id?.trim()) output.push(cue.id.replace(/[\r\n]+/g, " "));
    const settings = cue.settings?.replace(/[\r\n]+/g, " ").trim();
    output.push(
      `${formatVttClock(startMs)} --> ${formatVttClock(endMs)}${settings ? ` ${settings}` : ""}`,
    );
    output.push(...wrapVttCueText(cue).split("\n"), "");
  });

  return output.join(newline);
}

export const serializeVtt = serializeWebVtt;

function canonicalAssField(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function splitAssFields(value: string, fieldCount: number, preserveLast = false): string[] {
  if (fieldCount <= 1) return [value];
  const result: string[] = [];
  let cursor = 0;
  for (let index = 1; index < fieldCount; index += 1) {
    const comma = value.indexOf(",", cursor);
    if (comma < 0) break;
    result.push(value.slice(cursor, comma).trim());
    cursor = comma + 1;
  }
  const last = value.slice(cursor);
  result.push(preserveLast ? last : last.trim());
  while (result.length < fieldCount) result.push("");
  return result;
}

function assFieldsToRecord(format: string[], values: string[]): Record<string, string> {
  return Object.fromEntries(format.map((field, index) => [field, values[index] ?? ""]));
}

function findAssField(fields: Record<string, string>, names: string[]): string | undefined {
  const canonicalNames = new Set(names.map(canonicalAssField));
  const entry = Object.entries(fields).find(([key]) => canonicalNames.has(canonicalAssField(key)));
  return entry?.[1];
}

function parseOptionalNumber(value: string | undefined): number | undefined {
  if (value === undefined || value.trim() === "") return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function parseAssBoolean(value: string | undefined): boolean | undefined {
  const parsed = parseOptionalNumber(value);
  return parsed === undefined ? undefined : parsed !== 0;
}

function parseAssStyle(fields: Record<string, string>): AssStyle | null {
  const name = findAssField(fields, ["Name"])?.trim();
  if (!name) return null;
  return {
    name,
    fontName: findAssField(fields, ["Fontname"]),
    fontSize: parseOptionalNumber(findAssField(fields, ["Fontsize"])),
    primaryColor: findAssField(fields, ["PrimaryColour", "PrimaryColor"]),
    secondaryColor: findAssField(fields, ["SecondaryColour", "SecondaryColor"]),
    outlineColor: findAssField(fields, ["OutlineColour", "OutlineColor", "TertiaryColour"]),
    backColor: findAssField(fields, ["BackColour", "BackColor"]),
    bold: parseAssBoolean(findAssField(fields, ["Bold"])),
    italic: parseAssBoolean(findAssField(fields, ["Italic"])),
    underline: parseAssBoolean(findAssField(fields, ["Underline"])),
    strikeOut: parseAssBoolean(findAssField(fields, ["StrikeOut"])),
    scaleX: parseOptionalNumber(findAssField(fields, ["ScaleX"])),
    scaleY: parseOptionalNumber(findAssField(fields, ["ScaleY"])),
    spacing: parseOptionalNumber(findAssField(fields, ["Spacing"])),
    angle: parseOptionalNumber(findAssField(fields, ["Angle"])),
    borderStyle: parseOptionalNumber(findAssField(fields, ["BorderStyle"])),
    outline: parseOptionalNumber(findAssField(fields, ["Outline"])),
    shadow: parseOptionalNumber(findAssField(fields, ["Shadow"])),
    alignment: parseOptionalNumber(findAssField(fields, ["Alignment"])),
    marginL: parseOptionalNumber(findAssField(fields, ["MarginL"])),
    marginR: parseOptionalNumber(findAssField(fields, ["MarginR"])),
    marginV: parseOptionalNumber(findAssField(fields, ["MarginV", "MarginT"])),
    encoding: parseOptionalNumber(findAssField(fields, ["Encoding"])),
    fields,
  };
}

function isEscaped(value: string, index: number): boolean {
  let slashes = 0;
  for (let cursor = index - 1; cursor >= 0 && value[cursor] === "\\"; cursor -= 1) slashes += 1;
  return slashes % 2 === 1;
}

function stripAssOverrideBlocks(value: string): string {
  let result = "";
  let index = 0;
  while (index < value.length) {
    if (value[index] === "{" && !isEscaped(value, index)) {
      const end = value.indexOf("}", index + 1);
      if (end >= 0 && value.slice(index + 1, end).trimStart().startsWith("\\")) {
        index = end + 1;
        continue;
      }
    }
    result += value[index];
    index += 1;
  }
  return result;
}

function decodeAssEscapes(value: string): string {
  let result = "";
  for (let index = 0; index < value.length; index += 1) {
    if (value[index] !== "\\" || index + 1 >= value.length) {
      result += value[index];
      continue;
    }
    const next = value[index + 1];
    if (next === "N" || next === "n") result += "\n";
    else if (next === "h") result += "\u00A0";
    else if (next === "\\" || next === "{" || next === "}") result += next;
    else {
      result += `\\${next}`;
    }
    index += 1;
  }
  return result;
}

function decodeAssText(value: string): string {
  return decodeAssEscapes(stripAssOverrideBlocks(value));
}

function parseLeadingAssOverrides(value: string): {
  animations?: AssAnimation[];
  overrides?: AssTextStyleOverride;
} {
  const blocks: string[] = [];
  let rest = value;
  while (rest.startsWith("{")) {
    const end = rest.indexOf("}");
    if (end < 0) break;
    const block = rest.slice(1, end);
    if (!block.trimStart().startsWith("\\")) break;
    blocks.push(block);
    rest = rest.slice(end + 1);
  }
  if (!blocks.length) return {};

  const tags = blocks.join("");
  const animations: AssAnimation[] = [];
  const fade = /\\fad\(\s*(\d+)\s*,\s*(\d+)\s*\)/i.exec(tags);
  if (fade) {
    animations.push({ kind: "fade", fadeInMs: Number(fade[1]), fadeOutMs: Number(fade[2]) });
  }
  const move = /\\move\(\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)(?:\s*,\s*(\d+)\s*,\s*(\d+))?\s*\)/i.exec(
    tags,
  );
  if (move) {
    animations.push({
      kind: "move",
      fromX: Number(move[1]),
      fromY: Number(move[2]),
      toX: Number(move[3]),
      toY: Number(move[4]),
      ...(move[5] ? { startMs: Number(move[5]) } : {}),
      ...(move[6] ? { endMs: Number(move[6]) } : {}),
    });
  }
  const position = /\\pos\(\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\)/i.exec(tags);
  if (position) {
    animations.push({ kind: "position", x: Number(position[1]), y: Number(position[2]) });
  }

  const overrides: AssTextStyleOverride = {};
  const booleanTags: Array<[keyof AssTextStyleOverride, RegExp]> = [
    ["bold", /\\b(-?1|0)(?=\\|$)/i],
    ["italic", /\\i(-?1|0)(?=\\|$)/i],
    ["underline", /\\u(-?1|0)(?=\\|$)/i],
    ["strikeOut", /\\s(-?1|0)(?=\\|$)/i],
  ];
  for (const [key, pattern] of booleanTags) {
    const match = pattern.exec(tags);
    if (match) Object.assign(overrides, { [key]: match[1] !== "0" });
  }
  const fontName = /\\fn([^\\}]+)/i.exec(tags)?.[1]?.trim();
  const fontSize = parseOptionalNumber(/\\fs([\d.]+)/i.exec(tags)?.[1]);
  const primaryColor = /\\(?:1?c)(&H[0-9A-F]{6,8}&?)/i.exec(tags)?.[1];
  const outlineColor = /\\3c(&H[0-9A-F]{6,8}&?)/i.exec(tags)?.[1];
  const outline = parseOptionalNumber(/\\bord([\d.]+)/i.exec(tags)?.[1]);
  const shadow = parseOptionalNumber(/\\shad([\d.]+)/i.exec(tags)?.[1]);
  const alignment = parseOptionalNumber(/\\an([1-9])/i.exec(tags)?.[1]);
  if (fontName) overrides.fontName = fontName;
  if (fontSize !== undefined) overrides.fontSize = fontSize;
  if (primaryColor) overrides.primaryColor = primaryColor;
  if (outlineColor) overrides.outlineColor = outlineColor;
  if (outline !== undefined) overrides.outline = outline;
  if (shadow !== undefined) overrides.shadow = shadow;
  if (alignment !== undefined) overrides.alignment = alignment;

  return {
    ...(animations.length ? { animations } : {}),
    ...(Object.keys(overrides).length ? { overrides } : {}),
  };
}

function parseAssEvent(
  eventType: "Dialogue" | "Comment",
  format: string[],
  payload: string,
): CaptionCue | null {
  const fields = assFieldsToRecord(format, splitAssFields(payload, format.length, true));
  const startMs = parseAssClock(findAssField(fields, ["Start"]) ?? "");
  const endMs = parseAssClock(findAssField(fields, ["End"]) ?? "");
  const rawText = findAssField(fields, ["Text"]) ?? "";
  if (startMs === null || endMs === null || endMs < startMs) return null;

  const speaker = findAssField(fields, ["Name", "Actor"])?.trim();
  const style = findAssField(fields, ["Style"])?.trim();
  const animationsAndOverrides = parseLeadingAssOverrides(rawText);
  return {
    startMs,
    endMs,
    text: decodeAssText(rawText),
    ...(speaker ? { speaker } : {}),
    ...(style ? { style } : {}),
    ass: {
      eventType,
      layer: parseOptionalNumber(findAssField(fields, ["Layer"])),
      marked: parseOptionalNumber(findAssField(fields, ["Marked"])),
      marginL: parseOptionalNumber(findAssField(fields, ["MarginL"])),
      marginR: parseOptionalNumber(findAssField(fields, ["MarginR"])),
      marginV: parseOptionalNumber(findAssField(fields, ["MarginV"])),
      effect: findAssField(fields, ["Effect"]),
      rawText,
      fields,
      ...animationsAndOverrides,
    },
  };
}

function parseAssClock(value: string): number | null {
  const match = /^\s*(\d+):(\d{1,2}):(\d{1,2})[.](\d{1,3})\s*$/.exec(value);
  if (!match) return null;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  const seconds = Number(match[3]);
  if (minutes > 59 || seconds > 59) return null;
  const milliseconds = Number(match[4].padEnd(3, "0"));
  return hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + milliseconds;
}

export function parseAss(input: string): CaptionDocument {
  const lines = normalizeLines(input);
  const scriptInfo: Record<string, string> = {};
  const styles: AssStyle[] = [];
  const cues: CaptionCue[] = [];
  let section = "";
  let styleFormat: string[] = [...ASS_STYLE_FORMAT];
  let eventFormat: string[] = [...ASS_EVENT_FORMAT];

  for (const sourceLine of lines) {
    const line = sourceLine.trim();
    const sectionMatch = /^\[([^\]]+)\]$/.exec(line);
    if (sectionMatch) {
      section = sectionMatch[1].toLowerCase();
      continue;
    }
    if (!line || line.startsWith(";")) continue;

    if (section === "script info") {
      const separator = sourceLine.indexOf(":");
      if (separator > 0) {
        const key = sourceLine.slice(0, separator).trim();
        if (key) scriptInfo[key] = sourceLine.slice(separator + 1).trim();
      }
      continue;
    }

    if (section === "v4+ styles" || section === "v4 styles") {
      const formatMatch = /^Format\s*:\s*(.*)$/i.exec(sourceLine);
      if (formatMatch) {
        const parsedFormat = formatMatch[1].split(",").map((field) => field.trim()).filter(Boolean);
        if (parsedFormat.length) styleFormat = parsedFormat;
        continue;
      }
      const styleMatch = /^Style\s*:\s*(.*)$/i.exec(sourceLine);
      if (styleMatch) {
        const parsed = parseAssStyle(
          assFieldsToRecord(styleFormat, splitAssFields(styleMatch[1], styleFormat.length)),
        );
        if (parsed) styles.push(parsed);
      }
      continue;
    }

    if (section === "events") {
      const formatMatch = /^Format\s*:\s*(.*)$/i.exec(sourceLine);
      if (formatMatch) {
        const parsedFormat = formatMatch[1].split(",").map((field) => field.trim()).filter(Boolean);
        if (parsedFormat.length) eventFormat = parsedFormat;
        continue;
      }
      const eventMatch = /^(Dialogue|Comment)\s*:\s*(.*)$/i.exec(sourceLine);
      if (eventMatch) {
        const eventType = eventMatch[1].toLowerCase() === "comment" ? "Comment" : "Dialogue";
        const cue = parseAssEvent(eventType, eventFormat, eventMatch[2]);
        if (cue) cues.push(cue);
      }
    }
  }

  return {
    format: "ass",
    cues,
    ...(styles.length ? { styles } : {}),
    ass: {
      ...(Object.keys(scriptInfo).length ? { scriptInfo } : {}),
      styleFormat,
      eventFormat,
    },
  };
}

function formatAssClock(milliseconds: number): string {
  // ASS uses centiseconds. Round once here so carry-over at x.xx5 is correct.
  const centisecondsTotal = Math.round(milliseconds / 10);
  const hours = Math.floor(centisecondsTotal / 360_000);
  const minutes = Math.floor((centisecondsTotal % 360_000) / 6_000);
  const seconds = Math.floor((centisecondsTotal % 6_000) / 100);
  const centiseconds = centisecondsTotal % 100;
  return `${hours}:${pad(minutes, 2)}:${pad(seconds, 2)}.${pad(centiseconds, 2)}`;
}

function sanitizeAssField(value: string | undefined, fallback = ""): string {
  return (value ?? fallback).replace(/[\r\n,]+/g, " ").trim();
}

function sanitizeAssTagString(value: string): string {
  return value.replace(/[\\{}\r\n]/g, "").trim().slice(0, 128);
}

function finiteOr(value: number | undefined, fallback: number): number {
  return value !== undefined && Number.isFinite(value) ? value : fallback;
}

function bounded(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function compactNumber(value: number): string {
  return Number(value.toFixed(3)).toString();
}

function normalizeAssColor(value: string | undefined): string | undefined {
  if (!value) return undefined;
  const ass = /^&H([0-9A-F]{6}|[0-9A-F]{8})&?$/i.exec(value.trim());
  if (ass) return `&H${ass[1].toUpperCase()}`;
  const css = /^#([0-9A-F]{6})$/i.exec(value.trim());
  if (!css) return undefined;
  const [red, green, blue] = [css[1].slice(0, 2), css[1].slice(2, 4), css[1].slice(4, 6)];
  return `&H00${blue}${green}${red}`.toUpperCase();
}

function assBoolean(value: boolean | undefined, fallback: boolean): string {
  return value ?? fallback ? "-1" : "0";
}

function assStyleValue(style: AssStyle, field: string): string {
  const fallback = DEFAULT_ASS_STYLE;
  const original = style.fields?.[field];
  switch (canonicalAssField(field)) {
    case "name":
      return sanitizeAssField(style.name, fallback.name);
    case "fontname":
      return sanitizeAssField(style.fontName, fallback.fontName);
    case "fontsize":
      return compactNumber(finiteOr(style.fontSize, fallback.fontSize));
    case "primarycolour":
    case "primarycolor":
      return normalizeAssColor(style.primaryColor) ?? normalizeAssColor(original) ?? fallback.primaryColor;
    case "secondarycolour":
    case "secondarycolor":
      return normalizeAssColor(style.secondaryColor) ?? normalizeAssColor(original) ?? fallback.secondaryColor;
    case "outlinecolour":
    case "outlinecolor":
    case "tertiarycolour":
      return normalizeAssColor(style.outlineColor) ?? normalizeAssColor(original) ?? fallback.outlineColor;
    case "backcolour":
    case "backcolor":
      return normalizeAssColor(style.backColor) ?? normalizeAssColor(original) ?? fallback.backColor;
    case "bold":
      return assBoolean(style.bold, fallback.bold);
    case "italic":
      return assBoolean(style.italic, fallback.italic);
    case "underline":
      return assBoolean(style.underline, fallback.underline);
    case "strikeout":
      return assBoolean(style.strikeOut, fallback.strikeOut);
    case "scalex":
      return compactNumber(finiteOr(style.scaleX, fallback.scaleX));
    case "scaley":
      return compactNumber(finiteOr(style.scaleY, fallback.scaleY));
    case "spacing":
      return compactNumber(finiteOr(style.spacing, fallback.spacing));
    case "angle":
      return compactNumber(finiteOr(style.angle, fallback.angle));
    case "borderstyle":
      return compactNumber(finiteOr(style.borderStyle, fallback.borderStyle));
    case "outline":
      return compactNumber(finiteOr(style.outline, fallback.outline));
    case "shadow":
      return compactNumber(finiteOr(style.shadow, fallback.shadow));
    case "alignment":
      return compactNumber(finiteOr(style.alignment, fallback.alignment));
    case "marginl":
      return compactNumber(finiteOr(style.marginL, fallback.marginL));
    case "marginr":
      return compactNumber(finiteOr(style.marginR, fallback.marginR));
    case "marginv":
    case "margint":
      return compactNumber(finiteOr(style.marginV, fallback.marginV));
    case "encoding":
      return compactNumber(finiteOr(style.encoding, fallback.encoding));
    default:
      return sanitizeAssField(original);
  }
}

function escapeAssPlainText(value: string): string {
  return normalizeText(value)
    .replace(/\\/g, "\\\\")
    .replace(/{/g, "\\{")
    .replace(/}/g, "\\}")
    .replace(/\n/g, "\\N");
}

function safeAssDuration(value: number): number {
  return Math.round(bounded(Number.isFinite(value) ? value : 0, 0, 86_400_000));
}

function safeAssCoordinate(value: number): string {
  return compactNumber(bounded(Number.isFinite(value) ? value : 0, -100_000, 100_000));
}

function serializeAssOverrides(cue: CaptionCue): string {
  const tags: string[] = [];
  const override = cue.ass?.overrides;
  if (override) {
    if (override.bold !== undefined) tags.push(`\\b${override.bold ? 1 : 0}`);
    if (override.italic !== undefined) tags.push(`\\i${override.italic ? 1 : 0}`);
    if (override.underline !== undefined) tags.push(`\\u${override.underline ? 1 : 0}`);
    if (override.strikeOut !== undefined) tags.push(`\\s${override.strikeOut ? 1 : 0}`);
    const fontName = override.fontName ? sanitizeAssTagString(override.fontName) : "";
    if (fontName) tags.push(`\\fn${fontName}`);
    if (override.fontSize !== undefined && Number.isFinite(override.fontSize)) {
      tags.push(`\\fs${compactNumber(bounded(override.fontSize, 1, 1_000))}`);
    }
    const primaryColor = normalizeAssColor(override.primaryColor);
    if (primaryColor) tags.push(`\\c${primaryColor}`);
    const outlineColor = normalizeAssColor(override.outlineColor);
    if (outlineColor) tags.push(`\\3c${outlineColor}`);
    if (override.outline !== undefined && Number.isFinite(override.outline)) {
      tags.push(`\\bord${compactNumber(bounded(override.outline, 0, 100))}`);
    }
    if (override.shadow !== undefined && Number.isFinite(override.shadow)) {
      tags.push(`\\shad${compactNumber(bounded(override.shadow, 0, 100))}`);
    }
    if (override.alignment !== undefined && Number.isFinite(override.alignment)) {
      tags.push(`\\an${Math.round(bounded(override.alignment, 1, 9))}`);
    }
  }

  for (const animation of cue.ass?.animations ?? []) {
    if (animation.kind === "fade") {
      tags.push(`\\fad(${safeAssDuration(animation.fadeInMs)},${safeAssDuration(animation.fadeOutMs)})`);
    } else if (animation.kind === "position") {
      tags.push(`\\pos(${safeAssCoordinate(animation.x)},${safeAssCoordinate(animation.y)})`);
    } else {
      const coordinates = [
        safeAssCoordinate(animation.fromX),
        safeAssCoordinate(animation.fromY),
        safeAssCoordinate(animation.toX),
        safeAssCoordinate(animation.toY),
      ];
      if (animation.startMs !== undefined || animation.endMs !== undefined) {
        coordinates.push(
          String(safeAssDuration(animation.startMs ?? 0)),
          String(safeAssDuration(animation.endMs ?? animation.startMs ?? 0)),
        );
      }
      tags.push(`\\move(${coordinates.join(",")})`);
    }
  }
  return tags.length ? `{${tags.join("")}}` : "";
}

function serializeAssEventText(cue: CaptionCue, preserveRawText: boolean): string {
  const rawText = cue.ass?.rawText;
  if (
    preserveRawText &&
    rawText !== undefined &&
    !/[\r\n]/.test(rawText) &&
    decodeAssText(rawText) === normalizeText(cue.text)
  ) {
    return rawText;
  }
  return `${serializeAssOverrides(cue)}${escapeAssPlainText(cue.text)}`;
}

function safeScriptInfo(source: Record<string, string> | undefined): Array<[string, string]> {
  const entries: Array<[string, string]> = [];
  for (const [key, value] of Object.entries(source ?? {})) {
    if (!/^[A-Za-z][A-Za-z0-9 _-]{0,63}$/.test(key)) continue;
    entries.push([key, value.replace(/[\r\n]+/g, " ").trim()]);
  }
  return entries;
}

export function serializeAss(
  document: CaptionDocument,
  options: SerializeAssOptions = {},
): string {
  const newline = options.newline ?? "\r\n";
  const output: string[] = ["[Script Info]"];
  const scriptEntries = safeScriptInfo(document.ass?.scriptInfo);
  const hasScriptType = scriptEntries.some(([key]) => canonicalAssField(key) === "scripttype");
  if (!hasScriptType) output.push("ScriptType: v4.00+");
  if (document.title && !scriptEntries.some(([key]) => canonicalAssField(key) === "title")) {
    output.push(`Title: ${document.title.replace(/[\r\n]+/g, " ").trim()}`);
  }
  output.push(...scriptEntries.map(([key, value]) => `${key}: ${value}`), "", "[V4+ Styles]");

  const requestedStyleFormat = document.ass?.styleFormat?.length
    ? document.ass.styleFormat.map((field) => sanitizeAssField(field)).filter(Boolean)
    : [...ASS_STYLE_FORMAT];
  const styleFormat = requestedStyleFormat.length ? requestedStyleFormat : [...ASS_STYLE_FORMAT];
  output.push(`Format: ${styleFormat.join(", ")}`);
  const styles = document.styles?.length ? document.styles : [{ ...DEFAULT_ASS_STYLE }];
  for (const style of styles) {
    output.push(`Style: ${styleFormat.map((field) => assStyleValue(style, field)).join(",")}`);
  }

  output.push("", "[Events]", `Format: ${ASS_EVENT_FORMAT.join(", ")}`);
  document.cues.forEach((cue, index) => {
    const [startMs, endMs] = checkedCueTimes(cue, index);
    const eventType = cue.ass?.eventType === "Comment" ? "Comment" : "Dialogue";
    const layer = Math.round(
      bounded(finiteOr(cue.ass?.layer, finiteOr(cue.ass?.marked, 0)), 0, 9999),
    );
    const style = sanitizeAssField(cue.style, "Default") || "Default";
    const speaker = sanitizeAssField(cue.speaker);
    const marginL = Math.round(bounded(finiteOr(cue.ass?.marginL, 0), 0, 9999));
    const marginR = Math.round(bounded(finiteOr(cue.ass?.marginR, 0), 0, 9999));
    const marginV = Math.round(bounded(finiteOr(cue.ass?.marginV, 0), 0, 9999));
    const effect = sanitizeAssField(cue.ass?.effect);
    const text = serializeAssEventText(cue, options.preserveRawText === true);
    output.push(
      `${eventType}: ${layer},${formatAssClock(startMs)},${formatAssClock(endMs)},${style},${speaker},${marginL},${marginR},${marginV},${effect},${text}`,
    );
  });
  return `${output.join(newline)}${newline}`;
}

export function detectCaptionFormat(input: string): CaptionFormat {
  const normalized = input.replace(/^\uFEFF/, "").trimStart();
  if (/^WEBVTT(?:[ \t\r\n]|$)/.test(normalized)) return "vtt";
  if (/^\[Script Info\]/im.test(normalized) || /^\[Events\]/im.test(normalized)) return "ass";
  return "srt";
}

export function parseCaption(
  input: string,
  format: CaptionFormat | "webvtt" = detectCaptionFormat(input),
): CaptionDocument {
  if (format === "vtt" || format === "webvtt") return parseWebVtt(input);
  if (format === "ass") return parseAss(input);
  return parseSrt(input);
}

export function serializeCaption(
  document: CaptionDocument,
  format: CaptionFormat | "webvtt" = document.format ?? "srt",
  options: SerializeAssOptions = {},
): string {
  if (format === "vtt" || format === "webvtt") return serializeWebVtt(document, options);
  if (format === "ass") return serializeAss(document, options);
  return serializeSrt(document, options);
}
