import {
  generateWaveformPeaks,
  type AudioMixSource,
  type CompositorLayer,
  type FramePlan,
} from "./timeline-engine";
import {
  TRANSITION_FX_FRAME_COUNT,
  getTransitionFx,
  isTransitionFxType,
  type LoadedTransitionFx,
} from "./transition-fx";

export type VisualSourceResolver = (
  clipId: string,
) => CanvasImageSource | null | undefined;

export interface CompositorOptions {
  width: number;
  height: number;
  background?: string;
  fit?: "contain" | "cover" | "stretch";
  resolutionScale?: number;
}

const MIN_RESOLUTION_SCALE = 0.1;
const MAX_RESOLUTION_SCALE = 2;

/**
 * Deterministic Canvas 2D compositor used by realtime preview and frame export.
 * The caller owns decoding/seeking and resolves the decoded source for each layer.
 */
export class CanvasCompositor {
  readonly canvas: HTMLCanvasElement;
  readonly context: CanvasRenderingContext2D;
  private readonly stagingCanvas: HTMLCanvasElement;
  private readonly stagingContext: CanvasRenderingContext2D;
  /** Transparent scratch surface for luma-masked graphic transitions. */
  private readonly fxCanvas: HTMLCanvasElement;
  private readonly fxContext: CanvasRenderingContext2D;
  private options: Required<CompositorOptions>;

