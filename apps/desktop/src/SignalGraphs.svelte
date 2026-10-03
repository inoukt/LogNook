<script lang="ts">
  import { onMount } from "svelte";
  import LiveChart from "./LiveChart.svelte";
  import { buildGraphExportSvg, graphPngFilename, type GraphExportLegendItem } from "./graphExport";
  import type { ChartTrace, Marker } from "./types";
  import type { TimeWindow } from "./workspace";
  import type { DerivedSignalSource } from "./derivedSignals";
  import {
    captureGraphReference,
    defaultGraphSignalPreference,
    formatDerivedExpression,
    holdLastPointAtRightEdge,
    LIVE_GRAPH_WINDOW_NS,
    liveGraphTimeWindow,
    loadGraphPreferences,
    migrateDerivedGraphStyles,
    normalizeGraphPreferences,
    saveGraphPreferences,
    type GraphSignalPreference,
    type GraphView,
    type GraphLineStyle,
  } from "./graphPreferences";

  const traceColors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

  let { traces, graphMode, graphSignalIds, onReorder, onCreateDerivedSignal, onToggleGraphSignal, timeWindow, markers = [], playheadNs, onSeek, onWheelScrub, live = false }: {
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
    live?: boolean;
  } = $props();
  let draggedSignal = $state<string | null>(null);
  let exportError = $state(false);
  let graphPreferences = $state(loadGraphPreferences());
  let liveAnchor = $state<{ sampleNs: number; arrivedAtMs: number } | null>(null);
  let liveDisplayTimeNs = $state<number | undefined>();
  let lastPaintedTimeNs: number | undefined;
  const migratedDerivedStyles = new Set<string>();

  let latestLiveSampleNs = $derived.by(() => traces.reduce<number | undefined>((latest, trace) => {
    const sample = trace.samples.at(-1);
    return sample && (latest === undefined || sample.elapsed_ns > latest) ? sample.elapsed_ns : latest;
  }, undefined));
  let liveWindowSpanNs = $derived.by(() => {
    const times = traces.flatMap((trace) => trace.samples.map((sample) => sample.elapsed_ns));
    if (times.length < 2) return LIVE_GRAPH_WINDOW_NS;
    return Math.max(LIVE_GRAPH_WINDOW_NS, Math.max(...times) - Math.min(...times));
  });

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
    const legend: GraphExportLegendItem[] = showLegend ? traceList.map((trace, index) => ({
      color: trace.color ?? traceColors[index % traceColors.length],
      label: `${trace.pid === undefined ? trace.name : `PID ${trace.pid.toString(16).toUpperCase().padStart(2, "0")} · ${trace.name}`}${formatDerivedExpression(trace.formula)}`,
      value: `${trace.current ? trace.current.value.toFixed(1) : "—"} ${trace.unit}`,
    })) : [];
    const output = buildGraphExportSvg(chart.innerHTML, {
      width: bounds.width * 2,
      height: bounds.height * 2,
      title: showLegend ? "Selected signals" : traceList[0]?.name ?? "Graph",
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

<div class="graph-reference-tools">
  <button class="button secondary graph-export-button" onclick={saveReference} disabled={!traces.some((trace) => trace.samples.length)} title="Save the currently loaded samples as a persistent reference">Save reference</button>
  {#if graphPreferences.reference}
    <span role="status">Reference saved · {graphPreferences.reference.traces.length} signal{graphPreferences.reference.traces.length === 1 ? "" : "s"}</span>
    <button class="button secondary graph-export-button" onclick={() => {
      graphPreferences = normalizeGraphPreferences({ ...graphPreferences, reference: null });
      saveGraphPreferences(graphPreferences);
    }}>Clear reference</button>
  {:else}
    <span>Dashed overlay available in live and replay graphs</span>
  {/if}
</div>
<details class="graph-settings">
  <summary>Graph settings · ranges, styles, and views</summary>
  <div class="graph-settings-list">
    {#each traces as trace (trace.signalId)}
      {@render signalOptions(trace)}
    {/each}
  </div>
</details>

{#if graphMode === "overlay"}
  <div class="chart-layout overlay-layout">
    <article class="panel chart-card">
      <div class="chart-heading combined-heading">
        <div><h3>Selected signals</h3><p>Shared time axis · each signal uses its own vertical scale</p></div>
        <div class="graph-export-control">
          <button class="button secondary graph-export-button" title="Save graph as PNG" onclick={(event) => saveGraphPng(event, traces, true)}>Save PNG</button>
          {#if exportError}<span class="graph-export-error" role="status">PNG export failed</span>{/if}
        </div>
      </div>
      <LiveChart {traces} showLegend={true} {timeWindow} {markers} {playheadNs} {onSeek} {onWheelScrub} {liveDisplayTimeNs} {liveWindowSpanNs} signalPreferences={graphPreferences.signals} referenceTraces={graphPreferences.reference?.traces ?? []} />
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
          <div><h3>{trace.name}</h3><p>{trace.pid === undefined ? trace.signalId : `PID ${trace.pid.toString(16).toUpperCase().padStart(2, "0")}`} · {trace.unit}</p></div>
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
  .graph-reference-tools { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; margin-bottom: 7px; color: var(--text-muted); font-size: 10px; }
  .graph-settings { margin-bottom: 8px; border: 1px solid #2b3944; border-radius: 7px; background: #131c23; }
  .graph-settings summary { padding: 7px 9px; color: var(--text-muted); font-size: 10px; cursor: pointer; }
  .graph-settings-list { max-height: 180px; overflow: auto; border-top: 1px solid #2b3944; }
  .signal-graph-options { display: flex; align-items: center; flex-wrap: wrap; gap: 7px 10px; padding: 7px 9px; border-bottom: 1px solid #24313b; }
  .signal-graph-options > strong { min-width: 105px; font-size: 10px; }
  .derive-signal-button { min-width: 26px; height: 25px; padding: 0 5px; border: 1px solid #344450; border-radius: 4px; color: var(--accent); background: #0d151b; font-size: 10px; font-weight: 700; cursor: pointer; }
  .derive-signal-button:hover, .derive-signal-button:focus-visible { border-color: var(--accent); }
  .derive-signal-button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .signal-graph-options label { display: grid; gap: 2px; color: var(--text-muted); font-size: 9px; }
  .signal-graph-options select, .signal-graph-options input { width: 82px; min-height: 25px; padding: 2px 5px; border: 1px solid #344450; border-radius: 4px; background: #0d151b; color: #e7edf3; font-size: 10px; }
  .signal-graph-options input { width: 76px; }
  .signal-graph-options .keep-source-line { align-items: center; gap: 5px; }
  .signal-graph-options .keep-source-line input { width: auto; min-height: auto; padding: 0; accent-color: var(--accent); }
  .graph-export-control { display: flex; align-items: center; gap: 7px; }
  .graph-export-button { min-height: 28px; padding-inline: 8px; font-size: 10px; }
  .graph-export-error { color: #ffc3bb; font-size: 10px; white-space: nowrap; }
</style>
