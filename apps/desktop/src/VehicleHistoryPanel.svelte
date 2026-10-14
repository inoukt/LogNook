<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { groupVehicleHistory, recordingFromRow, type BrowserRecording, type BrowserRecordingRow } from "./sessionBrowser";
  import DiagnosticReportContent from "./DiagnosticReportContent.svelte";
  import {
    associateVehicleDiagnosticReport,
    getVehicleDiagnosticReport,
    listVehicleDiagnosticHistory,
    startVehicleHistoryImport,
    cancelVehicleHistoryImport,
    vehicleHistoryImportStatus,
    type DiagnosticHistoryRow,
    type DiagnosticReportDetail,
    type HistoryIndexStatus,
  } from "./vehicleDiagnostics";
  import { vinDecodeLabels, type VehicleProfile } from "./vehicleProfiles";

  let { open = false, profiles = [], onOpenRecording = (_path: string) => {} }: {
    open?: boolean;
    profiles?: VehicleProfile[];
    onOpenRecording?: (path: string) => void | Promise<void>;
  } = $props();
  let rows = $state<DiagnosticHistoryRow[]>([]);
  let recordings = $state<BrowserRecording[]>([]);
  let nextCursor = $state<string | null>(null);
  let detail = $state<DiagnosticReportDetail | null>(null);
  let activeTab = $state<"dtcs" | "identity" | "mode06" | "raw">("identity");
  let importStatus = $state<HistoryIndexStatus | null>(null);
  let selectedProfileId = $state("");
  let associationNote = $state("");
  let scopeFilter = $state<"" | "live" | "simulator" | "can_only">("");
  let vinDigestFilter = $state("");
  let ecuFilter = $state("");
  let fromDate = $state("");
  let toDate = $state("");
  let busy = $state(false);
  let error = $state("");
  let wasOpen = false;
  let historyGroups = $derived(groupVehicleHistory(rows, recordings));

  $effect(() => {
    const justOpened = open && !wasOpen;
    wasOpen = open;
    if (justOpened) queueMicrotask(() => { if (open) void refresh(); });
  });

  $effect(() => {
    if (!open || importStatus?.state !== "indexing") return;
    const timer = setInterval(() => void refreshImportStatus(), 750);
    return () => clearInterval(timer);
  });

  async function refreshImportStatus() {
    try { importStatus = await vehicleHistoryImportStatus(); }
    catch (cause) { error = String(cause); }
  }

  async function refresh(append = false) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const [page, status, recordingRows] = await Promise.all([
        listVehicleDiagnosticHistory(historyFilter(append ? nextCursor : null)),
        vehicleHistoryImportStatus(),
        invoke<BrowserRecordingRow[]>("list_recordings"),
      ]);
      rows = append ? [...rows, ...page.rows] : page.rows;
      recordings = recordingRows.map(recordingFromRow);
      nextCursor = page.next_cursor;
      importStatus = status;
    } catch (cause) { error = String(cause); }
    finally { busy = false; }
  }

  function historyFilter(cursor: string | null) {
    const from = fromDate ? Date.parse(fromDate) : null;
    const to = toDate ? Date.parse(toDate) : null;
    if ((from !== null && !Number.isFinite(from)) || (to !== null && !Number.isFinite(to))) {
      throw new Error("Enter a valid history date range.");
    }
    if (from !== null && to !== null && from > to) throw new Error("The start date must be before the end date.");
    return {
      scope: scopeFilter || null,
      vin_digest: vinDigestFilter.trim() || null,
      ecu_id: ecuFilter.trim() || null,
      from_unix_ms: from,
      to_unix_ms: to,
      page_size: 50,
      cursor,
    };
  }

  async function openReport(row: DiagnosticHistoryRow) {
    busy = true;
    error = "";
    try { detail = await getVehicleDiagnosticReport(row.report_id); activeTab = "identity"; }
    catch (cause) { error = String(cause); }
    finally { busy = false; }
  }

  async function startImport() {
    error = "";
    try { importStatus = await startVehicleHistoryImport(); }
    catch (cause) { error = String(cause); }
  }

  async function stopImport() {
    error = "";
    try { importStatus = await cancelVehicleHistoryImport(); }
    catch (cause) { error = String(cause); }
  }

  async function associateReport() {
    if (!detail || !selectedProfileId) return;
    const profile = profiles.find((item) => item.id === selectedProfileId);
    if (!profile?.vin_digest) return;
    busy = true;
    error = "";
    try {
      await associateVehicleDiagnosticReport(detail.index.report_id, profile.scope, profile.vin_digest, associationNote.trim() || null);
      detail = await getVehicleDiagnosticReport(detail.index.report_id);
      associationNote = "";
    } catch (cause) { error = String(cause); }
    finally { busy = false; }
  }

  function timeLabel(value: number): string {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? "Unknown date" : date.toLocaleString();
  }
