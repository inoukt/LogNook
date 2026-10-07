import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const can = await import("../src/canWorkbench.ts").catch(() => null);
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
const graphs = await readFile(new URL("../src/SignalGraphs.svelte", import.meta.url), "utf8");
const chart = await readFile(new URL("../src/LiveChart.svelte", import.meta.url), "utf8");
const panel = await readFile(new URL("../src/CanWorkbenchPanel.svelte", import.meta.url), "utf8").catch(() => "");
const trafficPanel = await readFile(new URL("../src/CanTrafficPanel.svelte", import.meta.url), "utf8").catch(() => "");
const oemPanel = await readFile(new URL("../src/OemCatalogPanel.svelte", import.meta.url), "utf8");
const styles = await readFile(new URL("../src/app.css", import.meta.url), "utf8");
const commands = await readFile(new URL("../src-tauri/src/commands/can.rs", import.meta.url), "utf8");
const registration = await readFile(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");

const field = {
  id: "can:engine-speed",
  name: "Engine speed CAN",
  can_id: 0x123,
  extended: false,
  start_bit: 0,
  bit_length: 16,
  byte_order: "intel",
  signed: false,
  factor: 0.25,
  offset: 0,
  unit: "rpm",
};

test("CAN workbench defaults to 500 kbit/s listen-only", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.deepEqual(can.defaultCanWorkbenchConfig, { bitrate: 500_000, listen_only: true });
});

test("CAN identifiers and field definitions reject invalid ranges before commands", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.equal(can.parseCanId("7E0", false), 0x7e0);
  assert.equal(can.parseCanId("0x1ABCDE", true), 0x1abcde);
  assert.equal(can.parseCanId("800", false), null);
  assert.equal(can.parseCanId("12junk", true), null);
  assert.equal(can.canSignalDefinitionError(field), null);
  assert.match(can.canSignalDefinitionError({ ...field, id: "engine_rpm" }), /start with can:/i);
  assert.match(can.canSignalDefinitionError({ ...field, start_bit: 63, bit_length: 2 }), /exceeds/i);
  assert.match(can.canSignalDefinitionError({ ...field, factor: Number.NaN }), /finite/i);
});

test("CAN scan configs preserve explicit Mode 01, 21, and 22 routes and bounds", () => {
  assert.ok(can, "CAN workbench helpers exist");
  const route = { request_id: 0x7e0, response_id: 0x7e8, extended: false };
  assert.equal(can.canScanConfigError({
    mode: { kind: "mode_01_support", ...route }, pacing_ms: 100, response_timeout_ms: 1000,
  }), null);
  assert.equal(can.canScanConfigError({
    mode: { kind: "mode_21", ...route, start_pid: 0, end_pid: 0xff }, pacing_ms: 100, response_timeout_ms: 1000,
  }), null);
  assert.equal(can.canScanConfigError({
    mode: { kind: "mode_22", ...route, start_did: 0, end_did: 0xffff }, pacing_ms: 100, response_timeout_ms: 1000,
  }), null);
  assert.match(can.canScanConfigError({
    mode: { kind: "mode_22", ...route, start_did: 0xffff, end_did: 0 }, pacing_ms: 100, response_timeout_ms: 1000,
  }), /start.*end/i);
  assert.deepEqual(can.buildCanScanConfig({
    kind: "mode_21", requestId: "7E0", responseId: "7E8", extended: false,
    start: "00", end: "FF", pacingMs: 100, responseTimeoutMs: 1000,
  }), {
    mode: { kind: "mode_21", request_id: 0x7e0, response_id: 0x7e8, extended: false, start_pid: 0, end_pid: 0xff },
    pacing_ms: 100, response_timeout_ms: 1000,
  });
  assert.deepEqual(can.buildCanScanConfig({
    kind: "can_id_range", requestId: "", responseId: "", extended: false,
    start: "7E0", end: "7EF", payload: "02 10 01", pacingMs: 20, responseTimeoutMs: 100,
  }), {
    mode: { kind: "can_id_range", start_id: 0x7e0, end_id: 0x7ef, extended: false, payload: [2, 0x10, 1] },
    pacing_ms: 20, response_timeout_ms: 100,
  });
  assert.deepEqual(can.buildCanScanConfig({
    kind: "mode_01_pids", requestId: "7DF", responseId: "7E8", extended: false,
    start: "00", end: "02", pacingMs: 100, responseTimeoutMs: 1000,
  }), {
    mode: { kind: "mode_01_pids", request_id: 0x7df, response_id: 0x7e8, extended: false, pids: [0, 1, 2], repeat: false },
    pacing_ms: 100, response_timeout_ms: 1000,
  });
});

