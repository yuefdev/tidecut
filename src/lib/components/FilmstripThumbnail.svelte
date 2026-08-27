<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import {
    FILMSTRIP_FRAME_HEIGHT,
    FILMSTRIP_FRAME_WIDTH,
    buildFilmstripRenderRequest,
    buildFilmstripFrameTimes,
    filmstripTaskQueue,
    mapFilmstripFrameSources,
    type FilmstripTask,
  } from "$lib/editor/filmstrip-runtime";

  interface Props {
    filePath: string;
    sourceDuration: number;
    clipWidth: number;
    videoOffset?: number;
  }

  let {
    filePath,
    sourceDuration,
    clipWidth,
    videoOffset = 0,
  }: Props = $props();

  let canvasRef = $state<HTMLCanvasElement>();
  let isLoading = $state(true);
  let hasFrames = $state(false);
  let renderFailed = $state(false);
  let renderGeneration = 0;
  let hasPublishedFrames = false;
  let publishedPath: string | null = null;

  let renderRequest = $derived(
    buildFilmstripRenderRequest(
      filePath,
      sourceDuration,
      videoOffset,
      clipWidth,
    ),
  );

  const IMAGE_FILE_RE =
    /\.(png|jpe?g|webp|gif|bmp|tiff?|avif|heic|heif|svg)$/i;

  $effect(() => {
    const canvas = canvasRef;
    const [path, sampledDuration, sourceOffset, frameCount] = JSON.parse(
      renderRequest,
    ) as [string, number, number, number];
    const generation = ++renderGeneration;

    if (!canvas) return;

    const visibleCtx = canvas.getContext("2d");
    if (!visibleCtx) {
      isLoading = false;
      renderFailed = true;
      return;
    }

    const targetWidth = frameCount * FILMSTRIP_FRAME_WIDTH;
    if (!path) {
      canvas.width = targetWidth;
      canvas.height = FILMSTRIP_FRAME_HEIGHT;
      visibleCtx.clearRect(0, 0, canvas.width, canvas.height);
      hasPublishedFrames = false;
      publishedPath = null;
      hasFrames = false;
      isLoading = false;
      renderFailed = true;
      return;
    }

    const preservePublishedFrames = hasPublishedFrames && publishedPath === path;
    if (!preservePublishedFrames) {
      // A different source must never display the old clip's frames. For trim,
      // duration and width updates on the same source, however, keep the last
      // complete strip visible while its replacement renders offscreen.
      canvas.width = targetWidth;
      canvas.height = FILMSTRIP_FRAME_HEIGHT;
      visibleCtx.clearRect(0, 0, canvas.width, canvas.height);
      hasPublishedFrames = false;
      publishedPath = null;
      hasFrames = false;
    }
    isLoading = true;
    renderFailed = false;

    let task: FilmstripTask | null = null;
    // Pointer trim events can arrive every animation frame. Debounce refreshes
    // once a strip exists so cancelled decoder jobs do not flood the queue.
    const refreshTimer = window.setTimeout(
      () => {
        if (generation !== renderGeneration) return;

        const stagingCanvas = document.createElement("canvas");
        stagingCanvas.width = targetWidth;
        stagingCanvas.height = FILMSTRIP_FRAME_HEIGHT;
        const stagingCtx = stagingCanvas.getContext("2d");
        if (!stagingCtx) {
          isLoading = false;
          renderFailed = !hasPublishedFrames;
          return;
        }

        task = filmstripTaskQueue.schedule(async (signal) => {
          let capturedFrames = 0;
          const revealFirstFrame = (frameIndex: number) => {
            if (
              signal.aborted ||
              generation !== renderGeneration ||
              hasPublishedFrames
            ) {
              return;
            }
            if (
              publishFirstFramePreview(
                canvas,
                visibleCtx,
                stagingCanvas,
                frameIndex,
                frameCount,
              )
            ) {
              hasPublishedFrames = true;
              publishedPath = path;
              hasFrames = true;
            }
          };

          try {
            capturedFrames = IMAGE_FILE_RE.test(path)
              ? await renderImageFilmstrip(
                  stagingCtx,
                  path,
                  frameCount,
                  signal,
                  revealFirstFrame,
                )
              : await renderVideoFilmstrip(
                  stagingCtx,
                  path,
                  sampledDuration,
                  sourceOffset,
                  frameCount,
                  signal,
                  revealFirstFrame,
                );
          } catch {
            capturedFrames = 0;
          }

          if (signal.aborted || generation !== renderGeneration) return;
          if (capturedFrames > 0) {
            publishCompletedFilmstrip(
              canvas,
              visibleCtx,
              stagingCanvas,
            );
            hasPublishedFrames = true;
            publishedPath = path;
            hasFrames = true;
          }
          isLoading = false;
          renderFailed = capturedFrames === 0 && !hasPublishedFrames;
        });

        // The decoder task handles media failures as a visual fallback. This
        // catch only protects against an unexpected queue/runtime failure.
        void task.done.catch(() => {
          if (generation !== renderGeneration) return;
          isLoading = false;
          renderFailed = !hasPublishedFrames;
        });
      },
      preservePublishedFrames ? 60 : 0,
    );

    return () => {
      window.clearTimeout(refreshTimer);
      task?.cancel();
      if (generation === renderGeneration) renderGeneration += 1;
    };
  });

  async function renderVideoFilmstrip(
    ctx: CanvasRenderingContext2D,
    path: string,
    sourceDuration: number,
    sourceOffset: number,
    frameCount: number,
    signal: AbortSignal,
    onFirstFrame: (frameIndex: number) => void,
  ): Promise<number> {
    const video = document.createElement("video");
    video.crossOrigin = "anonymous";
    video.muted = true;
    video.playsInline = true;
    video.preload = "auto";

    try {
      const metadataReady = waitForMediaEvent(
        video,
        "loadedmetadata",
        signal,
        5_000,
      );
      video.src = convertFileSrc(path);
      video.load();
      if (!(await metadataReady) || signal.aborted) return 0;

      const frameTimes = buildFilmstripFrameTimes({
        videoDuration: resolveVideoDuration(video),
        sourceOffset,
        sourceDuration,
        frameCount,
      });
      let captured = 0;
      const renderedFrames = frameTimes.map(() => false);

      for (let index = 0; index < frameTimes.length; index += 1) {
        if (signal.aborted) break;
        const frameReady = await seekVideo(video, frameTimes[index], signal);
        if (!frameReady || signal.aborted) continue;

        const drawn = drawCover(
          ctx,
          video,
          video.videoWidth,
          video.videoHeight,
          index * FILMSTRIP_FRAME_WIDTH,
        );
        if (!drawn) continue;
        renderedFrames[index] = true;
        captured += 1;
        if (captured === 1) onFirstFrame(index);
      }

      if (captured === 0) return 0;
      fillMissingFrames(ctx, renderedFrames);
      return frameTimes.length;
    } finally {
      video.removeAttribute("src");
      video.load();
    }
  }

  async function renderImageFilmstrip(
    ctx: CanvasRenderingContext2D,
    path: string,
    frameCount: number,
    signal: AbortSignal,
    onFirstFrame: (frameIndex: number) => void,
  ): Promise<number> {
    const source = new Image();
    source.crossOrigin = "anonymous";
    try {
      const imageReady = waitForImage(source, signal, 5_000);
      source.src = convertFileSrc(path);
      if (!(await imageReady) || signal.aborted) return 0;

      let captured = 0;
      for (let index = 0; index < frameCount; index += 1) {
        if (signal.aborted) break;
        if (
          drawCover(
            ctx,
            source,
            source.naturalWidth,
            source.naturalHeight,
            index * FILMSTRIP_FRAME_WIDTH,
          )
        ) {
          captured += 1;
          if (captured === 1) onFirstFrame(index);
        }
      }
      return captured;
    } finally {
      source.removeAttribute("src");
    }
  }

  function fillMissingFrames(
    ctx: CanvasRenderingContext2D,
    renderedFrames: readonly boolean[],
  ) {
    const sourceFrames = mapFilmstripFrameSources(renderedFrames);
    for (let index = 0; index < sourceFrames.length; index += 1) {
      if (renderedFrames[index]) continue;
      const sourceIndex = sourceFrames[index];
      if (sourceIndex < 0) continue;
      ctx.drawImage(
        ctx.canvas,
        sourceIndex * FILMSTRIP_FRAME_WIDTH,
        0,
        FILMSTRIP_FRAME_WIDTH,
        FILMSTRIP_FRAME_HEIGHT,
        index * FILMSTRIP_FRAME_WIDTH,
        0,
        FILMSTRIP_FRAME_WIDTH,
        FILMSTRIP_FRAME_HEIGHT,
      );
    }
  }

  function publishFirstFramePreview(
    canvas: HTMLCanvasElement,
    ctx: CanvasRenderingContext2D,
    stagingCanvas: HTMLCanvasElement,
    frameIndex: number,
    frameCount: number,
  ): boolean {
    try {
      canvas.width = stagingCanvas.width;
      canvas.height = stagingCanvas.height;
      for (let index = 0; index < frameCount; index += 1) {
        ctx.drawImage(
          stagingCanvas,
          frameIndex * FILMSTRIP_FRAME_WIDTH,
          0,
          FILMSTRIP_FRAME_WIDTH,
          FILMSTRIP_FRAME_HEIGHT,
          index * FILMSTRIP_FRAME_WIDTH,
          0,
          FILMSTRIP_FRAME_WIDTH,
          FILMSTRIP_FRAME_HEIGHT,
        );
      }
      return true;
    } catch {
      return false;
    }
  }

  function publishCompletedFilmstrip(
    canvas: HTMLCanvasElement,
    ctx: CanvasRenderingContext2D,
    stagingCanvas: HTMLCanvasElement,
  ) {
    canvas.width = stagingCanvas.width;
    canvas.height = stagingCanvas.height;
    ctx.drawImage(stagingCanvas, 0, 0);
  }

  function resolveVideoDuration(video: HTMLVideoElement): number {
    if (Number.isFinite(video.duration) && video.duration > 0) {
      return video.duration;
    }
    if (video.seekable.length > 0) {
      const end = video.seekable.end(video.seekable.length - 1);
      if (Number.isFinite(end) && end > 0) return end;
    }
    return 0;
  }

  async function seekVideo(
    video: HTMLVideoElement,
    targetTime: number,
    signal: AbortSignal,
  ): Promise<boolean> {
    if (signal.aborted) return false;

    if (Math.abs(video.currentTime - targetTime) < 0.001) {
      if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) return true;
      return waitForMediaEvent(video, "loadeddata", signal, 2_500);
    }

    const seeked = waitForMediaEvent(video, "seeked", signal, 2_500);
    try {
      video.currentTime = targetTime;
    } catch {
      return false;
    }
    return seeked;
  }

  function waitForMediaEvent(
    media: HTMLMediaElement,
    eventName: "loadedmetadata" | "loadeddata" | "seeked",
    signal: AbortSignal,
    timeoutMs: number,
  ): Promise<boolean> {
    return new Promise((resolve) => {
      let settled = false;
      const finish = (result: boolean) => {
        if (settled) return;
        settled = true;
        window.clearTimeout(timeoutId);
        media.removeEventListener(eventName, onReady);
        media.removeEventListener("error", onError);
        signal.removeEventListener("abort", onAbort);
        resolve(result);
      };
      const onReady = () => finish(true);
      const onError = () => finish(false);
      const onAbort = () => finish(false);
      const timeoutId = window.setTimeout(() => finish(false), timeoutMs);

      media.addEventListener(eventName, onReady, { once: true });
      media.addEventListener("error", onError, { once: true });
      signal.addEventListener("abort", onAbort, { once: true });
    });
  }

  function waitForImage(
    image: HTMLImageElement,
    signal: AbortSignal,
    timeoutMs: number,
  ): Promise<boolean> {
    return new Promise((resolve) => {
      let settled = false;
      const finish = (result: boolean) => {
        if (settled) return;
        settled = true;
        window.clearTimeout(timeoutId);
        image.removeEventListener("load", onLoad);
        image.removeEventListener("error", onError);
        signal.removeEventListener("abort", onAbort);
        resolve(result);
      };
      const onLoad = () => finish(true);
      const onError = () => finish(false);
      const onAbort = () => finish(false);
      const timeoutId = window.setTimeout(() => finish(false), timeoutMs);

      image.addEventListener("load", onLoad, { once: true });
      image.addEventListener("error", onError, { once: true });
      signal.addEventListener("abort", onAbort, { once: true });
    });
  }

  function drawCover(
    ctx: CanvasRenderingContext2D,
    source: CanvasImageSource,
    sourceWidth: number,
    sourceHeight: number,
    destinationX: number,
  ): boolean {
    if (sourceWidth <= 0 || sourceHeight <= 0) return false;

    const targetRatio = FILMSTRIP_FRAME_WIDTH / FILMSTRIP_FRAME_HEIGHT;
    const sourceRatio = sourceWidth / sourceHeight;
    let sampleWidth = sourceWidth;
    let sampleHeight = sourceHeight;
    let sampleX = 0;
    let sampleY = 0;

    if (sourceRatio > targetRatio) {
      sampleWidth = sourceHeight * targetRatio;
      sampleX = (sourceWidth - sampleWidth) / 2;
    } else {
      sampleHeight = sourceWidth / targetRatio;
      sampleY = (sourceHeight - sampleHeight) / 2;
    }

    try {
      ctx.drawImage(
        source,
        sampleX,
        sampleY,
        sampleWidth,
        sampleHeight,
        destinationX,
        0,
        FILMSTRIP_FRAME_WIDTH,
        FILMSTRIP_FRAME_HEIGHT,
      );
      return true;
    } catch {
      return false;
    }
  }
