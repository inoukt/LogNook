<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import type { CatalogInspection } from "./catalogInspection";
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

  export let onInspect: (inspection: CatalogInspection | null) => void = () => {};
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
  let filtersOpen = false;
  let activeFilterCount = 0;
  let inspectedId = "";
  let selectedRow: SubaruCatalogRow | null = null;
  let inspection: CatalogInspection | null = null;
  let detail: SubaruCatalogDetail | null = null;
  let detailLoading = false;
  let detailError = "";
  let detailRevision = 0;
  let searchTimer = 0;
  let searchRevision = 0;
  let destroyed = false;
  let filtersReady = false;

  $: if (filtersReady) onFiltersChange({ query, model, yearText, ecuId, protocolFamily });
  $: activeFilterCount = [model, yearText, ecuId, protocolFamily].filter((value) => value.trim()).length;
  $: selectedRow = page?.rows.find((row) => row.id === inspectedId) ?? null;
  $: inspection = selectedRow ? buildInspection(selectedRow, detail, detailLoading, detailError, disabled) : null;
  $: onInspect(inspection);

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
    detailRevision += 1;
    window.clearTimeout(searchTimer);
    onInspect(null);
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
      if (!destroyed && revision === searchRevision) {
        page = result;
        if (inspectedId && !result.rows.some((row) => row.id === inspectedId)) {
          inspectedId = "";
          detailRevision += 1;
          detail = null;
          detailLoading = false;
          detailError = "";
        }
      }
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

  async function inspectRow(row: SubaruCatalogRow) {
    if (inspectedId === row.id && (detail?.row.id === row.id || detailLoading)) return;

    const revision = ++detailRevision;
    inspectedId = row.id;
    detail = null;
    detailError = "";
    detailLoading = true;
    try {
      const result = await getSubaruCatalogDetail(row.id);
      if (!destroyed && revision === detailRevision && inspectedId === row.id) detail = result;
    } catch (caught) {
      if (!destroyed && revision === detailRevision && inspectedId === row.id) detailError = String(caught);
    } finally {
      if (!destroyed && revision === detailRevision && inspectedId === row.id) detailLoading = false;
    }
  }

  function buildInspection(
    row: SubaruCatalogRow,
    sourceDetail: SubaruCatalogDetail | null,
    loadingDetail: boolean,
    detailFailure: string,
    isDisabled: boolean,
  ): CatalogInspection {
    const loaded = sourceDetail?.row.id === row.id ? sourceDetail : null;
    const detailRow = loaded?.row ?? row;
    const decoder = row.decoder_supported ? "Decoded" : row.protocol === "SSM-KWP" && row.can_graph ? "Raw code" : "Raw only";
    const ssmAddress = loaded?.source_did || row.source_did || row.request_identifier || "Unavailable";
    const fields: CatalogInspection["fields"] = [
      { label: "ECU", value: row.ecu_name + " · " + row.ecu_id },
      { label: "Protocol", value: row.protocol },
      { label: "Request", value: (row.request_service || "Service unavailable") + " · " + (row.request_identifier || row.source_did || "Identifier unavailable") },
      { label: "Addressing", value: (row.request_header || "no TX ID") + " → " + (row.response_header || "no RX ID") },
      { label: "Decoder", value: decoder },
    ];
    if (row.protocol === "SSM-KWP") fields.push({ label: "SSM address", value: ssmAddress + " · OpenPort 2 K-Line" });
    if (loaded) fields.push({ label: "Signal ID", value: loaded.signal_id || loaded.source_pid_id || "Unavailable" });

    const provenance = [
      "Request hint: " + (detailRow.request_hint || "none"),
      "Mapping: " + detailRow.mapping_state.replaceAll("_", " "),
      "Auto-request: " + (row.auto_request ? "Enabled" : "Disabled"),
      "Source: " + (detailRow.source_display_name || "Unavailable"),
      "Source revision: " + (detailRow.source_revision || "revision unavailable"),
      "Catalog SHA-256: " + (detailRow.catalog_sha256 || "Unavailable"),
      "Source table and DID: " + (detailRow.source_table || "Unavailable") + " · " + (loaded?.source_did || row.source_did || "Unavailable"),
      !row.can_graph && row.graph_reason ? "Add to Live unavailable: " + row.graph_reason : "",
      !row.can_manual_read && row.manual_read_reason ? "CAN Workbench unavailable: " + row.manual_read_reason : "",
    ].filter(Boolean).join("\n");
    const blocks: NonNullable<CatalogInspection["blocks"]> = [
      { label: "Vehicle applicability", value: formatSubaruVehicleScope(row.vehicle_scope_json) },
      { label: "Source and request provenance", value: provenance },
    ];
    if (loaded) {
      blocks.push(
        { label: "Decoder payload", value: prettyJson(loaded.decoder_json) },
        { label: "Raw source row", value: prettyJson(loaded.raw_source_json) },
        { label: "ECU mapping provenance", value: prettyJson(loaded.link_metadata_json) },
      );
    }
    const note = loadingDetail
      ? "Loading detailed source metadata…"
      : detailFailure
        ? "Source details unavailable: " + detailFailure
        : undefined;

    return {
      id: row.id,
      name: row.name || row.request_hint || row.id,
      subtitle: row.ecu_name + " · " + row.protocol,
      status: row.vehicle_validation.replaceAll("_", " ") || "Validation unavailable",
      tone: "neutral",
      note,
      fields,
      blocks,
      actions: [
        {
          label: "Add to Live",
          run: () => onAddToSignals(row),
          disabled: isDisabled || !row.can_graph,
          title: isDisabled ? "Finish the current operation before adding this signal." : row.can_graph ? "Add to the Live picker, then select Log to start logging." : row.graph_reason,
        },
        {
          label: "CAN Workbench",
          run: () => onSelect(row),
          disabled: isDisabled || !row.can_manual_read,
          title: isDisabled ? "Finish the current operation before preparing a CAN request." : row.can_manual_read ? "Prepare this request in the CAN Workbench." : row.manual_read_reason,
        },
      ],
    };
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

  <div class="catalog-search-row">
    <input class="text-input" type="search" bind:value={query} oninput={scheduleSearch} placeholder="Name, DID, ECU or protocol" aria-label="Search Subaru catalog" />
    <button class="button secondary filters-toggle" type="button" onclick={() => filtersOpen = !filtersOpen} aria-expanded={filtersOpen} aria-controls="subaru-catalog-filters">
      Filters{#if activeFilterCount}<span class="filter-count">{activeFilterCount}</span>{/if}
    </button>
  </div>
  {#if filtersOpen}
    <div id="subaru-catalog-filters" class="catalog-filters">
      <input class="text-input" bind:value={model} oninput={scheduleSearch} placeholder="Model" aria-label="Filter Subaru model" />
      <input class="text-input year-filter" inputmode="numeric" maxlength="4" bind:value={yearText} oninput={scheduleSearch} placeholder="Year" aria-label="Filter Subaru model year" />
      <input class="text-input ecu-filter" bind:value={ecuId} oninput={scheduleSearch} placeholder="ECU ID or name" aria-label="Filter Subaru ECU" />
      <input class="text-input protocol-filter" bind:value={protocolFamily} oninput={scheduleSearch} placeholder="Protocol family" aria-label="Filter Subaru protocol family" />
    </div>
  {/if}

  {#if error}<p class="catalog-error" role="alert">Subaru catalog: {error}</p>{/if}
  {#if loading && !page}<p class="catalog-empty">Loading Subaru research rows…</p>
  {:else if page?.rows.length}
    <div class="catalog-rows" aria-busy={loading}>
      {#each page.rows as row (row.id)}
        {@const scopePreview = previewSubaruVehicleScope(row.vehicle_scope_json)}
        <article class="catalog-row" class:selected={inspectedId === row.id}>
          <div class="row-main">
            <button class="row-select" type="button" onclick={() => void inspectRow(row)} aria-pressed={inspectedId === row.id} aria-label={"Inspect " + (row.name || row.request_hint || row.id)}>
              <span class="row-title"><strong>{row.name || row.request_hint || row.id}</strong><span class="unit">{row.unit || "—"}</span></span>
              <span class="route" title={row.ecu_name + " · ECU " + row.ecu_id + " · " + row.protocol + " · " + (row.request_header || "no TX ID") + " → " + (row.response_header || "no RX ID")}>
                {row.ecu_name} · ECU {row.ecu_id} · {row.protocol} · {row.request_header || "no TX ID"} → {row.response_header || "no RX ID"}
              </span>
            </button>
            <div class="row-meta">
              <span class="scope" title={formatSubaruVehicleScope(row.vehicle_scope_json)}>
                {scopePreview.text}{scopePreview.remaining ? " · + " + scopePreview.remaining + " more" : ""}
              </span>
              <div class="row-status">
                <span>{row.vehicle_validation.replaceAll("_", " ")}</span>
                <span>{row.decoder_supported ? "Decoded" : row.protocol === "SSM-KWP" && row.can_graph ? "Raw code" : "Raw only"}</span>
              </div>
            </div>
          </div>
          <button class="button secondary add-live" type="button" onclick={() => onAddToSignals(row)} disabled={disabled || !row.can_graph} title={row.can_graph ? "Add to the Live picker, then select Log to start logging." : row.graph_reason}>
            Add to Live
          </button>
        </article>
      {/each}
    </div>
    <footer class="catalog-pages">
      <button class="button secondary" type="button" onclick={() => setPage(offset - pageSize)} disabled={offset === 0 || loading}>Previous</button>
      <span>{offset + 1}–{Math.min(offset + page.rows.length, page.total)} of {page.total}</span>
      <button class="button secondary" type="button" onclick={() => setPage(offset + pageSize)} disabled={offset + pageSize >= page.total || loading}>Next</button>
    </footer>
  {:else if page}
    <p class="catalog-empty">No Subaru rows match these filters.</p>
  {/if}
</section>

<style>
  .subaru-catalog { display: flex; flex: 1 1 0; min-width: 0; min-height: 0; flex-direction: column; gap: 5px; overflow: hidden; color: var(--text); }
  .catalog-heading, .catalog-row, .catalog-pages { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .catalog-heading { flex-wrap: wrap; gap: 3px; min-width: 0; font-size: 11px; }
  .catalog-heading > div { min-width: 0; }
  .catalog-heading small { display: block; color: var(--text-muted); font-size: 9px; }
  .catalog-count, .catalog-pages { color: var(--text-muted); font-size: 10px; }
  .catalog-search-row { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .catalog-search-row .text-input { flex: 1 1 auto; min-width: 0; height: 28px; padding: 0 7px; font-size: 11px; }
  .filters-toggle { flex: 0 0 auto; min-height: 28px; padding: 3px 7px; font-size: 10px; }
  .filter-count { display: inline-grid; min-width: 16px; height: 16px; place-items: center; margin-left: 4px; padding: 0 4px; border-radius: 9px; color: var(--text); background: var(--surface); font-size: 10px; font-variant-numeric: tabular-nums; }
  .catalog-filters { display: grid; flex: 0 0 auto; min-width: 0; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; }
  .catalog-filters .text-input { width: 100%; min-width: 0; height: 28px; padding: 0 7px; font-size: 10px; }
  .catalog-rows { display: flex; flex: 1 1 0; min-width: 0; min-height: 0; flex-direction: column; gap: 4px; overflow-x: hidden; overflow-y: auto; scrollbar-gutter: stable; padding-right: 2px; }
  .catalog-row { flex: 0 0 auto; min-width: 0; min-height: 54px; gap: 6px; border: 1px solid var(--border); border-radius: 6px; padding: 5px 6px; background: var(--surface-deep); }
  .catalog-row.selected { border-color: var(--border-strong); background: var(--surface-raised); }
  .row-main { flex: 1 1 auto; display: grid; min-width: 0; gap: 4px; }
  .row-select { display: grid; min-width: 0; width: 100%; gap: 2px; padding: 0; border: 0; border-radius: 4px; color: var(--text); background: transparent; text-align: left; cursor: pointer; }
  .row-select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .row-title { display: flex; min-width: 0; align-items: baseline; gap: 7px; }
  .row-title strong { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; font-size: 12px; line-height: 1.25; }
  .unit { flex: 0 0 auto; max-width: 30%; overflow: hidden; color: var(--text-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .route { display: block; min-width: 0; margin: 0; overflow: hidden; color: var(--text-muted); font-size: 10px; line-height: 1.25; white-space: nowrap; text-overflow: ellipsis; }
  .row-meta { display: flex; min-width: 0; align-items: center; justify-content: space-between; gap: 4px 8px; }
  .scope { flex: 1 1 60px; min-width: 0; overflow: hidden; color: var(--text-muted); font-size: 10px; white-space: nowrap; text-overflow: ellipsis; }
  .row-status { display: flex; flex: 0 0 auto; flex-wrap: wrap; gap: 4px; min-width: 0; color: var(--text-muted); font-size: 10px; }
  .row-status span { padding: 1px 5px; border-radius: 4px; background: var(--surface); }
  .add-live { flex: 0 0 auto; min-height: 26px; padding: 3px 7px; font-size: 10px; white-space: nowrap; }
  .catalog-error { color: var(--status-warning, #f5b664); }
  .catalog-empty, .catalog-error { margin: 3px 0; font-size: 10px; }
  .catalog-empty { color: var(--text-muted); }
  .catalog-pages { flex: 0 0 auto; min-width: 0; gap: 4px; justify-content: center; }
  .catalog-pages .button { min-width: 0; min-height: 26px; padding: 3px 7px; font-size: 10px; }
  .catalog-pages span { white-space: nowrap; }
</style>
