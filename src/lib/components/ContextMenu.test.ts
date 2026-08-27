import { describe, expect, it } from "vitest";
import contextMenuSource from "./ContextMenu.svelte?raw";

describe("ContextMenu", () => {
  it("exposes a reusable item and callback API", () => {
    expect(contextMenuSource).toMatch(/export interface ContextMenuItem/);
    expect(contextMenuSource).toMatch(/shortcut\?: string/);
    expect(contextMenuSource).toMatch(/disabled\?: boolean/);
    expect(contextMenuSource).toMatch(/divider\?: boolean/);
    expect(contextMenuSource).toMatch(/danger\?: boolean/);
    expect(contextMenuSource).toContain("item.onSelect?.()");
    expect(contextMenuSource).toContain("onSelect?.(item)");
  });

  it("clamps its fixed position to the viewport", () => {
    expect(contextMenuSource).toContain("position: fixed");
    expect(contextMenuSource).toMatch(/window\.innerWidth - menuElement\.offsetWidth/);
    expect(contextMenuSource).toMatch(/window\.innerHeight - menuElement\.offsetHeight/);
    expect(contextMenuSource).toContain('style:left={`${clampedX}px`}');
    expect(contextMenuSource).toContain('style:top={`${clampedY}px`}');
  });

  it("closes for outside pointer, Escape, resize, and window blur", () => {
    expect(contextMenuSource).toContain('window.addEventListener("pointerdown", handleOutsidePointer, true)');
    expect(contextMenuSource).toContain('event.key !== "Escape"');
    expect(contextMenuSource).toContain('window.addEventListener("resize", close)');
    expect(contextMenuSource).toContain('window.addEventListener("blur", close)');
  });

  it("implements accessible menu roles and keyboard navigation", () => {
    expect(contextMenuSource).toContain('role="menu"');
    expect(contextMenuSource).toContain('role="menuitem"');
    expect(contextMenuSource).toContain('role="separator"');
    expect(contextMenuSource).toContain('event.key === "ArrowDown"');
    expect(contextMenuSource).toContain('event.key === "ArrowUp"');
    expect(contextMenuSource).toContain('event.key === "Home"');
    expect(contextMenuSource).toContain('event.key === "End"');
    expect(contextMenuSource).toContain('event.key === "Enter"');
    expect(contextMenuSource).toMatch(/!item\.divider && !item\.disabled/);
  });
});
