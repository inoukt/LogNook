<script lang="ts">
  import { onMount } from "svelte";
  import { compareCanEventSnapshots } from "./canEventSnapshots";
  import type { CanEventSnapshotCell } from "./canEventSnapshots";
  import type { CanLiveEventGroup } from "./types";

  export let group: CanLiveEventGroup;
  export let onClose: () => void;

  let dialog: HTMLDialogElement;
  let idFilter = "";
  $: windows = [...group.windows].sort((a, b) => a.sequence - b.sequence);
  $: rows = compareCanEventSnapshots(group);
  $: matchingRows = rows.filter((row) => matchesId(row.can_id, row.extended, idFilter));
  $: sortedRows = [...matchingRows].sort((a, b) =>
    b.exact_recurrence_percent - a.exact_recurrence_percent
    || totalFrames(b.cells) - totalFrames(a.cells)
    || Number(a.extended) - Number(b.extended)
    || a.can_id - b.can_id).slice(0, 100);

  onMount(() => dialog.showModal());

  function close() {
    if (dialog.open) dialog.close();
    else onClose();
  }

  function matchesId(can_id: number, extended: boolean, query: string): boolean {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return true;
    const hex = can_id.toString(16).toLowerCase();
    return `0x${hex.padStart(extended ? 8 : 3, "0")}`.includes(normalized)
      || `0x${hex}`.includes(normalized)
      || (extended ? "29-bit" : "11-bit").includes(normalized);
  }

  function totalFrames(cells: CanEventSnapshotCell[]): number {
    return cells.reduce((total, cell) => total + (cell.state === "observed" ? cell.frame_count : 0), 0);
  }

  function canId(can_id: number, extended: boolean): string {
    return `0x${can_id.toString(16).toUpperCase().padStart(extended ? 8 : 3, "0")} · ${extended ? "29" : "11"}-bit`;
  }

  function payloadText(payload: number[]): string {
    return payload.map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ") || "(empty payload)";
  }

  function cellLabel(cell: CanEventSnapshotCell): string {
    return cell.state === "observed" ? cell.relation : cell.state;
  }
</script>

<dialog
  bind:this={dialog}
  class="can-snapshot-dialog"
  aria-labelledby="can-snapshot-title"
  oncancel={(event) => { event.preventDefault(); close(); }}
  onclose={onClose}
