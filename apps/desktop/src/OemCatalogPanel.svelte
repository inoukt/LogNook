<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { CatalogInspection } from "./catalogInspection";
  import SubaruCatalogPanel from "./SubaruCatalogPanel.svelte";
  import type { SubaruCatalogRow } from "./subaruCatalog";
  import { oemCandidateTryState } from "./oemCandidateControls";
  import {
    isCurrentOemDiscoveryConnection, nextVisibleOemCandidateIds, oemDiscoveryFilter,
    oemDiscoveryPresentation, oemReadDiscoveryResult, uniqueCandidateIds,
    updateVisibleOemCandidateIds,
    type OemDiscoveryResponse, type OemDiscoveryResult, type OemDiscoveryStatus,
  } from "./oemDiscovery";
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
  export let connectionGeneration: number | null = null;
  export let disabled = false;
  export let subaruDisabled = false;
  export let canConnected = false;
  export let canCandidateDisabled = true;
  export let onGraphCandidate: (candidate: OemCandidate) => void = () => {};
  export let onTryCanCandidate: (candidateId: string) => void = () => {};
  export let onGraphReadValue: (value: A30GraphValue) => void = () => {};
  export let onInspect: (inspection: CatalogInspection | null) => void = () => {};
  export let onRunUpdate: (state: ResearchRunViewState) => void = () => {};
  export let onSubaruSelect: (row: SubaruCatalogRow) => void = () => {};
  export let onAddSubaruSignal: (row: SubaruCatalogRow) => void = () => {};
  export let savedWorkspaceFilters: Record<string, unknown> = {};
  export let onWorkspaceFiltersChange: (filters: Record<string, unknown>) => void = () => {};
  let catalogSource: "community" | "subaru" = "community";
  let browseView: "all" | "values" | "selected" = "all";
  let filtersOpen = false;
  let inspectedCandidate: OemCandidate | null = null;
  let selectedOffset = 0;
  let selectedCandidates: OemCandidate[] = [];
  let visibleCandidates: OemCandidate[] = [];
  let valueCount = 0;
  let activeFilterCount = 0;
  let filtersReady = false;

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
  let valuesFirst = false;
  let discoveryStatus: OemDiscoveryStatus = {
    adapter_id: null, connection_generation: null, auto_try_visible: false, running: false,
    queued: 0, completed: 0, with_values: 0, current_candidate_id: null, last_error: null, revision: 0,
  };
  let discoveryResults: Record<string, OemDiscoveryResult> = {};
  let discoveryError = "";
  let discoveryAutoTry = false;
  let discoveryControlBusy = false;
  let discoveryStopPending = false;
  let discoveryStopOperations = 0;
  let discoveryFilteredActive = false;
  let visibleCandidateIds: string[] = [];
  let attemptedCandidateIds = new Set<string>();
  let discoveryPollTimer: number | undefined;
  let discoveryEpoch = 0;
  let discoveryContextKey = "";
  let discoveryContextDisabled = false;
  let destroyed = false;
  let currentDiscoveryAdapterId: string | null = null;
  let currentDiscoveryGeneration: number | null = null;
  let catalogListElement: HTMLElement | null = null;
  let candidateObserver: IntersectionObserver | null = null;
  let candidateElements = new Map<HTMLElement, string>();
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
  $: canTryCandidates = selectedAdapter === "obdlink_sx" || selectedAdapter === "a30" || selectedAdapter === "elm327"
    || selectedAdapter === "openport2" || selectedAdapter === "candlelight_obd";
  $: if (filtersReady) onWorkspaceFiltersChange({ ...savedWorkspaceFilters, oem_catalog: { source: catalogSource, query, brand, model, service, pollQuery } });
  function updateSubaruFilters(filters: Record<string, unknown>) {
    savedWorkspaceFilters = { ...savedWorkspaceFilters, subaru_catalog: filters };
    onWorkspaceFiltersChange(savedWorkspaceFilters);
  }
  $: catalogSwitchBusy = runInProgress || discoveryControlBusy || discoveryAutoTry
    || discoveryFilteredActive || discoveryStatus.running || discoveryStatus.queued > 0;
  $: selectedCandidates = selectedCandidateIds
    .flatMap((id) => selectedCandidateMetadata[id] ? [selectedCandidateMetadata[id]] : []);
  $: selectedOffset = Math.min(selectedOffset, Math.max(0, Math.floor(Math.max(0, selectedCandidates.length - 1) / pageSize) * pageSize));
  $: valueCount = (page?.candidates ?? []).filter((candidate) => {
    const result = discoveryResults[candidate.candidate_id];
    return result?.state === "value" && Number.isFinite(result.value);
  }).length;
  $: activeFilterCount = [brand.trim(), model.trim(), service].filter(Boolean).length;
  $: visibleCandidates = (browseView === "selected"
    ? selectedCandidates.slice(selectedOffset, selectedOffset + pageSize)
    : page?.candidates ?? []).filter((candidate) => {
      if (browseView !== "values") return true;
      const result = discoveryResults[candidate.candidate_id];
      return result?.state === "value" && Number.isFinite(result.value);
    });
  $: if (catalogSource === "community") {
    const inspectedId = inspectedCandidate?.candidate_id;
    const candidate = inspectedId
      ? page?.candidates.find((row) => row.candidate_id === inspectedId)
        ?? selectedCandidateMetadata[inspectedId]
        ?? inspectedCandidate
      : null;
    if (candidate) {
      const result = discoveryResults[candidate.candidate_id] ?? null;
      const presentation = oemDiscoveryPresentation(result);
      const candidateDevice = selectedAdapter === "a30" || selectedAdapter === "elm327"
        ? selectedBluetoothAddress
        : selectedPort || activeAdapter?.path || activeAdapter?.stable_id || "";
      const tryState = oemCandidateTryState(candidate, candidateDevice, disabled || runInProgress, selectedAdapter);
      const hasValue = result?.state === "value" && Number.isFinite(result.value);
      const inspection: CatalogInspection = {
        id: candidate.candidate_id,
        name: candidate.name,
        subtitle: `${candidate.brand} ${candidate.model_scope} · ${address(candidate)}`,
        status: hasValue ? "Numeric response" : presentation.label,
        tone: presentation.tone === "value" ? "value" : presentation.tone === "failure" ? "warning" : "neutral",
        value: hasValue ? `${result?.value} ${displayUnit(result?.unit || candidate.unit)}`.trim() : undefined,
        note: result?.detail ?? (candidate.vehicle_validation === "not_tested"
          ? "One-shot research · not vehicle verified."
          : `Validation: ${candidate.vehicle_validation.replaceAll("_", " ")}`),
        fields: [
          { label: "Vehicle validation", value: candidate.vehicle_validation.replaceAll("_", " ") },
          { label: "Module / category", value: candidate.module_type || candidate.category },
          { label: "Range / unit", value: `${candidate.min_value ?? "—"} to ${candidate.max_value ?? "—"} ${displayUnit(candidate.unit)}` },
          { label: "Frequency", value: candidate.frequency_hz === null ? "Not supplied" : `${candidate.frequency_hz} Hz` },
          { label: "Response / evidence", value: result ? `${result.outcome ?? result.state.replaceAll("_", " ")} · ${result.evidence_saved ? "raw saved" : "not saved"}` : "No response recorded" },
        ],
        blocks: [
          { label: "Provenance and applicability", value: [
            `Source ID: ${candidate.source_id}`,
            `Source revision: ${candidate.source_blob_sha}`,
            `License: ${candidate.license ?? "Not specified"}`,
            `Research state: ${candidate.research_state.replaceAll("_", " ")}`,
            `Auto-request: ${candidate.auto_request ? "enabled in source metadata" : "disabled"}`,
            `ECU filter: ${candidate.ecu_filter || "Not specified"}`,
            `Scope: ${candidate.source_scope || candidate.confidence}`,
            `Signal source: ${candidate.signal_source_id || "Not specified"}`,
            `Signal bits: ${candidate.bit_offset !== null && candidate.bit_length !== null ? `${candidate.bit_offset}–${candidate.bit_offset + candidate.bit_length - 1}` : "Not specified"}`,
          ].join("\n") },
          { label: "Formula", value: candidate.formula || "Not supplied" },
          { label: "Decoder payload", value: candidate.decoder_json || "Not supplied" },
          { label: "Year filter", value: candidate.year_filter_json || "Not supplied" },
          ...(candidate.notes ? [{ label: "Source notes", value: candidate.notes }] : []),
          ...(result ? [{ label: "Discovery result", value: JSON.stringify(result, null, 2) }] : []),
        ],
        links: [
          ...(candidate.source_url ? [{ label: "Open source record", url: candidate.source_url }] : []),
          ...(candidate.license_url ? [{ label: "Open source license", url: candidate.license_url }] : []),
        ],
        actions: [
          { label: "Graph", run: () => onGraphCandidate(candidate), title: "Add this research signal to the graph without sending a request." },
          {
            label: runInProgress && currentQueueKey === candidate.candidate_id ? "Running…" : "Try once",
            run: () => tryCandidate(candidate),
            disabled: tryState.disabled,
            title: tryState.reason || `Send one service ${candidate.service} read through ${adapterTryName()}`,
          },
          ...(canConnected ? [{
            label: "CAN read",
            run: () => onTryCanCandidate(candidate.candidate_id),
            disabled: canCandidateDisabled,
            title: canCandidateDisabled ? "Open an active CAN connection and paired capture, then finish other CAN operations." : "Read this source-backed candidate through the active Candlelight CAN route.",
          }] : []),
        ],
      };
      onInspect(inspection);
    }
  }
  $: syncDiscoveryContext(activeAdapter?.stable_id ?? null, connectionGeneration, disabled);
  $: if (catalogListElement && !candidateObserver) observeCatalogCandidates(catalogListElement);

  onMount(() => {
    destroyed = false;
    const filters = savedWorkspaceFilters.oem_catalog;
    if (filters && typeof filters === "object" && !Array.isArray(filters)) {
      const saved = filters as Record<string, unknown>;
      if (saved.source === "community" || saved.source === "subaru") catalogSource = saved.source;
      if (typeof saved.query === "string") query = saved.query;
      if (typeof saved.brand === "string") brand = saved.brand;
      if (typeof saved.model === "string") model = saved.model;
      if (typeof saved.service === "string") service = saved.service;
      if (typeof saved.pollQuery === "string") pollQuery = saved.pollQuery;
    }
    filtersReady = true;
    void loadPage();
    void loadPollProfiles();
    pollStatusTimer = window.setInterval(() => {
      if (pollingStatus.active) void refreshPollingStatus();
    }, 1000);
  });
  onDestroy(() => {
    destroyed = true;
    discoveryEpoch += 1;
    requestId += 1;
    window.clearTimeout(searchTimer);
    window.clearTimeout(pollSearchTimer);
    window.clearInterval(pollStatusTimer);
    window.clearTimeout(discoveryPollTimer);
    candidateObserver?.disconnect();
    onInspect(null);
    void stopDiscoveryFor(currentDiscoveryAdapterId, currentDiscoveryGeneration);
  });

  function observeCatalogCandidates(root: HTMLElement) {
    candidateObserver?.disconnect();
    candidateObserver = new IntersectionObserver((entries) => {
      const newlyVisible: string[] = [];
      for (const entry of entries) {
        const candidateId = candidateElements.get(entry.target as HTMLElement);
        if (!candidateId) continue;
        visibleCandidateIds = updateVisibleOemCandidateIds(visibleCandidateIds, candidateId, entry.isIntersecting);
        if (entry.isIntersecting && discoveryAutoTry) newlyVisible.push(candidateId);
      }
      if (newlyVisible.length) void enqueueVisibleCandidates(newlyVisible);
    }, { root, threshold: 0 });
    for (const node of candidateElements.keys()) candidateObserver.observe(node);
  }

  function trackCandidateVisibility(node: HTMLElement, candidateId: string) {
    candidateElements.set(node, candidateId);
    candidateObserver?.observe(node);
    return {
      update(nextCandidateId: string) {
        candidateObserver?.unobserve(node);
        candidateElements.set(node, nextCandidateId);
        candidateObserver?.observe(node);
      },
      destroy() {
        candidateObserver?.unobserve(node);
        candidateElements.delete(node);
        visibleCandidateIds = visibleCandidateIds.filter((id) => id !== candidateId);
      },
    };
  }

  function syncDiscoveryContext(adapterId: string | null, generation: number | null, isDisabled: boolean) {
    const key = `${adapterId ?? ""}:${generation ?? ""}:${isDisabled ? "disabled" : "ready"}`;
    if (key === discoveryContextKey) return;
    const previousAdapterId = currentDiscoveryAdapterId;
    const previousGeneration = currentDiscoveryGeneration;
    const connectionChanged = adapterId !== currentDiscoveryAdapterId || generation !== currentDiscoveryGeneration;
    const wasDisabled = discoveryContextDisabled;
    discoveryEpoch += 1;
    discoveryContextKey = key;
    discoveryContextDisabled = isDisabled;
    currentDiscoveryAdapterId = adapterId;
    currentDiscoveryGeneration = generation;
    discoveryAutoTry = false;
    discoveryControlBusy = false;
    discoveryFilteredActive = false;
    if (connectionChanged) {
      attemptedCandidateIds = new Set();
      discoveryResults = {};
      discoveryStatus = {
        adapter_id: null, connection_generation: null, auto_try_visible: false, running: false,
        queued: 0, completed: 0, with_values: 0, current_candidate_id: null, last_error: null, revision: 0,
      };
    } else if (!wasDisabled && isDisabled) {
      discoveryStatus = { ...discoveryStatus, auto_try_visible: false, queued: 0 };
    }
    window.clearTimeout(discoveryPollTimer);
    if (previousAdapterId && previousGeneration !== null && (connectionChanged || (!wasDisabled && isDisabled))) {
      void stopDiscoveryFor(previousAdapterId, previousGeneration);
    }
    if (adapterId && generation !== null && !isDisabled && !destroyed) void refreshDiscoveryStatus(adapterId, generation);
  }

  async function stopDiscoveryFor(adapterId: string | null, generation: number | null): Promise<OemDiscoveryResponse | null> {
    if (!adapterId || generation === null) return null;
    attemptedCandidateIds = new Set();
    window.clearTimeout(discoveryPollTimer);
    discoveryPollTimer = undefined;
    discoveryStopOperations += 1;
    discoveryStopPending = true;
    try {
      try {
        await invoke<OemDiscoveryResponse>("oem_discovery", {
          request: { kind: "set_auto_try", enabled: false, adapter_id: adapterId, connection_generation: generation },
        });
      } catch {
        // Stop still cancels queued work after a disconnect invalidates the connection-bound toggle.
      }
      return await invoke<OemDiscoveryResponse>("oem_discovery", { request: { kind: "stop" } });
    } catch {
      // Disconnect and unmount cleanup is best-effort; the backend also scopes work to the connection generation.
      return null;
    } finally {
      if (!destroyed) {
        discoveryStopOperations -= 1;
        discoveryStopPending = discoveryStopOperations > 0;
      }
    }
  }

  function applyDiscoveryResponse(response: OemDiscoveryResponse, adapterId: string, generation: number) {
    if (destroyed || disabled || adapterId !== currentDiscoveryAdapterId || generation !== currentDiscoveryGeneration
      || activeAdapter?.stable_id !== adapterId || connectionGeneration !== generation
      || !isCurrentOemDiscoveryConnection(response.status, adapterId, generation)) return false;
    if (response.status.revision < discoveryStatus.revision) return false;
    discoveryStatus = response.status;
    discoveryAutoTry = response.status.auto_try_visible;
    const next = { ...discoveryResults };
    for (const result of response.results ?? []) next[result.candidate_id] = result;
    discoveryResults = next;
    if (response.status.last_error) discoveryError = response.status.last_error;
    const active = response.status.running || response.status.queued > 0 || discoveryAutoTry || discoveryFilteredActive;
    window.clearTimeout(discoveryPollTimer);
    discoveryPollTimer = active ? window.setTimeout(() => void refreshDiscoveryStatus(adapterId, generation), 400) : undefined;
    if (discoveryFilteredActive && !response.status.running && response.status.queued === 0) {
      discoveryFilteredActive = false;
      void loadPage();
    }
    return true;
  }

  async function refreshDiscoveryStatus(adapterId = currentDiscoveryAdapterId, generation = currentDiscoveryGeneration) {
    if (!adapterId || generation === null || generation === undefined || disabled || destroyed) return;
    const expectedEpoch = discoveryEpoch;
    const candidateIds = uniqueCandidateIds(page?.candidates.map(({ candidate_id }) => candidate_id) ?? []).slice(0, pageSize);
    try {
      const response = await invoke<OemDiscoveryResponse>("oem_discovery", {
        request: { kind: "status", candidate_ids: candidateIds },
      });
      if (expectedEpoch === discoveryEpoch) applyDiscoveryResponse(response, adapterId, generation);
    } catch (caught) {
      if (expectedEpoch === discoveryEpoch) discoveryError = String(caught);
    }
  }

  async function enqueueVisibleCandidates(candidateIds: string[]) {
    const adapterId = currentDiscoveryAdapterId;
    const generation = currentDiscoveryGeneration;
    if (!discoveryAutoTry || disabled || destroyed || !adapterId || generation === null) return;
    const ids = nextVisibleOemCandidateIds(candidateIds, attemptedCandidateIds);
    if (!ids.length) return;
    for (const id of ids) attemptedCandidateIds.add(id);
    const expectedEpoch = discoveryEpoch;
    discoveryError = "";
    try {
      const response = await invoke<OemDiscoveryResponse>("oem_discovery", {
        request: { kind: "enqueue_visible", candidate_ids: ids, adapter_id: adapterId, connection_generation: generation },
      });
      if (expectedEpoch === discoveryEpoch) applyDiscoveryResponse(response, adapterId, generation);
    } catch (caught) {
      if (expectedEpoch === discoveryEpoch) {
        discoveryError = String(caught);
        for (const id of ids) attemptedCandidateIds.delete(id);
      }
    }
  }

  async function toggleAutoTryVisible(enabled: boolean) {
    const adapterId = currentDiscoveryAdapterId;
    const generation = currentDiscoveryGeneration;
    if (!adapterId || generation === null || disabled || discoveryStopPending) return;
    discoveryEpoch += 1;
    const expectedEpoch = discoveryEpoch;
    discoveryControlBusy = true;
    discoveryAutoTry = enabled;
    discoveryError = "";
    try {
      const response = await invoke<OemDiscoveryResponse>("oem_discovery", {
        request: { kind: "set_auto_try", enabled, adapter_id: adapterId, connection_generation: generation },
      });
      if (expectedEpoch !== discoveryEpoch) return;
      if (!applyDiscoveryResponse(response, adapterId, generation)) return;
      if (enabled) await enqueueVisibleCandidates(visibleCandidateIds);
      else {
        const stopped = await stopDiscoveryFor(adapterId, generation);
        if (stopped) applyDiscoveryResponse(stopped, adapterId, generation);
      }
    } catch (caught) {
      if (expectedEpoch === discoveryEpoch) {
        discoveryAutoTry = false;
        discoveryError = String(caught);
      }
    } finally {
      if (expectedEpoch === discoveryEpoch) discoveryControlBusy = false;
    }
  }

  async function stopDiscovery() {
    const adapterId = currentDiscoveryAdapterId;
    const generation = currentDiscoveryGeneration;
    discoveryAutoTry = false;
    discoveryControlBusy = false;
    discoveryFilteredActive = false;
    discoveryEpoch += 1;
    if (adapterId && generation !== null) {
      const response = await stopDiscoveryFor(adapterId, generation);
      if (response) applyDiscoveryResponse(response, adapterId, generation);
    }
  }

  async function tryFilteredCandidates() {
    const adapterId = currentDiscoveryAdapterId;
    const generation = currentDiscoveryGeneration;
    if (!adapterId || generation === null || disabled || discoveryFilteredActive || discoveryStopPending) return;
    if (discoveryAutoTry || discoveryStatus.running || discoveryStatus.queued > 0) await stopDiscovery();
    discoveryEpoch += 1;
    const expectedEpoch = discoveryEpoch;
    discoveryAutoTry = false;
    discoveryFilteredActive = true;
    discoveryError = "";
    valuesFirst = true;
    try {
      const response = await invoke<OemDiscoveryResponse>("oem_discovery", {
        request: {
          kind: "try_filtered",
          filter: oemDiscoveryFilter(query, brand, model, service),
          adapter_id: adapterId,
          connection_generation: generation,
        },
      });
      if (expectedEpoch !== discoveryEpoch) return;
      if (!applyDiscoveryResponse(response, adapterId, generation)) return;
      if (response.status.running || response.status.queued > 0) await loadPage();
    } catch (caught) {
      if (expectedEpoch === discoveryEpoch) {
        discoveryFilteredActive = false;
        discoveryError = String(caught);
      }
    }
  }

  function discoveryResult(candidateId: string): OemDiscoveryResult | null {
    return discoveryResults[candidateId] ?? null;
  }

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
    valuesFirst = false;
    discoveryError = "";
    requestId += 1;
    discoveryEpoch += 1;
    discoveryControlBusy = false;
    if (discoveryAutoTry || discoveryFilteredActive || discoveryStatus.running || discoveryStatus.queued > 0) void stopDiscovery();
    offset = 0;
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void loadPage(), 220);
  }

  async function loadPage() {
    const currentRequest = ++requestId;
    loading = true;
    error = "";
    try {
      const result = await invoke<OemDiscoveryResponse>("oem_discovery", {
        request: {
          kind: "search",
          filter: oemDiscoveryFilter(query, brand, model, service),
          offset,
          limit: pageSize,
          values_first: valuesFirst,
        },
      });
      if (!destroyed && currentRequest === requestId) {
        page = result.page ?? null;
        if (activeAdapter && connectionGeneration !== null) {
          applyDiscoveryResponse(result, activeAdapter.stable_id, connectionGeneration);
        }
      }
    } catch (caught) {
      if (!destroyed && currentRequest === requestId) error = String(caught);
    } finally {
      if (!destroyed && currentRequest === requestId) loading = false;
    }
  }

  function setPage(nextOffset: number) {
    if (nextOffset < 0 || nextOffset >= (page?.total ?? 0)) return;
    offset = nextOffset;
    void loadPage();
  }

  function setSelectedPage(nextOffset: number) {
    if (nextOffset < 0 || nextOffset >= selectedCandidates.length) return;
    selectedOffset = nextOffset;
  }

  function setCatalogSource(source: "community" | "subaru") {
    if (source === catalogSource || catalogSwitchBusy) return;
    inspectedCandidate = null;
    onInspect(null);
    catalogSource = source;
    browseView = "all";
    selectedOffset = 0;
  }

  function inspectCandidate(candidate: OemCandidate) {
    if (inspectedCandidate?.candidate_id === candidate.candidate_id) {
      inspectedCandidate = null;
      onInspect(null);
    } else {
      inspectedCandidate = candidate;
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
    return selectedAdapter === "a30" || selectedAdapter === "elm327"
      ? selectedBluetoothAddress
      : selectedPort || activeAdapter?.path || activeAdapter?.stable_id || "";
  }

  function adapterTryName() {
    return selectedAdapter === "simulator" ? "SIMULATOR (no vehicle transmission)"
      : selectedAdapter === "openport2" ? "OpenPort 2"
        : selectedAdapter === "candlelight_obd" ? "Candlelight OBD"
          : selectedAdapter === "a30" ? "Xtool A30/A30M"
            : selectedAdapter === "elm327" ? "Bluetooth ELM327" : "OBDLink SX";
  }

  async function runQueue(items: QueueSelection[]) {
    const adapterAtStart = activeAdapter;
    if (disabled || runInProgress || !items.length || !adapterAtStart) return;
    const adapterSnapshot = { ...adapterAtStart };
    const connectionAtStart = connectionGeneration;
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
            && activeAdapter?.stable_id === adapterSnapshot.stable_id && connectionGeneration === connectionAtStart) {
            discoveryResults = {
              ...discoveryResults,
              [item.id]: oemReadDiscoveryResult(typedResult.payload, item.selection.payload.source_blob_sha),
            };
          }
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
  <div class="catalog-source-row">
    <select class="text-input" aria-label="OEM catalog source" value={catalogSource} disabled={catalogSwitchBusy} title={catalogSwitchBusy ? "Stop discovery before switching catalogs" : "Choose a signal catalog"} onchange={(event) => setCatalogSource(event.currentTarget.value as "community" | "subaru")}>
      <option value="community">Community catalog</option>
      <option value="subaru">Subaru OEM catalog</option>
    </select>
    {#if selectedAdapter === "simulator"}<span class="oem-simulator-label" title="Simulator · no vehicle transmission">Simulator · no TX</span>
    {:else}<span class="oem-catalog-note" title="Source-backed candidates · research only">Research</span>{/if}
  </div>
  {#if catalogSource === "subaru"}
    <SubaruCatalogPanel disabled={subaruDisabled || runInProgress} savedFilters={savedWorkspaceFilters.subaru_catalog && typeof savedWorkspaceFilters.subaru_catalog === "object" ? savedWorkspaceFilters.subaru_catalog as Record<string, unknown> : {}} onFiltersChange={updateSubaruFilters} onSelect={onSubaruSelect} onAddToSignals={onAddSubaruSignal} onInspect={onInspect} />
  {:else}
  <div class="oem-search-row">
    <input class="text-input" type="search" bind:value={query} oninput={scheduleSearch} placeholder="Search name, PID/DID, unit…" aria-label="Search OEM catalog" />
    <button class="button secondary filters-toggle" type="button" aria-expanded={filtersOpen} aria-controls="oem-optional-filters" onclick={() => filtersOpen = !filtersOpen}>
      Filters{#if activeFilterCount}<span class="filter-count">{activeFilterCount}</span>{/if}
    </button>
  </div>
  <div class="oem-filters" id="oem-optional-filters" hidden={!filtersOpen}>
    <div class="oem-filter-row">
      <input class="text-input" bind:value={brand} oninput={scheduleSearch} placeholder="Brand" aria-label="Filter brand" />
      <input class="text-input" bind:value={model} oninput={scheduleSearch} placeholder="Model" aria-label="Filter model" />
      <select class="text-input" bind:value={service} onchange={scheduleSearch} aria-label="Filter service">
        <option value="">21 + 22</option><option value="21">Service 21</option><option value="22">Service 22</option>
      </select>
    </div>
  </div>

  <div class="oem-quick-views" role="group" aria-label="OEM candidate views">
    <button type="button" class:active={browseView === "all"} aria-pressed={browseView === "all"} onclick={() => browseView = "all"}>All <span>{page?.total.toLocaleString() ?? "…"}</span></button>
    <button type="button" class:active={browseView === "values"} aria-pressed={browseView === "values"} title="Numeric values on this page" aria-label={`With values on this page: ${valueCount}`} onclick={() => browseView = "values"}>With values <span>{valueCount}</span></button>
    <button type="button" class:active={browseView === "selected"} aria-pressed={browseView === "selected"} onclick={() => { browseView = "selected"; selectedOffset = 0; }}>Selected <span>{selectedCandidateIds.length}</span></button>
  </div>

  <div class="oem-discovery-controls">
    <label class="oem-auto-try" title="Try each visible row once per connection while browsing">
      <input
        type="checkbox"
        checked={discoveryAutoTry}
        onchange={(event) => toggleAutoTryVisible(event.currentTarget.checked)}
        disabled={disabled || discoveryControlBusy || discoveryStopPending || !activeAdapter || connectionGeneration === null}
        aria-label="Auto-try visible OEM candidates once per connection"
      />
      <span>Auto-try visible</span>
    </label>
    <button class="button secondary" onclick={tryFilteredCandidates} disabled={disabled || discoveryControlBusy || discoveryStopPending || !activeAdapter || connectionGeneration === null || discoveryFilteredActive}>
      {discoveryFilteredActive ? "Trying filtered list…" : "Try all filtered"}
    </button>
    {#if discoveryAutoTry || discoveryFilteredActive || discoveryStatus.running || discoveryStatus.queued > 0}
      <button class="button secondary" onclick={stopDiscovery} disabled={disabled}>Stop discovery</button>
    {/if}
    {#if discoveryAutoTry || discoveryFilteredActive || discoveryStatus.running || discoveryStatus.completed > 0}
    <span class="oem-discovery-progress" aria-live="polite">
      {#if discoveryStatus.running}
        {discoveryAutoTry || discoveryFilteredActive ? "Reading" : "Finishing current read"} {discoveryStatus.current_candidate_id ?? "candidate"} · {discoveryStatus.completed} complete · {discoveryStatus.with_values} numeric
      {:else if discoveryFilteredActive}
        {discoveryStatus.completed} complete · {discoveryStatus.with_values} numeric
      {:else if discoveryAutoTry}
        Watching visible rows · {discoveryStatus.completed} complete · {discoveryStatus.with_values} numeric
      {:else}
        Discovery: {discoveryStatus.with_values} numeric values this connection
      {/if}
    </span>
    {/if}
    {#if discoveryError}<span class="oem-discovery-error" role="alert">{discoveryError}</span>{/if}
  </div>

  <details class="oem-extra-tools">
    <summary>Read &amp; logging tools{#if pollingStatus.last_error || pollError}<span class="tools-error">Read error</span>{:else if pollingStatus.active}<span>Logging active</span>{:else if runInProgress && currentQueueKey === selectedReadTest}<span>Reading…</span>{/if}</summary>
    <div class="oem-extra-tools-body">
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
    <details class="oem-read-test">
      <summary>{runInProgress && currentQueueKey === selectedReadTest ? "Adapter read test · running" : "Adapter read test"}</summary>
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
    </details>
  {/if}

    </div>
  </details>

  {#if selectedCandidateIds.length > 0 || runInProgress || totalCount > 0}
  <div class="research-queue-controls">
    <span class="selected-candidate-count"><strong>{selectedCandidateIds.length}</strong> selected</span>
    <button class="button primary" onclick={runSelectedCandidates} disabled={disabled || runInProgress || selectedCandidateIds.length === 0}>Run selected</button>
    {#if runInProgress}<button class="button secondary" onclick={stopQueue} disabled={stopRequested}>{stopRequested ? "Stopping…" : "Stop"}</button>{/if}
  </div>
  {/if}
  {#if runInProgress || totalCount > 0}
  <p class="research-queue-progress" aria-live="polite">
    {#if runInProgress}
      {completedCount} of {totalCount} completed · {stopRequested ? "Stopping after the current read" : `Running ${completedCount + 1} of ${totalCount}: ${currentItemLabel}`}
    {:else if totalCount}
      {completedCount} of {totalCount} completed · {runRows.filter(({ status }) => status === "skipped").length} skipped
    {/if}
  </p>
  {/if}

  {#if error}<p class="oem-error" role="alert">Catalog unavailable: {error}</p>
  {:else if loading && !page}<p class="oem-empty">Loading the local PID catalog…</p>
  {:else if page || browseView === "selected"}
    <div class="oem-catalog-list" bind:this={catalogListElement} aria-busy={loading}>
      {#each visibleCandidates as candidate (candidate.candidate_id)}
        {@const result = discoveryResult(candidate.candidate_id)}
        {@const discoveryBadge = oemDiscoveryPresentation(result)}
        <article class="oem-candidate-row" class:inspected={inspectedCandidate?.candidate_id === candidate.candidate_id} use:trackCandidateVisibility={candidate.candidate_id}>
          <input class="candidate-select" type="checkbox" aria-label={`Select ${candidate.name} for the research queue`} checked={selectedCandidateIds.includes(candidate.candidate_id)} onchange={(event) => selectCandidate(candidate, event.currentTarget.checked)} disabled={runInProgress} />
          <button class="candidate-inspect" type="button" aria-label={`Inspect ${candidate.name}`} aria-pressed={inspectedCandidate?.candidate_id === candidate.candidate_id} onclick={() => inspectCandidate(candidate)}>
            <strong title={candidate.name}>{candidate.name} <span>{displayUnit(candidate.unit)}</span></strong>
            <small title={`${candidate.brand} ${candidate.model_scope} · ${address(candidate)}`}>{candidate.brand} {candidate.model_scope} · {address(candidate)}</small>
          </button>
          <span class={"oem-discovery-badge tone-" + discoveryBadge.tone} title={result?.detail ?? result?.outcome ?? "No response recorded"}>{discoveryBadge.label}</span>
          <button class="button secondary graph-candidate" type="button" onclick={() => onGraphCandidate(candidate)} title="Add this research signal to the graph. This does not send a request to the vehicle." aria-label={`Graph ${candidate.name}`}>Graph</button>
        </article>
      {:else}
        <p class="oem-empty">{browseView === "values" ? "No numeric values on this page. Try another page or wait for discovery." : browseView === "selected" ? "No candidates selected for the research queue." : "No candidates match these filters."}</p>
      {/each}
    </div>
    <footer class="oem-pagination">
      {#if browseView === "selected"}
        <button class="button secondary" onclick={() => setSelectedPage(selectedOffset - pageSize)} disabled={selectedOffset === 0}>Previous</button>
        <span>{selectedCandidates.length === 0 ? 0 : selectedOffset + 1}–{Math.min(selectedOffset + pageSize, selectedCandidates.length)} of {selectedCandidates.length} selected</span>
        <button class="button secondary" onclick={() => setSelectedPage(selectedOffset + pageSize)} disabled={selectedOffset + pageSize >= selectedCandidates.length}>Next</button>
      {:else}
        <button class="button secondary" onclick={() => setPage(offset - pageSize)} disabled={offset === 0 || loading}>Previous</button>
        <span>{browseView === "values" ? `${visibleCandidates.length} with values on this page · ` : ""}{page ? `${page.total === 0 ? 0 : offset + 1}–${Math.min(offset + page.candidates.length, page.total)} of ${page.total.toLocaleString()}` : "Loading catalog"}</span>
        <button class="button secondary" onclick={() => setPage(offset + pageSize)} disabled={!page || offset + pageSize >= page.total || loading}>Next</button>
      {/if}
    </footer>
  {/if}
  <p class="oem-footnote">Select a name for details · responses remain unverified research.</p>
  {/if}
</section>

<style>
  .catalog-source-row { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .catalog-source-row .text-input { flex: 1; min-width: 0; height: 27px; min-height: 27px; padding: 2px 6px; font-size: 11px; }
  .oem-research { display: flex; flex: 1 1 auto; flex-direction: column; gap: 4px; min-height: 0; overflow: hidden; }
  .oem-research > :not(.oem-catalog-list) { flex: 0 0 auto; }
  .oem-research > :global(.subaru-catalog) { flex: 1 1 0; min-width: 0; min-height: 0; }
  .oem-pagination { display: flex; align-items: center; justify-content: space-between; gap: 6px; }
  .oem-catalog-note { overflow: hidden; color: var(--text-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-search-row { display: flex; min-width: 0; gap: 5px; }
  .oem-search-row .text-input { flex: 1 1 auto; min-width: 0; min-height: 28px; height: 28px; padding: 3px 7px; font-size: 11px; }
  .filters-toggle { flex: 0 0 auto; min-height: 28px; padding: 3px 7px; font-size: 10px; }
  .filter-count { display: inline-grid; min-width: 16px; height: 16px; place-items: center; margin-left: 4px; border-radius: 50%; color: var(--accent-text); background: var(--accent); font-size: 9px; font-variant-numeric: tabular-nums; }
  .oem-filters { display: grid; gap: 3px; }
  .oem-filters[hidden] { display: none; }
  .oem-filter-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 78px; gap: 3px; }
  .oem-filters .text-input { min-width: 0; min-height: 29px; height: 29px; padding: 4px 7px; font-size: 10px; }
  .oem-quick-views { display: flex; min-width: 0; gap: 3px; padding: 2px; border: 1px solid var(--border-soft); border-radius: 7px; background: var(--surface-deep); }
  .oem-quick-views button { display: flex; flex: 1 1 auto; align-items: center; justify-content: center; gap: 5px; min-width: 0; min-height: 24px; padding: 2px 5px; border: 0; border-radius: 5px; color: var(--text-muted); background: transparent; cursor: pointer; font: inherit; font-size: 10px; white-space: nowrap; }
  .oem-quick-views button.active { color: var(--text); background: var(--surface-raised); }
  .oem-quick-views button span { color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  .oem-quick-views button:focus-visible, .candidate-inspect:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .oem-discovery-controls { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 1px 0; }
  .oem-auto-try { display: flex; flex: 1 1 auto; align-items: center; gap: 5px; min-width: 0; color: var(--text-secondary); font-size: 10px; }
  .oem-auto-try input { flex: 0 0 auto; accent-color: var(--accent); }
  .oem-discovery-controls > .button { min-height: 26px; padding: 2px 7px; font-size: 10px; }
  .oem-discovery-progress, .oem-discovery-error { flex: 1 1 100%; min-width: 0; color: var(--text-muted); font-size: 10px; overflow-wrap: anywhere; }
  .oem-discovery-error { color: #ffb8ae; }
  .oem-simulator-label { flex: 0 0 auto; color: var(--status-warning, #fbbf24); font-size: 9px; white-space: nowrap; }
  .oem-extra-tools { min-width: 0; border-bottom: 1px solid var(--border-soft); }
  .oem-research > .oem-extra-tools { flex: 0 1 auto; min-height: 25px; overflow: auto; }
  .oem-extra-tools > summary { display: flex; align-items: center; min-height: 23px; padding: 2px 3px; color: var(--text-muted); cursor: pointer; font-size: 10px; list-style: none; }
  .oem-extra-tools > summary::before { content: "▸"; margin-right: 5px; }
  .oem-extra-tools[open] > summary::before { content: "▾"; }
  .oem-extra-tools > summary span { margin-left: auto; color: var(--accent); }
  .oem-extra-tools > summary .tools-error { color: var(--danger); }
  .oem-extra-tools-body { display: grid; gap: 4px; max-height: min(240px, 25vh); overflow: auto; padding: 4px 0; }
  .oem-discovery-badge { display: inline-block; max-width: 100%; overflow: hidden; padding: 2px 6px; border-radius: 8px; font-size: 10px; line-height: 1.35; text-overflow: ellipsis; white-space: nowrap; }
  .oem-discovery-badge.tone-untested { color: #cbd5e1; background: #334155; }
  .oem-discovery-badge.tone-waiting, .oem-discovery-badge.tone-active { color: #bfdbfe; background: #1e3a5f; }
  .oem-discovery-badge.tone-value { color: #bbf7d0; background: #14532d; }
  .oem-discovery-badge.tone-reply { color: #fde68a; background: #713f12; }
  .oem-discovery-badge.tone-failure { color: #fecaca; background: #7f1d1d; }
  .oem-polling, .oem-read-test { min-width: 0; padding: 5px 6px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-top); }
  .oem-polling > summary, .oem-read-test > summary { display: flex; align-items: center; justify-content: space-between; gap: 5px; cursor: pointer; color: var(--accent); font-size: 10px; }
  .oem-polling > summary span { overflow: hidden; color: var(--text-muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .oem-polling-body { display: grid; gap: 4px; margin-top: 5px; }
  .oem-polling-controls { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 3px; }
  .oem-polling .text-input { min-width: 0; min-height: 29px; height: 29px; padding: 4px 7px; font-size: 10px; }
  .oem-poll-candidates { display: grid; gap: 2px; max-height: 112px; overflow: auto; overscroll-behavior: contain; padding-right: 3px; scrollbar-gutter: stable; }
  .oem-poll-candidates label { display: flex; align-items: center; gap: 4px; min-width: 0; padding: 3px; border-radius: 4px; background: #19242e; cursor: pointer; }
  .oem-poll-candidates input { flex: 0 0 auto; accent-color: var(--accent); }
  .oem-poll-candidates span { display: grid; min-width: 0; }
  .oem-poll-candidates strong, .oem-poll-candidates small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .oem-poll-candidates strong { color: var(--text); font-size: 10px; }
  .oem-poll-candidates small, .oem-poll-footer span { color: var(--text-muted); font-size: 9px; }
  .oem-poll-pages { display: flex; align-items: center; justify-content: center; gap: 4px; }
  .oem-poll-pages span { color: var(--text-muted); font-size: 9px; }
  .oem-poll-pages .button { min-height: 25px; padding: 3px 7px; font-size: 9px; }
  .oem-poll-footer { display: flex; align-items: center; justify-content: space-between; gap: 4px; }
  .oem-poll-footer span { min-width: 0; overflow-wrap: anywhere; }
  .oem-poll-footer .button { flex: 0 0 auto; min-height: 27px; padding: 3px 8px; font-size: 10px; }
  .obd-read-test { display: grid; gap: 5px; min-width: 0; margin-top: 6px; }
  .obd-read-test-controls { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .obd-read-test-controls select { flex: 1 1 auto; min-width: 0; min-height: 29px; height: 29px; padding: 4px 7px; font-size: 10px; }
  .obd-read-test-controls .button { flex: 0 0 auto; min-height: 29px; padding: 3px 8px; font-size: 10px; }
  .research-queue-controls { display: flex; align-items: center; gap: 5px; }
  .selected-candidate-count { flex: 1 1 auto; color: var(--text-muted); font-size: 10px; }
  .selected-candidate-count strong { color: var(--text); font-variant-numeric: tabular-nums; }
  .research-queue-controls .button { min-height: 28px; padding: 3px 8px; font-size: 10px; }
  .research-queue-progress { min-height: 0; margin: 0 !important; color: var(--text-muted); font-size: 10px; line-height: 1.35; }
  .candidate-select { width: 16px; height: 16px; flex: 0 0 auto; accent-color: var(--accent); }
  .oem-catalog-list { display: flex; flex: 1 1 0; flex-direction: column; min-width: 0; min-height: 80px; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; padding-right: 3px; scrollbar-gutter: stable; }
  .oem-candidate-row { display: grid; flex: 0 0 auto; grid-template-columns: 18px minmax(0, 1fr) max-content 48px; align-items: center; gap: 7px; min-width: 0; padding: 7px 4px; border-bottom: 1px solid var(--border-soft); }
  .oem-candidate-row:hover { background: var(--surface-raised); }
  .oem-candidate-row.inspected { box-shadow: inset 2px 0 var(--accent); background: var(--surface-raised); }
  .candidate-inspect { display: grid; gap: 3px; min-width: 0; padding: 1px 0; border: 0; color: var(--text); background: transparent; text-align: left; cursor: pointer; }
  .candidate-inspect strong { display: flex; align-items: baseline; justify-content: space-between; gap: 5px; min-width: 0; font-size: 11px; font-weight: 650; line-height: 1.25; }
  .candidate-inspect strong span { flex: 0 0 auto; color: var(--text-muted); font-size: 10px; font-weight: 500; }
  .candidate-inspect small { min-width: 0; overflow: hidden; color: var(--text-muted); font-size: 10px; line-height: 1.25; text-overflow: ellipsis; white-space: nowrap; }
  .oem-candidate-row .oem-discovery-badge { max-width: 112px; }
  .graph-candidate { min-width: 0; min-height: 28px; padding: 3px 7px; font-size: 10px; }
  .oem-error { color: #ffb8ae; font-size: 10px; overflow-wrap: anywhere; }
  .oem-empty, .oem-footnote { margin: 0; color: var(--text-muted); font-size: 10px; line-height: 1.4; }
  .oem-pagination { justify-content: space-between; }
  .oem-pagination span { min-width: 0; color: var(--text-muted); font-size: 10px; text-align: center; white-space: normal; }
  .oem-pagination .button { min-height: 28px; flex: 0 0 auto; padding: 3px 8px; font-size: 10px; }
  @media (max-width: 350px) {
    .oem-candidate-row { grid-template-columns: 18px minmax(0, 1fr) 42px; gap: 5px; }
    .oem-candidate-row .oem-discovery-badge { grid-column: 2; grid-row: 2; max-width: 100%; justify-self: start; }
    .graph-candidate { grid-column: 3; grid-row: 1 / span 2; }
  }
</style>
