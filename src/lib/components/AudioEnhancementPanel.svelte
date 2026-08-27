<script lang="ts">
  import type { SpeechSuggestion, BeatCutMode } from "$lib/editor/audio-ai";
  import type {
    AutoDuckingSettings,
    AutoDuckingDiagnostics,
  } from "$lib/editor/audio-ducking";

  export interface AudioAiTrackOption {
    id: string;
    label: string;
    selected: boolean;
  }

  export interface BeatUiAnalysis {
    bpm: number | null;
    beatCount: number;
    downbeatCount: number;
    model: string;
    cacheHit: boolean;
  }

  interface Props {
    disabled?: boolean;
    speechBusy?: boolean;
    speechMessage?: string | null;
    speechError?: boolean;
    speechSuggestions?: readonly SpeechSuggestion[];
    selectedSuggestionIds?: ReadonlySet<string>;
    onSpeechAnalyze?: () => void;
    onSpeechSuggestionToggle?: (id: string, selected: boolean) => void;
    onSpeechSuggestionSeek?: (suggestion: SpeechSuggestion) => void;
    onSpeechSuggestionsApply?: () => void;

    ducking?: AutoDuckingSettings | null;
    duckingActive?: boolean;
    duckingDiagnostics?: AutoDuckingDiagnostics | null;
    duckingTracks?: readonly AudioAiTrackOption[];
    duckingBusy?: boolean;
    duckingMessage?: string | null;
    duckingError?: boolean;
    onDuckingTrackToggle?: (id: string, selected: boolean) => void;
    onDuckingChange?: (updates: Partial<AutoDuckingSettings>) => void;
    onDuckingApply?: () => void;
    onDuckingRemove?: () => void;

    beatAnalysis?: BeatUiAnalysis | null;
    beatBusy?: boolean;
    beatMessage?: string | null;
    beatError?: boolean;
    beatCutMode?: BeatCutMode;
    onBeatAnalyze?: () => void;
    onBeatCutModeChange?: (mode: BeatCutMode) => void;
    onBeatCutsApply?: () => void;
    onBeatRemove?: () => void;
  }

  let {
    disabled = false,
    speechBusy = false,
    speechMessage = null,
    speechError = false,
    speechSuggestions = [],
    selectedSuggestionIds = new Set<string>(),
    onSpeechAnalyze,
    onSpeechSuggestionToggle,
    onSpeechSuggestionSeek,
    onSpeechSuggestionsApply,
    ducking = null,
    duckingActive = false,
    duckingDiagnostics = null,
    duckingTracks = [],
    duckingBusy = false,
    duckingMessage = null,
    duckingError = false,
    onDuckingTrackToggle,
    onDuckingChange,
    onDuckingApply,
    onDuckingRemove,
    beatAnalysis = null,
    beatBusy = false,
    beatMessage = null,
    beatError = false,
    beatCutMode = "smart",
    onBeatAnalyze,
    onBeatCutModeChange,
    onBeatCutsApply,
    onBeatRemove,
  }: Props = $props();

  let selectedSuggestionCount = $derived(
    speechSuggestions.filter((suggestion) => selectedSuggestionIds.has(suggestion.id)).length,
  );

  function timecode(milliseconds: number): string {
    const total = Math.max(0, milliseconds) / 1_000;
    const minutes = Math.floor(total / 60);
    const seconds = total - minutes * 60;
    return `${minutes}:${seconds.toFixed(2).padStart(5, "0")}`;
  }

  function suggestionLabel(suggestion: SpeechSuggestion): string {
    if (suggestion.kind === "silence") return "Sessizlik";
    if (suggestion.kind === "possible-filler-sound") return "Olası dolgu sesi";
    if (suggestion.kind === "contextual-word") return `Bağlama bağlı · ${suggestion.text}`;
    return `Dolgu · ${suggestion.text ?? "ses"}`;
  }

  function numberFrom(event: Event): number {
    return Number((event.currentTarget as HTMLInputElement).value);
  }
</script>

