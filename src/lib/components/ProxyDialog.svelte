<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    cancelRender,
    desktopRuntimeAvailable,
    ensureFfmpeg,
    getProxyCacheStats,
    getRenderCapabilities,
    pruneProxyCache,
    probeMediaDuration,
    startProxyWithListener,
  } from "$lib/render/client";
  import type {
    ProxyCacheStats,
    RenderCapabilities,
    RenderJobSnapshot,
  } from "$lib/render/types";

  interface Props {
    open: boolean;
    mediaFiles: string[];
    onClose: () => void;
    onProxyReady?: (sourcePath: string, proxyPath: string) => void;
    onCachePruned?: () => void;
  }

  let { open, mediaFiles, onClose, onProxyReady, onCachePruned }: Props = $props();
  let profile = $state<"540p" | "720p">("720p");
  let cacheLimitGb = $state(5);
  let stats = $state<ProxyCacheStats | null>(null);
  let capabilities = $state<RenderCapabilities | null>(null);
  let jobs = $state<Record<string, RenderJobSnapshot>>({});
  let preparingPath = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let initialized = false;
  const unlisteners = new Map<string, () => void>();
  let videos = $derived(
    mediaFiles.filter((path) => /\.(mp4|mov|m4v|mkv|avi|webm|mxf|ts|mts|m2ts|wmv|flv|ogv)$/i.test(path)),
  );

  $effect(() => {
    if (open && !initialized) {
      initialized = true;
      void refresh();
    } else if (!open) {
      initialized = false;
    }
  });

  onDestroy(() => {
    for (const unlisten of unlisteners.values()) unlisten();
    unlisteners.clear();
  });

  async function refresh() {
    busy = true;
    error = null;
    try {
      if (!desktopRuntimeAvailable()) {
        capabilities = await getRenderCapabilities();
        return;
      }
      [stats, capabilities] = await Promise.all([
        getProxyCacheStats(),
        getRenderCapabilities(),
      ]);
    } catch (reason) {
      error = formatError(reason);
    } finally {
      busy = false;
    }
  }

  async function installEngine() {
    busy = true;
    error = null;
    try {
      capabilities = await ensureFfmpeg();
    } catch (reason) {
      error = formatError(reason);
    } finally {
      busy = false;
    }
  }

  async function createProxy(path: string) {
    if (preparingPath) return;
    preparingPath = path;
    error = null;
    try {
      const durationMs = await probeMediaDuration(path, capabilities?.ffmpegPath || undefined);
      const subscription = await startProxyWithListener({
        sourcePath: path,
        durationMs,
        profile,
        frameRate: 30,
        force: jobs[path]?.status === "completed",
      }, (snapshot) => {
        jobs = { ...jobs, [path]: snapshot };
        if (["completed", "cancelled", "failed"].includes(snapshot.status)) {
          if (snapshot.status === "completed") onProxyReady?.(path, snapshot.outputPath);
          unlisteners.get(snapshot.jobId)?.();
          unlisteners.delete(snapshot.jobId);
          void refreshStats();
        }
      });
      const job = subscription.job;
      jobs = { ...jobs, [path]: job };
      if (job.status !== "completed") {
        if (["cancelled", "failed"].includes(job.status)) {
          subscription.unlisten();
          await refreshStats();
        } else {
          unlisteners.set(job.jobId, subscription.unlisten);
        }
      } else {
        onProxyReady?.(path, job.outputPath);
        subscription.unlisten();
        await refreshStats();
      }
    } catch (reason) {
      error = formatError(reason);
    } finally {
      preparingPath = null;
    }
  }

  async function cancel(job: RenderJobSnapshot) {
    try {
      const snapshot = await cancelRender(job.jobId);
      const path = Object.entries(jobs).find(([, item]) => item.jobId === job.jobId)?.[0];
      if (path) jobs = { ...jobs, [path]: snapshot };
    } catch (reason) {
      error = formatError(reason);
    }
  }

  async function prune() {
    busy = true;
    error = null;
    try {
      const result = await pruneProxyCache(cacheLimitGb * 1024 ** 3);
      stats = result.after;
      onCachePruned?.();
    } catch (reason) {
      error = formatError(reason);
    } finally {
      busy = false;
    }
  }

  async function refreshStats() {
    try {
      stats = await getProxyCacheStats();
    } catch {
      // Job state remains useful even if the optional stats refresh fails.
    }
  }

  function formatError(value: unknown) {
    if (value instanceof Error) return value.message;
    if (typeof value === "string") return value;
    const candidate = value as { userMessage?: string; message?: string } | null;
    return candidate?.userMessage ?? candidate?.message ?? "Proxy işlemi başarısız.";
  }

  function name(path: string) {
    return path.split(/[/\\]/).pop() || path;
  }

  function bytes(value = 0) {
    if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
    if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
    return `${(value / 1024 ** 3).toFixed(2)} GB`;
  }
</script>

