<script lang="ts">
  import type { ChartTrace, Marker } from "./types";
  import { chartPoints } from "./workspace";
  import type { TimeWindow } from "./workspace";

  let { traces, showLegend, timeWindow, markers = [], playheadNs }: {
    traces: ChartTrace[];
    showLegend: boolean;
    timeWindow?: TimeWindow;
    markers?: Marker[];
    playheadNs?: number;
  } = $props();
  const colors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

  let timeBounds = $derived.by(() => {
    const times = traces.flatMap((trace) => trace.samples.map((sample) => sample.elapsed_ns));
    if (timeWindow) return [timeWindow.startNs, timeWindow.endNs];
    return times.length ? [Math.min(...times), Math.max(...times)] : [0, 0];
  });
  let markerPositions = $derived.by(() => markers.flatMap((marker) => {
    if (marker.elapsed_ns < timeBounds[0] || marker.elapsed_ns > timeBounds[1]) return [];
    const x = timeBounds[1] > timeBounds[0]
      ? ((marker.elapsed_ns - timeBounds[0]) / (timeBounds[1] - timeBounds[0])) * 720
      : 0;
    return [{ marker, x }];
  }));
  let playheadX = $derived.by(() => playheadNs === undefined || playheadNs < timeBounds[0] || playheadNs > timeBounds[1]
    ? null
    : timeBounds[1] > timeBounds[0]
      ? ((playheadNs - timeBounds[0]) / (timeBounds[1] - timeBounds[0])) * 720
      : 0);

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
  {#each markerPositions as point (point.marker.id ?? `${point.marker.elapsed_ns}-${point.marker.label}`)}
    <line class="marker-guide" x1={point.x} y1="17" x2={point.x} y2="146" />
    <circle class="marker-cap" cx={point.x} cy="15" r="3" />
  {/each}
  {#if playheadX !== null}
    <line class="playhead-guide" x1={playheadX} y1="13" x2={playheadX} y2="146" />
  {/if}
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
        <span class="legend-name">{item.trace.pid === undefined ? item.trace.signalId : `PID ${item.trace.pid.toString(16).toUpperCase().padStart(2, "0")} · ${item.trace.name}`}</span>
        <strong>{item.trace.current ? item.trace.current.value.toFixed(1) : "—"} {item.trace.unit}</strong>
      </div>
    {/each}
  </div>
{/if}
