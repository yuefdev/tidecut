import { describe, expect, it } from "vitest";
import {
  detectCaptionFormat,
  parseAss,
  parseCaption,
  parseSrt,
  parseWebVtt,
  serializeAss,
  serializeCaption,
  serializeSrt,
  serializeWebVtt,
  type CaptionDocument,
} from "./caption-formats";

describe("SRT caption format", () => {
  it("parses BOM, identifiers, multiline text and millisecond timing", () => {
    const parsed = parseSrt(
      "\uFEFF1\r\n00:00:01,250 --> 00:00:03,005\r\nMerhaba\r\nDünya\r\n\r\nscene-b\r\n00:00:05.010 --> 00:00:06.000\r\nCode-switch test\r\n",
    );

    expect(parsed).toMatchObject({ format: "srt" });
    expect(parsed.cues).toEqual([
      { id: "1", startMs: 1_250, endMs: 3_005, text: "Merhaba\nDünya" },
      { id: "scene-b", startMs: 5_010, endMs: 6_000, text: "Code-switch test" },
    ]);
  });

  it("round-trips cue identity, times and text", () => {
    const document: CaptionDocument = {
      cues: [
        { id: "qa-1", startMs: 12, endMs: 2_345, text: "İstanbul\nOpenAI" },
        { startMs: 3_500, endMs: 4_000, text: "İkinci" },
      ],
    };

    const encoded = serializeSrt(document, { newline: "\n" });
    expect(encoded).toContain("00:00:00,012 --> 00:00:02,345");
    expect(parseSrt(encoded).cues).toEqual([
      { id: "qa-1", startMs: 12, endMs: 2_345, text: "İstanbul\nOpenAI" },
      { id: "2", startMs: 3_500, endMs: 4_000, text: "İkinci" },
    ]);
  });
});

describe("WebVTT caption format", () => {
  const sample = `WEBVTT Türkçe QA
Kind: captions
Language: tr

STYLE
::cue(.brand) { color: lime; }

NOTE edit lock
korunur

intro
00:01.000 --> 00:03.250 line:90% align:center
<v.presenter Ayşe &amp; Ece><c.brand>Merhaba
world</c></v>
`;

  it("retains header blocks, cue settings, speaker and a whole-cue class", () => {
    const parsed = parseWebVtt(sample);

    expect(parsed.vtt).toEqual({
      header: "Türkçe QA",
      headerLines: ["Kind: captions", "Language: tr"],
      blocks: [
        { type: "STYLE", text: "::cue(.brand) { color: lime; }" },
        { type: "NOTE", text: "edit lock\nkorunur" },
      ],
    });
    expect(parsed.cues).toEqual([
      {
        id: "intro",
        startMs: 1_000,
        endMs: 3_250,
        text: "Merhaba\nworld",
        speaker: "Ayşe & Ece",
        style: "brand",
        settings: "line:90% align:center",
        vtt: { classes: ["brand"], voiceClasses: ["presenter"] },
      },
    ]);
  });

  it("round-trips native voice/class metadata and cue timing", () => {
    const first = parseWebVtt(sample);
    const encoded = serializeWebVtt(first);
    const second = parseWebVtt(encoded);

    expect(encoded).toContain("<v.presenter Ayşe &amp; Ece><c.brand>");
    expect(second.cues).toEqual(first.cues);
    expect(second.vtt).toEqual(first.vtt);
  });
});

