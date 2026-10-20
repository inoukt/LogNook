import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const discovery = await import("../src/oemDiscovery.ts").catch(() => null);
const panel = await readFile(new URL("../src/OemCatalogPanel.svelte", import.meta.url), "utf8");

test("discovery value sorting puts finite values first and keeps zero", () => {
  assert.ok(discovery, "OEM discovery helpers are available");
  const results = [
    { candidate_id: "reply", state: "reply", value: null, unit: "" },
    { candidate_id: "two", state: "value", value: 2, unit: "kPa" },
    { candidate_id: "zero", state: "value", value: 0, unit: "kPa" },
    { candidate_id: "bad", state: "value", value: Number.NaN, unit: "kPa" },
  ];
  assert.deepEqual(discovery.sortOemDiscoveryResults(results).map(({ candidate_id }) => candidate_id), [
    "two", "zero", "reply", "bad",
  ]);
});

test("explicit OEM read presentation preserves zero and distinguishes raw replies and failed reads", () => {
  const result = { candidate_id: "signal", outcome: "positive", decoded_value: { value: 0, unit: "kPa", enum_value: null }, error: null, decode_error: null, evidence_saved: true };
  const value = discovery.oemReadDiscoveryResult(result, "source-revision");
  assert.equal(value.state, "value");
  assert.equal(value.value, 0);
  assert.equal(value.source_blob_sha, "source-revision");
  assert.equal(value.evidence_saved, true);
  assert.equal(discovery.oemReadDiscoveryResult({ ...result, decoded_value: null }, "sha").state, "reply");
  assert.equal(discovery.oemReadDiscoveryResult({ ...result, decoded_value: { ...result.decoded_value, value: Number.NaN } }, "sha").value, null);
  for (const outcome of ["negative_response", "response_pending", "session_required", "security_required", "temporarily_unavailable"]) {
    const response = discovery.oemReadDiscoveryResult({ ...result, outcome }, "sha");
    assert.equal(response.state, "negative");
    assert.equal(response.outcome, outcome);
    assert.equal(response.value, null);
  }
  assert.equal(discovery.oemReadDiscoveryResult({ ...result, outcome: "no_response" }, "sha").state, "no_response");
  const malformed = discovery.oemReadDiscoveryResult({ ...result, outcome: "malformed_response", decode_error: "short payload" }, "sha");
  assert.equal(malformed.state, "error");
  assert.equal(malformed.detail, "short payload");
});

test("discovery responses are current only for the same adapter connection generation", () => {
  assert.ok(discovery, "OEM discovery helpers are available");
  const status = { adapter_id: "simulator:1", connection_generation: 7 };
  assert.equal(discovery.isCurrentOemDiscoveryConnection(status, "simulator:1", 7), true);
  assert.equal(discovery.isCurrentOemDiscoveryConnection(status, "simulator:1", 8), false);
  assert.equal(discovery.isCurrentOemDiscoveryConnection(status, "openport:1", 7), false);
  assert.equal(discovery.isCurrentOemDiscoveryConnection(status, "simulator:1", null), false);
});

test("discovery candidate IDs deduplicate without changing visible order", () => {
  assert.ok(discovery, "OEM discovery helpers are available");
  assert.deepEqual(discovery.uniqueCandidateIds(["a", "b", "a", "", "b", "c"]), ["a", "b", "c"]);
  assert.deepEqual(discovery.nextVisibleOemCandidateIds(["a", "b", "a", "c"], new Set(["b"])), ["a", "c"]);
});

test("viewport tracking only queues intersecting candidates once", () => {
  assert.ok(discovery, "OEM discovery helpers are available");
  const visible = discovery.updateVisibleOemCandidateIds([], "in_view", true);
  assert.deepEqual(visible, ["in_view"]);
  assert.deepEqual(discovery.updateVisibleOemCandidateIds(visible, "in_view", true), ["in_view"]);
  assert.deepEqual(discovery.updateVisibleOemCandidateIds(visible, "off_view", false), ["in_view"]);
  assert.deepEqual(discovery.updateVisibleOemCandidateIds(visible, "in_view", false), []);
});

test("discovery filter keeps requests within the current visible search scope", () => {
  assert.ok(discovery, "OEM discovery helpers are available");
  assert.deepEqual(discovery.oemDiscoveryFilter(" boost ", " Ford ", "Flex", "22"), {
    query: "boost",
    brand: "Ford",
    model: "Flex",
    service: "22",
  });
  assert.deepEqual(discovery.oemDiscoveryFilter("", "", "", ""), {
    query: "",
    brand: "",
    model: "",
    service: null,
  });
});

test("discovery badges distinguish values, bare replies, and unsuccessful requests", () => {
  assert.ok(discovery, "OEM discovery helpers are available");
  assert.deepEqual(discovery.oemDiscoveryPresentation(null), {
    label: "Untested", tone: "untested",
  });
  assert.deepEqual(discovery.oemDiscoveryPresentation({ state: "value", value: 0, unit: "kPa" }), {
    label: "0 kPa", tone: "value",
  });
  assert.deepEqual(discovery.oemDiscoveryPresentation({ state: "reply", value: null, unit: "" }), {
    label: "Reply · no numeric decode", tone: "reply",
  });
  assert.deepEqual(discovery.oemDiscoveryPresentation({ state: "negative", value: null, unit: "" }), {
    label: "Negative response", tone: "failure",
  });
  assert.deepEqual(discovery.oemDiscoveryPresentation({ state: "no_response", value: null, unit: "" }), {
    label: "No response", tone: "failure",
  });
  assert.deepEqual(discovery.oemDiscoveryPresentation({ state: "running", value: null, unit: "" }), {
    label: "Running", tone: "active",
  });
});

test("the panel observes the scroll viewport and keeps discovery outside the graph and polling paths", () => {
  assert.match(panel, /new IntersectionObserver\([\s\S]*?\{ root, threshold: 0 \}/);
  assert.match(panel, /use:trackCandidateVisibility=\{candidate\.candidate_id\}/);
  assert.match(panel, /const candidateIds = uniqueCandidateIds[\s\S]*?\.slice\(0, pageSize\)/);
  assert.match(panel, /setTimeout\(\(\) => void refreshDiscoveryStatus\(adapterId, generation\), 400\)/);
  assert.match(panel, /kind: "try_filtered"/);
  assert.match(panel, /values_first: valuesFirst/);
  assert.match(panel, /adapterId !== currentDiscoveryAdapterId \|\| generation !== currentDiscoveryGeneration/);
  assert.match(panel, /destroyed = true;[\s\S]*?discoveryEpoch \+= 1/);
  assert.match(panel, /Simulator · no TX/);
  assert.match(panel, /title="Simulator · no vehicle transmission"/);
  assert.match(panel, /selectedAdapter === "obdlink_sx" \|\| selectedAdapter === "a30" \|\| selectedAdapter === "elm327"/);
  const apply = panel.match(/function applyDiscoveryResponse\([\s\S]*?\n  \}/)?.[0] ?? "";
  assert.doesNotMatch(apply, /onGraphCandidate|onGraphReadValue|startOemLogging/);
});
