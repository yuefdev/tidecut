<script lang="ts">
  import { onMount } from "svelte";
  import TransitionPreview from "./TransitionPreview.svelte";
  import type {
    TimelineClip,
    TransitionType,
  } from "$lib/editor/timeline-engine";
  import {
    TRANSITION_CATEGORY_OPTIONS,
    TRANSITION_PRESETS,
    isTransitionExportApproximate,
    type TransitionCategory,
    type TransitionSide,
  } from "$lib/editor/transition-presets";

  interface Props {
    clip?: TimelineClip | null;
    side?: TransitionSide;
    disabled?: boolean;
    onSideChange?: (side: TransitionSide) => void;
    onChange?: (
      side: TransitionSide,
      key: "type" | "duration",
      value: string,
    ) => void;
  }

  let {
    clip = null,
    side = "in",
    disabled = false,
    onSideChange,
    onChange,
  }: Props = $props();

  let category = $state<"all" | TransitionCategory>("all");
  let draftDuration = $state(0);
  let previewProgress = $state(0.46);
  let previewFrame = $state(12);
  let previewingType = $state<TransitionType | null>(null);
  let activeTransition = $derived(clip?.transition[side] ?? null);
  let maximumDuration = $derived(
    clip
      ? Math.max(
          0,
          clip.duration - clip.transition[side === "in" ? "out" : "in"].duration,
        )
      : 0,
  );
  let canEdit = $derived(Boolean(clip && !disabled && onChange));
  let visiblePresets = $derived(
    category === "all"
      ? TRANSITION_PRESETS
      : TRANSITION_PRESETS.filter((preset) => preset.category === category),
  );

  $effect(() => {
    clip?.id;
    side;
    activeTransition?.duration;
    draftDuration = activeTransition?.duration ?? 0;
  });

  onMount(() => {
    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) return;

    const startedAt = performance.now();
    const timer = window.setInterval(() => {
      const elapsed = (performance.now() - startedAt) % 2_600;
      const next = elapsed < 260
        ? 0
        : elapsed < 2_020
          ? (elapsed - 260) / 1_760
          : 1;
      previewProgress = Math.min(1, Math.max(0, next));
      previewFrame = Math.min(24, Math.max(1, Math.ceil(previewProgress * 24)));
    }, 50);
    return () => window.clearInterval(timer);
  });

  function getFileName(path: string): string {
    return path.split(/[/\\]/u).at(-1) || (clip?.kind === "text" ? "Metin klibi" : "Klip");
  }

  function chooseSide(next: TransitionSide) {
    onSideChange?.(next);
  }

  function chooseTransition(type: TransitionType) {
    if (!canEdit) return;
    onChange?.(side, "type", type);
  }

  function getPreviewFrame(type: TransitionType, assetDir?: string): number {
    if (!assetDir) return previewFrame;
    return previewingType === type || activeTransition?.type === type
      ? previewFrame
      : 12;
  }

  function updateDraftDuration(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const value = Number.isFinite(input.valueAsNumber) ? input.valueAsNumber : 0;
    draftDuration = Math.min(maximumDuration, Math.max(0, value));
  }

  function commitDraftDuration() {
    if (!canEdit || activeTransition?.type === "none") return;
    onChange?.(side, "duration", draftDuration.toFixed(3));
  }
</script>

