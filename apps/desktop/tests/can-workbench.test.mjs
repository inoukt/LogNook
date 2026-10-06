import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const can = await import("../src/canWorkbench.ts").catch(() => null);
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
const panel = await readFile(new URL("../src/CanWorkbenchPanel.svelte", import.meta.url), "utf8").catch(() => "");
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

test("recent per-ID counts and RX rates use only the bounded timestamped frame buffer", () => {
  assert.ok(can, "CAN workbench helpers exist");
  const frames = [
    { elapsed_ns: 100_000_000, can_id: 0x123, extended: false, direction: "rx" },
    { elapsed_ns: 200_000_000, can_id: 0x123, extended: false, direction: "rx" },
    { elapsed_ns: 300_000_000, can_id: 0x123, extended: false, direction: "tx" },
    { elapsed_ns: 300_000_000, can_id: 0x1abcdef, extended: true, direction: "rx" },
  ];
  assert.deepEqual(can.recentCanRates(frames), [
    { can_id: 0x123, extended: false, rx_count: 2, tx_count: 1, rate_hz: 10 },
    { can_id: 0x1abcdef, extended: true, rx_count: 1, tx_count: 0, rate_hz: null },
  ]);
});

test("a new CAN session resets its event history and only its own signal samples", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.deepEqual(can.emptyCanSessionEvents(), {
    frames: [], scanResults: [], txStatuses: [], transportErrors: [], scanRunning: false,
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
  for (const assignment of ["canFrames = empty.frames", "canScanResults = empty.scanResults", "canTxStatuses = empty.txStatuses", "canTransportErrors = empty.transportErrors"]) {
    assert.ok(app.includes(assignment), `new CAN session resets ${assignment}`);
  }
  assert.match(app, /emptyCanSessionEvents\(\)/, "a new CAN connection clears the old frame/scan/TX/error view");
  assert.match(app, /clearCanSignalSamples\(/, "CAN elapsed samples are cleared on session changes");
  assert.match(app, /event\.type === "disconnected"[\s\S]*?clearCanSignalSamples\(\)/, "disconnect clears CAN samples before another elapsed timeline starts");
});

test("replay status distinguishes absent sidecars from sidecars without a recognized session close", () => {
  assert.ok(can, "CAN workbench helpers exist");
  assert.equal(can.replaySidecarStatus({ evidence_journal: null, session_finalized: null }), "No JSONL sidecar found");
  assert.equal(can.replaySidecarStatus({ evidence_journal: null, session_finalized: false }), "No JSONL sidecar found");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", session_finalized: null }), "JSONL sidecar found; session state is unrecognized");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", session_finalized: false }), "JSONL session is incomplete");
  assert.equal(can.replaySidecarStatus({ evidence_journal: "run.candump.jsonl", session_finalized: true }), "JSONL session closed cleanly");
});

test("typed CAN command wrappers keep Tauri names and argument shapes aligned", async () => {
  assert.ok(can, "CAN workbench helpers exist");
  const calls = [];
  const invoke = async (name, args) => { calls.push([name, args]); return null; };
  const api = can.createCanWorkbenchCommands(invoke);
  await api.connect({ id: "usb:device", interface_number: 0, channel: 0 }, can.defaultCanWorkbenchConfig);
  await api.configureFields([field]);
  await api.startCapture("D:/captures/run.candump.csv");
  await api.sendFrame({ elapsed_ns: 0, wall_time_unix_ns: 0, can_id: 0x7e0, extended: false, direction: "tx", data: [2, 0x21, 1], flags: 0 }, 100, 1);
  await api.startScan({ mode: { kind: "mode_01_support", request_id: 0x7e0, response_id: 0x7e8, extended: false }, pacing_ms: 100, response_timeout_ms: 1000 });
  await api.stopScan();
  await api.startOemCandidate("scion:21:0100");
  await api.replay("D:/captures/run.candump.csv", [field]);
  assert.deepEqual(calls, [
    ["connect_candlelight", { selection: { id: "usb:device", interface_number: 0, channel: 0 }, config: { bitrate: 500_000, listen_only: true } }],
    ["configure_can_signals", { fields: [field] }],
    ["start_can_capture", { path: "D:/captures/run.candump.csv" }],
    ["send_can_frame", { frame: { elapsed_ns: 0, wall_time_unix_ns: 0, can_id: 0x7e0, extended: false, direction: "tx", data: [2, 0x21, 1], flags: 0 }, pacingMs: 100, repeat: 1 }],
    ["start_can_scan", { config: { mode: { kind: "mode_01_support", request_id: 0x7e0, response_id: 0x7e8, extended: false }, pacing_ms: 100, response_timeout_ms: 1000 } }],
    ["stop_can_scan", undefined],
    ["start_can_oem_candidate", { candidateId: "scion:21:0100" }],
    ["replay_can_capture_report", { path: "D:/captures/run.candump.csv", fields: [field] }],
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

test("capture path dialogs are native and registered for paired CAN CSV evidence", () => {
  assert.match(commands, /choose_can_capture_save_path/);
  assert.match(commands, /choose_can_capture_open_path/);
  assert.match(registration, /commands::choose_can_capture_save_path/);
  assert.match(registration, /commands::choose_can_capture_open_path/);
});

test("Workbench renders bounded frames, scan results, echo status, transport evidence, and Rust replay census", () => {
  for (const marker of ["frames={canFrames}", "scanResults={canScanResults}", "txStatuses={canTxStatuses}", "transportErrors={canTransportErrors}", "scanRunning={canScanRunning}"]) {
    assert.ok(app.includes(marker), `App passes ${marker}`);
  }
  for (const marker of ["aria-label=\"Recent CAN frames\"", "Echo confirmed", "raw_usb", "replaySidecarStatus", "rate_hz", "evidence_records", "chooseCaptureOpenPath", "sendFrame"]) {
    assert.ok(panel.includes(marker), `CAN panel presents ${marker}`);
  }
  assert.match(app, /queuedCanFrames = appendCanFrames\(queuedCanFrames, event\.payload\)/, "pending raw frame display state stays bounded between paints");
  assert.match(app, /requestAnimationFrame\([\s\S]*?appendCanFrames\(canFrames, queuedCanFrames\)/, "raw frame events coalesce to one display update per animation frame");
  assert.match(panel, /CAN ID range/);
  assert.match(panel, /Mode 01 PIDs/);
  assert.match(panel, /recentCanRates\(frames\)/);
  assert.match(panel, /0\/0\s*default|interfaceNumber = 0[\s\S]*?channel = 0/);
});

test("OEM catalog can launch a source-backed CAN candidate through the serialized engine command", () => {
  assert.match(oemPanel, /onTryCanCandidate/);
  assert.match(oemPanel, /CAN read/);
  assert.match(app, /onTryCanCandidate=\{startCanOemCandidate\}/);
  assert.match(app, /start_can_oem_candidate/);
  assert.match(app, /canCandidateDisabled=/);
  assert.match(app, /canCandidateDisabled=\{!canTransmitEnabled\(canWorkbenchStatus, canCaptureOpen\)/, "the CAN action requires an active connection and open paired capture");
  assert.match(oemPanel, /disabled=\{canCandidateDisabled\}/, "the OEM candidate action obeys the CAN transmit gate");
});