  constructor(canvas: HTMLCanvasElement, options: CompositorOptions) {
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) throw new Error("Canvas 2D is not available.");
    const stagingCanvas = document.createElement("canvas");
    const stagingContext = stagingCanvas.getContext("2d", { alpha: false });
    if (!stagingContext) throw new Error("Canvas 2D staging surface is not available.");
    const fxCanvas = document.createElement("canvas");
    const fxContext = fxCanvas.getContext("2d");
    if (!fxContext) throw new Error("Canvas 2D fx surface is not available.");
    this.canvas = canvas;
    this.context = context;
    this.stagingCanvas = stagingCanvas;
    this.stagingContext = stagingContext;
    this.fxCanvas = fxCanvas;
    this.fxContext = fxContext;
    this.options = {
      background: "#000000",
      fit: "contain",
      ...options,
      resolutionScale: normalizeResolutionScale(options.resolutionScale ?? 1),
    };
    this.resize(options.width, options.height);
  }

  setResolutionScale(scale: number): void {
    const nextScale = normalizeResolutionScale(scale);
    if (nextScale === this.options.resolutionScale) return;
    this.options.resolutionScale = nextScale;
    this.resize(this.options.width, this.options.height);
  }

  resize(width: number, height: number): void {
    if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
      throw new RangeError("Compositor dimensions must be positive integers.");
    }
    this.options.width = width;
    this.options.height = height;
    const resolutionScale = this.options.resolutionScale;
    const physicalWidth = Math.max(1, Math.round(width * resolutionScale));
    const physicalHeight = Math.max(1, Math.round(height * resolutionScale));
    const visibleSizeChanged =
      this.canvas.width !== physicalWidth || this.canvas.height !== physicalHeight;
    const stagingSizeChanged =
      this.stagingCanvas.width !== physicalWidth ||
      this.stagingCanvas.height !== physicalHeight;

    // Assigning canvas.width/height resets its bitmap even when the assigned
    // value is unchanged. Scrubbing must never erase the last committed frame.
    if (!visibleSizeChanged && !stagingSizeChanged) return;
    if (this.canvas.width !== physicalWidth) this.canvas.width = physicalWidth;
    if (this.canvas.height !== physicalHeight) this.canvas.height = physicalHeight;
    if (this.stagingCanvas.width !== physicalWidth) {
      this.stagingCanvas.width = physicalWidth;
    }
    if (this.stagingCanvas.height !== physicalHeight) {
      this.stagingCanvas.height = physicalHeight;
    }
    if (this.fxCanvas.width !== physicalWidth) this.fxCanvas.width = physicalWidth;
    if (this.fxCanvas.height !== physicalHeight) this.fxCanvas.height = physicalHeight;
    this.context.imageSmoothingEnabled = true;
    this.context.imageSmoothingQuality = "low";
    this.stagingContext.imageSmoothingEnabled = true;
    this.stagingContext.imageSmoothingQuality = "low";
  }

  render(plan: FramePlan, resolveSource: VisualSourceResolver): boolean {
    const resolvedSources = new Map<string, CanvasImageSource | null | undefined>();
    for (const layer of plan.layers) {
      const source = resolveSource(layer.clipId);
      if (layer.kind !== "text" && !source) return false;
      resolvedSources.set(layer.clipId, source);
    }

    // Compose away from the visible surface and publish only a complete frame.
    // Any source/draw failure keeps the previous visible bitmap untouched.
    const ctx = this.stagingContext;
    try {
      ctx.save();
      try {
        ctx.globalAlpha = 1;
        ctx.globalCompositeOperation = "source-over";
        ctx.fillStyle = this.options.background;
        ctx.fillRect(0, 0, this.stagingCanvas.width, this.stagingCanvas.height);
        ctx.scale(
          this.stagingCanvas.width / this.options.width,
          this.stagingCanvas.height / this.options.height,
        );
        for (const layer of plan.layers) {
          this.renderLayer(ctx, layer, resolvedSources.get(layer.clipId), plan.time);
        }
      } finally {
        ctx.restore();
      }

      this.context.save();
      try {
        this.context.globalAlpha = 1;
        this.context.globalCompositeOperation = "copy";
        this.context.drawImage(this.stagingCanvas, 0, 0);
      } finally {
        this.context.restore();
      }
      return true;
    } catch {
      // The visible canvas is transactional: a partial staging draw is never
      // committed. The next decoded frame will retry through renderFrame().
      return false;
    }
  }

  private renderLayer(
    ctx: CanvasRenderingContext2D,
    layer: CompositorLayer,
    source: CanvasImageSource | null | undefined,
    time: number,
  ): void {
    const fx =
      layer.transitionPhase !== "none" && isTransitionFxType(layer.transitionType)
        ? getTransitionFx(layer.transitionType)
        : null;

    if (fx?.mode === "mask") {
      // Draw the layer on the transparent fx surface, punch the luma matte
      // into it, then composite the masked result onto the staging frame.
      const fxCtx = this.fxContext;
      fxCtx.save();
      try {
        fxCtx.globalCompositeOperation = "source-over";
        fxCtx.clearRect(0, 0, this.fxCanvas.width, this.fxCanvas.height);
        fxCtx.scale(
          this.fxCanvas.width / this.options.width,
          this.fxCanvas.height / this.options.height,
        );
        this.drawLayerContent(fxCtx, layer, source, time);
        const frame = pickFxFrame(fx, layer, "mask");
        fxCtx.setTransform(1, 0, 0, 1, 0, 0);
        fxCtx.globalAlpha = 1;
        fxCtx.globalCompositeOperation =
          layer.transitionPhase === "in" ? "destination-in" : "destination-out";
        fxCtx.drawImage(frame, 0, 0, this.fxCanvas.width, this.fxCanvas.height);
      } finally {
        fxCtx.restore();
      }
      ctx.save();
      try {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.globalAlpha = 1;
        ctx.globalCompositeOperation = "source-over";
        ctx.drawImage(this.fxCanvas, 0, 0);
      } finally {
        ctx.restore();
      }
    } else {
      ctx.save();
      try {
        this.drawLayerContent(ctx, layer, source, time);
      } finally {
        ctx.restore();
      }
    }

    if (fx?.mode === "screen") {
      const frame = pickFxFrame(fx, layer, "screen");
      ctx.save();
      try {
        clipLayerViewport(ctx, layer, this.options.width, this.options.height);
        ctx.globalAlpha = 1;
        ctx.globalCompositeOperation = "screen";
        ctx.drawImage(frame, 0, 0, this.options.width, this.options.height);
      } finally {
        ctx.restore();
      }
    }

    if (layer.transitionType === "dip-to-black") {
      ctx.save();
      clipLayerViewport(ctx, layer, this.options.width, this.options.height);
      ctx.globalAlpha =
        layer.transitionPhase === "in"
          ? 1 - layer.transitionProgress
          : layer.transitionProgress;
      ctx.fillStyle = "#000000";
      ctx.fillRect(0, 0, this.options.width, this.options.height);
      ctx.restore();
    }
  }

  /** The shared clip/text draw path; the caller owns save/restore. */
  private drawLayerContent(
    ctx: CanvasRenderingContext2D,
    layer: CompositorLayer,
    source: CanvasImageSource | null | undefined,
    time: number,
  ): void {
    ctx.globalAlpha = layer.opacity;
    const viewport = resolveLayerViewport(layer, this.options.width, this.options.height);
    clipLayerViewport(ctx, layer, this.options.width, this.options.height);
    applyTransitionClip(ctx, layer, this.options.width, this.options.height);
    const motion = getTransitionMotion(layer, viewport.width, viewport.height);
    if (motion.blur > 0) {
      ctx.filter = `blur(${motion.blur.toFixed(2)}px)`;
    }

    let shakeDx = 0;
    let shakeDy = 0;
    if (layer.transform.shake && layer.transform.shake > 0) {
      const shakeAmt = layer.transform.shake;
      // High-frequency deterministic chaotic vibration based on playhead time
      shakeDx = (Math.sin(time * 70.0) * 0.7 + Math.sin(time * 133.0) * 0.3) * shakeAmt * 0.35;
      shakeDy = (Math.cos(time * 63.0) * 0.7 + Math.sin(time * 115.0) * 0.3) * shakeAmt * 0.35;
    }

    const centerX = viewport.x + viewport.width / 2 + layer.transform.x + motion.dx + shakeDx;
    const centerY = viewport.y + viewport.height / 2 + layer.transform.y + motion.dy + shakeDy;
    ctx.translate(centerX, centerY);
    ctx.rotate(((layer.transform.rotation + motion.rotation) * Math.PI) / 180);
    ctx.scale(
      layer.transform.scale * motion.scale * motion.scaleX,
      layer.transform.scale * motion.scale,
    );

    if (layer.kind === "text" && layer.text) {
      drawTextLayer(ctx, layer, viewport.width, viewport.height);
    } else if (source) {
      const sourceSize = getSourceSize(source);
      const target = fitRect(
        sourceSize.width,
        sourceSize.height,
        viewport.width,
        viewport.height,
        this.options.fit,
      );
      ctx.drawImage(source, -target.width / 2, -target.height / 2, target.width, target.height);
    }
  }
}