<div class="transition-browser" data-testid="transition-browser">
  <div class="transition-editor">
    <div class="editor-heading">
      <div>
        <strong>Geçiş hedefi</strong>
        <span>{clip ? getFileName(clip.file) : "Timeline'dan bir klip seçin"}</span>
      </div>
      {#if disabled && clip}<span class="lock-badge">Kilitli</span>{/if}
    </div>

    <div class="side-switch" role="tablist" aria-label="Düzenlenecek geçiş tarafı">
      <button
        type="button"
        role="tab"
        aria-selected={side === "in"}
        class:active={side === "in"}
        onclick={() => chooseSide("in")}
      >
        <span aria-hidden="true">◢</span>
        Giriş
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={side === "out"}
        class:active={side === "out"}
        onclick={() => chooseSide("out")}
      >
        Çıkış
        <span aria-hidden="true">◣</span>
      </button>
    </div>

    {#if clip && activeTransition?.type !== "none"}
      <div class="duration-editor">
        <div class="duration-copy">
          <label for="transition-duration">Süre</label>
          <output for="transition-duration">{draftDuration.toFixed(2)} sn</output>
        </div>
        <div class="duration-controls">
          <input
            id="transition-duration"
            type="range"
            min="0"
            max={maximumDuration}
            step="0.05"
            value={draftDuration}
            disabled={!canEdit}
            aria-label={`${side === "in" ? "Giriş" : "Çıkış"} geçiş süresi`}
            oninput={updateDraftDuration}
            onchange={commitDraftDuration}
          />
          <input
            class="duration-number"
            type="number"
            min="0"
            max={maximumDuration}
            step="0.05"
            value={draftDuration.toFixed(2)}
            disabled={!canEdit}
            aria-label="Geçiş süresi saniye"
            oninput={updateDraftDuration}
            onchange={commitDraftDuration}
          />
        </div>
        <p>Timeline'daki turkuaz tutamacı sürükleyerek de ayarlayabilirsiniz.</p>
      </div>
    {:else if clip}
      <p class="selection-hint">Aşağıdan bir geçiş seçin; uygun başlangıç süresi otomatik eklenir.</p>
    {:else}
      <p class="selection-hint">Örnekleri inceleyebilirsiniz. Uygulamak için timeline'da bir klip seçin.</p>
    {/if}
  </div>

  <div class="category-tabs" role="tablist" aria-label="Geçiş kategorileri">
    {#each TRANSITION_CATEGORY_OPTIONS as option (option.value)}
      <button
        type="button"
        role="tab"
        aria-selected={category === option.value}
        class:active={category === option.value}
        onclick={() => (category = option.value)}
      >{option.label}</button>
    {/each}
  </div>
  <p class="export-note"><strong>≈</strong> işaretli efektler dışa aktarımda zamanlamayı koruyup yumuşak geçiş olarak işlenir.</p>

  <div class="preset-scroll">
    <div class="preset-grid" aria-label="Geçiş örnekleri">
      {#each visiblePresets as preset (preset.type)}
        <button
          type="button"
          class="preset-card"
          class:selected={activeTransition?.type === preset.type}
          disabled={!canEdit}
          aria-pressed={activeTransition?.type === preset.type}
          aria-label={`${preset.label} geçişini ${side === "in" ? "girişe" : "çıkışa"} uygula`}
          title={canEdit
            ? `${preset.label} · ${side === "in" ? "girişe" : "çıkışa"} uygula`
            : "Uygulamak için kilitsiz bir timeline klibi seçin"}
          onmouseenter={() => (previewingType = preset.type)}
          onmouseleave={() => previewingType === preset.type && (previewingType = null)}
          onfocus={() => (previewingType = preset.type)}
          onblur={() => previewingType === preset.type && (previewingType = null)}
          onclick={() => chooseTransition(preset.type)}
        >
          <span class="preview-frame">
            <TransitionPreview
              type={preset.type}
              progress={preset.type === "none" ? 0.5 : previewProgress}
              frame={getPreviewFrame(preset.type, preset.assetDir)}
            />
            {#if isTransitionExportApproximate(preset.type)}
              <span
                class="export-badge"
                title="Dışa aktarımda yumuşak geçişe çevrilir"
                aria-hidden="true"
              >≈</span>
            {/if}
            {#if activeTransition?.type === preset.type}
              <span class="selected-mark" aria-hidden="true">✓</span>
            {/if}
          </span>
          <span class="preset-name">{preset.label}</span>
        </button>
      {/each}
    </div>
  </div>
</div>

<style>
  .transition-browser {
    display: flex;
    height: 100%;
    min-height: 0;
    flex-direction: column;
    color: #cfd5d2;
    background: #101211;
  }

  .transition-editor {
    flex: 0 0 auto;
    padding: 11px 11px 10px;
    background: linear-gradient(180deg, #171b19, #121513);
    border-bottom: 1px solid #292d2b;
  }

  .editor-heading,
  .duration-copy {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .editor-heading > div {
    min-width: 0;
  }

  .editor-heading strong,
  .editor-heading span {
    display: block;
  }

  .editor-heading strong {
    color: #e0e6e3;
    font-size: 11px;
  }

  .editor-heading span {
    margin-top: 3px;
    overflow: hidden;
    color: #707975;
    font-size: 9px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .editor-heading .lock-badge {
    flex: 0 0 auto;
    margin: 0;
    padding: 3px 6px;
    color: #e7b4aa;
    background: #2c1b18;
    border: 1px solid #59322c;
    border-radius: 10px;
    font: 700 8px/1 "JetBrains Mono", monospace;
  }

  .side-switch {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 3px;
    margin-top: 10px;
    padding: 3px;
    background: #0c0e0d;
    border: 1px solid #292d2b;
    border-radius: 7px;
  }

  .side-switch button {
    min-width: 0;
    height: 29px;
    color: #747d79;
    background: transparent;
    border: 0;
    border-radius: 5px;
    font: 700 9px/1 "Inter", sans-serif;
    cursor: pointer;
  }

  .side-switch button:hover {
    color: #cbd4d0;
    background: #191d1b;
  }

  .side-switch button.active {
    color: #d8fff4;
    background: #18352d;
    box-shadow: inset 0 0 0 1px #356b5c;
  }

  .side-switch span {
    color: #6fe0bf;
  }

  .duration-editor {
    margin-top: 10px;
    padding-top: 9px;
    border-top: 1px solid #252a27;
  }

  .duration-copy label {
    color: #949d99;
    font-size: 9px;
    font-weight: 700;
  }

  .duration-copy output {
    color: #9df3d8;
    font: 700 9px/1 "JetBrains Mono", monospace;
  }

  .duration-controls {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 58px;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .duration-controls input[type="range"] {
    width: 100%;
    accent-color: #61d8b6;
  }

  .duration-number {
    box-sizing: border-box;
    width: 58px;
    height: 25px;
    padding: 0 5px;
    color: #dce6e2;
    background: #0d100f;
    border: 1px solid #343a37;
    border-radius: 4px;
    font: 600 9px/1 "JetBrains Mono", monospace;
  }

  .duration-editor p,
  .selection-hint {
    margin: 7px 0 0;
    color: #626b67;
    font-size: 8px;
    line-height: 1.45;
  }

  .category-tabs {
    display: flex;
    flex: 0 0 auto;
    gap: 4px;
    padding: 8px 9px;
    overflow-x: auto;
    border-bottom: 1px solid #222624;
    scrollbar-width: none;
  }

  .category-tabs::-webkit-scrollbar {
    display: none;
  }

  .category-tabs button {
    flex: 0 0 auto;
    padding: 5px 8px;
    color: #707975;
    background: #151817;
    border: 1px solid #292e2b;
    border-radius: 12px;
    font: 700 8px/1 "Inter", sans-serif;
    cursor: pointer;
  }

  .category-tabs button:hover,
  .category-tabs button.active {
    color: #bff7e7;
    background: #182a25;
    border-color: #356c5d;
  }

  .export-note {
    flex: 0 0 auto;
    margin: 0;
    padding: 0 10px 8px;
    color: #666f6b;
    background: #101211;
    border-bottom: 1px solid #222624;
    font-size: 7px;
    line-height: 1.4;
  }

  .export-note strong {
    color: #d7b878;
    font: 800 10px/1 "JetBrains Mono", monospace;
  }

  .preset-scroll {
    flex: 1;
    min-height: 0;
    padding: 9px;
    overflow-y: auto;
  }

  .preset-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }

  .preset-card {
    min-width: 0;
    padding: 4px 4px 7px;
    color: #919a96;
    text-align: left;
    background: #141716;
    border: 1px solid #292d2b;
    border-radius: 7px;
    cursor: pointer;
  }

  .preset-card:hover:not(:disabled) {
    color: #eef5f2;
    background: #1a1e1c;
    border-color: #46504b;
    transform: translateY(-1px);
  }

  .preset-card.selected {
    color: #d9fff3;
    background: #172a24;
    border-color: #58c7a8;
    box-shadow: 0 0 0 1px rgba(88, 199, 168, 0.17);
  }

  .preset-card:disabled {
    cursor: not-allowed;
  }

  .preset-card:disabled .preview-frame,
  .preset-card:disabled .preset-name {
    opacity: 0.72;
  }

  .preview-frame {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    background: #080a09;
    border: 1px solid #313735;
    border-radius: 5px;
  }

  .selected-mark {
    position: absolute;
    z-index: 20;
    top: 4px;
    right: 4px;
    display: grid;
    width: 15px;
    height: 15px;
    place-items: center;
    color: #04100c;
    background: #72e1c0;
    border-radius: 50%;
    box-shadow: 0 2px 7px rgba(0, 0, 0, 0.45);
    font-size: 9px;
    font-weight: 900;
  }

  .export-badge {
    position: absolute;
    z-index: 20;
    bottom: 4px;
    left: 4px;
    display: grid;
    width: 15px;
    height: 14px;
    place-items: center;
    color: #2b1d06;
    background: rgba(239, 196, 107, 0.9);
    border-radius: 3px;
    box-shadow: 0 2px 7px rgba(0, 0, 0, 0.38);
    font: 900 10px/1 "JetBrains Mono", monospace;
  }

  .preset-name {
    display: block;
    margin: 6px 3px 0;
    overflow: hidden;
    font-size: 8px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 760px) {
    .preset-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
