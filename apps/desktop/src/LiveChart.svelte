<script lang="ts">
  import type { ChartTrace, Marker } from "./types";
  import { chartPoints, positionAtChartX, sampleTimeRange } from "./workspace";
  import type { TimeWindow } from "./workspace";

  let { traces, showLegend, timeWindow, markers = [], playheadNs, onSeek, onWheelScrub }: {
    traces: ChartTrace[];
    showLegend: boolean;
    timeWindow?: TimeWindow;
    markers?: Marker[];
    playheadNs?: number;
    onSeek?: (positionNs: number) => void;
    onWheelScrub?: (deltaY: number) => void;
  } = $props();
  const colors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

  let timeBounds = $derived.by(() => {
    const times = traces.flatMap((trace) => trace.samples.map((sample) => sample.elapsed_ns));
    if (timeWindow) return [timeWindow.startNs, timeWindow.endNs];
    const range = sampleTimeRange(times.map((elapsed_ns) => ({ elapsed_ns })));
    return [range.startNs, range.endNs];
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
    return { trace, color: trace.color ?? colors[index % colors.length], points, last };
  }));

  function seekAt(event: PointerEvent) {
    if (!onSeek) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    onSeek(positionAtChartX(event.clientX, rect.left, rect.width, { startNs: timeBounds[0], endNs: timeBounds[1] }));
  }

  function handleWheel(event: WheelEvent) {
    if (!onWheelScrub) return;
    event.preventDefault();
    onWheelScrub(event.deltaY);
  }

  function seekByKeyboard(event: KeyboardEvent) {
    if (!onSeek || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const step = Math.max(1, (timeBounds[1] - timeBounds[0]) / 100);
    const current = playheadNs ?? timeBounds[0];
    onSeek(event.key === "Home" ? timeBounds[0]
      : event.key === "End" ? timeBounds[1]
        : current + (event.key === "ArrowRight" ? step : -step));
  }
</script>

<div
  class="chart-interaction"
  role="slider"
  aria-label="Graph playback position"
  aria-orientation="horizontal"
  aria-valuemin={timeBounds[0]}
  aria-valuemax={Math.max(timeBounds[1], timeBounds[0] + 1)}
  aria-valuenow={playheadNs ?? timeBounds[0]}
  aria-disabled={!onSeek}
  tabindex={onSeek ? 0 : -1}
  onpointerdown={seekAt}
  onwheel={handleWheel}
  onkeydown={seekByKeyboard}
>
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
</div>

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