<section class="audio-ai" aria-labelledby="audio-ai-title">
  <div class="title-row">
    <div>
      <span class="ai-chip">AI · CİHAZDA</span>
      <h4 id="audio-ai-title">Ses iyileştirme paketi</h4>
    </div>
    <small>Önerir, siz onaylamadan kesmez</small>
  </div>

  <details open>
    <summary>
      <span>Konuşma temizliği</span>
      <b>{speechSuggestions.length > 0 ? `${speechSuggestions.length} öneri` : "Silero + Whisper"}</b>
    </summary>
    <div class="card-body">
      <p>Sessizlikleri, “ııı/eee” dolgularını ve bağlama bağlı “şey/yani” sözcüklerini ayrı güven düzeylerinde bulur.</p>
      <button
        type="button"
        class="primary"
        disabled={disabled || speechBusy || !onSpeechAnalyze}
        onclick={() => onSpeechAnalyze?.()}
      >{speechBusy ? "Türkçe konuşma analiz ediliyor…" : speechSuggestions.length ? "Yeniden tara" : "AI konuşmayı tara"}</button>

      {#if speechSuggestions.length > 0}
        <div class="suggestions" aria-label="Konuşma temizliği önerileri">
          {#each speechSuggestions as suggestion (suggestion.id)}
            <label class:contextual={suggestion.kind === "contextual-word"}>
              <input
                type="checkbox"
                checked={selectedSuggestionIds.has(suggestion.id)}
                disabled={disabled || speechBusy}
                onchange={(event) => onSpeechSuggestionToggle?.(
                  suggestion.id,
                  event.currentTarget.checked,
                )}
              />
              <span>
                <b>{suggestionLabel(suggestion)}</b>
                <small>{timecode(suggestion.startMs)}–{timecode(suggestion.endMs)} · güven %{Math.round(suggestion.confidence * 100)}</small>
              </span>
              <button
                type="button"
                title="Zaman çizelgesine git ve dinle"
                onclick={(event) => {
                  event.preventDefault();
                  onSpeechSuggestionSeek?.(suggestion);
                }}
              >Dinle</button>
            </label>
          {/each}
        </div>
        <button
          type="button"
          class="apply"
          disabled={disabled || selectedSuggestionCount === 0 || speechBusy || !onSpeechSuggestionsApply}
          onclick={() => onSpeechSuggestionsApply?.()}
        >Seçilen {selectedSuggestionCount} öneriyi uygula</button>
      {/if}
      {#if speechMessage}
        <p class="status" class:error={speechError} role={speechError ? "alert" : "status"}>{speechMessage}</p>
      {/if}
    </div>
  </details>

  <details>
    <summary>
      <span>Konuşma sırasında diğer sesi kıs</span>
      <b>{ducking ? `${ducking.reductionDb.toFixed(0)} dB` : "Silero Voice"}</b>
    </summary>
    <div class="card-body">
      <p>Bu bir içerik etiketi değildir. Seçili klibi kısılacak hedef kabul eder; işaretlediğiniz konuşma kanalları konuşurken sesini azaltır.</p>
      {#if duckingTracks.length > 0}
        <div class="track-options">
          {#each duckingTracks as track (track.id)}
            <label>
              <input
                type="checkbox"
                checked={track.selected}
                disabled={duckingBusy || disabled}
                onchange={(event) => onDuckingTrackToggle?.(track.id, event.currentTarget.checked)}
              />
              {track.label}
            </label>
          {/each}
        </div>
      {:else}
        <small class="hint">Başka bir Voice/video kanalı ekleyin; müzik kendi kendine konuşma kaynağı seçilmez.</small>
      {/if}

      <div class="preset-row">
        <button type="button" disabled={disabled || duckingBusy} class:active={ducking?.reductionDb === -9} onclick={() => onDuckingChange?.({ reductionDb: -9 })}>Doğal</button>
        <button type="button" disabled={disabled || duckingBusy} class:active={!ducking || ducking.reductionDb === -14} onclick={() => onDuckingChange?.({ reductionDb: -14 })}>Dengeli</button>
        <button type="button" disabled={disabled || duckingBusy} class:active={ducking?.reductionDb === -18} onclick={() => onDuckingChange?.({ reductionDb: -18 })}>Podcast</button>
      </div>

      <div class="slider-grid">
        <label>
          <span>Kısma <b>{ducking?.reductionDb ?? -14} dB</b></span>
          <input type="range" min="-30" max="0" step="1" value={ducking?.reductionDb ?? -14} disabled={disabled || duckingBusy} onchange={(event) => onDuckingChange?.({ reductionDb: numberFrom(event) })} />
        </label>
        <label>
          <span>Attack <b>{ducking?.attackMs ?? 100} ms</b></span>
          <input type="range" min="10" max="800" step="10" value={ducking?.attackMs ?? 100} disabled={disabled || duckingBusy} onchange={(event) => onDuckingChange?.({ attackMs: numberFrom(event) })} />
        </label>
        <label>
          <span>Release <b>{ducking?.releaseMs ?? 650} ms</b></span>
          <input type="range" min="50" max="2500" step="50" value={ducking?.releaseMs ?? 650} disabled={disabled || duckingBusy} onchange={(event) => onDuckingChange?.({ releaseMs: numberFrom(event) })} />
        </label>
      </div>

      {#if duckingDiagnostics?.configured}
        <div class="stats">
          <span>{duckingDiagnostics.validSourceClipCount} konuşma klibi</span>
          <span>{duckingDiagnostics.speechSegmentCount} konuşma bölümü</span>
          {#if duckingDiagnostics.staleSourceClipIds.length}<span class="warning">{duckingDiagnostics.staleSourceClipIds.length} eski analiz</span>{/if}
        </div>
      {/if}
      <div class="actions">
        <button
          type="button"
          class="primary"
          disabled={disabled || duckingBusy || duckingTracks.every((track) => !track.selected) || !onDuckingApply}
          onclick={() => onDuckingApply?.()}
        >{duckingBusy ? "Konuşmalar analiz ediliyor…" : duckingActive ? "Analiz et ve güncelle" : "Analiz et ve uygula"}</button>
        {#if duckingActive}
          <button type="button" class="remove" disabled={disabled || duckingBusy} onclick={() => onDuckingRemove?.()}>Kaldır</button>
        {/if}
      </div>
      {#if duckingMessage}
        <p class="status" class:error={duckingError} role={duckingError ? "alert" : "status"}>{duckingMessage}</p>
      {/if}
    </div>
  </details>

  <details>
    <summary>
      <span>Beat marker ve otomatik kesme</span>
      <b>{beatAnalysis?.bpm ? `${beatAnalysis.bpm.toFixed(1)} BPM` : "Beat This! AI"}</b>
    </summary>
    <div class="card-body">
      <p>Beat ve downbeat’leri kaynak zamanında saklar; klibi taşısanız, kırpsanız veya hızını değiştirseniz marker kaymaz.</p>
      <div class="actions">
        <button type="button" class="primary" disabled={disabled || beatBusy || !onBeatAnalyze} onclick={() => onBeatAnalyze?.()}>{beatBusy ? "Ritim analiz ediliyor…" : beatAnalysis ? "Beat’leri yeniden bul" : "AI beat’leri bul"}</button>
        {#if beatAnalysis}<button type="button" class="remove" disabled={disabled || beatBusy} onclick={() => onBeatRemove?.()}>Markerları kaldır</button>{/if}
      </div>
      {#if beatAnalysis}
        <div class="stats">
          <span>{beatAnalysis.beatCount} beat</span>
          <span>{beatAnalysis.downbeatCount} downbeat</span>
          <span>{beatAnalysis.cacheHit ? "önbellek" : "yeni analiz"}</span>
        </div>
        <label class="cut-mode">
          <span>Kesim yoğunluğu</span>
          <select value={beatCutMode} onchange={(event) => onBeatCutModeChange?.(event.currentTarget.value as BeatCutMode)}>
            <option value="smart">Akıllı · 1,5–4 sn plan</option>
            <option value="downbeat">Yalnız downbeat</option>
            <option value="every-2">Her 2 beat</option>
            <option value="every-beat">Her beat</option>
          </select>
        </label>
        <button type="button" class="apply" disabled={disabled || beatBusy || beatAnalysis.beatCount < 4 || !onBeatCutsApply} onclick={() => onBeatCutsApply?.()}>Görüntüleri markerlarda böl</button>
      {/if}
      {#if beatMessage}
        <p class="status" class:error={beatError} role={beatError ? "alert" : "status"}>{beatMessage}</p>
      {/if}
    </div>
  </details>
</section>

<style>
  .audio-ai {
    margin: 10px 12px 16px;
    padding: 10px;
    border: 1px solid rgba(118, 232, 251, 0.2);
    border-radius: 8px;
    color: #b7c8ca;
    background: linear-gradient(145deg, rgba(12, 31, 36, 0.96), rgba(10, 18, 22, 0.96));
    font: 500 9px/1.4 "Inter", sans-serif;
  }

  .title-row,
  summary,
  .actions,
  .stats,
  .preset-row {
    display: flex;
    align-items: center;
  }

  .title-row {
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 8px;
  }

  .title-row h4 {
    margin: 3px 0 0;
    color: #e2f7fa;
    font-size: 11px;
  }

  .title-row > small { color: #6f858a; text-align: right; }

  .ai-chip {
    padding: 2px 5px;
    border: 1px solid #387d89;
    border-radius: 3px;
    color: #8eeafb;
    font-size: 7px;
    font-weight: 800;
    letter-spacing: .06em;
  }

  details {
    border-top: 1px solid rgba(125, 168, 176, .14);
  }

  summary {
    justify-content: space-between;
    gap: 8px;
    padding: 9px 2px;
    color: #c8dadd;
    cursor: pointer;
    font-weight: 700;
  }

  summary b { color: #6fcbd9; font-size: 8px; }
  .card-body { padding: 0 2px 10px; }
  .card-body > p { margin: 0 0 8px; color: #82969a; }

  button, select {
    min-height: 29px;
    border: 1px solid #354b50;
    border-radius: 5px;
    color: #aebfc2;
    background: #182529;
    font: 650 8px/1.2 "Inter", sans-serif;
  }

  button { padding: 5px 8px; cursor: pointer; }
  button:disabled { cursor: wait; opacity: .48; }
  button.primary, button.apply { border-color: #3e8e9d; color: #bceff8; background: #173a43; }
  button.apply { width: 100%; margin-top: 7px; }
  button.remove { color: #9aa9ab; }
  button.active { border-color: #68d8ea; color: #d9fbff; background: #1b4650; }

  .suggestions {
    max-height: 178px;
    margin-top: 8px;
    overflow: auto;
    border: 1px solid #263c41;
    border-radius: 5px;
  }

  .suggestions label {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 6px;
    padding: 6px;
    border-bottom: 1px solid #22353a;
    background: rgba(14, 29, 34, .7);
  }

  .suggestions label:last-child { border-bottom: 0; }
  .suggestions label.contextual { border-left: 2px solid #8d7440; }
  .suggestions span b, .suggestions span small { display: block; }
  .suggestions span b { color: #c9e5ea; font-size: 8px; }
  .suggestions span small { margin-top: 2px; color: #6f858a; font-size: 7px; }
  .suggestions button { min-height: 24px; padding: 3px 6px; }

  .track-options {
    display: grid;
    gap: 4px;
    margin: 7px 0;
  }

  .track-options label {
    padding: 5px 6px;
    border: 1px solid #293f44;
    border-radius: 4px;
    background: #142126;
  }

  .preset-row, .actions { gap: 5px; }
  .preset-row { margin: 7px 0; }
  .preset-row button { flex: 1; }
  .actions .primary { flex: 1; }

  .slider-grid { display: grid; gap: 6px; margin: 8px 0; }
  .slider-grid label span { display: flex; justify-content: space-between; color: #879a9e; }
  .slider-grid label b { color: #9bd9e3; }
  .slider-grid input { width: 100%; accent-color: #55b8c8; }

  .stats {
    flex-wrap: wrap;
    gap: 4px;
    margin: 7px 0;
  }

  .stats span {
    padding: 3px 5px;
    border: 1px solid #2e4a50;
    border-radius: 4px;
    color: #88bfc8;
    background: #132529;
  }

  .stats span.warning { color: #e0b86e; border-color: #6c5730; }
  .hint { display: block; margin: 5px 0; color: #778b8f; }
  .cut-mode { display: grid; gap: 4px; margin-top: 8px; }
  .cut-mode select { width: 100%; padding: 4px 6px; }

  .status {
    margin: 7px 0 0 !important;
    padding: 6px;
    border: 1px solid #315e68;
    border-radius: 4px;
    color: #91dce9 !important;
    background: #11282d;
  }

  .status.error { color: #e5aaa2 !important; border-color: #693d37; background: #2b1816; }
</style>