interface ResolvedLayerViewport {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Invalid runtime data falls back to the full frame instead of clipping it away. */
export function resolveLayerViewport(
  layer: Pick<CompositorLayer, "viewport">,
  canvasWidth: number,
  canvasHeight: number,
): ResolvedLayerViewport {
  const value = layer.viewport;
  if (
    !value ||
    ![value.x, value.y, value.width, value.height].every(Number.isFinite) ||
    value.x < 0 ||
    value.y < 0 ||
    value.width <= 0 ||
    value.height <= 0 ||
    value.x + value.width > 1.000001 ||
    value.y + value.height > 1.000001
  ) {
    return { x: 0, y: 0, width: canvasWidth, height: canvasHeight };
  }
  return {
    x: value.x * canvasWidth,
    y: value.y * canvasHeight,
    width: value.width * canvasWidth,
    height: value.height * canvasHeight,
  };
}

function clipLayerViewport(
  ctx: CanvasRenderingContext2D,
  layer: Pick<CompositorLayer, "viewport">,
  canvasWidth: number,
  canvasHeight: number,
): void {
  if (!layer.viewport) return;
  const viewport = resolveLayerViewport(layer, canvasWidth, canvasHeight);
  ctx.beginPath();
  ctx.rect(viewport.x, viewport.y, viewport.width, viewport.height);
  ctx.clip();
}

/**
 * Maps transition progress to an overlay frame. Mask sequences play forward
 * on both sides. Screen overlays (light leak, burn) peak at the cut: the
 * exit plays the build-up half toward the cut and the entry plays the decay
 * half away from it, so adjacent clips together play the full sequence.
 */
function pickFxFrame(
  fx: LoadedTransitionFx,
  layer: CompositorLayer,
  mode: "mask" | "screen",
): CanvasImageSource {
  const count = Math.min(TRANSITION_FX_FRAME_COUNT, fx.frames.length);
  const progress = Math.min(1, Math.max(0, layer.transitionProgress));
  let index: number;
  if (mode === "mask") {
    index = Math.floor(progress * (count - 1));
  } else {
    const half = Math.floor(count / 2);
    index =
      layer.transitionPhase === "out"
        ? Math.floor(progress * (half - 1))
        : half + Math.floor(progress * (count - half - 1));
  }
  return fx.frames[Math.min(count - 1, Math.max(0, index))];
}

interface MixerStrip {
  element: HTMLMediaElement;
  source: MediaElementAudioSourceNode;
  gain: GainNode;
  pan: StereoPannerNode;
}

interface MixerAttachOptions {
  /** Keep the element currently backing this strip playing while its audio is replaced. */
  pausePreviousElement?: boolean;
}

interface MixerDetachOptions {
  /** Defaults to true. Visual video must stay alive when only its audio route changes. */
  pauseElement?: boolean;
}

interface MediaPlaybackIntentState {
  revision: number;
  playing: boolean;
}

/**
 * Guards direct HTMLMediaElement playback outside the Web Audio mixer.
 * A pending play() promise may settle after a newer rewind/pause; the latest
 * element intent always wins so that stale video playback cannot restart its
 * picture or its mixer-routed native soundtrack.
 */
export class MediaPlaybackIntentController {
  private revision = 0;
  private intents = new WeakMap<HTMLMediaElement, MediaPlaybackIntentState>();

