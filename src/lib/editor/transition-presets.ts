import type {
  ClipTransition,
  TransitionType,
} from "./timeline-engine";

export type TransitionCategory = "basic" | "motion" | "graphic";
export type TransitionSide = "in" | "out";

export interface TransitionPreset {
  type: TransitionType;
  label: string;
  shortLabel: string;
  category: TransitionCategory;
  defaultDuration: number;
  assetDir?: string;
  assetMode?: "screen" | "mask";
}

export const TRANSITION_CATEGORY_OPTIONS: readonly {
  value: "all" | TransitionCategory;
  label: string;
}[] = [
  { value: "all", label: "Tümü" },
  { value: "basic", label: "Temel" },
  { value: "motion", label: "Hareket" },
  { value: "graphic", label: "Grafik" },
];

/**
 * One shared catalog drives the transition browser, timeline labels and
 * default durations. Keeping it outside a component prevents the UI choices
 * from drifting away from the actual timeline transition union.
 */
export const TRANSITION_PRESETS: readonly TransitionPreset[] = [
  {
    type: "none",
    label: "Geçiş yok",
    shortLabel: "Yok",
    category: "basic",
    defaultDuration: 0,
  },
  {
    type: "crossfade",
    label: "Yumuşak geçiş",
    shortLabel: "Crossfade",
    category: "basic",
    defaultDuration: 0.65,
  },
  {
    type: "dip-to-black",
    label: "Siyaha geç",
    shortLabel: "Siyah",
    category: "basic",
    defaultDuration: 0.55,
  },
  {
    type: "wipe-left",
    label: "Sola süpür",
    shortLabel: "Wipe ←",
    category: "basic",
    defaultDuration: 0.55,
  },
  {
    type: "wipe-right",
    label: "Sağa süpür",
    shortLabel: "Wipe →",
    category: "basic",
    defaultDuration: 0.55,
  },
  {
    type: "slide-up",
    label: "Yukarı kaydır",
    shortLabel: "Kaydır ↑",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "slide-down",
    label: "Aşağı kaydır",
    shortLabel: "Kaydır ↓",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "slide-left",
    label: "Sola kaydır",
    shortLabel: "Kaydır ←",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "slide-right",
    label: "Sağa kaydır",
    shortLabel: "Kaydır →",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "zoom-in",
    label: "Yakınlaş",
    shortLabel: "Yakınlaş",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "zoom-out",
    label: "Uzaklaş",
    shortLabel: "Uzaklaş",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "spin",
    label: "Dönerek geç",
    shortLabel: "Dönüş",
    category: "motion",
    defaultDuration: 0.65,
  },
  {
    type: "flip-3d",
    label: "3D çevir",
    shortLabel: "3D çevir",
    category: "motion",
    defaultDuration: 0.65,
  },
  {
    type: "blur",
    label: "Bulanık geçiş",
    shortLabel: "Bulanık",
    category: "motion",
    defaultDuration: 0.55,
  },
  {
    type: "whip-left",
    label: "Hızlı sola savur",
    shortLabel: "Whip ←",
    category: "motion",
    defaultDuration: 0.4,
  },
  {
    type: "whip-right",
    label: "Hızlı sağa savur",
    shortLabel: "Whip →",
    category: "motion",
    defaultDuration: 0.4,
  },
  {
    type: "fx-light-leak",
    label: "Işık sızması",
    shortLabel: "Işık",
    category: "graphic",
    defaultDuration: 0.7,
    assetDir: "light-leak",
    assetMode: "screen",
  },
  {
    type: "fx-film-burn",
    label: "Film yanığı",
    shortLabel: "Film yanığı",
    category: "graphic",
    defaultDuration: 0.7,
    assetDir: "film-burn",
    assetMode: "screen",
  },
  {
    type: "fx-glitch",
    label: "Glitch",
    shortLabel: "Glitch",
    category: "graphic",
    defaultDuration: 0.55,
    assetDir: "glitch",
    assetMode: "screen",
  },
  {
    type: "fx-ink",
    label: "Mürekkep",
    shortLabel: "Mürekkep",
    category: "graphic",
    defaultDuration: 0.75,
    assetDir: "ink",
    assetMode: "mask",
  },
  {
    type: "fx-brush",
    label: "Fırça",
    shortLabel: "Fırça",
    category: "graphic",
    defaultDuration: 0.7,
    assetDir: "brush",
    assetMode: "mask",
  },
];

const PRESET_BY_TYPE = new Map<TransitionType, TransitionPreset>(
  TRANSITION_PRESETS.map((preset) => [preset.type, preset]),
);

const NATIVE_EXPORT_TYPES = new Set<TransitionType>([
  "none",
  "crossfade",
  "dip-to-black",
  "wipe-left",
  "wipe-right",
]);

export function isTransitionType(value: unknown): value is TransitionType {
  return typeof value === "string" && PRESET_BY_TYPE.has(value as TransitionType);
}

export function getTransitionPreset(type: TransitionType): TransitionPreset {
  return PRESET_BY_TYPE.get(type) ?? TRANSITION_PRESETS[0];
}

/** Motion/graphic effects currently retain timing but render as dissolve. */
export function isTransitionExportApproximate(type: TransitionType): boolean {
  return !NATIVE_EXPORT_TYPES.has(type);
}

export function clampTransitionDuration(
  value: number,
  clipDuration: number,
): number {
  const maximum = Math.max(0, Number.isFinite(clipDuration) ? clipDuration : 0);
  const duration = Number.isFinite(value) ? value : 0;
  return Math.min(maximum, Math.max(0, duration));
}

export function updateTransitionSide(
  current: ClipTransition,
  key: "type" | "duration",
  value: string | number,
  clipDuration: number,
): ClipTransition {
  if (key === "type") {
    const type = isTransitionType(value) ? value : "none";
    if (type === "none") return { type: "none", duration: 0 };

    const maximum = Math.max(0, clipDuration);
    if (maximum === 0) return { type: "none", duration: 0 };
    const requestedDuration = current.duration > 0
      ? current.duration
      : getTransitionPreset(type).defaultDuration;
    return {
      type,
      duration: clampTransitionDuration(requestedDuration, maximum),
    };
  }

  const duration = clampTransitionDuration(Number(value), clipDuration);
  if (duration === 0 || current.type === "none") {
    return { type: "none", duration: 0 };
  }
  return { ...current, duration };
}
