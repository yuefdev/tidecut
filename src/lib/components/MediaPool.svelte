<script lang="ts">
  import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import type { ContextMenuItem } from "$lib/components/ContextMenu.svelte";
  import { beginPointerDrag, endPointerDrag } from "$lib/stores/drag";
  import { chooseMediaPaths, inspectMedia } from "$lib/project/client";
  import type { MediaKind } from "$lib/project/types";

  interface Props {
    mediaFiles: string[];
    onSelect?: (file: string) => void;
    onAddToTimeline?: (file: string) => void;
    onExtractAudio?: (file: string) => void;
    /** Called right before the media list changes so the edit joins undo history. */
    onHistoryPoint?: () => void;
  }

  let {
    mediaFiles = $bindable([]),
    onSelect,
    onAddToTimeline,
    onExtractAudio,
    onHistoryPoint,
  }: Props = $props();
  let isImporting = $state(false);
  let importNotice = $state<string | null>(null);
  let selectedFile = $state<string | null>(null);
  let contextMenuOpen = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);
  let contextMenuFile = $state<string | null>(null);
  let canRevealFile = $derived(
    typeof window !== "undefined" && isTauri(),
  );
  let contextMenuItems = $derived.by<ContextMenuItem[]>(() => {
    const file = contextMenuFile;
    return [
      {
        id: "preview",
        label: "Önizle",
        disabled: !file,
        onSelect: () => file && handleSelect(file),
      },
      {
        id: "add-to-timeline",
        label: "Zaman çizelgesine ekle",
        disabled: !file || !onAddToTimeline,
        onSelect: () => file && onAddToTimeline?.(file),
      },
      {
        id: "extract-audio",
        label: "Sesi ayır ve Voice olarak ekle",
        disabled: !file || getMediaKind(file) !== "video" || !onExtractAudio,
        onSelect: () => file && onExtractAudio?.(file),
      },
      { id: "file-actions-divider", divider: true },
      {
        id: "reveal",
        label: "Dosya konumunu aç",
        disabled: !file || !canRevealFile,
        onSelect: () => {
          if (file) void revealFile(file);
        },
      },
      { id: "remove-divider", divider: true },
      {
        id: "remove-from-project",
        label: "Projeden kaldır",
        disabled: !file,
        danger: true,
        onSelect: () => file && removeFromProject(file),
      },
    ];
  });

  async function handleImport() {
    if (isImporting) return;
    importNotice = null;
    isImporting = true;
    try {
      const paths = await chooseMediaPaths();
      if (paths.length === 0) return;
      const inspected = await inspectMedia(paths);
      const accepted = inspected.filter(
        (media) =>
          media.exists && media.supported && media.kind && media.quickHash,
      );
      const existing = new Set(mediaFiles.map(normalizePath));
      const newFiles = accepted
        .map((media) => media.path)
        .filter((path) => !existing.has(normalizePath(path)));
      if (newFiles.length > 0) {
        onHistoryPoint?.();
        mediaFiles = [...mediaFiles, ...newFiles];
      }

      const rejectedCount = inspected.length - accepted.length;
      importNotice = rejectedCount
        ? `${newFiles.length} medya eklendi, ${rejectedCount} dosya desteklenmiyor veya okunamadı.`
        : `${newFiles.length} medya eklendi.`;
    } catch (error) {
      importNotice =
        error instanceof Error ? error.message : "Medya içe aktarılamadı.";
    } finally {
      isImporting = false;
    }
  }

  function getFileName(path: string): string {
    return path.split(/[/\\]/).pop() || path;
  }

  function handlePointerDown(e: PointerEvent, file: string) {
    if (e.button !== 0) return;
    beginPointerDrag(file, e.pointerId, e.clientX, e.clientY);
  }

  function handleSelect(file: string) {
    selectedFile = file;
    onSelect?.(file);
  }

  function handleContextMenu(event: MouseEvent, file: string) {
    event.preventDefault();
    event.stopPropagation();
    selectedFile = file;
    contextMenuFile = file;
    contextMenuX = event.clientX;
    contextMenuY = event.clientY;
    contextMenuOpen = true;
  }

  function closeContextMenu() {
    contextMenuOpen = false;
  }

  async function revealFile(file: string) {
    if (!isTauri()) return;
    try {
      await revealItemInDir(file);
    } catch (error) {
      importNotice =
        error instanceof Error
          ? `Dosya konumu açılamadı: ${error.message}`
          : "Dosya konumu açılamadı.";
    }
  }

  function removeFromProject(file: string) {
    const target = normalizePath(file);
    if (!mediaFiles.some((candidate) => normalizePath(candidate) === target)) return;
    onHistoryPoint?.();
    mediaFiles = mediaFiles.filter((candidate) => normalizePath(candidate) !== target);
    if (selectedFile && normalizePath(selectedFile) === target) {
      selectedFile = null;
    }
    if (contextMenuFile && normalizePath(contextMenuFile) === target) {
      contextMenuFile = null;
    }
  }

  function handleThumbLoadedMetadata(e: Event) {
    const video = e.currentTarget as HTMLVideoElement;
    if (!Number.isFinite(video.duration) || video.duration <= 0) return;

    const targetTime = Math.min(0.1, Math.max(0, video.duration - 0.001));
    if (video.currentTime === 0 && targetTime > 0) {
      try {
        video.currentTime = targetTime;
      } catch {
        // Ignore seek errors for thumbnails
      }
    }
  }

  function getMediaKind(path: string): MediaKind {
    if (/\.(png|jpe?g|webp|gif|bmp|tiff?|avif|heic|heif|svg)$/i.test(path)) {
      return "image";
    }
    if (/\.(mp3|wav|m4a|aac|flac|ogg|opus|aiff?|wma)$/i.test(path)) {
      return "audio";
    }
    return "video";
  }

  function normalizePath(path: string): string {
    const normalized = path.replaceAll("\\", "/");
    const windowsLike =
      /^[a-z]:\//i.test(normalized) || normalized.startsWith("//");
    return windowsLike ? normalized.toLowerCase() : normalized;
  }