test("diagnostic route labels and keys distinguish standard from extended IDs", () => {
  const standard = { request_id: 0x7e0, response_id: 0x7e8, extended: false };
  const extended = { ...standard, extended: true };
  assert.equal(can.formatCanDiagnosticRoute(standard), "11-bit 0x7E0 → 0x7E8");
  assert.equal(can.formatCanDiagnosticRoute(extended), "29-bit 0x7E0 → 0x7E8");
  assert.notEqual(can.canDiagnosticRouteKey(standard), can.canDiagnosticRouteKey(extended));
  assert.match(panel, /formatCanDiagnosticRoute\(timing\.route\)/);
  assert.match(panel, /canDiagnosticRouteKey\(provenance\.route\)/);
});

test("active transmit requires a connected active adapter and an open paired capture", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.equal(can.canTransmitEnabled(null, false), false);
  assert.equal(can.canTransmitEnabled({ connected: true, listen_only: true }, true), false);
  assert.equal(can.canTransmitEnabled({ connected: true, listen_only: false }, false), false);
  assert.equal(can.canTransmitEnabled({ connected: true, listen_only: false }, true), true);
});

test("manual CAN payloads parse strict byte tokens and produce classic TX frames", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.deepEqual(can.parseCanPayload("02 21 01"), [2, 0x21, 1]);
  assert.deepEqual(can.parseCanPayload("0x02, 0x21, 01"), [2, 0x21, 1]);
  assert.equal(can.parseCanPayload("1FF"), null);
  assert.equal(can.parseCanPayload("AA BB CC DD EE FF 00 11 22"), null);
  assert.deepEqual(can.createCanTxFrame(0x7e0, false, [2, 0x21, 1]), {
    elapsed_ns: 0, wall_time_unix_ns: 0, can_id: 0x7e0, extended: false,
    direction: "tx", data: [2, 0x21, 1], flags: 0,
  });
  assert.equal(can.createCanTxFrame(0x800, false, []), null);
  const frames = Array.from({ length: 5 }, (_, can_id) => ({ can_id }));
  assert.deepEqual(can.appendCanFrames(frames.slice(0, 2), frames.slice(2), 3), frames.slice(-3));
});

test("live CAN summary accumulates per-ID counts, period, latest data, and ASCII across batches", () => {
  assert.ok(can, "CAN workbench helpers exist");
  const frame = (elapsed_ns, direction, data, extended = false) => ({
    elapsed_ns, wall_time_unix_ns: elapsed_ns, can_id: 0x123, extended, direction, data, flags: 0,
  });
  let summary = can.appendCanTrafficSummary([], [frame(1_000_000_000, "rx", [0x41, 0x42])]);
  summary = can.appendCanTrafficSummary(summary, [
    frame(1_002_000_000, "rx", [0x43, 0x0a]),
    frame(1_003_000_000, "tx", [0x44]),
    frame(1_004_000_000, "rx", [0x45], true),
  ]);
  assert.deepEqual(summary, [
    {
      can_id: 0x123, extended: false, rx_count: 2, tx_count: 1,
      last_data: [0x44], last_direction: "tx", first_rx_elapsed_ns: 1_000_000_000,
      last_rx_elapsed_ns: 1_002_000_000, period_ms: 2, rate_hz: 500, ascii: "D",
    },
    {
      can_id: 0x123, extended: true, rx_count: 1, tx_count: 0,
      last_data: [0x45], last_direction: "rx", first_rx_elapsed_ns: 1_004_000_000,
      last_rx_elapsed_ns: 1_004_000_000, period_ms: null, rate_hz: null, ascii: "E",
    },
  ]);
});

