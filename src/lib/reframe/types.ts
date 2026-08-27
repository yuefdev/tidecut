import type { RenderErrorReport } from "../render/types";

export interface NormalizedBoundingBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface AutoReframeAnalysisRequest {
  sourcePath: string;
  /** Trim start in source-media time. */
  startMs: number;
  durationMs: number;
  /** Selection time relative to startMs, matching returned sample timeMs values. */
  seedTimeMs: number;
  selection: NormalizedBoundingBox;
  sampleFps?: number;
  analysisMaxDimension?: number;
  force?: boolean;
}

export interface AutoReframeTrackSample {
  /** Time relative to the analyzed clip range, not an absolute source timestamp. */
  timeMs: number;
  bbox: NormalizedBoundingBox;
  /** Binary CSRT update signal (0 or 1), not a calibrated probability. */
  confidence: number;
}

export interface AutoReframeAnalysis {
  engine: "opencv-csrt" | string;
  engineVersion: string;
  confidenceMetric: "binary-update-signal" | string;
  sourceWidth: number;
  sourceHeight: number;
  sourceFps: number | null;
  startMs: number;
  durationMs: number;
  seedTimeMs: number;
  sampleFps: number;
  analysisMaxDimension: number;
  selection: NormalizedBoundingBox;
  samples: AutoReframeTrackSample[];
  warnings: string[];
  cacheHit: boolean;
}

export interface FacecamDetectionRequest {
  sourcePath: string;
  /** Trim start in source-media time. */
  startMs: number;
  durationMs: number;
  /** Scan spacing; the backend derives a sensible default from duration. */
  intervalMs?: number;
  analysisMaxDimension?: number;
  force?: boolean;
  operationId?: string;
}

export interface FacecamDetection {
  engine: "opencv-haar-facecam" | string;
  engineVersion: string;
  confidenceMetric: "haar-cluster-stability" | string;
  sourceWidth: number;
  sourceHeight: number;
  sourceFps: number | null;
  startMs: number;
  durationMs: number;
  intervalMs: number;
  analysisMaxDimension: number;
  samples: AutoReframeTrackSample[];
  warnings: string[];
  cacheHit: boolean;
}

export interface AutoReframeRuntimePackage {
  name: string;
  version: string;
  declaredLicense: string;
  freeAndOpenSource: boolean;
}

export interface AutoReframeRuntimeStatus {
  ready: boolean;
  engine: "opencv-csrt" | string;
  runtimeDir: string;
  pythonPath: string | null;
  pythonVersion: string | null;
  opencvVersion: string | null;
  numpyVersion: string | null;
  packages: AutoReframeRuntimePackage[];
  uvAvailable: boolean;
  diagnostic: RenderErrorReport | null;
}

export interface AutoReframeProgress {
  operation: "setup" | "analysis" | string;
  stage: string;
  progressPercent: number | null;
  message: string;
}

