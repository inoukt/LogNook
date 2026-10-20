<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    getSubaruCatalogDetail,
    formatSubaruVehicleScope,
    previewSubaruVehicleScope,
    searchSubaruCatalog,
    type SubaruCatalogDetail,
    type SubaruCatalogFilter,
    type SubaruCatalogPage,
    type SubaruCatalogRow,
  } from "./subaruCatalog";

  export let onSelect: (row: SubaruCatalogRow) => void = () => {};
  export let onAddToSignals: (row: SubaruCatalogRow) => void = () => {};
  export let disabled = false;
  export let savedFilters: Record<string, unknown> = {};
  export let onFiltersChange: (filters: Record<string, unknown>) => void = () => {};

  const pageSize = 50;
  let query = "";
  let model = "";
  let yearText = "";
  let ecuId = "";
  let protocolFamily = "";
  let offset = 0;
  let page: SubaruCatalogPage | null = null;
  let loading = false;
  let error = "";
  let expandedId = "";
  let expandedScopeId = "";
  let detail: SubaruCatalogDetail | null = null;
  let detailLoading = false;
  let searchTimer = 0;
  let searchRevision = 0;
  let destroyed = false;
  let filtersReady = false;

  $: if (filtersReady) onFiltersChange({ query, model, yearText, ecuId, protocolFamily });

  onMount(() => {
    if (typeof savedFilters.query === "string") query = savedFilters.query;
    if (typeof savedFilters.model === "string") model = savedFilters.model;
    if (typeof savedFilters.yearText === "string") yearText = savedFilters.yearText;
    if (typeof savedFilters.ecuId === "string") ecuId = savedFilters.ecuId;
    if (typeof savedFilters.protocolFamily === "string") protocolFamily = savedFilters.protocolFamily;
    filtersReady = true;
    void loadPage();
  });
  onDestroy(() => {
    destroyed = true;
    searchRevision += 1;
    window.clearTimeout(searchTimer);
  });

  function filter(): SubaruCatalogFilter {
    const year = yearText.trim();
    return {
      query: query.trim(),
      model: model.trim(),
      year: /^\d{4}$/.test(year) ? Number(year) : null,
      ecuId: ecuId.trim(),
      protocolFamily: protocolFamily.trim(),
      offset,
      limit: pageSize,
    };
  }

  function scheduleSearch() {
    offset = 0;
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void loadPage(), 220);
  }

  async function loadPage() {
    const revision = ++searchRevision;
    loading = true;
    error = "";
    try {
      const result = await searchSubaruCatalog(filter());
      if (!destroyed && revision === searchRevision) page = result;
    } catch (caught) {
      if (!destroyed && revision === searchRevision) error = String(caught);
    } finally {
      if (!destroyed && revision === searchRevision) loading = false;
    }
  }

  function setPage(nextOffset: number) {
    if (nextOffset < 0 || nextOffset >= (page?.total ?? 0)) return;
    offset = nextOffset;
    void loadPage();
  }

  async function toggleDetails(row: SubaruCatalogRow) {
    if (expandedId === row.id) {
      expandedId = "";
      detail = null;
      return;
    }
    expandedId = row.id;
    detail = null;
    detailLoading = true;
    try {
      const result = await getSubaruCatalogDetail(row.id);
      if (!destroyed && expandedId === row.id) detail = result;
    } catch (caught) {
      if (!destroyed && expandedId === row.id) error = String(caught);
    } finally {
      if (!destroyed && expandedId === row.id) detailLoading = false;
    }
  }

  function prettyJson(value: string) {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }

</script>

