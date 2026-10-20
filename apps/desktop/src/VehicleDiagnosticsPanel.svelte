<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    availableMode06TestIds, clearConfirmationMatchesConnection, diagnosticOutcomeLabel, diagnosticReadLabel, formatHexByte,
    groupDiagnosticReadsByEcu, mode06MonitorSummary, parseMode06TestIds,
    supportsVehicleDiagnostics,
    type DiagnosticCapabilities, type DiagnosticRead, type DiagnosticReport, type DiagnosticValue,
  } from "./vehicleDiagnostics";
  import { copyFullVin, vinDecodeLabels } from "./vehicleProfiles";

  let { open = false, adapterFamily = null, adapterId = null, connectionGeneration = null, diagnosticCapabilities = null, onClose }: {
    open?: boolean;
    adapterFamily: string | null;
    adapterId: string | null;
    connectionGeneration: number | null;
    diagnosticCapabilities: DiagnosticCapabilities | null;
    onClose: () => void;
  } = $props();

  type Tab = "dtcs" | "identity" | "mode06" | "raw";
  let report = $state<DiagnosticReport | null>(null);
  let activeTab = $state<Tab>("dtcs");
  let mode06TestIds = $state<number[]>([]);
  let mode06Input = $state("");
  let running = $state(false);
  let error = $state("");
  let copyMessage = $state("");
  let clearBusy = $state(false);
  let clearConfirm = $state<HTMLDialogElement>();
  let clearAdapterId = $state<string | null>(null);
  let clearGeneration = $state<number | null>(null);
  let dialog: HTMLDialogElement;

  const tabs: [Tab, string][] = [["dtcs", "DTCs"], ["identity", "Identity"], ["mode06", "Mode 06"], ["raw", "Raw"]];
  let supported = $derived(diagnosticCapabilities !== null
    ? diagnosticCapabilities.dtc_snapshot || diagnosticCapabilities.identity || diagnosticCapabilities.mode06
    : supportsVehicleDiagnostics(adapterFamily));
  let clearSupported = $derived(diagnosticCapabilities?.clear_dtcs === true);
  let clearConfirmationStale = $derived(!clearConfirmationMatchesConnection(
    adapterId, connectionGeneration, clearAdapterId, clearGeneration,
  ));
  let stale = $derived(
    report !== null && (adapterId !== report.adapter.stable_id
      || adapterFamily !== report.adapter.hardware_family
      || (report.connection_generation !== null && report.connection_generation !== undefined
        && connectionGeneration !== report.connection_generation)),
  );
  let bitmapIds = $derived(report ? availableMode06TestIds(report.reads) : []);
  let identityContext = $derived(report?.vehicle_context ?? null);
  let tabReads = $derived((report?.reads ?? []).filter((read) => {
    if (activeTab === "dtcs") return read.kind.endsWith("dtcs");
    if (activeTab === "identity") return ["vin", "calibration_id", "ecu_name"].includes(read.kind);
    if (activeTab === "mode06") return read.kind.startsWith("mode06_");
    return true;
  }));

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  async function readDiagnostics() {
    if (running || !supported) return;
    running = true;
    error = "";
    try {
      report = await invoke<DiagnosticReport>("run_vehicle_diagnostics", {
        request: { mode06_test_ids: mode06TestIds },
      });
      copyMessage = "";
    } catch (reason) {
      error = String(reason);
    } finally {
      running = false;
    }
  }

  function beginClearConfirmation() {
    if (clearBusy || !clearSupported || !adapterId || connectionGeneration === null) return;
    clearAdapterId = adapterId;
    clearGeneration = connectionGeneration;
    error = "";
    clearConfirm?.showModal();
  }

  async function confirmClear() {
    if (clearBusy || clearConfirmationStale || !clearAdapterId || clearGeneration === null) return;
    clearBusy = true;
    error = "";
    try {
      report = await invoke<DiagnosticReport>("clear_vehicle_dtcs", {
        expectedAdapterId: clearAdapterId,
        expectedGeneration: clearGeneration,
      });
      activeTab = "dtcs";
      copyMessage = "";
      clearConfirm?.close();
    } catch (reason) {
      error = String(reason);
    } finally {
      clearBusy = false;
    }
  }

  async function copyVin(vin: string) {
    copyMessage = await copyFullVin(vin, (value) => navigator.clipboard.writeText(value))
      ? "Full VIN copied."
      : "Clipboard unavailable. The VIN remains visible above.";
  }

  function addMode06Ids() {
    const parsed = parseMode06TestIds(mode06Input);
    if (parsed.error) {
      error = parsed.error;
      return;
    }
    const next = [...new Set([...mode06TestIds, ...parsed.ids])].sort((a, b) => a - b);
    if (next.length > 32) {
      error = "Select no more than 32 unique Mode 06 test IDs.";
      return;
    }
    error = "";
    mode06TestIds = next;
    mode06Input = "";
  }

  function toggleMode06Id(id: number) {
    if (!mode06TestIds.includes(id) && mode06TestIds.length >= 32) {
      error = "Select no more than 32 unique Mode 06 test IDs.";
      return;
    }
    error = "";
    mode06TestIds = mode06TestIds.includes(id)
      ? mode06TestIds.filter((selected) => selected !== id)
      : [...mode06TestIds, id].sort((a, b) => a - b);
  }

  function handleCancel(event: Event) {
    event.preventDefault();
    onClose();
  }

  function valuesOf(read: DiagnosticRead, type: DiagnosticValue["type"]): DiagnosticValue[] {
    return read.values.filter((value) => value.type === type);
  }

  function readTitle(read: DiagnosticRead): string {
    return diagnosticReadLabel(read);
  }

  function payloadHex(bytes: number[]): string {
    return bytes.map(formatHexByte).join(" ");
  }
