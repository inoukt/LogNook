<script lang="ts">
  import type { ChartTrace } from "./types";
  import { chartPoints } from "./workspace";

  let { traces, showLegend }: { traces: ChartTrace[]; showLegend: boolean } = $props();
  const colors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

  let timeBounds = $derived.by(() => {
    const times = traces.flatMap((trace) => trace.samples.map((sample) => sample.elapsed_ns));
    return times.length ? [Math.min(...times), Math.max(...times)] : [0, 0];
  });

  let plottedTraces = $derived.by(() => traces.map((trace, index) => {
    const points = chartPoints(trace.samples, timeBounds[0], timeBounds[1]);
    const last = points.split(" ").at(-1)?.split(",").map(Number);
    return { trace, color: colors[index % colors.length], points, last };
  }));
</script>

<svg
  viewBox="0 0 720 168"
  preserveAspectRatio="none"
  role="img"
  aria-label={`${traces.map((trace) => trace.name).join(", ")} live graph`}
>
  <line x1="0" y1="22" x2="720" y2="22" />
  <line x1="0" y1="84" x2="720" y2="84" />
  <line x1="0" y1="146" x2="720" y2="146" />
  {#if plottedTraces.some((item) => item.points)}
    {#each plottedTraces as item (item.trace.signalId)}
      {#if item.trace.samples.length > 1}
        <polyline points={item.points} stroke={item.color} />
      {:else if item.last}
        <circle cx={item.last[0]} cy={item.last[1]} r="3.5" fill={item.color} />
      {/if}
    {/each}
  {:else}
    <text x="360" y="91" text-anchor="middle">Waiting for samples</text>
  {/if}
</svg>

{#if showLegend}
  <div class="chart-legend" aria-label="Signal values">
    {#each plottedTraces as item (item.trace.signalId)}
      <div class="legend-item">
        <span class="legend-swatch" style={`--trace-color: ${item.color}`}></span>
        <span class="legend-name">PID {item.trace.pid.toString(16).toUpperCase().padStart(2, "0")} · {item.trace.name}</span>
        <strong>{item.trace.current ? item.trace.current.value.toFixed(1) : "—"} {item.trace.unit}</strong>
      </div>
    {/each}
  </div>
{/if}
