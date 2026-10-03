<script lang="ts">
  import LiveChart from "./LiveChart.svelte";
  import type { ChartTrace, Marker } from "./types";
  import type { TimeWindow } from "./workspace";

  let { traces, graphMode, onReorder, timeWindow, markers = [], playheadNs, onSeek, onWheelScrub }: {
    traces: ChartTrace[];
    graphMode: "overlay" | "separate";
    onReorder: (fromId: string, toId: string) => void;
    timeWindow?: TimeWindow;
    markers?: Marker[];
    playheadNs?: number;
    onSeek?: (positionNs: number) => void;
    onWheelScrub?: (deltaY: number) => void;
  } = $props();
  let draggedSignal = $state<string | null>(null);

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

{#if graphMode === "overlay"}
  <div class="chart-layout overlay-layout">
    <article class="panel chart-card">
      <div class="chart-heading combined-heading"><div><h3>Selected signals</h3><p>Shared time axis · each signal uses its own vertical scale</p></div></div>
      <LiveChart {traces} showLegend={true} {timeWindow} {markers} {playheadNs} {onSeek} {onWheelScrub} />
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
          <div class="metric"><strong>{trace.current ? trace.current.value.toFixed(1) : "—"}</strong><span>{trace.unit}</span></div>
        </div>
        <LiveChart traces={[trace]} showLegend={false} {timeWindow} {markers} {playheadNs} {onSeek} {onWheelScrub} />
        <div class="chart-footer"><span>{trace.current?.quality ?? "waiting"}</span><span>{trace.sampleRate}</span></div>
      </article>
    {/each}
  </div>
{/if}
