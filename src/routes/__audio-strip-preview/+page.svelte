<script lang="ts">
  import { onMount, tick } from "svelte";
  import Timeline from "$lib/components/Timeline.svelte";
  import { createClip, createTrack } from "$lib/editor/timeline-engine";

  let timelineRef = $state<any>();
  let currentTime = $state(8);
  let projectChangeCount = $state(0);
  let projectChangeSerial = 0;
  let trackProjectChanges = false;

  function handleProjectStateChange() {
    if (!trackProjectChanges) return;
    projectChangeSerial += 1;
    projectChangeCount = projectChangeSerial;
  }

  onMount(() => {
    timelineRef?.loadProjectState({
      version: 2,
      tracks: [
        createTrack("v1", "V1", "video"),
        createTrack("a1", "A1", "audio"),
        createTrack("t1", "T1", "text"),
      ],
      clips: [
        createClip({
          id: "video-preview",
          trackId: "v1",
          kind: "video",
          file: "modern-demo-video.mp4",
          start: 0,
          duration: 30,
          sourceDuration: 30,
          volume: 1,
          keyframes: {
            volume: [
              { time: 7.82, value: 1, easing: "linear" },
              { time: 8, value: 0.35, easing: "linear" },
              { time: 16, value: 0.35, easing: "linear" },
              { time: 16.18, value: 1, easing: "linear" },
            ],
          },
          waveform: Array.from({ length: 160 }, (_, index) =>
            0.16 + Math.abs(Math.sin(index * 0.31)) * 0.76,
          ),
        }),
      ],
      selectedClipId: "video-preview",
    });
    void (async () => {
      await tick();
      await tick();
      projectChangeSerial = 0;
      projectChangeCount = 0;
      trackProjectChanges = true;
    })();
  });
</script>

<svelte:head><title>Audio strip preview</title></svelte:head>

<main>
  <Timeline
    bind:this={timelineRef}
    bind:currentTime
    duration={30}
    onProjectStateChange={handleProjectStateChange}
  />
  <output class="change-count" aria-label="Proje değişiklik sayısı"
    >{projectChangeCount}</output
  >
</main>

<style>
  :global(body) {
    margin: 0;
    background: #0c0c0c;
  }

  main {
    width: 100vw;
    height: 620px;
  }

  .change-count {
    position: fixed;
    top: 8px;
    right: 8px;
    z-index: 999;
    padding: 4px 7px;
    color: #ffe2a0;
    background: #251b08;
    border: 1px solid #866421;
    border-radius: 4px;
    font: 700 10px "JetBrains Mono", monospace;
  }
</style>
