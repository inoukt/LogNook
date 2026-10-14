import assert from "node:assert/strict";
import test from "node:test";

const diagnostics = await import("../src/vehicleDiagnostics.ts");

test("vehicle diagnostics are enabled only for the implemented CAN adapters", () => {
  assert.equal(diagnostics.supportsVehicleDiagnostics("Xtool A30/A30M Bluetooth SPP"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("ELM327 Bluetooth SPP"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OBDLink SX"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OpenPort 2 J2534"), false);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OpenPort 2.0"), false);
  assert.equal(diagnostics.supportsVehicleDiagnostics(null), false);
  assert.equal(diagnostics.supportsProfileIdentity("OpenPort 2.0"), true);
  assert.equal(diagnostics.supportsProfileIdentity("OpenPort 2 J2534"), true);
  assert.equal(diagnostics.supportsProfileIdentity(null), false);
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

test("missing DTC summaries stay unknown while a positive zero remains zero", () => {
  assert.equal(diagnostics.historyDtcCountLabel(null), "Unknown");
  assert.equal(diagnostics.historyDtcCountLabel(undefined), "Unknown");
  assert.equal(diagnostics.historyDtcCountLabel(0), "0");
  assert.equal(diagnostics.historyDtcCountLabel(3), "3");
});

test("history pagination carries the supplied cursor and merges rows without duplicates", () => {
  const filter = { scope: "live", vin_digest: "digest", ecu_id: "7E8", from_unix_ms: 1, to_unix_ms: 9, page_size: 50, cursor: null };
  assert.deepEqual(diagnostics.historyPageFilter(filter, "next-page"), { ...filter, cursor: "next-page" });
  assert.deepEqual(diagnostics.mergeHistoryRows(
    [{ report_id: "a" }, { report_id: "b" }],
    [{ report_id: "b" }, { report_id: "c" }],
  ).map(({ report_id }) => report_id), ["a", "b", "c"]);
});