</script>

<div class="filmstrip" aria-hidden="true">
  <canvas
    bind:this={canvasRef}
    class="filmstrip-canvas"
    class:loading={isLoading && !hasFrames}
  ></canvas>
  {#if isLoading && !hasFrames}
    <div class="filmstrip-loading"></div>
  {:else if renderFailed && !hasFrames}
    <div class="filmstrip-fallback"></div>
  {/if}
</div>

<style>
  .filmstrip {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #161a1d;
    pointer-events: none;
  }

  .filmstrip-canvas {
    display: block;
    width: 100%;
    height: 100%;
    opacity: 1;
    transition: opacity 0.12s ease-out;
  }

  .filmstrip-canvas.loading {
    opacity: 0;
  }

  .filmstrip-loading,
  .filmstrip-fallback {
    position: absolute;
    inset: 0;
  }

  .filmstrip-loading {
    background: linear-gradient(
      90deg,
      rgba(255, 255, 255, 0.02) 0%,
      rgba(255, 255, 255, 0.11) 50%,
      rgba(255, 255, 255, 0.02) 100%
    );
    animation: shimmer 1.2s infinite;
  }

  .filmstrip-fallback {
    background:
      linear-gradient(135deg, transparent 42%, rgba(255, 255, 255, 0.06) 42% 58%, transparent 58%),
      #1c2226;
    background-size: 12px 12px;
  }

  @keyframes shimmer {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(100%);
    }
  }
</style>
