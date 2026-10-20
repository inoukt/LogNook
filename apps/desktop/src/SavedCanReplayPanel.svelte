<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { CanCensusRow, CanIdReplayStatus, CanWorkbenchStatus } from "./types";
  import {
    createSavedCanReplayCommands,
    formatSavedCanId,
    formatSavedCanReplaySpan,
    sameReviewedCanTarget,
    parseSavedCanReplayRangeMs,
    type SavedCanReplayPreviewView,
    type SavedCanReplaySourceRef,
  } from "./savedCanReplay";

  export let sourcePath = "";
  export let census: CanCensusRow[] = [];
  export let status: CanWorkbenchStatus | null = null;
  export let captureOpen = false;
  export let workspaceBusy = false;
  export let replayStatus: CanIdReplayStatus | null = null;
  export let onStart: (
    preview: SavedCanReplayPreviewView,
    expectedCanGeneration: number,
    expectedOutputCaptureId: number,
  ) => Promise<CanIdReplayStatus> = async () => {
    throw new Error("Saved CAN replay start handler is unavailable");
  };
  export let onStop: () => Promise<void> = async () => {};
  export let onReplayStarted: (status: CanIdReplayStatus, sourceSpanNs: number) => void = () => {};

  const commands = createSavedCanReplayCommands(invoke);
  let sourceRef: SavedCanReplaySourceRef | null = null;
  let registeredPath = "";
  let sourceRevision = 0;
  let preview: SavedCanReplayPreviewView | null = null;
  let previewBusy = false;
  let error = "";
  let selectedKey = "";
  let startElapsedMs = "0";
  let endElapsedMs = "0";
  let lastTargetContext = "";
  let inspectRevision = 0;

  $: selectableRows = census.filter((row) => row.rx_count > 0);
  $: if (!selectableRows.some((row) => rowKey(row) === selectedKey)) {
    selectedKey = selectableRows[0] ? rowKey(selectableRows[0]) : "";
    if (selectableRows[0]) setRangeFromRow(selectableRows[0]);
  }
  $: selectedRow = selectableRows.find((row) => rowKey(row) === selectedKey) ?? null;
  $: parsedRange = parseSavedCanReplayRangeMs(startElapsedMs, endElapsedMs);
  $: selectionMatchesPreview = Boolean(
    preview &&
      selectedRow &&
      !("error" in parsedRange) &&
      preview.source_id === sourceRef?.source_id &&
      preview.summary.can_id === selectedRow.can_id &&
      preview.summary.extended === selectedRow.extended &&
      preview.summary.start_elapsed_ns === parsedRange.start_elapsed_ns &&
      preview.summary.end_elapsed_ns === parsedRange.end_elapsed_ns,
  );
  $: startDisabled = Boolean(
    workspaceBusy || previewBusy || !preview || !selectionMatchesPreview ||
      !preview.target.can_start || replayStatus?.state === "running",
  );
  $: targetContext = [
    preview?.target.generation ?? "",
    status?.connected ?? false,
    status?.source_id ?? "",
    preview?.target.channel ?? "",
    status?.bitrate ?? "",
    status?.listen_only ?? "",
    captureOpen,
    preview?.target.output_capture_id ?? "",
  ].join(":");
  $: if (preview && targetContext !== lastTargetContext) {
    lastTargetContext = targetContext;
    void refreshPreview(preview.preview_id);
  }
  $: if (sourcePath !== registeredPath) void registerSource(sourcePath);

  function rowKey(row: CanCensusRow): string {
    return `${row.extended ? "extended" : "standard"}:${row.can_id}`;
  }

  function setRangeFromRow(row: CanCensusRow) {
    startElapsedMs = String(Math.floor(row.first_elapsed_ns / 1_000_000));
    endElapsedMs = String(Math.ceil(row.last_elapsed_ns / 1_000_000));
  }

  async function registerSource(path: string) {
    registeredPath = path;
    const revision = ++sourceRevision;
    const oldPreview = preview;
    preview = null;
    sourceRef = null;
    error = "";
    if (oldPreview) void commands.discard(oldPreview.preview_id).catch(() => {});
    if (!path) return;
    try {
      const registered = await commands.registerSource(path);
      if (revision === sourceRevision) sourceRef = registered;
    } catch (reason) {
      if (revision === sourceRevision) error = String(reason);
    }
  }

  async function refreshPreview(previewId: string) {
    const revision = ++inspectRevision;
    try {
      const refreshed = await commands.inspect(previewId);
      if (revision === inspectRevision && preview?.preview_id === previewId) preview = refreshed;
    } catch (reason) {
      if (revision === inspectRevision && preview?.preview_id === previewId) {
        preview = null;
        error = String(reason);
      }
    }
  }

  async function prepareReplay() {
    if (!sourceRef || !selectedRow || "error" in parsedRange || workspaceBusy || previewBusy) return;
    previewBusy = true;
    error = "";
    try {
      preview = await commands.prepare(sourceRef.source_id, {
        can_id: selectedRow.can_id,
        extended: selectedRow.extended,
        ...parsedRange,
      });
    } catch (reason) {
      preview = null;
      error = String(reason);
    } finally {
      previewBusy = false;
    }
  }

  async function startReplay() {
    if (!preview || startDisabled) return;
    const reviewedTarget = preview.target;
    previewBusy = true;
    error = "";
    try {
      const current = await commands.inspect(preview.preview_id);
      preview = current;
      if (!current.target.can_start) return;
      if (!sameReviewedCanTarget(reviewedTarget, current.target)) {
        error = "The target or output capture changed. Review the updated target and click Start again.";
        return;
      }
      if (current.target.output_capture_id === null) return;
      const started = await onStart(current, current.target.generation, current.target.output_capture_id);
      onReplayStarted(started, current.summary.source_span_ns);
    } catch (reason) {
      error = String(reason);
    } finally {
      previewBusy = false;
    }
  }

  async function stopReplay() {
    error = "";
    try {
      await onStop();
    } catch (reason) {
      error = String(reason);
    }
  }
