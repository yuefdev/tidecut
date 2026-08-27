import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AudioCleanupAsset,
  AudioCleanupRequest,
  AudioEnhancementProgress,
  MossFormerTestCleanupAsset,
  MossFormerTestCleanupRequest,
  AudioExportRequest,
  AudioLoudnessAnalysisRequest,
  AudioLoudnessAnalysisResult,
  AudioEnergyAnalysisRequest,
  AudioEnergyAnalysisResult,
  LaughterAnalysisRequest,
  LaughterAnalysisResult,
  LaughterAnalysisProgress,
  ProxyRequest,
  ProxyCacheStats,
  ProxyPruneResult,
  FfmpegSetupProgress,
  RenderAccepted,
  RenderCapabilities,
  RenderErrorReport,
  RenderJobSnapshot,
  RenderRequest,
  VoiceRiderAnalysisRequest,
  VoiceRiderAnalysisResult,
  SpeechActivityAnalysisRequest,
  SpeechActivityAnalysisResult,
  SpeechTranscriptAnalysisRequest,
  SpeechTranscriptAnalysisResult,
  SpeechSetupProgress,
  BeatAnalysisRequest,
  BeatAnalysisResult,
} from "./types";

const RENDER_EVENTS = [
  "render://progress",
  "render://completed",
  "render://cancelled",
  "render://failed",
] as const;

function desktopRuntimeError(): RenderErrorReport {
  return {
    code: "desktop_runtime_required",
    userMessage: "Bu işlem yalnızca masaüstü uygulamasında kullanılabilir.",
    technicalMessage: "Tauri IPC is unavailable",
    retryable: false,
    stderrTail: [],
  };
}

function requireDesktopRuntime() {
  if (!isTauri()) throw desktopRuntimeError();
}

export function desktopRuntimeAvailable(): boolean {
  return isTauri();
}

export async function getRenderCapabilities(
  ffmpegPath?: string,
): Promise<RenderCapabilities> {
  if (!isTauri()) {
    return {
      available: false,
      ffmpegPath: "",
      diagnostic: desktopRuntimeError(),
    };
  }
  return invoke<RenderCapabilities>("get_render_capabilities", { ffmpegPath });
}

export async function startRender(
  request: RenderRequest,
): Promise<RenderJobSnapshot> {
  requireDesktopRuntime();
  const accepted = await invoke<RenderAccepted>("start_render", {
    request: await materializeTimelineCleanup(request),
  });
  return accepted.job;
}

export async function startAudioExport(
  request: AudioExportRequest,
): Promise<RenderJobSnapshot> {
  requireDesktopRuntime();
  const accepted = await invoke<RenderAccepted>("start_audio_export", {
    request: await materializeStandaloneCleanup(request),
  });
  return accepted.job;
}

export async function analyzeAudioLoudness(
  request: AudioLoudnessAnalysisRequest,
): Promise<AudioLoudnessAnalysisResult> {
  requireDesktopRuntime();
  return invoke<AudioLoudnessAnalysisResult>("analyze_audio_loudness", {
    request: await materializeStandaloneCleanup(request),
  });
}

export async function analyzeAudioEnergy(
  request: AudioEnergyAnalysisRequest,
): Promise<AudioEnergyAnalysisResult> {
  requireDesktopRuntime();
  return invoke<AudioEnergyAnalysisResult>("analyze_audio_energy", { request });
}

export async function analyzeLaughter(
  request: LaughterAnalysisRequest,
): Promise<LaughterAnalysisResult> {
  requireDesktopRuntime();
  return invoke<LaughterAnalysisResult>("analyze_laughter", { request });
}

export async function onLaughterAnalysisProgress(
  callback: (progress: LaughterAnalysisProgress) => void,
): Promise<UnlistenFn> {
  requireDesktopRuntime();
  return listen<LaughterAnalysisProgress>("laughter-analysis://progress", (event) => {
    callback(event.payload);
  });
}

