import assert from "node:assert/strict";
import test from "node:test";

const presets = await import("../src/signalPresets.ts");

test("generic presets browse without vehicle details and expose required vehicle details", () => {
  const generic = { scope: "generic", criteria: { make: null, model: null }, name: "Generic" };
  const model = { scope: "model_powertrain", criteria: { make: "Ford", model: "Flex" }, name: "Flex" };
  assert.equal(generic.scope, "generic");
  assert.equal(model.criteria.model, "Flex");
});

test("preview results include skipped and task-defining availability", () => {
  const preview = { eligible: [{ concept: "RPM", task_defining: true }], skipped: [{ concept: "CVT temp", task_defining: true, reason: "unsupported" }], task_defining_eligible: 1, task_defining_total: 2 };
  assert.equal(preview.skipped[0].reason, "unsupported");
  assert.deepEqual([preview.task_defining_eligible, preview.task_defining_total], [1, 2]);
});

test("stale responses cannot replace a newer preview", async () => {
  const state = { token: 0 };
  let release;
  const old = presets.latestPreview(() => new Promise((resolve) => { release = resolve; }), state);
  const current = presets.latestPreview(async () => "new", state);
  release("old");
  assert.equal(await old, null);
  assert.equal(await current, "new");
});

test("a preview is current only while its connection generation is unchanged", () => {
  const preview = { connection_generation: 7 };
  assert.equal(presets.presetPreviewMatchesConnection(preview, true, 7), true);
  assert.equal(presets.presetPreviewMatchesConnection(preview, false, 7), false);
  assert.equal(presets.presetPreviewMatchesConnection(preview, true, 8), false);
});

test("offline preset preview shows staged concepts without claiming signal support", () => {
  const recipe = {
    id: "engine-tuning-core", revision: 1, name: "Engine tuning", family: "engine_tuning", size: "core",
    origin: "builtin", scope: "generic", criteria: {}, selection: null, required_adapter: null, required_protocol: null,
    channels: [
      { concept: "RPM", signal_id: "engine_rpm", task_defining: true, priority: 0, slow_context: false, oem_alternatives: [] },
      { concept: "Knock correction", signal_id: null, task_defining: false, priority: 1, slow_context: false, oem_alternatives: [] },
    ],
  };
  const preview = presets.makeOfflinePresetPreview(recipe, "replace");
  assert.equal(preview.local_only, true);
  assert.equal(preview.can_apply, false);
  assert.equal(preview.connection_generation, null);
  assert.deepEqual(preview.eligible, []);
  assert.equal(preview.skipped.length, 2);
  assert.equal(preview.task_defining_total, 1);
  assert.equal(presets.presetPreviewMatchesConnection(preview, false, 0), true);
  assert.equal(presets.presetPreviewMatchesConnection(preview, true, 0), false);
});

test("staging a second offline preset carries forward the returned picker revision", async () => {
  let serverRevision = 4;
  const context = {
    make: null, model: null, year: null, engine: null, transmission: null,
    profile_id: null, revision: serverRevision, connection_generation: null,
  };
  const stageRequest = async (operation) => {
    assert.equal(operation.op, "stage");
    assert.equal(operation.expected_revision, serverRevision);
    serverRevision += 1;
    return {
      type: "picker_state",
      state: { revision: serverRevision, context: { ...context, revision: serverRevision } },
    };
  };

  const firstContext = await presets.stagePreset("first", 1, "replace", context, stageRequest);
  const secondContext = await presets.stagePreset("second", 1, "replace", firstContext, stageRequest);

  assert.equal(secondContext.revision, 6);
});

test("Save preserves separate log and graph selections", () => {
  const selection = { log_signal_ids: ["rpm", "map"], graph_signal_ids: ["rpm"], derived_definitions: [], can_fields: [], can_source_id: null, can_bitrate: null, can_channel: null, oem_references: [] };
  const saved = presets.presetFromSelection("My recipe", "generic", selection, [{ id: "rpm", name: "Engine RPM" }, { id: "map", name: "Manifold pressure" }]);
  assert.deepEqual(saved.selection.log_signal_ids, ["rpm", "map"]);
  assert.deepEqual(saved.selection.graph_signal_ids, ["rpm"]);
  assert.deepEqual(saved.channels.map((channel) => channel.concept), ["Engine RPM", "Manifold pressure"]);
});