>
  <header class="snapshot-heading">
    <div>
      <p class="eyebrow">CAN EVENT SNAPSHOTS</p>
      <h2 id="can-snapshot-title">Compare snapshots · {group.label}</h2>
      <p class="snapshot-summary">{group.completed_events} complete · {group.pending_events} pending · {group.incomplete_events} incomplete marks</p>
    </div>
    <button class="snapshot-close" type="button" aria-label="Close snapshot comparison" onclick={close}>×</button>
  </header>

  <div class="snapshot-tools">
    <label>Filter CAN IDs<input class="text-input" type="search" aria-label="Filter snapshot CAN IDs" placeholder="0x7E8…" bind:value={idFilter} /></label>
    <span class="snapshot-summary">Showing {Math.min(sortedRows.length, 100)} of {matchingRows.length} matching IDs · {rows.length} total</span>
  </div>

  <p class="snapshot-legend" aria-label="Snapshot comparison color legend">
    <span class="legend-exact">Exact payload</span>
    <span class="legend-similar">Similar payload</span>
    <span class="legend-different">Different payload</span>
    <span class="legend-absent">Absent</span>
    <span class="legend-unknown">Unknown: capped</span>
    <span>Pending / incomplete: no payload</span>
  </p>

  <div class="snapshot-table-wrap">
    <table aria-label={`CAN payload comparison for ${group.label}`}>
      <thead><tr>
        <th scope="col">CAN ID</th>
        <th scope="col">Exact recurrence</th>
        {#each windows as window (window.sequence)}
          <th scope="col">Mark {window.sequence}<small>{(window.elapsed_ns / 1e9).toFixed(3)}s · {window.state}</small></th>
        {/each}
      </tr></thead>
      <tbody>
        {#each sortedRows as row (`${row.extended}:${row.can_id}`)}
          <tr>
            <th scope="row">{canId(row.can_id, row.extended)}</th>
            <td class:recurrence-supported={row.known_observations >= 2}>
              {row.exact_recurrence_percent.toFixed(0)}%<small>{row.known_observations} known</small>
            </td>
            {#each row.cells as cell, index (`${row.extended}:${row.can_id}:${index}`)}
              <td class={`snapshot-cell ${cell.state === "observed" ? `cell-${cell.relation}` : `cell-${cell.state}`}`} aria-label={`Mark ${windows[index]?.sequence ?? index + 1}: ${cellLabel(cell)}`}>
                {#if cell.state === "observed"}
                  <code>{payloadText(cell.payload)}</code>
                  <small>{cell.frame_count} RX frame{cell.frame_count === 1 ? "" : "s"}{#if cell.bit_match_percent !== undefined} · {cell.bit_match_percent.toFixed(1)}% bits{/if}</small>
                {:else}
                  <span>{cell.state}</span>
                {/if}
              </td>
            {/each}
          </tr>
        {:else}
          <tr><td colspan={2 + windows.length} class="snapshot-empty">{idFilter ? "No CAN IDs match this filter." : "No retained CAN IDs to compare."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
  {#if matchingRows.length > 100}<p class="snapshot-summary">Showing the top 100 of {matchingRows.length} matching IDs. Filter CAN IDs to narrow the list.</p>{/if}
  <footer><span class="snapshot-summary">Recurrence is exact payload reuse across known complete marks; it is not signal decoding.</span><button class="can-small-action" type="button" onclick={close}>Close</button></footer>
</dialog>

<style>
  .can-snapshot-dialog { box-sizing:border-box; display:flex; flex-direction:column; gap:10px; width:min(1100px,calc(100vw - 32px)); height:min(760px,calc(100vh - 32px)); max-width:calc(100vw - 16px); max-height:calc(100dvh - 16px); min-width:min(360px,calc(100vw - 16px)); min-height:min(260px,calc(100dvh - 16px)); resize:both; overflow:hidden; margin:auto; padding:14px; border:1px solid var(--border); border-radius:10px; color:var(--text); background:var(--surface); box-shadow:0 16px 48px #0009; }
  .can-snapshot-dialog::backdrop { background:#05090dcc; backdrop-filter:blur(2px); }
  .snapshot-heading,.snapshot-tools,.can-snapshot-dialog footer { display:flex; align-items:center; justify-content:space-between; gap:12px; }
  .snapshot-heading h2 { margin:0; font-size:15px; }
  .snapshot-heading .eyebrow { margin:0 0 3px; color:var(--text-muted); font-size:9px; }
  .snapshot-summary,.snapshot-table-wrap small { color:var(--text-muted); font-size:10px; }
  .snapshot-heading .snapshot-summary { margin:4px 0 0; }
  .snapshot-close { flex:none; width:28px; height:28px; border:1px solid var(--border-strong); border-radius:5px; color:var(--text-secondary); background:var(--surface-deep); font-size:19px; line-height:1; cursor:pointer; }
  .snapshot-close:focus-visible,.can-snapshot-dialog button:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  .snapshot-tools label { display:flex; align-items:center; gap:8px; color:var(--text-secondary); font-size:10px; }
  .snapshot-tools input { width:150px; min-height:26px; }
  .snapshot-legend { display:flex; flex-wrap:wrap; gap:7px; margin:0; color:var(--text-muted); font-size:10px; }
  .snapshot-legend span { padding:2px 5px; border-radius:3px; }
  .legend-exact,.cell-exact { background:#164a3d; color:#b8f1dc; }
  .legend-similar,.cell-similar { background:#554318; color:#f7dfa0; }
  .legend-different,.cell-different { background:#542b32; color:#ffc0c7; }
  .legend-absent,.cell-absent { background:#26313b; color:#bdc8d4; }
  .legend-unknown,.cell-unknown { background:repeating-linear-gradient(135deg,#28323d,#28323d 4px,#39434d 4px,#39434d 8px); color:#e0e6ec; }
  .snapshot-table-wrap { flex:1 1 auto; min-height:0; min-width:0; overflow:auto; border:1px solid var(--border); border-radius:6px; }
  .snapshot-table-wrap table { width:max-content; min-width:100%; border-collapse:collapse; font-size:10px; font-variant-numeric:tabular-nums; }
  .snapshot-table-wrap th,.snapshot-table-wrap td { padding:5px 7px; border-right:1px solid var(--border-soft); border-bottom:1px solid var(--border-soft); text-align:left; white-space:nowrap; }
  .snapshot-table-wrap thead th { position:sticky; top:0; z-index:1; background:var(--surface); color:var(--text-muted); }
  .snapshot-table-wrap tbody th { position:sticky; left:0; z-index:1; background:var(--surface); }
  .snapshot-table-wrap small { display:block; margin-top:3px; font-weight:normal; }
  .snapshot-table-wrap td small { opacity:.85; }
  .snapshot-cell code { color:inherit; }
  .snapshot-cell.cell-pending,.snapshot-cell.cell-incomplete { color:var(--text-muted); background:var(--surface-deep); font-style:italic; }
  .snapshot-table-wrap td.recurrence-supported { color:#b8f1dc; background:color-mix(in srgb,#164a3d 55%,var(--surface)); }
  .snapshot-empty { color:var(--text-muted); text-align:center !important; }
  .can-snapshot-dialog footer { flex:none; }
  @media(max-width:600px) { .can-snapshot-dialog { padding:10px; } .snapshot-tools { align-items:flex-start; flex-direction:column; gap:5px; } }
</style>