export async function analyzeVoiceRider(
  request: VoiceRiderAnalysisRequest,
): Promise<VoiceRiderAnalysisResult> {
  requireDesktopRuntime();
  return invoke<VoiceRiderAnalysisResult>("analyze_voice_rider", {
    request: await materializeStandaloneCleanup(request),
  });
}

export async function analyzeSpeechActivity(
  request: SpeechActivityAnalysisRequest,
): Promise<SpeechActivityAnalysisResult> {
  requireDesktopRuntime();
  return invoke<SpeechActivityAnalysisResult>("analyze_speech_activity", { request });
}

export async function analyzeSpeechTranscript(
  request: SpeechTranscriptAnalysisRequest,
): Promise<SpeechTranscriptAnalysisResult> {
  requireDesktopRuntime();
  return invoke<SpeechTranscriptAnalysisResult>("analyze_speech_transcript", {
    request,
  });
}

export async function analyzeBeats(
  request: BeatAnalysisRequest,
): Promise<BeatAnalysisResult> {
  requireDesktopRuntime();
  return invoke<BeatAnalysisResult>("analyze_beats", { request });
}

export async function onSpeechSetupProgress(
  callback: (progress: SpeechSetupProgress) => void,
): Promise<UnlistenFn> {
  requireDesktopRuntime();
  return listen<SpeechSetupProgress>("speech-analysis://setup-progress", (event) => {
    callback(event.payload);
  });
}

export async function prepareAudioCleanup(
  request: AudioCleanupRequest,
): Promise<AudioCleanupAsset> {
  requireDesktopRuntime();
  return invoke<AudioCleanupAsset>("prepare_audio_cleanup", { request });
}

export async function prepareMossFormerTestCleanup(
  request: MossFormerTestCleanupRequest,
): Promise<MossFormerTestCleanupAsset> {
  requireDesktopRuntime();
  return invoke<MossFormerTestCleanupAsset>("prepare_mossformer_test_cleanup", {
    request,
  });
}

export async function onAudioEnhancementProgress(
  callback: (progress: AudioEnhancementProgress) => void,
): Promise<UnlistenFn> {
  requireDesktopRuntime();
  return listen<AudioEnhancementProgress>("audio-enhancement://progress", (event) => {
    callback(event.payload);
  });
}

async function materializeStandaloneCleanup<
  T extends AudioExportRequest | AudioLoudnessAnalysisRequest | VoiceRiderAnalysisRequest,
>(request: T): Promise<T> {
  if (!request.noiseReduction) return request;
  const asset = await prepareAudioCleanup({
    sourcePath: request.sourcePath,
    startMs: request.startMs,
    durationMs: request.durationMs,
    settings: request.noiseReduction,
    ffmpegPath: request.ffmpegPath,
  });
  return {
    ...request,
    sourcePath: asset.outputPath,
    startMs: 0,
    durationMs: asset.durationMs,
    noiseReduction: null,
  };
}

async function materializeTimelineCleanup(
  request: RenderRequest,
): Promise<RenderRequest> {
  // Rust defaults an omitted includeAudio field to true, so mirror that wire
  // contract here instead of accidentally skipping neural cleanup.
  if (request.includeAudio === false || !request.timeline) return request;
  const clips = [];
  for (const clip of request.timeline.clips) {
    if (!clip.noiseReduction || !clip.path || clip.kind === "image" || clip.kind === "text") {
      clips.push(clip);
      continue;
    }
    const speed = Number.isFinite(clip.speed) && (clip.speed ?? 0) > 0 ? clip.speed! : 1;
    const sourceDurationMs = Math.max(1, Math.round(clip.durationMs * speed));
    const asset = await prepareAudioCleanup({
      sourcePath: clip.path,
      startMs: Math.max(0, Math.round(clip.trimInMs ?? 0)),
      durationMs: sourceDurationMs,
      settings: clip.noiseReduction,
      ffmpegPath: request.ffmpegPath,
    });
    clips.push({
      ...clip,
      audioPath: asset.outputPath,
      audioTrimInMs: 0,
    });
  }
  return { ...request, timeline: { ...request.timeline, clips } };
}

export async function cancelRender(jobId: string): Promise<RenderJobSnapshot> {
  requireDesktopRuntime();
  return invoke<RenderJobSnapshot>("cancel_render", { jobId });
}