<section class="subaru-catalog" aria-label="Subaru OEM catalog">
  <header class="catalog-heading">
    <div>
      <strong>Subaru OEM catalog</strong>
      <small>Vendor-origin reference · research only · no automatic requests</small>
    </div>
    {#if page}<span class="catalog-count">{page.total.toLocaleString()} rows</span>{/if}
  </header>

  <div class="catalog-filters">
    <input class="text-input" type="search" bind:value={query} oninput={scheduleSearch} placeholder="Name, DID, ECU or protocol" aria-label="Search Subaru catalog" />
    <input class="text-input" bind:value={model} oninput={scheduleSearch} placeholder="Model" aria-label="Filter Subaru model" />
    <input class="text-input year-filter" inputmode="numeric" maxlength="4" bind:value={yearText} oninput={scheduleSearch} placeholder="Year" aria-label="Filter Subaru model year" />
    <input class="text-input ecu-filter" bind:value={ecuId} oninput={scheduleSearch} placeholder="ECU ID or name" aria-label="Filter Subaru ECU" />
    <input class="text-input protocol-filter" bind:value={protocolFamily} oninput={scheduleSearch} placeholder="Protocol family" aria-label="Filter Subaru protocol family" />
  </div>

  {#if error}<p class="catalog-error" role="alert">Subaru catalog: {error}</p>{/if}
  {#if loading && !page}<p class="catalog-empty">Loading Subaru research rows…</p>
  {:else if page?.rows.length}
    <div class="catalog-rows" aria-busy={loading}>
      {#each page.rows as row (row.id)}
        {@const scopePreview = previewSubaruVehicleScope(row.vehicle_scope_json)}
        <article class="catalog-row">
          <header>
            <strong>{row.name || row.request_hint || row.id}</strong>
            <span class="unit">{row.unit || "—"}</span>
          </header>
          <p class="route" title={`${row.ecu_name} · ECU ${row.ecu_id} · ${row.protocol}`}>{row.ecu_name} · {row.protocol} · {row.request_header || "no TX ID"} → {row.response_header || "no RX ID"}</p>
          <div class="row-actions">
            <button class="button secondary" onclick={() => onAddToSignals(row)} disabled={disabled || !row.can_graph} title={row.can_graph ? "Add to the Live picker, then select Log to start logging." : row.graph_reason}>Add to Live</button>
            <button class="button secondary" onclick={() => onSelect(row)} disabled={disabled || !row.can_manual_read} title={row.can_manual_read ? "Prepare this request in the CAN Workbench." : row.manual_read_reason}>CAN Workbench</button>
          </div>
          <div class="scope-row" class:expanded={expandedScopeId === row.id}>
            <span class="scope" title={scopePreview.text}>{expandedScopeId === row.id ? formatSubaruVehicleScope(row.vehicle_scope_json) : scopePreview.text}</span>
            {#if scopePreview.remaining > 0}
              <button
                class="scope-more"
                onclick={() => expandedScopeId = expandedScopeId === row.id ? "" : row.id}
                aria-expanded={expandedScopeId === row.id}
                aria-label={expandedScopeId === row.id ? "Show less vehicle applicability" : `Show ${scopePreview.remaining} more vehicle applicability entries`}
              >{expandedScopeId === row.id ? "Less" : `+ ${scopePreview.remaining} more`}</button>
            {/if}
          </div>
          <div class="row-footer">
            <div class="row-status">
              <span>{row.vehicle_validation.replaceAll("_", " ")}</span>
              <span>{row.decoder_supported ? "Decoded" : row.protocol === "SSM-KWP" && row.can_graph ? "Raw code" : "Raw only"}</span>
            </div>
            <button class="details-toggle" onclick={() => void toggleDetails(row)} aria-expanded={expandedId === row.id}>
              {expandedId === row.id ? "Less details" : "Details"}
            </button>
          </div>
          {#if expandedId === row.id}
            {#if detailLoading}<p class="catalog-empty">Loading source details…</p>
            {:else if detail}
              <div class="details">
                {#if !row.can_graph && row.graph_reason}<small class="disabled-reason">Graph: {row.graph_reason}</small>{/if}
                {#if !row.can_manual_read && row.manual_read_reason}<small class="disabled-reason">CAN: {row.manual_read_reason}</small>{/if}
                <dl>
                  <dt>ECU</dt><dd>{row.ecu_name} · {row.ecu_id}</dd>
                  <dt>Request hint</dt><dd>{row.request_hint || "none"} · {row.mapping_state.replaceAll("_", " ")}</dd>
                  {#if row.protocol === "SSM-KWP"}<dt>SSM address</dt><dd>{row.source_did || row.request_identifier || "unavailable"} · OpenPort 2 K-Line</dd>{/if}
                  <dt>Auto-request</dt><dd>{row.auto_request ? "Enabled" : "Disabled"}</dd>
                  <dt>Source</dt><dd>{detail.row.source_display_name} · {detail.row.source_revision}</dd>
                  <dt>Catalog SHA-256</dt><dd class="hash">{detail.row.catalog_sha256}</dd>
                  <dt>Source table and DID</dt><dd>{detail.row.source_table} · {detail.source_did}</dd>
                  <dt>Request identity</dt><dd>{detail.row.request_hint} · mapping {detail.row.mapping_state}</dd>
                  <dt>Signal ID</dt><dd>{detail.signal_id || detail.source_pid_id}</dd>
                </dl>
                <details><summary>Decoder payload</summary><pre>{prettyJson(detail.decoder_json)}</pre></details>
                <details><summary>Raw source row</summary><pre>{prettyJson(detail.raw_source_json)}</pre></details>
                <details><summary>ECU mapping provenance</summary><pre>{prettyJson(detail.link_metadata_json)}</pre></details>
              </div>
            {/if}
          {/if}
        </article>
      {/each}
    </div>
    <footer class="catalog-pages">
      <button class="button secondary" onclick={() => setPage(offset - pageSize)} disabled={offset === 0 || loading}>Previous</button>
      <span>{offset + 1}–{Math.min(offset + page.rows.length, page.total)} of {page.total}</span>
      <button class="button secondary" onclick={() => setPage(offset + pageSize)} disabled={offset + pageSize >= page.total || loading}>Next</button>
    </footer>
  {:else if page}
    <p class="catalog-empty">No Subaru rows match these filters.</p>
  {/if}
</section>

<style>
  .subaru-catalog { display: flex; flex: 1 1 0; min-width: 0; min-height: 0; flex-direction: column; gap: 5px; overflow: hidden; color: var(--text); }
  .catalog-heading, .catalog-row > header, .catalog-pages { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .catalog-heading { flex-wrap: wrap; gap: 3px; min-width: 0; font-size: 11px; }
  .catalog-heading > div { min-width: 0; }
  .catalog-heading small { display: block; color: var(--text-muted); font-size: 9px; }
  .catalog-count, .unit, .scope, .route, .catalog-pages { color: var(--text-muted); font-size: 9px; }
  .catalog-filters { display: grid; flex: 0 0 auto; min-width: 0; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 3px; }
  .catalog-filters > input:first-child { grid-column: 1 / -1; }
  .catalog-filters .text-input { width: 100%; min-width: 0; height: 26px; padding: 0 6px; font-size: 10px; }
  .catalog-rows { display: flex; flex: 1 1 0; min-width: 0; min-height: 0; flex-direction: column; gap: 4px; overflow-x: hidden; overflow-y: auto; scrollbar-gutter: stable; padding-right: 2px; }
  .catalog-row { display: grid; flex: 0 0 auto; min-width: 0; gap: 4px; border: 1px solid var(--border); border-radius: 6px; padding: 6px; background: var(--surface-deep); }
  .catalog-row > header { min-width: 0; align-items: baseline; gap: 5px; }
  .catalog-row > header strong { flex: 1 1 0; min-width: 0; overflow-wrap: anywhere; font-size: 11px; line-height: 1.3; }
  .unit { flex: 0 0 auto; max-width: 30%; overflow-wrap: anywhere; }
  .route { min-width: 0; margin: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .row-actions { display: grid; min-width: 0; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; }
  .row-actions .button, .catalog-pages .button { min-width: 0; min-height: 24px; padding: 3px 5px; font-size: 9px; white-space: normal; }
  .scope-row { display: flex; min-width: 0; align-items: baseline; gap: 4px; }
  .scope { flex: 1 1 0; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .scope-row.expanded { flex-wrap: wrap; }
  .scope-row.expanded .scope { flex-basis: 100%; white-space: normal; overflow-wrap: anywhere; line-height: 1.45; }
  .scope-more, .details-toggle { flex: 0 0 auto; padding: 0; border: 0; background: transparent; color: var(--accent); cursor: pointer; font: inherit; font-size: 9px; white-space: nowrap; }
  .row-footer { display: flex; min-width: 0; align-items: center; justify-content: space-between; gap: 4px; }
  .row-status { display: flex; flex-wrap: wrap; gap: 3px; min-width: 0; color: var(--text-muted); font-size: 8px; }
  .row-status span { padding: 1px 4px; border-radius: 4px; background: var(--surface-raised); }
  .disabled-reason, .catalog-error { color: var(--status-warning, #f5b664); }
  .disabled-reason { display: block; margin-bottom: 5px; font-size: 9px; overflow-wrap: anywhere; }
  .details { min-width: 0; padding-top: 5px; border-top: 1px solid var(--border); }
  dl { display: grid; grid-template-columns: minmax(0, 80px) minmax(0, 1fr); gap: 3px 5px; margin: 0 0 5px; font-size: 9px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .hash { font-family: var(--font-mono, monospace); }
  pre { max-height: 180px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; border-radius: 5px; padding: 5px; background: var(--surface-raised); font-size: 9px; }
  .details details { min-width: 0; color: var(--text-muted); font-size: 9px; }
  .catalog-empty, .catalog-error { margin: 3px 0; font-size: 10px; }
  .catalog-empty { color: var(--text-muted); }
  .catalog-pages { flex: 0 0 auto; min-width: 0; gap: 4px; }
</style>
