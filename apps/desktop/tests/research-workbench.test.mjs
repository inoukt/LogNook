import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { runResearchQueue, toggleCandidateSelection } from "../src/researchQueue.ts";
import { oemCandidateTryState } from "../src/oemCandidateControls.ts";

const panel = await readFile(new URL("../src/OemCatalogPanel.svelte", import.meta.url), "utf8");
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
const types = await readFile(new URL("../src/types.ts", import.meta.url), "utf8");
const results = await readFile(new URL("../src/ResearchRunResults.svelte", import.meta.url), "utf8").catch(() => "");
const includes = (source, pattern, message) => assert.ok(pattern.test(source), message);
const excludes = (source, pattern, message) => assert.ok(!pattern.test(source), message);

test("workbench_shows_only_supported_quick_tests", () => {
  includes(app, />Test Workbench<\/button>/, "the catalog tab is named Test Workbench");
  includes(app, />Live PIDs<\/button>/, "the Live PIDs tab remains available");
  includes(panel, /selectedAdapter === "a30"[\s\S]*?A30_READ_TESTS/, "captured quick tests are guarded by the A30 adapter check");
  excludes(panel, /selectedAdapter === "openport2"[\s\S]{0,250}Try once/, "OpenPort has no OEM one-shot controls");
  assert.equal(oemCandidateTryState({
    service: "21", identifier: "01", request_header: "7E0", response_header: "7E8",
    source_blob_sha: "sha", source_url: "https://example.test/source", license: "CC", license_url: "https://example.test/license",
  }, "provider-1", false, "openport2").disabled, true);
});