  play(element: HTMLMediaElement): Promise<void> {
    const revision = ++this.revision;
    this.intents.set(element, { revision, playing: true });
    let request: Promise<void>;
    try {
      request = element.play();
    } catch {
      return Promise.resolve();
    }
    return request
      .catch(() => undefined)
      .then(() => {
        const latest = this.intents.get(element);
        const superseded = !latest || latest.revision !== revision;
        if (
          superseded &&
          latest?.playing !== true &&
          !element.paused
        ) {
          element.pause();
        }
      });
  }

  pause(element: HTMLMediaElement): void {
    this.intents.set(element, {
      revision: ++this.revision,
      playing: false,
    });
    if (!element.paused) element.pause();
  }

  release(element: HTMLMediaElement): void {
    this.pause(element);
    this.intents.delete(element);
  }
}

/** A real Web Audio graph: one independently gain/pan controlled strip per clip. */
export class WebAudioMixer {
  readonly context: AudioContext;
  readonly masterGain: GainNode;
  private strips = new Map<string, MixerStrip>();
  /**
   * `HTMLMediaElement.play()` resolves asynchronously. A late resolution from
   * an older frame must never undo a newer pause/seek command, otherwise the
   * playhead can be at 0:00 while an old audio element keeps running ahead.
   */
  private syncRevision = 0;
  private playbackIntent = new WeakMap<HTMLMediaElement, boolean>();
  /**
   * The Web Audio spec allows only one MediaElementAudioSourceNode to be
   * created for a given HTMLMediaElement. Noise-cleanup switching temporarily
   * detaches the original video element, so keep and reconnect its node when
   * the user turns cleanup off/on instead of trying to create a second one.
   */
  private elementSources = new WeakMap<HTMLMediaElement, MediaElementAudioSourceNode>();

  constructor(
    context: AudioContext,
    destination: AudioNode = context.destination,
  ) {
    this.context = context;
    this.masterGain = context.createGain();
    this.masterGain.connect(destination);
  }

  /**
   * Route a media element into Web Audio without making it audible yet.
   * Creating the source also prevents a visible video's native audio from
   * leaking around the mixer when a cleaned companion WAV is used instead.
   */
  capture(element: HTMLMediaElement): MediaElementAudioSourceNode {
    let source = this.elementSources.get(element);
    if (!source) {
      source = this.context.createMediaElementSource(element);
      this.elementSources.set(element, source);
    }
    return source;
  }

  attach(
    clipId: string,
    element: HTMLMediaElement,
    options: MixerAttachOptions = {},
  ): void {
    this.syncRevision += 1;
    const current = this.strips.get(clipId);
    if (current?.element === element) return;
    if (current) {
      this.detach(clipId, {
        pauseElement: options.pausePreviousElement ?? true,
      });
    }
    for (const [otherClipId, strip] of this.strips) {
      if (strip.element === element) this.detach(otherClipId);
    }
    const source = this.capture(element);
    const gain = this.context.createGain();
    const pan = this.context.createStereoPanner();
    source.connect(gain).connect(pan).connect(this.masterGain);
    this.strips.set(clipId, { element, source, gain, pan });
    if (!this.playbackIntent.has(element)) this.playbackIntent.set(element, false);
  }

