export const EDITOR_FRAME_RATE = 30;

export interface TransportShortcutEvent {
  code: string;
  repeat?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  isComposing?: boolean;
}

export type TransportSeekAction =
  | { kind: "seek-relative"; deltaSeconds: number }
  | { kind: "seek-edge"; edge: "start" | "end" };

export type TransportShortcutAction =
  | { kind: "toggle-playback" }
  | TransportSeekAction
  | { kind: "consume" };

/**
 * Maps editor-wide transport keys without depending on DOM focus state.
 * Focus-sensitive guards stay in the component that owns the event.
 */
export function resolveTransportShortcut(
  event: TransportShortcutEvent,
): TransportShortcutAction | null {
  if (event.isComposing || event.ctrlKey || event.metaKey) return null;

  if (event.code === "Space" || event.code === "KeyK") {
    // Holding Space must never alternate between play and pause.
    return event.repeat ? { kind: "consume" } : { kind: "toggle-playback" };
  }

  if (event.code === "ArrowLeft" || event.code === "ArrowRight") {
    const direction = event.code === "ArrowRight" ? 1 : -1;
    const step = event.altKey
      ? 1 / EDITOR_FRAME_RATE
      : event.shiftKey
        ? 5
        : 1;
    return { kind: "seek-relative", deltaSeconds: direction * step };
  }

  if (event.code === "KeyJ" || event.code === "KeyL") {
    return {
      kind: "seek-relative",
      deltaSeconds: event.code === "KeyL" ? 5 : -5,
    };
  }

  if (event.code === "Home") return { kind: "seek-edge", edge: "start" };
  if (event.code === "End") return { kind: "seek-edge", edge: "end" };
  return null;
}

export function applyTransportSeek(
  currentTime: number,
  duration: number,
  action: TransportSeekAction,
): number {
  const safeDuration = Number.isFinite(duration) ? Math.max(0, duration) : 0;
  const safeCurrentTime = Number.isFinite(currentTime) ? currentTime : 0;
  const requestedTime =
    action.kind === "seek-relative"
      ? safeCurrentTime + action.deltaSeconds
      : action.edge === "end"
        ? safeDuration
        : 0;
  return Math.min(safeDuration, Math.max(0, requestedTime));
}

/** Returns the correct start point when playback is requested at the end. */
export function playbackStartTime(
  currentTime: number,
  duration: number,
): number {
  const safeDuration = Number.isFinite(duration) ? Math.max(0, duration) : 0;
  const safeCurrentTime = Number.isFinite(currentTime)
    ? Math.min(safeDuration, Math.max(0, currentTime))
    : 0;
  if (
    safeDuration > 0 &&
    safeCurrentTime >= safeDuration - 1 / EDITOR_FRAME_RATE
  ) {
    return 0;
  }
  return safeCurrentTime;
}
