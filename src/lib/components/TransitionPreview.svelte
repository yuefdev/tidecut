<script lang="ts">
  import type { TransitionType } from "$lib/editor/timeline-engine";
  import { getTransitionPreset } from "$lib/editor/transition-presets";

  interface Props {
    type: TransitionType;
    progress?: number;
    frame?: number;
  }

  let { type, progress = 0, frame = 1 }: Props = $props();

  let preset = $derived(getTransitionPreset(type));
  let p = $derived(Math.min(1, Math.max(0, progress)));
  let assetFrame = $derived(
    preset.assetDir
      ? `/transitions/${preset.assetDir}/f${String(Math.min(24, Math.max(1, frame))).padStart(2, "0")}.webp`
      : null,
  );

  let sceneAStyle = $derived.by(() => {
    if (type === "dip-to-black") {
      return `opacity: ${Math.max(0, 1 - p * 2)}`;
    }
    if (type === "slide-left") return `transform: translateX(${-22 * p}%)`;
    if (type === "slide-right") return `transform: translateX(${22 * p}%)`;
    if (type === "slide-up") return `transform: translateY(${-22 * p}%)`;
    if (type === "slide-down") return `transform: translateY(${22 * p}%)`;
    if (type === "whip-left") {
      return `transform: translateX(${-42 * p}%) skewX(-8deg); filter: blur(${p * 2.5}px)`;
    }
    if (type === "whip-right") {
      return `transform: translateX(${42 * p}%) skewX(8deg); filter: blur(${p * 2.5}px)`;
    }
    return "";
  });

  let sceneBStyle = $derived.by(() => {
    if (type === "none") {
      return "opacity: 1; clip-path: inset(0 0 0 50%)";
    }
    if (type === "crossfade") return `opacity: ${p}`;
    if (type === "dip-to-black") {
      return `opacity: ${Math.max(0, (p - 0.5) * 2)}`;
    }
    if (type === "wipe-left") {
      return `clip-path: inset(0 0 0 ${(1 - p) * 100}%)`;
    }
    if (type === "wipe-right") {
      return `clip-path: inset(0 ${(1 - p) * 100}% 0 0)`;
    }
    if (type === "slide-left") return `transform: translateX(${(1 - p) * 100}%)`;
    if (type === "slide-right") return `transform: translateX(${-(1 - p) * 100}%)`;
    if (type === "slide-up") return `transform: translateY(${(1 - p) * 100}%)`;
    if (type === "slide-down") return `transform: translateY(${-(1 - p) * 100}%)`;
    if (type === "zoom-in") {
      return `opacity: ${p}; transform: scale(${0.46 + p * 0.54})`;
    }
    if (type === "zoom-out") {
      return `opacity: ${p}; transform: scale(${1.62 - p * 0.62})`;
    }
    if (type === "spin") {
      return `opacity: ${p}; transform: scale(${0.48 + p * 0.52}) rotate(${(1 - p) * -190}deg)`;
    }
    if (type === "flip-3d") {
      return `opacity: ${Math.min(1, p * 1.6)}; transform: perspective(180px) rotateY(${(1 - p) * -92}deg)`;
    }
    if (type === "blur") {
      return `opacity: ${p}; filter: blur(${(1 - p) * 9}px); transform: scale(${1.08 - p * 0.08})`;
    }
    if (type === "whip-left") {
      return `transform: translateX(${(1 - p) * 115}%) skewX(-10deg); filter: blur(${(1 - p) * 3}px)`;
    }
    if (type === "whip-right") {
      return `transform: translateX(${-(1 - p) * 115}%) skewX(10deg); filter: blur(${(1 - p) * 3}px)`;
    }
    if (preset.assetMode === "mask") {
      return `opacity: 1; clip-path: circle(${p * 78}% at 50% 50%)`;
    }
    if (preset.assetMode === "screen") return `opacity: ${p}`;
    return `opacity: ${p}`;
  });

  let assetOpacity = $derived(Math.sin(p * Math.PI) * 0.92);
</script>

