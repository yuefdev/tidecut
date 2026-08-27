import { describe, expect, it } from "vitest";
import browserSource from "./TransitionBrowser.svelte?raw";
import inspectorSource from "./Inspector.svelte?raw";
import timelineSource from "./Timeline.svelte?raw";
import toolbarSource from "./Toolbar.svelte?raw";
import pageSource from "../../routes/+page.svelte?raw";

describe("transition editing UI", () => {
  it("hosts the transition gallery in the shared media library and wires the toolbar launcher", () => {
    expect(pageSource).toContain('role="tablist" aria-label="Kütüphane modu"');
    expect(pageSource).toContain("<TransitionBrowser");
    expect(pageSource).toContain("{@render libraryContent()}");
    expect(pageSource).toContain('title="Kütüphane"');
    expect(pageSource).toContain("onTransitionToggle={toggleTransitionLibrary}");
    expect(toolbarSource).toContain("onclick={onTransitionToggle}");
    expect(toolbarSource).toContain("Geçişler</span>");
    expect(inspectorSource).not.toContain('id="transition-section-title"');
    expect(inspectorSource).not.toContain("TRANSITION_OPTIONS");
  });

  it("renders every filtered preset through a moving example preview", () => {
    expect(browserSource).toContain("{#each visiblePresets as preset (preset.type)}");
    expect(browserSource).toContain("<TransitionPreview");
    expect(browserSource).toContain("TRANSITION_CATEGORY_OPTIONS");
    expect(browserSource).toContain("prefers-reduced-motion: reduce");
    expect(browserSource).toContain("getPreviewFrame(preset.type, preset.assetDir)");
    expect(browserSource).toContain("isTransitionExportApproximate(preset.type)");
  });

  it("keeps duration changes as a local draft until the control is committed", () => {
    const range = browserSource.match(
      /id="transition-duration"[\s\S]*?\/>/,
    )?.[0];
    expect(range, "transition duration range").toBeDefined();
    expect(range).toContain("oninput={updateDraftDuration}");
    expect(range).toContain("onchange={commitDraftDuration}");
    const draftHandler = browserSource.match(
      /function updateDraftDuration[\s\S]*?(?=\n\s*function commitDraftDuration)/,
    )?.[0];
    expect(draftHandler).not.toContain("onChange?.(");
  });

  it("previews timeline handle movement without mutating clips and commits once on pointerup", () => {
    const dragUpdate = timelineSource.match(
      /function updateTransitionDurationDrag[\s\S]*?(?=\n\s*function finishTransitionDurationDrag)/,
    )?.[0];
    const dragFinish = timelineSource.match(
      /function finishTransitionDurationDrag[\s\S]*?(?=\n\s*function clearTransitionDurationDrag)/,
    )?.[0];
    expect(dragUpdate, "transition drag update").toBeDefined();
    expect(dragUpdate).toContain("transitionDurationDrag = {");
    expect(dragUpdate).not.toContain("updateClip(");
    expect(dragUpdate).not.toContain("pushHistory(");
    expect(dragFinish, "transition drag finish").toBeDefined();
    expect(dragFinish?.match(/commitClipTransitionChange/g)).toHaveLength(1);
    expect(timelineSource).toContain("updateTransitionDurationDrag(e)");
    expect(timelineSource).toContain("finishTransitionDurationDrag(e, true)");
    expect(timelineSource).toContain("finishTransitionDurationDrag(e, false)");
  });

  it("exposes selectable envelopes, accessible handles and locked-track guards", () => {
    expect(timelineSource).toContain('class="transition-envelope transition-envelope-in"');
    expect(timelineSource).toContain('class="transition-envelope transition-envelope-out"');
    expect(timelineSource).toContain('role="slider"');
    expect(timelineSource).toContain('aria-label="Giriş geçişinin bitişini ayarla"');
    expect(timelineSource).toContain('aria-label="Çıkış geçişinin başlangıcını ayarla"');
    expect(timelineSource).toContain("disabled={isClipLocked(clip)}");
    expect(timelineSource).toContain("if (!clip || isClipLocked(clip)) return false;");
    expect(timelineSource).toContain("if (transitionDurationDrag) clearTransitionDurationDrag()");
    expect(pageSource).toContain("onTransitionSelect={openTransitionLibraryForSide}");
  });
});
