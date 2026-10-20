<script lang="ts">
  import type { ChartTrace } from "./types";
  import type { TimeWindow } from "./workspace";
  import { buildScatterPoints, scatterColor, scatterRange } from "./scatterPlot";

  let {
    traces,
    xSignalId,
    ySignalId,
    colorSignalId = null,
    timeWindow,
    live = false,
    xMin = null,
    xMax = null,
    yMin = null,
    yMax = null,
  }: {
    traces: ChartTrace[];
    xSignalId: string;
    ySignalId: string;
    colorSignalId?: string | null;
    timeWindow: TimeWindow;
    live?: boolean;
    xMin?: number | null;
    xMax?: number | null;
    yMin?: number | null;
    yMax?: number | null;
  } = $props();

  const width = 760;
  const height = 380;
  const left = 78;
  const right = 65;
  const top = 24;
  const bottom = 62;
  const MAX_TRAIL_NS = 15_000_000_000;
  const xEnd = width - right;
  const yEnd = height - bottom;
  let xTrace = $derived(traces.find((trace) => trace.signalId === xSignalId));
  let yTrace = $derived(traces.find((trace) => trace.signalId === ySignalId));
  let colorTrace = $derived(colorSignalId ? traces.find((trace) => trace.signalId === colorSignalId) : undefined);
  let plotWindow = $derived({
    startNs: Math.max(timeWindow.startNs, timeWindow.endNs - MAX_TRAIL_NS),
    endNs: timeWindow.endNs,
  });
  let points = $derived(xTrace && yTrace
    ? buildScatterPoints(xTrace.samples, yTrace.samples, colorTrace?.samples ?? null, plotWindow)
    : []);
  let xRange = $derived(scatterRange(points.map(({ x }) => x), xMin, xMax));
  let yRange = $derived(scatterRange(points.map(({ y }) => y), yMin, yMax));
  let colorValues = $derived(colorTrace?.samples.flatMap((sample) => Number.isFinite(sample.value)
    && sample.elapsed_ns >= plotWindow.startNs && sample.elapsed_ns <= plotWindow.endNs ? [sample.value] : []) ?? []);
  let colorRange = $derived(scatterRange(colorValues));
  let latestNs = $derived(points.at(-1)?.elapsed_ns ?? -Infinity);
  let colorTitle = $derived(colorTrace ? `${colorTrace.name} · ${colorTrace.unit}` : "Sample age");
  let visiblePoints = $derived(points.filter((point) => point.elapsed_ns >= plotWindow.endNs - (plotWindow.endNs - plotWindow.startNs) * 0.99));
  let accessibleLabel = $derived(`${yTrace?.name ?? "Y"} versus ${xTrace?.name ?? "X"}; color shows ${colorTitle}; ${visiblePoints.length} plotted samples`);

  function plotX(value: number): number {
    return left + (value - xRange.min) / (xRange.max - xRange.min) * (xEnd - left);
  }

  function plotY(value: number): number {
    return yEnd - (value - yRange.min) / (yRange.max - yRange.min) * (yEnd - top);
  }

  function grade(point: { elapsed_ns: number; colorValue: number | null }): { color: string; opacity: number } {
    const age = Math.min(1, Math.max(0, (plotWindow.endNs - point.elapsed_ns) / Math.max(1, plotWindow.endNs - plotWindow.startNs)));
    const amount = colorTrace && point.colorValue !== null
      ? (point.colorValue - colorRange.min) / (colorRange.max - colorRange.min)
      : age;
    return { color: point.colorValue === null && colorTrace ? "#8b9aa5" : scatterColor(amount), opacity: 0.03 + 0.93 * (1 - age) ** 1.6 };
  }

  function tick(range: { min: number; max: number }, index: number): number {
    return range.min + (range.max - range.min) * index / 4;
  }
</script>

