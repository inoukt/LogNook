<script lang="ts">
  import CanSignalEditor from "./CanSignalEditor.svelte";
  import CanEventSnapshotsDialog from "./CanEventSnapshotsDialog.svelte";
  import { changedBits, discoverySeed } from "./canDiscovery";
  import { canHelpBubblesEnabled } from "./canHelp";
  import type { CanEventCandidate, CanFrame, CanIdReplayStatus, CanLiveEventGroup, CanLiveEventSnapshot, CanSignalDefinition } from "./types";
  import {
    createCanIdReplaySnapshot, resetCanIdReplaySelectionForGeneration,
    type CanIdReplaySelection, type CanTrafficSummary,
  } from "./canWorkbench";

  export let frames: CanFrame[] = [];
  export let trafficSummary: CanTrafficSummary[] = [];
  export let liveEventAnalysis: CanLiveEventSnapshot | null = null;
  export let fields: CanSignalDefinition[] = [];
  export let onSaveField: (field: CanSignalDefinition) => unknown = () => {};
  export let onRemoveField: (id: string) => unknown = () => {};
  export let onClearEvents: () => unknown = () => {};
  export let canIdReplayEnabled = false;
  export let canIdReplayGeneration = 0;
  export let replayStartPending = false;
  export let replayStopPending = false;
  export let replayStatus: CanIdReplayStatus | null = null;
  export let replayError = "";
  export let onStartCanIdReplay: (frames: CanFrame[]) => Promise<unknown> = async () => {};
  export let onStopCanIdReplay: () => Promise<unknown> = async () => {};

  let trafficView: "summary" | "frames" | "events" = "summary";
  let idFilter = "";
  let idFormatFilter: "all" | "11" | "29" = "all";
  let directionFilter: "all" | "rx" | "tx" = "all";
  let minimumRate = "";
  let summarySort: "count" | "rx" | "tx" | "rate" | "period" | "id" = "count";
  let framesSort: "rate" | "latest" = "rate";
  let eventFilter = "";
  let frozenEvents: CanLiveEventSnapshot | null = null;
  let selectedSnapshotLabel: string | null = null;
  let selectedSnapshotCaptureId: number | null = null;
  let pinnedId: { can_id: number; extended: boolean } | null = null;
  let editor: { frame: CanFrame; fieldId: string | null; seed?: ReturnType<typeof discoverySeed>; evidence?: string } | null = null;
  let actionError = "";
  let replaySelection: CanIdReplaySelection = { generation: canIdReplayGeneration, snapshot: null, error: "" };
  $: trafficFrameCount = trafficSummary.reduce((total, row) => total + row.rx_count + row.tx_count, 0);
  $: eventSnapshot = frozenEvents ?? liveEventAnalysis;
  $: if (selectedSnapshotLabel && (!liveEventAnalysis
    || liveEventAnalysis.capture_id !== selectedSnapshotCaptureId
    || !eventSnapshot
    || eventSnapshot.capture_id !== selectedSnapshotCaptureId
    || !eventSnapshot.groups.some((group) => group.label === selectedSnapshotLabel))) {
    selectedSnapshotLabel = null;
    selectedSnapshotCaptureId = null;
  }
  $: selectedSnapshotGroup = selectedSnapshotLabel && eventSnapshot?.capture_id === selectedSnapshotCaptureId
    ? eventSnapshot.groups.find((group) => group.label === selectedSnapshotLabel) ?? null
    : null;
  $: latestElapsedNs = frames[frames.length - 1]?.elapsed_ns ?? 0;
  $: if (frozenEvents && liveEventAnalysis && frozenEvents.capture_id !== liveEventAnalysis.capture_id) frozenEvents = null;
  $: filteredTrafficSummary = filterTrafficSummary(trafficSummary, idFilter, idFormatFilter, directionFilter, minimumRate, summarySort);
  $: maxFrameCount = Math.max(1, ...filteredTrafficSummary.map((row) => row.rx_count + row.tx_count));
  $: maxFrameRate = Math.max(1, ...filteredTrafficSummary.map((row) => row.rate_hz ?? 0));
  $: rateById = new Map(trafficSummary.map((row) => [`${row.extended ? 1 : 0}:${row.can_id}`, row.rate_hz]));
  $: filteredFrames = filterTrafficFrames(frames, rateById, idFilter, idFormatFilter, directionFilter, minimumRate, framesSort);
  $: replayableIdKeys = new Set(frames.filter((frame) => frame.direction === "rx" && frame.flags === 0
    && Number.isSafeInteger(frame.elapsed_ns) && frame.elapsed_ns >= 0
    && frame.can_id <= (frame.extended ? 0x1fff_ffff : 0x7ff)
    && frame.data.length <= 8 && frame.data.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 0xff))
    .map((frame) => `${frame.extended ? 1 : 0}:${frame.can_id}`));
  $: if (replaySelection.generation !== canIdReplayGeneration) {
    replaySelection = resetCanIdReplaySelectionForGeneration(replaySelection, canIdReplayGeneration);
  }

  function byteHue(byte: number): number {
    return Math.round(220 - Math.min(Math.max(byte, 0), 255) / 255 * 220);
  }

  function gradePercent(value: number | null, maximum: number): number {
    return value === null || value <= 0 ? 0 : Math.round((12 + Math.log1p(value) / Math.log1p(Math.max(maximum, 1)) * 40) * 10) / 10;
  }

  function asciiCategory(byte: number): "nonprintable" | "letter" | "digit" | "punctuation" {
    if (byte < 0x20 || byte > 0x7e) return "nonprintable";
    const char = String.fromCharCode(byte);
    if (/[A-Za-z]/.test(char)) return "letter";
    if (/[0-9]/.test(char)) return "digit";
    return "punctuation";
  }

  function asciiCharacter(byte: number): string {
    return byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : ".";
  }

  function matchesCanId(can_id: number, extended: boolean, query: string, format: "all" | "11" | "29"): boolean {
    if ((format === "11" && extended) || (format === "29" && !extended)) return false;
    const normalized = query.trim().toLowerCase();
    if (!normalized) return true;
    const hex = can_id.toString(16).toLowerCase();
    const padded = hex.padStart(extended ? 8 : 3, "0");
    return `0x${padded}`.includes(normalized)
      || `0x${hex}`.includes(normalized)
      || (extended ? "29-bit" : "11-bit").includes(normalized);
  }

  function filterTrafficSummary(
    rows: CanTrafficSummary[],
    query: string,
    format: "all" | "11" | "29",
    direction: "all" | "rx" | "tx",
    minRateText: string,
    sort: "count" | "rx" | "tx" | "rate" | "period" | "id",
  ): CanTrafficSummary[] {
    const minRate = Number(minRateText);
    const threshold = minRateText.trim() && Number.isFinite(minRate) ? Math.max(0, minRate) : 0;
    return rows.filter((row) => matchesCanId(row.can_id, row.extended, query, format)
      && (direction === "all" || (direction === "rx" ? row.rx_count > 0 : row.tx_count > 0))
      && (threshold === 0 || (row.rate_hz !== null && row.rate_hz >= threshold)))
      .sort((left, right) => {
        if (sort === "id") return Number(left.extended) - Number(right.extended) || left.can_id - right.can_id;
        if (sort === "period") {
          const a = left.period_ms ?? Infinity;
          const b = right.period_ms ?? Infinity;
          return a - b || Number(left.extended) - Number(right.extended) || left.can_id - right.can_id;
        }
        const value = (row: CanTrafficSummary) => sort === "count" ? row.rx_count + row.tx_count
          : sort === "rx" ? row.rx_count
            : sort === "tx" ? row.tx_count
              : row.rate_hz ?? -Infinity;
        const leftValue = value(left);
        const rightValue = value(right);
        if (leftValue === rightValue) return Number(left.extended) - Number(right.extended) || left.can_id - right.can_id;
        if (leftValue === -Infinity) return 1;
        if (rightValue === -Infinity) return -1;
        return rightValue - leftValue || Number(left.extended) - Number(right.extended) || left.can_id - right.can_id;
      });
  }

  function filterTrafficFrames(
    input: CanFrame[],
    rates: Map<string, number | null>,
    query: string,
    format: "all" | "11" | "29",
    direction: "all" | "rx" | "tx",
    minRateText: string,
    sort: "rate" | "latest",
  ): CanFrame[] {
    const minRate = Number(minRateText);
    const threshold = minRateText.trim() && Number.isFinite(minRate) ? Math.max(0, minRate) : 0;
    return input.filter((frame) => {
      if (!matchesCanId(frame.can_id, frame.extended, query, format)) return false;
      if (direction !== "all" && frame.direction !== direction) return false;
      if (threshold > 0) {
        const rate = rates.get(`${frame.extended ? 1 : 0}:${frame.can_id}`);
        if (rate === null || rate === undefined || rate < threshold) return false;
      }
      return true;
    }).sort((left, right) => {
      if (sort === "latest") return right.elapsed_ns - left.elapsed_ns;
      const leftRate = rates.get(`${left.extended ? 1 : 0}:${left.can_id}`) ?? null;
      const rightRate = rates.get(`${right.extended ? 1 : 0}:${right.can_id}`) ?? null;
      if (leftRate === rightRate) return right.elapsed_ns - left.elapsed_ns;
      if (leftRate === null) return 1;
      if (rightRate === null) return -1;
      return rightRate - leftRate || right.elapsed_ns - left.elapsed_ns;
    });
  }

  function canId(can_id: number, extended: boolean): string {
    return `0x${can_id.toString(16).toUpperCase().padStart(extended ? 8 : 3, "0")} · ${extended ? "29" : "11"}-bit`;
  }

  function selectReplayId(can_id: number, extended: boolean) {
    replaySelection = {
      ...replaySelection,
      snapshot: createCanIdReplaySnapshot(frames, can_id, extended),
      error: "",
    };
  }

  async function startIdReplay() {
    const { snapshot, generation } = replaySelection;
    if (!snapshot?.frames.length || snapshot.has_gap_over_limit || !canIdReplayEnabled || replayStartPending || replayStatus?.state === "running") return;
    replaySelection = { ...replaySelection, error: "" };
    try {
      await onStartCanIdReplay(snapshot.frames);
    } catch (error) {
      if (replaySelection.generation === generation && replaySelection.snapshot === snapshot) {
        replaySelection = { ...replaySelection, error: String(error) };
      }
    }
  }

  async function stopIdReplay() {
    if (replayStatus?.state !== "running" || replayStopPending) return;
    const { generation, snapshot } = replaySelection;
    replaySelection = { ...replaySelection, error: "" };
    try {
      await onStopCanIdReplay();
    } catch (error) {
      if (replaySelection.generation === generation && replaySelection.snapshot === snapshot) {
        replaySelection = { ...replaySelection, error: String(error) };
      }
    }
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

  function openSnapshotComparison(label: string) {
    if (!eventSnapshot?.groups.some((group) => group.label === label)) return;
    selectedSnapshotLabel = label;
    selectedSnapshotCaptureId = eventSnapshot.capture_id;
  }
</script>

<section class="can-traffic-panel panel" aria-label="CAN bus traffic monitor">
  <div class="can-monitor-toolbar">
    <div class="can-monitor-title">
      <p class="eyebrow">LIVE CAN BUS</p>
      <h2>Traffic monitor</h2>
    </div>
    <span class="count-pill" title={$canHelpBubblesEnabled ? "Traffic totals and per-ID counts reset with the CAN connection." : undefined}>{trafficFrameCount.toLocaleString()} frames · {trafficSummary.length} IDs</span>
    <div class="can-view-switch" role="group" aria-label="CAN traffic view">
      <button class="can-view-option" aria-pressed={trafficView === "summary"} onclick={() => trafficView = "summary"} title={$canHelpBubblesEnabled ? "Show per-ID RX and TX totals observed since connection." : undefined}>Counts</button>
      <button class="can-view-option" aria-pressed={trafficView === "frames"} onclick={() => trafficView = "frames"} title={$canHelpBubblesEnabled ? "Show the latest 80 frames from up to 400 retained; an active capture records the full raw traffic." : undefined}>Frames</button>
      <button class="can-view-option" aria-pressed={trafficView === "events"} onclick={() => trafficView = "events"} title={$canHelpBubblesEnabled ? "Compare byte changes around marked actions; these correlations are not decoded signal meanings." : undefined}>Events</button>
    </div>
  </div>
  <p class="can-monitor-hint">{#if trafficView === "summary"}Counts since connection · RX period and rate are averages across received frames.{:else if trafficView === "frames"}Showing {Math.min(filteredFrames.length, 80)} of {filteredFrames.length} matching frames; sorted by ID rate or time.{:else}Mark each physical action during a paired capture; recurring RX changes rise to the top.{/if}</p>
  {#if replaySelection.snapshot}
    {@const selectedReplaySnapshot = replaySelection.snapshot}
    <section class="can-id-replay" aria-label="CAN ID replay source">
      <div class="can-id-replay-details">
        <strong>Frozen RX snapshot · {canId(selectedReplaySnapshot.can_id, selectedReplaySnapshot.extended)}</strong>
        <span>{selectedReplaySnapshot.frames.length.toLocaleString()} RX {selectedReplaySnapshot.frames.length === 1 ? "frame" : "frames"} · {(selectedReplaySnapshot.history_span_ns / 1e9).toFixed(3)} s captured span</span>
        <span>Snapshot covers only the current retained ring (up to 400 frames), not the full CSV capture. One pass; captured inter-frame timing with a 1 ms minimum.</span>
        {#if selectedReplaySnapshot.skipped_frames}<span class="can-muted">{selectedReplaySnapshot.skipped_frames} matching RX {selectedReplaySnapshot.skipped_frames === 1 ? "frame was" : "frames were"} skipped because flags, timestamp, or classic CAN payload was invalid.</span>{/if}
        {#if selectedReplaySnapshot.has_gap_over_limit}<span class="can-error" role="alert">Cannot replay: a captured inter-frame gap exceeds 60,000 ms. The backend rejects the whole replay before transmission.</span>{/if}
        {#if replayStatus}<span class="can-replay-status" role="status">Replay {replayStatus.state} · {replayStatus.sent_frames.toLocaleString()}/{replayStatus.total_frames.toLocaleString()} TX frames submitted · {canId(replayStatus.can_id, replayStatus.extended)}{#if replayStatus.error} · {replayStatus.error}{/if}</span>{/if}
        {#if replayError || replaySelection.error}<span class="can-error" role="alert">{replayError || replaySelection.error}</span>{/if}
      </div>
      <div class="can-id-replay-actions">
        <button class="button primary" type="button" onclick={startIdReplay}
          disabled={!canIdReplayEnabled || replayStartPending || replayStopPending || replayStatus?.state === "running" || !selectedReplaySnapshot.frames.length || selectedReplaySnapshot.has_gap_over_limit}
          title={$canHelpBubblesEnabled ? "Transmit this frozen snapshot once, in captured order and timing. Row selection itself never transmits." : undefined}>Start TX replay · once</button>
        {#if replayStatus?.state === "running"}
          <button class="button danger" type="button" onclick={stopIdReplay} disabled={replayStopPending}>Stop replay</button>
        {/if}
        <button class="can-small-action" type="button" onclick={() => replaySelection = { ...replaySelection, snapshot: null, error: "" }} disabled={replayStatus?.state === "running"}>Clear snapshot</button>
      </div>
      {#if !canIdReplayEnabled && replayStatus?.state !== "running"}<p class="can-id-replay-hint">Replay needs an active CAN connection, an open paired capture, and no scan or active replay.</p>{/if}
    </section>
  {/if}
  {#if trafficView !== "events"}
    <div class="can-traffic-filters" role="group" aria-label="Filter and order CAN traffic">
      <input class="text-input" type="search" aria-label="Filter CAN IDs" placeholder="Filter ID (0x7E8)…" bind:value={idFilter} />
      <select class="text-input" aria-label="Filter CAN ID format" value={idFormatFilter} onchange={(event) => idFormatFilter = event.currentTarget.value as typeof idFormatFilter}>
        <option value="all">11 + 29-bit</option><option value="11">11-bit</option><option value="29">29-bit</option>
      </select>
      <select class="text-input" aria-label="Filter CAN direction" value={directionFilter} onchange={(event) => directionFilter = event.currentTarget.value as typeof directionFilter}>
        <option value="all">RX + TX</option><option value="rx">RX only</option><option value="tx">TX only</option>
      </select>
      <label class="can-min-rate">Min Hz<input class="text-input" type="number" min="0" step="0.1" aria-label="Minimum CAN refresh rate in hertz" placeholder="Any" value={minimumRate} oninput={(event) => minimumRate = event.currentTarget.value} /></label>
      {#if trafficView === "summary"}
        <select class="text-input" aria-label="Sort CAN IDs" value={summarySort} onchange={(event) => summarySort = event.currentTarget.value as typeof summarySort}>
          <option value="count">Frame count ↓</option><option value="rx">RX count ↓</option><option value="tx">TX count ↓</option>
          <option value="rate">Rate (Hz) ↓</option><option value="period">Period (ms) ↑</option><option value="id">CAN ID ↑</option>
        </select>
      {:else}
        <select class="text-input" aria-label="Order CAN frames" value={framesSort} onchange={(event) => framesSort = event.currentTarget.value as typeof framesSort}>
          <option value="rate">ID rate (Hz) ↓</option><option value="latest">Newest frame ↓</option>
        </select>
      {/if}
      <span class="can-color-guide"><span class="can-ascii-letter">A</span> letters · <span class="can-ascii-digit">1</span> digits · <span class="can-ascii-punctuation">!</span> symbols · <span class="can-ascii-nonprintable">·</span> nonprintable · hex hue low→high</span>
    </div>
  {/if}
  {#if trafficView === "summary"}
    <div class="can-table-wrap">
      <table aria-label="Live CAN ID counts">
        <thead><tr>
          <th title={$canHelpBubblesEnabled ? "CAN identifier and 11-bit or 29-bit frame format." : undefined}>ID</th>
          <th title={$canHelpBubblesEnabled ? "Payload byte count in the most recent frame for this ID." : undefined}>DLC</th>
          <th title={$canHelpBubblesEnabled ? "Most recent payload bytes and their direction." : undefined}>Data</th>
          <th title={$canHelpBubblesEnabled ? "Average time between received frames for this ID." : undefined}>Period</th>
          <th title={$canHelpBubblesEnabled ? "Received and transmitted frame totals since this CAN connection." : undefined}>Count</th>
          <th title={$canHelpBubblesEnabled ? "Average received frames per second since the first frame for this ID." : undefined}>Rate</th>
          <th title={$canHelpBubblesEnabled ? "Printable ASCII view of the latest payload; non-printable bytes appear as dots." : undefined}>ASCII</th>
          <th>Field</th>
          <th>Replay</th>
        </tr></thead>
        <tbody>
          {#each filteredTrafficSummary as row (`${row.can_id}:${row.extended}`)}
            <tr>
              <td>{row.extended ? "29" : "11"}-bit 0x{row.can_id.toString(16).toUpperCase()}</td>
              <td>{row.last_data.length}</td>
              <td><code class="can-hex-data">{#each row.last_data as byte, index (`${index}:${byte}`)}<span class="can-hex-byte" style={`--byte-hue:${byteHue(byte)}deg`} title={$canHelpBubblesEnabled ? `Byte ${index}: 0x${byte.toString(16).padStart(2, "0").toUpperCase()}` : undefined}>{byte.toString(16).padStart(2, "0").toUpperCase()}</span>{/each}</code> · {row.last_direction.toUpperCase()}</td>
              <td>{#if row.period_ms === null}—{:else}<span class="can-metric-grade" style={`--metric-grade:${gradePercent(row.rate_hz, maxFrameRate)}%`}>{row.period_ms.toFixed(row.period_ms < 10 ? 2 : 1)} ms</span>{/if}</td>
              <td><span class="can-metric-grade" style={`--metric-grade:${gradePercent(row.rx_count + row.tx_count, maxFrameCount)}%`}>{row.rx_count.toLocaleString()} RX / {row.tx_count.toLocaleString()} TX</span></td>
              <td>{#if row.rate_hz === null}—{:else}<span class="can-metric-grade" style={`--metric-grade:${gradePercent(row.rate_hz, maxFrameRate)}%`}>{row.rate_hz.toFixed(1)} Hz</span>{/if}</td>
              <td><code class="can-ascii">{#each row.last_data as byte, index (`${index}:${byte}`)}<span class={`can-ascii-${asciiCategory(byte)}`} title={$canHelpBubblesEnabled ? `Byte ${index}: 0x${byte.toString(16).padStart(2, "0").toUpperCase()}` : undefined}>{asciiCharacter(byte)}</span>{/each}</code></td>
              <td><button class="can-small-action" type="button" disabled={!latestRxFrame(row.can_id, row.extended)} onclick={() => openTrafficEditor(row.can_id, row.extended)} title={$canHelpBubblesEnabled ? "Open the signal editor from the latest received frame for this ID." : undefined}>{fields.some((field) => field.can_id === row.can_id && field.extended === row.extended) ? "Edit" : "＋ Field"}</button></td>
              <td><button class="can-small-action" type="button" disabled={!replayableIdKeys.has(`${row.extended ? 1 : 0}:${row.can_id}`)} onclick={() => selectReplayId(row.can_id, row.extended)} title={$canHelpBubblesEnabled ? "Freeze valid RX frames for this exact ID and frame format from the current 400-frame retained ring. This does not transmit." : undefined}>Select ID</button></td>
            </tr>
          {:else}
            <tr><td colspan="9" class="can-muted">{trafficSummary.length ? "No CAN IDs match these filters" : "Waiting for Candlelight frames"}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if trafficView === "frames"}
    <div class="can-table-wrap">
      <table aria-label="Recent CAN frames">
        <thead><tr>
          <th title={$canHelpBubblesEnabled ? "Elapsed time for this frame." : undefined}>Time</th>
          <th title={$canHelpBubblesEnabled ? "Whether this frame was received (RX) or transmitted (TX)." : undefined}>Dir</th>
          <th title={$canHelpBubblesEnabled ? "CAN identifier and 11-bit or 29-bit frame format." : undefined}>ID</th>
          <th title={$canHelpBubblesEnabled ? "Average received frame rate for this CAN ID" : undefined}>ID Hz</th>
          <th title={$canHelpBubblesEnabled ? "Payload byte count in this frame." : undefined}>DLC</th>
          <th title={$canHelpBubblesEnabled ? "Raw bytes in this frame; hue tracks byte value from low to high." : undefined}>Data</th>
          <th title={$canHelpBubblesEnabled ? "Printable characters are colored by category; dots represent nonprintable bytes" : undefined}>ASCII</th>
          <th>Field</th>
          <th>Replay</th>
        </tr></thead>
        <tbody>
          {#each filteredFrames.slice(0, 80) as frame, index (`${frame.wall_time_unix_ns}:${frame.can_id}:${index}`)}
            {@const frameRate = rateById.get(`${frame.extended ? 1 : 0}:${frame.can_id}`) ?? null}
            <tr>
              <td>{(frame.elapsed_ns / 1e9).toFixed(3)}s</td><td>{frame.direction.toUpperCase()}</td>
              <td>{frame.extended ? "29" : "11"}-bit 0x{frame.can_id.toString(16).toUpperCase()}</td>
              <td>{#if frameRate === null}—{:else}<span class="can-metric-grade" style={`--metric-grade:${gradePercent(frameRate, maxFrameRate)}%`}>{frameRate.toFixed(1)} Hz</span>{/if}</td>
              <td>{frame.data.length}</td>
              <td><code class="can-hex-data">{#each frame.data as byte, byteIndex (`${byteIndex}:${byte}`)}<span class="can-hex-byte" style={`--byte-hue:${byteHue(byte)}deg`} title={$canHelpBubblesEnabled ? `Byte ${byteIndex}: 0x${byte.toString(16).padStart(2, "0").toUpperCase()}` : undefined}>{byte.toString(16).padStart(2, "0").toUpperCase()}</span>{/each}</code></td>
              <td><code class="can-ascii">{#each frame.data as byte, byteIndex (`${byteIndex}:${byte}`)}<span class={`can-ascii-${asciiCategory(byte)}`} title={$canHelpBubblesEnabled ? `Byte ${byteIndex}: 0x${byte.toString(16).padStart(2, "0").toUpperCase()}` : undefined}>{asciiCharacter(byte)}</span>{/each}</code></td>
              <td><button class="can-small-action" type="button" disabled={frame.direction !== "rx"} onclick={() => openEditor(frame, fields.find((field) => field.can_id === frame.can_id && field.extended === frame.extended)?.id ?? null)} title={$canHelpBubblesEnabled ? "Open the signal editor from this received frame and select any saved field for its ID." : undefined}>＋ Field</button></td>
              <td><button class="can-small-action" type="button" disabled={frame.direction !== "rx" || !replayableIdKeys.has(`${frame.extended ? 1 : 0}:${frame.can_id}`)} onclick={() => selectReplayId(frame.can_id, frame.extended)} title={$canHelpBubblesEnabled ? "Freeze all valid RX frames for this exact ID and frame format from the current retained ring. This does not transmit." : undefined}>Select ID</button></td>
            </tr>
          {:else}
            <tr><td colspan="9" class="can-muted">{frames.length ? "No frames match these filters" : "Waiting for Candlelight frames"}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="can-event-toolbar">
      <input class="text-input" aria-label="Filter CAN event candidates by ID" placeholder="Filter CAN ID…" bind:value={eventFilter} title={$canHelpBubblesEnabled ? "Filter each event group's candidate list by matching CAN ID text." : undefined} />
      <button class="can-small-action" type="button" onclick={freezeEvents} disabled={!liveEventAnalysis} title={$canHelpBubblesEnabled ? (frozenEvents ? "Resume following new event analysis; adapter polling and capture kept running while frozen." : "Hold the current event ranking while adapter polling and capture continue.") : undefined}>{frozenEvents ? "Resume live" : "Freeze ranking"}</button>
      <button class="can-small-action" type="button" onclick={clearEvents} disabled={!eventSnapshot || !eventSnapshot.active || !eventSnapshot.groups.length} title={$canHelpBubblesEnabled ? "Clear live trial results and pending analysis; saved capture evidence remains." : undefined}>Clear trials</button>
      {#if frozenEvents}<span class="can-muted">Frozen · capture {frozenEvents.capture_id}</span>{/if}
    </div>
    {#if pinnedId}<p class="can-pinned-id">Pinned {canId(pinnedId.can_id, pinnedId.extended)} <button class="can-small-action" type="button" onclick={() => pinnedId = null} title={$canHelpBubblesEnabled ? "Remove the candidate highlight for this CAN ID." : undefined}>Unpin</button></p>{/if}
    {#if actionError}<p class="can-event-error" role="alert">{actionError}</p>{/if}
    <div class="can-event-list-wrap">
      {#if eventSnapshot?.groups.length}
        {#each eventSnapshot.groups as group (group.label)}
          {@const candidates = matchingCandidates(group)}
          <details class="can-event-group" open>
            <summary>{group.label} · {group.completed_events} complete · {group.pending_events} pending · {group.incomplete_events} incomplete</summary>
            <button class="can-small-action" type="button" disabled={!liveEventAnalysis || liveEventAnalysis.capture_id !== eventSnapshot?.capture_id} onclick={() => openSnapshotComparison(group.label)}>Compare snapshots</button>
            {#if group.omitted_candidates}<p class="can-muted">{group.omitted_candidates} candidate observations omitted by the memory bound.</p>{/if}
            <div class="can-table-wrap can-candidate-wrap">
              <table aria-label={`CAN event candidates for ${group.label}`}>
                <thead><tr>
                  <th title={$canHelpBubblesEnabled ? "CAN ID whose traffic changed around this marked action; it is not a decoded signal." : undefined}>Candidate ID</th>
                  <th title={$canHelpBubblesEnabled ? "Changed and observed event counts, followed by observed trials over completed trials; correlation is not proof of meaning." : undefined}>Changed / seen</th>
                  <th title={$canHelpBubblesEnabled ? "Received frame counts and average rates in the before and after windows." : undefined}>RX frames · before / after rate</th>
                  <th title={$canHelpBubblesEnabled ? "Byte transitions observed across trials; these are raw traffic clues, not decoded values." : undefined}>Byte transitions</th>
                  <th>Field</th>
                </tr></thead>
                <tbody>
                  {#each candidates as item (`${item.candidate.can_id}:${item.candidate.extended}`)}
                    <tr class:pinned-candidate={pinnedId?.can_id === item.candidate.can_id && pinnedId.extended === item.candidate.extended}>
                      <td><button class="can-id-pin" type="button" onclick={() => togglePinned(item.candidate)} aria-pressed={pinnedId?.can_id === item.candidate.can_id && pinnedId.extended === item.candidate.extended} title={$canHelpBubblesEnabled ? (pinnedId?.can_id === item.candidate.can_id && pinnedId.extended === item.candidate.extended ? "Unpin this CAN ID and remove its row highlight." : "Pin this CAN ID to highlight its candidate row in the event results.") : undefined}>{canId(item.candidate.can_id, item.candidate.extended)}</button></td>
                      <td>{item.candidate.changed_events}/{item.candidate.observed_events} observed · {item.candidate.observed_events}/{group.completed_events} trials</td>
                      <td>{item.candidate.before_frames} / {item.candidate.after_frames} · {frameRate(item.candidate.before_frames, item.candidate.before_window_ns)} / {frameRate(item.candidate.after_frames, item.candidate.after_window_ns)}</td>
                      <td>{eventBytes(item.candidate)}
                        <div class="can-byte-actions">{#each item.candidate.changed_bytes as byte}
                          <button class="can-small-action" disabled={!item.sample_frame} onclick={() => item.sample_frame && inspectCandidate(item.sample_frame, item.candidate, group.label, byte.byte_index)} title={$canHelpBubblesEnabled ? "Open this byte as an 8-bit draft signal." : undefined}>Scope B{byte.byte_index}</button>
                          {#each changedBits(item.candidate, byte.byte_index) as bit}<button class="can-small-action" disabled={!item.sample_frame} onclick={() => item.sample_frame && inspectCandidate(item.sample_frame, item.candidate, group.label, byte.byte_index, bit)} title={$canHelpBubblesEnabled ? `Open this bit as a 1-bit draft signal (B${byte.byte_index}.${bit}).` : undefined}>B{byte.byte_index}.{bit}</button>{/each}
                        {/each}</div>
                        {#if item.counter_like_bytes.length || item.sum_checksum_like_bytes.length}<small class="can-noise-hint" title={$canHelpBubblesEnabled ? "Heuristic pattern hints only; these bytes are not confirmed counters or checksums." : undefined}>Noise hint · {item.counter_like_bytes.length ? `counter-like B${item.counter_like_bytes.join(", B")}` : ""}{#if item.counter_like_bytes.length && item.sum_checksum_like_bytes.length} · {/if}{item.sum_checksum_like_bytes.length ? `sum-checksum-like B${item.sum_checksum_like_bytes.join(", B")}` : ""}</small>{/if}</td>
                      <td><button class="can-small-action" type="button" disabled={!item.sample_frame} onclick={() => item.sample_frame && inspectCandidate(item.sample_frame, item.candidate, group.label)} title={$canHelpBubblesEnabled ? "Open the most changed byte as a draft signal and preview its values." : undefined}>Inspect → graph</button></td>
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

{#if selectedSnapshotGroup}
  <CanEventSnapshotsDialog group={selectedSnapshotGroup} onClose={() => { selectedSnapshotLabel = null; selectedSnapshotCaptureId = null; }} />
{/if}

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
  .can-traffic-filters { display:flex; align-items:center; flex-wrap:wrap; gap:4px; min-width:0; }
  .can-traffic-filters > .text-input { width:auto; min-width:70px; min-height:25px; padding:2px 5px; font-size:9px; }
  .can-traffic-filters > input[type="search"] { flex:1 1 125px; }
  .can-id-replay { display:grid; grid-template-columns:minmax(0, 1fr) auto; align-items:center; gap:6px; padding:5px 6px; border:1px solid var(--border-strong); border-radius:6px; background:var(--surface-raised); }
  .can-id-replay-details { display:grid; gap:2px; min-width:0; color:var(--text-muted); font-size:9px; white-space:normal; }
  .can-id-replay-details strong { color:var(--text-primary); font-size:10px; }
  .can-id-replay-actions { display:flex; flex-wrap:wrap; align-items:center; justify-content:flex-end; gap:4px; }
  .can-id-replay-actions .button { min-height:25px; padding:2px 6px; font-size:9px; }
  .can-replay-status { color:var(--accent); }
  .can-id-replay-hint { grid-column:1 / -1; margin:0; color:var(--text-muted); font-size:9px; }
  .can-min-rate { display:flex; align-items:center; gap:4px; color:var(--text-muted); font-size:9px; }
  .can-min-rate .text-input { width:60px; min-width:0; min-height:25px; padding:2px 4px; font-size:9px; }
  .can-color-guide { color:var(--text-muted); font-size:9px; white-space:nowrap; }
  .can-ascii-letter { color:#77d9be; }
  .can-ascii-digit { color:#83b7ff; }
  .can-ascii-punctuation { color:#f0cf83; }
  .can-ascii-nonprintable { color:var(--text-muted); }
  .can-ascii { letter-spacing:.04em; }
  .can-hex-data { white-space:nowrap; }
  .can-hex-byte { display:inline-block; margin-right:2px; padding:0 1px; border-radius:2px; background:hsl(var(--byte-hue) 65% 24%); color:var(--text-bright); }
  .can-metric-grade { display:inline-block; padding:1px 3px; border-radius:3px; background:color-mix(in srgb, var(--accent) var(--metric-grade), transparent); }
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
  @media (max-width:700px) { .can-monitor-toolbar { flex-wrap:wrap; } .can-monitor-toolbar .count-pill { margin-left:0; } .can-view-switch { margin-left:auto; } .can-candidate-wrap { max-height:45vh; } .can-id-replay { grid-template-columns:minmax(0, 1fr); } .can-id-replay-actions { justify-content:flex-start; } }
</style>
