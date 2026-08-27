<script lang="ts">
  import { onMount } from "svelte";

  interface Props {
    src?: string | null;
    videoSrc?: string | null;
    currentTime?: number;
    duration?: number;
    isPlaying?: boolean;
    timeOffset?: number;
    volume?: number;
    transform?: {
      x: number;
      y: number;
      scale: number;
      rotation: number;
      opacity: number;
    };
    onDetach?: () => void;
  }

  let {
    src = null,
    videoSrc = null,
    currentTime = $bindable(0),
    duration = $bindable(0),
    isPlaying = $bindable(false),
    timeOffset = 0,
    volume = 1,
    transform,
    onDetach,
  }: Props = $props();

  let videoRef = $state<HTMLVideoElement>();
  let containerRef = $state<HTMLDivElement>();

  // Transform Style
  let transformStyle = $derived(
    transform
      ? `
    transform: translate(${transform.x}px, ${transform.y}px) scale(${transform.scale}) rotate(${transform.rotation}deg);
    opacity: ${transform.opacity};
  `
      : "",
  );

  // Active Source
  let activeSrc = $derived(videoSrc || src);

  // Volume Effect
  $effect(() => {
    if (videoRef) {
      videoRef.volume = Math.max(0, Math.min(1, volume));
    }
  });

  // Playback Control
  $effect(() => {
    if (videoRef) {
      if (isPlaying) videoRef.play().catch(() => {});
      else videoRef.pause();
    }
  });

  // Time Sync: Playhead -> Video
  $effect(() => {
    if (!videoRef) return;
    // Video should be at (GlobalTime - Offset)
    // If offset is (ClipStart - VideoTrim), then GlobalTime - ClipStart + VideoTrim. Correct.
    const targetTime = Math.max(0, currentTime - timeOffset);
    if (Math.abs(videoRef.currentTime - targetTime) > 0.2) {
      videoRef.currentTime = targetTime;
    }
  });

  // Events
  function handleTimeUpdate() {
    if (!videoRef || !isPlaying) return;
    // Video updates Global Time
    // GlobalTime = VideoTime + Offset
    currentTime = videoRef.currentTime + timeOffset;
  }

  function handleLoadedMetadata() {
    if (videoRef) {
      // If this video is the main playhead source (e.g. previewing a file from media pool), update duration
      // But if it's a clip on timeline, duration is managed by timeline.
      // We can update duration if it's 0.
      if (duration === 0) duration = videoRef.duration;
    }
  }

  function togglePlay() {
    isPlaying = !isPlaying;
  }

  // Aspect Ratio & Layout Props
  let activeRatio = $state("fit"); // fit, fill, 16:9, etc.

  // Utils
  function formatTime(seconds: number) {
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    const ms = Math.floor((seconds % 1) * 100);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}:${ms.toString().padStart(2, "0")}`;
  }
</script>

<div class="player-container">
  <!-- Header / Toolbar -->
  <div class="player-header">
    <div class="left-controls">
      <span class="time-display"
        >{formatTime(currentTime)}
        <span class="duration">/ {formatTime(duration)}</span></span
      >
    </div>
    <div class="right-controls">
      {#if onDetach}
        <button class="icon-btn detach-btn" onclick={onDetach} title="Ayır">
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            ><path d="M15 3h6v6" /><path d="M14 10l6.1-6.1" /><path
              d="M9 21H3v-6"
            /><path d="M10 14l-6.1 6.1" /></svg
          >
        </button>
      {/if}
    </div>
  </div>

  <!-- Video Area -->
  <div class="video-area" bind:this={containerRef}>
    {#if activeSrc}
      <div
        class="video-wrapper"
        style={transformStyle}
        onclick={togglePlay}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === " " && togglePlay()}
      >
        <video
          bind:this={videoRef}
          src={activeSrc}
          class="video-element"
          playsinline
          ontimeupdate={handleTimeUpdate}
          onloadedmetadata={handleLoadedMetadata}
          onended={() => (isPlaying = false)}
          onplay={() => (isPlaying = true)}
          onpause={() => (isPlaying = false)}
        >
          <track kind="captions" />
        </video>
      </div>
    {:else}
      <div class="placeholder">
        <p>Medya yok</p>
      </div>
    {/if}
  </div>

  <!-- Controls (Minimal) -->
  <div class="controls-bar">
    <button class="ctrl-btn" onclick={() => (isPlaying = !isPlaying)}>
      {#if isPlaying}
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"
          ><rect x="6" y="4" width="4" height="16" /><rect
            x="14"
            y="4"
            width="4"
            height="16"
          /></svg
        >
      {:else}
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"
          ><path d="M5 3l14 9-14 9V3z" /></svg
        >
      {/if}
    </button>

    <div class="scrubber">
      <!-- Basit scrubber visual -->
      <div
        class="progress"
        style="width: {duration > 0
          ? Math.max(0, Math.min(100, (currentTime / duration) * 100))
          : 0}%"
      ></div>
    </div>
  </div>
</div>

<style>
  .player-container {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: #000;
    overflow: hidden;
  }

  .player-header {
    height: 32px;
    background: #111;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 12px;
    border-bottom: 1px solid #222;
    z-index: 10;
  }

  .time-display {
    font-family: "JetBrains Mono", monospace;
    font-size: 12px;
    color: #eee;
  }
  .duration {
    color: #666;
  }

  .icon-btn {
    background: transparent;
    border: none;
    color: #888;
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
  }
  .icon-btn:hover {
    color: #fff;
    background: #222;
  }

  .detach-btn {
    color: #3d5afe;
  }

  .video-area {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    background: #050505;
  }

  .video-wrapper {
    /* Transform anchor point center */
    display: flex;
    align-items: center;
    justify-content: center;
    transform-origin: center center;
    will-change: transform, opacity;
  }

  .video-element {
    max-width: 100%;
    max-height: 100%;
    display: block;
    pointer-events: none; /* Events are handled by wrapper */
  }

  .placeholder {
    color: #333;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 1px;
  }

  .controls-bar {
    height: 36px;
    background: #111;
    border-top: 1px solid #222;
    display: flex;
    align-items: center;
    padding: 0 8px;
    gap: 12px;
    z-index: 10;
  }

  .ctrl-btn {
    background: transparent;
    border: none;
    color: #ddd;
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
  }
  .ctrl-btn:hover {
    color: #fff;
  }

  .scrubber {
    flex: 1;
    height: 4px;
    background: #333;
    border-radius: 2px;
    overflow: hidden;
  }

  .progress {
    height: 100%;
    background: #3d5afe;
  }
</style>
