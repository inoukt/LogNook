<script lang="ts">
  import type { ChartTrace, Marker } from "./types";
  import { formatDurationNs, sampleTimeRange } from "./workspace";
  import { graphWindowSamples } from "./liveGraphHistory";
  import {
    formatDerivedExpression,
    graphCoordinates,
    graphHeatmapRow,
    graphRangeSamples,
    graphValueRange,
    holdLastPointAtRightEdge,
    isLiveTraceStale,
    liveGraphTimeWindow,
    MAX_GRAPH_SAMPLES,
    MAX_SESSION_GRAPH_SAMPLES,
    mapReferenceSamples,
    nearestGraphSample,
    type GraphReferenceTrace,
    type GraphSignalPreference,
    type GraphValueSample,
  } from "./graphPreferences";
  import type { TimeWindow } from "./workspace";

  let {
    traces,
    showLegend,
    timeWindow,
    markers = [],
    playheadNs,
    onSeek,
    onWheelScrub,
    liveDisplayTimeNs,
    liveWindowSpanNs,
    signalPreferences = {},
    referenceTraces = [],
    focusedSignalId,
    onSelectFocusedTrace,
  }: {
    traces: ChartTrace[];
    showLegend: boolean;
    timeWindow?: TimeWindow;
    markers?: Marker[];
    playheadNs?: number;
    onSeek?: (positionNs: number) => void;
    onWheelScrub?: (deltaY: number) => void;
    liveDisplayTimeNs?: number;
    liveWindowSpanNs?: number;
    signalPreferences?: Record<string, GraphSignalPreference>;
    referenceTraces?: GraphReferenceTrace[];
    focusedSignalId?: string | null;
    onSelectFocusedTrace?: (signalId: string) => void;
  } = $props();
  const colors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];
  const defaultPreference: GraphSignalPreference = { min: null, max: null, view: "line", lineStyle: "solid", lineWidth: 2 };

  let timeBounds = $derived.by(() => {
    if (timeWindow) return [timeWindow.startNs, timeWindow.endNs];
    const times = traces.flatMap((trace) => trace.samples.length ? [trace.samples[0], trace.samples[trace.samples.length - 1]] : []);
    const range = sampleTimeRange(times);
    if (times.length && (liveDisplayTimeNs !== undefined || liveWindowSpanNs !== undefined)) {
      const bounds = liveGraphTimeWindow(liveDisplayTimeNs ?? range.endNs, liveWindowSpanNs);
      return [bounds.startNs, bounds.endNs];
    }
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
    const sampleLimit = timeWindow ? MAX_SESSION_GRAPH_SAMPLES : MAX_GRAPH_SAMPLES;
    const preference = signalPreferences[trace.signalId] ?? defaultPreference;
    const reference = referenceTraces.find((candidate) => candidate.signalId === trace.signalId);
    const samples = graphWindowSamples(trace.samples, timeBounds[0], timeBounds[1]);
    const stale = liveDisplayTimeNs !== undefined
      && isLiveTraceStale(trace.samples, liveDisplayTimeNs);
    const referenceSamples = mapReferenceSamples(reference?.samples ?? [], timeBounds[0], timeBounds[1]);
    const rangeSamples = graphRangeSamples(samples, (trace.scaleSamples ?? []).filter((sample) => sample.elapsed_ns >= timeBounds[0] && sample.elapsed_ns <= timeBounds[1]), Boolean(trace.scaleSamples?.length));
    const range = graphValueRange(rangeSamples, referenceSamples, preference);
    const measuredPoints = graphCoordinates(samples, timeBounds[0], timeBounds[1], range, sampleLimit);
    const points = liveDisplayTimeNs === undefined || preference.view !== "line" || stale
      ? measuredPoints
      : holdLastPointAtRightEdge(measuredPoints);
    const referencePoints = graphCoordinates(referenceSamples, timeBounds[0], timeBounds[1], range, MAX_GRAPH_SAMPLES);
    const color = trace.color ?? colors[index % colors.length];
    const barWidth = Math.max(1, 720 / Math.max(points.length, 1) * 0.68);
    const baseline = range.min <= 0 && range.max >= 0 ? yForValue(0, range) : 146;
    const bars = points.map((point) => ({
      ...point,
      x: Math.max(0, point.x - barWidth / 2),
      y: Math.min(point.y, baseline),
      height: Math.max(1, Math.abs(baseline - point.y)),
    }));
    return { trace, preference, reference, samples, sampleLimit, points, referencePoints, color, range, bars, stale };
  }));
  let hasPlotData = $derived(plottedTraces.some((item) => item.points.length || item.referencePoints.length));
  let cursorTimeNs = $state<number | null>(null);
  let cursorPinned = $state(false);
  let cursorReadouts = $derived.by(() => cursorTimeNs === null ? [] : plottedTraces.map((item) => ({
    signalId: item.trace.signalId,
    name: item.trace.name,
    unit: item.trace.unit,
    formula: item.trace.formula,
    sample: nearestGraphSample(item.samples, cursorTimeNs!, item.samples.length),
  })));

  function moveCursor(event: PointerEvent) {
    if (cursorPinned || event.buttons === 0) updateCursor(event);
  }

  function updateCursor(event: PointerEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const ratio = rect.width > 0 ? Math.min(Math.max((event.clientX - rect.left) / rect.width, 0), 1) : 0;
    cursorTimeNs = Math.round(timeBounds[0] + (timeBounds[1] - timeBounds[0]) * ratio);
  }

  function seekAt(event: PointerEvent) {
    updateCursor(event);
    cursorPinned = true;
    onSeek?.(cursorTimeNs!);
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

  function pointerLeave() {
    if (!cursorPinned) cursorTimeNs = null;
  }

  function lineDash(style: GraphSignalPreference["lineStyle"]): string | undefined {
    return style === "dashed" ? "6 4" : style === "dotted" ? "1.5 3" : undefined;
  }

  function yForValue(value: number, range: { min: number; max: number }) {
    return Math.min(Math.max(146 - ((value - range.min) / (range.max - range.min || 1)) * 124, 22), 146);
  }

  function heatColor(value: number, range: { min: number; max: number }) {
    const ratio = Math.min(Math.max((value - range.min) / (range.max - range.min || 1), 0), 1);
    const red = Math.round(86 + ratio * 169);
    const green = Math.round(200 - ratio * 18);
    const blue = Math.round(189 - ratio * 104);
    return `rgb(${red} ${green} ${blue})`;
  }

  function heatWidth(points: Array<GraphValueSample & { x: number; y: number }>, index: number) {
    const next = points[index + 1];
    const prior = points[index - 1];
    const span = next ? next.x - points[index].x : prior ? points[index].x - prior.x : 720;
    return Math.max(1, Math.min(span || 720 / Math.max(points.length, 1), 720 / Math.max(points.length, 1) * 2));
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
  onpointermove={moveCursor}
  onpointerleave={pointerLeave}
  onwheel={handleWheel}
  onkeydown={seekByKeyboard}
>
<svg
  viewBox="0 0 720 168"
  preserveAspectRatio="none"
  role="img"
  aria-label={hasPlotData ? `${traces.map((trace) => trace.name).join(", ")} graph` : "Waiting for samples"}
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
  {#if hasPlotData}
    {#each plottedTraces as item, itemIndex (item.trace.signalId)}
      {#if item.preference.view === "line"}
        {#if item.points.length > 1}
          <polyline points={item.points.map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(" ")} stroke={item.color} style={`stroke-width:${item.preference.lineWidth}px`} stroke-dasharray={lineDash(item.preference.lineStyle)} />
        {:else if item.points[0]}
          <circle cx={item.points[0].x} cy={item.points[0].y} r="3.5" fill={item.color} />
        {/if}
      {:else if item.preference.view === "scatter"}
        {#each item.points as point, pointIndex (`${item.trace.signalId}-${pointIndex}`)}
          <circle class="scatter-point" cx={point.x} cy={point.y} r="2.8" fill={item.color} />
        {/each}
      {:else if item.preference.view === "bar"}
        {#each item.bars as bar, barIndex (`${item.trace.signalId}-${barIndex}`)}
          <rect class="bar-point" x={bar.x} y={bar.y} width={Math.max(1, Math.min(720 / Math.max(item.points.length, 1) * 0.68, 36))} height={bar.height} fill={item.color} />
        {/each}
      {:else}
        {@const heatRow = graphHeatmapRow(itemIndex, plottedTraces.length)}
        {#each item.points as point, pointIndex (`${item.trace.signalId}-${pointIndex}`)}
          <rect class="heatmap-cell" x={Math.max(0, point.x - heatWidth(item.points, pointIndex) / 2)} y={heatRow.y} width={heatWidth(item.points, pointIndex)} height={heatRow.height} fill={heatColor(point.value, item.range)} />
        {/each}
      {/if}
      {#if item.referencePoints.length > 1}
        <polyline class="reference-trace" points={item.referencePoints.map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(" ")} stroke={item.reference?.color ?? item.color} style={`stroke-width:${Math.max(2, item.preference.lineWidth)}px`} />
      {:else if item.referencePoints[0]}
        <circle class="reference-trace reference-point" cx={item.referencePoints[0].x} cy={item.referencePoints[0].y} r="4" fill="none" stroke={item.reference?.color ?? item.color} style={`stroke-width:${Math.max(2, item.preference.lineWidth)}px`} />
      {/if}
    {/each}
  {/if}
  {#if cursorTimeNs !== null}
    <line class="cursor-guide" x1={timeBounds[1] > timeBounds[0] ? ((cursorTimeNs - timeBounds[0]) / (timeBounds[1] - timeBounds[0])) * 720 : 0} y1="16" x2={timeBounds[1] > timeBounds[0] ? ((cursorTimeNs - timeBounds[0]) / (timeBounds[1] - timeBounds[0])) * 720 : 0} y2="146" />
  {/if}
</svg>
{#if !hasPlotData}
  <span class="waiting-message" aria-hidden="true">Waiting for samples</span>
{/if}
</div>

<div class="chart-cursor-readout" role="status" aria-live="polite">
  {#each plottedTraces.filter((item) => item.stale) as item (item.trace.signalId)}
    <span class="trace-stale">{item.trace.name} · stale</span>
  {/each}
  {#if cursorTimeNs !== null}
    <strong>{formatDurationNs(cursorTimeNs)}</strong>
    {#each cursorReadouts as item (item.signalId)}
      <span>{item.name}: {item.sample ? `${item.sample.value.toFixed(2)} ${item.unit}` : "—"}{formatDerivedExpression(item.formula)}</span>
    {/each}
    {#if cursorPinned}<button class="clear-cursor" aria-label="Clear pinned cursor" onclick={() => { cursorPinned = false; cursorTimeNs = null; }}>×</button>{/if}
  {/if}
</div>

{#if showLegend}
  <div class="chart-legend" aria-label="Signal values">
    {#each plottedTraces as item (item.trace.signalId)}
      <button class="legend-item" class:focused={item.trace.signalId === focusedSignalId} type="button" aria-label={`Focus Y range for ${item.trace.name}`} aria-pressed={item.trace.signalId === focusedSignalId} title={`Focus Y range for ${item.trace.name}`} onclick={() => onSelectFocusedTrace?.(item.trace.signalId)}>
        <span class="legend-swatch" style={`--trace-color: ${item.color}`}></span>
        <span class="legend-name">{item.trace.pid === undefined ? item.trace.name : `PID ${item.trace.pid.toString(16).toUpperCase().padStart(2, "0")} · ${item.trace.name}`}</span>
        <strong>{item.trace.current ? item.trace.current.value.toFixed(1) : "—"} {item.trace.unit}</strong>
        {#if item.trace.formula}<small class="legend-formula">{formatDerivedExpression(item.trace.formula)}</small>{/if}
      </button>
    {/each}
    {#if plottedTraces.some((item) => item.referencePoints.length)}<div class="reference-legend"><span></span> dashed trace: saved reference</div>{/if}
  </div>
{/if}

<style>
  .chart-cursor-readout { height: 24px; min-width: 0; flex: 0 0 24px; display: flex; align-items: center; flex-wrap: nowrap; gap: 4px 12px; overflow-x: auto; overflow-y: hidden; padding: 2px; color: var(--text-muted); font-size: 10px; white-space: nowrap; }
  .legend-item { border: 1px solid transparent; border-radius: 4px; padding: 2px 4px; background: transparent; text-align: left; cursor: pointer; }
  .legend-item:hover, .legend-item.focused { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .legend-item:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .chart-cursor-readout strong { color: var(--text); }
  .trace-stale { color: var(--danger, #ff7f86); font-size: 10px; font-weight: 700; }
  .clear-cursor { width: 18px; height: 18px; border: 0; border-radius: 50%; background: var(--surface-raised); color: var(--text-muted); cursor: pointer; }
  .reference-legend { display: inline-flex; align-items: center; gap: 5px; color: var(--text-muted); font-size: 10px; }
  .reference-legend span { width: 18px; border-top: 2px dashed var(--text); }
  .reference-trace { stroke-dasharray: 6 4; }
  line.cursor-guide { stroke: var(--text); stroke-width: 1; stroke-dasharray: 2 3; vector-effect: non-scaling-stroke; }
  rect.heatmap-cell { opacity: .72; }
</style>
