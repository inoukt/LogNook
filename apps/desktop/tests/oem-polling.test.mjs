import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const panel = await readFile(new URL("../src/OemCatalogPanel.svelte", import.meta.url), "utf8");
const backendCommands = await readFile(new URL("../src-tauri/src/commands/oem_catalog.rs", import.meta.url), "utf8");
const commandRegistration = await readFile(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");

test("continuous OEM controls use the selected vehicle's verified catalog candidates", () => {
  assert.match(panel, /invoke<OemVehicleProfile\[]>\("oem_vehicle_profiles"\)/);
  assert.match(panel, /invoke<OemCatalogPage>\("search_oem_poll_candidates"/);
  assert.match(panel, /No OEM signal is physically verified and enabled for this vehicle profile yet/);
  assert.match(panel, /vehicleId: pollVehicleId, candidateIds: pollCandidateIds/);
  assert.match(panel, /disabled=\{disabled \|\| !canTryCandidates \|\| !pollCandidateIds\.length\}/);
});

test("continuous OEM controls expose stop and live request/error feedback", () => {
  assert.match(panel, /invoke<OemPollingStatus>\("oem_logging_status"\)/);
  assert.match(panel, /invoke<OemPollingStatus>\("stop_oem_logging"\)/);
  assert.match(panel, /pollingStatus\.failed_requests/);
  assert.match(panel, /pollingStatus\.failed_signals/);
  assert.match(panel, /pollingStatus\.observed_request_rate_hz/);
  assert.match(backendCommands, /pub async fn start_oem_logging/);
  assert.match(backendCommands, /pub async fn stop_oem_logging/);
  assert.match(commandRegistration, /commands::start_oem_logging/);
  assert.match(commandRegistration, /commands::stop_oem_logging/);
});
