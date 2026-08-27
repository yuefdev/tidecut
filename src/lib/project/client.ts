import { invoke, isTauri } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  LoadProjectResult,
  MediaInspection,
  MediaPathStatus,
  MediaReference,
  ProjectCommandErrorShape,
  RecoverySnapshotInfo,
  RecoverySnapshotRequest,
  RelinkMediaRequest,
  RelinkMediaResult,
  SaveProjectRequest,
  SaveProjectResult,
} from "./types";

export const PROJECT_EXTENSION = "astral";

export class ProjectClientError extends Error {
  readonly code: string;
  readonly path?: string;

  constructor(error: ProjectCommandErrorShape, options?: ErrorOptions) {
    super(error.message, options);
    this.name = "ProjectClientError";
    this.code = error.code;
    this.path = error.path;
  }
}

export async function saveProject<TDocument extends object>(
  request: SaveProjectRequest<TDocument>,
): Promise<SaveProjectResult> {
  return call("save_project", {
    request: {
      ...request,
      createBackup: request.createBackup ?? true,
    },
  });
}

export async function loadProject<TDocument extends object>(
  path: string,
): Promise<LoadProjectResult<TDocument>> {
  return call("load_project", { path });
}

export async function writeRecoverySnapshot<TDocument extends object>(
  request: RecoverySnapshotRequest<TDocument>,
): Promise<RecoverySnapshotInfo> {
  return call("write_recovery_snapshot", { request });
}

export async function listRecoverySnapshots(): Promise<
  RecoverySnapshotInfo[]
> {
  return call("list_recovery_snapshots");
}

export async function loadRecoverySnapshot<TDocument extends object>(
  snapshotId: string,
): Promise<LoadProjectResult<TDocument>> {
  return call("load_recovery_snapshot", { snapshotId });
}

export async function discardRecoverySnapshot(
  snapshotId: string,
): Promise<void> {
  return call("discard_recovery_snapshot", { snapshotId });
}

export async function inspectMedia(
  paths: readonly string[],
): Promise<MediaInspection[]> {
  return call("inspect_media", { paths: [...paths] });
}

export async function findMissingMedia(
  references: readonly MediaReference[],
): Promise<MediaPathStatus[]> {
  return call("find_missing_media", { references: [...references] });
}

export async function relinkMedia(
  request: RelinkMediaRequest,
): Promise<RelinkMediaResult> {
  return call("relink_media", { request });
}

export async function chooseProjectToOpen(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: "Astral Lunar Project", extensions: [PROJECT_EXTENSION] },
      { name: "Legacy JSON", extensions: ["json"] },
    ],
  });
  return typeof selected === "string" ? selected : null;
}

export async function chooseProjectSavePath(
  defaultPath?: string,
): Promise<string | null> {
  return save({
    defaultPath,
    filters: [
      { name: "Astral Lunar Project", extensions: [PROJECT_EXTENSION] },
    ],
  });
}

export async function chooseMediaPaths(): Promise<string[]> {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [
      {
        name: "Supported Media",
        extensions: [
          "mp4",
          "mov",
          "m4v",
          "mkv",
          "avi",
          "webm",
          "mxf",
          "ts",
          "mts",
          "m2ts",
          "wmv",
          "flv",
          "ogv",
          "wav",
          "mp3",
          "m4a",
          "aac",
          "flac",
          "ogg",
          "opus",
          "aiff",
          "aif",
          "wma",
          "png",
          "jpg",
          "jpeg",
          "webp",
          "gif",
          "bmp",
          "tif",
          "tiff",
          "avif",
          "heic",
          "heif",
          "svg",
        ],
      },
    ],
  });
  if (!selected) return [];
  return Array.isArray(selected) ? selected : [selected];
}

export async function chooseRelinkRoots(): Promise<string[]> {
  const selected = await open({
    multiple: true,
    directory: true,
    title: "Choose folders to search for missing media",
  });
  if (!selected) return [];
  return Array.isArray(selected) ? selected : [selected];
}

async function call<TResult>(
  command: string,
  args?: Record<string, unknown>,
): Promise<TResult> {
  if (!isTauri()) {
    throw new ProjectClientError({
      code: "desktop_runtime_required",
      message: `${command} requires the Tauri desktop runtime`,
    });
  }

  try {
    return await invoke<TResult>(command, args);
  } catch (error) {
    throw normalizeCommandError(error);
  }
}

function normalizeCommandError(error: unknown): ProjectClientError {
  if (error instanceof ProjectClientError) return error;
  if (isCommandErrorShape(error)) return new ProjectClientError(error);
  if (error instanceof Error) {
    return new ProjectClientError(
      { code: "command_failed", message: error.message },
      { cause: error },
    );
  }
  return new ProjectClientError({
    code: "command_failed",
    message: typeof error === "string" ? error : "Desktop command failed",
  });
}

function isCommandErrorShape(value: unknown): value is ProjectCommandErrorShape {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" && typeof candidate.message === "string"
  );
}
