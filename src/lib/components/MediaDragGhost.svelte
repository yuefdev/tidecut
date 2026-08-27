<script lang="ts">
  import { mediaDropFeedback, pointerDragState } from "$lib/stores/drag";

  let pointerX = $state(0);
  let pointerY = $state(0);

  let isVisible = $derived(
    $pointerDragState.active &&
      $pointerDragState.hasMoved &&
      Boolean($pointerDragState.file),
  );
  let file = $derived($pointerDragState.file);
  let kind = $derived(file ? getMediaKind(file) : "video");
  let isRejected = $derived($mediaDropFeedback?.allowed === false);
  let rejectionMessage = $derived(
    isRejected ? $mediaDropFeedback?.message : null,
  );

  function handlePointerMove(event: PointerEvent) {
    if (!$pointerDragState.active) return;
    if (
      $pointerDragState.pointerId !== null &&
      event.pointerId !== $pointerDragState.pointerId
    ) {
      return;
    }
    pointerX = event.clientX;
    pointerY = event.clientY;
  }

  function getFileName(path: string): string {
    return path.split(/[/\\]/).pop() || path;
  }

  function getMediaKind(path: string): "video" | "image" | "audio" {
    if (/\.(png|jpe?g|webp|gif|bmp|tiff?|avif|heic|heif|svg)$/i.test(path)) {
      return "image";
    }
    if (/\.(mp3|wav|m4a|aac|flac|ogg|opus|aiff?|wma)$/i.test(path)) {
      return "audio";
    }
    return "video";
  }
</script>

<svelte:window onpointermove={handlePointerMove} />

{#if isVisible && file}
  <div
    class="media-drag-ghost"
    class:invalid={isRejected}
    style={`left: ${pointerX + 16}px; top: ${pointerY + 16}px`}
    aria-hidden="true"
  >
    <span class={`media-icon ${kind}`}>
      {#if kind === "audio"}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <path d="M9 18V5l10-2v13" />
          <circle cx="6" cy="18" r="3" />
          <circle cx="16" cy="16" r="3" />
        </svg>
      {:else if kind === "image"}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <circle cx="8" cy="9" r="1.5" />
          <path d="m4 18 5-5 3 3 3-4 5 6" />
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <rect x="3" y="5" width="13" height="14" rx="2" />
          <path d="m16 10 5-3v10l-5-3z" />
        </svg>
      {/if}
    </span>
    <span class="copy">
      <strong>{getFileName(file)}</strong>
      <small>{kind}</small>
    </span>
    <span class="drop-mark">{isRejected ? "×" : "+"}</span>
    {#if rejectionMessage}
      <span class="drop-warning">{rejectionMessage}</span>
    {/if}
  </div>
{/if}

<style>
  .media-drag-ghost {
    position: fixed;
    z-index: 2000;
    display: flex;
    align-items: center;
    gap: 9px;
    width: min(220px, calc(100vw - 40px));
    padding: 7px 9px 7px 7px;
    color: #e8efed;
    background: rgba(21, 25, 24, 0.94);
    border: 1px solid rgba(111, 224, 185, 0.62);
    border-radius: 8px;
    box-shadow:
      0 14px 30px rgba(0, 0, 0, 0.52),
      0 0 0 1px rgba(0, 0, 0, 0.32),
      0 0 18px rgba(98, 215, 177, 0.16);
    pointer-events: none;
    flex-wrap: wrap;
    transform: rotate(1deg);
    will-change: left, top;
  }

  .media-drag-ghost.invalid {
    border-color: rgba(239, 111, 101, 0.75);
    box-shadow:
      0 14px 30px rgba(0, 0, 0, 0.52),
      0 0 18px rgba(239, 111, 101, 0.2);
  }

  .media-icon {
    display: grid;
    width: 30px;
    height: 30px;
    flex: 0 0 auto;
    place-items: center;
    color: #aeb9ff;
    background: linear-gradient(135deg, #29376b, #151b38);
    border: 1px solid rgba(174, 185, 255, 0.25);
    border-radius: 5px;
  }

  .media-icon.audio {
    color: #65e0c5;
    background: linear-gradient(135deg, #17443b, #0b211d);
    border-color: rgba(101, 224, 197, 0.25);
  }

  .media-icon.image {
    color: #f3c778;
    background: linear-gradient(135deg, #514020, #28200f);
    border-color: rgba(243, 199, 120, 0.25);
  }

  .media-icon svg {
    width: 17px;
    height: 17px;
  }

  .copy {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }

  .copy strong,
  .copy small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .copy strong {
    font-size: 11px;
    font-weight: 600;
  }

  .copy small {
    color: #8a9995;
    font-size: 9px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .drop-mark {
    display: grid;
    width: 18px;
    height: 18px;
    flex: 0 0 auto;
    place-items: center;
    color: #0d2119;
    background: #62d7b1;
    border-radius: 50%;
    box-shadow: 0 2px 8px rgba(98, 215, 177, 0.32);
    font-size: 15px;
    font-weight: 700;
    line-height: 1;
  }

  .invalid .drop-mark {
    color: #2b100e;
    background: #ef6f65;
    box-shadow: 0 2px 8px rgba(239, 111, 101, 0.28);
  }

  .drop-warning {
    width: calc(100% - 39px);
    margin: -2px 0 0 39px;
    color: #ffaaa3;
    font-size: 9px;
    line-height: 1.35;
  }
</style>
