<script lang="ts">
    import type { Snippet } from "svelte";
    import {
        updateDetachedPanelPosition,
        updateDetachedPanelSize,
        type DetachedPanelState,
    } from "$lib/stores/panels";

    interface Props {
        id: string;
        title: string;
        isDetached?: boolean;
        detachedState?: DetachedPanelState | null;
        onDetach?: () => void;
        onAttach?: () => void;
        children: Snippet;
    }

    let {
        id,
        title,
        isDetached = false,
        detachedState = null,
        onDetach,
        onAttach,
        children,
    }: Props = $props();

    let isDragging = $state(false);
    let isResizing = $state(false);

    // Use detached state or defaults
    let posX = $derived(detachedState?.x ?? 100);
    let posY = $derived(detachedState?.y ?? 100);
    let width = $derived(detachedState?.width ?? 400);
    let height = $derived(detachedState?.height ?? 350);

    function handleDragStart(e: MouseEvent) {
        if (!isDetached) return;
        e.preventDefault();
        isDragging = true;

        const startX = e.clientX - posX;
        const startY = e.clientY - posY;

        function handleDrag(e: MouseEvent) {
            const newX = Math.max(0, e.clientX - startX);
            const newY = Math.max(0, e.clientY - startY);
            updateDetachedPanelPosition(id, newX, newY);
        }

        function handleDragEnd() {
            isDragging = false;
            document.removeEventListener("mousemove", handleDrag);
            document.removeEventListener("mouseup", handleDragEnd);
        }

        document.addEventListener("mousemove", handleDrag);
        document.addEventListener("mouseup", handleDragEnd);
    }

    function handleResizeStart(e: MouseEvent) {
        e.preventDefault();
        e.stopPropagation();
        isResizing = true;

        const startX = e.clientX;
        const startY = e.clientY;
        const startWidth = width;
        const startHeight = height;

        function handleResize(e: MouseEvent) {
            const newWidth = Math.max(200, startWidth + (e.clientX - startX));
            const newHeight = Math.max(150, startHeight + (e.clientY - startY));
            updateDetachedPanelSize(id, newWidth, newHeight);
        }

        function handleResizeEnd() {
            isResizing = false;
            document.removeEventListener("mousemove", handleResize);
            document.removeEventListener("mouseup", handleResizeEnd);
        }

        document.addEventListener("mousemove", handleResize);
        document.addEventListener("mouseup", handleResizeEnd);
    }
</script>

{#if isDetached}
    <!-- Floating Panel Mode -->
    <div
        class="floating-panel"
        class:dragging={isDragging}
        class:resizing={isResizing}
        style="left: {posX}px; top: {posY}px; width: {width}px; height: {height}px;"
    >
        <div
            class="floating-header"
            onmousedown={handleDragStart}
            role="button"
            tabindex="0"
            aria-label="Drag to move panel"
        >
            <span class="floating-title">{title}</span>
            <div class="floating-controls">
                <button class="float-btn" onclick={onAttach} title="Dock Panel">
                    <svg
                        width="12"
                        height="12"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                    >
                        <rect x="3" y="3" width="18" height="18" rx="2" />
                        <path d="M9 3v18" />
                    </svg>
                </button>
            </div>
        </div>
        <div class="floating-content">
            {@render children()}
        </div>
        <!-- Resize Handle -->
        <div
            class="resize-handle"
            onmousedown={handleResizeStart}
            role="button"
            tabindex="0"
            aria-label="Resize panel"
        ></div>
    </div>
{:else}
    <!-- Docked Panel Mode -->
    <div class="panel" data-panel-id={id}>
        <div class="panel-header">
            <span class="panel-title">{title}</span>
            {#if onDetach}
                <button
                    class="panel-action"
                    onclick={onDetach}
                    title="Ayrı pencerede aç"
                >
                    <svg
                        width="12"
                        height="12"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                    >
                        <path
                            d="M15 3h6v6M14 10l7-7M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"
                        />
                    </svg>
                </button>
            {/if}
        </div>
        <div class="panel-content">
            {@render children()}
        </div>
    </div>
{/if}

<style>
    .panel {
        display: flex;
        flex-direction: column;
        height: 100%;
        background: #111;
    }

    .panel-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 14px;
        border-bottom: 1px solid #1f1f1f;
    }

    .panel-title {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.5px;
        color: #666;
    }

    .panel-action {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 22px;
        height: 22px;
        background: transparent;
        border: none;
        border-radius: 4px;
        color: #555;
        cursor: pointer;
        transition: all 0.15s;
    }

    .panel-action:hover {
        background: #1a1a1a;
        color: #999;
    }

    .panel-content {
        flex: 1;
        min-height: 0;
        overflow: auto;
    }

    /* Floating Panel Styles */
    .floating-panel {
        position: fixed;
        background: #111;
        border: 1px solid #2a2a2a;
        border-radius: 8px;
        box-shadow:
            0 8px 32px rgba(0, 0, 0, 0.6),
            0 0 0 1px rgba(255, 255, 255, 0.05);
        z-index: 1000;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .floating-panel.dragging {
        opacity: 0.9;
        cursor: grabbing;
    }

    .floating-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 8px 12px;
        background: #1a1a1a;
        cursor: grab;
        border-bottom: 1px solid #2a2a2a;
        user-select: none;
    }

    .floating-header:active {
        cursor: grabbing;
    }

    .floating-title {
        font-size: 12px;
        font-weight: 500;
        color: #999;
    }

    .floating-controls {
        display: flex;
        gap: 4px;
    }

    .float-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 20px;
        height: 20px;
        background: transparent;
        border: none;
        border-radius: 4px;
        color: #666;
        cursor: pointer;
        transition: all 0.15s;
    }

    .float-btn:hover {
        background: #333;
        color: #fff;
    }

    .floating-content {
        flex: 1;
        min-height: 0;
        overflow: auto;
        background: #0a0a0a;
    }

    /* Resize Handle */
    .resize-handle {
        position: absolute;
        right: 0;
        bottom: 0;
        width: 16px;
        height: 16px;
        cursor: nwse-resize;
        background: linear-gradient(135deg, transparent 50%, #333 50%);
        border-radius: 0 0 8px 0;
    }

    .resize-handle:hover {
        background: linear-gradient(135deg, transparent 50%, #555 50%);
    }
</style>
