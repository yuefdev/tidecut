<script module lang="ts">
  export interface SmartShortsAnalyzeRequest {
    targetDurationMs: number;
    useGlm: boolean;
    apiKey?: string;
  }

  export type SmartShortsStreamerFacePosition = "top" | "bottom";

  export interface SmartShortsStreamerLayout {
    enabled: boolean;
    facePosition: SmartShortsStreamerFacePosition;
    faceFraction: number;
  }
</script>

<script lang="ts">
  import type { SmartShortsCandidate } from "$lib/editor/smart-shorts";

  interface Props {
    disabled?: boolean;
    busy?: boolean;
    progress?: number;
    progressStage?: string | null;
    progressElapsedMs?: number;
    progressRemainingMs?: number | null;
    progressOverdue?: boolean;
    cancelling?: boolean;
    message?: string | null;
    error?: boolean;
    candidates?: readonly SmartShortsCandidate[];
    selectedIds?: ReadonlySet<string>;
    fireworksConfigured?: boolean;
    glmUsed?: boolean;
    engineLabel?: string | null;
    streamerLayoutEnabled?: boolean;
    streamerFacePosition?: SmartShortsStreamerFacePosition;
    streamerFaceFraction?: number;
    streamerTargetReady?: boolean;
    facecamDetectBusy?: boolean;
    facecamDetectMessage?: string | null;
    facecamDetectError?: boolean;
    onAnalyze?: (request: SmartShortsAnalyzeRequest) => void | Promise<void>;
    onCancel?: () => void | Promise<void>;
    onCandidateToggle?: (id: string, selected: boolean) => void;
    onCandidateSeek?: (candidate: SmartShortsCandidate) => void;
    onApply?: () => void;
    onStreamerLayoutChange?: (layout: SmartShortsStreamerLayout) => void;
    onSelectStreamerTarget?: () => void;
    onAutoDetectFacecam?: () => void | Promise<void>;
  }

  let {
    disabled = false,
    busy = false,
    progress = 0,
    progressStage = null,
    progressElapsedMs = 0,
    progressRemainingMs = null,
    progressOverdue = false,
    cancelling = false,
    message = null,
    error = false,
    candidates = [],
    selectedIds = new Set<string>(),
    fireworksConfigured = false,
    glmUsed = false,
    engineLabel = null,
    streamerLayoutEnabled = false,
    streamerFacePosition = "top",
    streamerFaceFraction = 0.42,
    streamerTargetReady = false,
    facecamDetectBusy = false,
    facecamDetectMessage = null,
    facecamDetectError = false,
    onAnalyze,
    onCancel,
    onCandidateToggle,
    onCandidateSeek,
    onApply,
    onStreamerLayoutChange,
    onSelectStreamerTarget,
    onAutoDetectFacecam,
  }: Props = $props();

  let targetDurationMs = $state(30_000);
  let useGlm = $state(false);
  let apiKey = $state("");

  function unionDurationMs(input: readonly SmartShortsCandidate[]): number {
    const ranges = input
      .map((candidate) => ({ startMs: candidate.startMs, endMs: candidate.endMs }))
      .sort((left, right) => left.startMs - right.startMs);
    let total = 0;
    let cursorStart = -1;
    let cursorEnd = -1;
    for (const range of ranges) {
      if (range.startMs > cursorEnd) {
        if (cursorEnd > cursorStart) total += cursorEnd - cursorStart;
        cursorStart = range.startMs;
        cursorEnd = range.endMs;
      } else {
        cursorEnd = Math.max(cursorEnd, range.endMs);
      }
    }
    if (cursorEnd > cursorStart) total += cursorEnd - cursorStart;
    return total;
  }

  const selectedCount = $derived(
    candidates.filter((candidate) => selectedIds.has(candidate.id)).length,
  );
  const selectedDurationMs = $derived(
    unionDurationMs(candidates.filter((candidate) => selectedIds.has(candidate.id))),
  );
  const displayProgress = $derived(
    Math.floor(Math.max(0, Math.min(100, progress)) * 10) / 10,
  );
  const displayProgressText = $derived(formatPercent(displayProgress));
  const whisperBadge = $derived(
    engineLabel?.split(" · ")[0] || "Whisper · GPU/CPU otomatik",
  );
  const facePercent = $derived(Math.round(streamerFaceFraction * 100));
  const screenPercent = $derived(100 - facePercent);

  function formatTime(milliseconds: number): string {
    const totalSeconds = Math.max(0, Math.round(milliseconds / 1_000));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function formatPercent(value: number): string {
    const text = Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1);
    return text.replace(".", ",");
  }

  function formatEstimate(milliseconds: number): string {
    const seconds = Math.max(1, Math.ceil(milliseconds / 1_000));
    if (seconds < 60) return `${Math.max(5, Math.ceil(seconds / 5) * 5)} sn`;
    return `${Math.ceil(seconds / 60)} dk`;
  }

  function analyze() {
    void onAnalyze?.({
      targetDurationMs,
      useGlm,
      apiKey: useGlm ? apiKey.trim() || undefined : undefined,
    });
  }

  function cancel() {
    void onCancel?.();
  }

  function updateStreamerLayout(patch: Partial<SmartShortsStreamerLayout>) {
    onStreamerLayoutChange?.({
      enabled: patch.enabled ?? streamerLayoutEnabled,
      facePosition: patch.facePosition ?? streamerFacePosition,
      faceFraction: Math.max(
        0.35,
        Math.min(0.55, patch.faceFraction ?? streamerFaceFraction),
      ),
    });
  }
