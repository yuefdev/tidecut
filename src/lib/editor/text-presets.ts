import type { TextClipStyle } from "./timeline-engine";

/**
 * CapCut-style text templates. A preset patches only the *look* of the clip
 * (font, colors, stroke, shadow, gradient, animation); content, size,
 * alignment and background stay whatever the user already chose.
 */
export interface TextStylePreset {
  id: string;
  /** Short Turkish label shown on the preset chip. */
  label: string;
  /** Colors used to paint the chip preview in the Inspector. */
  chip: { color: string; background: string };
  style: Partial<TextClipStyle>;
}

/** Fonts that ship with Windows (plus Inter, the app default), so both the
 * WebView preview and the fontconfig-based ffmpeg export can resolve them. */
export const TEXT_FONT_FAMILIES: readonly string[] = [
  "Inter",
  "Arial",
  "Arial Black",
  "Segoe UI",
  "Bahnschrift",
  "Impact",
  "Georgia",
  "Times New Roman",
  "Courier New",
  "Consolas",
  "Verdana",
  "Tahoma",
  "Trebuchet MS",
  "Comic Sans MS",
];

export const TEXT_STYLE_PRESETS: readonly TextStylePreset[] = [
  {
    id: "plain",
    label: "Sade",
    chip: { color: "#ffffff", background: "#2a2d2c" },
    style: {
      fontFamily: "Inter",
      fontWeight: 600,
      color: "#ffffff",
      gradient: null,
      strokeWidth: 0,
      strokeColor: "#000000",
      shadowColor: "transparent",
      shadowBlur: 0,
      shadowOffsetX: 0,
      shadowOffsetY: 0,
      animationIn: { type: "fade", duration: 0.4 },
      animationOut: { type: "fade", duration: 0.4 },
    },
  },
  {
    id: "fire",
    label: "Ateşli",
    chip: { color: "#ffe259", background: "#4a1503" },
    style: {
      fontFamily: "Impact",
      fontWeight: 700,
      color: "#ffb52e",
      gradient: { from: "#ffe259", to: "#ff3d00" },
      strokeColor: "#5c1400",
      strokeWidth: 2,
      shadowColor: "#ff6a00",
      shadowBlur: 26,
      shadowOffsetX: 0,
      shadowOffsetY: 0,
      animationIn: { type: "zoom", duration: 0.5 },
      animationOut: { type: "fade", duration: 0.4 },
    },
  },
  {
    id: "neon",
    label: "Neon",
    chip: { color: "#7df9ff", background: "#04252b" },
    style: {
      fontFamily: "Bahnschrift",
      fontWeight: 600,
      color: "#eafffe",
      gradient: null,
      strokeColor: "#00b8d4",
      strokeWidth: 2,
      shadowColor: "#00e5ff",
      shadowBlur: 30,
      shadowOffsetX: 0,
      shadowOffsetY: 0,
      animationIn: { type: "fade", duration: 0.8 },
      animationOut: { type: "fade", duration: 0.6 },
    },
  },
  {
    id: "gold",
    label: "Altın",
    chip: { color: "#f9f295", background: "#3b2c05" },
    style: {
      fontFamily: "Georgia",
      fontWeight: 700,
      color: "#e6c34c",
      gradient: { from: "#f9f295", to: "#b8860b" },
      strokeColor: "#5a4300",
      strokeWidth: 1,
      shadowColor: "#000000",
      shadowBlur: 10,
      shadowOffsetX: 0,
      shadowOffsetY: 4,
      animationIn: { type: "slide-up", duration: 0.6 },
      animationOut: { type: "fade", duration: 0.5 },
    },
  },
  {
    id: "block-3d",
    label: "3D Blok",
    chip: { color: "#ffffff", background: "#1c1c22" },
    style: {
      fontFamily: "Arial Black",
      fontWeight: 900,
      color: "#ffffff",
      gradient: null,
      strokeColor: "#111111",
      strokeWidth: 3,
      shadowColor: "#000000",
      shadowBlur: 0,
      shadowOffsetX: 7,
      shadowOffsetY: 7,
      animationIn: { type: "flip-3d", duration: 0.7 },
      animationOut: { type: "flip-3d", duration: 0.5 },
    },
  },
  {
    id: "retro",
    label: "Retro",
    chip: { color: "#ffd93d", background: "#3d1b2c" },
    style: {
      fontFamily: "Verdana",
      fontWeight: 700,
      color: "#ffd93d",
      gradient: null,
      strokeColor: "#6b2737",
      strokeWidth: 3,
      shadowColor: "#2ec4b6",
      shadowBlur: 0,
      shadowOffsetX: 5,
      shadowOffsetY: 5,
      animationIn: { type: "bounce", duration: 0.8 },
      animationOut: { type: "slide-down", duration: 0.5 },
    },
  },
  {
    id: "cinema",
    label: "Sinema",
    chip: { color: "#ececec", background: "#101312" },
    style: {
      fontFamily: "Georgia",
      fontWeight: 400,
      color: "#ececec",
      gradient: null,
      strokeWidth: 0,
      strokeColor: "#000000",
      shadowColor: "#000000",
      shadowBlur: 18,
      shadowOffsetX: 0,
      shadowOffsetY: 2,
      animationIn: { type: "fade", duration: 1.2 },
      animationOut: { type: "fade", duration: 1 },
    },
  },
  {
    id: "typewriter",
    label: "Daktilo",
    chip: { color: "#d8ffd9", background: "#14231a" },
    style: {
      fontFamily: "Consolas",
      fontWeight: 500,
      color: "#d8ffd9",
      gradient: null,
      strokeWidth: 0,
      strokeColor: "#000000",
      shadowColor: "#123f24",
      shadowBlur: 12,
      shadowOffsetX: 0,
      shadowOffsetY: 0,
      animationIn: { type: "typewriter", duration: 1.5 },
      animationOut: { type: "fade", duration: 0.4 },
    },
  },
];
