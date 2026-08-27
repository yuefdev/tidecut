export type ProjectCoverAspect = "9:16" | "1:1" | "16:9";

/**
 * A compact project-only thumbnail. It is deliberately separate from the
 * timeline: choosing a cover must not change the first frame of the export.
 */
export interface ProjectCover {
  imageDataUrl: string;
  timeMs: number;
  aspect: ProjectCoverAspect;
  updatedAtMs: number;
}

const MAX_COVER_DATA_URL_LENGTH = 2_500_000;
const COVER_IMAGE_DATA_URL = /^data:image\/(?:jpeg|png);base64,[a-z0-9+/=]+$/i;

export function normalizeProjectCover(value: unknown): ProjectCover | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const candidate = value as Record<string, unknown>;
  if (
    typeof candidate.imageDataUrl !== "string" ||
    candidate.imageDataUrl.length === 0 ||
    candidate.imageDataUrl.length > MAX_COVER_DATA_URL_LENGTH ||
    !COVER_IMAGE_DATA_URL.test(candidate.imageDataUrl) ||
    typeof candidate.timeMs !== "number" ||
    !Number.isFinite(candidate.timeMs) ||
    candidate.timeMs < 0 ||
    (candidate.aspect !== "9:16" &&
      candidate.aspect !== "1:1" &&
      candidate.aspect !== "16:9") ||
    typeof candidate.updatedAtMs !== "number" ||
    !Number.isFinite(candidate.updatedAtMs)
  ) {
    return null;
  }

  return {
    imageDataUrl: candidate.imageDataUrl,
    timeMs: Math.round(candidate.timeMs),
    aspect: candidate.aspect,
    updatedAtMs: Math.round(candidate.updatedAtMs),
  };
}
