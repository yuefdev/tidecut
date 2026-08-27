import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RenderErrorReport } from "../render/types";
import type {
  AutoReframeAnalysis,
  AutoReframeAnalysisRequest,
  AutoReframeProgress,
  AutoReframeRuntimeStatus,
  FacecamDetection,
  FacecamDetectionRequest,
} from "./types";

export const AUTO_REFRAME_PROGRESS_EVENT = "auto-reframe://progress";

function desktopRuntimeError(): RenderErrorReport {
  return {
    code: "desktop_runtime_required",
    userMessage: "Yerel nesne takibi yalnızca masaüstü uygulamasında kullanılabilir.",
    technicalMessage: "Tauri IPC is unavailable",
    retryable: false,
    stderrTail: [],
  };
}

function requireDesktopRuntime(): void {
  if (!isTauri()) throw desktopRuntimeError();
}

export function autoReframeRuntimeAvailable(): boolean {
  return isTauri();
}

export async function checkAutoReframeRuntime(): Promise<AutoReframeRuntimeStatus> {
  if (!isTauri()) {
    return {
      ready: false,
      engine: "opencv-csrt",
      runtimeDir: "",
      pythonPath: null,
      pythonVersion: null,
      opencvVersion: null,
      numpyVersion: null,
      packages: [],
      uvAvailable: false,
      diagnostic: desktopRuntimeError(),
    };
  }
  return invoke<AutoReframeRuntimeStatus>("check_auto_reframe_runtime");
}

export async function setupAutoReframeRuntime(
  force = false,
): Promise<AutoReframeRuntimeStatus> {
  requireDesktopRuntime();
  return invoke<AutoReframeRuntimeStatus>("setup_auto_reframe_runtime", {
    force,
  });
}

export async function analyzeAutoReframe(
  request: AutoReframeAnalysisRequest,
): Promise<AutoReframeAnalysis> {
  requireDesktopRuntime();
  return invoke<AutoReframeAnalysis>("analyze_auto_reframe", { request });
}

export async function detectFacecam(
  request: FacecamDetectionRequest,
): Promise<FacecamDetection> {
  requireDesktopRuntime();
  return invoke<FacecamDetection>("detect_facecam", { request });
}

export async function onAutoReframeProgress(
  callback: (progress: AutoReframeProgress) => void,
): Promise<UnlistenFn> {
  requireDesktopRuntime();
  return listen<AutoReframeProgress>(AUTO_REFRAME_PROGRESS_EVENT, ({ payload }) => {
    callback(payload);
  });
}
