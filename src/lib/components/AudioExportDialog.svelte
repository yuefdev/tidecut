<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import { onDestroy } from "svelte";
  import {
    analyzeAudioLoudness,
    cancelRender,
    desktopRuntimeAvailable,
    ensureFfmpeg,
    getRenderCapabilities,
    probeMediaDuration,
    startAudioExportWithListener,
  } from "$lib/render/client";
  import type {
    AudioLoudnessAnalysisResult,
    AudioLoudnessTarget,
    AudioNormalizationPass,
    AudioGainKeyframe,
    FfmpegSetupProgress,
    RenderCapabilities,
    RenderJobSnapshot,
  } from "$lib/render/types";
  import type { NoiseReductionSettings } from "$lib/editor/noise-reduction";

  type AudioFormat = "mp3" | "m4a" | "wav";
  type NormalizationPreset = "voice" | "streaming" | "broadcast";
  type AudioJobSnapshot = Pick<
    RenderJobSnapshot,
    | "jobId"
    | "status"
    | "progressPercent"
    | "encodedMs"
    | "durationMs"
    | "etaMs"
    | "speed"
    | "outputPath"
    | "error"
  >;

  const AUDIO_FORMATS: Array<{ id: AudioFormat; label: string; extension: string }> = [
    { id: "mp3", label: "MP3", extension: "mp3" },
    { id: "m4a", label: "M4A / AAC", extension: "m4a" },
    { id: "wav", label: "WAV", extension: "wav" },
  ];

  const NORMALIZATION_PRESETS: Record<
    NormalizationPreset,
    { label: string; detail: string; target: AudioLoudnessTarget }
  > = {
    voice: {
      label: "Voice · Konuşma",
      detail: "Net ve dengeli konuşma",
      target: { integratedLufs: -16, truePeakDb: -1.5, loudnessRange: 11 },
    },
    streaming: {
      label: "Streaming",
      detail: "YouTube / sosyal medya",
      target: { integratedLufs: -14, truePeakDb: -1, loudnessRange: 11 },
    },
    broadcast: {
      label: "Yayın",
      detail: "EBU R128 televizyon standardı",
      target: { integratedLufs: -23, truePeakDb: -1, loudnessRange: 7 },
    },
  };

  interface Props {
    open: boolean;
    /** Source media path. */
    sourcePath?: string | null;
    /** Duration of the source range, rather than the full media duration. */
    durationMs: number;
    /** Base source offset for the provided range. */
    startMs?: number;
    playbackRate?: number;
    volume?: number;
    volumeKeyframes?: AudioGainKeyframe[];
    riderGainKeyframes?: AudioGainKeyframe[];
    duckGainKeyframes?: AudioGainKeyframe[];
    noiseReduction?: NoiseReductionSettings | null;
    onClose: () => void;
    onCompleted?: (outputPath: string, addToTimeline: boolean) => void;
  }

  let {
    open,
    sourcePath = null,
    durationMs,
    startMs = 0,
    playbackRate = 1,
    volume = 1,
    volumeKeyframes = [],
    riderGainKeyframes = [],
    duckGainKeyframes = [],
    noiseReduction = null,
    onClose,
    onCompleted,
  }: Props = $props();

  let format = $state<AudioFormat>("mp3");
  let outputPath = $state("");
  let addToTimeline = $state(true);
  let normalizeAudio = $state(true);
  let normalizationPreset = $state<NormalizationPreset>("voice");
  let analyzingLoudness = $state(false);
  let preparingCleanup = $state(false);
  let loudnessAnalysis = $state<AudioLoudnessAnalysisResult | null>(null);
  let rangeStartSeconds = $state(0);
  let rangeEndSeconds = $state(0);
  let probedDurationMs = $state<number | null>(null);
  let capabilities = $state<RenderCapabilities | null>(null);
  let checkingEngine = $state(false);
  let settingUpEngine = $state(false);
  let probingDuration = $state(false);
  let setupProgress = $state<FfmpegSetupProgress | null>(null);
  let durationError = $state<string | null>(null);
  let localError = $state<string | null>(null);
  let job = $state<AudioJobSnapshot | null>(null);
  let unlisten: (() => void) | null = null;
  let lastOpened = false;
  let initializationId = 0;
  let completedJobId: string | null = null;

  let suppliedDurationMs = $derived(normalizeDuration(durationMs));
  let sourceOffsetMs = $derived(normalizeOffset(startMs));
  let availableDurationMs = $derived(suppliedDurationMs || probedDurationMs || 0);
  let availableDurationSeconds = $derived(availableDurationMs / 1000);
  let rangeStartMs = $derived(
    secondsToMs(clamp(rangeStartSeconds, 0, availableDurationSeconds)),
  );
  let rangeEndMs = $derived(
    secondsToMs(clamp(rangeEndSeconds, 0, availableDurationSeconds)),
  );
  let selectedDurationMs = $derived(Math.max(0, rangeEndMs - rangeStartMs));
  let isBusy = $derived(
    job?.status === "queued" ||
      job?.status === "running" ||
      job?.status === "cancelling",
  );
  let isWorking = $derived(isBusy || analyzingLoudness || preparingCleanup);
  let canStart = $derived(
    !isWorking &&
      !checkingEngine &&
      !settingUpEngine &&
      !probingDuration &&
      Boolean(sourcePath) &&
      Boolean(outputPath) &&
      availableDurationMs > 0 &&
      selectedDurationMs > 0 &&
      capabilities?.available === true,
  );
  let currentFormatLabel = $derived(
    AUDIO_FORMATS.find((item) => item.id === format)?.label ?? "MP3",
  );
  let normalizationTarget = $derived(
    NORMALIZATION_PRESETS[normalizationPreset].target,
  );
  let hasManualAutomation = $derived(volumeKeyframes.length > 0);
  let hasVoiceRiderAutomation = $derived(riderGainKeyframes.length > 0);
  let hasDuckingAutomation = $derived(duckGainKeyframes.length > 0);

  $effect(() => {
    if (open && !lastOpened) {
      lastOpened = true;
      const currentInitialization = ++initializationId;
      resetForOpen();
      void initialize(currentInitialization);
    } else if (!open && lastOpened) {
      lastOpened = false;
      initializationId += 1;
    }
  });

  onDestroy(() => {
    initializationId += 1;
    unlisten?.();
    unlisten = null;
  });

  function normalizeDuration(value: number) {
    return Number.isFinite(value) && value > 0 ? Math.round(value) : 0;
  }

  function normalizeOffset(value: number) {
    return Number.isFinite(value) && value > 0 ? Math.round(value) : 0;
  }

  function clamp(value: number, minimum: number, maximum: number) {
    if (!Number.isFinite(value)) return minimum;
    return Math.min(maximum, Math.max(minimum, value));
  }

  function secondsToMs(value: number) {
    return Math.round(Math.max(0, Number.isFinite(value) ? value : 0) * 1000);
  }

  function resetForOpen() {
    format = "mp3";
    outputPath = "";
    addToTimeline = true;
    normalizeAudio = true;
    normalizationPreset = "voice";
    analyzingLoudness = false;
    preparingCleanup = false;
    loudnessAnalysis = null;
    job = null;
    completedJobId = null;
    localError = null;
    durationError = null;
    probedDurationMs = null;
    setRangeForDuration(suppliedDurationMs);
  }

  function setRangeForDuration(value: number) {
    rangeStartSeconds = 0;
    rangeEndSeconds = Math.max(0, value / 1000);
  }

  function isCurrent(initialization: number) {
    return open && initialization === initializationId;
  }

  async function initialize(initialization: number) {
    const result = await refreshCapabilities(initialization);
    if (!isCurrent(initialization) || suppliedDurationMs > 0) return;
    await resolveSourceDuration(initialization, result?.ffmpegPath);
  }

  async function refreshCapabilities(initialization?: number): Promise<RenderCapabilities | null> {
    if (initialization === undefined || isCurrent(initialization)) {
      checkingEngine = true;
      localError = null;
    }

    let result: RenderCapabilities;
    try {
      result = await getRenderCapabilities();
    } catch (error) {
      result = {
        available: false,
        ffmpegPath: "",
        diagnostic: {
          code: "capability_check_failed",
          userMessage: formatUnknownError(error),
          technicalMessage: formatUnknownError(error),
          retryable: true,
          stderrTail: [],
        },
      };
    }

    if (initialization === undefined || isCurrent(initialization)) {
      capabilities = result;
      checkingEngine = false;
      return result;
    }
    return null;
  }

  async function resolveSourceDuration(
    initialization = initializationId,
    ffmpegPath?: string,
  ) {
    if (suppliedDurationMs > 0) {
      if (isCurrent(initialization)) {
        probedDurationMs = null;
        durationError = null;
        setRangeForDuration(suppliedDurationMs);
      }
      return;
    }

    const path = sourcePath;
    if (!path) {
      if (isCurrent(initialization)) {
        durationError = "Sesi ayırmak için bir video seçin.";
      }
      return;
    }
    if (!capabilities?.available && !ffmpegPath) {
      if (isCurrent(initialization)) {
        durationError =
          capabilities?.diagnostic?.userMessage ??
          "Video süresi FFmpeg hazır olduğunda okunabilir.";
      }
      return;
    }

    if (isCurrent(initialization)) {
      probingDuration = true;
      durationError = null;
    }
    try {
      const fullDurationMs = await probeMediaDuration(path, ffmpegPath);
      const rangeDurationMs = Math.max(0, Math.round(fullDurationMs) - sourceOffsetMs);
      if (!rangeDurationMs) {
        throw new Error("Videoda dışa aktarılabilecek bir ses aralığı bulunamadı.");
      }
      if (isCurrent(initialization) && path === sourcePath) {
        probedDurationMs = rangeDurationMs;
        setRangeForDuration(rangeDurationMs);
      }
    } catch (error) {
      if (isCurrent(initialization)) {
        durationError = `Video süresi okunamadı: ${formatUnknownError(error)}`;
      }
    } finally {
      if (isCurrent(initialization)) probingDuration = false;
    }
  }

  async function installEngine() {
    settingUpEngine = true;
    localError = null;
    setupProgress = {
      stage: "starting",
      downloadedBytes: 0,
      totalBytes: 0,
      progressPercent: 0,
      message: "FFmpeg hazırlanıyor…",
    };
    try {
      capabilities = await ensureFfmpeg((progress) => (setupProgress = progress));
      if (suppliedDurationMs === 0) {
        await resolveSourceDuration(initializationId, capabilities.ffmpegPath || undefined);
      }
    } catch (error) {
      localError = formatUnknownError(error);
    } finally {
      settingUpEngine = false;
    }
  }

  function normalizeRangeStart() {
    loudnessAnalysis = null;
    rangeStartSeconds = clamp(rangeStartSeconds, 0, availableDurationSeconds);
    if (rangeEndSeconds < rangeStartSeconds) rangeEndSeconds = rangeStartSeconds;
  }

  function normalizeRangeEnd() {
    loudnessAnalysis = null;
    rangeEndSeconds = clamp(rangeEndSeconds, rangeStartSeconds, availableDurationSeconds);
  }

  function setFormat(next: string) {
    format = next as AudioFormat;
    if (outputPath) outputPath = setPathExtension(outputPath, format);
  }

  function sourceName(path = sourcePath) {
    if (!path) return "Video seçilmedi";
    return path.split(/[/\\]/).pop() || path;
  }

  function suggestedOutputPath() {
    const base = sourceName().replace(/\.[^.]+$/, "") || "astral-ses";
    return `${base}-ses.${format}`;
  }

  function audioFormatFromPath(path: string): AudioFormat | null {
    const extension = path.split(".").at(-1)?.toLowerCase();
    return AUDIO_FORMATS.find((item) => item.extension === extension)?.id ?? null;
  }

  function setPathExtension(path: string, nextFormat: AudioFormat) {
    const existingAudioFormat = audioFormatFromPath(path);
    if (existingAudioFormat) return path.replace(/\.(mp3|m4a|wav)$/i, `.${nextFormat}`);
    return `${path}.${nextFormat}`;
  }

  async function chooseOutput() {
    if (!desktopRuntimeAvailable()) {
      localError = "Hedef dosya masaüstü uygulamasında seçilebilir.";
      return;
    }
    localError = null;
    try {
      const selected = await save({
        title: "Sesi dışa aktar",
        defaultPath: suggestedOutputPath(),
        filters: [
          { name: "MP3 ses", extensions: ["mp3"] },
          { name: "M4A ses", extensions: ["m4a"] },
          { name: "WAV ses", extensions: ["wav"] },
        ],
      });
      if (!selected) return;
      const selectedFormat = audioFormatFromPath(selected);
      if (selectedFormat) format = selectedFormat;
      outputPath = selectedFormat ? selected : setPathExtension(selected, format);
    } catch (error) {
      localError = formatUnknownError(error);
    }
  }

  async function beginExport() {
    if (!canStart || !sourcePath) return;
    const exportInitialization = initializationId;
    const exportSourcePath = sourcePath;
    localError = null;
    job = null;
    completedJobId = null;
    loudnessAnalysis = null;
    unlisten?.();
    unlisten = null;
    const effectivePlaybackRate = normalizedPlaybackRate(playbackRate);
    const automation = {
      automationStartMs: Math.max(0, Math.round(rangeStartMs / effectivePlaybackRate)),
      volumeKeyframes: volumeKeyframes.map((frame) => ({ ...frame })),
      riderGainKeyframes: riderGainKeyframes.map((frame) => ({ ...frame })),
      duckGainKeyframes: duckGainKeyframes.map((frame) => ({ ...frame })),
    };

    try {
      preparingCleanup = Boolean(noiseReduction);
      let normalization: AudioNormalizationPass | undefined;
      if (normalizeAudio) {
        analyzingLoudness = true;
        try {
          loudnessAnalysis = await analyzeAudioLoudness({
            sourcePath,
            startMs: sourceOffsetMs + rangeStartMs,
            durationMs: selectedDurationMs,
            playbackRate: effectivePlaybackRate,
            volume: normalizedVolume(volume),
            ...automation,
            noiseReduction,
            target: normalizationTarget,
          });
          normalization = loudnessAnalysis.pass;
        } finally {
          analyzingLoudness = false;
        }
      }

      if (!isCurrent(exportInitialization) || sourcePath !== exportSourcePath) return;

      const subscription = await startAudioExportWithListener(
        {
          sourcePath,
          outputPath,
          startMs: sourceOffsetMs + rangeStartMs,
          durationMs: selectedDurationMs,
          playbackRate: effectivePlaybackRate,
          volume: normalizedVolume(volume),
          ...automation,
          noiseReduction,
          normalization,
          overwrite: true,
        },
        handleJobSnapshot,
      );
      job = subscription.job;
      preparingCleanup = false;
      unlisten = subscription.unlisten;
      if (isTerminal(subscription.job)) handleJobSnapshot(subscription.job);
    } catch (error) {
      analyzingLoudness = false;
      preparingCleanup = false;
      localError = formatUnknownError(error);
    }
  }

  function handleJobSnapshot(snapshot: AudioJobSnapshot) {
    job = snapshot;
    if (!isTerminal(snapshot)) return;

    unlisten?.();
    unlisten = null;
    if (snapshot.status === "completed" && completedJobId !== snapshot.jobId) {
      completedJobId = snapshot.jobId;
      onCompleted?.(snapshot.outputPath, addToTimeline);
    }
  }

  function isTerminal(snapshot: AudioJobSnapshot) {
    return ["completed", "cancelled", "failed"].includes(snapshot.status);
  }

  async function requestCancel() {
    if (!job || !isBusy) return;
    try {
      handleJobSnapshot(await cancelRender(job.jobId));
    } catch (error) {
      localError = formatUnknownError(error);
    }
  }

  function closeDialog() {
    if (isWorking || settingUpEngine) return;
    unlisten?.();
    unlisten = null;
    onClose();
  }

  function handleDialogKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") closeDialog();
  }

  function normalizedPlaybackRate(value: number) {
    return Number.isFinite(value) && value > 0 ? value : 1;
  }

  function normalizedVolume(value: number) {
    return Number.isFinite(value) && value >= 0 ? value : 1;
  }

  function formatUnknownError(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    const value = error as { userMessage?: string; message?: string } | null;
    if (value?.userMessage || value?.message) return value.userMessage ?? value.message ?? "Bilinmeyen hata";
    try {
      return JSON.stringify(error);
    } catch {
      return "Bilinmeyen hata";
    }
  }

  function formatDuration(value?: number) {
    if (value === undefined || !Number.isFinite(value)) return "—";
    const totalSeconds = Math.max(0, Math.round(value / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    return hours > 0
      ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
      : `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function formatSignedDb(value: number) {
    if (!Number.isFinite(value)) return "—";
    const sign = value > 0 ? "+" : "";
    return `${sign}${value.toFixed(1)} dB`;
  }
</script>

<svelte:window onkeydown={handleDialogKeydown} />

{#if open}
  <div class="backdrop">
    <div
      class="dialog"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="audio-export-title"
    >
      <header>
        <div>
          <span class="eyebrow">Ses</span>
          <h2 id="audio-export-title">Sesi ayır ve dışa aktar</h2>
        </div>
        <button class="icon-button" onclick={closeDialog} disabled={isWorking || settingUpEngine} aria-label="Kapat">×</button>
      </header>

      <div class="content">
        <div class="source-card">
          <span class="wave" aria-hidden="true">∿</span>
          <div>
            <strong>{sourceName()}</strong>
            <small>
              {#if probingDuration}
                Video süresi okunuyor…
              {:else if availableDurationMs > 0}
                Kullanılabilir ses · {formatDuration(availableDurationMs)}
              {:else}
                Ses aralığı hazırlanıyor
              {/if}
            </small>
            {#if hasVoiceRiderAutomation || hasManualAutomation || hasDuckingAutomation}
              <small class="automation-preserved">
                {hasVoiceRiderAutomation ? "AI Voice Rider" : "Klip ses eğrisi"}
                {hasVoiceRiderAutomation && hasManualAutomation ? " + manuel otomasyon" : ""}
                {hasDuckingAutomation ? " + AI Ducking" : ""}
                dışa aktarımda korunacak
              </small>
            {/if}
            {#if noiseReduction}
              <small class="automation-preserved">
                AI Ses Temizleme · RNNoise %{Math.round(noiseReduction.strength * 100)} · aynı önbellek dışa aktarımda kullanılacak
              </small>
            {/if}
          </div>
        </div>

        <div class="section-label">Alınacak aralık</div>
        <div class="settings-grid">
          <label>
            <span>Başlangıç</span>
            <div class="number-input">
              <input
                type="number"
                min={0}
                max={rangeEndSeconds}
                step="0.01"
                bind:value={rangeStartSeconds}
                oninput={normalizeRangeStart}
                disabled={isWorking || availableDurationMs <= 0}
                aria-label="Başlangıç saniyesi"
              />
              <em>sn</em>
            </div>
          </label>
          <label>
            <span>Bitiş</span>
            <div class="number-input">
              <input
                type="number"
                min={rangeStartSeconds}
                max={availableDurationSeconds}
                step="0.01"
                bind:value={rangeEndSeconds}
                oninput={normalizeRangeEnd}
                disabled={isWorking || availableDurationMs <= 0}
                aria-label="Bitiş saniyesi"
              />
              <em>sn</em>
            </div>
          </label>
        </div>
        <div class="range-summary">
          <span>Seçili: <strong>{formatDuration(selectedDurationMs)}</strong></span>
          <span>Kaynakta: {formatDuration(sourceOffsetMs + rangeStartMs)} – {formatDuration(sourceOffsetMs + rangeEndMs)}</span>
        </div>

        {#if durationError}
          <div class="source-warning" role="status">
            <span>{durationError}</span>
            {#if sourcePath && !probingDuration && capabilities?.available}
              <button onclick={() => resolveSourceDuration()}>Yeniden dene</button>
            {/if}
          </div>
        {:else if selectedDurationMs <= 0 && availableDurationMs > 0}
          <div class="source-warning" role="status">Dışa aktarmak için başlangıçtan daha ileri bir bitiş seçin.</div>
        {/if}

        <div class="section-label">Dosya</div>
        <div class="settings-grid">
          <label>
            <span>Biçim</span>
            <select value={format} onchange={(event) => setFormat((event.currentTarget as HTMLSelectElement).value)} disabled={isWorking}>
              {#each AUDIO_FORMATS as option}
                <option value={option.id}>{option.label}</option>
              {/each}
            </select>
          </label>
          <label class="check-row">
            <input type="checkbox" bind:checked={addToTimeline} disabled={isWorking} />
            <span>Ses parçası olarak zaman çizelgesine ekle</span>
          </label>
        </div>

        <div class="normalization-card" class:active={normalizeAudio}>
          <label class="normalization-toggle">
            <input
              type="checkbox"
              bind:checked={normalizeAudio}
              disabled={isWorking}
              onchange={() => (loudnessAnalysis = null)}
            />
            <span>
              <strong>Akıllı Normalize</strong>
              <small>İki aşamalı ses analiziyle ani tepe yapmadan dengeler.</small>
            </span>
          </label>
          {#if normalizeAudio}
            <label class="preset-control">
              <span>Hedef</span>
              <select
                bind:value={normalizationPreset}
                disabled={isWorking}
                onchange={() => (loudnessAnalysis = null)}
              >
                {#each Object.entries(NORMALIZATION_PRESETS) as [id, option]}
                  <option value={id}>{option.label} · {option.target.integratedLufs} LUFS</option>
                {/each}
              </select>
              <small>{NORMALIZATION_PRESETS[normalizationPreset].detail} · {normalizationTarget.truePeakDb} dBTP</small>
            </label>
          {/if}
        </div>

        {#if analyzingLoudness}
          <div class="analysis-status" role="status">
            <span class="analysis-pulse" aria-hidden="true"></span>
            Sesin gerçek yüksekliği ve tepe noktaları analiz ediliyor…
          </div>
        {:else if loudnessAnalysis}
          <div class="analysis-result" role="status">
            <strong>{loudnessAnalysis.pass.measuredIntegratedLufs.toFixed(1)} LUFS</strong>
            <span>→ {loudnessAnalysis.pass.integratedLufs.toFixed(1)} LUFS</span>
            <span>{formatSignedDb(loudnessAnalysis.recommendedGainDb)} önerilen düzeltme</span>
          </div>
        {/if}

        <button class="path-picker" onclick={chooseOutput} disabled={isWorking}>
          <span class:placeholder={!outputPath}>{outputPath || `${currentFormatLabel} hedefi seç…`}</span>
          <strong>Seç</strong>
        </button>

        <div class="engine-status" class:error={capabilities && !capabilities.available}>
          <span class="status-dot"></span>
          {#if checkingEngine}
            FFmpeg kontrol ediliyor…
          {:else if settingUpEngine}
            {setupProgress?.message || "FFmpeg kuruluyor…"}
          {:else if capabilities?.available}
            FFmpeg hazır{capabilities.version ? ` · ${capabilities.version}` : ""}
          {:else}
            {capabilities?.diagnostic?.userMessage || "Ses dışa aktarma motoru bulunamadı"}
          {/if}
          {#if !checkingEngine && !capabilities?.available && desktopRuntimeAvailable()}
            <button onclick={installEngine} disabled={settingUpEngine}>Kur ve hazırla</button>
          {/if}
        </div>

        {#if settingUpEngine && setupProgress}
          <div class="setup-progress">
            <div style={`width: ${Math.max(0, Math.min(100, setupProgress.progressPercent ?? 0))}%`}></div>
          </div>
        {/if}

        {#if job}
          <div class="job-card" data-status={job.status}>
            <div class="job-heading">
              <div>
                <strong>
                  {job.status === "completed" ? "Ses dışa aktarıldı" :
                   job.status === "failed" ? "Ses dışa aktarılamadı" :
                   job.status === "cancelled" ? "Dışa aktarma iptal edildi" :
                   job.status === "cancelling" ? "İptal ediliyor…" : "Ses işleniyor"}
                </strong>
                <small>{Math.round(job.progressPercent)}% · {formatDuration(job.encodedMs)} / {formatDuration(job.durationMs)}</small>
              </div>
              {#if job.status === "running"}
                <span class="eta">{formatDuration(job.etaMs)} kaldı</span>
              {/if}
            </div>
            <div class="progress-track" aria-label="Ses dışa aktarma ilerlemesi" aria-valuenow={job.progressPercent} role="progressbar">
              <div style="width: {Math.max(0, Math.min(100, job.progressPercent))}%"></div>
            </div>
            {#if job.status === "running" && job.speed}
              <div class="metrics">{job.speed.toFixed(2)}× hız</div>
            {/if}
            {#if job.error}
              <details open>
                <summary>{job.error.code} · {job.error.userMessage}</summary>
                <p>{job.error.technicalMessage}</p>
                <pre>{Array.isArray(job.error.stderrTail) ? job.error.stderrTail.join("\n") : job.error.stderrTail}</pre>
              </details>
            {/if}
          </div>
        {/if}

        {#if localError}
          <div class="local-error" role="alert">{localError}</div>
        {/if}
      </div>

      <footer>
        <div class="summary">
          {currentFormatLabel} · {formatDuration(selectedDurationMs)} · {normalizedPlaybackRate(playbackRate).toFixed(2)}× · %{Math.round(normalizedVolume(volume) * 100)}{noiseReduction ? " · AI temiz" : ""}{normalizeAudio ? ` · ${normalizationTarget.integratedLufs} LUFS` : ""}
        </div>
        <div class="footer-actions">
          {#if preparingCleanup}
            <button class="primary" disabled>AI temiz ses hazırlanıyor…</button>
          {:else if analyzingLoudness}
            <button class="primary" disabled>Ses analiz ediliyor…</button>
          {:else if isBusy}
            <button class="secondary danger" onclick={requestCancel} disabled={job?.status === "cancelling"}>İptal et</button>
          {:else}
            <button class="secondary" onclick={closeDialog} disabled={isWorking || settingUpEngine}>Kapat</button>
            <button class="primary" onclick={beginExport} disabled={!canStart}>Sesi dışa aktar</button>
          {/if}
        </div>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1005; display: grid; place-items: center; padding: 24px; background: rgba(0, 0, 0, .72); backdrop-filter: blur(8px); }
  .dialog { width: min(650px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto; color: #e8e8e8; background: #121313; border: 1px solid #2b2c2c; border-radius: 14px; box-shadow: 0 28px 90px rgba(0,0,0,.58); }
  header, footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px 22px; }
  header { border-bottom: 1px solid #242525; }
  footer { border-top: 1px solid #242525; background: #101111; }
  h2 { margin: 2px 0 0; font-size: 19px; letter-spacing: -.02em; }
  .eyebrow, .section-label { color: #6f7473; font-size: 10px; font-weight: 700; letter-spacing: .12em; text-transform: uppercase; }
  .content { display: grid; gap: 14px; padding: 20px 22px; }
  button, input, select { font: inherit; }
  button { cursor: pointer; }
  button:disabled, input:disabled, select:disabled { cursor: not-allowed; opacity: .45; }
  .icon-button { width: 30px; height: 30px; color: #888; background: #1a1b1b; border: 1px solid #2b2c2c; border-radius: 7px; font-size: 20px; }
  .source-card { display: flex; align-items: center; gap: 11px; padding: 11px 12px; background: linear-gradient(135deg, rgba(69, 143, 118, .14), #181919); border: 1px solid rgba(98, 215, 177, .24); border-radius: 9px; }
  .wave { display: grid; width: 30px; height: 30px; place-items: center; color: #78e2bd; background: rgba(98, 215, 177, .1); border-radius: 7px; font-size: 25px; line-height: 1; }
  .source-card strong, .source-card small { display: block; max-width: 520px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .source-card strong { color: #dfe8e5; font-size: 12px; }
  .source-card small { margin-top: 4px; color: #79807d; font-size: 10px; }
  .source-card small.automation-preserved { color: #74dcb9; }
  .settings-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  label > span { display: block; margin-bottom: 6px; color: #8c908f; font-size: 11px; }
  select, .number-input { width: 100%; box-sizing: border-box; padding: 9px 10px; color: #ddd; background: #191a1a; border: 1px solid #2a2c2b; border-radius: 7px; font-size: 12px; outline: none; }
  select:focus, .number-input:focus-within { border-color: #62d7b1; }
  .number-input { display: flex; align-items: center; gap: 7px; padding-right: 8px; }
  .number-input input { width: 100%; min-width: 0; padding: 0; color: inherit; background: transparent; border: 0; outline: 0; }
  .number-input em { color: #707774; font-size: 10px; font-style: normal; }
  .range-summary { display: flex; justify-content: space-between; gap: 10px; margin-top: -6px; color: #747a77; font-size: 10px; }
  .range-summary strong { color: #c9d6d1; }
  .check-row { display: flex; align-items: center; align-self: end; min-height: 37px; box-sizing: border-box; padding: 0 10px; background: #191a1a; border: 1px solid #2a2c2b; border-radius: 7px; }
  .check-row span { margin: 0 0 0 8px; color: #c4cac7; line-height: 1.25; }
  .check-row input { flex: 0 0 auto; accent-color: #62d7b1; }
  .normalization-card { display: grid; gap: 11px; padding: 12px; background: #171818; border: 1px solid #292b2a; border-radius: 9px; transition: border-color .15s, background .15s; }
  .normalization-card.active { background: linear-gradient(135deg, rgba(98,215,177,.09), #171918); border-color: rgba(98,215,177,.36); }
  .normalization-toggle { display: flex; align-items: flex-start; gap: 9px; cursor: pointer; }
  .normalization-toggle input { margin-top: 2px; accent-color: #62d7b1; }
  .normalization-toggle span, .normalization-toggle strong, .normalization-toggle small { display: block; margin: 0; }
  .normalization-toggle strong { color: #d9e8e2; font-size: 12px; }
  .normalization-toggle small, .preset-control small { margin-top: 4px; color: #747c79; font-size: 10px; line-height: 1.35; }
  .preset-control { display: grid; grid-template-columns: 62px 1fr; align-items: center; gap: 5px 10px; padding-top: 10px; border-top: 1px solid rgba(98,215,177,.13); }
  .preset-control > span { margin: 0; color: #87908c; font-size: 10px; }
  .preset-control small { grid-column: 2; margin-top: 0; }
  .analysis-status, .analysis-result { display: flex; align-items: center; gap: 8px; margin-top: -5px; padding: 8px 10px; border-radius: 7px; font-size: 10px; }
  .analysis-status { color: #b9d8cd; background: rgba(70,139,116,.1); border: 1px solid rgba(98,215,177,.2); }
  .analysis-pulse { width: 7px; height: 7px; flex: 0 0 auto; background: #62d7b1; border-radius: 50%; box-shadow: 0 0 0 0 rgba(98,215,177,.5); animation: analysis-pulse 1.25s infinite; }
  .analysis-result { color: #8d9894; background: #171b19; border: 1px solid #2b3732; }
  .analysis-result strong { color: #c8f2e3; }
  .analysis-result span:last-child { margin-left: auto; color: #75d7b7; }
  .path-picker { display: flex; justify-content: space-between; gap: 12px; width: 100%; padding: 10px 12px; color: #ccc; text-align: left; background: #191a1a; border: 1px solid #2a2c2b; border-radius: 7px; }
  .path-picker span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path-picker .placeholder { color: #656968; }
  .path-picker strong { color: #62d7b1; font-size: 11px; }
  .source-warning { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 8px 10px; color: #c8a977; background: rgba(164,116,42,.1); border: 1px solid rgba(190,139,57,.24); border-radius: 6px; font-size: 10px; }
  .source-warning button { flex: 0 0 auto; padding: 3px 7px; color: #e4c28c; background: transparent; border: 1px solid rgba(190,139,57,.4); border-radius: 5px; font-size: 10px; }
  .engine-status { display: flex; align-items: center; gap: 7px; min-height: 22px; color: #858b89; font-size: 11px; }
  .engine-status .status-dot { width: 7px; height: 7px; flex: 0 0 auto; background: #62d7b1; border-radius: 50%; box-shadow: 0 0 10px rgba(98,215,177,.5); }
  .engine-status.error .status-dot { background: #df7f72; box-shadow: none; }
  .engine-status button { margin-left: auto; padding: 3px 7px; color: #ddd; background: transparent; border: 1px solid #343636; border-radius: 5px; }
  .setup-progress { height: 3px; margin-top: -10px; overflow: hidden; background: #292b2a; border-radius: 999px; }
  .setup-progress div { height: 100%; background: #62d7b1; transition: width .2s ease; }
  .job-card { padding: 13px; background: #181919; border: 1px solid #2a2c2b; border-radius: 9px; }
  .job-card[data-status="failed"] { border-color: rgba(223,127,114,.55); }
  .job-card[data-status="completed"] { border-color: rgba(98,215,177,.5); }
  .job-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .job-heading strong, .job-heading small { display: block; }
  .job-heading strong { font-size: 12px; }
  .job-heading small, .eta, .metrics { margin-top: 4px; color: #777c7a; font-size: 10px; }
  .progress-track { height: 5px; margin: 12px 0 8px; overflow: hidden; background: #292b2a; border-radius: 999px; }
  .progress-track div { height: 100%; background: linear-gradient(90deg, #55bfa0, #75e5bd); transition: width .2s ease; }
  details { margin-top: 12px; padding-top: 10px; border-top: 1px solid #2b2c2c; color: #c7867b; font-size: 11px; }
  details p { color: #aaa; }
  pre { max-height: 110px; overflow: auto; padding: 9px; color: #9b9f9e; background: #0d0e0e; border-radius: 5px; font-size: 9px; white-space: pre-wrap; }
  .local-error { padding: 9px 11px; color: #efafa5; background: rgba(194,76,61,.12); border: 1px solid rgba(194,76,61,.35); border-radius: 7px; font-size: 11px; }
  .summary { color: #737876; font-size: 10px; }
  .footer-actions { display: flex; gap: 8px; }
  .primary, .secondary { padding: 8px 14px; border-radius: 7px; font-size: 11px; font-weight: 650; }
  .primary { color: #0a1712; background: #62d7b1; border: 1px solid #74e9c2; }
  .secondary { color: #aaa; background: #191a1a; border: 1px solid #303232; }
  .danger { color: #f0a79b; border-color: rgba(223,127,114,.5); }
  @keyframes analysis-pulse { 70% { box-shadow: 0 0 0 7px rgba(98,215,177,0); } 100% { box-shadow: 0 0 0 0 rgba(98,215,177,0); } }
  @media (max-width: 620px) { .settings-grid { grid-template-columns: 1fr; } .range-summary { display: grid; gap: 3px; } .summary { display: none; } }
</style>
