import { describe, expect, it } from "vitest";
import pageSource from "../../routes/+page.svelte?raw";
import captionStudioSource from "./CaptionQaStudio.svelte?raw";

describe("compact Caption QA inspector layout", () => {
  it("renders as a bounded inspector panel instead of a viewport modal", () => {
    expect(captionStudioSource).toContain(
      'class="caption-panel" data-testid="caption-qa-panel"',
    );
    expect(captionStudioSource).not.toContain('class="backdrop"');
    expect(captionStudioSource).not.toContain('role="dialog"');
    expect(captionStudioSource).not.toContain("aria-modal");

    const panelRule = captionStudioSource.match(
      /^\s*\.caption-panel\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];
    const scrollRule = captionStudioSource.match(
      /^\s*\.panel-scroll\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];

    expect(panelRule, "caption inspector root CSS rule").toBeDefined();
    expect(panelRule).toMatch(/height:\s*100%\s*;/);
    expect(panelRule).toMatch(/display:\s*flex\s*;/);
    expect(panelRule).toMatch(/flex-direction:\s*column\s*;/);
    expect(panelRule).toMatch(/overflow:\s*hidden\s*;/);
    expect(panelRule).not.toMatch(/position:\s*fixed/);

    expect(scrollRule, "caption inspector scroll body CSS rule").toBeDefined();
    expect(scrollRule).toMatch(/min-height:\s*0\s*;/);
    expect(scrollRule).toMatch(/flex:\s*1\s*;/);
    expect(scrollRule).toMatch(/overflow:\s*auto\s*;/);
    expect(captionStudioSource).toContain('class="panel-footer"');
    expect(captionStudioSource).toContain("@container");
  });

  it("keeps review, styling, and advanced settings in compact progressive disclosure", () => {
    expect(captionStudioSource).toContain(
      'class="panel-tabs" role="tablist"',
    );
    expect(captionStudioSource).toContain(">Kontrol</button>");
    expect(captionStudioSource).toContain(">Stil</button>");
    expect(captionStudioSource).toContain(">Ayarlar</button>");
    expect(captionStudioSource).toContain('class="quality-strip"');
    expect(captionStudioSource).toContain("class:low=");
    expect(captionStudioSource).toContain("class:code-switch=");
    expect(captionStudioSource).toContain("Zamanlama korunur");
    expect(captionStudioSource).toContain(
      '<details class="settings-section" open>',
    );
    expect(captionStudioSource).toContain("Özel isim sözlüğü");
    expect(captionStudioSource).toContain("Konuşmacılar");
    expect(captionStudioSource).toContain("Dosya işlemleri");
  });

  it("mounts Caption QA once inside the shared docked and detached Inspector content", () => {
    const snippetStart = pageSource.indexOf("{#snippet inspectorContent()}");
    const snippetEnd = pageSource.indexOf("\n{/snippet}", snippetStart);
    const inspectorSnippet = pageSource.slice(snippetStart, snippetEnd);

    expect(snippetStart, "shared inspector snippet").toBeGreaterThanOrEqual(0);
    expect(snippetEnd, "shared inspector snippet closing tag").toBeGreaterThan(
      snippetStart,
    );
    expect(inspectorSnippet).toContain(
      'class="inspector-mode-tabs" role="tablist"',
    );
    expect(inspectorSnippet).toContain("Klip");
    expect(inspectorSnippet).toContain("Altyazı");
    expect(inspectorSnippet).toContain('{#if captionStudioOpen}');
    expect(inspectorSnippet).toContain("<CaptionQaStudio");
    expect(inspectorSnippet).toContain("{:else if selectedClip}");
    expect(pageSource.match(/<CaptionQaStudio/g) ?? []).toHaveLength(1);
    expect(pageSource.match(/\{@render inspectorContent\(\)\}/g) ?? []).toHaveLength(
      2,
    );

    const openHandler = pageSource.match(
      /function openCaptionStudioPanel\(\)[\s\S]*?(?=\n\s*function closeCaptionStudioPanel)/,
    )?.[0];
    expect(openHandler, "Caption QA panel opener").toBeDefined();
    expect(openHandler).toContain("captionStudioOpen = true");
    expect(openHandler).toContain("inspector: true");
    expect(openHandler).toContain('updatePanelWidth("inspector", 340)');
  });

  it("lets the user pick a timeline audio or video clip and explicitly rescan it", () => {
    expect(captionStudioSource).toContain('id="caption-transcription-source"');
    expect(captionStudioSource).toContain("Tarama kaynağı");
    expect(captionStudioSource).toContain("Seçili klibi yeniden tara");
    expect(captionStudioSource).toContain("onTranscriptionSourceChange");
    expect(captionStudioSource).toContain("yeni ses eklediğinde buradan değiştir");

    expect(pageSource).toContain("captionSourceClipId");
    expect(pageSource).toContain("captionTranscriptionSources");
    expect(pageSource).toContain("transcribeCaptionSourceForCaptions");
    expect(pageSource).toContain("force: true");
  });
});