test("Save keeps exact OEM references as research alternatives and never invents them", () => {
  const reference = { candidate_id: "abc", source_id: "fixture", source_revision: "sha", decoder_revision: "decoder-v1" };
  const selection = {
    log_signal_ids: ["engine_rpm", "oem:abc", "oem:missing"], graph_signal_ids: ["engine_rpm"],
    derived_definitions: [], can_fields: [], can_source_id: null, can_bitrate: null, can_channel: null,
    oem_references: [reference],
  };
  const saved = presets.presetFromSelection("Research", "generic", selection, []);
  assert.deepEqual(saved.channels[1].oem_alternatives, [reference]);
  assert.equal(saved.channels[1].signal_id, null);
  assert.deepEqual(saved.channels[2].oem_alternatives, []);
  assert.equal(saved.channels[2].signal_id, null);
  assert.deepEqual(saved.selection.oem_references, [reference]);
});

test("Generic presets do not retain the current vehicle filter", () => {
  const criteria = presets.presetCriteriaForScope("generic", {
    make: "Toyota", model: "xB", year: 2014, engine: "2.4L", transmission: "automatic",
    profile_id: "recognized-xb", revision: 4, connection_generation: 11,
  });
  assert.deepEqual(criteria, { make: null, model: null, year: null, engine: null, transmission: null, profile_id: null });
});

test("Editing custom preset metadata preserves its channel and selection recipe", () => {
  const original = presets.presetFromSelection("Flex tuning", "generic", {
    log_signal_ids: ["engine_rpm"], graph_signal_ids: [], derived_definitions: [], can_fields: [],
    can_source_id: null, can_bitrate: null, can_channel: null, oem_references: [],
  }, []);
  const edited = presets.updateCustomPresetMetadata(original, "  Flex tune  ", "cvt_diagnostics", "model_powertrain", {
    make: "Ford", model: "Flex", year: 2011, engine: "3.5L", transmission: "automatic",
    profile_id: null, revision: 3, connection_generation: null,
  });
  assert.equal(edited.name, "Flex tune");
  assert.equal(edited.scope, "model_powertrain");
  assert.deepEqual(edited.channels, original.channels);
  assert.deepEqual(edited.selection, original.selection);
  assert.deepEqual(original.criteria, { make: null, model: null, year: null, engine: null, transmission: null, profile_id: null });
});

test("Metadata edits keep exact scope and require context before changing applicability", () => {
  const original = presets.presetFromSelection("Vehicle recipe", "exact_profile", {
    log_signal_ids: ["engine_rpm"], graph_signal_ids: ["engine_rpm"], derived_definitions: [], can_fields: [],
    can_source_id: null, can_bitrate: null, can_channel: null, oem_references: [],
  }, []);
  original.scope = "exact_profile";
  original.criteria = { make: "Toyota", model: "xB", year: 2014, engine: "2.4L", transmission: null, profile_id: "scion-xb" };
  const emptyContext = { make: null, model: null, year: null, engine: null, transmission: null, profile_id: null, revision: 0, connection_generation: null };
  const edited = presets.updateCustomPresetMetadata(original, "Renamed", "engine_tuning", "exact_profile", emptyContext);
  assert.deepEqual(edited.criteria, original.criteria);
  assert.equal(presets.presetScopeContextError("model_powertrain", emptyContext), "Enter a make and model before using Model / powertrain scope.");
  assert.equal(presets.presetScopeContextError("exact_profile", emptyContext), "Choose a saved vehicle profile before using Exact saved profile scope.");
  assert.equal(presets.presetScopeContextError("model_powertrain", { ...emptyContext, make: "Toyota", model: "xB" }), null);
});