test("candidate_selection_survives_catalog_navigation", () => {
  const onFirstPage = toggleCandidateSelection([], "candidate-1", true);
  const afterSearching = toggleCandidateSelection(onFirstPage, "candidate-2", true);
  const afterChangingPages = toggleCandidateSelection(afterSearching, "candidate-1", true);

  assert.deepEqual(afterChangingPages, ["candidate-1", "candidate-2"]);
  includes(panel, /selectedCandidateIds\.includes\(candidate\.candidate_id\)/, "candidate checkboxes use retained IDs");
  includes(panel, /toggleCandidateSelection\(/, "checkbox changes use the ordered selection helper");
  includes(panel, /selectedCandidateIds\.length/, "the retained selection count is shown");
});

test("mixed_validity_queue_continues_past_local_rejections", async () => {
  const valid = {
    service: "21", identifier: "01", request_header: "7E0", response_header: "7E8",
    source_blob_sha: "sha", source_url: "https://example.test/source", license: "CC", license_url: "https://example.test/license",
  };
  const candidates = { invalid: { ...valid, service: "10" }, valid };
  const calls = [];
  const outcomes = [];
  await runResearchQueue(["invalid", "valid"], async (id) => {
    const eligibility = oemCandidateTryState(candidates[id], "COM4", false, "obdlink_sx");
    if (eligibility.disabled) return { kind: "ineligible", reason: eligibility.reason };
    calls.push(id);
    return { kind: "result", id };
  }, () => false, (id, _index, result) => outcomes.push([id, result]));

  assert.deepEqual(calls, ["valid"]);
  assert.deepEqual(outcomes, [
    ["invalid", { kind: "ineligible", reason: "Candidate lacks a source-backed service 21/22 request and exact 11-bit CAN route" }],
    ["valid", { kind: "result", id: "valid" }],
  ]);
  includes(panel, /oemCandidateTryState\(item\.selection\.payload, candidateDeviceAtStart, false, selectedAdapterAtStart\)/, "Run selected applies static candidate and adapter checks without masking link loss");
  includes(panel, /const candidateDeviceAtStart = currentCandidateDevice\(\)/, "candidate eligibility uses the device selected at run start");
  includes(panel, /const selectedAdapterAtStart = selectedAdapter/, "the target adapter remains stable for the whole run");
  includes(panel, /kind:\s*"ineligible"/, "a local eligibility rejection is a row outcome, not a transport failure");
  includes(panel, /oem-disabled-reason[\s\S]*?tryState\.reason/, "disabled Try once controls explain their eligibility reason");
  includes(results, /if \(row\.error\) return row\.error/, "local rejection reasons are visible in the results table");
});

test("A30 queue records an unsupported route locally and continues to the next candidate", async () => {
  const candidate = {
    service: "22", identifier: "1234", request_header: "6F1", response_header: "607",
    source_blob_sha: "sha", source_url: "https://example.test/source", license: "CC", license_url: "https://example.test/license",
  };
  const candidates = {
    a30Ineligible: candidate,
    a30Valid: { ...candidate, request_header: "7E0", response_header: "7E8" },
  };
  const calls = [];
  const outcomes = [];
  await runResearchQueue(["a30Ineligible", "a30Valid"], async (id) => {
    const eligibility = oemCandidateTryState(candidates[id], "AA:BB:CC:DD:EE:FF", false, "a30");
    if (eligibility.disabled) return { kind: "ineligible", reason: eligibility.reason };
    calls.push(id);
    return { kind: "result", id };
  }, () => false, (id, _index, result) => outcomes.push([id, result]));

  assert.deepEqual(calls, ["a30Valid"]);
  assert.deepEqual(outcomes, [
    ["a30Ineligible", { kind: "ineligible", reason: "A30 OEM reads require a 0x7xx request CAN ID" }],
    ["a30Valid", { kind: "result", id: "a30Valid" }],
  ]);
});

test("run_selected_reports_progress_in_order", async () => {
  const calls = [];
  const progress = [];
  await runResearchQueue(["candidate-1", "candidate-2"], async (id, index) => {
    calls.push(id);
    return { id, index, outcome: "no_response" };
  }, () => false, (id, index, result) => progress.push([id, index, result.outcome]));

  assert.deepEqual(calls, ["candidate-1", "candidate-2"]);
  assert.deepEqual(progress, [["candidate-1", 0, "no_response"], ["candidate-2", 1, "no_response"]]);
  includes(panel, /runResearchQueue(?:<[^>]+>)?\(/, "selected tests run through the sequential queue helper");
  includes(panel, /queueIndex:\s*index\s*\+\s*1/, "Tauri receives one-based queue indices");
  includes(panel, /currentItem|current item/i, "the active request is shown in progress");
  includes(results, /row\.queue_index\} \/ \{row\.queue_size/, "result rows retain their original queue position");
});

test("stop_does_not_send_the_next_request", async () => {
  let stopped = false;
  const calls = [];
  const progress = [];
  await runResearchQueue(["candidate-1", "candidate-2"], async (id) => {
    calls.push(id);
    stopped = true;
    return "in-flight result";
  }, () => stopped, (id, _index, result) => progress.push([id, result]));

  assert.deepEqual(calls, ["candidate-1"]);
  assert.deepEqual(progress, [["candidate-1", "in-flight result"]]);
  includes(panel, /stopRequested/, "Stop is a cooperative queue flag");
  includes(panel, /Stop/, "the running queue exposes a Stop control");
  includes(app, /disabled=\{researchRunView\.running\}/, "view switching stays available for Stop until the active request finishes");
  includes(app, /onclick=\{disconnect\} disabled=\{busy \|\| researchRunView\.running\}/, "disconnect waits until the queue is stopped");
  includes(app, /class="pid-list-toggle"[^\r\n]*disabled=\{researchRunView\.running\}/, "Hide List cannot hide Stop during a queue");
  includes(app, /onclick=\{stopRecording\} disabled=\{busy \|\| researchRunView\.running\}/, "stopping recording cannot transition to replay during a queue");
  includes(app, /if \(busy \|\| researchRunView\.running\) return;[\s\S]*?if \(recording\) stopRecording\(\)/, "the recording shortcut also respects queue lifecycle");
  includes(app, /<SessionBrowser open=\{sessionBrowserOpen\} disabled=\{busy \|\| recording \|\| researchRunView\.running\} openDisabled=\{connected \|\| recording \|\| researchRunView\.running\}/, "saved-session controls cannot unmount the panel during queue settling");
  includes(app, /class="button secondary open-csv-compact"[^\r\n]*researchRunView\.running/, "opening a session file stays blocked until the queue ends");
  includes(app, /resetDisabled=\{[^}]*researchRunView\.running/, "preference reset cannot hide the active workbench");
  includes(app, /function resetPreferences\(\)[\s\S]*?if \(researchRunView\.running\) return;/, "the reset handler guards the visibility transition too");
  includes(app, /busy:\s*busy \|\| researchRunView\.running/, "Identify and Connect use the queue lock");
  for (const control of ["Adapter", "OBDLink SX serial port", "OpenPort J2534 provider", "Paired Bluetooth device"]) {
    includes(app, new RegExp(`aria-label="${control}"[^\\r\\n]*researchRunView\\.running`), `${control} selection stays fixed during a queue`);
  }
});

test("numeric_result_updates_graph_while_recording", () => {
  const catalogProps = app.match(/<OemCatalogPanel([\s\S]*?)\/>/)?.[1] ?? "";

  includes(catalogProps, /onGraphCandidate=\{graphResearchCandidate\}/, "the existing candidate graph callback remains connected");
  includes(catalogProps, /onGraphReadValue=\{graphA30ReadValue\}/, "the existing A30 graph callback remains connected");
  includes(catalogProps, /disabled=\{[^}]*busy[^}]*connected[^}]*replaying[^}]*session/, "only non-live busy/replay/session states block tests");
  const testDisabledProp = catalogProps.match(/disabled=\{([^}]*)\}/)?.[1] ?? "";
  excludes(testDisabledProp, /recording/, "recording does not block active-session research reads");
  includes(panel, /Number\.isFinite\(typedResult\.payload\.decoded_value\?\.value\)/, "only finite positive candidate values select a graph trace");
  includes(app, /signal_id\]: \[\.\.\.\(series\[sample\.signal_id\] \?\? \[\]\), sample\]/, "graph values arrive through engine sample events");
  excludes(panel, /series\s*=|push\(\{[^}]*signal_id/, "the workbench does not synthesize sample data");
  includes(results, /within_catalog_range\s*===\s*false/, "out-of-range values keep their Suspect research diagnostic");
});

