import { describe, expect, it } from "vitest";
import timelineSource from "./Timeline.svelte?raw";
import compositorSource from "./CompositorPlayer.svelte?raw";
import inspectorSource from "./Inspector.svelte?raw";
import pageSource from "../../routes/+page.svelte?raw";

describe("timeline clip layout", () => {
  it("keeps border and padding inside the time-based clip width", () => {
    const clipRule = timelineSource.match(
      /^\s*\.clip\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];

    expect(clipRule, "base .clip CSS rule").toBeDefined();
    expect(clipRule).toMatch(/box-sizing:\s*border-box\s*;/);
    expect(clipRule).toMatch(/overflow:\s*hidden\s*;/);
  });

  it("keeps the trim drag anchor at the actually applied clamped position", () => {
    expect(timelineSource).toMatch(
      /trimDrag\.lastClientX\s*\+=\s*\(result\.appliedDelta\s*\/\s*viewDuration\)\s*\*\s*rect\.width/,
    );
    expect(timelineSource).not.toMatch(
      /trimDrag\.lastClientX\s*=\s*e\.clientX/,
    );
  });

  it("separates the roomy ruler span from the real media content end", () => {
    expect(timelineSource).toMatch(
      /viewDuration\s*=\s*\$derived\([\s\S]*MIN_TIMELINE_DURATION[\s\S]*timelineContentEnd/,
    );
    expect(timelineSource).toMatch(
      /const nextDuration = timelineContentEnd;[\s\S]*onDurationChange\(nextDuration\)/,
    );
    expect(timelineSource).not.toContain(
      "const nextDuration = Math.max(MIN_TIMELINE_DURATION, maxEnd)",
    );
  });

  it("preserves clip child identity across immutable timeline updates", () => {
    expect(timelineSource).toMatch(
      /\{#each\s+clips\.filter\([^}]+\)\s+as\s+clip\s+\(clip\.id\)\}/,
    );
  });

  it("renders background AI framing progress on its source clip", () => {
    expect(timelineSource).toContain("autoReframeJob?: TimelineAutoReframeJob | null");
    expect(timelineSource).toContain('class="clip-auto-reframe-job"');
    expect(timelineSource).toContain('aria-label="Klip akıllı kadraj ilerlemesi"');
    expect(timelineSource).toContain("onAutoReframeJobOpen?.(clip.id)");
  });

  it("previews clip moves without mutating the real clip before pointerup", () => {
    const previewHandler = timelineSource.match(
      /function updateClipMovePreview\([\s\S]*?(?=\n\s*function clearClipDrag)/,
    )?.[0];
    const pointerMoveHandler = timelineSource.match(
      /function handleWindowPointerMove\([\s\S]*?(?=\n\s*function handleWindowPointerUp)/,
    )?.[0];
    const pointerUpHandler = timelineSource.match(
      /function handleWindowPointerUp\([\s\S]*?(?=\n\s*function handleWindowPointerCancel)/,
    )?.[0];

    expect(pointerMoveHandler, "timeline pointer move handler").toBeDefined();
    expect(pointerMoveHandler).toContain("updateClipMovePreview(e)");
    expect(pointerMoveHandler).not.toMatch(
      /clips\s*=\s*clips\.map\(\(clip\).*clipDragId/s,
    );
    expect(pointerUpHandler, "timeline pointer up handler").toBeDefined();
    expect(pointerUpHandler).toContain("const result = moveClip(");
    expect(pointerUpHandler).toContain("preview.requestedStart");
    expect(pointerUpHandler).toContain(
      "{ mode: preview.mode, rippleSource: preview.ripple }",
    );
    expect(previewHandler, "clip move preview handler").toBeDefined();
    expect(previewHandler).toContain("finalStart = insertedClip.start");
    expect(previewHandler!.indexOf("findInsertBoundaryNearPointer(")).toBeLessThan(
      previewHandler!.indexOf("const moveCandidates"),
    );
    expect(previewHandler).toContain(
      "rippleMode && insertionTime !== null",
    );
    expect(previewHandler).toContain(
      '{ mode: "insert", rippleSource: rippleMode }',
    );
    expect(previewHandler).toContain(
      '{ mode: "move", rippleSource: rippleMode }',
    );
    expect(timelineSource).toContain('class="clip move-preview"');
    expect(timelineSource).toContain("class:invalid-clip-move-target");
    expect(timelineSource).toContain("findInsertBoundaryNearPointer");
    expect(timelineSource).toContain("insert-shift-preview");
    expect(timelineSource).toContain("ripple-close-preview");
    expect(timelineSource).toMatch(
      /\(previewStart\s*-\s*clip\.start\)\s*\/\s*viewDuration/,
    );
    expect(timelineSource).toContain(
      "kaynak boşluğu kapanacak",
    );
    expect(timelineSource).toContain(
      "Araya eklemek ve boşluğu kapatmak için Ripple'ı açın.",
    );
  });

  it("provides clip and empty-timeline context menus", () => {
    expect(timelineSource).toContain(
      "oncontextmenu={(e) => openClipContextMenu(e, clip)}",
    );
    expect(timelineSource).toContain(
      "oncontextmenu={openTimelineContextMenu}",
    );
    expect(timelineSource).toContain("Sil ve boşluğu kapat");
    expect(timelineSource).toContain("Konum, boyut ve sesi sıfırla");
  });

  it("exposes compact, state-aware timeline edit actions", () => {
    expect(timelineSource).toContain('class="timeline-quick-actions"');
    expect(timelineSource).toContain('aria-label="Geri al"');
    expect(timelineSource).toContain('aria-label="İleri al"');
    expect(timelineSource).toContain("onclick={splitSelectedClipAtPlayhead}");
    expect(timelineSource).toContain("disabled={!canSplitSelectedClip}");
    expect(timelineSource).toContain("onclick={deleteSelectedClip}");
    expect(timelineSource).toContain("disabled={!canDeleteSelectedClip}");
    expect(timelineSource).toContain("onclick={cancelActiveTimelineOperation}");
    expect(timelineSource).toContain("disabled={!canCancelTimelineAction}");
    expect(timelineSource).toContain(
      "let undoStack = $state.raw<EditorSnapshot[]>([])",
    );

    const cancelHandler = timelineSource.match(
      /function cancelActiveTimelineOperation[\s\S]*?(?=\n\s*function handleDragLeave)/,
    )?.[0];
    expect(cancelHandler, "shared timeline cancel handler").toBeDefined();
    expect(cancelHandler).toContain("applySnapshot(trimDrag.snapshot)");
    expect(cancelHandler).toContain("clearClipDrag(clipDragPointerId ?? undefined)");
    expect(cancelHandler).toContain('setActiveTool("select")');
    expect(timelineSource).toMatch(
      /e\.code === "Escape"[\s\S]*?cancelActiveTimelineOperation\(\)/,
    );
  });

  it("can separate a video's audio into an independently editable Voice clip", () => {
    expect(timelineSource).toContain("function separateAudioFromVideoClip");
    expect(timelineSource).toContain('kind: "audio"');
    expect(timelineSource).toContain('"Sesi ayır → Voice"');
    expect(timelineSource).toContain('volume: 0');
    expect(timelineSource).toContain('keyframes: { ...source.keyframes, volume: [] }');
    expect(timelineSource).toContain('id: "export-audio"');
    expect(timelineSource).toContain("onAudioExport?.(createClip(menuClip))");
  });

  it("offers smart normalization from the timeline and Inspector with a global busy guard", () => {
    expect(timelineSource).toContain('id: "normalize-audio"');
    expect(timelineSource).toContain('label: "Akıllı normalize"');
    expect(timelineSource).toContain("audioNormalizeBusy ||");
    expect(timelineSource).toContain("onAudioNormalize?.(createClip(menuClip))");
    expect(inspectorSource).toContain("onNormalizeAudio?.()");
    expect(inspectorSource).toContain("audioNormalizeBusy");
    expect(inspectorSource).toContain("audioNormalizeError");
    expect(pageSource).toContain(
      "Boolean(audioNormalizeBusyClipId || voiceRiderBusyClipId)",
    );
    expect(pageSource).toContain("limitGainForVolumeAutomation(");
    expect(pageSource).toContain("applyNormalizationGain(currentClip, peakSafeRecommendation)");
    expect(pageSource).toContain('{ history: "immediate" }');
  });

  it("runs the on-device AI Voice Rider as separate visible automation", () => {
    expect(timelineSource).toContain('id: "ai-voice-rider"');
    expect(timelineSource).toContain('id: "remove-ai-voice-rider"');
    expect(timelineSource).toContain("voiceRiderBusy ||");
    expect(timelineSource).toContain("export function applyVoiceRider(");
    expect(timelineSource).toContain('class="ai-rider-curve"');
    expect(timelineSource).toContain("clip.keyframes.riderGain");
    expect(inspectorSource).toContain("AI · CİHAZDA");
    expect(inspectorSource).toContain("Silero Neural VAD");
    expect(inspectorSource).toContain("voiceRiderCurrentGain");
    expect(inspectorSource).toContain("effectiveVolume");
    expect(pageSource).toContain("await analyzeVoiceRider({");
    expect(pageSource).toMatch(/toVoiceRiderKeyframes\(\s*result\.points,/);
    expect(pageSource).toContain("timelineRef.applyVoiceRider?.(");
    expect(pageSource).toContain(
      "riderGainKeyframes: toAudioGainKeyframes(clip.keyframes.riderGain)",
    );
    expect(pageSource).toContain(
      "riderGainKeyframes={audioExportTarget?.riderGainKeyframes ?? []}",
    );
    expect(pageSource).toContain("selectedManualVolume * selectedVoiceRiderGain");
  });

  it("applies noise cleanup immediately to the whole selected clip and auditions from its start", () => {
    expect(pageSource).toContain(
      "applied = await prepareNoiseCleanupForClip(updatedClip, true);",
    );
    expect(pageSource).toContain("const rewindRevision = ++previewTransportRevision;");
    expect(pageSource).toContain("manualNoiseCleanupRequests.add(manualRequestKey)");
    expect(pageSource).toContain("manualNoiseCleanupRequests.delete(manualRequestKey)");
    expect(pageSource).toContain("await tick();");
    expect(pageSource).toContain("function ownsNoiseCleanupTransport(");
    expect(pageSource).toMatch(
      /let resumeRevision = rewindRevision;[\s\S]*?if \(applied\) \{[\s\S]*?await tick\(\);[\s\S]*?\}[\s\S]*?shouldResume[\s\S]*?isPlaying = true;/,
    );
    expect(pageSource).toContain("transportRevision={previewTransportRevision}");
    expect(pageSource).toContain(
      "Güçlü ortam gürültüsü temizleme klibin tamamına uygulandı · oynatma ve dışa aktarma aktif",
    );
    expect(inspectorSource).toContain("Gürültü azalt");
    expect(inspectorSource).toContain("AI · HIZLI");
    expect(inspectorSource).toContain("Fan, motor, rüzgâr, dip ses ve uğultu");
    expect(inspectorSource).not.toContain("Alt frekans");
    expect(inspectorSource).not.toContain("Şebeke uğultusu");
    expect(inspectorSource).not.toContain("AI miktarı");
    expect(pageSource).toContain("Konuşma bulunmadı · orijinal ses değiştirilmeden korundu");
    expect(inspectorSource).toContain("A/B/C Karşılaştır");
    expect(inspectorSource).toContain("Dışa aktarımda seçili sürüm kullanılır");
    expect(inspectorSource).toContain("Sesi geliştir");
    expect(inspectorSource).toContain("AI · STÜDYO");
    expect(inspectorSource).toContain("Stüdyo hazırla");
    expect(inspectorSource).toContain("MossFormer2 + DeepFilterNet3");
    expect(inspectorSource).toContain('case "mastering": return "Stüdyo tonlaması"');
    expect(inspectorSource).toContain('role="progressbar"');
    expect(pageSource).toContain("await prepareMossFormerTestCleanup({");
    expect(pageSource).toContain("await onAudioEnhancementProgress(");
    expect(pageSource).toContain("renderTimelineWithPreparedHybrid(timelineSnapshot)");
    expect(pageSource).toContain("audioPath: asset.outputPath");
    expect(pageSource).toContain("progress.operationId !== operationId");
    expect(pageSource).toContain("progress.sequence <= activeProgress.sequence");
    expect(pageSource).toContain("async function handleMossFormerPrepare()");
    expect(pageSource).toContain("if (!clip || clip.audioSeparated) return;");
    expect(pageSource).not.toContain("if (!clip?.noiseReduction) return;");
    expect(pageSource).toContain('mode === "mossformer"');
    expect(pageSource).toContain('validAudition?.mode === "original"');
    expect(pageSource).toContain("handleNoiseAuditionModeChange");
    expect(pageSource).toContain(
      '[clip.id]: { signature: audioSourceSignature(current), mode: "cleaned" }',
    );
    expect(compositorSource).toContain("pausePreviousElement: !replacingVisibleVideo");
    expect(compositorSource).toContain("audioMixer.capture(visualElement)");
    expect(compositorSource).toContain("element.muted =");
    expect(compositorSource).toContain("directPlayback.play(element)");
    expect(compositorSource).toContain("directPlayback.pause(element)");
    expect(compositorSource).toContain("pendingTransportSeeks.set");
    expect(compositorSource).toContain("const isContinuousPause = Boolean(");
  });

  it("marks the muted video clip left behind by audio separation instead of leaving a bare 0% slider", () => {
    // The original video clip keeps volume: 0 (so preview/export mixing
    // stays silent) but is flagged so the UI treats it as audio-less rather
    // than showing a confusing muted volume control.
    expect(timelineSource).toContain("audioSeparated: true");
    expect(timelineSource).toContain(
      '(clip.kind === "video" && !clip.audioSeparated) || clip.kind === "audio"',
    );
    expect(timelineSource).toContain("menuClip.audioSeparated");
    expect(inspectorSource).toContain("audioSeparated");
    expect(inspectorSource).toContain(
      '(clipKind === "audio" || clipKind === "video") && !audioSeparated',
    );
  });

  it("keeps an applied audio region visibly overlaid on its clip", () => {
    expect(timelineSource).toContain('class="audio-region-quiet"');
    expect(timelineSource).toContain('class="audio-region-applied-overlay"');
    expect(timelineSource).toContain(
      'style:left={`${(region.start / clip.duration) * 100}%`}',
    );
    expect(timelineSource).toContain(
      'style:width={`${((region.end - region.start) / clip.duration) * 100}%`}',
    );

    const quietRegionRule = timelineSource.match(
      /^\s*\.audio-region-quiet\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];
    expect(quietRegionRule, "persistent audio region CSS rule").toBeDefined();
    expect(quietRegionRule).toMatch(/position:\s*absolute\s*!important\s*;/);
    expect(quietRegionRule).toMatch(/pointer-events:\s*auto\s*;/);
    expect(quietRegionRule).toMatch(/z-index:\s*4\s*!important\s*;/);
    expect(quietRegionRule).toMatch(/background:/);

    const appliedOverlayRule = timelineSource.match(
      /^\s*\.audio-region-applied-overlay\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];
    expect(appliedOverlayRule, "full-height applied audio region CSS rule").toBeDefined();
    expect(appliedOverlayRule).toMatch(/position:\s*absolute\s*!important\s*;/);
    expect(appliedOverlayRule).toMatch(/top:\s*0\s*;/);
    expect(appliedOverlayRule).toMatch(/bottom:\s*0\s*;/);
    expect(appliedOverlayRule).toMatch(/pointer-events:\s*none\s*;/);
    expect(appliedOverlayRule).toMatch(/background:/);
    expect(timelineSource).toContain("clip.volume * 0.35");
    expect(inspectorSource).toContain(
      "audioRegion.selection.volume >= volume - 0.01",
    );
  });

  it("changes region gain vertically and commits only when the pointer is released", () => {
    expect(timelineSource).toContain("function handleAudioRegionGainPointerDown");
    expect(timelineSource).toContain("function handleAudioRegionGainPointerMove");
    expect(timelineSource).toContain("function finishAudioRegionGainDrag");
    expect(timelineSource).toContain('class="audio-region-gain-zone"');
    expect(timelineSource).toContain('data-testid="audio-region-gain-zone"');
    expect(timelineSource).toContain('class="audio-region-gain-value"');

    const gainZoneRule = timelineSource.match(
      /^\s*\.audio-region-gain-zone\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];
    expect(gainZoneRule, "vertical region gain hit area CSS rule").toBeDefined();
    expect(gainZoneRule).toMatch(/pointer-events:\s*auto\s*;/);
    expect(gainZoneRule).toMatch(/cursor:\s*ns-resize\s*;/);
    expect(gainZoneRule).toMatch(/touch-action:\s*none\s*;/);

    const moveHandler = timelineSource.match(
      /function handleAudioRegionGainPointerMove[\s\S]*?(?=\n\s*function finishAudioRegionGainDrag)/,
    )?.[0];
    const finishHandler = timelineSource.match(
      /function finishAudioRegionGainDrag[\s\S]*?(?=\n\s*export function toggleAudioRegionSelectMode)/,
    )?.[0];
    expect(moveHandler, "vertical region gain move handler").toBeDefined();
    expect(moveHandler).not.toContain("commitAudioRegion");
    expect(moveHandler).not.toContain("pushHistory");
    expect(moveHandler).not.toContain("applyProjectState");
    expect(finishHandler, "vertical region gain finish handler").toBeDefined();
    expect(finishHandler).toContain("commitAudioRegion(selection.volume)");
    expect(finishHandler?.match(/commitAudioRegion\(/g)).toHaveLength(1);
    expect(finishHandler!.indexOf("audioRegionGainDrag = null")).toBeLessThan(
      finishHandler!.indexOf("commitAudioRegion(selection.volume)"),
    );
    expect(timelineSource).toContain("finishAudioRegionGainDrag(e, true)");
    expect(inspectorSource).toContain(
      "(audioRegion.selection.volume / Math.max(volume, 0.0001)) * 100",
    );
  });

  it("edits clip and channel gain in relative decibels with a direct timeline gesture", () => {
    expect(timelineSource).toContain('data-testid="clip-volume-drag-handle"');
    expect(timelineSource).toContain("function handleClipVolumeGainPointerDown");
    expect(timelineSource).toContain("function handleClipVolumeGainPointerMove");
    expect(timelineSource).toContain("function finishClipVolumeGainDrag");
    expect(timelineSource).toContain("updateClipVolumeFromDecibels");
    expect(timelineSource).toContain("formatGainReadout");
    expect(timelineSource).toContain("0 dB orijinal ses seviyesidir");
    expect(timelineSource).toContain("const scale = nextVolume / clip.volume");
    expect(timelineSource).toContain("newClip.keyframes.volume.map");

    const clipGainHandleRule = timelineSource.match(
      /^\s*\.clip-volume-drag-handle\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];
    expect(clipGainHandleRule, "clip gain vertical drag control CSS rule").toBeDefined();
    expect(clipGainHandleRule).toMatch(/cursor:\s*ns-resize\s*;/);
    expect(clipGainHandleRule).toMatch(/touch-action:\s*none\s*;/);

    expect(inspectorSource).toContain("Klip kazancı");
    expect(inspectorSource).toContain("Kanal kazancı");
    expect(inspectorSource).toContain("handleVolumeDbChange");
    expect(inspectorSource).toContain("handleTrackGainDbChange");
    expect(inspectorSource).toContain("0 dB klibin orijinal seviyesidir");
  });

  it("lets users remove added media tracks visibly or from the context menu", () => {
    expect(timelineSource).toContain(
      "oncontextmenu={(event) => openTrackContextMenu(event, track.id)}",
    );
    expect(timelineSource).toContain('class="track-switch remove"');
    expect(timelineSource).toContain('id: "remove-track"');
    expect(timelineSource).toContain("deleteTrack(createProjectState(), trackId");
    expect(timelineSource).toContain("cascade: true");
    expect(timelineSource).toContain("window.confirm(");
    expect(timelineSource).toContain("tracks: cloneTracks(tracks)");
  });

  it("gives selected text a focused, multiline editor in the inspector", () => {
    expect(inspectorSource).toContain('placeholder="Önizlemede görünecek metni yazın"');
    expect(inspectorSource).toMatch(/<textarea[\s\S]*?rows="4"/);
    // "+ Metin" hands focus to the inspector's content field via the host page.
    expect(timelineSource).toContain("onTextClipCreated?.()");
    expect(pageSource).toContain('document.getElementById(\n      "text-content",\n    )');
    expect(pageSource).toContain("input?.focus()");
    expect(pageSource).toContain("input?.select()");
    // The tips popover uses a JS-driven fixed-position overlay (not <details>)
    // so it can escape the timeline panel's `overflow: hidden` clipping.
    expect(timelineSource).toContain('class="tool-help-trigger"');
    expect(timelineSource).toContain("position: fixed");
  });

  it("makes the red playhead directly draggable with frame-coalesced scrubbing", () => {
    expect(timelineSource).toContain(
      "onpointerdown={handlePlayheadPointerDown}",
    );
    expect(timelineSource).toContain("queueScrubFrame(e.clientX)");
    expect(timelineSource).toContain("requestAnimationFrame(() =>");

    const handleRule = timelineSource.match(
      /^\s*\.playhead-handle\s*\{([\s\S]*?)^\s*\}/m,
    )?.[1];
    expect(handleRule, "playhead drag handle CSS rule").toBeDefined();
    expect(handleRule).toMatch(/pointer-events:\s*auto\s*;/);
    expect(handleRule).toMatch(/touch-action:\s*none\s*;/);
  });

  it("publishes each decoded paused frame before chasing the latest scrub target", () => {
    const seekedHandler = compositorSource.match(
      /function handleSeeked\([\s\S]*?(?=\n\s*function pauseMedia)/,
    )?.[0];
    expect(seekedHandler, "paused seek handler").toBeDefined();
    expect(seekedHandler!.indexOf("renderFrame();")).toBeLessThan(
      seekedHandler!.indexOf("pendingPausedSeeks.get"),
    );
  });

  it("chases an explicit transport seek that arrived during playback decoding", () => {
    expect(compositorSource).toContain("pendingTransportSeeks.get(element)");
    expect(compositorSource).toContain("transportRevision?: number");
    expect(compositorSource).toContain("previousSync = null;");
  });

  it("keeps the last complete preview frame while a video seek is pending", () => {
    const renderHandler = compositorSource.match(
      /function renderFrame\([\s\S]*?(?=\n\s*function changeAspect)/,
    )?.[0];
    expect(renderHandler, "guarded compositor render path").toBeDefined();
    expect(renderHandler).toMatch(/element instanceof HTMLVideoElement/);
    expect(renderHandler).toMatch(/!element\.seeking/);
    expect(renderHandler).toMatch(/element\.readyState >= 2/);
    expect(compositorSource).toContain(
      "Keep the last complete canvas frame while a seek/decode is pending.",
    );
  });

  it("keeps canvas resize isolated from scrub plans and renders seeked frames directly", () => {
    const resizeEffect = compositorSource.match(
      /\$effect\(\(\) => \{[\s\S]*?compositor\.resize[\s\S]*?(?=\n\s*\$effect)/,
    )?.[0];
    expect(resizeEffect, "aspect-only resize effect").toBeDefined();
    expect(resizeEffect).toContain("untrack(renderFrame)");
    expect(compositorSource).toMatch(
      /element\.onseeked\s*=\s*\(\)\s*=>\s*handleSeeked\(element\)/,
    );
    expect(compositorSource).not.toContain("waitForPresentedVideoFrame");
  });

  it("routes the blue transport slider through a RAF-coalesced scrub path without pausing playback", () => {
    const transport = compositorSource.match(
      /<input\s+aria-label="Oynatma konumu"[\s\S]*?\/>/,
    )?.[0];
    expect(transport, "blue transport slider").toBeDefined();
    expect(transport).toContain(
      "onpointerdown={handleTransportScrubStart}",
    );
    expect(transport).toContain("oninput={handleTransportScrubInput}");
    expect(transport).not.toMatch(/syncMediaToPlan\(\)|renderFrame\(\)/);

    const startHandler = compositorSource.match(
      /function handleTransportScrubStart\([\s\S]*?(?=\n\s*function )/,
    )?.[0];
    expect(startHandler, "transport scrub start handler").toBeDefined();
    expect(startHandler).not.toMatch(/isPlaying\s*=\s*false/);

    const inputHandler = compositorSource.match(
      /function handleTransportScrubInput\([\s\S]*?(?=\n\s*function )/,
    )?.[0];
    expect(inputHandler, "transport scrub input handler").toBeDefined();
    expect(inputHandler).toMatch(/requestAnimationFrame\(/);
    expect(inputHandler).not.toMatch(/isPlaying\s*=\s*false/);
    expect(inputHandler).not.toMatch(/syncMediaToPlan\(\)|renderFrame\(\)/);
  });

  it("replaces generated captions in one guarded undo step", () => {
    const replaceHandler = timelineSource.match(
      /export function replaceGeneratedTextClips\([\s\S]*?(?=\n\s*function addTextClipToTimeline)/,
    )?.[0];

    expect(replaceHandler, "generated caption replacement handler").toBeDefined();
    expect(replaceHandler).toContain("generatedTrackIds");
    expect(replaceHandler).toContain("generatedTrackIds.has(track.id) && track.locked");
    expect(replaceHandler).toContain("existingTrack.type !== \"text\" || existingTrack.locked");
    expect(replaceHandler).toContain("clip.file.startsWith(sourcePrefix)");
    expect(replaceHandler).toContain("!clip.file.startsWith(sourcePrefix)");
    expect(replaceHandler?.match(/pushHistory\(\)/g)).toHaveLength(1);
  });
});
