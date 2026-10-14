<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { oemCandidateTryState } from "./oemCandidateControls";
  import { A30_READ_TESTS, type A30ReadTestId } from "./a30ReadTests";
  import { runResearchQueue, toggleCandidateSelection } from "./researchQueue";
  import type {
    A30GraphValue, A30ReadTestResult, OemCandidateTryResult, OemCatalogPage, OemCandidate,
    DeviceDescriptor, OemPollingStatus, OemVehicleProfile, ResearchRunItem, ResearchRunReport,
    ResearchRunRow, ResearchRunViewState, ResearchTestResult, ResearchTestSelection,
  } from "./types";

  export let selectedPort = "";
  export let selectedBluetoothAddress = "";
  export let selectedAdapter: "obdlink_sx" | "a30" | "elm327" | "openport2" | "simulator" | "candlelight_obd" = "obdlink_sx";
  export let activeAdapter: DeviceDescriptor | null = null;
  export let disabled = false;
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
  let selectedCandidateIds: string[] = [];
  let selectedCandidateMetadata: Record<string, OemCandidate> = {};
  let error = "";
  let searchTimer: number | undefined;
  let requestId = 0;
  let selectedReadTest: A30ReadTestId = A30_READ_TESTS[0][0];
  let runRows: ResearchRunRow[] = [];
  let runInProgress = false;
  let stopRequested = false;
  let currentItemLabel = "";
  let currentQueueKey = "";
  let completedRunReport: ResearchRunReport | null = null;
  let completedCount = 0;
  let totalCount = 0;
  let canTryCandidates = false;
  let vehicleProfiles: OemVehicleProfile[] = [];
  let pollVehicleId = "";
  let pollQuery = "";
  let pollOffset = 0;
  let pollPage: OemCatalogPage | null = null;
  let pollLoading = false;
  let pollCandidateIds: string[] = [];
  let pollCandidateMetadata: Record<string, OemCandidate> = {};
  let pollingStatus: OemPollingStatus = {
    active: false, vehicle_id: null, signal_count: 0, request_count: 0,
    successful_requests: 0, failed_requests: 0, failed_signals: 0, samples_published: 0,
    observed_request_rate_hz: null, last_latency_ms: null, last_error: null,
  };
  let pollError = "";
  let pollSearchTimer: number | undefined;
  let pollStatusTimer: number | undefined;
  $: completedCount = runRows.filter(({ status }) => status === "completed" || status === "error").length;
  $: totalCount = runRows.length;
  $: canTryCandidates = selectedAdapter === "obdlink_sx" || selectedAdapter === "a30" || selectedAdapter === "elm327";

  onMount(() => {
    void loadPage();
    void loadPollProfiles();
    pollStatusTimer = window.setInterval(() => {
      if (pollingStatus.active) void refreshPollingStatus();
    }, 1000);
  });
  onDestroy(() => {
    window.clearTimeout(searchTimer);
    window.clearTimeout(pollSearchTimer);
    window.clearInterval(pollStatusTimer);
  });

  async function loadPollProfiles() {
    try {
      vehicleProfiles = await invoke<OemVehicleProfile[]>("oem_vehicle_profiles");
      pollingStatus = await invoke<OemPollingStatus>("oem_logging_status");
      const activeVehicleId = pollingStatus.active ? pollingStatus.vehicle_id : null;
      if (activeVehicleId && vehicleProfiles.some(({ vehicle_id }) => vehicle_id === activeVehicleId)) {
        pollVehicleId = activeVehicleId;
      } else if (!pollVehicleId && vehicleProfiles.length) {
        pollVehicleId = vehicleProfiles[0].vehicle_id;
      }
      if (pollVehicleId) {
        await loadPollCandidates();
      }
    } catch (caught) {
      pollError = String(caught);
    }
  }

  async function loadPollCandidates() {
    if (!pollVehicleId) return;
    pollLoading = true;
    pollError = "";
    try {
      pollPage = await invoke<OemCatalogPage>("search_oem_poll_candidates", {
        vehicleId: pollVehicleId, query: pollQuery, offset: pollOffset, limit: 100,
      });
    } catch (caught) {
      pollPage = null;
      pollError = String(caught);
    } finally {
      pollLoading = false;
    }
  }

  function schedulePollSearch() {
    pollOffset = 0;
    window.clearTimeout(pollSearchTimer);
    pollSearchTimer = window.setTimeout(() => void loadPollCandidates(), 220);
  }

  function changePollVehicle() {
    pollOffset = 0;
    pollCandidateIds = [];
    pollCandidateMetadata = {};
    void loadPollCandidates();
  }

  function setPollPage(nextOffset: number) {
    if (nextOffset < 0 || nextOffset >= (pollPage?.total ?? 0)) return;
    pollOffset = nextOffset;
    void loadPollCandidates();
  }

  async function refreshPollingStatus() {
    try {
      pollingStatus = await invoke<OemPollingStatus>("oem_logging_status");
    } catch (caught) {
      pollError = String(caught);
    }
  }

  function togglePollCandidate(candidate: OemCandidate, selected: boolean) {
    pollCandidateIds = selected
      ? [...new Set([...pollCandidateIds, candidate.candidate_id])].slice(0, 64)
      : pollCandidateIds.filter((id) => id !== candidate.candidate_id);
    if (selected) pollCandidateMetadata = { ...pollCandidateMetadata, [candidate.candidate_id]: candidate };
    else {
      const { [candidate.candidate_id]: _removed, ...remaining } = pollCandidateMetadata;
      pollCandidateMetadata = remaining;
    }
  }

  async function startOemLogging() {
    if (disabled || !canTryCandidates || !pollCandidateIds.length) return;
    pollError = "";
    try {
      pollingStatus = await invoke<OemPollingStatus>("start_oem_logging", {
        vehicleId: pollVehicleId, candidateIds: pollCandidateIds,
      });
      for (const candidateId of pollCandidateIds) {
        const candidate = pollCandidateMetadata[candidateId];
        if (candidate) onGraphCandidate(candidate);
      }
    } catch (caught) {
      pollError = String(caught);
    }
  }

  async function stopOemLogging() {
    pollError = "";
    try {
      pollingStatus = await invoke<OemPollingStatus>("stop_oem_logging");
    } catch (caught) {
      pollError = String(caught);
    }
  }

  function pollStatusSummary() {
    const rate = pollingStatus.observed_request_rate_hz === null
      ? "rate waiting"
      : `${pollingStatus.observed_request_rate_hz.toFixed(2)} req/s`;
    const latency = pollingStatus.last_latency_ms === null ? "—" : `${pollingStatus.last_latency_ms} ms`;
    return `${pollingStatus.request_count} requests · ${pollingStatus.samples_published} values · ${pollingStatus.failed_requests} request failures · ${pollingStatus.failed_signals} signal errors · ${rate} · last ${latency}`;
  }

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
          const payload = await invoke<A30ReadTestResult>("try_obd_read_test", {
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

  function runObdReadTest() {
    const label = A30_READ_TESTS.find(([id]) => id === selectedReadTest)?.[1] ?? selectedReadTest;
    void runQueue([{
      id: selectedReadTest,
      label,
      selection: { type: "a30ReadTest", payload: selectedReadTest },
    }]);
  }

  function stopQueue() {
    if (runInProgress) stopRequested = true;
  }

  function tryCandidate(candidate: OemCandidate) {
    const state = oemCandidateTryState(candidate, currentCandidateDevice(), disabled || runInProgress, selectedAdapter);
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

  <details class="oem-polling">
    <summary>
      <strong>Continuous OEM logging</strong>
      <span>{pollingStatus.active ? "Running" : `${pollPage?.total ?? "…"} verified signals`}</span>
    </summary>
    <div class="oem-polling-body">
      <div class="oem-polling-controls">
        <select class="text-input" bind:value={pollVehicleId} aria-label="Verified vehicle scope" disabled={pollingStatus.active} onchange={changePollVehicle}>
          {#each vehicleProfiles as profile (profile.vehicle_id)}
            <option value={profile.vehicle_id}>{profile.brand} {profile.model}{profile.model_year ? ` ${profile.model_year}` : ""}{profile.engine ? ` · ${profile.engine}` : ""}</option>
          {/each}
        </select>
        <input class="text-input" type="search" bind:value={pollQuery} oninput={schedulePollSearch} placeholder="Filter verified signals" aria-label="Filter verified OEM signals" />
      </div>
      {#if pollLoading}
        <p class="oem-empty">Checking vehicle-specific validation…</p>
      {:else if pollPage?.candidates.length}
        <div class="oem-poll-candidates">
          {#each pollPage.candidates as candidate (candidate.candidate_id)}
            <label>
              <input type="checkbox" checked={pollCandidateIds.includes(candidate.candidate_id)} onchange={(event) => togglePollCandidate(candidate, event.currentTarget.checked)} disabled={pollingStatus.active || disabled || !canTryCandidates || (pollCandidateIds.length >= 64 && !pollCandidateIds.includes(candidate.candidate_id))} />
              <span><strong>{candidate.name}</strong><small>{candidate.service} {candidate.identifier} · {candidate.request_header} → {candidate.response_header} · {candidate.unit}</small></span>
            </label>
          {/each}
        </div>
        {#if pollPage.total > pollPage.limit}
          <div class="oem-poll-pages">
            <button class="button secondary" onclick={() => setPollPage(pollOffset - 100)} disabled={pollOffset === 0 || pollLoading}>Previous</button>
            <span>{pollOffset + 1}–{Math.min(pollOffset + pollPage.candidates.length, pollPage.total)} of {pollPage.total}</span>
            <button class="button secondary" onclick={() => setPollPage(pollOffset + 100)} disabled={pollOffset + pollPage.limit >= pollPage.total || pollLoading}>Next</button>
          </div>
        {/if}
      {:else}
        <p class="oem-empty">No OEM signal is physically verified and enabled for this vehicle profile yet. Research candidates remain one-shot only.</p>
      {/if}
      <div class="oem-poll-footer">
        <span aria-live="polite">{pollingStatus.active ? pollStatusSummary() : `${pollCandidateIds.length} selected · max 64`}</span>
        {#if pollingStatus.active}
          <button class="button secondary" onclick={stopOemLogging} disabled={disabled}>Stop logging</button>
        {:else}
          <button class="button primary" onclick={startOemLogging} disabled={disabled || !canTryCandidates || !pollCandidateIds.length}>{pollLoading ? "Checking…" : "Start logging"}</button>
        {/if}
      </div>
      {#if pollingStatus.last_error}<p class="oem-error" role="status">Last OEM read error: {pollingStatus.last_error}</p>{/if}
      {#if pollError}<p class="oem-error" role="alert">OEM logging: {pollError}</p>{/if}
    </div>
  </details>

  {#if selectedAdapter === "obdlink_sx" || selectedAdapter === "a30"}
    <div class="obd-read-test">
      <div class="obd-read-test-controls">
        <select class="text-input" bind:value={selectedReadTest} aria-label="OBD read mode">
          {#each A30_READ_TESTS as [value, label]}<option {value}>{label}</option>{/each}
        </select>
        <button class="button secondary" onclick={runObdReadTest} disabled={disabled || !activeAdapter || !currentCandidateDevice() || runInProgress}>
          {runInProgress && currentQueueKey === selectedReadTest ? "Running…" : "Try once"}
        </button>
      </div>
    </div>
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
        {@const tryState = oemCandidateTryState(candidate, candidateDevice, disabled || runInProgress, selectedAdapter)}
        <article class="oem-candidate">
          <header>
            <input class="candidate-select" type="checkbox" aria-label={`Select ${candidate.name}`} checked={selectedCandidateIds.includes(candidate.candidate_id)} onchange={(event) => selectCandidate(candidate, event.currentTarget.checked)} disabled={runInProgress} />
            <strong title={candidate.name}>{candidate.name}</strong><span>{displayUnit(candidate.unit)}</span>
            <button class="button secondary oem-try-button" onclick={() => onGraphCandidate(candidate)} title="Add this research signal to the graph. This does not send a request to the vehicle." aria-label={`Graph ${candidate.name}`}>Graph</button>
            <button class="button secondary oem-try-button" onclick={() => tryCandidate(candidate)} disabled={tryState.disabled} title={tryState.reason || `Send one service ${candidate.service} read through ${selectedAdapter === "a30" ? "Xtool A30/A30M" : selectedAdapter === "elm327" ? "Bluetooth ELM327" : "OBDLink SX"}`} aria-label={`Try ${candidate.name} once`}>{runInProgress && currentQueueKey === candidate.candidate_id ? "Running…" : "Try once"}</button>
            {#if canConnected}
              <button class="button secondary oem-try-button" onclick={() => onTryCanCandidate(candidate.candidate_id)} disabled={canCandidateDisabled} title="Read this source-backed service 21/22 candidate over the active Candlelight CAN route" aria-label={`Try ${candidate.name} over CAN`}>CAN read</button>
            {/if}
          </header>
          {#if tryState.disabled && !runInProgress}<small class="oem-disabled-reason">{tryState.reason}</small>{/if}
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
  .oem-research-heading, .oem-candidate > header, .oem-pagination { display: flex; align-items: center; justify-content: space-between; gap: 4px; }
  .oem-catalog-note { overflow: hidden; color: var(--text-muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-candidate p { margin: 2px 0 0; color: var(--text-muted); font-size: 9px; line-height: 1.25; }
  .oem-count, .oem-pagination span { color: var(--text-muted); font-size: 10px; white-space: nowrap; }
  .oem-filters { display: grid; gap: 3px; }
  .oem-filter-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 78px; gap: 3px; }
  .oem-filters .text-input { min-width: 0; min-height: 25px; height: 25px; padding: 3px 5px; font-size: 9px; }
  .oem-polling { min-width: 0; padding: 4px 5px; border: 1px solid var(--border); border-radius: 6px; background: #142029; }
  .oem-polling > summary { display: flex; align-items: center; justify-content: space-between; gap: 5px; cursor: pointer; color: var(--accent); font-size: 9px; }
  .oem-polling > summary span { overflow: hidden; color: var(--text-muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-polling-body { display: grid; gap: 4px; margin-top: 5px; }
  .oem-polling-controls { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 3px; }
  .oem-polling .text-input { min-width: 0; min-height: 25px; height: 25px; padding: 3px 5px; font-size: 9px; }
  .oem-poll-candidates { display: grid; gap: 2px; max-height: 112px; overflow: auto; overscroll-behavior: contain; padding-right: 3px; scrollbar-gutter: stable; }
  .oem-poll-candidates label { display: flex; align-items: center; gap: 4px; min-width: 0; padding: 3px; border-radius: 4px; background: #19242e; cursor: pointer; }
  .oem-poll-candidates input { flex: 0 0 auto; accent-color: var(--accent); }
  .oem-poll-candidates span { display: grid; min-width: 0; }
  .oem-poll-candidates strong, .oem-poll-candidates small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .oem-poll-candidates strong { color: var(--text); font-size: 9px; }
  .oem-poll-candidates small, .oem-poll-footer span { color: var(--text-muted); font-size: 8px; }
  .oem-poll-pages { display: flex; align-items: center; justify-content: center; gap: 4px; }
  .oem-poll-pages span { color: var(--text-muted); font-size: 8px; }
  .oem-poll-pages .button { min-height: 22px; padding: 2px 5px; font-size: 8px; }
  .oem-poll-footer { display: flex; align-items: center; justify-content: space-between; gap: 4px; }
  .oem-poll-footer span { min-width: 0; overflow-wrap: anywhere; }
  .oem-poll-footer .button { flex: 0 0 auto; min-height: 24px; padding: 2px 6px; font-size: 9px; }
  .obd-read-test { display: grid; gap: 3px; min-width: 0; }
  .obd-read-test-controls { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .obd-read-test-controls select { flex: 1 1 auto; min-width: 0; min-height: 25px; height: 25px; padding: 3px 5px; font-size: 9px; }
  .obd-read-test-controls .button { flex: 0 0 auto; min-height: 25px; padding: 3px 6px; font-size: 9px; }
  .research-queue-controls { display: flex; align-items: center; gap: 3px; }
  .selected-candidate-count { flex: 1 1 auto; color: var(--text-muted); font-size: 10px; }
  .selected-candidate-count strong { color: var(--text); font-variant-numeric: tabular-nums; }
  .research-queue-controls .button { min-height: 24px; padding: 2px 6px; font-size: 9px; }
  .research-queue-progress { min-height: 0; margin: -2px 0 0 !important; color: var(--text-muted); font-size: 8px; line-height: 1.2; }
  .candidate-select { flex: 0 0 auto; accent-color: var(--accent); }
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
  .oem-candidate details { margin-top: 2px; color: var(--text-muted); font-size: 9px; }
  .oem-candidate summary { cursor: pointer; }
  .oem-candidate dl { display: grid; grid-template-columns: 78px minmax(0, 1fr); gap: 3px 6px; margin: 6px 0; }
  .oem-candidate dt { color: var(--text-muted); }
  .oem-candidate dd { min-width: 0; margin: 0; overflow-wrap: anywhere; color: var(--text); }
  .oem-candidate code { white-space: pre-wrap; overflow-wrap: anywhere; }
  .oem-candidate a { display: inline-block; margin: 3px 8px 0 0; color: var(--accent); font-size: 9px; }
  .oem-error { color: #ffb8ae; font-size: 10px; overflow-wrap: anywhere; }
  .oem-empty, .oem-footnote { margin: 0; color: var(--text-muted); font-size: 9px; line-height: 1.4; }
  .oem-pagination { justify-content: center; }
  .oem-pagination .button { min-height: 26px; padding: 3px 7px; font-size: 9px; }
</style>
