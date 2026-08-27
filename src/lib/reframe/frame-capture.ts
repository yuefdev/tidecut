export interface CapturedVideoFrame {
  dataUrl: string;
  sourceWidth: number;
  sourceHeight: number;
  previewWidth: number;
  previewHeight: number;
  capturedAtMs: number;
}

export function fitFrameWithin(
  width: number,
  height: number,
  maxDimension = 1280,
): { width: number; height: number } {
  if (
    !Number.isFinite(width) ||
    !Number.isFinite(height) ||
    !Number.isFinite(maxDimension) ||
    width <= 0 ||
    height <= 0 ||
    maxDimension <= 0
  ) {
    throw new RangeError("Video frame dimensions must be positive finite numbers.");
  }
  const scale = Math.min(1, maxDimension / Math.max(width, height));
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

export function clampCaptureTime(
  requestedSeconds: number,
  durationSeconds: number,
): number {
  if (!Number.isFinite(requestedSeconds)) return 0;
  const safeDuration = Number.isFinite(durationSeconds) && durationSeconds > 0
    ? durationSeconds
    : Number.POSITIVE_INFINITY;
  // Avoid asking decoders for the exact stream end where no displayable frame
  // may exist. A 1 ms guard is enough without changing the selected moment.
  const lastFrameTime = Number.isFinite(safeDuration)
    ? Math.max(0, safeDuration - 0.001)
    : safeDuration;
  return Math.min(lastFrameTime, Math.max(0, requestedSeconds));
}

export async function captureVideoFrame(
  sourceUrl: string,
  requestedSeconds: number,
  options: { maxDimension?: number; timeoutMs?: number } = {},
): Promise<CapturedVideoFrame> {
  if (!sourceUrl) throw new Error("Video source is required for target selection.");
  if (typeof document === "undefined") {
    throw new Error("Video frame capture requires a browser document.");
  }

  const video = document.createElement("video");
  video.preload = "auto";
  video.muted = true;
  video.playsInline = true;
  // Tauri's asset protocol is a different origin from the editor webview.
  // Request it with CORS before assigning src so the decoded frame does not
  // taint the canvas and can safely be exported for target selection.
  video.crossOrigin = "anonymous";
  video.src = sourceUrl;
  const timeoutMs = Math.max(1_000, options.timeoutMs ?? 20_000);

  try {
    await waitForMediaEvent(video, "loadedmetadata", timeoutMs);
    if (video.videoWidth <= 0 || video.videoHeight <= 0) {
      throw new Error("Video metadata did not include usable frame dimensions.");
    }
    const captureSeconds = clampCaptureTime(requestedSeconds, video.duration);
    if (Math.abs(video.currentTime - captureSeconds) > 0.0005 || video.readyState < 2) {
      video.currentTime = captureSeconds;
      await waitForMediaEvent(video, "seeked", timeoutMs);
    }

    const size = fitFrameWithin(
      video.videoWidth,
      video.videoHeight,
      options.maxDimension ?? 1280,
    );
    const canvas = document.createElement("canvas");
    canvas.width = size.width;
    canvas.height = size.height;
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) throw new Error("Canvas 2D is unavailable for target selection.");
    context.imageSmoothingEnabled = true;
    context.imageSmoothingQuality = "high";
    context.drawImage(video, 0, 0, size.width, size.height);
    return {
      dataUrl: canvas.toDataURL("image/jpeg", 0.92),
      sourceWidth: video.videoWidth,
      sourceHeight: video.videoHeight,
      previewWidth: size.width,
      previewHeight: size.height,
      capturedAtMs: Math.round(captureSeconds * 1_000),
    };
  } finally {
    video.pause();
    video.removeAttribute("src");
    video.load();
  }
}

function waitForMediaEvent(
  media: HTMLMediaElement,
  eventName: "loadedmetadata" | "seeked",
  timeoutMs: number,
): Promise<void> {
  if (eventName === "loadedmetadata" && media.readyState >= 1) return Promise.resolve();
  return new Promise((resolve, reject) => {
    let settled = false;
    const cleanup = () => {
      media.removeEventListener(eventName, handleReady);
      media.removeEventListener("error", handleError);
      window.clearTimeout(timeout);
    };
    const finish = (callback: () => void) => {
      if (settled) return;
      settled = true;
      cleanup();
      callback();
    };
    const handleReady = () => finish(resolve);
    const handleError = () => finish(() => reject(new Error("Video frame could not be decoded.")));
    const timeout = window.setTimeout(
      () => finish(() => reject(new Error("Video frame capture timed out."))),
      timeoutMs,
    );
    media.addEventListener(eventName, handleReady, { once: true });
    media.addEventListener("error", handleError, { once: true });
  });
}
