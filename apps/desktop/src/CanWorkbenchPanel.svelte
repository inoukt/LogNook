<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    buildCanScanConfig, canSignalDefinitionError, canTransmitEnabled, createCanTxFrame,
    createCanWorkbenchCommands, defaultCanWorkbenchConfig, parseCanId, parseCanPayload,
    recentCanRates, replaySidecarStatus,
    type CanScanKind,
  } from "./canWorkbench";
  import type {
    CanCaptureReplayReport, CanDeviceCandidate, CanFrame, CanScanConfig, CanScanResult,
    CanSignalDefinition, CanTransportError, CanTxStatus, CanWorkbenchStatus,
  } from "./types";

  export let hidden = false;
  export let workspaceBusy = false;
  export let otherAdapterConnected = false;
  export let status: CanWorkbenchStatus | null = null;
  export let fields: CanSignalDefinition[] = [];
  export let frames: CanFrame[] = [];
  export let scanResults: CanScanResult[] = [];
  export let txStatuses: CanTxStatus[] = [];
  export let transportErrors: CanTransportError[] = [];
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

  $: selectedAdapter = adapters.find(({ id }) => id === selectedAdapterId);
  $: connected = Boolean(status?.connected);
  $: transmissionReady = canTransmitEnabled(status, captureOpen) && !busy && !workspaceBusy;
  $: scanConfig = currentScanConfig();
  $: recentRates = recentCanRates(frames);
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
      replayReport = await commands.replay(path, fields);
      replayPath = path;
    });
  }

  function bytesHex(data: number[]): string {
    return data.map((byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(" ");
  }

  function scanOutcome(result: CanScanResult): string {
    return typeof result.outcome === "string" ? result.outcome : JSON.stringify(result.outcome);
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
  <header class="can-heading">
    <div><p class="eyebrow">TEST WORKBENCH</p><h2>CAN Workbench</h2></div>
    <span class:online={connected} class="can-state">{connected ? status?.listen_only ? "Listen-only" : "Active" : "Disconnected"}</span>
  </header>
  {#if otherAdapterConnected}<p class="can-hint">Disconnect the currently connected adapter from the top bar before connecting Candlelight.</p>{/if}

  <div class="can-connection">
    <label>GS_USB / Candlelight adapter
      <select class="text-input" bind:value={selectedAdapterId} disabled={connected || busy || workspaceBusy}>
        {#if !adapters.length}<option value="">No Candlelight adapter found</option>{/if}
        {#each adapters as adapter (adapter.id)}
          <option value={adapter.id}>{adapter.product || `${adapter.vendor_id.toString(16)}:${adapter.product_id.toString(16)}`} · {adapter.serial_number || adapter.bus_id}</option>
        {/each}
      </select>
    </label>
    <button class="button secondary" onclick={refreshAdapters} disabled={connected || refreshing || busy || workspaceBusy}>Refresh</button>
    <label>USB interface<input class="text-input" type="number" min="0" max="255" bind:value={interfaceNumber} disabled={connected || busy || workspaceBusy} /></label>
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

  <div class="can-capture-row">
    <strong>Raw capture</strong>
    {#if captureOpen}
      <span class="can-capture-path" title={capturePath}>{capturePath}</span>
      <button class="button danger" onclick={stopCapture} disabled={busy || workspaceBusy}>Stop capture</button>
    {:else}
      <span class="can-muted">CSV + JSONL evidence</span>
      <button class="button secondary" onclick={startCapture} disabled={!connected || busy || workspaceBusy}>Start capture</button>
    {/if}
    {#if captureAction}<span class="can-muted">{captureAction}</span>{/if}
  </div>

  <details class="can-scan-settings">
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

  <details class="can-fields-settings">
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

  <details class="can-send-settings">
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
    <summary>Live traffic · {frames.length} recent frames</summary>
    <p class="can-muted">Per-ID counts and RX rates cover only the most recent {frames.length} displayed frames (bounded to 400).</p>
    <div class="can-rate-summary" aria-label="Recent CAN ID counts and rates">
      {#each recentRates as row (`${row.can_id}:${row.extended}`)}
        <span>{row.extended ? "29" : "11"}-bit 0x{row.can_id.toString(16).toUpperCase()} · {row.rx_count} RX / {row.tx_count} TX · {row.rate_hz === null ? "rate —" : `${row.rate_hz.toFixed(1)} Hz`}</span>
      {:else}
        <span class="can-muted">No recent IDs</span>
      {/each}
    </div>
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
    <button class="button secondary" onclick={replayCapture} disabled={captureOpen || busy || workspaceBusy}>Open .candump.csv</button>
    {#if captureOpen}<p class="can-hint">Stop the current capture before replaying its CSV and JSONL sidecar.</p>{/if}
    {#if replayReport}
      <p class="can-muted" title={replayPath}>{replayPath}</p>
      <p class="can-report-status">{replaySidecarStatus(replayReport)}</p>
      {#if replayReport.evidence_journal_error}<p class="can-error" role="status">{replayReport.evidence_journal_error}</p>{/if}
      <div class="can-report-counts">
        <span>{replayReport.frames.toLocaleString()} frames</span><span>{replayReport.decoded_samples.toLocaleString()} field samples</span>
        <span>{replayReport.evidence_records.toLocaleString()} evidence records</span><span>{replayReport.scan_results.toLocaleString()} scans</span>
        <span>{replayReport.tx_submitted.toLocaleString()} TX</span><span>{replayReport.tx_echo_confirmed.toLocaleString()} echoes</span>
        <span>{replayReport.transport_errors.toLocaleString()} transport errors</span>
      </div>
      {#if replayReport.evidence_journal}<small class="can-muted">Sidecar: {replayReport.evidence_journal}</small>{/if}
      {#if replayReport.evidence_errors.length || replayReport.analyzer_issues.length || replayReport.malformed_rows.length}
        <ul class="can-event-list" aria-label="Replay issues">
          {#each [...replayReport.evidence_errors, ...replayReport.analyzer_issues, ...replayReport.malformed_rows].slice(0, 20) as issue, index (`${index}:${issue}`)}<li class="can-error">{issue}</li>{/each}
        </ul>
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
                  <ul class="can-byte-list">{#each row.bytes as byte, index}<li><strong>B{index}</strong> · {byte.min ?? "—"}–{byte.max ?? "—"} · {byte.changes} changes{#if byte.is_static} · static{/if}{#if byte.counter_hypothesis} · counter?{/if}{#if byte.checksum_hypothesis} · checksum?{/if}</li>{/each}</ul>
                </details></td>
              </tr>
            {:else}
              <tr><td colspan="6" class="can-muted">No valid frames in the capture</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </details>

  {#if error}<p class="can-error" role="alert">{error}</p>{/if}
</section>

<style>
  .can-workbench { display:grid; gap:.75rem; min-width:0; min-height:0; overflow:auto; padding:.85rem; color:var(--text-primary,#e7edf2); }
  .can-workbench[hidden] { display:none; }
  .can-heading,.can-connection,.can-capture-row,.can-field-row { display:flex; align-items:center; gap:.6rem; min-width:0; }
  .can-heading { justify-content:space-between; }
  .can-heading h2 { margin:.1rem 0 0; font-size:1.05rem; }
  .eyebrow { margin:0; color:var(--text-muted,#8ca0ae); font-size:.68rem; letter-spacing:.1em; }
  .can-connection { flex-wrap:wrap; }
  label { display:grid; gap:.25rem; color:var(--text-muted,#a8b6c1); font-size:.75rem; }
  .can-connection label:first-child { flex:1 1 15rem; }
  .can-connection label:not(:first-child) { flex:0 1 7rem; }
  .can-check { display:flex; align-items:center; gap:.4rem; white-space:nowrap; }
  .can-state,.can-capture-row,.can-scan-settings,.can-fields-settings,.can-field-row,.can-send-settings,.can-live-data,.can-replay-report { border:1px solid var(--border,#344652); border-radius:.55rem; background:var(--panel,#192630); }
  .can-state { padding:.3rem .55rem; color:var(--text-muted,#9badb8); font-size:.75rem; }
  .can-state.online { color:var(--accent,#56c8bd); }
  .can-capture-row { padding:.55rem; }
  .can-capture-row strong { white-space:nowrap; }
  .can-capture-path { overflow:hidden; flex:1; color:var(--text-muted,#9badb8); text-overflow:ellipsis; white-space:nowrap; }
  .can-muted,.can-hint { color:var(--text-muted,#9badb8); font-size:.76rem; }
  .can-source { flex:1 1 100%; color:var(--text-muted,#9badb8); font-size:.75rem; overflow-wrap:anywhere; }
  details { min-width:0; padding:.55rem; }
  summary { cursor:pointer; color:var(--accent,#56c8bd); font-weight:650; }
  .can-scan-form,.can-field-form { display:flex; flex-wrap:wrap; align-items:end; gap:.55rem; margin-top:.65rem; }
  .can-scan-form label,.can-field-form label { min-width:7rem; flex:1 1 8rem; }
  .can-field-form label.can-check,.can-scan-form label.can-check { flex:0 0 auto; min-height:2.5rem; }
  .can-error { color:var(--danger,#ff7f86); font-size:.78rem; }
  .can-field-row { justify-content:space-between; margin-top:.35rem; padding:.4rem .55rem; font-size:.78rem; }
  .can-send-form { display:flex; flex-wrap:wrap; align-items:end; gap:.55rem; margin-top:.65rem; }
  .can-send-form label { min-width:6rem; flex:1 1 7rem; }
  .can-send-form label.can-check { flex:0 0 auto; }
  .can-table-wrap { max-height:16rem; overflow:auto; margin-top:.5rem; border:1px solid var(--border,#344652); border-radius:.45rem; }
  table { width:100%; border-collapse:collapse; font-size:.72rem; font-variant-numeric:tabular-nums; }
  th,td { padding:.35rem .45rem; border-bottom:1px solid var(--border,#344652); text-align:left; white-space:nowrap; }
  th { position:sticky; top:0; background:var(--panel,#192630); color:var(--text-muted,#9badb8); }
  code { overflow-wrap:anywhere; white-space:normal; }
  h3 { margin:.65rem 0 .3rem; font-size:.82rem; }
  .can-event-list { display:grid; gap:.3rem; max-height:10rem; overflow:auto; margin:.4rem 0 0; padding-left:1rem; font-size:.73rem; }
  .can-event-list details { padding:.25rem 0; border:0; background:none; }
  .can-event-list details summary { font-size:.7rem; }
  .can-event-list details code { display:block; margin:.2rem 0; }
  .can-byte-list { max-height:11rem; overflow:auto; margin:.3rem 0; padding-left:1rem; font-size:.67rem; }
  .can-report-counts { display:flex; flex-wrap:wrap; gap:.35rem; margin:.5rem 0; }
  .can-report-counts span { padding:.25rem .4rem; border:1px solid var(--border,#344652); border-radius:.4rem; font-size:.72rem; }
  .can-rate-summary { display:flex; flex-wrap:wrap; gap:.3rem; margin:.4rem 0; }
  .can-rate-summary span { padding:.2rem .35rem; border:1px solid var(--border,#344652); border-radius:.35rem; color:var(--text-muted,#9badb8); font-size:.68rem; }
  .can-report-status { color:var(--accent,#56c8bd); font-size:.76rem; }
  .can-hint { margin:.65rem 0 .1rem; }
  @media (max-width:650px) { .can-capture-row { flex-wrap:wrap; } .can-capture-path { flex:1 1 100%; } }
</style>
