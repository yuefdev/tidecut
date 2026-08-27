<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { onDestroy, onMount, untrack } from "svelte";
  import ContextMenu, {
    type ContextMenuItem,
  } from "./ContextMenu.svelte";
  import type {
    AudioMixSource,
    CompositorLayer,
    FramePlan,
  } from "$lib/editor/timeline-engine";
  import {
    CanvasCompositor,
    MediaPlaybackIntentController,
    WebAudioMixer,
  } from "$lib/editor/media-runtime";
  import { fitPreviewDisplaySize } from "$lib/editor/preview-layout";
  import {
    applyTransportSeek,
    playbackStartTime,
    resolveTransportShortcut,
  } from "$lib/editor/transport-shortcuts";

  type PreviewTransformUpdate = Partial<{
    x: number;
    y: number;
    scale: number;
    rotation: number;
  }>;

  interface CoverCapture {
    imageDataUrl: string;
    timeMs: number;
    aspect: "9:16" | "1:1" | "16:9";
    updatedAtMs: number;
  }

  interface LayerBounds {
    left: number;
    top: number;
    width: number;
    height: number;
  }

  interface SelectionLayout {
    originX: number;
    originY: number;
    left: number;
    top: number;
    width: number;
    height: number;
    rotation: number;
  }

  type TransformDragMode = "move" | "scale" | "rotate";

  interface TransformDragState {
    mode: TransformDragMode;
    clipId: string;
    pointerId: number;
    captureTarget: HTMLElement;
    clientX: number;
    clientY: number;
    centerClientX: number;
    centerClientY: number;
    startDistance: number;
    startAngle: number;
    x: number;
    y: number;
    scale: number;
    rotation: number;
  }

  interface Props {
    plan: FramePlan;
    currentTime?: number;
    duration?: number;
    isPlaying?: boolean;
    /** Incremented for explicit transport discontinuities such as AI source swaps. */
    transportRevision?: number;
    aspect?: "9:16" | "1:1" | "16:9";
    selectedClipId?: string | null;
    onAspectChange?: (aspect: "9:16" | "1:1" | "16:9") => void;
    onMediaDuration?: (duration: number) => void;
    onPositionChange?: (position: { x: number; y: number }) => void;
    onTransformChange?: (updates: PreviewTransformUpdate) => void;
    transformDisabled?: boolean;
    onResetTransform?: () => void;
    /** Receives a compact copy of the fully composited frame for project cover artwork. */
    onCoverCapture?: (cover: CoverCapture) => void;
    coverTimeMs?: number | null;
    onDetach?: () => void;
  }

  let {
    plan,
    currentTime = $bindable(0),
    duration = 0,
    isPlaying = $bindable(false),
    transportRevision = 0,
    aspect = "16:9",
    selectedClipId = null,
    onAspectChange,
    onMediaDuration,
    onPositionChange,
    onTransformChange,
    transformDisabled = false,
    onResetTransform,
    onCoverCapture,
    coverTimeMs = null,
    onDetach,
  }: Props = $props();

  let canvas = $state<HTMLCanvasElement>();
  let stage = $state<HTMLDivElement>();
  let player = $state<HTMLDivElement>();
  let mediaHost = $state<HTMLDivElement>();
  let stageSize = $state({ width: 0, height: 0 });
  let stageResizeObserver: ResizeObserver | null = null;
  let compositor: CanvasCompositor | null = null;
  let audioMixer: WebAudioMixer | null = null;
  let audioContext: AudioContext | null = null;
  let rafId: number | null = null;
  let transportSeekRafId: number | null = null;
  let pendingTransportTime: number | null = null;
  let lastTick: number | null = null;
  let mediaError = $state<string | null>(null);
  let previewQuality = $state<1 | 0.5 | 0.25>(1);
  let showActionSafe = $state(false);
  let showTitleSafe = $state(false);
  let showCenterGuides = $state(false);
  let showThirdsGrid = $state(false);
  let isFullscreen = $state(false);
  let previewContextMenu = $state({ open: false, x: 0, y: 0 });
  let coverNotice = $state<string | null>(null);
  let coverNoticeTimeout: ReturnType<typeof window.setTimeout> | null = null;
  let transformDrag = $state<TransformDragState | null>(null);
  let textMeasureCanvas: HTMLCanvasElement | null = null;
  // Keep project coordinates in delivery pixels so transforms and typography
  // match FFmpeg output at the same aspect ratio.
  let canvasWidth = $derived(aspect === "9:16" ? 1080 : aspect === "1:1" ? 1080 : 1920);
  let canvasHeight = $derived(aspect === "9:16" ? 1920 : 1080);
  let canvasDisplaySize = $derived(
    fitPreviewDisplaySize(
      stageSize.width,
      stageSize.height,
      canvasWidth,
      canvasHeight,
      // Leave enough room for the rotation handle and labels around the frame.
      104,
    ),
  );
  let selectedLayer = $derived(
    selectedClipId
      ? plan.layers.find((candidate) => candidate.clipId === selectedClipId) ?? null
      : null,
  );
  let selectedLayerBounds = $derived(
    selectedLayer ? getLayerBounds(selectedLayer) : null,
  );
  let selectionLayout = $derived(
    selectedLayer && selectedLayerBounds
      ? getSelectionLayout(selectedLayer, selectedLayerBounds)
      : null,
  );
  let canMoveSelection = $derived(
    !transformDisabled && Boolean(onTransformChange || onPositionChange),
  );
  let canResizeSelection = $derived(!transformDisabled && Boolean(onTransformChange));

  const elements = new Map<string, HTMLMediaElement | HTMLImageElement>();
  const elementPaths = new Map<string, string>();
  const directPlayback = new MediaPlaybackIntentController();
  /** Mixer clip id -> backing media-element key. */
  const attachedAudio = new Map<string, string>();
  const pendingPausedSeeks = new WeakMap<HTMLMediaElement, number>();
  const pendingTransportSeeks = new WeakMap<HTMLMediaElement, number>();
  const PAUSED_SEEK_TOLERANCE = 1 / 60;
  let previousSync: { planTime: number; wallTime: number; playing: boolean } | null = null;
  let appliedTransportRevision: number | undefined;

  function handleFontsLoaded() {
    renderFrame();
  }

  onMount(() => {
    const updateStageSize = () => {
      if (!stage) return;
      const bounds = stage.getBoundingClientRect();
      stageSize = { width: bounds.width, height: bounds.height };
    };
    updateStageSize();
    if (stage) {
      stageResizeObserver = new ResizeObserver(updateStageSize);
      stageResizeObserver.observe(stage);
    }

    if (canvas) {
      compositor = new CanvasCompositor(canvas, {
        width: canvasWidth,
        height: canvasHeight,
        background: "#050606",
        fit: "contain",
        resolutionScale: previewQuality,
      });
      renderFrame();
    }
    document.addEventListener("fullscreenchange", syncFullscreenState);
    document.fonts.addEventListener("loadingdone", handleFontsLoaded);
  });

  onDestroy(() => {
    document.removeEventListener("fullscreenchange", syncFullscreenState);
    document.fonts.removeEventListener("loadingdone", handleFontsLoaded);
    releaseTransformPointer();
    stageResizeObserver?.disconnect();
    stageResizeObserver = null;
    stopClock();
    if (transportSeekRafId !== null) cancelAnimationFrame(transportSeekRafId);
    if (coverNoticeTimeout !== null) window.clearTimeout(coverNoticeTimeout);
    audioMixer?.dispose();
    void audioContext?.close();
    for (const element of elements.values()) {
      if (element instanceof HTMLMediaElement) {
        directPlayback.release(element);
        element.removeAttribute("src");
        element.load();
      } else {
        element.removeAttribute("src");
      }
      element.remove();
    }
    elements.clear();
  });

  $effect(() => {
    const width = canvasWidth;
    const height = canvasHeight;
    if (!compositor) return;
    compositor.resize(width, height);
    // renderFrame reads `plan`. Without untrack this resize-only effect also
    // subscribes to every scrub plan and resets the canvas on each pointer move.
    untrack(renderFrame);
  });

  $effect(() => {
    plan;
    ensurePlanElements();
    syncMediaToPlan();
    renderFrame();
  });

  $effect(() => {
    const revision = transportRevision;
    if (revision === appliedTransportRevision) return;
    appliedTransportRevision = revision;
    // An explicit rewind/source replacement is not continuous playback. Stop
    // every decoder first and invalidate wall-clock continuity so the next
    // plan sync must seek both the picture and the active soundtrack.
    untrack(() => {
      previousSync = null;
      lastTick = null;
      pauseMedia();
      void audioMixer?.sync(plan.audio, { playing: false, rampSeconds: 0 });
      ensurePlanElements();
      syncMediaToPlan();
      renderFrame();
    });
  });

  $effect(() => {
    if (isPlaying) void activatePlayback();
    else {
      stopClock();
      pauseMedia();
      void audioMixer?.sync(plan.audio, { playing: false });
    }
    return stopClock;
  });

  async function activatePlayback() {
    try {
      await ensureAudioGraph();
    } catch (error) {
      mediaError = error instanceof Error ? error.message : String(error);
    }
    if (!isPlaying) return;
    ensurePlanElements();
    syncMediaToPlan();
    startClock();
  }

  function startClock() {
    if (rafId !== null) return;
    lastTick = null;
    const tick = (timestamp: number) => {
      if (!isPlaying) {
        stopClock();
        return;
      }
      if (lastTick !== null) {
        // Follow real elapsed time. Capping this delta made the playhead lag
        // behind media decoding after a slow frame and forced repeated seeks.
        const delta = Math.max(0, (timestamp - lastTick) / 1000);
        currentTime = Math.min(duration, currentTime + delta);
        if (currentTime >= duration) {
          isPlaying = false;
          stopClock();
          return;
        }
      }
      lastTick = timestamp;
      // Updating currentTime creates a new frame plan. That plan effect performs
      // the single compositor draw for this tick.
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
  }

  function stopClock() {
    if (rafId !== null) cancelAnimationFrame(rafId);
    rafId = null;
    lastTick = null;
  }

  function togglePlayback() {
    if (isPlaying) {
      isPlaying = false;
      return;
    }
    currentTime = playbackStartTime(currentTime, duration);
    isPlaying = true;
    // Start audio initialization inside the user gesture. The reactive playback
    // effect remains the single fallback for toggles coming from the app shell.
    void activatePlayback();
  }

  function syncFullscreenState() {
    isFullscreen = document.fullscreenElement === player;
  }

  function openPreviewContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    previewContextMenu = {
      open: true,
      x: event.clientX,
      y: event.clientY,
    };
  }

  function closePreviewContextMenu() {
    previewContextMenu = { ...previewContextMenu, open: false };
  }

  function stepFrame(direction: -1 | 1) {
    isPlaying = false;
    currentTime = Math.min(
      Math.max(duration, 0),
      Math.max(0, currentTime + direction / 30),
    );
  }

  function setPreviewQuality(quality: 1 | 0.5 | 0.25) {
    if (previewQuality === quality) return;
    previewQuality = quality;
    if (!compositor) return;
    try {
      compositor.setResolutionScale(quality);
      renderFrame();
      mediaError = null;
    } catch (error) {
      mediaError = error instanceof Error ? error.message : String(error);
    }
  }

  async function copyCurrentFrame() {
    if (!canvas) return;
    try {
      if (!navigator.clipboard?.write || typeof ClipboardItem === "undefined") {
        throw new Error("Bu sistem görüntüyü panoya kopyalamayı desteklemiyor.");
      }
      const blob = await new Promise<Blob>((resolve, reject) => {
        canvas?.toBlob((result) => {
          if (result) resolve(result);
          else reject(new Error("Önizleme karesi PNG olarak hazırlanamadı."));
        }, "image/png");
      });
      await navigator.clipboard.write([
        new ClipboardItem({ "image/png": blob }),
      ]);
      mediaError = null;
    } catch (error) {
      mediaError = `Kare panoya kopyalanamadı: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  function showCoverNotice(message: string) {
    coverNotice = message;
    if (coverNoticeTimeout !== null) window.clearTimeout(coverNoticeTimeout);
    coverNoticeTimeout = window.setTimeout(() => {
      coverNotice = null;
      coverNoticeTimeout = null;
    }, 2_600);
  }

  function captureCurrentFrameAsCover() {
    if (!canvas || !onCoverCapture || plan.layers.length === 0) return;
    try {
      // Keep the project file small while preserving a crisp cover in every
      // aspect ratio. This copies the compositor, not a raw source-video frame.
      const longestEdge = Math.max(canvas.width, canvas.height);
      const scale = longestEdge > 960 ? 960 / longestEdge : 1;
      const width = Math.max(1, Math.round(canvas.width * scale));
      const height = Math.max(1, Math.round(canvas.height * scale));
      const capture = document.createElement("canvas");
      capture.width = width;
      capture.height = height;
      const context = capture.getContext("2d");
      if (!context) throw new Error("Kapak görseli için çizim alanı oluşturulamadı.");
      context.drawImage(canvas, 0, 0, width, height);
      const imageDataUrl = capture.toDataURL("image/jpeg", 0.88);
      if (imageDataUrl.length > 2_500_000) {
        throw new Error("Kapak görseli çok büyük; önizleme kalitesini düşürüp tekrar deneyin.");
      }
      onCoverCapture({
        imageDataUrl,
        timeMs: Math.max(0, Math.round(currentTime * 1_000)),
        aspect,
        updatedAtMs: Date.now(),
      });
      showCoverNotice("✓ Bu kare proje kapağı olarak kaydedildi");
    } catch (error) {
      mediaError = `Kapak oluşturulamadı: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else if (player) await player.requestFullscreen();
    } catch (error) {
      mediaError = `Tam ekran açılamadı: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  function checkedLabel(active: boolean, label: string) {
    return `${active ? "✓ " : ""}${label}`;
  }

  function getPreviewContextMenuItems(): ContextMenuItem[] {
    return [
      {
        id: "toggle-playback",
        label: isPlaying ? "Duraklat" : "Oynat",
        shortcut: "Space",
        onSelect: () => void togglePlayback(),
      },
      {
        id: "previous-frame",
        label: "Önceki kare",
        shortcut: "Alt ←",
        onSelect: () => stepFrame(-1),
      },
      {
        id: "next-frame",
        label: "Sonraki kare",
        shortcut: "Alt →",
        onSelect: () => stepFrame(1),
      },
      { id: "playback-divider", divider: true },
      {
        id: "quality-full",
        label: checkedLabel(previewQuality === 1, "Önizleme kalitesi: Tam"),
        onSelect: () => setPreviewQuality(1),
      },
      {
        id: "quality-half",
        label: checkedLabel(previewQuality === 0.5, "Önizleme kalitesi: 1/2"),
        onSelect: () => setPreviewQuality(0.5),
      },
      {
        id: "quality-quarter",
        label: checkedLabel(previewQuality === 0.25, "Önizleme kalitesi: 1/4"),
        onSelect: () => setPreviewQuality(0.25),
      },
      { id: "guides-divider", divider: true },
      {
        id: "action-safe",
        label: checkedLabel(showActionSafe, "Eylem güvenli alanı"),
        onSelect: () => (showActionSafe = !showActionSafe),
      },
      {
        id: "title-safe",
        label: checkedLabel(showTitleSafe, "Başlık güvenli alanı"),
        onSelect: () => (showTitleSafe = !showTitleSafe),
      },
      {
        id: "thirds-grid",
        label: checkedLabel(showThirdsGrid, "Üçler ızgarası"),
        onSelect: () => (showThirdsGrid = !showThirdsGrid),
      },
      {
        id: "center-guides",
        label: checkedLabel(showCenterGuides, "Merkez çizgileri"),
        onSelect: () => (showCenterGuides = !showCenterGuides),
      },
      { id: "frame-divider", divider: true },
      {
        id: "copy-frame",
        label: "Geçerli kareyi panoya kopyala",
        disabled: plan.layers.length === 0,
        onSelect: () => void copyCurrentFrame(),
      },
      {
        id: "set-cover",
        label: coverTimeMs === null ? "Bu kareyi kapak yap" : "Kapak karesini güncelle",
        disabled: plan.layers.length === 0 || !onCoverCapture,
        onSelect: captureCurrentFrameAsCover,
      },
      {
        id: "reset-transform",
        label: "Seçili klibin dönüşümünü sıfırla",
        disabled: !selectedClipId || !onResetTransform || transformDisabled,
        onSelect: onResetTransform,
      },
      { id: "window-divider", divider: true },
      {
        id: "detach-preview",
        label: "Ayrı pencerede aç",
        disabled: !onDetach,
        onSelect: onDetach,
      },
      {
        id: "fullscreen",
        label: isFullscreen ? "Tam ekrandan çık" : "Tam ekrana geç",
        disabled:
          typeof document === "undefined" ||
          (!document.fullscreenEnabled && !isFullscreen),
        onSelect: () => void toggleFullscreen(),
      },
    ];
  }

  function getLayerBounds(layer: CompositorLayer): LayerBounds {
    if (layer.kind !== "text" || !layer.text) {
      const viewportWidth = (layer.viewport?.width ?? 1) * canvasWidth;
      const viewportHeight = (layer.viewport?.height ?? 1) * canvasHeight;
      return {
        left: -viewportWidth / 2,
        top: -viewportHeight / 2,
        width: viewportWidth,
        height: viewportHeight,
      };
    }

    const text = layer.text;
    const lines = text.content.split(/\r?\n/);
    const resolvedLines = lines.length > 0 ? lines : [""];
    let measuredWidth = Math.max(text.fontSize * 0.75, 1);

    if (typeof document !== "undefined") {
      textMeasureCanvas ??= document.createElement("canvas");
      const context = textMeasureCanvas.getContext("2d");
      if (context) {
        context.font = `${text.fontWeight} ${text.fontSize}px ${text.fontFamily}`;
        measuredWidth = Math.max(
          measuredWidth,
          ...resolvedLines.map((line) => context.measureText(line || " ").width),
        );
      }
    } else {
      measuredWidth = Math.max(
        measuredWidth,
        ...resolvedLines.map((line) => Math.max(1, line.length) * text.fontSize * 0.58),
      );
    }

    measuredWidth = Math.min(canvasWidth, Math.max(text.fontSize, measuredWidth));
    const lineHeight = text.fontSize * 1.2;
    const blockHeight = Math.max(lineHeight, resolvedLines.length * lineHeight);
    const padding = Math.max(8, text.fontSize * 0.2);

    return {
      left: -measuredWidth / 2 - padding,
      top: -blockHeight / 2 - padding,
      width: measuredWidth + padding * 2,
      height: blockHeight + padding * 2,
    };
  }

  function getSelectionLayout(
    layer: CompositorLayer,
    bounds: LayerBounds,
  ): SelectionLayout {
    const displayScaleX = canvasWidth > 0 ? canvasDisplaySize.width / canvasWidth : 0;
    const displayScaleY = canvasHeight > 0 ? canvasDisplaySize.height / canvasHeight : 0;
    const layerScale = Math.max(0.001, layer.transform.scale);
    const localCenterX = (bounds.left + bounds.width / 2) * displayScaleX * layerScale;
    const localCenterY = (bounds.top + bounds.height / 2) * displayScaleY * layerScale;
    const width = Math.max(24, bounds.width * displayScaleX * layerScale);
    const height = Math.max(20, bounds.height * displayScaleY * layerScale);
    const viewportCenterX =
      ((layer.viewport?.x ?? 0) + (layer.viewport?.width ?? 1) / 2) * canvasWidth;
    const viewportCenterY =
      ((layer.viewport?.y ?? 0) + (layer.viewport?.height ?? 1) / 2) * canvasHeight;

    return {
      originX: (viewportCenterX + layer.transform.x) * displayScaleX,
      originY: (viewportCenterY + layer.transform.y) * displayScaleY,
      left: localCenterX - width / 2,
      top: localCenterY - height / 2,
      width,
      height,
      rotation: layer.transform.rotation,
    };
  }

  function selectionKindLabel(layer: CompositorLayer) {
    if (layer.kind === "text") return "METİN";
    if (layer.kind === "image") return "GÖRSEL";
    return "VİDEO";
  }

  function handleStageClick() {
    void togglePlayback();
  }

  function handleStageKeydown(event: KeyboardEvent) {
    if (event.target !== event.currentTarget) return;
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    void togglePlayback();
  }

  function handleTransportKeydown(event: KeyboardEvent) {
    const shortcut = resolveTransportShortcut(event);
    if (shortcut?.kind !== "seek-relative") return;
    event.preventDefault();
    event.stopPropagation();
    currentTime = applyTransportSeek(currentTime, duration, shortcut);
  }

  function startTransformDrag(event: PointerEvent, mode: TransformDragMode) {
    const layer = selectedLayer;
    if (event.button !== 0 || !canvas || !layer) return;
    event.preventDefault();
    event.stopPropagation();
    if (mode === "move" ? !canMoveSelection : !canResizeSelection) return;

    const captureTarget = event.currentTarget as HTMLElement;
    const frame = captureTarget.closest(".selection-frame") as HTMLElement | null;
    const frameBounds = frame?.getBoundingClientRect();
    if (!frameBounds) return;

    const centerClientX = frameBounds.left + frameBounds.width / 2;
    const centerClientY = frameBounds.top + frameBounds.height / 2;
    const deltaX = event.clientX - centerClientX;
    const deltaY = event.clientY - centerClientY;
    transformDrag = {
      mode,
      clipId: layer.clipId,
      pointerId: event.pointerId,
      captureTarget,
      clientX: event.clientX,
      clientY: event.clientY,
      centerClientX,
      centerClientY,
      startDistance: Math.max(1, Math.hypot(deltaX, deltaY)),
      startAngle: Math.atan2(deltaY, deltaX) * (180 / Math.PI),
      x: layer.transform.x,
      y: layer.transform.y,
      scale: layer.transform.scale,
      rotation: layer.transform.rotation,
    };
    try {
      captureTarget.setPointerCapture(event.pointerId);
    } catch {
      // Embedded webviews may not support pointer capture for every element.
    }
  }

  function moveTransformDrag(event: PointerEvent) {
    const drag = transformDrag;
    if (!canvas || !drag || event.pointerId !== drag.pointerId) return;
    if (!selectedLayer || selectedLayer.clipId !== drag.clipId) {
      releaseTransformPointer();
      return;
    }
    event.preventDefault();
    event.stopPropagation();

    if (drag.mode === "scale") {
      const distance = Math.hypot(
        event.clientX - drag.centerClientX,
        event.clientY - drag.centerClientY,
      );
      let scale = clamp(drag.scale * (distance / drag.startDistance), 0.1, 3);
      // Magnetic snap to 1.0 (original size) if within 0.05
      if (Math.abs(scale - 1.0) <= 0.05) scale = 1.0;
      else if (Math.abs(scale - 2.0) <= 0.05) scale = 2.0;
      else if (Math.abs(scale - 0.5) <= 0.05) scale = 0.5;
      notifyTransformChange({ scale: roundTo(scale, 3) });
      return;
    }

    if (drag.mode === "rotate") {
      const angle =
        Math.atan2(
          event.clientY - drag.centerClientY,
          event.clientX - drag.centerClientX,
        ) *
        (180 / Math.PI);
      let rotation = normalizeRotation(drag.rotation + angle - drag.startAngle);
      // Magnetic snap to 0, 90, -90, 180, -180 if within 4 degrees
      if (Math.abs(rotation) <= 4) rotation = 0;
      else if (Math.abs(rotation - 90) <= 3) rotation = 90;
      else if (Math.abs(rotation + 90) <= 3) rotation = -90;
      else if (Math.abs(rotation - 180) <= 3) rotation = 180;
      else if (Math.abs(rotation + 180) <= 3) rotation = -180;
      notifyTransformChange({ rotation: roundTo(rotation, 1) });
      return;
    }

    const bounds = canvas.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) return;
    let x = drag.x + (event.clientX - drag.clientX) * (canvasWidth / bounds.width);
    let y = drag.y + (event.clientY - drag.clientY) * (canvasHeight / bounds.height);
    // Magnetic snap to 0 (canvas center) if within 6 coordinate units
    if (Math.abs(x) <= 6) x = 0;
    if (Math.abs(y) <= 6) y = 0;
    notifyTransformChange({
      x: roundTo(x, 1),
      y: roundTo(y, 1),
    });
  }

  function endTransformDrag(event: PointerEvent) {
    if (!transformDrag || event.pointerId !== transformDrag.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    releaseTransformPointer();
  }

  function releaseTransformPointer() {
    const drag = transformDrag;
    if (!drag) return;
    try {
      if (drag.captureTarget.hasPointerCapture(drag.pointerId)) {
        drag.captureTarget.releasePointerCapture(drag.pointerId);
      }
    } catch {
      // The pointer may already have been released by the browser.
    }
    transformDrag = null;
  }

  function notifyTransformChange(updates: PreviewTransformUpdate) {
    if (onTransformChange) {
      onTransformChange(updates);
      return;
    }
    if (updates.x === undefined || updates.y === undefined) return;
    onPositionChange?.({ x: updates.x, y: updates.y });
  }

  function handleSelectionKeydown(event: KeyboardEvent) {
    const layer = selectedLayer;
    if (!layer || !canMoveSelection) return;
    const direction =
      event.key === "ArrowLeft"
        ? { x: -1, y: 0 }
        : event.key === "ArrowRight"
          ? { x: 1, y: 0 }
          : event.key === "ArrowUp"
            ? { x: 0, y: -1 }
            : event.key === "ArrowDown"
              ? { x: 0, y: 1 }
              : null;
    if (!direction) return;
    event.preventDefault();
    event.stopPropagation();
    const step = event.shiftKey ? 10 : 1;
    notifyTransformChange({
      x: roundTo(layer.transform.x + direction.x * step, 1),
      y: roundTo(layer.transform.y + direction.y * step, 1),
    });
  }

  function handleScaleKeydown(event: KeyboardEvent) {
    if (!selectedLayer || !canResizeSelection) return;
    if (!["ArrowLeft", "ArrowRight", "ArrowDown", "ArrowUp"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    const direction = event.key === "ArrowRight" || event.key === "ArrowUp" ? 1 : -1;
    const step = event.shiftKey ? 0.25 : 0.05;
    notifyTransformChange({
      scale: roundTo(clamp(selectedLayer.transform.scale + direction * step, 0.1, 3), 3),
    });
  }

  function handleRotationKeydown(event: KeyboardEvent) {
    if (!selectedLayer || !canResizeSelection) return;
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation();
    const direction = event.key === "ArrowRight" ? 1 : -1;
    const step = event.shiftKey ? 15 : 1;
    notifyTransformChange({
      rotation: normalizeRotation(selectedLayer.transform.rotation + direction * step),
    });
  }

  function roundTo(value: number, digits: number) {
    const factor = 10 ** digits;
    return Math.round(value * factor) / factor;
  }

  function clamp(value: number, minimum: number, maximum: number) {
    return Math.min(maximum, Math.max(minimum, value));
  }

  function normalizeRotation(value: number) {
    return ((value + 180) % 360 + 360) % 360 - 180;
  }

  async function ensureAudioGraph() {
    if (!audioContext) {
      audioContext = new AudioContext({ latencyHint: "interactive" });
      audioMixer = new WebAudioMixer(audioContext);
    }
    if (audioContext.state === "suspended") await audioContext.resume();
    attachPendingAudio();
  }

  function attachPendingAudio() {
    if (!audioMixer) return;
    for (const source of plan.audio) {
      const elementKey = audioElementKey(source);
      const element = elements.get(elementKey);
      if (!(element instanceof HTMLMediaElement)) continue;
      const currentElementKey = attachedAudio.get(source.clipId);
      if (currentElementKey === elementKey) continue;
      try {
        const visualElement = elements.get(source.clipId);
        const replacingVisibleVideo =
          currentElementKey === source.clipId && elementKey !== source.clipId;
        // A cleaned companion WAV bypasses the visible video's mixer strip.
        // Capture that video anyway so its original native soundtrack cannot
        // leak underneath the cleaned result on first playback.
        if (
          elementKey !== source.clipId &&
          visualElement instanceof HTMLVideoElement
        ) {
          audioMixer.capture(visualElement);
        }
        audioMixer.attach(source.clipId, element, {
          // Swapping only the soundtrack must not pause the video decoder and
          // freeze the current frame.
          pausePreviousElement: !replacingVisibleVideo,
        });
        attachedAudio.set(source.clipId, elementKey);
      } catch (error) {
        mediaError = error instanceof Error ? error.message : String(error);
      }
    }
  }

  function ensurePlanElements() {
    if (!mediaHost) return;
    for (const layer of plan.layers) {
      if (layer.kind === "video" || layer.kind === "image") {
        ensureElement(layer.clipId, layer.file, layer.kind);
      }
    }
    for (const source of plan.audio) {
      const elementKey = audioElementKey(source);
      ensureElement(elementKey, source.file, "audio");
    }
    attachPendingAudio();
  }

  function audioElementKey(source: AudioMixSource) {
    const visualLayer = plan.layers.find(
      (layer) => layer.clipId === source.clipId && layer.kind === "video",
    );
    return visualLayer && visualLayer.file !== source.file
      // Keep each alternative soundtrack warm. A/B/C comparisons can then
      // switch back without destroying/reloading the previously decoded WAV.
      ? `${source.clipId}::alternate-audio::${source.file}`
      : source.clipId;
  }

  function ensureElement(
    elementKey: string,
    path: string,
    kind: "video" | "image" | "audio",
  ) {
    const existing = elements.get(elementKey);
    if (existing && elementPaths.get(elementKey) === path) return existing;
    if (existing) {
      for (const [clipId, attachedElementKey] of attachedAudio) {
        if (attachedElementKey !== elementKey) continue;
        audioMixer?.detach(clipId);
        attachedAudio.delete(clipId);
      }
      if (existing instanceof HTMLMediaElement) directPlayback.release(existing);
      existing.remove();
    }

    const element = kind === "image" ? new Image() : document.createElement(kind);
    element.dataset.clipId = elementKey.split("::", 1)[0];
    element.className = "decode-source";
    const sourceUrl = convertFileSrc(path);
    if (element instanceof HTMLMediaElement) {
      element.preload = "auto";
      element.crossOrigin = "anonymous";
      if (element instanceof HTMLVideoElement) element.playsInline = true;
      element.onloadeddata = renderFrame;
      element.onloadedmetadata = () => {
        if (Number.isFinite(element.duration) && element.duration > 0) {
          onMediaDuration?.(element.duration);
        }
        syncMediaToPlan();
        renderFrame();
      };
      element.onseeked = () => handleSeeked(element);
      element.onerror = () => (mediaError = `Medya çözümlenemedi: ${fileName(path)}`);
      element.src = sourceUrl;
    } else {
      element.crossOrigin = "anonymous";
      element.onload = renderFrame;
      element.onerror = () => (mediaError = `Görsel yüklenemedi: ${fileName(path)}`);
      element.src = sourceUrl;
    }
    mediaHost?.appendChild(element);
    elements.set(elementKey, element);
    elementPaths.set(elementKey, path);
    return element;
  }

  function syncMediaToPlan() {
    const wallTime = performance.now() / 1000;
    const planDelta = previousSync ? plan.time - previousSync.planTime : 0;
    const wallDelta = previousSync ? wallTime - previousSync.wallTime : 0;
    const isContinuousPlayback = Boolean(
      isPlaying &&
      previousSync?.playing &&
      planDelta >= -0.01 &&
      Math.abs(planDelta - wallDelta) < 0.2,
    );

    const wasPlaying = previousSync?.playing;
    const isContinuousPause = Boolean(
      wasPlaying &&
      !isPlaying &&
      planDelta >= -0.01 &&
      Math.abs(planDelta - wallDelta) < 0.2,
    );
    if (isContinuousPause) {
      // Transitioning from playing to paused. Find a master media element to sync playhead.
      let masterElement: HTMLMediaElement | null = null;
      let masterLayerSourceTime = 0;
      
      // Look for video elements first
      for (const layer of plan.layers) {
        const element = elements.get(layer.clipId);
        if (element instanceof HTMLVideoElement && Number.isFinite(element.duration) && element.playbackRate > 0) {
          masterElement = element;
          masterLayerSourceTime = layer.sourceTime;
          break;
        }
      }
      // If no video elements, look for audio elements
      if (!masterElement) {
        for (const source of plan.audio) {
          const element = elements.get(audioElementKey(source));
          if (element instanceof HTMLMediaElement && !(element instanceof HTMLVideoElement) && Number.isFinite(element.duration) && element.playbackRate > 0) {
            masterElement = element;
            masterLayerSourceTime = source.sourceTime;
            break;
          }
        }
      }
      
      if (masterElement) {
        const playbackRate = masterElement.playbackRate;
        const sourceDelta = masterElement.currentTime - masterLayerSourceTime;
        const timelineDelta = sourceDelta / playbackRate;
        if (Math.abs(timelineDelta) < 1.0) {
          const adjustedTime = Math.min(duration, Math.max(0, plan.time + timelineDelta));
          if (Math.abs(currentTime - adjustedTime) > 0.001) {
            currentTime = adjustedTime;
            plan.time = adjustedTime;
            
            // Adjust each layer's sourceTime so seekIfNeeded uses the adjusted time
            for (const layer of plan.layers) {
              const clipRate = findPlaybackRate(layer.clipId);
              layer.sourceTime += timelineDelta * clipRate;
            }
            for (const source of plan.audio) {
              source.sourceTime += timelineDelta * source.playbackRate;
            }
          }
        }
      }
    }

    const allowTimelineSeek = !isContinuousPlayback;

    // Elements are cached for smooth scrubbing. Stop cached media that has
    // left the current frame plan; a clean companion audio strip means its
    // visible video is no longer represented in the mixer strip map.
    const activeElementKeys = new Set<string>();
    for (const layer of plan.layers) {
      if (layer.kind === "video") activeElementKeys.add(layer.clipId);
    }
    for (const source of plan.audio) activeElementKeys.add(audioElementKey(source));
    for (const [elementKey, element] of elements) {
      if (
        element instanceof HTMLMediaElement &&
        !activeElementKeys.has(elementKey)
      ) {
        directPlayback.pause(element);
      }
    }

    for (const layer of plan.layers) {
      const element = elements.get(layer.clipId);
      if (!(element instanceof HTMLVideoElement)) continue;
      const audioSource = plan.audio.find((source) => source.clipId === layer.clipId);
      // Keep native media audio from escaping before the AudioContext graph is
      // ready. A companion clean WAV (or an intentionally absent strip) owns
      // the soundtrack instead of the visible video in those cases.
      element.muted =
        !audioSource || audioElementKey(audioSource) !== layer.clipId;
      element.playbackRate = findPlaybackRate(layer.clipId);
      seekIfNeeded(element, layer.sourceTime, allowTimelineSeek || element.paused);
      if (isPlaying && element.paused) void directPlayback.play(element);
      if (!isPlaying) directPlayback.pause(element);
    }
    for (const source of plan.audio) {
      const element = elements.get(audioElementKey(source));
      if (!(element instanceof HTMLMediaElement)) continue;
      // Video clips were already synchronized above. Running the same seek a
      // second time through their audio strip caused the visible frame repeat.
      if (element instanceof HTMLVideoElement) continue;
      element.playbackRate = source.playbackRate;
      seekIfNeeded(element, source.sourceTime, allowTimelineSeek || element.paused);
    }
    if (audioMixer) {
      void audioMixer
        .sync(plan.audio, {
          playing: isPlaying,
        })
        .catch((error) => {
          mediaError = error instanceof Error ? error.message : String(error);
        });
    }
    previousSync = { planTime: plan.time, wallTime, playing: isPlaying };
  }

  function findPlaybackRate(clipId: string) {
    return plan.audio.find((source) => source.clipId === clipId)?.playbackRate ?? 1;
  }

  function seekIfNeeded(element: HTMLMediaElement, target: number, allowSeek: boolean) {
    if (!allowSeek) return;
    const safeTarget = Math.max(0, target);
    if (Number.isFinite(element.duration)) {
      const resolvedTarget = Math.min(
        safeTarget,
        Math.max(0, element.duration - 0.001),
      );
      if (element.seeking) {
        // Scrubbing can issue a newer target before the previous seek settles.
        // During playback, chasing every intermediate target causes flicker.
        if (isPlaying) pendingTransportSeeks.set(element, resolvedTarget);
        else pendingPausedSeeks.set(element, resolvedTarget);
        return;
      }
      if (Math.abs(element.currentTime - resolvedTarget) > PAUSED_SEEK_TOLERANCE) {
        try {
          pendingPausedSeeks.delete(element);
          element.currentTime = resolvedTarget;
        } catch {
          // Metadata may still be loading; onloadeddata will retry on the next frame plan.
        }
      }
    }
  }

  function handleSeeked(element: HTMLMediaElement) {
    if (isPlaying) {
      // A transport jump can arrive while an older seek is still decoding.
      // Always chase that newest discontinuity before allowing playback;
      // otherwise a cached clean WAV can resume from its former future time.
      const pendingTarget = pendingTransportSeeks.get(element);
      pendingTransportSeeks.delete(element);
      if (
        pendingTarget !== undefined &&
        Math.abs(element.currentTime - pendingTarget) > PAUSED_SEEK_TOLERANCE
      ) {
        try {
          element.currentTime = pendingTarget;
          return;
        } catch {
          // The settled frame below is still safe to display.
        }
      }
      renderFrame();
      return;
    }
    {
      // Publish every decoded intermediate frame while the user scrubs. The
      // newest pointer target is still chased immediately afterwards.
      renderFrame();
      const pendingTarget = pendingPausedSeeks.get(element);
      pendingPausedSeeks.delete(element);
      if (
        pendingTarget !== undefined &&
        Math.abs(element.currentTime - pendingTarget) > PAUSED_SEEK_TOLERANCE
      ) {
        try {
          element.currentTime = pendingTarget;
          return;
        } catch {
          // The settled frame below is still safe to display.
        }
      }
      return;
    }
  }

  function pauseMedia() {
    for (const element of elements.values()) {
      if (element instanceof HTMLMediaElement) directPlayback.pause(element);
    }
  }

  function renderFrame() {
    if (!compositor) return;
    const resolvedSources = new Map<string, HTMLVideoElement | HTMLImageElement>();
    for (const layer of plan.layers) {
      if (layer.kind === "text") continue;
      const element = elements.get(layer.clipId);
      if (
        element instanceof HTMLVideoElement &&
        !element.seeking &&
        element.readyState >= 2 &&
        element.videoWidth > 0 &&
        element.videoHeight > 0
      ) {
        resolvedSources.set(layer.clipId, element);
        continue;
      }
      if (
        element instanceof HTMLImageElement &&
        element.complete &&
        element.naturalWidth > 0 &&
        element.naturalHeight > 0
      ) {
        resolvedSources.set(layer.clipId, element);
        continue;
      }

      // Keep the last complete canvas frame while a seek/decode is pending.
      // Clearing first and drawing nothing caused the visible black flash.
      return;
    }
    compositor.render(plan, (clipId) => {
      return resolvedSources.get(clipId) ?? null;
    });
  }

  function changeAspect(next: "9:16" | "1:1" | "16:9") {
    onAspectChange?.(next);
  }

  function handleTransportScrubStart() {
    // Seeking preserves transport state: playing media continues from the
    // chosen position, while paused media remains paused.
  }

  function handleTransportScrubInput(event: Event) {
    const nextTime = Number((event.currentTarget as HTMLInputElement).value);
    if (!Number.isFinite(nextTime)) return;
    pendingTransportTime = nextTime;
    if (transportSeekRafId !== null) return;
    transportSeekRafId = requestAnimationFrame(() => {
      transportSeekRafId = null;
      applyPendingTransportTime();
    });
  }

  function finishTransportScrub(event: Event) {
    const nextTime = Number((event.currentTarget as HTMLInputElement).value);
    if (Number.isFinite(nextTime)) pendingTransportTime = nextTime;
    if (transportSeekRafId !== null) cancelAnimationFrame(transportSeekRafId);
    transportSeekRafId = null;
    applyPendingTransportTime();
  }

  function applyPendingTransportTime() {
    if (pendingTransportTime === null) return;
    currentTime = Math.min(Math.max(duration, 0), Math.max(0, pendingTransportTime));
    pendingTransportTime = null;
  }

  function fileName(path: string) {
    return path.split(/[/\\]/).pop() || path;
  }

  function formatTime(seconds: number) {
    const totalFrames = Math.max(0, Math.floor(seconds * 30));
    const frame = totalFrames % 30;
    const totalSeconds = Math.floor(totalFrames / 30);
    const s = totalSeconds % 60;
    const m = Math.floor(totalSeconds / 60) % 60;
    const h = Math.floor(totalSeconds / 3600);
    return `${h.toString().padStart(2, "0")}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}:${frame.toString().padStart(2, "0")}`;
  }
</script>

<svelte:window
  onpointermove={moveTransformDrag}
  onpointerup={endTransformDrag}
  onpointercancel={endTransformDrag}
/>

<div class="player" bind:this={player}>
  <header>
    <div class="timecode">{formatTime(currentTime)} <span>/ {formatTime(duration)}</span></div>
    <div class="header-actions">
      <div class="ratios" aria-label="Önizleme kadrosu">
        {#each ["9:16", "1:1", "16:9"] as ratio}
          <button class:active={aspect === ratio} onclick={() => changeAspect(ratio as typeof aspect)}>{ratio}</button>
        {/each}
      </div>
      {#if onCoverCapture}
        <button
          class="cover-button"
          class:active={coverTimeMs !== null}
          disabled={plan.layers.length === 0}
          onclick={captureCurrentFrameAsCover}
          title={coverTimeMs === null
            ? "Geçerli birleşik kareyi proje kapağı yap"
            : `Kapak: ${formatTime(coverTimeMs / 1_000)} · Güncellemek için tıklayın`}
        >▣ Kapak</button>
      {/if}
      {#if onDetach}
        <button class="detach" onclick={onDetach} title="Önizlemeyi ayır" aria-label="Önizlemeyi ayır">↗</button>
      {/if}
    </div>
  </header>

  <div
    bind:this={stage}
    class="stage"
    onclick={handleStageClick}
    onkeydown={handleStageKeydown}
    oncontextmenu={openPreviewContextMenu}
    role="button"
    tabindex="0"
    aria-label={isPlaying ? "Duraklat" : "Oynat"}
    aria-pressed={isPlaying}
    aria-keyshortcuts="Space Enter ArrowLeft ArrowRight Shift+ArrowLeft Shift+ArrowRight Alt+ArrowLeft Alt+ArrowRight"
    title="Space: oynat/duraklat · ←/→: 1 sn · Shift: 5 sn · Alt: 1 kare"
  >
    <div
      class="canvas-shell"
      style:width={`${canvasDisplaySize.width}px`}
      style:height={`${canvasDisplaySize.height}px`}
    >
      <canvas
        bind:this={canvas}
        class:positionable={Boolean(selectedLayer && canMoveSelection)}
        class:dragging={transformDrag?.mode === "move"}
        aria-hidden="true"
      ></canvas>

      {#if showActionSafe || showTitleSafe || showCenterGuides || showThirdsGrid}
        <div class="viewer-guides" aria-hidden="true">
          {#if showActionSafe}<span class="safe-zone action-safe"></span>{/if}
          {#if showTitleSafe}<span class="safe-zone title-safe"></span>{/if}
          {#if showCenterGuides}<span class="center-guides"></span>{/if}
          {#if showThirdsGrid}
            <span class="thirds-line vertical first"></span>
            <span class="thirds-line vertical second"></span>
            <span class="thirds-line horizontal first"></span>
            <span class="thirds-line horizontal second"></span>
          {/if}
        </div>
      {/if}

      {#if selectedLayer && selectionLayout && canvasDisplaySize.width > 0 && canvasDisplaySize.height > 0}
        <div
          class="transform-origin"
          style:left={`${selectionLayout.originX}px`}
          style:top={`${selectionLayout.originY}px`}
          style:transform={`rotate(${selectionLayout.rotation}deg)`}
        >
          <div
            class="selection-frame"
            class:moving={transformDrag?.mode === "move"}
            class:locked={transformDisabled}
            style:left={`${selectionLayout.left}px`}
            style:top={`${selectionLayout.top}px`}
            style:width={`${selectionLayout.width}px`}
            style:height={`${selectionLayout.height}px`}
            onpointerdown={(event) => startTransformDrag(event, "move")}
            onclick={(event) => event.stopPropagation()}
            onkeydown={handleSelectionKeydown}
            role="button"
            tabindex="0"
            aria-label={`${selectionKindLabel(selectedLayer)} katmanı seçili. ${transformDisabled ? "Kanal kilitli." : "Taşımak için sürükleyin."}`}
            title={transformDisabled
              ? "Kanal kilitli; dönüşümü değiştirmek için kilidi açın"
              : "Taşımak için sürükleyin · Köşelerden ölçekleyin · Üst tutamaçtan döndürün"}
          >
            <span class="selection-label">{selectionKindLabel(selectedLayer)}</span>
            <span class="selection-hint">
              {transformDisabled
                ? "Kanal kilitli"
                : canResizeSelection
                  ? "Taşı · Ölçekle · Döndür"
                  : "Taşımak için sürükle"}
            </span>

            <span class="rotation-stem" aria-hidden="true"></span>
            <button
              type="button"
              class="rotation-handle"
              disabled={!canResizeSelection}
              aria-label="Seçili katmanı döndür"
              title={transformDisabled ? "Kanal kilitli" : "Döndürmek için sürükleyin"}
              onpointerdown={(event) => startTransformDrag(event, "rotate")}
              onkeydown={handleRotationKeydown}
            ></button>

            {#each [
              { position: "top-left", label: "Sol üst köşeden ölçekle" },
              { position: "top-right", label: "Sağ üst köşeden ölçekle" },
              { position: "bottom-left", label: "Sol alt köşeden ölçekle" },
              { position: "bottom-right", label: "Sağ alt köşeden ölçekle" },
            ] as handle}
              <button
                type="button"
                class={`scale-handle ${handle.position}`}
                disabled={!canResizeSelection}
                aria-label={handle.label}
                title={transformDisabled ? "Kanal kilitli" : handle.label}
                onpointerdown={(event) => startTransformDrag(event, "scale")}
                onkeydown={handleScaleKeydown}
              ></button>
            {/each}
          </div>
        </div>
      {/if}

      {#if plan.layers.length === 0}
        <div class="empty">Zaman çizelgesine medya veya metin ekleyin</div>
      {/if}
      {#if mediaError}
        <div class="media-error">{mediaError}</div>
      {/if}
      {#if coverNotice}
        <div class="cover-notice" role="status">{coverNotice}</div>
      {/if}
    </div>
  </div>

  <div class="transport">
    <button class="play" onclick={togglePlayback} aria-label={isPlaying ? "Duraklat" : "Oynat"}>
      {isPlaying ? "Ⅱ" : "▶"}
    </button>
    <input
      aria-label="Oynatma konumu"
      type="range"
      min="0"
      max={Math.max(duration, 0.001)}
      step="0.01"
      value={currentTime}
      aria-valuetext={formatTime(currentTime)}
      aria-keyshortcuts="ArrowLeft ArrowRight Shift+ArrowLeft Shift+ArrowRight Alt+ArrowLeft Alt+ArrowRight"
      title="←/→: 1 sn · Shift: 5 sn · Alt: 1 kare"
      onpointerdown={handleTransportScrubStart}
      onkeydown={handleTransportKeydown}
      oninput={handleTransportScrubInput}
      onchange={finishTransportScrub}
    />
    <div class="layers" title="Aktif katman / ses şeridi">{plan.layers.length}V · {plan.audio.length}A</div>
  </div>

  <div class="media-host" bind:this={mediaHost} aria-hidden="true"></div>
</div>

<ContextMenu
  open={previewContextMenu.open}
  x={previewContextMenu.x}
  y={previewContextMenu.y}
  items={getPreviewContextMenuItems()}
  onClose={closePreviewContextMenu}
  ariaLabel="Önizleme ayarları"
/>

<style>
  .player { position: relative; display: flex; flex-direction: column; width: 100%; height: 100%; min-height: 0; overflow: hidden; background: #050606; }
  header { height: 34px; flex: 0 0 auto; display: flex; align-items: center; justify-content: space-between; padding: 0 10px 0 12px; background: #111212; border-bottom: 1px solid #242525; }
  .timecode { color: #d9dddb; font: 11px/1 "JetBrains Mono", Consolas, monospace; letter-spacing: .02em; }
  .timecode span { color: #606462; }
  .header-actions, .ratios { display: flex; align-items: center; gap: 4px; }
  .ratios { padding: 2px; background: #181919; border: 1px solid #292a2a; border-radius: 5px; }
  .ratios button, .detach, .cover-button { padding: 3px 6px; color: #666b69; background: transparent; border: 0; border-radius: 3px; font: 9px/1.2 inherit; cursor: pointer; }
  .ratios button.active { color: #d8e4df; background: #2b302e; }
  .cover-button { margin-left: 3px; color: #aeb8b4; border: 1px solid #2c3531; background: #171b19; }
  .cover-button:hover:not(:disabled), .cover-button.active { color: #d8f4e9; border-color: #4f9b81; background: #20332b; }
  .cover-button:disabled { opacity: .45; cursor: not-allowed; }
  .detach { margin-left: 4px; color: #6cd8b3; font-size: 13px; }
  .stage { position: relative; min-height: 0; flex: 1; display: grid; place-items: center; width: 100%; padding: 18px; box-sizing: border-box; overflow: hidden; background: radial-gradient(circle at 50% 45%, #171919 0, #090a0a 58%, #050606 100%); border: 0; cursor: pointer; }
  .stage:focus-visible { outline: 1px solid rgba(102, 231, 207, .7); outline-offset: -2px; }
  .canvas-shell {
    position: relative;
    flex: 0 0 auto;
    overflow: visible;
    background: #000;
    box-shadow: 0 8px 38px rgba(0,0,0,.48);
  }
  canvas {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    background: #000;
    object-fit: contain;
  }
  .cover-notice { position: absolute; z-index: 6; right: 10px; bottom: 10px; max-width: min(310px, calc(100% - 20px)); padding: 7px 9px; color: #d9fff0; background: rgba(18, 52, 41, .94); border: 1px solid rgba(105, 224, 179, .62); border-radius: 5px; box-shadow: 0 5px 18px rgba(0, 0, 0, .34); font-size: 11px; pointer-events: none; }
  .viewer-guides {
    position: absolute;
    inset: 0;
    z-index: 2;
    overflow: hidden;
    pointer-events: none;
  }
  .safe-zone { position: absolute; border: 1px solid rgba(255,255,255,.78); box-shadow: 0 0 0 1px rgba(0,0,0,.42); }
  .safe-zone::after { position: absolute; top: 3px; left: 4px; color: rgba(255,255,255,.8); font: 8px/1 "JetBrains Mono", Consolas, monospace; text-shadow: 0 1px 2px #000; }
  .action-safe { inset: 5%; border-style: dashed; }
  .action-safe::after { content: "ACTION 90%"; }
  .title-safe { inset: 10%; }
  .title-safe::after { content: "TITLE 80%"; }
  .center-guides::before, .center-guides::after, .thirds-line { content: ""; position: absolute; background: rgba(102, 231, 207, .78); box-shadow: 0 0 0 1px rgba(0,0,0,.25); }
  .center-guides::before { top: 0; bottom: 0; left: 50%; width: 1px; }
  .center-guides::after { left: 0; right: 0; top: 50%; height: 1px; }
  .thirds-line { background: rgba(255, 209, 102, .72); }
  .thirds-line.vertical { top: 0; bottom: 0; width: 1px; }
  .thirds-line.horizontal { left: 0; right: 0; height: 1px; }
  .thirds-line.vertical.first { left: 33.333%; }
  .thirds-line.vertical.second { left: 66.666%; }
  .thirds-line.horizontal.first { top: 33.333%; }
  .thirds-line.horizontal.second { top: 66.666%; }
  canvas.positionable { cursor: grab; touch-action: none; }
  canvas.dragging { cursor: grabbing; }
  .transform-origin {
    position: absolute;
    z-index: 4;
    width: 0;
    height: 0;
    transform-origin: 0 0;
    pointer-events: none;
  }
  .selection-frame {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid #66e7cf;
    border-radius: 2px;
    outline: 1px solid rgba(3, 12, 11, .72);
    box-shadow: 0 0 0 1px rgba(102, 231, 207, .22), 0 0 16px rgba(59, 224, 195, .18);
    cursor: grab;
    pointer-events: auto;
    touch-action: none;
  }
  .selection-frame.moving { cursor: grabbing; }
  .selection-frame.locked {
    border-color: rgba(144, 202, 249, .66);
    box-shadow: 0 0 0 1px rgba(144, 202, 249, .14);
    cursor: not-allowed;
    opacity: .72;
  }
  .selection-frame:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 3px;
  }
  .selection-label,
  .selection-hint {
    position: absolute;
    z-index: 2;
    max-width: 220px;
    padding: 3px 5px;
    border: 1px solid rgba(102, 231, 207, .45);
    border-radius: 3px;
    color: #dffff8;
    background: rgba(7, 30, 26, .9);
    box-shadow: 0 2px 8px rgba(0, 0, 0, .4);
    font: 700 8px/1 "JetBrains Mono", Consolas, monospace;
    letter-spacing: .08em;
    white-space: nowrap;
    pointer-events: none;
    user-select: none;
  }
  .selection-label { top: -25px; left: -1px; }
  .selection-hint {
    top: calc(100% + 9px);
    left: 50%;
    transform: translateX(-50%);
    color: #a9c8c1;
    font-weight: 500;
    letter-spacing: .02em;
  }
  .rotation-stem {
    position: absolute;
    left: 50%;
    top: -31px;
    width: 1px;
    height: 30px;
    background: #66e7cf;
    box-shadow: 0 0 0 1px rgba(3, 12, 11, .55);
    pointer-events: none;
  }
  .rotation-handle,
  .scale-handle {
    position: absolute;
    z-index: 3;
    display: block;
    width: 15px;
    height: 15px;
    margin: 0;
    padding: 0;
    border: 2px solid #073a31;
    border-radius: 3px;
    background: #78f1d9;
    box-shadow: 0 0 0 1px rgba(223, 255, 249, .9), 0 2px 6px rgba(0, 0, 0, .52);
    touch-action: none;
  }
  .rotation-handle:hover:not(:disabled),
  .scale-handle:hover:not(:disabled) { background: #d6fff7; }
  .rotation-handle:focus-visible,
  .scale-handle:focus-visible { outline: 2px solid #fff; outline-offset: 2px; }
  .rotation-handle:disabled,
  .scale-handle:disabled { cursor: not-allowed; filter: saturate(.3); opacity: .62; }
  .rotation-handle {
    top: -42px;
    left: 50%;
    border-radius: 50%;
    transform: translate(-50%, -50%);
    cursor: grab;
  }
  .rotation-handle:active { cursor: grabbing; }
  .scale-handle.top-left { top: 0; left: 0; transform: translate(-50%, -50%); cursor: nwse-resize; }
  .scale-handle.top-right { top: 0; right: 0; transform: translate(50%, -50%); cursor: nesw-resize; }
  .scale-handle.bottom-left { bottom: 0; left: 0; transform: translate(-50%, 50%); cursor: nesw-resize; }
  .scale-handle.bottom-right { right: 0; bottom: 0; transform: translate(50%, 50%); cursor: nwse-resize; }
  .empty { position: absolute; inset: 0; z-index: 3; display: grid; place-items: center; color: #4e5351; font-size: 11px; letter-spacing: .04em; pointer-events: none; }
  .media-error { position: absolute; z-index: 3; left: 12px; right: 12px; bottom: 10px; padding: 7px 9px; color: #eab0a6; text-align: left; background: rgba(84, 25, 20, .84); border: 1px solid rgba(223,127,114,.4); border-radius: 5px; font-size: 10px; }
  .transport { height: 38px; flex: 0 0 auto; display: flex; align-items: center; gap: 10px; padding: 0 10px; background: #111212; border-top: 1px solid #242525; }
  .play { width: 25px; height: 25px; color: #e3e5e4; background: #1d1f1e; border: 1px solid #303331; border-radius: 50%; cursor: pointer; font-size: 10px; }
  input[type="range"] { min-width: 0; flex: 1; height: 3px; accent-color: #65d7b2; cursor: pointer; }
  .layers { color: #626765; font: 9px/1 "JetBrains Mono", Consolas, monospace; }
  .media-host { position: fixed; left: -10000px; top: -10000px; width: 1px; height: 1px; overflow: hidden; opacity: 0; pointer-events: none; }
  :global(.decode-source) { width: 1px; height: 1px; }
</style>
