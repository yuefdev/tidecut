import { describe, expect, it, vi } from "vitest";
import {
  MediaPlaybackIntentController,
  WebAudioMixer,
  resolveLayerViewport,
} from "./media-runtime";

describe("Canvas compositor viewport", () => {
  it("maps normalized split panels into delivery pixels", () => {
    expect(resolveLayerViewport({ viewport: { x: 0, y: 0.42, width: 1, height: 0.58 } }, 1080, 1920))
      .toEqual({ x: 0, y: 806.4, width: 1080, height: 1113.6 });
  });

  it("fails open to the full frame for unsafe viewport data", () => {
    expect(resolveLayerViewport(
      { viewport: { x: 0.8, y: 0, width: 0.4, height: 1 } },
      1080,
      1920,
    )).toEqual({ x: 0, y: 0, width: 1080, height: 1920 });
  });
});

function audioNode(extra: Record<string, unknown> = {}) {
  return {
    connect: vi.fn((destination: unknown) => destination),
    disconnect: vi.fn(),
    ...extra,
  };
}

function audioParam() {
  return {
    value: 1,
    cancelScheduledValues: vi.fn(),
    setValueAtTime: vi.fn(),
    linearRampToValueAtTime: vi.fn(),
  };
}

function mixerFixture() {
  const sources: ReturnType<typeof audioNode>[] = [];
  const destination = audioNode();
  const context = {
    destination,
    currentTime: 0,
    state: "running",
    resume: vi.fn(async () => undefined),
    createMediaElementSource: vi.fn(() => {
      const source = audioNode();
      sources.push(source);
      return source;
    }),
    createGain: vi.fn(() => audioNode({ gain: audioParam() })),
    createStereoPanner: vi.fn(() => audioNode({ pan: audioParam() })),
  } as unknown as AudioContext;
  const element = {
    pause: vi.fn(),
    paused: true,
  } as unknown as HTMLMediaElement;
  return { context, element, mixer: new WebAudioMixer(context) };
}

describe("WebAudioMixer media element lifecycle", () => {
  it("reuses the one legal source node when a detached media element returns", () => {
    const { context, element, mixer } = mixerFixture();
    mixer.attach("video", element);
    mixer.detach("video");
    mixer.attach("video", element);

    expect(context.createMediaElementSource).toHaveBeenCalledTimes(1);
  });

  it("moves one media element between strips without creating another source node", () => {
    const { context, element, mixer } = mixerFixture();
    mixer.attach("video", element);
    mixer.attach("voice", element);

    expect(context.createMediaElementSource).toHaveBeenCalledTimes(1);
  });

  it("keeps a visible video playing when only its audio backing is replaced", () => {
    const { context, element: video, mixer } = mixerFixture();
    const cleanedAudio = {
      pause: vi.fn(),
      paused: true,
    } as unknown as HTMLMediaElement;
    mixer.attach("video", video);
    mixer.attach("video", cleanedAudio, { pausePreviousElement: false });

    expect(video.pause).not.toHaveBeenCalled();
    expect(context.createMediaElementSource).toHaveBeenCalledTimes(2);
  });

  it("captures a visible video once so native audio cannot bypass a cleaned strip", () => {
    const { context, element: video, mixer } = mixerFixture();
    mixer.capture(video);
    mixer.capture(video);

    expect(context.createMediaElementSource).toHaveBeenCalledTimes(1);
    expect(video.pause).not.toHaveBeenCalled();
  });

  it("does not let an older pending play undo a newer pause command", async () => {
    const { mixer } = mixerFixture();
    let releasePlay!: () => void;
    const playGate = new Promise<void>((resolve) => (releasePlay = resolve));
    let paused = true;
    const element = {
      get paused() {
        return paused;
      },
      play: vi.fn(async () => {
        await playGate;
        paused = false;
      }),
      pause: vi.fn(() => {
        paused = true;
      }),
      playbackRate: 1,
    } as unknown as HTMLMediaElement;
    const source = {
      clipId: "voice",
      trackId: "track",
      file: "voice.wav",
      sourceTime: 0,
      localTime: 0,
      playbackRate: 1,
      gain: 1,
      pan: 0,
    };
    mixer.attach("voice", element);

    const stalePlay = mixer.sync([source], { playing: true });
    await Promise.resolve();
    await mixer.sync([source], { playing: false });
    releasePlay();
    await stalePlay;

    expect(element.pause).toHaveBeenCalled();
    expect(element.paused).toBe(true);
  });
});

describe("direct media playback intent", () => {
  it("stops a video when an older play promise settles after a rewind", async () => {
    const playback = new MediaPlaybackIntentController();
    let releasePlay!: () => void;
    const playGate = new Promise<void>((resolve) => (releasePlay = resolve));
    let paused = true;
    const video = {
      get paused() {
        return paused;
      },
      play: vi.fn(async () => {
        await playGate;
        paused = false;
      }),
      pause: vi.fn(() => {
        paused = true;
      }),
    } as unknown as HTMLVideoElement;

    const stalePlay = playback.play(video);
    playback.pause(video);
    releasePlay();
    await stalePlay;

    expect(video.pause).toHaveBeenCalled();
    expect(video.paused).toBe(true);
  });

  it("does not stop a newer play intent when an older request settles", async () => {
    const playback = new MediaPlaybackIntentController();
    let releaseFirst!: () => void;
    const firstGate = new Promise<void>((resolve) => (releaseFirst = resolve));
    let paused = true;
    let playCount = 0;
    const video = {
      get paused() {
        return paused;
      },
      play: vi.fn(async () => {
        playCount += 1;
        if (playCount === 1) await firstGate;
        paused = false;
      }),
      pause: vi.fn(() => {
        paused = true;
      }),
    } as unknown as HTMLVideoElement;

    const olderPlay = playback.play(video);
    await playback.play(video);
    releaseFirst();
    await olderPlay;

    expect(video.paused).toBe(false);
  });
});
