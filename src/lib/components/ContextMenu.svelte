<script module lang="ts">
  export interface ContextMenuItem {
    id: string;
    label?: string;
    shortcut?: string;
    disabled?: boolean;
    divider?: boolean;
    danger?: boolean;
    onSelect?: () => void;
  }

  export interface ContextMenuProps {
    open: boolean;
    x: number;
    y: number;
    items: ContextMenuItem[];
    onClose: () => void;
    onSelect?: (item: ContextMenuItem) => void;
    ariaLabel?: string;
  }
</script>

<script lang="ts">
  import { tick } from "svelte";

  let {
    open,
    x,
    y,
    items,
    onClose,
    onSelect,
    ariaLabel = "Bağlam menüsü",
  }: ContextMenuProps = $props();

  let menuElement: HTMLDivElement | undefined = $state();
  let itemElements: Array<HTMLButtonElement | undefined> = [];
  let activeIndex = $state(-1);
  let clampedX = $state(0);
  let clampedY = $state(0);
  let positioned = $state(false);
  let placementRun = 0;

  $effect(() => {
    if (!open) return;

    // Reading these values makes a moved/repopulated open menu place itself again.
    x;
    y;
    items;
    const run = ++placementRun;
    positioned = false;
    activeIndex = firstEnabledIndex();

    void tick().then(() => {
      if (run !== placementRun || !open || !menuElement) return;
      clampToViewport();
      positioned = true;
      void tick().then(() => {
        if (run !== placementRun || !open) return;
        focusActiveItem();
      });
    });

    return () => {
      placementRun += 1;
    };
  });

  $effect(() => {
    if (!open) return;

    const handleOutsidePointer = (event: PointerEvent) => {
      if (!menuElement?.contains(event.target as Node)) onClose();
    };
    const handleWindowKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    const close = () => onClose();

    window.addEventListener("pointerdown", handleOutsidePointer, true);
    window.addEventListener("keydown", handleWindowKey, true);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);

    return () => {
      window.removeEventListener("pointerdown", handleOutsidePointer, true);
      window.removeEventListener("keydown", handleWindowKey, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  });

  function clampToViewport() {
    if (!menuElement) return;
    const gutter = 8;
    const maxX = Math.max(gutter, window.innerWidth - menuElement.offsetWidth - gutter);
    const maxY = Math.max(gutter, window.innerHeight - menuElement.offsetHeight - gutter);
    clampedX = Math.min(Math.max(x, gutter), maxX);
    clampedY = Math.min(Math.max(y, gutter), maxY);
  }

  function isEnabled(index: number) {
    const item = items[index];
    return Boolean(item && !item.divider && !item.disabled && item.label);
  }

  function firstEnabledIndex() {
    return items.findIndex((_, index) => isEnabled(index));
  }

  function lastEnabledIndex() {
    for (let index = items.length - 1; index >= 0; index -= 1) {
      if (isEnabled(index)) return index;
    }
    return -1;
  }

  function adjacentEnabledIndex(direction: 1 | -1) {
    if (items.length === 0) return -1;
    let index = activeIndex;
    for (let attempt = 0; attempt < items.length; attempt += 1) {
      index = (index + direction + items.length) % items.length;
      if (isEnabled(index)) return index;
    }
    return -1;
  }

  function focusItem(index: number) {
    if (!isEnabled(index)) return;
    activeIndex = index;
    itemElements[index]?.focus({ preventScroll: true });
  }

  function focusActiveItem() {
    if (activeIndex >= 0) {
      itemElements[activeIndex]?.focus({ preventScroll: true });
    } else {
      menuElement?.focus({ preventScroll: true });
    }
  }

  function selectItem(index: number) {
    if (!isEnabled(index)) return;
    const item = items[index];
    onClose();
    item.onSelect?.();
    onSelect?.(item);
  }

  function handleKeydown(event: KeyboardEvent) {
    let targetIndex = -1;
    if (event.key === "ArrowDown") targetIndex = adjacentEnabledIndex(1);
    else if (event.key === "ArrowUp") targetIndex = adjacentEnabledIndex(-1);
    else if (event.key === "Home") targetIndex = firstEnabledIndex();
    else if (event.key === "End") targetIndex = lastEnabledIndex();
    else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      event.stopPropagation();
      selectItem(activeIndex);
      return;
    } else if (event.key === "Tab") {
      onClose();
      return;
    } else {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    if (targetIndex >= 0) focusItem(targetIndex);
  }
</script>

{#if open}
  <div
    bind:this={menuElement}
    class="context-menu"
    class:positioned
    role="menu"
    tabindex="-1"
    aria-label={ariaLabel}
    style:left={`${clampedX}px`}
    style:top={`${clampedY}px`}
    onkeydown={handleKeydown}
    oncontextmenu={(event) => event.preventDefault()}
  >
    {#each items as item, index (item.id)}
      {#if item.divider}
        <div class="divider" role="separator"></div>
      {:else}
        <button
          bind:this={itemElements[index]}
          type="button"
          role="menuitem"
          class:danger={item.danger}
          class:active={activeIndex === index}
          disabled={item.disabled}
          aria-disabled={item.disabled ? "true" : undefined}
          tabindex={activeIndex === index ? 0 : -1}
          onclick={() => selectItem(index)}
          onpointermove={() => {
            if (isEnabled(index)) activeIndex = index;
          }}
          onfocus={() => {
            if (isEnabled(index)) activeIndex = index;
          }}
        >
          <span class="label">{item.label}</span>
          {#if item.shortcut}<kbd aria-hidden="true">{item.shortcut}</kbd>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 2200;
    visibility: hidden;
    width: max-content;
    min-width: 228px;
    max-width: min(320px, calc(100vw - 16px));
    max-height: calc(100vh - 16px);
    padding: 5px;
    overflow-x: hidden;
    overflow-y: auto;
    color: #d8dcda;
    background: #242625;
    border: 1px solid #3a3d3b;
    border-radius: 8px;
    box-shadow: 0 14px 38px rgba(0, 0, 0, 0.48), 0 2px 8px rgba(0, 0, 0, 0.35);
    outline: none;
    user-select: none;
  }

  .context-menu.positioned {
    visibility: visible;
  }

  button {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 24px;
    width: 100%;
    min-height: 29px;
    padding: 6px 9px;
    color: inherit;
    background: transparent;
    border: 0;
    border-radius: 5px;
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: default;
  }

  button:hover:not(:disabled),
  button.active:not(:disabled),
  button:focus-visible:not(:disabled) {
    color: #f0f4f2;
    background: #3a3d3b;
    outline: none;
  }

  button.danger {
    color: #e89286;
  }

  button.danger:hover:not(:disabled),
  button.danger.active:not(:disabled),
  button.danger:focus-visible:not(:disabled) {
    color: #ffd3cd;
    background: rgba(184, 69, 55, 0.3);
  }

  button:disabled {
    color: #686d6a;
    cursor: not-allowed;
  }

  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  kbd {
    color: #8d938f;
    font: inherit;
    font-size: 9px;
    white-space: nowrap;
  }

  button:disabled kbd {
    color: #5b5f5d;
  }

  .divider {
    height: 1px;
    margin: 5px 4px;
    background: #3b3e3c;
  }
</style>