test("saved CAN field definitions restore only unique, valid typed fields", () => {
  assert.equal(typeof can?.restoreCanSignalDefinitions, "function");
  if (!can?.restoreCanSignalDefinitions) return;
  const newer = { ...field, name: "Latest name" };
  assert.deepEqual(can.restoreCanSignalDefinitions(JSON.stringify([
    field,
    { ...field, id: "invalid id" },
    { ...field, bit_length: 65 },
    newer,
  ])), [newer]);
  assert.deepEqual(can.restoreCanSignalDefinitions("not json"), []);
});

test("CAN field graph selections and definitions survive restart", () => {
  assert.match(app, /canSignalDefinitions = restoreCanSignalDefinitions\(savedCanSignalDefinitions\)/);
  assert.match(app, /localStorage\.setItem\(canSignalDefinitionsKey, JSON\.stringify\(canSignalDefinitions\)\)/);
  assert.match(app, /canSignalDefinitions\.map\(\(\{ id \}\) => id\)/);
  assert.match(app, /enabled && canSignalDefinitions\.some[\s\S]*?rememberWorkbenchGraphSignalId/);
});

test("connecting a standard OBD adapter filters out saved CAN fields", () => {
  assert.match(app, /function requestedAdapterSignals\(\)[\s\S]*?!id\.startsWith\("can:"\)/);
  assert.match(app, /const requested = requestedAdapterSignals\(\)/);
});

test("a successful new CAN capture clears the old trigger window", () => {
  assert.match(panel, /await commands\.startCapture\(path\);[\s\S]*?onClearTriggerCapture\(\)/);
  assert.match(app, /onClearTriggerCapture=\{clearCanTriggerCapture\}/);
});

test("live CAN summary counts frames beyond the recent display buffer", () => {
  assert.ok(can, "CAN workbench helpers exist");
  const frames = Array.from({ length: 405 }, (_, index) => ({
    elapsed_ns: index * 1_000_000, wall_time_unix_ns: index * 1_000_000,
    can_id: 0x321, extended: false, direction: "rx", data: [index & 0xff], flags: 0,
  }));
  let summary = [];
  for (let index = 0; index < frames.length; index += 80) {
    summary = can.appendCanTrafficSummary(summary, frames.slice(index, index + 80));
  }
  assert.equal(summary[0].rx_count, 405);
  assert.deepEqual(summary[0].last_data, [404 & 0xff]);
});

test("CAN monitor and decoded graph share a vertically resizable right workspace", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.equal(can.clampCanMonitorHeight(40, 600), 120);
  assert.equal(can.clampCanMonitorHeight(250, 600), 250);
  assert.equal(can.clampCanMonitorHeight(900, 600), 432);
  assert.equal(can.clampCanMonitorHeight(40, 200), 120);
  assert.equal(can.adjustCanMonitorHeight(240, 16, 600), 256);
  assert.equal(can.adjustCanMonitorHeight(420, 16, 600), 432);
  assert.equal(can.adjustCanMonitorHeight(136, -16, 600), 120);
  assert.match(app, /class:can-split={!session && pidPickerView === "can"}/);
  assert.match(app, /<CanTrafficPanel[\s\S]*?trafficSummary=\{canTrafficSummary\}/);
  assert.match(app, /role="slider" tabindex="0" aria-orientation="vertical"[\s\S]*?aria-label="CAN monitor height"[\s\S]*?onpointerdown[\s\S]*?onpointermove[\s\S]*?onkeydown/);
  assert.match(app, /<CanTrafficPanel[\s\S]*?class="can-monitor-splitter"[\s\S]*?SignalGraphs/);
  assert.match(trafficPanel, /aria-label="Live CAN ID counts"/);
  assert.doesNotMatch(panel, /aria-label="Live CAN ID counts"/, "traffic view is removed from the narrow controls panel");
});

