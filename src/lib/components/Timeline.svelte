<script lang="ts">
  import { onMount, tick } from "svelte";
  import { isTauri, convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import ContextMenu, {
    type ContextMenuItem,
  } from "./ContextMenu.svelte";
  import FilmstripThumbnail from "./FilmstripThumbnail.svelte";
  import {
    draggedMediaFile,
    endMediaDrag,
    pointerDragState,
    markPointerDragMoved,
    endPointerDrag,
    setMediaDropFeedback,
  } from "$lib/stores/drag";
  import { activeTool, setActiveTool } from "$lib/stores/tools";
  import {
    buildFramePlan,
    createClip,
    createTextClip,
    createTrack,
    decibelsToGain,
    deleteClip,
    deleteTrack,
    evaluateKeyframes,
    getClipVolumeRegions,
    getVerticalVolumeDragValue,
    gainToDecibels,
    moveClip,
    normalizeProjectState,
    serializeProjectState,
    setClipSpeed,
    setClipVolumeRegion,
    setTrackState,
    setVoiceRiderEnvelope,
    removeVoiceRiderEnvelope,
    splitClip as splitTimelineClip,
    TIMELINE_EPSILON,
    trimClip,
    upsertKeyframe,
    type AnimatableProperty,
    type ClipVolumeRegion,
    type TimelineClip as Clip,
    type TimelineKeyframe,
    type TimelineProjectSnapshot,
    type TimelineProjectState,
    type TimelineTrack as Track,
    type VoiceRiderMetadata,
  } from "$lib/editor/timeline-engine";
  import { decodeWaveform } from "$lib/editor/media-runtime";
  import {
    isBeatAnalysisRangeCurrent,
    projectSourceBeats,
    type SpeechSuggestion,
  } from "$lib/editor/audio-ai";
  import {
    deleteClipRanges,
    splitVisualsAtTimes,
    type TimelineLocalRange,
  } from "$lib/editor/timeline-batch-edits";
  import {
    applyTransportSeek,
    resolveTransportShortcut,
  } from "$lib/editor/transport-shortcuts";
  import {
    clampTransitionDuration,
    getTransitionPreset,
    updateTransitionSide,
    type TransitionSide,
  } from "$lib/editor/transition-presets";

  type AutoReframeJobStatus = "preparing" | "analyzing" | "completed" | "error";

  interface TimelineAutoReframeJob {
    clipId: string;
    status: AutoReframeJobStatus;
    progress: number;
    message: string;
  }

  interface Props {
    currentTime: number;
    duration: number;
    onSelect?: (clip: Clip | null, options?: { autoplay?: boolean }) => void;
    onClipTimeChange?: (clip: Clip | null, localTime: number) => void;
    onDurationChange?: (duration: number) => void;
    onProjectStateChange?: (state: TimelineProjectSnapshot) => void;
    onScrubBegin?: () => void;
    onAudioExport?: (clip: Clip) => void;
    onAudioNormalize?: (clip: Clip) => void;
    audioNormalizeBusy?: boolean;
    onVoiceRider?: (clip: Clip) => void;
    onVoiceRiderRemove?: (clip: Clip) => void;
    voiceRiderBusy?: boolean;
    speechSuggestions?: readonly SpeechSuggestion[];
    /**
     * Mirrors the audio-region editing state (draft selection, select mode,
     * committed quiet regions) so the Inspector can host the region editor
     * while the waveform interaction stays on the timeline.
     */
    onAudioRegionChange?: (state: AudioRegionPanelState | null) => void;
    /** Fired after "+ Metin" creates a clip so the host can focus its editor. */
    onTextClipCreated?: () => void;
    /** Highlights the transition side currently open in the transition library. */
    transitionEditorSide?: TransitionSide | null;
    /** Opens the transition library for the clicked timeline envelope. */
    onTransitionSelect?: (side: TransitionSide) => void;
    /**
     * Captures app state that lives outside the timeline (e.g. the media
     * pool) so undo/redo restores it together with the timeline.
     */
    getExternalSnapshot?: () => unknown;
    applyExternalSnapshot?: (snapshot: unknown) => void;
    /** Background AI framing state rendered directly on its source clip. */
    autoReframeJob?: TimelineAutoReframeJob | null;
    /** Reopens the running/completed AI framing job. */
    onAutoReframeJobOpen?: (clipId: string) => void;
  }

  let {
    currentTime = $bindable(0),
    duration = 0,
    onSelect,
    onClipTimeChange,
    onDurationChange,
    onProjectStateChange,
    onScrubBegin,
    onAudioExport,
    onAudioNormalize,
    audioNormalizeBusy = false,
    onVoiceRider,
    onVoiceRiderRemove,
    voiceRiderBusy = false,
    speechSuggestions = [],
    onAudioRegionChange,
    onTextClipCreated,
    transitionEditorSide = null,
    onTransitionSelect,
    getExternalSnapshot,
    applyExternalSnapshot,
    autoReframeJob = null,
    onAutoReframeJobOpen,
  }: Props = $props();

  let timelineRef = $state<HTMLDivElement>();
  let trackHeadersRef = $state<HTMLDivElement>();
  let zoom = $state(1);
  let toolHelpOpen = $state(false);
  let toolHelpTriggerRef = $state<HTMLButtonElement>();
  let toolHelpPos = $state({ left: 0, bottom: 0 });

  function toggleToolHelp() {
    if (!toolHelpOpen && toolHelpTriggerRef) {
      const rect = toolHelpTriggerRef.getBoundingClientRect();
      toolHelpPos = {
        left: rect.left,
        bottom: window.innerHeight - rect.top + 6,
      };
    }
    toolHelpOpen = !toolHelpOpen;
  }

  function closeToolHelp() {
    toolHelpOpen = false;
  }

  let tracks = $state<Track[]>([
    createTrack("v1", "V1", "video"),
    createTrack("a1", "A1", "audio"),
    createTrack("t1", "T1", "text"),
  ]);

  let clips = $state<Clip[]>([]);
  let selectedClipId = $state<string | null>(null);
  interface SelectedTransition {
    clipId: string;
    side: TransitionSide;
  }

  let selectedTransition = $state<SelectedTransition | null>(null);

  function selectClip(clipId: string | null) {
    selectedClipId = clipId;
    selectedTransition = null;
  }

  let selectedTimelineClip = $derived(
    clips.find((clip) => clip.id === selectedClipId) ?? null,
  );
  let selectedTimelineTrack = $derived(
    selectedTimelineClip
      ? tracks.find((track) => track.id === selectedTimelineClip.trackId) ?? null
      : null,
  );
  let clipDragId = $state<string | null>(null);
  let clipDragPointerId = $state<number | null>(null);
  let clipDragStartX = $state(0);
  let clipDragStartY = $state(0);
  let clipDragStartTime = $state(0);
  let clipDragHasMoved = $state(false);
  let clipDragCaptureElement: HTMLElement | null = null;
  let clipMovePreview = $state<ClipMovePreview | null>(null);
  let rippleMode = $state(false);
  let clipClipboard = $state<Clip | null>(null);
  let timelineContextMenu = $state<TimelineContextMenuState>({
    open: false,
    x: 0,
    y: 0,
    clipId: null,
    transitionSide: null,
    trackId: null,
    time: 0,
  });

  interface AudioRegionSelection {
    clipId: string;
    start: number;
    end: number;
    volume: number;
  }

  interface AudioRegionDrag {
    clipId: string;
    pointerId: number;
    anchorTime: number;
    element: HTMLElement;
  }

  interface AudioRegionResizeDrag {
    clipId: string;
    edge: "start" | "end";
    pointerId: number;
    stripElement: HTMLElement;
  }

  interface AudioRegionGainDrag {
    clipId: string;
    pointerId: number;
    startClientY: number;
    startVolume: number;
    element: HTMLElement;
    changed: boolean;
  }

  interface ClipVolumeDrag {
    clipId: string;
    pointerId: number;
    startClientY: number;
    startVolume: number;
    volume: number;
    element: HTMLElement;
    changed: boolean;
  }

  interface AudioRegionPanelState {
    selection: { start: number; end: number; volume: number } | null;
    selectMode: boolean;
    quietRegions: QuietRegion[];
  }

  type QuietRegion = ClipVolumeRegion;

  let audioRegionSelection = $state<AudioRegionSelection | null>(null);
  let audioRegionDrag = $state<AudioRegionDrag | null>(null);
  let audioRegionResizeDrag = $state<AudioRegionResizeDrag | null>(null);
  let audioRegionGainDrag = $state<AudioRegionGainDrag | null>(null);
  let clipVolumeDrag = $state<ClipVolumeDrag | null>(null);
  let audioRegionModeClipId = $state<string | null>(null);
  let trimDrag = $state<{
    clipId: string;
    edge: "start" | "end";
    pointerId: number;
    lastClientX: number;
    snapshot: EditorSnapshot;
    changed: boolean;
  } | null>(null);
  let transitionDurationDrag = $state<{
    clipId: string;
    side: TransitionSide;
    pointerId: number;
    clipElement: HTMLElement;
    handleElement: HTMLElement;
    startDuration: number;
    duration: number;
    changed: boolean;
  } | null>(null);

  type DragSource = "none" | "internal" | "os";

  interface DragPreview {
    trackId: string | null;
    file: string | null;
    start: number;
    isAllowed: boolean;
  }

  interface ClipMovePreview {
    clipId: string;
    trackId: string;
    requestedStart: number;
    start: number;
    duration: number;
    mode: "move" | "insert";
    ripple: boolean;
    rippleClipStarts: Record<string, number>;
    isAllowed: boolean;
    message: string;
  }

  interface TimelineContextMenuState {
    open: boolean;
    x: number;
    y: number;
    clipId: string | null;
    transitionSide: TransitionSide | null;
    trackId: string | null;
    time: number;
  }

  const TRACK_HEIGHT_PX = 64;
  const RULER_HEIGHT_PX = 24;
  const DEFAULT_CLIP_DURATION = 10;
  const POINTER_DRAG_THRESHOLD = 6;
  const MIN_TIMELINE_DURATION = 120;
  const MAX_HISTORY = 50;
  const CLIP_OVERLAP_EPS = TIMELINE_EPSILON;
  const CLIP_SNAP_PX = 10;
  const CLIP_GAP = 0; // Minimum gap between clips in seconds (0 = gapless)
  const MIN_CLIP_DURATION = 0.1;
  const MIN_AUDIO_REGION_DURATION = 0.05;
  const FRAME_SAMPLE_WIDTH = 32;
  const FRAME_SAMPLE_HEIGHT = 18;
  const SEEK_TIMEOUT_MS = 600;
  const TRAILING_BLACK_SCAN_MAX_SECONDS = 2;
  const TRAILING_BLACK_SCAN_STEP_SECONDS = 0.25;
  const TRAILING_BLACK_PAD_SECONDS = 0.02;
  const TRAILING_BLACK_END_EPS = 0.05;
  const TRAILING_BLACK_LUMA_THRESHOLD = 8;
  const TRAILING_BLACK_BRIGHT_RATIO = 0.02;
  const AUDIO_FILE_RE = /\.(mp3|wav|m4a|aac|ogg|flac|opus|aiff?|wma)$/i;
  const IMAGE_FILE_RE = /\.(png|jpe?g|webp|gif|bmp|avif|heic|heif|svg)$/i;
  const SUPPORTED_MEDIA_RE =
    /\.(mp4|m4v|mov|avi|mkv|webm|mxf|mts|m2ts|ts|wmv|flv|ogv|mp3|wav|m4a|aac|ogg|flac|opus|aiff?|wma|png|jpe?g|webp|gif|bmp|avif|heic|heif|svg)$/i;
  let timelineContentEnd = $derived(
    clips.reduce((max, clip) => Math.max(max, clip.start + clip.duration), 0),
  );
  // Keep a roomy editing ruler without pretending the project contains media
  // all the way to that visual endpoint.
  let viewDuration = $derived(
    Math.max(MIN_TIMELINE_DURATION, duration, timelineContentEnd),
  );

  function inferClipKind(file: string, track: Track): Clip["kind"] {
    if (track.type === "audio") return "audio";
    if (IMAGE_FILE_RE.test(file)) return "image";
    return "video";
  }

  function canPlaceFileOnTrack(file: string, track: Track): boolean {
    if (track.locked || track.type === "text") return false;
    if (!SUPPORTED_MEDIA_RE.test(file)) return false;
    return track.type === "audio" ? AUDIO_FILE_RE.test(file) : !AUDIO_FILE_RE.test(file);
  }

  function getDropRejectionMessage(file: string, track: Track): string {
    if (track.locked) return `${track.name} kanalı kilitli.`;
    if (track.type === "text") return "Medya dosyaları metin kanalına bırakılamaz.";
    if (AUDIO_FILE_RE.test(file)) return "Ses dosyasını ses kanalına bırakın.";
    return "Video veya görseli video kanalına bırakın.";
  }

  function canPlaceClipOnTrack(clip: Clip, track: Track): boolean {
    if (track.locked) return false;
    if (clip.kind === "text") return track.type === "text";
    if (clip.kind === "audio") return track.type === "audio";
    return track.type === "video";
  }

  async function hydrateWaveform(clipId: string, file: string) {
    if (IMAGE_FILE_RE.test(file) || typeof AudioContext === "undefined") return;
    const context = new AudioContext();
    try {
      const response = await fetch(convertFileSrc(file));
      if (!response.ok) return;
      const waveform = await decodeWaveform(context, await response.arrayBuffer(), 160);
      if (clips.some((clip) => clip.id === clipId)) setClipWaveform(clipId, waveform);
    } catch {
      // Unsupported codecs keep an empty waveform; proxy generation can hydrate later.
    } finally {
      await context.close().catch(() => undefined);
    }
  }

  // Helper function to get video duration from file
  function resolveVideoDuration(video: HTMLVideoElement): number {
    let resolved = Number.isFinite(video.duration) ? video.duration : 0;

    if (video.seekable && video.seekable.length > 0) {
      const seekEnd = video.seekable.end(video.seekable.length - 1);
      if (Number.isFinite(seekEnd) && seekEnd > 0) {
        resolved = resolved > 0 ? Math.min(resolved, seekEnd) : seekEnd;
      }
    }

    if (!Number.isFinite(resolved) || resolved <= 0) return DEFAULT_CLIP_DURATION;
    return Math.max(MIN_CLIP_DURATION, resolved);
  }

  function isFrameMostlyBlack(data: Uint8ClampedArray): boolean {
    let brightCount = 0;
    let sampleCount = 0;

    for (let i = 0; i < data.length; i += 16) {
      const r = data[i] ?? 0;
      const g = data[i + 1] ?? 0;
      const b = data[i + 2] ?? 0;
      const luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      if (luma > TRAILING_BLACK_LUMA_THRESHOLD) brightCount += 1;
      sampleCount += 1;
    }

    if (sampleCount === 0) return false;
    return brightCount / sampleCount < TRAILING_BLACK_BRIGHT_RATIO;
  }

  async function seekVideo(
    video: HTMLVideoElement,
    time: number,
  ): Promise<boolean> {
    return new Promise((resolve) => {
      let settled = false;
      const finish = (ok: boolean) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeoutId);
        video.removeEventListener("seeked", onSeeked);
        video.removeEventListener("error", onError);
        resolve(ok);
      };

      const onSeeked = () => finish(true);
      const onError = () => finish(false);
      const timeoutId = window.setTimeout(() => finish(false), SEEK_TIMEOUT_MS);

      video.addEventListener("seeked", onSeeked);
      video.addEventListener("error", onError);

      const duration = Number.isFinite(video.duration) ? video.duration : time;
      const targetTime = Math.max(0, Math.min(time, duration));
      try {
        video.currentTime = targetTime;
      } catch {
        finish(false);
      }
    });
  }

  async function isFrameMostlyBlackAt(
    video: HTMLVideoElement,
    ctx: CanvasRenderingContext2D,
    time: number,
  ): Promise<boolean | null> {
    const ok = await seekVideo(video, time);
    if (!ok) return null;

    try {
      ctx.drawImage(video, 0, 0, FRAME_SAMPLE_WIDTH, FRAME_SAMPLE_HEIGHT);
      const data = ctx.getImageData(
        0,
        0,
        FRAME_SAMPLE_WIDTH,
        FRAME_SAMPLE_HEIGHT,
      ).data;
      return isFrameMostlyBlack(data);
    } catch {
      return null;
    }
  }

  async function trimTrailingBlack(
    video: HTMLVideoElement,
    baseDuration: number,
  ): Promise<number> {
    const scanWindow = Math.min(
      TRAILING_BLACK_SCAN_MAX_SECONDS,
      baseDuration - MIN_CLIP_DURATION,
    );
    if (scanWindow <= 0) return baseDuration;

    const endTime = Math.max(0, baseDuration - TRAILING_BLACK_END_EPS);
    if (endTime <= 0) return baseDuration;

    const canvas = document.createElement("canvas");
    canvas.width = FRAME_SAMPLE_WIDTH;
    canvas.height = FRAME_SAMPLE_HEIGHT;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return baseDuration;

    const endIsBlack = await isFrameMostlyBlackAt(video, ctx, endTime);
    if (endIsBlack === null || !endIsBlack) return baseDuration;

    const scanStart = Math.max(0, endTime - scanWindow);
    let lastNonBlack: number | null = null;

    for (
      let t = endTime - TRAILING_BLACK_SCAN_STEP_SECONDS;
      t >= scanStart;
      t -= TRAILING_BLACK_SCAN_STEP_SECONDS
    ) {
      const isBlack = await isFrameMostlyBlackAt(video, ctx, t);
      if (isBlack === null) return baseDuration;
      if (!isBlack) {
        lastNonBlack = t;
        break;
      }
    }

    if (lastNonBlack === null) return baseDuration;

    let low = lastNonBlack;
    let high = Math.min(
      endTime,
      lastNonBlack + TRAILING_BLACK_SCAN_STEP_SECONDS,
    );

    for (let i = 0; i < 4; i += 1) {
      const mid = (low + high) / 2;
      const isBlack = await isFrameMostlyBlackAt(video, ctx, mid);
      if (isBlack === null) return baseDuration;
      if (isBlack) high = mid;
      else low = mid;
    }

    return Math.max(
      MIN_CLIP_DURATION,
      Math.min(baseDuration, low + TRAILING_BLACK_PAD_SECONDS),
    );
  }

  async function getVideoDuration(filePath: string): Promise<number> {
    return new Promise((resolve) => {
      const video = document.createElement("video");
      video.preload = "metadata";
      video.muted = true;
      video.crossOrigin = "anonymous";
      let settled = false;

      const finalize = (value: number) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeoutId);
        video.src = "";
        video.load();
        resolve(value);
      };

      const timeoutId = window.setTimeout(() => {
        finalize(resolveVideoDuration(video));
      }, 5000);

      video.onloadedmetadata = () => {
        clearTimeout(timeoutId);
        finalize(resolveVideoDuration(video));
      };

      video.onerror = () => {
        finalize(DEFAULT_CLIP_DURATION);
      };

      video.src = convertFileSrc(filePath);
    });
  }

  let dragSource = $state<DragSource>("none");
  let dragPreview = $state<DragPreview | null>(null);
  let isDragActive = $state(false);
  let osDragPaths = $state<string[] | null>(null);
  let dragDepth = $state(0);
  let clearDragTimer: number | null = null;
  let isScrubbing = $state(false);
  let scrubPointerId = $state<number | null>(null);
  let scrubPointerOffsetX = 0;
  let scrubFrameId: number | null = null;
  let pendingScrubClientX: number | null = null;
  let lastDurationHint = $state(0);

  interface EditorSnapshot {
    tracks: Track[];
    clips: Clip[];
    selectedClipId: string | null;
    selectedTransition: SelectedTransition | null;
    external?: unknown;
  }

  // Raw state keeps the history buttons reactive without proxy-wrapping the
  // external snapshots stored inside each undo point.
  let undoStack = $state.raw<EditorSnapshot[]>([]);
  let redoStack = $state.raw<EditorSnapshot[]>([]);
  let dragSnapshot: EditorSnapshot | null = null;
  let coalescedHistoryKey: string | null = null;
  let coalescedHistoryTimer: number | null = null;

  let canDeleteSelectedClip = $derived(
    selectedTimelineClip !== null &&
      selectedTimelineTrack !== null &&
      !selectedTimelineTrack.locked,
  );
  let canSplitSelectedClip = $derived(
    canDeleteSelectedClip &&
      selectedTimelineClip !== null &&
      currentTime > selectedTimelineClip.start + 0.1 &&
      currentTime <
        selectedTimelineClip.start + selectedTimelineClip.duration - 0.1,
  );
  let hasActiveTimelineInteraction = $derived(
    Boolean(
      trimDrag ||
        transitionDurationDrag ||
        clipDragId ||
        isScrubbing ||
        dragPreview ||
        isDragActive ||
        osDragPaths ||
        audioRegionSelection ||
        audioRegionModeClipId ||
        audioRegionDrag ||
        audioRegionResizeDrag ||
        audioRegionGainDrag ||
        clipVolumeDrag ||
        timelineContextMenu.open ||
        toolHelpOpen ||
        $pointerDragState.active ||
        $draggedMediaFile ||
        $activeTool !== "select",
    ),
  );
  let canCancelTimelineAction = $derived(
    hasActiveTimelineInteraction || selectedClipId !== null,
  );

  function clamp(value: number, min: number, max: number): number {
    return Math.min(max, Math.max(min, value));
  }

  const MIN_EDITABLE_GAIN_DB = -60;
  const MAX_EDITABLE_GAIN_DB = gainToDecibels(4);

  /** The persisted value is linear; this is the editor-facing relative gain label. */
  function formatGainDb(gain: number): string {
    const decibels = gainToDecibels(gain);
    if (!Number.isFinite(decibels)) return "Sessiz";
    return `${decibels > 0 ? "+" : ""}${decibels.toFixed(1)} dB`;
  }

  function formatGainReadout(gain: number): string {
    return `${formatGainDb(gain)} · %${Math.round(Math.max(0, gain) * 100)}`;
  }

  function getGainDbSliderValue(gain: number): number {
    const decibels = gainToDecibels(gain);
    return Number.isFinite(decibels)
      ? clamp(decibels, MIN_EDITABLE_GAIN_DB, MAX_EDITABLE_GAIN_DB)
      : MIN_EDITABLE_GAIN_DB;
  }

  function getDisplayedClipVolume(clip: Clip): number {
    return clipVolumeDrag?.clipId === clip.id ? clipVolumeDrag.volume : clip.volume;
  }

  function formatRelativeGainDb(gain: number, reference: number): string {
    return formatGainDb(reference > 0 ? gain / reference : 0);
  }

  function clampClipStart(start: number, clipDuration: number): number {
    if (!Number.isFinite(viewDuration) || viewDuration <= 0) return 0;
    const maxStart = Math.max(0, viewDuration - clipDuration);
    return clamp(start, 0, maxStart);
  }

  function getSnapThresholdSeconds(): number {
    if (!timelineRef || viewDuration <= 0) return 0;
    const pixelsPerSecond = timelineRef.clientWidth / viewDuration;
    if (!Number.isFinite(pixelsPerSecond) || pixelsPerSecond <= 0) return 0;
    return CLIP_SNAP_PX / pixelsPerSecond;
  }

  function cloneClips(source: Clip[]): Clip[] {
    return source.map((clip) => createClip(clip));
  }

  function cloneTracks(source: Track[]): Track[] {
    return source.map((track) => createTrack(track.id, track.name, track.type, track));
  }

  function createProjectState(): TimelineProjectState {
    return normalizeProjectState({
      version: 2,
      tracks,
      clips,
      selectedClipId,
    });
  }

  function applyProjectState(state: TimelineProjectState) {
    tracks = state.tracks;
    clips = state.clips;
    selectedClipId = state.selectedClipId;
    if (
      selectedTransition &&
      (!clips.some((clip) => clip.id === selectedTransition?.clipId) ||
        clips.find((clip) => clip.id === selectedTransition?.clipId)?.transition[
          selectedTransition.side
        ].type === "none")
    ) {
      selectedTransition = null;
    }
  }

  export function getProjectState(): TimelineProjectSnapshot {
    return serializeProjectState(createProjectState());
  }

  export function loadProjectState(input: TimelineProjectState | TimelineProjectSnapshot) {
    const source = input as unknown as {
      version: number;
      tracks: Track[];
      selectedClipId: string | null;
      clips: Array<
        Partial<Clip> & {
          id: string;
          trackId: string;
          start?: number;
          duration?: number;
          trimIn?: number;
          path?: string;
          startMs?: number;
          durationMs?: number;
          trimInMs?: number;
        }
      >;
    };
    const hydrated = {
      ...source,
      clips: source.clips.map((clip) => ({
        ...clip,
        file: clip.file ?? clip.path ?? "",
        start: clip.start ?? (clip.startMs ?? 0) / 1000,
        duration: clip.duration ?? (clip.durationMs ?? 1000) / 1000,
        trimIn: clip.trimIn ?? (clip.trimInMs ?? 0) / 1000,
      })),
    };
    applyProjectState(normalizeProjectState(hydrated as TimelineProjectState));
    for (const clip of clips) {
      if (
        clip.file &&
        clip.waveform.length === 0 &&
        !clip.audioSeparated &&
        (clip.kind === "video" || clip.kind === "audio")
      ) {
        void hydrateWaveform(clip.id, clip.file);
      }
    }
    clearCoalescedHistory();
    undoStack = [];
    redoStack = [];
  }

  export function getFramePlan(time = currentTime) {
    return buildFramePlan(createProjectState(), time);
  }

  function createSnapshot(): EditorSnapshot {
    return {
      tracks: cloneTracks(tracks),
      clips: cloneClips(clips),
      selectedClipId,
      selectedTransition: selectedTransition
        ? { ...selectedTransition }
        : null,
      external: getExternalSnapshot?.(),
    };
  }

  function pushHistory(snapshot: EditorSnapshot = createSnapshot()) {
    clearCoalescedHistory();
    undoStack = [...undoStack, snapshot];
    if (undoStack.length > MAX_HISTORY) {
      undoStack = undoStack.slice(-MAX_HISTORY);
    }
    redoStack = [];
  }

  function clearCoalescedHistory() {
    if (coalescedHistoryTimer !== null) {
      window.clearTimeout(coalescedHistoryTimer);
      coalescedHistoryTimer = null;
    }
    coalescedHistoryKey = null;
  }

  function prepareCoalescedHistory(key: string) {
    if (coalescedHistoryKey !== key) {
      pushHistory();
      coalescedHistoryKey = key;
    }
    if (coalescedHistoryTimer !== null) {
      window.clearTimeout(coalescedHistoryTimer);
    }
    coalescedHistoryTimer = window.setTimeout(() => {
      coalescedHistoryKey = null;
      coalescedHistoryTimer = null;
    }, 650);
  }

  function prepareClipUpdateHistory(
    clipId: string,
    updates: Partial<Clip> | Partial<Clip["transform"]>,
    mode: "none" | "immediate" | "coalesce",
  ) {
    if (mode === "none") return;
    if (mode === "immediate") {
      pushHistory();
      return;
    }
    prepareCoalescedHistory(`${clipId}:${Object.keys(updates).sort().join(",")}`);
  }

  function applySnapshot(snapshot: EditorSnapshot) {
    tracks = cloneTracks(snapshot.tracks);
    clips = cloneClips(snapshot.clips);
    selectedClipId = snapshot.selectedClipId;
    selectedTransition = snapshot.selectedTransition
      ? { ...snapshot.selectedTransition }
      : null;
    if (snapshot.external !== undefined) {
      applyExternalSnapshot?.(snapshot.external);
    }
  }

  /**
   * Records an undo point for edits initiated outside the timeline (media
   * pool changes). Call it right before mutating the external state.
   */
  export function commitHistory() {
    pushHistory();
  }

  function undo() {
    if (undoStack.length === 0) return;
    clearCoalescedHistory();
    const snapshot = undoStack[undoStack.length - 1];
    undoStack = undoStack.slice(0, -1);
    redoStack = [...redoStack, createSnapshot()];
    applySnapshot(snapshot);
  }

  function redo() {
    if (redoStack.length === 0) return;
    clearCoalescedHistory();
    const snapshot = redoStack[redoStack.length - 1];
    redoStack = redoStack.slice(0, -1);
    undoStack = [...undoStack, createSnapshot()];
    applySnapshot(snapshot);
  }

  function resolveNonOverlappingStart(
    trackId: string,
    clipId: string,
    desiredStart: number,
    clipDuration: number,
  ): number {
    const trackClips = clips
      .filter((clip) => clip.trackId === trackId && clip.id !== clipId)
      .sort((a, b) => a.start - b.start);

    if (trackClips.length === 0) return Math.max(0, desiredStart);

    const clampedDesired = Math.max(
      0,
      clampClipStart(desiredStart, clipDuration),
    );
    const clipEnd = clampedDesired + clipDuration;

    // Check if there's an overlap with any clip
    const findOverlappingClip = (start: number, dur: number) => {
      const end = start + dur;
      return trackClips.find(
        (clip) =>
          end > clip.start + CLIP_OVERLAP_EPS &&
          start < clip.start + clip.duration - CLIP_OVERLAP_EPS,
      );
    };

    const overlappingClip = findOverlappingClip(clampedDesired, clipDuration);

    // No overlap, return desired position
    if (!overlappingClip) return clampedDesired;

    // Calculate both possible positions: before or after the overlapping clip
    const positionBefore = overlappingClip.start - clipDuration - CLIP_GAP;
    const positionAfter =
      overlappingClip.start + overlappingClip.duration + CLIP_GAP;

    // Check which position is closer to the desired position
    const distanceBefore = Math.abs(clampedDesired - positionBefore);
    const distanceAfter = Math.abs(clampedDesired - positionAfter);

    // Try both positions and pick the valid one that's closest
    const tryPosition = (pos: number): number | null => {
      const clampedPos = Math.max(0, clampClipStart(pos, clipDuration));
      if (clampedPos < 0) return null;

      // Check if this position has any overlap
      const overlap = findOverlappingClip(clampedPos, clipDuration);
      if (!overlap) return clampedPos;

      return null;
    };

    // First try the closer position
    if (distanceBefore <= distanceAfter) {
      const beforeResult = tryPosition(positionBefore);
      if (beforeResult !== null) return beforeResult;

      const afterResult = tryPosition(positionAfter);
      if (afterResult !== null) return afterResult;
    } else {
      const afterResult = tryPosition(positionAfter);
      if (afterResult !== null) return afterResult;

      const beforeResult = tryPosition(positionBefore);
      if (beforeResult !== null) return beforeResult;
    }

    // If both positions are taken, find the first available gap
    // Build a list of all gaps on the timeline
    const gaps: Array<{ start: number; end: number }> = [];

    // Gap at the beginning (before first clip)
    if (trackClips[0].start > CLIP_GAP) {
      gaps.push({ start: 0, end: trackClips[0].start - CLIP_GAP });
    }

    // Gaps between clips
    for (let i = 0; i < trackClips.length - 1; i++) {
      const gapStart = trackClips[i].start + trackClips[i].duration + CLIP_GAP;
      const gapEnd = trackClips[i + 1].start - CLIP_GAP;
      if (gapEnd > gapStart) {
        gaps.push({ start: gapStart, end: gapEnd });
      }
    }

    // Gap at the end (after last clip)
    const lastClip = trackClips[trackClips.length - 1];
    gaps.push({
      start: lastClip.start + lastClip.duration + CLIP_GAP,
      end: Infinity,
    });

    // Find the gap closest to the desired position that can fit the clip
    let bestGapStart: number | null = null;
    let bestDistance = Infinity;

    for (const gap of gaps) {
      if (gap.end - gap.start >= clipDuration) {
        // Clip can fit in this gap
        const gapMidpoint = (gap.start + gap.end) / 2;
        const distance = Math.abs(clampedDesired - gapMidpoint);

        if (distance < bestDistance) {
          bestDistance = distance;
          // Position clip at the start of the gap or as close to desired as possible
          bestGapStart = Math.max(
            gap.start,
            Math.min(clampedDesired, gap.end - clipDuration),
          );
        }
      }
    }

    return bestGapStart !== null ? bestGapStart : clampedDesired;
  }

  function isPointInsideTimeline(clientX: number, clientY: number): boolean {
    if (!timelineRef) return false;
    const rect = timelineRef.getBoundingClientRect();
    return (
      clientX >= rect.left &&
      clientX <= rect.right &&
      clientY >= rect.top &&
      clientY <= rect.bottom
    );
  }

  function getDropInfoFromPoint(
    clientX: number,
    clientY: number,
  ): { trackId: string | null; start: number } | null {
    if (!timelineRef) return null;

    const rect = timelineRef.getBoundingClientRect();
    const x = clamp(clientX - rect.left, 0, rect.width);
    const y = clamp(clientY - rect.top, 0, Math.max(0, rect.height - 1));

    const percentage = rect.width > 0 ? x / rect.width : 0;
    const rawStart = percentage * viewDuration;
    const snappedStart = snapTime(rawStart, [], DEFAULT_CLIP_DURATION);
    const start = clampClipStart(snappedStart, DEFAULT_CLIP_DURATION);

    if (tracks.length === 0) return null;

    const rawIndex = Math.floor(y / TRACK_HEIGHT_PX);
    const index = clamp(rawIndex, 0, tracks.length - 1);
    const trackId = tracks[index]?.id ?? null;

    return { trackId, start };
  }

  function getDropInfoFromHeaderPoint(
    clientX: number,
    clientY: number,
  ): { trackId: string | null; start: number } | null {
    if (!trackHeadersRef) return null;
    const rect = trackHeadersRef.getBoundingClientRect();
    if (
      clientX < rect.left ||
      clientX > rect.right ||
      clientY < rect.top + RULER_HEIGHT_PX ||
      clientY > rect.bottom
    )
      return null;

    const y = clamp(
      clientY - rect.top - RULER_HEIGHT_PX,
      0,
      Math.max(0, rect.height - RULER_HEIGHT_PX - 1),
    );
    const rawIndex = Math.floor(y / TRACK_HEIGHT_PX);
    const index = clamp(rawIndex, 0, tracks.length - 1);
    const trackId = tracks[index]?.id ?? null;
    if (!trackId) return null;

    const start = clampClipStart(currentTime, DEFAULT_CLIP_DURATION);
    return { trackId, start };
  }

  function formatTime(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  function formatTimecode(seconds: number): string {
    const totalFrames = Math.max(0, Math.round(seconds * 30));
    const frames = totalFrames % 30;
    const totalSeconds = Math.floor(totalFrames / 30);
    const secs = totalSeconds % 60;
    const totalMinutes = Math.floor(totalSeconds / 60);
    const mins = totalMinutes % 60;
    const hours = Math.floor(totalMinutes / 60);
    return [hours, mins, secs, frames]
      .map((value) => value.toString().padStart(2, "0"))
      .join(":");
  }

  function resolveDropInfo(
    clientX: number,
    clientY: number,
  ): { trackId: string | null; start: number } | null {
    if (isPointInsideTimeline(clientX, clientY)) {
      return getDropInfoFromPoint(clientX, clientY);
    }
    return getDropInfoFromHeaderPoint(clientX, clientY);
  }

  function getActiveClipAtTime(time: number): Clip | null {
    return (
      clips.find(
        (clip) => time >= clip.start && time < clip.start + clip.duration,
      ) ?? null
    );
  }

  function getTrackIdFromClientY(clientY: number): string | null {
    if (!timelineRef) return null;
    const rect = timelineRef.getBoundingClientRect();
    if (clientY < rect.top || clientY > rect.bottom) return null;

    const y = clamp(clientY - rect.top, 0, Math.max(0, rect.height - 1));
    const rawIndex = Math.floor(y / TRACK_HEIGHT_PX);
    const index = clamp(rawIndex, 0, tracks.length - 1);
    return tracks[index]?.id ?? null;
  }

  function getTimeFromClientX(clientX: number): number {
    if (!timelineRef) return currentTime;
    const rect = timelineRef.getBoundingClientRect();
    const x = clamp(clientX - rect.left, 0, rect.width);
    const percentage = rect.width > 0 ? x / rect.width : 0;
    return percentage * viewDuration;
  }

  function getFileName(path: string): string {
    return path.split(/[/\\]/).pop() || path;
  }

  function snapTime(
    time: number,
    ignoreClipIds: string[] = [],
    clipDuration?: number,
  ): number {
    const thresholdSeconds = getSnapThresholdSeconds();
    if (!timelineRef || viewDuration <= 0 || thresholdSeconds <= 0) return time;

    // Eğer timeline boşsa ve ignore list de boşsa, direkt 0'a snap et
    if (clips.length === 0 && ignoreClipIds.length === 0 && time < 5) return 0;

    let bestTime = time;
    let minDiff = thresholdSeconds; // Eşikten daha küçük olmalı

    // Snap noktaları: Başlangıç, Playhead
    const points = [0, currentTime];

    // Diğer kliplerin başı ve sonu
    for (const clip of clips) {
      if (ignoreClipIds.includes(clip.id)) continue;
      points.push(clip.start);
      points.push(clip.start + clip.duration);
    }

    // En yakın snap noktasını bul
    const considerCandidate = (candidate: number) => {
      const diff = Math.abs(time - candidate);
      if (diff < minDiff) {
        minDiff = diff;
        bestTime = candidate;
      }
    };

    for (const point of points) {
      considerCandidate(point);
      if (clipDuration !== undefined) {
        considerCandidate(point - clipDuration);
      }
    }

    return bestTime;
  }

  async function addClipFromDrop(
    dropInfo: { trackId: string | null; start: number } | null,
    file: string,
  ) {
    if (!dropInfo?.trackId || !file) return;
    const targetTrack = tracks.find((track) => track.id === dropInfo.trackId);
    if (!targetTrack || !canPlaceFileOnTrack(file, targetTrack)) return;

    // Get actual video duration
    const videoDuration = IMAGE_FILE_RE.test(file)
      ? 5
      : await getVideoDuration(file);
    console.log("[Timeline] Adding clip:", {
      file,
      videoDuration,
      trackId: dropInfo.trackId,
    });

    const id = crypto.randomUUID();
    const start = resolveNonOverlappingStart(
      dropInfo.trackId,
      id,
      dropInfo.start,
      videoDuration,
    );

    pushHistory();

    const newClip = createClip({
      id,
      trackId: dropInfo.trackId,
      file,
      kind: inferClipKind(file, targetTrack),
      start,
      duration: videoDuration,
      sourceDuration: videoDuration,
    });

    console.log("[Timeline] Created clip:", {
      id,
      start,
      duration: videoDuration,
      file,
    });

    clips = [...clips, newClip];
    selectClip(newClip.id);
    void hydrateWaveform(newClip.id, newClip.file);
  }

  export async function addMediaAtPlayhead(file: string) {
    const targetTrack = tracks.find((track) => canPlaceFileOnTrack(file, track));
    if (!targetTrack) return;
    await addClipFromDrop(
      { trackId: targetTrack.id, start: Math.max(0, currentTime) },
      file,
    );
  }

  function closeTimelineContextMenu() {
    timelineContextMenu = { ...timelineContextMenu, open: false };
  }

  function isClipLocked(clip: Clip): boolean {
    return tracks.find((track) => track.id === clip.trackId)?.locked ?? true;
  }

  function deleteClipById(clipId: string, ripple: boolean): boolean {
    const snapshot = createSnapshot();
    try {
      const result = deleteClip(createProjectState(), clipId, { ripple });
      pushHistory(snapshot);
      applyProjectState(result.state);
      return true;
    } catch {
      return false;
    }
  }

  function removeTransitionById(
    clipId: string,
    side: TransitionSide,
  ): boolean {
    const clip = clips.find((item) => item.id === clipId);
    if (!clip || clip.transition[side].type === "none") {
      if (selectedTransition?.clipId === clipId && selectedTransition.side === side) {
        selectedTransition = null;
      }
      return false;
    }
    if (isClipLocked(clip)) return false;

    const changed = commitClipTransitionChange(clipId, side, "type", "none");
    if (changed && selectedTransition?.clipId === clipId && selectedTransition.side === side) {
      selectedTransition = null;
    }
    return changed;
  }

  function copyClipById(clipId: string): boolean {
    const clip = clips.find((item) => item.id === clipId);
    if (!clip) return false;
    clipClipboard = createClip(clip);
    return true;
  }

  function cutClipById(clipId: string) {
    const clip = clips.find((item) => item.id === clipId);
    if (!clip || isClipLocked(clip)) return;
    copyClipById(clipId);
    deleteClipById(clipId, false);
  }

  function firstFreeStartAtOrAfter(
    trackId: string,
    desiredStart: number,
    clipDuration: number,
  ): number {
    let candidate = Math.max(0, desiredStart);
    const occupied = clips
      .filter((clip) => clip.trackId === trackId)
      .sort((a, b) => a.start - b.start);

    for (const clip of occupied) {
      if (candidate + clipDuration <= clip.start + TIMELINE_EPSILON) break;
      if (candidate < clip.start + clip.duration - TIMELINE_EPSILON) {
        candidate = clip.start + clip.duration;
      }
    }
    return candidate;
  }

  function findPasteTrack(clip: Clip, preferredTrackId?: string | null) {
    const preferred = preferredTrackId
      ? tracks.find((track) => track.id === preferredTrackId)
      : null;
    if (preferred && canPlaceClipOnTrack(clip, preferred)) return preferred;
    const original = tracks.find((track) => track.id === clip.trackId);
    if (original && canPlaceClipOnTrack(clip, original)) return original;
    return tracks.find((track) => canPlaceClipOnTrack(clip, track)) ?? null;
  }

  function pasteClipAt(time: number, preferredTrackId?: string | null) {
    if (!clipClipboard) return;
    const targetTrack = findPasteTrack(clipClipboard, preferredTrackId);
    if (!targetTrack) return;
    const start = firstFreeStartAtOrAfter(
      targetTrack.id,
      time,
      clipClipboard.duration,
    );
    const pasted = createClip({
      ...clipClipboard,
      id: crypto.randomUUID(),
      trackId: targetTrack.id,
      start,
    });
    pushHistory();
    clips = [...clips, pasted];
    selectClip(pasted.id);
    if (pasted.waveform.length === 0 && pasted.file) {
      void hydrateWaveform(pasted.id, pasted.file);
    }
  }

  function duplicateClipById(clipId: string) {
    const clip = clips.find((item) => item.id === clipId);
    if (!clip || isClipLocked(clip)) return;
    const start = firstFreeStartAtOrAfter(
      clip.trackId,
      clip.start + clip.duration,
      clip.duration,
    );
    const duplicate = createClip({
      ...clip,
      id: crypto.randomUUID(),
      start,
    });
    pushHistory();
    clips = [...clips, duplicate];
    selectClip(duplicate.id);
  }

  function resetClipProperties(clipId: string) {
    const clip = clips.find((item) => item.id === clipId);
    if (!clip || isClipLocked(clip)) return;
    const defaults = createClip({
      id: "defaults",
      trackId: clip.trackId,
      kind: clip.kind,
      file: clip.file,
      start: 0,
      duration: 1,
      sourceDuration: 1,
    });
    pushHistory();
    clips = clips.map((item) =>
      item.id === clipId
        ? createClip({
            ...item,
            transform: defaults.transform,
            volume: defaults.volume,
            pan: defaults.pan,
          })
        : item,
    );
  }

  function revealClipFile(clip: Clip) {
    if (!isTauri() || !clip.file) return;
    void revealItemInDir(clip.file).catch(() => undefined);
  }

  function openClipContextMenu(e: MouseEvent, clip: Clip) {
    e.preventDefault();
    e.stopPropagation();
    selectClip(clip.id);
    timelineContextMenu = {
      open: true,
      x: e.clientX,
      y: e.clientY,
      clipId: clip.id,
      transitionSide: null,
      trackId: clip.trackId,
      time: getTimeFromClientX(e.clientX),
    };
  }

  function openTransitionContextMenu(
    e: MouseEvent,
    clip: Clip,
    side: TransitionSide,
  ) {
    if (clip.transition[side].type === "none") return;
    e.preventDefault();
    e.stopPropagation();
    selectTransitionSide(clip, side);
    timelineContextMenu = {
      open: true,
      x: e.clientX,
      y: e.clientY,
      clipId: clip.id,
      transitionSide: side,
      trackId: clip.trackId,
      time: getTimeFromClientX(e.clientX),
    };
  }

  function getTrackRemovalState(trackId: string): {
    allowed: boolean;
    reason: string;
  } {
    const track = tracks.find((item) => item.id === trackId);
    if (!track) return { allowed: false, reason: "Kanal bulunamadı" };
    if (track.type === "text") {
      return { allowed: false, reason: "Metin kanalı proje düzeninin sabit parçasıdır" };
    }
    if (track.locked) {
      return { allowed: false, reason: "Kanal kilitli; kaldırmadan önce kilidi açın" };
    }
    if (tracks.filter((item) => item.type === track.type).length <= 1) {
      return {
        allowed: false,
        reason: `En az bir ${track.type === "video" ? "video" : "ses"} kanalı kalmalıdır`,
      };
    }
    return { allowed: true, reason: `${track.name} kanalını kaldır` };
  }

  function removeTrackById(trackId: string) {
    const track = tracks.find((item) => item.id === trackId);
    const removal = getTrackRemovalState(trackId);
    if (!track || !removal.allowed) return;
    const clipCount = clips.filter((clip) => clip.trackId === trackId).length;
    if (
      clipCount > 0 &&
      !window.confirm(
        `${track.name} kanalı ve içindeki ${clipCount} klip silinsin mi? Bu işlemi geri alabilirsiniz.`,
      )
    ) return;

    const snapshot = createSnapshot();
    try {
      const nextState = deleteTrack(createProjectState(), trackId, {
        cascade: true,
      });
      pushHistory(snapshot);
      applyProjectState(nextState);
    } catch {
      // Engine keeps locked, fixed and last-of-type channels safe.
    }
  }

  function openTrackContextMenu(e: MouseEvent, trackId: string) {
    e.preventDefault();
    e.stopPropagation();
    timelineContextMenu = {
      open: true,
      x: e.clientX,
      y: e.clientY,
      clipId: null,
      transitionSide: null,
      trackId,
      time: currentTime,
    };
  }

  function openTimelineContextMenu(e: MouseEvent) {
    if ((e.target as HTMLElement).closest(".clip")) return;
    e.preventDefault();
    timelineContextMenu = {
      open: true,
      x: e.clientX,
      y: e.clientY,
      clipId: null,
      transitionSide: null,
      trackId: getTrackIdFromClientY(e.clientY),
      time: getTimeFromClientX(e.clientX),
    };
  }

  function separateAudioFromVideoClip(clipId: string) {
    const source = clips.find((clip) => clip.id === clipId);
    if (!source || source.kind !== "video" || !source.file || isClipLocked(source)) {
      return;
    }

    const snapshot = createSnapshot();
    let nextTracks = tracks;
    let voiceTrack = tracks.find(
      (track) => track.type === "audio" && !track.locked && /^voice(?:\s+\d+)?$/i.test(track.name),
    );

    if (!voiceTrack) {
      const existingNames = new Set(tracks.map((track) => track.name.toLocaleLowerCase()));
      let voiceName = "Voice";
      let suffix = 2;
      while (existingNames.has(voiceName.toLocaleLowerCase())) {
        voiceName = `Voice ${suffix++}`;
      }
      voiceTrack = createTrack(`a-voice-${crypto.randomUUID()}`, voiceName, "audio");
      const textTrackIndex = tracks.findIndex((track) => track.type === "text");
      nextTracks =
        textTrackIndex === -1
          ? [...tracks, voiceTrack]
          : [
              ...tracks.slice(0, textTrackIndex),
              voiceTrack,
              ...tracks.slice(textTrackIndex),
            ];
    }

    const voiceClip = createClip({
      ...source,
      id: crypto.randomUUID(),
      trackId: voiceTrack.id,
      kind: "audio",
    });
    const mutedVideo = createClip({
      ...source,
      volume: 0,
      keyframes: { ...source.keyframes, volume: [] },
      audioSeparated: true,
      waveform: [],
    });

    pushHistory(snapshot);
    tracks = nextTracks;
    clips = [
      ...clips.map((clip) => (clip.id === source.id ? mutedVideo : clip)),
      voiceClip,
    ];
    selectClip(voiceClip.id);
    if (voiceClip.waveform.length === 0) {
      void hydrateWaveform(voiceClip.id, voiceClip.file);
    }
  }

  function getTimelineContextMenuItems(): ContextMenuItem[] {
    const menuClip = timelineContextMenu.clipId
      ? clips.find((clip) => clip.id === timelineContextMenu.clipId) ?? null
      : null;
    const menuTransitionSide = timelineContextMenu.transitionSide;

    if (
      menuClip &&
      menuTransitionSide &&
      menuClip.transition[menuTransitionSide].type !== "none"
    ) {
      const transition = menuClip.transition[menuTransitionSide];
      const preset = getTransitionPreset(transition.type);
      const sideLabel = menuTransitionSide === "in" ? "giriş" : "çıkış";
      const locked = isClipLocked(menuClip);
      return [
        {
          id: "edit-transition",
          label: "Geçişi düzenle",
          onSelect: () => selectTransitionSide(menuClip, menuTransitionSide),
        },
        {
          id: "reset-transition-duration",
          label: "Süreyi varsayılana getir",
          disabled: locked,
          onSelect: () =>
            commitClipTransitionChange(
              menuClip.id,
              menuTransitionSide,
              "duration",
              preset.defaultDuration,
            ),
        },
        { id: "transition-divider", divider: true },
        {
          id: "remove-transition",
          label: `${sideLabel[0].toLocaleUpperCase("tr-TR") + sideLabel.slice(1)} geçişini kaldır`,
          shortcut: "Delete",
          disabled: locked,
          danger: true,
          onSelect: () => removeTransitionById(menuClip.id, menuTransitionSide),
        },
      ];
    }

    if (!menuClip) {
      const items: ContextMenuItem[] = [
        {
          id: "paste",
          label: "Buraya yapıştır",
          shortcut: "Ctrl V",
          disabled: !clipClipboard,
          onSelect: () =>
            pasteClipAt(timelineContextMenu.time, timelineContextMenu.trackId),
        },
        { id: "background-divider-1", divider: true },
        {
          id: "add-text",
          label: "Metin klibi ekle",
          onSelect: () => addTextClipToTimeline(timelineContextMenu.time),
        },
        {
          id: "add-video-track",
          label: "Yeni video kanalı ekle",
          onSelect: () => addTrack("video"),
        },
        {
          id: "add-audio-track",
          label: "Yeni ses kanalı ekle",
          onSelect: () => addTrack("audio"),
        },
      ];
      const menuTrack = timelineContextMenu.trackId
        ? tracks.find((track) => track.id === timelineContextMenu.trackId) ?? null
        : null;
      if (menuTrack && menuTrack.type !== "text") {
        const removal = getTrackRemovalState(menuTrack.id);
        items.push(
          { id: "track-divider", divider: true },
          {
            id: "remove-track",
            label: `${menuTrack.name} kanalını kaldır`,
            disabled: !removal.allowed,
            danger: true,
            onSelect: () => removeTrackById(menuTrack.id),
          },
        );
      }
      items.push(
        { id: "background-divider-2", divider: true },
        {
          id: "toggle-ripple",
          label: `Boşluğu kapat (Ripple): ${rippleMode ? "Açık" : "Kapalı"}`,
          onSelect: () => (rippleMode = !rippleMode),
        },
      );
      return items;
    }

    const locked = isClipLocked(menuClip);
    const splitAllowed =
      !locked &&
      timelineContextMenu.time > menuClip.start + MIN_CLIP_DURATION &&
      timelineContextMenu.time <
        menuClip.start + menuClip.duration - MIN_CLIP_DURATION;

    return [
      ...(menuClip.kind === "video"
        ? [
            {
              id: "separate-audio",
              label: menuClip.audioSeparated
                ? "Ses zaten ayrıldı"
                : "Sesi ayır → Voice",
              disabled: locked || !menuClip.file || menuClip.audioSeparated,
              onSelect: () => separateAudioFromVideoClip(menuClip.id),
            },
            { id: "voice-divider", divider: true },
          ]
        : []),
      ...((menuClip.kind === "video" || menuClip.kind === "audio")
        ? [
            {
              id: "ai-voice-rider",
              label: menuClip.voiceRider
                ? "AI Voice Rider'ı yeniden analiz et"
                : "AI Voice Rider uygula",
              disabled:
                locked ||
                voiceRiderBusy ||
                audioNormalizeBusy ||
                !menuClip.file ||
                menuClip.audioSeparated ||
                !onVoiceRider,
              onSelect: () => onVoiceRider?.(createClip(menuClip)),
            },
            ...(menuClip.voiceRider
              ? [
                  {
                    id: "remove-ai-voice-rider",
                    label: "AI Voice Rider'ı kaldır",
                    disabled: locked || voiceRiderBusy || !onVoiceRiderRemove,
                    onSelect: () => onVoiceRiderRemove?.(createClip(menuClip)),
                  },
                ]
              : []),
            {
              id: "normalize-audio",
              label: "Akıllı normalize",
              disabled:
                locked ||
                audioNormalizeBusy ||
                Boolean(menuClip.voiceRider) ||
                !menuClip.file ||
                menuClip.audioSeparated ||
                !onAudioNormalize,
              onSelect: () => onAudioNormalize?.(createClip(menuClip)),
            },
            {
              id: "export-audio",
              label:
                menuClip.kind === "video"
                  ? "Sesi dışa aktar…"
                  : "Voice klibini dışa aktar…",
              disabled: !menuClip.file || !onAudioExport,
              onSelect: () => onAudioExport?.(createClip(menuClip)),
            },
            { id: "audio-export-divider", divider: true },
          ]
        : []),
      {
        id: "split",
        label: "Burada böl",
        shortcut: "S",
        disabled: !splitAllowed,
        onSelect: () =>
          splitClipAtTime(timelineContextMenu.time, menuClip.id),
      },
      { id: "clip-divider-1", divider: true },
      {
        id: "copy",
        label: "Kopyala",
        shortcut: "Ctrl C",
        onSelect: () => copyClipById(menuClip.id),
      },
      {
        id: "cut",
        label: "Kes",
        shortcut: "Ctrl X",
        disabled: locked,
        onSelect: () => cutClipById(menuClip.id),
      },
      {
        id: "duplicate",
        label: "Çoğalt",
        shortcut: "Ctrl D",
        disabled: locked,
        onSelect: () => duplicateClipById(menuClip.id),
      },
      { id: "clip-divider-2", divider: true },
      {
        id: "delete",
        label: "Sil",
        shortcut: "Backspace",
        disabled: locked,
        danger: true,
        onSelect: () => deleteClipById(menuClip.id, false),
      },
      {
        id: "ripple-delete",
        label: "Sil ve boşluğu kapat",
        disabled: locked,
        danger: true,
        onSelect: () => deleteClipById(menuClip.id, true),
      },
      { id: "clip-divider-3", divider: true },
      {
        id: "reset",
        label: "Konum, boyut ve sesi sıfırla",
        disabled: locked,
        onSelect: () => resetClipProperties(menuClip.id),
      },
      {
        id: "reveal",
        label: "Dosya konumunu aç",
        disabled: !menuClip.file || !isTauri(),
        onSelect: () => revealClipFile(menuClip),
      },
    ];
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    // A focused range slider (clip/track volume, region level) has no native
    // text undo to protect, so it must not swallow Escape/Ctrl+Z like a real
    // text field would.
    const isRangeInput =
      e.target instanceof HTMLInputElement && e.target.type === "range";
    const isTextEntry =
      !isRangeInput &&
      (e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement ||
        (e.target instanceof HTMLElement && e.target.isContentEditable));

    if (
      e.code === "Escape" &&
      (hasActiveTimelineInteraction || (!isTextEntry && selectedClipId !== null))
    ) {
      e.preventDefault();
      cancelActiveTimelineOperation();
      return;
    }

    if (isTextEntry) return;

    const isModifier = e.ctrlKey || e.metaKey;
    if (isModifier && e.code === "KeyZ") {
      e.preventDefault();
      // An in-progress region draft hasn't been committed to history yet, so
      // the first Ctrl+Z discards it instead of undoing an unrelated edit.
      if (audioRegionSelection || audioRegionModeClipId) {
        cancelAudioRegionEdit();
        return;
      }
      if (e.shiftKey) redo();
      else undo();
      return;
    }

    if (isModifier && e.code === "KeyY") {
      e.preventDefault();
      redo();
      return;
    }

    if (isRangeInput) return;

    if (isModifier && !e.shiftKey && e.code === "KeyC") {
      if (!selectedClipId) return;
      e.preventDefault();
      copyClipById(selectedClipId);
      return;
    }

    if (isModifier && !e.shiftKey && e.code === "KeyX") {
      if (!selectedClipId) return;
      e.preventDefault();
      cutClipById(selectedClipId);
      return;
    }

    if (isModifier && !e.shiftKey && e.code === "KeyV") {
      if (!clipClipboard) return;
      e.preventDefault();
      pasteClipAt(currentTime, selectedTimelineClip?.trackId);
      return;
    }

    if (isModifier && !e.shiftKey && e.code === "KeyD") {
      if (!selectedClipId) return;
      e.preventDefault();
      duplicateClipById(selectedClipId);
      return;
    }

    if (isModifier) return;

    if (e.code === "KeyV") {
      e.preventDefault();
      setActiveTool("select");
      return;
    }

    if (e.code === "KeyC") {
      e.preventDefault();
      setActiveTool("cut");
      splitClip();
      return;
    }

    // Split (S)
    if (e.code === "KeyS") {
      e.preventDefault();
      splitClip();
      return;
    }

    // Delete
    if (e.key === "Delete" || e.key === "Backspace") {
      if (selectedTransition) {
        e.preventDefault();
        removeTransitionById(selectedTransition.clipId, selectedTransition.side);
        return;
      }
      if (!selectedClipId) return;
      e.preventDefault();
      deleteClipById(selectedClipId, rippleMode);
    }
  }

  function handleTimelineClick(e: MouseEvent) {
    if (!timelineRef) return;
    // Prevent seeking if clicking a clip
    if ((e.target as HTMLElement).closest(".clip")) return;

    selectClip(null);
    currentTime = getTimeFromClientX(e.clientX);
  }

  function handleFocusedTimelineSeek(e: KeyboardEvent) {
    const shortcut = resolveTransportShortcut(e);
    if (
      shortcut?.kind !== "seek-relative" &&
      shortcut?.kind !== "seek-edge"
    ) {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    currentTime = applyTransportSeek(currentTime, duration, shortcut);
  }

  function handleTimelineKeydown(e: KeyboardEvent) {
    if (e.target !== timelineRef) return;
    handleFocusedTimelineSeek(e);
  }

  function handlePlayheadKeydown(e: KeyboardEvent) {
    handleFocusedTimelineSeek(e);
  }

  function handleClipPointerDown(e: PointerEvent, clip: Clip) {
    if (e.button !== 0) return;
    if ($pointerDragState.active) return;

    if ($activeTool !== "select") {
      e.stopPropagation();
      return;
    }

    e.preventDefault();
    e.stopPropagation();

    selectClip(clip.id);
    if (tracks.find((track) => track.id === clip.trackId)?.locked) return;
    clipDragId = clip.id;
    clipDragPointerId = e.pointerId;
    clipDragStartX = e.clientX;
    clipDragStartY = e.clientY;
    clipDragStartTime = clip.start;
    clipDragHasMoved = false;
    dragSnapshot = createSnapshot();
    clipMovePreview = {
      clipId: clip.id,
      trackId: clip.trackId,
      requestedStart: clip.start,
      start: clip.start,
      duration: clip.duration,
      mode: "move",
      ripple: rippleMode,
      rippleClipStarts: {},
      isAllowed: true,
      message: `${tracks.find((track) => track.id === clip.trackId)?.name ?? "Kanal"} · ${formatTimecode(clip.start)}`,
    };
    clipDragCaptureElement = e.currentTarget as HTMLElement;
    try {
      clipDragCaptureElement.setPointerCapture(e.pointerId);
    } catch {
      // Pointer capture is a progressive enhancement in embedded webviews.
    }
  }

  function getClipMoveRejectionMessage(clip: Clip, track: Track): string {
    if (track.locked) return `${track.name} kilitli; önce L kilidini açın.`;
    if (clip.kind === "audio") return "Ses klibi yalnız bir ses kanalına taşınabilir.";
    if (clip.kind === "text") return "Metin klibi yalnız bir metin kanalına taşınabilir.";
    return "Video ve görseller yalnız bir video kanalına taşınabilir.";
  }

  function findInsertBoundaryNearPointer(
    trackId: string,
    pointerTime: number,
    movingClipId: string,
  ): number | null {
    const threshold = getSnapThresholdSeconds();
    if (threshold <= 0) return null;

    const targetClips = clips.filter(
      (clip) => clip.trackId === trackId && clip.id !== movingClipId,
    );
    const boundaries = [
      ...new Set(
        targetClips.flatMap((clip) => [
          clip.start,
          clip.start + clip.duration,
        ]),
      ),
    ];
    let nearest: number | null = null;
    let nearestDistance = threshold;

    for (const boundary of boundaries) {
      const distance = Math.abs(pointerTime - boundary);
      if (distance <= nearestDistance) {
        nearest = boundary;
        nearestDistance = distance;
      }
    }

    if (nearest === null) return null;
    const crossesExistingClip = targetClips.some(
      (clip) =>
        nearest! > clip.start + CLIP_OVERLAP_EPS &&
        nearest! < clip.start + clip.duration - CLIP_OVERLAP_EPS,
    );
    return crossesExistingClip ? null : nearest;
  }

  function getRipplePreviewTransform(clip: Clip): string | undefined {
    if (!clipMovePreview || !timelineRef) return undefined;
    const previewStart = clipMovePreview.rippleClipStarts[clip.id];
    if (previewStart === undefined) return undefined;
    const shiftPixels =
      ((previewStart - clip.start) / viewDuration) * timelineRef.clientWidth;
    return `translateX(${shiftPixels}px)`;
  }

  function isRipplePreviewShift(
    clip: Clip,
    direction: "left" | "right",
  ): boolean {
    if (!clipMovePreview) return false;
    const previewStart = clipMovePreview.rippleClipStarts[clip.id];
    if (previewStart === undefined) return false;
    return direction === "left"
      ? previewStart < clip.start - TIMELINE_EPSILON
      : previewStart > clip.start + TIMELINE_EPSILON;
  }

  function getRippleClipStarts(
    resultState: TimelineProjectState,
    movingClipId: string,
  ): Record<string, number> {
    const currentStarts = new Map(clips.map((clip) => [clip.id, clip.start]));
    return Object.fromEntries(
      resultState.clips.flatMap((clip) => {
        const currentStart = currentStarts.get(clip.id);
        return clip.id !== movingClipId &&
          currentStart !== undefined &&
          Math.abs(clip.start - currentStart) > TIMELINE_EPSILON
          ? [[clip.id, clip.start] as const]
          : [];
      }),
    );
  }

  function updateClipMovePreview(e: PointerEvent) {
    if (!clipDragId || !timelineRef) return;
    const clip = clips.find((item) => item.id === clipDragId);
    if (!clip) return;

    const deltaX = e.clientX - clipDragStartX;
    const rect = timelineRef.getBoundingClientRect();
    const deltaTime = rect.width > 0 ? (deltaX / rect.width) * viewDuration : 0;
    const rawStart = clampClipStart(clipDragStartTime + deltaTime, clip.duration);
    const preserveTime = Math.abs(deltaX) < POINTER_DRAG_THRESHOLD;
    const snappedStart = preserveTime
      ? clipDragStartTime
      : clampClipStart(
          snapTime(rawStart, [clip.id], clip.duration),
          clip.duration,
        );
    const candidateTrackId = getTrackIdFromClientY(e.clientY);

    if (!candidateTrackId) {
      clipMovePreview = {
        clipId: clip.id,
        trackId: clip.trackId,
        requestedStart: snappedStart,
        start: snappedStart,
        duration: clip.duration,
        mode: "move",
        ripple: rippleMode,
        rippleClipStarts: {},
        isAllowed: false,
        message: "Klibi zaman çizelgesindeki bir kanala bırakın.",
      };
      return;
    }

    const candidateTrack = tracks.find((track) => track.id === candidateTrackId);
    if (!candidateTrack) return;
    if (!canPlaceClipOnTrack(clip, candidateTrack)) {
      clipMovePreview = {
        clipId: clip.id,
        trackId: candidateTrack.id,
        requestedStart: snappedStart,
        start: snappedStart,
        duration: clip.duration,
        mode: "move",
        ripple: rippleMode,
        rippleClipStarts: {},
        isAllowed: false,
        message: getClipMoveRejectionMessage(clip, candidateTrack),
      };
      return;
    }

    let finalStart = snappedStart;
    let requestedStart = snappedStart;
    let isAllowed = false;
    let mode: ClipMovePreview["mode"] = "move";
    let rippleClipStarts: Record<string, number> = {};
    let insertMessage = "";

    // Ripple turns a nearby edit boundary into an insert edit. With Ripple
    // disabled, dragging remains a non-destructive free move and cannot push
    // neighbouring clips out of the way.
    const insertionTime = findInsertBoundaryNearPointer(
      candidateTrack.id,
      getTimeFromClientX(e.clientX),
      clip.id,
    );
    if (rippleMode && insertionTime !== null) {
      try {
        const insertResult = moveClip(
          createProjectState(),
          clip.id,
          candidateTrack.id,
          insertionTime,
          { mode: "insert", rippleSource: rippleMode },
        );
        const insertedClip = insertResult.state.clips.find(
          (item) => item.id === clip.id,
        );
        const nextRippleClipStarts = getRippleClipStarts(
          insertResult.state,
          clip.id,
        );
        const insertChangesTimeline =
          insertedClip !== undefined &&
          (insertedClip.trackId !== clip.trackId ||
            Math.abs(insertedClip.start - clip.start) > TIMELINE_EPSILON ||
            Object.keys(nextRippleClipStarts).length > 0);

        if (insertedClip && insertChangesTimeline) {
          mode = "insert";
          requestedStart = insertionTime;
          finalStart = insertedClip.start;
          rippleClipStarts = nextRippleClipStarts;
          isAllowed = true;

          const leftShiftCount = Object.entries(rippleClipStarts).filter(
            ([id, start]) =>
              start <
              (clips.find((item) => item.id === id)?.start ?? start) -
                TIMELINE_EPSILON,
          ).length;
          const rightShiftCount = Object.entries(rippleClipStarts).filter(
            ([id, start]) =>
              start >
              (clips.find((item) => item.id === id)?.start ?? start) +
                TIMELINE_EPSILON,
          ).length;
          const rippleDetails = [
            leftShiftCount > 0
              ? `kaynak boşluğu kapanacak (${leftShiftCount} klip sola)`
              : "",
            rightShiftCount > 0
              ? `sağdaki ${rightShiftCount} klip yer açacak`
              : "",
          ].filter(Boolean);
          insertMessage = `Araya eklenecek · ${candidateTrack.name} · ${formatTimecode(finalStart)} · ${rippleDetails.length > 0 ? rippleDetails.join(" · ") : "mevcut boşluk kullanılacak"}`;
        }
      } catch {
        // Fall through to a normal move when the boundary cannot accept it.
      }
    }

    if (mode !== "insert") {
      const moveCandidates = [
        snappedStart,
        ...(Math.abs(snappedStart - rawStart) > TIMELINE_EPSILON
          ? [rawStart]
          : []),
      ];
      for (const candidateStart of moveCandidates) {
        try {
          const moveResult = moveClip(
            createProjectState(),
            clip.id,
            candidateTrack.id,
            candidateStart,
            { mode: "move", rippleSource: rippleMode },
          );
          const movedClip = moveResult.state.clips.find(
            (item) => item.id === clip.id,
          );
          if (!movedClip) continue;

          finalStart = movedClip.start;
          requestedStart = candidateStart;
          rippleClipStarts = getRippleClipStarts(moveResult.state, clip.id);
          isAllowed = true;
          const leftShiftCount = Object.entries(rippleClipStarts).filter(
            ([id, start]) =>
              start <
              (clips.find((item) => item.id === id)?.start ?? start) -
                TIMELINE_EPSILON,
          ).length;
          insertMessage =
            leftShiftCount > 0
              ? `${candidateTrack.name} · ${formatTimecode(finalStart)} · kaynak boşluğu kapanacak (${leftShiftCount} klip sola)`
              : `${candidateTrack.name} · ${formatTimecode(finalStart)}`;
          break;
        } catch {
          // A raw unsnapped candidate may still be valid after a snap collision.
        }
      }
    }

    clipMovePreview = {
      clipId: clip.id,
      trackId: candidateTrack.id,
      requestedStart,
      start: finalStart,
      duration: clip.duration,
      mode,
      ripple: rippleMode,
      rippleClipStarts,
      isAllowed,
      message: isAllowed
        ? mode === "insert"
          ? insertMessage
          : insertMessage
        : rippleMode
          ? "Bu aralık dolu. Kesim çizgisine yaklaştırarak araya ekleyin."
          : "Bu aralık dolu. Araya eklemek ve boşluğu kapatmak için Ripple'ı açın.",
    };
  }

  function clearClipDrag(pointerId?: number) {
    if (
      pointerId !== undefined &&
      clipDragCaptureElement?.hasPointerCapture(pointerId)
    ) {
      try {
        clipDragCaptureElement.releasePointerCapture(pointerId);
      } catch {
        // The webview may already have released capture on pointerup/cancel.
      }
    }
    dragSnapshot = null;
    clipDragId = null;
    clipDragPointerId = null;
    clipDragHasMoved = false;
    clipMovePreview = null;
    clipDragCaptureElement = null;
  }

  function selectTransitionSide(clip: Clip, side: TransitionSide) {
    selectedClipId = clip.id;
    selectedTransition = { clipId: clip.id, side };
    onTransitionSelect?.(side);
  }

  function isTransitionSelected(clipId: string, side: TransitionSide): boolean {
    return selectedTransition?.clipId === clipId && selectedTransition.side === side;
  }

  function getActiveTransitionSide(clip: Clip): TransitionSide | null {
    if (selectedTransition?.clipId === clip.id) return selectedTransition.side;
    return selectedClipId === clip.id ? transitionEditorSide : null;
  }

  function getDisplayedTransitionDuration(
    clip: Clip,
    side: TransitionSide,
  ): number {
    if (
      transitionDurationDrag?.clipId === clip.id &&
      transitionDurationDrag.side === side
    ) {
      return transitionDurationDrag.duration;
    }
    return clip.transition[side].duration;
  }

  function getTransitionBoundaryPercent(
    clip: Clip,
    side: TransitionSide,
  ): number {
    const duration = getDisplayedTransitionDuration(clip, side);
    const ratio = clip.duration > 0 ? duration / clip.duration : 0;
    return (side === "in" ? ratio : 1 - ratio) * 100;
  }

  function getMaximumTransitionDuration(
    clip: Clip,
    side: TransitionSide,
  ): number {
    const opposite = side === "in" ? clip.transition.out : clip.transition.in;
    return Math.max(0, clip.duration - opposite.duration);
  }

  function commitClipTransitionChange(
    clipId: string,
    side: TransitionSide,
    key: "type" | "duration",
    value: string | number,
  ): boolean {
    const clip = clips.find((item) => item.id === clipId);
    if (!clip || isClipLocked(clip)) return false;
    const nextSide = updateTransitionSide(
      clip.transition[side],
      key,
      value,
      getMaximumTransitionDuration(clip, side),
    );
    const currentSide = clip.transition[side];
    if (
      currentSide.type === nextSide.type &&
      Math.abs(currentSide.duration - nextSide.duration) <= TIMELINE_EPSILON
    ) {
      return false;
    }
    const transition = {
      ...clip.transition,
      [side]: nextSide,
    };
    updateClip(
      clip.id,
      { transition } as Partial<Clip>,
      { history: "immediate" },
    );
    if (nextSide.type === "none" && isTransitionSelected(clip.id, side)) {
      selectedTransition = null;
    }
    return true;
  }

  function handleTransitionEnvelopeClick(
    event: MouseEvent,
    clip: Clip,
    side: TransitionSide,
  ) {
    event.stopPropagation();
    selectTransitionSide(clip, side);
  }

  function handleTransitionPointerDown(
    event: PointerEvent,
    clip: Clip,
    side: TransitionSide,
  ) {
    if (event.button !== 0 || $activeTool !== "select" || isClipLocked(clip)) return;
    const handleElement = event.currentTarget as HTMLElement;
    const clipElement = handleElement.closest<HTMLElement>(".clip");
    if (!clipElement) return;
    event.preventDefault();
    event.stopPropagation();
    selectTransitionSide(clip, side);
    const startDuration = clip.transition[side].duration;
    transitionDurationDrag = {
      clipId: clip.id,
      side,
      pointerId: event.pointerId,
      clipElement,
      handleElement,
      startDuration,
      duration: startDuration,
      changed: false,
    };
    try {
      handleElement.setPointerCapture(event.pointerId);
    } catch {
      // Pointer capture is optional in older embedded webviews.
    }
  }

  function updateTransitionDurationDrag(event: PointerEvent) {
    const drag = transitionDurationDrag;
    if (!drag || event.pointerId !== drag.pointerId) return;
    const clip = clips.find((item) => item.id === drag.clipId);
    if (!clip || isClipLocked(clip)) {
      finishTransitionDurationDrag(event, false);
      return;
    }
    const rect = drag.clipElement.getBoundingClientRect();
    if (rect.width <= 0) return;
    const pointerRatio = clamp((event.clientX - rect.left) / rect.width, 0, 1);
    const rawDuration = drag.side === "in"
      ? pointerRatio * clip.duration
      : (1 - pointerRatio) * clip.duration;
    const step = event.shiftKey ? 0.01 : 0.05;
    const duration = clampTransitionDuration(
      Math.round(rawDuration / step) * step,
      getMaximumTransitionDuration(clip, drag.side),
    );
    transitionDurationDrag = {
      ...drag,
      duration,
      changed: Math.abs(duration - drag.startDuration) > TIMELINE_EPSILON,
    };
  }

  function finishTransitionDurationDrag(
    event: PointerEvent,
    shouldCommit: boolean,
  ) {
    const drag = transitionDurationDrag;
    if (!drag || event.pointerId !== drag.pointerId) return;
    clearTransitionDurationDrag();
    if (shouldCommit && drag.changed) {
      commitClipTransitionChange(drag.clipId, drag.side, "duration", drag.duration);
    }
  }

  function clearTransitionDurationDrag() {
    const drag = transitionDurationDrag;
    if (!drag) return;
    if (drag.handleElement.hasPointerCapture(drag.pointerId)) {
      try {
        drag.handleElement.releasePointerCapture(drag.pointerId);
      } catch {
        // The webview may release capture before pointerup reaches window.
      }
    }
    transitionDurationDrag = null;
  }

  function handleTransitionHandleKeydown(
    event: KeyboardEvent,
    clip: Clip,
    side: TransitionSide,
  ) {
    if (isClipLocked(clip)) return;
    let duration = clip.transition[side].duration;
    const step = event.shiftKey ? 0.01 : 0.05;
    if (event.key === "Home") duration = 0;
    else if (event.key === "End") duration = clip.duration;
    else if (event.key === "ArrowLeft") duration += side === "in" ? -step : step;
    else if (event.key === "ArrowRight") duration += side === "in" ? step : -step;
    else return;
    event.preventDefault();
    event.stopPropagation();
    selectTransitionSide(clip, side);
    commitClipTransitionChange(clip.id, side, "duration", duration);
  }

  function handleTrimPointerDown(
    e: PointerEvent,
    clip: Clip,
    edge: "start" | "end",
  ) {
    if (e.button !== 0 || $activeTool !== "select") return;
    if (tracks.find((track) => track.id === clip.trackId)?.locked) return;
    e.preventDefault();
    e.stopPropagation();
    selectClip(clip.id);
    trimDrag = {
      clipId: clip.id,
      edge,
      pointerId: e.pointerId,
      lastClientX: e.clientX,
      snapshot: createSnapshot(),
      changed: false,
    };
  }

  function handleScrubStart(e: PointerEvent) {
    if (e.button !== 0) return;
    if ($pointerDragState.active) return;
    if ((e.target as HTMLElement).closest(".clip")) return;

    beginScrub(e);
  }

  function handlePlayheadPointerDown(e: PointerEvent) {
    if (e.button !== 0 || $pointerDragState.active) return;
    e.stopPropagation();
    beginScrub(e, true);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function beginScrub(e: PointerEvent, preserveGrabOffset = false) {

    e.preventDefault();
    isScrubbing = true;
    scrubPointerId = e.pointerId;
    if (preserveGrabOffset && timelineRef && viewDuration > 0) {
      const rect = timelineRef.getBoundingClientRect();
      const playheadClientX = rect.left + (currentTime / viewDuration) * rect.width;
      scrubPointerOffsetX = e.clientX - playheadClientX;
    } else {
      scrubPointerOffsetX = 0;
    }
    currentTime = getTimeFromClientX(e.clientX - scrubPointerOffsetX);
  }

  function queueScrubFrame(clientX: number) {
    pendingScrubClientX = clientX;
    if (scrubFrameId !== null) return;
    scrubFrameId = requestAnimationFrame(() => {
      scrubFrameId = null;
      const nextClientX = pendingScrubClientX;
      pendingScrubClientX = null;
      if (nextClientX !== null && isScrubbing) {
        currentTime = getTimeFromClientX(
          nextClientX - scrubPointerOffsetX,
        );
      }
    });
  }

  function finishScrub(commitPending: boolean) {
    if (scrubFrameId !== null) cancelAnimationFrame(scrubFrameId);
    scrubFrameId = null;
    if (commitPending && pendingScrubClientX !== null) {
      currentTime = getTimeFromClientX(
        pendingScrubClientX - scrubPointerOffsetX,
      );
    }
    pendingScrubClientX = null;
    isScrubbing = false;
    scrubPointerId = null;
    scrubPointerOffsetX = 0;
  }

  function updateDragPreviewFromPoint(
    clientX: number,
    clientY: number,
    file: string | null,
  ) {
    const dropInfo = resolveDropInfo(clientX, clientY);
    if (!dropInfo) {
      dragPreview = null;
      setMediaDropFeedback(null);
      return false;
    }

    const targetTrack = dropInfo.trackId
      ? tracks.find((track) => track.id === dropInfo.trackId) ?? null
      : null;
    const isAllowed = Boolean(
      file && targetTrack && canPlaceFileOnTrack(file, targetTrack),
    );
    dragPreview = { ...dropInfo, file, isAllowed };
    setMediaDropFeedback(
      file && targetTrack
        ? {
            allowed: isAllowed,
            message: isAllowed
              ? undefined
              : getDropRejectionMessage(file, targetTrack),
          }
        : null,
    );
    return true;
  }

  function handleDragEnter(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer?.files?.length) return;

    dragDepth += 1;
    isDragActive = true;
    dragSource = "internal";

    if (clearDragTimer !== null) {
      clearTimeout(clearDragTimer);
      clearDragTimer = null;
    }

    const file =
      e.dataTransfer?.getData("text/plain") ||
      e.dataTransfer?.getData("application/x-astral-lunar-media") ||
      e.dataTransfer?.getData("text/uri-list") ||
      $draggedMediaFile ||
      null;
    const hasDropTarget = updateDragPreviewFromPoint(
      e.clientX,
      e.clientY,
      file,
    );
    isDragActive = hasDropTarget;
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    if (e.dataTransfer?.files?.length) return;

    isDragActive = true;
    dragSource = "internal";

    const file =
      e.dataTransfer?.getData("text/plain") ||
      e.dataTransfer?.getData("application/x-astral-lunar-media") ||
      e.dataTransfer?.getData("text/uri-list") ||
      $draggedMediaFile ||
      null;
    const hasDropTarget = updateDragPreviewFromPoint(
      e.clientX,
      e.clientY,
      file,
    );
    isDragActive = hasDropTarget;
  }

  function clearDragState() {
    dragPreview = null;
    setMediaDropFeedback(null);
    isDragActive = false;
    dragSource = "none";
    dragDepth = 0;

    if (clearDragTimer !== null) {
      clearTimeout(clearDragTimer);
      clearDragTimer = null;
    }
  }

  /**
   * Gives the visible Cancel button and Escape one honest exit path. Pointer
   * edits that already previewed a mutation are restored; draft/overlay state
   * is simply closed. With no active interaction, the same action clears the
   * current clip selection.
   */
  function cancelActiveTimelineOperation(): boolean {
    const hadActiveInteraction = hasActiveTimelineInteraction;
    const hadSelectedClip = selectedClipId !== null;

    if (trimDrag) {
      applySnapshot(trimDrag.snapshot);
      trimDrag = null;
    }

    if (transitionDurationDrag) clearTransitionDurationDrag();

    if (clipDragId) {
      if (dragSnapshot) applySnapshot(dragSnapshot);
      clearClipDrag(clipDragPointerId ?? undefined);
    }

    if (isScrubbing) finishScrub(false);

    if (clipVolumeDrag) cancelClipVolumeGainDrag();

    if (
      audioRegionSelection ||
      audioRegionModeClipId ||
      audioRegionDrag ||
      audioRegionResizeDrag ||
      audioRegionGainDrag
    ) {
      cancelAudioRegionEdit();
      audioRegionResizeDrag = null;
    }

    if ($pointerDragState.active) endPointerDrag();
    else if ($draggedMediaFile) endMediaDrag();

    osDragPaths = null;
    clearDragState();
    if (timelineContextMenu.open) closeTimelineContextMenu();
    if (toolHelpOpen) closeToolHelp();
    if ($activeTool !== "select") setActiveTool("select");

    if (!hadActiveInteraction) selectClip(null);
    return hadActiveInteraction || hadSelectedClip;
  }

  function handleDragLeave() {
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth > 0) return;

    if (clearDragTimer !== null) clearTimeout(clearDragTimer);
    clearDragTimer = window.setTimeout(() => {
      if (dragDepth === 0 && dragSource === "internal") clearDragState();
    }, 0);
  }

  function handleTracksDrop(e: DragEvent) {
    e.preventDefault();
    const file =
      e.dataTransfer?.getData("text/plain") ||
      e.dataTransfer?.getData("application/x-astral-lunar-media") ||
      e.dataTransfer?.getData("text/uri-list") ||
      $draggedMediaFile ||
      dragPreview?.file ||
      null;
    if (!file) {
      clearDragState();
      return;
    }

    const dropInfo = resolveDropInfo(e.clientX, e.clientY);
    if (!dropInfo?.trackId) {
      clearDragState();
      return;
    }

    addClipFromDrop(dropInfo, file);
    endMediaDrag();
    clearDragState();
  }

  function handleWindowPointerMove(e: PointerEvent) {
    if (transitionDurationDrag && e.pointerId === transitionDurationDrag.pointerId) {
      updateTransitionDurationDrag(e);
      return;
    }

    if (clipVolumeDrag && e.pointerId === clipVolumeDrag.pointerId) {
      const clip = clips.find((item) => item.id === clipVolumeDrag?.clipId);
      if (clip) handleClipVolumeGainPointerMove(e, clip);
      else cancelClipVolumeGainDrag();
      return;
    }

    if (audioRegionGainDrag && e.pointerId === audioRegionGainDrag.pointerId) {
      const clip = clips.find((item) => item.id === audioRegionGainDrag?.clipId);
      if (clip) handleAudioRegionGainPointerMove(e, clip);
      else cancelAudioRegionEdit();
      return;
    }

    if (trimDrag) {
      if (e.pointerId !== trimDrag.pointerId || !timelineRef) return;
      const rect = timelineRef.getBoundingClientRect();
      const delta =
        rect.width > 0
          ? ((e.clientX - trimDrag.lastClientX) / rect.width) * viewDuration
          : 0;
      if (Math.abs(delta) < TIMELINE_EPSILON) return;
      try {
        const result = trimClip(
          createProjectState(),
          trimDrag.clipId,
          trimDrag.edge,
          delta,
          { ripple: rippleMode },
        );
        if (Math.abs(result.appliedDelta) > TIMELINE_EPSILON) {
          applyProjectState(result.state);
          // Keep the anchor at the handle's actual clamped position. After an
          // overshoot, one pixel back must immediately move the trim handle.
          trimDrag.lastClientX +=
            (result.appliedDelta / viewDuration) * rect.width;
          trimDrag.changed = true;
        }
      } catch {
        // Locked tracks and invalid edits are no-ops in the pointer interaction.
      }
      return;
    }

    if (clipDragId) {
      if (clipDragPointerId !== null && e.pointerId !== clipDragPointerId)
        return;
      if (!timelineRef) return;

      const deltaX = e.clientX - clipDragStartX;
      const deltaY = e.clientY - clipDragStartY;
      if (
        !clipDragHasMoved &&
        Math.hypot(deltaX, deltaY) < POINTER_DRAG_THRESHOLD
      )
        return;

      if (!clipDragHasMoved) {
        clipDragHasMoved = true;
      }
      updateClipMovePreview(e);
      return;
    }

    if (isScrubbing) {
      if (scrubPointerId === null || e.pointerId === scrubPointerId) {
        queueScrubFrame(e.clientX);
      }
    }

    if (!$pointerDragState.active || !$pointerDragState.file || !timelineRef)
      return;
    if (
      $pointerDragState.pointerId !== null &&
      e.pointerId !== $pointerDragState.pointerId
    )
      return;

    const deltaX = e.clientX - $pointerDragState.startX;
    const deltaY = e.clientY - $pointerDragState.startY;
    const distance = Math.hypot(deltaX, deltaY);

    if (!$pointerDragState.hasMoved && distance < POINTER_DRAG_THRESHOLD) {
      return;
    }

    if (!$pointerDragState.hasMoved) {
      markPointerDragMoved();
    }

    const hasDropTarget = updateDragPreviewFromPoint(
      e.clientX,
      e.clientY,
      $pointerDragState.file,
    );
    if (!hasDropTarget) {
      isDragActive = false;
      return;
    }

    dragSource = "internal";
    isDragActive = true;
  }

  function handleWindowPointerUp(e: PointerEvent) {
    if (transitionDurationDrag && e.pointerId === transitionDurationDrag.pointerId) {
      finishTransitionDurationDrag(e, true);
      return;
    }

    if (clipVolumeDrag && e.pointerId === clipVolumeDrag.pointerId) {
      finishClipVolumeGainDrag(e, true);
      return;
    }

    if (audioRegionGainDrag && e.pointerId === audioRegionGainDrag.pointerId) {
      finishAudioRegionGainDrag(e, true);
      return;
    }

    if (audioRegionDrag && e.pointerId === audioRegionDrag.pointerId) {
      const clip = clips.find((item) => item.id === audioRegionDrag?.clipId);
      if (clip) finishAudioRegionSelection(e, clip);
      else cancelAudioRegionEdit();
      return;
    }

    if (trimDrag) {
      if (e.pointerId !== trimDrag.pointerId) return;
      if (trimDrag.changed) pushHistory(trimDrag.snapshot);
      trimDrag = null;
      return;
    }

    if (clipDragId) {
      if (clipDragPointerId !== null && e.pointerId !== clipDragPointerId)
        return;

      const clip = clips.find((item) => item.id === clipDragId);
      const preview = clipMovePreview;
      const snapshot = dragSnapshot;
      if (
        clip &&
        preview?.isAllowed &&
        clipDragHasMoved &&
        (preview.trackId !== clip.trackId ||
          Math.abs(preview.start - clip.start) > TIMELINE_EPSILON ||
          Object.keys(preview.rippleClipStarts).length > 0)
      ) {
        try {
          const result = moveClip(
            createProjectState(),
            clip.id,
            preview.trackId,
            preview.requestedStart,
            { mode: preview.mode, rippleSource: preview.ripple },
          );
          if (snapshot) pushHistory(snapshot);
          applyProjectState(result.state);
        } catch {
          // A last-moment collision leaves the clip in its original position.
        }
      }
      clearClipDrag(e.pointerId);
      return;
    }

    if (isScrubbing) {
      if (scrubPointerId === null || e.pointerId === scrubPointerId) {
        finishScrub(true);
      }
    }

    if (!$pointerDragState.active || !$pointerDragState.file) return;
    if (
      $pointerDragState.pointerId !== null &&
      e.pointerId !== $pointerDragState.pointerId
    )
      return;

    if (!$pointerDragState.hasMoved) {
      endPointerDrag();
      clearDragState();
      return;
    }

    const dropInfo = resolveDropInfo(e.clientX, e.clientY);
    if (!dropInfo?.trackId) {
      endPointerDrag();
      clearDragState();
      return;
    }

    addClipFromDrop(dropInfo, $pointerDragState.file);
    endPointerDrag();
    clearDragState();
  }

  function handleWindowPointerCancel(e: PointerEvent) {
    if (transitionDurationDrag && e.pointerId === transitionDurationDrag.pointerId) {
      finishTransitionDurationDrag(e, false);
      return;
    }

    if (clipVolumeDrag && e.pointerId === clipVolumeDrag.pointerId) {
      finishClipVolumeGainDrag(e, false);
      return;
    }

    if (audioRegionGainDrag && e.pointerId === audioRegionGainDrag.pointerId) {
      finishAudioRegionGainDrag(e, false);
      return;
    }

    if (audioRegionDrag && e.pointerId === audioRegionDrag.pointerId) {
      cancelAudioRegionEdit();
      return;
    }

    if (trimDrag && e.pointerId === trimDrag.pointerId) {
      applySnapshot(trimDrag.snapshot);
      trimDrag = null;
      return;
    }

    if (clipDragId) {
      if (clipDragPointerId !== null && e.pointerId !== clipDragPointerId)
        return;
      if (dragSnapshot) applySnapshot(dragSnapshot);
      clearClipDrag(e.pointerId);
    }

    if (isScrubbing) {
      if (scrubPointerId === null || e.pointerId === scrubPointerId) {
        finishScrub(false);
      }
    }

    if (!$pointerDragState.active) return;
    if (
      $pointerDragState.pointerId !== null &&
      e.pointerId !== $pointerDragState.pointerId
    )
      return;
    endPointerDrag();
    clearDragState();
  }

  function handleWindowDragOver(e: DragEvent) {
    if (!$draggedMediaFile || !timelineRef) return;

    e.preventDefault();

    const hasDropTarget = updateDragPreviewFromPoint(
      e.clientX,
      e.clientY,
      $draggedMediaFile,
    );
    if (!hasDropTarget) {
      if (dragSource === "internal") {
        isDragActive = false;
      }
      return;
    }

    dragSource = "internal";
    isDragActive = true;
  }

  function handleWindowDrop(e: DragEvent) {
    if (!$draggedMediaFile || !timelineRef) return;

    e.preventDefault();

    const dropInfo = resolveDropInfo(e.clientX, e.clientY);
    if (!dropInfo?.trackId) {
      clearDragState();
      endMediaDrag();
      return;
    }

    addClipFromDrop(dropInfo, $draggedMediaFile);
    endMediaDrag();
    clearDragState();
  }

  function handleWindowDragLeave(e: DragEvent) {
    if (!$draggedMediaFile) return;
    if (
      e.relatedTarget &&
      (timelineRef?.contains(e.relatedTarget as Node) ||
        trackHeadersRef?.contains(e.relatedTarget as Node))
    )
      return;
    if (dragSource === "internal") {
      clearDragState();
    }
  }

  function getPlayheadPosition(): number {
    return (currentTime / viewDuration) * 100;
  }

  // Human-friendly ruler steps in seconds. Spacing snaps up to one of these so a
  // long clip never crams hundreds of labels onto the ruler.
  const RULER_STEPS_SECONDS = [
    1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1_800, 3_600,
  ];

  function niceTimeInterval(rawSeconds: number): number {
    for (const step of RULER_STEPS_SECONDS) {
      if (rawSeconds <= step) return step;
    }
    // Beyond one hour, round up to a whole number of hours.
    return Math.ceil(rawSeconds / 3_600) * 3_600;
  }

  function getTimeMarkers(): number[] {
    const total = Math.max(0, viewDuration);
    if (!(total > 0)) return [0];
    // The ruler width scales with zoom (see `width: zoom*100%`), so more labels
    // fit as the user zooms in. Deriving the step from duration keeps the label
    // count bounded (~10 at min zoom) instead of a fixed 10s interval that
    // overlaps on long videos.
    const targetCount = Math.max(6, Math.round(10 * Math.max(1, zoom)));
    const interval = niceTimeInterval(total / targetCount);
    const markers: number[] = [];
    for (let t = 0; t <= total + 1e-6; t += interval) {
      markers.push(Math.round(t * 1_000) / 1_000);
    }
    return markers;
  }

  $effect(() => {
    if (!onClipTimeChange) return;
    const activeClip = getActiveClipAtTime(currentTime);
    const localTime = activeClip ? currentTime - activeClip.start : 0;
    onClipTimeChange(activeClip, localTime);
  });

  $effect(() => {
    if (!onDurationChange) return;
    const nextDuration = timelineContentEnd;
    if (Math.abs(nextDuration - lastDurationHint) < 0.01) return;
    lastDurationHint = nextDuration;
    onDurationChange(nextDuration);
  });

  $effect(() => {
    if (!onProjectStateChange) return;
    // Reading the collections here makes every edit publish a serializable snapshot.
    tracks;
    clips;
    selectedClipId;
    onProjectStateChange(getProjectState());
  });

  $effect(() => {
    if (!onSelect) return;
    const clip = clips.find((item) => item.id === selectedClipId);
    onSelect(clip ? createClip(clip) : null);
  });

  $effect(() => {
    if (!onAudioRegionChange) return;
    const clip = selectedTimelineClip;
    if (
      !clip ||
      (clip.kind !== "video" && clip.kind !== "audio") ||
      clip.audioSeparated
    ) {
      onAudioRegionChange(null);
      return;
    }
    onAudioRegionChange({
      selection:
        audioRegionSelection && audioRegionSelection.clipId === clip.id
          ? {
              start: audioRegionSelection.start,
              end: audioRegionSelection.end,
              volume: audioRegionSelection.volume,
            }
          : null,
      selectMode: audioRegionModeClipId === clip.id,
      quietRegions: getQuietRegions(clip),
    });
  });

  export function updateClip(
    id: string,
    updates: Partial<Clip> | Partial<Clip["transform"]>,
    options: { history?: "none" | "immediate" | "coalesce" } = {},
  ) {
    const index = clips.findIndex((c) => c.id === id);
    if (index === -1) return;

    const clip = clips[index];
    if (tracks.find((track) => track.id === clip.trackId)?.locked) return;
    prepareClipUpdateHistory(id, updates, options.history ?? "none");
    let newClip = createClip(clip);

    // Transform updates
    if (
      "x" in updates ||
      "y" in updates ||
      "scale" in updates ||
      "rotation" in updates ||
      "opacity" in updates
    ) {
      newClip.transform = { ...clip.transform, ...(updates as any) };
    } else {
      newClip = createClip({ ...clip, ...(updates as Partial<Clip>) });
    }

    // Root properties (volume, etc.)
    if ("volume" in updates) {
      const requestedVolume = Number((updates as Partial<Clip>).volume);
      const nextVolume = Number.isFinite(requestedVolume)
        ? clamp(requestedVolume, 0, 4)
        : clip.volume;
      // A clip-level gain change should lift/lower existing manual volume
      // regions with it. Otherwise a quiet-region envelope would silently
      // override the dB control everywhere it has keyframes.
      if (
        !("keyframes" in updates) &&
        clip.volume > 0 &&
        nextVolume > 0 &&
        newClip.keyframes.volume?.length
      ) {
        const scale = nextVolume / clip.volume;
        newClip.keyframes = {
          ...newClip.keyframes,
          volume: newClip.keyframes.volume.map((frame) => ({
            ...frame,
            value: clamp(frame.value * scale, 0, 4),
          })),
        };
      }
      newClip.volume = nextVolume;
    }

    const newClips = [...clips];
    newClips[index] = newClip;
    clips = newClips;
  }

  /** Applies several metadata/settings updates as one undoable transaction. */
  export function updateClips(
    updates: readonly { id: string; updates: Partial<Clip> }[],
  ): boolean {
    if (updates.length === 0) return false;
    const updateMap = new Map(updates.map((item) => [item.id, item.updates]));
    if (
      [...updateMap.keys()].some((id) => {
        const clip = clips.find((item) => item.id === id);
        return !clip || tracks.find((track) => track.id === clip.trackId)?.locked;
      })
    ) return false;
    const snapshot = createSnapshot();
    clips = clips.map((clip) => {
      const patch = updateMap.get(clip.id);
      return patch ? createClip({ ...clip, ...patch }) : clip;
    });
    pushHistory(snapshot);
    return true;
  }

  /** Removes AI suggestions in one history transaction, optionally forcing a contiguous reel. */
  export function applySpeechSuggestionRanges(
    clipId: string,
    ranges: readonly TimelineLocalRange[],
    options: { forceRipple?: boolean } = {},
  ): number {
    const snapshot = createSnapshot();
    try {
      const result = deleteClipRanges(createProjectState(), clipId, ranges, {
        ripple: options.forceRipple ?? rippleMode,
      });
      if (result.removedDuration <= TIMELINE_EPSILON) return 0;
      pushHistory(snapshot);
      applyProjectState(result.state);
      return 1;
    } catch {
      return 0;
    }
  }

  /** Splits unlocked visual clips at beat suggestions as one undoable edit. */
  export function applyBeatCuts(
    times: readonly number[],
    trackIds?: readonly string[],
  ): number {
    const snapshot = createSnapshot();
    try {
      const result = splitVisualsAtTimes(createProjectState(), times, {
        trackIds,
        minimumSegmentSeconds: 0.1,
      });
      if (result.cutCount === 0) return 0;
      pushHistory(snapshot);
      applyProjectState(result.state);
      return result.cutCount;
    } catch {
      return 0;
    }
  }

  export function applyVoiceRider(
    id: string,
    metadata: VoiceRiderMetadata,
    frames: readonly TimelineKeyframe[],
  ): boolean {
    const clip = clips.find((item) => item.id === id);
    if (!clip || tracks.find((track) => track.id === clip.trackId)?.locked) return false;
    const snapshot = createSnapshot();
    try {
      const nextState = setVoiceRiderEnvelope(
        createProjectState(),
        id,
        metadata,
        frames,
      );
      pushHistory(snapshot);
      applyProjectState(nextState);
      return true;
    } catch {
      return false;
    }
  }

  export function removeVoiceRider(id: string): boolean {
    const clip = clips.find((item) => item.id === id);
    if (!clip || tracks.find((track) => track.id === clip.trackId)?.locked) return false;
    const snapshot = createSnapshot();
    try {
      const nextState = removeVoiceRiderEnvelope(createProjectState(), id);
      pushHistory(snapshot);
      applyProjectState(nextState);
      return true;
    } catch {
      return false;
    }
  }

  export function setClipWaveform(id: string, waveform: number[]) {
    const index = clips.findIndex((clip) => clip.id === id);
    if (index === -1) return;
    const nextClips = [...clips];
    nextClips[index] = createClip({ ...clips[index], waveform });
    clips = nextClips;
  }

  function toggleTrackFlag(
    trackId: string,
    flag: "muted" | "solo" | "locked",
  ) {
    const track = tracks.find((item) => item.id === trackId);
    if (!track) return;
    const snapshot = createSnapshot();
    const nextState = setTrackState(createProjectState(), trackId, {
      [flag]: !track[flag],
    });
    pushHistory(snapshot);
    applyProjectState(nextState);
  }

  export function updateTrackMix(
    trackId: string,
    key: "gain" | "pan",
    value: number,
  ) {
    if (!Number.isFinite(value)) return;
    prepareCoalescedHistory(`track:${trackId}:${key}`);
    const nextState = setTrackState(createProjectState(), trackId, {
      [key]: value,
    });
    applyProjectState(nextState);
  }

  /**
   * Replaces only text clips produced by an external generator (captions,
   * titles, etc.) while preserving hand-authored text clips. The operation is
   * one undo step and creates its dedicated text track on first use.
   */
  export function replaceGeneratedTextClips(
    sourcePrefix: string,
    nextClips: readonly Clip[],
    targetTracks: readonly Track[],
  ): boolean {
    if (!sourcePrefix || targetTracks.length === 0) return false;
    const targetTrackIds = new Set(targetTracks.map((track) => track.id));
    const generatedTrackIds = new Set(
      clips
        .filter((clip) => clip.file.startsWith(sourcePrefix))
        .map((clip) => clip.trackId),
    );
    if (
      targetTrackIds.size !== targetTracks.length ||
      tracks.some((track) => generatedTrackIds.has(track.id) && track.locked) ||
      targetTracks.some((targetTrack) => {
        if (targetTrack.type !== "text") return true;
        const existingTrack = tracks.find((track) => track.id === targetTrack.id);
        return Boolean(existingTrack && (existingTrack.type !== "text" || existingTrack.locked));
      })
    ) return false;
    if (
      nextClips.some(
        (clip) =>
          clip.kind !== "text" ||
          !targetTrackIds.has(clip.trackId) ||
          !clip.file.startsWith(sourcePrefix),
      )
    ) {
      return false;
    }

    pushHistory();
    const existingTrackIds = new Set(tracks.map((track) => track.id));
    const missingTracks = targetTracks
      .filter((track) => !existingTrackIds.has(track.id))
      .map((track) => createTrack(track.id, track.name, "text", track));
    if (missingTracks.length > 0) {
      tracks = [...tracks, ...missingTracks];
    }
    const preserved = clips.filter((clip) => !clip.file.startsWith(sourcePrefix));
    clips = [...preserved, ...nextClips.map((clip) => createClip(clip))];
    selectClip(nextClips[0]?.id ?? null);
    if (nextClips[0]) currentTime = nextClips[0].start;
    return true;
  }

  function addTextClipToTimeline(start = currentTime) {
    const textTrack = tracks.find((track) => track.type === "text");
    if (!textTrack || textTrack.locked) return;
    pushHistory();
    const clip = createTextClip({
      id: crypto.randomUUID(),
      trackId: textTrack.id,
      start: firstFreeStartAtOrAfter(textTrack.id, start, 5),
      duration: 5,
    });
    clips = [...clips, clip];
    selectClip(clip.id);
    currentTime = clip.start;
    void tick().then(() => onTextClipCreated?.());
  }

  function addTrack(type: "video" | "audio") {
    const prefix = type === "video" ? "V" : "A";
    const usedNumbers = new Set(
      tracks
        .filter((track) => track.type === type)
        .map((track) => Number(track.name.slice(1)))
        .filter((value) => Number.isInteger(value) && value > 0),
    );
    const nextNumber = Math.max(0, ...usedNumbers) + 1;
    const track = createTrack(
      `${type[0]}-${crypto.randomUUID()}`,
      `${prefix}${nextNumber}`,
      type,
    );
    pushHistory();
    const firstLaterType = tracks.findIndex((item) =>
      type === "video" ? item.type !== "video" : item.type === "text",
    );
    if (firstLaterType === -1) tracks = [...tracks, track];
    else {
      tracks = [
        ...tracks.slice(0, firstLaterType),
        track,
        ...tracks.slice(firstLaterType),
      ];
    }
  }

  export function setSelectedClipSpeed(speed: number) {
    if (!selectedClipId || !Number.isFinite(speed)) return;
    const snapshot = createSnapshot();
    try {
      const result = setClipSpeed(
        createProjectState(),
        selectedClipId,
        speed,
        "source",
        rippleMode,
      );
      pushHistory(snapshot);
      applyProjectState(result.state);
    } catch {
      // Invalid speed and locked tracks are rejected by the engine.
    }
  }

  export function updateSelectedTransition(
    side: TransitionSide,
    key: "type" | "duration",
    value: string,
  ) {
    if (!selectedClipId) return;
    commitClipTransitionChange(selectedClipId, side, key, value);
  }

  function updateClipVolumeFromDecibels(clipId: string, event: Event) {
    let decibels = (event.currentTarget as HTMLInputElement).valueAsNumber;
    if (!Number.isFinite(decibels)) return;
    // Reaching the bottom of the dB slider is a real mute, not a barely
    // audible -60 dB multiplier. The center also snaps cleanly to 0 dB.
    if (decibels <= MIN_EDITABLE_GAIN_DB) {
      updateClip(clipId, { volume: 0 }, { history: "coalesce" });
      return;
    }
    if (Math.abs(decibels) <= 0.15) decibels = 0;
    updateClip(
      clipId,
      { volume: decibelsToGain(decibels) },
      { history: "coalesce" },
    );
  }

  function handleClipVolumeGainPointerDown(event: PointerEvent, clip: Clip) {
    if (event.button !== 0 || isClipLocked(clip)) return;
    event.preventDefault();
    event.stopPropagation();
    selectClip(clip.id);

    const element = event.currentTarget as HTMLElement;
    clipVolumeDrag = {
      clipId: clip.id,
      pointerId: event.pointerId,
      startClientY: event.clientY,
      startVolume: clip.volume,
      volume: clip.volume,
      element,
      changed: false,
    };
    element.setPointerCapture?.(event.pointerId);
  }

  function handleClipVolumeGainPointerMove(event: PointerEvent, clip: Clip) {
    const drag = clipVolumeDrag;
    if (!drag || drag.pointerId !== event.pointerId || drag.clipId !== clip.id) return;

    event.preventDefault();
    event.stopPropagation();
    const startDb = getGainDbSliderValue(drag.startVolume);
    const pixelsPerDb = event.shiftKey ? 42 : 14;
    let nextDb = clamp(
      startDb + (drag.startClientY - event.clientY) / pixelsPerDb,
      MIN_EDITABLE_GAIN_DB,
      MAX_EDITABLE_GAIN_DB,
    );
    if (Math.abs(nextDb) <= 0.18) nextDb = 0;
    drag.volume =
      nextDb <= MIN_EDITABLE_GAIN_DB ? 0 : decibelsToGain(nextDb);
    drag.changed = Math.abs(drag.volume - drag.startVolume) >= 0.0005;
  }

  function finishClipVolumeGainDrag(event: PointerEvent, shouldCommit: boolean) {
    const drag = clipVolumeDrag;
    if (!drag || drag.pointerId !== event.pointerId) return;

    event.preventDefault();
    event.stopPropagation();
    if (drag.element.hasPointerCapture?.(event.pointerId)) {
      drag.element.releasePointerCapture(event.pointerId);
    }
    clipVolumeDrag = null;
    if (shouldCommit && drag.changed) {
      updateClip(drag.clipId, { volume: drag.volume }, { history: "immediate" });
    }
  }

  function cancelClipVolumeGainDrag() {
    const drag = clipVolumeDrag;
    if (drag?.element.hasPointerCapture?.(drag.pointerId)) {
      drag.element.releasePointerCapture(drag.pointerId);
    }
    clipVolumeDrag = null;
  }

  function toggleAudioRegionMode(event: PointerEvent | MouseEvent, clip: Clip) {
    event.preventDefault();
    event.stopPropagation();
    if (isClipLocked(clip)) return;
    selectClip(clip.id);
    audioRegionSelection = null;
    audioRegionModeClipId =
      audioRegionModeClipId === clip.id ? null : clip.id;
  }

  function getAudioStripTime(
    event: PointerEvent,
    clip: Clip,
    element: HTMLElement,
  ): number {
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0) return 0;
    return clamp(
      ((event.clientX - rect.left) / rect.width) * clip.duration,
      0,
      clip.duration,
    );
  }

  function handleAudioRegionPointerDown(event: PointerEvent, clip: Clip) {
    if (
      audioRegionModeClipId !== clip.id ||
      isClipLocked(clip) ||
      event.button !== 0
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const element = event.currentTarget as HTMLElement;
    const anchorTime = getAudioStripTime(event, clip, element);
    selectClip(clip.id);
    audioRegionSelection = {
      clipId: clip.id,
      start: anchorTime,
      end: anchorTime,
      // Default to a clearly quieter level even when the whole clip is
      // already below 35%; matching the base volume would only add yellow
      // keyframes without producing a visible/effective region.
      volume: clip.volume <= 0 ? 0 : clip.volume * 0.35,
    };
    audioRegionDrag = {
      clipId: clip.id,
      pointerId: event.pointerId,
      anchorTime,
      element,
    };
    element.setPointerCapture?.(event.pointerId);
    currentTime = clip.start + anchorTime;
    onScrubBegin?.();
  }

  function handleAudioRegionPointerMove(event: PointerEvent, clip: Clip) {
    const drag = audioRegionDrag;
    if (
      !drag ||
      drag.clipId !== clip.id ||
      drag.pointerId !== event.pointerId ||
      !audioRegionSelection
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const time = getAudioStripTime(event, clip, drag.element);
    audioRegionSelection.start = Math.min(drag.anchorTime, time);
    audioRegionSelection.end = Math.max(drag.anchorTime, time);
    currentTime = clip.start + time;
  }

  function finishAudioRegionSelection(event: PointerEvent, clip: Clip) {
    const drag = audioRegionDrag;
    if (
      !drag ||
      drag.clipId !== clip.id ||
      drag.pointerId !== event.pointerId
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();

    if (audioRegionSelection) {
      const regionLength = audioRegionSelection.end - audioRegionSelection.start;
      if (regionLength < 0.1) {
        const fallbackLength = Math.min(1, clip.duration);
        let start = clamp(
          drag.anchorTime - fallbackLength / 2,
          0,
          Math.max(0, clip.duration - fallbackLength),
        );
        let end = Math.min(clip.duration, start + fallbackLength);
        start = Math.max(0, end - fallbackLength);
        audioRegionSelection.start = start;
        audioRegionSelection.end = end;
      }
    }

    audioRegionDrag = null;
    audioRegionModeClipId = null;
    if (drag.element.hasPointerCapture?.(event.pointerId)) {
      drag.element.releasePointerCapture(event.pointerId);
    }
  }

  // Lets a drafted (or reopened) region be fine-tuned edge-by-edge after the
  // rough drag-select, instead of forcing a full redo for small corrections.
  function handleAudioRegionHandlePointerDown(
    event: PointerEvent,
    clip: Clip,
    edge: "start" | "end",
  ) {
    if (
      isClipLocked(clip) ||
      event.button !== 0 ||
      !audioRegionSelection ||
      audioRegionSelection.clipId !== clip.id
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget as HTMLElement;
    const stripElement = handle.closest(".clip-audio-strip") as HTMLElement | null;
    if (!stripElement) return;
    audioRegionResizeDrag = {
      clipId: clip.id,
      edge,
      pointerId: event.pointerId,
      stripElement,
    };
    handle.setPointerCapture?.(event.pointerId);
    onScrubBegin?.();
  }

  function handleAudioRegionHandlePointerMove(event: PointerEvent, clip: Clip) {
    const drag = audioRegionResizeDrag;
    if (
      !drag ||
      drag.pointerId !== event.pointerId ||
      drag.clipId !== clip.id ||
      !audioRegionSelection
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const time = getAudioStripTime(event, clip, drag.stripElement);
    if (drag.edge === "start") {
      const maxStart = audioRegionSelection.end - MIN_AUDIO_REGION_DURATION;
      audioRegionSelection.start = clamp(time, 0, Math.max(0, maxStart));
    } else {
      const minEnd = audioRegionSelection.start + MIN_AUDIO_REGION_DURATION;
      audioRegionSelection.end = clamp(time, Math.min(clip.duration, minEnd), clip.duration);
    }
    currentTime = clip.start + time;
  }

  function finishAudioRegionResize(event: PointerEvent) {
    const drag = audioRegionResizeDrag;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    audioRegionResizeDrag = null;
    const handle = event.currentTarget as HTMLElement;
    if (handle.hasPointerCapture?.(event.pointerId)) {
      handle.releasePointerCapture(event.pointerId);
    }
  }

  function handleAudioRegionGainPointerDown(
    event: PointerEvent,
    clip: Clip,
    appliedRegion?: QuietRegion,
  ) {
    if (event.button !== 0 || isClipLocked(clip) || clip.volume <= 0) return;

    const selection = appliedRegion
      ? {
          clipId: clip.id,
          start: appliedRegion.start,
          end: appliedRegion.end,
          volume: appliedRegion.volume,
        }
      : audioRegionSelection?.clipId === clip.id
        ? audioRegionSelection
        : null;
    if (!selection) return;

    event.preventDefault();
    event.stopPropagation();
    selectClip(clip.id);
    audioRegionModeClipId = null;
    audioRegionSelection = selection;

    const element = event.currentTarget as HTMLElement;
    audioRegionGainDrag = {
      clipId: clip.id,
      pointerId: event.pointerId,
      startClientY: event.clientY,
      startVolume: selection.volume,
      element,
      changed: false,
    };
    element.setPointerCapture?.(event.pointerId);
  }

  function handleAudioRegionGainPointerMove(event: PointerEvent, clip: Clip) {
    const drag = audioRegionGainDrag;
    const selection = audioRegionSelection;
    if (
      !drag ||
      drag.pointerId !== event.pointerId ||
      drag.clipId !== clip.id ||
      selection?.clipId !== clip.id
    ) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    const fineScale = event.shiftKey ? 0.25 : 1;
    const upwardPixels = (drag.startClientY - event.clientY) * fineScale;
    const maximum = Math.max(0, clip.volume - 0.01);
    const nextVolume = Math.min(
      maximum,
      getVerticalVolumeDragValue(
        drag.startVolume,
        clip.volume,
        upwardPixels,
      ),
    );
    selection.volume = nextVolume;
    drag.changed = Math.abs(nextVolume - drag.startVolume) >= 0.002;
  }

  function finishAudioRegionGainDrag(
    event: PointerEvent,
    shouldCommit: boolean,
  ) {
    const drag = audioRegionGainDrag;
    if (!drag || drag.pointerId !== event.pointerId) return;

    event.preventDefault();
    event.stopPropagation();
    const selection = audioRegionSelection;
    if (drag.element.hasPointerCapture?.(event.pointerId)) {
      drag.element.releasePointerCapture(event.pointerId);
    }
    audioRegionGainDrag = null;

    if (!selection || selection.clipId !== drag.clipId) return;
    if (!shouldCommit) {
      selection.volume = drag.startVolume;
      return;
    }
    if (drag.changed) commitAudioRegion(selection.volume);
  }

  export function toggleAudioRegionSelectMode() {
    const clip = selectedTimelineClip;
    if (!clip || isClipLocked(clip) || clip.audioSeparated) return;
    audioRegionSelection = null;
    audioRegionModeClipId = audioRegionModeClipId === clip.id ? null : clip.id;
  }

  export function cancelAudioRegionEdit() {
    const drag = audioRegionDrag;
    if (drag?.element.hasPointerCapture?.(drag.pointerId)) {
      drag.element.releasePointerCapture(drag.pointerId);
    }
    const gainDrag = audioRegionGainDrag;
    if (gainDrag?.element.hasPointerCapture?.(gainDrag.pointerId)) {
      gainDrag.element.releasePointerCapture(gainDrag.pointerId);
    }
    audioRegionDrag = null;
    audioRegionGainDrag = null;
    audioRegionModeClipId = null;
    audioRegionSelection = null;
  }

  export function setAudioRegionVolume(volume: number) {
    if (!audioRegionSelection || !Number.isFinite(volume)) return;
    audioRegionSelection.volume = clamp(volume, 0, 2);
  }

  function commitAudioRegion(volume: number, fadeDuration = 0.18) {
    const region = audioRegionSelection;
    if (!region) return;
    const clip = clips.find((item) => item.id === region.clipId);
    if (!clip || isClipLocked(clip)) return;

    const start = clamp(Math.min(region.start, region.end), 0, clip.duration);
    const end = clamp(Math.max(region.start, region.end), start, clip.duration);
    if (end - start < TIMELINE_EPSILON) return;

    const snapshot = createSnapshot();
    try {
      const nextState = setClipVolumeRegion(
        createProjectState(),
        clip.id,
        start,
        end,
        volume,
        fadeDuration,
      );
      pushHistory(snapshot);
      applyProjectState(nextState);
      audioRegionSelection = null;
      audioRegionModeClipId = null;
    } catch {
      // Invalid or locked selections leave the draft untouched for correction.
    }
  }

  export function applyAudioRegionVolume() {
    if (!audioRegionSelection) return;
    const clip = clips.find((item) => item.id === audioRegionSelection?.clipId);
    if (!clip || audioRegionSelection.volume >= clip.volume - 0.01) return;
    commitAudioRegion(audioRegionSelection.volume);
  }

  /**
   * Volume regions aren't stored as their own entity — `setClipVolumeRegion`
   * just writes a flat plateau of keyframes. This reconstructs the visible
   * "quiet region" list from that plateau so it stays visible (and
   * clickable) after the draft selection is gone.
   */
  function getQuietRegions(clip: Clip): QuietRegion[] {
    return getClipVolumeRegions(clip);
  }

  function openQuietRegionEditor(
    event: PointerEvent | MouseEvent,
    clip: Clip,
    region: QuietRegion,
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (isClipLocked(clip)) return;
    selectClip(clip.id);
    audioRegionModeClipId = null;
    audioRegionSelection = {
      clipId: clip.id,
      start: region.start,
      end: region.end,
      volume: region.volume,
    };
  }

  export function openQuietRegion(region: QuietRegion) {
    const clip = selectedTimelineClip;
    if (!clip || isClipLocked(clip)) return;
    audioRegionModeClipId = null;
    audioRegionSelection = {
      clipId: clip.id,
      start: region.start,
      end: region.end,
      volume: region.volume,
    };
  }

  export function deleteSelectedClip() {
    if (!selectedClipId) return;
    deleteClipById(selectedClipId, rippleMode);
  }

  export function removeAudioRegion() {
    const region = audioRegionSelection;
    if (!region) return;
    const clip = clips.find((item) => item.id === region.clipId);
    if (!clip) return;
    const overlapsQuietRegion = getQuietRegions(clip).some(
      (quiet) =>
        Math.min(region.start, region.end) < quiet.end - TIMELINE_EPSILON &&
        Math.max(region.start, region.end) > quiet.start + TIMELINE_EPSILON,
    );
    if (!overlapsQuietRegion) {
      cancelAudioRegionEdit();
      return;
    }
    commitAudioRegion(clip.volume, 0);
  }

  $effect(() => {
    if (
      audioRegionSelection &&
      selectedClipId !== audioRegionSelection.clipId &&
      !audioRegionDrag
    ) {
      audioRegionSelection = null;
    }
    if (
      audioRegionModeClipId &&
      selectedClipId !== audioRegionModeClipId &&
      !audioRegionDrag
    ) {
      audioRegionModeClipId = null;
    }
  });

  function addKeyframeAtPlayhead(property: AnimatableProperty) {
    if (!selectedClipId) return;
    const clip = clips.find((item) => item.id === selectedClipId);
    if (!clip) return;
    if (tracks.find((track) => track.id === clip.trackId)?.locked) return;
    const localTime = clamp(currentTime - clip.start, 0, clip.duration);
    const value =
      property === "volume"
        ? clip.volume
        : property === "pan"
          ? clip.pan
          : (clip.transform[property as keyof Clip["transform"]] ??
            (property === "scale" || property === "opacity" ? 1 : 0));
    const snapshot = createSnapshot();
    const nextState = upsertKeyframe(createProjectState(), clip.id, property, {
      time: localTime,
      value,
      easing: "linear",
    });
    pushHistory(snapshot);
    applyProjectState(nextState);
  }

  function getWaveformPath(waveform: number[], volume: number = 1): string {
    if (waveform.length === 0) return "";
    return waveform
      .map((peak, index) => {
        const x = waveform.length === 1 ? 0 : (index / (waveform.length - 1)) * 100;
        const halfHeight = clamp(peak * volume, 0, 1.2) * 20;
        return `M ${x.toFixed(2)} ${(24 - halfHeight).toFixed(2)} V ${(24 + halfHeight).toFixed(2)}`;
      })
      .join(" ");
  }

  function getVoiceRiderPolyline(clip: Clip): string {
    const frames = clip.keyframes.riderGain ?? [];
    if (frames.length < 2 || clip.duration <= 0) return "";
    return frames
      .map((frame) => {
        const x = clamp((frame.time / clip.duration) * 100, 0, 100);
        const gainDb = 20 * Math.log10(Math.max(frame.value, 0.0001));
        const y = 24 - (clamp(gainDb, -12, 12) / 12) * 18;
        return `${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(" ");
  }

  function getVisibleBeatMarkers(clip: Clip) {
    if (!clip.beatAnalysis) return [];
    const projected = projectSourceBeats(clip, clip.beatAnalysis.markers).map(
      (marker) => ({ ...marker, localTime: marker.timelineTime - clip.start }),
    );
    if (projected.length <= 500) return projected;
    const stride = Math.ceil(projected.length / 500);
    return projected.filter((marker, index) => marker.downbeat || index % stride === 0);
  }

  function getManualKeyframeMarkers(clip: Clip): TimelineKeyframe[] {
    return Object.entries(clip.keyframes)
      .filter(([property]) => property !== "riderGain")
      .flatMap(([, frames]) => frames ?? []);
  }

  function splitClipAtTime(time: number, clipId?: string) {
    const clipToSplit = clipId
      ? clips.find((clip) => clip.id === clipId)
      : getActiveClipAtTime(time);

    if (!clipToSplit) return;

    if (
      time <= clipToSplit.start + 0.1 ||
      time >= clipToSplit.start + clipToSplit.duration - 0.1
    ) {
      return;
    }

    const snapshot = createSnapshot();
    try {
      const result = splitTimelineClip(
        createProjectState(),
        clipToSplit.id,
        time,
        crypto.randomUUID(),
      );
      pushHistory(snapshot);
      applyProjectState(result.state);
    } catch {
      // The engine rejects boundary splits and edits on locked tracks.
    }
  }

  function splitClip() {
    splitClipAtTime(currentTime);
  }

  function splitSelectedClipAtPlayhead() {
    if (!canSplitSelectedClip || !selectedClipId) return;
    splitClipAtTime(currentTime, selectedClipId);
  }

  onMount(() => {
    if (!isTauri()) return;

    const appWindow = getCurrentWindow();

    let scaleFactor = 1;
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    (async () => {
      scaleFactor = await appWindow.scaleFactor();
      if (cancelled) return;

      unlisten = await appWindow.onDragDropEvent(async (event) => {
        if (!timelineRef) return;

        if (event.payload.type === "enter") {
          const mediaPaths = (event.payload.paths ?? []).filter((path) =>
            SUPPORTED_MEDIA_RE.test(path),
          );
          osDragPaths = mediaPaths.length ? mediaPaths : null;
          dragSource = "os";
          return;
        }

        if (event.payload.type === "over") {
          if (!osDragPaths?.length) return;
          const logical = event.payload.position.toLogical(scaleFactor);

          const hasDropTarget = updateDragPreviewFromPoint(
            logical.x,
            logical.y,
            osDragPaths[0] ?? null,
          );
          if (!hasDropTarget) {
            isDragActive = false;
            return;
          }

          dragSource = "os";
          isDragActive = true;
          return;
        }

        if (event.payload.type === "drop") {
          const mediaPaths = (event.payload.paths ?? []).filter((path) =>
            SUPPORTED_MEDIA_RE.test(path),
          );
          const logical = event.payload.position.toLogical(scaleFactor);

          const dropInfo = resolveDropInfo(logical.x, logical.y);
          const targetTrack = tracks.find((track) => track.id === dropInfo?.trackId);
          const compatiblePaths = targetTrack
            ? mediaPaths.filter((path) => canPlaceFileOnTrack(path, targetTrack))
            : [];
          if (!dropInfo?.trackId || !targetTrack || compatiblePaths.length === 0) {
            osDragPaths = null;
            clearDragState();
            return;
          }

          const baseStart = dropInfo.start;
          const trackId = dropInfo.trackId;

          const durations = await Promise.all(
            compatiblePaths.map((path) =>
              IMAGE_FILE_RE.test(path) ? Promise.resolve(5) : getVideoDuration(path),
            ),
          );
          let currentStart = baseStart;
          const newClips: Clip[] = compatiblePaths.map((path, idx) => {
            const clipDuration =
              durations[idx] && Number.isFinite(durations[idx])
                ? durations[idx]
                : DEFAULT_CLIP_DURATION;
            const start = clampClipStart(currentStart, clipDuration);
            currentStart = start + clipDuration;
            return createClip({
              id: crypto.randomUUID(),
              trackId,
              file: path,
              kind: inferClipKind(path, targetTrack),
              start,
              duration: clipDuration,
              sourceDuration: clipDuration,
            });
          });

          pushHistory();
          clips = [...clips, ...newClips];
          for (const clip of newClips) void hydrateWaveform(clip.id, clip.file);
          osDragPaths = null;
          clearDragState();
          return;
        }

        if (event.payload.type === "leave") {
          osDragPaths = null;
          clearDragState();
        }
      });
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  });
</script>

<svelte:window
  ondragover={handleWindowDragOver}
  ondrop={handleWindowDrop}
  ondragleave={handleWindowDragLeave}
  onpointermove={handleWindowPointerMove}
  onpointerup={handleWindowPointerUp}
  onpointercancel={handleWindowPointerCancel}
  onkeydown={handleWindowKeydown}
  onpointerdown={(event) => {
    if (
      toolHelpOpen &&
      !(event.target as HTMLElement)?.closest(".tool-help")
    ) {
      closeToolHelp();
    }
  }}
/>

<div class="timeline">
  <div class="timeline-toolbar">
    <div
      class="timeline-quick-actions"
      role="toolbar"
      aria-label="Hızlı zaman çizelgesi düzenleme"
    >
      <button
        type="button"
        class="quick-action icon-only"
        title="Geri al (Ctrl+Z)"
        aria-label="Geri al"
        disabled={undoStack.length === 0}
        onclick={undo}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M9 7 5 11l4 4"></path>
          <path d="M5 11h8a6 6 0 0 1 6 6"></path>
        </svg>
      </button>
      <button
        type="button"
        class="quick-action icon-only"
        title="İleri al (Ctrl+Y)"
        aria-label="İleri al"
        disabled={redoStack.length === 0}
        onclick={redo}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m15 7 4 4-4 4"></path>
          <path d="M19 11h-8a6 6 0 0 0-6 6"></path>
        </svg>
      </button>
      <span class="quick-separator" aria-hidden="true"></span>
      <button
        type="button"
        class="quick-action"
        title="Seçili klibi oynatma kafasında böl (S)"
        aria-label="Seçili klibi oynatma kafasında böl"
        disabled={!canSplitSelectedClip}
        onclick={splitSelectedClipAtPlayhead}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="6" cy="7" r="2.5"></circle>
          <circle cx="6" cy="17" r="2.5"></circle>
          <path d="m8.2 8.2 10.3 7.3M8.2 15.8 18.5 8.5"></path>
        </svg>
        <span>Böl</span>
      </button>
      <button
        type="button"
        class="quick-action danger"
        title={rippleMode
          ? "Seçili klibi sil ve boşluğu kapat (Delete)"
          : "Seçili klibi sil (Delete)"}
        aria-label={rippleMode
          ? "Seçili klibi sil ve boşluğu kapat"
          : "Seçili klibi sil"}
        disabled={!canDeleteSelectedClip}
        onclick={deleteSelectedClip}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13M10 11v5M14 11v5"></path>
        </svg>
        <span>Sil</span>
      </button>
      <button
        type="button"
        class="quick-action cancel"
        class:active={hasActiveTimelineInteraction}
        title={hasActiveTimelineInteraction
          ? "Aktif işlemi iptal et (Esc)"
          : "Klip seçimini bırak (Esc)"}
        aria-label={hasActiveTimelineInteraction
          ? "Aktif işlemi iptal et"
          : "Klip seçimini bırak"}
        disabled={!canCancelTimelineAction}
        onclick={cancelActiveTimelineOperation}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m7 7 10 10M17 7 7 17"></path>
        </svg>
        <span>İptal</span>
      </button>
    </div>
    <span class="toolbar-divider" aria-hidden="true"></span>
    <button
      class:active={rippleMode}
      class="mode-btn"
      title="Ripple: Açıkken taşıdığınız klibin eski boşluğunu kapatır ve kesim çizgisine bırakınca klibi araya ekleyip sağ tarafı kaydırır."
      aria-pressed={rippleMode}
      onclick={() => (rippleMode = !rippleMode)}
      >Ripple: {rippleMode ? "Açık" : "Kapalı"}</button
    >
    <button
      class="mode-btn"
      title="Metin: Oynatma kafasında düzenlenebilir bir yazı klibi oluşturur."
      onclick={() => addTextClipToTimeline()}
      >+ Metin</button
    >
    <button
      class="mode-btn"
      title="Video kanalı: Üst katmanda görüntü bindirmek ve klipleri aşağı/yukarı taşımak için yeni bir video katmanı ekler."
      onclick={() => addTrack("video")}
      >+ Video</button
    >
    <button
      class="mode-btn"
      title="Ses kanalı: Ayrı sesleri birlikte mikslemek için yeni bir ses kanalı ekler."
      onclick={() => addTrack("audio")}
      >+ Ses</button
    >
    <div class="tool-help">
      <button
        type="button"
        class="tool-help-trigger"
        class:active={toolHelpOpen}
        title="Araçlar ve taşıma ipuçları"
        aria-label="Araçlar ve taşıma ipuçları"
        aria-expanded={toolHelpOpen}
        bind:this={toolHelpTriggerRef}
        onclick={toggleToolHelp}
      >?</button>
      {#if toolHelpOpen}
        <div
          class="tool-help-body"
          style={`position: fixed; left: ${toolHelpPos.left}px; bottom: ${toolHelpPos.bottom}px;`}
        >
          <p><strong>Taşı:</strong> Klibi tutup aynı tür kanala veya sığdığı boşluğa sürükleyin.</p>
          <p><strong>Araya ekle:</strong> Ripple açıkken klibi bir kesim çizgisine bırakın; hedef boşluk kullanılır ve sağdaki klipler gereken kadar yer açar.</p>
          <p><strong>Ripple:</strong> Taşıma, kırpma veya silmeden sonra kaynak boşluğunu kapatır. Kapalıyken yalnız boş bir alana normal taşıma yapılır.</p>
          <p><strong>+ Video:</strong> Yeni görüntü katmanı; üst kanal alttakinin üzerinde görünür.</p>
          <p><strong>+ Ses:</strong> Yeni miks kanalı. <strong>+ Metin:</strong> Yazı klibi ekler.</p>
          <p><strong>× / kanal sağ tık:</strong> Eklenen video veya ses kanalını kaldırır.</p>
          <p><strong>M / S / L:</strong> Gizle-sustur / Solo / Kilitle.</p>
          <p><strong>Sağ tık:</strong> Böl, kopyala, çoğalt, sil ve sıfırla işlemleri.</p>
        </div>
      {/if}
    </div>
    <span class="toolbar-spacer"></span>
    <div class="zoom-tools">
      <button class="tool-btn" title="Uzaklaştır" onclick={() => (zoom = Math.max(1, zoom - 0.25))}>−</button>
      <span class="zoom-label">{Math.round(zoom * 100)}%</span>
      <button class="tool-btn" title="Yakınlaştır" onclick={() => (zoom = Math.min(4, zoom + 0.25))}>+</button>
    </div>
  </div>

  <div class="timeline-body">
  <div
    class="track-headers"
    role="toolbar"
    tabindex="0"
    aria-label="Kanal kontrolleri"
    bind:this={trackHeadersRef}
    ondragenter={handleDragEnter}
    ondragover={handleDragOver}
    ondragleave={handleDragLeave}
    ondrop={handleTracksDrop}
  >
    {#each tracks as track}
      <div
        class="track-header"
        class:drop-target={dragPreview?.trackId === track.id}
        class:invalid-drop-target={dragPreview?.trackId === track.id && !dragPreview.isAllowed}
        class:clip-move-target={clipDragHasMoved && clipMovePreview?.trackId === track.id && clipMovePreview?.isAllowed}
        class:insert-target={clipDragHasMoved && clipMovePreview?.trackId === track.id && clipMovePreview?.mode === "insert" && clipMovePreview?.isAllowed}
        class:invalid-clip-move-target={clipDragHasMoved && clipMovePreview?.trackId === track.id && !clipMovePreview?.isAllowed}
        class:locked={track.locked}
        data-type={track.type}
        role="group"
        aria-label={`${track.name} kanal başlığı`}
        oncontextmenu={(event) => openTrackContextMenu(event, track.id)}
      >
        <span class="track-name">{track.name}</span>
        <div class="track-switches" aria-label={`${track.name} kanal kontrolleri`}>
          <button
            class:active={track.muted}
            class="track-switch mute"
            title={track.type === "audio"
              ? "Sustur (M): Bu kanalın sesini önizleme ve dışa aktarmada kapatır."
              : "Gizle (M): Bu görsel katmanı önizleme ve dışa aktarmada gizler."}
            aria-label={`${track.name} ${track.type === "audio" ? "sustur" : "gizle"}`}
            onclick={(event) => {
              event.stopPropagation();
              toggleTrackFlag(track.id, "muted");
            }}>M</button
          >
          <button
            class:active={track.solo}
            class="track-switch solo"
            title={track.type === "audio"
              ? "Solo (S): Yalnız solo seçilmiş ses kanallarını dinletir."
              : "Solo (S): Yalnız bu görsel katmanı görünür tutar."}
            aria-label={`${track.name} solo`}
            onclick={(event) => {
              event.stopPropagation();
              toggleTrackFlag(track.id, "solo");
            }}>S</button
          >
          <button
            class:active={track.locked}
            class="track-switch lock"
            title="Kilitle (L): Bu kanaldaki taşıma, kırpma, bölme ve silme işlemlerini engeller."
            aria-label={`${track.name} kilit`}
            onclick={(event) => {
              event.stopPropagation();
              toggleTrackFlag(track.id, "locked");
            }}>L</button
          >
          {#if track.type !== "text"}
            {@const removal = getTrackRemovalState(track.id)}
            <button
              class="track-switch remove"
              disabled={!removal.allowed}
              title={removal.reason}
              aria-label={`${track.name} kanalını kaldır`}
              onclick={(event) => {
                event.stopPropagation();
                removeTrackById(track.id);
              }}>×</button
            >
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <div class="track-area">
    <div class="ruler" style={`width: ${Math.max(1, zoom) * 100}%`}>
      {#each getTimeMarkers() as time}
        <div class="tick" style="left: {(time / viewDuration) * 100}%">
          <span class="tick-label">{formatTime(time)}</span>
        </div>
      {/each}
    </div>

    <!-- Pointer scrubbing and focused arrow-key stepping are implemented together. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
    <div
      class="tracks"
      bind:this={timelineRef}
      class:drag-active={isDragActive}
      class:clip-dragging={clipDragHasMoved}
      class:cut-mode={$activeTool === "cut"}
      style={`width: ${Math.max(1, zoom) * 100}%`}
      onclick={handleTimelineClick}
      onkeydown={handleTimelineKeydown}
      tabindex="0"
      onpointerdown={handleScrubStart}
      ondragenter={handleDragEnter}
      ondragover={handleDragOver}
      ondragleave={handleDragLeave}
      ondrop={handleTracksDrop}
      oncontextmenu={openTimelineContextMenu}
      role="application"
      aria-label="Zaman çizelgesi"
      aria-keyshortcuts="ArrowLeft ArrowRight Shift+ArrowLeft Shift+ArrowRight Alt+ArrowLeft Alt+ArrowRight"
      title="←/→: 1 sn · Shift: 5 sn · Alt: 1 kare"
    >
      {#each tracks as track}
        <div
          class="track"
          class:drop-target={dragPreview?.trackId === track.id}
          class:invalid-drop-target={dragPreview?.trackId === track.id && !dragPreview.isAllowed}
          class:clip-move-target={clipDragHasMoved && clipMovePreview?.trackId === track.id && clipMovePreview?.isAllowed}
          class:insert-target={clipDragHasMoved && clipMovePreview?.trackId === track.id && clipMovePreview?.mode === "insert" && clipMovePreview?.isAllowed}
          class:invalid-clip-move-target={clipDragHasMoved && clipMovePreview?.trackId === track.id && !clipMovePreview?.isAllowed}
          class:muted={track.muted}
          class:locked={track.locked}
          data-type={track.type}
          data-id={track.id}
          role="region"
          aria-label="Timeline Track"
        >
          <!-- Clips -->
          {#each clips.filter((c) => c.trackId === track.id) as clip (clip.id)}
            <div
              class="clip"
              style="left: {(clip.start / viewDuration) *
                100}%; width: {(clip.duration / viewDuration) * 100}%"
              style:transform={getRipplePreviewTransform(clip)}
              title={clip.file}
              onpointerdown={(e) => handleClipPointerDown(e, clip)}
              oncontextmenu={(e) => openClipContextMenu(e, clip)}
              onclick={(e) => {
                e.stopPropagation(); // Prevent timeline seek
                if ($activeTool === "cut") {
                  const splitTime = getTimeFromClientX(e.clientX);
                  currentTime = splitTime;
                  splitClipAtTime(splitTime, clip.id);
                  return;
                }
                if ($activeTool !== "select") return;
                selectClip(clip.id);
              }}
              class:selected={selectedClipId === clip.id}
              class:pressed={clipDragId === clip.id && !clipDragHasMoved}
              class:drag-origin={clipDragId === clip.id && clipDragHasMoved}
              class:ripple-close-preview={isRipplePreviewShift(clip, "left")}
              class:insert-shift-preview={isRipplePreviewShift(clip, "right")}
              class:text-clip={clip.kind === "text"}
              class:audio-clip={clip.kind === "audio"}
              class:video-clip={clip.kind === "video"}
              role="button"
              tabindex="0"
              aria-grabbed={clipDragId === clip.id}
              onkeydown={(e) => {
                if (e.key !== "Enter") return;
                selectClip(clip.id);
              }}
            >
              {#if clip.kind === "video" || clip.kind === "image"}
                <FilmstripThumbnail
                  filePath={clip.file}
                  sourceDuration={clip.duration * clip.speed}
                  clipWidth={timelineRef
                    ? (clip.duration / viewDuration) * timelineRef.clientWidth
                    : 100}
                  videoOffset={clip.trimIn}
                />
              {/if}
              {#if autoReframeJob?.clipId === clip.id}
                <button
                  type="button"
                  class="clip-auto-reframe-job"
                  class:completed={autoReframeJob.status === "completed"}
                  class:error={autoReframeJob.status === "error"}
                  title={`${autoReframeJob.message} · Ayrıntıları aç`}
                  aria-label={autoReframeJob.status === "completed"
                    ? "Akıllı kadraj analizi hazır. Ayrıntıları aç."
                    : autoReframeJob.status === "error"
                      ? `Akıllı kadraj analizi başarısız: ${autoReframeJob.message}`
                      : `Akıllı kadraj analizi yüzde ${Math.round(autoReframeJob.progress)}`}
                  onpointerdown={(event) => event.stopPropagation()}
                  onclick={(event) => {
                    event.stopPropagation();
                    onAutoReframeJobOpen?.(clip.id);
                  }}
                >
                  <span class="clip-auto-reframe-label">
                    {autoReframeJob.status === "completed"
                      ? "✓ AI Kadraj hazır"
                      : autoReframeJob.status === "error"
                        ? "! AI Kadraj hata"
                        : `${autoReframeJob.status === "preparing" ? "AI hazırlanıyor" : "AI Kadraj"} · %${Math.round(autoReframeJob.progress)}`}
                  </span>
                  {#if autoReframeJob.status !== "error"}
                    <span
                      class="clip-auto-reframe-rail"
                      role="progressbar"
                      aria-label="Klip akıllı kadraj ilerlemesi"
                      aria-valuemin="0"
                      aria-valuemax="100"
                      aria-valuenow={Math.round(autoReframeJob.progress)}
                    >
                      <span
                        class="clip-auto-reframe-fill"
                        style:width={`${Math.max(0, Math.min(100, autoReframeJob.progress))}%`}
                      ></span>
                    </span>
                  {/if}
                </button>
              {/if}
              {#if clip.waveform.length > 0}
                <svg
                  class="waveform"
                  viewBox="0 0 100 48"
                  preserveAspectRatio="none"
                  aria-label="Ses dalga formu"
                >
                  <path d={getWaveformPath(clip.waveform, clip.volume)} />
                </svg>
              {/if}
              {#if clip.beatAnalysis && isBeatAnalysisRangeCurrent(clip, clip.beatAnalysis)}
                {#if selectedClipId === clip.id}
                  <div class="beat-grid" aria-label="AI beat markerları">
                    {#each getVisibleBeatMarkers(clip) as marker (marker.sourceMs)}
                      <span
                        class:downbeat={marker.downbeat}
                        style:left={`${(marker.localTime / clip.duration) * 100}%`}
                        title={`${marker.downbeat ? "Downbeat" : "Beat"} · güven %${Math.round(marker.confidence * 100)}`}
                      ></span>
                    {/each}
                  </div>
                {/if}
                <span
                  class="beat-badge"
                  title={`${clip.beatAnalysis.bpm?.toFixed(1) ?? "?"} BPM · ${clip.beatAnalysis.markers.length} marker · Beat This! AI`}
                >♫ {clip.beatAnalysis.bpm?.toFixed(0) ?? "AI"}</span>
              {/if}
              {#if selectedClipId === clip.id && speechSuggestions.length > 0}
                <div class="speech-suggestion-grid" aria-label="Konuşma temizliği önerileri">
                  {#each speechSuggestions as suggestion (suggestion.id)}
                    <span
                      class:silence={suggestion.kind === "silence"}
                      class:filler={suggestion.kind === "filler-word"}
                      class:possible={suggestion.kind === "possible-filler-sound"}
                      class:contextual={suggestion.kind === "contextual-word"}
                      style:left={`${(suggestion.startMs / 1_000 / clip.duration) * 100}%`}
                      style:width={`${((suggestion.endMs - suggestion.startMs) / 1_000 / clip.duration) * 100}%`}
                      title={`${suggestion.kind} · güven %${Math.round(suggestion.confidence * 100)}`}
                    ></span>
                  {/each}
                </div>
              {/if}
              {#if clip.voiceRider && (clip.keyframes.riderGain?.length ?? 0) > 1}
                <svg
                  class="ai-rider-curve"
                  viewBox="0 0 100 48"
                  preserveAspectRatio="none"
                  aria-label="AI Voice Rider ses eğrisi"
                >
                  <polyline points={getVoiceRiderPolyline(clip)} />
                </svg>
                <span
                  class="ai-rider-badge"
                  title={`Silero Neural VAD · konuşma %${Math.round(clip.voiceRider.speechCoverage * 100)} · o an ${Math.round(evaluateKeyframes(clip.keyframes.riderGain, clamp(currentTime - clip.start, 0, clip.duration), 1) * 100)}%`}
                >AI</span>
              {/if}
              {#each getQuietRegions(clip) as region (region.start)}
                <span
                  class="audio-region-applied-overlay"
                  style:left={`${(region.start / clip.duration) * 100}%`}
                  style:width={`${((region.end - region.start) / clip.duration) * 100}%`}
                  title={`Uygulanmış ses bölgesi: ${formatTime(region.start)} – ${formatTime(region.end)}, ${formatRelativeGainDb(region.volume, clip.volume)} (%${Math.round((region.volume / Math.max(clip.volume, 0.0001)) * 100)})`}
                  aria-hidden="true"
                >
                  <span class="audio-region-applied-value"
                    >Ses {formatRelativeGainDb(region.volume, clip.volume)}</span
                  >
                </span>
              {/each}
              {#if audioRegionSelection?.clipId === clip.id}
                <button
                  type="button"
                  class="audio-region-gain-zone"
                  data-testid="audio-region-gain-zone"
                  class:dragging={audioRegionGainDrag?.clipId === clip.id}
                  style:left={`${(audioRegionSelection.start / clip.duration) * 100}%`}
                  style:width={`${((audioRegionSelection.end - audioRegionSelection.start) / clip.duration) * 100}%`}
                  disabled={isClipLocked(clip) || clip.volume <= 0}
                  title="Yukarı sürükle: sesi artır · Aşağı sürükle: sesi azalt · Shift: hassas ayar"
                  aria-label={`Bölge sesi ${formatRelativeGainDb(audioRegionSelection.volume, clip.volume)}. Yukarı veya aşağı sürükleyin.`}
                  onpointerdown={(event) =>
                    handleAudioRegionGainPointerDown(event, clip)}
                  onpointermove={(event) =>
                    handleAudioRegionGainPointerMove(event, clip)}
                  onpointerup={(event) =>
                    finishAudioRegionGainDrag(event, true)}
                  onpointercancel={(event) =>
                    finishAudioRegionGainDrag(event, false)}
                  onclick={(event) => event.stopPropagation()}
                >
                  <span class="audio-region-gain-value"
                    >↕ Ses {formatRelativeGainDb(audioRegionSelection.volume, clip.volume)}</span
                  >
                </button>
              {/if}
              {#if (clip.kind === "video" && !clip.audioSeparated) || clip.kind === "audio"}
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <div
                  class="clip-audio-strip"
                  class:has-waveform={clip.waveform.length > 0}
                  class:region-mode={audioRegionModeClipId === clip.id}
                  class:gain-dragging={clipVolumeDrag?.clipId === clip.id}
                  title={audioRegionModeClipId === clip.id
                    ? "Kısılacak ses bölgesini sürükleyerek seçin"
                    : `Klip kazancı: ${formatGainReadout(getDisplayedClipVolume(clip))}`}
                  role="group"
                  aria-label="Klip ses kontrolleri"
                  onpointerdown={(event) =>
                    handleAudioRegionPointerDown(event, clip)}
                  onpointermove={(event) =>
                    handleAudioRegionPointerMove(event, clip)}
                  onpointerup={(event) =>
                    finishAudioRegionSelection(event, clip)}
                  onpointercancel={(event) =>
                    finishAudioRegionSelection(event, clip)}
                >
                  {#each getQuietRegions(clip) as region (region.start)}
                    <button
                      type="button"
                      class="audio-region-quiet"
                      style:left={`${(region.start / clip.duration) * 100}%`}
                      style:width={`${((region.end - region.start) / clip.duration) * 100}%`}
                      disabled={isClipLocked(clip)}
                      title={`Bu bölge ${formatRelativeGainDb(region.volume, clip.volume)} seviyesine kısıldı. Düzenlemek için tıklayın.`}
                      aria-label="Kısılmış ses bölgesini düzenle"
                      onpointerdown={(event) => event.stopPropagation()}
                      onclick={(event) => openQuietRegionEditor(event, clip, region)}
                    ></button>
                  {/each}
                  {#if audioRegionSelection?.clipId === clip.id}
                    <span
                      class="audio-region-selection"
                      style:left={`${(audioRegionSelection.start / clip.duration) * 100}%`}
                      style:width={`${((audioRegionSelection.end - audioRegionSelection.start) / clip.duration) * 100}%`}
                      aria-hidden="true"
                    ></span>
                    <button
                      type="button"
                      class="audio-region-handle start"
                      style:left={`${(audioRegionSelection.start / clip.duration) * 100}%`}
                      disabled={isClipLocked(clip)}
                      title="Bölgenin başlangıcını sürükle"
                      aria-label="Bölge başlangıcını sürükle"
                      onpointerdown={(event) =>
                        handleAudioRegionHandlePointerDown(event, clip, "start")}
                      onpointermove={(event) =>
                        handleAudioRegionHandlePointerMove(event, clip)}
                      onpointerup={finishAudioRegionResize}
                      onpointercancel={finishAudioRegionResize}
                    ></button>
                    <button
                      type="button"
                      class="audio-region-handle end"
                      style:left={`${(audioRegionSelection.end / clip.duration) * 100}%`}
                      disabled={isClipLocked(clip)}
                      title="Bölgenin bitişini sürükle"
                      aria-label="Bölge bitişini sürükle"
                      onpointerdown={(event) =>
                        handleAudioRegionHandlePointerDown(event, clip, "end")}
                      onpointermove={(event) =>
                        handleAudioRegionHandlePointerMove(event, clip)}
                      onpointerup={finishAudioRegionResize}
                      onpointercancel={finishAudioRegionResize}
                    ></button>
                  {/if}
                  <span class="clip-audio-icon" aria-hidden="true">♪</span>
                  {#if audioRegionModeClipId === clip.id}
                    <span class="audio-region-hint">Bölgeyi sürükle</span>
                  {:else if selectedClipId === clip.id}
                    <button
                      type="button"
                      class="clip-region-button"
                      disabled={selectedTimelineTrack?.locked ?? false}
                      title="Belirli bir ses aralığını kıs"
                      aria-label="Kısılacak ses bölgesini seç"
                      onpointerdown={(event) => event.stopPropagation()}
                      onclick={(event) => toggleAudioRegionMode(event, clip)}
                    >Bölge</button>
                    <button
                      type="button"
                      class="clip-volume-drag-handle"
                      data-testid="clip-volume-drag-handle"
                      class:dragging={clipVolumeDrag?.clipId === clip.id}
                      disabled={isClipLocked(clip)}
                      title="Yukarı sürükle: dB artır · Aşağı sürükle: dB azalt · Shift: hassas ayar"
                      aria-label={`Klip kazancı ${formatGainReadout(getDisplayedClipVolume(clip))}. Yukarı veya aşağı sürükleyin.`}
                      onpointerdown={(event) =>
                        handleClipVolumeGainPointerDown(event, clip)}
                      onpointermove={(event) =>
                        handleClipVolumeGainPointerMove(event, clip)}
                      onpointerup={(event) => finishClipVolumeGainDrag(event, true)}
                      onpointercancel={(event) => finishClipVolumeGainDrag(event, false)}
                      onclick={(event) => event.stopPropagation()}
                    >↕</button>
                    <input
                      class="clip-volume-slider"
                      type="range"
                      min={MIN_EDITABLE_GAIN_DB}
                      max={MAX_EDITABLE_GAIN_DB}
                      step="0.1"
                      value={getGainDbSliderValue(getDisplayedClipVolume(clip))}
                      disabled={isClipLocked(clip)}
                      aria-label={`${getFileName(clip.file)} ses seviyesi`}
                      aria-valuetext={formatGainReadout(getDisplayedClipVolume(clip))}
                      title="Klip kazancı (dB). 0 dB orijinal ses seviyesidir."
                      onpointerdown={(event) => event.stopPropagation()}
                      onclick={(event) => event.stopPropagation()}
                      oninput={(event) => updateClipVolumeFromDecibels(clip.id, event)}
                    />
                    <output class="clip-volume-value"
                      >{formatGainReadout(getDisplayedClipVolume(clip))}</output
                    >
                  {/if}
                </div>
              {/if}
              {#if clip.kind === "text" && clip.text}
                <span class="text-preview">{clip.text.content}</span>
              {/if}
              <span class="clip-name"
                >{clip.kind === "text"
                  ? clip.text?.content
                  : getFileName(clip.file)}</span
              >
              {#each getManualKeyframeMarkers(clip) as keyframe}
                <span
                  class="keyframe-marker"
                  style={`left: ${(keyframe.time / clip.duration) * 100}%`}
                  title={`Keyframe ${keyframe.time.toFixed(2)}s`}
                ></span>
              {/each}
              {#if clip.transition.in.type !== "none"}
                <button
                  type="button"
                  class="transition-envelope transition-envelope-in"
                  class:active={getActiveTransitionSide(clip) === "in"}
                  style:width={`${(getDisplayedTransitionDuration(clip, "in") / clip.duration) * 100}%`}
                  title={`${getTransitionPreset(clip.transition.in.type).label} giriş geçişi · ${getDisplayedTransitionDuration(clip, "in").toFixed(2)} sn`}
                  aria-label={`${getTransitionPreset(clip.transition.in.type).label} giriş geçişini seç`}
                  onpointerdown={(event) => event.stopPropagation()}
                  onclick={(event) => handleTransitionEnvelopeClick(event, clip, "in")}
                  oncontextmenu={(event) => openTransitionContextMenu(event, clip, "in")}
                >
                  <span aria-hidden="true">◢</span>
                  <span>{getTransitionPreset(clip.transition.in.type).shortLabel}</span>
                </button>
                {#if selectedClipId === clip.id}
                  <button
                    type="button"
                    role="slider"
                    class="transition-handle"
                    class:dragging={transitionDurationDrag?.clipId === clip.id && transitionDurationDrag.side === "in"}
                    style:left={`${getTransitionBoundaryPercent(clip, "in")}%`}
                    disabled={isClipLocked(clip)}
                    aria-label="Giriş geçişinin bitişini ayarla"
                    aria-valuemin="0"
                    aria-valuemax={getMaximumTransitionDuration(clip, "in")}
                    aria-valuenow={getDisplayedTransitionDuration(clip, "in")}
                    aria-valuetext={`${getDisplayedTransitionDuration(clip, "in").toFixed(2)} saniye`}
                    title="Giriş geçişinin bitişini sürükle · Shift: hassas ayar"
                    onpointerdown={(event) => handleTransitionPointerDown(event, clip, "in")}
                    onclick={(event) => event.stopPropagation()}
                    onkeydown={(event) => handleTransitionHandleKeydown(event, clip, "in")}
                  >
                    <span aria-hidden="true"></span>
                    {#if transitionDurationDrag?.clipId === clip.id && transitionDurationDrag.side === "in"}
                      <output>{getDisplayedTransitionDuration(clip, "in").toFixed(2)} sn</output>
                    {/if}
                  </button>
                {/if}
              {:else if selectedClipId === clip.id}
                <button
                  type="button"
                  class="transition-add transition-add-in"
                  class:active={getActiveTransitionSide(clip) === "in"}
                  disabled={isClipLocked(clip)}
                  title="Giriş geçişi ekle"
                  aria-label="Giriş geçişi ekle"
                  onpointerdown={(event) => event.stopPropagation()}
                  onclick={(event) => handleTransitionEnvelopeClick(event, clip, "in")}
                >＋</button>
              {/if}
              {#if clip.transition.out.type !== "none"}
                <button
                  type="button"
                  class="transition-envelope transition-envelope-out"
                  class:active={getActiveTransitionSide(clip) === "out"}
                  style:width={`${(getDisplayedTransitionDuration(clip, "out") / clip.duration) * 100}%`}
                  title={`${getTransitionPreset(clip.transition.out.type).label} çıkış geçişi · ${getDisplayedTransitionDuration(clip, "out").toFixed(2)} sn`}
                  aria-label={`${getTransitionPreset(clip.transition.out.type).label} çıkış geçişini seç`}
                  onpointerdown={(event) => event.stopPropagation()}
                  onclick={(event) => handleTransitionEnvelopeClick(event, clip, "out")}
                  oncontextmenu={(event) => openTransitionContextMenu(event, clip, "out")}
                >
                  <span>{getTransitionPreset(clip.transition.out.type).shortLabel}</span>
                  <span aria-hidden="true">◣</span>
                </button>
                {#if selectedClipId === clip.id}
                  <button
                    type="button"
                    role="slider"
                    class="transition-handle"
                    class:dragging={transitionDurationDrag?.clipId === clip.id && transitionDurationDrag.side === "out"}
                    style:left={`${getTransitionBoundaryPercent(clip, "out")}%`}
                    disabled={isClipLocked(clip)}
                    aria-label="Çıkış geçişinin başlangıcını ayarla"
                    aria-valuemin="0"
                    aria-valuemax={getMaximumTransitionDuration(clip, "out")}
                    aria-valuenow={getDisplayedTransitionDuration(clip, "out")}
                    aria-valuetext={`${getDisplayedTransitionDuration(clip, "out").toFixed(2)} saniye`}
                    title="Çıkış geçişinin başlangıcını sürükle · Shift: hassas ayar"
                    onpointerdown={(event) => handleTransitionPointerDown(event, clip, "out")}
                    onclick={(event) => event.stopPropagation()}
                    onkeydown={(event) => handleTransitionHandleKeydown(event, clip, "out")}
                  >
                    <span aria-hidden="true"></span>
                    {#if transitionDurationDrag?.clipId === clip.id && transitionDurationDrag.side === "out"}
                      <output>{getDisplayedTransitionDuration(clip, "out").toFixed(2)} sn</output>
                    {/if}
                  </button>
                {/if}
              {:else if selectedClipId === clip.id}
                <button
                  type="button"
                  class="transition-add transition-add-out"
                  class:active={getActiveTransitionSide(clip) === "out"}
                  disabled={isClipLocked(clip)}
                  title="Çıkış geçişi ekle"
                  aria-label="Çıkış geçişi ekle"
                  onpointerdown={(event) => event.stopPropagation()}
                  onclick={(event) => handleTransitionEnvelopeClick(event, clip, "out")}
                >＋</button>
              {/if}
              <button
                class="trim-handle trim-start"
                aria-label="Klip başlangıcını kırp"
                title="Başlangıcı kırp"
                onpointerdown={(event) =>
                  handleTrimPointerDown(event, clip, "start")}
              ></button>
              <button
                class="trim-handle trim-end"
                aria-label="Klip sonunu kırp"
                title="Sonu kırp"
                onpointerdown={(event) =>
                  handleTrimPointerDown(event, clip, "end")}
              ></button>
            </div>
          {/each}

          {#if clipDragHasMoved && clipMovePreview?.trackId === track.id}
            <div
              class="clip move-preview"
              class:invalid={!clipMovePreview.isAllowed}
              class:insert={clipMovePreview.mode === "insert"}
              style="left: {(clipMovePreview.start / viewDuration) *
                100}%; width: {(clipMovePreview.duration / viewDuration) * 100}%"
              aria-hidden="true"
            >
              <span class="clip-name">
                {clipMovePreview.isAllowed
                  ? clipMovePreview.mode === "insert"
                    ? "Araya ekle"
                    : track.name
                  : "Bırakılamaz"}
              </span>
            </div>
          {/if}

          {#if isDragActive && dragPreview?.trackId === track.id && dragPreview.isAllowed}
            <div
              class="clip preview"
              style="left: {(dragPreview.start / viewDuration) *
                100}%; width: {(DEFAULT_CLIP_DURATION / viewDuration) * 100}%"
              aria-hidden="true"
            >
              <span class="clip-name">
                {dragPreview.file ? getFileName(dragPreview.file) : "Bırak"}
              </span>
            </div>
          {/if}
        </div>
      {/each}

      {#if clipDragHasMoved && clipMovePreview}
        <div
          class="clip-move-guide"
          class:invalid={!clipMovePreview.isAllowed}
          class:insert={clipMovePreview.mode === "insert"}
          style:left={`${(clipMovePreview.start / viewDuration) * 100}%`}
          aria-hidden="true"
        ></div>
        <div
          class="clip-move-status"
          class:invalid={!clipMovePreview.isAllowed}
          class:insert={clipMovePreview.mode === "insert"}
          style:left={`${(clipMovePreview.start / viewDuration) * 100}%`}
          style:top={`${Math.max(2, tracks.findIndex((track) => track.id === clipMovePreview?.trackId) * TRACK_HEIGHT_PX + 2)}px`}
          role="status"
        >
          {clipMovePreview.message}
        </div>
      {/if}

      <div
        class="playhead"
        class:scrubbing={isScrubbing}
        style="left: {getPlayheadPosition()}%"
      >
        <div
          class="playhead-handle"
          role="slider"
          tabindex="0"
          aria-label="Oynatma kafası"
          aria-valuemin="0"
          aria-valuemax={Math.max(0, duration)}
          aria-valuenow={currentTime}
          aria-valuetext={formatTime(currentTime)}
          aria-keyshortcuts="ArrowLeft ArrowRight Shift+ArrowLeft Shift+ArrowRight Alt+ArrowLeft Alt+ArrowRight"
          title="Sürükle · ←/→ 1 sn · Shift 5 sn · Alt 1 kare"
          onpointerdown={handlePlayheadPointerDown}
          onclick={(event) => event.stopPropagation()}
          onkeydown={handlePlayheadKeydown}
        >
          <span class="playhead-head"></span>
          <span class="playhead-line"></span>
        </div>
      </div>
    </div>
  </div>
  </div>

</div>

<ContextMenu
  open={timelineContextMenu.open}
  x={timelineContextMenu.x}
  y={timelineContextMenu.y}
  items={getTimelineContextMenuItems()}
  onClose={closeTimelineContextMenu}
  ariaLabel={timelineContextMenu.transitionSide
    ? "Geçiş işlemleri"
    : timelineContextMenu.clipId
      ? "Klip işlemleri"
      : "Zaman çizelgesi işlemleri"}
/>

<style>
  .timeline {
    height: 100%;
    display: flex;
    flex-direction: column;
    position: relative;
    background: #0e0e0e;
    user-select: none;
  }

  .timeline-toolbar {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    gap: 4px;
    padding: 4px 6px;
    overflow-x: auto;
    scrollbar-width: none;
    border-bottom: 1px solid #1f1f1f;
    background: #111;
  }

  .timeline-toolbar::-webkit-scrollbar {
    display: none;
  }

  .timeline-quick-actions {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border: 1px solid #292d2c;
    border-radius: 6px;
    background: linear-gradient(180deg, #181a19, #121413);
    box-shadow: inset 0 1px rgba(255, 255, 255, 0.025);
  }

  .quick-action {
    min-width: 26px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    box-sizing: border-box;
    padding: 0 7px;
    border: 1px solid transparent;
    border-radius: 4px;
    background: transparent;
    color: #a7adaa;
    font: 600 10px/1 system-ui, sans-serif;
    white-space: nowrap;
    cursor: pointer;
    transition:
      color 100ms ease,
      border-color 100ms ease,
      background 100ms ease;
  }

  .quick-action.icon-only {
    width: 26px;
    padding: 0;
  }

  .quick-action svg {
    width: 14px;
    height: 14px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .quick-action:hover:not(:disabled) {
    color: #f3f7f5;
    border-color: #3c4542;
    background: #252a28;
  }

  .quick-action.danger:hover:not(:disabled) {
    color: #ffd5d1;
    border-color: #70423e;
    background: #36201e;
  }

  .quick-action.cancel.active {
    color: #ffe5aa;
    border-color: #5d4d2b;
    background: #302817;
  }

  .quick-action:focus-visible,
  .mode-btn:focus-visible,
  .tool-btn:focus-visible,
  .tool-help-trigger:focus-visible {
    outline: 2px solid rgba(93, 225, 201, 0.9);
    outline-offset: 1px;
  }

  .quick-action:disabled {
    opacity: 0.32;
    cursor: not-allowed;
  }

  .quick-separator,
  .toolbar-divider {
    width: 1px;
    height: 16px;
    flex: none;
    background: #303432;
  }

  .quick-separator {
    margin: 0 2px;
  }

  .toolbar-divider {
    margin: 0 2px;
  }

  .toolbar-spacer {
    flex: 1;
    min-width: 8px;
  }

  .timeline-body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .track-headers {
    width: 112px;
    flex-shrink: 0;
    border-right: 1px solid #1f1f1f;
    padding-top: 24px;
    background: #111;
  }

  .track-header {
    height: 64px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    padding: 0 5px;
    box-sizing: border-box;
    border-bottom: 1px solid #1a1a1a;
  }

  .track-header.locked,
  .track.locked .clip {
    filter: saturate(0.45);
  }

  .track-switches {
    display: flex;
    gap: 2px;
  }

  .track-switch {
    width: 18px;
    height: 20px;
    padding: 0;
    border: 1px solid #303030;
    border-radius: 3px;
    background: #191919;
    color: #777;
    font: 600 9px "JetBrains Mono", monospace;
    cursor: pointer;
  }

  .track-switch:hover {
    color: #eee;
    border-color: #555;
  }

  .track-switch.mute.active {
    color: #ff8a80;
    border-color: #b23c36;
    background: #3a1917;
  }

  .track-switch.solo.active {
    color: #ffe082;
    border-color: #8d6e20;
    background: #382f12;
  }

  .track-switch.lock.active {
    color: #90caf9;
    border-color: #335d7d;
    background: #132737;
  }

  .track-switch.remove {
    color: #9b716d;
    font-size: 15px;
    line-height: 1;
  }

  .track-switch.remove:hover:not(:disabled) {
    color: #ffc0b9;
    border-color: #7f403a;
    background: #351917;
  }

  .track-switch:disabled {
    cursor: not-allowed;
    opacity: 0.28;
  }

  .track-header.drop-target {
    background: #1a1a1a;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.2);
  }

  .track-header.invalid-drop-target {
    background: rgba(182, 53, 46, 0.12);
    box-shadow: inset 0 0 0 1px rgba(239, 111, 101, 0.62);
  }

  .track-header.clip-move-target {
    background: rgba(53, 214, 184, 0.12);
    box-shadow: inset 0 0 0 1px rgba(83, 225, 198, 0.72);
  }

  .track-header.invalid-clip-move-target {
    background: rgba(182, 53, 46, 0.14);
    box-shadow: inset 0 0 0 1px rgba(239, 111, 101, 0.72);
  }

  .track-header.insert-target {
    background: rgba(171, 124, 22, 0.14);
    box-shadow: inset 0 0 0 1px rgba(255, 209, 102, 0.78);
  }

  .track-name {
    min-width: 18px;
    font-size: 11px;
    font-weight: 600;
    color: #666;
    font-family: "JetBrains Mono", monospace;
  }

  .track-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow-x: auto;
    position: relative;
  }

  .ruler {
    height: 24px;
    background: #111;
    border-bottom: 1px solid #1f1f1f;
    position: relative;
    width: 100%;
  }

  .tick {
    position: absolute;
    top: 0;
    height: 100%;
    transform: translateX(-50%); /* Center tick on time */
  }

  .tick::before {
    content: "";
    position: absolute;
    top: 14px;
    left: 50%;
    width: 1px;
    height: 10px;
    background: #333;
  }

  .tick-label {
    position: absolute;
    top: 4px;
    left: 4px;
    font-size: 9px;
    font-family: "JetBrains Mono", monospace;
    color: #555;
    transform: translateX(-50%);
  }

  .tracks {
    flex: 1;
    position: relative;
    cursor: crosshair;
    background: #0a0a0a;
  }

  .tracks.drag-active {
    outline: 1px dashed rgba(255, 255, 255, 0.15);
    outline-offset: -1px;
  }

  .tracks.clip-dragging {
    cursor: grabbing;
  }

  .track {
    height: 64px;
    border-bottom: 1px solid #1a1a1a;
    position: relative;
    width: 100%;
  }

  .track.drop-target::after {
    content: "";
    position: absolute;
    inset: 2px;
    border-radius: 6px;
    border: 1px dashed rgba(255, 255, 255, 0.35);
    pointer-events: none;
  }

  .track.invalid-drop-target::after {
    border-color: rgba(239, 111, 101, 0.82);
    background: rgba(182, 53, 46, 0.07);
  }

  .track.clip-move-target::after,
  .track.invalid-clip-move-target::after {
    content: "";
    position: absolute;
    z-index: 45;
    inset: 2px;
    border-radius: 6px;
    border: 1px solid rgba(83, 225, 198, 0.72);
    background: rgba(53, 214, 184, 0.055);
    pointer-events: none;
  }

  .track.invalid-clip-move-target::after {
    border-color: rgba(239, 111, 101, 0.88);
    background: rgba(182, 53, 46, 0.09);
  }

  .track.insert-target::after {
    border-color: rgba(255, 209, 102, 0.9);
    background: rgba(171, 124, 22, 0.08);
  }

  .track[data-type="video"] {
    background: rgba(255, 255, 255, 0.01);
  }
  .track[data-type="audio"] {
    background: rgba(255, 255, 255, 0.015);
  }
  .track[data-type="text"] {
    background: rgba(255, 255, 255, 0.02);
  }

  .track.muted .clip {
    opacity: 0.42;
  }

  /* Clips */
  .clip {
    position: absolute;
    box-sizing: border-box;
    top: 4px;
    bottom: 4px;
    background: #2a4;
    border-radius: 4px;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
    border: 1px solid rgba(255, 255, 255, 0.2);
    display: flex;
    align-items: center;
    padding: 0 8px;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
  }

  .tracks.cut-mode .clip {
    cursor: crosshair;
  }

  .clip.selected {
    z-index: 20;
    outline: 2px solid rgba(88, 225, 205, 0.95);
    box-shadow:
      0 0 0 1px rgba(0, 0, 0, 0.5),
      0 0 0 3px rgba(66, 214, 190, 0.15),
      0 8px 18px rgba(0, 0, 0, 0.48);
  }

  .clip.pressed {
    z-index: 24;
    cursor: grabbing;
    transform: translateY(-1px);
    outline-color: #7ff7e2;
    filter: brightness(1.13);
  }

  .clip.drag-origin {
    z-index: 12;
    cursor: grabbing;
    opacity: 0.3;
    outline: 1px dashed rgba(127, 247, 226, 0.88);
    box-shadow: none;
  }

  .clip.insert-shift-preview {
    z-index: 30;
    opacity: 0.72;
    outline: 1px dashed rgba(255, 209, 102, 0.95);
    transition: transform 90ms ease-out;
  }

  .clip.ripple-close-preview {
    z-index: 30;
    opacity: 0.76;
    outline: 1px dashed rgba(105, 239, 215, 0.95);
    transition: transform 90ms ease-out;
  }

  .trim-handle {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 5;
    width: 8px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: ew-resize;
  }

  .trim-handle::after {
    content: "";
    position: absolute;
    top: 6px;
    bottom: 6px;
    width: 2px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.75);
    opacity: 0;
  }

  .clip:hover .trim-handle::after,
  .clip.selected .trim-handle::after {
    opacity: 1;
  }

  .trim-start {
    left: 0;
  }

  .trim-start::after {
    left: 2px;
  }

  .trim-end {
    right: 0;
  }

  .trim-end::after {
    right: 2px;
  }

  .waveform {
    position: absolute !important;
    inset: 2px 4px;
    width: calc(100% - 8px);
    height: calc(100% - 4px);
    z-index: 1;
    opacity: 0.8;
    pointer-events: none;
  }

  .waveform path {
    fill: none;
    stroke: rgba(255, 255, 255, 0.75);
    stroke-width: 0.55;
  }

  .beat-grid {
    position: absolute;
    inset: 0;
    z-index: 4;
    overflow: hidden;
    pointer-events: none;
  }

  .beat-grid > span {
    position: absolute;
    top: 5px;
    bottom: 4px;
    width: 1px;
    background: rgba(188, 113, 255, 0.7);
    box-shadow: 0 0 2px rgba(181, 101, 255, 0.55);
  }

  .beat-grid > span.downbeat {
    width: 2px;
    background: #ffd36d;
    box-shadow: 0 0 3px rgba(255, 205, 94, 0.8);
  }

  .beat-badge {
    position: absolute;
    top: 4px;
    left: 5px;
    z-index: 7;
    padding: 1px 4px;
    border: 1px solid rgba(196, 125, 255, 0.68);
    border-radius: 4px;
    color: #e2bcff;
    background: rgba(27, 12, 37, 0.86);
    font-size: 8px;
    font-weight: 800;
    line-height: 1.35;
    pointer-events: none;
  }

  .speech-suggestion-grid {
    position: absolute;
    inset: 0;
    z-index: 6;
    overflow: hidden;
    pointer-events: none;
  }

  .speech-suggestion-grid > span {
    position: absolute;
    bottom: 1px;
    min-width: 2px;
    height: 5px;
    border-radius: 2px;
    opacity: 0.88;
  }

  .speech-suggestion-grid > span.silence { background: #62d9c5; }
  .speech-suggestion-grid > span.filler { background: #ff9b62; }
  .speech-suggestion-grid > span.possible { background: #f3cf67; }
  .speech-suggestion-grid > span.contextual {
    height: 4px;
    border: 1px dashed #c69cff;
    background: rgba(164, 108, 222, 0.26);
  }

  .ai-rider-curve {
    position: absolute !important;
    inset: 2px 4px;
    width: calc(100% - 8px);
    height: calc(100% - 4px);
    z-index: 5;
    overflow: visible;
    pointer-events: none;
    filter: drop-shadow(0 0 2px rgba(116, 233, 255, 0.55));
  }

  .ai-rider-curve polyline {
    fill: none;
    stroke: #74e9ff;
    stroke-width: 1.2;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .ai-rider-badge {
    position: absolute !important;
    top: 4px;
    right: 5px;
    z-index: 6;
    padding: 1px 4px;
    border: 1px solid rgba(116, 233, 255, 0.62);
    border-radius: 4px;
    background: rgba(7, 25, 30, 0.86);
    color: #8ceeff;
    font-size: 8px;
    font-weight: 800;
    letter-spacing: 0.08em;
    line-height: 1.35;
    pointer-events: none;
  }

  .video-clip :global(.filmstrip) {
    bottom: 18px;
  }

  .video-clip .waveform {
    inset: auto 3px 2px;
    width: calc(100% - 6px);
    height: 15px;
    z-index: 2;
    opacity: 0.9;
  }

  .video-clip .waveform path {
    stroke: rgba(118, 241, 217, 0.9);
    stroke-width: 0.8;
  }

  .clip-audio-strip {
    position: absolute !important;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 3 !important;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 18px;
    padding: 0 5px;
    box-sizing: border-box;
    overflow: hidden;
    color: #a9fff0;
    background: linear-gradient(
      180deg,
      rgba(6, 45, 48, 0.84),
      rgba(4, 31, 34, 0.96)
    );
    border-top: 1px solid rgba(92, 226, 202, 0.42);
    pointer-events: none;
  }

  .clip-audio-strip.region-mode {
    border-top-color: rgba(255, 205, 96, 0.86);
    background: linear-gradient(
      180deg,
      rgba(62, 43, 9, 0.9),
      rgba(38, 27, 7, 0.98)
    );
    cursor: crosshair;
    pointer-events: auto;
    touch-action: none;
  }

  .clip-audio-strip.gain-dragging {
    border-top-color: rgba(152, 255, 232, 0.96);
    background: linear-gradient(
      180deg,
      rgba(9, 75, 68, 0.95),
      rgba(4, 39, 38, 0.98)
    );
  }

  .clip-audio-strip::before {
    content: "";
    position: absolute;
    inset: 3px 4px;
    opacity: 0.34;
    background: repeating-linear-gradient(
      90deg,
      rgba(113, 240, 216, 0.78) 0 1px,
      transparent 1px 4px
    );
    mask-image: linear-gradient(
      180deg,
      transparent 0%,
      #000 28%,
      #000 72%,
      transparent 100%
    );
  }

  .clip-audio-strip.has-waveform::before {
    opacity: 0.08;
  }

  .clip-audio-strip.region-mode::before {
    opacity: 0.28;
    background: repeating-linear-gradient(
      90deg,
      rgba(255, 214, 113, 0.82) 0 1px,
      transparent 1px 4px
    );
  }

  .audio-region-selection {
    position: absolute !important;
    top: 0;
    bottom: 0;
    z-index: 1 !important;
    min-width: 2px;
    box-sizing: border-box;
    background: rgba(255, 190, 65, 0.28);
    border-right: 1px solid rgba(255, 222, 140, 0.95);
    border-left: 1px solid rgba(255, 222, 140, 0.95);
    box-shadow:
      inset 0 0 0 1px rgba(255, 196, 76, 0.18),
      0 0 8px rgba(255, 185, 54, 0.22);
    pointer-events: none;
  }

  .audio-region-applied-overlay {
    position: absolute !important;
    top: 0;
    bottom: 0;
    z-index: 2 !important;
    min-width: 4px;
    box-sizing: border-box;
    background: linear-gradient(
      180deg,
      rgba(255, 213, 112, 0.2),
      rgba(255, 164, 52, 0.34)
    );
    border-right: 2px solid rgba(255, 222, 140, 0.96);
    border-left: 2px solid rgba(255, 222, 140, 0.96);
    box-shadow:
      inset 0 0 0 1px rgba(255, 196, 76, 0.18),
      0 0 9px rgba(255, 178, 45, 0.3);
    pointer-events: none;
  }

  .audio-region-applied-value,
  .audio-region-gain-value {
    position: absolute;
    top: 50%;
    left: 50%;
    max-width: calc(100% - 6px);
    padding: 2px 4px;
    transform: translate(-50%, -50%);
    overflow: hidden;
    color: #2b1b00;
    background: rgba(255, 231, 166, 0.9);
    border: 1px solid rgba(100, 61, 0, 0.38);
    border-radius: 4px;
    box-shadow: 0 2px 6px rgba(35, 19, 0, 0.3);
    font: 800 8px/1 "JetBrains Mono", monospace;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }

  .audio-region-gain-zone {
    position: absolute !important;
    top: 0;
    bottom: 18px;
    z-index: 4 !important;
    min-width: 5px;
    margin: 0;
    padding: 0;
    appearance: none;
    box-sizing: border-box;
    background: rgba(255, 196, 76, 0.08);
    border: 1px dashed rgba(255, 226, 157, 0.58);
    cursor: ns-resize;
    pointer-events: auto;
    touch-action: none;
  }

  .audio-region-gain-zone:hover:not(:disabled),
  .audio-region-gain-zone.dragging {
    background: rgba(255, 196, 76, 0.2);
    border-color: rgba(255, 238, 191, 0.95);
    box-shadow: inset 0 0 12px rgba(255, 190, 65, 0.18);
  }

  .audio-region-gain-zone.dragging .audio-region-gain-value {
    color: #211300;
    background: #ffe4a3;
    transform: translate(-50%, -50%) scale(1.08);
  }

  .audio-region-gain-zone:disabled {
    cursor: not-allowed;
    opacity: 0.48;
  }

  .audio-region-handle {
    position: absolute !important;
    top: 0;
    bottom: 0;
    z-index: 5 !important;
    width: 9px;
    margin: 0;
    padding: 0;
    box-sizing: border-box;
    border: 0;
    background: transparent;
    transform: translateX(-50%);
    cursor: ew-resize;
    touch-action: none;
    pointer-events: auto;
  }

  .audio-region-handle::before {
    content: "";
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 50%;
    width: 3px;
    transform: translateX(-50%);
    border-radius: 2px;
    background: rgba(255, 222, 140, 0.95);
    box-shadow: 0 0 4px rgba(255, 185, 54, 0.5);
  }

  .audio-region-handle:hover::before,
  .audio-region-handle:focus-visible::before {
    background: #fff3cf;
    box-shadow: 0 0 6px rgba(255, 214, 113, 0.85);
  }

  .audio-region-handle:disabled {
    cursor: not-allowed;
  }

  .clip-audio-icon,
  .clip-region-button,
  .clip-volume-drag-handle,
  .clip-volume-slider,
  .clip-volume-value,
  .audio-region-hint {
    position: relative;
    z-index: 2;
  }

  .clip-audio-icon {
    flex: 0 0 auto;
    font: 700 9px/1 "JetBrains Mono", monospace;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.85);
  }

  .clip-region-button {
    min-width: 34px;
    height: 14px;
    flex: 0 0 auto;
    padding: 0 4px;
    color: #bafff1;
    background: rgba(4, 27, 29, 0.78);
    border: 1px solid rgba(109, 232, 209, 0.42);
    border-radius: 3px;
    font: 700 6.5px/1 "JetBrains Mono", monospace;
    cursor: pointer;
    pointer-events: auto;
  }

  .clip-region-button:hover:not(:disabled),
  .clip-region-button:focus-visible {
    color: #fff9dd;
    background: rgba(78, 52, 7, 0.92);
    border-color: rgba(255, 210, 104, 0.82);
    outline: none;
  }

  .clip-region-button:disabled {
    cursor: not-allowed;
    opacity: 0.42;
  }

  .clip-volume-drag-handle {
    width: 16px;
    height: 14px;
    flex: 0 0 auto;
    margin: 0;
    padding: 0;
    color: #bafff1;
    background: rgba(4, 27, 29, 0.78);
    border: 1px solid rgba(109, 232, 209, 0.42);
    border-radius: 3px;
    font: 800 10px/1 "JetBrains Mono", monospace;
    cursor: ns-resize;
    pointer-events: auto;
    touch-action: none;
  }

  .clip-volume-drag-handle:hover:not(:disabled),
  .clip-volume-drag-handle:focus-visible,
  .clip-volume-drag-handle.dragging {
    color: #062523;
    background: #a7ffec;
    border-color: #e1fff9;
    box-shadow: 0 0 0 2px rgba(110, 246, 218, 0.18);
    outline: none;
  }

  .clip-volume-drag-handle:disabled {
    cursor: not-allowed;
    opacity: 0.42;
  }

  .audio-region-hint {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: #ffe3a2;
    font: 700 7px/1 "JetBrains Mono", monospace;
    letter-spacing: 0.02em;
    text-align: center;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-shadow: 0 1px 3px #171000;
  }

  .clip-volume-slider {
    min-width: 28px;
    height: 14px;
    flex: 1 1 72px;
    margin: 0;
    appearance: none;
    background: transparent;
    cursor: pointer;
    pointer-events: auto;
  }

  .clip-volume-slider::-webkit-slider-runnable-track {
    height: 3px;
    border-radius: 999px;
    background: rgba(179, 255, 240, 0.52);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
  }

  .clip-volume-slider::-webkit-slider-thumb {
    width: 9px;
    height: 9px;
    margin-top: -3px;
    appearance: none;
    border: 1px solid #062d2d;
    border-radius: 50%;
    background: #8af5df;
    box-shadow: 0 0 0 2px rgba(83, 225, 198, 0.22);
  }

  .clip-volume-slider:focus-visible {
    outline: 1px solid #95ffe9;
    outline-offset: 1px;
  }

  .clip-volume-value {
    min-width: 58px;
    flex: 0 0 auto;
    color: #d8fff7;
    font: 700 7px/1 "JetBrains Mono", monospace;
    text-align: right;
    text-shadow: 0 1px 3px #001b19;
  }

  .text-preview {
    position: absolute !important;
    inset: 5px 10px 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    font-size: 12px;
    overflow: hidden;
    pointer-events: none;
  }

  .keyframe-marker {
    position: absolute !important;
    top: 6px;
    z-index: 4;
    width: 7px;
    height: 7px;
    background: #ffeb3b;
    border: 1px solid rgba(0, 0, 0, 0.65);
    transform: translateX(-50%) rotate(45deg);
    pointer-events: none;
  }

  .transition-envelope {
    position: absolute !important;
    top: 2px;
    z-index: 9 !important;
    display: flex;
    box-sizing: border-box;
    height: 22px;
    min-width: 10px;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    padding: 0 7px;
    color: #bcf8e6;
    border: 1px solid rgba(100, 226, 190, 0.54);
    border-radius: 3px;
    font: 750 7px/1 "Inter", sans-serif;
    text-shadow: 0 1px 2px #001b14;
    white-space: nowrap;
    cursor: pointer;
  }

  .transition-envelope:hover,
  .transition-envelope.active {
    color: #edfff9;
    border-color: #9affdf;
    filter: brightness(1.16);
    box-shadow:
      inset 0 0 0 1px rgba(174, 255, 230, 0.18),
      0 0 9px rgba(77, 232, 190, 0.26);
  }

  .transition-envelope-in {
    left: 1px;
    justify-content: flex-start;
    background:
      repeating-linear-gradient(135deg, rgba(124, 244, 210, 0.18) 0 4px, rgba(13, 54, 44, 0.2) 4px 8px),
      rgba(12, 38, 32, 0.82);
    border-left-color: rgba(192, 255, 236, 0.92);
  }

  .transition-envelope-out {
    right: 1px;
    justify-content: flex-end;
    background:
      repeating-linear-gradient(45deg, rgba(124, 244, 210, 0.18) 0 4px, rgba(13, 54, 44, 0.2) 4px 8px),
      rgba(12, 38, 32, 0.82);
    border-right-color: rgba(192, 255, 236, 0.92);
  }

  .transition-handle {
    position: absolute !important;
    top: 0;
    z-index: 14 !important;
    width: 16px;
    height: 29px;
    padding: 0;
    color: #c8fff0;
    background: transparent;
    border: 0;
    transform: translateX(-50%);
    cursor: ew-resize;
    touch-action: none;
  }

  .transition-handle > span {
    position: absolute;
    top: 3px;
    bottom: 3px;
    left: 6px;
    width: 4px;
    background: #78e7c7;
    border: 1px solid #092b23;
    border-radius: 3px;
    box-shadow:
      0 0 0 1px rgba(208, 255, 242, 0.22),
      0 0 8px rgba(65, 231, 186, 0.62);
  }

  .transition-handle:hover > span,
  .transition-handle:focus-visible > span,
  .transition-handle.dragging > span {
    background: #c1ffed;
    box-shadow: 0 0 11px rgba(110, 255, 216, 0.88);
  }

  .transition-handle:focus-visible {
    outline: 1px solid #d3fff2;
    outline-offset: -2px;
  }

  .transition-handle:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .transition-handle output {
    position: absolute;
    top: 29px;
    left: 50%;
    min-width: 49px;
    padding: 3px 5px;
    color: #e8fff8;
    background: rgba(5, 25, 20, 0.94);
    border: 1px solid #56bda0;
    border-radius: 4px;
    transform: translateX(-50%);
    font: 700 7px/1 "JetBrains Mono", monospace;
    pointer-events: none;
  }

  .transition-add {
    position: absolute !important;
    top: 3px;
    z-index: 10 !important;
    display: grid;
    width: 19px;
    height: 19px;
    padding: 0;
    place-items: center;
    color: #91cdbb;
    background: rgba(6, 28, 23, 0.76);
    border: 1px dashed rgba(112, 217, 188, 0.7);
    border-radius: 4px;
    font: 800 12px/1 "Inter", sans-serif;
    cursor: pointer;
  }

  .transition-add:hover,
  .transition-add.active {
    color: #e6fff8;
    background: #174438;
    border-color: #9effe2;
  }

  .transition-add:disabled {
    cursor: not-allowed;
    opacity: 0.42;
  }

  .transition-add-in { left: 9px; }
  .transition-add-out { right: 9px; }

  .clip::before {
    content: "";
    position: absolute;
    inset: 0;
    background-image: repeating-linear-gradient(
      90deg,
      rgba(255, 255, 255, 0.18) 0px,
      rgba(255, 255, 255, 0.18) 1px,
      rgba(255, 255, 255, 0) 1px,
      rgba(255, 255, 255, 0) 16px
    );
    opacity: 0.25;
    pointer-events: none;
  }

  .clip::after {
    content: "";
    position: absolute;
    inset: 0;
    box-shadow:
      inset 1px 0 0 rgba(255, 255, 255, 0.2),
      inset -1px 0 0 rgba(0, 0, 0, 0.45);
    pointer-events: none;
  }

  .clip > * {
    z-index: 1;
  }

  .track[data-type="video"] .clip {
    background: #3d5afe;
  }
  .track[data-type="audio"] .clip {
    background: #00bfa5;
  }
  .track[data-type="text"] .clip {
    background: #ffa000;
  }

  .clip-name {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.9);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
    pointer-events: none;
    background: rgba(0, 0, 0, 0.5);
    padding: 2px 6px;
    border-radius: 3px;
    position: absolute;
    bottom: 4px;
    left: 4px;
    max-width: calc(100% - 8px);
  }

  .video-clip .clip-name {
    top: 3px;
    bottom: auto;
    background: rgba(4, 53, 60, 0.88);
    border: 1px solid rgba(93, 229, 206, 0.24);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
  }

  .audio-clip .clip-name {
    top: 3px;
    bottom: auto;
  }

  .clip-auto-reframe-job {
    position: absolute;
    right: 4px;
    bottom: 4px;
    left: 4px;
    z-index: 14;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 3px;
    min-width: 0;
    padding: 4px 6px;
    color: #dffbf5;
    text-align: left;
    background: rgba(7, 27, 30, 0.94);
    border: 1px solid rgba(78, 224, 199, 0.58);
    border-radius: 5px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.46);
    cursor: pointer;
  }

  .clip-auto-reframe-job:hover,
  .clip-auto-reframe-job:focus-visible {
    background: rgba(11, 42, 43, 0.98);
    border-color: rgba(116, 245, 221, 0.92);
    outline: none;
  }

  .clip-auto-reframe-job.completed {
    color: #d9ffea;
    background: rgba(13, 48, 34, 0.95);
    border-color: rgba(91, 225, 151, 0.72);
  }

  .clip-auto-reframe-job.error {
    color: #ffdcd5;
    background: rgba(58, 22, 20, 0.95);
    border-color: rgba(242, 112, 95, 0.72);
  }

  .clip-auto-reframe-label {
    overflow: hidden;
    font-size: 9px;
    font-weight: 800;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .clip-auto-reframe-rail {
    display: block;
    width: 100%;
    height: 3px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.14);
    border-radius: 999px;
  }

  .clip-auto-reframe-fill {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, #42d8c0, #8ff8e5);
    border-radius: inherit;
    box-shadow: 0 0 8px rgba(83, 232, 207, 0.7);
    transition: width 180ms ease;
  }

  .clip-auto-reframe-job.completed .clip-auto-reframe-fill {
    background: linear-gradient(90deg, #55d68c, #a0f2bc);
  }

  .clip.preview {
    background: rgba(255, 255, 255, 0.12) !important;
    border: 1px dashed rgba(255, 255, 255, 0.65);
    cursor: copy;
    opacity: 0.85;
    pointer-events: none;
  }

  .clip.preview::before {
    opacity: 0.15;
  }

  .clip.move-preview {
    z-index: 55;
    background: rgba(37, 168, 145, 0.7) !important;
    border: 2px solid #69efd7;
    outline: 1px solid rgba(0, 0, 0, 0.7);
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.55);
    cursor: grabbing;
    opacity: 0.92;
    pointer-events: none;
  }

  .clip.move-preview.invalid {
    background: rgba(151, 49, 42, 0.68) !important;
    border-color: #ff8c80;
  }

  .clip.move-preview.insert {
    background: rgba(171, 124, 22, 0.78) !important;
    border-color: #ffd166;
    box-shadow:
      0 0 0 2px rgba(255, 209, 102, 0.15),
      0 10px 24px rgba(0, 0, 0, 0.55);
  }

  .clip-move-guide {
    position: absolute;
    z-index: 58;
    top: 0;
    bottom: 0;
    width: 1px;
    background: #69efd7;
    box-shadow: 0 0 7px rgba(105, 239, 215, 0.75);
    pointer-events: none;
  }

  .clip-move-guide.invalid {
    background: #ff756a;
    box-shadow: 0 0 7px rgba(255, 117, 106, 0.75);
  }

  .clip-move-guide.insert {
    width: 2px;
    background: #ffd166;
    box-shadow: 0 0 9px rgba(255, 209, 102, 0.85);
  }

  .clip-move-guide.insert::before {
    content: "";
    position: absolute;
    top: 0;
    left: 50%;
    width: 0;
    height: 0;
    border-right: 6px solid transparent;
    border-left: 6px solid transparent;
    border-top: 8px solid #ffd166;
    transform: translateX(-50%);
  }

  .clip-move-status {
    position: absolute;
    z-index: 70;
    max-width: min(360px, 50vw);
    min-height: 18px;
    padding: 3px 7px;
    transform: translate(7px, 3px);
    overflow: hidden;
    color: #dffdf7;
    background: rgba(14, 56, 50, 0.96);
    border: 1px solid rgba(105, 239, 215, 0.75);
    border-radius: 4px;
    box-shadow: 0 5px 14px rgba(0, 0, 0, 0.48);
    font: 600 9px "JetBrains Mono", monospace;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }

  .clip-move-status.invalid {
    color: #ffe2de;
    background: rgba(91, 25, 22, 0.97);
    border-color: rgba(255, 117, 106, 0.82);
  }

  .clip-move-status.insert {
    color: #fff1c6;
    background: rgba(75, 52, 9, 0.97);
    border-color: rgba(255, 209, 102, 0.88);
  }

  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 100;
    width: 18px;
    pointer-events: none;
    transform: translateX(-50%);
  }

  .playhead-handle {
    position: absolute;
    top: -12px;
    right: 0;
    bottom: 0;
    left: 0;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: ew-resize;
    pointer-events: auto;
    touch-action: none;
  }

  .playhead-handle:focus-visible {
    outline: 1px solid rgba(255, 112, 104, 0.95);
    outline-offset: 2px;
  }

  .playhead-head {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 0;
    height: 0;
    border-left: 8px solid transparent;
    border-right: 8px solid transparent;
    border-top: 12px solid #e53935;
  }

  .playhead-line {
    position: absolute;
    top: 12px;
    bottom: 0;
    left: 50%;
    width: 2px; /* Thicker for visibility */
    background: #e53935;
    transform: translateX(-50%);
    box-shadow: 0 0 4px rgba(229, 57, 53, 0.4);
    pointer-events: none;
  }

  .playhead.scrubbing .playhead-head {
    border-top-color: #ff6b62;
  }

  .playhead.scrubbing .playhead-line {
    background: #ff6b62;
    box-shadow: 0 0 8px rgba(255, 107, 98, 0.7);
  }

  .zoom-tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .mode-btn {
    min-height: 26px;
    padding: 4px 9px;
    border: 1px solid #303030;
    border-radius: 4px;
    background: #1a1a1a;
    color: #999;
    font-size: 10px;
    white-space: nowrap;
    cursor: pointer;
  }

  .mode-btn:hover:not(:disabled),
  .mode-btn.active {
    border-color: #536dfe;
    color: #fff;
    background: #25306f;
  }

  .mode-btn:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  .tool-help {
    position: relative;
  }

  .tool-help-trigger {
    width: 26px;
    min-height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    border: 1px solid #303030;
    border-radius: 4px;
    background: #1a1a1a;
    color: #999;
    cursor: pointer;
    font-size: 11px;
    font-weight: 600;
  }

  .tool-help-trigger:hover,
  .tool-help-trigger.active {
    color: #eee;
    border-color: #555;
  }

  .tool-help-body {
    z-index: 40;
    width: min(320px, 60vw);
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 11px;
    border: 1px solid #2c302e;
    border-radius: 7px;
    background: #151716;
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.55);
    color: #858b87;
    font-size: 10px;
    line-height: 1.4;
  }

  .tool-help-body p {
    margin: 0;
  }

  .tool-help-body strong {
    color: #d0d6d3;
  }

  .audio-region-quiet {
    position: absolute !important;
    top: 0;
    bottom: 0;
    z-index: 4 !important;
    min-width: 4px;
    margin: 0;
    box-sizing: border-box;
    padding: 0;
    appearance: none;
    background: repeating-linear-gradient(
      45deg,
      rgba(255, 190, 65, 0.24) 0 3px,
      rgba(255, 190, 65, 0.1) 3px 6px
    );
    border: 0;
    border-top: 1px solid rgba(255, 205, 96, 0.65);
    border-bottom: 1px solid rgba(255, 205, 96, 0.65);
    cursor: pointer;
    pointer-events: auto;
  }

  .audio-region-quiet:hover:not(:disabled) {
    background: repeating-linear-gradient(
      45deg,
      rgba(255, 190, 65, 0.4) 0 3px,
      rgba(255, 190, 65, 0.18) 3px 6px
    );
  }

  .audio-region-quiet:disabled {
    cursor: not-allowed;
  }

  .tool-btn {
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    border-radius: 4px;
    color: #888;
    font-size: 16px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tool-btn:hover {
    background: #222;
    color: #fff;
    border-color: #444;
  }

  .zoom-label {
    font-size: 9px;
    color: #555;
    font-family: "JetBrains Mono", monospace;
    text-align: center;
  }

  @media (max-width: 940px) {
    .quick-action:not(.icon-only) {
      width: 26px;
      padding: 0;
    }

    .quick-action:not(.icon-only) > span {
      display: none;
    }
  }
</style>