  detach(clipId: string, options: MixerDetachOptions = {}): void {
    this.syncRevision += 1;
    const strip = this.strips.get(clipId);
    if (!strip) return;
    if (options.pauseElement ?? true) {
      this.playbackIntent.set(strip.element, false);
      strip.element.pause();
    }
    strip.source.disconnect();
    strip.gain.disconnect();
    strip.pan.disconnect();
    this.strips.delete(clipId);
  }

  setMasterGain(value: number, rampSeconds = 0.01): void {
    scheduleParam(this.masterGain.gain, clamp(value, 0, 4), this.context.currentTime, rampSeconds);
  }

  async sync(
    sources: readonly AudioMixSource[],
    options: { playing: boolean; rampSeconds?: number },
  ): Promise<void> {
    const revision = ++this.syncRevision;
    if (this.context.state === "suspended" && options.playing) await this.context.resume();
    if (revision !== this.syncRevision) return;
    const active = new Map(sources.map((source) => [source.clipId, source]));
    const ramp = options.rampSeconds ?? 0.01;

    for (const [clipId, strip] of this.strips) {
      if (revision !== this.syncRevision) return;
      const mix = active.get(clipId);
      if (!mix) {
        this.playbackIntent.set(strip.element, false);
        scheduleParam(strip.gain.gain, 0, this.context.currentTime, ramp);
        if (!strip.element.paused) strip.element.pause();
        continue;
      }
      this.applyStrip(strip, mix, ramp);
      this.playbackIntent.set(strip.element, options.playing);
      if (options.playing && strip.element.paused) {
        await strip.element.play().catch(() => undefined);
        if (
          revision !== this.syncRevision &&
          this.playbackIntent.get(strip.element) !== true &&
          !strip.element.paused
        ) {
          strip.element.pause();
        }
        if (revision !== this.syncRevision) return;
      } else if (!options.playing && !strip.element.paused) {
        strip.element.pause();
      }
    }
  }

  dispose(): void {
    for (const clipId of [...this.strips.keys()]) this.detach(clipId);
    this.masterGain.disconnect();
  }