</script>

<section class="smart-shorts-workspace" aria-labelledby="smart-shorts-title">
  <header class="workspace-header">
    <span class="smart-shorts-icon" aria-hidden="true">✦</span>
    <span class="workspace-title">
      <strong id="smart-shorts-title">Akıllı Shorts</strong>
      <small>Bul, kadrajla, kontrol et ve tek akışa dönüştür</small>
    </span>
    <span class="local-badge">YEREL AI</span>
  </header>

  <ol class="workflow-strip" aria-label="Akıllı Shorts iş akışı">
    <li class:complete={candidates.length > 0}><b>1</b><span>Analiz</span></li>
    <li class:active={streamerLayoutEnabled}><b>2</b><span>Dikey düzen</span></li>
    <li class:active={candidates.length > 0}><b>3</b><span>İncele</span></li>
  </ol>

  <section class="workspace-section analysis-section" aria-labelledby="shorts-analysis-title">
    <div class="section-heading">
      <span class="step-number">1</span>
      <span>
        <strong id="shorts-analysis-title">Güçlü anları bul</strong>
        <small>Konuşma, ritim, ses enerjisi ve kahkaha ipuçlarını birlikte tara.</small>
      </span>
    </div>

    <div class="analysis-controls">
      <label>
        <span>Hedef klip süresi</span>
        <select bind:value={targetDurationMs} disabled={disabled || busy}>
          <option value={15_000}>15 saniye</option>
          <option value={30_000}>30 saniye</option>
          <option value={45_000}>45 saniye</option>
        </select>
      </label>
      <label class="glm-toggle">
        <input type="checkbox" bind:checked={useGlm} disabled={disabled || busy} />
        <span>
          <b>Fireworks GLM hakemi</b>
          <small>İsteğe bağlı son sıralama</small>
        </span>
      </label>
    </div>

    {#if useGlm && !fireworksConfigured}
      <label class="api-key-field">
        <span>Fireworks anahtarı · yalnız bu oturum</span>
        <input
          type="password"
          bind:value={apiKey}
          autocomplete="off"
          placeholder="fw_..."
          disabled={disabled || busy}
        />
      </label>
    {/if}

    <button
      type="button"
      class="analyze-button"
      onclick={analyze}
      disabled={disabled || busy || (useGlm && !fireworksConfigured && !apiKey.trim())}
    >
      <span aria-hidden="true">⌁</span>
      {busy ? `Analiz ediliyor · %${displayProgressText}` : candidates.length > 0 ? "Yeniden analiz et" : "Videodaki güçlü anları bul"}
    </button>

    <p class="first-run-note">
      <b>İlk kullanım:</b> Yönetilen Python/MediaPipe ortamı ve doğrulanmış YAMNet modeli
      indirilebilir. Kurulumdan sonra kahkaha taraması cihazda çalışır.
    </p>

    {#if busy}
      <div
        class="analysis-progress"
        role="progressbar"
        aria-label="Akıllı Shorts analiz ilerlemesi"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={displayProgress}
      >
        <span style:width={`${displayProgress}%`}></span>
      </div>
      <div class="analysis-progress-meta">
        <span class="progress-stage">
          <i aria-hidden="true"></i>
          {progressStage || "Analiz sürüyor"}
        </span>
        <span class:overdue={progressOverdue}>
          {#if progressOverdue}
            Tahmin uzadı · çalışıyor · {formatTime(progressElapsedMs)} geçti
          {:else if progressRemainingMs !== null}
            ≈ {formatEstimate(progressRemainingMs)} kaldı · {formatTime(progressElapsedMs)} geçti
          {:else}
            Çalışıyor · {formatTime(progressElapsedMs)} geçti
          {/if}
        </span>
      </div>
      <button
        type="button"
        class="cancel-button"
        onclick={cancel}
        disabled={cancelling}
      >
        {cancelling ? "İptal ediliyor…" : "Analizi iptal et"}
      </button>
    {/if}

    {#if message}
      <p class:error class="analysis-message" role={error ? "alert" : "status"}>{message}</p>
    {/if}

    <details class="engine-details">
      <summary>Analiz motorları <span>{whisperBadge}</span></summary>
      <div class="engine-row" aria-label="Akıllı Shorts motorları">
        <span>{whisperBadge}</span>
        <span>Silero VAD</span>
        <span>FFmpeg ses enerjisi</span>
        <span>Beat This</span>
        <span>YAMNet kahkaha · CPU</span>
        {#if glmUsed}<span class="cloud-active">GLM sıraladı</span>{/if}
      </div>
      {#if engineLabel}<p class="engine-label">{engineLabel}</p>{/if}
    </details>
  </section>

  <section class="workspace-section layout-section" aria-labelledby="streamer-layout-title">
    <div class="layout-heading">
      <div class="section-heading">
        <span class="step-number">2</span>
        <span>
          <strong id="streamer-layout-title">Dikey yayıncı düzeni</strong>
          <small>Kamera ile oyun/ekranı üst üste yerleştir.</small>
        </span>
      </div>
      <span class="aspect-badge">9:16</span>
    </div>

    <label class="layout-toggle">
      <span>
        <b>İkiye bölünmüş Shorts</b>
        <small>Yüz takibi bir alanda, ana içerik diğer alanda kalır.</small>
      </span>
      <input
        type="checkbox"
        role="switch"
        checked={streamerLayoutEnabled}
        disabled={disabled || busy}
        onchange={(event) => updateStreamerLayout({ enabled: event.currentTarget.checked })}
      />
    </label>

    {#if streamerLayoutEnabled}
      <div class="layout-editor">
        <div
          class="layout-preview"
          role="img"
          style={`--face-share: ${facePercent}%`}
          aria-label={`9:16 önizleme: kamera ${streamerFacePosition === "top" ? "üstte" : "altta"}`}
        >
          {#if streamerFacePosition === "top"}
            <div class="preview-face"><span>YÜZ + TAKİP</span></div>
            <div class="preview-divider"></div>
            <div class="preview-screen"><span>OYUN / EKRAN</span></div>
          {:else}
            <div class="preview-screen"><span>OYUN / EKRAN</span></div>
            <div class="preview-divider"></div>
            <div class="preview-face"><span>YÜZ + TAKİP</span></div>
          {/if}
        </div>

        <div class="layout-settings">
          <fieldset>
            <legend>Kamera konumu</legend>
            <div class="segmented-control">
              <button
                type="button"
                class:active={streamerFacePosition === "top"}
                aria-pressed={streamerFacePosition === "top"}
                disabled={disabled || busy}
                onclick={() => updateStreamerLayout({ facePosition: "top" })}
              >Üstte</button>
              <button
                type="button"
                class:active={streamerFacePosition === "bottom"}
                aria-pressed={streamerFacePosition === "bottom"}
                disabled={disabled || busy}
                onclick={() => updateStreamerLayout({ facePosition: "bottom" })}
              >Altta</button>
            </div>
          </fieldset>

          <label class="ratio-control">
            <span>Kamera %{facePercent} · ekran %{screenPercent}</span>
            <input
              type="range"
              min="0.35"
              max="0.55"
              step="0.01"
              value={streamerFaceFraction}
              disabled={disabled || busy}
              onchange={(event) =>
                updateStreamerLayout({ faceFraction: Number(event.currentTarget.value) })}
            />
          </label>

          <div class="ratio-presets" aria-label="Hazır bölme oranları">
            {#each [0.4, 0.45, 0.5] as fraction}
              <button
                type="button"
                class:active={Math.abs(streamerFaceFraction - fraction) < 0.005}
                aria-pressed={Math.abs(streamerFaceFraction - fraction) < 0.005}
                disabled={disabled || busy}
                onclick={() => updateStreamerLayout({ faceFraction: fraction })}
              >{Math.round(fraction * 100)}/{Math.round((1 - fraction) * 100)}</button>
            {/each}
          </div>

          <button
            type="button"
            class="auto-detect-button"
            disabled={disabled || busy || facecamDetectBusy}
            onclick={() => void onAutoDetectFacecam?.()}
          >
            <span aria-hidden="true">✨</span>
            {facecamDetectBusy ? "Facecam aranıyor…" : "Facecam'i otomatik bul"}
          </button>
          {#if facecamDetectMessage}
            <p
              class="target-state"
              class:ready={!facecamDetectError && !facecamDetectBusy}
              class:error={facecamDetectError}
              role={facecamDetectError ? "alert" : "status"}
            >{facecamDetectMessage}</p>
          {/if}
          <button
            type="button"
            class="target-button"
            class:ready={streamerTargetReady}
            disabled={disabled || busy || facecamDetectBusy}
            onclick={onSelectStreamerTarget}
          >
            <span aria-hidden="true">{streamerTargetReady ? "✓" : "◎"}</span>
            {streamerTargetReady ? "Takip hedefini değiştir" : "Elle seç (yedek)"}
          </button>
          <p class="target-state" class:ready={streamerTargetReady}>
            {streamerTargetReady
              ? "Hedef hazır; kamera alanı otomatik izlenecek."
              : "Otomatik bulma yüz bulamazsa facecam'i elle işaretleyebilirsiniz."}
          </p>
        </div>
      </div>
    {:else}
      <p class="layout-summary">Kapalıyken normal Akıllı Kadraj ile tek görüntülü 9:16, 1:1 ve 16:9 sürümleri kullanılır.</p>
    {/if}
  </section>

  <section class="workspace-section review-section" aria-labelledby="shorts-review-title">
    <div class="section-heading">
      <span class="step-number">3</span>
      <span>
        <strong id="shorts-review-title">Adayları kontrol et</strong>
        <small>Bir adaya tıklayarak dinle; istemediklerini seçimden çıkar.</small>
      </span>
    </div>

    {#if candidates.length > 0}
      <div class="candidate-heading">
        <strong>{candidates.length} aday bulundu</strong>
        <span>{selectedCount} seçili · {formatTime(selectedDurationMs)}</span>
      </div>
      <div class="candidate-list">
        {#each candidates as candidate, index (candidate.id)}
          <article class:selected={selectedIds.has(candidate.id)}>
            <label class="candidate-check" title="Adayı kullan">
              <input
                type="checkbox"
                checked={selectedIds.has(candidate.id)}
                disabled={disabled || busy}
                onchange={(event) =>
                  onCandidateToggle?.(candidate.id, event.currentTarget.checked)}
              />
              <span>#{index + 1}</span>
            </label>
            <button
              type="button"
              class="candidate-main"
              onclick={() => onCandidateSeek?.(candidate)}
              title="Bu adayı önizle"
            >
              <span class="candidate-meta">
                <b>%{candidate.score}</b>
                <span>{formatTime(candidate.startMs)}–{formatTime(candidate.endMs)}</span>
                <em>{Math.round((candidate.endMs - candidate.startMs) / 1_000)} sn</em>
              </span>
              <span class="candidate-reason">{candidate.reasons.join(" · ")}</span>
              <span class="candidate-transcript">{candidate.transcript}</span>
            </button>
          </article>
        {/each}
      </div>
      <div class="apply-footer">
        <span>{selectedCount > 0 ? `${formatTime(selectedDurationMs)} uzunluğunda yeni akış` : "En az bir aday seçin"}</span>
        <button
          type="button"
          class="apply-button"
          onclick={onApply}
          disabled={disabled || busy || selectedCount === 0}
        >Seçilileri tek highlight akışına dönüştür</button>
      </div>
      <p class="safety-note">Kesimler tek Ctrl+Z adımında uygulanır. Kaynak video silinmez.</p>
    {:else if !busy}
      <div class="empty-state">
        <span aria-hidden="true">⌁</span>
        <strong>Henüz aday yok</strong>
        <p>Analizi başlatın; Whisper metni, Silero konuşma yoğunluğu, kısa süreli ses yüksekliği ve ritim birlikte puanlanır. GLM açıksa yalnız zaman damgalı metin ve sinyaller gönderilir; video gönderilmez.</p>
      </div>
    {/if}
  </section>
</section>

<style>
  .smart-shorts-workspace {
    display: grid;
    gap: 10px;
    min-height: 0;
    padding: 10px;
    color: #cbd7d3;
    background:
      radial-gradient(circle at 85% 2%, rgba(64, 196, 167, 0.08), transparent 28%),
      #101312;
  }

  .workspace-header {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 4px 2px 2px;
  }
  .workspace-title { display: grid; gap: 2px; min-width: 0; }
  .workspace-title strong { color: #f0f8f5; font-size: 14px; letter-spacing: -0.01em; }
  .workspace-title small { color: #77847f; font-size: 11px; line-height: 1.4; }
  .smart-shorts-icon {
    display: grid;
    width: 36px;
    height: 36px;
    place-items: center;
    color: #092019;
    background: linear-gradient(145deg, #a4f4df, #4bcfaf);
    border-radius: 10px;
    box-shadow: 0 5px 16px rgba(40, 181, 148, 0.16);
    font-size: 17px;
  }
  .local-badge,
  .aspect-badge {
    padding: 4px 6px;
    color: #72dbc1;
    background: rgba(72, 196, 166, 0.09);
    border: 1px solid rgba(82, 210, 179, 0.25);
    border-radius: 5px;
    font: 850 9px/1 "JetBrains Mono", monospace;
  }

  .workflow-strip {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 4px;
    margin: 0;
    padding: 5px;
    list-style: none;
    background: rgba(255, 255, 255, 0.025);
    border: 1px solid #252b29;
    border-radius: 8px;
  }
  .workflow-strip li {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    min-width: 0;
    padding: 5px 3px;
    color: #69736f;
    border-radius: 5px;
    font-size: 10px;
  }
  .workflow-strip b {
    display: grid;
    width: 15px;
    height: 15px;
    place-items: center;
    color: #7d8884;
    background: #252b29;
    border-radius: 50%;
    font-size: 9px;
  }
  .workflow-strip li.active,
  .workflow-strip li.complete { color: #b9ded3; background: rgba(75, 207, 175, 0.06); }
  .workflow-strip li.active b,
  .workflow-strip li.complete b { color: #092019; background: #61d9bb; }

  .workspace-section {
    display: grid;
    gap: 10px;
    padding: 12px;
    background: linear-gradient(150deg, rgba(26, 32, 30, 0.98), rgba(18, 22, 21, 0.98));
    border: 1px solid #2b332f;
    border-radius: 10px;
  }
  .section-heading,
  .layout-heading { display: flex; align-items: center; gap: 8px; }
  .section-heading > span:last-child { display: grid; gap: 2px; min-width: 0; }
  .section-heading strong { color: #e6efeb; font-size: 12px; }
  .section-heading small { color: #76827d; font-size: 10px; line-height: 1.45; }
  .step-number {
    display: grid;
    flex: 0 0 22px;
    width: 22px;
    height: 22px;
    place-items: center;
    color: #87e2cc;
    background: rgba(82, 208, 177, 0.1);
    border: 1px solid rgba(82, 208, 177, 0.22);
    border-radius: 7px;
    font: 800 10px/1 "JetBrains Mono", monospace;
  }

  .analysis-controls { display: grid; grid-template-columns: minmax(0, .9fr) minmax(0, 1.35fr); gap: 7px; }
  .analysis-controls > label,
  .api-key-field { display: grid; gap: 5px; color: #88948f; font-size: 10px; }
  select,
  .api-key-field input {
    min-width: 0;
    height: 32px;
    padding: 0 8px;
    color: #dde7e3;
    background: #101513;
    border: 1px solid #303a36;
    border-radius: 6px;
    font: inherit;
  }
  .glm-toggle {
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    align-self: end;
    min-height: 32px;
    padding: 0 8px;
    background: #101513;
    border: 1px solid #303a36;
    border-radius: 6px;
  }
  .glm-toggle > span { display: grid; gap: 1px; }
  .glm-toggle b { color: #c9d4d0; font-size: 10px; }
  .glm-toggle small { color: #65716d; font-size: 9px; }
  .glm-toggle input,
  .layout-toggle input,
  .candidate-check input { accent-color: #55d6b6; }

  button { font-family: inherit; }
  :is(button, input, select, summary):focus-visible {
    outline: 2px solid #8be7d1;
    outline-offset: 2px;
  }
  .analyze-button,
  .apply-button {
    min-height: 34px;
    padding: 7px 10px;
    color: #092019;
    background: linear-gradient(145deg, #90e9d1, #4bcfaf);
    border: 0;
    border-radius: 7px;
    font-size: 10px;
    font-weight: 850;
    cursor: pointer;
  }
  .analyze-button { display: inline-flex; align-items: center; justify-content: center; gap: 6px; }
  .apply-button { color: #eff9f6; background: #327b69; }
  .cancel-button {
    width: 100%;
    min-height: 30px;
    margin-top: 2px;
    padding: 6px 10px;
    color: #f0b3a8;
    background: rgba(214, 96, 77, 0.12);
    border: 1px solid rgba(214, 96, 77, 0.42);
    border-radius: 7px;
    font-size: 10px;
    font-weight: 750;
    cursor: pointer;
    transition: background 160ms ease, border-color 160ms ease;
  }
  .cancel-button:hover:not(:disabled) {
    background: rgba(214, 96, 77, 0.2);
    border-color: rgba(214, 96, 77, 0.62);
  }
  button:disabled { cursor: not-allowed; opacity: 0.42; }

  .analysis-progress {
    height: 5px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.08);
    border-radius: 999px;
  }
  .analysis-progress span {
    position: relative;
    display: block;
    height: 100%;
    overflow: hidden;
    background: linear-gradient(90deg, #42d8c0, #a1f4e4);
    transition: width 260ms ease;
  }
  .analysis-progress span::after {
    position: absolute;
    inset: 0;
    content: "";
    background: linear-gradient(100deg, transparent 10%, rgba(255, 255, 255, .65) 48%, transparent 86%);
    transform: translateX(-120%);
    animation: progress-shimmer 1.35s linear infinite;
  }
  .analysis-progress-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    color: #7e8e88;
    font-size: 9px;
    line-height: 1.35;
  }
  .analysis-progress-meta > span:last-child {
    color: #849b94;
    text-align: right;
  }
  .analysis-progress-meta > span.overdue { color: #d4a96d; }
  .progress-stage {
    display: inline-flex;
    min-width: 0;
    align-items: center;
    gap: 6px;
    color: #a8bbb5;
  }
  .progress-stage i {
    flex: 0 0 6px;
    width: 6px;
    height: 6px;
    background: #61ddbf;
    border-radius: 50%;
    box-shadow: 0 0 0 0 rgba(97, 221, 191, .45);
    animation: progress-pulse 1.2s ease-out infinite;
  }
  .analysis-message { margin: 0; color: #91aaa3; font-size: 10px; line-height: 1.45; }
  .analysis-message.error { color: #ef9589; }
  .first-run-note {
    margin: -2px 0 0;
    color: #75827d;
    font-size: 10px;
    line-height: 1.45;
  }
  .first-run-note b { color: #a9bbb5; }

  .engine-details {
    padding: 0 8px;
    color: #75817d;
    background: rgba(5, 10, 9, 0.28);
    border: 1px solid #28302d;
    border-radius: 6px;
    font-size: 10px;
  }
  .engine-details summary { display: flex; justify-content: space-between; gap: 8px; padding: 7px 0; cursor: pointer; }
  .engine-details summary span { overflow: hidden; color: #56635e; text-overflow: ellipsis; white-space: nowrap; }
  .engine-row { display: flex; flex-wrap: wrap; gap: 4px; padding: 0 0 8px; }
  .engine-row span { padding: 3px 5px; color: #82918d; background: rgba(255, 255, 255, 0.035); border-radius: 4px; font-size: 9px; }
  .engine-row .cloud-active { color: #a7b8ff; background: rgba(96, 112, 230, 0.12); }
  .engine-label { margin: -3px 0 8px; color: #596762; font: 9px/1.45 "JetBrains Mono", monospace; }

  .layout-heading { justify-content: space-between; }
  .layout-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px;
    background: #111715;
    border: 1px solid #303a36;
    border-radius: 7px;
  }
  .layout-toggle > span { display: grid; gap: 2px; }
  .layout-toggle b { color: #d7e2de; font-size: 11px; }
  .layout-toggle small { color: #71807a; font-size: 10px; line-height: 1.45; }
  .layout-toggle input { width: 28px; height: 16px; }
  .layout-editor { display: grid; grid-template-columns: 90px minmax(0, 1fr); align-items: start; gap: 11px; }
  .layout-preview {
    display: flex;
    flex-direction: column;
    aspect-ratio: 9 / 16;
    overflow: hidden;
    background: #080b0a;
    border: 1px solid #42504b;
    border-radius: 8px;
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.2);
  }
  .preview-face,
  .preview-screen { display: grid; min-height: 0; place-items: center; }
  .preview-face { flex: 0 0 var(--face-share); color: #a5ead7; background: radial-gradient(circle at 50% 35%, #41665b 0 17%, #213a33 18% 31%, #16231f 32%); }
  .preview-screen { flex: 1 1 auto; color: #96a7d6; background: linear-gradient(145deg, #26314b, #171d2d 60%, #2c253d); }
  .preview-divider { flex: 0 0 3px; background: #66dbbc; box-shadow: 0 0 8px rgba(102, 219, 188, .42); }
  .layout-preview span { padding: 3px; background: rgba(5, 9, 8, .55); border-radius: 3px; font: 750 8px/1 "JetBrains Mono", monospace; }
  .layout-settings { display: grid; gap: 8px; min-width: 0; }
  fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
  legend,
  .ratio-control > span { margin-bottom: 5px; color: #87938e; font-size: 10px; }
  .segmented-control { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); padding: 2px; background: #0d1210; border: 1px solid #303a36; border-radius: 6px; }
  .segmented-control button,
  .ratio-presets button {
    min-height: 27px;
    color: #79847f;
    background: transparent;
    border: 0;
    border-radius: 4px;
    font-size: 10px;
    cursor: pointer;
  }
  .segmented-control button.active,
  .ratio-presets button.active { color: #d8f1e9; background: rgba(81, 211, 178, .15); }
  .ratio-control { display: grid; }
  .ratio-control input { width: 100%; accent-color: #55d6b6; }
  .ratio-presets { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; }
  .ratio-presets button { border: 1px solid #303a36; }
  .target-button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 31px;
    color: #b9c5c0;
    background: #161d1a;
    border: 1px solid #39443f;
    border-radius: 6px;
    font-size: 10px;
    font-weight: 750;
    cursor: pointer;
  }
  .target-button.ready { color: #9de5d2; border-color: rgba(82, 208, 177, .38); }
  .auto-detect-button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 33px;
    color: #092019;
    background: linear-gradient(145deg, #90e9d1, #4bcfaf);
    border: 0;
    border-radius: 6px;
    font-size: 10px;
    font-weight: 850;
    cursor: pointer;
  }
  .target-state,
  .layout-summary { margin: 0; color: #6f7c77; font-size: 10px; line-height: 1.45; }
  .target-state.ready { color: #75aa9c; }
  .target-state.error { color: #ef9589; }

  .candidate-heading { display: flex; justify-content: space-between; gap: 8px; font-size: 10px; }
  .candidate-heading strong { color: #dce9e5; }
  .candidate-heading span { color: #6f7f7a; }
  .candidate-list { display: grid; gap: 6px; max-height: min(42vh, 380px); overflow: auto; }
  article {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr);
    overflow: hidden;
    background: rgba(8, 14, 14, 0.62);
    border: 1px solid #2b3531;
    border-radius: 7px;
  }
  article.selected { border-color: rgba(79, 215, 189, 0.52); box-shadow: inset 3px 0 rgba(79, 215, 189, .48); }
  .candidate-check {
    display: grid;
    place-content: center;
    gap: 4px;
    color: #729088;
    font: 800 9px/1 "JetBrains Mono", monospace;
    border-right: 1px solid #2b3531;
  }
  .candidate-main {
    display: grid;
    gap: 4px;
    min-width: 0;
    padding: 8px;
    color: inherit;
    text-align: left;
    background: transparent;
    border: 0;
    cursor: pointer;
  }
  .candidate-main:hover { background: rgba(73, 211, 185, 0.06); }
  .candidate-meta { display: flex; align-items: center; gap: 6px; font-size: 10px; }
  .candidate-meta b { color: #69e4cb; font-size: 12px; }
  .candidate-meta span { color: #b6c5c1; }
  .candidate-meta em { color: #66736f; font-style: normal; }
  .candidate-reason { color: #80a69c; font-size: 9px; }
  .candidate-transcript {
    display: -webkit-box;
    overflow: hidden;
    color: #929f9b;
    font-size: 10px;
    line-height: 1.4;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .apply-footer { display: grid; grid-template-columns: minmax(0, .7fr) minmax(0, 1.3fr); align-items: center; gap: 8px; }
  .apply-footer > span { color: #788680; font-size: 10px; line-height: 1.4; }
  .safety-note { margin: 0; color: #788f88; font-size: 9px; line-height: 1.45; }
  .empty-state { display: grid; justify-items: center; gap: 5px; padding: 16px 10px; color: #6c7974; text-align: center; background: rgba(4, 8, 7, .24); border: 1px dashed #303936; border-radius: 7px; }
  .empty-state > span { color: #55cdae; font-size: 18px; }
  .empty-state strong { color: #9eaaa5; font-size: 11px; }
  .empty-state p { max-width: 300px; margin: 0; font-size: 10px; line-height: 1.5; }

  @keyframes progress-shimmer {
    to { transform: translateX(120%); }
  }

  @keyframes progress-pulse {
    70%, 100% { box-shadow: 0 0 0 6px rgba(97, 221, 191, 0); }
  }

  @media (prefers-reduced-motion: reduce) {
    .analysis-progress span::after,
    .progress-stage i { animation: none; }
  }

  @container (max-width: 300px) {
    .analysis-controls,
    .layout-editor,
    .apply-footer { grid-template-columns: 1fr; }
    .layout-preview { width: 76px; }
    .workspace-title small,
    .workflow-strip span { display: none; }
  }
</style>