</script>

<section class="history-panel" aria-label="Vehicle diagnostic history">
  <header class="history-header">
    <div><p class="eyebrow">LOCAL DIAGNOSTIC HISTORY</p><h2>Vehicle history</h2></div>
    <button class="button secondary" type="button" onclick={() => void refresh()} disabled={busy}>Refresh</button>
  </header>
  <div class="history-import" role="status">
    <span>{importStatus ? `${importStatus.state.replaceAll("_", " ")} · ${importStatus.indexed_reports.toLocaleString()} reports · ${importStatus.scanned_bytes.toLocaleString()}/${importStatus.total_bytes.toLocaleString()} bytes` : "History index status unavailable"}</span>
    {#if importStatus?.state === "indexing"}
      <button class="button secondary" type="button" onclick={() => void stopImport()}>Stop import</button>
    {:else}
      <button class="button secondary" type="button" onclick={() => void startImport()} disabled={busy}>Import older diagnostic reports</button>
    {/if}
  </div>
  <form class="history-filters" onsubmit={(event) => { event.preventDefault(); void refresh(); }}>
    <label>Scope<select bind:value={scopeFilter}><option value="">All</option><option value="live">Live</option><option value="simulator">Simulator</option><option value="can_only">CAN-only</option></select></label>
    <label>VIN fingerprint<input bind:value={vinDigestFilter} placeholder="vin:v1:sha256:…" /></label>
    <label>ECU address<input bind:value={ecuFilter} placeholder="7E8" /></label>
    <label>From<input type="datetime-local" bind:value={fromDate} /></label>
    <label>To<input type="datetime-local" bind:value={toDate} /></label>
    <button class="button secondary" type="submit" disabled={busy}>Apply filters</button>
  </form>
  {#if importStatus?.error}<p class="history-error" role="alert">{importStatus.error}</p>{/if}
  {#if error}<p class="history-error" role="alert">{error}</p>{/if}

  {#if detail}
    <section class="history-detail" aria-label={`Diagnostic report ${detail.index.report_id}`}>
      <header><div><p class="eyebrow">{timeLabel(detail.index.started_at_unix_ms)}</p><h3>{detail.index.masked_vin ?? "VIN unavailable"} · {detail.index.identity_source.replaceAll("_", " ")}</h3></div><button class="button secondary" type="button" onclick={() => detail = null}>Back to reports</button></header>
      <p class="evidence-state">Raw evidence: {detail.evidence_status.replaceAll("_", " ")}</p>
      {#if detail.context_snapshot && detail.report}
        <div class="tab-row" role="tablist" aria-label="Report results">
          {#each ["identity", "dtcs", "mode06", "raw"] as tab}
            <button class:active={activeTab === tab} type="button" role="tab" aria-selected={activeTab === tab} onclick={() => activeTab = tab as typeof activeTab}>{tab === "mode06" ? "Mode 06" : tab.toUpperCase()}</button>
          {/each}
        </div>
        <DiagnosticReportContent report={detail.report} {activeTab} identitySnapshot={detail.context_snapshot} />
      {:else if detail.context_snapshot}
        <div class="frozen-context">
          <strong>Frozen report context</strong>
          <p>{detail.context_snapshot.full_vin ?? detail.context_snapshot.masked_vin ?? "VIN unavailable"} · {detail.context_snapshot.scope.replaceAll("_", " ")} · {detail.context_snapshot.identity_source.replaceAll("_", " ")}</p>
          {#if detail.context_snapshot.decoded_snapshot}
            {@const labels = vinDecodeLabels(detail.context_snapshot.decoded_snapshot)}
            <p>Offline VIN decode · {labels.manufacturer} · {labels.year} · {labels.checkDigit}</p>
            {#if detail.context_snapshot.decoded_snapshot.wmi_assignment}<small>WMI source {detail.context_snapshot.decoded_snapshot.wmi_assignment.source_id} · revision {detail.context_snapshot.decoded_snapshot.wmi_assignment.source_revision} · decoder {detail.context_snapshot.decoded_snapshot.decoder_revision}</small>{/if}
          {/if}
          {#if detail.context_snapshot.owner_details.length}<details><summary>Owner-entered details</summary><ul>{#each detail.context_snapshot.owner_details as item, index (`${item.field}:${item.entered_at_unix_ms}:${index}`)}<li>{item.field.replaceAll("_", " ")}: {item.value}{#if item.note} · {item.note}{/if}</li>{/each}</ul></details>{/if}
          <p>The original report bytes are unavailable or changed; this is indexed metadata, not recovered raw evidence.</p>
        </div>
      {/if}
      {#if detail.index.vin_digest && profiles.some((profile) => profile.vin_digest === detail?.index.vin_digest && profile.scope === detail?.index.scope)}
        <p class="evidence-state">This report already matches a saved vehicle profile.</p>
      {:else if profiles.some((profile) => profile.vin_digest)}
        <form class="associate-form" onsubmit={(event) => { event.preventDefault(); void associateReport(); }}>
          <label for="history-profile">Add a manual vehicle association</label>
          <select id="history-profile" bind:value={selectedProfileId} disabled={busy}>
            <option value="">Choose a saved profile</option>
            {#each profiles.filter((profile) => profile.vin_digest) as profile (profile.id)}<option value={profile.id}>{profile.name} · {profile.masked_vin ?? profile.scope}</option>{/each}
          </select>
          <input bind:value={associationNote} maxlength="500" placeholder="Optional note" aria-label="Association note" disabled={busy} />
          <button class="button secondary" type="submit" disabled={busy || !selectedProfileId}>Associate</button>
          <small>Association is an append-only owner annotation; the captured VIN and conflict state stay unchanged.</small>
        </form>
      {/if}
      {#if detail.manual_associations?.length}<details><summary>Manual associations ({detail.manual_associations.length})</summary><ul>{#each detail.manual_associations as item, index (`${item.associated_at_unix_ms}:${index}`)}<li>{item.scope} · {item.vin_digest}{#if item.note} · {item.note}{/if}</li>{/each}</ul></details>{/if}
    </section>
  {:else}
    {#if historyGroups.length === 0 && !busy}<p class="history-empty">No vehicle reports or recordings are indexed yet. Run a read-only identity or diagnostics report, or import older reports.</p>{/if}
    {#each historyGroups as group (group.id)}
      <section class="history-group" aria-label={`Vehicle history ${group.masked_vin ?? group.identity_source}`}>
        <header><strong>{group.masked_vin ?? (group.identity_source === "conflict" ? "Conflicting VIN evidence" : "Vehicle not identified")}</strong><small>{group.scope.replaceAll("_", " ")} · {group.reportRows.length} report{group.reportRows.length === 1 ? "" : "s"} · {group.recordings.length} recording{group.recordings.length === 1 ? "" : "s"}</small></header>
        {#if group.reportRows.length}
          <ul class="report-list">
            {#each group.reportRows as row (row.report_id)}
              <li><button class="report-row" type="button" onclick={() => void openReport(row)} disabled={busy}>
                <span><strong>{timeLabel(row.started_at_unix_ms)} · {row.identity_source.replaceAll("_", " ")}</strong><small>{row.scope.replaceAll("_", " ")}</small></span>
                <span class="report-meta">{row.ecu_ids.length} ECU · {row.dtc_counts.stored ?? "?"} stored DTC · {row.evidence_status}</span>
              </button></li>
            {/each}
          </ul>
        {/if}
        {#if group.recordings.length}
          <ul class="recording-list">
            {#each group.recordings as recording (recording.path)}
              <li><span><strong>{recording.name}</strong><small>{timeLabel(recording.modified_unix_ms)} · {recording.sample_count.toLocaleString()} samples{#if recording.vehicle_context?.has_full_vin} · native session contains full VIN{/if}</small></span><button class="button secondary" type="button" onclick={() => void onOpenRecording(recording.path)} disabled={busy || recording.sample_count === 0}>Open recording</button></li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
    {#if nextCursor}<button class="button secondary" type="button" onclick={() => void refresh(true)} disabled={busy}>Load more</button>{/if}
  {/if}
</section>

<style>
  .history-panel { display: grid; gap: 12px; min-width: 0; }
  .history-header, .history-header > div, .history-detail > header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .history-header h2, .history-detail h3 { margin: 0; font-size: 16px; }
  .history-import { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; color: var(--text-muted); font-size: 11px; }
  .history-filters { display: grid; grid-template-columns: repeat(auto-fit, minmax(130px, 1fr)); align-items: end; gap: 7px; }
  .history-filters label { display: grid; gap: 4px; color: var(--text-muted); font-size: 10px; }
  .history-filters input, .history-filters select { min-width: 0; width: 100%; border: 1px solid var(--border); border-radius: 6px; padding: 6px; color: inherit; background: var(--surface); font: inherit; }
  .report-list { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .history-group { display: grid; gap: 8px; padding: 9px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); }
  .history-group > header { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
  .history-group > header small, .recording-list small { color: var(--text-muted); font-size: 10px; }
  .recording-list { display: grid; gap: 5px; margin: 0; padding: 0; list-style: none; }
  .recording-list li { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 7px; border-top: 1px solid var(--border-soft); }
  .recording-list li > span { display: grid; gap: 3px; min-width: 0; }
  .report-row { display: flex; align-items: center; justify-content: space-between; width: 100%; gap: 12px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); color: inherit; text-align: left; cursor: pointer; }
  .report-row > span:first-child { display: grid; gap: 3px; min-width: 0; }
  .report-row small, .report-meta, .evidence-state, .history-empty, .history-error, .associate-form small { color: var(--text-muted); font-size: 10px; }
  .report-meta { flex: 0 0 auto; }
  .history-detail { display: grid; gap: 10px; min-width: 0; }
  .evidence-state { margin: 0; }
  .tab-row { display: flex; gap: 5px; }
  .tab-row button { border: 1px solid var(--border); border-radius: 6px; padding: 5px 8px; color: inherit; background: transparent; cursor: pointer; }
  .tab-row button.active { background: var(--surface-top); }
  .associate-form { display: grid; grid-template-columns: minmax(120px, 1fr) minmax(120px, 1fr) auto; align-items: end; gap: 7px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; }
  .associate-form label, .associate-form small { grid-column: 1 / -1; }
  .associate-form select, .associate-form input { min-width: 0; border: 1px solid var(--border); border-radius: 6px; padding: 6px; color: inherit; background: var(--surface); }
  .frozen-context { padding: 10px; border: 1px solid var(--border); border-radius: 8px; }
  .frozen-context p { margin: 5px 0 0; color: var(--text-muted); font-size: 11px; }
  @media (max-width: 650px) { .report-row { align-items: flex-start; flex-direction: column; } .report-meta { white-space: normal; } .associate-form { grid-template-columns: 1fr; } }
</style>
