<script lang="ts">
  import { A30_READ_TESTS } from "./a30ReadTests";
  import { adjustResearchResultsHeight } from "./researchResultsResize";
  import type { ResearchRunViewState, ResearchRunRow, ResearchTestSelection } from "./types";

  export let state: ResearchRunViewState;
  export let exportDisabled = false;
  export let exportPath = "";
  export let onExport: () => void = () => {};
  let collapsed = false;
  let resultsHeight = 190;
  let viewportHeight = window.innerHeight;
  let resultsPanelElement: HTMLElement | undefined;
  let resultsResize: { y: number; height: number } | null = null;

  function startResultsResize(event: PointerEvent) {
    const splitter = event.currentTarget as HTMLElement | null;
    if (!resultsPanelElement || !splitter || collapsed) return;
    resultsResize = { y: event.clientY, height: resultsPanelElement.getBoundingClientRect().height };
    splitter.setPointerCapture(event.pointerId);
  }

  function resizeResults(event: PointerEvent) {
    if (!resultsResize) return;
    resultsHeight = adjustResearchResultsHeight(resultsResize.height, resultsResize.y - event.clientY, viewportHeight);
  }

  function finishResultsResize() {
    resultsResize = null;
  }

  function resizeResultsFromKeyboard(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const height = resultsPanelElement?.getBoundingClientRect().height ?? resultsHeight;
    resultsHeight = adjustResearchResultsHeight(height, event.key === "ArrowUp" ? 16 : -16, viewportHeight);
  }

  function label(selection: ResearchTestSelection) {
    if (selection.type === "oemCandidate") return selection.payload.name;
    return A30_READ_TESTS.find(([id]) => id === selection.payload)?.[1] ?? selection.payload;
  }

  function hasMultipleResponders(row: ResearchRunRow) {
    return row.result?.type === "a30ReadTest"
      && row.result.payload.outcome === "wrong_responder"
      && row.result.payload.detail?.startsWith("Multiple ECU responders were returned") === true;
  }

  function outcome(row: ResearchRunRow) {
    if (hasMultipleResponders(row)) return "multiple ECU responders";
    return row.result?.payload.outcome.replaceAll("_", " ") ?? "—";
  }

  function latency(row: ResearchRunRow) {
    return row.result ? `${row.result.payload.latency_ms} ms` : "—";
  }

  function responder(row: ResearchRunRow) {
    if (hasMultipleResponders(row)) return "Multiple ECUs";
    return row.result?.payload.responder_header ?? "No responding ECU";
  }

  function values(row: ResearchRunRow) {
    if (row.result?.type === "oemCandidate") {
      const value = row.result.payload.decoded_value;
      if (!value) return row.result.payload.decode_error ?? "No decoded value";
      return value.enum_value ?? (value.value === null ? `Raw ${value.raw_value}` : `${value.value} ${value.unit}`);
    }
    if (row.result?.type === "a30ReadTest") {
      const graphs = row.result.payload.graph_values;
      return graphs.length ? graphs.map(({ name, value, unit, raw_value }) => `${name}: ${value} ${unit} · raw ${raw_value}`).join("; ") : row.result.payload.detail ?? "No numeric channels";
    }
    return row.error ?? "—";
  }

  function diagnostics(row: ResearchRunRow) {
    if (row.error) return row.error;
    if (row.result?.type === "oemCandidate") {
      const result = row.result.payload;
      return [
        result.error,
        result.decode_error,
        result.decoded_value?.within_catalog_range === false ? "Value is outside the catalog range; graph quality is suspect." : null,
        result.evidence_save_error,
      ].filter(Boolean).join(" · ");
    }
    if (row.result?.type === "a30ReadTest") {
      return [row.result.payload.detail, row.result.payload.evidence_save_error].filter(Boolean).join(" · ");
    }
    return "";
  }

  function command(row: ResearchRunRow) {
    return row.result?.payload.exact_adapter_command ?? "";
  }

  function rawResponse(row: ResearchRunRow) {
    return row.result?.payload.raw_response ?? "";
  }

  function nrc(row: ResearchRunRow) {
    if (!row.result) return null;
    return row.result.payload.negative_response_code;
  }
</script>

<svelte:window bind:innerHeight={viewportHeight} />

