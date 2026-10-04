import assert from "node:assert/strict";
import test from "node:test";

const controls = await import("../src/oemCandidateControls.ts").catch(() => null);

const candidate = (service, identifier) => ({
  service,
  identifier,
  request_header: "7E0",
  response_header: "7E8",
  source_blob_sha: "reviewed-sha",
  source_url: "https://source.test/vehicle",
  license: "CC BY-SA 4.0",
  license_url: "https://source.test/license",
});

test("one-shot controls allow only explicit supported SX request shapes", () => {
  assert.ok(controls, "OEM candidate controls helper is available");
  assert.deepEqual(controls.oemCandidateTryState(candidate("21", "01"), "COM4", false), {
    disabled: false,
    reason: "",
  });
  assert.equal(controls.oemCandidateTryState(candidate("22", "1234"), "COM4", false).disabled, false);
  assert.equal(controls.oemCandidateTryState(candidate("22", "123"), "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState(candidate("10", "01"), "COM4", false).disabled, true);
});

test("one-shot controls require a selected SX port, an idle workspace, and provenance", () => {
  assert.ok(controls, "OEM candidate controls helper is available");
  const valid = candidate("21", "01");
  assert.equal(controls.oemCandidateTryState(valid, "", false).reason, "Select an OBDLink SX port");
  assert.equal(controls.oemCandidateTryState(valid, "COM4", true).reason, "Stop acquisition, recording, or replay first");
  assert.equal(controls.oemCandidateTryState({ ...valid, source_blob_sha: "" }, "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState({ ...valid, request_header: "7EX" }, "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState({ ...valid, request_header: "800" }, "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState({ ...valid, response_header: "800" }, "COM4", false).disabled, true);
});