</script>

<section class="saved-can-replay" aria-labelledby="saved-can-replay-title">
  <h3 id="saved-can-replay-title">Saved CAN TX replay</h3>
  {#if sourcePath}
    <p class="saved-can-muted" title={sourcePath}>{sourceRef?.name ?? sourcePath}</p>
  {:else}
    <p class="saved-can-muted">Open a saved .candump.csv capture to prepare a replay.</p>
  {/if}

  {#if selectableRows.length}
    <div class="saved-can-form">
      <label>
        CAN ID and format
        <select class="text-input" bind:value={selectedKey} onchange={() => {
          const row = selectableRows.find((item) => rowKey(item) === selectedKey);
          if (row) setRangeFromRow(row);
        }} disabled={workspaceBusy || previewBusy}>
          {#each selectableRows as row (rowKey(row))}
            <option value={rowKey(row)}>{formatSavedCanId(row.can_id, row.extended)} · {row.rx_count.toLocaleString()} RX</option>
          {/each}
        </select>
      </label>
      <label>
        Start (elapsed ms)
        <input class="text-input" type="number" min="0" step="1" bind:value={startElapsedMs} disabled={workspaceBusy || previewBusy} />
      </label>
      <label>
        End (elapsed ms)
        <input class="text-input" type="number" min="0" step="1" bind:value={endElapsedMs} disabled={workspaceBusy || previewBusy} />
      </label>
      <button class="button secondary" onclick={prepareReplay} disabled={!sourceRef || !selectedRow || "error" in parsedRange || workspaceBusy || previewBusy}>
        {previewBusy ? "Preparing…" : "Prepare TX replay"}
      </button>
    </div>
    {#if "error" in parsedRange}<p class="saved-can-error" role="status">{parsedRange.error}</p>{/if}
  {:else}
    <p class="saved-can-muted">No received CAN IDs are available in this capture.</p>
  {/if}

  {#if error}<p class="saved-can-error" role="alert">{error}</p>{/if}

  {#if preview}
    <div class="saved-can-preview" aria-label="Prepared saved CAN replay preview">
      <h4>Prepared sequence</h4>
      <dl>
        <dt>Source</dt><dd>{preview.source_name}</dd>
        <dt>CAN ID</dt><dd>{formatSavedCanId(preview.summary.can_id, preview.summary.extended)}</dd>
        <dt>Source rows</dt><dd>{preview.summary.first_row.toLocaleString()}–{preview.summary.last_row.toLocaleString()}</dd>
        <dt>Source elapsed</dt><dd>{(preview.summary.first_elapsed_ns / 1_000_000).toFixed(3)}–{(preview.summary.last_elapsed_ns / 1_000_000).toFixed(3)} ms</dd>
        <dt>Frames to transmit</dt><dd>{preview.summary.matching_rx_count.toLocaleString()} unflagged RX</dd>
        <dt>Excluded</dt><dd>{preview.summary.excluded_tx_count.toLocaleString()} TX · {preview.summary.excluded_flagged_count.toLocaleString()} flagged RX</dd>
        <dt>Captured span</dt><dd>{formatSavedCanReplaySpan(preview.summary.source_span_ns)}</dd>
        <dt>Paced duration</dt><dd>{formatSavedCanReplaySpan(preview.summary.paced_duration_ns)}</dd>
        <dt>Source bitrate</dt><dd>{preview.source_bitrate ? `${preview.source_bitrate.toLocaleString()} bit/s` : "Unknown"}</dd>
        <dt>Source SHA-256</dt><dd><code>{preview.summary.source_sha256}</code></dd>
      </dl>
      <p class="saved-can-target" role="status">
        {#if preview.target.connected}
          Target: {preview.target.source_id} · channel {preview.target.channel ?? "unknown"} · {preview.target.bitrate?.toLocaleString()} bit/s · {preview.target.listen_only ? "listen-only" : "active"}
        {:else}
          Target: not connected
        {/if}
      </p>
      {#if preview.target.start_block_reason}<p class="saved-can-muted">{preview.target.start_block_reason}</p>{/if}
      <button class="button danger" onclick={startReplay} disabled={startDisabled}>Start TX replay · once</button>
      <small class="saved-can-muted">Preview expires {new Date(preview.expires_at_unix_ms).toLocaleTimeString()}.</small>
    </div>
  {/if}

  {#if replayStatus}
    <div class="saved-can-progress" role="status" aria-live="polite">
      <p>{replayStatus.state === "running" ? "Replay in progress" : `Replay ${replayStatus.state}`} · {replayStatus.sent_frames.toLocaleString()} / {replayStatus.total_frames.toLocaleString()} submitted</p>
      <progress aria-label="Submitted CAN replay frames" max={replayStatus.total_frames || 1} value={replayStatus.sent_frames}></progress>
      {#if replayStatus.error}<p class="saved-can-error">{replayStatus.error}</p>{/if}
      {#if replayStatus.state === "running"}<button class="button danger" onclick={stopReplay} disabled={workspaceBusy}>Stop replay</button>{/if}
    </div>
  {/if}
</section>

<style>
  .saved-can-replay { display: grid; gap: 0.65rem; margin: 0.8rem 0; padding: 0.8rem; border: 1px solid var(--border, #444); border-radius: 0.6rem; }
  .saved-can-replay h3, .saved-can-replay h4, .saved-can-replay p { margin: 0; }
  .saved-can-form { display: grid; grid-template-columns: minmax(12rem, 2fr) repeat(2, minmax(8rem, 1fr)) auto; gap: 0.6rem; align-items: end; }
  .saved-can-form label { display: grid; gap: 0.25rem; font-size: 0.85rem; }
  .saved-can-preview dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 0.3rem 0.7rem; margin: 0.5rem 0; }
  .saved-can-preview dt { color: var(--muted, #aaa); }
  .saved-can-preview dd { margin: 0; overflow-wrap: anywhere; }
  .saved-can-preview code { font-size: 0.78rem; }
  .saved-can-muted { color: var(--muted, #aaa); font-size: 0.86rem; }
  .saved-can-error { color: var(--danger, #f77); }
  .saved-can-progress { display: grid; gap: 0.5rem; }
  .saved-can-progress progress { width: 100%; }
  @media (max-width: 800px) { .saved-can-form { grid-template-columns: 1fr; } }
</style>
