export const FILMSTRIP_FRAME_WIDTH = 48;
export const FILMSTRIP_FRAME_HEIGHT = 56;
export const MAX_FILMSTRIP_FRAMES = 48;

export interface FilmstripFramePlanInput {
  videoDuration: number;
  sourceOffset: number;
  sourceDuration: number;
  frameCount: number;
}

export function getFilmstripFrameCount(clipWidth: number): number {
  const safeWidth = Number.isFinite(clipWidth) ? Math.max(0, clipWidth) : 0;
  return Math.min(
    MAX_FILMSTRIP_FRAMES,
    Math.max(1, Math.ceil(safeWidth / FILMSTRIP_FRAME_WIDTH)),
  );
}

export type FilmstripRenderRequest = readonly [
  filePath: string,
  sourceDuration: number,
  sourceOffset: number,
  frameCount: number,
];

/**
 * A primitive equality key for Svelte effects. Timeline project updates clone
 * clip objects, but unchanged scalar thumbnail inputs must not restart every
 * decoder in the track.
 */
export function buildFilmstripRenderRequest(
  filePath: string,
  sourceDuration: number,
  sourceOffset: number,
  clipWidth: number,
): string {
  const request: FilmstripRenderRequest = [
    filePath,
    Math.max(0, finiteOr(sourceDuration, 0)),
    Math.max(0, finiteOr(sourceOffset, 0)),
    getFilmstripFrameCount(clipWidth),
  ];
  return JSON.stringify(request);
}

export function buildFilmstripFrameTimes({
  videoDuration,
  sourceOffset,
  sourceDuration,
  frameCount,
}: FilmstripFramePlanInput): number[] {
  if (!Number.isFinite(videoDuration) || videoDuration <= 0) return [];

  const count = Math.max(1, Math.floor(frameCount));
  // Avoid asking Chromium for the exact EOF frame; several containers expose an
  // empty/undecodable frame at precisely video.duration.
  const endGuard = Math.min(0.05, Math.max(0.001, videoDuration * 0.005));
  const maxSeekTime = Math.max(0, videoDuration - endGuard);
  const start = clamp(finiteOr(sourceOffset, 0), 0, maxSeekTime);
  const requestedDuration = Math.max(0, finiteOr(sourceDuration, 0));
  const end = clamp(start + requestedDuration, start, maxSeekTime);

  if (count === 1 || end <= start) return [start];

  return Array.from({ length: count }, (_, index) => {
    const progress = index / (count - 1);
    return start + (end - start) * progress;
  });
}

/**
 * Returns the closest successfully decoded frame for every filmstrip slot.
 * Missing decoder frames can then be filled without publishing transparent
 * gaps to the visible timeline canvas. A value of -1 means no frame decoded.
 */
export function mapFilmstripFrameSources(
  renderedFrames: readonly boolean[],
): number[] {
  const available = renderedFrames
    .map((rendered, index) => (rendered ? index : -1))
    .filter((index) => index >= 0);
  if (available.length === 0) {
    return renderedFrames.map(() => -1);
  }

  return renderedFrames.map((rendered, index) => {
    if (rendered) return index;
    let nearest = available[0];
    for (const candidate of available.slice(1)) {
      if (Math.abs(candidate - index) < Math.abs(nearest - index)) {
        nearest = candidate;
      }
    }
    return nearest;
  });
}

export interface FilmstripTask {
  readonly done: Promise<void>;
  cancel(): void;
}

interface QueuedTask {
  readonly controller: AbortController;
  readonly run: (signal: AbortSignal) => Promise<void>;
  readonly resolve: () => void;
  readonly reject: (reason: unknown) => void;
}

export interface FilmstripTaskQueue {
  schedule(run: (signal: AbortSignal) => Promise<void>): FilmstripTask;
}

export function createFilmstripTaskQueue(concurrency = 2): FilmstripTaskQueue {
  const limit = Math.max(1, Math.floor(concurrency));
  const waiting: QueuedTask[] = [];
  let active = 0;

  const pump = () => {
    while (active < limit && waiting.length > 0) {
      const next = waiting.shift();
      if (!next) return;
      if (next.controller.signal.aborted) {
        next.resolve();
        continue;
      }

      active += 1;
      void next
        .run(next.controller.signal)
        .then(next.resolve, next.reject)
        .finally(() => {
          active -= 1;
          pump();
        });
    }
  };

  return {
    schedule(run) {
      const controller = new AbortController();
      let resolve!: () => void;
      let reject!: (reason: unknown) => void;
      const done = new Promise<void>((resolvePromise, rejectPromise) => {
        resolve = resolvePromise;
        reject = rejectPromise;
      });

      const queuedTask = { controller, run, resolve, reject };
      waiting.push(queuedTask);
      pump();

      return {
        done,
        cancel() {
          controller.abort();
          // Remove work that has not started immediately. Without this, rapid
          // trim pointer events can leave hundreds of cancelled entries waiting
          // behind the active decoder before their promises settle.
          const waitingIndex = waiting.indexOf(queuedTask);
          if (waitingIndex !== -1) {
            waiting.splice(waitingIndex, 1);
            resolve();
            pump();
          }
        },
      };
    },
  };
}

// WebView video decoders are a finite resource. Splitting one source into many
// clips must not create one simultaneous decoder per timeline segment.
export const filmstripTaskQueue = createFilmstripTaskQueue(2);

function finiteOr(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
