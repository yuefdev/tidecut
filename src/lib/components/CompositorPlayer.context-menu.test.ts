import { describe, expect, it } from "vitest";
import compositorSource from "./CompositorPlayer.svelte?raw";

describe("CompositorPlayer context menu", () => {
  it("opens a reusable context menu from the preview stage", () => {
    expect(compositorSource).toContain('from "./ContextMenu.svelte"');
    expect(compositorSource).toContain("oncontextmenu={openPreviewContextMenu}");
    expect(compositorSource).toContain('ariaLabel="Önizleme ayarları"');
  });

  it("exposes real playback and preview-quality actions", () => {
    expect(compositorSource).toContain('label: isPlaying ? "Duraklat" : "Oynat"');
    expect(compositorSource).toContain('label: "Önceki kare"');
    expect(compositorSource).toContain('label: "Sonraki kare"');
    expect(compositorSource).toContain('"Önizleme kalitesi: Tam"');
    expect(compositorSource).toContain('"Önizleme kalitesi: 1/2"');
    expect(compositorSource).toContain('"Önizleme kalitesi: 1/4"');
    expect(compositorSource).toContain("compositor.setResolutionScale(quality)");
  });

  it("provides guides, frame copy and window actions without placeholders", () => {
    expect(compositorSource).toContain('label: checkedLabel(showActionSafe, "Eylem güvenli alanı")');
    expect(compositorSource).toContain('label: checkedLabel(showTitleSafe, "Başlık güvenli alanı")');
    expect(compositorSource).toContain('label: checkedLabel(showThirdsGrid, "Üçler ızgarası")');
    expect(compositorSource).toContain("navigator.clipboard.write");
    expect(compositorSource).toContain('label: "Seçili klibin dönüşümünü sıfırla"');
    expect(compositorSource).toContain('label: "Ayrı pencerede aç"');
    expect(compositorSource).toContain('isFullscreen ? "Tam ekrandan çık" : "Tam ekrana geç"');
  });

  it("can persist the fully composited current frame as a compact project cover", () => {
    expect(compositorSource).toContain("onCoverCapture?: (cover: CoverCapture) => void");
    expect(compositorSource).toContain('id: "set-cover"');
    expect(compositorSource).toContain("capture.toDataURL(\"image/jpeg\", 0.88)");
    expect(compositorSource).toContain("context.drawImage(canvas, 0, 0, width, height)");
    expect(compositorSource).toContain("▣ Kapak");
  });
});
