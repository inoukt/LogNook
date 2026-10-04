<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OemCatalogPage, OemCandidate, ScionMode21ProbeResult } from "./types";

  export let selectedPort = "";
  export let disabled = false;

  const pageSize = 50;
  let query = "";
  let brand = "";
  let model = "";
  let service = "";
  let offset = 0;
  let page: OemCatalogPage | null = null;
  let loading = false;
  let probing = false;
  let error = "";
  let probeError = "";
  let probe: ScionMode21ProbeResult | null = null;
  let searchTimer: number | undefined;
  let requestId = 0;

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
    if (!selectedPort || disabled || probing) return;
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

  function displayUnit(unit: string) {
    return ({ celsius: "°C", percent: "%", kilometersPerHour: "km/h", seconds: "s", minutes: "min" } as Record<string, string>)[unit] ?? unit;
  }
  function address(candidate: OemCandidate) {
    return `Service ${candidate.service} · ${candidate.service === "22" ? "DID" : "PID"} ${candidate.identifier} · ${candidate.request_header} → ${candidate.response_header}`;
  }
</script>

<section class="oem-research" aria-label="OEM PID research catalog">
  <div class="oem-research-heading">
    <div><h3>OEM research catalog</h3><p>Source-backed candidates for lookup; these are not added to live logging.</p></div>
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

  <div class="scion-probe-card">
    <div><strong>Scion xB 2014 · 21 01</strong><p>One source-backed read via the selected OBDLink SX. Only use on the 2AZ-FE Scion xB, stationary in Park with the parking brake set, ignition on and engine off. Close other diagnostic apps.</p></div>
    <button class="button secondary" onclick={runScionProbe} disabled={disabled || !selectedPort || probing}>
      {probing ? "Reading once…" : "Read 21 01 once"}
    </button>
  </div>
  {#if probeError}<p class="oem-error" role="alert">Probe could not start: {probeError}</p>{/if}
  {#if probe}
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

  {#if error}<p class="oem-error" role="alert">Catalog unavailable: {error}</p>
  {:else if loading && !page}<p class="oem-empty">Loading the local PID catalog…</p>
  {:else if page}
    <div class="oem-catalog-list" aria-busy={loading}>
      {#each page.candidates as candidate (candidate.candidate_id)}
        <article class="oem-candidate">
          <header><strong>{candidate.name}</strong><span>{displayUnit(candidate.unit)}</span></header>
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
  <p class="oem-footnote">Candidates remain research-only until validated on the exact vehicle. Probe evidence is saved outside the read-only catalog.</p>
</section>

<style>
  .oem-research { display: grid; flex: 1 1 auto; align-content: start; gap: 9px; min-height: 0; overflow: auto; }
  .oem-research-heading, .scion-probe-card, .probe-result > header, .oem-candidate > header, .oem-pagination { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .oem-research-heading h3 { margin: 0; font-size: 13px; }
  .oem-research-heading p, .scion-probe-card p, .probe-result p, .oem-candidate p { margin: 3px 0 0; color: var(--text-muted); font-size: 10px; line-height: 1.35; }
  .oem-count, .oem-pagination span { color: var(--text-muted); font-size: 10px; white-space: nowrap; }
  .oem-filters { display: grid; gap: 5px; }
  .oem-filter-row { display: grid; grid-template-columns: 1fr 1fr 95px; gap: 5px; }
  .oem-filters .text-input { min-width: 0; min-height: 29px; padding: 5px 7px; font-size: 10px; }
  .scion-probe-card, .probe-result { padding: 8px; border: 1px solid var(--border); border-radius: 7px; background: #142029; }
  .scion-probe-card strong, .probe-result strong { font-size: 11px; }
  .scion-probe-card .button { flex: 0 0 auto; min-height: 29px; padding: 4px 7px; font-size: 10px; }
  .oem-catalog-list { display: grid; align-content: start; gap: 5px; max-height: min(48vh, 500px); min-height: 80px; overflow: auto; padding-right: 3px; }
  .oem-candidate { padding: 7px; border: 1px solid var(--border); border-radius: 7px; background: #19242e; }
  .oem-candidate > header strong { min-width: 0; font-size: 11px; }
  .oem-candidate > header span { color: var(--text); font-size: 10px; white-space: nowrap; }
  .oem-address { color: #b8c9d3 !important; }
  .oem-badges { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 5px; }
  .oem-badges span { padding: 2px 5px; border-radius: 8px; color: #c3d2da; background: #263742; font-size: 8px; }
  .oem-candidate details, .probe-result details { margin-top: 5px; color: var(--text-muted); font-size: 10px; }
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
