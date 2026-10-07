<script lang="ts">
  import type { CanFrame } from "./types";
  import type { CanTrafficSummary } from "./canWorkbench";

  export let frames: CanFrame[] = [];
  export let trafficSummary: CanTrafficSummary[] = [];

  let trafficView: "summary" | "frames" = "summary";
  $: trafficFrameCount = trafficSummary.reduce((total, row) => total + row.rx_count + row.tx_count, 0);

  function bytesHex(data: number[]): string {
    return data.map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
  }
</script>

<section class="can-traffic-panel panel" aria-label="CAN bus traffic monitor">
  <div class="can-monitor-toolbar">
    <div class="can-monitor-title">
      <p class="eyebrow">LIVE CAN BUS</p>
      <h2>Traffic monitor</h2>
    </div>
    <span class="count-pill">{trafficFrameCount.toLocaleString()} frames · {trafficSummary.length} IDs</span>
    <div class="can-view-switch" role="group" aria-label="CAN traffic view">
      <button class="can-view-option" aria-pressed={trafficView === "summary"} onclick={() => trafficView = "summary"}>Counts</button>
      <button class="can-view-option" aria-pressed={trafficView === "frames"} onclick={() => trafficView = "frames"}>Frames</button>
    </div>
  </div>
  <p class="can-monitor-hint">{#if trafficView === "summary"}Counts since connection · RX period and rate are averages across received frames.{:else}Latest {frames.length} frames retained for inspection (up to 400).{/if}</p>
  {#if trafficView === "summary"}
    <div class="can-table-wrap">
      <table aria-label="Live CAN ID counts">
        <thead><tr><th>ID</th><th>DLC</th><th>Data</th><th>Period</th><th>Count</th><th>Rate</th><th>ASCII</th></tr></thead>
        <tbody>
          {#each trafficSummary as row (`${row.can_id}:${row.extended}`)}
            <tr>
              <td>{row.extended ? "29" : "11"}-bit 0x{row.can_id.toString(16).toUpperCase()}</td>
              <td>{row.last_data.length}</td>
              <td><code>{bytesHex(row.last_data)}</code> · {row.last_direction.toUpperCase()}</td>
              <td>{row.period_ms === null ? "—" : `${row.period_ms.toFixed(row.period_ms < 10 ? 2 : 1)} ms`}</td>
              <td>{row.rx_count.toLocaleString()} RX / {row.tx_count.toLocaleString()} TX</td>
              <td>{row.rate_hz === null ? "—" : `${row.rate_hz.toFixed(1)} Hz`}</td>
              <td><code>{row.ascii || "—"}</code></td>
            </tr>
          {:else}
            <tr><td colspan="7" class="can-muted">Waiting for Candlelight frames</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="can-table-wrap">
      <table aria-label="Recent CAN frames">
        <thead><tr><th>Time</th><th>Dir</th><th>ID</th><th>DLC</th><th>Data</th></tr></thead>
        <tbody>
          {#each frames.slice(-80).reverse() as frame, index (`${frame.wall_time_unix_ns}:${frame.can_id}:${index}`)}
            <tr>
              <td>{(frame.elapsed_ns / 1e9).toFixed(3)}s</td><td>{frame.direction.toUpperCase()}</td>
              <td>{frame.extended ? "29" : "11"}-bit 0x{frame.can_id.toString(16).toUpperCase()}</td>
              <td>{frame.data.length}</td><td><code>{bytesHex(frame.data)}</code></td>
            </tr>
          {:else}
            <tr><td colspan="5" class="can-muted">Waiting for Candlelight frames</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .can-traffic-panel { display:flex; flex-direction:column; gap:4px; min-width:0; min-height:0; overflow:hidden; padding:6px; }
  .can-monitor-toolbar { display:flex; align-items:center; gap:8px; min-width:0; }
  .can-monitor-title { min-width:0; }
  .can-monitor-title .eyebrow { margin:0 0 2px; font-size:9px; }
  .can-monitor-title h2 { margin:0; font-size:13px; }
  .count-pill { flex:0 0 auto; }
  .can-monitor-toolbar .count-pill { margin-left:auto; }
  .can-view-switch { display:inline-flex; flex:0 0 auto; gap:2px; }
  .can-view-option { min-height:24px; padding:2px 8px; border:1px solid var(--border-strong); border-radius:5px; background:var(--surface-raised); color:var(--text-secondary); cursor:pointer; font-size:10px; }
  .can-view-option[aria-pressed="true"] { border-color:var(--accent); background:var(--accent); color:var(--accent-text); }
  .can-monitor-hint { margin:0; color:var(--text-muted); font-size:9px; }
  .can-table-wrap { flex:1 1 auto; min-width:0; min-height:0; overflow:auto; border:1px solid var(--border); border-radius:6px; }
  table { width:100%; border-collapse:collapse; font-size:10px; font-variant-numeric:tabular-nums; }
  th,td { padding:3px 5px; border-bottom:1px solid var(--border-soft); text-align:left; white-space:nowrap; }
  th { position:sticky; top:0; z-index:1; background:var(--surface); color:var(--text-muted); }
  code { color:var(--text-primary); }
  .can-muted { color:var(--text-muted); }
  @media (max-width:700px) {
    .can-monitor-toolbar { flex-wrap:wrap; }
    .can-monitor-toolbar .count-pill { margin-left:0; }
    .can-view-switch { margin-left:auto; }
  }
</style>
