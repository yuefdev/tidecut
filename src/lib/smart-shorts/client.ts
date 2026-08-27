import { invoke, isTauri } from "@tauri-apps/api/core";

export const FIREWORKS_GLM_MODEL = "accounts/fireworks/models/glm-5p2" as const;

export interface FireworksSmartShortsStatus {
  configured: boolean;
  model: string;
  privacyMode: "transcript-only-store-false";
}

export interface SmartShortsCandidatePrompt {
  candidateId: string;
  startMs: number;
  endMs: number;
  localScore: number;
  transcript: string;
  evidence: string[];
}

export interface SmartShortsGlmRanking {
  candidateId: string;
  score: number;
  reason: string;
  focusTarget: string;
}

export interface RankSmartShortsResponse {
  model: string;
  rankings: SmartShortsGlmRanking[];
}

function requireDesktopRuntime(): void {
  if (!isTauri()) throw new Error("Akıllı Shorts masaüstü uygulamasında çalışır.");
}

export async function checkFireworksSmartShorts(): Promise<FireworksSmartShortsStatus> {
  requireDesktopRuntime();
  return invoke<FireworksSmartShortsStatus>("check_fireworks_smart_shorts");
}

export async function beginSmartShortsAnalysis(operationId: string): Promise<void> {
  requireDesktopRuntime();
  await invoke("begin_smart_shorts_analysis", { operationId });
}

export async function cancelSmartShortsAnalysis(operationId: string): Promise<void> {
  requireDesktopRuntime();
  await invoke("cancel_smart_shorts_analysis", { operationId });
}

export async function finishSmartShortsAnalysis(operationId: string): Promise<void> {
  requireDesktopRuntime();
  await invoke("finish_smart_shorts_analysis", { operationId });
}

export async function rankSmartShortCandidates(
  candidates: readonly SmartShortsCandidatePrompt[],
  apiKey?: string,
): Promise<RankSmartShortsResponse> {
  requireDesktopRuntime();
  return invoke<RankSmartShortsResponse>("rank_smart_short_candidates", {
    request: {
      apiKey: apiKey?.trim() || null,
      candidates,
    },
  });
}
