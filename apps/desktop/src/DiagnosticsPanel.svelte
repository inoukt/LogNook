<script lang="ts">
  import { untrack } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { filterDiagnostics, formatDiagnostic, mergeDiagnostics, selectedDiagnosticsRecording } from "./diagnostics";
  import type { DiagnosticRecord } from "./types";

  let { open = false, liveRecords = [], listenerError = "", onClose, onChooseRecording, onExport }: {
    open?: boolean;
    liveRecords?: DiagnosticRecord[];
    listenerError?: string;
    onClose: () => void;
    onChooseRecording: () => Promise<string | null>;
    onExport: (recordingPath: string | null) => Promise<string | null>;
  } = $props();
  let records = $state<DiagnosticRecord[]>([]);
  let adapter = $state("");
  let operation = $state("");
  let errorsOnly = $state(false);
  let recordingPath = $state<string | null>(null);
  let actionBusy = $state(false);
  let loading = $state(false);
  let error = $state("");
  let exportPath = $state("");
  let wasOpen = false;
  let dialog: HTMLDialogElement;

  $effect(() => {
    const incoming = liveRecords;
    records = untrack(() => mergeDiagnostics(records, incoming));
  });
  $effect(() => {
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
    const justOpened = open && !wasOpen;
    wasOpen = open;
    if (justOpened) void refresh();
  });
  const adapters = $derived([...new Set(records.flatMap(record => record.adapter_id ? [record.adapter_id] : []))].sort());
  const operations = $derived([...new Set(records.map(record => record.operation))].sort());
  const visible = $derived(filterDiagnostics(records, { adapter, operation, errorsOnly }).slice().reverse());

  async function refresh() {
    if (loading) return;
    if (!isTauri()) { error = "Open LogNook as a desktop app to load diagnostics."; return; }
    loading = true;
    error = "";
    try {
      const snapshot = await invoke<DiagnosticRecord[]>("diagnostic_snapshot");
      records = mergeDiagnostics(records, snapshot);
    } catch { error = "Diagnostics could not be loaded. Try refreshing."; }
    finally { loading = false; }
  }
  async function chooseRecording() {
    actionBusy = true;
    error = "";
    try { recordingPath = selectedDiagnosticsRecording(recordingPath, await onChooseRecording()); }
    catch { error = "The recording could not be selected."; }
    finally { actionBusy = false; }
  }
  async function exportBundle() {
    actionBusy = true;
    error = "";
    try {
      const path = await onExport(recordingPath);
      if (path !== null) exportPath = path;
    } catch { error = "The support bundle could not be exported. Choose a new ZIP filename and try again."; }
    finally { actionBusy = false; }
  }
</script>

<dialog bind:this={dialog} class="diagnostics-dialog" aria-labelledby="diagnostics-title" oncancel={onClose} onclose={onClose}>
  <header>
    <div><p class="eyebrow">ADAPTER OPERATIONS</p><h2 id="diagnostics-title">Diagnostics</h2></div>
    <button class="button secondary" type="button" onclick={onClose}>Close</button>
  </header>
  <p class="hint">Recent progress, PID attempts, attributable errors, first responses, and delivery rates. Attempts count requested polls; grouped failures remain in the operation log. Support bundles include diagnostics and existing OEM research evidence.</p>
  <div class="controls">
    <label>Adapter <select class="text-input" aria-label="Adapter filter" bind:value={adapter}><option value="">All adapters</option>{#each adapters as id}<option value={id}>{id}</option>{/each}</select></label>
    <label>Operation <select class="text-input" aria-label="Operation filter" bind:value={operation}><option value="">All operations</option>{#each operations as name}<option value={name}>{name.replaceAll("_", " ")}</option>{/each}</select></label>
    <label><input type="checkbox" bind:checked={errorsOnly} /> Errors only</label>
    <button class="button secondary" type="button" onclick={refresh} disabled={loading}>{loading ? "Loading…" : "Refresh"}</button>
  </div>
  {#if error || listenerError}<p role="alert">{error || listenerError}</p>{/if}
  <div class="records" role="region" aria-label="Recent diagnostic records">
    <table>
      <thead><tr><th>Time / adapter</th><th>Operation / stage</th><th>Status</th><th>Duration</th><th>Progress / error</th></tr></thead>
      <tbody>
        {#each visible as record (record.sequence_id)}
          {@const row = formatDiagnostic(record)}
          <tr class:failed={record.phase === "failed"}>
            <td>{row.time}<small>{row.adapter}</small></td>
            <td>{row.operation}<small>{row.stage} · {row.operationId}</small></td>
            <td>{row.status}</td><td>{row.duration}</td>
            <td>{#if row.signals}<small>Signals: {row.signals}</small>{/if}{#each row.summaries as summary}<small>{summary}</small>{/each}{#if row.detail}<span>{row.detail}</span>{/if}{#if row.omitted}<small>{row.omitted} events omitted by diagnostics storage</small>{/if}</td>
          </tr>
        {:else}<tr><td colspan="5">{loading ? "Loading diagnostics…" : "No matching diagnostics"}</td></tr>{/each}
      </tbody>
    </table>
  </div>
  <footer>
    <p class="hint">{visible.length} of {records.length} recent events (up to 500). Storage failures appear as errors.</p>
    <p class="hint">Optional recording: {recordingPath ?? "None"}. A selected recording includes its recorded measurements in the ZIP.</p>
    <div class="controls">
      <button class="button secondary" type="button" onclick={chooseRecording} disabled={actionBusy}>Choose .lognook recording…</button>
      {#if recordingPath}<button class="button secondary" type="button" onclick={() => recordingPath = null} disabled={actionBusy}>Remove recording</button>{/if}
      <button class="button" type="button" onclick={exportBundle} disabled={actionBusy}>{actionBusy ? "Working…" : "Export support bundle"}</button>
    </div>
    {#if exportPath}<p class="hint" role="status">Support bundle saved: {exportPath}</p>{/if}
  </footer>
</dialog>

<style>
  .diagnostics-dialog { width: min(1100px, calc(100vw - 32px)); max-height: calc(100vh - 32px); overflow: auto; padding: 18px; color: var(--text); background: var(--surface); border: 1px solid var(--border); border-radius: 10px; box-shadow: 0 16px 48px #0009; }
  .diagnostics-dialog::backdrop { background: #05090dcc; }
  header, .controls { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  header { justify-content: space-between; }
  h2 { margin: 0; color: var(--text-bright); }
  label { display: flex; align-items: center; gap: 6px; }
  input { accent-color: var(--accent); }
  .hint, small { color: var(--text-secondary); font-size: 12px; }
  .records { max-height: 48vh; overflow: auto; margin-top: 14px; border: 1px solid var(--border); }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  th { position: sticky; top: 0; background: var(--surface-deep); text-align: left; }
  th, td { padding: 9px; border-bottom: 1px solid var(--border); vertical-align: top; overflow-wrap: anywhere; }
  small { display: block; margin-top: 4px; }
  .failed td { color: var(--danger, #f87171); }
</style>
