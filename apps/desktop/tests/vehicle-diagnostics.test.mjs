import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";

const diagnostics = await import("../src/vehicleDiagnostics.ts");
const diagnosticsPanelSource = await readFile(new URL("../src/VehicleDiagnosticsPanel.svelte", import.meta.url), "utf8");

test("vehicle diagnostics capability helper recognizes every supported adapter family", () => {
  assert.equal(diagnostics.supportsVehicleDiagnostics("Xtool A30/A30M Bluetooth SPP"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("ELM327 Bluetooth SPP"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OBDLink SX"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OpenPort 2 J2534"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("OpenPort 2.0"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("Candlelight GS-USB OBD-II"), true);
  assert.equal(diagnostics.supportsVehicleDiagnostics("simulator"), true);
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

test("DTC clear confirmation is valid only for the same active adapter generation", () => {
  const matches = diagnostics.clearConfirmationMatchesConnection;
  assert.equal(matches("sx-1", 7, "sx-1", 7), true);
  assert.equal(matches("sx-1", 8, "sx-1", 7), false);
  assert.equal(matches("sx-2", 7, "sx-1", 7), false);
  assert.equal(matches(null, 7, "sx-1", 7), false);
  assert.equal(matches("sx-1", null, "sx-1", 7), false);
});

test("DTC clear is a separate confirmed UI action with cancel and stale-connection guards", () => {
  const start = diagnosticsPanelSource.indexOf("function beginClearConfirmation()");
  const confirm = diagnosticsPanelSource.indexOf("async function confirmClear()");
  assert.ok(start >= 0 && confirm > start);
  assert.match(diagnosticsPanelSource.slice(start, confirm), /clearConfirm\?\.showModal\(\)/);
  assert.match(diagnosticsPanelSource.slice(confirm), /clearConfirmationStale/);
  assert.match(diagnosticsPanelSource.slice(confirm), /invoke<DiagnosticReport>\("clear_vehicle_dtcs"/);
  assert.match(diagnosticsPanelSource, /onclick=\{\(\) => clearConfirm\?\.close\(\)\}>Cancel/);
  assert.match(diagnosticsPanelSource, /affect readiness state/);
  assert.match(diagnosticsPanelSource, /acknowledged the Mode 04 clear request/);
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
