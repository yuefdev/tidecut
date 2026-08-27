<script lang="ts">
    import { createEventDispatcher } from "svelte";

    interface Props {
        direction: "horizontal" | "vertical";
        position?: "left" | "right" | "top" | "bottom";
    }

    let { direction = "horizontal", position = "right" }: Props = $props();

    const dispatch = createEventDispatcher<{ resize: { delta: number } }>();

    let isDragging = $state(false);
    let startPos = $state(0);

    function handleMouseDown(e: MouseEvent) {
        e.preventDefault();
        isDragging = true;
        startPos = direction === "horizontal" ? e.clientX : e.clientY;

        document.addEventListener("mousemove", handleMouseMove);
        document.addEventListener("mouseup", handleMouseUp);
        document.body.style.cursor =
            direction === "horizontal" ? "col-resize" : "row-resize";
        document.body.style.userSelect = "none";
    }

    function handleMouseMove(e: MouseEvent) {
        if (!isDragging) return;

        const currentPos = direction === "horizontal" ? e.clientX : e.clientY;
        let delta = currentPos - startPos;

        // Invert delta for right/bottom positioned splitters
        if (position === "right" || position === "bottom") {
            delta = -delta;
        }

        dispatch("resize", { delta });
        startPos = currentPos;
    }

    function handleMouseUp() {
        isDragging = false;
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleMouseUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
    }

    function handleKeyDown(e: KeyboardEvent) {
        const isDecrease = e.key === "ArrowLeft" || e.key === "ArrowUp";
        const isIncrease = e.key === "ArrowRight" || e.key === "ArrowDown";
        if (!isDecrease && !isIncrease) return;

        e.preventDefault();
        const step = e.shiftKey ? 24 : 8;
        let delta = isDecrease ? -step : step;
        if (position === "right" || position === "bottom") delta = -delta;
        dispatch("resize", { delta });
    }
</script>

<button
    type="button"
    class="splitter"
    class:horizontal={direction === "horizontal"}
    class:vertical={direction === "vertical"}
    class:dragging={isDragging}
    onmousedown={handleMouseDown}
    onkeydown={handleKeyDown}
    aria-label={direction === "horizontal" ? "Panel genişliğini ayarla" : "Zaman çizelgesi yüksekliğini ayarla"}
>
    <div class="handle"></div>
</button>

<style>
    .splitter {
        position: relative;
        flex-shrink: 0;
        background: #000; /* Koyu arka plan */
        transition: background 0.15s;
        z-index: 10;
        border: 0;
        padding: 0;
    }

    .splitter.horizontal {
        width: 8px; /* Biraz daha geniş tutma alanı */
        cursor: col-resize;
        border-right: 1px solid #1a1a1a;
        border-left: 1px solid #1a1a1a;
    }

    .splitter.vertical {
        height: 8px; /* Biraz daha geniş tutma alanı */
        cursor: row-resize;
        border-top: 1px solid #1a1a1a;
        border-bottom: 1px solid #1a1a1a;
    }

    .splitter:hover,
    .splitter.dragging {
        background: #111;
    }

    .handle {
        position: absolute;
        background: #333;
        border-radius: 4px;
        opacity: 0.5; /* Her zaman biraz görünür olsun */
        transition: all 0.15s;
    }

    .splitter.horizontal .handle {
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        width: 4px;
        height: 32px;
    }

    .splitter.vertical .handle {
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        width: 32px;
        height: 4px;
    }

    .splitter:hover .handle,
    .splitter.dragging .handle {
        opacity: 1;
        background: #007bff; /* Mavi vurgu */
        box-shadow: 0 0 8px rgba(0, 123, 255, 0.5); /* Glow efekti */
    }
</style>