export async function getRenderJob(jobId: string): Promise<RenderJobSnapshot> {
  requireDesktopRuntime();
  return invoke<RenderJobSnapshot>("get_render_job", { jobId });
}

export async function startProxy(
  request: ProxyRequest,
): Promise<RenderJobSnapshot> {
  requireDesktopRuntime();
  const accepted = await invoke<RenderAccepted>("start_proxy", { request });
  return accepted.job;
}

export interface RenderSubscription {
  job: RenderJobSnapshot;
  unlisten: UnlistenFn;
}

async function startWithListener(
  start: () => Promise<RenderJobSnapshot>,
  onSnapshot: (snapshot: RenderJobSnapshot) => void,
): Promise<RenderSubscription> {
  requireDesktopRuntime();
  let targetJobId: string | null = null;
  const buffered: RenderJobSnapshot[] = [];
  const unlisten = await listenToAllRenderJobs((snapshot) => {
    if (targetJobId === null) {
      if (buffered.length >= 64) buffered.shift();
      buffered.push(snapshot);
    } else if (snapshot.jobId === targetJobId) {
      onSnapshot(snapshot);
    }
  });
  try {
    const accepted = await start();
    targetJobId = accepted.jobId;
    const latest = buffered.filter((item) => item.jobId === targetJobId).at(-1);
    return { job: latest ?? accepted, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}

export function startRenderWithListener(
  request: RenderRequest,
  onSnapshot: (snapshot: RenderJobSnapshot) => void,
): Promise<RenderSubscription> {
  return startWithListener(() => startRender(request), onSnapshot);
}

export function startAudioExportWithListener(
  request: AudioExportRequest,
  onSnapshot: (snapshot: RenderJobSnapshot) => void,
): Promise<RenderSubscription> {
  return startWithListener(() => startAudioExport(request), onSnapshot);
}

export function startProxyWithListener(
  request: ProxyRequest,
  onSnapshot: (snapshot: RenderJobSnapshot) => void,
): Promise<RenderSubscription> {
  return startWithListener(() => startProxy(request), onSnapshot);
}

export async function getProxyCacheStats(): Promise<ProxyCacheStats> {
  requireDesktopRuntime();
  return invoke<ProxyCacheStats>("get_proxy_cache_stats");
}

export async function probeMediaDuration(
  path: string,
  ffmpegPath?: string,
): Promise<number> {
  requireDesktopRuntime();
  return invoke<number>("probe_media_duration", { path, ffmpegPath });
}

export async function pruneProxyCache(maxBytes: number): Promise<ProxyPruneResult> {
  requireDesktopRuntime();
  return invoke<ProxyPruneResult>("prune_proxy_cache", { maxBytes });
}

export async function ensureFfmpeg(
  onProgress?: (progress: FfmpegSetupProgress) => void,
  force = false,
): Promise<RenderCapabilities> {
  requireDesktopRuntime();
  const unlisten = onProgress
    ? await listen<FfmpegSetupProgress>("ffmpeg://setup-progress", ({ payload }) =>
        onProgress(payload),
      )
    : null;
  try {
    return await invoke<RenderCapabilities>("ensure_ffmpeg", { force });
  } finally {
    unlisten?.();
  }
}

export async function listenToRenderJob(
  jobId: string,
  onSnapshot: (snapshot: RenderJobSnapshot) => void,
): Promise<UnlistenFn> {
  return listenToAllRenderJobs((snapshot) => {
    if (snapshot.jobId === jobId) onSnapshot(snapshot);
  });
}

async function listenToAllRenderJobs(
  onSnapshot: (snapshot: RenderJobSnapshot) => void,
): Promise<UnlistenFn> {
  requireDesktopRuntime();
  const unlisteners = await Promise.all(
    RENDER_EVENTS.map((eventName) =>
      listen<RenderJobSnapshot>(eventName, ({ payload }) => {
        onSnapshot(payload);
      }),
    ),
  );
  return () => unlisteners.forEach((unlisten) => unlisten());
}
