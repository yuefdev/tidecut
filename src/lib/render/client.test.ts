import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriCore = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));
const tauriEvent = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => tauriCore);
vi.mock("@tauri-apps/api/event", () => tauriEvent);

import {
  analyzeBeats,
  analyzeAudioEnergy,
  analyzeAudioLoudness,
  analyzeLaughter,
  analyzeSpeechActivity,
  analyzeSpeechTranscript,
  analyzeVoiceRider,
  onAudioEnhancementProgress,
  onLaughterAnalysisProgress,
  prepareMossFormerTestCleanup,
  startAudioExport,
  startRender,
} from "./client";
import type { NoiseReductionSettings } from "$lib/editor/noise-reduction";
import type { RenderJobSnapshot } from "./types";

const cleanup: NoiseReductionSettings = {
  engine: "rnnoise",
  model: "rnnoise-bd-v1",
  strength: 0.72,
  highPassHz: 80,
  humFrequency: 50,
};

const job: RenderJobSnapshot = {
  jobId: "job-1",
  kind: "export",
  status: "queued",
  progressPercent: 0,
  encodedMs: 0,
  durationMs: 10_000,
  outputPath: "C:/out/result.mp4",
};

describe("render client neural cleanup materialization", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriCore.isTauri.mockReturnValue(true);
    tauriEvent.listen.mockResolvedValue(vi.fn());
    tauriCore.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === "prepare_audio_cleanup") {
        return {
          outputPath: "C:/cache/clean.wav",
          durationMs: args.request.durationMs,
          cacheHit: false,
          engine: "rnnoise",
          model: "rnnoise-bd-v1",
        };
      }
      if (command === "start_render" || command === "start_audio_export") {
        return { job, cacheHit: false };
      }
      return {};
    });
  });

  it("uses explicit desktop commands for speech, clip-local energy, laughter, and beats", async () => {
    await analyzeSpeechActivity({
      sourcePath: "C:/media/voice.wav",
      startMs: 100,
      durationMs: 5_000,
      playbackRate: 1.25,
    });
    await analyzeSpeechTranscript({
      sourcePath: "C:/media/voice.wav",
      startMs: 100,
      durationMs: 5_000,
      playbackRate: 1.25,
    });
    await analyzeAudioEnergy({
      sourcePath: "C:/media/voice.wav",
      startMs: 100,
      durationMs: 5_000,
      playbackRate: 1.25,
    });
    await analyzeLaughter({
      sourcePath: "C:/media/voice.wav",
      startMs: 100,
      durationMs: 5_000,
      playbackRate: 1.25,
      minConfidence: 0.32,
    });
    await analyzeBeats({
      sourcePath: "C:/media/music.wav",
      startMs: 0,
      durationMs: 30_000,
    });

    expect(tauriCore.invoke.mock.calls.slice(-5)).toEqual([
      ["analyze_speech_activity", { request: {
        sourcePath: "C:/media/voice.wav",
        startMs: 100,
        durationMs: 5_000,
        playbackRate: 1.25,
      } }],
      ["analyze_speech_transcript", { request: {
        sourcePath: "C:/media/voice.wav",
        startMs: 100,
        durationMs: 5_000,
        playbackRate: 1.25,
      } }],
      ["analyze_audio_energy", { request: {
        sourcePath: "C:/media/voice.wav",
        startMs: 100,
        durationMs: 5_000,
        playbackRate: 1.25,
      } }],
      ["analyze_laughter", { request: {
        sourcePath: "C:/media/voice.wav",
        startMs: 100,
        durationMs: 5_000,
        playbackRate: 1.25,
        minConfidence: 0.32,
      } }],
      ["analyze_beats", { request: {
        sourcePath: "C:/media/music.wav",
        startMs: 0,
        durationMs: 30_000,
      } }],
    ]);
  });

  it("forwards local YAMNet setup and inference progress", async () => {
    const callback = vi.fn();
    await onLaughterAnalysisProgress(callback);

    expect(tauriEvent.listen).toHaveBeenCalledWith(
      "laughter-analysis://progress",
      expect.any(Function),
    );
    const handler = tauriEvent.listen.mock.calls[0][1];
    const progress = {
      stage: "classifying",
      progressPercent: 78,
      message: "YAMNet kahkaha ve gülme sınıflarını tarıyor.",
    };
    handler({ payload: progress });

    expect(callback).toHaveBeenCalledWith(progress);
  });

  it("keeps the experimental MossFormer preview on its own command", async () => {
    await prepareMossFormerTestCleanup({
      operationId: "cleanup-123",
      sourcePath: "C:/media/voice.mp4",
      startMs: 750,
      durationMs: 9_000,
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith(
      "prepare_mossformer_test_cleanup",
      {
        request: {
          operationId: "cleanup-123",
          sourcePath: "C:/media/voice.mp4",
          startMs: 750,
          durationMs: 9_000,
        },
      },
    );
  });

  it("forwards structured hybrid enhancement progress", async () => {
    const callback = vi.fn();
    await onAudioEnhancementProgress(callback);

    expect(tauriEvent.listen).toHaveBeenCalledWith(
      "audio-enhancement://progress",
      expect.any(Function),
    );
    const handler = tauriEvent.listen.mock.calls[0][1];
    const progress = {
      operationId: "cleanup-123",
      sequence: 4,
      phase: "mossformer",
      overallProgressPercent: 72,
      phaseProgressPercent: 40,
      processedMs: 12_000,
      totalMs: 30_000,
      message: "MossFormer2 konuşmayı ayırıyor",
    };
    handler({ payload: progress });

    expect(callback).toHaveBeenCalledWith(progress);
  });

  it("uses a clean cache input for timeline audio even when includeAudio is omitted", async () => {
    const request = {
      outputPath: "C:/out/result.mp4",
      durationMs: 10_000,
      preset: "16:9" as const,
      timeline: {
        tracks: [{ id: "v1", kind: "video" as const }],
        clips: [
          {
            id: "clip-1",
            trackId: "v1",
            kind: "video" as const,
            path: "C:/media/source.mp4",
            startMs: 0,
            durationMs: 10_000,
            trimInMs: 2_250,
            speed: 1.5,
            noiseReduction: cleanup,
          },
        ],
      },
    };

    await startRender(request);

    expect(tauriCore.invoke).toHaveBeenNthCalledWith(1, "prepare_audio_cleanup", {
      request: {
        sourcePath: "C:/media/source.mp4",
        startMs: 2_250,
        durationMs: 15_000,
        settings: cleanup,
        ffmpegPath: undefined,
      },
    });
    const renderRequest = tauriCore.invoke.mock.calls[1][1].request;
    expect(renderRequest.timeline.clips[0]).toMatchObject({
      path: "C:/media/source.mp4",
      audioPath: "C:/cache/clean.wav",
      audioTrimInMs: 0,
      noiseReduction: cleanup,
    });
    expect(request.timeline.clips[0]).not.toHaveProperty("audioPath");
  });

  it("preserves an explicitly selected hybrid cache asset for final render", async () => {
    await startRender({
      outputPath: "C:/out/hybrid.mp4",
      durationMs: 10_000,
      preset: "16:9",
      timeline: {
        tracks: [{ id: "v1", kind: "video" }],
        clips: [{
          id: "clip-1",
          trackId: "v1",
          kind: "video",
          path: "C:/media/source.mp4",
          audioPath: "C:/cache/mossformer-deepfilter.wav",
          audioTrimInMs: 0,
          startMs: 0,
          durationMs: 10_000,
          trimInMs: 2_250,
          speed: 1.5,
          noiseReduction: null,
        }],
      },
    });

    expect(tauriCore.invoke).toHaveBeenCalledTimes(1);
    expect(tauriCore.invoke).toHaveBeenCalledWith(
      "start_render",
      expect.objectContaining({
        request: expect.objectContaining({
          timeline: expect.objectContaining({
            clips: [expect.objectContaining({
              audioPath: "C:/cache/mossformer-deepfilter.wav",
              audioTrimInMs: 0,
              noiseReduction: null,
            })],
          }),
        }),
      }),
    );
  });

  it("does not prepare clean audio when timeline audio is explicitly disabled", async () => {
    await startRender({
      outputPath: "C:/out/silent.mp4",
      durationMs: 1_000,
      preset: "16:9",
      includeAudio: false,
      timeline: {
        tracks: [{ id: "a1", kind: "audio" }],
        clips: [
          {
            id: "clip-1",
            trackId: "a1",
            kind: "audio",
            path: "C:/media/source.wav",
            startMs: 0,
            durationMs: 1_000,
            noiseReduction: cleanup,
          },
        ],
      },
    });

    expect(tauriCore.invoke).toHaveBeenCalledTimes(1);
    expect(tauriCore.invoke).toHaveBeenCalledWith("start_render", expect.any(Object));
  });

  it.each([
    {
      label: "standalone export",
      command: "start_audio_export",
      run: () =>
        startAudioExport({
          sourcePath: "C:/media/source.mp4",
          outputPath: "C:/out/voice.wav",
          startMs: 500,
          durationMs: 15_000,
          playbackRate: 1.25,
          noiseReduction: cleanup,
        }),
    },
    {
      label: "loudness analysis",
      command: "analyze_audio_loudness",
      run: () =>
        analyzeAudioLoudness({
          sourcePath: "C:/media/source.mp4",
          startMs: 500,
          durationMs: 15_000,
          noiseReduction: cleanup,
          target: { integratedLufs: -16, truePeakDb: -1.5, loudnessRange: 11 },
        }),
    },
    {
      label: "Voice Rider analysis",
      command: "analyze_voice_rider",
      run: () =>
        analyzeVoiceRider({
          sourcePath: "C:/media/source.mp4",
          startMs: 500,
          durationMs: 15_000,
          playbackRate: 1.25,
          baseVolume: 1,
          noiseReduction: cleanup,
        }),
    },
  ])("reuses the same clean range for $label", async ({ command, run }) => {
    await run();

    expect(tauriCore.invoke).toHaveBeenNthCalledWith(1, "prepare_audio_cleanup", {
      request: {
        sourcePath: "C:/media/source.mp4",
        startMs: 500,
        durationMs: 15_000,
        settings: cleanup,
        ffmpegPath: undefined,
      },
    });
    const finalRequest = tauriCore.invoke.mock.calls[1][1].request;
    expect(tauriCore.invoke.mock.calls[1][0]).toBe(command);
    expect(finalRequest).toMatchObject({
      sourcePath: "C:/cache/clean.wav",
      startMs: 0,
      durationMs: 15_000,
      noiseReduction: null,
    });
  });
});