test("disconnected live graphs label retained values as the last received samples", () => {
  assert.match(graphs, /showDisconnectedSamples = \$derived\(!live && !timeWindow && traces\.some\(\(trace\) => trace\.samples\.length > 0\)\)/);
  assert.match(graphs, /Disconnected · showing last received samples/);
  assert.match(chart, /isLiveTraceStale\(trace\.samples, liveDisplayTimeNs/);
  assert.match(chart, /preference\.view !== "line" \|\| stale/);
  assert.match(chart, /class="chart-cursor-readout" role="status" aria-live="polite">[\s\S]*?class="trace-stale"/);
});

test("a new CAN session resets its event history and only its own signal samples", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.deepEqual(can.emptyCanSessionEvents(), {
    frames: [], trafficSummary: [], scanResults: [], txStatuses: [], transportErrors: [], scanRunning: false, triggerCapture: null,
  });
  const latest = {
    engine_rpm: { signal_id: "engine_rpm", value: 900 },
    "can:engine-speed": { signal_id: "can:engine-speed", value: 1234 },
  };
  const series = {
    engine_rpm: [latest.engine_rpm],
    "can:engine-speed": [latest["can:engine-speed"]],
  };
  assert.deepEqual(can.clearCanSessionSamples(latest, series, ["can:engine-speed"]), {
    latest: { engine_rpm: latest.engine_rpm },
    series: { engine_rpm: [latest.engine_rpm] },
  });
  assert.match(app, /if \(event\.payload\.connected && !canWorkbenchStatus\?\.connected\) \{[\s\S]*?const empty = emptyCanSessionEvents\(\)/, "new connected status starts with fresh CAN session event state");
  for (const assignment of ["canFrames = empty.frames", "canTrafficSummary = empty.trafficSummary", "canScanResults = empty.scanResults", "canTxStatuses = empty.txStatuses", "canTransportErrors = empty.transportErrors"]) {
    assert.ok(app.includes(assignment), `new CAN session resets ${assignment}`);
  }
  assert.match(app, /emptyCanSessionEvents\(\)/, "a new CAN connection clears the old frame/scan/TX/error view");
  assert.match(app, /clearCanSignalSamples\(/, "CAN elapsed samples are cleared on session changes");
  assert.match(app, /event\.type === "disconnected"[\s\S]*?clearCanSignalSamples\(\)/, "disconnect clears CAN samples before another elapsed timeline starts");
});

test("changed and removed CAN definitions clear stale values while unchanged definitions stay live", () => {
  assert.ok(can, "CAN workbench helpers exist");
  const unchanged = { id: "can:unchanged", can_id: 1, name: "Steady", unit: "rpm", factor: 1 };
  const changed = { id: "can:changed", can_id: 2, name: "Old", unit: "rpm", factor: 1 };
  const removed = { id: "can:removed", can_id: 3, name: "Removed", unit: "rpm", factor: 1 };
  assert.deepEqual(can.canSignalIdsNeedingReset(
    [unchanged, changed, removed],
    [unchanged, { ...changed, can_id: 4 }],
  ), ["can:changed", "can:removed"]);
  assert.match(app, /canSignalIdsNeedingReset\(canSignalDefinitions, fields\)/, "field updates detect removed or redefined CAN IDs");
  assert.match(app, /clearCanSignalSamples\(resetSignalIds\)/, "only affected CAN signal values are cleared");
});

test("replay status distinguishes absent sidecars from sidecars without a recognized session close", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.equal(can.replaySidecarStatus({ evidence_journal: null, evidence_journal_error: null, session_finalized: null }), "No JSONL sidecar found");
  assert.equal(can.replaySidecarStatus({ evidence_journal: null, evidence_journal_error: null, session_finalized: false }), "No JSONL sidecar found");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", evidence_journal_error: null, session_finalized: null }), "JSONL sidecar found; session state is unrecognized");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", evidence_journal_error: null, session_finalized: false }), "JSONL session is incomplete");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", evidence_journal_error: null, session_finalized: true }), "JSONL session closed cleanly");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", evidence_journal_error: "permission denied", session_finalized: null }), "JSONL sidecar is unreadable");
  assert.match(panel, /evidence_journal_error/, "unreadable sidecar errors are visible in the replay report");
  assert.match(commands, /evidence_journal_open_error/);
});

