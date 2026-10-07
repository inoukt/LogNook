<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    buildCanScanConfig, canDiagnosticRouteKey, canSignalDefinitionError,
    canTransmitEnabled, canTriggerConfigError, createCanTxFrame,
    createCanWorkbenchCommands, defaultCanWorkbenchConfig, parseCanId, parseCanPayload,
    canCorrelationHypothesisKey, formatCanCorrelationSource,
    formatCanDiagnosticRoute, replaySidecarStatus,
    type CanScanKind, type CanTrafficSummary,
  } from "./canWorkbench";
  import type {
    CanCaptureComparison, CanCaptureReplayReport, CanDeviceCandidate, CanEventCorrelation, CanFrame, CanManualEventMark, CanScanConfig,
    CanScanResult, CanSignalDefinition, CanTransportError, CanTriggerCaptureResult, CanTxStatus,
    CanWorkbenchStatus, Sample,
  } from "./types";

  export let hidden = false;
  export let workspaceBusy = false;
  export let otherAdapterConnected = false;
  export let status: CanWorkbenchStatus | null = null;
  export let fields: CanSignalDefinition[] = [];
  export let frames: CanFrame[] = [];
  export let trafficSummary: CanTrafficSummary[] = [];
  export let scanResults: CanScanResult[] = [];
  export let txStatuses: CanTxStatus[] = [];
  export let transportErrors: CanTransportError[] = [];
  export let triggerCapture: CanTriggerCaptureResult | null = null;
  export let correlationSamples: Sample[] = [];
  export let scanRunning = false;
  export let captureOpen = false;
  export let capturePath = "";
  export let onFieldsChanged: (fields: CanSignalDefinition[]) => void = () => {};

  const commands = createCanWorkbenchCommands(invoke);
  let adapters: CanDeviceCandidate[] = [];
  let selectedAdapterId = "";
  let interfaceNumber = 0;
  let channel = 0;
  let adaptersLoaded = false;
  let bitrate = defaultCanWorkbenchConfig.bitrate;
  let listenOnly = defaultCanWorkbenchConfig.listen_only;
  let refreshing = false;
  let busy = false;
  let error = "";
  let scanStatus = "Idle";
  let requestId = "7E0";
  let responseId = "7E8";
  let extended = false;
  let scanKind: CanScanKind = "mode_01_support";
  let scanStart = "00";
  let scanEnd = "FF";
  let scanPayload = "";
  let pacingMs = 100;
  let responseTimeoutMs = 1000;
  let fieldName = "CAN signal";
  let fieldId = "can:signal";
  let fieldCanId = "7E8";
  let fieldExtended = false;
  let fieldStartBit = 0;
  let fieldBitLength = 16;
  let byteOrder: "intel" | "motorola" = "intel";
  let signed = false;
  let factor = 1;
  let offset = 0;
  let unit = "";
  let captureAction = "";
  let txCanId = "7E0";
  let txExtended = false;
  let txPayload = "02 21 01";
  let txPacingMs = 100;
  let txRepeat = 1;
  let replayReport: CanCaptureReplayReport | null = null;
  let replayPath = "";
  let comparison: CanCaptureComparison | null = null;
  let comparisonPath = "";
  let triggerPreMs = 5000;
  let triggerPostMs = 2000;
  let triggerMaxFrames = 10000;
  let triggerArmed = false;
  let triggerWaiting = false;
  let eventLabel = "";
  let eventPreMs = 1000;
  let eventPostMs = 1000;
  let eventMarks: CanManualEventMark[] = [];
  let eventStatus = "";
  let trafficView: "summary" | "frames" = "summary";

  $: selectedAdapter = adapters.find(({ id }) => id === selectedAdapterId);
  $: connected = Boolean(status?.connected);
  $: transmissionReady = canTransmitEnabled(status, captureOpen) && !busy && !workspaceBusy;
  $: scanConfig = currentScanConfig();
  $: trafficFrameCount = trafficSummary.reduce((total, row) => total + row.rx_count + row.tx_count, 0);
  $: scanError = scanConfig ? "" : scanKind === "can_id_range"
    ? "Enter a valid CAN ID range and up to eight payload bytes."
    : scanKind === "mode_01_support"
    ? "Enter valid, different request and response CAN IDs."
    : "Enter a valid route and hexadecimal identifier range.";
  $: if (!hidden && !adaptersLoaded && !refreshing) void refreshAdapters();

  async function refreshAdapters() {
    refreshing = true;
    error = "";
    try {
      adapters = await commands.availableAdapters();
      if (!adapters.some(({ id }) => id === selectedAdapterId)) selectedAdapterId = adapters[0]?.id ?? "";
    } catch (reason) {
      error = String(reason);
    } finally {
      refreshing = false;
      adaptersLoaded = true;
    }
  }

  function currentScanConfig(): CanScanConfig | null {
    return buildCanScanConfig({
      kind: scanKind,
      requestId,
      responseId,
      extended,
      start: scanStart,
      end: scanEnd,
      payload: scanPayload,
      pacingMs,
      responseTimeoutMs,
    });
  }

  function selectScanKind(kind: CanScanKind) {
    scanKind = kind;
    scanStart = "00";
    scanEnd = kind === "mode_22" ? "FFFF" : kind === "can_id_range" ? "7FF" : "FF";
  }

  async function connect() {
    if (!selectedAdapter || otherAdapterConnected || workspaceBusy || busy || connected) return;
    if (!Number.isInteger(interfaceNumber) || interfaceNumber < 0 || interfaceNumber > 255
      || !Number.isInteger(channel) || channel < 0 || channel > 255) {
      error = "USB interface and channel must be byte-sized integers.";
      return;
    }
    busy = true;
    error = "";
    try {
      await commands.connect(
        { id: selectedAdapter.id, interface_number: interfaceNumber, channel },
        { bitrate, listen_only: listenOnly },
      );
      if (fields.length) await commands.configureFields(fields);
    } catch (reason) {
      error = String(reason);
    } finally {
      busy = false;
    }
  }

  async function startCapture() {
    if (!connected || captureOpen || busy || workspaceBusy) return;
    busy = true;
    captureAction = "Starting paired capture…";
    error = "";
    try {
      const path = await commands.chooseCaptureSavePath();
      if (path) {
        await commands.startCapture(path);
        capturePath = path;
        captureOpen = true;
        captureAction = "Capture open";
        eventMarks = [];
        eventStatus = "";
      } else {
        captureAction = "Capture canceled";
      }
    } catch (reason) {
      error = String(reason);
      captureAction = "Capture failed";
    } finally {
      busy = false;
    }
  }

  async function stopCapture() {
    if (!captureOpen || busy || workspaceBusy) return;
    busy = true;
    error = "";
    try {
      await commands.stopCapture();
      captureOpen = false;
      captureAction = "Capture closed";
    } catch (reason) {
      error = String(reason);
    } finally {
      busy = false;
    }
  }

  async function startScan() {
    const config = currentScanConfig();
    if (!config || !transmissionReady) return;
    await runBusy(async () => {
      await commands.startScan(config);
      scanStatus = "Scan started";
    });
  }

  async function stopScan() {
    await runBusy(async () => {
      await commands.stopScan();
      scanStatus = "Stop requested";
    });
  }

  async function sendCanFrame() {
    if (!transmissionReady) return;
    const can_id = parseCanId(txCanId, txExtended);
    const data = parseCanPayload(txPayload);
    if (can_id === null || data === null) {
      error = "Enter a valid CAN ID and up to eight hexadecimal payload bytes.";
      return;
    }
    const frame = createCanTxFrame(can_id, txExtended, data);
    if (!frame) { error = "Invalid classic CAN frame."; return; }
    await runBusy(() => commands.sendFrame(frame, txPacingMs, txRepeat));
  }

  async function replayCapture() {
    if (captureOpen || busy || workspaceBusy) return;
    await runBusy(async () => {
      const path = await commands.chooseCaptureOpenPath();
      if (!path) return;
      replayReport = await commands.replay(path, fields, correlationSamples);
      replayPath = path;
      comparison = null;
      comparisonPath = "";
    });
  }

  async function compareCapture() {
    if (!replayPath || captureOpen || busy || workspaceBusy) return;
    await runBusy(async () => {
      const path = await commands.chooseCaptureOpenPath();
      if (!path) return;
      comparison = await commands.compareCaptures(replayPath, path);
      comparisonPath = path;
    });
  }

  async function exportDbc() {
    if (!replayPath || busy || workspaceBusy) return;
    await runBusy(async () => {
      const path = await commands.chooseDbcExportPath();
      if (path) await commands.exportDbcSkeleton(replayPath, path);
    });
  }

  async function armTrigger() {
    const validation = canTriggerConfigError(triggerPreMs, triggerPostMs, triggerMaxFrames);
    if (!captureOpen || validation) {
      error = validation ?? "Start a paired raw capture before arming a CAN trigger.";
      return;
    }
    await runBusy(async () => {
      await commands.armTrigger(triggerPreMs, triggerPostMs, triggerMaxFrames);
      triggerArmed = true;
      triggerWaiting = false;
    });
  }

  async function markTrigger() {
    if (!triggerArmed || triggerWaiting || busy || workspaceBusy) return;
    await runBusy(async () => {
      const previousCapture = triggerCapture;
      await commands.triggerNow();
      triggerArmed = false;
      triggerWaiting = triggerCapture === previousCapture;
    });
  }

  async function disarmTrigger() {
    await runBusy(async () => {
      await commands.disarmTrigger();
      triggerArmed = false;
      triggerWaiting = false;
    });
  }

  async function markEvent() {
    if (!captureOpen || !eventLabel.trim() || busy || workspaceBusy) return;
    await runBusy(async () => {
      const mark = await commands.markEvent(eventLabel, eventPreMs, eventPostMs);
      eventMarks = [...eventMarks, mark].slice(-256);
      const count = eventMarks.filter(({ label }) => label === mark.label).length;
      eventStatus = `${mark.label} at ${(mark.elapsed_ns / 1e9).toFixed(2)}s · ${count} mark${count === 1 ? "" : "s"}`;
    });
  }

  async function saveTrigger() {
    if (!triggerCapture || busy || workspaceBusy) return;
    await runBusy(async () => {
      const path = await commands.chooseTriggerSavePath();
      if (path) await commands.saveTriggerCapture(path, triggerCapture);
    });
  }

  function bytesHex(data: number[]): string {
    return data.map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
  }

  function scanOutcome(result: CanScanResult): string {
    return typeof result.outcome === "string" ? result.outcome : JSON.stringify(result.outcome);
  }

  function timingOutcome(outcome: string | Record<string, unknown>): string {
    if (typeof outcome === "string") return outcome;
    const [name, detail] = Object.entries(outcome)[0] ?? ["Unknown", null];
    if (detail && typeof detail === "object" && "nrc" in detail) {
      return `${name} · NRC 0x${Number(detail.nrc).toString(16).padStart(2, "0").toUpperCase()}`;
    }
    return name;
  }

  function eventCanId(id: number, extended: boolean): string {
    return `0x${id.toString(16).toUpperCase().padStart(extended ? 8 : 3, "0")} · ${extended ? "29" : "11"}-bit`;
  }

  function eventByteValue(value: number | null): string {
    return value === null ? "—" : `0x${value.toString(16).padStart(2, "0").toUpperCase()}`;
  }

  function eventCorrelationSummary(correlation: CanEventCorrelation): string {
    const best = correlation.candidates[0];
    return best
      ? `${best.changed_events}/${correlation.event_count} marks · ${eventCanId(best.can_id, best.extended)}`
      : `${correlation.event_count} marks · no changed frames found`;
  }

  function eventMarkTimes(label: string): string {
    return replayReport?.manual_event_marks
      .filter((mark) => mark.label === label)
      .map((mark) => `${(mark.elapsed_ns / 1e9).toFixed(2)}s`)
      .join(", ") ?? "";
  }

  function byteCounts(counts: Record<string, number>): string {
    return Object.entries(counts).map(([value, count]) => `${Number(value).toString(16).padStart(2, "0").toUpperCase()}:${count}`).join(" ") || "—";
  }

  function differs(left: Record<string, number>, right: Record<string, number>): boolean {
    return JSON.stringify(left) !== JSON.stringify(right);
  }

  $: if (triggerCapture) {
    triggerArmed = false;
    triggerWaiting = false;
  }
  $: if (!connected || !captureOpen) {
    triggerArmed = false;
    triggerWaiting = false;
  }

  async function runBusy(action: () => Promise<void>) {
    busy = true;
    error = "";
    try { await action(); }
    catch (reason) { error = String(reason); }
    finally { busy = false; }
  }

  async function addField(event: SubmitEvent) {
    event.preventDefault();
    const can_id = parseCanId(fieldCanId, fieldExtended);
    if (can_id === null) {
      error = "Enter a valid hexadecimal CAN ID for the selected format.";
      return;
    }
    const definition: CanSignalDefinition = {
      id: fieldId.trim(), name: fieldName.trim(), can_id, extended: fieldExtended,
      start_bit: fieldStartBit, bit_length: fieldBitLength, byte_order: byteOrder,
      signed, factor, offset, unit: unit.trim(),
    };
    const validation = canSignalDefinitionError(definition);
    if (validation) { error = validation; return; }
    if (fields.some(({ id }) => id === definition.id)) { error = "Signal ID already exists."; return; }
    await runBusy(async () => {
      const next = [...fields, definition];
      if (connected) await commands.configureFields(next);
      fields = next;
      onFieldsChanged(next);
    });
  }

  async function removeField(id: string) {
    await runBusy(async () => {
      const next = fields.filter((field) => field.id !== id);
      if (connected) await commands.configureFields(next);
      fields = next;
      onFieldsChanged(next);
    });
  }
