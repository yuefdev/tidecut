import {
  MIN_CLIP_DURATION,
  TIMELINE_EPSILON,
  cloneProjectState,
  deleteClip,
  splitClip,
  type TimelineClip,
  type TimelineProjectState,
} from "./timeline-engine";

export interface TimelineLocalRange {
  start: number;
  end: number;
}

export interface BatchEditResult {
  state: TimelineProjectState;
  createdClipIds: string[];
  removedDuration: number;
  cutCount: number;
}

export interface DeleteClipRangesOptions {
  ripple?: boolean;
  idFactory?: () => string;
}

function defaultIdFactory(): string {
  return globalThis.crypto?.randomUUID?.() ??
    `clip-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function normalizeRanges(
  input: readonly TimelineLocalRange[],
  duration: number,
): TimelineLocalRange[] {
  const sorted = input
    .map((range) => ({
      start: Math.max(0, Math.min(duration, Number(range.start) || 0)),
      end: Math.max(0, Math.min(duration, Number(range.end) || 0)),
    }))
    .map((range) => ({
      start: Math.min(range.start, range.end),
      end: Math.max(range.start, range.end),
    }))
    .filter((range) => range.end - range.start >= MIN_CLIP_DURATION)
    .sort((left, right) => left.start - right.start);

  return sorted.reduce<TimelineLocalRange[]>((merged, range) => {
    const previous = merged.at(-1);
    if (!previous || range.start > previous.end + TIMELINE_EPSILON) {
      merged.push({ ...range });
    } else {
      previous.end = Math.max(previous.end, range.end);
    }
    return merged;
  }, []);
}

function clipAt(
  state: TimelineProjectState,
  ids: ReadonlySet<string>,
  time: number,
): TimelineClip | undefined {
  return state.clips.find(
    (clip) =>
      ids.has(clip.id) &&
      time >= clip.start - TIMELINE_EPSILON &&
      time <= clip.start + clip.duration + TIMELINE_EPSILON,
  );
}

/**
 * Removes several clip-local ranges in one pure state transaction. Ranges are
 * processed from right to left, so every input time remains relative to the
 * original clip. The caller can push exactly one history snapshot.
 */
export function deleteClipRanges(
  inputState: TimelineProjectState,
  clipId: string,
  inputRanges: readonly TimelineLocalRange[],
  options: DeleteClipRangesOptions = {},
): BatchEditResult {
  let state = cloneProjectState(inputState);
  const original = state.clips.find((clip) => clip.id === clipId);
  if (!original) throw new Error(`Unknown clip: ${clipId}`);
  const track = state.tracks.find((item) => item.id === original.trackId);
  if (!track || track.locked) throw new Error(`Track ${original.trackId} is locked.`);
  const ranges = normalizeRanges(inputRanges, original.duration);
  if (ranges.length === 0) {
    return { state, createdClipIds: [], removedDuration: 0, cutCount: 0 };
  }

  const idFactory = options.idFactory ?? defaultIdFactory;
  const pieceIds = new Set([clipId]);
  const createdClipIds: string[] = [];
  const removedRanges: TimelineLocalRange[] = [];
  let cutCount = 0;

  for (const range of [...ranges].reverse()) {
    const absoluteStart = original.start + range.start;
    const absoluteEnd = original.start + range.end;
    let piece = clipAt(
      state,
      pieceIds,
      Math.max(absoluteStart, absoluteEnd - TIMELINE_EPSILON),
    );
    if (!piece) continue;

    const pieceEnd = piece.start + piece.duration;
    let effectiveEnd = Math.min(pieceEnd, Math.max(piece.start, absoluteEnd));
    if (pieceEnd - effectiveEnd < MIN_CLIP_DURATION) effectiveEnd = pieceEnd;
    if (
      effectiveEnd <= pieceEnd - MIN_CLIP_DURATION &&
      effectiveEnd >= piece.start + MIN_CLIP_DURATION
    ) {
      const rightId = idFactory();
      state = splitClip(state, piece.id, effectiveEnd, rightId).state;
      pieceIds.add(rightId);
      createdClipIds.push(rightId);
      cutCount += 1;
      piece = state.clips.find((clip) => clip.id === piece!.id)!;
    }

    let effectiveStart = Math.min(effectiveEnd, Math.max(piece.start, absoluteStart));
    if (effectiveStart - piece.start < MIN_CLIP_DURATION) effectiveStart = piece.start;
    if (
      effectiveStart >= piece.start + MIN_CLIP_DURATION &&
      effectiveStart <= piece.start + piece.duration - MIN_CLIP_DURATION
    ) {
      const middleId = idFactory();
      state = splitClip(state, piece.id, effectiveStart, middleId).state;
      pieceIds.add(middleId);
      createdClipIds.push(middleId);
      cutCount += 1;
      piece = state.clips.find((clip) => clip.id === middleId)!;
    }

    const removable = state.clips.find(
      (clip) =>
        pieceIds.has(clip.id) &&
        clip.start >= effectiveStart - TIMELINE_EPSILON &&
        clip.start + clip.duration <= effectiveEnd + TIMELINE_EPSILON,
    );
    if (removable) {
      removedRanges.push({
        start: removable.start - original.start,
        end: removable.start + removable.duration - original.start,
      });
      state = deleteClip(state, removable.id, { ripple: false }).state;
      pieceIds.delete(removable.id);
    }
  }

  const actualRemovedRanges = normalizeRanges(removedRanges, original.duration);
  const removedDuration = actualRemovedRanges.reduce(
    (total, range) => total + range.end - range.start,
    0,
  );
  if (options.ripple && removedDuration > TIMELINE_EPSILON) {
    for (const clip of state.clips) {
      if (clip.trackId !== original.trackId) continue;
      if (pieceIds.has(clip.id)) {
        const originalLocalStart = clip.start - original.start;
        const removedBefore = actualRemovedRanges.reduce((total, range) => {
          if (range.end <= originalLocalStart + TIMELINE_EPSILON) {
            return total + range.end - range.start;
          }
          return total;
        }, 0);
        clip.start = Math.max(original.start, clip.start - removedBefore);
      } else if (clip.start >= original.start + original.duration - TIMELINE_EPSILON) {
        clip.start = Math.max(original.start, clip.start - removedDuration);
      }
    }
  }

  const survivingPieces = state.clips
    .filter((clip) => pieceIds.has(clip.id))
    .sort((left, right) => left.start - right.start);
  state.selectedClipId = survivingPieces[0]?.id ?? null;
  return { state, createdClipIds, removedDuration, cutCount };
}

export interface SplitVisualsAtTimesOptions {
  trackIds?: readonly string[];
  idFactory?: () => string;
  minimumSegmentSeconds?: number;
}

/** Splits unlocked visual clips at marker times without moving media. */
export function splitVisualsAtTimes(
  inputState: TimelineProjectState,
  inputTimes: readonly number[],
  options: SplitVisualsAtTimesOptions = {},
): BatchEditResult {
  let state = cloneProjectState(inputState);
  const allowedTracks = options.trackIds ? new Set(options.trackIds) : null;
  const idFactory = options.idFactory ?? defaultIdFactory;
  const minimum = Math.max(
    MIN_CLIP_DURATION,
    Number(options.minimumSegmentSeconds) || 0.1,
  );
  const times = [...inputTimes]
    .filter(Number.isFinite)
    .sort((left, right) => left - right)
    .filter((time, index, values) => index === 0 || time - values[index - 1] > 0.02);
  const createdClipIds: string[] = [];
  let cutCount = 0;

  for (const time of times) {
    const candidates = state.clips.filter((clip) => {
      if (clip.kind !== "video" && clip.kind !== "image") return false;
      if (allowedTracks && !allowedTracks.has(clip.trackId)) return false;
      const track = state.tracks.find((item) => item.id === clip.trackId);
      if (!track || track.locked) return false;
      return time > clip.start + minimum && time < clip.start + clip.duration - minimum;
    });
    for (const clip of candidates) {
      const newId = idFactory();
      state = splitClip(state, clip.id, time, newId).state;
      createdClipIds.push(newId);
      cutCount += 1;
    }
  }

  return { state, createdClipIds, removedDuration: 0, cutCount };
}
