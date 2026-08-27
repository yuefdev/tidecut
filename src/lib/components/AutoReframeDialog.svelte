<script module lang="ts">
  export type AutoReframeAspect = "9:16" | "1:1" | "16:9";
  export type AutoReframeStyle = "calm" | "natural" | "dynamic";
  export type AutoReframeFraming = "auto" | "close" | "medium" | "full" | "object";
  export type AutoReframeAnalysisStatus = "idle" | "preparing" | "analyzing" | "completed" | "error";

  export interface NormalizedBoundingBox {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  export interface AutoReframeRequest {
    targetBox: NormalizedBoundingBox;
    aspects: AutoReframeAspect[];
    style: AutoReframeStyle;
    framing: AutoReframeFraming;
    source: {
      width: number;
      height: number;
      durationMs: number;
      seedTimeMs: number;
    };
  }

  export interface AutoReframeResultSummary {
    shotCount: number;
    keyframeCount: number;
    lowConfidenceCount: number;
  }

  export interface AutoReframeApplyPayload {
    request: AutoReframeRequest;
    result: AutoReframeResultSummary | null;
  }

  export interface AutoReframeDialogProps {
    open: boolean;
    clipName?: string;
    seedFrameUrl: string;
    sourceWidth: number;
    sourceHeight: number;
    durationMs: number;
    seedTimeMs?: number;
    targetBox?: NormalizedBoundingBox | null;
    analysisStatus?: AutoReframeAnalysisStatus;
    analysisProgress?: number;
    analysisMessage?: string | null;
    analysisError?: string | null;
    result?: AutoReframeResultSummary | null;
    busy?: boolean;
    onTargetBoxChange?: (box: NormalizedBoundingBox | null) => void;
    onAnalyze: (request: AutoReframeRequest) => void | Promise<void>;
    onApply: (payload: AutoReframeApplyPayload) => void | Promise<void>;
    onCancel: () => void | Promise<void>;
  }
</script>

<script lang="ts">
  type WizardStep = "target" | "setup" | "analysis";

  interface DragState {
    pointerId: number;
    startX: number;
    startY: number;
    previousTarget: NormalizedBoundingBox | null;
  }

  const ASPECTS: Array<{
    id: AutoReframeAspect;
    label: string;
    detail: string;
    shape: string;
  }> = [
    { id: "9:16", label: "Dikey", detail: "Reels · Shorts · TikTok", shape: "portrait" },
    { id: "1:1", label: "Kare", detail: "Akış gönderisi", shape: "square" },
    { id: "16:9", label: "Yatay", detail: "YouTube · ekran", shape: "landscape" },
  ];

  const STYLES: Array<{
    id: AutoReframeStyle;
    label: string;
    detail: string;
  }> = [
    { id: "calm", label: "Sakin", detail: "Az hareket, uzun planlar" },
    { id: "natural", label: "Doğal", detail: "Dengeli takip ve zoom" },
    { id: "dynamic", label: "Dinamik", detail: "Vurgulu pan ve yakınlaşma" },
  ];

  const FRAMINGS: Array<{
    id: AutoReframeFraming;
    label: string;
    detail: string;
  }> = [
    { id: "auto", label: "Otomatik", detail: "Plana göre karar ver" },
    { id: "close", label: "Yakın", detail: "Yüz ve omuz" },
    { id: "medium", label: "Orta", detail: "Bel üstü" },
    { id: "full", label: "Tam boy", detail: "Kişinin tamamı" },
    { id: "object", label: "Nesne", detail: "Seçili nesneyi doldur" },
  ];

  let {
    open,
    clipName = "Seçili video klibi",
    seedFrameUrl,
    sourceWidth,
    sourceHeight,
    durationMs,
    seedTimeMs = 0,
    targetBox = null,
    analysisStatus = "idle",
    analysisProgress = 0,
    analysisMessage = null,
    analysisError = null,
    result = null,
    busy = false,
    onTargetBoxChange,
    onAnalyze,
    onApply,
    onCancel,
  }: AutoReframeDialogProps = $props();

  let step = $state<WizardStep>("target");
  let selectedAspects = $state<Record<AutoReframeAspect, boolean>>({
    "9:16": true,
    "1:1": true,
    "16:9": true,
  });
  let framingStyle = $state<AutoReframeStyle>("natural");
  let subjectFraming = $state<AutoReframeFraming>("auto");
  let currentTarget = $state<NormalizedBoundingBox | null>(null);
  let dragState = $state<DragState | null>(null);
  let submittingAnalysis = $state(false);
  let applying = $state(false);
  let closing = $state(false);
  let actionError = $state<string | null>(null);
  let imageFailed = $state(false);
  let dialogElement = $state<HTMLElement>();
  let frameElement = $state<HTMLElement>();
  let previouslyFocused: HTMLElement | null = null;
  let wasOpen = false;
  let lastExternalBoxSignature = "";
  let lastSeedFrameUrl = "";

  let selectedAspectIds = $derived(
    ASPECTS.filter((aspect) => selectedAspects[aspect.id]).map((aspect) => aspect.id),
  );
  let externalAnalysisBusy = $derived(
    analysisStatus === "preparing" || analysisStatus === "analyzing",
  );
  let isBusy = $derived(
    busy || externalAnalysisBusy || submittingAnalysis || applying || closing,
  );
  let normalizedProgress = $derived(clamp(analysisProgress, 0, 100));
  let displayedError = $derived(actionError ?? analysisError);
  let previewAspect = $derived(`${Math.max(1, sourceWidth)} / ${Math.max(1, sourceHeight)}`);
  let previewStyle = $derived.by(() => {
    const ratio = Math.max(1, sourceWidth) / Math.max(1, sourceHeight);
    return `aspect-ratio:${previewAspect};width:min(100%,min(${ratio * 50}vh,${ratio * 430}px))`;
  });
  let targetOverlayStyle = $derived(
    currentTarget
      ? `left:${currentTarget.x * 100}%;top:${currentTarget.y * 100}%;width:${currentTarget.width * 100}%;height:${currentTarget.height * 100}%`
      : "",
  );
  let targetDescription = $derived(
    currentTarget
      ? `Hedef seçildi: görüntünün soldan yüzde ${Math.round(currentTarget.x * 100)}, üstten yüzde ${Math.round(currentTarget.y * 100)} konumunda; genişlik yüzde ${Math.round(currentTarget.width * 100)}, yükseklik yüzde ${Math.round(currentTarget.height * 100)}.`
      : "Henüz hedef seçilmedi.",
  );

  $effect(() => {
    const signature = boxSignature(targetBox);
    if (signature === lastExternalBoxSignature) return;
    lastExternalBoxSignature = signature;
    currentTarget = normalizeBox(targetBox);
  });

  $effect(() => {
    if (seedFrameUrl === lastSeedFrameUrl) return;
    lastSeedFrameUrl = seedFrameUrl;
    imageFailed = false;
  });

  $effect(() => {
    if (open && !wasOpen) {
      wasOpen = true;
      previouslyFocused = document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
      step = analysisStatus === "idle" ? "target" : "analysis";
      currentTarget = normalizeBox(targetBox);
      actionError = null;
      queueMicrotask(() => focusableElements()[0]?.focus() ?? dialogElement?.focus());
    } else if (!open) {
      const focusTarget = previouslyFocused;
      previouslyFocused = null;
      wasOpen = false;
      dragState = null;
      actionError = null;
      queueMicrotask(() => focusTarget?.focus());
    }
  });

  $effect(() => {
    if (open && analysisStatus !== "idle") step = "analysis";
  });

  function clamp(value: number, min: number, max: number): number {
    if (!Number.isFinite(value)) return min;
    return Math.min(max, Math.max(min, value));
  }

  function normalizeBox(box: NormalizedBoundingBox | null | undefined): NormalizedBoundingBox | null {
    if (!box || box.width <= 0 || box.height <= 0) return null;
    const x = clamp(box.x, 0, 1);
    const y = clamp(box.y, 0, 1);
    const width = clamp(box.width, 0, 1 - x);
    const height = clamp(box.height, 0, 1 - y);
    if (width <= 0 || height <= 0) return null;
    return {
      x,
      y,
      width,
      height,
    };
  }

  function boxSignature(box: NormalizedBoundingBox | null | undefined): string {
    const normalized = normalizeBox(box);
    if (!normalized) return "none";
    return [normalized.x, normalized.y, normalized.width, normalized.height]
      .map((value) => value.toFixed(6))
      .join(":");
  }

  function setTarget(box: NormalizedBoundingBox | null, notify = true) {
    currentTarget = normalizeBox(box);
    if (notify) onTargetBoxChange?.(currentTarget ? { ...currentTarget } : null);
  }

  function pointInFrame(event: PointerEvent): { x: number; y: number } | null {
    if (!frameElement) return null;
    const bounds = frameElement.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) return null;
    return {
      x: clamp((event.clientX - bounds.left) / bounds.width, 0, 1),
      y: clamp((event.clientY - bounds.top) / bounds.height, 0, 1),
    };
  }

  function beginTargetDrag(event: PointerEvent) {
    if (isBusy || step !== "target" || (event.pointerType === "mouse" && event.button !== 0)) return;
    const frame = frameElement;
    if (!frame) return;
    const point = pointInFrame(event);
    if (!point) return;
    event.preventDefault();
    frame.focus();
    frame.setPointerCapture?.(event.pointerId);
    dragState = {
      pointerId: event.pointerId,
      startX: point.x,
      startY: point.y,
      previousTarget: currentTarget ? { ...currentTarget } : null,
    };
    currentTarget = {
      x: point.x,
      y: point.y,
      width: 0.001,
      height: 0.001,
    };
  }

  function updateTargetDrag(event: PointerEvent) {
    if (!dragState || dragState.pointerId !== event.pointerId) return;
    const point = pointInFrame(event);
    if (!point) return;
    event.preventDefault();
    currentTarget = {
      x: Math.min(dragState.startX, point.x),
      y: Math.min(dragState.startY, point.y),
      width: Math.abs(point.x - dragState.startX),
      height: Math.abs(point.y - dragState.startY),
    };
  }

  function finishTargetDrag(event: PointerEvent) {
    if (!dragState || dragState.pointerId !== event.pointerId) return;
    const point = pointInFrame(event);
    const origin = dragState;
    dragState = null;
    frameElement?.releasePointerCapture?.(event.pointerId);

    if (!point) {
      setTarget(null);
      return;
    }

    const width = Math.abs(point.x - origin.startX);
    const height = Math.abs(point.y - origin.startY);
    if (width < 0.015 || height < 0.015) {
      setTarget(centeredTarget(point.x, point.y));
      return;
    }

    setTarget({
      x: Math.min(origin.startX, point.x),
      y: Math.min(origin.startY, point.y),
      width,
      height,
    });
  }

  function cancelTargetDrag(event: PointerEvent) {
    if (!dragState || dragState.pointerId !== event.pointerId) return;
    const previousTarget = dragState.previousTarget;
    dragState = null;
    frameElement?.releasePointerCapture?.(event.pointerId);
    setTarget(previousTarget);
  }

  function centeredTarget(centerX = 0.5, centerY = 0.46): NormalizedBoundingBox {
    const width = subjectFraming === "object" ? 0.32 : 0.24;
    const height = subjectFraming === "object" ? 0.32 : 0.46;
    return {
      x: clamp(centerX - width / 2, 0, 1 - width),
      y: clamp(centerY - height / 2, 0, 1 - height),
      width,
      height,
    };
  }

  function handleFrameKeydown(event: KeyboardEvent) {
    if (isBusy) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setTarget(centeredTarget());
      return;
    }
    if ((event.key === "Delete" || event.key === "Backspace") && currentTarget) {
      event.preventDefault();
      setTarget(null);
      return;
    }
    if (!currentTarget || !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
      return;
    }
    event.preventDefault();
    const increment = event.shiftKey ? 0.03 : 0.01;
    const xDelta = event.key === "ArrowLeft" ? -increment : event.key === "ArrowRight" ? increment : 0;
    const yDelta = event.key === "ArrowUp" ? -increment : event.key === "ArrowDown" ? increment : 0;
    setTarget({
      ...currentTarget,
      x: clamp(currentTarget.x + xDelta, 0, 1 - currentTarget.width),
      y: clamp(currentTarget.y + yDelta, 0, 1 - currentTarget.height),
    });
  }

  function toggleAspect(aspect: AutoReframeAspect) {
    if (isBusy) return;
    selectedAspects[aspect] = !selectedAspects[aspect];
  }

  function buildRequest(): AutoReframeRequest | null {
    if (!currentTarget || selectedAspectIds.length === 0) return null;
    return {
      targetBox: { ...currentTarget },
      aspects: [...selectedAspectIds],
      style: framingStyle,
      framing: subjectFraming,
      source: {
        width: Math.max(1, Math.round(sourceWidth)),
        height: Math.max(1, Math.round(sourceHeight)),
        durationMs: Math.max(1, Math.round(durationMs)),
        seedTimeMs: clamp(Math.round(seedTimeMs), 0, Math.max(0, Math.round(durationMs))),
      },
    };
  }

  async function startAnalysis() {
    if (isBusy) return;
    const request = buildRequest();
    if (!request) return;
    step = "analysis";
    actionError = null;
    submittingAnalysis = true;
    try {
      await onAnalyze(request);
    } catch (error) {
      actionError = formatUnknownError(error);
    } finally {
      submittingAnalysis = false;
    }
  }

  async function applyReframe() {
    if (isBusy || analysisStatus !== "completed") return;
    const request = buildRequest();
    if (!request) return;
    actionError = null;
    applying = true;
    try {
      await onApply({ request, result });
    } catch (error) {
      actionError = formatUnknownError(error);
    } finally {
      applying = false;
    }
  }

  async function requestClose() {
    if (closing) return;
    actionError = null;
    closing = true;
    try {
      await onCancel();
    } catch (error) {
      actionError = formatUnknownError(error);
    } finally {
      closing = false;
    }
  }

  function goToStep(nextStep: WizardStep) {
    if (isBusy) return;
    if (nextStep === "setup" && !currentTarget) return;
    step = nextStep;
    actionError = null;
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void requestClose();
      return;
    }
    if (event.key !== "Tab" || !dialogElement) return;

    const focusable = focusableElements();
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

  function focusableElements(): HTMLElement[] {
    if (!dialogElement) return [];
    return Array.from(
      dialogElement.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((element) => element.offsetParent !== null);
  }

  function formatUnknownError(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    return "İşlem tamamlanamadı. Lütfen yeniden deneyin.";
  }

  function formatDuration(milliseconds: number): string {
    const totalSeconds = Math.max(0, Math.round(milliseconds / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    return hours > 0
      ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
      : `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function statusTitle(): string {
    if (submittingAnalysis || analysisStatus === "preparing") return "Yerel AI hazırlanıyor";
    if (analysisStatus === "analyzing") return "Kişi ve kamera hareketi analiz ediliyor";
    if (analysisStatus === "completed") return "Akıllı kadraj hazır";
    if (analysisStatus === "error" || displayedError) return "Analiz tamamlanamadı";
    return "Analize hazır";
  }

  function statusMessage(): string {
    if (analysisMessage) return analysisMessage;
    if (submittingAnalysis || analysisStatus === "preparing") {
      return "Model ve video kareleri hazırlanıyor. İlk kullanım biraz daha uzun sürebilir.";
    }
    if (analysisStatus === "analyzing") {
      return "Seçtiğiniz hedef takip ediliyor; pan ve yakınlaşmalar yumuşatılıyor.";
    }
    if (analysisStatus === "completed") {
      return "Her oran için ayrı, düzenlenebilir bir kamera yolu oluşturuldu.";
    }
    return "Video cihazınızda işlenecek; görüntü buluta gönderilmeyecek.";
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if open}
  <div class="backdrop" data-testid="auto-reframe-backdrop">
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="auto-reframe-title"
      aria-describedby="auto-reframe-description"
      tabindex="-1"
      bind:this={dialogElement}
    >
      <header class="dialog-header">
        <div class="title-block">
          <span class="eyebrow">YEREL AI · AKILLI KADRAJ</span>
          <h2 id="auto-reframe-title">Kişiyi veya nesneyi takip et</h2>
          <p id="auto-reframe-description">Tek kurgudan her ekran için doğal bir kamera hareketi oluştur.</p>
        </div>
        <button
          class="icon-button"
          type="button"
          aria-label="Akıllı kadraj penceresini kapat"
          disabled={closing}
          onclick={() => void requestClose()}
        >×</button>
      </header>

      <nav class="steps" aria-label="Akıllı kadraj adımları">
        <ol>
          <li class:active={step === "target"} class:complete={step !== "target"} aria-current={step === "target" ? "step" : undefined}>
            <span>1</span><div><strong>Hedef</strong><small>Kişiyi seç</small></div>
          </li>
          <li class:active={step === "setup"} class:complete={step === "analysis"} aria-current={step === "setup" ? "step" : undefined}>
            <span>2</span><div><strong>Kadraj</strong><small>Çıktıları ayarla</small></div>
          </li>
          <li class:active={step === "analysis"} aria-current={step === "analysis" ? "step" : undefined}>
            <span>3</span><div><strong>Analiz</strong><small>Kontrol et</small></div>
          </li>
        </ol>
      </nav>

      <main class="content">
        {#if step === "target"}
          <section class="step-panel" aria-labelledby="target-step-title">
            <div class="section-heading">
              <div>
                <span class="section-kicker">1 · TAKİP HEDEFİ</span>
                <h3 id="target-step-title">Takip edilecek kişiyi veya nesneyi çerçeveleyin</h3>
                <p>Karenin üzerinde sürükleyerek bir kutu çizin. Tek tıklama hızlı bir merkez kutusu ekler.</p>
              </div>
              <div class="source-chip" title={clipName}>
                <span class="source-dot"></span>
                <div><strong>{clipName}</strong><small>{sourceWidth}×{sourceHeight} · Kare {formatDuration(seedTimeMs)} · Video {formatDuration(durationMs)}</small></div>
              </div>
            </div>

            <div
              class="frame-shell"
              style={previewStyle}
              role="button"
              tabindex="0"
              aria-label="Takip hedefi seçim alanı. Fareyle kutu çizin; klavyeyle Enter tuşu merkez seçim ekler, ok tuşları seçimi taşır, Delete tuşu temizler."
              aria-describedby="target-selection-status target-selection-help"
              onpointerdown={beginTargetDrag}
              onpointermove={updateTargetDrag}
              onpointerup={finishTargetDrag}
              onpointercancel={cancelTargetDrag}
              onkeydown={handleFrameKeydown}
              bind:this={frameElement}
            >
              {#if seedFrameUrl && !imageFailed}
                <img
                  src={seedFrameUrl}
                  alt=""
                  draggable="false"
                  onerror={() => (imageFailed = true)}
                />
              {:else}
                <div class="frame-placeholder" role="status">
                  <span>▧</span>
                  <strong>Önizleme karesi yüklenemedi</strong>
                  <small>Hedef seçimi için klipten bir kare oluşturun.</small>
                </div>
              {/if}
              <div class="frame-vignette"></div>
              <div class="thirds thirds-x"></div>
              <div class="thirds thirds-y"></div>
              {#if currentTarget}
                <div class="target-box" style={targetOverlayStyle} aria-hidden="true">
                  <span class="corner corner-tl"></span>
                  <span class="corner corner-tr"></span>
                  <span class="corner corner-bl"></span>
                  <span class="corner corner-br"></span>
                  <span class="target-tag">TAKİP HEDEFİ</span>
                </div>
              {/if}
            </div>

            <div class="target-toolbar">
              <div>
                <span id="target-selection-status" class:ready={currentTarget}>{targetDescription}</span>
                <small id="target-selection-help">İpucu: Kutuyu hedefin çevresinde biraz boşluk bırakarak çizin.</small>
              </div>
              <div class="target-actions">
                <button type="button" class="quiet-button" onclick={() => setTarget(centeredTarget())} disabled={isBusy}>Ortaya seç</button>
                <button type="button" class="quiet-button" onclick={() => setTarget(null)} disabled={isBusy || !currentTarget}>Temizle</button>
              </div>
            </div>
          </section>
        {:else if step === "setup"}
          <section class="step-panel setup-panel" aria-labelledby="setup-step-title">
            <div class="section-heading compact">
              <div>
                <span class="section-kicker">2 · KADRAJ AYARLARI</span>
                <h3 id="setup-step-title">Hangi versiyonlar oluşturulsun?</h3>
                <p>Her oran için ayrı kamera yolu hesaplanır; kurgu ve ses aynı kalır.</p>
              </div>
              <div class="privacy-badge"><span>✓</span> Tamamen cihazınızda</div>
            </div>

            <fieldset class="setting-group aspect-group">
              <legend>Çıktı oranları</legend>
              <div class="aspect-grid">
                {#each ASPECTS as aspect}
                  <label class="aspect-card" class:selected={selectedAspects[aspect.id]}>
                    <input
                      type="checkbox"
                      checked={selectedAspects[aspect.id]}
                      onchange={() => toggleAspect(aspect.id)}
                      disabled={isBusy}
                    />
                    <span class="ratio-shape {aspect.shape}"></span>
                    <span class="option-copy"><strong>{aspect.id} · {aspect.label}</strong><small>{aspect.detail}</small></span>
                    <span class="checkmark" aria-hidden="true">✓</span>
                  </label>
                {/each}
              </div>
              {#if selectedAspectIds.length === 0}
                <p class="field-error" role="alert">En az bir çıktı oranı seçin.</p>
              {/if}
            </fieldset>

            <fieldset class="setting-group">
              <legend>Kamera hareketi</legend>
              <div class="choice-grid style-grid">
                {#each STYLES as styleOption}
                  <label class="choice-card" class:selected={framingStyle === styleOption.id}>
                    <input type="radio" name="reframe-style" value={styleOption.id} bind:group={framingStyle} disabled={isBusy} />
                    <strong>{styleOption.label}</strong>
                    <small>{styleOption.detail}</small>
                  </label>
                {/each}
              </div>
            </fieldset>

            <fieldset class="setting-group">
              <legend>Kişi / nesne kadrajı</legend>
              <div class="choice-grid framing-grid">
                {#each FRAMINGS as framingOption}
                  <label class="choice-card framing-card" class:selected={subjectFraming === framingOption.id}>
                    <input type="radio" name="subject-framing" value={framingOption.id} bind:group={subjectFraming} disabled={isBusy} />
                    <strong>{framingOption.label}</strong>
                    <small>{framingOption.detail}</small>
                  </label>
                {/each}
              </div>
            </fieldset>

            <aside class="analysis-note">
              <span class="analysis-note-icon">✦</span>
              <div><strong>Ücretsiz yerel analiz</strong><p>Hedef takibi cihazınızda çalışır. İlk kurulum internet isteyebilir; ücretli API gerekmez.</p></div>
            </aside>
          </section>
        {:else}
          <section class="step-panel analysis-panel" aria-labelledby="analysis-step-title" aria-live="polite">
            <div class="analysis-hero" class:success={analysisStatus === "completed"} class:failed={analysisStatus === "error" || Boolean(displayedError)}>
              <div class="analysis-orbit" class:spinning={externalAnalysisBusy || submittingAnalysis}>
                <span>{analysisStatus === "completed" ? "✓" : analysisStatus === "error" || displayedError ? "!" : "✦"}</span>
              </div>
              <div>
                <span class="section-kicker">3 · YEREL ANALİZ</span>
                <h3 id="analysis-step-title">{statusTitle()}</h3>
                <p>{statusMessage()}</p>
              </div>
              {#if externalAnalysisBusy || submittingAnalysis}
                <strong class="progress-number">%{Math.round(normalizedProgress)}</strong>
              {/if}
            </div>

            {#if externalAnalysisBusy || submittingAnalysis}
              <div class="progress-card">
                <div
                  class="progress-track"
                  role="progressbar"
                  aria-label="Akıllı kadraj analiz ilerlemesi"
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-valuenow={normalizedProgress}
                ><div style={`width:${normalizedProgress}%`}></div></div>
                <div class="progress-meta"><span>Hedef takip ediliyor</span><span>{selectedAspectIds.join(" · ")}</span></div>
              </div>
            {:else if analysisStatus === "completed"}
              <div class="result-grid" aria-label="Analiz özeti">
                <div class="result-card"><span>KLİP</span><strong>{result?.shotCount ?? 0}</strong><small>analiz planı</small></div>
                <div class="result-card"><span>KAMERA</span><strong>{result?.keyframeCount ?? 0}</strong><small>hareket noktası</small></div>
                <div class="result-card confidence" class:warning={(result?.lowConfidenceCount ?? 0) > 0}>
                  <span>DÜŞÜK GÜVEN</span><strong>{result?.lowConfidenceCount ?? 0}</strong><small>{(result?.lowConfidenceCount ?? 0) > 0 ? "kontrol önerilir" : "sorun bulunmadı"}</small>
                </div>
              </div>

              <div class="ready-summary">
                <div class="preview-stack" aria-hidden="true">
                  {#each selectedAspectIds as aspect, index}
                    <span class:portrait={aspect === "9:16"} class:square={aspect === "1:1"} class:landscape={aspect === "16:9"} style={`--stack-index:${index}`}></span>
                  {/each}
                </div>
                <div><strong>{selectedAspectIds.length} versiyon zaman çizelgesine hazır</strong><p>Uyguladıktan sonra kamera hareketlerini keyframe olarak düzenleyebilirsiniz.</p></div>
              </div>
            {:else if analysisStatus === "error" || displayedError}
              <div class="error-card" role="alert">
                <strong>Analiz sırasında bir sorun oluştu</strong>
                <p>{displayedError || "Yerel analiz tamamlanamadı. Ayarları kontrol edip yeniden deneyin."}</p>
              </div>
            {:else}
              <div class="ready-card">
                <div><span>✓</span><strong>Hedef seçildi</strong></div>
                <div><span>✓</span><strong>{selectedAspectIds.length} çıktı oranı hazır</strong></div>
                <div><span>✓</span><strong>{STYLES.find((item) => item.id === framingStyle)?.label} hareket</strong></div>
              </div>
            {/if}
          </section>
        {/if}

        {#if actionError && step !== "analysis"}
          <div class="error-card compact-error" role="alert">{actionError}</div>
        {/if}
      </main>

      <footer class="dialog-footer">
        <div class="footer-summary">
          {#if step === "target"}
            <span class:ready={currentTarget}></span>{currentTarget ? "Hedef hazır" : "Bir hedef seçin"}
          {:else if step === "setup"}
            {selectedAspectIds.length} oran · {STYLES.find((item) => item.id === framingStyle)?.label} · {FRAMINGS.find((item) => item.id === subjectFraming)?.label}
          {:else if externalAnalysisBusy || submittingAnalysis}
            Pencereyi kapatırsanız analiz arka planda sürer
          {:else if analysisStatus === "completed"}
            {result?.lowConfidenceCount ?? 0} düşük güvenli bölüm
          {:else}
            Görüntüler cihazınızdan çıkmaz
          {/if}
        </div>
        <div class="footer-actions">
          <button type="button" class="secondary" onclick={() => void requestClose()} disabled={closing}>{externalAnalysisBusy || submittingAnalysis ? "Arka planda sürdür" : "Vazgeç"}</button>
          {#if step === "target"}
            <button type="button" class="primary" onclick={() => goToStep("setup")} disabled={!currentTarget || isBusy}>Kadrajı ayarla <span>→</span></button>
          {:else if step === "setup"}
            <button type="button" class="secondary" onclick={() => goToStep("target")} disabled={isBusy}>Geri</button>
            <button type="button" class="primary" onclick={() => void startAnalysis()} disabled={!currentTarget || selectedAspectIds.length === 0 || isBusy}>Yerel analizi başlat <span>✦</span></button>
          {:else if externalAnalysisBusy || submittingAnalysis}
            <button type="button" class="primary busy-button" disabled><span class="mini-spinner"></span> Analiz sürüyor</button>
          {:else if analysisStatus === "completed"}
            <button type="button" class="secondary" onclick={() => goToStep("setup")} disabled={isBusy}>Ayarlar</button>
            <button type="button" class="primary" onclick={() => void applyReframe()} disabled={isBusy}>Zaman çizelgesine uygula <span>✓</span></button>
          {:else if analysisStatus === "error" || displayedError}
            <button type="button" class="secondary" onclick={() => goToStep("setup")} disabled={isBusy}>Ayarları değiştir</button>
            <button type="button" class="primary" onclick={() => void startAnalysis()} disabled={!currentTarget || selectedAspectIds.length === 0 || isBusy}>Yeniden dene</button>
          {:else}
            <button type="button" class="secondary" onclick={() => goToStep("setup")} disabled={isBusy}>Geri</button>
            <button type="button" class="primary" onclick={() => void startAnalysis()} disabled={!currentTarget || selectedAspectIds.length === 0 || isBusy}>Analizi başlat <span>✦</span></button>
          {/if}
        </div>
      </footer>
    </div>
  </div>
{/if}

<style>
  .dialog, .dialog * { box-sizing: border-box; }
  .backdrop { position: fixed; inset: 0; z-index: 1020; display: grid; place-items: center; padding: 22px; background: rgba(2, 4, 4, .8); backdrop-filter: blur(12px); }
  .dialog { width: min(860px, calc(100vw - 32px)); max-height: calc(100vh - 44px); display: flex; flex-direction: column; overflow: hidden; color: #e9efed; background: radial-gradient(circle at 18% -20%, rgba(70, 162, 130, .14), transparent 40%), #111313; border: 1px solid #2b302e; border-radius: 16px; box-shadow: 0 34px 110px rgba(0, 0, 0, .68); outline: none; }
  .dialog:focus-visible { border-color: rgba(98, 215, 177, .58); }
  button, input { font: inherit; }
  button { cursor: pointer; }
  button:disabled, input:disabled { cursor: not-allowed; opacity: .45; }
  .dialog-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 18px; padding: 18px 22px 16px; border-bottom: 1px solid #252927; }
  .title-block { min-width: 0; }
  .eyebrow, .section-kicker { color: #66c9a7; font-size: 9px; font-weight: 800; letter-spacing: .14em; }
  h2 { margin: 4px 0 0; font-size: 20px; letter-spacing: -.025em; }
  .title-block p { margin: 5px 0 0; color: #747c79; font-size: 11px; }
  .icon-button { width: 32px; height: 32px; flex: 0 0 auto; display: grid; place-items: center; padding: 0 0 2px; color: #8b918f; background: #191c1b; border: 1px solid #2c312f; border-radius: 8px; font-size: 20px; line-height: 1; }
  .icon-button:hover:not(:disabled) { color: #e7edeb; border-color: #48504d; }
  .steps { padding: 0 22px; background: rgba(12, 14, 14, .7); border-bottom: 1px solid #232725; }
  .steps ol { display: grid; grid-template-columns: repeat(3, 1fr); max-width: 610px; padding: 0; margin: 0 auto; list-style: none; }
  .steps li { position: relative; display: flex; align-items: center; gap: 9px; padding: 11px 14px; color: #555d5a; }
  .steps li:not(:last-child)::after { position: absolute; top: 50%; right: -5px; width: 22px; height: 1px; content: ""; background: #303532; }
  .steps li > span { width: 24px; height: 24px; flex: 0 0 auto; display: grid; place-items: center; border: 1px solid #333936; border-radius: 50%; font-size: 9px; font-weight: 800; }
  .steps strong, .steps small { display: block; }
  .steps strong { color: #777f7c; font-size: 10px; }
  .steps small { margin-top: 2px; font-size: 8px; }
  .steps li.active { color: #7ee0be; }
  .steps li.active > span { color: #10251e; background: #6cd8b3; border-color: #7fe8c4; box-shadow: 0 0 16px rgba(98, 215, 177, .2); }
  .steps li.active strong { color: #d9e8e3; }
  .steps li.complete > span { color: #75d7b7; border-color: rgba(98, 215, 177, .55); }
  .content { min-height: 0; flex: 1; overflow: auto; padding: 20px 22px; }
  .step-panel { display: grid; gap: 15px; max-width: 780px; margin: 0 auto; }
  .section-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 18px; }
  .section-heading.compact { align-items: center; }
  h3 { margin: 4px 0 0; color: #e5ece9; font-size: 15px; letter-spacing: -.015em; }
  .section-heading p, .analysis-hero p { margin: 5px 0 0; color: #747d79; font-size: 10px; line-height: 1.45; }
  .source-chip { max-width: 260px; display: flex; align-items: center; gap: 8px; padding: 8px 10px; background: #171a19; border: 1px solid #292e2c; border-radius: 8px; }
  .source-dot { width: 7px; height: 7px; flex: 0 0 auto; background: #62d7b1; border-radius: 50%; box-shadow: 0 0 10px rgba(98, 215, 177, .45); }
  .source-chip div { min-width: 0; }
  .source-chip strong, .source-chip small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .source-chip strong { color: #bcc6c2; font-size: 9px; }
  .source-chip small { margin-top: 3px; color: #69716e; font-size: 8px; }
  .frame-shell { position: relative; max-width: 100%; max-height: min(50vh, 430px); margin: 0 auto; overflow: hidden; touch-action: none; user-select: none; background: #080a09; border: 1px solid #333a37; border-radius: 11px; box-shadow: inset 0 0 0 1px rgba(255,255,255,.02), 0 12px 34px rgba(0,0,0,.25); outline: none; cursor: crosshair; }
  .frame-shell:focus-visible { border-color: #62d7b1; box-shadow: 0 0 0 3px rgba(98, 215, 177, .16); }
  .frame-shell img { width: 100%; height: 100%; display: block; pointer-events: none; object-fit: fill; }
  .frame-placeholder { position: absolute; inset: 0; display: grid; align-content: center; justify-items: center; gap: 4px; color: #636b68; background: linear-gradient(135deg, #101311, #171b19); }
  .frame-placeholder span { color: #4f8b76; font-size: 28px; }
  .frame-placeholder strong { font-size: 11px; }
  .frame-placeholder small { font-size: 9px; }
  .frame-vignette { position: absolute; inset: 0; pointer-events: none; box-shadow: inset 0 0 50px rgba(0,0,0,.3); }
  .thirds { position: absolute; inset: 0; pointer-events: none; opacity: .13; }
  .thirds-x { background: linear-gradient(90deg, transparent calc(33.33% - .5px), #fff 33.33%, transparent calc(33.33% + .5px), transparent calc(66.66% - .5px), #fff 66.66%, transparent calc(66.66% + .5px)); }
  .thirds-y { background: linear-gradient(0deg, transparent calc(33.33% - .5px), #fff 33.33%, transparent calc(33.33% + .5px), transparent calc(66.66% - .5px), #fff 66.66%, transparent calc(66.66% + .5px)); }
  .target-box { position: absolute; min-width: 2px; min-height: 2px; pointer-events: none; background: rgba(98, 215, 177, .1); border: 1px solid #78e8c2; box-shadow: 0 0 0 9999px rgba(0,0,0,.32), 0 0 22px rgba(73, 205, 160, .25), inset 0 0 20px rgba(77, 210, 164, .08); }
  .target-tag { position: absolute; top: -20px; left: -1px; padding: 3px 6px; color: #092018; background: #72dfba; border-radius: 4px 4px 4px 0; font-size: 7px; font-weight: 900; letter-spacing: .08em; white-space: nowrap; }
  .corner { position: absolute; width: 11px; height: 11px; border-color: #d5fff1; }
  .corner-tl { top: -2px; left: -2px; border-top: 2px solid; border-left: 2px solid; }
  .corner-tr { top: -2px; right: -2px; border-top: 2px solid; border-right: 2px solid; }
  .corner-bl { bottom: -2px; left: -2px; border-bottom: 2px solid; border-left: 2px solid; }
  .corner-br { right: -2px; bottom: -2px; border-right: 2px solid; border-bottom: 2px solid; }
  .target-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .target-toolbar > div:first-child { min-width: 0; }
  .target-toolbar span, .target-toolbar small { display: block; }
  .target-toolbar span { color: #89918e; font-size: 9px; }
  .target-toolbar span.ready { color: #79dcbc; }
  .target-toolbar small { margin-top: 4px; color: #5f6764; font-size: 8px; }
  .target-actions { flex: 0 0 auto; display: flex; gap: 7px; }
  .quiet-button { padding: 6px 8px; color: #939b98; background: #171a19; border: 1px solid #2c322f; border-radius: 6px; font-size: 9px; }
  .quiet-button:hover:not(:disabled) { color: #d9e5e1; border-color: #45504c; }
  .privacy-badge { display: flex; align-items: center; gap: 6px; padding: 7px 9px; color: #79c9ae; background: rgba(58, 126, 103, .1); border: 1px solid rgba(98, 215, 177, .2); border-radius: 999px; font-size: 8px; white-space: nowrap; }
  .privacy-badge span { font-weight: 800; }
  .setting-group { min-width: 0; padding: 0; margin: 0; border: 0; }
  .setting-group legend { margin-bottom: 8px; color: #8c9591; font-size: 9px; font-weight: 750; }
  .aspect-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
  .aspect-card { position: relative; min-height: 68px; display: flex; align-items: center; gap: 11px; padding: 10px 12px; background: #171a19; border: 1px solid #292e2c; border-radius: 9px; cursor: pointer; transition: .15s ease; }
  .aspect-card:hover { border-color: #3b4642; transform: translateY(-1px); }
  .aspect-card.selected { background: linear-gradient(135deg, rgba(98,215,177,.11), #171a19 65%); border-color: rgba(98, 215, 177, .52); box-shadow: inset 0 0 0 1px rgba(98,215,177,.06); }
  .aspect-card input { position: absolute; width: 1px; height: 1px; overflow: hidden; opacity: 0; }
  .aspect-card:focus-within { outline: 2px solid rgba(98, 215, 177, .48); outline-offset: 2px; }
  .ratio-shape { width: 22px; height: 22px; flex: 0 0 auto; border: 1.5px solid #707976; border-radius: 2px; }
  .aspect-card.selected .ratio-shape { border-color: #7ae1bd; box-shadow: inset 0 0 8px rgba(98,215,177,.09); }
  .ratio-shape.portrait { width: 16px; height: 28px; margin: 0 3px; }
  .ratio-shape.landscape { width: 30px; height: 17px; margin: 0 -4px; }
  .option-copy { min-width: 0; }
  .option-copy strong, .option-copy small { display: block; }
  .option-copy strong { color: #c8d1ce; font-size: 10px; }
  .option-copy small { margin-top: 4px; overflow: hidden; color: #656e6a; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .checkmark { position: absolute; top: 7px; right: 8px; width: 15px; height: 15px; display: grid; place-items: center; color: #0c2019; background: #69d5b0; border-radius: 50%; font-size: 8px; font-weight: 900; opacity: 0; transform: scale(.7); transition: .15s; }
  .aspect-card.selected .checkmark { opacity: 1; transform: scale(1); }
  .field-error { margin: 6px 0 0; color: #e69b8f; font-size: 8px; }
  .choice-grid { display: grid; gap: 7px; }
  .style-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); }
  .framing-grid { grid-template-columns: repeat(5, minmax(0, 1fr)); }
  .choice-card { position: relative; display: grid; gap: 3px; padding: 9px 10px; background: #161918; border: 1px solid #282d2b; border-radius: 8px; cursor: pointer; }
  .choice-card:hover { border-color: #3b4440; }
  .choice-card.selected { background: rgba(98, 215, 177, .07); border-color: rgba(98, 215, 177, .48); }
  .choice-card input { position: absolute; width: 1px; height: 1px; opacity: 0; }
  .choice-card:focus-within { outline: 2px solid rgba(98,215,177,.48); outline-offset: 2px; }
  .choice-card strong { color: #b9c2bf; font-size: 9px; }
  .choice-card small { color: #626b67; font-size: 8px; }
  .choice-card.selected strong { color: #d9ede6; }
  .analysis-note { display: flex; align-items: flex-start; gap: 10px; padding: 10px 12px; color: #8ba39a; background: linear-gradient(90deg, rgba(60, 130, 107, .11), rgba(60,130,107,.03)); border: 1px solid rgba(98, 215, 177, .16); border-radius: 8px; }
  .analysis-note-icon { color: #6dd9b4; font-size: 15px; }
  .analysis-note strong { display: block; color: #bcd3ca; font-size: 9px; }
  .analysis-note p { margin: 3px 0 0; font-size: 8px; line-height: 1.4; }
  .analysis-panel { gap: 14px; }
  .analysis-hero { display: flex; align-items: center; gap: 14px; min-height: 86px; padding: 15px 16px; background: radial-gradient(circle at 8% 50%, rgba(98,215,177,.11), transparent 25%), #161918; border: 1px solid #2a302d; border-radius: 11px; }
  .analysis-hero > div:nth-child(2) { min-width: 0; flex: 1; }
  .analysis-hero.success { border-color: rgba(98, 215, 177, .36); }
  .analysis-hero.failed { border-color: rgba(218, 111, 97, .36); background: radial-gradient(circle at 8% 50%, rgba(198,82,68,.1), transparent 25%), #191716; }
  .analysis-orbit { width: 45px; height: 45px; flex: 0 0 auto; display: grid; place-items: center; color: #77e1bd; border: 1px solid rgba(98,215,177,.5); border-radius: 50%; box-shadow: inset 0 0 17px rgba(98,215,177,.08), 0 0 20px rgba(98,215,177,.1); }
  .analysis-orbit.spinning { border-style: dashed; animation: orbit 2.4s linear infinite; }
  .analysis-orbit.spinning span { animation: orbit 2.4s linear infinite reverse; }
  .analysis-hero.failed .analysis-orbit { color: #e28e81; border-color: rgba(226, 125, 111, .5); }
  .progress-number { color: #82dcbc; font-size: 17px; letter-spacing: -.04em; }
  .progress-card { padding: 12px; background: #151817; border: 1px solid #272c2a; border-radius: 9px; }
  .progress-track { height: 6px; overflow: hidden; background: #252b28; border-radius: 999px; }
  .progress-track div { height: 100%; background: linear-gradient(90deg, #52ba98, #78e4be); border-radius: inherit; box-shadow: 0 0 16px rgba(98,215,177,.3); transition: width .2s ease; }
  .progress-meta { display: flex; justify-content: space-between; gap: 12px; margin-top: 7px; color: #68716d; font-size: 8px; }
  .result-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .result-card { display: grid; grid-template-columns: 1fr auto; align-items: end; gap: 2px 8px; padding: 12px; background: #161918; border: 1px solid #292f2c; border-radius: 9px; }
  .result-card > span { color: #69736f; font-size: 7px; font-weight: 800; letter-spacing: .11em; }
  .result-card strong { grid-row: 1 / span 2; grid-column: 2; color: #d9e8e2; font-size: 22px; line-height: 1; }
  .result-card small { color: #66706c; font-size: 8px; }
  .result-card.confidence { border-color: rgba(98,215,177,.32); }
  .result-card.confidence strong { color: #71d8b4; }
  .result-card.confidence.warning { border-color: rgba(217, 159, 79, .4); background: rgba(117,80,29,.08); }
  .result-card.confidence.warning strong { color: #e3b268; }
  .ready-summary { display: flex; align-items: center; gap: 15px; padding: 13px 14px; background: linear-gradient(90deg, rgba(98,215,177,.09), rgba(98,215,177,.02)); border: 1px solid rgba(98,215,177,.18); border-radius: 9px; }
  .preview-stack { position: relative; width: 57px; height: 40px; flex: 0 0 auto; }
  .preview-stack span { position: absolute; top: calc(var(--stack-index) * 4px); left: calc(var(--stack-index) * 15px); display: block; background: rgba(98,215,177,.07); border: 1px solid #70d8b5; border-radius: 2px; box-shadow: 3px 3px 8px rgba(0,0,0,.3); }
  .preview-stack span.portrait { width: 15px; height: 27px; }
  .preview-stack span.square { width: 24px; height: 24px; }
  .preview-stack span.landscape { width: 32px; height: 18px; }
  .ready-summary strong { display: block; color: #cde2da; font-size: 10px; }
  .ready-summary p { margin: 4px 0 0; color: #71807a; font-size: 8px; }
  .error-card { padding: 11px 13px; color: #e8a69c; background: rgba(160, 61, 48, .1); border: 1px solid rgba(218, 111, 97, .32); border-radius: 8px; }
  .error-card strong { display: block; font-size: 10px; }
  .error-card p { margin: 4px 0 0; color: #af7f78; font-size: 9px; line-height: 1.45; }
  .compact-error { max-width: 780px; margin: 12px auto 0; font-size: 9px; }
  .ready-card { display: grid; grid-template-columns: repeat(3, 1fr); gap: 7px; }
  .ready-card div { display: flex; align-items: center; gap: 7px; padding: 10px; color: #8c9793; background: #151817; border: 1px solid #272c2a; border-radius: 8px; font-size: 9px; }
  .ready-card span { color: #71d8b4; }
  .dialog-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 13px 22px; background: #0e1010; border-top: 1px solid #252927; }
  .footer-summary { min-width: 0; display: flex; align-items: center; gap: 7px; overflow: hidden; color: #68716e; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .footer-summary > span { width: 6px; height: 6px; flex: 0 0 auto; background: #4b524f; border-radius: 50%; }
  .footer-summary > span.ready { background: #62d7b1; box-shadow: 0 0 8px rgba(98,215,177,.4); }
  .footer-actions { flex: 0 0 auto; display: flex; gap: 7px; }
  .primary, .secondary { min-height: 32px; padding: 7px 12px; border-radius: 7px; font-size: 9px; font-weight: 750; }
  .secondary { color: #9ca4a1; background: #171a19; border: 1px solid #2e3431; }
  .secondary:hover:not(:disabled) { color: #e0e8e5; border-color: #46504c; }
  .primary { display: inline-flex; align-items: center; justify-content: center; gap: 7px; color: #0a1d16; background: linear-gradient(180deg, #77e2bd, #5ecba6); border: 1px solid #82ebc7; box-shadow: 0 4px 14px rgba(69, 174, 138, .15); }
  .primary:hover:not(:disabled) { filter: brightness(1.06); transform: translateY(-1px); }
  .busy-button { min-width: 118px; opacity: .75 !important; }
  .mini-spinner { width: 11px; height: 11px; border: 1.5px solid rgba(8,28,21,.35); border-top-color: #0a1d16; border-radius: 50%; animation: orbit .8s linear infinite; }
  @keyframes orbit { to { transform: rotate(360deg); } }
  @media (max-width: 720px) {
    .dialog { max-height: calc(100vh - 20px); }
    .backdrop { padding: 10px; }
    .dialog-header, .content, .dialog-footer { padding-right: 15px; padding-left: 15px; }
    .steps { padding: 0 8px; }
    .steps li { padding: 9px 6px; }
    .steps small { display: none; }
    .section-heading { display: grid; }
    .source-chip { max-width: 100%; }
    .aspect-grid, .style-grid, .result-grid, .ready-card { grid-template-columns: 1fr; }
    .framing-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .target-toolbar, .dialog-footer { align-items: stretch; flex-direction: column; }
    .target-actions, .footer-actions { justify-content: flex-end; }
    .footer-summary { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .analysis-orbit.spinning, .analysis-orbit.spinning span, .mini-spinner { animation-duration: 4s; }
    .aspect-card, .primary { transition: none; transform: none !important; }
  }
</style>