test("workbench graph definitions and selections persist without measurement samples", () => {
  includes(app, /researchSignalOptionsKey\s*=\s*"lognook\.research-signal-options"/, "research signal metadata has its own local preference key");
  includes(app, /workbenchGraphSignalIdsKey\s*=\s*"lognook\.workbench-graph-signal-ids"/, "Workbench graphable IDs are remembered separately from logged PIDs");
  includes(app, /savedResearchSignalOptions\s*=\s*localStorage\.getItem\(researchSignalOptionsKey\)/, "research metadata is restored from local storage");
  includes(app, /savedWorkbenchGraphSignalIds\s*=\s*localStorage\.getItem\(workbenchGraphSignalIdsKey\)/, "graph-only Workbench selections are restored");
  includes(app, /researchSignalOptions\s*=\s*restoreResearchSignalOptions\(savedResearchSignalOptions\)/, "saved research definitions are validated before use");
  includes(app, /restoreWorkspacePreferences\([\s\S]*?workbenchGraphSignalIds\s*,\s*\)/, "saved graph-only IDs are allowed without entering the log selection");
  includes(app, /localStorage\.setItem\(researchSignalOptionsKey,\s*JSON\.stringify\(researchSignalOptions\)\)/, "only signal definitions are persisted");
  includes(app, /localStorage\.setItem\(workbenchGraphSignalIdsKey,\s*JSON\.stringify\(workbenchGraphSignalIds\)\)/, "Workbench graphable IDs are persisted");
  includes(app, /rememberWorkbenchGraphSignalId\(workbenchGraphSignalIds,\s*value\.signal_id\)/, "successful A30 Workbench values are remembered as graphable");
  excludes(app, /captured read result · raw \$\{value\.raw_value\}/, "A30 sample values are not copied into persistent signal metadata");
  includes(app, /&& !workbenchGraphSignalIds\.includes\(signal\.id\)/, "previously graphed Workbench signals stay selectable after restart");
});

