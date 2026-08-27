<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";

  export interface MissingReference {
    assetId?: string;
    path: string;
    sizeBytes?: number;
    quickHash?: string;
  }

  interface Candidate {
    path: string;
    confidence: number;
    reason: string;
  }

  interface RelinkResult {
    resolved: Array<{ reference: MissingReference; replacementPath: string; confidence: number }>;
    ambiguous: Array<{ reference: MissingReference; candidates: Candidate[] }>;
    unresolved: MissingReference[];
    scannedFiles: number;
    truncated: boolean;
  }

  interface Props {
    open: boolean;
    missing: MissingReference[];
    result?: RelinkResult | null;
    busy?: boolean;
    onSearch: (roots: string[]) => void | Promise<void>;
    onApply: (replacements: Map<string, string>) => void | Promise<void>;
    onClose: () => void;
  }

  let { open, missing, result = null, busy = false, onSearch, onApply, onClose }: Props = $props();
  let manual = $state<Record<string, string>>({});
  let error = $state<string | null>(null);
  let automatic = $derived(
    new Map(result?.resolved.map((item) => [item.reference.path, item.replacementPath]) ?? []),
  );
  let replacementCount = $derived(automatic.size + Object.keys(manual).length);

  async function chooseSearchRoots() {
    const roots = await openDialog({ directory: true, multiple: true, title: "Medyayı ara" });
    if (!roots) return;
    await onSearch(Array.isArray(roots) ? roots : [roots]);
  }

  async function chooseManual(reference: MissingReference) {
    const path = await openDialog({
      multiple: false,
      title: "Eksik medyanın yeni konumunu seç",
      filters: [{
        name: "Medya",
        extensions: ["mp4", "mov", "avi", "mkv", "webm", "mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "avif"],
      }],
    });
    if (typeof path === "string") manual = { ...manual, [reference.path]: path };
  }

  async function applyReplacements() {
    const replacements = new Map(automatic);
    for (const [from, to] of Object.entries(manual)) replacements.set(from, to);
    if (replacements.size === 0) {
      error = "Uygulanacak eşleşme yok.";
      return;
    }
    await onApply(replacements);
  }

  function name(path: string) {
    return path.split(/[/\\]/).pop() || path;
  }
</script>

{#if open}
  <div class="backdrop">
    <div class="dialog" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="missing-title">
      <header>
        <div>
          <span class="eyebrow">Medya yönetimi</span>
          <h2 id="missing-title">{missing.length} eksik medya dosyası</h2>
        </div>
        <button class="close" onclick={onClose} disabled={busy} aria-label="Kapat">×</button>
      </header>
      <p class="lead">Taşınmış dosyaları klasörlerde otomatik arayın veya her dosya için yeni konumu elle seçin. Boyut ve hızlı hash eşleşmeleri önceliklendirilir.</p>
      <button class="search" onclick={chooseSearchRoots} disabled={busy}>{busy ? "Taranıyor…" : "Arama klasörlerini seç"}</button>

      {#if result}
        <div class="scan-summary">{result.scannedFiles.toLocaleString("tr-TR")} dosya tarandı · {result.resolved.length} kesin · {result.ambiguous.length} olası · {result.unresolved.length} bulunamadı{result.truncated ? " · limit doldu" : ""}</div>
      {/if}

      <div class="file-list">
        {#each missing as reference}
          {@const resolvedPath = manual[reference.path] ?? automatic.get(reference.path)}
          {@const ambiguous = result?.ambiguous.find((item) => item.reference.path === reference.path)}
          <div class="file" class:resolved={Boolean(resolvedPath)}>
            <span class="state">{resolvedPath ? "✓" : "!"}</span>
            <div class="file-main">
              <strong>{name(reference.path)}</strong>
              <small>{reference.path}</small>
              {#if resolvedPath}<small class="replacement">→ {resolvedPath}</small>{/if}
              {#if ambiguous && !resolvedPath}<small>{ambiguous.candidates.length} olası eşleşme bulundu</small>{/if}
            </div>
            <button onclick={() => chooseManual(reference)} disabled={busy}>{resolvedPath ? "Değiştir" : "Bul"}</button>
          </div>
        {/each}
      </div>
      {#if error}<div class="error" role="alert">{error}</div>{/if}
      <footer>
        <button class="secondary" onclick={onClose} disabled={busy}>Şimdilik atla</button>
        <button class="primary" onclick={applyReplacements} disabled={busy || replacementCount === 0}>{replacementCount} bağlantıyı uygula</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1080; display: grid; place-items: center; padding: 24px; background: rgba(0,0,0,.76); backdrop-filter: blur(8px); }
  .dialog { width: min(680px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto; padding: 22px; color: #e5e8e7; background: #121313; border: 1px solid #2b2d2c; border-radius: 14px; box-shadow: 0 30px 100px rgba(0,0,0,.6); }
  header, footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .eyebrow { color: #6bd9b5; font-size: 9px; font-weight: 800; letter-spacing: .13em; text-transform: uppercase; }
  h2 { margin: 4px 0 0; font-size: 18px; }
  .close { width: 30px; height: 30px; color: #888; background: #1a1b1b; border: 1px solid #2e302f; border-radius: 7px; font-size: 19px; cursor: pointer; }
  .lead { color: #777c7a; font-size: 11px; line-height: 1.55; }
  .search { width: 100%; padding: 9px; color: #b9d8ce; background: rgba(98,215,177,.07); border: 1px dashed rgba(98,215,177,.38); border-radius: 7px; cursor: pointer; font-size: 11px; }
  .scan-summary { margin: 12px 0 4px; color: #707573; font-size: 9px; }
  .file-list { display: grid; gap: 5px; max-height: 310px; margin: 12px 0; overflow: auto; }
  .file { display: flex; align-items: center; gap: 10px; padding: 9px 10px; background: #181919; border: 1px solid #292b2a; border-radius: 7px; }
  .state { display: grid; place-items: center; width: 18px; height: 18px; flex: 0 0 auto; color: #dd8f83; background: rgba(221,143,131,.1); border-radius: 50%; font-size: 10px; }
  .resolved .state { color: #66d7b1; background: rgba(102,215,177,.1); }
  .file-main { min-width: 0; flex: 1; }
  .file-main strong, .file-main small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file-main strong { font-size: 11px; }
  .file-main small { margin-top: 2px; color: #606563; font-size: 8px; }
  .file-main .replacement { color: #5f9c88; }
  .file button { padding: 5px 8px; color: #aeb3b1; background: #202221; border: 1px solid #343735; border-radius: 5px; cursor: pointer; font-size: 9px; }
  .error { padding: 8px; color: #edaaa0; background: rgba(194,76,61,.1); border-radius: 6px; font-size: 10px; }
  footer { padding-top: 14px; border-top: 1px solid #282a29; }
  footer button { padding: 8px 12px; border-radius: 7px; cursor: pointer; font-size: 10px; font-weight: 650; }
  button:disabled { cursor: not-allowed; opacity: .45; }
  .secondary { color: #a7abaa; background: #1a1b1b; border: 1px solid #303231; }
  .primary { color: #0c1a15; background: #62d7b1; border: 1px solid #75e6c0; }
</style>
