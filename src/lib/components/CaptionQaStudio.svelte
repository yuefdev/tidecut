<script lang="ts">
  import { isTauri } from "@tauri-apps/api/core";
  import {
    analyzeCaptionCue,
    classifyWordConfidence,
    type CaptionDictionaryEntry,
    type CaptionLanguage,
    type CaptionQaResult,
    type CaptionWord,
  } from "$lib/captions/caption-qa";
  import {
    parseCaption,
    serializeCaption,
    type CaptionDocument,
    type CaptionFormat,
  } from "$lib/captions/caption-formats";
  import {
    applyCaptionDictionarySuggestion,
    captionDocumentFromStudio,
    captionQaSummary,
    captionStudioFromDocument,
    createCaptionDictionaryEntry,
    setCaptionCueSpeaker,
    updateCaptionCueText,
    upsertCaptionTranslation,
    type CaptionStudioProject,
  } from "$lib/captions/caption-studio";
  import {
    chooseCaptionToOpen,
    saveCaptionText,
    type CaptionFileFormat,
  } from "$lib/captions/client";

  interface Props {
    open: boolean;
    project: CaptionStudioProject;
    transcriptionSources: Array<{
      id: string;
      label: string;
      detail: string;
    }>;
    transcriptionSourceId: string | null;
    canTranscribe: boolean;
    transcribeBusy: boolean;
    transcribeMessage?: string | null;
    transcribeError?: boolean;
    onChange: (project: CaptionStudioProject) => void;
    onTranscriptionSourceChange: (sourceId: string) => void;
    onTranscribe: () => void;
    onApply: (project: CaptionStudioProject) => void;
    onClose: () => void;
  }

  type CaptionPanelTab = "review" | "style" | "settings";
  const CAPTION_PANEL_TABS: readonly CaptionPanelTab[] = ["review", "style", "settings"];

  let {
    open,
    project,
    transcriptionSources,
    transcriptionSourceId,
    canTranscribe,
    transcribeBusy,
    transcribeMessage = null,
    transcribeError = false,
    onChange,
    onTranscriptionSourceChange,
    onTranscribe,
    onApply,
    onClose,
  }: Props = $props();
  let selectedCueId = $state<string | null>(null);
  let selectedWordId = $state<string | null>(null);
  let query = $state("");
  let filter = $state<"all" | "review" | "mixed" | "translation">("all");
  let activeTab = $state<CaptionPanelTab>("review");
  let cueDetailOpen = $state(false);
  let exportFormat = $state<CaptionFormat>("srt");
  let fileInput = $state<HTMLInputElement>();
  let busy = $state(false);
  let message = $state<string | null>(null);
  let messageError = $state(false);
  let dictionaryTerm = $state("");
  let dictionaryVariants = $state("");
  let dictionaryCategory = $state<CaptionDictionaryEntry["category"]>("proper-noun");
  let speakerName = $state("");
  let selectedTranscriptionSource = $derived(
    transcriptionSources.find((source) => source.id === transcriptionSourceId) ?? null,
  );

  let summary = $derived(captionQaSummary(project));
  let selectedCue = $derived(
    project.cues.find((cue) => cue.id === selectedCueId) ?? project.cues[0] ?? null,
  );
  let selectedWord = $derived(
    selectedCue?.words.find((word) => word.id === selectedWordId) ?? null,
  );
  let selectedQa = $derived(
    selectedCue
      ? analyzeCaptionCue(selectedCue, {
          primaryLanguage: project.primaryLanguage,
          dictionary: project.dictionary,
          thresholds: project.thresholds,
        })
      : null,
  );
  let translationText = $derived(
    selectedCue
      ? project.translations.find((item) => item.cueId === selectedCue.id)?.text ?? ""
      : "",
  );
  let filteredCues = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase("tr-TR");
    return project.cues.filter((cue) => {
      const translation = project.translations.find((item) => item.cueId === cue.id)?.text ?? "";
      const speaker = project.speakers.find((item) => item.id === cue.speakerId)?.label ?? "";
      if (
        needle &&
        !`${cue.text} ${translation} ${speaker}`
          .toLocaleLowerCase("tr-TR")
          .includes(needle)
      ) {
        return false;
      }
      if (filter === "all") return true;
      const qa = analyzeCaptionCue(cue, {
        primaryLanguage: project.primaryLanguage,
        dictionary: project.dictionary,
        thresholds: project.thresholds,
      });
      if (filter === "review") return meaningfulReviewWordCount(qa) > 0;
      if (filter === "mixed") return qa.codeSwitchWordCount > 0;
      return !project.translations.some(
        (translation) => translation.cueId === cue.id && translation.text.trim(),
      );
    });
  });
  let selectedFilteredIndex = $derived(
    selectedCue ? filteredCues.findIndex((cue) => cue.id === selectedCue.id) : -1,
  );
  let reviewCues = $derived.by(() =>
    project.cues.filter(
      (cue) => meaningfulReviewWordCount(
        analyzeCaptionCue(cue, {
          primaryLanguage: project.primaryLanguage,
          dictionary: project.dictionary,
          thresholds: project.thresholds,
        }),
      ) > 0,
    ),
  );
  let mixedCueCount = $derived.by(() =>
    project.cues.filter(
      (cue) =>
        analyzeCaptionCue(cue, {
          primaryLanguage: project.primaryLanguage,
          dictionary: project.dictionary,
          thresholds: project.thresholds,
        }).codeSwitchWordCount > 0,
    ),
  );

  $effect(() => {
    if (!open) return;
    if (!selectedCueId || !project.cues.some((cue) => cue.id === selectedCueId)) {
      selectedCueId = project.cues[0]?.id ?? null;
      selectedWordId = null;
    }
  });

  function emit(next: CaptionStudioProject) {
    onChange(next);
  }

  function selectCue(cueId: string) {
    selectedCueId = cueId;
    selectedWordId = null;
    cueDetailOpen = true;
  }

  function showCueList() {
    cueDetailOpen = false;
    selectedWordId = null;
  }

  function setFilter(next: typeof filter) {
    filter = next;
    showCueList();
  }

  function selectAdjacentCue(offset: number) {
    if (filteredCues.length === 0) return;
    const currentIndex = Math.max(0, selectedFilteredIndex);
    const nextIndex = Math.max(0, Math.min(filteredCues.length - 1, currentIndex + offset));
    selectCue(filteredCues[nextIndex].id);
  }

  function selectNextIssue() {
    if (reviewCues.length === 0) return;
    const currentIndex = selectedCue
      ? reviewCues.findIndex((cue) => cue.id === selectedCue.id)
      : -1;
    selectCue(reviewCues[(currentIndex + 1) % reviewCues.length].id);
    activeTab = "review";
  }

  function prepareSelectedWordForDictionary() {
    if (!selectedWord) return;
    dictionaryTerm = selectedWord.text;
    dictionaryVariants = "";
    dictionaryCategory = "proper-noun";
    activeTab = "settings";
  }

  function updateConfidenceThreshold(
    key: "lowBelow" | "reviewBelow",
    percent: number,
  ) {
    if (!Number.isFinite(percent)) return;
    const value = Math.max(0, Math.min(1, percent / 100));
    const minimumGap = 0.01;
    const thresholds = { ...project.thresholds, [key]: value };
    if (key === "lowBelow") {
      thresholds.lowBelow = Math.min(value, 1 - minimumGap);
      thresholds.reviewBelow = Math.max(
        thresholds.reviewBelow,
        thresholds.lowBelow + minimumGap,
      );
    } else {
      thresholds.reviewBelow = Math.max(minimumGap, value);
      thresholds.lowBelow = Math.min(
        thresholds.lowBelow,
        thresholds.reviewBelow - minimumGap,
      );
    }
    emit({ ...project, thresholds });
  }

  function handlePanelTabKeydown(event: KeyboardEvent) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const tabList = event.currentTarget as HTMLElement;
    const currentIndex = CAPTION_PANEL_TABS.indexOf(activeTab);
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? CAPTION_PANEL_TABS.length - 1
          : (currentIndex + (event.key === "ArrowRight" ? 1 : -1) + CAPTION_PANEL_TABS.length) %
            CAPTION_PANEL_TABS.length;
    activeTab = CAPTION_PANEL_TABS[nextIndex];
    requestAnimationFrame(() => {
      tabList.querySelector<HTMLButtonElement>(`[data-caption-tab="${activeTab}"]`)
        ?.focus();
    });
  }

  function beginTranscription() {
    message = null;
    onTranscribe();
  }

  function transcriptionButtonLabel() {
    if (transcribeBusy) return project.cues.length ? "Yeniden taranıyor…" : "Taranıyor…";
    return project.cues.length ? "Seçili klibi yeniden tara" : "Seçili klibi tara";
  }

  function transcriptionButtonTitle() {
    if (transcriptionSources.length === 0) return "Önce timeline'a ses veya video klibi ekleyin";
    if (!selectedTranscriptionSource) return "Önce taranacak ses veya video klibini seçin";
    return `${selectedTranscriptionSource.label} klibini Whisper ile tara`;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open || event.key !== "Escape") return;
    const target = event.target;
    if (!(target instanceof HTMLElement) || !target.closest('[data-testid="caption-qa-panel"]')) {
      return;
    }
    if (
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement ||
      (target instanceof HTMLElement && target.isContentEditable)
    ) {
      return;
    }
    event.preventDefault();
    if (selectedWordId) {
      selectedWordId = null;
      return;
    }
    if (cueDetailOpen) {
      showCueList();
      return;
    }
    onClose();
  }

  async function beginImport() {
    message = null;
    if (!isTauri()) {
      fileInput?.click();
      return;
    }
    busy = true;
    try {
      const payload = await chooseCaptionToOpen();
      if (!payload) return;
      const format: CaptionFormat = payload.extension === "ssa" ? "ass" : payload.extension;
      importText(payload.content, format, payload.fileName);
    } catch (error) {
      setMessage(formatError(error), true);
    } finally {
      busy = false;
    }
  }

  async function handleBrowserImport(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    busy = true;
    try {
      const extension = file.name.split(".").pop()?.toLocaleLowerCase("en-US");
      const format: CaptionFormat =
        extension === "vtt" ? "vtt" : extension === "ass" || extension === "ssa" ? "ass" : "srt";
      importText(await file.text(), format, file.name);
    } catch (error) {
      setMessage(formatError(error), true);
    } finally {
      busy = false;
    }
  }

  function importText(content: string, format: CaptionFormat, sourceName: string) {
    const document = parseCaption(content, format);
    const imported = captionStudioFromDocument(document, {
      sourceName,
      primaryLanguage: project.primaryLanguage,
    });
    const next: CaptionStudioProject = {
      ...imported,
      dictionary: project.dictionary,
      thresholds: project.thresholds,
      targetLanguage: project.targetLanguage,
      template: project.template,
      bilingual: project.bilingual,
      bilingualOrder: project.bilingualOrder,
    };
    emit(next);
    selectedCueId = next.cues[0]?.id ?? null;
    selectedWordId = null;
    cueDetailOpen = next.cues.length > 0;
    activeTab = "review";
    setMessage(`${sourceName} · ${next.cues.length} cue içe aktarıldı.`, false);
  }

  async function exportCaptions() {
    if (project.cues.length === 0) return;
    busy = true;
    message = null;
    try {
      let document = captionDocumentFromStudio(project, {
        format: exportFormat,
        bilingual: project.bilingual,
      });
      if (exportFormat === "ass") document = withAssMotionTemplate(document);
      const content = serializeCaption(document, exportFormat);
      const baseName = (project.sourceName || "altyazi").replace(/\.(srt|vtt|ass|ssa)$/iu, "");
      const fileName = `${baseName}${project.bilingual ? "-tr-en" : ""}.${exportFormat}`;
      if (isTauri()) {
        const result = await saveCaptionText(
          content,
          exportFormat as CaptionFileFormat,
          fileName,
        );
        if (!result) return;
        setMessage(`${result.bytesWritten.toLocaleString("tr-TR")} bayt dışa aktarıldı.`, false);
      } else {
        downloadText(content, fileName, exportFormat);
        setMessage(`${fileName} indirildi.`, false);
      }
    } catch (error) {
      setMessage(formatError(error), true);
    } finally {
      busy = false;
    }
  }

  function withAssMotionTemplate(document: CaptionDocument): CaptionDocument {
    const captionStyle = {
      name: "Astral Caption",
      fontName: "Arial",
      fontSize: project.template === "pop" ? 64 : 56,
      primaryColor: project.template === "pop" ? "#FFE27A" : "#FFFFFF",
      outlineColor: "#101010",
      backColor: "#000000CC",
      bold: true,
      borderStyle: 1,
      outline: project.template === "pop" ? 4 : 2,
      shadow: 1,
      alignment: 2,
      marginV: 90,
    };
    return {
      ...document,
      styles: [
        ...(document.styles ?? []).filter((style) => style.name !== captionStyle.name),
        captionStyle,
      ],
      cues: document.cues.map((cue) => ({
        ...cue,
        style: "Astral Caption",
        ass: {
          ...cue.ass,
          animations:
            project.template === "pop"
              ? [
                  { kind: "fade" as const, fadeInMs: 90, fadeOutMs: 130 },
                  {
                    kind: "move" as const,
                    fromX: 960,
                    fromY: 930,
                    toX: 960,
                    toY: 850,
                    startMs: 0,
                    endMs: 180,
                  },
                ]
              : project.template === "slide"
                ? [
                    { kind: "fade" as const, fadeInMs: 110, fadeOutMs: 150 },
                    {
                      kind: "move" as const,
                      fromX: 960,
                      fromY: 950,
                      toX: 960,
                      toY: 850,
                      startMs: 0,
                      endMs: 260,
                    },
                  ]
                : [{ kind: "fade" as const, fadeInMs: 140, fadeOutMs: 160 }],
        },
      })),
    };
  }

  function editCueText(event: Event) {
    if (!selectedCue) return;
    emit(updateCaptionCueText(project, selectedCue.id, (event.currentTarget as HTMLTextAreaElement).value));
  }

  function editTranslation(event: Event) {
    if (!selectedCue) return;
    emit(
      upsertCaptionTranslation(
        project,
        selectedCue.id,
        (event.currentTarget as HTMLTextAreaElement).value,
      ),
    );
  }

  function updateSelectedWord(
    updates: Partial<Pick<CaptionWord, "startMs" | "endMs" | "confidence" | "language">>,
  ) {
    if (!selectedCue || !selectedWord) return;
    const words = selectedCue.words.map((word) =>
      word.id === selectedWord.id ? { ...word, ...updates } : word,
    );
    const startMs = Math.min(selectedCue.startMs, ...words.map((word) => word.startMs));
    const endMs = Math.max(selectedCue.endMs, ...words.map((word) => word.endMs));
    emit({
      ...project,
      confidenceMode: "confidence" in updates ? "word" : project.confidenceMode,
      cues: project.cues.map((cue) =>
        cue.id === selectedCue.id ? { ...cue, startMs, endMs, words } : cue,
      ),
    });
  }

  function addDictionaryEntry() {
    try {
      const entry = createCaptionDictionaryEntry({
        canonical: dictionaryTerm,
        variants: dictionaryVariants,
        category: dictionaryCategory,
        language: project.primaryLanguage,
      });
      emit({ ...project, dictionary: [...project.dictionary, entry] });
      dictionaryTerm = "";
      dictionaryVariants = "";
      setMessage(`${entry.canonical} sözlüğe eklendi.`, false);
    } catch (error) {
      setMessage(formatError(error), true);
    }
  }

  function removeDictionaryEntry(entryId: string) {
    emit({ ...project, dictionary: project.dictionary.filter((entry) => entry.id !== entryId) });
  }

  function addSpeaker() {
    const label = speakerName.trim();
    if (!label) return;
    const index = project.speakers.length;
    emit({
      ...project,
      speakers: [
        ...project.speakers,
        {
          id: `speaker-${Date.now().toString(36)}`,
          label,
          color: ["#65d7b2", "#79a8ff", "#d59cff", "#ffbd70", "#ff7f91"][index % 5],
          language: project.primaryLanguage,
        },
      ],
    });
    speakerName = "";
  }

  function setMessage(text: string, error: boolean) {
    message = text;
    messageError = error;
  }

  function wordBand(word: CaptionWord) {
    if (project.confidenceMode === "unavailable") return "unknown";
    return classifyWordConfidence(word.confidence, project.thresholds);
  }

  function wordQa(wordId: string) {
    return selectedQa?.words.find((item) => item.wordId === wordId);
  }

  function meaningfulReviewWordCount(qa: CaptionQaResult) {
    return project.confidenceMode === "word"
      ? qa.reviewWordCount
      : qa.words.filter((word) => Boolean(word.dictionarySuggestion)).length;
  }

  function formatClock(milliseconds: number) {
    const totalSeconds = Math.max(0, milliseconds) / 1000;
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds - minutes * 60;
    return `${minutes.toString().padStart(2, "0")}:${seconds.toFixed(3).padStart(6, "0")}`;
  }

  function formatError(error: unknown) {
    if (error instanceof Error) return error.message;
    if (error && typeof error === "object" && "message" in error) {
      return String((error as { message: unknown }).message);
    }
    return String(error);
  }

  function downloadText(content: string, name: string, format: CaptionFormat) {
    const type = format === "vtt" ? "text/vtt" : "text/plain";
    const url = URL.createObjectURL(new Blob([content], { type: `${type};charset=utf-8` }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = name;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="caption-panel" data-testid="caption-qa-panel">
    <header class="panel-overview">
      <div class="source-summary">
        <span title={project.sourceName ?? "Caption projesi"}>{project.sourceName ?? "Caption projesi"}</span>
        <small>{summary.cueCount} cue · {summary.wordCount} kelime</small>
      </div>
      <button
        class="issue-jump"
        class:alert={reviewCues.length > 0}
        disabled={reviewCues.length === 0}
        onclick={selectNextIssue}
        title="Sonraki kontrol gerektiren cue'ya git"
      >{reviewCues.length} cue kontrol <span>›</span></button>
    </header>

    <div class="transcription-source">
      <label for="caption-transcription-source">
        <span>Tarama kaynağı</span>
        <small>Timeline'daki ses/video klibi</small>
      </label>
      <select
        id="caption-transcription-source"
        value={transcriptionSourceId ?? ""}
        disabled={transcribeBusy || transcriptionSources.length === 0}
        onchange={(event) => onTranscriptionSourceChange((event.currentTarget as HTMLSelectElement).value)}
      >
        <option value="" disabled>Ses veya video klibi seçin</option>
        {#each transcriptionSources as source (source.id)}
          <option value={source.id}>{source.label}</option>
        {/each}
      </select>
      {#if selectedTranscriptionSource}
        <small class="transcription-source-detail">{selectedTranscriptionSource.detail} · yeni ses eklediğinde buradan değiştir</small>
      {:else if transcriptionSources.length === 0}
        <small class="transcription-source-detail empty">Timeline'a ses veya video ekleyin; burada görünecek.</small>
      {/if}
    </div>

    <div class="quick-actions">
      <button
        class="transcribe-button"
        disabled={!canTranscribe || transcribeBusy}
        onclick={beginTranscription}
        title={transcriptionButtonTitle()}
      >{transcriptionButtonLabel()}</button>
      <button class="compact-action" disabled={busy} onclick={beginImport}>İçe aktar</button>
    </div>

    {#if message}
      <div class="notice" class:error={messageError} role={messageError ? "alert" : "status"}>{message}</div>
    {:else if transcribeMessage}
      <div class="notice" class:error={transcribeError} role={transcribeError ? "alert" : "status"}>{transcribeMessage}</div>
    {/if}

    <div class="quality-strip" aria-label="Caption kalite özeti">
      <span class:alert={project.confidenceMode === "word" && summary.lowConfidenceCount > 0} title={project.confidenceMode === "word" ? "Düşük güvenli kelimeler" : "Bu kaynakta kelime güven skoru yok"}><strong>{project.confidenceMode === "word" ? summary.lowConfidenceCount : "—"}</strong> güven</span>
      <span class:mixed={summary.codeSwitchWordCount > 0}><strong>{summary.codeSwitchWordCount}</strong> TR/EN</span>
      <span class:missing={summary.missingTranslationCount > 0}><strong>{summary.missingTranslationCount}</strong> çeviri</span>
    </div>

    <div class="panel-tabs" role="tablist" aria-label="Caption panel bölümleri" tabindex="-1" onkeydown={handlePanelTabKeydown}>
      <button id="caption-tab-review" data-caption-tab="review" role="tab" aria-controls="caption-panel-content" aria-selected={activeTab === "review"} tabindex={activeTab === "review" ? 0 : -1} class:active={activeTab === "review"} onclick={() => (activeTab = "review")}>Kontrol</button>
      <button id="caption-tab-style" data-caption-tab="style" role="tab" aria-controls="caption-panel-content" aria-selected={activeTab === "style"} tabindex={activeTab === "style" ? 0 : -1} class:active={activeTab === "style"} onclick={() => (activeTab = "style")}>Stil</button>
      <button id="caption-tab-settings" data-caption-tab="settings" role="tab" aria-controls="caption-panel-content" aria-selected={activeTab === "settings"} tabindex={activeTab === "settings" ? 0 : -1} class:active={activeTab === "settings"} onclick={() => (activeTab = "settings")}>Ayarlar</button>
    </div>

    <div class="panel-scroll" id="caption-panel-content" role="tabpanel" aria-labelledby={`caption-tab-${activeTab}`}>
      {#if activeTab === "review"}
        {#if project.cues.length === 0}
          <div class="empty-state">
            <span>CC</span>
            <strong>Henüz altyazı yok</strong>
            <p>Bir klibi yazıya dökün veya SRT, VTT, ASS dosyası içe aktarın.</p>
            <button class="primary" onclick={beginImport}>Dosya seç</button>
          </div>
        {:else if !cueDetailOpen}
          <section class="cue-browser" aria-label="Caption listesi">
            <div class="search-field">
              <span aria-hidden="true">⌕</span>
              <input bind:value={query} placeholder="Metin, çeviri veya konuşmacı ara" aria-label="Altyazıda ara" />
              {#if query}<button type="button" onclick={() => (query = "")} aria-label="Aramayı temizle">×</button>{/if}
            </div>
            <div class="filters" aria-label="QA filtreleri">
              <button class:active={filter === "all"} onclick={() => setFilter("all")}>Tümü <i>{project.cues.length}</i></button>
              <button class:active={filter === "review"} onclick={() => setFilter("review")}>Şüpheli <i>{reviewCues.length}</i></button>
              <button class:active={filter === "mixed"} onclick={() => setFilter("mixed")}>TR/EN <i>{mixedCueCount}</i></button>
              <button class:active={filter === "translation"} onclick={() => setFilter("translation")}>Çeviri <i>{summary.missingTranslationCount}</i></button>
            </div>
            <div class="cue-list">
              {#each filteredCues as cue (cue.id)}
                {@const qa = analyzeCaptionCue(cue, { primaryLanguage: project.primaryLanguage, dictionary: project.dictionary, thresholds: project.thresholds })}
                {@const reviewCount = meaningfulReviewWordCount(qa)}
                {@const speaker = project.speakers.find((item) => item.id === cue.speakerId)}
                <button class="cue-row" onclick={() => selectCue(cue.id)}>
                  <span class="cue-meta">
                    <time>{formatClock(cue.startMs)}</time>
                    {#if speaker}<i style={`--speaker:${speaker.color ?? "#65d7b2"}`}>{speaker.label}</i>{/if}
                    {#if reviewCount > 0}<em>{reviewCount}</em>{/if}
                  </span>
                  <span class="cue-text">
                    {#each cue.words as word (word.id)}
                      {@const qaWord = qa.words.find((item) => item.wordId === word.id)}
                      {@const needsReview = project.confidenceMode === "word" ? qaWord?.needsReview : Boolean(qaWord?.dictionarySuggestion)}
                      <mark class:needs-review={needsReview} class:low={project.confidenceMode === "word" && qaWord?.confidenceBand === "low"} class:code-switch={qaWord?.isCodeSwitch}>{word.text}</mark>{" "}
                    {:else}
                      <mark>{cue.text || "(boş cue)"}</mark>
                    {/each}
                  </span>
                  <span class="row-arrow" aria-hidden="true">›</span>
                </button>
              {:else}
                <div class="empty-small">Bu filtrede cue yok.</div>
              {/each}
            </div>
          </section>
        {:else if selectedCue}
          <section class="cue-editor" aria-label="Seçili caption düzenleyici">
            <div class="detail-nav">
              <button class="back-button" onclick={showCueList}>‹ Liste</button>
              <span>Cue {project.cues.indexOf(selectedCue) + 1}/{project.cues.length}</span>
              <div>
                <button aria-label="Önceki cue" disabled={selectedFilteredIndex <= 0} onclick={() => selectAdjacentCue(-1)}>‹</button>
                <button aria-label="Sonraki cue" disabled={selectedFilteredIndex < 0 || selectedFilteredIndex >= filteredCues.length - 1} onclick={() => selectAdjacentCue(1)}>›</button>
              </div>
            </div>

            <div class="cue-heading">
              <strong>{formatClock(selectedCue.startMs)} — {formatClock(selectedCue.endMs)}</strong>
              <span class:alert={selectedQa ? meaningfulReviewWordCount(selectedQa) > 0 : false}>{selectedQa ? meaningfulReviewWordCount(selectedQa) : 0} kontrol</span>
            </div>

            <label class="field-label">Konuşmacı
              <select value={selectedCue.speakerId ?? ""} onchange={(event) => emit(setCaptionCueSpeaker(project, selectedCue!.id, (event.currentTarget as HTMLSelectElement).value || undefined))}>
                <option value="">Belirsiz</option>
                {#each project.speakers as speaker (speaker.id)}<option value={speaker.id}>{speaker.label}</option>{/each}
              </select>
            </label>

            <div class="word-strip" aria-label="Kelime düzeyinde zamanlama">
              {#each selectedCue.words as word (word.id)}
                {@const qaWord = wordQa(word.id)}
                <button
                  class="word"
                  class:low={wordBand(word) === "low"}
                  class:review={wordBand(word) === "review"}
                  class:unknown={wordBand(word) === "unknown"}
                  class:codeswitch={qaWord?.isCodeSwitch}
                  class:selected={selectedWord?.id === word.id}
                  onclick={() => (selectedWordId = selectedWordId === word.id ? null : word.id)}
                  title={`${formatClock(word.startMs)} – ${formatClock(word.endMs)} · ${project.confidenceMode === "word" ? `%${Math.round(word.confidence * 100)} güven` : "güven skoru yok"}`}
                ><span>{word.text}</span><small>{(word.startMs / 1000).toFixed(2)}s</small></button>
              {:else}
                <span class="empty-small">Bu cue boş.</span>
              {/each}
            </div>

            {#if project.confidenceMode === "unavailable"}
              <p class="confidence-note">Dosya formatında güven skoru yok. Kelime süreleri korunur ve elle düzenlenebilir.</p>
            {/if}

            {#if selectedWord}
              <section class="word-inspector">
                <header><span>Kelime ayrıntısı</span><strong>{selectedWord.text}</strong><button onclick={() => (selectedWordId = null)} aria-label="Kelime ayrıntısını kapat">×</button></header>
                <div class="field-grid">
                  <label>Başlangıç<input type="number" min="0" step="0.001" value={(selectedWord.startMs / 1000).toFixed(3)} onchange={(event) => updateSelectedWord({ startMs: Math.max(0, Math.min(selectedWord!.endMs, Number((event.currentTarget as HTMLInputElement).value) * 1000)) })} /></label>
                  <label>Bitiş<input type="number" min="0" step="0.001" value={(selectedWord.endMs / 1000).toFixed(3)} onchange={(event) => updateSelectedWord({ endMs: Math.max(selectedWord!.startMs, Number((event.currentTarget as HTMLInputElement).value) * 1000) })} /></label>
                  {#if project.confidenceMode === "word"}
                    <label>Güven %<input type="number" min="0" max="100" step="1" value={Math.round(selectedWord.confidence * 100)} onchange={(event) => updateSelectedWord({ confidence: Math.max(0, Math.min(1, Number((event.currentTarget as HTMLInputElement).value) / 100)) })} /></label>
                  {:else}
                    <label>Güven %<input value="—" disabled title="İçe aktarılan altyazıda güven skoru yok" /></label>
                  {/if}
                  <label>Dil<select value={selectedWord.language ?? "unknown"} onchange={(event) => updateSelectedWord({ language: (event.currentTarget as HTMLSelectElement).value as CaptionLanguage })}><option value="unknown">Otomatik</option><option value="tr">TR</option><option value="en">EN</option></select></label>
                </div>
                <button class="dictionary-shortcut" onclick={prepareSelectedWordForDictionary}>Sözlüğe ekle</button>
              </section>
            {/if}

            <label class="text-label" for="caption-source-text">Kaynak metin <small>Zamanlama korunur</small></label>
            <textarea id="caption-source-text" class="source-text" value={selectedCue.text} oninput={editCueText}></textarea>

            {#if selectedQa?.dictionarySuggestions.length}
              <div class="suggestions">
                <span>Sözlük önerileri</span>
                {#each selectedQa.dictionarySuggestions as suggestion}
                  <button onclick={() => emit(applyCaptionDictionarySuggestion(project, selectedCue!.id, suggestion))}><s>{suggestion.inputText}</s> → <strong>{suggestion.canonical}</strong></button>
                {/each}
              </div>
            {/if}

            <label class="toggle-row">
              <span><strong>TR + EN</strong><small>İki dilli altyazı</small></span>
              <input type="checkbox" checked={project.bilingual} onchange={(event) => emit({ ...project, bilingual: (event.currentTarget as HTMLInputElement).checked })} />
            </label>
            {#if project.bilingual}
              <select class="order-select" value={project.bilingualOrder} onchange={(event) => emit({ ...project, bilingualOrder: (event.currentTarget as HTMLSelectElement).value as CaptionStudioProject["bilingualOrder"] })} aria-label="İki dilli satır sırası">
                <option value="source-first">Türkçe üstte</option><option value="translation-first">English üstte</option>
              </select>
              <label class="text-label" for="caption-translation">English çeviri <small>{translationText.trim() ? "hazır" : "eksik"}</small></label>
              <textarea id="caption-translation" class="translation-text" value={translationText} oninput={editTranslation} placeholder="Bu cue'nun İngilizce karşılığı…"></textarea>
            {/if}
          </section>
        {/if}
      {:else if activeTab === "style"}
        <section class="settings-section">
          <div class="section-title"><span>Hareketli şablon</span><small>Tüm caption'lara uygulanır</small></div>
          <div class="template-list">
            <button class:active={project.template === "clean"} onclick={() => emit({ ...project, template: "clean" })}><i>Aa</i><span><strong>Temiz</strong><small>Yumuşak giriş</small></span><em>✓</em></button>
            <button class:active={project.template === "pop"} onclick={() => emit({ ...project, template: "pop" })}><i class="template-pop">POP</i><span><strong>Pop</strong><small>Hızlı giriş</small></span><em>✓</em></button>
            <button class:active={project.template === "slide"} onclick={() => emit({ ...project, template: "slide" })}><i>↑Aa</i><span><strong>Yüksel</strong><small>Aşağıdan giriş</small></span><em>✓</em></button>
          </div>
        </section>
        <section class="settings-section">
          <div class="section-title"><span>İki dilli görünüm</span><small>TR + EN</small></div>
          <label class="toggle-row">
            <span><strong>İki dili göster</strong><small>Çeviriyi ikinci satıra ekler</small></span>
            <input type="checkbox" checked={project.bilingual} onchange={(event) => emit({ ...project, bilingual: (event.currentTarget as HTMLInputElement).checked })} />
          </label>
          {#if project.bilingual}
            <label class="field-label">Satır sırası
              <select value={project.bilingualOrder} onchange={(event) => emit({ ...project, bilingualOrder: (event.currentTarget as HTMLSelectElement).value as CaptionStudioProject["bilingualOrder"] })}>
                <option value="source-first">Türkçe üstte</option><option value="translation-first">English üstte</option>
              </select>
            </label>
          {/if}
        </section>
      {:else}
        <section class="settings-section compact-fields">
          <div class="section-title"><span>Diller</span><small>Code-switch analizi</small></div>
          <div class="field-grid">
            <label>Ana dil<select value={project.primaryLanguage} onchange={(event) => emit({ ...project, primaryLanguage: (event.currentTarget as HTMLSelectElement).value as Exclude<CaptionLanguage, "unknown"> })}><option value="tr">Türkçe</option><option value="en">English</option></select></label>
            <label>Çeviri<select value={project.targetLanguage} onchange={(event) => emit({ ...project, targetLanguage: (event.currentTarget as HTMLSelectElement).value as Exclude<CaptionLanguage, "unknown"> })}><option value="en">English</option><option value="tr">Türkçe</option></select></label>
          </div>
        </section>

        <details class="settings-section" open>
          <summary><span>Özel isim sözlüğü</span><small>{project.dictionary.length} kayıt</small></summary>
          <div class="details-body">
            <input bind:value={dictionaryTerm} placeholder="Doğru yazım: OpenAI" aria-label="Sözlük doğru yazımı" />
            <input bind:value={dictionaryVariants} placeholder="Varyantlar: Open AI, OpanAI" aria-label="Sözlük varyantları" />
            <div class="inline-row"><select bind:value={dictionaryCategory}><option value="proper-noun">Özel isim</option><option value="brand">Marka</option><option value="term">Terim</option></select><button onclick={addDictionaryEntry}>Ekle</button></div>
            <div class="dictionary-list">
              {#each project.dictionary as entry (entry.id)}
                <div><span><strong>{entry.canonical}</strong><small>{entry.variants?.join(", ") || "varyant yok"}</small></span><button onclick={() => removeDictionaryEntry(entry.id)} aria-label={`${entry.canonical} kaydını sil`}>×</button></div>
              {:else}<p class="empty-inline">Marka, kişi ve teknik terimleri buraya ekleyin.</p>{/each}
            </div>
          </div>
        </details>

        <details class="settings-section">
          <summary><span>Konuşmacılar</span><small>{project.speakers.length}</small></summary>
          <div class="details-body">
            <div class="speaker-list">{#each project.speakers as speaker (speaker.id)}<span style={`--speaker:${speaker.color ?? "#65d7b2"}`}><i></i>{speaker.label}</span>{:else}<p class="empty-inline">Henüz konuşmacı yok.</p>{/each}</div>
            <div class="inline-row"><input bind:value={speakerName} placeholder="Konuşmacı adı" aria-label="Yeni konuşmacı" /><button onclick={addSpeaker}>Ekle</button></div>
          </div>
        </details>

        <details class="settings-section">
          <summary><span>Güven eşikleri</span><small>QA renkleri</small></summary>
          <div class="details-body field-grid">
            <label>Düşük altı %<input type="number" min="0" max="100" value={Math.round(project.thresholds.lowBelow * 100)} onchange={(event) => updateConfidenceThreshold("lowBelow", (event.currentTarget as HTMLInputElement).valueAsNumber)} /></label>
            <label>Kontrol altı %<input type="number" min="0" max="100" value={Math.round(project.thresholds.reviewBelow * 100)} onchange={(event) => updateConfidenceThreshold("reviewBelow", (event.currentTarget as HTMLInputElement).valueAsNumber)} /></label>
          </div>
        </details>

        <details class="settings-section">
          <summary><span>Dosya işlemleri</span><small>{project.sourceFormat?.toUpperCase() ?? "—"}</small></summary>
          <div class="details-body file-actions">
            <button class="secondary" disabled={busy} onclick={beginImport}>SRT / VTT / ASS içe aktar</button>
            <div class="inline-row"><select bind:value={exportFormat} aria-label="Altyazı dışa aktarma biçimi"><option value="srt">SRT</option><option value="vtt">WebVTT</option><option value="ass">ASS + hareket</option></select><button disabled={busy || project.cues.length === 0} onclick={exportCaptions}>Dışa aktar</button></div>
          </div>
        </details>
      {/if}
    </div>

    <footer class="panel-footer">
      <button class="apply-button" disabled={busy || project.cues.length === 0} onclick={() => onApply(project)}>Timeline'a uygula</button>
      <select bind:value={exportFormat} aria-label="Hızlı dışa aktarma biçimi"><option value="srt">SRT</option><option value="vtt">VTT</option><option value="ass">ASS</option></select>
      <button class="export-icon" disabled={busy || project.cues.length === 0} onclick={exportCaptions} aria-label="Altyazıyı dışa aktar" title="Altyazıyı dışa aktar">↗</button>
    </footer>
    <input class="hidden-input" bind:this={fileInput} type="file" accept=".srt,.vtt,.ass,.ssa" onchange={handleBrowserImport} />
  </div>
{/if}

<style>
  .caption-panel,
  .caption-panel * { box-sizing: border-box; }
  button, input, textarea, select { font: inherit; }
  button { cursor: pointer; }
  button:disabled { cursor: not-allowed; opacity: .38; }
  input, textarea, select { min-width: 0; color: #dfe4e1; background: #0c0e0d; border: 1px solid #303532; border-radius: 5px; outline: none; }
  input, select { min-height: 28px; padding: 0 7px; }
  textarea { padding: 8px; resize: vertical; }
  input:focus, textarea:focus, select:focus { border-color: #59cba8; box-shadow: 0 0 0 2px #59cba81c; }

  .caption-panel { container-type: inline-size; height: 100%; min-height: 0; display: flex; flex-direction: column; overflow: hidden; color: #d9ddda; background: #101211; }
  .panel-overview { display: flex; gap: 8px; align-items: center; justify-content: space-between; padding: 9px 10px 6px; }
  .source-summary { min-width: 0; display: grid; gap: 2px; }
  .source-summary span { overflow: hidden; color: #e8ebe9; font-size: 11px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
  .source-summary small { color: #6f7672; font-size: 8px; }
  .issue-jump { flex: none; display: flex; gap: 5px; align-items: center; min-height: 25px; padding: 0 7px; color: #7e8782; background: #191c1a; border: 1px solid #2b302d; border-radius: 12px; font-size: 8px; font-weight: 700; }
  .issue-jump.alert { color: #ffc17f; background: #2d2014; border-color: #604329; }
  .issue-jump span { font-size: 13px; }
  .transcription-source { display: grid; gap: 3px; padding: 0 10px 7px; }
  .transcription-source label { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; color: #9ea6a1; font-size: 9px; font-weight: 700; }
  .transcription-source label small { color: #626b66; font-size: 7px; font-weight: 500; }
  .transcription-source select { width: 100%; min-height: 29px; color: #dce5e0; font-size: 9px; }
  .transcription-source-detail { overflow: hidden; color: #68716c; font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }
  .transcription-source-detail.empty { color: #aa8c62; white-space: normal; }
  .quick-actions { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 5px; padding: 0 10px 8px; }
  .quick-actions button { min-height: 29px; padding: 0 9px; border-radius: 5px; font-size: 9px; font-weight: 700; }
  .transcribe-button { color: #092018; background: #62d7b1; border: 1px solid #7ce7c3; }
  .compact-action, .secondary { color: #c5cac7; background: #202422; border: 1px solid #343936; }
  .notice { padding: 6px 10px; color: #8ce3c4; background: #10251e; border-block: 1px solid #1f4b3b; font-size: 9px; line-height: 1.35; }
  .notice.error { color: #ffabab; background: #2c1515; border-color: #542424; }
  .quality-strip { display: grid; grid-template-columns: repeat(3, 1fr); border-block: 1px solid #242825; background: #0d0f0e; }
  .quality-strip span { padding: 6px 3px; color: #68706c; border-right: 1px solid #202421; font-size: 7px; text-align: center; }
  .quality-strip span:last-child { border-right: 0; }
  .quality-strip strong { color: #b7bdb9; font-size: 10px; }
  .quality-strip .alert strong { color: #ff8f86; }
  .quality-strip .mixed strong { color: #86adff; }
  .quality-strip .missing strong { color: #e5bd72; }
  .panel-tabs { display: grid; grid-template-columns: repeat(3, 1fr); padding: 5px 7px 0; border-bottom: 1px solid #292d2b; background: #141715; }
  .panel-tabs button { position: relative; min-height: 29px; color: #727a75; background: transparent; border: 0; font-size: 9px; font-weight: 700; }
  .panel-tabs button::after { content: ""; position: absolute; right: 8px; bottom: -1px; left: 8px; height: 2px; border-radius: 2px; background: transparent; }
  .panel-tabs button.active { color: #dff8ef; }
  .panel-tabs button.active::after { background: #65d7b2; }
  .panel-scroll { min-height: 0; flex: 1; overflow: auto; scrollbar-width: thin; scrollbar-color: #343a36 transparent; }

  .search-field { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 5px; align-items: center; margin: 8px; padding: 0 7px; color: #6d7570; background: #0c0e0d; border: 1px solid #2d322f; border-radius: 5px; }
  .search-field input { width: 100%; padding: 0; background: transparent; border: 0; box-shadow: none; font-size: 9px; }
  .search-field button { padding: 2px; color: #7d8580; background: transparent; border: 0; }
  .filters { display: flex; gap: 4px; overflow-x: auto; padding: 0 8px 7px; scrollbar-width: none; }
  .filters::-webkit-scrollbar { display: none; }
  .filters button { flex: none; min-height: 24px; padding: 0 7px; color: #858d88; background: #191c1a; border: 1px solid #292e2b; border-radius: 12px; font-size: 8px; white-space: nowrap; }
  .filters button.active { color: #b9f2df; background: #18342b; border-color: #346e5a; }
  .filters i { margin-left: 3px; color: #69716c; font-style: normal; }
  .cue-list { border-top: 1px solid #202321; }
  .cue-row { width: 100%; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px 8px; align-items: center; padding: 8px 9px; color: inherit; background: transparent; border: 0; border-bottom: 1px solid #202321; text-align: left; }
  .cue-row:hover { background: #171a18; }
  .cue-meta { grid-column: 1; display: flex; gap: 5px; align-items: center; min-width: 0; }
  .cue-meta time { color: #69716d; font: 7px ui-monospace, monospace; }
  .cue-meta i { max-width: 90px; overflow: hidden; padding: 1px 4px; color: #bfc7c2; background: color-mix(in srgb, var(--speaker) 16%, #171a18); border-radius: 3px; font-size: 7px; font-style: normal; text-overflow: ellipsis; white-space: nowrap; }
  .cue-meta em { margin-left: auto; padding: 1px 4px; color: #ffc08a; background: #3b2516; border-radius: 7px; font-size: 7px; font-style: normal; }
  .cue-text { grid-column: 1; overflow: hidden; color: #cfd4d1; font-size: 10px; line-height: 1.45; }
  .cue-text mark { padding: 0; color: inherit; background: transparent; }
  .cue-text mark.needs-review { color: #ffd09e; text-decoration: underline; text-decoration-color: #c67a36; text-decoration-thickness: 1px; text-underline-offset: 2px; }
  .cue-text mark.low { color: #ffaaa4; text-decoration-color: #e75a52; }
  .cue-text mark.code-switch { border-bottom: 1px solid #6e9dff; }
  .row-arrow { grid-column: 2; grid-row: 1 / span 2; color: #4d5550; font-size: 15px; }

  .cue-editor { padding: 8px 9px 14px; }
  .detail-nav { display: grid; grid-template-columns: auto 1fr auto; gap: 5px; align-items: center; padding-bottom: 7px; border-bottom: 1px solid #282d2a; }
  .detail-nav > span { color: #777f7a; font-size: 8px; text-align: center; }
  .detail-nav button { min-width: 25px; height: 25px; color: #aeb5b0; background: #1a1d1b; border: 1px solid #2e3330; border-radius: 4px; font-size: 9px; }
  .detail-nav .back-button { padding: 0 7px; }
  .detail-nav > div { display: flex; gap: 3px; }
  .cue-heading { display: flex; gap: 6px; align-items: center; justify-content: space-between; padding: 9px 0 7px; }
  .cue-heading strong { color: #aeb5b1; font: 8px ui-monospace, monospace; }
  .cue-heading span { padding: 2px 5px; color: #6f7772; background: #1a1d1b; border-radius: 8px; font-size: 7px; }
  .cue-heading span.alert { color: #ffc28e; background: #3a2517; }
  .field-label, .field-grid label { display: grid; gap: 4px; color: #7c847f; font-size: 8px; }
  .field-label select { width: 100%; margin-bottom: 7px; }
  .word-strip { min-height: 48px; display: flex; flex-wrap: wrap; gap: 4px; align-content: flex-start; padding: 7px; background: #141716; border: 1px solid #292e2b; border-radius: 6px; }
  .word { display: grid; gap: 1px; padding: 4px 5px; color: #b7f0dc; background: #163329; border: 1px solid #285e4b; border-radius: 4px; font-size: 9px; }
  .word small { color: #6b9c8b; font: 6px ui-monospace, monospace; }
  .word.low { color: #ffd5d5; background: #3c1719; border-color: #8b3439; }
  .word.review { color: #ffe0bd; background: #3a2915; border-color: #7c5727; }
  .word.unknown { color: #c9cecb; background: #222624; border-color: #373d3a; }
  .word.codeswitch { box-shadow: inset 0 -2px #79a8ff; }
  .word.selected { outline: 1px solid #eef4f0; outline-offset: 1px; }
  .confidence-note { margin: 6px 1px; color: #737b76; font-size: 8px; line-height: 1.4; }
  .text-label { display: flex; justify-content: space-between; gap: 8px; margin: 10px 0 4px; color: #b9bfbb; font-size: 9px; font-weight: 700; }
  .text-label small { color: #6d7570; font-size: 7px; font-weight: 450; }
  .source-text, .translation-text { width: 100%; min-height: 68px; font-size: 12px; line-height: 1.42; }
  .translation-text { min-height: 58px; color: #d1dcff; border-color: #2d3e5e; }
  .order-select { width: 100%; margin-top: 7px; }
  .suggestions { display: grid; gap: 5px; margin-top: 7px; padding: 7px; background: #261f13; border: 1px solid #554126; border-radius: 5px; }
  .suggestions > span { color: #caa96b; font-size: 8px; font-weight: 700; }
  .suggestions button { padding: 4px 6px; color: #efdcaf; background: #382d19; border: 1px solid #67512d; border-radius: 4px; font-size: 8px; text-align: left; }
  .word-inspector { margin-top: 7px; padding: 7px; background: #171a18; border: 1px solid #343a36; border-radius: 6px; }
  .word-inspector header { display: grid; grid-template-columns: 1fr auto auto; gap: 6px; align-items: center; margin-bottom: 6px; }
  .word-inspector header span { color: #737b76; font-size: 7px; }
  .word-inspector header strong { color: #e0e5e2; font-size: 10px; }
  .word-inspector header button { color: #858d88; background: transparent; border: 0; }
  .field-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; }
  .field-grid input, .field-grid select { width: 100%; }
  .dictionary-shortcut { width: 100%; min-height: 26px; margin-top: 6px; color: #b9f0dd; background: #1b4033; border: 1px solid #326e59; border-radius: 4px; font-size: 8px; }
  .toggle-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: 10px; padding: 7px 0; border-top: 1px solid #282d2a; cursor: pointer; }
  .toggle-row span { display: grid; gap: 2px; }
  .toggle-row strong { color: #cbd0cd; font-size: 9px; }
  .toggle-row small { color: #6d7570; font-size: 7px; }
  .toggle-row input { width: 28px; height: 15px; min-height: 0; accent-color: #65d7b2; }

  .settings-section { margin: 0; padding: 11px 10px; border: 0; border-bottom: 1px solid #292d2b; }
  .section-title, .settings-section > summary { display: flex; align-items: center; justify-content: space-between; gap: 8px; color: #d3d8d5; font-size: 9px; font-weight: 750; list-style: none; }
  .settings-section > summary { cursor: pointer; }
  .settings-section > summary::-webkit-details-marker { display: none; }
  .settings-section > summary::before { content: "›"; color: #69716c; font-size: 12px; transition: transform .12s; }
  .settings-section[open] > summary::before { transform: rotate(90deg); }
  .section-title small, .settings-section > summary small { margin-left: auto; color: #6f7672; font-size: 7px; font-weight: 500; }
  .details-body { display: grid; gap: 5px; padding-top: 8px; }
  .details-body > input { width: 100%; }
  .template-list { display: grid; gap: 5px; margin-top: 8px; }
  .template-list button { display: grid; grid-template-columns: 40px minmax(0, 1fr) auto; gap: 8px; align-items: center; min-height: 48px; padding: 5px 7px; color: #9da49f; background: #181b19; border: 1px solid #2d322f; border-radius: 6px; text-align: left; }
  .template-list button.active { color: #c9f5e5; background: #173128; border-color: #3b8068; }
  .template-list i { display: grid; place-items: center; width: 40px; height: 29px; color: #fff; background: #080908; border-radius: 4px; font-size: 10px; font-style: normal; font-weight: 900; text-align: center; }
  .template-list .template-pop { color: #ffe073; transform: rotate(-2deg); }
  .template-list span { display: grid; gap: 2px; }
  .template-list strong { font-size: 9px; }
  .template-list small { color: #6e7671; font-size: 7px; }
  .template-list em { visibility: hidden; color: #65d7b2; font-style: normal; }
  .template-list button.active em { visibility: visible; }
  .compact-fields .field-grid { margin-top: 8px; }
  .inline-row { display: flex; gap: 5px; }
  .inline-row > input, .inline-row > select { min-width: 0; flex: 1; }
  .inline-row button, .file-actions button { min-height: 28px; padding: 0 8px; color: #b9f0dd; background: #1b4033; border: 1px solid #326e59; border-radius: 5px; font-size: 8px; }
  .dictionary-list { max-height: 150px; overflow: auto; }
  .dictionary-list > div { display: flex; justify-content: space-between; gap: 5px; padding: 6px 2px; border-top: 1px solid #242825; }
  .dictionary-list span { min-width: 0; display: grid; gap: 2px; }
  .dictionary-list strong { color: #dce0dd; font-size: 9px; }
  .dictionary-list small { overflow: hidden; color: #707773; font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }
  .dictionary-list button { color: #a77b77; background: transparent; border: 0; }
  .speaker-list { display: flex; flex-wrap: wrap; gap: 4px; }
  .speaker-list span { display: flex; gap: 5px; align-items: center; padding: 4px 6px; color: #b9beba; background: #1d211f; border-radius: 4px; font-size: 8px; }
  .speaker-list i { width: 6px; height: 6px; background: var(--speaker); border-radius: 50%; }
  .empty-inline { margin: 2px 0 5px; color: #69716c; font-size: 8px; line-height: 1.4; }
  .file-actions .secondary { width: 100%; color: #c7ccc9; background: #202422; border-color: #343936; }

  .empty-state { display: grid; place-items: center; padding: 34px 20px; text-align: center; }
  .empty-state > span { display: grid; place-items: center; width: 42px; height: 32px; color: #65d7b2; border: 1px solid #3d856d; border-radius: 6px; font-size: 11px; font-weight: 900; }
  .empty-state strong { margin-top: 12px; color: #d8ddda; font-size: 11px; }
  .empty-state p { max-width: 240px; margin: 5px 0 12px; color: #737b76; font-size: 9px; line-height: 1.45; }
  .primary { min-height: 29px; padding: 0 10px; color: #08110e; background: #65d7b2; border: 1px solid #7de9c5; border-radius: 5px; font-size: 9px; font-weight: 750; }
  .empty-small { padding: 20px; color: #707773; font-size: 9px; text-align: center; }
  .panel-footer { display: grid; grid-template-columns: minmax(0, 1fr) 50px 29px; gap: 5px; padding: 7px 8px; border-top: 1px solid #2b302d; background: #151817; box-shadow: 0 -8px 18px rgba(0, 0, 0, .2); }
  .panel-footer button, .panel-footer select { min-height: 29px; }
  .apply-button { color: #082018; background: #65d7b2; border: 1px solid #7ce7c3; border-radius: 5px; font-size: 9px; font-weight: 800; }
  .panel-footer select { width: 100%; padding: 0 4px; font-size: 8px; }
  .export-icon { padding: 0; color: #c7ccc9; background: #222624; border: 1px solid #363c38; border-radius: 5px; }
  .hidden-input { display: none; }

  @container (max-width: 280px) {
    .quick-actions { grid-template-columns: 1fr; }
    .compact-action { min-height: 25px !important; }
    .quality-strip span { font-size: 0; }
    .quality-strip strong { font-size: 9px; }
    .quality-strip span::after { margin-left: 3px; font-size: 7px; }
    .quality-strip span.alert::after { content: "düşük"; }
    .quality-strip span.mixed::after { content: "TR/EN"; }
    .quality-strip span.missing::after { content: "çeviri"; }
    .field-grid { grid-template-columns: 1fr; }
    .panel-footer { grid-template-columns: minmax(0, 1fr) 44px 27px; }
  }
</style>
