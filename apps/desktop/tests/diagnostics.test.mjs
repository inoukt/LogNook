import assert from "node:assert/strict";
import test from "node:test";
import ts from "typescript";
import { readFile } from "node:fs/promises";
const diagnostics = await import("../src/diagnostics.ts").catch(() => null);
const panel = await readFile(new URL("../src/DiagnosticsPanel.svelte", import.meta.url), "utf8").catch(() => "");
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
const record = (sequence_id, overrides = {}) => ({ sequence_id, timestamp_unix_ms: 1000, operation_id: "op-1", adapter_id: "COM4", operation: "connect", stage: "setup", phase: "succeeded", duration_ms: 12, signal_ids: [], signal_summaries: null, detail: null, transient: null, omitted_events: 0, ...overrides });

// Execute the panel's real script with Tauri dialogs/snapshot injected at the boundary.
// Rendering wiring is checked below; this harness deliberately does not simulate Svelte's DOM.
function panelHarness(props, invoke = async () => []) {
  const effects = [];
  const script = panel.match(/<script lang="ts">([\s\S]*?)<\/script>/)?.[1];
  assert.ok(script, "typed diagnostics panel exists");
  const compiled = ts.transpileModule(script.replace(/^\s*import .*?;\s*$/gm, ""), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
  }).outputText;
  return new Function("$props", "$state", "$effect", "$derived", "untrack", "invoke", "isTauri", "mergeDiagnostics", "filterDiagnostics", "selectedDiagnosticsRecording", "runLiveEffect", `${compiled}
    return { refresh, chooseRecording, exportBundle, append: () => runLiveEffect(), state: () => ({records, adapter, operation, errorsOnly, recordingPath, exportPath, error, actionBusy}) };
  `)(
    () => props, value => value, callback => effects.push(callback), value => value,
    callback => callback(), invoke, () => true, diagnostics.mergeDiagnostics, diagnostics.filterDiagnostics,
    diagnostics.selectedDiagnosticsRecording, () => effects[0](),
  );
}
test("diagnostics_panel_loads_snapshot_and_appends_live_events", async () => {
  assert.ok(diagnostics, "diagnostics helper exists");
  assert.deepEqual(diagnostics.mergeDiagnostics([record(2), record(3)], [record(1), record(2)]).map(r => r.sequence_id), [1, 2, 3]);
  assert.equal(diagnostics.mergeDiagnostics([], Array.from({ length: 510 }, (_, i) => record(i))).length, 500);
  assert.match(panel, /invoke<DiagnosticRecord\[\]>\("diagnostic_snapshot"\)/);
  assert.equal((app.match(/listen<DiagnosticRecord>\("diagnostics-event"/g) ?? []).length, 1);
  assert.doesNotMatch(panel, /\blisten\(/);
  let releaseSnapshot;
  const pendingSnapshot = new Promise(resolve => { releaseSnapshot = resolve; });
  const liveRecords = [record(2)];
  const harness = panelHarness({ liveRecords }, async command => {
    assert.equal(command, "diagnostic_snapshot");
    return pendingSnapshot;
  });
  const loading = harness.refresh();
  liveRecords.push(record(3));
  harness.append();
  releaseSnapshot([record(1), record(2)]);
  await loading;
  assert.deepEqual(harness.state().records.map(row => row.sequence_id), [1, 2, 3]);
  assert.match(app, /diagnosticsUnlisten\?\.\(\)/);
});

test("diagnostics_filters_select_adapter_operation_and_errors", () => {
  assert.ok(diagnostics);
  const rows = [record(1), record(2, {phase: "failed"}), record(3, {adapter_id: "COM5", phase: "failed"}), record(4, {operation: "identify", phase: "failed"})];
  assert.deepEqual(diagnostics.filterDiagnostics(rows, {adapter: "COM4", operation: "connect", errorsOnly: true}).map(r => r.sequence_id), [2]);
  assert.equal(diagnostics.filterDiagnostics(rows, {adapter: "", operation: "", errorsOnly: false}).length, 4);
  assert.match(panel, /aria-label="Adapter filter"/);
  assert.match(panel, /aria-label="Operation filter"/);
  assert.match(panel, /Errors only/);
});

test("diagnostics_view_never_renders_packet_or_sample_values", () => {
  assert.ok(diagnostics);
  const rendered = JSON.stringify(diagnostics.formatDiagnostic(record(1, { raw_request: "SECRET_TX", raw_response: "SECRET_RX", value: "SECRET_VALUE", signal_summaries: [{ signal_id: "rpm", success_count: 4, rate_hz: 2, latency_ms: 5, value: "SECRET_SUMMARY" }] })));
  assert.doesNotMatch(rendered, /SECRET/);
  assert.match(rendered, /4 successful/);
  assert.match(rendered, /2.00 Hz/);
  assert.doesNotMatch(panel, /JSON\.stringify|raw_request|raw_response|\.value\}/);
});

test("export_cancellation_keeps_panel_state", async () => {
  assert.ok(diagnostics);
  assert.equal(diagnostics.selectedDiagnosticsRecording("existing.lognook", null), "existing.lognook");
  assert.equal(diagnostics.selectedDiagnosticsRecording(null, null), null);
  assert.match(panel, /selectedDiagnosticsRecording\(recordingPath, await onChooseRecording\(\)\)/);
  assert.match(panel, /if \(path !== null\)/);
  const choices = ["chosen.lognook", null];
  const destinations = ["support.zip", null];
  const harness = panelHarness({ liveRecords: [record(7)], onChooseRecording: async () => choices.shift(), onExport: async path => {
    assert.equal(path, "chosen.lognook");
    return destinations.shift();
  }});
  harness.append();
  await harness.chooseRecording();
  await harness.exportBundle();
  const before = harness.state();
  await harness.chooseRecording();
  await harness.exportBundle();
  assert.deepEqual(harness.state(), before);
});

test("support_export_passes_optional_recording_path", () => {
  assert.ok(diagnostics);
  assert.deepEqual(diagnostics.supportBundleArgs(null), {recordingPath: null});
  assert.deepEqual(diagnostics.supportBundleArgs("chosen.lognook"), {recordingPath: "chosen.lognook"});
  assert.match(app, /invoke<string \| null>\("export_support_bundle", supportBundleArgs\(recordingPath\)\)/);
  assert.match(app, /invoke<string \| null>\("choose_diagnostics_recording"\)/);
  assert.match(panel, /recordingPath = \$state<string \| null>\(null\)/);
});
