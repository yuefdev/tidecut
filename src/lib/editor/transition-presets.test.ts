import { describe, expect, it } from "vitest";
import { TRANSITION_FX } from "./transition-fx";
import { TRANSITION_TYPES } from "./timeline-engine";
import {
  TRANSITION_PRESETS,
  isTransitionExportApproximate,
  updateTransitionSide,
} from "./transition-presets";

describe("transition preset catalog", () => {
  it("covers every timeline transition exactly once", () => {
    const catalogTypes = TRANSITION_PRESETS.map((preset) => preset.type);
    expect(new Set(catalogTypes).size).toBe(catalogTypes.length);
    expect([...catalogTypes].sort()).toEqual([...TRANSITION_TYPES].sort());
  });

  it("keeps every frame-backed effect connected to its asset metadata", () => {
    const catalogFx = TRANSITION_PRESETS
      .filter((preset) => preset.assetDir)
      .map((preset) => ({
        type: preset.type,
        mode: preset.assetMode,
        dir: preset.assetDir,
      }));
    expect(catalogFx).toEqual(TRANSITION_FX);
  });

  it("assigns a useful duration when a transition is chosen", () => {
    expect(
      updateTransitionSide(
        { type: "none", duration: 0 },
        "type",
        "crossfade",
        4,
      ),
    ).toEqual({ type: "crossfade", duration: 0.65 });
  });

  it("clamps duration to the available envelope and clears zero-duration effects", () => {
    expect(
      updateTransitionSide(
        { type: "crossfade", duration: 0.65 },
        "duration",
        9,
        1.2,
      ),
    ).toEqual({ type: "crossfade", duration: 1.2 });
    expect(
      updateTransitionSide(
        { type: "crossfade", duration: 0.65 },
        "duration",
        0,
        4,
      ),
    ).toEqual({ type: "none", duration: 0 });
  });

  it("labels motion and graphic export fallbacks honestly", () => {
    expect(isTransitionExportApproximate("wipe-left")).toBe(false);
    expect(isTransitionExportApproximate("slide-left")).toBe(true);
    expect(isTransitionExportApproximate("fx-glitch")).toBe(true);
  });
});