test("reset_preferences_clears_remembered_workbench_graph_state", () => {
  const reset = app.match(/function resetPreferences\(\)\s*\{([\s\S]*?)\n  \}/)?.[1] ?? "";
  includes(reset, /researchSignalOptions\s*=\s*\[\]/, "reset clears persisted research signal definitions");
  includes(reset, /workbenchGraphSignalIds\s*=\s*\[\]/, "reset clears graph-only Workbench IDs");
  includes(reset, /localStorage\.removeItem\(researchSignalOptionsKey\)/, "reset removes the research metadata key");
  includes(reset, /localStorage\.removeItem\(workbenchGraphSignalIdsKey\)/, "reset removes the Workbench graph IDs key");
});

test("export_uses_completed_run_report", () => {
  includes(types, /export type ResearchRunReport\s*=\s*\{/, "the UI defines the Rust report contract");
  for (const field of ["run_id", "adapter", "started_at_unix_ms", "completed_at_unix_ms", "items", "queue_index", "queue_size", "selection", "result"]) {
    includes(types, new RegExp(`\\b${field}\\b`), `the report contract includes ${field}`);
  }
  includes(types, /ResearchRunItemStatus\s*=\s*"completed"\s*\|\s*"error"\s*\|\s*"skipped"/, "the UI report contract preserves completed, error, and skipped statuses");
  includes(types, /result:\s*ResearchTestResult\s*\|\s*null/, "error and skipped rows can omit a typed adapter result");
  includes(types, /error:\s*string\s*\|\s*null/, "transport errors remain reportable");
  includes(types, /type:\s*"oemCandidate";\s*payload:\s*OemCandidate/, "OEM selection metadata uses the Rust camelCase enum tag and full candidate");
  includes(types, /type:\s*"a30ReadTest";\s*payload:\s*A30ReadTestId/, "quick-test selection metadata uses the Rust camelCase tag and enum value");
  includes(app, /activeAdapterDescriptor\s*=\s*event\.payload\.device/, "the active descriptor is captured from the acquisition event");
  includes(app, /activeAdapter=\{activeAdapterDescriptor\}/, "the active descriptor is passed to the workbench");
  includes(panel, /const adapterAtStart = activeAdapter/, "the report snapshots adapter identity at run start");
  includes(panel, /const reportItems: ResearchRunItem\[\] = runRows\.map\(/, "every original queue row enters the exported report");
  includes(panel, /status: row\.status === "completed" \? "completed" : row\.status === "skipped" \? "skipped" : "error"/, "all row states become report statuses");
  includes(panel, /result:\s*row\.result,\s*error:\s*row\.error/, "each report item retains its optional result and error");
  includes(panel, /completedRunReport = \{\s*run_id: runId,\s*adapter: adapterSnapshot/, "an initial transport failure can still export the adapter and run metadata");
  includes(app, /invoke\s*<[^>]*>\s*\(\s*"export_research_run_report"/, "report export calls the registered Tauri command");
  includes(panel, /completed_at_unix_ms/, "the exported report records completion time");
  includes(results, /Export run report/, "the result table exposes report export");
  includes(results, /completed/, "the table distinguishes completed results");
});

test("results_panel_can_collapse_and_resize_without_growing_past_its_limit", () => {
  includes(results, /aria-expanded=\{!collapsed\}/, "the collapse control announces its expanded state");
  includes(results, /collapsed \? "Expand results" : "Collapse results"/, "the collapse control remains discoverable in both states");
  includes(results, /type="range"[^>]*min="120"[^>]*max="360"/, "a native keyboard-accessible range resizes the table");
  includes(results, /--research-results-height/, "the selected height controls the results panel");
  includes(results, /max-height:\s*min\([^;]*40vh\)/, "the panel height remains bounded relative to the graph viewport");
});
