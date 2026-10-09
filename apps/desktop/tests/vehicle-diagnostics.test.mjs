import assert from "node:assert/strict";
import test from "node:test";

const diagnostics = await import("../src/vehicleDiagnostics.ts");

test("vehicle diagnostics are enabled only for the implemented CAN adapters", () => {
  assert.equal(diagnostics.supportsVehicleDiagnostics("Xtool A30/A30M Bluetooth SPP"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("ELM327 Bluetooth SPP"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OBDLink SX"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OpenPort 2 J2534"), false);
  assert.equal(diagnostics.supportsVehicleDiagnostics(null), false);
});

test("Mode 06 selections parse hex IDs, deduplicate, and reject bitmap pages", () => {
  assert.deepEqual(diagnostics.parseMode06TestIds("35, 0xA2; 35"), {
    ids: [0x35, 0xA2], error: null,
  });
  assert.match(diagnostics.parseMode06TestIds("20").error, /bitmap page/);
  assert.match(diagnostics.parseMode06TestIds("xyz").error, /Invalid/);
  assert.match(diagnostics.parseMode06TestIds(Array(33).fill("35").join(",")).error, /32/);
});

test("monitor summaries keep UASID and raw limits without assigning a pass result", () => {
  const summary = diagnostics.mode06MonitorSummary({
    test_id: 0x35, component_id: 0x0A, uasid: 0x01, value: 100, minimum: 16, maximum: 200,
  });
  assert.equal(summary, "TID 35 · MID 0A · UASID 01 · raw count 100 (min 16, max 200)");
  assert.doesNotMatch(summary, /pass|fail/i);
});

test("diagnostic results remain grouped by responding ECU", () => {
  const reads = [
    { ecu_id: "7E8", kind: "stored_dtcs" },
    { ecu_id: "7E9", kind: "stored_dtcs" },
    { ecu_id: null, kind: "vin" },
  ];
  assert.deepEqual(diagnostics.groupDiagnosticReadsByEcu(reads).map(([ecu]) => ecu), ["7E8", "7E9", "No ECU response"]);
});