<div class="transition-preview" data-transition={type} aria-hidden="true">
  <div class="scene scene-a" style={sceneAStyle}>
    <span>A</span>
    <i></i>
  </div>
  <div class="scene scene-b" style={sceneBStyle}>
    <span>B</span>
    <i></i>
  </div>
  {#if type === "dip-to-black"}
    <div class="blackout" style:opacity={Math.max(0, 1 - Math.abs(p * 2 - 1))}></div>
  {/if}
  {#if type === "whip-left" || type === "whip-right"}
    <div
      class="speed-lines"
      class:right={type === "whip-right"}
      style:opacity={Math.sin(p * Math.PI)}
    ></div>
  {/if}
  {#if assetFrame}
    <img
      class:mask={preset.assetMode === "mask"}
      src={assetFrame}
      alt=""
      draggable="false"
      decoding="async"
      style:opacity={assetOpacity}
    />
  {/if}
  {#if type === "none"}
    <span class="cut-line"></span>
  {/if}
  <span class="preview-shine"></span>
</div>

<style>
  .transition-preview {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    isolation: isolate;
    color: #fff;
    background: #07100e;
    border-radius: inherit;
    perspective: 180px;
  }

  .scene {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    overflow: hidden;
    transform-origin: center;
    will-change: transform, opacity, filter, clip-path;
  }

  .scene::before {
    content: "";
    position: absolute;
    inset: 0;
    opacity: 0.42;
    background-image:
      linear-gradient(rgba(255, 255, 255, 0.12) 1px, transparent 1px),
      linear-gradient(90deg, rgba(255, 255, 255, 0.1) 1px, transparent 1px);
    background-size: 13px 13px;
  }

  .scene-a {
    background:
      radial-gradient(circle at 22% 30%, rgba(255, 222, 122, 0.88), transparent 27%),
      linear-gradient(135deg, #e66c4d, #56254e 72%);
  }

  .scene-b {
    background:
      radial-gradient(circle at 73% 30%, rgba(151, 255, 224, 0.9), transparent 25%),
      linear-gradient(135deg, #17586c, #14332d 72%);
  }

  .scene span {
    position: relative;
    z-index: 1;
    display: grid;
    width: 23px;
    height: 23px;
    place-items: center;
    border: 1px solid rgba(255, 255, 255, 0.62);
    border-radius: 50%;
    background: rgba(4, 10, 9, 0.32);
    box-shadow: 0 4px 15px rgba(0, 0, 0, 0.28);
    font: 800 10px/1 "JetBrains Mono", monospace;
  }

  .scene i {
    position: absolute;
    right: 11px;
    bottom: 8px;
    width: 26px;
    height: 7px;
    border-top: 2px solid rgba(255, 255, 255, 0.72);
    border-bottom: 2px solid rgba(255, 255, 255, 0.35);
    transform: skewX(-22deg);
  }

  .blackout {
    position: absolute;
    z-index: 3;
    inset: 0;
    background: #000;
    pointer-events: none;
  }

  .speed-lines {
    position: absolute;
    z-index: 4;
    inset: 12% -8%;
    background: repeating-linear-gradient(
      -8deg,
      transparent 0 8px,
      rgba(235, 255, 249, 0.58) 9px 10px,
      transparent 11px 17px
    );
    filter: blur(0.6px);
    transform: translateX(-8%);
    pointer-events: none;
  }

  .speed-lines.right {
    transform: scaleX(-1) translateX(-8%);
  }

  img {
    position: absolute;
    z-index: 5;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    mix-blend-mode: screen;
    pointer-events: none;
  }

  img.mask {
    filter: contrast(1.25);
    mix-blend-mode: screen;
  }

  .cut-line {
    position: absolute;
    z-index: 6;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 1px;
    background: rgba(255, 255, 255, 0.8);
    box-shadow: 0 0 0 2px rgba(0, 0, 0, 0.24);
  }

  .preview-shine {
    position: absolute;
    z-index: 7;
    inset: 0;
    background: linear-gradient(135deg, rgba(255, 255, 255, 0.11), transparent 36%);
    pointer-events: none;
  }
</style>
