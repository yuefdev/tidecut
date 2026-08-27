<script lang="ts">
  import { activeTool, setActiveTool, type ToolId } from "$lib/stores/tools";

  interface Props {
    transitionOpen?: boolean;
    onTransitionToggle?: () => void;
  }

  let {
    transitionOpen = false,
    onTransitionToggle,
  }: Props = $props();

  const tools: { id: ToolId; icon: string; label: string }[] = [
    { id: "select", icon: "V", label: "Seç" },
    { id: "cut", icon: "C", label: "Kes" },
    { id: "text", icon: "T", label: "Metin" },
  ];
</script>

<div class="toolbar">
  <div class="tool-group">
    {#each tools as tool}
      <button
        class="tool"
        class:active={$activeTool === tool.id}
        onclick={() => setActiveTool(tool.id)}
        title={tool.label}
      >
        <span class="tool-key">{tool.icon}</span>
      </button>
    {/each}
  </div>

  <div class="divider"></div>

  <div class="tool-group">
    <button class="tool" title="Efekt">
      <svg
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
      >
        <path d="M12 3v18M3 12h18" />
      </svg>
    </button>
    <button
      class="tool transition-tool"
      class:active={transitionOpen}
      title="Geçiş kütüphanesini aç"
      aria-label="Geçiş kütüphanesi"
      aria-pressed={transitionOpen}
      onclick={onTransitionToggle}
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
      >
        <rect x="3" y="3" width="7" height="7" />
        <rect x="14" y="14" width="7" height="7" />
        <path d="M10 7l4 10M14 17l-4-10" />
      </svg>
      <span>Geçişler</span>
    </button>
  </div>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 40px; /* Fixed height match layout */
    padding: 0 16px;
    background: #111;
    border-bottom: 1px solid #1f1f1f;
    flex-shrink: 0;
    overflow-x: auto; /* Allow scrolling on small screens */
    white-space: nowrap;

    /* Hide scrollbar but keep functionality */
    scrollbar-width: none;
    -ms-overflow-style: none;
  }

  .toolbar::-webkit-scrollbar {
    display: none;
  }

  .tool-group {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }

  .tool {
    width: 32px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: #666;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tool:hover {
    background: #1a1a1a;
    color: #ccc;
  }

  .tool.active {
    background: #1a1a1a;
    color: #fff;
  }

  .transition-tool {
    width: auto;
    min-width: 82px;
    gap: 7px;
    padding: 0 10px;
    border: 1px solid transparent;
    color: #7f8a86;
    font: 700 9px/1 "Inter", sans-serif;
  }

  .transition-tool.active {
    color: #caffef;
    background: #183029;
    border-color: #356759;
  }

  .tool-key {
    font-size: 11px;
    font-weight: 600;
    font-family: "JetBrains Mono", monospace;
  }

  .divider {
    width: 1px;
    height: 16px;
    background: #2a2a2a;
  }
</style>