test("typed CAN command wrappers keep Tauri names and argument shapes aligned", async () => {
  assert.ok(can, "CAN workbench helpers exist");
  const calls = [];
  const invoke = async (name, args) => { calls.push([name, args]); return null; };
  const api = can.createCanWorkbenchCommands(invoke);
  await api.connect({ id: "usb:device", interface_number: 0, channel: 0 }, can.defaultCanWorkbenchConfig);
  await api.configureFields([field]);
  await api.startCapture("D:/captures/run.candump.csv");
  await api.markEvent("Traction control toggle", 1000, 1000);
  await api.sendFrame({ elapsed_ns: 0, wall_time_unix_ns: 0, can_id: 0x7e0, extended: false, direction: "tx", data: [2, 0x21, 1], flags: 0 }, 100, 1);
  await api.startScan({ mode: { kind: "mode_01_support", request_id: 0x7e0, response_id: 0x7e8, extended: false }, pacing_ms: 100, response_timeout_ms: 1000 });
  await api.stopScan();
  await api.startOemCandidate("scion:21:0100");
  await api.replay("D:/captures/run.candump.csv", [field]);
  assert.deepEqual(calls, [
    ["connect_candlelight", { selection: { id: "usb:device", interface_number: 0, channel: 0 }, config: { bitrate: 500_000, listen_only: true } }],
    ["configure_can_signals", { fields: [field] }],
    ["start_can_capture", { path: "D:/captures/run.candump.csv" }],
    ["mark_can_event", { label: "Traction control toggle", preMs: 1000, postMs: 1000 }],
    ["send_can_frame", { frame: { elapsed_ns: 0, wall_time_unix_ns: 0, can_id: 0x7e0, extended: false, direction: "tx", data: [2, 0x21, 1], flags: 0 }, pacingMs: 100, repeat: 1 }],
    ["start_can_scan", { config: { mode: { kind: "mode_01_support", request_id: 0x7e0, response_id: 0x7e8, extended: false }, pacing_ms: 100, response_timeout_ms: 1000 } }],
    ["stop_can_scan", undefined],
    ["start_can_oem_candidate", { candidateId: "scion:21:0100" }],
    ["replay_can_capture_report", { path: "D:/captures/run.candump.csv", fields: [field], obdSamples: [] }],
  ]);
});

test("configured CAN fields become ordinary signal-picker graph options", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.deepEqual(can.canSignalOption(field), {
    id: "can:engine-speed", name: "Engine speed CAN", unit: "rpm", description: "CAN 0x123 · bits 0–15 · Intel",
  });
  assert.match(app, /\.\.\.\s*canSignalOptions/, "CAN field options are merged into the normal signal picker");
  assert.match(app, /<CanWorkbenchPanel[\s\S]*?hidden=\{(?:session !== null \|\| )?pidPickerView !== "can"\}/, "the CAN panel stays mounted when another picker view is active");
  assert.match(app, /CanWorkbenchStatus|canWorkbenchStatus/, "CAN connection status flows through App's event handler");
  assert.match(app, /connected = event\.payload\.connected/, "CAN status controls the existing app-wide Disconnect/Record state");
  assert.match(styles, /pid-picker-tabs\.three-tabs\s*\{\s*grid-template-columns:\s*repeat\(3/, "the three signal/workbench tabs stay on one row");
});

test("CAN workbench navigation and primary controls stay compact and visible", () => {
  assert.match(app, />Live<\/button>[\s\S]*?>OEM<\/button>[\s\S]*?>CAN<\/button>/);
  assert.match(panel, /class="can-adapter-row"/);
  assert.match(panel, /class="can-connection-options"/);
  assert.doesNotMatch(panel, /<header class="can-heading">/);
  assert.match(panel, /<details class="can-trigger-settings" open>/);
  assert.match(panel, /<details class="can-scan-settings" open>/);
  assert.match(panel, /<details class="can-fields-settings" open>/);
  assert.match(panel, /<details class="can-send-settings" open>/);
  assert.match(panel, /\.can-workbench\s*\{[^}]*gap:\s*4px/);
  assert.match(panel, /\.can-connection-options\s*\{[^}]*grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)/);
});

