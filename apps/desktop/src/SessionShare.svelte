<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let { path, onClose, onSaved }: {
    path: string;
    onClose: () => void;
    onSaved: (message: string) => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let busy = $state(false);
  let error = $state("");
  let link = $state("");
  let expiry = $state("");
  let copied = $state(false);
  let previousFocus: HTMLElement | null = null;

  $effect(() => {
    if (path) {
      error = "";
      link = "";
      expiry = "";
      copied = false;
      previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      if (!dialog.open) dialog.showModal();
    } else if (dialog.open) dialog.close();
  });

  function close() {
    if (!busy) dialog.close();
  }

  async function upload() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const result = await invoke<{ url: string; expires_in: string; filename: string }>("share_recording_link", { path });
      link = result.url;
      expiry = result.expires_in;
      onSaved("Temporary session link ready");
    } catch (cause) {
      error = String(cause);
    } finally { busy = false; }
  }

  async function saveCsv() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const destination = await invoke<string | null>("share_recording_csv", { path });
      if (destination) onSaved(`Shareable CSV saved · ${destination}`);
    } catch (cause) {
      error = String(cause);
    } finally { busy = false; }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(link);
      copied = true;
      error = "";
    } catch { error = "Clipboard unavailable. Select and copy the link below."; }
  }
</script>

<dialog class="share-dialog" bind:this={dialog} aria-labelledby="share-title"
  oncancel={(event) => { if (busy) event.preventDefault(); }}
  onclose={() => { onClose(); previousFocus?.focus(); }}>
  <header><h2 id="share-title">Share session</h2><button class="button secondary" onclick={close} disabled={busy}>Close</button></header>
  <strong class="filename">{path.split(/[\\/]/).at(-1)}</strong>
  <p>Upload the complete <code>.lognook</code> session to Catbox. Anyone with the link can download it, including its timestamps, ECU identities and raw measurement data.</p>
  <p class="limits">Up to 200 MB · No scheduled expiry. Service retention is not guaranteed; keep your own copy. Anonymous uploads cannot be deleted through LogNook.</p>
  {#if link}
    <label class="link-label">Download link<input class="text-input" aria-label="Session download link" value={link} readonly onclick={(event) => event.currentTarget.select()} /></label>
    <div class="link-actions"><span>{expiry}</span><button class="button secondary" onclick={copy}>{copied ? "Copied" : "Copy link"}</button></div>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <footer>
    <button class="button primary" onclick={upload} disabled={busy || !!link}>{busy ? "Working…" : "Upload public link"}</button>
    <button class="button secondary" onclick={saveCsv} disabled={busy}>Save CSV locally</button>
  </footer>
</dialog>

<style>
  .share-dialog { box-sizing:border-box; width:min(470px,calc(100vw - 24px)); max-height:calc(100dvh - 24px); overflow:auto; margin:auto; padding:16px; border:1px solid var(--border); border-radius:10px; color:var(--text); background:var(--surface); }
  .share-dialog::backdrop { background:#05090dcc; }
  header,footer,.link-actions { display:flex; align-items:center; justify-content:space-between; gap:8px; }
  header h2 { font-size:16px; }
  .filename { display:block; margin-top:12px; overflow-wrap:anywhere; }
  p,.link-actions { font-size:12px; line-height:1.5; color:var(--text-secondary); }
  .limits { color:var(--text-bright); }
  .link-label { display:grid; gap:5px; font-size:12px; }
  .link-label input { box-sizing:border-box; width:100%; }
  .link-actions { margin:6px 0 12px; }
  footer { flex-wrap:wrap; margin-top:12px; }
  [role="alert"] { color:var(--danger,#ff8993); overflow-wrap:anywhere; }
</style>
