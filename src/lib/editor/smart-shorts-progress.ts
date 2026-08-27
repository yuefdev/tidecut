export type SmartShortsProgressStage =
  | "speech"
  | "analysis"
  | "planning"
  | "ranking"
  | "finalizing";

export interface SmartShortsTimingProfile {
  version: 1;
  ratios: Partial<Record<SmartShortsProgressStage, number>>;
}

export interface SmartShortsProgressSnapshot {
  percent: number;
  elapsedMs: number;
  remainingMs: number;
  overdue: boolean;
  stage: SmartShortsProgressStage;
  stageLabel: string;
}

interface StageDefinition {
  floor: number;
  ceiling: number;
  label: string;
}

const STAGES: Record<SmartShortsProgressStage, StageDefinition> = {
  speech: { floor: 3, ceiling: 28, label: "Konuşma sınırları çıkarılıyor" },
  analysis: { floor: 28, ceiling: 84, label: "Güçlü anlar ve kahkahalar taranıyor" },
  planning: { floor: 84, ceiling: 92, label: "Shorts adayları hazırlanıyor" },
  ranking: { floor: 92, ceiling: 98, label: "GLM adayları sıralıyor" },
  finalizing: { floor: 98, ceiling: 99.4, label: "Sonuçlar kaydediliyor" },
};

const PROFILE_RATIO_MIN = 0.35;
const PROFILE_RATIO_MAX = 4;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function finiteDuration(value: number): number {
  return Number.isFinite(value) && value > 0 ? value : 30_000;
}

export function createSmartShortsTimingProfile(): SmartShortsTimingProfile {
  return { version: 1, ratios: {} };
}

export function parseSmartShortsTimingProfile(
  raw: string | null | undefined,
): SmartShortsTimingProfile {
  if (!raw) return createSmartShortsTimingProfile();
  try {
    const parsed = JSON.parse(raw) as Partial<SmartShortsTimingProfile>;
    if (parsed.version !== 1 || !parsed.ratios || typeof parsed.ratios !== "object") {
      return createSmartShortsTimingProfile();
    }
    const ratios: SmartShortsTimingProfile["ratios"] = {};
    for (const stage of Object.keys(STAGES) as SmartShortsProgressStage[]) {
      const ratio = parsed.ratios[stage];
      if (typeof ratio === "number" && Number.isFinite(ratio)) {
        ratios[stage] = clamp(ratio, PROFILE_RATIO_MIN, PROFILE_RATIO_MAX);
      }
    }
    return { version: 1, ratios };
  } catch {
    return createSmartShortsTimingProfile();
  }
}

export function baselineSmartShortsStageMs(
  stage: SmartShortsProgressStage,
  sourceDurationMs: number,
): number {
  const durationMs = finiteDuration(sourceDurationMs);
  switch (stage) {
    case "speech":
      return clamp(6_000 + durationMs * 0.12, 9_000, 15 * 60_000);
    case "analysis":
      return clamp(14_000 + durationMs * 0.55, 22_000, 60 * 60_000);
    case "planning":
      return 2_500;
    case "ranking":
      return 12_000;
    case "finalizing":
      return 1_800;
  }
}

export function expectedSmartShortsStageMs(
  stage: SmartShortsProgressStage,
  sourceDurationMs: number,
  profile: SmartShortsTimingProfile,
): number {
  const ratio = profile.ratios[stage] ?? 1;
  return baselineSmartShortsStageMs(stage, sourceDurationMs) * ratio;
}

function smoothStageFraction(elapsedMs: number, expectedMs: number): number {
  const normalized = Math.max(0, elapsedMs) / Math.max(1, expectedMs);
  if (normalized <= 1) return normalized * 0.88;
  return Math.min(0.998, 0.88 + 0.118 * (1 - Math.exp(-(normalized - 1) * 0.72)));
}

function stageSequence(includeRanking: boolean): SmartShortsProgressStage[] {
  return [
    "speech",
    "analysis",
    "planning",
    ...(includeRanking ? (["ranking"] as const) : []),
    "finalizing",
  ];
}

export class SmartShortsProgressEstimator {
  private readonly runStartedAtMs: number;
  private readonly sequence: SmartShortsProgressStage[];
  private profile: SmartShortsTimingProfile;
  private stage: SmartShortsProgressStage = "speech";
  private stageStartedAtMs: number;
  private stageFloor = STAGES.speech.floor;
  private percent = STAGES.speech.floor;

  constructor(
    private readonly sourceDurationMs: number,
    includeRanking: boolean,
    profile: SmartShortsTimingProfile,
    nowMs: number,
  ) {
    this.profile = parseSmartShortsTimingProfile(JSON.stringify(profile));
    this.sequence = stageSequence(includeRanking);
    this.runStartedAtMs = nowMs;
    this.stageStartedAtMs = nowMs;
  }

  begin(stage: SmartShortsProgressStage, nowMs: number): void {
    if (stage === this.stage) return;
    this.learnCompletedStage(nowMs);
    this.stage = stage;
    this.stageStartedAtMs = nowMs;
    this.percent = Math.max(this.percent, STAGES[stage].floor);
    this.stageFloor = this.percent;
  }

  observe(percent: number): void {
    if (!Number.isFinite(percent)) return;
    this.percent = Math.max(this.percent, clamp(percent, 0, 99.4));
  }

  snapshot(nowMs: number): SmartShortsProgressSnapshot {
    const definition = STAGES[this.stage];
    const stageElapsedMs = Math.max(0, nowMs - this.stageStartedAtMs);
    const expectedMs = expectedSmartShortsStageMs(
      this.stage,
      this.sourceDurationMs,
      this.profile,
    );
    const estimated =
      this.stageFloor +
      (definition.ceiling - this.stageFloor) *
        smoothStageFraction(stageElapsedMs, expectedMs);
    this.percent = Math.max(
      this.percent,
      Math.min(definition.ceiling - 0.01, estimated),
    );

    const currentIndex = this.sequence.indexOf(this.stage);
    const remainingStages = this.sequence.slice(Math.max(0, currentIndex + 1));
    const laterMs = remainingStages.reduce(
      (total, stage) =>
        total + expectedSmartShortsStageMs(stage, this.sourceDurationMs, this.profile),
      0,
    );

    return {
      percent: this.percent,
      elapsedMs: Math.max(0, nowMs - this.runStartedAtMs),
      remainingMs: Math.max(0, expectedMs - stageElapsedMs) + laterMs,
      overdue: stageElapsedMs > expectedMs,
      stage: this.stage,
      stageLabel: definition.label,
    };
  }

  complete(nowMs: number): SmartShortsTimingProfile {
    this.learnCompletedStage(nowMs);
    this.percent = 100;
    return this.timingProfile();
  }

  timingProfile(): SmartShortsTimingProfile {
    return {
      version: 1,
      ratios: { ...this.profile.ratios },
    };
  }

  private learnCompletedStage(nowMs: number): void {
    const elapsedMs = Math.max(250, nowMs - this.stageStartedAtMs);
    const baselineMs = baselineSmartShortsStageMs(this.stage, this.sourceDurationMs);
    const observedRatio = clamp(elapsedMs / baselineMs, PROFILE_RATIO_MIN, PROFILE_RATIO_MAX);
    const previousRatio = this.profile.ratios[this.stage] ?? 1;
    this.profile = {
      version: 1,
      ratios: {
        ...this.profile.ratios,
        [this.stage]: clamp(
          previousRatio * 0.72 + observedRatio * 0.28,
          PROFILE_RATIO_MIN,
          PROFILE_RATIO_MAX,
        ),
      },
    };
  }
}