  private applyStrip(
    strip: MixerStrip,
    mix: AudioMixSource,
    rampSeconds: number,
  ): void {
    strip.element.playbackRate = clamp(mix.playbackRate, 0.05, 16);
    scheduleParam(strip.gain.gain, clamp(mix.gain, 0, 4), this.context.currentTime, rampSeconds);
    scheduleParam(strip.pan.pan, clamp(mix.pan, -1, 1), this.context.currentTime, rampSeconds);
  }
}

export async function decodeWaveform(
  context: BaseAudioContext,
  encodedAudio: ArrayBuffer,
  bucketCount: number,
): Promise<number[]> {
  // Safari mutates the supplied buffer, so keep ownership with the caller.
  const decoded = await context.decodeAudioData(encodedAudio.slice(0));
  const channels = Array.from({ length: decoded.numberOfChannels }, (_, channel) =>
    decoded.getChannelData(channel),
  );
  return generateWaveformPeaks(channels, bucketCount);
}

function drawTextLayer(
  ctx: CanvasRenderingContext2D,
  layer: CompositorLayer,
  width: number,
  height: number,
): void {
  const text = layer.text!;
  const fx = layer.textFx;

  if (fx) {
    ctx.globalAlpha = ctx.globalAlpha * fx.alpha;
    ctx.translate(fx.dxFrac * width, fx.dyFrac * height);
    if (fx.rotation !== 0) ctx.rotate((fx.rotation * Math.PI) / 180);
    ctx.scale(fx.scale * fx.scaleX, fx.scale);
  }

  ctx.font = `${text.fontWeight} ${text.fontSize}px ${text.fontFamily}`;
  ctx.textAlign = text.align;
  ctx.textBaseline = "middle";

  // Typewriter reveals characters from the full multi-line content while the
  // block metrics stay locked to the final text, so lines never re-wrap.
  const fullLines = text.content.split(/\r?\n/);
  let lines = fullLines;
  if (fx && fx.charProgress < 1) {
    const totalChars = text.content.length;
    const visibleChars = Math.ceil(fx.charProgress * totalChars);
    lines = text.content.slice(0, visibleChars).split(/\r?\n/);
  }

  const lineHeight = text.fontSize * 1.2;
  const blockWidth = Math.min(
    width,
    Math.max(
      text.fontSize,
      ...fullLines.map((line) => ctx.measureText(line).width),
    ),
  );
  const blockHeight = Math.max(lineHeight, fullLines.length * lineHeight);
  const padding = Math.max(8, text.fontSize * 0.2);
  if (text.backgroundColor !== "transparent") {
    ctx.fillStyle = text.backgroundColor;
    ctx.fillRect(
      -blockWidth / 2 - padding,
      -blockHeight / 2 - padding,
      blockWidth + padding * 2,
      blockHeight + padding * 2,
    );
  }

  const x =
    text.align === "left"
      ? -blockWidth / 2
      : text.align === "right"
        ? blockWidth / 2
        : 0;
  const firstLineY = -blockHeight / 2 + lineHeight / 2;

  const hasShadow =
    text.shadowColor !== "transparent" &&
    (text.shadowBlur > 0 || text.shadowOffsetX !== 0 || text.shadowOffsetY !== 0);
  const hasStroke = text.strokeWidth > 0;

  let fill: string | CanvasGradient = text.color;
  if (text.gradient) {
    const gradient = ctx.createLinearGradient(0, -blockHeight / 2, 0, blockHeight / 2);
    gradient.addColorStop(0, text.gradient.from);
    gradient.addColorStop(1, text.gradient.to);
    fill = gradient;
  }

  lines.forEach((line, index) => {
    const lineY = firstLineY + index * lineHeight;

    // Shadow/glow pass first so stroke and fill stay crisp above it.
    if (hasShadow) {
      ctx.save();
      ctx.shadowColor = text.shadowColor;
      ctx.shadowBlur = text.shadowBlur;
      ctx.shadowOffsetX = text.shadowOffsetX;
      ctx.shadowOffsetY = text.shadowOffsetY;
      ctx.fillStyle = fill;
      ctx.fillText(line, x, lineY, width);
      ctx.restore();
    }
    if (hasStroke) {
      ctx.save();
      ctx.lineJoin = "round";
      ctx.miterLimit = 2;
      ctx.lineWidth = text.strokeWidth * 2;
      ctx.strokeStyle = text.strokeColor;
      ctx.strokeText(line, x, lineY, width);
      ctx.restore();
    }
    ctx.fillStyle = fill;
    ctx.fillText(line, x, lineY, width);
  });
}

interface TransitionMotion {
  dx: number;
  dy: number;
  scale: number;
  scaleX: number;
  rotation: number;
  blur: number;
}

const NEUTRAL_MOTION: TransitionMotion = {
  dx: 0,
  dy: 0,
  scale: 1,
  scaleX: 1,
  rotation: 0,
  blur: 0,
};

function motionEaseOut(x: number): number {
  return 1 - Math.pow(1 - Math.min(1, Math.max(0, x)), 3);
}

function motionEaseIn(x: number): number {
  const t = Math.min(1, Math.max(0, x));
  return t * t * t;
}

/**
 * Motion-based transitions (slide/zoom/spin/3D flip/blur). `settled` is 1
 * when the clip is fully on screen, 0 at the outer edge of the transition,
 * so enter and exit share the same formulas mirrored in time.
 */
function getTransitionMotion(
  layer: CompositorLayer,
  width: number,
  height: number,
): TransitionMotion {
  if (layer.transitionPhase === "none") return NEUTRAL_MOTION;
  const settled =
    layer.transitionPhase === "in"
      ? motionEaseOut(layer.transitionProgress)
      : 1 - motionEaseIn(layer.transitionProgress);
  const away = 1 - settled;
  const direction = layer.transitionPhase === "in" ? 1 : -1;
  switch (layer.transitionType) {
    case "slide-up":
      return { ...NEUTRAL_MOTION, dy: height * 0.22 * away * direction };
    case "slide-down":
      return { ...NEUTRAL_MOTION, dy: -height * 0.22 * away * direction };
    case "slide-left":
      return { ...NEUTRAL_MOTION, dx: width * 0.24 * away * direction };
    case "slide-right":
      return { ...NEUTRAL_MOTION, dx: -width * 0.24 * away * direction };
    case "zoom-in":
      return { ...NEUTRAL_MOTION, scale: 1 - 0.6 * away * direction };
    case "zoom-out":
      return { ...NEUTRAL_MOTION, scale: 1 + 0.6 * away * direction };
    case "spin":
      return {
        ...NEUTRAL_MOTION,
        rotation: -120 * away * direction,
        scale: Math.max(0.05, 1 - 0.75 * away),
      };
    case "flip-3d":
      return {
        ...NEUTRAL_MOTION,
        scaleX: Math.max(0.02, Math.cos((away * Math.PI) / 2)),
        scale: 1 - 0.08 * away,
      };
    case "blur":
      return {
        ...NEUTRAL_MOTION,
        blur: Math.min(width, height) * 0.03 * away,
      };
    // Whip pan: fast horizontal sweep with directional motion blur, like
    // CapCut's "hızlı savurma". Larger displacement than a plain slide plus
    // blur that peaks at the outer edge and clears as the clip settles.
    case "whip-left":
      return {
        ...NEUTRAL_MOTION,
        dx: width * 0.5 * away * direction,
        blur: Math.min(width, height) * 0.05 * away,
      };
    case "whip-right":
      return {
        ...NEUTRAL_MOTION,
        dx: -width * 0.5 * away * direction,
        blur: Math.min(width, height) * 0.05 * away,
      };
    default:
      return NEUTRAL_MOTION;
  }
}

function applyTransitionClip(
  ctx: CanvasRenderingContext2D,
  layer: CompositorLayer,
  width: number,
  height: number,
): void {
  if (layer.transitionType === "wipe-left" || layer.transitionType === "wipe-right") {
    const visible = clamp(
      layer.transitionPhase === "out"
        ? 1 - layer.transitionProgress
        : layer.transitionProgress,
      0,
      1,
    );
    const clipWidth = width * visible;
    const x = layer.transitionType === "wipe-left" ? width - clipWidth : 0;
    ctx.beginPath();
    ctx.rect(x, 0, clipWidth, height);
    ctx.clip();
  }
}

function fitRect(
  sourceWidth: number,
  sourceHeight: number,
  targetWidth: number,
  targetHeight: number,
  fit: "contain" | "cover" | "stretch",
): { width: number; height: number } {
  if (fit === "stretch" || sourceWidth <= 0 || sourceHeight <= 0) {
    return { width: targetWidth, height: targetHeight };
  }
  const scale =
    fit === "cover"
      ? Math.max(targetWidth / sourceWidth, targetHeight / sourceHeight)
      : Math.min(targetWidth / sourceWidth, targetHeight / sourceHeight);
  return { width: sourceWidth * scale, height: sourceHeight * scale };
}

function getSourceSize(source: CanvasImageSource): { width: number; height: number } {
  if (source instanceof HTMLVideoElement) {
    return { width: source.videoWidth, height: source.videoHeight };
  }
  if (source instanceof HTMLImageElement) {
    return { width: source.naturalWidth, height: source.naturalHeight };
  }
  if (source instanceof SVGImageElement) {
    return {
      width: source.width.baseVal.value,
      height: source.height.baseVal.value,
    };
  }
  if (typeof VideoFrame !== "undefined" && source instanceof VideoFrame) {
    return { width: source.displayWidth, height: source.displayHeight };
  }
  const sized = source as { width?: number; height?: number };
  return { width: Number(sized.width) || 0, height: Number(sized.height) || 0 };
}

function scheduleParam(
  parameter: AudioParam,
  value: number,
  now: number,
  rampSeconds: number,
): void {
  parameter.cancelScheduledValues(now);
  parameter.setValueAtTime(parameter.value, now);
  parameter.linearRampToValueAtTime(value, now + Math.max(0.001, rampSeconds));
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function normalizeResolutionScale(scale: number): number {
  if (!Number.isFinite(scale) || scale <= 0) {
    throw new RangeError("Compositor resolution scale must be a finite positive number.");
  }
  return clamp(scale, MIN_RESOLUTION_SCALE, MAX_RESOLUTION_SCALE);
}