</script>

<section class="can-workbench" hidden={hidden} aria-label="CAN Workbench">
  {#if otherAdapterConnected}<p class="can-hint">Disconnect the currently connected adapter from the top bar before connecting Candlelight.</p>{/if}

  <div class="can-connection">
    <div class="can-adapter-row">
      <label>GS_USB / Candlelight
        <select class="text-input" bind:value={selectedAdapterId} disabled={connected || busy || workspaceBusy}>
          {#if !adapters.length}<option value="">No Candlelight adapter found</option>{/if}
          {#each adapters as adapter (adapter.id)}
            <option value={adapter.id}>{adapter.product || `${adapter.vendor_id.toString(16)}:${adapter.product_id.toString(16)}`} · {adapter.serial_number || adapter.bus_id}</option>
          {/each}
        </select>
      </label>
      <button class="button secondary" aria-label="Refresh Candlelight adapters" title="Refresh adapters" onclick={refreshAdapters} disabled={connected || refreshing || busy || workspaceBusy}>↻</button>
      <span class:online={connected} class="can-state" role="status">{connected ? status?.listen_only ? "Listen-only" : "Active" : "Disconnected"}</span>
    </div>
    <div class="can-connection-options">
      <label>Interface<input class="text-input" type="number" min="0" max="255" bind:value={interfaceNumber} disabled={connected || busy || workspaceBusy} /></label>
      <label>Channel<input class="text-input" type="number" min="0" max="255" bind:value={channel} disabled={connected || busy || workspaceBusy} /></label>
      <label>Bitrate
        <select class="text-input" bind:value={bitrate} disabled={connected || busy || workspaceBusy}>
          <option value={1000000}>1 Mbit/s</option>
          <option value={800000}>800 kbit/s</option>
          <option value={500000}>500 kbit/s</option>
          <option value={250000}>250 kbit/s</option>
          <option value={125000}>125 kbit/s</option>
          <option value={100000}>100 kbit/s</option>
          <option value={50000}>50 kbit/s</option>
        </select>
      </label>
      <label class="can-check"><input type="checkbox" bind:checked={listenOnly} disabled={connected || busy || workspaceBusy} /> Listen-only</label>
      {#if !connected}
        <button class="button primary" onclick={connect} disabled={!selectedAdapter || otherAdapterConnected || busy || workspaceBusy}>Connect</button>
      {:else}
        <span class="can-source">{status?.source_id} · {status?.bitrate.toLocaleString()} bit/s</span>
      {/if}
    </div>
  </div>

  <div class="can-capture-row">
    <strong>Capture</strong>
    {#if captureOpen}
      <span class="can-capture-path" title={capturePath}>{capturePath}</span>
      <button class="button danger" onclick={stopCapture} disabled={busy || workspaceBusy}>Stop capture</button>
    {:else}
      <span class="can-muted">CSV + JSONL evidence</span>
      <button class="button secondary" onclick={startCapture} disabled={!connected || busy || workspaceBusy}>Start capture</button>
    {/if}
    {#if captureAction}<span class="can-capture-status" role="status">{captureAction}</span>{/if}
  </div>

  <div class="can-event-mark">
    <strong>Vehicle event</strong>
    <label class="can-event-label">Label<input class="text-input" aria-label="Vehicle event label" bind:value={eventLabel} placeholder="e.g. traction control toggle" maxlength="128" disabled={busy || workspaceBusy} /></label>
    <label>Before (ms)<input class="text-input" type="number" min="0" max="60000" bind:value={eventPreMs} disabled={busy || workspaceBusy} /></label>
    <label>After (ms)<input class="text-input" type="number" min="0" max="60000" bind:value={eventPostMs} disabled={busy || workspaceBusy} /></label>
    <button class="button primary" onclick={markEvent} disabled={!connected || !captureOpen || !eventLabel.trim() || busy || workspaceBusy}>Mark event</button>
    {#if eventStatus}<span class="can-event-status" role="status">{eventStatus} · {eventMarks.length} total</span>{:else}<span class="can-event-status">Click once per physical action during capture; repeat the label to compare trials.</span>{/if}
  </div>

  <details class="can-trigger-settings" open>
    <summary>Manual trigger window</summary>
    <p class="can-muted">Capture a bounded frame window around a manual mark. Frames continue through the serialized Candlelight owner.</p>
    <div class="can-trigger-form">
      <label>Pre-trigger (ms)<input class="text-input" type="number" min="0" max="60000" bind:value={triggerPreMs} disabled={busy || workspaceBusy || triggerArmed || triggerWaiting} /></label>
      <label>Post-trigger (ms)<input class="text-input" type="number" min="0" max="60000" bind:value={triggerPostMs} disabled={busy || workspaceBusy || triggerArmed || triggerWaiting} /></label>
      <label>Maximum frames<input class="text-input" type="number" min="1" max="100000" bind:value={triggerMaxFrames} disabled={busy || workspaceBusy || triggerArmed || triggerWaiting} /></label>
      <button class="button secondary" onclick={armTrigger} disabled={!connected || !captureOpen || busy || workspaceBusy || triggerArmed || triggerWaiting}>Arm window</button>
      <button class="button primary" onclick={markTrigger} disabled={!triggerArmed || triggerWaiting || busy || workspaceBusy}>Mark trigger now</button>
      {#if triggerArmed}<span class="can-muted" role="status">Armed · mark a trigger when ready</span>{:else if triggerWaiting}<span class="can-muted" role="status">Waiting for post-trigger frames…</span>{/if}
      {#if triggerArmed || triggerWaiting}<button class="button secondary" onclick={disarmTrigger} disabled={busy || workspaceBusy}>Disarm</button>{/if}
    </div>
    {#if triggerCapture}
      <div class="can-trigger-result" role="status">
        <strong>Triggered window · {triggerCapture.capture.frames.length} frames</strong>
        <span>{(triggerCapture.capture.pre_ns / 1e6).toFixed(0)} ms pre / {(triggerCapture.capture.post_ns / 1e6).toFixed(0)} ms post · limit {triggerCapture.capture.max_frames.toLocaleString()}</span>
        <span>{triggerCapture.capture.complete ? "Complete" : "Incomplete"}{triggerCapture.capture.truncated ? " · frame limit reached" : ""}</span>
        <span>{triggerCapture.source_id} · source capture {triggerCapture.source_capture_id}</span>
        <button class="button secondary" onclick={saveTrigger} disabled={busy || workspaceBusy}>Save as raw CSV + JSONL</button>
      </div>
    {/if}
  </details>

  <details class="can-scan-settings" open>
    <summary>Scan configuration</summary>
    <div class="can-scan-form">
      <label>Test
        <select class="text-input" value={scanKind} onchange={(event) => selectScanKind(event.currentTarget.value as CanScanKind)} disabled={busy || workspaceBusy}>
          <option value="can_id_range">CAN ID range</option>
          <option value="mode_01_support">Mode 01 support</option>
          <option value="mode_01_pids">Mode 01 PIDs</option>
          <option value="mode_21">Mode 21 range</option>
          <option value="mode_22">Mode 22 range</option>
        </select>
      </label>
      {#if scanKind !== "can_id_range"}
        <label>Request ID (hex)<input class="text-input" bind:value={requestId} disabled={busy || workspaceBusy} /></label>
        <label>Response ID (hex)<input class="text-input" bind:value={responseId} disabled={busy || workspaceBusy} /></label>
      {/if}
      <label class="can-check"><input type="checkbox" bind:checked={extended} disabled={busy || workspaceBusy} /> 29-bit IDs</label>
      {#if scanKind === "can_id_range"}
        <label>Start CAN ID (hex)<input class="text-input" bind:value={scanStart} disabled={busy || workspaceBusy} /></label>
        <label>End CAN ID (hex)<input class="text-input" bind:value={scanEnd} disabled={busy || workspaceBusy} /></label>
        <label>Payload (hex bytes)<input class="text-input" bind:value={scanPayload} placeholder="optional" disabled={busy || workspaceBusy} /></label>
      {:else if scanKind !== "mode_01_support"}
        <label>Start {scanKind === "mode_01_pids" || scanKind === "mode_21" ? "PID" : "DID"} (hex)<input class="text-input" bind:value={scanStart} disabled={busy || workspaceBusy} /></label>
        <label>End {scanKind === "mode_01_pids" || scanKind === "mode_21" ? "PID" : "DID"} (hex)<input class="text-input" bind:value={scanEnd} disabled={busy || workspaceBusy} /></label>
      {/if}
      <label>Pacing (ms)<input class="text-input" type="number" min="1" max="60000" bind:value={pacingMs} disabled={busy || workspaceBusy} /></label>
      <label>Response timeout (ms)<input class="text-input" type="number" min="10" max="10000" bind:value={responseTimeoutMs} disabled={busy || workspaceBusy} /></label>
      {#if scanError}<span class="can-error">{scanError}</span>{/if}
      {#if scanRunning}<span class="can-muted" role="status">Scan running</span>{:else if scanStatus}<span class="can-muted" role="status">{scanStatus}</span>{/if}
      <button class="button primary" onclick={startScan} disabled={!transmissionReady || !scanConfig || scanRunning}>Start scan</button>
      <button class="button secondary" onclick={stopScan} disabled={!scanRunning || busy || workspaceBusy}>Stop scan</button>
    </div>
    {#if !canTransmitEnabled(status, captureOpen)}<p class="can-hint">Start a paired capture and reconnect in active mode to enable scans. Connect defaults to listen-only.</p>{/if}
  </details>

  <details class="can-fields-settings" open>
    <summary>CAN fields · {fields.length}</summary>
    <form class="can-field-form" onsubmit={addField}>
      <label>Name<input class="text-input" bind:value={fieldName} /></label>
      <label>Signal ID<input class="text-input" bind:value={fieldId} /></label>
      <label>CAN ID (hex)<input class="text-input" bind:value={fieldCanId} /></label>
      <label class="can-check"><input type="checkbox" bind:checked={fieldExtended} /> 29-bit</label>
      <label>Start bit<input class="text-input" type="number" min="0" max="63" bind:value={fieldStartBit} /></label>
      <label>Length<input class="text-input" type="number" min="1" max="64" bind:value={fieldBitLength} /></label>
      <label>Byte order<select class="text-input" bind:value={byteOrder}><option value="intel">Intel</option><option value="motorola">Motorola</option></select></label>
      <label class="can-check"><input type="checkbox" bind:checked={signed} /> Signed</label>
      <label>Factor<input class="text-input" type="number" step="any" bind:value={factor} /></label>
      <label>Offset<input class="text-input" type="number" step="any" bind:value={offset} /></label>
      <label>Unit<input class="text-input" bind:value={unit} /></label>
      <button class="button secondary" type="submit" disabled={!connected || busy || workspaceBusy}>Add field</button>
    </form>
    {#each fields as field (field.id)}
      <div class="can-field-row"><span>{field.name} · 0x{field.can_id.toString(16).toUpperCase()} · {field.bit_length} bit · {field.unit}</span><button class="button secondary" onclick={() => removeField(field.id)} disabled={busy || workspaceBusy}>Remove</button></div>
    {:else}
      <p class="can-muted">Fields are configured in Rust and publish timestamped samples to the common graph/recording pipeline.</p>
    {/each}
  </details>

  <details class="can-send-settings" open>
    <summary>Manual CAN frame</summary>
    <div class="can-send-form">
      <label>CAN ID (hex)<input class="text-input" bind:value={txCanId} disabled={busy || workspaceBusy} /></label>
      <label class="can-check"><input type="checkbox" bind:checked={txExtended} disabled={busy || workspaceBusy} /> 29-bit</label>
      <label>Data bytes (hex)<input class="text-input" bind:value={txPayload} placeholder="02 21 01" disabled={busy || workspaceBusy} /></label>
      <label>Pacing (ms)<input class="text-input" type="number" min="1" max="60000" bind:value={txPacingMs} disabled={busy || workspaceBusy} /></label>
      <label>Repeat<input class="text-input" type="number" min="1" max="10000" bind:value={txRepeat} disabled={busy || workspaceBusy} /></label>
      <button class="button danger" onclick={sendCanFrame} disabled={!transmissionReady}>Send frame</button>
    </div>
    {#if !transmissionReady}<p class="can-hint">Sending is enabled only in active mode with a paired capture open.</p>{/if}
    {#if txStatuses.length}
      <ul class="can-event-list" aria-label="Transmit status">
        {#each txStatuses.slice(-12).reverse() as item (`${item.capture_id}:${item.sequence}`)}
          <li><strong>{item.confirmed ? "Echo confirmed" : "Submitted · awaiting echo"}</strong> · {item.frame.extended ? "29-bit" : "11-bit"} 0x{item.frame.can_id.toString(16).toUpperCase()} · {bytesHex(item.frame.data)}</li>
        {/each}
      </ul>
    {/if}
  </details>

  <details class="can-live-data" open>
    <summary>Live traffic · {trafficFrameCount.toLocaleString()} frames · {trafficSummary.length} IDs</summary>
    <div class="can-live-toolbar">
      <div class="can-view-switch" role="group" aria-label="CAN traffic view">
        <button class="can-view-option" aria-pressed={trafficView === "summary"} onclick={() => trafficView = "summary"}>Counts</button>
        <button class="can-view-option" aria-pressed={trafficView === "frames"} onclick={() => trafficView = "frames"}>Frames</button>
      </div>
      <span class="can-muted">Counts since connection</span>
    </div>
    {#if trafficView === "summary"}
      <div class="can-table-wrap">
        <table aria-label="Live CAN ID counts">
          <thead><tr><th>ID</th><th>DLC</th><th>Data</th><th>Period</th><th>Count</th><th>Rate</th><th>ASCII</th></tr></thead>
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
              </tr>
            {:else}
              <tr><td colspan="7" class="can-muted">Waiting for Candlelight frames</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="can-muted">Latest {frames.length} frames retained for inspection (up to 400).</p>
      <div class="can-table-wrap">
        <table aria-label="Recent CAN frames">
          <thead><tr><th>Time</th><th>Dir</th><th>ID</th><th>DLC</th><th>Data</th></tr></thead>
          <tbody>
            {#each frames.slice(-80).reverse() as frame, index (`${frame.wall_time_unix_ns}:${frame.can_id}:${index}`)}
              <tr><td>{(frame.elapsed_ns / 1e9).toFixed(3)}s</td><td>{frame.direction.toUpperCase()}</td><td>{frame.extended ? "29" : "11"}-bit 0x{frame.can_id.toString(16).toUpperCase()}</td><td>{frame.data.length}</td><td><code>{bytesHex(frame.data)}</code></td></tr>
            {:else}
              <tr><td colspan="5" class="can-muted">Waiting for Candlelight frames</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
    {#if scanResults.length}
      <h3>Scan results · {scanResults.length.toLocaleString()}</h3>
      <ul class="can-event-list" aria-label="CAN scan results">
        {#each scanResults.slice(-30).reverse() as result, index (`${result.identifier}:${index}`)}
          <li>
            <strong>{result.identifier}</strong> · {scanOutcome(result)}{#if result.supported_pids?.length} · PIDs {bytesHex(result.supported_pids)}{/if}
            <details><summary>Request/reply frames</summary><code>TX 0x{result.request.can_id.toString(16).toUpperCase()} {bytesHex(result.request.data)}</code>
              {#each result.response_frames as response}<code>RX 0x{response.can_id.toString(16).toUpperCase()} {bytesHex(response.data)}</code>{:else}<span class="can-muted">No response frame</span>{/each}
            </details>
          </li>
        {/each}
      </ul>
    {/if}
    {#if transportErrors.length}
      <h3>Transport errors · {transportErrors.length}</h3>
      <ul class="can-event-list" aria-label="CAN transport errors">
        {#each transportErrors.slice(-12).reverse() as item, index (`${item.kind}:${index}`)}
          <li class="can-error"><strong>{item.kind}</strong> · {item.message}{#if item.raw_usb}<code>USB: {bytesHex(item.raw_usb)}</code>{/if}</li>
        {/each}
      </ul>
    {/if}
  </details>

  <details class="can-replay-report">
    <summary>Replay a saved capture and review Rust analysis</summary>
    <div class="can-report-actions">
      <button class="button secondary" onclick={replayCapture} disabled={captureOpen || busy || workspaceBusy}>Open .candump.csv</button>
      <button class="button secondary" onclick={compareCapture} disabled={!replayReport || captureOpen || busy || workspaceBusy}>Compare another CSV</button>
      <button class="button secondary" onclick={exportDbc} disabled={!replayReport || busy || workspaceBusy}>Export UNVERIFIED DBC skeleton</button>
    </div>
    {#if captureOpen}<p class="can-hint">Stop the current capture before replaying its CSV and JSONL sidecar.</p>{/if}
    {#if replayReport}
      <p class="can-muted" title={replayPath}>{replayPath}</p>
      <p class="can-report-status">{replaySidecarStatus(replayReport)}</p>
      {#if replayReport.source_id}<p class="can-muted">Adapter: {replayReport.source_id} · {replayReport.bitrate?.toLocaleString()} bit/s · {replayReport.listen_only ? "listen-only" : "active"}</p>{/if}
      {#if replayReport.evidence_journal_error}<p class="can-error" role="status">{replayReport.evidence_journal_error}</p>{/if}
      <div class="can-report-counts">
        <span>{replayReport.frames.toLocaleString()} frames</span><span>{replayReport.decoded_samples.toLocaleString()} field samples</span>
        <span>{replayReport.evidence_records.toLocaleString()} evidence records</span><span>{replayReport.scan_results.toLocaleString()} scans</span>
        <span>{replayReport.tx_submitted.toLocaleString()} TX</span><span>{replayReport.tx_echo_confirmed.toLocaleString()} echoes</span>
        <span>{replayReport.transport_errors.toLocaleString()} transport errors</span><span>{replayReport.trigger_marks.toLocaleString()} trigger marks</span><span>{replayReport.trigger_completions.toLocaleString()} complete windows</span><span>{replayReport.trigger_early_ends.toLocaleString()} early ends</span><span>{replayReport.trigger_window_exports.toLocaleString()} trigger exports</span>
      </div>
      {#if replayReport.evidence_journal}<small class="can-muted">Sidecar: {replayReport.evidence_journal}</small>{/if}
      {#if replayReport.event_correlations.length}
        <h3>Event-linked CAN messages · repeated marks ranked by matching changes</h3>
        {#each replayReport.event_correlations as correlation (correlation.label)}
          <details class="can-event-correlation">
            <summary>{correlation.label} · {eventCorrelationSummary(correlation)}</summary>
            <p class="can-muted">Marks at {eventMarkTimes(correlation.label)} · windows compare each event’s before/after traffic.</p>
            <div class="can-table-wrap">
              <table aria-label={`CAN messages correlated with ${correlation.label}`}>
                <thead><tr><th>Candidate ID</th><th>Changed marks</th><th>Frames before / after</th><th>Byte transitions</th></tr></thead>
                <tbody>
                  {#each correlation.candidates.slice(0, 30) as candidate (`${candidate.can_id}:${candidate.extended}`)}
                    <tr>
                      <td>{eventCanId(candidate.can_id, candidate.extended)}</td>
                      <td>{candidate.changed_events}/{correlation.event_count}</td>
                      <td>{candidate.before_frames} / {candidate.after_frames}</td>
                      <td>{candidate.changed_bytes.map((byte) => `B${byte.byte_index}: ${byte.transitions.map((transition) => `${eventByteValue(transition.before)}→${eventByteValue(transition.after)} ×${transition.occurrences}`).join(", ")}`).join("; ") || (candidate.before_frames === 0 ? "appears after" : candidate.after_frames === 0 ? "stops after" : `${candidate.count_change_events} activity shifts`)}</td>
                    </tr>
                  {:else}
                    <tr><td colspan="4" class="can-muted">No CAN frames were found around these marks.</td></tr>
                  {/each}
                </tbody>
              </table>
            </div>
            <p class="can-muted">Ranked correlations are candidates, not decoded signal meanings. Use the CAN ID and byte transitions to define and graph a field.</p>
          </details>
        {/each}
      {/if}
      {#if replayReport.evidence_errors.length || replayReport.analyzer_issues.length || replayReport.malformed_rows.length}
        <ul class="can-event-list" aria-label="Replay issues">
          {#each [...replayReport.evidence_errors, ...replayReport.analyzer_issues, ...replayReport.malformed_rows].slice(0, 20) as issue, index (`${index}:${issue}`)}<li class="can-error">{issue}</li>{/each}
        </ul>
      {/if}
      {#if replayReport.timings.length}
        <h3>Request/reply timing · routes from evidence journal</h3>
        <ul class="can-event-list" aria-label="Recorded CAN diagnostic route sources">
          {#each replayReport.route_provenance as provenance (canDiagnosticRouteKey(provenance.route))}
            <li>{formatCanDiagnosticRoute(provenance.route)} · {provenance.source_candidate_ids.join(", ") || "operator-configured bench route"}</li>
          {/each}
        </ul>
        <div class="can-table-wrap">
          <table aria-label="CAN diagnostic request and reply timing">
            <thead><tr><th>Route</th><th>Latency</th><th>Outcome</th><th>Request → reply (raw PDU)</th></tr></thead>
            <tbody>
              {#each replayReport.timings as timing, index (`${canDiagnosticRouteKey(timing.route)}:${timing.request_elapsed_ns}:${index}`)}
                <tr><td>{formatCanDiagnosticRoute(timing.route)}</td><td>{(timing.latency_ns / 1e6).toFixed(2)} ms</td><td>{timingOutcome(timing.outcome)}</td><td><code>{bytesHex(timing.request_payload)} → {bytesHex(timing.response_payload)}</code></td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="can-muted">No request/reply pairs found for the routes recorded in this capture’s JSONL evidence.</p>
      {/if}
      <h3>CAN-to-OBD correlation hypotheses</h3>
      <p class="can-muted">{replayReport.correlation_sample_count} timestamped app samples supplied · correlations are research hypotheses only, never decoded or vehicle-validated signals.</p>
      {#if replayReport.correlation_hypotheses.length}
        <ul class="can-event-list" aria-label="CAN-to-OBD correlation hypotheses">
          {#each replayReport.correlation_hypotheses as hypothesis (canCorrelationHypothesisKey(hypothesis))}
            <li><strong>{hypothesis.field_id}</strong> ↔ {hypothesis.obd_signal_id} · {formatCanCorrelationSource(hypothesis)} · r={hypothesis.pearson_r.toFixed(3)} · {hypothesis.pairs} pairs · hypothesis only</li>
          {/each}
        </ul>
      {:else}
        <p class="can-muted">No matching field/sample pairs were close enough in wall-clock time to report a correlation.</p>
      {/if}
      <h3>CAN ID census · Rust calculated rates</h3>
      <div class="can-table-wrap">
        <table aria-label="CAN ID rates from replay">
          <thead><tr><th>ID</th><th>Frames</th><th>RX</th><th>TX</th><th>Rate</th><th>Byte details</th></tr></thead>
          <tbody>
            {#each replayReport.census.slice(0, 40) as row (`${row.can_id}:${row.extended}`)}
              <tr>
                <td>{row.extended ? "29" : "11"}-bit 0x{row.can_id.toString(16).toUpperCase()}</td><td>{row.count}</td><td>{row.rx_count}</td><td>{row.tx_count}</td><td>{row.rate_hz === null ? "—" : `${row.rate_hz.toFixed(1)} Hz`}</td>
                <td><details><summary>{row.bytes.reduce((total, byte) => total + byte.changes, 0)} changes</summary>
                  <ul class="can-byte-list">{#each row.bytes as byte, index}<li><strong>B{index}</strong> · {byte.min ?? "—"}–{byte.max ?? "—"} · {byte.changes} changes{#if byte.is_static} · static{/if}{#if byte.counter_hypothesis} · counter?{/if}{#if byte.checksum_hypothesis} · checksum?{/if}<details><summary>Values: {byte.observations}</summary><code>{byteCounts(byte.value_counts)}</code></details></li>{/each}</ul>
                </details></td>
              </tr>
            {:else}
              <tr><td colspan="6" class="can-muted">No valid frames in the capture</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if comparison}
        <h3>Capture comparison · {comparison.left.frames} vs {comparison.right.frames} frames</h3>
        <p class="can-muted" title={comparisonPath}>Right capture: {comparisonPath}</p>
        <p class="can-muted">Left: {comparison.left.source_id ?? "unknown source"} · {comparison.left.session_finalized === true ? "closed cleanly" : "not finalized"} · Right: {comparison.right.source_id ?? "unknown source"} · {comparison.right.session_finalized === true ? "closed cleanly" : "not finalized"}</p>
        {#if comparison.left.evidence_journal_error}<p class="can-error">Left JSONL: {comparison.left.evidence_journal_error}</p>{/if}
        {#if comparison.right.evidence_journal_error}<p class="can-error">Right JSONL: {comparison.right.evidence_journal_error}</p>{/if}
        {#each [...comparison.left.evidence_errors, ...comparison.right.evidence_errors, ...comparison.left.malformed_rows, ...comparison.right.malformed_rows, ...comparison.left.analyzer_issues, ...comparison.right.analyzer_issues].slice(0, 10) as issue, index (`${index}:${issue}`)}<p class="can-error">{issue}</p>{/each}
        <div class="can-table-wrap">
          <table aria-label="CAN identifier and payload-byte capture differences">
            <thead><tr><th>ID</th><th>Frame counts</th><th>Changed byte distributions</th></tr></thead>
            <tbody>
              {#each comparison.differences.filter((row) => row.left_count !== row.right_count || row.bytes.some((byte) => differs(byte.left, byte.right))) as row (`${row.can_id}:${row.extended}`)}
                <tr><td>{row.extended ? "29" : "11"}-bit 0x{row.can_id.toString(16).toUpperCase()}</td><td>{row.left_count} → {row.right_count}</td><td>
                  <ul class="can-byte-list">{#each row.bytes as byte, index}{#if differs(byte.left, byte.right)}<li><strong>B{index}</strong> · L {byteCounts(byte.left)} · R {byteCounts(byte.right)}</li>{/if}{/each}</ul>
                </td></tr>
              {:else}
                <tr><td colspan="3" class="can-muted">No ID counts or byte distributions differ.</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </details>

  {#if error}<p class="can-error" role="alert">{error}</p>{/if}
</section>

<style>
  .can-workbench { display:grid; align-content:start; gap:4px; min-width:0; min-height:0; overflow-x:hidden; overflow-y:auto; padding:4px; color:var(--text-primary,#e7edf2); }
  .can-workbench[hidden] { display:none; }
  .can-connection { display:grid; gap:4px; min-width:0; }
  .can-adapter-row { display:grid; grid-template-columns:minmax(0, 1fr) 27px auto; align-items:end; gap:4px; min-width:0; }
  .can-connection-options { display:grid; grid-template-columns:repeat(3, minmax(0, 1fr)); align-items:end; gap:4px; min-width:0; }
  label { display:grid; gap:2px; min-width:0; color:var(--text-muted,#a8b6c1); font-size:9px; }
  .can-connection .text-input { width:100%; min-width:0; height:25px; padding:0 5px; font-size:9px; }
  .can-connection .button { min-height:25px; padding:2px 5px; font-size:9px; }
  .can-check { display:flex; align-items:center; gap:3px; white-space:nowrap; }
  .can-connection-options .can-check { grid-column:span 2; }
  .can-state { min-width:0; overflow:hidden; padding:3px 5px; color:var(--text-muted,#9badb8); font-size:9px; text-overflow:ellipsis; white-space:nowrap; }
  .can-state.online { color:var(--accent,#56c8bd); }
  .can-state,.can-capture-row,.can-scan-settings,.can-fields-settings,.can-field-row,.can-send-settings,.can-live-data,.can-replay-report { border:1px solid var(--border,#344652); border-radius:.55rem; background:var(--panel,#192630); }
  .can-capture-row { display:grid; grid-template-columns:auto minmax(0, 1fr) auto; align-items:center; gap:4px; min-width:0; padding:3px 5px; }
  .can-capture-row strong { font-size:10px; white-space:nowrap; }
  .can-capture-row .can-muted { font-size:9px; }
  .can-capture-path { overflow:hidden; min-width:0; color:var(--text-muted,#9badb8); text-overflow:ellipsis; white-space:nowrap; }
  .can-capture-status { grid-column:1 / -1; overflow:hidden; color:var(--text-muted,#9badb8); font-size:9px; text-overflow:ellipsis; white-space:nowrap; }
  .can-event-mark { display:grid; grid-template-columns:repeat(3, minmax(0, 1fr)); align-items:end; gap:4px; min-width:0; padding:4px 5px; border:1px solid var(--border,#344652); border-radius:.55rem; background:var(--panel,#192630); }
  .can-event-mark > strong,.can-event-label,.can-event-status { grid-column:1 / -1; }
  .can-event-mark > strong { font-size:10px; }
  .can-event-mark .text-input { width:100%; min-width:0; height:25px; padding:0 5px; font-size:9px; }
  .can-event-mark .button { min-height:25px; padding:2px 5px; font-size:9px; }
  .can-event-status { min-width:0; overflow:hidden; color:var(--text-muted,#9badb8); font-size:9px; text-overflow:ellipsis; white-space:nowrap; }
  .can-event-correlation { margin:3px 0; padding:3px 5px; border:1px solid var(--border,#344652); border-radius:.4rem; }
  .can-event-correlation summary { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .can-muted,.can-hint { color:var(--text-muted,#9badb8); font-size:9px; }
  .can-source { grid-column:span 2; overflow:hidden; color:var(--text-muted,#9badb8); font-size:9px; text-overflow:ellipsis; white-space:nowrap; }
  details { min-width:0; padding:3px 5px; }
  summary { cursor:pointer; color:var(--accent,#56c8bd); font-size:10px; font-weight:650; line-height:1.25; }
  .can-scan-form,.can-field-form,.can-trigger-form,.can-report-actions { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); align-items:end; gap:4px; margin-top:4px; min-width:0; }
  .can-trigger-form { grid-template-columns:repeat(3, minmax(0, 1fr)); }
  .can-trigger-form label { min-width:0; }
  .can-trigger-form .button { min-height:24px; padding:2px 4px; font-size:9px; }
  .can-trigger-form .can-muted { grid-column:1 / -1; font-size:9px; }
  .can-trigger-result { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); align-items:center; gap:3px; margin-top:4px; padding:4px; border:1px solid var(--border,#344652); border-radius:.45rem; color:var(--text-muted,#9badb8); font-size:9px; }
  .can-trigger-result strong { color:var(--text-primary,#e7edf2); }
  .can-scan-form label,.can-field-form label { min-width:0; }
  .can-field-form { grid-template-columns:repeat(3, minmax(0, 1fr)); }
  .can-field-form label.can-check,.can-scan-form label.can-check { display:flex; align-items:center; min-height:25px; }
  .can-scan-form .text-input,.can-field-form .text-input,.can-trigger-form .text-input,.can-send-form .text-input { width:100%; min-width:0; height:25px; padding:0 5px; font-size:9px; }
  .can-scan-form .button,.can-field-form .button,.can-send-form .button,.can-report-actions .button { min-height:25px; padding:2px 5px; font-size:9px; }
  .can-trigger-result button { grid-column:1 / -1; }
  .can-error { color:var(--danger,#ff7f86); font-size:9px; overflow-wrap:anywhere; }
  .can-field-row { display:flex; align-items:center; justify-content:space-between; gap:4px; min-width:0; margin-top:3px; padding:3px 4px; font-size:9px; }
  .can-field-row span { min-width:0; overflow-wrap:anywhere; }
  .can-field-row .button { flex:0 0 auto; min-height:23px; padding:2px 4px; font-size:9px; }
  .can-send-form { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); align-items:end; gap:4px; margin-top:4px; min-width:0; }
  .can-send-form label { min-width:0; }
  .can-send-form label.can-check { display:flex; align-items:center; min-height:25px; }
  .can-table-wrap { max-height:10rem; overflow:auto; margin-top:4px; border:1px solid var(--border,#344652); border-radius:.45rem; }
  table { width:100%; border-collapse:collapse; font-size:9px; font-variant-numeric:tabular-nums; }
  th,td { padding:2px 3px; border-bottom:1px solid var(--border,#344652); text-align:left; white-space:nowrap; }
  th { position:sticky; top:0; background:var(--panel,#192630); color:var(--text-muted,#9badb8); }
  code { overflow-wrap:anywhere; white-space:normal; }
  h3 { margin:4px 0 2px; font-size:10px; }
  .can-event-list { display:grid; gap:2px; max-height:8rem; overflow:auto; margin:3px 0 0; padding-left:12px; font-size:9px; }
  .can-event-list details { padding:2px 0; border:0; background:none; }
  .can-event-list details summary { font-size:9px; }
  .can-event-list details code { display:block; margin:2px 0; }
  .can-byte-list { max-height:8rem; overflow:auto; margin:3px 0; padding-left:12px; font-size:9px; }
  .can-report-counts { display:flex; flex-wrap:wrap; gap:3px; margin:4px 0; }
  .can-report-counts span { padding:2px 4px; border:1px solid var(--border,#344652); border-radius:.4rem; font-size:9px; }
  .can-live-toolbar { display:flex; align-items:center; justify-content:space-between; gap:4px; margin-top:3px; }
  .can-view-switch { display:inline-flex; gap:2px; }
  .can-view-option { min-height:21px; padding:1px 6px; border:1px solid var(--border,#344652); border-radius:.35rem; background:transparent; color:var(--text-muted,#9badb8); font-size:9px; }
  .can-view-option[aria-pressed="true"] { background:var(--accent,#56c8bd); color:var(--text-on-accent,#102027); }
  .can-report-status { color:var(--accent,#56c8bd); font-size:9px; }
  .can-hint { margin:3px 0 0; }
</style>