<section class="research-run-results" class:collapsed bind:this={resultsPanelElement} style={`--research-results-height:${resultsHeight}px`} aria-label="Research run results">
  {#if !collapsed}
    <div class="can-monitor-splitter research-results-splitter" role="slider" tabindex="0" aria-orientation="vertical"
      aria-valuemin={Math.min(120, Math.min(360, viewportHeight * 0.4))}
      aria-valuemax={Math.min(360, viewportHeight * 0.4)}
      aria-valuenow={Math.round(adjustResearchResultsHeight(resultsHeight, 0, viewportHeight))}
      aria-label="Research results height"
      onpointerdown={startResultsResize} onpointermove={resizeResults} onpointerup={finishResultsResize}
      onpointercancel={finishResultsResize} onlostpointercapture={finishResultsResize} onkeydown={resizeResultsFromKeyboard}></div>
  {/if}
  <header class="research-results-heading">
    <div>
      <h3>Research run results</h3>
      <p aria-live="polite">
        {#if state.running}
          {state.completedCount} of {state.totalCount} completed · Running {state.currentItem}
        {:else if state.totalCount}
          {state.completedCount} of {state.totalCount} completed
        {:else}
          Completed requests and diagnostics appear here.
        {/if}
      </p>
    </div>
    <button class="research-collapse" type="button" aria-expanded={!collapsed} aria-label={collapsed ? "Expand results" : "Collapse results"} onclick={() => collapsed = !collapsed}>{collapsed ? "▾" : "▴"}</button>
    <button class="button secondary" onclick={onExport} disabled={exportDisabled || state.running || !state.report}>Export run report</button>
  </header>
  {#if exportPath}<p class="research-export-path" role="status">Report saved: {exportPath}</p>{/if}
  {#if !collapsed}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -- keyboard users need to focus this independently scrolling results region. -->
    <div class="research-results-scroll" role="region" aria-label="Per-request results" tabindex="0">
      <table>
        <thead><tr><th>Test</th><th>Status / outcome</th><th>Latency</th><th>ECU</th><th>Value</th><th>Diagnostic</th><th>Evidence</th></tr></thead>
        <tbody>
          {#each state.rows as row (row.queue_index)}
            <tr>
              <td>{label(row.selection)}<small>{row.queue_index} / {row.queue_size}</small></td>
              <td><span class={`research-result-state ${row.status}`}>{row.status}</span>{#if row.result}<small>{outcome(row)}</small>{/if}</td>
              <td class="research-latency">{latency(row)}</td>
              <td>{responder(row)}</td>
              <td>{values(row)}</td>
              <td class:research-error={Boolean(diagnostics(row))}>{diagnostics(row) || "—"}</td>
              <td>
                {#if row.result}
                  <details>
                    <summary>Details</summary>
                    <code>{command(row)}</code>
                    <pre>{rawResponse(row) || "No raw response"}</pre>
                    {#if nrc(row) !== null}<small>NRC 0x{nrc(row)?.toString(16).toUpperCase().padStart(2, "0")}</small>{/if}
                    {#if row.result.payload.evidence_saved}<small>Raw evidence saved outside the catalog.</small>{:else if row.result.payload.evidence_save_error}<small class="research-error">Evidence save failed: {row.result.payload.evidence_save_error}</small>{/if}
                    {#if row.selection.type === "oemCandidate"}
                      <small>Source {row.selection.payload.source_id} · SHA {row.selection.payload.source_blob_sha} · {row.selection.payload.license ?? "License not listed"}</small>
                      {#if row.selection.payload.source_url}<a href={row.selection.payload.source_url} target="_blank" rel="noreferrer">Source</a>{/if}
                    {:else if row.result.type === "a30ReadTest"}
                      <small>{row.result.payload.request_description}</small>
                    {/if}
                  </details>
                {:else}
                  <span>—</span>
                {/if}
              </td>
            </tr>
          {:else}
            <tr><td class="research-empty" colspan="7">No workbench requests have run yet.</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .research-run-results:not(.collapsed) { display: flex; flex: 0 1 auto; flex-direction: column; height: clamp(min(120px, 40vh), var(--research-results-height, 190px), min(360px, 40vh)); max-height: min(360px, 40vh); min-width: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 9px; background: var(--surface); }
  .research-run-results.collapsed { display: flex; flex: 0 0 auto; flex-direction: column; height: auto; min-width: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 9px; background: var(--surface); }
  .research-results-heading { display: flex; flex: 0 0 auto; align-items: center; justify-content: space-between; gap: 10px; padding: 8px 10px; border-bottom: 1px solid var(--border); }
  .research-results-heading h3 { margin: 0; font-size: 12px; }
  .research-results-heading p { margin: 3px 0 0; color: var(--text-muted); font-size: 10px; }
  .research-results-heading .button { min-height: 29px; flex: 0 0 auto; padding: 4px 8px; font-size: 10px; }
  .research-collapse { width: 25px; height: 25px; flex: 0 0 25px; border: 1px solid var(--border-strong); border-radius: 5px; color: var(--text-secondary); background: var(--surface-raised); cursor: pointer; }
  .research-collapse:hover { color: var(--accent); border-color: var(--accent); }
  .research-results-splitter { flex: 0 0 8px; }
  .research-export-path { flex: 0 0 auto; margin: 0; padding: 3px 10px; color: var(--text-muted); font-size: 9px; overflow-wrap: anywhere; }
  .research-results-scroll { flex: 1 1 auto; min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; }
  .research-results-scroll table { width: 100%; border-collapse: collapse; font-size: 10px; text-align: left; }
  .research-results-scroll th { position: sticky; top: 0; z-index: 1; padding: 5px 8px; color: var(--text-muted); background: var(--surface-top); font-size: 9px; font-weight: 650; white-space: nowrap; }
  .research-results-scroll td { max-width: 260px; padding: 5px 8px; border-top: 1px solid var(--border); color: var(--text-secondary); overflow-wrap: anywhere; vertical-align: top; }
  .research-results-scroll td:first-child { color: var(--text); font-weight: 600; }
  .research-results-scroll small { display: block; margin-top: 3px; color: var(--text-muted); font-size: 9px; }
  .research-result-state { display: inline-block; padding: 2px 5px; border-radius: 8px; color: var(--text-muted); background: #25323c; text-transform: capitalize; white-space: nowrap; }
  .research-result-state.completed { color: #b9e1d4; background: #1c3933; }
  .research-result-state.error { color: #ffd0cb; background: #492825; }
  .research-result-state.skipped { color: #e5cc9c; background: #3e3524; }
  .research-result-state.running { color: #d0e3ff; background: #24384f; }
  .research-latency { white-space: nowrap; font-variant-numeric: tabular-nums; }
  .research-error { color: #ffd0cb !important; }
  .research-results-scroll details { min-width: 100px; color: var(--text-muted); }
  .research-results-scroll summary { cursor: pointer; }
  .research-results-scroll code { display: block; margin-top: 5px; color: var(--text); font-size: 9px; overflow-wrap: anywhere; }
  .research-results-scroll pre { max-width: 360px; margin: 4px 0; color: var(--text-secondary); font: 9px/1.35 ui-monospace, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
  .research-empty { color: var(--text-muted) !important; text-align: center; }
</style>