describe("ASS caption format", () => {
  const sample = `[Script Info]
Title: Marka QA
ScriptType: v4.00+
PlayResX: 1080
PlayResY: 1920

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Brand,Montserrat,56,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,-1,0,0,0,100,100,0,0,1,3,1,2,24,24,72,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 2,0:00:01.20,0:00:04.56,Brand,Ayşe,0010,0020,0030,template-a,{\\fad(150,200)\\move(100,800,100,600,0,500)\\b1}Merhaba,\\NDünya
Comment: 0,0:00:05.00,0:00:06.00,Brand,Editör,0,0,0,qa-note,Kontrol
`;

  it("parses styles, speakers, commas, line breaks and structured motion tags", () => {
    const parsed = parseAss(sample);

    expect(parsed.ass?.scriptInfo).toMatchObject({
      Title: "Marka QA",
      PlayResX: "1080",
      PlayResY: "1920",
    });
    expect(parsed.styles?.[0]).toMatchObject({
      name: "Brand",
      fontName: "Montserrat",
      fontSize: 56,
      bold: true,
      alignment: 2,
    });
    expect(parsed.cues[0]).toMatchObject({
      startMs: 1_200,
      endMs: 4_560,
      text: "Merhaba,\nDünya",
      speaker: "Ayşe",
      style: "Brand",
      ass: {
        eventType: "Dialogue",
        layer: 2,
        marginL: 10,
        marginR: 20,
        marginV: 30,
        effect: "template-a",
        overrides: { bold: true },
        animations: [
          { kind: "fade", fadeInMs: 150, fadeOutMs: 200 },
          {
            kind: "move",
            fromX: 100,
            fromY: 800,
            toX: 100,
            toY: 600,
            startMs: 0,
            endMs: 500,
          },
        ],
      },
    });
    expect(parsed.cues[1].ass?.eventType).toBe("Comment");
  });

  it("round-trips the safe style, event and animation subset", () => {
    const first = parseAss(sample);
    const encoded = serializeAss(first, { newline: "\n" });
    const second = parseAss(encoded);

    expect(encoded).toContain("{\\b1\\fad(150,200)\\move(100,800,100,600,0,500)}");
    expect(second.cues.map(({ startMs, endMs, text, speaker, style, ass }) => ({
      startMs,
      endMs,
      text,
      speaker,
      style,
      eventType: ass?.eventType,
      animations: ass?.animations,
    }))).toEqual(first.cues.map(({ startMs, endMs, text, speaker, style, ass }) => ({
      startMs,
      endMs,
      text,
      speaker,
      style,
      eventType: ass?.eventType,
      animations: ass?.animations,
    })));
    expect(second.styles?.[0]).toMatchObject({ name: "Brand", fontName: "Montserrat", fontSize: 56 });
  });

  it("escapes untrusted fields and text while emitting bounded structured animations", () => {
    const text = "literal {\\p1} ve \\N\nikinci satır";
    const encoded = serializeAss(
      {
        cues: [
          {
            startMs: 0,
            endMs: 2_000,
            text,
            speaker: "Ayşe,\n[Events]",
            style: "Brand,\nInjected",
            ass: {
              effect: "fx,\nDialogue: injected",
              overrides: { fontName: "Inter}\\p1{", primaryColor: "#112233", alignment: 99 },
              animations: [
                { kind: "fade", fadeInMs: -10, fadeOutMs: Number.POSITIVE_INFINITY },
                { kind: "position", x: 1_000_000, y: -1_000_000 },
              ],
            },
          },
        ],
      },
      { newline: "\n" },
    );

    expect(encoded.match(/^\[Events\]$/gm)).toHaveLength(1);
    expect(encoded.match(/^Dialogue:/gm)).toHaveLength(1);
    expect(encoded).not.toContain("\nDialogue: injected");
    expect(encoded).toContain("{\\fnInterp1\\c&H00332211\\an9\\fad(0,0)\\pos(100000,-100000)}");
    expect(parseAss(encoded).cues[0].text).toBe(text);
  });

  it("rejects invalid cue ranges instead of serializing corrupt timestamps", () => {
    expect(() =>
      serializeAss({ cues: [{ startMs: 2_000, endMs: 1_000, text: "bozuk" }] }),
    ).toThrow(/ends before it starts/);
  });
});

describe("caption format dispatch", () => {
  it("detects and dispatches all public format names", () => {
    expect(detectCaptionFormat("WEBVTT\n\n")).toBe("vtt");
    expect(detectCaptionFormat("[Script Info]\nScriptType: v4.00+\n")).toBe("ass");
    expect(detectCaptionFormat("1\n00:00:00,000 --> 00:00:01,000\nx\n")).toBe("srt");
    expect(parseCaption("WEBVTT\n\n", "webvtt").format).toBe("vtt");
    expect(serializeCaption({ cues: [] }, "webvtt")).toBe("WEBVTT\n");
  });
});
