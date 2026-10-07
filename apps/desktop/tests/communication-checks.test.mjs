import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";

const checks = await import("../src/communicationChecks.ts").catch(() => null);
const can = await import("../src/canWorkbench.ts");
const panel = await readFile(new URL("../src/DiagnosticsPanel.svelte", import.meta.url), "utf8");
const canPanel = await readFile(new URL("../src/CanWorkbenchPanel.svelte", import.meta.url), "utf8");

const record = (sequence_id, overrides = {}) => ({
  sequence_id,
  timestamp_unix_ms: 1000,
  operation_id: `op-${sequence_id}`,
  adapter_id: "OpenPort2",
  operation: "acquisition",
  stage: "poll",
  phase: "started",
  duration_ms: null,
  signal_ids: ["engine_rpm"],
  signal_summaries: null,
  detail: null,
  transient: null,
  omitted_events: 0,
  ...overrides,
});

test("communication check summarizes only polls and PID deltas in its time window", () => {
  assert.ok(checks, "communication check helper exists");
  const rows = [
    record(1, { timestamp_unix_ms: 900, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 3, success_count: 2, error_count: 1, rate_hz: 1, latency_ms: 4 },
    ] }),
    record(2, { timestamp_unix_ms: 1100 }),
    record(3, { timestamp_unix_ms: 1200, phase: "succeeded", duration_ms: 10 }),
    record(4, { timestamp_unix_ms: 1300 }),
    record(5, { timestamp_unix_ms: 1400, phase: "succeeded", duration_ms: 30 }),
    record(6, { timestamp_unix_ms: 1500 }),
    record(7, { timestamp_unix_ms: 1600, phase: "failed", duration_ms: 50 }),
    record(8, { timestamp_unix_ms: 1700, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 8, success_count: 5, error_count: 2, rate_hz: 2, latency_ms: 6 },
    ] }),
    record(9, { timestamp_unix_ms: 1200, adapter_id: "Other adapter", phase: "failed", duration_ms: 90 }),
    record(10, { timestamp_unix_ms: 1800, phase: "failed", duration_ms: 90 }),
  ];

  assert.deepEqual(checks.summarizeCommunicationCheck(rows, "OpenPort2", 1000, 1700), {
    adapterId: "OpenPort2",
    startedAtMs: 1000,
    finishedAtMs: 1700,
    status: "issues",
    attempts: 3,
    successes: 2,
    failures: 1,
    medianLatencyMs: 30,
    p95LatencyMs: 50,
    signals: [{ signalId: "engine_rpm", supported: true, attempts: 5, successes: 3, errors: 1 }],
  });
});

test("communication check labels an empty interval as no data", () => {
  assert.ok(checks, "communication check helper exists");
  assert.equal(checks.summarizeCommunicationCheck([], "OpenPort2", 1000, 1100).status, "no_data");
});

test("communication check flags attributable PID failures even when poll batches succeed", () => {
  const rows = [
    record(1, { timestamp_unix_ms: 900, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 2, success_count: 2, error_count: 0, rate_hz: 1, latency_ms: 4 },
    ] }),
    record(2, { timestamp_unix_ms: 1100 }),
    record(3, { timestamp_unix_ms: 1200, phase: "succeeded", duration_ms: 10 }),
    record(4, { timestamp_unix_ms: 1300, stage: "pid_read", phase: "failed", signal_ids: ["engine_rpm"] }),
    record(5, { timestamp_unix_ms: 1400, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 3, success_count: 2, error_count: 1, rate_hz: 1, latency_ms: 4 },
    ] }),
  ];
  assert.equal(checks.summarizeCommunicationCheck(rows, "OpenPort2", 1000, 1400).status, "issues");
});

test("communication check catches PID failures before the next aggregate snapshot", () => {
  const rows = [
    record(1, { timestamp_unix_ms: 1100 }),
    record(2, { timestamp_unix_ms: 1200, phase: "succeeded", duration_ms: 10 }),
    record(3, { timestamp_unix_ms: 1300, stage: "pid_read", phase: "failed", signal_ids: ["engine_rpm"] }),
  ];
  assert.equal(checks.summarizeCommunicationCheck(rows, "OpenPort2", 1000, 1400).status, "issues");
});

test("communication check reports an in-flight PID failure even without a poll start in the window", () => {
  const rows = [record(1, {
    timestamp_unix_ms: 1100,
    stage: "pid_read",
    phase: "failed",
    signal_ids: ["engine_rpm"],
  })];
  assert.equal(checks.summarizeCommunicationCheck(rows, "OpenPort2", 1000, 1400).status, "issues");
});

test("communication check does not call successful empty polls healthy", () => {
  const rows = [
    record(1, { timestamp_unix_ms: 900, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 0, success_count: 0, error_count: 0, rate_hz: 0, latency_ms: null },
    ] }),
    record(2, { timestamp_unix_ms: 1100 }),
    record(3, { timestamp_unix_ms: 1200, phase: "succeeded", duration_ms: 10 }),
    record(4, { timestamp_unix_ms: 1300, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 1, success_count: 0, error_count: 0, rate_hz: 0, latency_ms: null },
    ] }),
  ];
  assert.equal(checks.summarizeCommunicationCheck(rows, "OpenPort2", 1000, 1400).status, "issues");
});

test("communication check retains its poll window when unrelated PID events flood the diagnostics view", () => {
  const rows = [
    record(1, { timestamp_unix_ms: 900, stage: "signal_summary", signal_summaries: [
      { signal_id: "engine_rpm", supported: true, attempt_count: 0, success_count: 0, error_count: 0, rate_hz: 0, latency_ms: null },
    ] }),
    record(2, { timestamp_unix_ms: 1100 }),
    record(3, { timestamp_unix_ms: 1200, phase: "succeeded", duration_ms: 10 }),
    ...Array.from({ length: 600 }, (_, index) => record(index + 4, {
      timestamp_unix_ms: 1250 + index,
      stage: "pid_read",
      phase: "succeeded",
    })),
    record(604, { timestamp_unix_ms: 1900 }),
    record(605, { timestamp_unix_ms: 2000, phase: "succeeded", duration_ms: 20 }),
  ];
  const captured = checks.appendCommunicationCheckRecords([], rows, "OpenPort2", 1000);
  assert.equal(captured.length, 5);
  assert.equal(checks.summarizeCommunicationCheck(captured, "OpenPort2", 1000, 2100).attempts, 2);
  assert.equal(checks.appendCommunicationCheckRecords(captured, captured, "OpenPort2", 1000).length, 5);
});

test("quick CAN check sends one Mode 01 PID 00 request on the configured route", () => {
  const config = can.buildCanCommunicationCheck("7E0", "7E8", false, 100, 1000);
  assert.deepEqual(config, {
    mode: { kind: "mode_01_pids", request_id: 0x7e0, response_id: 0x7e8, extended: false, pids: [0], repeat: false },
    pacing_ms: 100,
    response_timeout_ms: 1000,
  });
  assert.equal(can.buildCanCommunicationCheck("invalid", "7E8", false, 100, 1000), null);
  assert.match(canPanel, /Try CAN once/);
  assert.match(canPanel, /buildCanCommunicationCheck/);
});

test("Diagnostics offers a timed check only for a connected OBD adapter", () => {
  assert.match(panel, /Start 10 s check/);
  assert.match(panel, /connectedAdapterId/);
  assert.match(panel, /summarizeCommunicationCheck/);
});
