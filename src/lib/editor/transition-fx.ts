import type { TransitionType } from "./timeline-engine";

/**
 * Graphic transition overlays (CapCut-style). Each entry is a 24-frame WebP
 * sequence under `static/transitions/<dir>/f01..f24.webp`, converted from
 * royalty-free Pixabay clips (Pixabay Content License).
 *
 * Modes:
 * - "screen": the frames are additively screened over the whole canvas
 *   around the cut (light leaks, film burns, glitch noise).
 * - "mask":   the frame's luminance drives the clip's alpha so the ink/brush
 *   shape reveals (enter) or swallows (exit) the clip.
 */
export type TransitionFxMode = "screen" | "mask";

export interface TransitionFxDef {
  type: TransitionType;
  mode: TransitionFxMode;
  dir: string;
}

export const TRANSITION_FX_FRAME_COUNT = 24;

export const TRANSITION_FX: readonly TransitionFxDef[] = [
  { type: "fx-light-leak", mode: "screen", dir: "light-leak" },
  { type: "fx-film-burn", mode: "screen", dir: "film-burn" },
  { type: "fx-glitch", mode: "screen", dir: "glitch" },
  { type: "fx-ink", mode: "mask", dir: "ink" },
  { type: "fx-brush", mode: "mask", dir: "brush" },
];

const FX_BY_TYPE = new Map(TRANSITION_FX.map((fx) => [fx.type as string, fx]));

export function isTransitionFxType(type: string): boolean {
  return FX_BY_TYPE.has(type);
}

export interface LoadedTransitionFx {
  mode: TransitionFxMode;
  /** Screen mode: raw frames. Mask mode: frames with alpha = luminance. */
  frames: CanvasImageSource[];
}

const loadedFx = new Map<string, LoadedTransitionFx | "loading" | "failed">();

/**
 * Synchronous lookup used from the render loop. A cache miss starts the
 * async load and returns null; the compositor falls back to a plain fade
 * until the frames are ready.
 */
export function getTransitionFx(type: string): LoadedTransitionFx | null {
  const state = loadedFx.get(type);
  if (state && state !== "loading" && state !== "failed") return state;
  if (state === undefined) void loadTransitionFx(type);
  return null;
}

async function loadTransitionFx(type: string): Promise<void> {
  const def = FX_BY_TYPE.get(type);
  if (!def || typeof window === "undefined") return;
  loadedFx.set(type, "loading");
  try {
    const images = await Promise.all(
      Array.from({ length: TRANSITION_FX_FRAME_COUNT }, (_, index) =>
        loadImage(
          `/transitions/${def.dir}/f${String(index + 1).padStart(2, "0")}.webp`,
        ),
      ),
    );
    const frames =
      def.mode === "mask" ? images.map(toLumaAlphaCanvas) : images;
    loadedFx.set(type, { mode: def.mode, frames });
  } catch {
    // Missing/corrupt frames leave the transition on its fade fallback.
    loadedFx.set(type, "failed");
  }
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.decoding = "async";
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`load failed: ${src}`));
    image.src = src;
  });
}

/** Bakes luminance into the alpha channel so `destination-in/out` can use it. */
function toLumaAlphaCanvas(image: HTMLImageElement): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(image, 0, 0);
  const data = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const pixels = data.data;
  for (let i = 0; i < pixels.length; i += 4) {
    const luma =
      0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2];
    pixels[i + 3] = luma;
  }
  ctx.putImageData(data, 0, 0);
  return canvas;
}
