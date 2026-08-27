import { describe, expect, it } from "vitest";
import source from "./SmartShortsPanel.svelte?raw";

describe("SmartShortsPanel", () => {
  it("shows local GPU signals and the optional transcript-only GLM judge", () => {
    expect(source).toContain("Whisper · GPU/CPU otomatik");
    expect(source).toContain("engineLabel?.split");
    expect(source).toContain("Silero VAD");
    expect(source).toContain("FFmpeg ses enerjisi");
    expect(source).toContain("Beat This");
    expect(source).toContain("YAMNet kahkaha · CPU");
    expect(source).toContain("Fireworks GLM hakemi");
    expect(source).toContain("video gönderilmez");
    expect(source).toContain('type="password"');
  });

  it("renders reviewable candidates and requires explicit apply", () => {
    expect(source).toContain("onCandidateSeek?.(candidate)");
    expect(source).toContain("onCandidateToggle?.(candidate.id");
    expect(source).toContain("Seçilileri tek highlight akışına dönüştür");
    expect(source).toContain("tek Ctrl+Z adımında");
  });

  it("exposes determinate analysis progress", () => {
    expect(source).toContain('role="progressbar"');
    expect(source).toContain('aria-label="Akıllı Shorts analiz ilerlemesi"');
    expect(source).toContain("displayProgress");
    expect(source).toContain("displayProgressText");
    expect(source).toContain("progressRemainingMs");
    expect(source).toContain("Tahmin uzadı · çalışıyor");
    expect(source).toContain("progress-shimmer");
  });

  it("sets first-run expectations and exposes the layout diagram accessibly", () => {
    expect(source).toContain("İlk kullanım:");
    expect(source).toContain("Yönetilen Python/MediaPipe ortamı");
    expect(source).toContain("Kurulumdan sonra kahkaha taraması cihazda çalışır.");
    expect(source).toContain('role="img"');
    expect(source).toContain(":focus-visible");
  });

  it("organizes the workflow into analysis, vertical layout, and review steps", () => {
    expect(source).toContain('class="workflow-strip"');
    expect(source).toContain("Güçlü anları bul");
    expect(source).toContain("Dikey yayıncı düzeni");
    expect(source).toContain("Adayları kontrol et");
    expect(source).toContain('class="empty-state"');
  });

  it("exposes a controlled 9:16 streamer layout contract", () => {
    expect(source).toContain("export interface SmartShortsStreamerLayout");
    expect(source).toContain('facePosition: SmartShortsStreamerFacePosition');
    expect(source).toContain("streamerLayoutEnabled?: boolean");
    expect(source).toContain("onStreamerLayoutChange?:");
    expect(source).toContain("onSelectStreamerTarget?:");
    expect(source).toContain('role="switch"');
    expect(source).toContain('min="0.35"');
    expect(source).toContain('max="0.55"');
    expect(source).toContain("Elle seç (yedek)");
  });

  it("offers automatic facecam detection as the primary streamer action", () => {
    expect(source).toContain("onAutoDetectFacecam?:");
    expect(source).toContain("facecamDetectBusy?: boolean");
    expect(source).toContain("Facecam'i otomatik bul");
    expect(source).toContain("Facecam aranıyor…");
  });
});