{#if open}
  <div class="backdrop">
    <div class="dialog" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="proxy-title">
      <header>
        <div><span class="eyebrow">Performans</span><h2 id="proxy-title">Proxy ve cache</h2></div>
        <button class="close" onclick={onClose} aria-label="Kapat">×</button>
      </header>

      <div class="cache-card">
        <div><strong>{stats ? bytes(stats.totalBytes) : "—"}</strong><small>{stats?.fileCount ?? 0} proxy · {stats?.partialFileCount ?? 0} yarım dosya</small></div>
        <div class="cache-actions">
          <select bind:value={cacheLimitGb} aria-label="Cache limiti"><option value={1}>1 GB</option><option value={5}>5 GB</option><option value={10}>10 GB</option><option value={25}>25 GB</option></select>
          <button onclick={prune} disabled={busy}>LRU temizle</button>
        </div>
      </div>

      <div class="engine" class:error={!capabilities?.available}>
        <span></span>{capabilities?.available ? `FFmpeg hazır${capabilities.version ? ` · ${capabilities.version}` : ""}` : capabilities?.diagnostic?.userMessage ?? "FFmpeg proxy motoru hazır değil"}
        {#if desktopRuntimeAvailable() && !capabilities?.available}<button onclick={installEngine} disabled={busy}>Kur</button>{/if}
      </div>

      <div class="profile"><span>Proxy profili</span><button class:active={profile === "540p"} onclick={() => (profile = "540p")}>540p · küçük</button><button class:active={profile === "720p"} onclick={() => (profile = "720p")}>720p · dengeli</button></div>

      <div class="media-list">
        {#if videos.length === 0}
          <div class="empty">Proxy oluşturulabilecek video yok.</div>
        {/if}
        {#each videos as path}
          {@const job = jobs[path]}
          <div class="media-row">
            <div class="media-copy"><strong>{name(path)}</strong><small>{job?.status === "completed" ? `Hazır · ${job.outputPath}` : job?.status === "failed" ? job.error?.userMessage : job ? `${Math.round(job.progressPercent)}% · ${job.status}` : "Orijinal medya"}</small></div>
            {#if job && ["queued", "running", "cancelling"].includes(job.status)}
              <div class="mini-progress"><div style={`width:${job.progressPercent}%`}></div></div>
              <button class="cancel" onclick={() => cancel(job)} disabled={job.status === "cancelling"}>İptal</button>
            {:else}
              <button onclick={() => createProxy(path)} disabled={Boolean(preparingPath) || !capabilities?.available}>{preparingPath === path ? "Hazırlanıyor…" : job?.status === "completed" ? "Yenile" : "Proxy üret"}</button>
            {/if}
          </div>
        {/each}
      </div>
      {#if error}<div class="error-box" role="alert">{error}</div>{/if}
      <footer><span>{stats?.cacheDir ?? (desktopRuntimeAvailable() ? "Cache konumu yükleniyor…" : "Cache masaüstü uygulamasında yönetilir")}</span><button onclick={onClose}>Bitti</button></footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1060; display: grid; place-items: center; padding: 24px; background: rgba(0,0,0,.76); backdrop-filter: blur(8px); }
  .dialog { width: min(720px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto; padding: 21px; color: #e5e8e7; background: #121313; border: 1px solid #2b2d2c; border-radius: 14px; box-shadow: 0 30px 100px rgba(0,0,0,.6); }
  header, footer, .cache-card, .engine, .profile, .media-row { display: flex; align-items: center; }
  header, footer, .cache-card { justify-content: space-between; }
  .eyebrow { color: #68dcb6; font-size: 9px; font-weight: 800; letter-spacing: .13em; text-transform: uppercase; }
  h2 { margin: 3px 0 0; font-size: 18px; }
  button, select { font: inherit; }
  button { cursor: pointer; }
  button:disabled { cursor: not-allowed; opacity: .45; }
  .close { width: 30px; height: 30px; color: #888; background: #1a1b1b; border: 1px solid #303231; border-radius: 7px; font-size: 19px; }
  .cache-card { margin: 18px 0 10px; padding: 13px; background: linear-gradient(135deg,#1b2220,#181919); border: 1px solid #2b3330; border-radius: 9px; }
  .cache-card strong, .cache-card small { display: block; }
  .cache-card strong { color: #b8e0d3; font-size: 18px; }
  .cache-card small { margin-top: 3px; color: #6b716e; font-size: 9px; }
  .cache-actions { display: flex; gap: 6px; }
  select, .cache-actions button, .profile button, .media-row button, footer button { padding: 6px 9px; color: #aeb3b1; background: #1c1e1d; border: 1px solid #333634; border-radius: 6px; font-size: 9px; }
  .engine { gap: 7px; min-height: 26px; color: #6f7773; font-size: 10px; }
  .engine > span { width: 7px; height: 7px; background: #64d7b1; border-radius: 50%; }
  .engine.error > span { background: #dc8579; }
  .engine button { margin-left: auto; padding: 4px 8px; color: #b8d9ce; background: transparent; border: 1px solid #343735; border-radius: 5px; }
  .profile { gap: 6px; margin: 9px 0; }
  .profile > span { margin-right: auto; color: #777c7a; font-size: 10px; }
  .profile button.active { color: #0c1915; background: #62d7b1; border-color: #73e6bf; }
  .media-list { display: grid; gap: 5px; max-height: 310px; margin: 12px 0; overflow: auto; }
  .media-row { gap: 9px; padding: 9px 10px; background: #181919; border: 1px solid #292b2a; border-radius: 7px; }
  .media-copy { min-width: 0; flex: 1; }
  .media-copy strong, .media-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .media-copy strong { font-size: 11px; }
  .media-copy small { margin-top: 3px; color: #626765; font-size: 8px; }
  .mini-progress { width: 80px; height: 3px; overflow: hidden; background: #2b2d2c; border-radius: 9px; }
  .mini-progress div { height: 100%; background: #62d7b1; }
  .cancel { color: #da9489 !important; }
  .empty { padding: 34px; color: #535856; text-align: center; border: 1px dashed #2b2d2c; border-radius: 7px; font-size: 10px; }
  .error-box { padding: 8px 10px; color: #ecaaa0; background: rgba(194,76,61,.1); border-radius: 6px; font-size: 10px; }
  footer { gap: 12px; padding-top: 13px; border-top: 1px solid #292b2a; }
  footer span { min-width: 0; overflow: hidden; color: #555b58; text-overflow: ellipsis; white-space: nowrap; font-size: 8px; }
</style>
