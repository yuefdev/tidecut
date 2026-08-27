import { describe, expect, it } from "vitest";
import mediaPoolSource from "./MediaPool.svelte?raw";
import pageSource from "../../routes/+page.svelte?raw";

describe("MediaPool context menu", () => {
  it("offers the professional media actions in Turkish", () => {
    expect(mediaPoolSource).toContain('label: "Önizle"');
    expect(mediaPoolSource).toContain('label: "Zaman çizelgesine ekle"');
    expect(mediaPoolSource).toContain('label: "Dosya konumunu aç"');
    expect(mediaPoolSource).toContain('label: "Projeden kaldır"');
    expect(mediaPoolSource).toContain("divider: true");
  });

  it("guards native reveal and removes only the project media reference", () => {
    expect(mediaPoolSource).toContain('import { revealItemInDir } from "@tauri-apps/plugin-opener"');
    expect(mediaPoolSource).toContain("if (!isTauri()) return");
    expect(mediaPoolSource).toContain("await revealItemInDir(file)");
    expect(mediaPoolSource).toMatch(/mediaFiles = mediaFiles\.filter/);
    expect(mediaPoolSource).not.toMatch(/remove(File|Dir)|delete(File|Path)/);
  });

  it("selects right-clicked media and wires add-to-timeline through the page", () => {
    expect(mediaPoolSource).toContain("selectedFile = file");
    expect(mediaPoolSource).toContain("oncontextmenu={(event) => handleContextMenu(event, file)}");
    expect(mediaPoolSource).toContain("class:selected={selectedFile === file}");
    expect(mediaPoolSource).toContain("onAddToTimeline?: (file: string) => void");
    expect(pageSource).toContain("onAddToTimeline={handleAddMediaToTimeline}");
    expect(pageSource).toContain("timelineRef?.addMediaAtPlayhead?.(file)");
  });
});
