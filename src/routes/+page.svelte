<script lang="ts">
  import { onMount, tick } from "svelte";
  import { isTauri, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Timeline from "$lib/components/Timeline.svelte";
  import CompositorPlayer from "$lib/components/CompositorPlayer.svelte";
  import ExportDialog from "$lib/components/ExportDialog.svelte";
  import AudioExportDialog from "$lib/components/AudioExportDialog.svelte";
  import RecoveryDialog from "$lib/components/RecoveryDialog.svelte";
  import MissingMediaDialog from "$lib/components/MissingMediaDialog.svelte";
  import ProxyDialog from "$lib/components/ProxyDialog.svelte";
  import CaptionQaStudio from "$lib/components/CaptionQaStudio.svelte";
  import MediaPool from "$lib/components/MediaPool.svelte";
  import TransitionBrowser from "$lib/components/TransitionBrowser.svelte";
  import MediaDragGhost from "$lib/components/MediaDragGhost.svelte";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Splitter from "$lib/components/Splitter.svelte";
  import Panel from "$lib/components/Panel.svelte";
  import Inspector from "$lib/components/Inspector.svelte";
  import AudioEnhancementPanel from "$lib/components/AudioEnhancementPanel.svelte";
  import SmartShortsPanel, {
    type SmartShortsAnalyzeRequest,
    type SmartShortsStreamerLayout,
  } from "$lib/components/SmartShortsPanel.svelte";
  import AutoReframeDialog, {
    type AutoReframeAnalysisStatus,
    type AutoReframeApplyPayload,
    type AutoReframeRequest as AutoReframeDialogRequest,
    type AutoReframeResultSummary,
  } from "$lib/components/AutoReframeDialog.svelte";
  import PanelHeader from "$lib/components/PanelHeader.svelte";
  import type {
    FramePlan,
    TextClipStyle,
    TimelineClip,
    TimelineKeyframe,
    TimelineProjectSnapshot,
    VoiceRiderMetadata,
  } from "$lib/editor/timeline-engine";
  import {
    createTrack,
    evaluateKeyframes,
    isTimelineProjectSnapshot,
    serializeProjectState,
  } from "$lib/editor/timeline-engine";
  import {
    captionQaSummary,
    captionStudioFromWordTranscript,
    createEmptyCaptionStudioProject,
    normalizeCaptionStudioProject,
    type CaptionStudioProject,
  } from "$lib/captions/caption-studio";
  import {
    buildCaptionTimelinePlan,
    CAPTION_CLIP_SOURCE_PREFIX,
  } from "$lib/captions/caption-timeline";
  import { toRenderTimeline } from "$lib/render/timeline-adapter";
  import { planAutoReframe, type AutoReframeAspect } from "$lib/editor/auto-reframe";
  import {
    analyzeAutoReframe,
    checkAutoReframeRuntime,
    detectFacecam,
    onAutoReframeProgress,
    setupAutoReframeRuntime,
  } from "$lib/reframe/client";
  import { captureVideoFrame, type CapturedVideoFrame } from "$lib/reframe/frame-capture";
  import type { NormalizedBoundingBox } from "$lib/reframe/types";
  import {
    applyAutoReframeToFramePlan,
    applyAutoReframeToSnapshot,
    applyStreamerLayoutToRenderTimeline,
    autoReframeClipSignature,
    autoReframeRotationSupported,
    cloneAutoReframeProjectState,
    createEmptyAutoReframeProjectState,
    DEFAULT_STREAMER_LAYOUT,
    getAutoReframeExportAspects,
    isAutoReframeEntryCurrent,
    normalizeAutoReframeProjectState,
    plannerOptionsForStyle,
    removeClipAutoReframe,
    setClipAutoReframe,
    setClipStreamerLayout,
    trackerAnalysisToPlannerSamples,
    type AutoReframeProjectState,
    type ClipAutoReframeState,
    type StreamerLayoutSettings,
  } from "$lib/reframe/project-state";
  import {
    analyzeBeats,
    analyzeAudioEnergy,
    analyzeLaughter,
    analyzeAudioLoudness,
    analyzeSpeechActivity,
    analyzeSpeechTranscript,
    analyzeVoiceRider,
    onAudioEnhancementProgress,
    onLaughterAnalysisProgress,
    onSpeechSetupProgress,
    prepareAudioCleanup,
    prepareMossFormerTestCleanup,
  } from "$lib/render/client";
  import {
    beginSmartShortsAnalysis,
    cancelSmartShortsAnalysis,
    checkFireworksSmartShorts,
    finishSmartShortsAnalysis,
    rankSmartShortCandidates,
  } from "$lib/smart-shorts/client";
  import type {
    AudioEnhancementProgress,
    AudioGainKeyframe,
  } from "$lib/render/types";
  import {
    noiseReductionSignature,
    normalizeNoiseReduction,
    type NoiseReductionSettings,
  } from "$lib/editor/noise-reduction";
  import {
    applyNormalizationGain,
    limitGainForVolumeAutomation,
  } from "$lib/editor/audio-normalization";
  import { toVoiceRiderKeyframes } from "$lib/editor/voice-rider";
  import {
    BALANCED_SPEECH_SUGGESTIONS,
    buildSpeechSuggestions,
    createBeatAnalysisMetadata,
    isBeatAnalysisRangeCurrent,
    planBeatCuts,
    projectSourceBeats,
    type BeatCutMode,
    type SpeechSuggestion,
  } from "$lib/editor/audio-ai";
  import {
    discardRangesOutsideHighlights,
    extractLoudnessEnergyEvents,
    extractWaveformEnergyEvents,
    fuseNeuralLaughterEvents,
    mergeExternalSmartShortsRankings,
    planSmartShorts,
    type SmartShortsCandidate,
  } from "$lib/editor/smart-shorts";
  import {
    SmartShortsProgressEstimator,
    createSmartShortsTimingProfile,
    parseSmartShortsTimingProfile,
    type SmartShortsProgressStage,
    type SmartShortsTimingProfile,
  } from "$lib/editor/smart-shorts-progress";
  import {
    DEFAULT_AUTO_DUCKING,
    buildAutoDuckingEnvelope,
    createSpeechAnalysisMetadata,
    getAutoDuckingDiagnostics,
    isSpeechAnalysisCurrent,
    normalizeAutoDucking,
    speechAnalysisSignature,
    type AutoDuckingSettings,
  } from "$lib/editor/audio-ducking";
  import {
    applyTransportSeek,
    playbackStartTime,
    resolveTransportShortcut,
    type TransportSeekAction,
  } from "$lib/editor/transport-shortcuts";
  import { ProjectSession, type ProjectSessionState } from "$lib/project/session";
  import {
    chooseProjectSavePath,
    chooseProjectToOpen,
    discardRecoverySnapshot,
    listRecoverySnapshots,
  } from "$lib/project/client";
  import {
    auditMissingMedia,
    applyRelinkResolutions,
    importMediaPaths,
    searchForMissingMedia,
  } from "$lib/project/media";
  import {
    createProjectDocument,
    type MediaAsset,
    type MediaReference,
    type ProjectDocument,
    type RecoverySnapshotInfo,
    type RelinkMediaResult,
    type RelinkResolution,
  } from "$lib/project/types";
  import {
    normalizeProjectCover,
    type ProjectCover,
  } from "$lib/project/cover";
  import {
    layoutStore,
    panelVisibility,
    detachedPanels,
    updatePanelWidth,
    updateTimelineHeight,
    detachPanel,
    attachPanel,
  } from "$lib/stores/panels";

  // Project state
  let mediaFiles = $state<string[]>([]);
  let directPreviewPath = $state<string | null>(null);
  let timelineSnapshot = $state<TimelineProjectSnapshot | null>(null);
  let exportOpen = $state(false);
  interface AudioExportTarget {
    sourcePath: string;
    sourceLabel: string;
    startMs: number;
    durationMs: number;
    playbackRate: number;
    volume: number;
    volumeKeyframes?: AudioGainKeyframe[];
    riderGainKeyframes?: AudioGainKeyframe[];
    duckGainKeyframes?: AudioGainKeyframe[];
    noiseReduction?: NoiseReductionSettings | null;
  }
  let audioExportTarget = $state<AudioExportTarget | null>(null);
  let proxyOpen = $state(false);
  let captionStudioOpen = $state(false);
  let smartShortsStudioOpen = $state(false);
  let audioStudioOpen = $state(false);
  let captionStudio = $state<CaptionStudioProject>(createEmptyCaptionStudioProject());
  let projectCover = $state<ProjectCover | null>(null);
  let captionSourceClipId = $state<string | null>(null);
  let captionTranscriptionBusy = $state(false);
  let captionTranscriptionMessage = $state<string | null>(null);
  let captionTranscriptionError = $state(false);
  let proxyPaths = $state<Record<string, string>>({});
  let previewAspect = $state<"9:16" | "1:1" | "16:9">("16:9");
  let autoReframeState = $state<AutoReframeProjectState>(
    createEmptyAutoReframeProjectState(),
  );
  let autoReframeOpen = $state(false);
  let autoReframeOpening = $state(false);
  let autoReframeClipId = $state<string | null>(null);
  let autoReframeSeedTimeMs = $state(0);
  let autoReframeSeedFrame = $state<CapturedVideoFrame | null>(null);
  let autoReframeTarget = $state<NormalizedBoundingBox | null>(null);
  let autoReframeStatus = $state<AutoReframeAnalysisStatus>("idle");
  let autoReframeProgress = $state(0);
  let autoReframeMessage = $state<string | null>(null);
  let autoReframeError = $state<string | null>(null);
  let autoReframeResult = $state<AutoReframeResultSummary | null>(null);
  let pendingAutoReframeEntry = $state<ClipAutoReframeState | null>(null);
  let facecamDetectBusyClipId = $state<string | null>(null);
  let facecamDetectMessage = $state<string | null>(null);
  let facecamDetectMessageClipId = $state<string | null>(null);
  let facecamDetectError = $state(false);
  let pendingStreamerLayout = $state<{
    clipId: string;
    settings: StreamerLayoutSettings;
  } | null>(null);
  interface AutoReframeBackgroundJob {
    clipId: string;
    clipName: string;
    status: Exclude<AutoReframeAnalysisStatus, "idle">;
    progress: number;
    message: string;
  }
  let timelineFramePlan = $state<FramePlan>({ time: 0, layers: [], audio: [] });
  type EditorSettings = {
    previewAspect: "9:16" | "1:1" | "16:9";
    captionStudio?: CaptionStudioProject;
    cover?: ProjectCover;
    autoReframe?: AutoReframeProjectState;
  };
  type EditorDocument = ProjectDocument<TimelineProjectSnapshot, EditorSettings>;
  const DEV_ACTIVE_PROJECT_KEY = "astral-lunar.dev-active-project";
  const AUTOSAVE_DELAY_MS = import.meta.env.DEV ? 300 : 1_500;
  const AUTOSAVE_MAX_WAIT_MS = import.meta.env.DEV ? 2_000 : 12_000;
  let projectSession = $state<ProjectSession<TimelineProjectSnapshot, EditorSettings> | null>(null);
  let projectState = $state<ProjectSessionState<TimelineProjectSnapshot, EditorSettings> | null>(null);
  let projectBusy = $state(false);
  let projectError = $state<string | null>(null);
  let autosaveError = $state<string | null>(null);
  let recoverySnapshots = $state<RecoverySnapshotInfo[]>([]);
  let recoveryOpen = $state(false);
  let devRecoveryNotice = $state<string | null>(null);
  let appMenuOpen = $state(false);
  let appMenuRef = $state<HTMLDivElement>();
  let missingMediaOpen = $state(false);
  let relinkBusy = $state(false);
  let relinkResult = $state<RelinkMediaResult | null>(null);
  let applyingProject = false;
  let lastMediaSignature = "";
  let mediaSyncRequest = 0;
  let projectName = $derived(projectState?.document.name ?? "İsimsiz Proje");
  let projectDisplayError = $derived(projectError ?? autosaveError);
  let summaryCaptionIssues = $derived(captionQaSummary(captionStudio).reviewWordCount);
  let projectSaveStatus = $derived(
    projectBusy
      ? "Kaydediliyor…"
      : projectError
        ? "Kayıt hatası"
        : autosaveError || projectState?.saveStatus.phase === "error"
            ? "Otomatik kayıt hatası"
            : projectState?.saveStatus.phase === "saving"
              ? "Otomatik kaydediliyor…"
              : projectState?.dirty
                ? "Kaydedilmedi"
                : "Kaydedildi",
  );
  let missingReferences = $derived(
    (projectState?.document.media ?? [])
      .filter((asset) => asset.availability === "missing")
      .map<MediaReference>((asset) => ({
        assetId: asset.id,
        path: asset.sourcePath,
        sizeBytes: asset.sizeBytes,
        quickHash: asset.quickHash,
      })),
  );

  // Selected Clip for Inspector
  let selectedClip = $state<any>(null);
  let timelineRef = $state<any>(null);
  let libraryTab = $state<"media" | "transitions">("media");
  let transitionSide = $state<"in" | "out">("in");
  let captionTranscriptionClips = $derived.by(() => {
    const clips = timelineSnapshot?.clips ?? [];
    if (clips.length > 0) {
      return clips.filter(isCaptionTranscriptionClip);
    }
    const selected = selectedClip as TimelineClip | null;
    return isCaptionTranscriptionClip(selected) ? [selected] : [];
  });
  let captionTranscriptionClip = $derived(
    captionTranscriptionClips.find((clip) => clip.id === captionSourceClipId) ?? null,
  );
  let captionTranscriptionSources = $derived.by(() => {
    const trackNames = new Map((timelineSnapshot?.tracks ?? []).map((track) => [track.id, track.name]));
    return captionTranscriptionClips.map((clip) => {
      const detail = `${trackNames.get(clip.trackId) ?? (clip.kind === "audio" ? "Ses" : "Video")} · ${formatCaptionSourceRange(clip)}`;
      return {
        id: clip.id,
        label: `${captionSourceFileName(clip.file)} · ${detail}`,
        detail,
      };
    });
  });
  let canCaptionTranscribe = $derived(Boolean(isTauri() && captionTranscriptionClip));
  let audioNormalizeBusyClipId = $state<string | null>(null);
  let audioNormalizeMessage = $state<string | null>(null);
  let audioNormalizeMessageClipId = $state<string | null>(null);
  let audioNormalizeError = $state(false);
  let voiceRiderBusyClipId = $state<string | null>(null);
  let voiceRiderMessage = $state<string | null>(null);
  let voiceRiderMessageClipId = $state<string | null>(null);
  let voiceRiderError = $state(false);
  interface SpeechReviewState {
    signature: string;
    suggestions: SpeechSuggestion[];
    selectedIds: Set<string>;
    transcript: string;
  }
  let speechReviews = $state<Record<string, SpeechReviewState>>({});
  let speechBusyClipId = $state<string | null>(null);
  let speechMessage = $state<string | null>(null);
  let speechMessageClipId = $state<string | null>(null);
  let speechError = $state(false);
  interface SmartShortsReviewState {
    signature: string;
    candidates: SmartShortsCandidate[];
    selectedIds: Set<string>;
    glmUsed: boolean;
    engineLabel: string;
  }
  let smartShortsReviews = $state<Record<string, SmartShortsReviewState>>({});
  let smartShortsBusyClipId = $state<string | null>(null);
  let smartShortsProgress = $state(0);
  let smartShortsProgressStage = $state<string | null>(null);
  let smartShortsProgressElapsedMs = $state(0);
  let smartShortsProgressRemainingMs = $state<number | null>(null);
  let smartShortsProgressOverdue = $state(false);
  let smartShortsMessage = $state<string | null>(null);
  let smartShortsMessageClipId = $state<string | null>(null);
  let smartShortsError = $state(false);
  let smartShortsOperationId = $state<string | null>(null);
  let smartShortsCancelling = $state(false);
  let fireworksSmartShortsConfigured = $state(false);
  let duckingBusyClipId = $state<string | null>(null);
  let duckingMessage = $state<string | null>(null);
  let duckingMessageClipId = $state<string | null>(null);
  let duckingError = $state(false);
  let duckingDraftClipId = $state<string | null>(null);
  let duckingDraft = $state<AutoDuckingSettings>({ ...DEFAULT_AUTO_DUCKING });
  let beatBusyClipId = $state<string | null>(null);
  let beatMessage = $state<string | null>(null);
  let beatMessageClipId = $state<string | null>(null);
  let beatError = $state(false);
  let beatCutMode = $state<BeatCutMode>("smart");
  let beatCacheHits = $state<Record<string, boolean>>({});
  interface NoisePreviewState {
    signature: string;
    outputPath: string;
    cacheHit: boolean;
    applied: boolean;
    speechCoverage: number;
    estimatedSnrDb: number;
    strength: number;
  }
  interface MossFormerPreviewState {
    signature: string;
    outputPath: string;
    cacheHit: boolean;
    modelRevision: string;
  }
  interface MossFormerProgressState extends AudioEnhancementProgress {
    signature: string;
  }
  type NoiseAuditionMode = "original" | "cleaned" | "mossformer";
  interface NoiseAuditionState {
    signature: string;
    mode: NoiseAuditionMode;
  }
  let noisePreviewAssets = $state<Record<string, NoisePreviewState>>({});
  let noisePreviewBusy = $state<Record<string, string>>({});
  let noisePreviewFailures = $state<Record<string, { signature: string; message: string }>>({});
  // The project-load effect prepares persisted cleanup in the background.
  // Explicit button presses own their request so that background work cannot
  // steal the promise and prevent the transport from resuming afterwards.
  const manualNoiseCleanupRequests = new Set<string>();
  let mossFormerPreviewAssets = $state<Record<string, MossFormerPreviewState>>({});
  let mossFormerPreviewBusy = $state<Record<string, string>>({});
  let mossFormerPreviewProgress = $state<Record<string, MossFormerProgressState>>({});
  let mossFormerPreviewFailures = $state<
    Record<string, { signature: string; message: string }>
  >({});
  // Session-only source/noise/studio audition choice. The signature follows
  // the media range rather than the noise toggle because studio enhancement is
  // an independent feature. A selected verified studio cache asset is routed
  // to preview, render, and standalone audio export.
  let noiseAuditions = $state<Record<string, NoiseAuditionState>>({});
  let selectedClipLocked = $derived(
    Boolean(
      selectedClip &&
        timelineSnapshot?.tracks.find((track) => track.id === selectedClip.trackId)?.locked,
    ),
  );
  let selectedAutoReframeEntry = $derived.by(() => {
    const clip = selectedClip as TimelineClip | null;
    if (!clip) return null;
    const entry = autoReframeState.clips[clip.id];
    return isAutoReframeEntryCurrent(clip, entry) ? entry : null;
  });
  let selectedStreamerLayout = $derived.by(() => {
    const clip = selectedClip as TimelineClip | null;
    if (clip && pendingStreamerLayout?.clipId === clip.id) {
      return pendingStreamerLayout.settings;
    }
    return selectedAutoReframeEntry?.streamerLayout ?? DEFAULT_STREAMER_LAYOUT;
  });
  let selectedFacecamDetectBusy = $derived(
    Boolean(selectedClip && facecamDetectBusyClipId === selectedClip.id),
  );
  let selectedFacecamDetectMessage = $derived(
    selectedClip && facecamDetectMessageClipId === selectedClip.id
      ? facecamDetectMessage
      : null,
  );
  let selectedAutoReframeBusy = $derived(
    Boolean(
      selectedClip &&
        autoReframeClipId === selectedClip.id &&
        (autoReframeStatus === "preparing" || autoReframeStatus === "analyzing"),
    ),
  );
  let selectedAutoReframePending = $derived(
    Boolean(
      selectedClip &&
        autoReframeClipId === selectedClip.id &&
      pendingAutoReframeEntry,
    ),
  );
  let autoReframeBackgroundJob = $derived.by((): AutoReframeBackgroundJob | null => {
    if (!autoReframeClipId || autoReframeStatus === "idle") return null;
    if (autoReframeStatus === "completed" && !pendingAutoReframeEntry) return null;
    if (autoReframeStatus === "error" && !autoReframeError) return null;
    const clip = timelineSnapshot?.clips.find((item) => item.id === autoReframeClipId);
    const clipName = clip?.file.split(/[/\\]/).pop() || "Video klibi";
    return {
      clipId: autoReframeClipId,
      clipName,
      status: autoReframeStatus,
      progress: Math.max(0, Math.min(100, autoReframeProgress)),
      message:
        autoReframeStatus === "error"
          ? autoReframeError ?? "Akıllı kadraj analizi tamamlanamadı."
          : autoReframeMessage ??
            (autoReframeStatus === "completed"
              ? "Sonuç hazır. Açıp zaman çizelgesine uygulayın."
              : "Yerel analiz sürüyor…"),
    };
  });
  let selectedAudioNormalizeBusy = $derived(
    Boolean(audioNormalizeBusyClipId || voiceRiderBusyClipId),
  );
  let selectedAudioNormalizeMessage = $derived(
    selectedClip && audioNormalizeMessageClipId === selectedClip.id
      ? audioNormalizeMessage
      : null,
  );
  let selectedVoiceRiderBusy = $derived(
    Boolean(voiceRiderBusyClipId || audioNormalizeBusyClipId),
  );
  let selectedVoiceRiderMessage = $derived(
    selectedClip && voiceRiderMessageClipId === selectedClip.id
      ? voiceRiderMessage
      : null,
  );
  let selectedSpeechReview = $derived.by(() => {
    if (!selectedClip) return null;
    const review = speechReviews[selectedClip.id];
    return review?.signature === speechSourceSignature(selectedClip) ? review : null;
  });
  let selectedSpeechMessage = $derived(
    selectedClip && speechMessageClipId === selectedClip.id ? speechMessage : null,
  );
  let selectedSmartShortsReview = $derived.by(() => {
    const clip = selectedClip as TimelineClip | null;
    if (!clip) return null;
    const review = smartShortsReviews[clip.id];
    return review?.signature === speechSourceSignature(clip) ? review : null;
  });
  let selectedSmartShortsBusy = $derived(
    Boolean(selectedClip && smartShortsBusyClipId === selectedClip.id),
  );
  let selectedSmartShortsMessage = $derived(
    selectedClip && smartShortsMessageClipId === selectedClip.id
      ? smartShortsMessage
      : null,
  );
  let selectedSmartShortsError = $derived(
    Boolean(
      selectedClip &&
        smartShortsMessageClipId === selectedClip.id &&
        smartShortsError,
    ),
  );
  let selectedDuckingMessage = $derived(
    selectedClip && duckingMessageClipId === selectedClip.id ? duckingMessage : null,
  );
  let selectedBeatMessage = $derived(
    selectedClip && beatMessageClipId === selectedClip.id ? beatMessage : null,
  );
  let selectedNoiseReductionBusy = $derived(
    Boolean(
      selectedClip?.noiseReduction &&
      noisePreviewBusy[selectedClip.id] === noiseClipSignature(selectedClip),
    ),
  );
  let selectedNoiseReductionError = $derived.by(() => {
    if (!selectedClip?.noiseReduction) return false;
    const failure = noisePreviewFailures[selectedClip.id];
    return Boolean(failure && failure.signature === noiseClipSignature(selectedClip));
  });
  let selectedNoiseReductionMessage = $derived.by(() => {
    if (!selectedClip?.noiseReduction) return null;
    const signature = noiseClipSignature(selectedClip);
    if (noisePreviewBusy[selectedClip.id] === signature) {
      return "Klibin tamamına AI gürültü azaltma uygulanıyor (başlangıç → son)…";
    }
    if (noisePreviewBusy[selectedClip.id]) {
      return "Yeni AI ayarı tüm klibe uygulanmak üzere sırada…";
    }
    const asset = noisePreviewAssets[selectedClip.id];
    if (asset?.signature === signature) {
      if (!asset.applied) {
        return "Konuşma bulunmadı · orijinal ses değiştirilmeden korundu";
      }
      return `✓ Güçlü ortam gürültüsü temizleme klibin tamamına uygulandı · oynatma ve dışa aktarma aktif${asset.cacheHit ? " · önbellekten" : ""}`;
    }
    const failure = noisePreviewFailures[selectedClip.id];
    if (failure?.signature === signature) return failure.message;
    return isTauri()
      ? "AI gürültü azaltma klibin tamamına uygulanıyor."
      : "AI temizleme masaüstü uygulamasında klibin tamamına uygulanır.";
  });
  let selectedNoiseComparisonAvailable = $derived.by(() => {
    if (!selectedClip?.noiseReduction) return false;
    const signature = noiseClipSignature(selectedClip);
    const asset = noisePreviewAssets[selectedClip.id];
    return Boolean(asset?.applied && asset.signature === signature);
  });
  let selectedMossFormerAvailable = $derived.by(() => {
    if (!selectedClip) return false;
    const signature = mossFormerClipSignature(selectedClip);
    return mossFormerPreviewAssets[selectedClip.id]?.signature === signature;
  });
  let selectedMossFormerBusy = $derived.by(() => {
    if (!selectedClip) return false;
    return (
      mossFormerPreviewBusy[selectedClip.id] ===
      mossFormerClipSignature(selectedClip)
    );
  });
  let selectedMossFormerError = $derived.by(() => {
    if (!selectedClip) return false;
    const failure = mossFormerPreviewFailures[selectedClip.id];
    return Boolean(
      failure && failure.signature === mossFormerClipSignature(selectedClip),
    );
  });
  let selectedMossFormerProgress = $derived.by(() => {
    if (!selectedClip) return null;
    const progress = mossFormerPreviewProgress[selectedClip.id];
    return progress?.signature === mossFormerClipSignature(selectedClip)
      ? progress
      : null;
  });
  let selectedMossFormerMessage = $derived.by(() => {
    if (!selectedClip) return null;
    const signature = mossFormerClipSignature(selectedClip);
    if (mossFormerPreviewBusy[selectedClip.id] === signature) {
      return selectedMossFormerProgress?.message
        ?? "Stüdyo sesi hazırlanıyor · MossFormer2 + DeepFilterNet3 cihazda çalışır ve CPU’da uzun sürebilir…";
    }
    const asset = mossFormerPreviewAssets[selectedClip.id];
    if (asset?.signature === signature) {
      return `✓ Stüdyo sesi hazır · doğal ton ve güvenli dinamikler uygulandı${asset.cacheHit ? " · önbellekten" : ""}`;
    }
    const failure = mossFormerPreviewFailures[selectedClip.id];
    if (failure?.signature === signature) return failure.message;
    return "MossFormer2 konuşmayı ayıklar; DeepFilterNet3 kalıntıyı temizler; doğal karışım, konuşma tonu ve limiter sesi tamamlar.";
  });
  let selectedNoiseAuditionMode = $derived.by<NoiseAuditionMode>(() => {
    if (!selectedClip) return "original";
    const signature = audioSourceSignature(selectedClip);
    const audition = noiseAuditions[selectedClip.id];
    const cleaned = noisePreviewAssets[selectedClip.id];
    const cleanedAvailable = Boolean(
      selectedClip.noiseReduction &&
      cleaned?.applied &&
      cleaned.signature === noiseClipSignature(selectedClip),
    );
    const studio = mossFormerPreviewAssets[selectedClip.id];
    const studioAvailable = studio?.signature === mossFormerClipSignature(selectedClip);
    if (audition?.signature !== signature) return cleanedAvailable ? "cleaned" : "original";
    if (audition.mode === "cleaned" && !cleanedAvailable) return "original";
    if (audition.mode === "mossformer" && !studioAvailable) {
      return cleanedAvailable ? "cleaned" : "original";
    }
    return audition.mode;
  });

  // Audio-region editing state mirrored from the timeline so the Inspector
  // can host the region editor while the waveform drag stays on the timeline.
  interface AudioRegionUiState {
    selection: { start: number; end: number; volume: number } | null;
    selectMode: boolean;
    quietRegions: { start: number; end: number; volume: number }[];
  }
  let audioRegionUi = $state<AudioRegionUiState | null>(null);
  let selectedTrackMix = $derived.by(() => {
    if (!selectedClip || !timelineSnapshot) return null;
    const track = timelineSnapshot.tracks.find(
      (item) => item.id === selectedClip.trackId,
    );
    return track ? { gain: track.gain, pan: track.pan } : null;
  });
  let selectedTrackAudible = $derived.by(() => {
    if (!selectedClip || !timelineSnapshot) return true;
    const track = timelineSnapshot.tracks.find(
      (item) => item.id === selectedClip.trackId,
    );
    if (!track || track.muted) return false;
    const hasSolo = timelineSnapshot.tracks.some((item) => item.solo);
    return !hasSolo || track.solo;
  });
  let selectedDuckingTracks = $derived.by(() => {
    if (!selectedClip || !timelineSnapshot) return [];
    const settings = selectedDuckingSettings();
    return timelineSnapshot.tracks
      .filter(
        (track) =>
          track.id !== selectedClip.trackId &&
          (track.type === "audio" || track.type === "video"),
      )
      .map((track) => ({
        id: track.id,
        label: `${track.name} · ${track.type === "audio" ? "ses" : "video sesi"}`,
        selected: settings.sourceTrackIds.includes(track.id),
      }));
  });
  let selectedDuckingDiagnostics = $derived.by(() => {
    if (!selectedClip || !timelineSnapshot) return null;
    return getAutoDuckingDiagnostics(timelineSnapshot, {
      ...selectedClip,
      autoDucking: selectedDuckingSettings(),
    });
  });
  let selectedBeatUiAnalysis = $derived.by(() => {
    const analysis = selectedClip?.beatAnalysis;
    if (!analysis || !isBeatAnalysisRangeCurrent(selectedClip, analysis)) return null;
    return {
      bpm: analysis.bpm,
      beatCount: analysis.markers.length,
      downbeatCount: analysis.markers.filter((marker: { downbeat: boolean }) => marker.downbeat)
        .length,
      model: analysis.model,
      cacheHit: Boolean(beatCacheHits[selectedClip.id]),
    };
  });

  // Time & Playback
  let playheadTime = $state(0);
  let timelineContentDuration = $state(0);
  let directPreviewDuration = $state(0);
  let playheadDuration = $derived(
    directPreviewPath ? directPreviewDuration : timelineContentDuration,
  );
  let isPlaying = $state(false);
  // Explicit source swaps/rewinds must invalidate the compositor's previous
  // wall-clock sync; changing only the numeric playhead is not sufficient
  // while an older media-element play promise is still settling.
  let previewTransportRevision = $state(0);
  let selectedClipLocalTime = $derived(
    selectedClip
      ? Math.max(
          0,
          Math.min(selectedClip.duration ?? 0, playheadTime - (selectedClip.start ?? 0)),
        )
      : 0,
  );
  let selectedVoiceRiderGain = $derived.by(() => {
    if (!selectedClip) return 1;
    return evaluateKeyframes(
      selectedClip.keyframes?.riderGain,
      selectedClipLocalTime,
      1,
    );
  });
  let selectedManualVolume = $derived(
    selectedClip
      ? evaluateKeyframes(
          selectedClip.keyframes?.volume,
          selectedClipLocalTime,
          selectedClip.volume,
        )
      : 1,
  );
  let selectedEffectiveVolume = $derived.by(() => {
    if (!selectedClip) return 1;
    const activeGain = timelineFramePlan.audio.find(
      (source) => source.clipId === selectedClip.id,
    )?.gain;
    if (activeGain !== undefined) return activeGain;
    if (!selectedTrackAudible || selectedClip.audioSeparated) return 0;
    return Math.min(
      4,
      Math.max(
        0,
        selectedManualVolume * selectedVoiceRiderGain * (selectedTrackMix?.gain ?? 1),
      ),
    );
  });
  let previewPlan = $derived(
    applyNoiseCleanupOverrides(
      applyProxyOverrides(
        applyAutoReframeToFramePlan(
          directPreviewPath
            ? createDirectPreviewPlan(directPreviewPath, playheadTime)
            : timelineFramePlan,
          timelineSnapshot,
          autoReframeState,
          previewAspect,
        ),
      ),
    ),
  );
  let autoReframeExportAspects = $derived(
    getAutoReframeExportAspects(timelineSnapshot, autoReframeState),
  );
  let autoReframeTimelinesByPreset = $derived.by(() => {
    const timelines: Partial<
      Record<AutoReframeAspect, ReturnType<typeof renderTimelineWithPreparedHybrid>>
    > = {};
    if (!timelineSnapshot) return timelines;
    for (const aspect of autoReframeExportAspects) {
      const deliverySnapshot = applyAutoReframeToSnapshot(
        timelineSnapshot,
        autoReframeState,
        aspect,
      );
      timelines[aspect] = applyStreamerLayoutToRenderTimeline(
        renderTimelineWithPreparedHybrid(deliverySnapshot),
        timelineSnapshot,
        autoReframeState,
        aspect,
      );
    }
    return timelines;
  });

  // Panel Dimensions
  let sidebarWidth = $derived($layoutStore.sidebarWidth);
  let inspectorWidth = $derived($layoutStore.inspectorWidth);
  let timelineHeight = $derived($layoutStore.timelineHeight);

  // Panel Visibility
  let showSidebar = $derived($panelVisibility.sidebar);
  let showInspector = $derived($panelVisibility.inspector);
  let showTimeline = $derived($panelVisibility.timeline);

  // Detached Panels
  let isPreviewDetached = $derived("preview" in $detachedPanels);
  let previewState = $derived($detachedPanels["preview"] ?? null);
  let isMediaPoolDetached = $derived("media-pool" in $detachedPanels);
  let mediaPoolState = $derived($detachedPanels["media-pool"] ?? null);
  let isTimelineDetached = $derived("timeline" in $detachedPanels);
  let timelineState = $derived($detachedPanels["timeline"] ?? null);
  let isInspectorDetached = $derived("inspector" in $detachedPanels);
  let inspectorState = $derived($detachedPanels["inspector"] ?? null);

  // HANDLERS
  function showLibraryTab(tab: "media" | "transitions") {
    libraryTab = tab;
    if (!$panelVisibility.sidebar && !isMediaPoolDetached) {
      $panelVisibility = { ...$panelVisibility, sidebar: true };
    }
  }

  function toggleTransitionLibrary() {
    showLibraryTab(libraryTab === "transitions" ? "media" : "transitions");
  }

  function openTransitionLibraryForSide(side: "in" | "out") {
    transitionSide = side;
    showLibraryTab("transitions");
  }

  function openCaptionStudioPanel() {
    const selected = selectedClip as TimelineClip | null;
    if (isCaptionTranscriptionClip(selected)) {
      captionSourceClipId = selected.id;
    } else if (!captionTranscriptionClip) {
      captionSourceClipId = captionTranscriptionClips[0]?.id ?? null;
    }
    smartShortsStudioOpen = false;
    audioStudioOpen = false;
    captionStudioOpen = true;
    if (!$panelVisibility.inspector) {
      $panelVisibility = { ...$panelVisibility, inspector: true };
    }
    if (!isInspectorDetached && inspectorWidth < 320) {
      updatePanelWidth("inspector", 340);
    }
  }

  function closeCaptionStudioPanel() {
    captionStudioOpen = false;
  }

  function openSmartShortsStudioPanel() {
    captionStudioOpen = false;
    audioStudioOpen = false;
    smartShortsStudioOpen = true;
    if (!$panelVisibility.inspector) {
      $panelVisibility = { ...$panelVisibility, inspector: true };
    }
    if (!isInspectorDetached && inspectorWidth < 340) {
      updatePanelWidth("inspector", 370);
    }
  }

  function openClipInspectorPanel() {
    captionStudioOpen = false;
    smartShortsStudioOpen = false;
    audioStudioOpen = false;
  }

  function openAudioStudioPanel() {
    captionStudioOpen = false;
    smartShortsStudioOpen = false;
    audioStudioOpen = true;
    if (!$panelVisibility.inspector) {
      $panelVisibility = { ...$panelVisibility, inspector: true };
    }
    if (!isInspectorDetached && inspectorWidth < 340) {
      updatePanelWidth("inspector", 370);
    }
  }

  function isCaptionTranscriptionClip(
    clip: TimelineClip | null | undefined,
  ): clip is TimelineClip {
    return Boolean(
      clip?.file &&
        !clip.audioSeparated &&
        (clip.kind === "audio" || clip.kind === "video"),
    );
  }

  function captionSourceFileName(path: string) {
    return path.split(/[\\/]/u).at(-1) || "Ses/video klibi";
  }

  function formatCaptionSourceRange(clip: TimelineClip) {
    const start = Math.max(0, clip.start);
    const end = start + Math.max(0, clip.duration);
    const clock = (seconds: number) => {
      const wholeSeconds = Math.floor(seconds);
      return `${Math.floor(wholeSeconds / 60).toString().padStart(2, "0")}:${(wholeSeconds % 60).toString().padStart(2, "0")}`;
    };
    return `${clock(start)}–${clock(end)}`;
  }

  function selectCaptionSourceClip(clipId: string) {
    if (!captionTranscriptionClips.some((clip) => clip.id === clipId)) return;
    captionSourceClipId = clipId;
    captionTranscriptionMessage = null;
    captionTranscriptionError = false;
  }

  function handleMediaSelect(fileOrClip: string | any) {
    if (typeof fileOrClip === "string") {
      directPreviewPath = fileOrClip;
      directPreviewDuration = 0;
      playheadTime = 0;
      selectedClip = null;
      isPlaying = false;
    } else {
      if (!fileOrClip) {
        selectedClip = null;
        return;
      }
      directPreviewPath = null;
      playheadTime = Math.min(playheadTime, timelineContentDuration);
      selectedClip = fileOrClip;
    }
  }

  function autoReframeClip(): TimelineClip | null {
    if (!autoReframeClipId || !timelineSnapshot) return null;
    return timelineSnapshot.clips.find((clip) => clip.id === autoReframeClipId) ?? null;
  }

  async function openAutoReframeDialog() {
    const clip = selectedClip as TimelineClip | null;
    if (!clip || clip.kind !== "video" || !clip.file || selectedClipLocked) return;
    if (!autoReframeRotationSupported(clip)) {
      window.alert(
        "AI Kadraj şu anda döndürülmüş videolarda güvenli hedef konumu üretemiyor. Önce klip dönüşünü 0° yapın.",
      );
      return;
    }
    if (!isTauri()) {
      window.alert("AI Kadraj yerel takip motorunu kullandığı için masaüstü uygulamasında çalışır.");
      return;
    }
    if (
      (autoReframeStatus === "preparing" || autoReframeStatus === "analyzing") &&
      autoReframeClipId !== clip.id
    ) {
      window.alert(
        "Başka bir klibin yerel takibi sürüyor. O klibi seçip ilerlemeyi açabilir veya tamamlanmasını bekleyebilirsiniz.",
      );
      return;
    }
    if (
      autoReframeClipId === clip.id &&
      autoReframeSeedFrame &&
      (selectedAutoReframeBusy || pendingAutoReframeEntry)
    ) {
      autoReframeOpen = true;
      return;
    }
    autoReframeOpening = true;
    autoReframeError = null;
    try {
      const localTime = Math.max(
        0,
        Math.min(Math.max(0, clip.duration - 0.001), playheadTime - clip.start),
      );
      const sourceTime = Math.max(0, clip.trimIn + localTime * Math.max(0.01, clip.speed));
      const frame = await captureVideoFrame(convertFileSrc(clip.file), sourceTime);
      const currentEntry = autoReframeState.clips[clip.id];
      autoReframeClipId = clip.id;
      autoReframeSeedFrame = frame;
      autoReframeSeedTimeMs = Math.max(
        0,
        frame.capturedAtMs - Math.round(clip.trimIn * 1_000),
      );
      autoReframeTarget = isAutoReframeEntryCurrent(clip, currentEntry)
        ? { ...currentEntry.selection }
        : null;
      autoReframeStatus = "idle";
      autoReframeProgress = 0;
      autoReframeMessage = null;
      autoReframeResult = null;
      pendingAutoReframeEntry = null;
      autoReframeOpen = true;
    } catch (error) {
      window.alert(`Seçim karesi açılamadı: ${formatAutoReframeError(error)}`);
    } finally {
      autoReframeOpening = false;
    }
  }

  function handleAutoReframeTargetChange(box: NormalizedBoundingBox | null) {
    autoReframeTarget = box ? { ...box } : null;
  }

  async function handleAutoReframeAnalyze(request: AutoReframeDialogRequest) {
    const clip = autoReframeClip();
    if (!clip || clip.kind !== "video") {
      autoReframeStatus = "error";
      autoReframeError = "Video klibi artık zaman çizelgesinde bulunmuyor.";
      return;
    }
    const signature = autoReframeClipSignature(clip);
    const speed = Math.max(0.01, clip.speed);
    const analyzedDurationMs = Math.max(1, Math.round(clip.duration * speed * 1_000));
    autoReframeTarget = { ...request.targetBox };
    autoReframeStatus = "preparing";
    autoReframeProgress = 0;
    autoReframeMessage = "Ücretsiz yerel takip motoru kontrol ediliyor…";
    autoReframeError = null;
    autoReframeResult = null;
    pendingAutoReframeEntry = null;
    let unlistenProgress: (() => void) | null = null;
    try {
      unlistenProgress = await onAutoReframeProgress((progress) => {
        autoReframeStatus = progress.operation === "analysis" ? "analyzing" : "preparing";
        if (progress.progressPercent !== null) {
          autoReframeProgress = Math.max(0, Math.min(100, progress.progressPercent));
        }
        autoReframeMessage = progress.message;
      });
      let runtime = await checkAutoReframeRuntime();
      if (!runtime.ready) runtime = await setupAutoReframeRuntime();
      if (!runtime.ready) {
        throw runtime.diagnostic ?? new Error("Yerel OpenCV takip motoru hazırlanamadı.");
      }

      autoReframeStatus = "analyzing";
      autoReframeMessage = "Kişi/nesne klip boyunca takip ediliyor…";
      const baseSampleFps = request.style === "dynamic" ? 8 : request.style === "calm" ? 4 : 6;
      // Keep the delivered-timeline sampling dense enough for slow-motion clips.
      const sampleFps = Math.max(0.5, Math.min(12, baseSampleFps / speed));
      const timelineSampleIntervalMs = 1_000 / (sampleFps * speed);
      const analysis = await analyzeAutoReframe({
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: analyzedDurationMs,
        seedTimeMs: Math.min(
          Math.max(0, Math.round(autoReframeSeedTimeMs)),
          Math.max(0, analyzedDurationMs - 1),
        ),
        selection: request.targetBox,
        sampleFps,
        analysisMaxDimension: 960,
      });
      // Backend timestamps are source-relative; timeline animation is after speed.
      const samples = trackerAnalysisToPlannerSamples(analysis, speed, request.framing);
      const coverageToleranceMs = Math.max(1_500, timelineSampleIntervalMs * 2.1);
      const firstSampleTimeMs = samples[0]?.timeMs ?? Number.POSITIVE_INFINITY;
      const lastSampleTimeMs = samples.at(-1)?.timeMs ?? Number.NEGATIVE_INFINITY;
      const timelineDurationMs = clip.duration * 1_000;
      if (
        firstSampleTimeMs > coverageToleranceMs ||
        timelineDurationMs - lastSampleTimeMs > coverageToleranceMs
      ) {
        throw new Error(
          "Takip klibin tamamını güvenle kapsamadı. Hedefin net göründüğü başka bir kare seçin veya klibi bölerek yeniden deneyin.",
        );
      }
      const planned = planAutoReframe({
        sourceWidth: analysis.sourceWidth,
        sourceHeight: analysis.sourceHeight,
        samples,
        options: {
          ...plannerOptionsForStyle(request.style),
          maxSampleIntervalMs: Math.max(1_500, timelineSampleIntervalMs * 1.5),
          maxConfidenceHoldMs: Math.max(900, timelineSampleIntervalMs * 2.2),
          // A momentary tracking loss (streamer looking away, occlusion) must not
          // abort a long analysis; hold the last reliable box and warn instead.
          holdThroughLowConfidence: true,
        },
      });
      if (!planned.ok) {
        throw new Error(
          [planned.error.message, ...(planned.error.details ?? [])].filter(Boolean).join(" "),
        );
      }
      const aspects = request.aspects.filter(
        (aspect): aspect is AutoReframeAspect =>
          aspect === "9:16" || aspect === "1:1" || aspect === "16:9",
      );
      const warnings = [
        ...analysis.warnings,
        ...planned.plan.warnings,
        ...(planned.rejectedSamples.length
          ? [`${planned.rejectedSamples.length} geçersiz takip örneği kullanılmadı.`]
          : []),
      ];
      pendingAutoReframeEntry = {
        clipSignature: signature,
        aspects,
        style: request.style,
        framing: request.framing,
        selection: { ...request.targetBox },
        engine: `${analysis.engine} ${analysis.engineVersion}`.trim(),
        generatedAtMs: Date.now(),
        warnings: [...new Set(warnings)],
        plan: planned.plan,
        ...(isAutoReframeEntryCurrent(clip, autoReframeState.clips[clip.id]) &&
        autoReframeState.clips[clip.id].streamerLayout
          ? { streamerLayout: autoReframeState.clips[clip.id].streamerLayout }
          : {}),
      };
      autoReframeResult = {
        shotCount: 1,
        keyframeCount: aspects.reduce(
          (count, aspect) => count + planned.plan.variants[aspect].cameraKeyframes.length,
          0,
        ),
        lowConfidenceCount: planned.plan.heldSampleCount,
      };
      autoReframeProgress = 100;
      autoReframeMessage = warnings.length
        ? `Kamera yolları hazır · ${warnings.length} kontrol notu var.`
        : `${aspects.length} teslim oranı için kamera yolları hazır.`;
      autoReframeStatus = "completed";
    } catch (error) {
      autoReframeStatus = "error";
      autoReframeError = formatAutoReframeError(error);
      autoReframeMessage = null;
    } finally {
      unlistenProgress?.();
    }
  }

  async function handleAutoReframeApply(payload: AutoReframeApplyPayload) {
    const clip = autoReframeClip();
    const entry = pendingAutoReframeEntry;
    if (!clip || !entry || entry.clipSignature !== autoReframeClipSignature(clip)) {
      autoReframeStatus = "error";
      autoReframeError = "Klip analizden sonra değişti. Güvenli sonuç için yeniden analiz edin.";
      return;
    }
    // Timeline history owns external snapshots, so the AI plan is one Ctrl+Z step.
    timelineRef?.commitHistory?.();
    const requestedLayout =
      pendingStreamerLayout?.clipId === clip.id ? pendingStreamerLayout.settings : null;
    const appliedEntry: ClipAutoReframeState = requestedLayout
      ? {
          ...entry,
          aspects: [...new Set<AutoReframeAspect>([...entry.aspects, "9:16"])],
          streamerLayout: requestedLayout,
        }
      : entry;
    autoReframeState = setClipAutoReframe(autoReframeState, clip.id, appliedEntry);
    pendingStreamerLayout = null;
    const firstAspect = payload.request.aspects[0];
    if (firstAspect === "9:16" || firstAspect === "1:1" || firstAspect === "16:9") {
      previewAspect = firstAspect;
    }
    persistAutoReframeSettings();
    autoReframeOpen = false;
    pendingAutoReframeEntry = null;
  }

  function closeAutoReframeDialog() {
    autoReframeOpen = false;
    if (autoReframeStatus === "preparing" || autoReframeStatus === "analyzing") return;
    if (pendingStreamerLayout?.clipId === autoReframeClipId) pendingStreamerLayout = null;
    pendingAutoReframeEntry = null;
    autoReframeError = null;
    autoReframeStatus = "idle";
  }

  function openAutoReframeBackgroundJob(clipId = autoReframeClipId) {
    if (!clipId || clipId !== autoReframeClipId || !autoReframeBackgroundJob) return;
    autoReframeOpen = true;
  }

  function removeSelectedAutoReframe() {
    const clip = selectedClip as TimelineClip | null;
    if (!clip || !autoReframeState.clips[clip.id] || selectedClipLocked) return;
    timelineRef?.commitHistory?.();
    autoReframeState = removeClipAutoReframe(autoReframeState, clip.id);
    if (pendingStreamerLayout?.clipId === clip.id) pendingStreamerLayout = null;
    persistAutoReframeSettings();
  }

  function toStreamerLayoutSettings(
    layout: SmartShortsStreamerLayout,
  ): StreamerLayoutSettings {
    return {
      enabled: layout.enabled,
      facePosition: layout.facePosition,
      faceFraction: layout.faceFraction,
      contentFocusX: selectedStreamerLayout.contentFocusX,
      contentFocusY: selectedStreamerLayout.contentFocusY,
    };
  }

  function handleStreamerLayoutChange(layout: SmartShortsStreamerLayout) {
    const clip = selectedClip as TimelineClip | null;
    if (!clip || clip.kind !== "video" || selectedClipLocked) return;
    const settings = toStreamerLayoutSettings(layout);
    if (settings.enabled) previewAspect = "9:16";
    if (!selectedAutoReframeEntry) {
      if (!settings.enabled) {
        pendingStreamerLayout = null;
        return;
      }
      pendingStreamerLayout = { clipId: clip.id, settings };
      void openAutoReframeDialog();
      return;
    }
    timelineRef?.commitHistory?.();
    autoReframeState = setClipStreamerLayout(autoReframeState, clip, settings);
    persistAutoReframeSettings();
  }

  function handleSelectStreamerTarget() {
    const clip = selectedClip as TimelineClip | null;
    if (!clip || clip.kind !== "video" || selectedClipLocked) return;
    pendingStreamerLayout = {
      clipId: clip.id,
      settings: { ...selectedStreamerLayout, enabled: true },
    };
    previewAspect = "9:16";
    void openAutoReframeDialog();
  }

  async function handleAutoDetectFacecam() {
    const clip = selectedClip as TimelineClip | null;
    if (
      !clip ||
      clip.kind !== "video" ||
      !clip.file ||
      selectedClipLocked ||
      facecamDetectBusyClipId
    ) return;
    facecamDetectMessageClipId = clip.id;
    if (!isTauri()) {
      facecamDetectMessage = "Otomatik facecam algılama masaüstü uygulamasında çalışır.";
      facecamDetectError = true;
      return;
    }
    if (!autoReframeRotationSupported(clip)) {
      facecamDetectMessage =
        "Algılama döndürülmüş videoda çalışmaz; önce klip dönüşünü 0° yapın.";
      facecamDetectError = true;
      return;
    }
    facecamDetectBusyClipId = clip.id;
    facecamDetectMessage = "Yerel yüz algılama motoru hazırlanıyor…";
    facecamDetectError = false;
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onAutoReframeProgress((progress) => {
        if (facecamDetectBusyClipId !== clip.id) return;
        facecamDetectMessage = progress.message;
      });
      let runtime = await checkAutoReframeRuntime();
      if (!runtime.ready) runtime = await setupAutoReframeRuntime();
      if (!runtime.ready) {
        throw runtime.diagnostic ?? new Error("Yerel OpenCV motoru hazırlanamadı.");
      }
      const speed = Math.max(0.01, clip.speed);
      const durationMs = Math.max(1, Math.round(clip.duration * speed * 1_000));
      const detection = await detectFacecam({
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs,
      });
      const samples = trackerAnalysisToPlannerSamples(detection, speed, "auto");
      const intervalTimelineMs = Math.max(1, detection.intervalMs / speed);
      const planned = planAutoReframe({
        sourceWidth: detection.sourceWidth,
        sourceHeight: detection.sourceHeight,
        samples,
        options: {
          ...plannerOptionsForStyle("calm"),
          maxSampleIntervalMs: Math.min(60_000, Math.max(1_500, intervalTimelineMs * 2.5)),
          maxConfidenceHoldMs: Math.min(60_000, Math.max(900, intervalTimelineMs * 2.2)),
          // Detection intentionally emits low-confidence held samples for
          // face-free stretches; hold the facecam box instead of failing.
          holdThroughLowConfidence: true,
        },
      });
      if (!planned.ok) {
        throw new Error(
          [planned.error.message, ...(planned.error.details ?? [])].filter(Boolean).join(" "),
        );
      }
      const median = detection.samples[Math.floor(detection.samples.length / 2)];
      // The face panel frames these detected camera boxes directly; timestamps
      // move from source time to timeline time to match the plan keyframes.
      const faceTrack = detection.samples
        .map((sample) => ({
          timeMs: Math.round(sample.timeMs / speed),
          centerX: sample.bbox.x + sample.bbox.width / 2,
          centerY: sample.bbox.y + sample.bbox.height / 2,
          width: sample.bbox.width,
          height: sample.bbox.height,
        }))
        .filter(
          (sample, index, all) => index === 0 || sample.timeMs > all[index - 1].timeMs,
        );
      timelineRef?.commitHistory?.();
      const entry: ClipAutoReframeState = {
        clipSignature: autoReframeClipSignature(clip),
        aspects: ["9:16"],
        style: "calm",
        framing: "auto",
        selection: { ...median.bbox },
        engine: `${detection.engine} ${detection.engineVersion}`.trim(),
        generatedAtMs: Date.now(),
        warnings: [...new Set([...detection.warnings, ...planned.plan.warnings])],
        plan: planned.plan,
        streamerLayout: { ...selectedStreamerLayout, enabled: true },
        faceTrack,
      };
      autoReframeState = setClipAutoReframe(autoReframeState, clip.id, entry);
      pendingStreamerLayout = null;
      previewAspect = "9:16";
      persistAutoReframeSettings();
      facecamDetectMessage = `✓ Facecam otomatik bulundu · ${detection.samples.length} örnek${detection.cacheHit ? " · önbellek" : ""}`;
      facecamDetectError = false;
    } catch (error) {
      facecamDetectMessage = formatAutoReframeError(error);
      facecamDetectError = true;
    } finally {
      unlisten?.();
      if (facecamDetectBusyClipId === clip.id) facecamDetectBusyClipId = null;
    }
  }

  function persistAutoReframeSettings() {
    if (!projectSession || applyingProject || !isTauri()) return;
    projectSession.update((document) => ({
      ...document,
      settings: editorSettings(),
    }));
  }

  function formatAutoReframeError(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (error && typeof error === "object") {
      const value = error as { userMessage?: unknown; message?: unknown; technicalMessage?: unknown };
      if (typeof value.userMessage === "string") return value.userMessage;
      if (typeof value.message === "string") return value.message;
      if (typeof value.technicalMessage === "string") return value.technicalMessage;
    }
    return typeof error === "string" ? error : "AI Kadraj analizi tamamlanamadı.";
  }

  function handleAddMediaToTimeline(file: string) {
    void timelineRef?.addMediaAtPlayhead?.(file);
  }

  // Undo/redo integration: the timeline owns the history stacks; these
  // callbacks fold the media pool into every snapshot so Ctrl+Z restores it.
  interface ExternalUndoState {
    mediaFiles: string[];
    autoReframe: AutoReframeProjectState;
  }

  function captureExternalUndoState(): ExternalUndoState {
    return {
      mediaFiles: [...mediaFiles],
      autoReframe: cloneAutoReframeProjectState(autoReframeState),
    };
  }

  function restoreExternalUndoState(snapshot: unknown) {
    const state = snapshot as Partial<ExternalUndoState> | null;
    if (Array.isArray(state?.mediaFiles)) {
      mediaFiles = [...state.mediaFiles];
    }
    if (state?.autoReframe) {
      autoReframeState = normalizeAutoReframeProjectState(state.autoReframe);
    }
  }

  function commitUndoPoint() {
    timelineRef?.commitHistory?.();
  }

  function openAudioExtraction(file: string) {
    audioExportTarget = {
      sourcePath: file,
      sourceLabel: fileNameFromPath(file),
      startMs: 0,
      // The dialog probes a full source when a media-pool item has no known
      // duration yet.
      durationMs: 0,
      playbackRate: 1,
      volume: 1,
    };
  }

  function openTimelineAudioExport(clip: TimelineClip) {
    if (!clip.file || (clip.kind !== "video" && clip.kind !== "audio")) return;
    if (clip.autoDucking && timelineSnapshot) {
      const diagnostics = getAutoDuckingDiagnostics(timelineSnapshot, clip);
      if (diagnostics.enabled && diagnostics.staleSourceClipIds.length > 0) {
        duckingMessageClipId = clip.id;
        duckingMessage = "Konuşma klibi değişti; sesi dışa aktarmadan önce AI Ducking’i yeniden analiz edin.";
        duckingError = true;
        return;
      }
    }
    const playbackRate = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
    const hybridAsset = selectedHybridAssetForClip(clip);
    audioExportTarget = {
      sourcePath: hybridAsset?.outputPath ?? clip.file,
      sourceLabel: fileNameFromPath(clip.file),
      startMs: hybridAsset ? 0 : Math.max(0, Math.round(clip.trimIn * 1_000)),
      // A timeline clip's visible duration is after speed has been applied.
      durationMs: Math.max(1, Math.round(clip.duration * playbackRate * 1_000)),
      playbackRate,
      volume: Math.max(0, Number.isFinite(clip.volume) ? clip.volume : 1),
      volumeKeyframes: toAudioGainKeyframes(clip.keyframes.volume),
      riderGainKeyframes: toAudioGainKeyframes(clip.keyframes.riderGain),
      duckGainKeyframes: timelineSnapshot
        ? toAudioGainKeyframes(buildAutoDuckingEnvelope(timelineSnapshot, clip))
        : [],
      noiseReduction: hybridAsset ? null : clip.noiseReduction,
    };
  }

  function selectedHybridAssetForClip(clip: TimelineClip): MossFormerPreviewState | null {
    const audition = noiseAuditions[clip.id];
    if (
      audition?.mode !== "mossformer" ||
      audition.signature !== audioSourceSignature(clip)
    ) return null;
    const asset = mossFormerPreviewAssets[clip.id];
    return asset?.signature === mossFormerClipSignature(clip) ? asset : null;
  }

  function renderTimelineWithPreparedHybrid(snapshot: TimelineProjectSnapshot) {
    const timeline = toRenderTimeline(snapshot);
    const clips = new Map(snapshot.clips.map((clip) => [clip.id, clip]));
    return {
      ...timeline,
      clips: timeline.clips.map((renderClip) => {
        const clip = clips.get(renderClip.id);
        const asset = clip ? selectedHybridAssetForClip(clip) : null;
        return asset
          ? {
              ...renderClip,
              audioPath: asset.outputPath,
              audioTrimInMs: 0,
              noiseReduction: null,
            }
          : renderClip;
      }),
    };
  }

  function toAudioGainKeyframes(
    frames: readonly TimelineKeyframe[] | undefined,
  ): AudioGainKeyframe[] {
    return (frames ?? []).map((frame) => ({
      atMs: Math.max(0, Math.round(frame.time * 1_000)),
      value: frame.value,
      easing: frame.easing,
    }));
  }

  async function handleAudioExportCompleted(outputPath: string, addToTimeline: boolean) {
    if (!mediaFiles.some((file) => normalizeMediaPath(file) === normalizeMediaPath(outputPath))) {
      commitUndoPoint();
      mediaFiles = [...mediaFiles, outputPath];
    }
    if (addToTimeline) {
      await tick();
      await timelineRef?.addMediaAtPlayhead?.(outputPath);
    }
  }

  function fileNameFromPath(path: string): string {
    return path.split(/[/\\]/).pop() || path;
  }

  function handleInspectorUpdate(key: string, value: number) {
    if (!selectedClip || !timelineRef || selectedClipLocked) return;
    if (selectedAutoReframeEntry && (key === "x" || key === "y" || key === "scale")) return;

    if (key === "volume") {
      selectedClip = { ...selectedClip, volume: value };
    } else {
      selectedClip = {
        ...selectedClip,
        transform: { ...selectedClip.transform, [key]: value },
      };
    }
    timelineRef.updateClip(
      selectedClip.id,
      { [key]: value },
      { history: "coalesce" },
    );
  }

  function noiseClipSignature(clip: Partial<TimelineClip>): string {
    return JSON.stringify([
      audioSourceSignature(clip),
      noiseReductionSignature(clip.noiseReduction),
    ]);
  }

  function audioSourceSignature(clip: Partial<TimelineClip>): string {
    return JSON.stringify([
      clip.file,
      clip.trimIn,
      clip.duration,
      clip.speed,
    ]);
  }

  function mossFormerClipSignature(clip: Partial<TimelineClip>): string {
    return JSON.stringify([
      audioSourceSignature(clip),
      "studio-voice-mossformer2-se-48k+deepfilternet3-atten6+natural92-gain145-v3",
    ]);
  }

  function createAudioEnhancementOperationId(): string {
    const randomId = globalThis.crypto?.randomUUID?.()
      ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
    return `audio-enhancement-${randomId}`;
  }

  function speechSourceSignature(clip: Partial<TimelineClip>): string {
    return speechAnalysisSignature({
      file: clip.file,
      trimIn: clip.trimIn,
      duration: clip.duration,
      speed: clip.speed,
      noiseReduction: clip.noiseReduction,
    });
  }

  function selectedDuckingSettings(): AutoDuckingSettings {
    if (selectedClip && duckingDraftClipId === selectedClip.id) return duckingDraft;
    return (
      normalizeAutoDucking(selectedClip?.autoDucking) ?? {
        ...DEFAULT_AUTO_DUCKING,
        sourceTrackIds: [],
      }
    );
  }

  $effect(() => {
    const clip = selectedClip as TimelineClip | null;
    if (!clip || (clip.kind !== "audio" && clip.kind !== "video")) {
      duckingDraftClipId = null;
      return;
    }
    if (duckingDraftClipId === clip.id) return;
    const existing = normalizeAutoDucking(clip.autoDucking);
    const suggestedTrackIds = (timelineSnapshot?.tracks ?? [])
      .filter(
        (track) =>
          track.id !== clip.trackId &&
          /voice|konuşma|speech/i.test(track.name),
      )
      .map((track) => track.id);
    duckingDraft = existing ?? {
      ...DEFAULT_AUTO_DUCKING,
      sourceTrackIds: suggestedTrackIds,
    };
    duckingDraftClipId = clip.id;
  });

  async function prepareNoiseCleanupForClip(
    clip: TimelineClip,
    force = false,
  ): Promise<boolean> {
    const settings = normalizeNoiseReduction(clip.noiseReduction);
    if (
      !settings ||
      !clip.file ||
      clip.audioSeparated ||
      (clip.kind !== "video" && clip.kind !== "audio") ||
      !isTauri()
    ) return false;
    const signature = noiseClipSignature(clip);
    if (!force && noisePreviewAssets[clip.id]?.signature === signature) return true;
    if (!force && noisePreviewFailures[clip.id]?.signature === signature) return false;
    if (noisePreviewBusy[clip.id]) return false;

    noisePreviewBusy = { ...noisePreviewBusy, [clip.id]: signature };
    const { [clip.id]: _failure, ...remainingFailures } = noisePreviewFailures;
    noisePreviewFailures = remainingFailures;
    try {
      const playbackRate = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
      const asset = await prepareAudioCleanup({
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: Math.max(1, Math.round(clip.duration * playbackRate * 1_000)),
        settings,
      });
      const current = (
        timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined
      )?.clips.find((item) => item.id === clip.id)
        ?? timelineSnapshot?.clips.find((item) => item.id === clip.id);
      if (!current || noiseClipSignature(current) !== signature) return false;
      noisePreviewAssets = {
        ...noisePreviewAssets,
        [clip.id]: {
          signature,
          outputPath: asset.outputPath,
          cacheHit: asset.cacheHit,
          applied: asset.applied,
          speechCoverage: asset.speechCoverage,
          estimatedSnrDb: asset.estimatedSnrDb,
          strength: asset.strength,
        },
      };
      // A completed render becomes the audible B side immediately. A previous
      // A selection must never carry across a changed clip/settings signature.
      noiseAuditions = {
        ...noiseAuditions,
        [clip.id]: { signature: audioSourceSignature(current), mode: "cleaned" },
      };
      return true;
    } catch (error) {
      const current = (
        timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined
      )?.clips.find((item) => item.id === clip.id)
        ?? timelineSnapshot?.clips.find((item) => item.id === clip.id);
      if (current && noiseClipSignature(current) === signature) {
        noisePreviewFailures = {
          ...noisePreviewFailures,
          [clip.id]: { signature, message: formatAudioCommandError(error) },
        };
      }
      return false;
    } finally {
      if (noisePreviewBusy[clip.id] === signature) {
        const { [clip.id]: _busy, ...remainingBusy } = noisePreviewBusy;
        noisePreviewBusy = remainingBusy;
      }
    }
  }

  async function prepareMossFormerForClip(
    clip: TimelineClip,
    force = false,
  ): Promise<boolean> {
    if (
      !clip.file ||
      clip.audioSeparated ||
      (clip.kind !== "video" && clip.kind !== "audio") ||
      !isTauri()
    ) return false;
    const signature = mossFormerClipSignature(clip);
    if (!force && mossFormerPreviewAssets[clip.id]?.signature === signature) return true;
    if (!force && mossFormerPreviewFailures[clip.id]?.signature === signature) return false;
    if (mossFormerPreviewBusy[clip.id]) return false;

    const operationId = createAudioEnhancementOperationId();
    mossFormerPreviewBusy = { ...mossFormerPreviewBusy, [clip.id]: signature };
    mossFormerPreviewProgress = {
      ...mossFormerPreviewProgress,
      [clip.id]: {
        signature,
        operationId,
        sequence: -1,
        phase: "queued",
        overallProgressPercent: 0,
        phaseProgressPercent: null,
        processedMs: null,
        totalMs: Math.max(1, Math.round(clip.duration * 1_000)),
        message: "Stüdyo ses geliştirme sıraya alındı…",
      },
    };
    const { [clip.id]: _failure, ...remainingFailures } = mossFormerPreviewFailures;
    mossFormerPreviewFailures = remainingFailures;
    let unlistenProgress: (() => void) | null = null;
    try {
      unlistenProgress = await onAudioEnhancementProgress((progress) => {
        const activeProgress = mossFormerPreviewProgress[clip.id];
        if (
          progress.operationId !== operationId ||
          activeProgress?.operationId !== operationId ||
          progress.sequence <= activeProgress.sequence ||
          mossFormerPreviewBusy[clip.id] !== signature
        ) return;
        const current = (
          timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined
        )?.clips.find((item) => item.id === clip.id)
          ?? timelineSnapshot?.clips.find((item) => item.id === clip.id);
        if (!current || mossFormerClipSignature(current) !== signature) return;
        mossFormerPreviewProgress = {
          ...mossFormerPreviewProgress,
          [clip.id]: { ...progress, signature },
        };
      });
      const playbackRate = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
      const asset = await prepareMossFormerTestCleanup({
        operationId,
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: Math.max(1, Math.round(clip.duration * playbackRate * 1_000)),
        force,
      });
      const current = (
        timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined
      )?.clips.find((item) => item.id === clip.id)
        ?? timelineSnapshot?.clips.find((item) => item.id === clip.id);
      if (
        !current ||
        mossFormerClipSignature(current) !== signature
      ) return false;
      mossFormerPreviewAssets = {
        ...mossFormerPreviewAssets,
        [clip.id]: {
          signature,
          outputPath: asset.outputPath,
          cacheHit: asset.cacheHit,
          modelRevision: asset.modelRevision,
        },
      };
      return true;
    } catch (error) {
      const current = (
        timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined
      )?.clips.find((item) => item.id === clip.id)
        ?? timelineSnapshot?.clips.find((item) => item.id === clip.id);
      if (current && mossFormerClipSignature(current) === signature) {
        mossFormerPreviewFailures = {
          ...mossFormerPreviewFailures,
          [clip.id]: {
            signature,
            message: `Stüdyo sesi hazırlanamadı: ${formatAudioCommandError(error)}`,
          },
        };
      }
      return false;
    } finally {
      unlistenProgress?.();
      if (mossFormerPreviewProgress[clip.id]?.operationId === operationId) {
        const { [clip.id]: _progress, ...remainingProgress } = mossFormerPreviewProgress;
        mossFormerPreviewProgress = remainingProgress;
      }
      if (mossFormerPreviewBusy[clip.id] === signature) {
        const { [clip.id]: _busy, ...remainingBusy } = mossFormerPreviewBusy;
        mossFormerPreviewBusy = remainingBusy;
      }
    }
  }

  function ownsNoiseCleanupTransport(
    clipId: string,
    clipStart: number,
    revision: number,
  ): boolean {
    return Boolean(
      selectedClip?.id === clipId &&
      previewTransportRevision === revision &&
      !isPlaying &&
      Math.abs(playheadTime - clipStart) < 1 / 60
    );
  }

  async function handleNoiseReductionChange(settings: NoiseReductionSettings | null) {
    if (!selectedClip || !timelineRef || selectedClipLocked) return;
    const normalized = normalizeNoiseReduction(settings);
    if (
      noiseReductionSignature(selectedClip.noiseReduction) ===
      noiseReductionSignature(normalized)
    ) return;

    const { riderGain: _riderGain, ...manualKeyframes } = selectedClip.keyframes;
    const removedRider = Boolean(selectedClip.voiceRider);
    const updates = {
      noiseReduction: normalized,
      keyframes: manualKeyframes,
      voiceRider: null,
    };
    const clipId = selectedClip.id;
    timelineRef.updateClip(clipId, updates, { history: "immediate" });
    const updatedClip = { ...selectedClip, ...updates } as TimelineClip;
    selectedClip = updatedClip;
    if (removedRider) {
      voiceRiderMessageClipId = selectedClip.id;
      voiceRiderMessage =
        "Temizleme değiştiği için eski Voice Rider analizi kaldırıldı; yeniden analiz edin.";
      voiceRiderError = false;
    }
    const { [clipId]: _asset, ...remainingAssets } = noisePreviewAssets;
    const { [clipId]: _failure, ...remainingFailures } = noisePreviewFailures;
    const { [clipId]: _audition, ...remainingAuditions } = noiseAuditions;
    noisePreviewAssets = remainingAssets;
    noisePreviewFailures = remainingFailures;
    noiseAuditions = remainingAuditions;
    if (!normalized) return;

    // Applying the switch is an explicit whole-clip action. Stop the old,
    // untreated audio, render the complete visible clip, then audition the
    // result from the clip's local 0:00 instead of waiting for a side effect.
    const shouldResume = isPlaying;
    directPreviewPath = null;
    isPlaying = false;
    playheadTime = updatedClip.start;
    const rewindRevision = ++previewTransportRevision;
    const manualRequestKey = `${clipId}:${noiseClipSignature(updatedClip)}`;
    manualNoiseCleanupRequests.add(manualRequestKey);
    // Let the compositor commit the pause + seek before a long native render
    // begins. This prevents an older asynchronous play() from surviving it.
    await tick();
    let applied = false;
    try {
      applied = await prepareNoiseCleanupForClip(updatedClip, true);
    } finally {
      manualNoiseCleanupRequests.delete(manualRequestKey);
    }
    if (ownsNoiseCleanupTransport(clipId, updatedClip.start, rewindRevision)) {
      // A failed render leaves the original source valid. Restore the user's
      // prior playback intent instead of marooning the transport in pause.
      // Successful renders get one more discontinuity so the new audio asset
      // is sought before playback resumes.
      let resumeRevision = rewindRevision;
      if (applied) {
        playheadTime = updatedClip.start;
        resumeRevision = ++previewTransportRevision;
        await tick();
      }
      if (
        ownsNoiseCleanupTransport(clipId, updatedClip.start, resumeRevision) &&
        shouldResume
      ) {
        isPlaying = true;
      }
    }
  }

  async function retryNoiseReductionPreview() {
    if (!selectedClip?.noiseReduction) return;
    const clip = selectedClip as TimelineClip;
    const { [clip.id]: _failure, ...remainingFailures } = noisePreviewFailures;
    noisePreviewFailures = remainingFailures;
    directPreviewPath = null;
    const shouldResume = isPlaying;
    isPlaying = false;
    playheadTime = clip.start;
    const rewindRevision = ++previewTransportRevision;
    await tick();
    const applied = await prepareNoiseCleanupForClip(clip, true);
    if (ownsNoiseCleanupTransport(clip.id, clip.start, rewindRevision)) {
      let resumeRevision = rewindRevision;
      if (applied) {
        playheadTime = clip.start;
        resumeRevision = ++previewTransportRevision;
        await tick();
      }
      if (
        ownsNoiseCleanupTransport(clip.id, clip.start, resumeRevision) &&
        shouldResume
      ) {
        isPlaying = true;
      }
    }
  }

  function handleNoiseAuditionModeChange(mode: NoiseAuditionMode) {
    if (!selectedClip) return;
    if (mode === "cleaned") {
      const asset = noisePreviewAssets[selectedClip.id];
      if (
        !selectedClip.noiseReduction ||
        !asset?.applied ||
        asset.signature !== noiseClipSignature(selectedClip)
      ) return;
    }
    if (mode === "mossformer") {
      const studio = mossFormerPreviewAssets[selectedClip.id];
      if (studio?.signature !== mossFormerClipSignature(selectedClip)) return;
    }
    noiseAuditions = {
      ...noiseAuditions,
      [selectedClip.id]: { signature: audioSourceSignature(selectedClip), mode },
    };
    previewTransportRevision += 1;
  }

  async function handleMossFormerPrepare() {
    const clip = selectedClip as TimelineClip | null;
    // Studio enhancement is an audition/render cache and does not mutate the
    // locked timeline clip itself, so it remains usable on a locked track.
    if (!clip || clip.audioSeparated) return;
    const existing = mossFormerPreviewAssets[clip.id];
    if (existing?.signature === mossFormerClipSignature(clip)) {
      handleNoiseAuditionModeChange("mossformer");
      return;
    }
    if (await prepareMossFormerForClip(clip, selectedMossFormerError)) {
      const current = selectedClip as TimelineClip | null;
      if (
        current?.id === clip.id &&
        mossFormerPreviewAssets[clip.id]?.signature === mossFormerClipSignature(current)
      ) {
        handleNoiseAuditionModeChange("mossformer");
      }
    }
  }

  $effect(() => {
    const snapshot = timelineSnapshot;
    if (!snapshot || !isTauri()) return;
    for (const clip of snapshot.clips) {
      if (!clip.noiseReduction || clip.audioSeparated) continue;
      if (
        manualNoiseCleanupRequests.has(
          `${clip.id}:${noiseClipSignature(clip)}`,
        )
      ) continue;
      void prepareNoiseCleanupForClip(clip);
    }
  });

  const VOICE_NORMALIZATION_TARGET = {
    integratedLufs: -16,
    truePeakDb: -1.5,
    loudnessRange: 11,
  } as const;

  function audioClipSignature(clip: Partial<TimelineClip>): string {
    return JSON.stringify([
      clip.id,
      clip.file,
      clip.trimIn,
      clip.duration,
      clip.speed,
      clip.volume,
      clip.keyframes?.volume,
      clip.keyframes?.riderGain,
      clip.voiceRider,
      clip.noiseReduction,
      clip.audioSeparated,
    ]);
  }

  async function handleNormalizeSelectedAudio(targetClip?: TimelineClip) {
    const clip = (targetClip ?? selectedClip) as TimelineClip | null;
    const initialSnapshot = timelineRef?.getProjectState?.() as
      | TimelineProjectSnapshot
      | undefined;
    const initialTrack = initialSnapshot?.tracks.find(
      (track) => track.id === clip?.trackId,
    );
    if (
      !clip ||
      !timelineRef ||
      !initialTrack ||
      initialTrack.locked ||
      clip.audioSeparated ||
      (clip.kind !== "video" && clip.kind !== "audio") ||
      !clip.file ||
      audioNormalizeBusyClipId ||
      voiceRiderBusyClipId
    ) return;

    const baseVolume = Number.isFinite(clip.volume) ? Math.max(0, clip.volume) : 1;
    audioNormalizeMessageClipId = clip.id;
    audioNormalizeError = false;
    if (clip.voiceRider) {
      audioNormalizeMessage =
        "AI Voice Rider zaten bölgesel seviyeyi yönetiyor. Önce Rider'ı kaldırın.";
      audioNormalizeError = true;
      return;
    }
    if (baseVolume <= 0) {
      audioNormalizeMessage = "Ses seviyesi %0. Normalize etmeden önce sesi açın.";
      audioNormalizeError = true;
      return;
    }

    const signature = audioClipSignature(clip);
    const playbackRate = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
    audioNormalizeBusyClipId = clip.id;
    audioNormalizeMessage = "Ses yüksekliği ve tepe noktaları analiz ediliyor…";

    try {
      const result = await analyzeAudioLoudness({
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: Math.max(1, Math.round(clip.duration * playbackRate * 1_000)),
        playbackRate,
        volume: baseVolume,
        noiseReduction: clip.noiseReduction,
        target: VOICE_NORMALIZATION_TARGET,
      });

      const snapshot = timelineRef?.getProjectState?.() as
        | TimelineProjectSnapshot
        | undefined;
      const currentClip = snapshot?.clips.find((item) => item.id === clip.id);
      const currentTrack = snapshot?.tracks.find(
        (track) => track.id === currentClip?.trackId,
      );
      if (
        !currentClip ||
        selectedClip?.id !== clip.id ||
        !currentTrack ||
        currentTrack.locked ||
        audioClipSignature(currentClip) !== signature
      ) {
        audioNormalizeMessage = "Klip analiz sırasında değişti; sonuç uygulanmadı.";
        audioNormalizeError = true;
        return;
      }

      const peakSafeRecommendation = limitGainForVolumeAutomation(
        currentClip,
        result.recommendedGain,
        result.pass.measuredTruePeakDb,
        result.pass.truePeakDb,
      );
      const applied = applyNormalizationGain(currentClip, peakSafeRecommendation);

      timelineRef.updateClip(
        clip.id,
        { volume: applied.volume, keyframes: applied.keyframes },
        { history: "immediate" },
      );
      selectedClip = {
        ...selectedClip,
        volume: applied.volume,
        keyframes: applied.keyframes,
      };
      const appliedDb = applied.appliedMultiplier > 0
        ? 20 * Math.log10(applied.appliedMultiplier)
        : -Infinity;
      const limited =
        applied.limited || peakSafeRecommendation + 1e-6 < result.recommendedGain;
      audioNormalizeMessage = `✓ ${result.pass.measuredIntegratedLufs.toFixed(1)} LUFS → ${result.pass.integratedLufs.toFixed(0)} LUFS · ${formatSignedDb(appliedDb)}${limited ? " · true-peak güvenlik sınırı" : ""}`;
      audioNormalizeError = false;
    } catch (error) {
      audioNormalizeMessage = formatAudioCommandError(error);
      audioNormalizeError = true;
    } finally {
      if (audioNormalizeBusyClipId === clip.id) audioNormalizeBusyClipId = null;
    }
  }

  async function handleVoiceRider(targetClip?: TimelineClip) {
    const clip = (targetClip ?? selectedClip) as TimelineClip | null;
    const initialSnapshot = timelineRef?.getProjectState?.() as
      | TimelineProjectSnapshot
      | undefined;
    const initialTrack = initialSnapshot?.tracks.find(
      (track) => track.id === clip?.trackId,
    );
    if (
      !clip ||
      !timelineRef ||
      !initialTrack ||
      initialTrack.locked ||
      clip.audioSeparated ||
      (clip.kind !== "video" && clip.kind !== "audio") ||
      !clip.file ||
      audioNormalizeBusyClipId ||
      voiceRiderBusyClipId
    ) return;

    const baseVolume = Number.isFinite(clip.volume) ? Math.max(0, clip.volume) : 1;
    voiceRiderMessageClipId = clip.id;
    voiceRiderError = false;
    if (baseVolume <= 0) {
      voiceRiderMessage = "Klip taban sesi %0. AI analizinden önce sesi açın.";
      voiceRiderError = true;
      return;
    }

    const signature = audioClipSignature(clip);
    const playbackRate = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
    voiceRiderBusyClipId = clip.id;
    voiceRiderMessage =
      "Silero sinir ağı konuşmayı buluyor; bağırma ve kısık bölümler dengeleniyor…";

    try {
      const result = await analyzeVoiceRider({
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: Math.max(1, Math.round(clip.duration * playbackRate * 1_000)),
        playbackRate,
        baseVolume,
        volumeKeyframes: (clip.keyframes.volume ?? []).map((frame) => ({
          atMs: Math.max(0, Math.round(frame.time * 1_000)),
          value: frame.value,
          easing: frame.easing,
        })),
        noiseReduction: clip.noiseReduction,
        targetLufs: VOICE_NORMALIZATION_TARGET.integratedLufs,
        truePeakDb: VOICE_NORMALIZATION_TARGET.truePeakDb,
      });

      const snapshot = timelineRef?.getProjectState?.() as
        | TimelineProjectSnapshot
        | undefined;
      const currentClip = snapshot?.clips.find((item) => item.id === clip.id);
      const currentTrack = snapshot?.tracks.find(
        (track) => track.id === currentClip?.trackId,
      );
      if (
        !currentClip ||
        !currentTrack ||
        currentTrack.locked ||
        audioClipSignature(currentClip) !== signature
      ) {
        voiceRiderMessage = "Klip AI analizi sırasında değişti; sonuç uygulanmadı.";
        voiceRiderError = true;
        return;
      }

      const frames: TimelineKeyframe[] = toVoiceRiderKeyframes(
        result.points,
        currentClip.duration,
      );
      const metadata: VoiceRiderMetadata = {
        model: result.model,
        speechCoverage: result.speechCoverage,
        averageSpeechProbability: result.averageSpeechProbability,
        strongestCutDb: result.strongestCutDb,
        strongestBoostDb: result.strongestBoostDb,
      };
      const applied = timelineRef.applyVoiceRider?.(clip.id, metadata, frames);
      if (!applied) {
        voiceRiderMessage = "AI ses eğrisi klibe uygulanamadı.";
        voiceRiderError = true;
        return;
      }
      if (selectedClip?.id === clip.id) {
        selectedClip = {
          ...selectedClip,
          keyframes: { ...selectedClip.keyframes, riderGain: frames },
          voiceRider: metadata,
        };
      }
      voiceRiderMessage = `✓ AI Voice Rider · konuşma %${Math.round(result.speechCoverage * 100)} · ${formatSignedDb(result.strongestCutDb)} kesme · ${formatSignedDb(result.strongestBoostDb)} yükseltme · ${frames.length} nokta`;
      voiceRiderError = false;
    } catch (error) {
      voiceRiderMessage = formatVoiceRiderError(error);
      voiceRiderError = true;
    } finally {
      if (voiceRiderBusyClipId === clip.id) voiceRiderBusyClipId = null;
    }
  }

  function handleRemoveVoiceRider(targetClip?: TimelineClip) {
    const clip = (targetClip ?? selectedClip) as TimelineClip | null;
    if (!clip || !clip.voiceRider || voiceRiderBusyClipId || audioNormalizeBusyClipId) return;
    const removed = timelineRef?.removeVoiceRider?.(clip.id);
    if (!removed) return;
    if (selectedClip?.id === clip.id) {
      const { riderGain: _riderGain, ...manualKeyframes } = selectedClip.keyframes;
      selectedClip = { ...selectedClip, keyframes: manualKeyframes, voiceRider: null };
    }
    voiceRiderMessageClipId = clip.id;
    voiceRiderMessage = "AI Voice Rider kaldırıldı; manuel volume ayarları korundu.";
    voiceRiderError = false;
  }

  async function analysisSourceForClip(clip: TimelineClip) {
    const playbackRate = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
    const sourceDurationMs = Math.max(
      1,
      Math.round(clip.duration * playbackRate * 1_000),
    );
    const cleanup = normalizeNoiseReduction(clip.noiseReduction);
    if (!cleanup) {
      return {
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: sourceDurationMs,
        playbackRate,
      };
    }
    const signature = noiseClipSignature(clip);
    const cached = noisePreviewAssets[clip.id];
    const asset =
      cached?.signature === signature
        ? { outputPath: cached.outputPath, durationMs: sourceDurationMs }
        : await prepareAudioCleanup({
            sourcePath: clip.file,
            startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
            durationMs: sourceDurationMs,
            settings: cleanup,
          });
    return {
      sourcePath: asset.outputPath,
      startMs: 0,
      durationMs: asset.durationMs,
      playbackRate,
    };
  }

  async function analyzeClipSpeechActivity(clip: TimelineClip, operationId?: string) {
    const source = await analysisSourceForClip(clip);
    const result = await analyzeSpeechActivity(
      operationId ? { ...source, operationId } : source,
    );
    const metadata = createSpeechAnalysisMetadata(
      result.speechSegments.map((segment) => ({
        start: segment.startMs / 1_000,
        end: segment.endMs / 1_000,
        confidence: segment.confidence,
      })),
      speechSourceSignature(clip),
      clip.duration,
    );
    return { result, metadata, source };
  }

  function clipLocalWaveform(clip: TimelineClip): number[] {
    if (clip.waveform.length < 3) return [];
    const speed = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
    const sourceRangeEnd = Math.max(0, clip.trimIn) + clip.duration * speed;
    const sourceDuration = Math.max(sourceRangeEnd, clip.sourceDuration ?? 0);
    if (!Number.isFinite(sourceDuration) || sourceDuration <= 0) return [];
    const startIndex = Math.max(
      0,
      Math.min(
        clip.waveform.length - 1,
        Math.floor((Math.max(0, clip.trimIn) / sourceDuration) * clip.waveform.length),
      ),
    );
    const endIndex = Math.max(
      startIndex + 1,
      Math.min(
        clip.waveform.length,
        Math.ceil((sourceRangeEnd / sourceDuration) * clip.waveform.length),
      ),
    );
    return clip.waveform.slice(startIndex, endIndex);
  }

  const SMART_SHORTS_TIMING_PROFILE_KEY = "astral.smart-shorts.timing-profile.v1";

  function loadSmartShortsTimingProfile(): SmartShortsTimingProfile {
    if (typeof window === "undefined") return createSmartShortsTimingProfile();
    try {
      return parseSmartShortsTimingProfile(
        window.localStorage.getItem(SMART_SHORTS_TIMING_PROFILE_KEY),
      );
    } catch {
      return createSmartShortsTimingProfile();
    }
  }

  function saveSmartShortsTimingProfile(profile: SmartShortsTimingProfile): void {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(SMART_SHORTS_TIMING_PROFILE_KEY, JSON.stringify(profile));
    } catch {
      // Progress estimation remains functional when storage is unavailable.
    }
  }

  const SMART_SHORTS_CANCELLED_CODE = "smart_shorts_cancelled";

  // Rust's valid_operation_id accepts only [A-Za-z0-9_-], up to 128 chars.
  function createSmartShortsOperationId(): string {
    const random =
      typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
        ? crypto.randomUUID()
        : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    return `smart-shorts-${random}`.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 128);
  }

  function isSmartShortsCancelledError(error: unknown): boolean {
    return Boolean(
      error &&
        typeof error === "object" &&
        (error as { code?: unknown }).code === SMART_SHORTS_CANCELLED_CODE,
    );
  }

  function smartShortsCancelledError(): Error {
    return Object.assign(new Error("Akıllı Shorts analizi iptal edildi."), {
      code: SMART_SHORTS_CANCELLED_CODE,
    });
  }

  async function handleSmartShortsCancel() {
    const operationId = smartShortsOperationId;
    if (!operationId || smartShortsCancelling) return;
    // Refuse to apply any in-flight result immediately, even before the engine
    // processes actually stop.
    smartShortsCancelling = true;
    smartShortsMessage = "Analiz iptal ediliyor…";
    smartShortsError = false;
    try {
      await cancelSmartShortsAnalysis(operationId);
    } catch (error) {
      // The frontend already stops applying results; a failed signal only means
      // native engines may run to completion in the background.
      console.warn("Akıllı Shorts iptal sinyali gönderilemedi", error);
    }
  }

  // Beat-This is CPU-only (no GPU path) and can take tens of minutes on a long
  // clip, so time-box it: rhythm is a minor signal and must never stall the whole
  // pipeline. It gets its own operation id so the timeout can kill just the beat
  // engine without touching the other stages.
  const BEAT_ANALYSIS_TIMEOUT_MS = 90_000;

  async function analyzeBeatsBounded(request: {
    sourcePath: string;
    startMs: number;
    durationMs: number;
  }) {
    const beatsOperationId = createSmartShortsOperationId();
    let timer: ReturnType<typeof setTimeout> | null = null;
    try {
      await beginSmartShortsAnalysis(beatsOperationId);
      const timeout = new Promise<null>((resolve) => {
        timer = setTimeout(() => {
          void cancelSmartShortsAnalysis(beatsOperationId).catch(() => {});
          resolve(null);
        }, BEAT_ANALYSIS_TIMEOUT_MS);
      });
      return await Promise.race([
        analyzeBeats({ ...request, operationId: beatsOperationId }).catch(() => null),
        timeout,
      ]);
    } catch {
      return null;
    } finally {
      if (timer !== null) clearTimeout(timer);
      void finishSmartShortsAnalysis(beatsOperationId).catch(() => {});
    }
  }

  async function handleSmartShortsAnalyze(request: SmartShortsAnalyzeRequest) {
    const clip = selectedClip as TimelineClip | null;
    if (
      !clip ||
      selectedClipLocked ||
      clip.kind !== "video" ||
      clip.audioSeparated ||
      !clip.file ||
      smartShortsBusyClipId
    ) return;
    smartShortsMessageClipId = clip.id;
    if (!isTauri()) {
      smartShortsMessage = "Akıllı Shorts yerel masaüstü çalışma motorunu gerektirir.";
      smartShortsError = true;
      return;
    }
    const signature = speechSourceSignature(clip);
    const progressEstimator = new SmartShortsProgressEstimator(
      Math.max(1_000, clip.duration * 1_000),
      request.useGlm,
      loadSmartShortsTimingProfile(),
      performance.now(),
    );
    let progressTimer: ReturnType<typeof setInterval> | null = null;
    const syncEstimatedProgress = () => {
      const snapshot = progressEstimator.snapshot(performance.now());
      smartShortsProgress = Math.max(smartShortsProgress, snapshot.percent);
      smartShortsProgressStage = snapshot.stageLabel;
      smartShortsProgressElapsedMs = snapshot.elapsedMs;
      smartShortsProgressRemainingMs = snapshot.remainingMs;
      smartShortsProgressOverdue = snapshot.overdue;
    };
    const beginProgressStage = (stage: SmartShortsProgressStage) => {
      progressEstimator.begin(stage, performance.now());
      syncEstimatedProgress();
    };
    const observeProgress = (percent: number) => {
      progressEstimator.observe(percent);
      syncEstimatedProgress();
    };
    const operationId = createSmartShortsOperationId();
    smartShortsBusyClipId = clip.id;
    smartShortsOperationId = operationId;
    smartShortsCancelling = false;
    smartShortsProgress = 3;
    smartShortsProgressStage = "Konuşma sınırları çıkarılıyor";
    smartShortsProgressElapsedMs = 0;
    smartShortsProgressRemainingMs = null;
    smartShortsProgressOverdue = false;
    smartShortsMessage = "Silero konuşma sınırlarını çıkarıyor…";
    smartShortsError = false;
    syncEstimatedProgress();
    progressTimer = setInterval(syncEstimatedProgress, 400);
    let unlisten: (() => void) | null = null;
    let unlistenLaughter: (() => void) | null = null;
    try {
      // Reset any stale cancellation flag for this fresh operation id.
      await beginSmartShortsAnalysis(operationId);
      const [speechListener, laughterListener] = await Promise.allSettled([
        onSpeechSetupProgress((progress) => {
          if (smartShortsBusyClipId !== clip.id) return;
          const setupPercent = progress.progressPercent ?? 0;
          observeProgress(Math.min(48, 6 + setupPercent * 0.42));
          smartShortsMessage = progress.message;
        }),
        onLaughterAnalysisProgress((progress) => {
          if (smartShortsBusyClipId !== clip.id) return;
          const setupPercent = progress.progressPercent ?? 0;
          observeProgress(Math.min(82, 28 + setupPercent * 0.54));
          smartShortsMessage = progress.message;
        }),
      ]);
      if (speechListener.status === "fulfilled") unlisten = speechListener.value;
      if (laughterListener.status === "fulfilled") {
        unlistenLaughter = laughterListener.value;
      }
      const activity = await analyzeClipSpeechActivity(clip, operationId);
      if (smartShortsCancelling) throw smartShortsCancelledError();
      beginProgressStage("analysis");
      smartShortsMessage = "Whisper, YAMNet kahkaha, ses enerjisi ve ritim birlikte taranıyor…";
      let laughterWarning: string | null = null;
      const [transcript, beatResult, energyResult, laughterResult] = await Promise.all([
        analyzeSpeechTranscript({
          ...activity.source,
          force: false,
          language: "auto",
          operationId,
        }),
        analyzeBeatsBounded({
          sourcePath: activity.source.sourcePath,
          startMs: activity.source.startMs,
          durationMs: activity.source.durationMs,
        }),
        analyzeAudioEnergy({
          sourcePath: activity.source.sourcePath,
          startMs: activity.source.startMs,
          durationMs: activity.source.durationMs,
          playbackRate: activity.source.playbackRate,
          operationId,
        }).catch(() => null),
        analyzeLaughter({
          sourcePath: activity.source.sourcePath,
          startMs: activity.source.startMs,
          durationMs: activity.source.durationMs,
          playbackRate: activity.source.playbackRate,
          minConfidence: 0.28,
          operationId,
        }).catch((error) => {
          if (isSmartShortsCancelledError(error)) throw error;
          laughterWarning = formatAudioCommandError(
            error,
            "YAMNet kahkaha analizi tamamlanamadı.",
          );
          return null;
        }),
      ]);
      // The .catch handlers above swallow non-cancel failures, so a user cancel
      // that lands after Whisper finishes must still block result application.
      if (smartShortsCancelling) throw smartShortsCancelledError();
      beginProgressStage("planning");
      const snapshot = timelineRef?.getProjectState?.() as
        | TimelineProjectSnapshot
        | undefined;
      const current = snapshot?.clips.find((item) => item.id === clip.id);
      if (!current || speechSourceSignature(current) !== signature) {
        throw new Error("Klip analiz sırasında değişti; Akıllı Shorts sonucu uygulanmadı.");
      }

      const beatMetadata = beatResult
        ? createBeatAnalysisMetadata(activity.source.sourcePath, beatResult)
        : null;
      const playbackRate = Math.max(0.01, activity.source.playbackRate);
      const beats = (beatMetadata?.markers ?? [])
        .map((marker) => ({
          atMs: Math.round((marker.sourceMs - (beatResult?.analyzedStartMs ?? 0)) / playbackRate),
          confidence: marker.confidence,
          downbeat: marker.downbeat,
        }))
        .filter((beat) => beat.atMs >= 0 && beat.atMs <= transcript.durationMs);
      const localWaveform = clipLocalWaveform(current);
      const energyEvents = energyResult
        ? extractLoudnessEnergyEvents(energyResult.points)
        : extractWaveformEnergyEvents(localWaveform, transcript.durationMs);
      const laughterEvents = fuseNeuralLaughterEvents(
        (laughterResult?.events ?? []).map((event) => ({
          startMs: event.startMs,
          endMs: event.endMs,
          score: event.confidence,
          kind: event.kind,
        })),
        energyEvents,
        transcript.words,
      );
      const audioEvents = [...energyEvents, ...laughterEvents].sort(
        (left, right) => left.atMs - right.atMs,
      );
      let candidates = planSmartShorts(
        {
          durationMs: transcript.durationMs,
          words: transcript.words,
          speechSegments: activity.result.speechSegments,
          beats,
          audioEvents,
        },
        {
          targetDurationMs: request.targetDurationMs,
          minDurationMs: Math.max(8_000, Math.round(request.targetDurationMs * 0.65)),
          maxDurationMs: Math.min(60_000, Math.round(request.targetDurationMs * 1.35)),
          maxCandidates: 6,
        },
      );
      if (candidates.length === 0) {
        throw new Error(
          "Zaman damgalı ve yeterince net bir konuşma anı bulunamadı; kaynak sesi kontrol edin.",
        );
      }

      let glmUsed = false;
      let glmWarning: string | null = null;
      if (request.useGlm) {
        beginProgressStage("ranking");
        smartShortsMessage = "Fireworks GLM yalnız zaman damgalı metin adaylarını sıralıyor…";
        try {
          const ranked = await rankSmartShortCandidates(
            candidates.map((candidate) => ({
              candidateId: candidate.id,
              startMs: candidate.startMs,
              endMs: candidate.endMs,
              localScore: candidate.score,
              transcript: candidate.transcript,
              evidence: candidate.reasons,
            })),
            request.apiKey,
          );
          candidates = mergeExternalSmartShortsRankings(candidates, ranked.rankings);
          glmUsed = ranked.rankings.length > 0;
        } catch (error) {
          glmWarning = `GLM kullanılamadı; yerel sıralama korundu (${formatAudioCommandError(error, "GLM sıralaması tamamlanamadı.")})`;
        }
      }

      beginProgressStage("finalizing");
      timelineRef.updateClip(
        clip.id,
        { speechAnalysis: activity.metadata },
        { history: "immediate" },
      );
      if (selectedClip?.id === clip.id) {
        selectedClip = { ...selectedClip, speechAnalysis: activity.metadata };
      }
      const engineParts = [
        transcript.engine,
        "Silero VAD",
        beatResult?.model ?? "ritim analizi yok",
      ];
      if (energyResult) engineParts.push(energyResult.engine);
      else if (energyEvents.length > 0) engineParts.push("kaba waveform enerjisi");
      if (laughterResult) {
        engineParts.push(`YAMNet ${laughterResult.engineVersion}`);
      } else {
        engineParts.push("YAMNet kullanılamadı");
      }
      if (glmUsed) engineParts.push("Fireworks GLM");
      smartShortsReviews = {
        ...smartShortsReviews,
        [clip.id]: {
          signature,
          candidates,
          selectedIds: new Set(candidates.slice(0, 1).map((candidate) => candidate.id)),
          glmUsed,
          engineLabel: engineParts.join(" · "),
        },
      };
      saveSmartShortsTimingProfile(progressEstimator.complete(performance.now()));
      smartShortsProgress = 100;
      smartShortsProgressRemainingMs = 0;
      smartShortsProgressOverdue = false;
      smartShortsMessage = `✓ ${candidates.length} aday · ${laughterEvents.length} kahkaha/gülme olayı · en iyi skor %${candidates[0].score}${transcript.cacheHit ? " · Whisper önbelleği" : ""}${laughterWarning ? ` · YAMNet uyarısı: ${laughterWarning}` : ""}${glmWarning ? ` · ${glmWarning}` : ""}`;
    } catch (error) {
      saveSmartShortsTimingProfile(progressEstimator.timingProfile());
      if (smartShortsCancelling || isSmartShortsCancelledError(error)) {
        smartShortsMessage = "Analiz iptal edildi.";
        smartShortsError = false;
      } else {
        smartShortsMessage = formatAudioCommandError(
          error,
          "Akıllı Shorts analizi tamamlanamadı.",
        );
        smartShortsError = true;
      }
    } finally {
      if (progressTimer !== null) clearInterval(progressTimer);
      unlisten?.();
      unlistenLaughter?.();
      // Best-effort: clear this operation's cancellation flag on the backend.
      void finishSmartShortsAnalysis(operationId).catch(() => {});
      if (smartShortsBusyClipId === clip.id) {
        smartShortsBusyClipId = null;
        smartShortsProgressStage = null;
        smartShortsProgressElapsedMs = 0;
        smartShortsProgressRemainingMs = null;
        smartShortsProgressOverdue = false;
      }
      if (smartShortsOperationId === operationId) {
        smartShortsOperationId = null;
        smartShortsCancelling = false;
      }
    }
  }

  function handleSmartShortsCandidateToggle(id: string, selected: boolean) {
    const clip = selectedClip as TimelineClip | null;
    if (!clip) return;
    const review = smartShortsReviews[clip.id];
    if (!review || review.signature !== speechSourceSignature(clip)) return;
    const selectedIds = new Set(review.selectedIds);
    if (selected) selectedIds.add(id);
    else selectedIds.delete(id);
    smartShortsReviews = {
      ...smartShortsReviews,
      [clip.id]: { ...review, selectedIds },
    };
  }

  function handleSmartShortsCandidateSeek(candidate: SmartShortsCandidate) {
    const clip = selectedClip as TimelineClip | null;
    if (!clip) return;
    directPreviewPath = null;
    playheadTime = Math.max(
      clip.start,
      clip.start + candidate.startMs / 1_000 - 0.35,
    );
    isPlaying = true;
  }

  function handleSmartShortsApply() {
    const clip = selectedClip as TimelineClip | null;
    const review = clip ? smartShortsReviews[clip.id] : null;
    if (!clip || !review || selectedClipLocked) return;
    const selected = review.candidates.filter((candidate) =>
      review.selectedIds.has(candidate.id),
    );
    if (selected.length === 0) return;
    const discard = discardRangesOutsideHighlights(
      Math.round(clip.duration * 1_000),
      selected,
    );
    smartShortsMessageClipId = clip.id;
    if (discard.length === 0) {
      smartShortsMessage = "Seçim klibin tamamını kapsıyor; kesilecek dış aralık yok.";
      smartShortsError = false;
      return;
    }
    const applied = timelineRef?.applySpeechSuggestionRanges?.(
      clip.id,
      discard.map((range) => ({
        start: range.startMs / 1_000,
        end: range.endMs / 1_000,
      })),
      { forceRipple: true },
    );
    if (!applied) {
      smartShortsMessage = "Highlight akışı oluşturulamadı; klip veya kanal kilidini kontrol edin.";
      smartShortsError = true;
      return;
    }
    const { [clip.id]: _removed, ...remaining } = smartShortsReviews;
    smartShortsReviews = remaining;
    const nextSnapshot = timelineRef?.getProjectState?.() as
      | TimelineProjectSnapshot
      | undefined;
    smartShortsMessageClipId = nextSnapshot?.selectedClipId ?? clip.id;
    smartShortsMessage = `✓ ${selected.length} güçlü an tek, boşluksuz akışa dönüştürüldü · tek Ctrl+Z`;
    smartShortsError = false;
    isPlaying = false;
  }

  async function handleSpeechAnalyze() {
    const clip = selectedClip as TimelineClip | null;
    if (
      !clip ||
      selectedClipLocked ||
      !clip.file ||
      clip.audioSeparated ||
      (clip.kind !== "audio" && clip.kind !== "video") ||
      speechBusyClipId
    ) return;
    const signature = speechSourceSignature(clip);
    speechBusyClipId = clip.id;
    speechMessageClipId = clip.id;
    speechMessage = "Silero konuşma sınırlarını buluyor; Whisper Türkçe sözcükleri zamanlıyor…";
    speechError = false;
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onSpeechSetupProgress((progress) => {
        if (speechBusyClipId !== clip.id) return;
        speechMessage = progress.progressPercent === undefined
          ? progress.message
          : `${progress.message} · %${Math.round(progress.progressPercent)}`;
      });
      const activity = await analyzeClipSpeechActivity(clip);
      const transcript = await analyzeSpeechTranscript({
        ...activity.source,
        force: false,
      });
      const snapshot = timelineRef?.getProjectState?.() as
        | TimelineProjectSnapshot
        | undefined;
      const current = snapshot?.clips.find((item) => item.id === clip.id);
      if (!current || speechSourceSignature(current) !== signature) {
        throw new Error("Klip analiz sırasında değişti; öneriler uygulanmadı.");
      }
      const suggestions = buildSpeechSuggestions(
        transcript.durationMs,
        activity.result.speechSegments,
        transcript.words,
        BALANCED_SPEECH_SUGGESTIONS,
      );
      const selectedIds = new Set(
        suggestions.filter((suggestion) => suggestion.defaultSelected).map((item) => item.id),
      );
      speechReviews = {
        ...speechReviews,
        [clip.id]: {
          signature,
          suggestions,
          selectedIds,
          transcript: transcript.transcript,
        },
      };
      timelineRef.updateClip(
        clip.id,
        { speechAnalysis: activity.metadata },
        { history: "immediate" },
      );
      if (selectedClip?.id === clip.id) {
        selectedClip = { ...selectedClip, speechAnalysis: activity.metadata };
      }
      const contextualCount = suggestions.filter(
        (suggestion) => suggestion.kind === "contextual-word",
      ).length;
      speechMessage = `✓ ${suggestions.length} öneri · ${selectedIds.size} güvenli seçim${contextualCount ? ` · ${contextualCount} bağlama bağlı sözcük seçilmedi` : ""}${transcript.cacheHit ? " · önbellekten" : ""}`;
    } catch (error) {
      speechMessage = formatAudioCommandError(error);
      speechError = true;
    } finally {
      unlisten?.();
      if (speechBusyClipId === clip.id) speechBusyClipId = null;
    }
  }

  function handleSpeechSuggestionToggle(id: string, selected: boolean) {
    const clip = selectedClip as TimelineClip | null;
    if (!clip) return;
    const review = speechReviews[clip.id];
    if (!review) return;
    const selectedIds = new Set(review.selectedIds);
    if (selected) selectedIds.add(id);
    else selectedIds.delete(id);
    speechReviews = {
      ...speechReviews,
      [clip.id]: { ...review, selectedIds },
    };
  }

  function handleSpeechSuggestionSeek(suggestion: SpeechSuggestion) {
    const clip = selectedClip as TimelineClip | null;
    if (!clip) return;
    directPreviewPath = null;
    playheadTime = Math.max(clip.start, clip.start + suggestion.startMs / 1_000 - 0.25);
    isPlaying = true;
  }

  function handleSpeechSuggestionsApply() {
    const clip = selectedClip as TimelineClip | null;
    const review = clip ? speechReviews[clip.id] : null;
    if (!clip || !review || selectedClipLocked) return;
    const selected = review.suggestions.filter((suggestion) =>
      review.selectedIds.has(suggestion.id),
    );
    const applied = timelineRef?.applySpeechSuggestionRanges?.(
      clip.id,
      selected.map((suggestion) => ({
        start: suggestion.startMs / 1_000,
        end: suggestion.endMs / 1_000,
      })),
    );
    if (!applied) {
      speechMessage = "Seçilen aralıklar kesilemedi; klip veya kanal kilidini kontrol edin.";
      speechError = true;
      return;
    }
    const { [clip.id]: _removed, ...remaining } = speechReviews;
    speechReviews = remaining;
    speechMessageClipId = null;
    speechMessage = null;
    isPlaying = false;
  }

  function handleDuckingTrackToggle(trackId: string, selected: boolean) {
    const ids = new Set(selectedDuckingSettings().sourceTrackIds);
    if (selected) ids.add(trackId);
    else ids.delete(trackId);
    duckingDraft = { ...selectedDuckingSettings(), sourceTrackIds: [...ids] };
    duckingDraftClipId = selectedClip?.id ?? null;
  }

  function handleDuckingChange(updates: Partial<AutoDuckingSettings>) {
    const clip = selectedClip as TimelineClip | null;
    if (!clip || selectedClipLocked) return;
    const next = normalizeAutoDucking({ ...selectedDuckingSettings(), ...updates });
    if (!next) return;
    duckingDraft = next;
    duckingDraftClipId = clip.id;
    if (clip.autoDucking) {
      timelineRef?.updateClip?.(clip.id, { autoDucking: next }, { history: "coalesce" });
      selectedClip = { ...selectedClip, autoDucking: next };
    }
  }

  async function handleDuckingApply() {
    const target = selectedClip as TimelineClip | null;
    const snapshot = timelineRef?.getProjectState?.() as
      | TimelineProjectSnapshot
      | undefined;
    const settings = normalizeAutoDucking(selectedDuckingSettings());
    if (!target || !snapshot || !settings || settings.sourceTrackIds.length === 0) return;
    const targetEnd = target.start + target.duration;
    const sources = snapshot.clips.filter(
      (clip) =>
        clip.id !== target.id &&
        settings.sourceTrackIds.includes(clip.trackId) &&
        (clip.kind === "audio" || clip.kind === "video") &&
        !clip.audioSeparated &&
        clip.start < targetEnd &&
        clip.start + clip.duration > target.start,
    );
    if (sources.length === 0) {
      duckingMessageClipId = target.id;
      duckingMessage = "Seçili kanallarda bu müzikle örtüşen bir konuşma klibi yok; AI Ducking etkinleştirilmedi.";
      duckingError = true;
      return;
    }
    duckingBusyClipId = target.id;
    duckingMessageClipId = target.id;
    duckingMessage = `${sources.length} kaynak klipte konuşma aranıyor…`;
    duckingError = false;
    try {
      const updates: { id: string; updates: Partial<TimelineClip> }[] = [];
      let analyzedCount = 0;
      for (const source of sources) {
        if (isSpeechAnalysisCurrent(source)) continue;
        duckingMessage = `Konuşma analizi ${analyzedCount + 1}/${sources.length} · ${fileNameFromPath(source.file)}`;
        const analyzed = await analyzeClipSpeechActivity(source);
        updates.push({ id: source.id, updates: { speechAnalysis: analyzed.metadata } });
        analyzedCount += 1;
      }
      const latest = timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined;
      const currentTarget = latest?.clips.find((clip) => clip.id === target.id);
      if (!currentTarget || speechSourceSignature(currentTarget) !== speechSourceSignature(target)) {
        throw new Error("Müzik klibi analiz sırasında değişti; ducking uygulanmadı.");
      }
      for (const source of sources) {
        const currentSource = latest?.clips.find((clip) => clip.id === source.id);
        if (
          !currentSource ||
          speechSourceSignature(currentSource) !== speechSourceSignature(source)
        ) {
          throw new Error(
            `${fileNameFromPath(source.file)} analiz sırasında değişti; ducking uygulanmadı.`,
          );
        }
      }
      const analyzedById = new Map(
        updates.map((update) => [update.id, update.updates.speechAnalysis]),
      );
      const speechSegmentCount = sources.reduce(
        (total, source) =>
          total +
          (analyzedById.get(source.id)?.segments.length ??
            source.speechAnalysis?.segments.length ??
            0),
        0,
      );
      if (speechSegmentCount === 0) {
        if (updates.length > 0 && !timelineRef.updateClips?.(updates)) {
          throw new Error("Konuşma analizleri zaman çizelgesine kaydedilemedi.");
        }
        duckingMessage = "Seçili kliplerde konuşma bulunmadı; AI Ducking etkinleştirilmedi.";
        duckingError = true;
        return;
      }
      updates.push({ id: target.id, updates: { autoDucking: settings } });
      if (!timelineRef.updateClips?.(updates)) {
        throw new Error("Ducking ayarları zaman çizelgesine uygulanamadı.");
      }
      selectedClip = { ...selectedClip, autoDucking: settings };
      duckingMessage = `✓ AI Ducking · ${sources.length} klip · ${settings.reductionDb.toFixed(0)} dB · attack ${settings.attackMs} ms · release ${settings.releaseMs} ms`;
    } catch (error) {
      duckingMessage = formatAudioCommandError(error);
      duckingError = true;
    } finally {
      if (duckingBusyClipId === target.id) duckingBusyClipId = null;
    }
  }

  function handleDuckingRemove() {
    const clip = selectedClip as TimelineClip | null;
    if (!clip?.autoDucking || selectedClipLocked) return;
    timelineRef?.updateClip?.(clip.id, { autoDucking: null }, { history: "immediate" });
    selectedClip = { ...selectedClip, autoDucking: null };
    duckingDraft = { ...DEFAULT_AUTO_DUCKING, sourceTrackIds: [] };
    duckingDraftClipId = clip.id;
    duckingMessageClipId = clip.id;
    duckingMessage = "AI Ducking kaldırıldı; manuel volume ve Voice Rider korundu.";
    duckingError = false;
  }

  async function handleBeatAnalyze() {
    const clip = selectedClip as TimelineClip | null;
    if (
      !clip ||
      selectedClipLocked ||
      !clip.file ||
      clip.audioSeparated ||
      (clip.kind !== "audio" && clip.kind !== "video") ||
      beatBusyClipId
    ) return;
    beatBusyClipId = clip.id;
    beatMessageClipId = clip.id;
    beatMessage = "Beat This! sinir ağı beat ve downbeat’leri analiz ediyor…";
    beatError = false;
    try {
      const speed = Number.isFinite(clip.speed) && clip.speed > 0 ? clip.speed : 1;
      const result = await analyzeBeats({
        sourcePath: clip.file,
        startMs: Math.max(0, Math.round(clip.trimIn * 1_000)),
        durationMs: Math.max(1, Math.round(clip.duration * speed * 1_000)),
      });
      const metadata = createBeatAnalysisMetadata(clip.file, result);
      const current = (timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined)
        ?.clips.find((item) => item.id === clip.id);
      if (
        !current ||
        current.file !== clip.file ||
        !isBeatAnalysisRangeCurrent(current, metadata)
      ) {
        throw new Error("Klip beat analizi sırasında değişti; markerlar uygulanmadı.");
      }
      timelineRef.updateClip(clip.id, { beatAnalysis: metadata }, { history: "immediate" });
      selectedClip = { ...selectedClip, beatAnalysis: metadata };
      beatCacheHits = { ...beatCacheHits, [clip.id]: result.cacheHit };
      beatMessage = `✓ ${metadata.bpm?.toFixed(1) ?? "?"} BPM · ${metadata.markers.length} beat · ${metadata.markers.filter((marker) => marker.downbeat).length} downbeat${result.cacheHit ? " · önbellekten" : ""}`;
    } catch (error) {
      beatMessage = formatAudioCommandError(error);
      beatError = true;
    } finally {
      if (beatBusyClipId === clip.id) beatBusyClipId = null;
    }
  }

  function handleBeatCutsApply() {
    const clip = selectedClip as TimelineClip | null;
    if (
      !clip?.beatAnalysis ||
      !timelineSnapshot ||
      !isBeatAnalysisRangeCurrent(clip, clip.beatAnalysis)
    ) return;
    const markers = projectSourceBeats(clip, clip.beatAnalysis.markers);
    const cuts = planBeatCuts(markers, {
      mode: beatCutMode,
      rangeStart: clip.start,
      rangeEnd: clip.start + clip.duration,
      minShotSeconds: beatCutMode === "smart" ? 1.5 : 0.1,
      maxShotSeconds: 4,
    });
    const visualTrackIds = timelineSnapshot.tracks
      .filter((track) => track.type === "video" && track.id !== clip.trackId)
      .map((track) => track.id);
    const count = timelineRef?.applyBeatCuts?.(cuts, visualTrackIds);
    beatMessageClipId = clip.id;
    if (!count) {
      beatMessage = "Bu markerlarda bölünebilen, kilitsiz bir görüntü klibi bulunamadı.";
      beatError = true;
      return;
    }
    beatMessage = `✓ ${count} müziğe uyumlu görüntü kesimi · tek geri alma adımı`;
    beatError = false;
  }

  function handleBeatRemove() {
    const clip = selectedClip as TimelineClip | null;
    if (!clip?.beatAnalysis || selectedClipLocked) return;
    timelineRef?.updateClip?.(clip.id, { beatAnalysis: null }, { history: "immediate" });
    selectedClip = { ...selectedClip, beatAnalysis: null };
    beatMessageClipId = clip.id;
    beatMessage = "Beat markerları kaldırıldı; medya değişmedi.";
    beatError = false;
  }

  function formatSignedDb(value: number): string {
    if (!Number.isFinite(value)) return "−∞ dB";
    return `${value > 0 ? "+" : ""}${value.toFixed(1)} dB`;
  }

  function formatAudioCommandError(
    error: unknown,
    fallback = "Ses normalizasyonu tamamlanamadı.",
  ): string {
    if (error instanceof Error) return error.message;
    if (error && typeof error === "object") {
      const value = error as { userMessage?: unknown; message?: unknown };
      if (typeof value.userMessage === "string") return value.userMessage;
      if (typeof value.message === "string") return value.message;
    }
    return typeof error === "string" ? error : fallback;
  }

  function formatVoiceRiderError(error: unknown): string {
    const message = formatAudioCommandError(error);
    return message === "Ses normalizasyonu tamamlanamadı."
      ? "AI Voice Rider analizi tamamlanamadı."
      : message;
  }

  function handleInspectorTextUpdate(updates: Partial<TextClipStyle>) {
    if (!selectedClip?.text || !timelineRef || selectedClipLocked) return;
    const text = { ...selectedClip.text, ...updates };
    selectedClip = { ...selectedClip, text };
    timelineRef.updateClip(selectedClip.id, { text }, { history: "coalesce" });
  }

  function handleClipSpeedChange(speed: number) {
    if (selectedClipLocked) return;
    timelineRef?.setSelectedClipSpeed?.(speed);
  }

  function handleTrackMixChange(key: "gain" | "pan", value: number) {
    if (!selectedClip) return;
    timelineRef?.updateTrackMix?.(selectedClip.trackId, key, value);
  }

  function handleTransitionChange(
    side: "in" | "out",
    key: "type" | "duration",
    value: string,
  ) {
    if (selectedClipLocked) return;
    timelineRef?.updateSelectedTransition?.(side, key, value);
  }

  function handleAddKeyframe(property: "opacity" | "volume") {
    timelineRef?.addKeyframeAtPlayhead?.(property);
  }

  function handleDeleteSelectedClip() {
    if (selectedClipLocked) return;
    timelineRef?.deleteSelectedClip?.();
  }

  function handleQuietRegionOpen(region: {
    start: number;
    end: number;
    volume: number;
  }) {
    timelineRef?.openQuietRegion?.(region);
  }

  // After "+ Metin" creates a clip, focus the Inspector's content field so
  // typing can start immediately (parity with the old quick-settings panel).
  async function handleTextClipCreated() {
    await tick();
    const input = document.getElementById(
      "text-content",
    ) as HTMLTextAreaElement | null;
    input?.focus();
    input?.select();
  }

  function handlePreviewPositionChange(position: { x: number; y: number }) {
    handlePreviewTransformChange(position);
  }

  function handlePreviewTransformChange(
    updates: Partial<{ x: number; y: number; scale: number; rotation: number }>,
  ) {
    if (!selectedClip || !timelineRef || selectedClipLocked) return;
    if (
      selectedAutoReframeEntry &&
      (updates.x !== undefined || updates.y !== undefined || updates.scale !== undefined)
    ) return;
    selectedClip = {
      ...selectedClip,
      transform: {
        ...selectedClip.transform,
        ...updates,
      },
    };
    timelineRef.updateClip(selectedClip.id, updates, { history: "coalesce" });
  }

  function handleResetPosition() {
    if (!selectedClip || !timelineRef || selectedClipLocked || selectedAutoReframeEntry) return;
    const position = { x: 0, y: 0 };
    selectedClip = {
      ...selectedClip,
      transform: { ...selectedClip.transform, ...position },
    };
    timelineRef.updateClip(selectedClip.id, position, { history: "immediate" });
  }

  function handleResetTransform() {
    if (!selectedClip || !timelineRef || selectedClipLocked || selectedAutoReframeEntry) return;
    const transform = { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 };
    selectedClip = { ...selectedClip, transform };
    timelineRef.updateClip(selectedClip.id, transform, { history: "immediate" });
  }

  function handleTimelineClipTimeChange() {
    if (directPreviewPath) {
      directPreviewPath = null;
      playheadTime = Math.min(playheadTime, timelineContentDuration);
    }
  }

  function handleTimelineDurationChange(nextDuration: number) {
    if (!Number.isFinite(nextDuration) || nextDuration < 0) return;
    timelineContentDuration = nextDuration;
    if (!directPreviewPath && playheadTime > nextDuration) {
      playheadTime = nextDuration;
    }
  }

  function handlePreviewMediaDuration(nextDuration: number) {
    if (!directPreviewPath || !Number.isFinite(nextDuration) || nextDuration <= 0) return;
    directPreviewDuration = nextDuration;
    if (playheadTime > nextDuration) playheadTime = nextDuration;
  }

  $effect(() => {
    const time = playheadTime;
    if (!timelineRef || directPreviewPath) return;
    timelineFramePlan = timelineRef.getFramePlan(time);
  });

  // Dynamic font face loading for custom fonts
  const loadedFonts = new Set<string>();

  async function ensureCustomFontLoaded(fontFamily: string, fontPath: string) {
    const cacheKey = `${fontFamily}:${fontPath}`;
    if (loadedFonts.has(cacheKey)) return;
    
    try {
      const assetUrl = convertFileSrc(fontPath);
      const font = new FontFace(fontFamily, `url(${assetUrl})`);
      const loadedFont = await font.load();
      document.fonts.add(loadedFont);
      loadedFonts.add(cacheKey);
      console.log(`Loaded custom font: ${fontFamily} from ${fontPath}`);
    } catch (err) {
      console.error(`Failed to load custom font ${fontFamily}:`, err);
    }
  }

  $effect(() => {
    if (!timelineSnapshot) return;
    for (const clip of timelineSnapshot.clips) {
      if (clip.kind === "text" && clip.text?.fontPath) {
        ensureCustomFontLoaded(clip.text.fontFamily, clip.text.fontPath);
      }
    }
  });

  function createDirectPreviewPlan(path: string, time: number): FramePlan {
    const kind = /\.(png|jpe?g|webp|gif|bmp|tiff?|avif)$/i.test(path)
      ? "image"
      : /\.(mp3|wav|m4a|aac|flac|ogg|opus)$/i.test(path)
        ? "audio"
        : "video";
    const clipId = `preview:${path}`;
    const visual = kind === "audio" ? [] : [{
      clipId,
      trackId: "preview-video",
      kind: kind as "video" | "image",
      file: path,
      sourceTime: kind === "image" ? 0 : time,
      localTime: time,
      transform: { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1 },
      opacity: 1,
      transitionType: "none" as const,
      transitionPhase: "none" as const,
      transitionProgress: 1,
      text: null,
    }];
    const audio = kind === "image" ? [] : [{
      clipId,
      trackId: "preview-audio",
      file: path,
      sourceTime: time,
      localTime: time,
      playbackRate: 1,
      gain: 1,
      pan: 0,
    }];
    return { time, layers: visual, audio };
  }

  function applyProxyOverrides(plan: FramePlan): FramePlan {
    const proxyFor = (path: string) => proxyPaths[normalizeMediaPath(path)] ?? path;
    return {
      ...plan,
      layers: plan.layers.map((layer) => ({ ...layer, file: proxyFor(layer.file) })),
      audio: plan.audio.map((source) => ({ ...source, file: proxyFor(source.file) })),
    };
  }

  function applyNoiseCleanupOverrides(plan: FramePlan): FramePlan {
    if (directPreviewPath || !timelineSnapshot) return plan;
    const clips = new Map(timelineSnapshot.clips.map((clip) => [clip.id, clip]));
    return {
      ...plan,
      audio: plan.audio.map((source) => {
        const clip = clips.get(source.clipId);
        if (!clip || clip.audioSeparated) return source;
        const audition = noiseAuditions[source.clipId];
        const validAudition = audition?.signature === audioSourceSignature(clip)
          ? audition
          : null;
        if (validAudition?.mode === "original") return source;

        const studio = mossFormerPreviewAssets[source.clipId];
        if (
          validAudition?.mode === "mossformer" &&
          studio?.signature === mossFormerClipSignature(clip)
        ) {
          return {
            ...source,
            file: studio.outputPath,
            sourceTime: Math.max(0, source.sourceTime - clip.trimIn),
          };
        }

        if (!clip.noiseReduction) return source;
        const asset = noisePreviewAssets[source.clipId];
        if (
          !asset ||
          !asset.applied ||
          asset.signature !== noiseClipSignature(clip)
        ) return source;
        return {
          ...source,
          file: asset.outputPath,
          // The cached WAV begins at the clip's trim point, while the regular
          // frame plan addresses the original source timeline.
          sourceTime: Math.max(0, source.sourceTime - clip.trimIn),
        };
      }),
    };
  }

  function handleProxyReady(sourcePath: string, proxyPath: string) {
    proxyPaths = { ...proxyPaths, [normalizeMediaPath(sourcePath)]: proxyPath };
  }

  function showExport() {
    timelineSnapshot = timelineRef?.getProjectState?.() ?? null;
    const staleDucking = timelineSnapshot?.clips.find((clip) => {
      if (!clip.autoDucking) return false;
      const diagnostics = getAutoDuckingDiagnostics(timelineSnapshot!, clip);
      return diagnostics.enabled && diagnostics.staleSourceClipIds.length > 0;
    });
    if (staleDucking) {
      selectedClip = staleDucking;
      projectError = "Dışa aktarma durduruldu: konuşma değiştiği için AI Ducking’i yeniden analiz edin.";
      return;
    }
    exportOpen = true;
  }

  function handleCaptionStudioChange(next: CaptionStudioProject) {
    captionStudio = next;
    if (projectSession && !applyingProject && isTauri()) {
      projectSession.update((document) => ({
        ...document,
        settings: editorSettings(),
      }));
    }
  }

  async function transcribeCaptionSourceForCaptions() {
    const clip = captionTranscriptionClip;
    if (
      !isTauri() ||
      !clip ||
      !clip.file ||
      clip.audioSeparated ||
      (clip.kind !== "audio" && clip.kind !== "video") ||
      captionTranscriptionBusy
    ) return;
    const signature = speechSourceSignature(clip);
    captionTranscriptionBusy = true;
    captionTranscriptionMessage = "Whisper kelime zamanlarını ve güven skorlarını çıkarıyor…";
    captionTranscriptionError = false;
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onSpeechSetupProgress((progress) => {
        captionTranscriptionMessage = progress.progressPercent === undefined
          ? progress.message
          : `${progress.message} · %${Math.round(progress.progressPercent)}`;
      });
      const source = await analysisSourceForClip(clip);
      const transcript = await analyzeSpeechTranscript({
        ...source,
        force: true,
        language: "auto",
      });
      const snapshot = (timelineRef?.getProjectState?.() as TimelineProjectSnapshot | undefined)
        ?? timelineSnapshot;
      const current = snapshot?.clips.find((item) => item.id === clip.id);
      if (!current || speechSourceSignature(current) !== signature) {
        throw new Error("Klip transkripsiyon sırasında değişti; caption sonucu uygulanmadı.");
      }
      const imported = captionStudioFromWordTranscript(transcript.words, {
        timelineOffsetMs: Math.round(clip.start * 1_000),
          sourceName: captionSourceFileName(clip.file),
        primaryLanguage: "tr",
      });
      const next: CaptionStudioProject = {
        ...imported,
        dictionary: captionStudio.dictionary,
        thresholds: captionStudio.thresholds,
        targetLanguage: captionStudio.targetLanguage,
        template: captionStudio.template,
        bilingualOrder: captionStudio.bilingualOrder,
      };
      handleCaptionStudioChange(next);
      captionTranscriptionMessage = transcript.words.length
        ? `✓ ${transcript.words.length} kelime · ${next.cues.length} cue${transcript.cacheHit ? " · önbellekten" : ""}`
        : "Konuşma bulunamadı; caption listesi boş bırakıldı.";
    } catch (error) {
      captionTranscriptionMessage = formatAudioCommandError(error);
      captionTranscriptionError = true;
    } finally {
      unlisten?.();
      captionTranscriptionBusy = false;
    }
  }

  function applyCaptionStudioToTimeline(next: CaptionStudioProject) {
    if (!timelineRef || next.cues.length === 0) return;
    handleCaptionStudioChange(next);
    const { tracks, clips } = buildCaptionTimelinePlan(next, previewAspect);
    const applied = timelineRef.replaceGeneratedTextClips?.(
      CAPTION_CLIP_SOURCE_PREFIX,
      clips,
      tracks,
    );
    if (!applied) {
      window.alert("Caption kanalı kilitli. Timeline'da CC kanalının kilidini açıp yeniden deneyin.");
      return;
    }
    timelineSnapshot = timelineRef.getProjectState?.() ?? timelineSnapshot;
    captionStudioOpen = false;
  }

  function editorSettings(): EditorSettings {
    return {
      previewAspect,
      captionStudio,
      autoReframe: cloneAutoReframeProjectState(autoReframeState),
      ...(projectCover ? { cover: projectCover } : {}),
    };
  }

  function handleCoverCapture(capture: ProjectCover) {
    const cover = normalizeProjectCover(capture);
    if (!cover) {
      projectError = "Kapak görseli geçersiz olduğu için kaydedilmedi.";
      return;
    }
    projectCover = cover;
    if (projectSession && !applyingProject && isTauri()) {
      projectSession.update((document) => ({
        ...document,
        settings: editorSettings(),
      }));
    }
  }

  function clearProjectCover() {
    projectCover = null;
    if (projectSession && !applyingProject && isTauri()) {
      projectSession.update((document) => ({
        ...document,
        settings: editorSettings(),
      }));
    }
  }

  function formatCoverTime(timeMs: number) {
    const totalSeconds = Math.max(0, Math.floor(timeMs / 1_000));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function createEmptyTimelineSnapshot(): TimelineProjectSnapshot {
    return serializeProjectState({
      version: 2,
      tracks: [
        createTrack("v1", "V1", "video"),
        createTrack("a1", "A1", "audio"),
        createTrack("t1", "T1", "text"),
      ],
      clips: [],
      selectedClipId: null,
    });
  }

  async function createNewProject() {
    appMenuOpen = false;
    if (projectBusy) return;
    if (
      projectState?.dirty &&
      !window.confirm("Kaydedilmemiş değişiklikler silinecek. Yeni proje oluşturulsun mu?")
    ) {
      return;
    }

    projectBusy = true;
    projectError = null;
    try {
      const previous = projectSession;
      const previousProjectId = previous?.snapshot.document.projectId;
      if (previous) await previous.dispose({ flush: false });

      if (isTauri() && previousProjectId) {
        try {
          const snapshots = await listRecoverySnapshots();
          const obsolete = snapshots.filter(
            (snapshot) => snapshot.projectId === previousProjectId,
          );
          await Promise.all(
            obsolete.map((snapshot) => discardRecoverySnapshot(snapshot.snapshotId)),
          );
          recoverySnapshots = snapshots.filter(
            (snapshot) => snapshot.projectId !== previousProjectId,
          );
        } catch {
          // A new project can still start if stale recovery cleanup is unavailable.
        }
      }

      const timeline = createEmptyTimelineSnapshot();
      captionStudio = createEmptyCaptionStudioProject();
      projectCover = null;
      const base = createProjectDocument<TimelineProjectSnapshot>({
        name: "Project 01",
        timeline,
      });
      const document: EditorDocument = {
        ...base,
        settings: editorSettings(),
      };
      await activateSession(createSession(document));
      recoveryOpen = false;
      missingMediaOpen = false;
      relinkResult = null;
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    } finally {
      projectBusy = false;
    }
  }

  function handleSessionChange(
    state: ProjectSessionState<TimelineProjectSnapshot, EditorSettings>,
  ) {
    projectState = state;
    rememberDevelopmentSession(state);
    if (state.saveStatus.phase === "error") {
      const error = state.saveStatus.error;
      autosaveError = error instanceof Error ? error.message : String(error ?? "Otomatik kayıt başarısız");
    } else {
      autosaveError = null;
    }
  }

  function rememberDevelopmentSession(
    state: ProjectSessionState<TimelineProjectSnapshot, EditorSettings>,
  ) {
    if (!import.meta.env.DEV || typeof localStorage === "undefined") return;
    try {
      const activeProjectId = localStorage.getItem(DEV_ACTIVE_PROJECT_KEY);
      if (state.dirty) {
        localStorage.setItem(DEV_ACTIVE_PROJECT_KEY, state.document.projectId);
      } else if (activeProjectId === state.document.projectId) {
        localStorage.removeItem(DEV_ACTIVE_PROJECT_KEY);
      }
    } catch {
      // Development continuity is optional when web storage is unavailable.
    }
  }

  function selectDevelopmentRecovery(
    snapshots: RecoverySnapshotInfo[],
  ): RecoverySnapshotInfo | null {
    if (!import.meta.env.DEV || typeof localStorage === "undefined") return null;
    const validSnapshots = snapshots.filter((snapshot) => snapshot.isValid);
    if (validSnapshots.length === 0) return null;

    try {
      const activeProjectId = localStorage.getItem(DEV_ACTIVE_PROJECT_KEY);
      const activeSnapshot = activeProjectId
        ? validSnapshots.find((snapshot) => snapshot.projectId === activeProjectId)
        : undefined;
      if (activeSnapshot) return activeSnapshot;
      return validSnapshots[0] ?? null;
    } catch {
      return null;
    }
    return null;
  }

  function showDevelopmentRecoveryNotice() {
    const message = "Kod yenilendi · çalışma otomatik geri yüklendi";
    devRecoveryNotice = message;
    window.setTimeout(() => {
      if (devRecoveryNotice === message) devRecoveryNotice = null;
    }, 4_500);
  }

  function createSession(document: EditorDocument) {
    return new ProjectSession<TimelineProjectSnapshot, EditorSettings>({
      document,
      autosaveDelayMs: AUTOSAVE_DELAY_MS,
      autosaveMaxWaitMs: AUTOSAVE_MAX_WAIT_MS,
      onChange: handleSessionChange,
    });
  }

  async function ensureBlankProjectSession() {
    if (projectSession) return;
    await tick();
    const initialTimeline = timelineRef?.getProjectState?.() ?? timelineSnapshot;
    if (!initialTimeline) return;
    const base = createProjectDocument<TimelineProjectSnapshot>({
      name: "Project 01",
      timeline: initialTimeline,
    });
    const document: EditorDocument = {
      ...base,
      settings: editorSettings(),
    };
    await activateSession(createSession(document), false);
  }

  async function activateSession(
    next: ProjectSession<TimelineProjectSnapshot, EditorSettings>,
    apply = true,
  ) {
    const previous = projectSession;
    try {
      if (apply) await applyProjectDocument(next.snapshot.document);
    } catch (error) {
      await next.dispose();
      throw error;
    }
    projectSession = next;
    projectState = next.snapshot;
    mediaSyncRequest += 1;
    projectError = null;
    if (previous && previous !== next) await previous.dispose();
  }

  async function applyProjectDocument(document: EditorDocument) {
    if (!isTimelineProjectSnapshot(document.timeline)) {
      throw new Error("Proje zaman çizelgesi bozuk veya desteklenmeyen bir sürüm kullanıyor.");
    }
    applyingProject = true;
    try {
      timelineSnapshot = document.timeline;
      timelineRef?.loadProjectState?.(document.timeline);
      mediaFiles = document.media.map((asset) => asset.sourcePath);
      lastMediaSignature = mediaFiles.join("\u0000");
      previewAspect = document.settings?.previewAspect ?? "16:9";
      autoReframeState = normalizeAutoReframeProjectState(document.settings?.autoReframe);
      captionStudio = normalizeCaptionStudioProject(document.settings?.captionStudio);
      projectCover = normalizeProjectCover(document.settings?.cover);
      playheadTime = 0;
      directPreviewPath = null;
      selectedClip = null;
      await tick();
      timelineFramePlan = timelineRef?.getFramePlan?.(0) ?? {
        time: 0,
        layers: [],
        audio: [],
      };
    } finally {
      applyingProject = false;
    }
  }

  function handleTimelineStateChange(snapshot: TimelineProjectSnapshot) {
    timelineSnapshot = snapshot;
    if (!directPreviewPath) {
      timelineFramePlan = timelineRef?.getFramePlan?.(playheadTime) ?? timelineFramePlan;
    }

    const discovered = snapshot.clips
      .filter((clip) => clip.kind !== "text")
      .map((clip) => clip.file)
      .filter(Boolean);
    if (discovered.some((path) => !mediaFiles.includes(path))) {
      mediaFiles = [...new Set([...mediaFiles, ...discovered])];
    }

    if (applyingProject || !projectSession || !isTauri()) return;
    const activeDocument = projectState?.document ?? projectSession.snapshot.document;
    if (
      activeDocument.settings?.previewAspect === previewAspect &&
      JSON.stringify(activeDocument.timeline) === JSON.stringify(snapshot)
    ) {
      return;
    }
    projectSession.update((document) => ({
      ...document,
      timeline: snapshot,
      settings: editorSettings(),
    }));
  }

  async function syncMediaLibrary(paths: readonly string[]) {
    if (!projectSession || !isTauri() || applyingProject) return;
    const session = projectSession;
    const requestId = ++mediaSyncRequest;
    const pathSet = new Set(paths.map(normalizeMediaPath));
    const existing = session.snapshot.document.media.filter((asset) =>
      pathSet.has(normalizeMediaPath(asset.sourcePath)),
    );
    const existingPaths = new Set(existing.map((asset) => normalizeMediaPath(asset.sourcePath)));
    const pendingPaths = paths.filter((path) => !existingPaths.has(normalizeMediaPath(path)));
    const imported = pendingPaths.length
      ? await importMediaPaths(pendingPaths, existing)
      : { assets: [] as MediaAsset[], rejected: [] };
    if (requestId !== mediaSyncRequest || projectSession !== session) return;
    const nextMedia = [...existing, ...imported.assets];
    const previousSignature = session.snapshot.document.media
      .map((asset) => `${asset.id}:${asset.sourcePath}:${asset.availability}`)
      .join("|");
    const nextSignature = nextMedia
      .map((asset) => `${asset.id}:${asset.sourcePath}:${asset.availability}`)
      .join("|");
    if (previousSignature !== nextSignature) {
      session.update((document) => ({ ...document, media: nextMedia }));
    }
  }

  function normalizeMediaPath(path: string) {
    return path.replaceAll("\\", "/").toLowerCase();
  }

  async function saveCurrentProject(saveAs = false) {
    if (!projectSession) return;
    projectBusy = true;
    projectError = null;
    try {
      const snapshot = timelineRef?.getProjectState?.() ?? timelineSnapshot;
      if (snapshot) {
        projectSession.update((document) => ({
          ...document,
          timeline: snapshot,
          settings: editorSettings(),
        }));
      }
      await syncMediaLibrary(mediaFiles);
      let path = saveAs ? undefined : projectSession.snapshot.path;
      if (!path) {
        const safeName = projectSession.snapshot.document.name.replace(/[<>:"/\\|?*]+/g, "-");
        path = await chooseProjectSavePath(`${safeName}.astral`) ?? undefined;
      }
      if (!path) return;
      await projectSession.save(path);
      recoverySnapshots = recoverySnapshots.filter(
        (snapshotInfo) => snapshotInfo.projectId !== projectSession?.snapshot.document.projectId,
      );
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    } finally {
      projectBusy = false;
    }
  }

  async function openProjectFile() {
    if (!isTauri()) return;
    const path = await chooseProjectToOpen();
    if (!path) return;
    projectBusy = true;
    projectError = null;
    try {
      const next = await ProjectSession.open<TimelineProjectSnapshot, EditorSettings>(path, {
        autosaveDelayMs: AUTOSAVE_DELAY_MS,
        autosaveMaxWaitMs: AUTOSAVE_MAX_WAIT_MS,
        onChange: handleSessionChange,
      });
      await activateSession(next);
      const audited = await auditMissingMedia(next.snapshot.document.media);
      if (audited.some((asset, index) => asset.availability !== next.snapshot.document.media[index]?.availability)) {
        next.replace({ ...next.snapshot.document, media: audited });
      }
      missingMediaOpen = audited.some((asset) => asset.availability === "missing");
      relinkResult = null;
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    } finally {
      projectBusy = false;
    }
  }

  async function recoverProject(snapshotId: string) {
    projectBusy = true;
    projectError = null;
    try {
      const next = await ProjectSession.recover<TimelineProjectSnapshot, EditorSettings>(snapshotId, {
        autosaveDelayMs: AUTOSAVE_DELAY_MS,
        autosaveMaxWaitMs: AUTOSAVE_MAX_WAIT_MS,
        onChange: handleSessionChange,
      });
      await activateSession(next);
      recoveryOpen = false;
      const previousMedia = next.snapshot.document.media;
      const audited = await auditMissingMedia(next.snapshot.document.media);
      if (audited.some((asset, index) => asset.availability !== previousMedia[index]?.availability)) {
        next.replace({ ...next.snapshot.document, media: audited });
      }
      if (audited.some((asset) => asset.availability === "missing")) {
        missingMediaOpen = true;
      }
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    } finally {
      projectBusy = false;
    }
  }

  async function skipRecovery() {
    recoveryOpen = false;
    try {
      await ensureBlankProjectSession();
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    }
  }

  async function discardRecovery(snapshotId: string) {
    projectBusy = true;
    projectError = null;
    try {
      await discardRecoverySnapshot(snapshotId);
      recoverySnapshots = recoverySnapshots.filter((item) => item.snapshotId !== snapshotId);
      if (recoverySnapshots.length === 0) {
        recoveryOpen = false;
        await ensureBlankProjectSession();
      }
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    } finally {
      projectBusy = false;
    }
  }

  async function searchMissingMedia(roots: string[]) {
    if (!projectState) return;
    relinkBusy = true;
    projectError = null;
    try {
      relinkResult = await searchForMissingMedia(projectState.document.media, roots);
    } catch (error) {
      projectError = error instanceof Error ? error.message : String(error);
    } finally {
      relinkBusy = false;
    }
  }

  async function applyMediaReplacements(replacements: Map<string, string>) {
    if (!projectSession) return;
    const references = new Map(
      projectSession.snapshot.document.media.map((asset) => [asset.sourcePath, asset]),
    );
    const resolutions: RelinkResolution[] = [...replacements].map(([from, to]) => ({
      reference: {
        assetId: references.get(from)?.id,
        path: from,
        sizeBytes: references.get(from)?.sizeBytes,
        quickHash: references.get(from)?.quickHash,
      },
      replacementPath: to,
      confidence: 1,
      reason: "Kullanıcı tarafından doğrulandı",
    }));
    const updated = applyRelinkResolutions(projectSession.snapshot.document, resolutions);
    projectSession.replace(updated);
    await applyProjectDocument(updated);
    missingMediaOpen = updated.media.some((asset) => asset.availability === "missing");
    relinkResult = null;
  }

  function changePreviewAspect(next: "9:16" | "1:1" | "16:9") {
    previewAspect = next;
    if (projectSession && !applyingProject && isTauri()) {
      projectSession.update((document) => ({
        ...document,
        settings: editorSettings(),
      }));
    }
  }

  // Resize
  function handleSidebarResize(e: CustomEvent<{ delta: number }>) {
    updatePanelWidth("sidebar", sidebarWidth - e.detail.delta);
  }
  function handleInspectorResize(e: CustomEvent<{ delta: number }>) {
    updatePanelWidth("inspector", inspectorWidth - e.detail.delta);
  }
  function handleTimelineResize(e: CustomEvent<{ delta: number }>) {
    updateTimelineHeight(timelineHeight - e.detail.delta);
  }

  // Keyboard
  function isTextEntryTarget(target: EventTarget | null) {
    if (
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement
    ) {
      return true;
    }
    if (target instanceof HTMLElement && target.isContentEditable) return true;
    if (!(target instanceof HTMLInputElement)) return false;
    return ![
      "range",
      "button",
      "submit",
      "reset",
      "checkbox",
      "radio",
    ].includes(target.type);
  }

  function isNativeSpaceControl(target: EventTarget | null) {
    if (
      target instanceof HTMLButtonElement ||
      target instanceof HTMLSelectElement ||
      target instanceof HTMLAnchorElement
    ) {
      return true;
    }
    return target instanceof HTMLInputElement && target.type !== "range";
  }

  function isNativeSeekControl(target: EventTarget | null) {
    if (
      target instanceof HTMLInputElement ||
      target instanceof HTMLSelectElement
    ) {
      return true;
    }
    return (
      target instanceof HTMLElement &&
      target.matches(
        '[role="slider"], [role="spinbutton"], [role="separator"], [role="listbox"], [role="option"]',
      )
    );
  }

  function transportIsBlocked() {
    return (
      appMenuOpen ||
      exportOpen ||
      autoReframeOpen ||
      autoReframeOpening ||
      audioExportTarget !== null ||
      proxyOpen ||
      recoveryOpen ||
      missingMediaOpen
    );
  }

  function toggleEditorPlayback() {
    if (!isPlaying) {
      playheadTime = playbackStartTime(playheadTime, playheadDuration);
    }
    isPlaying = !isPlaying;
  }

  function seekEditor(action: TransportSeekAction) {
    playheadTime = applyTransportSeek(
      playheadTime,
      playheadDuration,
      action,
    );
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && appMenuOpen) {
      appMenuOpen = false;
      return;
    }
    if (e.defaultPrevented || isTextEntryTarget(e.target)) return;
    if (transportIsBlocked()) return;
    const modifier = e.ctrlKey || e.metaKey;
    if (modifier && e.code === "KeyN") {
      e.preventDefault();
      void createNewProject();
      return;
    }
    if (modifier && e.code === "KeyS") {
      e.preventDefault();
      void saveCurrentProject(e.shiftKey);
      return;
    }
    if (modifier && e.code === "KeyO") {
      e.preventDefault();
      void openProjectFile();
      return;
    }

    const shortcut = resolveTransportShortcut(e);
    if (!shortcut) return;
    if (
      shortcut.kind === "toggle-playback" &&
      e.code === "Space" &&
      isNativeSpaceControl(e.target)
    ) {
      return;
    }
    if (
      (shortcut.kind === "seek-relative" || shortcut.kind === "seek-edge") &&
      isNativeSeekControl(e.target)
    ) {
      return;
    }

    e.preventDefault();
    if (shortcut.kind === "consume") return;
    if (shortcut.kind === "toggle-playback") {
      toggleEditorPlayback();
      return;
    }
    seekEditor(shortcut);
  }

  function handleWindowPointerDown(event: PointerEvent) {
    if (!appMenuOpen || !appMenuRef) return;
    if (event.target instanceof Node && appMenuRef.contains(event.target)) return;
    appMenuOpen = false;
  }

  $effect(() => {
    const signature = mediaFiles.join("\u0000");
    if (signature === lastMediaSignature) return;
    lastMediaSignature = signature;
    void syncMediaLibrary([...mediaFiles]).catch((error) => {
      projectError = error instanceof Error ? error.message : String(error);
    });
  });

  // File Drop
  onMount(() => {
    let disposed = false;
    if (isTauri()) {
      void checkFireworksSmartShorts()
        .then((status) => {
          if (!disposed) fireworksSmartShortsConfigured = status.configured;
        })
        .catch(() => undefined);
    }
    void (async () => {
      await tick();
      if (disposed || projectSession) return;

      if (isTauri()) {
        try {
          recoverySnapshots = await listRecoverySnapshots();
          if (disposed) return;
          const developmentRecovery = selectDevelopmentRecovery(recoverySnapshots);
          if (developmentRecovery) {
            await recoverProject(developmentRecovery.snapshotId);
            if (projectError) {
              recoveryOpen = recoverySnapshots.length > 0;
            } else {
              showDevelopmentRecoveryNotice();
            }
          } else {
            recoveryOpen = recoverySnapshots.length > 0;
          }
          if (recoveryOpen || projectSession) return;
        } catch (error) {
          projectError = error instanceof Error ? error.message : String(error);
        }
      }

      if (!disposed) await ensureBlankProjectSession();
    })();

    const unlistenPromise = isTauri() ? listen("tauri://drag-drop", (event) => {
      const payload = event.payload as { paths: string[] };
      if (payload.paths?.length) {
        void importMediaPaths(payload.paths, projectState?.document.media ?? [])
          .then(({ assets }) => {
            const paths = assets.map((asset) => asset.sourcePath);
            const merged = [...new Set([...mediaFiles, ...paths])];
            if (merged.length === mediaFiles.length) return;
            commitUndoPoint();
            mediaFiles = merged;
          })
          .catch((error) => {
            projectError = error instanceof Error ? error.message : String(error);
          });
      }
    }) : Promise.resolve(() => undefined);
    return () => {
      disposed = true;
      void unlistenPromise.then((unlisten) => unlisten());
      void projectSession?.dispose();
    };
  });
</script>

<svelte:window onkeydown={handleKeydown} onpointerdown={handleWindowPointerDown} />

<MediaDragGhost />

{#if devRecoveryNotice}
  <div class="dev-recovery-notice" role="status">
    <span class="notice-dot"></span>
    <span>{devRecoveryNotice}</span>
  </div>
{/if}

{#if !autoReframeOpen && autoReframeBackgroundJob}
  <button
    type="button"
    class="auto-reframe-background-toast"
    class:completed={autoReframeBackgroundJob.status === "completed"}
    class:error={autoReframeBackgroundJob.status === "error"}
    style:bottom={`${showTimeline && !isTimelineDetached ? timelineHeight + 16 : 16}px`}
    aria-label={`Akıllı kadraj: ${autoReframeBackgroundJob.clipName}. ${autoReframeBackgroundJob.message}`}
    aria-live="polite"
    title="Akıllı Kadraj ayrıntılarını aç"
    onclick={() => openAutoReframeBackgroundJob()}
  >
    <span class="auto-reframe-toast-badge" aria-hidden="true">
      {autoReframeBackgroundJob.status === "completed"
        ? "✓"
        : autoReframeBackgroundJob.status === "error"
          ? "!"
          : "AI"}
    </span>
    <span class="auto-reframe-toast-copy">
      <strong>{autoReframeBackgroundJob.clipName}</strong>
      <small>{autoReframeBackgroundJob.message}</small>
    </span>
    <span class="auto-reframe-toast-value">
      {autoReframeBackgroundJob.status === "completed"
        ? "HAZIR"
        : autoReframeBackgroundJob.status === "error"
          ? "HATA"
          : `%${Math.round(autoReframeBackgroundJob.progress)}`}
    </span>
    {#if autoReframeBackgroundJob.status !== "error"}
      <span
        class="auto-reframe-toast-rail"
        role="progressbar"
        aria-label="Arka plan akıllı kadraj ilerlemesi"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={Math.round(autoReframeBackgroundJob.progress)}
      >
        <span
          class="auto-reframe-toast-fill"
          style:width={`${autoReframeBackgroundJob.progress}%`}
        ></span>
      </span>
    {/if}
  </button>
{/if}

<div class="editor">
  <!-- Header -->
  <header class="topbar">
    <div class="brand">
      <div class="app-menu" bind:this={appMenuRef}>
        <button
          class="logo app-menu-trigger"
          class:active={appMenuOpen}
          aria-label="Uygulama menüsü"
          aria-haspopup="menu"
          aria-expanded={appMenuOpen}
          title="Proje ve uygulama menüsü"
          onclick={() => (appMenuOpen = !appMenuOpen)}
        >
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <circle cx="12" cy="12" r="10" /> <path d="M12 8v8" />
            <path d="M8 12h8" />
          </svg>
        </button>

        {#if appMenuOpen}
          <div class="app-menu-popover" role="menu">
            <span class="menu-label">Proje</span>
            <button role="menuitem" disabled={projectBusy} onclick={() => void createNewProject()}>
              <span class="menu-icon">＋</span><span>Yeni proje</span><kbd>Ctrl+N</kbd>
            </button>
            <button role="menuitem" disabled={projectBusy} onclick={() => { appMenuOpen = false; void openProjectFile(); }}>
              <span class="menu-icon">⌁</span><span>Aç…</span><kbd>Ctrl+O</kbd>
            </button>
            <button role="menuitem" disabled={projectBusy} onclick={() => { appMenuOpen = false; void saveCurrentProject(false); }}>
              <span class="menu-icon">↓</span><span>Kaydet</span><kbd>Ctrl+S</kbd>
            </button>
            <button role="menuitem" disabled={projectBusy} onclick={() => { appMenuOpen = false; void saveCurrentProject(true); }}>
              <span class="menu-icon">⇣</span><span>Farklı kaydet…</span><kbd>Ctrl+Shift+S</kbd>
            </button>

            <div class="menu-separator"></div>
            <span class="menu-label">Araçlar</span>
            <button role="menuitem" onclick={() => { appMenuOpen = false; proxyOpen = true; }}>
              <span class="menu-icon">◇</span><span>Proxy / cache</span>
            </button>
            <button role="menuitem" onclick={() => { appMenuOpen = false; openCaptionStudioPanel(); }}>
              <span class="menu-icon">CC</span><span>Caption QA Studio</span>
            </button>
            <button class="menu-export" role="menuitem" onclick={() => { appMenuOpen = false; showExport(); }}>
              <span class="menu-icon">↗</span><span>Dışa aktar</span>
            </button>
          </div>
        {/if}
      </div>
      <span class="app-name">astral</span>
    </div>
    <div class="project-info">
      <span class="project-title">{projectName}</span>
      <span
        class="save-status"
        class:dirty={projectState?.dirty}
        class:error={Boolean(projectDisplayError)}
        title={projectDisplayError ?? projectState?.path ?? ""}
      >{projectSaveStatus}</span>
    </div>
    <div class="actions">
      {#if projectCover}
        <div class="project-cover-chip" title={`Kapak karesi: ${formatCoverTime(projectCover.timeMs)}`}>
          <img src={projectCover.imageDataUrl} alt="Seçili proje kapağı" />
          <span>Kapak · {formatCoverTime(projectCover.timeMs)}</span>
          <button type="button" onclick={clearProjectCover} aria-label="Proje kapağını kaldır" title="Kapağı kaldır">×</button>
        </div>
      {/if}
      <button
        class="btn-secondary caption-qa-button"
        class:active={captionStudioOpen}
        aria-pressed={captionStudioOpen}
        onclick={() => (captionStudioOpen ? closeCaptionStudioPanel() : openCaptionStudioPanel())}
      >
        <span>CC</span> Caption QA
        {#if summaryCaptionIssues > 0}<i>{summaryCaptionIssues}</i>{/if}
      </button>
      {#if missingReferences.length > 0}
        <button class="btn-secondary warning" onclick={() => (missingMediaOpen = true)}>{missingReferences.length} eksik medya</button>
      {/if}
    </div>
  </header>

  <div class="workspace">
    <!-- LEFT: Sidebar (Media Pool) -->
    {#if showSidebar && !isMediaPoolDetached}
      <aside class="sidebar" style="width: {sidebarWidth}px;">
        <Panel
          id="media-pool"
          title="Kütüphane"
          onDetach={() => detachPanel("media-pool")}
        >
          {#snippet children()}
            {@render libraryContent()}
          {/snippet}
        </Panel>
      </aside>
      <Splitter
        direction="horizontal"
        position="left"
        on:resize={handleSidebarResize}
      />
    {/if}

    <!-- CENTER: Canvas / Player -->
    <main class="canvas-area">
      {#if !isPreviewDetached}
        <CompositorPlayer
          plan={previewPlan}
          bind:currentTime={playheadTime}
          duration={playheadDuration}
          bind:isPlaying
          transportRevision={previewTransportRevision}
          aspect={previewAspect}
          selectedClipId={selectedClip?.id ?? null}
          transformDisabled={selectedClipLocked || Boolean(selectedAutoReframeEntry)}
          onAspectChange={changePreviewAspect}
          onMediaDuration={handlePreviewMediaDuration}
          onPositionChange={handlePreviewPositionChange}
          onTransformChange={handlePreviewTransformChange}
          onResetTransform={handleResetTransform}
          onCoverCapture={handleCoverCapture}
          coverTimeMs={projectCover?.timeMs ?? null}
          onDetach={() => detachPanel("preview")}
        />
      {:else}
        <div class="detached-placeholder">
          <p>Video önizleme ayrı pencerede</p>
          <button class="restore-btn" onclick={() => attachPanel("preview")}
            >Geri Getir</button
          >
        </div>
      {/if}
    </main>

    <!-- RIGHT: Inspector -->
    {#if showInspector && !isInspectorDetached}
      <Splitter
        direction="horizontal"
        position="right"
        on:resize={handleInspectorResize}
      />
      <aside class="inspector" style="width: {inspectorWidth}px;">
        <Panel
          id="inspector"
          title="Özellikler"
          onDetach={() => detachPanel("inspector")}
        >
          {#snippet children()}
            {@render inspectorContent()}
          {/snippet}
        </Panel>
      </aside>
    {/if}
  </div>

  <Toolbar
    transitionOpen={libraryTab === "transitions"}
    onTransitionToggle={toggleTransitionLibrary}
  />

  <!-- BOTTOM: Timeline -->
  {#if showTimeline && !isTimelineDetached}
    <Splitter
      direction="vertical"
      position="top"
      on:resize={handleTimelineResize}
    />
    <footer class="timeline-area" style="height: {timelineHeight}px;">
      <!-- Timeline header removed/integrated into Panel if needed, or kept simple -->
      <Timeline
        bind:this={timelineRef}
        bind:currentTime={playheadTime}
        duration={timelineContentDuration}
        onClipTimeChange={handleTimelineClipTimeChange}
        onDurationChange={handleTimelineDurationChange}
        onProjectStateChange={handleTimelineStateChange}
        onSelect={handleMediaSelect}
        onScrubBegin={() => (isPlaying = false)}
        onAudioExport={openTimelineAudioExport}
        onAudioNormalize={handleNormalizeSelectedAudio}
        audioNormalizeBusy={Boolean(audioNormalizeBusyClipId || voiceRiderBusyClipId)}
        onVoiceRider={handleVoiceRider}
        onVoiceRiderRemove={handleRemoveVoiceRider}
        voiceRiderBusy={Boolean(voiceRiderBusyClipId || audioNormalizeBusyClipId)}
        speechSuggestions={selectedSpeechReview?.suggestions ?? []}
        onAudioRegionChange={(state) => (audioRegionUi = state)}
        onTextClipCreated={handleTextClipCreated}
        transitionEditorSide={libraryTab === "transitions" ? transitionSide : null}
        onTransitionSelect={openTransitionLibraryForSide}
        getExternalSnapshot={captureExternalUndoState}
        applyExternalSnapshot={restoreExternalUndoState}
        autoReframeJob={autoReframeBackgroundJob}
        onAutoReframeJobOpen={openAutoReframeBackgroundJob}
      />
    </footer>
  {/if}
</div>

<!-- FLOATING PANELS -->
{#if isMediaPoolDetached}
  <Panel
    id="media-pool"
    title="Kütüphane"
    isDetached={true}
    detachedState={mediaPoolState}
    onAttach={() => attachPanel("media-pool")}
  >
    {#snippet children()}
      {@render libraryContent()}
    {/snippet}
  </Panel>
{/if}

{#if isPreviewDetached}
  <Panel
    id="preview"
    title="Video Önizleme"
    isDetached={true}
    detachedState={previewState}
    onAttach={() => attachPanel("preview")}
  >
    {#snippet children()}
      <CompositorPlayer
        plan={previewPlan}
        bind:currentTime={playheadTime}
        duration={playheadDuration}
        bind:isPlaying
        transportRevision={previewTransportRevision}
        aspect={previewAspect}
        selectedClipId={selectedClip?.id ?? null}
        transformDisabled={selectedClipLocked || Boolean(selectedAutoReframeEntry)}
        onAspectChange={changePreviewAspect}
        onMediaDuration={handlePreviewMediaDuration}
        onPositionChange={handlePreviewPositionChange}
        onTransformChange={handlePreviewTransformChange}
        onResetTransform={handleResetTransform}
        onCoverCapture={handleCoverCapture}
        coverTimeMs={projectCover?.timeMs ?? null}
        onDetach={undefined}
      />
    {/snippet}
  </Panel>
{/if}

{#if isInspectorDetached}
  <Panel
    id="inspector"
    title="Özellikler"
    isDetached={true}
    detachedState={inspectorState}
    onAttach={() => attachPanel("inspector")}
  >
    {#snippet children()}
      {@render inspectorContent()}
    {/snippet}
  </Panel>
{/if}

{#snippet libraryContent()}
  <div class="library-stack">
    <div class="library-tabs" role="tablist" aria-label="Kütüphane modu">
      <button
        type="button"
        role="tab"
        aria-selected={libraryTab === "media"}
        class:active={libraryTab === "media"}
        onclick={() => showLibraryTab("media")}
      >
        <span aria-hidden="true">▣</span>
        Medya
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={libraryTab === "transitions"}
        class:active={libraryTab === "transitions"}
        onclick={() => showLibraryTab("transitions")}
      >
        <span aria-hidden="true">◢◣</span>
        Geçişler
      </button>
    </div>
    <div class="library-body">
      {#if libraryTab === "media"}
        <MediaPool
          bind:mediaFiles
          onSelect={handleMediaSelect}
          onAddToTimeline={handleAddMediaToTimeline}
          onExtractAudio={openAudioExtraction}
          onHistoryPoint={commitUndoPoint}
        />
      {:else}
        <TransitionBrowser
          clip={selectedClip}
          side={transitionSide}
          disabled={selectedClipLocked}
          onSideChange={(next: "in" | "out") => (transitionSide = next)}
          onChange={handleTransitionChange}
        />
      {/if}
    </div>
  </div>
{/snippet}

{#snippet inspectorContent()}
  <div class="inspector-stack">
    <div class="inspector-mode-tabs" role="tablist" aria-label="Özellik paneli modu">
      <button
        type="button"
        role="tab"
        aria-selected={!captionStudioOpen && !smartShortsStudioOpen && !audioStudioOpen}
        class:active={!captionStudioOpen && !smartShortsStudioOpen && !audioStudioOpen}
        onclick={openClipInspectorPanel}
      >
        <span aria-hidden="true">◇</span>
        Klip
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={audioStudioOpen}
        class:active={audioStudioOpen}
        onclick={openAudioStudioPanel}
      >
        <span class="audio-label" aria-hidden="true">◒</span>
        Ses AI
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={smartShortsStudioOpen}
        class:active={smartShortsStudioOpen}
        onclick={openSmartShortsStudioPanel}
      >
        <span class="shorts-label" aria-hidden="true">✦</span>
        Shorts
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={captionStudioOpen}
        class:active={captionStudioOpen}
        onclick={openCaptionStudioPanel}
      >
        <span class="cc-label" aria-hidden="true">CC</span>
        Altyazı
        {#if summaryCaptionIssues > 0}<i>{summaryCaptionIssues}</i>{/if}
      </button>
    </div>

    <div
      class="inspector-mode-body"
      class:caption-mode={captionStudioOpen}
      class:shorts-mode={smartShortsStudioOpen}
      class:audio-mode={audioStudioOpen}
    >
      {#if captionStudioOpen}
        <CaptionQaStudio
          open={true}
          project={captionStudio}
          transcriptionSources={captionTranscriptionSources}
          transcriptionSourceId={captionSourceClipId}
          canTranscribe={canCaptionTranscribe}
          transcribeBusy={captionTranscriptionBusy}
          transcribeMessage={captionTranscriptionMessage}
          transcribeError={captionTranscriptionError}
          onChange={handleCaptionStudioChange}
          onTranscriptionSourceChange={selectCaptionSourceClip}
          onTranscribe={() => void transcribeCaptionSourceForCaptions()}
          onApply={applyCaptionStudioToTimeline}
          onClose={closeCaptionStudioPanel}
        />
      {:else if smartShortsStudioOpen}
        {#if selectedClip?.kind === "video" && !selectedClip.audioSeparated}
          <SmartShortsPanel
            disabled={selectedClipLocked}
            busy={selectedSmartShortsBusy}
            progress={selectedSmartShortsBusy ? smartShortsProgress : 0}
            progressStage={selectedSmartShortsBusy ? smartShortsProgressStage : null}
            progressElapsedMs={selectedSmartShortsBusy ? smartShortsProgressElapsedMs : 0}
            progressRemainingMs={selectedSmartShortsBusy
              ? smartShortsProgressRemainingMs
              : null}
            progressOverdue={selectedSmartShortsBusy && smartShortsProgressOverdue}
            cancelling={selectedSmartShortsBusy && smartShortsCancelling}
            message={selectedSmartShortsMessage}
            error={selectedSmartShortsError}
            candidates={selectedSmartShortsReview?.candidates ?? []}
            selectedIds={selectedSmartShortsReview?.selectedIds ?? new Set<string>()}
            fireworksConfigured={fireworksSmartShortsConfigured}
            glmUsed={selectedSmartShortsReview?.glmUsed ?? false}
            engineLabel={selectedSmartShortsReview?.engineLabel ?? null}
            streamerLayoutEnabled={selectedStreamerLayout.enabled}
            streamerFacePosition={selectedStreamerLayout.facePosition}
            streamerFaceFraction={selectedStreamerLayout.faceFraction}
            streamerTargetReady={Boolean(selectedAutoReframeEntry)}
            facecamDetectBusy={selectedFacecamDetectBusy}
            facecamDetectMessage={selectedFacecamDetectMessage}
            facecamDetectError={selectedFacecamDetectMessage ? facecamDetectError : false}
            onAnalyze={handleSmartShortsAnalyze}
            onCancel={handleSmartShortsCancel}
            onCandidateToggle={handleSmartShortsCandidateToggle}
            onCandidateSeek={handleSmartShortsCandidateSeek}
            onApply={handleSmartShortsApply}
            onStreamerLayoutChange={handleStreamerLayoutChange}
            onSelectStreamerTarget={handleSelectStreamerTarget}
            onAutoDetectFacecam={handleAutoDetectFacecam}
          />
          <section class="auto-reframe-card shorts-reframe-card" class:active={Boolean(selectedAutoReframeEntry)}>
            <div class="auto-reframe-heading">
              <div>
                <span>YEREL AI</span>
                <strong>Akıllı Kadraj</strong>
              </div>
              {#if selectedAutoReframeEntry}<i>HAZIR</i>{/if}
            </div>
            <p>Kişiyi veya nesneyi takip edip 9:16, 1:1 ve 16:9 kamera yolları üretir.</p>
            {#if selectedAutoReframeEntry}
              <div class="auto-reframe-aspects">
                {#each selectedAutoReframeEntry.aspects as aspect}<span>{aspect}</span>{/each}
                <small>{selectedAutoReframeEntry.engine}</small>
              </div>
              {#if selectedAutoReframeEntry.warnings.length > 0}
                <div class="auto-reframe-note">⚠ {selectedAutoReframeEntry.warnings.length} kontrol notu</div>
              {/if}
            {:else if autoReframeState.clips[selectedClip.id]}
              <div class="auto-reframe-note">Klip kesildi veya hızı değişti; yeniden analiz gerekli.</div>
            {/if}
            <div class="auto-reframe-actions">
              {#if selectedAutoReframeEntry}
                <button type="button" class="ghost" onclick={removeSelectedAutoReframe} disabled={selectedClipLocked}>Kaldır</button>
              {/if}
              <button
                type="button"
                class="run"
                onclick={() => void openAutoReframeDialog()}
                disabled={selectedClipLocked || autoReframeOpening}
              >
                {autoReframeOpening
                  ? "Kare hazırlanıyor…"
                  : selectedAutoReframeBusy
                    ? "Süren analizi aç"
                    : selectedAutoReframePending
                      ? "Sonucu aç"
                      : selectedAutoReframeEntry
                        ? "Yeniden analiz"
                        : "Hedef seç ve takip et"}
              </button>
            </div>
          </section>
        {:else}
          <div class="shorts-empty-state">
            <span aria-hidden="true">✦</span>
            <strong>Bir video klibi seçin</strong>
            <p>Akıllı Shorts; seçili videoda güçlü anları bulur, dikey düzeni hazırlar ve sonuçları incelemenizi sağlar.</p>
          </div>
        {/if}
      {:else if audioStudioOpen}
        {#if (selectedClip?.kind === "audio" || selectedClip?.kind === "video") && !selectedClip.audioSeparated}
          <div class="audio-studio-heading">
            <span aria-hidden="true">◒</span>
            <div>
              <strong>Ses AI çalışma alanı</strong>
              <small>Dolgu sözcükleri, otomatik ducking ve ritmik kesimleri burada yönetin.</small>
            </div>
          </div>
          <AudioEnhancementPanel
            disabled={selectedClipLocked}
            speechBusy={speechBusyClipId === selectedClip.id}
            speechMessage={selectedSpeechMessage}
            speechError={speechError}
            speechSuggestions={selectedSpeechReview?.suggestions ?? []}
            selectedSuggestionIds={selectedSpeechReview?.selectedIds ?? new Set<string>()}
            onSpeechAnalyze={handleSpeechAnalyze}
            onSpeechSuggestionToggle={handleSpeechSuggestionToggle}
            onSpeechSuggestionSeek={handleSpeechSuggestionSeek}
            onSpeechSuggestionsApply={handleSpeechSuggestionsApply}
            ducking={selectedDuckingSettings()}
            duckingActive={Boolean(selectedClip.autoDucking)}
            duckingDiagnostics={selectedDuckingDiagnostics}
            duckingTracks={selectedDuckingTracks}
            duckingBusy={duckingBusyClipId === selectedClip.id}
            duckingMessage={selectedDuckingMessage}
            duckingError={duckingError}
            onDuckingTrackToggle={handleDuckingTrackToggle}
            onDuckingChange={handleDuckingChange}
            onDuckingApply={handleDuckingApply}
            onDuckingRemove={handleDuckingRemove}
            beatAnalysis={selectedBeatUiAnalysis}
            beatBusy={beatBusyClipId === selectedClip.id}
            beatMessage={selectedBeatMessage}
            beatError={beatError}
            {beatCutMode}
            onBeatAnalyze={handleBeatAnalyze}
            onBeatCutModeChange={(mode: BeatCutMode) => (beatCutMode = mode)}
            onBeatCutsApply={handleBeatCutsApply}
            onBeatRemove={handleBeatRemove}
          />
        {:else}
          <div class="shorts-empty-state">
            <span aria-hidden="true">◒</span>
            <strong>Ses içeren bir klip seçin</strong>
            <p>Ses AI araçları video veya ses klibindeki konuşma, müzik ve ritim sinyalleriyle çalışır.</p>
          </div>
        {/if}
      {:else if selectedClip}
        <Inspector
      transform={selectedClip.transform}
      volume={selectedClip.volume}
      clipKind={selectedClip.kind}
      text={selectedClip.text}
      disabled={selectedClipLocked}
      cameraAutomationActive={Boolean(selectedAutoReframeEntry)}
      audioSeparated={selectedClip.audioSeparated ?? false}
      speed={selectedClip.speed}
      trackMix={selectedTrackMix}
      audioRegion={audioRegionUi}
      audioNormalizeBusy={selectedAudioNormalizeBusy}
      audioNormalizeMessage={selectedAudioNormalizeMessage}
      audioNormalizeError={audioNormalizeError}
      voiceRider={selectedClip.voiceRider ?? null}
      voiceRiderCurrentGain={selectedVoiceRiderGain}
      effectiveVolume={selectedEffectiveVolume}
      noiseReduction={selectedClip.noiseReduction ?? null}
      noiseReductionBusy={selectedNoiseReductionBusy}
      noiseReductionMessage={selectedNoiseReductionMessage}
      noiseReductionError={selectedNoiseReductionError}
      noiseComparisonAvailable={selectedNoiseComparisonAvailable}
      noiseAuditionMode={selectedNoiseAuditionMode}
      mossFormerAvailable={selectedMossFormerAvailable}
      mossFormerBusy={selectedMossFormerBusy}
      mossFormerProgress={selectedMossFormerProgress}
      mossFormerMessage={selectedMossFormerMessage}
      mossFormerError={selectedMossFormerError}
      voiceRiderBusy={selectedVoiceRiderBusy}
      voiceRiderMessage={selectedVoiceRiderMessage}
      voiceRiderError={voiceRiderError}
      onUpdate={handleInspectorUpdate}
      onTextUpdate={handleInspectorTextUpdate}
      onResetPosition={handleResetPosition}
      onResetTransform={handleResetTransform}
      onSpeedChange={handleClipSpeedChange}
      onTrackMixChange={handleTrackMixChange}
      onAddKeyframe={handleAddKeyframe}
      onDelete={handleDeleteSelectedClip}
      onAudioRegionToggle={() => timelineRef?.toggleAudioRegionSelectMode?.()}
      onAudioRegionVolumeChange={(volume: number) =>
        timelineRef?.setAudioRegionVolume?.(volume)}
      onAudioRegionApply={() => timelineRef?.applyAudioRegionVolume?.()}
      onAudioRegionRemove={() => timelineRef?.removeAudioRegion?.()}
      onAudioRegionCancel={() => timelineRef?.cancelAudioRegionEdit?.()}
      onQuietRegionOpen={handleQuietRegionOpen}
      onNormalizeAudio={handleNormalizeSelectedAudio}
      onNoiseReductionChange={handleNoiseReductionChange}
      onNoiseReductionRetry={retryNoiseReductionPreview}
      onNoiseAuditionModeChange={handleNoiseAuditionModeChange}
      onMossFormerPrepare={handleMossFormerPrepare}
      onVoiceRider={handleVoiceRider}
      onVoiceRiderRemove={handleRemoveVoiceRider}
        />
      {:else}
        <div class="inspector-content">
          <p class="placeholder-text">Klip seçili değil</p>
        </div>
      {/if}
    </div>
  </div>
{/snippet}

<AutoReframeDialog
  open={autoReframeOpen}
  clipName={autoReframeClip()?.file.split(/[/\\]/).pop() ?? "Seçili video klibi"}
  seedFrameUrl={autoReframeSeedFrame?.dataUrl ?? ""}
  sourceWidth={autoReframeSeedFrame?.sourceWidth ?? 1}
  sourceHeight={autoReframeSeedFrame?.sourceHeight ?? 1}
  durationMs={Math.max(
    1,
    Math.round((autoReframeClip()?.duration ?? 0) * 1_000),
  )}
  seedTimeMs={autoReframeSeedTimeMs / Math.max(0.01, autoReframeClip()?.speed ?? 1)}
  targetBox={autoReframeTarget}
  analysisStatus={autoReframeStatus}
  analysisProgress={autoReframeProgress}
  analysisMessage={autoReframeMessage}
  analysisError={autoReframeError}
  result={autoReframeResult}
  onTargetBoxChange={handleAutoReframeTargetChange}
  onAnalyze={handleAutoReframeAnalyze}
  onApply={handleAutoReframeApply}
  onCancel={closeAutoReframeDialog}
/>

<ExportDialog
  open={exportOpen}
  durationMs={timelineSnapshot?.clips.length ? timelineSnapshot.durationMs : 0}
  sourcePath={null}
  timeline={timelineSnapshot?.clips.length
    ? renderTimelineWithPreparedHybrid(timelineSnapshot)
    : undefined}
  timelinesByPreset={autoReframeTimelinesByPreset}
  autoReframePresets={autoReframeExportAspects}
  defaultPreset={previewAspect}
  onClose={() => (exportOpen = false)}
/>

<AudioExportDialog
  open={audioExportTarget !== null}
  sourcePath={audioExportTarget?.sourcePath ?? null}
  durationMs={audioExportTarget?.durationMs ?? 0}
  startMs={audioExportTarget?.startMs ?? 0}
  playbackRate={audioExportTarget?.playbackRate ?? 1}
  volume={audioExportTarget?.volume ?? 1}
  volumeKeyframes={audioExportTarget?.volumeKeyframes ?? []}
  riderGainKeyframes={audioExportTarget?.riderGainKeyframes ?? []}
  duckGainKeyframes={audioExportTarget?.duckGainKeyframes ?? []}
  noiseReduction={audioExportTarget?.noiseReduction ?? null}
  onClose={() => (audioExportTarget = null)}
  onCompleted={handleAudioExportCompleted}
/>

<RecoveryDialog
  open={recoveryOpen}
  snapshots={recoverySnapshots}
  busy={projectBusy}
  onRecover={recoverProject}
  onDiscard={discardRecovery}
  onClose={skipRecovery}
/>

<MissingMediaDialog
  open={missingMediaOpen}
  missing={missingReferences}
  result={relinkResult}
  busy={relinkBusy}
  onSearch={searchMissingMedia}
  onApply={applyMediaReplacements}
  onClose={() => (missingMediaOpen = false)}
/>

<ProxyDialog
  open={proxyOpen}
  {mediaFiles}
  onProxyReady={handleProxyReady}
  onCachePruned={() => (proxyPaths = {})}
  onClose={() => (proxyOpen = false)}
/>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    font-family:
      "Inter",
      -apple-system,
      sans-serif;
    background: #0c0c0c;
    color: #d4d4d4;
    overflow: hidden;
  }
  .editor {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: #0c0c0c;
  }
  .topbar {
    height: 48px;
    background: #111;
    border-bottom: 1px solid #1f1f1f;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    padding: 0 16px;
    flex-shrink: 0;
    position: relative;
    z-index: 500;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .app-menu {
    position: relative;
  }
  .logo {
    color: #3d5afe;
  }
  .app-menu-trigger {
    display: grid;
    width: 30px;
    height: 30px;
    padding: 0;
    place-items: center;
    color: #667cff;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 7px;
    cursor: pointer;
  }
  .app-menu-trigger:hover,
  .app-menu-trigger.active {
    color: #8f9fff;
    background: #1b1e28;
    border-color: #30384f;
  }
  .app-menu-popover {
    position: absolute;
    top: calc(100% + 9px);
    left: 0;
    width: 242px;
    padding: 8px;
    color: #d9dddc;
    background: rgba(18, 19, 19, 0.98);
    border: 1px solid #303332;
    border-radius: 10px;
    box-shadow: 0 20px 55px rgba(0, 0, 0, 0.58);
  }
  .menu-label {
    display: block;
    padding: 5px 8px 6px;
    color: #606765;
    font-size: 8px;
    font-weight: 800;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  .app-menu-popover button {
    display: grid;
    grid-template-columns: 22px 1fr auto;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 8px;
    color: #b9bfbd;
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: 6px;
    font-family: inherit;
    font-size: 11px;
    font-weight: 500;
    line-height: 1.2;
    cursor: pointer;
  }
  .app-menu-popover button:hover {
    color: #f2f5f4;
    background: #202322;
  }
  .app-menu-popover button:disabled {
    cursor: wait;
    opacity: 0.45;
  }
  .app-menu-popover button.menu-export {
    color: #72ddb9;
  }
  .menu-icon {
    color: #707876;
    font: 600 15px/1 "JetBrains Mono", monospace;
    text-align: center;
  }
  .app-menu-popover button:hover .menu-icon,
  .menu-export .menu-icon {
    color: #72ddb9;
  }
  .app-menu-popover kbd {
    color: #555c59;
    font: 500 8px/1 "JetBrains Mono", monospace;
  }
  .menu-separator {
    height: 1px;
    margin: 7px 4px;
    background: #292c2b;
  }
  .app-name {
    font-weight: 600;
    color: #fff;
  }
  .project-info {
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .project-title {
    font-size: 13px;
    font-weight: 500;
    color: #ddd;
  }
  .save-status {
    font-size: 10px;
    color: #666;
  }
  .save-status.dirty { color: #c5a769; }
  .save-status.error { color: #df887c; }
  .actions { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .project-cover-chip {
    display: inline-flex;
    align-items: center;
    height: 28px;
    overflow: hidden;
    color: #b8c6c0;
    background: #171b19;
    border: 1px solid #31433b;
    border-radius: 6px;
    font-size: 10px;
  }
  .project-cover-chip img { width: 31px; height: 100%; object-fit: cover; background: #050606; }
  .project-cover-chip span { padding: 0 6px; white-space: nowrap; }
  .project-cover-chip button {
    width: 23px;
    height: 100%;
    padding: 0;
    color: #718078;
    background: transparent;
    border: 0;
    border-left: 1px solid #31433b;
    cursor: pointer;
    font-size: 15px;
    line-height: 1;
  }
  .project-cover-chip button:hover { color: #f1c4bd; background: #382522; }

  .dev-recovery-notice {
    position: fixed;
    top: 58px;
    right: 14px;
    z-index: 1250;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    color: #c9ded7;
    background: rgba(18, 25, 23, 0.94);
    border: 1px solid rgba(98, 215, 177, 0.34);
    border-radius: 8px;
    box-shadow: 0 12px 30px rgba(0, 0, 0, 0.42);
    font-size: 10px;
    pointer-events: none;
  }

  .notice-dot {
    width: 7px;
    height: 7px;
    background: #62d7b1;
    border-radius: 50%;
    box-shadow: 0 0 10px rgba(98, 215, 177, 0.52);
  }

  .auto-reframe-background-toast {
    position: fixed;
    right: 16px;
    z-index: 1240;
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) auto;
    align-items: center;
    gap: 5px 10px;
    width: min(356px, calc(100vw - 32px));
    padding: 10px 12px;
    color: #e2faf6;
    text-align: left;
    font-family: inherit;
    background: rgba(11, 25, 27, 0.97);
    border: 1px solid rgba(74, 220, 196, 0.48);
    border-radius: 11px;
    box-shadow: 0 15px 38px rgba(0, 0, 0, 0.52);
    cursor: pointer;
    transition:
      transform 140ms ease,
      border-color 140ms ease,
      background 140ms ease,
      bottom 180ms ease;
  }

  .auto-reframe-background-toast:hover,
  .auto-reframe-background-toast:focus-visible {
    transform: translateY(-2px);
    background: rgba(14, 36, 37, 0.99);
    border-color: rgba(112, 241, 218, 0.82);
    outline: none;
  }

  .auto-reframe-background-toast.completed {
    color: #e4ffed;
    background: rgba(13, 39, 29, 0.98);
    border-color: rgba(91, 225, 151, 0.58);
  }

  .auto-reframe-background-toast.error {
    color: #ffe2dc;
    background: rgba(48, 21, 20, 0.98);
    border-color: rgba(241, 107, 91, 0.62);
  }

  .auto-reframe-toast-badge {
    grid-row: 1 / span 2;
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    color: #091817;
    background: linear-gradient(145deg, #8ff8e5, #42d8c0);
    border-radius: 9px;
    font: 900 10px/1 "JetBrains Mono", monospace;
    box-shadow: 0 0 18px rgba(66, 216, 192, 0.24);
  }

  .completed .auto-reframe-toast-badge {
    background: linear-gradient(145deg, #a9f5c2, #55d68c);
  }

  .error .auto-reframe-toast-badge {
    color: #fff;
    background: linear-gradient(145deg, #f28b7b, #c94f43);
  }

  .auto-reframe-toast-copy {
    display: grid;
    min-width: 0;
    gap: 2px;
  }

  .auto-reframe-toast-copy strong,
  .auto-reframe-toast-copy small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .auto-reframe-toast-copy strong {
    color: inherit;
    font-size: 11px;
    font-weight: 750;
  }

  .auto-reframe-toast-copy small {
    color: rgba(218, 239, 234, 0.62);
    font-size: 9px;
  }

  .auto-reframe-toast-value {
    color: #7cebd7;
    font: 800 10px/1 "JetBrains Mono", monospace;
  }

  .completed .auto-reframe-toast-value { color: #7fe5a5; }
  .error .auto-reframe-toast-value { color: #ef8d7f; }

  .auto-reframe-toast-rail {
    grid-column: 2 / -1;
    display: block;
    height: 4px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.11);
    border-radius: 999px;
  }

  .auto-reframe-toast-fill {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, #42d8c0, #98f7e7);
    border-radius: inherit;
    box-shadow: 0 0 10px rgba(66, 216, 192, 0.65);
    transition: width 180ms ease;
  }

  .completed .auto-reframe-toast-fill {
    background: linear-gradient(90deg, #55d68c, #a9f5c2);
  }

  .workspace {
    flex: 1;
    display: flex;
    overflow: hidden;
    min-height: 0;
  }

  .sidebar,
  .inspector {
    background: #111;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    flex-shrink: 0;
  }
  .canvas-area {
    flex: 1;
    background: #000;
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
    overflow: hidden;
  }
  .timeline-area {
    flex-shrink: 0;
    background: #161616;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .inspector-content,
  .detached-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: #555;
    font-size: 13px;
  }
  .library-stack {
    display: flex;
    height: 100%;
    min-height: 0;
    flex-direction: column;
    overflow: hidden;
  }
  .library-tabs {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    flex: 0 0 auto;
    padding: 4px 6px 0;
    background: #121413;
    border-bottom: 1px solid #262a28;
  }
  .library-tabs button {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 30px;
    padding: 0 6px;
    color: #747c78;
    background: transparent;
    border: 0;
    font: 700 10px/1 "Inter", sans-serif;
    cursor: pointer;
  }
  .library-tabs button::after {
    content: "";
    position: absolute;
    right: 9px;
    bottom: -1px;
    left: 9px;
    height: 2px;
    background: transparent;
    border-radius: 2px;
  }
  .library-tabs button:hover { color: #c8cfcc; }
  .library-tabs button.active { color: #e4ece8; }
  .library-tabs button.active::after { background: #65d7b2; }
  .library-tabs span { color: #6c7772; font-size: 10px; }
  .library-tabs button.active span { color: #77dfc1; }
  .library-body {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .inspector-stack {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .inspector-mode-tabs {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    flex: 0 0 auto;
    padding: 5px 6px 0;
    background: #121413;
    border-bottom: 1px solid #262a28;
  }
  .inspector-mode-tabs button {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 30px;
    padding: 0 7px;
    color: #737b77;
    background: transparent;
    border: 0;
    font-size: 10px;
    font-weight: 700;
    cursor: pointer;
  }
  .inspector-mode-tabs button::after {
    content: "";
    position: absolute;
    right: 10px;
    bottom: -1px;
    left: 10px;
    height: 2px;
    background: transparent;
    border-radius: 2px;
  }
  .inspector-mode-tabs button.active { color: #e0e5e2; }
  .inspector-mode-tabs button.active::after { background: #65d7b2; }
  .inspector-mode-tabs button > span:first-child { color: #808984; font-size: 12px; }
  .inspector-mode-tabs .cc-label {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 15px;
    padding: 0 3px;
    color: #07120e;
    background: #65d7b2;
    border-radius: 3px;
    font-size: 7px;
    font-weight: 900;
  }
  .inspector-mode-tabs .shorts-label {
    color: #61d9bb;
    font-size: 12px;
  }
  .inspector-mode-tabs .audio-label { color: #83c9ee; font-size: 12px; }
  .inspector-mode-tabs i {
    display: inline-grid;
    place-items: center;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    color: #2b1609;
    background: #f2a45e;
    border-radius: 8px;
    font-size: 8px;
    font-style: normal;
    font-weight: 900;
  }
  .inspector-mode-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .inspector-mode-body.caption-mode { overflow: hidden; }
  .inspector-mode-body.shorts-mode { container-type: inline-size; }
  .inspector-mode-body.audio-mode { container-type: inline-size; }
  .audio-studio-heading {
    display: flex;
    align-items: center;
    gap: 9px;
    margin: 9px 9px 0;
    padding: 11px;
    color: #7fc8ed;
    background: linear-gradient(145deg, rgba(29, 39, 44, .96), rgba(18, 23, 25, .98));
    border: 1px solid rgba(94, 166, 205, .28);
    border-radius: 9px;
  }
  .audio-studio-heading > span { font-size: 19px; }
  .audio-studio-heading > div { display: grid; gap: 2px; }
  .audio-studio-heading strong { color: #e0edf3; font-size: 11px; }
  .audio-studio-heading small { color: #788a92; font-size: 8px; line-height: 1.4; }
  .auto-reframe-card {
    display: grid;
    gap: 9px;
    margin: 8px;
    padding: 12px;
    color: #aeb8b4;
    background: linear-gradient(145deg, rgba(31, 39, 36, .92), rgba(19, 23, 22, .96));
    border: 1px solid #303936;
    border-radius: 9px;
  }
  .auto-reframe-card.active { border-color: rgba(101, 215, 178, .42); box-shadow: inset 0 0 0 1px rgba(101, 215, 178, .06); }
  .auto-reframe-heading, .auto-reframe-actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .auto-reframe-heading div { display: grid; gap: 2px; }
  .auto-reframe-heading span { color: #65d7b2; font-size: 8px; font-weight: 850; letter-spacing: .12em; }
  .auto-reframe-heading strong { color: #e2ebe7; font-size: 12px; }
  .auto-reframe-heading i { padding: 3px 5px; color: #153126; background: #65d7b2; border-radius: 4px; font-size: 7px; font-style: normal; font-weight: 900; }
  .auto-reframe-card p { margin: 0; color: #77817d; font-size: 9px; line-height: 1.45; }
  .auto-reframe-aspects { display: flex; align-items: center; gap: 5px; flex-wrap: wrap; }
  .auto-reframe-aspects span { padding: 3px 5px; color: #bfe9da; background: rgba(101, 215, 178, .09); border: 1px solid rgba(101, 215, 178, .2); border-radius: 4px; font-size: 8px; font-weight: 750; }
  .auto-reframe-aspects small { margin-left: auto; color: #606a66; font-size: 7px; }
  .auto-reframe-note { padding: 6px 7px; color: #c5a66f; background: rgba(183, 133, 53, .08); border-radius: 5px; font-size: 8px; }
  .auto-reframe-actions { justify-content: flex-end; }
  .auto-reframe-actions button { min-height: 29px; padding: 6px 9px; border-radius: 6px; font: 750 8px/1 inherit; cursor: pointer; }
  .auto-reframe-actions button:disabled { cursor: not-allowed; opacity: .45; }
  .auto-reframe-actions .ghost { color: #87908c; background: transparent; border: 1px solid #343b38; }
  .auto-reframe-actions .run { color: #092017; background: #65d7b2; border: 1px solid #78e1bd; }
  .shorts-reframe-card { margin-top: 0; }
  .shorts-empty-state {
    display: grid;
    justify-items: center;
    gap: 7px;
    margin: 12px;
    padding: 28px 18px;
    color: #77837e;
    text-align: center;
    background: linear-gradient(145deg, rgba(27, 34, 31, .94), rgba(17, 21, 20, .98));
    border: 1px dashed #34403b;
    border-radius: 10px;
  }
  .shorts-empty-state > span { color: #61d9bb; font-size: 24px; }
  .shorts-empty-state strong { color: #d9e3df; font-size: 11px; }
  .shorts-empty-state p { max-width: 280px; margin: 0; font-size: 9px; line-height: 1.55; }
  .detached-placeholder {
    flex-direction: column;
    gap: 10px;
  }

  .restore-btn,
  .btn-secondary {
    padding: 6px 12px;
    background: #222;
    border: 1px solid #333;
    color: #aaa;
    border-radius: 4px;
    cursor: pointer;
  }
  .restore-btn:hover,
  .btn-secondary:hover {
    background: #333;
    color: #fff;
  }
  .btn-secondary:disabled { cursor: not-allowed; opacity: 0.45; }
  .btn-secondary.warning { color: #d5ae70; border-color: rgba(213,174,112,.38); font-size: 10px; }
  .caption-qa-button { display: inline-flex; align-items: center; gap: 6px; padding: 5px 8px; color: #b8e8d7; border-color: rgba(101, 215, 178, .34); font-size: 10px; }
  .caption-qa-button span { display: inline-grid; place-items: center; height: 17px; padding: 0 4px; color: #07120e; background: #65d7b2; border-radius: 3px; font-size: 8px; font-weight: 900; }
  .caption-qa-button i { min-width: 14px; height: 14px; display: inline-grid; place-items: center; color: #281408; background: #ffad66; border-radius: 8px; font-size: 8px; font-style: normal; font-weight: 800; }

  /* Scrollbars */
  :global(::-webkit-scrollbar) {
    width: 6px;
    height: 6px;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: #333;
    border-radius: 3px;
  }
  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }
</style>
