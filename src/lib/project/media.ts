import {
  chooseMediaPaths,
  findMissingMedia,
  inspectMedia,
  relinkMedia,
} from "./client";
import type {
  MediaAsset,
  MediaInspection,
  MediaReference,
  ProjectDocument,
  RelinkMediaRequest,
  RelinkMediaResult,
  RelinkResolution,
} from "./types";

export interface ImportMediaResult {
  assets: MediaAsset[];
  rejected: MediaInspection[];
}

export async function importMediaFromDialog(
  existingAssets: readonly MediaAsset[] = [],
): Promise<ImportMediaResult> {
  const paths = await chooseMediaPaths();
  if (paths.length === 0) return { assets: [], rejected: [] };
  return importMediaPaths(paths, existingAssets);
}

export async function importMediaPaths(
  paths: readonly string[],
  existingAssets: readonly MediaAsset[] = [],
): Promise<ImportMediaResult> {
  const inspections = await inspectMedia(paths);
  // Every distinct source path stays in the project inventory. Content-level
  // dedupe would orphan timeline aliases that point at a byte-identical copy.
  const existingKeys = new Set(
    existingAssets.map((asset) => normalizedPath(asset.sourcePath)),
  );
  const assets: MediaAsset[] = [];
  const rejected: MediaInspection[] = [];

  for (const inspection of inspections) {
    if (
      !inspection.exists ||
      !inspection.supported ||
      !inspection.kind ||
      inspection.sizeBytes === undefined ||
      !inspection.quickHash
    ) {
      rejected.push(inspection);
      continue;
    }

    const pathKey = normalizedPath(inspection.path);
    if (existingKeys.has(pathKey)) continue;
    existingKeys.add(pathKey);
    assets.push({
      id: createAssetId(),
      sourcePath: inspection.path,
      fileName: inspection.fileName,
      kind: inspection.kind,
      sizeBytes: inspection.sizeBytes,
      modifiedAtMs: inspection.modifiedAtMs,
      quickHash: inspection.quickHash,
      availability: "online",
    });
  }

  return { assets, rejected };
}

export async function auditMissingMedia(
  assets: readonly MediaAsset[],
): Promise<MediaAsset[]> {
  const references = assets.map(toMediaReference);
  const statuses = await findMissingMedia(references);
  const missingIds = new Set(
    statuses
      .filter((status) => !status.exists || !status.isFile)
      .map((status) => status.assetId),
  );
  return assets.map((asset) => ({
    ...asset,
    availability: missingIds.has(asset.id) ? "missing" : "online",
  }));
}

export async function searchForMissingMedia(
  assets: readonly MediaAsset[],
  searchRoots: readonly string[],
  maxDepth = 8,
): Promise<RelinkMediaResult> {
  const request: RelinkMediaRequest = {
    missing: assets
      .filter((asset) => asset.availability === "missing")
      .map(toMediaReference),
    searchRoots: [...searchRoots],
    maxDepth,
  };
  return relinkMedia(request);
}

export function applyRelinkResolutions<TTimeline, TSettings>(
  document: ProjectDocument<TTimeline, TSettings>,
  resolutions: readonly RelinkResolution[],
): ProjectDocument<TTimeline, TSettings> {
  if (resolutions.length === 0) return document;
  const replacements = new Map(
    resolutions.map((resolution) => [
      normalizedPath(resolution.reference.path),
      resolution.replacementPath,
    ]),
  );
  const resolvedAssetIds = new Set(
    resolutions
      .map((resolution) => resolution.reference.assetId)
      .filter((value): value is string => Boolean(value)),
  );
  const replacePath = (path: string): string =>
    replacements.get(normalizedPath(path)) ?? path;

  return {
    ...document,
    updatedAtMs: Date.now(),
    media: document.media.map((asset) => ({
      ...asset,
      sourcePath: replacePath(asset.sourcePath),
      availability: resolvedAssetIds.has(asset.id)
        ? ("online" as const)
        : asset.availability,
    })),
    timeline: replacePathsDeep(document.timeline, replacePath),
  };
}

function toMediaReference(asset: MediaAsset): MediaReference {
  return {
    assetId: asset.id,
    path: asset.sourcePath,
    sizeBytes: asset.sizeBytes,
    quickHash: asset.quickHash,
  };
}

function replacePathsDeep<T>(value: T, replace: (path: string) => string): T {
  if (typeof value === "string") return replace(value) as T;
  if (Array.isArray(value)) {
    return value.map((item) => replacePathsDeep(item, replace)) as T;
  }
  if (!value || typeof value !== "object") return value;

  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [
      key,
      replacePathsDeep(item, replace),
    ]),
  ) as T;
}

function normalizedPath(path: string): string {
  const normalized = path.replaceAll("\\", "/");
  const windowsLike = /^[a-z]:\//i.test(normalized) || normalized.startsWith("//");
  return windowsLike ? normalized.toLowerCase() : normalized;
}

function createAssetId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `media-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}