</script>

<div class="media-pool">
  <div class="toolbar">
    <button
      class="extract-btn"
      onclick={() => selectedFile && onExtractAudio?.(selectedFile)}
      title="Seçili videonun sesini ayır, kırp ve Voice olarak dışa aktar"
      disabled={!selectedFile || getMediaKind(selectedFile) !== "video" || !onExtractAudio}
    >
      Sesi ayıkla
    </button>
    <button
      class="add-btn"
      onclick={handleImport}
      title="Video, görsel veya ses ekle"
      disabled={isImporting}
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
      >
        <path d="M12 5v14M5 12h14" />
      </svg>
      <span>{isImporting ? "İnceleniyor…" : "Ekle"}</span>
    </button>
  </div>

  {#if importNotice}
    <div class="import-notice" role="status">{importNotice}</div>
  {/if}

  <div class="pool-content">
    {#if mediaFiles.length === 0}
      <div class="empty">
        <div class="empty-icon">
          <svg
            width="32"
            height="32"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
          >
            <path
              d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
            />
          </svg>
        </div>
        <p>Dosya ekleyin</p>
      </div>
    {:else}
      <div class="media-list">
        {#each mediaFiles as file}
          <div
            class="media-item"
            class:selected={selectedFile === file}
            onpointerdown={(e) => handlePointerDown(e, file)}
            onpointercancel={endPointerDrag}
            onclick={() => handleSelect(file)}
            oncontextmenu={(event) => handleContextMenu(event, file)}
            role="button"
            tabindex="0"
            aria-pressed={selectedFile === file}
            onkeydown={(e) => e.key === "Enter" && handleSelect(file)}
          >
            <div class="thumb">
              {#if getMediaKind(file) === "image"}
                <img src={convertFileSrc(file)} alt="" loading="lazy" />
              {:else if getMediaKind(file) === "audio"}
                <div class="audio-thumb" aria-hidden="true">
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.8"
                  >
                    <path d="M9 18V5l10-2v13" />
                    <circle cx="6" cy="18" r="3" />
                    <circle cx="16" cy="16" r="3" />
                  </svg>
                </div>
              {:else}
                <!-- svelte-ignore a11y_media_has_caption -->
                <video
                  src={convertFileSrc(file)}
                  preload="metadata"
                  muted
                  playsinline
                  onloadedmetadata={handleThumbLoadedMetadata}
                ></video>
              {/if}
            </div>
            <div class="media-copy">
              <span class="name">{getFileName(file)}</span>
              <span class="kind">{getMediaKind(file)}</span>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<ContextMenu
  open={contextMenuOpen}
  x={contextMenuX}
  y={contextMenuY}
  items={contextMenuItems}
  onClose={closeContextMenu}
  ariaLabel="Medya işlemleri"
/>

<style>
  .media-pool {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  /* Toolbar */
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid #1f1f1f;
  }

  .add-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    border-radius: 4px;
    color: #888;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
    font-family: inherit;
  }

  .extract-btn {
    min-width: 0;
    padding: 4px 8px;
    overflow: hidden;
    color: #7ee3c6;
    background: #13211e;
    border: 1px solid #315e51;
    border-radius: 4px;
    font: 600 10px/1.25 inherit;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .extract-btn:hover:not(:disabled) {
    color: #c3ffed;
    background: #173128;
    border-color: #55b99d;
  }

  .extract-btn:disabled {
    cursor: not-allowed;
    opacity: 0.4;
  }

  .add-btn:hover {
    background: #222;
    border-color: #3a3a3a;
    color: #fff;
  }

  .add-btn:disabled {
    cursor: wait;
    opacity: 0.6;
  }

  .import-notice {
    padding: 7px 12px;
    border-bottom: 1px solid #1f1f1f;
    color: #8b97b8;
    background: #111522;
    font-size: 10px;
    line-height: 1.4;
  }

  .pool-content {
    flex: 1;
    padding: 12px;
    overflow-y: auto;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 12px;
  }

  .empty-icon {
    color: #333;
  }

  .empty p {
    font-size: 12px;
    color: #444;
  }

  .media-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .media-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    cursor: grab;
    transition: background 0.15s;
    user-select: none;
  }

  .media-item:hover {
    background: #1a1a1a;
  }

  .media-item.selected {
    background: #172725;
    border-color: rgba(80, 201, 178, 0.58);
    box-shadow: inset 3px 0 0 #50c9b2;
  }

  .media-item.selected .name {
    color: #e6f6f2;
  }

  .media-item:active {
    cursor: grabbing;
  }

  .thumb {
    width: 48px;
    height: 32px;
    background: #000;
    border-radius: 4px;
    flex-shrink: 0;
    overflow: hidden;
    position: relative;
    border: 1px solid #333;
  }

  .thumb video,
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
  }

  .audio-thumb {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: #50c9b2;
    background: linear-gradient(135deg, #102823, #091513);
  }

  .media-copy {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }

  .name {
    font-size: 12px;
    color: #999;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .kind {
    color: #505050;
    font-size: 9px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
</style>
