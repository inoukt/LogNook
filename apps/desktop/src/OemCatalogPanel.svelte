<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { oemCandidateTryState } from "./oemCandidateControls";
  import { A30_READ_TESTS, type A30ReadTestId } from "./a30ReadTests";
  import { runResearchQueue, toggleCandidateSelection } from "./researchQueue";
  import type {
    A30GraphValue, A30ReadTestResult, OemCandidateTryResult, OemCatalogPage, OemCandidate,
    DeviceDescriptor, ResearchRunItem, ResearchRunReport, ResearchRunRow, ResearchRunViewState, ResearchTestResult,
    ResearchTestSelection, ScionMode21ProbeResult,
  } from "./types";

  export let selectedPort = "";
  export let selectedBluetoothAddress = "";
  export let selectedAdapter: "obdlink_sx" | "a30" | "elm327" | "openport2" | "simulator" = "obdlink_sx";
  export let activeAdapter: DeviceDescriptor | null = null;
  export let disabled = false;
  export let scionProbeDisabled = false;
  export let canConnected = false;
  export let canCandidateDisabled = true;
  export let onGraphCandidate: (candidate: OemCandidate) => void = () => {};
  export let onTryCanCandidate: (candidateId: string) => void = () => {};
  export let onGraphReadValue: (value: A30GraphValue) => void = () => {};
  export let onRunUpdate: (state: ResearchRunViewState) => void = () => {};

  const pageSize = 50;
  let query = "";
  let brand = "";
  let model = "";
  let service = "";
  let offset = 0;
  let page: OemCatalogPage | null = null;
  let loading = false;
  let probing = false;
  let selectedCandidateIds: string[] = [];
  let selectedCandidateMetadata: Record<string, OemCandidate> = {};
  let error = "";
  let probeError = "";
  let probe: ScionMode21ProbeResult | null = null;
  let searchTimer: number | undefined;
  let requestId = 0;
  let selectedA30Test: A30ReadTestId = A30_READ_TESTS[0][0];
  let runRows: ResearchRunRow[] = [];
  let runInProgress = false;
  let stopRequested = false;
  let currentItemLabel = "";
  let currentQueueKey = "";
  let completedRunReport: ResearchRunReport | null = null;
  let completedCount = 0;
  let totalCount = 0;
  let canTryCandidates = false;
  $: completedCount = runRows.filter(({ status }) => status === "completed" || status === "error").length;
  $: totalCount = runRows.length;
  $: canTryCandidates = selectedAdapter === "obdlink_sx" || selectedAdapter === "a30" || selectedAdapter === "elm327";

  onMount(() => { void loadPage(); });
  onDestroy(() => window.clearTimeout(searchTimer));

  function scheduleSearch() {
    offset = 0;
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void loadPage(), 220);
  }

  async function loadPage() {
    const currentRequest = ++requestId;
    loading = true;
    error = "";
    try {
      const result = await invoke<OemCatalogPage>("search_oem_catalog", {
        query, brand, model, service: service || null, offset, limit: pageSize,
      });
      if (currentRequest === requestId) page = result;
    } catch (caught) {
      if (currentRequest === requestId) error = String(caught);
    } finally {
      if (currentRequest === requestId) loading = false;
    }
  }

  function setPage(nextOffset: number) {
    if (nextOffset < 0 || nextOffset >= (page?.total ?? 0)) return;
    offset = nextOffset;
    void loadPage();
  }

  async function runScionProbe() {
    if (selectedAdapter !== "obdlink_sx" || !selectedPort || scionProbeDisabled || probing || runInProgress) return;
    probing = true;
    probe = null;
    probeError = "";
    try {
      probe = await invoke<ScionMode21ProbeResult>("probe_scion_xb_mode21", { portName: selectedPort });
    } catch (caught) {
      probeError = String(caught);
    } finally {
      probing = false;
    }
  }

  function selectCandidate(candidate: OemCandidate, selected: boolean) {
    selectedCandidateIds = toggleCandidateSelection(selectedCandidateIds, candidate.candidate_id, selected);
    if (selected) {
      selectedCandidateMetadata = { ...selectedCandidateMetadata, [candidate.candidate_id]: candidate };
    } else {
      const { [candidate.candidate_id]: _removed, ...remaining } = selectedCandidateMetadata;
      selectedCandidateMetadata = remaining;
    }
  }

  function publishRunUpdate() {
    onRunUpdate({
      rows: runRows,
      running: runInProgress,
      currentItem: currentItemLabel,
      completedCount: runRows.filter(({ status }) => status === "completed" || status === "error").length,
      totalCount: runRows.length,
      report: completedRunReport,
    });
  }

  type QueueSelection = { id: string; label: string; selection: ResearchTestSelection };
  type QueueOutcome = { kind: "result"; result: ResearchTestResult } | { kind: "ineligible"; error: string };

  function currentCandidateDevice() {
    return selectedAdapter === "a30" || selectedAdapter === "elm327" ? selectedBluetoothAddress : selectedPort;
  }

  async function runQueue(items: QueueSelection[]) {
    const adapterAtStart = activeAdapter;
    if (disabled || runInProgress || !items.length || !adapterAtStart) return;
    const adapterSnapshot = { ...adapterAtStart };
    const selectedAdapterAtStart = selectedAdapter;
    const candidateDeviceAtStart = currentCandidateDevice();
    const runId = crypto.randomUUID();
    const startedAt = Date.now();
    const queueSize = items.length;
    stopRequested = false;
    runInProgress = true;
    currentItemLabel = "";
    currentQueueKey = "";
    completedRunReport = null;
    runRows = items.map(({ selection }, index) => ({
      queue_index: index + 1,
      queue_size: queueSize,
      selection,
      status: "queued",
      result: null,
      error: null,
    }));
    publishRunUpdate();

    await runResearchQueue<QueueOutcome>(
      items.map(({ id }) => id),
      async (id, index) => {
        const item = items[index];
        currentItemLabel = item.label;
        currentQueueKey = id;
        runRows = runRows.map((row, rowIndex) => rowIndex === index ? { ...row, status: "running" } : row);
        publishRunUpdate();

        if (item.selection.type === "a30ReadTest") {
          const payload = await invoke<A30ReadTestResult>("try_a30_read_test", {
            test: item.selection.payload,
            runId,
            queueIndex: index + 1,
            queueSize,
          });
          return { kind: "result", result: { type: "a30ReadTest", payload } };
        }

        const eligibility = oemCandidateTryState(item.selection.payload, candidateDeviceAtStart, false, selectedAdapterAtStart);
        if (eligibility.disabled) return { kind: "ineligible", error: eligibility.reason };

        const command = selectedAdapterAtStart === "a30"
          ? "try_a30_oem_candidate"
          : selectedAdapterAtStart === "elm327" ? "try_elm_oem_candidate" : "try_oem_candidate";
        const payload = await invoke<OemCandidateTryResult>(command, {
          candidateId: id,
          runId,
          queueIndex: index + 1,
          queueSize,
        });
        return { kind: "result", result: { type: "oemCandidate", payload } };
      },
      () => stopRequested,
      (_id, index, result) => {
        const item = items[index];
        runRows = runRows.map((row, rowIndex) => rowIndex !== index ? row : result instanceof Error
          ? { ...row, status: "error", error: result.message }
          : result.kind === "ineligible"
            ? { ...row, status: "error", error: result.error }
            : { ...row, status: "completed", result: result.result, error: null });
        if (!(result instanceof Error) && result.kind === "result") {
          const typedResult = result.result;
          if (typedResult.type === "oemCandidate" && item.selection.type === "oemCandidate"
            && typedResult.payload.outcome === "positive" && Number.isFinite(typedResult.payload.decoded_value?.value)) {
            onGraphCandidate(item.selection.payload);
          } else if (typedResult.type === "a30ReadTest" && typedResult.payload.outcome === "positive") {
            for (const value of typedResult.payload.graph_values) {
              if (Number.isFinite(value.value)) onGraphReadValue(value);
            }
          }
        }
        publishRunUpdate();
      },
    );

    runRows = runRows.map((row) => row.status === "queued" ? { ...row, status: "skipped" } : row);
    runInProgress = false;
    currentItemLabel = "";
    currentQueueKey = "";
    const reportItems: ResearchRunItem[] = runRows.map((row) => ({
      queue_index: row.queue_index,
      queue_size: row.queue_size,
      selection: row.selection,
      status: row.status === "completed" ? "completed" : row.status === "skipped" ? "skipped" : "error",
      result: row.result,
      error: row.error,
    }));
    completedRunReport = {
      run_id: runId,
      adapter: adapterSnapshot,
      started_at_unix_ms: startedAt,
      completed_at_unix_ms: Date.now(),
      items: reportItems,
    };
    publishRunUpdate();
  }

  function runSelectedCandidates() {
    const items = selectedCandidateIds.flatMap((id) => {
      const candidate = selectedCandidateMetadata[id];
      return candidate ? [{ id, label: candidate.name, selection: { type: "oemCandidate" as const, payload: candidate } }] : [];
    });
    void runQueue(items);
  }

  function runA30ReadTest() {
    const label = A30_READ_TESTS.find(([id]) => id === selectedA30Test)?.[1] ?? selectedA30Test;
    void runQueue([{
      id: selectedA30Test,
      label,
      selection: { type: "a30ReadTest", payload: selectedA30Test },
    }]);
  }

  function stopQueue() {
    if (runInProgress) stopRequested = true;
  }

  function tryCandidate(candidate: OemCandidate) {
    const state = oemCandidateTryState(candidate, currentCandidateDevice(), disabled || probing || runInProgress, selectedAdapter);
    if (state.disabled) return;
    void runQueue([{
      id: candidate.candidate_id,
      label: candidate.name,
      selection: { type: "oemCandidate", payload: candidate },
    }]);
  }

  function displayUnit(unit: string) {
    return ({ celsius: "°C", percent: "%", kilometersPerHour: "km/h", seconds: "s", minutes: "min" } as Record<string, string>)[unit] ?? unit;
  }
  function address(candidate: OemCandidate) {
    return `Service ${candidate.service} · ${candidate.service === "22" ? "DID" : "PID"} ${candidate.identifier} · ${candidate.request_header} → ${candidate.response_header}`;
  }
