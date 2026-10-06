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
});

test("numeric_result_updates_graph_while_recording", () => {
  const catalogProps = app.match(/<OemCatalogPanel([\s\S]*?)\/>/)?.[1] ?? "";

  includes(catalogProps, /onGraphCandidate=\{graphResearchCandidate\}/, "the existing candidate graph callback remains connected");
  includes(catalogProps, /onGraphReadValue=\{graphA30ReadValue\}/, "the existing A30 graph callback remains connected");
  includes(catalogProps, /disabled=\{[^}]*busy[^}]*connected[^}]*replaying[^}]*session/, "only non-live busy/replay/session states block tests");
  const testDisabledProp = catalogProps.match(/disabled=\{([^}]*)\}/)?.[1] ?? "";
  excludes(testDisabledProp, /recording/, "recording does not block active-session research reads");
  includes(panel, /Number\.isFinite\(result\.payload\.decoded_value\?\.value\)/, "only finite positive candidate values select a graph trace");
  includes(app, /signal_id\]: \[\.\.\.\(series\[sample\.signal_id\] \?\? \[\]\), sample\]/, "graph values arrive through engine sample events");
  excludes(panel, /series\s*=|push\(\{[^}]*signal_id/, "the workbench does not synthesize sample data");
  includes(results, /within_catalog_range\s*===\s*false/, "out-of-range values keep their Suspect research diagnostic");
});

test("export_uses_completed_run_report", () => {
  includes(types, /export type ResearchRunReport\s*=\s*\{/, "the UI defines the Rust report contract");
  for (const field of ["run_id", "adapter", "started_at_unix_ms", "completed_at_unix_ms", "items", "queue_index", "queue_size", "selection", "result"]) {
    includes(types, new RegExp(`\\b${field}\\b`), `the report contract includes ${field}`);
  }
  includes(types, /type:\s*"oemCandidate";\s*payload:\s*OemCandidate/, "OEM selection metadata uses the Rust camelCase enum tag and full candidate");
  includes(types, /type:\s*"a30ReadTest";\s*payload:\s*A30ReadTestId/, "quick-test selection metadata uses the Rust camelCase tag and enum value");
  includes(app, /invoke\s*<[^>]*>\s*\(\s*"export_research_run_report"/, "report export calls the registered Tauri command");
  includes(panel, /completed_at_unix_ms/, "the exported report records completion time");
  includes(results, /Export run report/, "the result table exposes report export");
  includes(results, /completed/, "the table distinguishes completed results");
});