test("manual vehicle event marks repeat by label and replay ranks linked CAN changes", () => {
  assert.match(panel, /placeholder="e\.g\. traction control toggle"/);
  assert.match(panel, /commands\.markEvent\(eventLabel, eventPreMs, eventPostMs\)/);
  assert.match(panel, /event_correlations\.length/);
  assert.match(panel, /candidate\.changed_events/);
  assert.match(commands, /CanEventCorrelator::new\(&report\.manual_event_marks\)/);
  assert.match(commands, /CanEvidenceRecord::ManualEventMarked/);
  assert.match(registration, /commands::mark_can_event/);
});

test("capture path dialogs are native and registered for paired CAN CSV evidence", () => {
  assert.match(commands, /choose_can_capture_save_path/);
  assert.match(commands, /choose_can_capture_open_path/);
  assert.match(registration, /commands::choose_can_capture_save_path/);
  assert.match(registration, /commands::choose_can_capture_open_path/);
  for (const command of ["choose_can_dbc_export_path", "choose_can_trigger_save_path", "save_can_trigger_capture", "compare_can_captures", "export_can_dbc_skeleton"]) {
    assert.match(commands, new RegExp(command));
    assert.match(registration, new RegExp(`commands::${command}`));
  }
});

test("Workbench renders bounded frames, scan results, echo status, transport evidence, and Rust replay census", () => {
  for (const marker of ["frames={canFrames}", "scanResults={canScanResults}", "txStatuses={canTxStatuses}", "transportErrors={canTransportErrors}", "scanRunning={canScanRunning}"]) {
    assert.ok(app.includes(marker), `App passes ${marker}`);
  }
  for (const marker of ["Echo confirmed", "raw_usb", "replaySidecarStatus", "evidence_records", "chooseCaptureOpenPath", "sendFrame"]) {
    assert.ok(panel.includes(marker), `CAN controls panel presents ${marker}`);
  }
  for (const marker of ["aria-label=\"Recent CAN frames\"", "aria-label=\"Live CAN ID counts\"", "rate_hz", "Counts since connection", "Frames"]) {
    assert.ok(trafficPanel.includes(marker), `main traffic panel presents ${marker}`);
  }
  assert.match(app, /queuedCanFrames = appendCanFrames\(queuedCanFrames, event\.payload, 400\)/, "raw frame updates stay bounded while paint is throttled");
  assert.match(app, /queuedCanTrafficSummary = appendCanTrafficSummary\(queuedCanTrafficSummary \?\? canTrafficSummary, event\.payload\)/, "every incoming frame updates the pending cumulative counts");
  assert.match(app, /requestAnimationFrame\([\s\S]*?const incoming = queuedCanFrames;[\s\S]*?appendCanFrames\(canFrames, incoming\)[\s\S]*?canTrafficSummary = queuedCanTrafficSummary \?\? canTrafficSummary/, "recent frame rows and cumulative counts coalesce to one UI update per animation frame");
  assert.match(panel, /CAN ID range/);
  assert.match(panel, /Mode 01 PIDs/);
  assert.match(panel, /0\/0\s*default|interfaceNumber = 0[\s\S]*?channel = 0/);
});

test("OEM catalog can launch a source-backed CAN candidate through the serialized engine command", () => {
  assert.match(oemPanel, /onTryCanCandidate/);
  assert.match(oemPanel, /CAN read/);
  assert.match(app, /onTryCanCandidate=\{startCanOemCandidate\}/);
  assert.match(app, /start_can_oem_candidate/);
  assert.match(app, /canCandidateDisabled=/);
  assert.match(app, /canCandidateDisabled=\{!canTransmitEnabled\(canWorkbenchStatus, canCaptureOpen\)/, "the CAN action requires an active connection and open paired capture");
  assert.match(app, /const spec = await invoke<OemCandidateTrySpec>\("start_can_oem_candidate", \{ candidateId \}\);[\s\S]*?graphResearchCandidate\(spec\.candidate\)/, "CAN reads add the catalog-decoded signal to the live graph");
  assert.match(commands, /start_can_oem_candidate\(spec\.clone\(\)\)/, "the backend passes the exact catalog spec to the serialized CAN worker");
  assert.match(oemPanel, /disabled=\{canCandidateDisabled\}/, "the OEM candidate action obeys the CAN transmit gate");
});

test("saved capture reports expose timing hypotheses, comparisons, and unverified DBC export", async () => {
  const calls = [];
  const api = can.createCanWorkbenchCommands(async (name, args) => { calls.push([name, args]); return null; });
  await api.compareCaptures("left.candump.csv", "right.candump.csv");
  await api.exportDbcSkeleton("left.candump.csv", "left.dbc");
  assert.deepEqual(calls, [
    ["compare_can_captures", { leftPath: "left.candump.csv", rightPath: "right.candump.csv" }],
    ["export_can_dbc_skeleton", { capturePath: "left.candump.csv", path: "left.dbc" }],
  ]);
  for (const marker of ["compareCaptures", "exportDbcSkeleton", "timings", "correlation_hypotheses", "UNVERIFIED"]) {
    assert.ok(panel.includes(marker), `CAN report UI presents ${marker}`);
  }
});

test("correlation hypothesis rows identify their OBD source and ECU with fallbacks", () => {
  const sx = { obd_source_id: "obd:sx", obd_ecu_id: "7E8", obd_signal_id: "rpm" };
  const elm = { obd_source_id: "obd:elm", obd_ecu_id: "engine", obd_signal_id: "rpm" };
  assert.equal(can.formatCanCorrelationSource(sx), "obd:sx · ECU 7E8");
  assert.equal(can.formatCanCorrelationSource(elm), "obd:elm · ECU engine");
  assert.notEqual(can.formatCanCorrelationSource(sx), can.formatCanCorrelationSource(elm));
  assert.equal(can.formatCanCorrelationSource({ obd_source_id: "", obd_ecu_id: null }), "unknown source · ECU unknown ECU");
  assert.match(panel, /formatCanCorrelationSource\(hypothesis\)/);
});

test("correlation hypothesis keys distinguish the same source and signal from different ECUs", () => {
  const engine7e8 = {
    field_id: "can:engine-speed", obd_signal_id: "rpm", obd_source_id: "obd:scion", obd_ecu_id: "7E8",
  };
  const engine7e9 = { ...engine7e8, obd_ecu_id: "7E9" };
  assert.notEqual(can.canCorrelationHypothesisKey(engine7e8), can.canCorrelationHypothesisKey(engine7e9));
  assert.match(panel, /#each replayReport\.correlation_hypotheses as hypothesis \(canCorrelationHypothesisKey\(hypothesis\)\)/);
});

test("bounded manual trigger controls use the serialized backend and save the captured window", async () => {
  const calls = [];
  const api = can.createCanWorkbenchCommands(async (name, args) => { calls.push([name, args]); return null; });
  await api.armTrigger(5000, 2000, 1000);
  await api.triggerNow();
  await api.saveTriggerCapture("window.candump.csv", { trigger_elapsed_ns: 7, frames: [] });
  assert.deepEqual(calls, [
    ["arm_can_trigger", { preMs: 5000, postMs: 2000, maxFrames: 1000 }],
    ["trigger_can_now", undefined],
    ["save_can_trigger_capture", { path: "window.candump.csv", capture: { trigger_elapsed_ns: 7, frames: [] } }],
  ]);
  assert.match(panel, /Pre-trigger \(ms\)/);
  assert.match(panel, /Post-trigger \(ms\)/);
  assert.match(panel, /Mark trigger now/);
  assert.match(app, /event\.type === "canTriggerCapture"/);
});

test("trigger windows enforce bounded millisecond durations and frame counts", () => {
  assert.equal(can.canTriggerConfigError(0, 60_000, 1), null);
  assert.match(can.canTriggerConfigError(-1, 10, 1), /0–60000/);
  assert.match(can.canTriggerConfigError(10, 60_001, 1), /0–60000/);
  assert.match(can.canTriggerConfigError(10, 10, 0), /1–100000/);
  assert.match(can.canTriggerConfigError(10, 10, 100_001), /1–100000/);
});
