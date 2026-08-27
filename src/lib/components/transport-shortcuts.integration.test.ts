import { describe, expect, it } from "vitest";
import pageSource from "../../routes/+page.svelte?raw";
import compositorSource from "./CompositorPlayer.svelte?raw";
import timelineSource from "./Timeline.svelte?raw";

describe("editor transport shortcut wiring", () => {
  it("routes global playback and seeking through the shared shortcut resolver", () => {
    expect(pageSource).toContain("resolveTransportShortcut(e)");
    expect(pageSource).toContain("applyTransportSeek(");
    expect(pageSource).toContain("playbackStartTime(playheadTime, playheadDuration)");
    expect(pageSource).toContain("e.defaultPrevented || isTextEntryTarget(e.target)");
    expect(pageSource).toContain("transportIsBlocked()");
    expect(pageSource).toContain("isNativeSpaceControl(e.target)");
    expect(pageSource).toContain("isNativeSeekControl(e.target)");
  });

  it("uses the real timeline duration for every focused arrow seek", () => {
    expect(timelineSource).toContain("handleFocusedTimelineSeek(e)");
    expect(timelineSource).toContain(
      "applyTransportSeek(currentTime, duration, shortcut)",
    );
    const focusedSeek = timelineSource.match(
      /function handleFocusedTimelineSeek\([\s\S]*?(?=\n\s*function )/,
    )?.[0];
    expect(focusedSeek, "focused timeline seek handler").toBeDefined();
    expect(focusedSeek).not.toContain("onScrubBegin?.()");
    expect(timelineSource).not.toMatch(
      /handleTimelineKeydown[\s\S]{0,500}currentTime\s*\+\s*\(e\.key/,
    );
  });

  it("overrides the transport range's 10 ms native arrows and ignores held Space", () => {
    expect(compositorSource).toContain("onkeydown={handleTransportKeydown}");
    expect(compositorSource).toContain(
      "currentTime = applyTransportSeek(currentTime, duration, shortcut)",
    );
    expect(compositorSource).toMatch(
      /function handleStageKeydown[\s\S]{0,260}if \(event\.repeat\) return/,
    );
    const transportSeek = compositorSource.match(
      /function handleTransportKeydown\([\s\S]*?(?=\n\s*function )/,
    )?.[0];
    expect(transportSeek, "transport seek handler").toBeDefined();
    expect(transportSeek).not.toMatch(/isPlaying\s*=\s*false/);
    expect(compositorSource).toContain(
      "currentTime = playbackStartTime(currentTime, duration)",
    );
    expect(compositorSource).toContain('shortcut: "Alt ←"');
    expect(compositorSource).toContain('shortcut: "Alt →"');
  });
});
