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
  assert.equal(controls.oemCandidateTryState(valid, "COM4", true).reason, "Connect a supported adapter and finish replay or other current actions before testing");
  assert.equal(controls.oemCandidateTryState({ ...valid, source_blob_sha: "" }, "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState({ ...valid, request_header: "7EX" }, "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState({ ...valid, request_header: "800" }, "COM4", false).disabled, true);
  assert.equal(controls.oemCandidateTryState({ ...valid, response_header: "800" }, "COM4", false).disabled, true);
});

test("source-backed candidates can be tried over A30 or ELM Bluetooth", () => {
  assert.ok(controls, "OEM candidate controls helper is available");
  const valid = candidate("21", "01");
  assert.deepEqual(controls.oemCandidateTryState(valid, "AA:BB:CC:DD:EE:FF", false, "a30"), {
    disabled: false,
    reason: "",
  });
  assert.equal(controls.oemCandidateTryState(valid, "", false, "a30").reason, "Select a paired Bluetooth device");
  assert.equal(controls.oemCandidateTryState(valid, "AA:BB:CC:DD:EE:FF", false, "elm327").disabled, false);
});

test("A30 requires its supported 0x7xx request route while SX and ELM allow other source routes", () => {
  const valid = { ...candidate("22", "1234"), request_header: "6F1", response_header: "607" };
  assert.equal(controls.oemCandidateTryState(valid, "COM4", false, "obdlink_sx").disabled, false);
  assert.equal(controls.oemCandidateTryState(valid, "AA:BB:CC:DD:EE:FF", false, "elm327").disabled, false);
  assert.deepEqual(controls.oemCandidateTryState(valid, "AA:BB:CC:DD:EE:FF", false, "a30"), {
    disabled: true,
    reason: "A30 OEM reads require a 0x7xx request CAN ID",
  });

  const a30Route = { ...valid, request_header: "7E0", response_header: "7E8" };
  assert.equal(controls.oemCandidateTryState(a30Route, "AA:BB:CC:DD:EE:FF", false, "a30").disabled, false);
});

test("OpenPort, Candlelight OBD, and the labeled simulator allow source-backed one-shot OEM reads", () => {
  assert.ok(controls, "OEM candidate controls helper is available");
  const valid = candidate("22", "1234");
  for (const adapter of ["openport2", "candlelight_obd", "simulator"]) {
    assert.deepEqual(controls.oemCandidateTryState(valid, `${adapter}:connected`, false, adapter), {
      disabled: false,
      reason: "",
    });
  }
});
