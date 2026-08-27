export const PROJECT_SCHEMA_VERSION = 1 as const;

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

export type MediaKind = "video" | "image" | "audio";

export interface MediaAsset {
  id: string;
  sourcePath: string;
  fileName: string;
  kind: MediaKind;
  sizeBytes: number;
  modifiedAtMs?: number;
  quickHash: string;
  availability: "online" | "missing";
}

/**
 * Timeline and editor settings intentionally remain generic. Feature modules can
 * version their own payload without coupling disk persistence to UI internals.
 */
export interface ProjectDocument<
  TTimeline = unknown,
  TSettings = unknown,
> {
  schemaVersion: typeof PROJECT_SCHEMA_VERSION;
  projectId: string;
  name: string;
  createdAtMs: number;
  updatedAtMs: number;
  media: MediaAsset[];
  timeline: TTimeline;
  settings?: TSettings;
}

export interface SaveProjectRequest<TDocument extends object = ProjectDocument> {
  path: string;
  document: TDocument;
  projectId?: string;
  expectedChecksum?: string;
  createBackup?: boolean;
}

export interface SaveProjectResult {
  path: string;
  checksum: string;
  bytesWritten: number;
  savedAtMs: number;
  backupPath?: string;
}

export interface LoadProjectResult<
  TDocument extends object = ProjectDocument,
> {
  path: string;
  document: TDocument;
  checksum: string;
  savedAtMs: number;
  verified: boolean;
  recoveredFromBackup: boolean;
  originalPath?: string;
  baseChecksum?: string;
  recoverySnapshotId?: string;
}

export interface RecoverySnapshotRequest<
  TDocument extends object = ProjectDocument,
> {
  projectId: string;
  document: TDocument;
  originalPath?: string;
  baseChecksum?: string;
  recoveredFromBackup?: boolean;
  reason?: string;
}

export interface RecoverySnapshotInfo {
  snapshotId: string;
  projectId: string;
  checksum: string;
  updatedAtMs: number;
  bytesWritten: number;
  originalPath?: string;
  reason?: string;
  isValid: boolean;
  error?: string;
}

export interface MediaInspection {
  path: string;
  fileName: string;
  extension?: string;
  kind?: MediaKind;
  exists: boolean;
  supported: boolean;
  sizeBytes?: number;
  modifiedAtMs?: number;
  quickHash?: string;
  error?: string;
}

export interface MediaReference {
  assetId?: string;
  path: string;
  sizeBytes?: number;
  quickHash?: string;
}

export interface MediaPathStatus {
  assetId?: string;
  path: string;
  exists: boolean;
  isFile: boolean;
  sizeBytes?: number;
  modifiedAtMs?: number;
  reason?: string;
}

export interface RelinkMediaRequest {
  missing: MediaReference[];
  searchRoots: string[];
  maxDepth?: number;
}

export interface RelinkCandidate {
  path: string;
  sizeBytes: number;
  quickHash?: string;
  confidence: number;
  reason: string;
}

export interface RelinkResolution {
  reference: MediaReference;
  replacementPath: string;
  confidence: number;
  reason: string;
}

export interface AmbiguousRelink {
  reference: MediaReference;
  candidates: RelinkCandidate[];
}

export interface RelinkMediaResult {
  resolved: RelinkResolution[];
  ambiguous: AmbiguousRelink[];
  unresolved: MediaReference[];
  scannedFiles: number;
  truncated: boolean;
}

export interface ProjectCommandErrorShape {
  code: string;
  message: string;
  path?: string;
}

export function createProjectDocument<TTimeline = unknown>(options?: {
  name?: string;
  timeline?: TTimeline;
  now?: number;
  projectId?: string;
}): ProjectDocument<TTimeline> {
  const now = options?.now ?? Date.now();
  return {
    schemaVersion: PROJECT_SCHEMA_VERSION,
    projectId: options?.projectId ?? createProjectId(),
    name: options?.name?.trim() || "Untitled Project",
    createdAtMs: now,
    updatedAtMs: now,
    media: [],
    timeline:
      options && "timeline" in options
        ? (options.timeline as TTimeline)
        : (null as TTimeline),
  };
}

export function isProjectDocument(value: unknown): value is ProjectDocument {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const candidate = value as Record<string, unknown>;
  return (
    candidate.schemaVersion === PROJECT_SCHEMA_VERSION &&
    typeof candidate.projectId === "string" &&
    candidate.projectId.length > 0 &&
    typeof candidate.name === "string" &&
    Number.isFinite(candidate.createdAtMs) &&
    Number.isFinite(candidate.updatedAtMs) &&
    Array.isArray(candidate.media) &&
    candidate.media.every(isMediaAsset) &&
    "timeline" in candidate
  );
}

export function isMediaAsset(value: unknown): value is MediaAsset {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.id === "string" &&
    candidate.id.length > 0 &&
    typeof candidate.sourcePath === "string" &&
    candidate.sourcePath.length > 0 &&
    typeof candidate.fileName === "string" &&
    (candidate.kind === "video" ||
      candidate.kind === "image" ||
      candidate.kind === "audio") &&
    typeof candidate.sizeBytes === "number" &&
    Number.isFinite(candidate.sizeBytes) &&
    candidate.sizeBytes >= 0 &&
    typeof candidate.quickHash === "string" &&
    (candidate.availability === "online" ||
      candidate.availability === "missing") &&
    (candidate.modifiedAtMs === undefined ||
      (typeof candidate.modifiedAtMs === "number" &&
        Number.isFinite(candidate.modifiedAtMs)))
  );
}

function createProjectId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `project-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}
