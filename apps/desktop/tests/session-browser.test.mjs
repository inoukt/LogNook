import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const browser = await import("../src/sessionBrowser.ts").catch(() => null);

test("recent recordings are sorted newest first without mutating the source list", () => {
  const recordings = [
    { path: "older", name: "older", modified_unix_ms: 100, sample_count: 3 },
    { path: "newer", name: "newer", modified_unix_ms: 300, sample_count: 1 },
    { path: "middle", name: "middle", modified_unix_ms: 200, sample_count: 2 },
  ];

  assert.ok(browser, "session browser helper is available");
  assert.deepEqual(
    browser.recentRecordingsFirst(recordings).map(({ path }) => path),
    ["newer", "middle", "older"],
  );
  assert.deepEqual(recordings.map(({ path }) => path), ["older", "newer", "middle"]);
});

test("recording timestamps fall back to a readable label when the value is invalid", () => {
  assert.ok(browser, "session browser helper is available");
  assert.equal(browser.formatRecordingTime(Number.NaN), "Unknown time");
});

test("session browser preserves optional frozen vehicle metadata and reads old four-column rows", () => {
  const context = {
    scope: "live", vin_digest: "vin:v1:sha256:abc", masked_vin: "1FT•••••••••••2345",
    identity_source: "report_ecu", vin_conflict: false, has_full_vin: true,
  };
  assert.deepEqual(browser.recordingFromRow(["new", "new", 10, 4, context]), {
    path: "new", name: "new", modified_unix_ms: 10, sample_count: 4, vehicle_context: context,
  });
  assert.deepEqual(browser.recordingFromRow(["old", "old", 5, 2]), {
    path: "old", name: "old", modified_unix_ms: 5, sample_count: 2, vehicle_context: null,
  });
});

test("opening a session waits until the player has activated it", async () => {
  assert.equal(typeof browser.openAndActivateRecording, "function");
  if (typeof browser.openAndActivateRecording !== "function") return;

  let releaseActivation;
  const activationGate = new Promise((resolve) => { releaseActivation = resolve; });
  let active = false;
  const pending = browser.openAndActivateRecording(
    async () => ({ id: "session" }),
    async () => { await activationGate; active = true; },
  );

  await Promise.resolve();
  assert.equal(active, false);
  releaseActivation();
  await pending;
  assert.equal(active, true);
});

test("session browser exposes inline rename, confirmed delete, and folder actions", async () => {
  const source = await readFile(new URL("../src/SessionBrowser.svelte", import.meta.url), "utf8");
  assert.match(source, /invoke\("rename_recording"/);
  assert.match(source, /invoke\("delete_recording"/);
  assert.match(source, /invoke\("open_recording_folder"/);
  assert.match(source, /Confirm delete/);
  assert.match(source, /New filename for/);
  assert.match(source, /use:focusRenameInput/);
  assert.match(source, /Open folder for/);
});

test("legacy four-column recordings remain readable without inventing vehicle context", () => {
  assert.deepEqual(browser.recordingFromRow(["old.lognook", "old", 1, 8]), {
    path: "old.lognook", name: "old", modified_unix_ms: 1, sample_count: 8, vehicle_context: null,
  });
  const summary = { scope: "live", vin_digest: "digest", masked_vin: "1FT••••2345", identity_source: "report_ecu", vin_conflict: false, has_full_vin: true };
  assert.deepEqual(browser.recordingFromRow(["new.lognook", "new", 2, 9, summary]).vehicle_context, summary);
});

test("history groups only matching scoped VINs and leaves conflict/unassigned items distinct", () => {
  const reports = [
    { report_id: "r1", scope: "live", vin_digest: "same", masked_vin: "MASK1", identity_source: "report_ecu", vin_conflict: false },
    { report_id: "r2", scope: "simulator", vin_digest: "same", masked_vin: "MASK1", identity_source: "report_ecu", vin_conflict: false },
    { report_id: "r3", scope: "live", vin_digest: "same", masked_vin: null, identity_source: "conflict", vin_conflict: true },
    { report_id: "r4", scope: "live", vin_digest: null, masked_vin: null, identity_source: "unassigned", vin_conflict: false },
  ];
  const recordings = [
    { path: "car.lognook", name: "car", modified_unix_ms: 1, sample_count: 1, vehicle_context: { scope: "live", vin_digest: "same", masked_vin: "MASK1", identity_source: "report_ecu", vin_conflict: false, has_full_vin: true } },
    { path: "old.lognook", name: "old", modified_unix_ms: 1, sample_count: 1, vehicle_context: null },
  ];
  const groups = browser.groupVehicleHistory(reports, recordings);
  const live = groups.find((group) => group.id === "vehicle:live:same");
  assert.deepEqual(live.reportRows.map(({ report_id }) => report_id), ["r1"]);
  assert.deepEqual(live.recordings.map(({ path }) => path), ["car.lognook"]);
  assert.ok(groups.some((group) => group.id === "vehicle:simulator:same"));
  assert.ok(groups.some((group) => group.id === "conflict:live:r3"));
  assert.ok(groups.some((group) => group.id === "unassigned:live:r4"));
  assert.ok(groups.some((group) => group.id === "unassigned:live:recording:old.lognook"));
});

test("native session privacy labels identify full VIN metadata without adding CSV columns", () => {
  assert.equal(browser.recordingPrivacyLabel(true), "Native session metadata contains a full VIN.");
  assert.equal(browser.recordingPrivacyLabel(false), "Native session metadata has no full VIN.");
  assert.match(browser.recordingPrivacyLabel(null), /older session/);
  assert.equal(browser.csvVehiclePrivacyLabel(), "CSV export does not add VIN columns.");
});

test("recording context boundaries disclose both elapsed time and the stored sample ordinal", () => {
  assert.equal(browser.recordingContextBoundaryLabel({ context_from_elapsed_ns: 1234567890, context_from_sample_index: 4 }),
    "Vehicle context starts at row index 4 (sample 5) and 1.234567890 s.");
});
