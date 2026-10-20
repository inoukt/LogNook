<script lang="ts">
  import { onMount } from "svelte";
  import LiveChart from "./LiveChart.svelte";
  import ScatterPlot from "./ScatterPlot.svelte";
  import { graphWindowSamples } from "./liveGraphHistory";
  import { buildScatterPoints, scatterRange } from "./scatterPlot";
  import { buildGraphExportSvg, graphPngFilename, type GraphExportLegendItem } from "./graphExport";
  import type { ChartTrace, Marker } from "./types";
  import { sampleTimeRange, type TimeWindow } from "./workspace";
  import type { DerivedSignalSource } from "./derivedSignals";
  import {
    captureGraphReference,
    defaultGraphSignalPreference,
    formatDerivedExpression,
    graphZoomAvailability,
    graphRangeSamples,
    graphValueRange,
    holdLastPointAtRightEdge,
    liveGraphTimeWindow,
    loadGraphPreferences,
    mapReferenceSamples,
    migrateDerivedGraphStyles,
    normalizeGraphPreferences,
    resolveFocusedGraphSignalId,
    saveGraphPreferences,
    zoomGraphRange,
    zoomLiveWindowFactor,
    MIN_LIVE_WINDOW_SECONDS,
    MAX_LIVE_WINDOW_SECONDS,
    type GraphSignalPreference,
    type GraphView,
    type GraphType,
    type GraphLineStyle,
  } from "./graphPreferences";

  const traceColors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

  let { traces, graphMode, graphSignalIds, onReorder, onCreateDerivedSignal, onToggleGraphSignal, timeWindow, markers = [], playheadNs, onSeek, onWheelScrub, onZoomTime, onResetTimeZoom, live = false }: {
    traces: ChartTrace[];
    graphMode: "overlay" | "separate";
    graphSignalIds: string[];
    onReorder: (fromId: string, toId: string) => void;
    onCreateDerivedSignal: (source: DerivedSignalSource) => void;
    onToggleGraphSignal: (signalId: string, enabled: boolean) => void;
    timeWindow?: TimeWindow;
    markers?: Marker[];
    playheadNs?: number;
    onSeek?: (positionNs: number) => void;
    onWheelScrub?: (deltaY: number) => void;
    onZoomTime?: (factor: number) => void;
    onResetTimeZoom?: () => void;
    live?: boolean;
  } = $props();
  let draggedSignal = $state<string | null>(null);
  let exportError = $state(false);
  let selectedTraceId = $state<string | null>(null);
  let graphPreferences = $state(loadGraphPreferences());
  let liveAnchor = $state<{ sampleNs: number; arrivedAtMs: number } | null>(null);
  let liveDisplayTimeNs = $state<number | undefined>();
  let lastPaintedTimeNs: number | undefined;
  const migratedDerivedStyles = new Set<string>();

  $effect(() => {
    const restore = (event: Event) => {
      graphPreferences = normalizeGraphPreferences((event as CustomEvent<unknown>).detail);
    };
    window.addEventListener("lognook-graph-preferences-restored", restore);
    return () => window.removeEventListener("lognook-graph-preferences-restored", restore);
  });

  let latestLiveSampleNs = $derived.by(() => traces.reduce<number | undefined>((latest, trace) => {
    const sample = trace.samples.at(-1);
    return sample && (latest === undefined || sample.elapsed_ns > latest) ? sample.elapsed_ns : latest;
  }, undefined));
  let showDisconnectedSamples = $derived(!live && !timeWindow && traces.some((trace) => trace.samples.length > 0));
  let liveWindowSeconds = $derived(Math.min(Math.max(graphPreferences.liveWindowSeconds * graphPreferences.liveZoomFactor, MIN_LIVE_WINDOW_SECONDS), MAX_LIVE_WINDOW_SECONDS));
  let liveWindowSpanNs = $derived(liveWindowSeconds * 1_000_000_000);
  let focusedSignalId = $derived(resolveFocusedGraphSignalId(traces.map(({ signalId }) => signalId), selectedTraceId));
  let focusedTrace = $derived(traces.find((trace) => trace.signalId === focusedSignalId));
  let scatterX = $derived(traces.find((trace) => trace.signalId === graphPreferences.scatter.xSignalId) ?? traces[0]);
  let scatterY = $derived(traces.find((trace) => trace.signalId === graphPreferences.scatter.ySignalId) ?? traces[1] ?? traces[0]);
  let scatterColorId = $derived(traces.some((trace) => trace.signalId === graphPreferences.scatter.colorSignalId) ? graphPreferences.scatter.colorSignalId : null);
  let graphTimeBounds = $derived.by(() => {
    if (timeWindow) return [timeWindow.startNs, timeWindow.endNs] as const;
    const times = traces.flatMap((trace) => trace.samples.length ? [trace.samples[0], trace.samples.at(-1)!] : []);
    const range = sampleTimeRange(times);
    const bounds = times.length ? liveGraphTimeWindow(liveDisplayTimeNs ?? range.endNs, liveWindowSpanNs) : range;
    return [bounds.startNs, bounds.endNs] as const;
  });
  let scatterWindow = $derived(timeWindow
    ? { ...timeWindow, endNs: Math.max(timeWindow.startNs, Math.min(timeWindow.endNs, playheadNs ?? timeWindow.endNs)) }
    : liveGraphTimeWindow(latestLiveSampleNs ?? 0, liveWindowSpanNs));
  let scatterPlotWindow = $derived({
    startNs: Math.max(scatterWindow.startNs, scatterWindow.endNs - 15_000_000_000),
    endNs: scatterWindow.endNs,
  });
  let scatterColorTrace = $derived(scatterColorId ? traces.find((trace) => trace.signalId === scatterColorId) : undefined);
  let scatterPoints = $derived.by(() => graphPreferences.graphType === "xy_scatter" && scatterX && scatterY
    ? buildScatterPoints(scatterX.samples, scatterY.samples, scatterColorTrace?.samples ?? null, scatterPlotWindow)
    : []);
  let focusedRangeHasData = $derived(focusedTrace ? traceHasRangeData(focusedTrace) : false);
  let scatterHasData = $derived(traces.length >= 2 && scatterPoints.length > 0);
  let hasManualBounds = $derived.by(() => {
    const targets = graphPreferences.graphType === "xy_scatter" ? [scatterX, scatterY] : [focusedTrace];
    return targets.some((trace) => {
      if (!trace) return false;
      const preference = signalPreference(trace.signalId, trace.formula !== undefined);
      return preference.min !== null || preference.max !== null;
    });
  });
  let xZoomOutTitle = $derived(graphPreferences.graphType === "xy_scatter" ? `Zoom X axis out${scatterX ? ` · ${scatterX.name}` : ""}` : timeWindow ? "Zoom replay time out around the playback cursor" : "Zoom time out");
  let xZoomInTitle = $derived(graphPreferences.graphType === "xy_scatter" ? `Zoom X axis in${scatterX ? ` · ${scatterX.name}` : ""}` : timeWindow ? "Zoom replay time in around the playback cursor" : "Zoom time in");
  let yZoomOutTitle = $derived(graphPreferences.graphType === "xy_scatter" ? `Zoom Y axis out${scatterY ? ` · ${scatterY.name}` : ""}` : `Zoom Y range out${focusedTrace ? ` · ${focusedTrace.name}` : ""}`);
  let yZoomInTitle = $derived(graphPreferences.graphType === "xy_scatter" ? `Zoom Y axis in${scatterY ? ` · ${scatterY.name}` : ""}` : `Zoom Y range in${focusedTrace ? ` · ${focusedTrace.name}` : ""}`);
  let autoZoomTitle = $derived.by(() => {
    if (graphPreferences.graphType === "xy_scatter") {
      return `Auto: fit ${scatterX?.name ?? "X"} on X and ${scatterY?.name ?? "Y"} on Y`;
    }
    const values = `${focusedTrace?.name ?? "selected trace"}'s value range`;
    const time = timeWindow ? "the full recording span" : live ? "the configured live time span" : "the loaded sample span";
    return `Auto: fit ${values} and ${time}`;
  });
  let zoomAvailability = $derived(graphZoomAvailability({
    graphType: graphPreferences.graphType,
    hasScatterAxes: Boolean(scatterX && scatterY && traces.length >= 2),
    hasScatterData: scatterHasData,
    hasLiveXData: live && traces.some(traceHasRangeData),
    hasReplayWindow: Boolean(timeWindow),
    canZoomReplay: Boolean(onZoomTime),
    canResetReplay: Boolean(onResetTimeZoom),
    canResetLive: graphPreferences.liveZoomFactor !== 1,
    hasFocusedTraceData: focusedRangeHasData,
    hasManualBounds,
  }));
  let autoZoomAvailable = $derived(zoomAvailability.auto);
  let canZoomX = $derived(zoomAvailability.x);
  let canZoomY = $derived(zoomAvailability.y);

  $effect(() => {
    if (selectedTraceId && !traces.some((trace) => trace.signalId === selectedTraceId)) selectedTraceId = null;
  });
  function updateGraphType(graphType: GraphType) {
    graphPreferences = normalizeGraphPreferences({ ...graphPreferences, graphType });
    saveGraphPreferences(graphPreferences);
  }

  function updateScatterAxis(axis: "xSignalId" | "ySignalId" | "colorSignalId", value: string) {
    graphPreferences = normalizeGraphPreferences({ ...graphPreferences,
      scatter: { ...graphPreferences.scatter, [axis]: value || null } });
    saveGraphPreferences(graphPreferences);
  }

  $effect(() => {
    if (!live || latestLiveSampleNs === undefined) {
      liveAnchor = null;
      liveDisplayTimeNs = undefined;
      lastPaintedTimeNs = undefined;
      return;
    }
    if (!liveAnchor || liveAnchor.sampleNs !== latestLiveSampleNs) {
      const sampleNs = liveAnchor && latestLiveSampleNs < liveAnchor.sampleNs
        ? latestLiveSampleNs
        : Math.max(latestLiveSampleNs, lastPaintedTimeNs ?? latestLiveSampleNs);
      liveAnchor = { sampleNs, arrivedAtMs: performance.now() };
      liveDisplayTimeNs = sampleNs;
      lastPaintedTimeNs = sampleNs;
    }
  });

  $effect(() => {
    const anchor = liveAnchor;
    if (!live || !anchor) return;
    let frame = 0;
    const advance = () => {
      lastPaintedTimeNs = anchor.sampleNs + (performance.now() - anchor.arrivedAtMs) * 1_000_000;
      liveDisplayTimeNs = lastPaintedTimeNs;
      frame = requestAnimationFrame(advance);
    };
    frame = requestAnimationFrame(advance);
    return () => cancelAnimationFrame(frame);
  });

  $effect(() => {
    const ids = traces.filter((trace) => trace.formula && !migratedDerivedStyles.has(trace.signalId)).map(({ signalId }) => signalId);
    if (!ids.length) return;
    ids.forEach((id) => migratedDerivedStyles.add(id));
    const migrated = migrateDerivedGraphStyles(graphPreferences, ids);
    if (migrated !== graphPreferences) {
      graphPreferences = migrated;
      saveGraphPreferences(graphPreferences);
    }
  });

  function signalPreference(signalId: string, isDerived = false) {
    return graphPreferences.signals[signalId] ?? defaultGraphSignalPreference(isDerived);
  }

  function updateSignalPreference(signalId: string, patch: Partial<GraphSignalPreference>, isDerived = false) {
    graphPreferences = normalizeGraphPreferences({
      ...graphPreferences,
      signals: { ...graphPreferences.signals, [signalId]: { ...signalPreference(signalId, isDerived), ...patch } },
    });
    saveGraphPreferences(graphPreferences);
  }

  function updateLiveWindowSeconds(value: string) {
    if (!value.trim()) return;
    const seconds = Number(value);
    if (!Number.isFinite(seconds)) return;
    graphPreferences = normalizeGraphPreferences({ ...graphPreferences, liveWindowSeconds: seconds });
    saveGraphPreferences(graphPreferences);
  }

  function updateLiveZoomFactor(direction: "in" | "out") {
    graphPreferences = normalizeGraphPreferences({
      ...graphPreferences,
      liveZoomFactor: zoomLiveWindowFactor(graphPreferences.liveWindowSeconds, graphPreferences.liveZoomFactor, direction),
    });
    saveGraphPreferences(graphPreferences);
  }

  function traceValueRange(trace: ChartTrace) {
    const samples = graphWindowSamples(trace.samples, graphTimeBounds[0], graphTimeBounds[1]);
    const scaleSamples = (trace.scaleSamples ?? []).filter((sample) => sample.elapsed_ns >= graphTimeBounds[0] && sample.elapsed_ns <= graphTimeBounds[1]);
    const reference = graphPreferences.reference?.traces.find((candidate) => candidate.signalId === trace.signalId);
    return graphValueRange(
      graphRangeSamples(samples, scaleSamples, Boolean(trace.scaleSamples?.length)),
      mapReferenceSamples(reference?.samples ?? [], graphTimeBounds[0], graphTimeBounds[1]),
      signalPreference(trace.signalId, trace.formula !== undefined),
    );
  }

  function traceHasRangeData(trace: ChartTrace) {
    const displayed = graphWindowSamples(trace.samples, graphTimeBounds[0], graphTimeBounds[1]);
    const scaleSamples = (trace.scaleSamples ?? []).filter((sample) => sample.elapsed_ns >= graphTimeBounds[0] && sample.elapsed_ns <= graphTimeBounds[1]);
    const reference = graphPreferences.reference?.traces.find((candidate) => candidate.signalId === trace.signalId);
    return displayed.some((sample) => Number.isFinite(sample.value)) || scaleSamples.some((sample) => Number.isFinite(sample.value))
      || Boolean(reference && mapReferenceSamples(reference.samples, graphTimeBounds[0], graphTimeBounds[1]).length);
  }

  function updateValueRange(signalId: string, range: { min: number; max: number }) {
    updateSignalPreference(signalId, range);
  }

  function zoomScatterAxis(axis: "x" | "y", direction: "in" | "out") {
    const trace = axis === "x" ? scatterX : scatterY;
    if (!trace || !scatterPoints.length) return;
    const preference = signalPreference(trace.signalId, trace.formula !== undefined);
    const values = scatterPoints.map((point) => axis === "x" ? point.x : point.y);
    updateValueRange(trace.signalId, zoomGraphRange(scatterRange(values, preference.min, preference.max), direction));
  }

  function zoomTimeAxis(direction: "in" | "out") {
    if (graphPreferences.graphType === "xy_scatter") {
      zoomScatterAxis("x", direction);
    } else if (timeWindow) {
      onZoomTime?.(direction === "in" ? 0.8 : 1.25);
    } else if (live) {
      updateLiveZoomFactor(direction);
    }
  }

  function zoomValueAxis(direction: "in" | "out") {
    if (graphPreferences.graphType === "xy_scatter") {
      zoomScatterAxis("y", direction);
      return;
    }
    const targets = focusedTrace ? [focusedTrace] : [];
    for (const trace of targets) {
      if (traceHasRangeData(trace)) updateValueRange(trace.signalId, zoomGraphRange(traceValueRange(trace), direction));
    }
  }

  function autoZoom() {
    if (graphPreferences.graphType === "xy_scatter") {
      for (const trace of new Set([scatterX, scatterY].filter((candidate): candidate is ChartTrace => Boolean(candidate)))) {
        updateSignalPreference(trace.signalId, { min: null, max: null });
      }
      return;
    }
    const targets = focusedTrace ? [focusedTrace] : [];
    for (const trace of targets) updateSignalPreference(trace.signalId, { min: null, max: null });
    if (timeWindow) onResetTimeZoom?.();
    else if (live) {
      graphPreferences = normalizeGraphPreferences({ ...graphPreferences, liveZoomFactor: 1 });
      saveGraphPreferences(graphPreferences);
    }
  }

  function handleZoomButton(event: MouseEvent, action: () => void) {
    event.preventDefault();
    event.stopPropagation();
    action();
  }

  function parseOptionalRange(value: string): number | null {
    if (!value.trim()) return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function saveReference() {
    graphPreferences = captureGraphReference(graphPreferences, traces);
    saveGraphPreferences(graphPreferences);
  }

  async function exportGraphPng(chart: SVGSVGElement, traceList: ChartTrace[], showLegend: boolean) {
    exportError = false;
    const bounds = chart.getBoundingClientRect();
    const viewBox = chart.viewBox.baseVal;
    const legend: GraphExportLegendItem[] = showLegend ? traceList.map((trace, index) => ({
      color: trace.color ?? traceColors[index % traceColors.length],
      label: `${trace.pid === undefined ? trace.name : `PID ${trace.pid.toString(16).toUpperCase().padStart(2, "0")} · ${trace.name}`}${formatDerivedExpression(trace.formula)}`,
      value: `${trace.current ? trace.current.value.toFixed(1) : "—"} ${trace.unit}`,
    })) : [];
    const output = buildGraphExportSvg(chart.innerHTML, {
      width: bounds.width * 2,
      height: bounds.height * 2,
      viewBox: [viewBox.x, viewBox.y, viewBox.width, viewBox.height],
      title: graphPreferences.graphType === "xy_scatter" ? `${scatterY?.name ?? "Y"} vs ${scatterX?.name ?? "X"}` : showLegend ? "Selected signals" : traceList[0]?.name ?? "Graph",
      legend,
    });
    const svgUrl = URL.createObjectURL(new Blob([output.svg], { type: "image/svg+xml;charset=utf-8" }));

    try {
      const image = new Image();
      image.src = svgUrl;
      await image.decode();

      const canvas = document.createElement("canvas");
      canvas.width = output.width;
      canvas.height = output.height;
      const context = canvas.getContext("2d");
      if (!context) throw new Error("Canvas is unavailable.");
      context.drawImage(image, 0, 0, output.width, output.height);
      const png = await new Promise<Blob>((resolve, reject) => {
        canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error("PNG encoding failed.")), "image/png");
      });
      const pngUrl = URL.createObjectURL(png);
      const link = document.createElement("a");
      link.href = pngUrl;
      link.download = graphPngFilename(new Date());
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(pngUrl), 0);
    } catch (error) {
      console.error("Could not export graph as PNG.", error);
      exportError = true;
    } finally {
      URL.revokeObjectURL(svgUrl);
    }
  }

  function saveGraphPng(event: MouseEvent, traceList: ChartTrace[], showLegend: boolean) {
    const chart = (event.currentTarget as HTMLElement).closest(".chart-card")?.querySelector("svg");
    if (chart) void exportGraphPng(chart, traceList, showLegend);
  }

  onMount(() => {
    const capture = () => {
      const chart = document.querySelector<SVGSVGElement>(".chart-layout .chart-card svg");
      if (chart) void exportGraphPng(chart, graphMode === "overlay" ? traces : traces.slice(0, 1), graphMode === "overlay");
    };
    const reset = () => {
      graphPreferences = normalizeGraphPreferences(null);
      saveGraphPreferences(graphPreferences);
    };
    window.addEventListener("lognook-capture-screenshot", capture);
    window.addEventListener("lognook-reset-graph-preferences", reset);
    return () => {
      window.removeEventListener("lognook-capture-screenshot", capture);
      window.removeEventListener("lognook-reset-graph-preferences", reset);
    };
  });

  function startDrag(event: DragEvent, signalId: string) {
    draggedSignal = signalId;
    if (!event.dataTransfer) return;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", signalId);
  }

  function allowDrop(event: DragEvent) {
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  function dropGraph(event: DragEvent, targetId: string) {
    event.preventDefault();
    const sourceId = draggedSignal ?? event.dataTransfer?.getData("text/plain") ?? "";
    if (sourceId && sourceId !== targetId) onReorder(sourceId, targetId);
    draggedSignal = null;
  }

  function moveBy(signalId: string, offset: -1 | 1) {
    const index = traces.findIndex((trace) => trace.signalId === signalId);
    const target = traces[index + offset];
    if (target) onReorder(signalId, target.signalId);
  }
</script>

{#if showDisconnectedSamples}
  <div class="live-ended-banner" role="status">Disconnected · showing last received samples</div>
{/if}

{#snippet signalOptions(trace: ChartTrace)}
  {@const isDerived = trace.formula !== undefined}
  {@const preference = signalPreference(trace.signalId, isDerived)}
  <div class="signal-graph-options">
    <strong>{trace.name}</strong>
    {#if isDerived && trace.sourceSignalIds?.length}
      <label class="keep-source-line">
        <span>{trace.sourceSignalIds.length === 1 ? "Keep source line" : "Keep input lines"}</span>
        <input
          type="checkbox"
          aria-label={`Keep source line for ${trace.name}`}
          checked={trace.sourceSignalIds.every((id) => graphSignalIds.includes(id))}
          onchange={(event) => trace.sourceSignalIds?.forEach((id) => onToggleGraphSignal(id, event.currentTarget.checked))}
        />
      </label>
    {/if}
    {#if trace.pid !== undefined}
      <button class="derive-signal-button" type="button" title={`Create a derived signal from ${trace.name}`} aria-label={`Create a derived signal from ${trace.name}`} onclick={() => onCreateDerivedSignal({ id: trace.signalId, name: trace.name, unit: trace.unit })}>ƒx</button>
    {/if}
    <label><span>View</span><select aria-label={`${trace.name} graph view`} value={preference.view} onchange={(event) => updateSignalPreference(trace.signalId, { view: event.currentTarget.value as GraphView }, isDerived)}>
      <option value="line">Line</option><option value="scatter">Scatter</option><option value="bar">Bar</option><option value="heatmap">Heatmap</option>
    </select></label>
    <label><span>Minimum</span><input type="number" step="any" aria-label={`${trace.name} minimum`} value={preference.min ?? ""} placeholder="Auto" onchange={(event) => updateSignalPreference(trace.signalId, { min: parseOptionalRange(event.currentTarget.value) }, isDerived)} /></label>
    <label><span>Maximum</span><input type="number" step="any" aria-label={`${trace.name} maximum`} value={preference.max ?? ""} placeholder="Auto" onchange={(event) => updateSignalPreference(trace.signalId, { max: parseOptionalRange(event.currentTarget.value) }, isDerived)} /></label>
    <label><span>Line</span><select aria-label={`${trace.name} line style`} value={preference.lineStyle} onchange={(event) => updateSignalPreference(trace.signalId, { lineStyle: event.currentTarget.value as GraphLineStyle }, isDerived)}>
      <option value="solid">Solid</option><option value="dashed">Dashed</option><option value="dotted">Dotted</option>
    </select></label>
    <label><span>Thickness</span><input type="number" min="1" max="5" step="0.5" aria-label={`${trace.name} line thickness`} value={preference.lineWidth} onchange={(event) => updateSignalPreference(trace.signalId, { lineWidth: Number(event.currentTarget.value) }, isDerived)} /></label>
  </div>
{/snippet}

<div class="graph-control-row" class:has-reference={Boolean(graphPreferences.reference)}>
  <details class="graph-settings">
    <summary>
      <span class="graph-settings-title">Graph settings</span>
      <label class="graph-type-compact"><span>Graph</span><select aria-label="Graph type" value={graphPreferences.graphType} onclick={(event) => event.stopPropagation()} onchange={(event) => updateGraphType(event.currentTarget.value as GraphType)}>
        <option value="time_series">Time series</option><option value="xy_scatter">X/Y scatter</option>
      </select></label>
      <span class="graph-zoom-controls" role="group" aria-label="Graph zoom controls">
        <span class="graph-zoom-axis"><span>X</span><button type="button" aria-label={xZoomOutTitle} title={xZoomOutTitle} disabled={!canZoomX} onclick={(event) => handleZoomButton(event, () => zoomTimeAxis("out"))}>−</button><button type="button" aria-label={xZoomInTitle} title={xZoomInTitle} disabled={!canZoomX} onclick={(event) => handleZoomButton(event, () => zoomTimeAxis("in"))}>+</button></span>
        <span class="graph-zoom-axis"><span>Y</span><button type="button" aria-label={yZoomOutTitle} title={yZoomOutTitle} disabled={!canZoomY} onclick={(event) => handleZoomButton(event, () => zoomValueAxis("out"))}>−</button><button type="button" aria-label={yZoomInTitle} title={yZoomInTitle} disabled={!canZoomY} onclick={(event) => handleZoomButton(event, () => zoomValueAxis("in"))}>+</button></span>
        <button class="graph-zoom-auto" type="button" aria-label="Auto scale graph" title={autoZoomTitle} disabled={!autoZoomAvailable} onclick={(event) => handleZoomButton(event, autoZoom)}>Auto</button>
      </span>
    </summary>
    <div class="graph-settings-list">
      {#if graphPreferences.graphType === "xy_scatter"}
        <div class="scatter-axis-controls">
          <label><span>X axis</span><select aria-label="Scatter X axis" value={scatterX?.signalId ?? ""} onchange={(event) => updateScatterAxis("xSignalId", event.currentTarget.value)}>
            {#each traces as trace (trace.signalId)}<option value={trace.signalId}>{trace.name} · {trace.unit}</option>{/each}
          </select></label>
          <label><span>Y axis</span><select aria-label="Scatter Y axis" value={scatterY?.signalId ?? ""} onchange={(event) => updateScatterAxis("ySignalId", event.currentTarget.value)}>
            {#each traces as trace (trace.signalId)}<option value={trace.signalId}>{trace.name} · {trace.unit}</option>{/each}
          </select></label>
          <label><span>Color by</span><select aria-label="Scatter color grading" value={scatterColorId ?? ""} onchange={(event) => updateScatterAxis("colorSignalId", event.currentTarget.value)}>
            <option value="">Sample age</option>
            {#each traces as trace (trace.signalId)}<option value={trace.signalId}>{trace.name} · {trace.unit}</option>{/each}
          </select></label>
        </div>
      {/if}
      {#if !timeWindow}
        <label class="live-time-scale">
          <span>Live time scale (seconds)</span>
          <input type="number" min={MIN_LIVE_WINDOW_SECONDS} max={MAX_LIVE_WINDOW_SECONDS} step="0.5" aria-label="Live graph time scale in seconds" value={graphPreferences.liveWindowSeconds} onchange={(event) => updateLiveWindowSeconds(event.currentTarget.value)} />
          <small>0.5–600 seconds · saved with graph settings</small>
        </label>
      {/if}
      {#each traces as trace (trace.signalId)}
        {@render signalOptions(trace)}
      {/each}
    </div>
  </details>
  <div class="graph-reference-tools">
    <button class="button secondary graph-export-button graph-reference-bubble" onclick={saveReference} disabled={!traces.some((trace) => trace.samples.length)} title="Save the currently loaded samples as a persistent reference">Save reference</button>
    {#if graphPreferences.reference}
      <span role="status">Reference saved · {graphPreferences.reference.traces.length} signal{graphPreferences.reference.traces.length === 1 ? "" : "s"}</span>
      <button class="button secondary graph-export-button" onclick={() => {
        graphPreferences = normalizeGraphPreferences({ ...graphPreferences, reference: null });
        saveGraphPreferences(graphPreferences);
      }}>Clear reference</button>
    {/if}
  </div>
</div>

{#if graphPreferences.graphType === "xy_scatter"}
  <div class="chart-layout overlay-layout">
    <article class="panel chart-card">
      <div class="chart-heading combined-heading">
        <div><h3>X/Y scatter</h3><p>Time-aligned samples · color shows {scatterColorId ? traces.find((trace) => trace.signalId === scatterColorId)?.name : "sample age"}</p></div>
        <div class="graph-export-control">
          <button class="button secondary graph-export-button" title="Save graph as PNG" onclick={(event) => saveGraphPng(event, traces, false)}>Save PNG</button>
          {#if exportError}<span class="graph-export-error" role="status">PNG export failed</span>{/if}
        </div>
      </div>
      <ScatterPlot {traces} xSignalId={scatterX?.signalId ?? ""} ySignalId={scatterY?.signalId ?? ""} colorSignalId={scatterColorId} timeWindow={scatterWindow} {live}
        xMin={scatterX ? signalPreference(scatterX.signalId).min : null} xMax={scatterX ? signalPreference(scatterX.signalId).max : null}
        yMin={scatterY ? signalPreference(scatterY.signalId).min : null} yMax={scatterY ? signalPreference(scatterY.signalId).max : null} />
    </article>
  </div>
{:else if graphMode === "overlay"}
  <div class="chart-layout overlay-layout">
    <article class="panel chart-card">
      <div class="chart-heading combined-heading">
        <div><h3>Selected signals</h3><p>Shared time axis · each signal uses its own vertical scale</p></div>
        <div class="graph-export-control">
          <button class="button secondary graph-export-button" title="Save graph as PNG" onclick={(event) => saveGraphPng(event, traces, true)}>Save PNG</button>
          {#if exportError}<span class="graph-export-error" role="status">PNG export failed</span>{/if}
        </div>
      </div>
      <LiveChart {traces} showLegend={true} {timeWindow} {markers} {playheadNs} {onSeek} {onWheelScrub} {liveDisplayTimeNs} {liveWindowSpanNs} signalPreferences={graphPreferences.signals} referenceTraces={graphPreferences.reference?.traces ?? []} {focusedSignalId} onSelectFocusedTrace={(signalId) => selectedTraceId = signalId} />
    </article>
  </div>
{:else}
  <div class="chart-layout separate-layout">
    {#each traces as trace (trace.signalId)}
      <article
        class="panel chart-card"
        class:dragging={draggedSignal === trace.signalId}
        draggable={true}
        ondragstart={(event) => startDrag(event, trace.signalId)}
        ondragover={allowDrop}
        ondrop={(event) => dropGraph(event, trace.signalId)}
        ondragend={() => draggedSignal = null}
      >
        <div class="chart-heading">
          <div><h3><button class="graph-focus-title" class:focused={trace.signalId === focusedSignalId} type="button" aria-label={`Focus Y range for ${trace.name}`} aria-pressed={trace.signalId === focusedSignalId} title={`Focus Y range for ${trace.name}`} onclick={() => selectedTraceId = trace.signalId}>{trace.name}</button></h3><p>{trace.pid === undefined ? trace.signalId : `PID ${trace.pid.toString(16).toUpperCase().padStart(2, "0")}`} · {trace.unit}</p></div>
          <div class="graph-order-controls" aria-label="Reorder graph">
            <button aria-label={`Move ${trace.name} earlier`} title="Move earlier" onclick={() => moveBy(trace.signalId, -1)} disabled={traces[0]?.signalId === trace.signalId}>←</button>
            <button aria-label={`Move ${trace.name} later`} title="Move later" onclick={() => moveBy(trace.signalId, 1)} disabled={traces.at(-1)?.signalId === trace.signalId}>→</button>
          </div>
          <div class="graph-export-control">
            <button class="button secondary graph-export-button" title="Save graph as PNG" onclick={(event) => saveGraphPng(event, [trace], false)}>Save PNG</button>
            {#if exportError}<span class="graph-export-error" role="status">PNG export failed</span>{/if}
          </div>
          <div class="metric"><strong>{trace.current ? trace.current.value.toFixed(1) : "—"}</strong><span>{trace.unit}</span></div>
        </div>
        <LiveChart traces={[trace]} showLegend={false} {timeWindow} {markers} {playheadNs} {onSeek} {onWheelScrub} {liveDisplayTimeNs} {liveWindowSpanNs} signalPreferences={graphPreferences.signals} referenceTraces={graphPreferences.reference?.traces ?? []} />
        <div class="chart-footer"><span>{trace.current?.quality ?? "waiting"}</span>{#if trace.formula}<span class="chart-formula">{formatDerivedExpression(trace.formula)}</span>{/if}<span>{trace.sampleRate}</span></div>
      </article>
    {/each}
  </div>
{/if}

<style>
  .live-ended-banner { margin-bottom: 6px; border: 1px solid var(--border-soft); border-radius: 5px; padding: 4px 8px; color: var(--text-muted); font-size: 10px; }
  .graph-control-row { position: relative; min-width: 0; margin-bottom: 8px; }
  .graph-reference-tools { position: absolute; z-index: 1; top: 2px; right: 9px; display: flex; align-items: center; flex-wrap: nowrap; gap: 7px; min-width: 0; color: var(--text-muted); font-size: 10px; }
  .graph-reference-tools .graph-reference-bubble { min-height: 26px; padding-inline: 11px; border-radius: 999px; }
  .graph-settings { width: 100%; min-width: 0; margin: 0; border: 1px solid var(--border-soft); border-radius: 7px; background: var(--surface-deep); }
  .graph-settings summary { display: flex; align-items: center; gap: 8px; min-width: 0; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; padding: 4px 120px 4px 9px; color: var(--text-muted); font-size: 10px; cursor: pointer; }
  .graph-settings summary::-webkit-scrollbar { display: none; }
  .graph-settings-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .graph-type-compact { display: flex; flex: 0 0 auto; align-items: center; gap: 5px; color: var(--text-muted); font-size: 9px; }
  .graph-type-compact select { width: 112px; min-height: 25px; padding: 2px 5px; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--surface-deep); color: var(--text); font-size: 10px; }
  .graph-zoom-controls { display: flex; flex: 0 0 auto; align-items: center; gap: 4px; color: var(--text-muted); }
  .graph-zoom-axis { display: inline-flex; align-items: center; gap: 2px; padding-right: 4px; border-right: 1px solid var(--border-soft); }
  .graph-zoom-axis > span { padding-inline: 2px; font-size: 9px; font-weight: 700; }
  .graph-zoom-controls button { display: inline-grid; min-width: 24px; min-height: 24px; place-items: center; border: 1px solid var(--border-strong); border-radius: 4px; padding: 0 4px; background: var(--surface-deep); color: var(--text); font-size: 12px; line-height: 1; cursor: pointer; }
  .graph-zoom-controls button:hover:not(:disabled), .graph-zoom-controls button:focus-visible { border-color: var(--accent); }
  .graph-zoom-controls button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .graph-zoom-controls button:disabled { opacity: .45; cursor: not-allowed; }
  .graph-zoom-auto { min-width: 34px !important; font-size: 9px !important; }
  .graph-focus-title { border: 0; border-radius: 3px; padding: 1px 3px; background: transparent; color: inherit; font: inherit; cursor: pointer; }
  .graph-focus-title.focused { outline: 1px solid var(--accent); outline-offset: 1px; color: var(--text-bright); }
  .graph-focus-title:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .scatter-axis-controls { display: flex; flex-wrap: wrap; align-items: end; gap: 6px 12px; padding: 7px 9px; border-bottom: 1px solid var(--border-soft); color: var(--text-muted); font-size: 10px; }
  .scatter-axis-controls label { display: grid; gap: 3px; min-width: 0; }
  .scatter-axis-controls select { max-width: 220px; min-width: 110px; min-height: 25px; padding: 2px 5px; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--surface-deep); color: var(--text); font-size: 10px; }
  .graph-control-row.has-reference .graph-settings summary { padding-right: 340px; }
  .graph-settings-list { max-height: 180px; overflow: auto; border-top: 1px solid var(--border-soft); }
  .live-time-scale { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 9px; padding: 7px 9px; border-bottom: 1px solid var(--border-soft); color: var(--text-muted); font-size: 10px; }
  .live-time-scale input { width: 76px; min-height: 25px; padding: 2px 5px; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--surface-deep); color: var(--text); font-size: 10px; }
  .live-time-scale small { font-size: 9px; }
  .signal-graph-options { display: flex; align-items: center; flex-wrap: wrap; gap: 7px 10px; padding: 7px 9px; border-bottom: 1px solid var(--border-soft); }
  .signal-graph-options > strong { min-width: 105px; font-size: 10px; }
  .derive-signal-button { min-width: 26px; height: 25px; padding: 0 5px; border: 1px solid var(--border-strong); border-radius: 4px; color: var(--accent); background: var(--surface-deep); font-size: 10px; font-weight: 700; cursor: pointer; }
  .derive-signal-button:hover, .derive-signal-button:focus-visible { border-color: var(--accent); }
  .derive-signal-button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .signal-graph-options label { display: grid; gap: 2px; color: var(--text-muted); font-size: 9px; }
  .signal-graph-options select, .signal-graph-options input { width: 82px; min-height: 25px; padding: 2px 5px; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--surface-deep); color: var(--text); font-size: 10px; }
  .signal-graph-options input { width: 76px; }
  .signal-graph-options .keep-source-line { align-items: center; gap: 5px; }
  .signal-graph-options .keep-source-line input { width: auto; min-height: auto; padding: 0; accent-color: var(--accent); }
  .graph-export-control { display: flex; align-items: center; gap: 7px; }
  .graph-export-button { min-height: 28px; padding-inline: 8px; font-size: 10px; }
  .graph-export-error { color: #ffc3bb; font-size: 10px; white-space: nowrap; }
  @media (max-width: 640px) {
    .graph-settings summary { gap: 4px; }
    .graph-settings-title { flex: 0 1 0; max-width: 0; }
    .graph-type-compact > span { display: none; }
    .graph-type-compact select { width: 94px; }
  }
</style>
