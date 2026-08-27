import type { TimelineKeyframe } from "./timeline-engine";
import type { VoiceRiderPoint } from "$lib/render/types";

const MAX_RIDER_POINTS = 4_000;

/** Normalizes the Rust AI envelope into clip-local, persisted keyframes. */
export function toVoiceRiderKeyframes(
  points: readonly VoiceRiderPoint[],
  durationSeconds: number,
): TimelineKeyframe[] {
  const durationMs = Math.max(1, Math.round(durationSeconds * 1_000));
  const byTime = new Map<number, VoiceRiderPoint>();
  for (const point of points) {
    if (
      !Number.isFinite(point.atMs) ||
      !Number.isFinite(point.gain) ||
      point.gain <= 0
    ) continue;
    const atMs = Math.max(0, Math.min(durationMs, Math.round(point.atMs)));
    byTime.set(atMs, { ...point, atMs });
  }
  const normalized = [...byTime.values()].sort((left, right) => left.atMs - right.atMs);
  if (normalized.length === 0) {
    throw new Error("AI Voice Rider geçerli bir ses eğrisi üretmedi.");
  }
  if (normalized[0].atMs !== 0) {
    normalized.unshift({ ...normalized[0], atMs: 0 });
  }
  const last = normalized[normalized.length - 1];
  if (last.atMs !== durationMs) {
    normalized.push({ ...last, atMs: durationMs });
  }
  if (normalized.length > MAX_RIDER_POINTS) {
    throw new Error("AI Voice Rider eğrisi güvenli keyframe sınırını aştı.");
  }

  return normalized.map((point) => ({
    time: point.atMs / 1_000,
    value: Math.max(0, Math.min(4, point.gain)),
    easing: "ease-in-out",
  }));
}
