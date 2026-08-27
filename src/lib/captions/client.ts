import { invoke, isTauri } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export type CaptionFileFormat = "srt" | "vtt" | "ass";

export interface CaptionFilePayload {
  path: string;
  fileName: string;
  extension: CaptionFileFormat | "ssa";
  content: string;
}

export interface CaptionWriteResult {
  path: string;
  bytesWritten: number;
}

interface CaptionCommandErrorShape {
  code?: string;
  message?: string;
  path?: string;
}

export class CaptionClientError extends Error {
  readonly code: string;
  readonly path?: string;

  constructor(error: CaptionCommandErrorShape) {
    super(error.message || "Altyazı dosyası işlenemedi.");
    this.name = "CaptionClientError";
    this.code = error.code || "caption_command_failed";
    this.path = error.path;
  }
}

export async function chooseCaptionToOpen(): Promise<CaptionFilePayload | null> {
  ensureDesktopRuntime();
  const selected = await open({
    multiple: false,
    directory: false,
    title: "Altyazı içe aktar",
    filters: [
      { name: "Altyazı", extensions: ["srt", "vtt", "ass", "ssa"] },
    ],
  });
  if (typeof selected !== "string") return null;
  return call<CaptionFilePayload>("read_caption_file", { path: selected });
}

export async function saveCaptionText(
  content: string,
  format: CaptionFileFormat,
  defaultName = `altyazi.${format}`,
): Promise<CaptionWriteResult | null> {
  ensureDesktopRuntime();
  const path = await save({
    defaultPath: defaultName,
    title: "Altyazıyı dışa aktar",
    filters: [{ name: formatLabel(format), extensions: [format] }],
  });
  if (!path) return null;
  return call<CaptionWriteResult>("write_caption_file", { path, content });
}

function ensureDesktopRuntime() {
  if (!isTauri()) {
    throw new CaptionClientError({
      code: "desktop_runtime_required",
      message: "Bu dosya işlemi Astral masaüstü uygulamasında kullanılabilir.",
    });
  }
}

async function call<TResult>(
  command: string,
  args: Record<string, unknown>,
): Promise<TResult> {
  try {
    return await invoke<TResult>(command, args);
  } catch (error) {
    if (error && typeof error === "object") {
      throw new CaptionClientError(error as CaptionCommandErrorShape);
    }
    throw new CaptionClientError({ message: String(error) });
  }
}

function formatLabel(format: CaptionFileFormat): string {
  if (format === "srt") return "SubRip (SRT)";
  if (format === "vtt") return "WebVTT";
  return "Advanced SubStation Alpha (ASS)";
}
