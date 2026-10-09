<script lang="ts">
  import CanSignalEditor from "./CanSignalEditor.svelte";
  import { changedBits, discoverySeed } from "./canDiscovery";
  import type { CanEventCandidate, CanFrame, CanLiveEventGroup, CanLiveEventSnapshot, CanSignalDefinition } from "./types";
  import type { CanTrafficSummary } from "./canWorkbench";

  export let frames: CanFrame[] = [];
  export let trafficSummary: CanTrafficSummary[] = [];
  export let liveEventAnalysis: CanLiveEventSnapshot | null = null;
  export let fields: CanSignalDefinition[] = [];
  export let onSaveField: (field: CanSignalDefinition) => unknown = () => {};
  export let onRemoveField: (id: string) => unknown = () => {};
  export let onClearEvents: () => unknown = () => {};

  let trafficView: "summary" | "frames" | "events" = "summary";
  let eventFilter = "";
  let frozenEvents: CanLiveEventSnapshot | null = null;
  let pinnedId: { can_id: number; extended: boolean } | null = null;
  let editor: { frame: CanFrame; fieldId: string | null; seed?: ReturnType<typeof discoverySeed>; evidence?: string } | null = null;
  let actionError = "";
  $: trafficFrameCount = trafficSummary.reduce((total, row) => total + row.rx_count + row.tx_count, 0);
  $: eventSnapshot = frozenEvents ?? liveEventAnalysis;
  $: latestElapsedNs = frames[frames.length - 1]?.elapsed_ns ?? 0;
  $: if (frozenEvents && liveEventAnalysis && frozenEvents.capture_id !== liveEventAnalysis.capture_id) frozenEvents = null;

  function bytesHex(data: number[]): string {
    return data.map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
  }

  function canId(can_id: number, extended: boolean): string {
    return `0x${can_id.toString(16).toUpperCase().padStart(extended ? 8 : 3, "0")} · ${extended ? "29" : "11"}-bit`;
  }

  function latestRxFrame(can_id: number, extended: boolean): CanFrame | null {
    for (let index = frames.length - 1; index >= 0; index--) {
      const frame = frames[index];
      if (frame.direction === "rx" && frame.can_id === can_id && frame.extended === extended) return frame;
    }
    return null;
  }

  function openEditor(frame: CanFrame, fieldId: string | null = null) {
    editor = { frame: { ...frame, data: [...frame.data] }, fieldId };
  }

  function openTrafficEditor(can_id: number, extended: boolean) {
    const frame = latestRxFrame(can_id, extended);
    if (!frame) return;
    const field = fields.find((item) => item.can_id === can_id && item.extended === extended);
    openEditor(frame, field?.id ?? null);
  }

  function inspectCandidate(frame: CanFrame, candidate: CanEventCandidate, label: string, byte?: number, bit?: number) {
    editor = { frame, fieldId: null, seed: discoverySeed(candidate, label, byte, bit), evidence: eventBytes(candidate) };
  }

  function matchingCandidates(group: CanLiveEventGroup): Array<{
    candidate: CanEventCandidate;
    sample_frame: CanFrame | null;
    counter_like_bytes: number[];
    sum_checksum_like_bytes: number[];
  }> {
    const search = eventFilter.trim().toLowerCase();
    return group.candidates.filter(({ correlation }) => !search
      || canId(correlation.can_id, correlation.extended).toLowerCase().includes(search))
      .map(({ correlation: candidate, sample_frame, counter_like_bytes, sum_checksum_like_bytes }) => ({
        candidate, sample_frame, counter_like_bytes, sum_checksum_like_bytes,
      }));
  }

  function togglePinned(candidate: CanEventCandidate) {
    pinnedId = pinnedId?.can_id === candidate.can_id && pinnedId.extended === candidate.extended
      ? null
      : { can_id: candidate.can_id, extended: candidate.extended };
  }

  function eventBytes(candidate: CanEventCandidate): string {
    return candidate.changed_bytes.map((byte) => `B${byte.byte_index} ${byte.transitions.map((transition) =>
      `${transition.before === null ? "—" : `0x${transition.before.toString(16).padStart(2, "0").toUpperCase()}`}→${transition.after === null ? "—" : `0x${transition.after.toString(16).padStart(2, "0").toUpperCase()}`}${transition.before === null || transition.after === null ? "" : ` XOR 0x${(transition.before ^ transition.after).toString(16).padStart(2, "0").toUpperCase()}`} ×${transition.occurrences}`).join(", ")}`)
      .join(" · ") || (candidate.before_frames === 0 ? "appears after" : candidate.after_frames === 0 ? "stops after" : `${candidate.count_change_events} activity shifts`);
  }

  function frameRate(count: number, windowNs: number): string {
    return windowNs > 0 ? `${(count * 1e9 / windowNs).toFixed(1)} Hz` : "—";
  }

  async function clearEvents() {
    actionError = "";
    try { await onClearEvents(); frozenEvents = null; pinnedId = null; }
    catch (error) { actionError = String(error); }
  }

  function freezeEvents() {
    frozenEvents = frozenEvents ? null : liveEventAnalysis;
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
      <button class="can-view-option" aria-pressed={trafficView === "events"} onclick={() => trafficView = "events"}>Events</button>
    </div>
  </div>
  <p class="can-monitor-hint">{#if trafficView === "summary"}Counts since connection · RX period and rate are averages across received frames.{:else if trafficView === "frames"}Latest {frames.length} frames retained for inspection (up to 400).{:else}Mark each physical action during a paired capture; recurring RX changes rise to the top.{/if}</p>
  {#if trafficView === "summary"}
    <div class="can-table-wrap">
      <table aria-label="Live CAN ID counts">
        <thead><tr><th>ID</th><th>DLC</th><th>Data</th><th>Period</th><th>Count</th><th>Rate</th><th>ASCII</th><th>Field</th></tr></thead>
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
              <td><button class="can-small-action" type="button" disabled={!latestRxFrame(row.can_id, row.extended)} onclick={() => openTrafficEditor(row.can_id, row.extended)}>{fields.some((field) => field.can_id === row.can_id && field.extended === row.extended) ? "Edit" : "＋ Field"}</button></td>
            </tr>
          {:else}
            <tr><td colspan="8" class="can-muted">Waiting for Candlelight frames</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if trafficView === "frames"}
    <div class="can-table-wrap">
      <table aria-label="Recent CAN frames">
        <thead><tr><th>Time</th><th>Dir</th><th>ID</th><th>DLC</th><th>Data</th><th>Field</th></tr></thead>
        <tbody>
          {#each frames.slice(-80).reverse() as frame, index (`${frame.wall_time_unix_ns}:${frame.can_id}:${index}`)}
            <tr>
              <td>{(frame.elapsed_ns / 1e9).toFixed(3)}s</td><td>{frame.direction.toUpperCase()}</td>
              <td>{frame.extended ? "29" : "11"}-bit 0x{frame.can_id.toString(16).toUpperCase()}</td>
              <td>{frame.data.length}</td><td><code>{bytesHex(frame.data)}</code></td>
              <td><button class="can-small-action" type="button" disabled={frame.direction !== "rx"} onclick={() => openEditor(frame, fields.find((field) => field.can_id === frame.can_id && field.extended === frame.extended)?.id ?? null)}>＋ Field</button></td>
            </tr>
          {:else}
            <tr><td colspan="6" class="can-muted">Waiting for Candlelight frames</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="can-event-toolbar">
      <input class="text-input" aria-label="Filter CAN event candidates by ID" placeholder="Filter CAN ID…" bind:value={eventFilter} />
      <button class="can-small-action" type="button" onclick={freezeEvents} disabled={!liveEventAnalysis}>{frozenEvents ? "Resume live" : "Freeze ranking"}</button>
      <button class="can-small-action" type="button" onclick={clearEvents} disabled={!eventSnapshot || !eventSnapshot.active || !eventSnapshot.groups.length} title="Clears live results and pending analysis; capture evidence remains saved">Clear trials</button>
      {#if frozenEvents}<span class="can-muted">Frozen · capture {frozenEvents.capture_id}</span>{/if}
    </div>
    {#if pinnedId}<p class="can-pinned-id">Pinned {canId(pinnedId.can_id, pinnedId.extended)} <button class="can-small-action" type="button" onclick={() => pinnedId = null}>Unpin</button></p>{/if}
    {#if actionError}<p class="can-event-error" role="alert">{actionError}</p>{/if}
    <div class="can-event-list-wrap">
      {#if eventSnapshot?.groups.length}
        {#each eventSnapshot.groups as group (group.label)}
          {@const candidates = matchingCandidates(group)}
          <details class="can-event-group" open>
            <summary>{group.label} · {group.completed_events} complete · {group.pending_events} pending · {group.incomplete_events} incomplete</summary>
            {#if group.omitted_candidates}<p class="can-muted">{group.omitted_candidates} candidate observations omitted by the memory bound.</p>{/if}
            <div class="can-table-wrap can-candidate-wrap">
              <table aria-label={`CAN event candidates for ${group.label}`}>
                <thead><tr><th>Candidate ID</th><th>Changed / seen</th><th>RX frames · before / after rate</th><th>Byte transitions</th><th>Field</th></tr></thead>
                <tbody>
                  {#each candidates as item (`${item.candidate.can_id}:${item.candidate.extended}`)}
                    <tr class:pinned-candidate={pinnedId?.can_id === item.candidate.can_id && pinnedId.extended === item.candidate.extended}>
                      <td><button class="can-id-pin" type="button" onclick={() => togglePinned(item.candidate)} aria-pressed={pinnedId?.can_id === item.candidate.can_id && pinnedId.extended === item.candidate.extended}>{canId(item.candidate.can_id, item.candidate.extended)}</button></td>
                      <td>{item.candidate.changed_events}/{item.candidate.observed_events} observed · {item.candidate.observed_events}/{group.completed_events} trials</td>
                      <td>{item.candidate.before_frames} / {item.candidate.after_frames} · {frameRate(item.candidate.before_frames, item.candidate.before_window_ns)} / {frameRate(item.candidate.after_frames, item.candidate.after_window_ns)}</td>
                      <td>{eventBytes(item.candidate)}
                        <div class="can-byte-actions">{#each item.candidate.changed_bytes as byte}
                          <button class="can-small-action" disabled={!item.sample_frame} onclick={() => item.sample_frame && inspectCandidate(item.sample_frame, item.candidate, group.label, byte.byte_index)}>Scope B{byte.byte_index}</button>
                          {#each changedBits(item.candidate, byte.byte_index) as bit}<button class="can-small-action" disabled={!item.sample_frame} onclick={() => item.sample_frame && inspectCandidate(item.sample_frame, item.candidate, group.label, byte.byte_index, bit)}>B{byte.byte_index}.{bit}</button>{/each}
                        {/each}</div>
                        {#if item.counter_like_bytes.length || item.sum_checksum_like_bytes.length}<small class="can-noise-hint">Noise hint · {item.counter_like_bytes.length ? `counter-like B${item.counter_like_bytes.join(", B")}` : ""}{#if item.counter_like_bytes.length && item.sum_checksum_like_bytes.length} · {/if}{item.sum_checksum_like_bytes.length ? `sum-checksum-like B${item.sum_checksum_like_bytes.join(", B")}` : ""}</small>{/if}</td>
                      <td><button class="can-small-action" type="button" disabled={!item.sample_frame} onclick={() => item.sample_frame && inspectCandidate(item.sample_frame, item.candidate, group.label)}>Inspect → graph</button></td>
                    </tr>
                  {:else}
                    <tr><td colspan="5" class="can-muted">{eventFilter ? "No candidate IDs match this filter." : group.pending_events ? "Waiting for the post-event window to finish." : "No recurring changed IDs in the completed trials."}</td></tr>
                  {/each}
                </tbody>
              </table>
            </div>
            <p class="can-muted">The shortlist shows observed trials in its denominator. It is correlation evidence, not a decoded meaning.</p>
          </details>
        {/each}
      {:else}
        <p class="can-muted">{liveEventAnalysis ? "No event trials yet. Mark the same labeled action more than once to compare traffic." : "Start a paired CAN capture, then mark a vehicle action to begin."}</p>
      {/if}
    </div>
  {/if}
</section>

{#if editor}
  <CanSignalEditor
    frame={editor.frame}
    fields={fields}
    latestElapsedNs={latestElapsedNs}
    initialFieldId={editor.fieldId}
    initialStartBit={editor.seed?.startBit ?? 0}
    initialBitLength={editor.seed?.bitLength ?? 16}
    initialName={editor.seed?.name ?? ""}
    evidence={editor.evidence ?? ""}
    history={frames}
    onSave={onSaveField}
    onRemove={onRemoveField}
    onClose={() => editor = null}
  />
{/if}

<style>
  .can-byte-actions { display:flex; flex-wrap:wrap; gap:3px; margin-top:4px; }
  .can-traffic-panel { display:flex; flex-direction:column; gap:4px; min-width:0; min-height:0; overflow:hidden; padding:6px; }
  .can-monitor-toolbar { display:flex; align-items:center; gap:8px; min-width:0; }
  .can-monitor-title { min-width:0; }
  .can-monitor-title .eyebrow { margin:0 0 2px; font-size:9px; }
  .can-monitor-title h2 { margin:0; font-size:13px; }
  .count-pill { flex:0 0 auto; }
  .can-monitor-toolbar .count-pill { margin-left:auto; }
  .can-view-switch { display:inline-flex; flex:0 0 auto; gap:2px; }
  .can-view-option, .can-small-action { min-height:24px; padding:2px 8px; border:1px solid var(--border-strong); border-radius:5px; background:var(--surface-raised); color:var(--text-secondary); cursor:pointer; font-size:10px; }
  .can-view-option[aria-pressed="true"] { border-color:var(--accent); background:var(--accent); color:var(--accent-text); }
  .can-small-action:disabled { opacity:.45; cursor:not-allowed; }
  .can-monitor-hint, .can-muted { margin:0; color:var(--text-muted); font-size:9px; }
  .can-table-wrap { flex:1 1 auto; min-width:0; min-height:0; overflow:auto; border:1px solid var(--border); border-radius:6px; }
  table { width:100%; border-collapse:collapse; font-size:10px; font-variant-numeric:tabular-nums; }
  th,td { padding:3px 5px; border-bottom:1px solid var(--border-soft); text-align:left; white-space:nowrap; }
  th { position:sticky; top:0; z-index:1; background:var(--surface); color:var(--text-muted); }
  code { color:var(--text-primary); }
  .can-event-toolbar { display:flex; align-items:center; flex-wrap:wrap; gap:5px; }
  .can-event-toolbar input { width:min(220px, 45vw); min-height:26px; }
  .can-event-list-wrap { flex:1 1 auto; min-height:0; overflow:auto; display:grid; align-content:start; gap:5px; }
  .can-event-group { min-width:0; padding:5px; border:1px solid var(--border); border-radius:5px; }
  .can-event-group > summary { color:var(--text-secondary); font-size:10px; cursor:pointer; }
  .can-candidate-wrap { max-height:220px; margin-top:5px; }
  .can-id-pin { padding:0; border:0; color:var(--accent); background:transparent; font:inherit; cursor:pointer; }
  .can-id-pin[aria-pressed="true"], .pinned-candidate { background:color-mix(in srgb, var(--accent) 12%, transparent); }
  .can-pinned-id, .can-event-error { margin:0; color:var(--text-secondary); font-size:10px; }
  .can-event-error { color:var(--danger); }
  .can-noise-hint { display:block; margin-top:2px; color:var(--text-muted); white-space:normal; }
  @media (max-width:700px) { .can-monitor-toolbar { flex-wrap:wrap; } .can-monitor-toolbar .count-pill { margin-left:0; } .can-view-switch { margin-left:auto; } .can-candidate-wrap { max-height:45vh; } }
</style>