{#if !xTrace || !yTrace || traces.length < 2}
  <div class="scatter-empty" role="status">Select at least two signals to compare them.</div>
{:else if visiblePoints.length === 0}
  <div class="scatter-empty" role="status">Waiting for time-aligned samples in this window.</div>
{:else}
  <figure class="scatter-figure">
    <svg class="scatter-svg" xmlns="http://www.w3.org/2000/svg" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={accessibleLabel}>
      <defs>
        <linearGradient id="scatter-color" x1="0" y1="1" x2="0" y2="0">
          <stop offset="0%" stop-color={scatterColor(0)} />
          <stop offset="50%" stop-color={scatterColor(0.5)} />
          <stop offset="100%" stop-color={scatterColor(1)} />
        </linearGradient>
        <clipPath id="scatter-clip"><rect x={left} y={top} width={xEnd - left} height={yEnd - top} /></clipPath>
      </defs>
      {#each [0, 1, 2, 3, 4] as index (index)}
        {@const y = top + index * (yEnd - top) / 4}
        {@const x = left + index * (xEnd - left) / 4}
        <line x1={left} y1={y} x2={xEnd} y2={y} stroke="#2d3c47" stroke-dasharray="3 5" />
        <line x1={x} y1={top} x2={x} y2={yEnd} stroke="#2d3c47" stroke-dasharray="3 5" />
        <text x={left - 9} y={y + 4} text-anchor="end" style="font: 10px Inter, 'Segoe UI', sans-serif; fill: #94a7b2">{tick(yRange, 4 - index).toLocaleString(undefined, { maximumFractionDigits: 2 })}</text>
        <text x={x} y={yEnd + 18} text-anchor="middle" style="font: 10px Inter, 'Segoe UI', sans-serif; fill: #94a7b2">{tick(xRange, index).toLocaleString(undefined, { maximumFractionDigits: 2 })}</text>
      {/each}
      <g clip-path="url(#scatter-clip)">
        {#each visiblePoints as point (point.elapsed_ns)}
          {@const appearance = grade(point)}
          <circle class:scatter-newest={live && point.elapsed_ns === latestNs} cx={plotX(point.x)} cy={plotY(point.y)} r={point.elapsed_ns === latestNs ? 4.2 : 3} fill={appearance.color} style={`color: ${appearance.color}`} fill-opacity={appearance.opacity} stroke="#0d151c" stroke-opacity="0.25" stroke-width="0.8">
            <title>{`${xTrace.name}: ${point.x} ${xTrace.unit} · ${yTrace.name}: ${point.y} ${yTrace.unit} · ${colorTitle}: ${point.colorValue ?? ((timeWindow.endNs - point.elapsed_ns) / 1_000_000_000).toFixed(2)}`}</title>
          </circle>
        {/each}
      </g>
      <text x={(left + xEnd) / 2} y={height - 12} text-anchor="middle" style="font: 12px Inter, 'Segoe UI', sans-serif; fill: #d1dce3">{xTrace.name} · {xTrace.unit}</text>
      <text x="18" y={(top + yEnd) / 2} text-anchor="middle" transform={`rotate(-90 18 ${(top + yEnd) / 2})`} style="font: 12px Inter, 'Segoe UI', sans-serif; fill: #d1dce3">{yTrace.name} · {yTrace.unit}</text>
      <rect x={width - 43} y={top + 12} width="9" height={Math.max(24, yEnd - top - 24)} rx="4.5" fill="url(#scatter-color)" />
      <text x={width - 25} y={top + 18} style="font: 8px Inter, 'Segoe UI', sans-serif; fill: #94a7b2" text-anchor="end">{colorTrace ? colorRange.max.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "Older"}</text>
      <text x={width - 25} y={yEnd - 6} style="font: 8px Inter, 'Segoe UI', sans-serif; fill: #94a7b2" text-anchor="end">{colorTrace ? colorRange.min.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "Newer"}</text>
    </svg>
    <figcaption>{visiblePoints.length.toLocaleString()} samples · {colorTitle} · recent samples fade over up to 15 seconds</figcaption>
  </figure>
{/if}

<style>
  .scatter-figure { margin: 0; padding: 6px 10px 10px; }
  .scatter-svg { display: block; width: 100%; height: auto; overflow: visible; }
  .scatter-svg line { vector-effect: non-scaling-stroke; }
  .scatter-svg circle { transition: fill 180ms ease, fill-opacity 180ms ease; }
  .scatter-svg circle:hover { stroke: #f3f7fa; stroke-opacity: 1; stroke-width: 1.6; }
  .scatter-svg .scatter-newest { animation: scatter-glow 1.4s ease-in-out infinite alternate; filter: drop-shadow(0 0 5px currentColor); }
  .scatter-figure figcaption { padding-left: 67px; color: #8fa1ad; font-size: 9px; }
  .scatter-empty { display: grid; min-height: 180px; place-items: center; color: #8fa1ad; font-size: 12px; }
  @keyframes scatter-glow { from { opacity: 0.7; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    .scatter-svg circle { transition: none; }
    .scatter-svg .scatter-newest { animation: none; filter: none; }
  }
</style>
