<script lang="ts">
  interface RecoverySnapshot {
    snapshotId: string;
    projectId: string;
    checksum: string;
    updatedAtMs: number;
    bytesWritten: number;
    originalPath?: string;
    reason?: string;
    isValid: boolean;
    error?: string;
  }

  interface Props {
    open: boolean;
    snapshots: RecoverySnapshot[];
    busy?: boolean;
    onRecover: (snapshotId: string) => void | Promise<void>;
    onDiscard: (snapshotId: string) => void | Promise<void>;
    onClose: () => void;
  }

  let { open, snapshots, busy = false, onRecover, onDiscard, onClose }: Props = $props();
  let selectedId = $state<string | null>(null);
  let sorted = $derived([...snapshots].sort((a, b) => b.updatedAtMs - a.updatedAtMs));
  let selected = $derived(sorted.find((item) => item.snapshotId === selectedId) ?? sorted[0] ?? null);

  $effect(() => {
    if (open && !selectedId && sorted[0]) selectedId = sorted[0].snapshotId;
  });

  function fileName(path?: string) {
    if (!path) return "Kaydedilmemiş proje";
    return path.split(/[/\\]/).pop() || path;
  }

  function formatBytes(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (open && !busy && event.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="backdrop">
    <div class="dialog" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="recovery-title">
      <div class="icon">↺</div>
      <div class="intro">
        <span class="eyebrow">Crash recovery</span>
        <h2 id="recovery-title">Kurtarılabilir çalışma bulundu</h2>
        <p>Uygulama kapanmadan önce alınan otomatik kayıtlar doğrulandı. Devam etmek istediğiniz sürümü seçin.</p>
      </div>

      <div class="snapshots">
        {#each sorted as snapshot}
          <button
            class:selected={selected?.snapshotId === snapshot.snapshotId}
            class:invalid={!snapshot.isValid}
            onclick={() => (selectedId = snapshot.snapshotId)}
          >
            <span class="radio"></span>
            <span class="snapshot-main">
              <strong>{fileName(snapshot.originalPath)}</strong>
              <small>{new Date(snapshot.updatedAtMs).toLocaleString("tr-TR")} · {formatBytes(snapshot.bytesWritten)}</small>
              {#if snapshot.reason}<small>{snapshot.reason}</small>{/if}
            </span>
            <span class="validity">{snapshot.isValid ? "Doğrulandı" : "Hasarlı"}</span>
          </button>
        {/each}
      </div>

      {#if selected?.error}
        <div class="error" role="alert">{selected.error}</div>
      {/if}

      <footer>
        <button class="discard" disabled={busy || !selected} onclick={() => selected && onDiscard(selected.snapshotId)}>Bu kaydı sil</button>
        <div class="actions">
          <button class="secondary" disabled={busy} onclick={onClose}>Atla</button>
          <button class="primary" disabled={busy || !selected?.isValid} onclick={() => selected && onRecover(selected.snapshotId)}>
            {busy ? "Açılıyor…" : "Çalışmayı kurtar"}
          </button>
        </div>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1100; display: grid; place-items: center; padding: 24px; background: rgba(0,0,0,.78); backdrop-filter: blur(9px); }
  .dialog { width: min(600px, calc(100vw - 32px)); padding: 24px; color: #e7eae9; background: #121313; border: 1px solid #2c2e2d; border-radius: 14px; box-shadow: 0 30px 100px rgba(0,0,0,.6); }
  .icon { float: left; display: grid; place-items: center; width: 42px; height: 42px; margin-right: 13px; color: #6be0b9; background: rgba(107,224,185,.1); border: 1px solid rgba(107,224,185,.25); border-radius: 10px; font-size: 23px; }
  .intro { min-height: 48px; }
  .eyebrow { color: #6be0b9; font-size: 9px; font-weight: 800; letter-spacing: .13em; text-transform: uppercase; }
  h2 { margin: 3px 0 6px; font-size: 18px; }
  p { margin: 0; color: #7c817f; font-size: 11px; line-height: 1.5; }
  .snapshots { display: grid; gap: 7px; max-height: 250px; margin: 20px 0 14px; overflow: auto; }
  .snapshots button { display: flex; align-items: center; gap: 11px; width: 100%; padding: 11px; color: #aaaead; text-align: left; background: #181919; border: 1px solid #292b2a; border-radius: 8px; cursor: pointer; }
  .snapshots button.selected { color: #e6ebe9; border-color: #55bd9b; background: rgba(85,189,155,.08); }
  .snapshots button.invalid { opacity: .55; }
  .radio { width: 9px; height: 9px; flex: 0 0 auto; border: 1px solid #565b59; border-radius: 50%; }
  .selected .radio { background: #62d7b1; border-color: #62d7b1; box-shadow: 0 0 0 3px rgba(98,215,177,.12); }
  .snapshot-main { min-width: 0; flex: 1; }
  .snapshot-main strong, .snapshot-main small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .snapshot-main strong { font-size: 12px; }
  .snapshot-main small { margin-top: 3px; color: #686d6b; font-size: 9px; }
  .validity { color: #579f87; font-size: 9px; }
  .error { padding: 8px 10px; color: #eeaaa0; background: rgba(194,76,61,.1); border-radius: 6px; font-size: 10px; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-top: 15px; border-top: 1px solid #272928; }
  .actions { display: flex; gap: 8px; }
  footer button { padding: 8px 12px; border-radius: 7px; font: 600 10px/1 inherit; cursor: pointer; }
  footer button:disabled { cursor: not-allowed; opacity: .45; }
  .discard { padding-left: 0; color: #a26c64; background: transparent; border: 0; }
  .secondary { color: #a8acab; background: #1a1b1b; border: 1px solid #303231; }
  .primary { color: #0c1a15; background: #62d7b1; border: 1px solid #75e6c0; }
</style>