</script>

<section class="oem-research" aria-label="OEM PID research catalog">
  <div class="oem-research-heading">
    <span class="oem-catalog-note">Source-backed candidates · research only</span>
    {#if page}<span class="oem-count">{page.total.toLocaleString()} results</span>{/if}
  </div>

  <div class="oem-filters">
    <input class="text-input" type="search" bind:value={query} oninput={scheduleSearch} placeholder="Search name, PID, unit…" aria-label="Search OEM catalog" />
    <div class="oem-filter-row">
      <input class="text-input" bind:value={brand} oninput={scheduleSearch} placeholder="Brand" aria-label="Filter brand" />
      <input class="text-input" bind:value={model} oninput={scheduleSearch} placeholder="Model" aria-label="Filter model" />
      <select class="text-input" bind:value={service} onchange={scheduleSearch} aria-label="Filter service">
        <option value="">21 + 22</option><option value="21">Service 21</option><option value="22">Service 22</option>
      </select>
    </div>
  </div>

  {#if selectedAdapter === "a30"}
    <div class="a30-mode-test">
      <div class="a30-mode-test-controls">
        <select class="text-input" bind:value={selectedA30Test} aria-label="A30 read mode">
          {#each A30_READ_TESTS as [value, label]}<option {value}>{label}</option>{/each}
        </select>
        <button class="button secondary" onclick={runA30ReadTest} disabled={disabled || !selectedBluetoothAddress || runInProgress}>
          {runInProgress && currentQueueKey === selectedA30Test ? "Running…" : "Try once"}
        </button>
      </div>
    </div>
  {/if}

  {#if selectedAdapter === "obdlink_sx"}<div class="scion-probe-card">
    <div><strong>Scion xB 2014 · 21 01</strong><p>One source-backed read via the selected OBDLink SX. Only use on the 2AZ-FE Scion xB, stationary in Park with the parking brake set, ignition on and engine off. Close other diagnostic apps.</p></div>
    <button class="button secondary" onclick={runScionProbe} disabled={scionProbeDisabled || !selectedPort || probing || runInProgress}>
      {probing ? "Reading once…" : "Read 21 01 once"}
    </button>
  </div>{/if}
  {#if selectedAdapter === "obdlink_sx" && probeError}<p class="oem-error" role="alert">Probe could not start: {probeError}</p>{/if}
  {#if selectedAdapter === "obdlink_sx" && probe}
    <article class="probe-result" aria-live="polite">
      <header><strong>Probe outcome: {probe.outcome.replaceAll("_", " ")}</strong><span>{probe.latency_ms} ms</span></header>
      <p>{probe.adapter.hardware_family} · {probe.adapter.stable_id} · {probe.adapter.firmware ?? "firmware unknown"}</p>
      <p>{probe.vehicle_profile_id} · {probe.request_header} → {probe.expected_response_header} · {probe.exact_adapter_command}</p>
      <p>Source {probe.source_id} · SHA {probe.source_blob_sha} · {probe.license}</p>
      {#if probe.error}<p class="oem-error">{probe.error}</p>{/if}
      {#if probe.evidence_saved}<p>Evidence saved in app data, outside the catalog.</p>{:else if probe.evidence_save_error}<p class="oem-error">Probe completed, but evidence could not be saved: {probe.evidence_save_error}</p>{/if}
      {#if probe.decoded_signals.length}
        <div class="probe-values">
          {#each probe.decoded_signals as signal (signal.candidate_id)}
            <span>{signal.name}<strong>{signal.value.toFixed(2)} {displayUnit(signal.unit)}</strong><small>raw {signal.raw_value} · bits {signal.bit_offset}–{signal.bit_offset + signal.bit_length - 1}</small><code>{signal.formula}</code></span>
          {/each}
        </div>
      {/if}
      <details><summary>Raw SX response</summary><pre>{probe.raw_response || "No response text received"}</pre></details>
    </article>
  {/if}

  <div class="research-queue-controls">
    <span class="selected-candidate-count"><strong>{selectedCandidateIds.length}</strong> selected</span>
    <button class="button primary" onclick={runSelectedCandidates} disabled={disabled || runInProgress || !canTryCandidates || selectedCandidateIds.length === 0}>Run selected</button>
    <button class="button secondary" onclick={stopQueue} disabled={!runInProgress || stopRequested}>{stopRequested ? "Stopping…" : "Stop"}</button>
  </div>
  <p class="research-queue-progress" aria-live="polite">
    {#if runInProgress}
      {completedCount} of {totalCount} completed · {stopRequested ? "Stopping after the current read" : `Running ${completedCount + 1} of ${totalCount}: ${currentItemLabel}`}
    {:else if totalCount}
      {completedCount} of {totalCount} completed · {runRows.filter(({ status }) => status === "skipped").length} skipped
    {:else if !canTryCandidates}
      OEM candidate reads are unavailable for {selectedAdapter}.
    {:else}
      Select source-backed candidates; selection order is retained across search and pages.
    {/if}
  </p>

  {#if error}<p class="oem-error" role="alert">Catalog unavailable: {error}</p>
  {:else if loading && !page}<p class="oem-empty">Loading the local PID catalog…</p>
  {:else if page}
    <div class="oem-catalog-list" aria-busy={loading}>
      {#each page.candidates as candidate (candidate.candidate_id)}
        {@const candidateDevice = currentCandidateDevice()}
        {@const tryState = oemCandidateTryState(candidate, candidateDevice, disabled || probing || runInProgress, selectedAdapter)}
        <article class="oem-candidate">
          <header>
            <input class="candidate-select" type="checkbox" aria-label={`Select ${candidate.name}`} checked={selectedCandidateIds.includes(candidate.candidate_id)} onchange={(event) => selectCandidate(candidate, event.currentTarget.checked)} disabled={runInProgress} />
            <strong title={candidate.name}>{candidate.name}</strong><span>{displayUnit(candidate.unit)}</span>
            <button class="button secondary oem-try-button" onclick={() => tryCandidate(candidate)} disabled={tryState.disabled} title={tryState.reason || `Send one service ${candidate.service} read through ${selectedAdapter === "a30" ? "Xtool A30/A30M" : selectedAdapter === "elm327" ? "Bluetooth ELM327" : "OBDLink SX"}`} aria-label={`Try ${candidate.name} once`}>{runInProgress && currentQueueKey === candidate.candidate_id ? "Running…" : "Try once"}</button>
            {#if canConnected}
              <button class="button secondary oem-try-button" onclick={() => onTryCanCandidate(candidate.candidate_id)} disabled={canCandidateDisabled} title="Read this source-backed service 21/22 candidate over the active Candlelight CAN route" aria-label={`Try ${candidate.name} over CAN`}>CAN read</button>
            {/if}
          </header>
          {#if tryState.disabled && !runInProgress && !probing}<small class="oem-disabled-reason">{tryState.reason}</small>{/if}
          <p class="oem-address">{candidate.brand} {candidate.model_scope} · {address(candidate)}</p>
          <p>{candidate.module_type || candidate.category}{candidate.signal_source_id ? ` · ${candidate.signal_source_id}` : ""}{candidate.frequency_hz === null ? "" : ` · ${candidate.frequency_hz} Hz`}
            {#if candidate.bit_offset !== null && candidate.bit_length !== null} · bits {candidate.bit_offset}–{candidate.bit_offset + candidate.bit_length - 1}{/if}
          </p>
          <div class="oem-badges"><span>Research</span><span>{candidate.vehicle_validation.replaceAll("_", " ")}</span><span>{candidate.auto_request ? "Auto-request allowed" : "Not auto-polled"}</span></div>
          <details>
            <summary>Decoder, applicability and source</summary>
            <dl>
              <dt>Formula</dt><dd>{candidate.formula || "Not supplied"}</dd>
              <dt>Decoder</dt><dd><code>{candidate.decoder_json}</code></dd>
              <dt>Range</dt><dd>{candidate.min_value ?? "—"} to {candidate.max_value ?? "—"} {displayUnit(candidate.unit)}</dd>
              <dt>Year filter</dt><dd><code>{candidate.year_filter_json}</code></dd>
              <dt>ECU filter</dt><dd>{candidate.ecu_filter || "Not specified"}</dd>
              <dt>Scope</dt><dd>{candidate.source_scope || candidate.confidence}</dd>
              <dt>Source revision</dt><dd>{candidate.source_blob_sha}</dd>
              <dt>License</dt><dd>{candidate.license ?? "Not specified"}</dd>
            </dl>
            {#if candidate.notes}<p>{candidate.notes}</p>{/if}
            {#if candidate.source_url}<a href={candidate.source_url} target="_blank" rel="noreferrer">Open source record</a>{/if}
            {#if candidate.license_url}<a href={candidate.license_url} target="_blank" rel="noreferrer">Open source license</a>{/if}
          </details>
        </article>
      {:else}
        <p class="oem-empty">No candidates match these filters.</p>
      {/each}
    </div>
    <footer class="oem-pagination">
      <button class="button secondary" onclick={() => setPage(offset - pageSize)} disabled={offset === 0 || loading}>Previous</button>
      <span>{page.total === 0 ? 0 : offset + 1}–{Math.min(offset + page.candidates.length, page.total)} of {page.total.toLocaleString()}</span>
      <button class="button secondary" onclick={() => setPage(offset + pageSize)} disabled={offset + pageSize >= page.total || loading}>Next</button>
    </footer>
  {/if}
  <p class="oem-footnote">Candidate responses remain research evidence until validated on the exact vehicle. Numeric results use the shared graph and recording pipeline.</p>
</section>

<style>
  .oem-research { display: flex; flex: 1 1 auto; flex-direction: column; gap: 4px; min-height: 0; overflow: hidden; }
  .oem-research > :not(.oem-catalog-list) { flex: 0 0 auto; }
  .oem-research-heading, .scion-probe-card, .probe-result > header, .oem-candidate > header, .oem-pagination { display: flex; align-items: center; justify-content: space-between; gap: 4px; }
  .oem-catalog-note { overflow: hidden; color: var(--text-muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .scion-probe-card p, .probe-result p, .oem-candidate p { margin: 2px 0 0; color: var(--text-muted); font-size: 9px; line-height: 1.25; }
  .oem-count, .oem-pagination span { color: var(--text-muted); font-size: 10px; white-space: nowrap; }
  .oem-filters { display: grid; gap: 3px; }
  .oem-filter-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 78px; gap: 3px; }
  .oem-filters .text-input { min-width: 0; min-height: 25px; height: 25px; padding: 3px 5px; font-size: 9px; }
  .a30-mode-test { display: grid; gap: 3px; min-width: 0; }
  .a30-mode-test-controls { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .a30-mode-test-controls select { flex: 1 1 auto; min-width: 0; min-height: 25px; height: 25px; padding: 3px 5px; font-size: 9px; }
  .a30-mode-test-controls .button { flex: 0 0 auto; min-height: 25px; padding: 3px 6px; font-size: 9px; }
  .research-queue-controls { display: flex; align-items: center; gap: 3px; }
  .selected-candidate-count { flex: 1 1 auto; color: var(--text-muted); font-size: 10px; }
  .selected-candidate-count strong { color: var(--text); font-variant-numeric: tabular-nums; }
  .research-queue-controls .button { min-height: 24px; padding: 2px 6px; font-size: 9px; }
  .research-queue-progress { min-height: 0; margin: -2px 0 0 !important; color: var(--text-muted); font-size: 8px; line-height: 1.2; }
  .candidate-select { flex: 0 0 auto; accent-color: var(--accent); }
  .scion-probe-card, .probe-result { padding: 5px; border: 1px solid var(--border); border-radius: 6px; background: #142029; }
  .scion-probe-card strong, .probe-result strong { font-size: 10px; }
  .scion-probe-card .button { flex: 0 0 auto; min-height: 25px; padding: 3px 5px; font-size: 9px; }
  .oem-catalog-list { display: grid; grid-template-columns: minmax(0, 1fr); flex: 1 1 0; align-content: start; gap: 3px; max-height: none; min-width: 0; min-height: 80px; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; padding-right: 4px; scrollbar-gutter: stable; }
  .oem-candidate { min-width: 0; padding: 4px 5px; border: 1px solid var(--border); border-radius: 6px; background: #19242e; }
  .oem-candidate > header { min-width: 0; gap: 4px; }
  .oem-candidate > header strong { flex: 1 1 0; min-width: 0; overflow: hidden; font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-candidate > header span { flex: 0 1 auto; min-width: 0; max-width: 20%; overflow: hidden; color: var(--text); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-try-button { flex: 0 0 auto; min-height: 22px; padding: 2px 5px; font-size: 8px; }
  .oem-disabled-reason { display: block; margin: 2px 0; color: var(--text-muted); font-size: 8px; line-height: 1.2; }
  .oem-address { color: #b8c9d3 !important; }
  .oem-badges { display: flex; flex-wrap: nowrap; gap: 3px; margin-top: 3px; overflow: hidden; }
  .oem-badges span { overflow: hidden; padding: 1px 4px; border-radius: 8px; color: #c3d2da; background: #263742; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-candidate details, .probe-result details { margin-top: 2px; color: var(--text-muted); font-size: 9px; }
  .oem-candidate summary, .probe-result summary { cursor: pointer; }
  .oem-candidate dl { display: grid; grid-template-columns: 78px minmax(0, 1fr); gap: 3px 6px; margin: 6px 0; }
  .oem-candidate dt { color: var(--text-muted); }
  .oem-candidate dd { min-width: 0; margin: 0; overflow-wrap: anywhere; color: var(--text); }
  .oem-candidate code { white-space: pre-wrap; overflow-wrap: anywhere; }
  .oem-candidate a { display: inline-block; margin: 3px 8px 0 0; color: var(--accent); font-size: 9px; }
  .probe-result > header strong { text-transform: capitalize; }
  .probe-values { display: grid; grid-template-columns: 1fr 1fr; gap: 4px; margin-top: 7px; }
  .probe-values span { display: flex; flex-direction: column; padding: 5px; border-radius: 5px; color: var(--text-muted); background: #202d37; font-size: 9px; }
  .probe-values strong { color: var(--text); font-size: 10px; }
  .probe-values small, .probe-values code { overflow-wrap: anywhere; color: var(--text-muted); font-size: 8px; }
  .probe-result pre { max-height: 140px; overflow: auto; white-space: pre-wrap; color: var(--text); font-size: 9px; }
  .oem-error { color: #ffb8ae; font-size: 10px; overflow-wrap: anywhere; }
  .oem-empty, .oem-footnote { margin: 0; color: var(--text-muted); font-size: 9px; line-height: 1.4; }
  .oem-pagination { justify-content: center; }
  .oem-pagination .button { min-height: 26px; padding: 3px 7px; font-size: 9px; }
</style>