</script>

<dialog bind:this={dialog} class="vehicle-diagnostics" aria-labelledby="vehicle-diagnostics-title" oncancel={handleCancel}>
    <header class="vehicle-diagnostics-heading">
      <div>
        <p class="eyebrow">VEHICLE READS</p>
        <h2 id="vehicle-diagnostics-title">Vehicle diagnostics</h2>
        <p>Read-only ECU information and evidence from the connected adapter.</p>
      </div>
      <button class="button secondary" type="button" aria-label="Close vehicle diagnostics" onclick={onClose}>Close</button>
    </header>

    {#if !supported}
      <div class="diagnostic-notice" role="status">
        Vehicle diagnostics are currently unsupported by {adapterFamily ?? "the selected adapter"}.
        Supported adapters: Xtool A30/A30M, Bluetooth ELM327, OBDLink SX, OpenPort 2, Candlelight OBD and simulator.
      </div>
    {:else}
      <div class="vehicle-diagnostics-toolbar">
        <button class="button primary" type="button" onclick={readDiagnostics} disabled={running}>
          {running ? "Reading vehicle…" : mode06TestIds.length ? "Read diagnostics and selected monitors" : "Read diagnostics"}
        </button>
        {#if running}<span class="diagnostic-running" role="status">Using the serialized adapter; this may take several seconds.</span>{/if}
        {#if clearSupported}
          <button class="button danger" type="button" onclick={beginClearConfirmation} disabled={running || clearBusy || connectionGeneration === null}>
            {clearBusy ? "Clearing DTCs…" : "Clear DTCs"}
          </button>
        {/if}
        {#if report}<span class="diagnostic-report-time">Last read {new Date(report.started_at_unix_ms).toLocaleString()}</span>{/if}
      </div>
    {/if}

    {#if error}<p class="diagnostic-error" role="alert">{error}</p>{/if}
    {#if report}
      <div class="diagnostic-report-meta">
        <span>{report.adapter.hardware_family}</span>
        <code>{report.adapter.stable_id}</code>
        <span>{report.protocol}</span>
        <span>{report.operation_kind === "clear_dtcs" ? "Explicit Mode 04 operation" : "Read-only snapshot"}</span>
      </div>
      {#if stale}
        <p class="diagnostic-notice stale" role="status">These results are from a previous adapter connection. Read again to refresh.</p>
      {/if}
      {#if report.route_restore_error}
        <p class="diagnostic-error" role="alert">The adapter did not confirm restoring its functional CAN route: {report.route_restore_error}</p>
      {/if}
      {#if report.evidence_saved}
        <p class="diagnostic-saved" role="status">Raw diagnostic reads saved to vehicle-reads.jsonl.</p>
      {:else if report.evidence_save_error}
        <p class="diagnostic-error" role="alert">Evidence could not be saved: {report.evidence_save_error}</p>
      {/if}

      <nav class="diagnostic-tabs" aria-label="Vehicle diagnostic pages">
        {#each tabs as [tab, label]}
          <button type="button" class:active={activeTab === tab} aria-pressed={activeTab === tab} onclick={() => activeTab = tab}>{label}</button>
        {/each}
      </nav>

      <div class="diagnostic-content">
        {#if activeTab === "identity" && identityContext}
          <section class="diagnostic-vehicle-context" aria-label="Captured vehicle identity">
            <strong>Captured vehicle context · {identityContext.identity_source.replaceAll("_", " ")}</strong>
            {#if identityContext.full_vin}
              <div><span>Full VIN <code>{identityContext.full_vin}</code></span><button class="button secondary" type="button" onclick={() => void copyVin(identityContext?.full_vin ?? "")}>Copy VIN</button></div>
            {:else if identityContext.masked_vin}
              <p>Masked VIN {identityContext.masked_vin} · full VIN unavailable in this report.</p>
            {:else if identityContext.vin_conflict}
              <p>Multiple ECU VINs conflict; this report is not assigned to one vehicle.</p>
            {:else}
              <p>No valid VIN was captured in this report or same-connection recognition.</p>
            {/if}
            {#if identityContext.decoded_snapshot}
              {@const labels = vinDecodeLabels(identityContext.decoded_snapshot)}
              <p>Offline VIN decode · {labels.manufacturer} · {labels.year} · {labels.checkDigit}</p>
              {#if identityContext.decoded_snapshot.wmi_assignment}
                <small>WMI source {identityContext.decoded_snapshot.wmi_assignment.source_id} · revision {identityContext.decoded_snapshot.wmi_assignment.source_revision} · decoder {identityContext.decoded_snapshot.decoder_revision}</small>
              {/if}
            {/if}
            {#if copyMessage}<small role="status">{copyMessage}</small>{/if}
          </section>
        {/if}
        {#if activeTab === "mode06"}
          <section class="mode06-selection" aria-label="Select Mode 06 monitors">
            <div>
              <h3>Available test IDs</h3>
              <p>Bitmap continuation pages are read only when the ECU advertises them. Select IDs, then start another explicit read.</p>
            </div>
            {#if bitmapIds.length}
              <div class="mode06-id-list">
                {#each bitmapIds as id}
                  <button type="button" class:selected={mode06TestIds.includes(id)} aria-pressed={mode06TestIds.includes(id)} onclick={() => toggleMode06Id(id)}>TID {formatHexByte(id)}</button>
                {/each}
              </div>
            {:else}
              <p class="diagnostic-empty">No monitor IDs were reported by the bitmap pages read.</p>
            {/if}
            <div class="mode06-add">
              <label for="mode06-id-input">Add TID values in hex</label>
              <input id="mode06-id-input" class="text-input" bind:value={mode06Input} placeholder="35, A2" disabled={running} onkeydown={(event) => event.key === "Enter" && addMode06Ids()} />
              <button class="button secondary" type="button" onclick={addMode06Ids} disabled={running || !mode06Input.trim()}>Add</button>
            </div>
            {#if mode06TestIds.length}
              <p class="mode06-selected">Selected: {mode06TestIds.map(formatHexByte).join(", ")}</p>
            {/if}
          </section>
        {/if}

        {#if !tabReads.length}
          <p class="diagnostic-empty">No results in this page yet. Start a read to query the connected ECU.</p>
        {:else}
          {#each groupDiagnosticReadsByEcu(tabReads) as [ecu, reads]}
            <section class="diagnostic-ecu" aria-label={`Results from ECU ${ecu}`}>
              <header><h3>{ecu}</h3><span>{reads[0]?.route ?? "route unavailable"} · {reads.length} read{reads.length === 1 ? "" : "s"}</span></header>
              {#each reads as read}
                <article class="diagnostic-read">
                  <div class="diagnostic-read-heading">
                    <h4>{readTitle(read)}</h4>
                    <span class={`diagnostic-outcome ${read.outcome}`}>{diagnosticOutcomeLabel(read.outcome)}</span>
                  </div>
                  <p class="diagnostic-read-meta">{read.latency_ms} ms · Mode {read.service.toString(16).toUpperCase().padStart(2, "0")}{read.identifier === null ? "" : ` · ID ${formatHexByte(read.identifier)}`}</p>
                  {#if read.negative_response_code !== null}<p class="diagnostic-detail">Negative response NRC {formatHexByte(read.negative_response_code)}</p>{/if}
                  {#if read.detail}<p class="diagnostic-detail">{read.detail}</p>{/if}

                  {#each valuesOf(read, "dtcList") as value}
                    {#if value.type === "dtcList"}
                      {#if value.payload.codes.length}
                        <div class="diagnostic-dtcs">{#each value.payload.codes as code}<code>{code}</code>{/each}</div>
                      {:else}<p class="diagnostic-empty">No codes reported.</p>{/if}
                    {/if}
                  {/each}
                  {#each valuesOf(read, "identity") as value}
                    {#if value.type === "identity"}<p class="diagnostic-identity"><span>{value.payload.label}</span><strong>{value.payload.value}</strong></p>{/if}
                  {/each}
                  {#each valuesOf(read, "mode06Bitmap") as value}
                    {#if value.type === "mode06Bitmap"}
                      <p class="diagnostic-bitmap">Page {formatHexByte(value.payload.start)} · {value.payload.test_ids.length} IDs {value.payload.has_more ? "· continuation advertised" : ""}</p>
                    {/if}
                  {/each}
                  {#each valuesOf(read, "mode06Monitor") as value}
                    {#if value.type === "mode06Monitor"}<p class="diagnostic-monitor">{mode06MonitorSummary(value.payload)}</p>{/if}
                  {/each}
                  {#each valuesOf(read, "dtcClearAcknowledgement") as value}
                    {#if value.type === "dtcClearAcknowledgement"}<p class="diagnostic-clear-ack">ECU acknowledged the Mode 04 clear request. Run a new diagnostic read to verify current codes.</p>{/if}
                  {/each}

                  {#if activeTab === "raw"}
                    <details class="diagnostic-raw" open>
                      <summary>Request and raw response</summary>
                      <small>{read.route} · TX {payloadHex(read.request_payload)}</small>
                      <code>{read.exact_adapter_command}</code>
                      <small>RX {payloadHex(read.response_payload) || "(no payload)"}</small>
                      <pre>{read.raw_response || "(no response bytes)"}</pre>
                    </details>
                  {/if}
                </article>
              {/each}
            </section>
          {/each}
        {/if}
      </div>
    {:else}
      <p class="diagnostic-empty first-read">Open this view when ready, then select Read diagnostics to send the one-shot requests.</p>
    {/if}
    {#if clearSupported && !clearBusy}
      <dialog bind:this={clearConfirm} class="clear-dtc-confirm" aria-labelledby="clear-dtc-title">
        <h3 id="clear-dtc-title">Clear emission-related DTC information?</h3>
        <p>This sends one standard Mode 04 request through <strong>{adapterFamily}</strong> to the responding ECU scope.</p>
        <p>It can erase stored diagnostic information and affect readiness state. A positive response means the ECU acknowledged the request; it does not prove that a later DTC read will be empty.</p>
        {#if adapterFamily === "Candlelight GS-USB OBD-II"}<p>The current Candlelight connection is limited to the single OBD ECU accepted during connection.</p>{/if}
        {#if clearConfirmationStale}<p class="diagnostic-error" role="alert">The adapter or connection changed. Cancel and reopen this confirmation.</p>{/if}
        <div class="clear-confirm-actions">
          <button class="button secondary" type="button" onclick={() => clearConfirm?.close()}>Cancel</button>
          <button class="button danger" type="button" onclick={confirmClear} disabled={clearConfirmationStale || clearBusy}>Confirm clear</button>
        </div>
      </dialog>
    {/if}
</dialog>

<style>
  .vehicle-diagnostics { width: min(920px, calc(100% - 36px)); max-height: min(90vh, 900px); flex-direction: column; overflow: hidden; margin: auto; border: 1px solid var(--border-strong); border-radius: 12px; color: var(--text); background: var(--surface); box-shadow: 0 24px 80px #0009; }
  .vehicle-diagnostics[open] { display: flex; }
  .vehicle-diagnostics::backdrop { background: #081016bb; }
  .vehicle-diagnostics-heading { display: flex; flex: 0 0 auto; align-items: center; justify-content: space-between; gap: 16px; padding: 15px 18px; border-bottom: 1px solid var(--border); }
  .vehicle-diagnostics-heading .eyebrow { margin: 0 0 4px; color: var(--accent); font-size: 9px; font-weight: 700; letter-spacing: .14em; }
  .vehicle-diagnostics-heading h2 { margin: 0; font-size: 20px; }
  .vehicle-diagnostics-heading p:last-child { margin: 4px 0 0; color: var(--text-muted); font-size: 11px; }
  .vehicle-diagnostics-heading .button { flex: 0 0 auto; }
  .vehicle-diagnostics-toolbar { display: flex; flex: 0 0 auto; align-items: center; gap: 12px; padding: 11px 18px; border-bottom: 1px solid var(--border-soft); }
  .vehicle-diagnostics-toolbar .button { flex: 0 0 auto; }
  .diagnostic-running, .diagnostic-report-time { color: var(--text-muted); font-size: 10px; }
  .diagnostic-report-time { margin-left: auto; text-align: right; }
  .diagnostic-notice, .diagnostic-error, .diagnostic-saved { flex: 0 0 auto; margin: 8px 18px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 7px; color: var(--text-secondary); background: var(--surface-raised); font-size: 11px; }
  .diagnostic-notice.stale { color: #f5d8a1; border-color: #80663c; background: #3b3327; }
  .diagnostic-error { color: #ffd0cb; border-color: #68403f; background: #382528; }
  .diagnostic-saved { color: #b9e1d4; border-color: #31564d; background: #1c3933; }
  .diagnostic-report-meta { display: flex; flex: 0 0 auto; flex-wrap: wrap; gap: 7px 12px; padding: 8px 18px 4px; color: var(--text-muted); font-size: 9px; }
  .diagnostic-report-meta code { color: var(--text-secondary); }
  .diagnostic-tabs { display: flex; flex: 0 0 auto; gap: 4px; padding: 9px 18px 0; border-bottom: 1px solid var(--border); }
  .diagnostic-tabs button { padding: 7px 11px; border: 0; border-bottom: 2px solid transparent; color: var(--text-muted); background: transparent; cursor: pointer; font: inherit; font-size: 11px; }
  .diagnostic-tabs button:hover, .diagnostic-tabs button.active { color: var(--text); border-bottom-color: var(--accent); }
  .diagnostic-content { flex: 1 1 auto; min-height: 140px; overflow: auto; padding: 10px 18px 18px; }
  .diagnostic-vehicle-context { display: grid; gap: 6px; margin-bottom: 10px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); font-size: 10px; }
  .diagnostic-vehicle-context > div { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; justify-content: space-between; }
  .diagnostic-vehicle-context p, .diagnostic-vehicle-context small { margin: 0; color: var(--text-muted); }
  .diagnostic-vehicle-context code { color: var(--text-primary); font: 600 11px ui-monospace, monospace; overflow-wrap: anywhere; }
  .diagnostic-ecu { margin: 0 0 12px; border: 1px solid var(--border); border-radius: 8px; overflow: hidden; }
  .diagnostic-ecu > header { display: flex; justify-content: space-between; gap: 10px; align-items: center; padding: 7px 10px; background: var(--surface-top); }
  .diagnostic-ecu > header h3 { margin: 0; font-size: 11px; }
  .diagnostic-ecu > header span { color: var(--text-muted); font-size: 9px; }
  .diagnostic-read { padding: 9px 10px; border-top: 1px solid var(--border-soft); }
  .diagnostic-read:first-of-type { border-top: 0; }
  .diagnostic-read-heading { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .diagnostic-read-heading h4 { margin: 0; font-size: 11px; }
  .diagnostic-outcome { padding: 2px 6px; border-radius: 10px; color: var(--text-muted); background: var(--surface-raised); font-size: 9px; text-transform: capitalize; white-space: nowrap; }
  .diagnostic-outcome.positive { color: #b9e1d4; background: #1c3933; }
  .diagnostic-outcome.negative_response, .diagnostic-outcome.malformed_response, .diagnostic-outcome.transport_error { color: #ffd0cb; background: #492825; }
  .diagnostic-outcome.no_response, .diagnostic-outcome.temporarily_unavailable, .diagnostic-outcome.response_pending { color: #f5d8a1; background: #3b3327; }
  .diagnostic-read-meta, .diagnostic-detail, .diagnostic-bitmap, .diagnostic-monitor { margin: 4px 0 0; color: var(--text-muted); font-size: 9px; }
  .diagnostic-detail { color: #ffd0cb; }
  .diagnostic-clear-ack { margin: 7px 0 0; color: #f5d8a1; font-size: 10px; }
  .clear-dtc-confirm { width: min(480px, calc(100% - 28px)); margin: auto; padding: 18px; border: 1px solid var(--border-strong); border-radius: 10px; color: var(--text); background: var(--surface); box-shadow: 0 24px 80px #000b; }
  .clear-dtc-confirm::backdrop { background: #081016cc; }
  .clear-dtc-confirm h3 { margin: 0 0 10px; font-size: 15px; }
  .clear-dtc-confirm p { margin: 8px 0; color: var(--text-secondary); font-size: 11px; line-height: 1.5; }
  .clear-confirm-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 15px; }
  .diagnostic-dtcs { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; }
  .diagnostic-dtcs code { padding: 3px 6px; border: 1px solid var(--border); border-radius: 5px; color: var(--text); background: var(--surface-raised); font-size: 10px; }
  .diagnostic-identity { display: flex; flex-wrap: wrap; gap: 4px 12px; margin: 7px 0 0; font-size: 10px; }
  .diagnostic-identity span { min-width: 90px; color: var(--text-muted); }
  .diagnostic-identity strong { overflow-wrap: anywhere; font-weight: 600; }
  .diagnostic-empty { margin: 7px 0; color: var(--text-muted); font-size: 10px; }
  .diagnostic-empty.first-read { margin: 18px; }
  .mode06-selection { display: grid; gap: 8px; margin-bottom: 10px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-deep); }
  .mode06-selection h3 { margin: 0; font-size: 11px; }
  .mode06-selection p { margin: 3px 0 0; color: var(--text-muted); font-size: 9px; }
  .mode06-id-list { display: flex; flex-wrap: wrap; gap: 5px; }
  .mode06-id-list button { padding: 4px 7px; border: 1px solid var(--border-strong); border-radius: 6px; color: var(--text-secondary); background: var(--surface); cursor: pointer; font: inherit; font-size: 9px; }
  .mode06-id-list button.selected { color: var(--accent); border-color: var(--accent); background: var(--surface-hover); }
  .mode06-add { display: flex; align-items: center; gap: 6px; }
  .mode06-add label { color: var(--text-muted); font-size: 9px; }
  .mode06-add input { width: 120px; min-width: 0; padding: 5px 7px; font-size: 10px; }
  .mode06-selected { color: var(--text-secondary) !important; }
  .diagnostic-raw { margin-top: 8px; color: var(--text-muted); font-size: 9px; }
  .diagnostic-raw summary { cursor: pointer; }
  .diagnostic-raw small, .diagnostic-raw code, .diagnostic-raw pre { display: block; margin: 5px 0 0; overflow-wrap: anywhere; }
  .diagnostic-raw code, .diagnostic-raw pre { color: var(--text-secondary); font: 9px/1.4 ui-monospace, monospace; white-space: pre-wrap; }
  .diagnostic-raw pre { max-height: 110px; overflow: auto; }
  @media (max-width: 620px) { .vehicle-diagnostics { width: calc(100% - 12px); max-height: 96vh; } .vehicle-diagnostics-heading, .vehicle-diagnostics-toolbar { padding-left: 10px; padding-right: 10px; } .diagnostic-content { padding-left: 10px; padding-right: 10px; } .diagnostic-tabs { padding-left: 10px; padding-right: 10px; } .diagnostic-report-meta { padding-left: 10px; padding-right: 10px; } .diagnostic-report-time { display: none; } }
</style>
