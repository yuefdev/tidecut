<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import { onDestroy } from "svelte";
  import {
    cancelRender,
    desktopRuntimeAvailable,
    ensureFfmpeg,
    getRenderCapabilities,
    startRenderWithListener,
  } from "$lib/render/client";
  import {
    EXPORT_PRESETS,
    type ExportPreset,
    type FfmpegSetupProgress,
    type RenderCapabilities,
    type RenderFit,
    type RenderJobSnapshot,
    type RenderQuality,
  } from "$lib/render/types";

  interface Props {
    open: boolean;
    durationMs: number;
    sourcePath?: string | null;
    timeline?: import("$lib/render/types").RenderTimeline;
    timelinesByPreset?: Partial<
      Record<ExportPreset, import("$lib/render/types").RenderTimeline>
    >;
    autoReframePresets?: ExportPreset[];
    defaultPreset?: ExportPreset;
    onClose: () => void;
    onCompleted?: (outputPath: string) => void;
  }

  let {
    open,
    durationMs,
    sourcePath = null,
    timeline,
    timelinesByPreset,
    autoReframePresets = [],
    defaultPreset = "16:9",
    onClose,
    onCompleted,
  }: Props = $props();

  let preset = $state<ExportPreset>("16:9");
  let quality = $state<RenderQuality>("high");
  let fit = $state<RenderFit>("contain");
  let frameRate = $state(30);
  let includeAudio = $state(true);
  let batchEnabled = $state(false);
  let outputPath = $state("");
  let capabilities = $state<RenderCapabilities | null>(null);
  let checkingEngine = $state(false);
  let settingUpEngine = $state(false);
  let preparingAudio = $state(false);
  let setupProgress = $state<FfmpegSetupProgress | null>(null);
  let job = $state<RenderJobSnapshot | null>(null);
  let localError = $state<string | null>(null);
  let batchBusy = $state(false);
  let batchIndex = $state(0);
  let batchTotal = $state(0);
  let completedOutputPaths = $state<string[]>([]);
  let cancelBatchRequested = false;
  let unlisten: (() => void) | null = null;
  let lastOpened = false;
  let dialogElement = $state<HTMLElement>();
  let previouslyFocused: HTMLElement | null = null;

  let normalizedAutoReframePresets = $derived(
    [...new Set(autoReframePresets)].filter((value): value is ExportPreset =>
      value === "9:16" || value === "1:1" || value === "16:9",
    ),
  );
  let requestedPresets = $derived(
    batchEnabled && normalizedAutoReframePresets.length > 1
      ? normalizedAutoReframePresets
      : [preset],
  );
  let selectedTimeline = $derived(timelinesByPreset?.[preset] ?? timeline);
  let autoReframeFitLocked = $derived(
    requestedPresets.some((targetPreset) => Boolean(timelinesByPreset?.[targetPreset])),
  );

  let renderIsBusy = $derived(
    job?.status === "queued" ||
      job?.status === "running" ||
      job?.status === "cancelling",
  );
  let isBusy = $derived(renderIsBusy || preparingAudio || batchBusy);
  let hasNeuralCleanup = $derived(
    Boolean(
      includeAudio &&
      requestedPresets.some((targetPreset) =>
        (timelinesByPreset?.[targetPreset] ?? timeline)?.clips.some(
          (clip) => clip.noiseReduction,
        ),
      )
    ),
  );
  let canStart = $derived(
    !isBusy &&
      Boolean(outputPath) &&
      durationMs > 0 &&
      requestedPresets.every((targetPreset) =>
        Boolean(sourcePath || (timelinesByPreset?.[targetPreset] ?? timeline)?.clips.length),
      ) &&
      capabilities?.available === true,
  );

  $effect(() => {
    if (open && !lastOpened) {
      lastOpened = true;
      previouslyFocused = document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
      preset = defaultPreset;
      batchEnabled = normalizedAutoReframePresets.length > 1;
      batchIndex = 0;
      batchTotal = 0;
      completedOutputPaths = [];
      queueMicrotask(() => exportFocusableElements()[0]?.focus() ?? dialogElement?.focus());
      void refreshCapabilities();
    } else if (!open) {
      const focusTarget = previouslyFocused;
      previouslyFocused = null;
      lastOpened = false;
      queueMicrotask(() => focusTarget?.focus());
    }
  });

  $effect(() => {
    if (autoReframeFitLocked) fit = "contain";
  });

  onDestroy(() => unlisten?.());

  async function refreshCapabilities() {
    checkingEngine = true;
    localError = null;
    try {
      capabilities = await getRenderCapabilities();
    } catch (error) {
      capabilities = {
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
    } finally {
      checkingEngine = false;
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
    } catch (error) {
      localError = formatUnknownError(error);
    } finally {
      settingUpEngine = false;
    }
  }

  function formatUnknownError(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    try {
      return JSON.stringify(error);
    } catch {
      return "Bilinmeyen hata";
    }
  }

  async function chooseOutput() {
    if (!desktopRuntimeAvailable()) {
      localError = "Hedef dosya masaüstü uygulamasında seçilebilir.";
      return;
    }
    const selected = await save({
      title: "Videoyu dışa aktar",
      defaultPath: batchEnabled
        ? "astral-versiyonlar.mp4"
        : `astral-${preset.replace(":", "x")}.mp4`,
      filters: [{ name: "MPEG-4 Video", extensions: ["mp4"] }],
    });
    if (selected) outputPath = selected;
  }

  async function beginExport() {
    if (!canStart) return;
    localError = null;
    job = null;
    completedOutputPaths = [];
    unlisten?.();
    unlisten = null;
    cancelBatchRequested = false;
    const targets = [...requestedPresets];
    batchTotal = targets.length;
    batchIndex = 0;
    batchBusy = targets.length > 1;
    try {
      for (let index = 0; index < targets.length; index += 1) {
        if (cancelBatchRequested) break;
        const targetPreset = targets[index];
        batchIndex = index + 1;
        const targetPath = outputPathForPreset(
          outputPath,
          targetPreset,
          targets.length > 1,
        );
        const completed = await renderOnePreset(
          targetPreset,
          targetPath,
          targets.length === 1,
        );
        completedOutputPaths = [...completedOutputPaths, completed.outputPath];
        onCompleted?.(completed.outputPath);
      }
    } catch (error) {
      if (!cancelBatchRequested) localError = formatUnknownError(error);
    } finally {
      preparingAudio = false;
      batchBusy = false;
      stopListening();
    }
  }

  function stopListening() {
    const listener = unlisten;
    unlisten = null;
    listener?.();
  }

  async function renderOnePreset(
    targetPreset: ExportPreset,
    targetPath: string,
    overwrite: boolean,
  ): Promise<RenderJobSnapshot> {
    preparingAudio = Boolean(
      includeAudio &&
      (timelinesByPreset?.[targetPreset] ?? timeline)?.clips.some(
        (clip) => clip.noiseReduction,
      ),
    );
    return new Promise<RenderJobSnapshot>(async (resolve, reject) => {
      let localUnlisten: (() => void) | null = null;
      let terminalBeforeSubscription: RenderJobSnapshot | null = null;
      let settled = false;

      const settle = (snapshot: RenderJobSnapshot) => {
        if (settled || !["completed", "cancelled", "failed"].includes(snapshot.status)) return;
        settled = true;
        localUnlisten?.();
        if (unlisten === localUnlisten) unlisten = null;
        if (snapshot.status === "completed") resolve(snapshot);
        else if (snapshot.status === "cancelled") reject(new Error("Dışa aktarma iptal edildi."));
        else reject(snapshot.error ?? new Error("Dışa aktarma başarısız."));
      };

      try {
        const subscription = await startRenderWithListener({
          sourcePath: sourcePath ?? undefined,
          outputPath: targetPath,
          durationMs: Math.max(1, Math.round(durationMs)),
          preset: targetPreset,
          frameRate,
          // Auto Reframe matrices are generated from the compositor's contain
          // coordinate system; pre-cropping with cover would discard pixels
          // before the moving camera can reach them.
          fit: timelinesByPreset?.[targetPreset] ? "contain" : fit,
          quality,
          includeAudio,
          // A save dialog approves the exact single path only. Batch sibling
          // names must fail safely instead of overwriting unseen files.
          overwrite,
          timeline: timelinesByPreset?.[targetPreset] ?? timeline,
        }, (snapshot) => {
          job = snapshot;
          preparingAudio = false;
          if (["completed", "cancelled", "failed"].includes(snapshot.status)) {
            if (localUnlisten) settle(snapshot);
            else terminalBeforeSubscription = snapshot;
          }
        });
        localUnlisten = subscription.unlisten;
        unlisten = subscription.unlisten;
        job = subscription.job;
        preparingAudio = false;
        settle(terminalBeforeSubscription ?? subscription.job);
      } catch (error) {
        settled = true;
        preparingAudio = false;
        localUnlisten?.();
        if (unlisten === localUnlisten) unlisten = null;
        reject(error);
      }
    });
  }

  function outputPathForPreset(
    basePath: string,
    targetPreset: ExportPreset,
    multiple: boolean,
  ) {
    if (!multiple) return basePath;
    const separator = Math.max(basePath.lastIndexOf("/"), basePath.lastIndexOf("\\"));
    const extension = basePath.lastIndexOf(".");
    const suffix = `-${targetPreset.replace(":", "x")}`;
    if (extension > separator) {
      return `${basePath.slice(0, extension)}${suffix}${basePath.slice(extension)}`;
    }
    return `${basePath}${suffix}.mp4`;
  }

  async function requestCancel() {
    cancelBatchRequested = true;
    if (!job || !renderIsBusy) return;
    try {
      job = await cancelRender(job.jobId);
    } catch (error) {
      localError = formatUnknownError(error);
    }
  }

  function closeDialog() {
    if (isBusy) return;
    unlisten?.();
    unlisten = null;
    onClose();
  }

  function handleDialogKeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closeDialog();
      return;
    }
    if (event.key !== "Tab" || !dialogElement) return;
    const focusable = exportFocusableElements();
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (!active || !dialogElement.contains(active) || active === dialogElement) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    } else if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function exportFocusableElements(): HTMLElement[] {
    if (!dialogElement) return [];
    return Array.from(
      dialogElement.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((element) => element.offsetParent !== null);
  }

  function formatDuration(ms?: number) {
    if (ms === undefined || !Number.isFinite(ms)) return "—";
    const total = Math.max(0, Math.round(ms / 1000));
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor((total % 3600) / 60);
    const seconds = total % 60;
    return hours > 0
      ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
      : `${minutes}:${seconds.toString().padStart(2, "0")}`;
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
      aria-labelledby="export-title"
      bind:this={dialogElement}
    >
      <header>
        <div>
          <span class="eyebrow">Teslim</span>
          <h2 id="export-title">Videoyu dışa aktar</h2>
        </div>
        <button class="icon-button" onclick={closeDialog} disabled={isBusy} aria-label="Kapat">×</button>
      </header>

      <div class="content">
        <div class="section-label">Kadro</div>
        <div class="presets">
          {#each Object.entries(EXPORT_PRESETS) as [id, option]}
            <button
              class="preset"
              class:active={preset === id}
              onclick={() => (preset = id as ExportPreset)}
              disabled={isBusy}
            >
              <span class="ratio-shape ratio-{id.replace(':', '-')}"></span>
              <span><strong>{id}</strong><small>{option.label} · {option.detail}</small></span>
            </button>
          {/each}
        </div>

        <div class="settings-grid">
          <label>
            <span>Kalite</span>
            <select bind:value={quality} disabled={isBusy}>
              <option value="draft">Taslak · hızlı</option>
              <option value="standard">Standart</option>
              <option value="high">Yüksek · teslim</option>
            </select>
          </label>
          <label>
            <span>Kare hızı</span>
            <select bind:value={frameRate} disabled={isBusy}>
              <option value={24}>24 fps</option>
              <option value={25}>25 fps</option>
              <option value={30}>30 fps</option>
              <option value={50}>50 fps</option>
              <option value={60}>60 fps</option>
            </select>
          </label>
          <label>
            <span>Yerleşim</span>
            <select bind:value={fit} disabled={isBusy || autoReframeFitLocked}>
              <option value="contain">Sığdır · letterbox</option>
              <option value="cover">Doldur · kırp</option>
            </select>
            {#if autoReframeFitLocked}<small class="fit-note">AI kamera yolu için Sığdır kullanılır</small>{/if}
          </label>
          <label class="check-row">
            <input type="checkbox" bind:checked={includeAudio} disabled={isBusy} />
            <span>Sesi dahil et</span>
          </label>
        </div>

        {#if normalizedAutoReframePresets.length > 1}
          <label class="batch-row">
            <input type="checkbox" bind:checked={batchEnabled} disabled={isBusy} />
            <span>
              <strong>Tüm AI kadrajlarını üret</strong>
              <small>{normalizedAutoReframePresets.join(" · ")} sürümleri sırayla render edilir; AI yolu hazır kliplere uygulanır</small>
            </span>
          </label>
        {/if}

        <div class="section-label">Hedef</div>
        <button class="path-picker" onclick={chooseOutput} disabled={isBusy}>
          <span class:placeholder={!outputPath}>{outputPath || "MP4 hedefi seç…"}</span>
          <strong>Seç</strong>
        </button>

        {#if !sourcePath && !selectedTimeline?.clips.length}
          <div class="source-warning" role="status">Dışa aktarmak için zaman çizelgesine en az bir klip ekleyin.</div>
        {/if}

        <div class="engine-status" class:error={capabilities && !capabilities.available}>
          <span class="status-dot"></span>
          {#if checkingEngine}
            Render motoru kontrol ediliyor…
          {:else if settingUpEngine}
            {setupProgress?.message || "FFmpeg kuruluyor…"}
          {:else if capabilities?.available}
            FFmpeg hazır{capabilities.version ? ` · ${capabilities.version}` : ""}
          {:else}
            {capabilities?.diagnostic?.userMessage || "Render motoru bulunamadı"}
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
                  {job.status === "completed" ? "Dışa aktarma tamamlandı" :
                   job.status === "failed" ? "Dışa aktarma başarısız" :
                   job.status === "cancelled" ? "Dışa aktarma iptal edildi" :
                   job.status === "cancelling" ? "İptal ediliyor…" : "Video işleniyor"}
                </strong>
                <small>
                  {#if batchTotal > 1}{batchIndex}/{batchTotal} · {/if}{Math.round(job.progressPercent)}% · {formatDuration(job.encodedMs)} / {formatDuration(job.durationMs)}
                </small>
              </div>
              {#if job.status === "running"}
                <span class="eta">{formatDuration(job.etaMs)} kaldı</span>
              {/if}
            </div>
            <div class="progress-track" aria-label="Render ilerlemesi" aria-valuenow={job.progressPercent} role="progressbar">
              <div style="width: {Math.max(0, Math.min(100, job.progressPercent))}%"></div>
            </div>
            {#if job.status === "running"}
              <div class="metrics">
                <span>{job.frame ? `${job.frame} kare` : "Kare hazırlanıyor"}</span>
                <span>{job.encodingFps ? `${job.encodingFps.toFixed(1)} fps` : "—"}</span>
                <span>{job.speed ? `${job.speed.toFixed(2)}×` : "—"}</span>
              </div>
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

        {#if completedOutputPaths.length > 0 && batchTotal > 1 && !batchBusy}
          <div class="batch-complete" role="status">
            <strong>{completedOutputPaths.length} sürüm hazır</strong>
            <span>{completedOutputPaths.join("\n")}</span>
          </div>
        {/if}

        {#if localError}
          <div class="local-error" role="alert">{localError}</div>
        {/if}
      </div>

      <footer>
        <div class="summary">{EXPORT_PRESETS[preset].detail} · H.264/AAC · {formatDuration(durationMs)}</div>
        <div class="footer-actions">
          {#if preparingAudio}
            <button class="primary" disabled>AI temiz sesler hazırlanıyor…</button>
          {:else if renderIsBusy || batchBusy}
            <button class="secondary danger" onclick={requestCancel} disabled={job?.status === "cancelling"}>İptal et</button>
          {:else}
            <button class="secondary" onclick={closeDialog}>Kapat</button>
            <button class="primary" onclick={beginExport} disabled={!canStart}>Dışa aktar</button>
          {/if}
        </div>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1000; display: grid; place-items: center; padding: 24px; background: rgba(0, 0, 0, .72); backdrop-filter: blur(8px); }
  .dialog { width: min(720px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto; color: #e8e8e8; background: #121313; border: 1px solid #2b2c2c; border-radius: 14px; box-shadow: 0 28px 90px rgba(0,0,0,.58); }
  header, footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px 22px; }
  header { border-bottom: 1px solid #242525; }
  footer { border-top: 1px solid #242525; background: #101111; }
  h2 { margin: 2px 0 0; font-size: 19px; letter-spacing: -.02em; }
  .eyebrow, .section-label { color: #6f7473; font-size: 10px; font-weight: 700; letter-spacing: .12em; text-transform: uppercase; }
  .content { display: grid; gap: 14px; padding: 20px 22px; }
  button, select { font: inherit; }
  button { cursor: pointer; }
  button:disabled { cursor: not-allowed; opacity: .45; }
  .icon-button { width: 30px; height: 30px; color: #888; background: #1a1b1b; border: 1px solid #2b2c2c; border-radius: 7px; font-size: 20px; }
  .presets { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .preset { min-height: 74px; display: flex; align-items: center; gap: 12px; padding: 11px; text-align: left; color: #aaa; background: #181919; border: 1px solid #292a2a; border-radius: 9px; }
  .preset.active { color: #f1f5f4; border-color: #62d7b1; background: rgba(98, 215, 177, .08); box-shadow: inset 0 0 0 1px rgba(98,215,177,.15); }
  .preset strong, .preset small { display: block; }
  .preset strong { font-size: 13px; }
  .preset small { margin-top: 4px; color: #717675; font-size: 10px; }
  .ratio-shape { width: 27px; height: 27px; flex: 0 0 auto; border: 1.5px solid currentColor; border-radius: 2px; }
  .ratio-9-16 { width: 19px; height: 32px; }
  .ratio-16-9 { width: 34px; height: 19px; }
  .settings-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }
  label > span { display: block; margin-bottom: 6px; color: #8c908f; font-size: 11px; }
  select { width: 100%; padding: 9px 10px; color: #ddd; background: #191a1a; border: 1px solid #2a2c2b; border-radius: 7px; font-size: 12px; outline: none; }
  select:focus { border-color: #62d7b1; }
  .fit-note { display: block; margin-top: 4px; color: #6f8f84; font-size: 8px; }
  .check-row { display: flex; align-items: center; align-self: end; min-height: 35px; padding: 0 10px; background: #191a1a; border: 1px solid #2a2c2b; border-radius: 7px; }
  .check-row span { margin: 0 0 0 8px; }
  .check-row input { accent-color: #62d7b1; }
  .batch-row { display: flex; align-items: flex-start; gap: 10px; padding: 11px 12px; color: #b8c8c2; background: rgba(98,215,177,.07); border: 1px solid rgba(98,215,177,.2); border-radius: 8px; }
  .batch-row input { margin-top: 2px; accent-color: #62d7b1; }
  .batch-row span, .batch-row strong, .batch-row small { display: block; }
  .batch-row strong { font-size: 11px; }
  .batch-row small { margin-top: 3px; color: #77827e; font-size: 9px; }
  .path-picker { display: flex; justify-content: space-between; gap: 12px; width: 100%; padding: 10px 12px; color: #ccc; text-align: left; background: #191a1a; border: 1px solid #2a2c2b; border-radius: 7px; }
  .path-picker span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path-picker .placeholder { color: #656968; }
  .path-picker strong { color: #62d7b1; font-size: 11px; }
  .engine-status { display: flex; align-items: center; gap: 7px; min-height: 22px; color: #858b89; font-size: 11px; }
  .source-warning { margin-top: 8px; padding: 8px 10px; color: #c8a977; background: rgba(164,116,42,.1); border: 1px solid rgba(190,139,57,.24); border-radius: 6px; font-size: 10px; }
  .engine-status .status-dot { width: 7px; height: 7px; background: #62d7b1; border-radius: 50%; box-shadow: 0 0 10px rgba(98,215,177,.5); }
  .engine-status.error .status-dot { background: #df7f72; box-shadow: none; }
  .engine-status button { margin-left: auto; padding: 3px 7px; color: #ddd; background: transparent; border: 1px solid #343636; border-radius: 5px; }
  .setup-progress { height: 3px; margin-top: -10px; overflow: hidden; background: #292b2a; border-radius: 999px; }
  .setup-progress div { height: 100%; background: #62d7b1; transition: width .2s ease; }
  .job-card { padding: 13px; background: #181919; border: 1px solid #2a2c2b; border-radius: 9px; }
  .job-card[data-status="failed"] { border-color: rgba(223,127,114,.55); }
  .job-card[data-status="completed"] { border-color: rgba(98,215,177,.5); }
  .job-heading, .metrics { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .job-heading strong, .job-heading small { display: block; }
  .job-heading strong { font-size: 12px; }
  .job-heading small, .eta, .metrics { margin-top: 4px; color: #777c7a; font-size: 10px; }
  .progress-track { height: 5px; margin: 12px 0 8px; overflow: hidden; background: #292b2a; border-radius: 999px; }
  .progress-track div { height: 100%; background: linear-gradient(90deg, #55bfa0, #75e5bd); transition: width .2s ease; }
  details { margin-top: 12px; padding-top: 10px; border-top: 1px solid #2b2c2c; color: #c7867b; font-size: 11px; }
  details p { color: #aaa; }
  pre { max-height: 110px; overflow: auto; padding: 9px; color: #9b9f9e; background: #0d0e0e; border-radius: 5px; font-size: 9px; white-space: pre-wrap; }
  .local-error { padding: 9px 11px; color: #efafa5; background: rgba(194,76,61,.12); border: 1px solid rgba(194,76,61,.35); border-radius: 7px; font-size: 11px; }
  .batch-complete { display: grid; gap: 5px; padding: 10px 12px; color: #bfe9da; background: rgba(98,215,177,.08); border: 1px solid rgba(98,215,177,.25); border-radius: 7px; font-size: 10px; }
  .batch-complete span { color: #7e948c; white-space: pre-line; overflow-wrap: anywhere; }
  .summary { color: #737876; font-size: 10px; }
  .footer-actions { display: flex; gap: 8px; }
  .primary, .secondary { padding: 8px 14px; border-radius: 7px; font-size: 11px; font-weight: 650; }
  .primary { color: #0a1712; background: #62d7b1; border: 1px solid #74e9c2; }
  .secondary { color: #aaa; background: #191a1a; border: 1px solid #303232; }
  .danger { color: #f0a79b; border-color: rgba(223,127,114,.5); }
  @media (max-width: 620px) { .presets, .settings-grid { grid-template-columns: 1fr; } .summary { display: none; } }
</style>
