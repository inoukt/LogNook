import assert from "node:assert/strict";
import test from "node:test";

const profiles = await import("../src/vehicleProfiles.ts");

test("Resume requires a current standby preview with at least one eligible logging signal", () => {
  const preview = {
    preview_id: "resume-1",
    profile_id: "flex",
    profile_revision: 2,
    obd_connection_generation: 7,
    profile_match: { state: "review_required", fingerprint_digest: null, matched_ecu_variants: [], review_reasons: [] },
    eligible_log_refs: ["engine_rpm"],
    eligible_graph_refs: ["engine_rpm"],
    skipped: [],
    can_apply: true,
    manual_association_available: false,
  };
  assert.equal(profiles.canApplyProfileResume(preview, true, true, 7), true);
  assert.equal(profiles.canApplyProfileResume(preview, true, false, 7), false);
  assert.equal(profiles.canApplyProfileResume(preview, true, true, 8), false);
  assert.equal(profiles.canApplyProfileResume({ ...preview, eligible_log_refs: [] }, true, true, 7), false);
  assert.equal(profiles.canApplyProfileResume(null, true, true, 7), false);
});

test("Save arguments trim names, preserve the selection snapshot, and require an explicit revision on update", () => {
  const selection = {
    log_signal_ids: ["engine_rpm"],
    graph_signal_ids: ["engine_rpm", "oem:research"],
    derived_definitions: [], can_fields: [], can_source_id: null, can_bitrate: null, oem_references: [],
  };
  const args = profiles.createSaveVehicleProfileArgs("  Flex EcoBoost  ", selection, true);
  assert.deepEqual(args, {
    profile_id: null, expected_revision: null, name: "Flex EcoBoost", selection, manual_association: true,
    catalog_vehicle_id: null,
  });
  assert.throws(() => profiles.createSaveVehicleProfileArgs(" ", selection, false), /Enter a name/);
  assert.throws(() => profiles.createSaveVehicleProfileArgs("Flex", selection, false, "profile-1"), /current revision/);
});

test("identity detail prefers the full VIN, falls back to the mask, and labels hash-only records", () => {
  assert.deepEqual(profiles.vehicleIdentityVinPresentation({ full_vin: "1FTEXAMPLEVIN12345", masked_vin: "1FT•••••••••••2345", vin_digest: "abc" }), {
    value: "1FTEXAMPLEVIN12345", full: true, label: "Full VIN",
  });
  assert.deepEqual(profiles.vehicleIdentityVinPresentation({ full_vin: null, masked_vin: "1FT•••••••••••2345", vin_digest: "abc" }), {
    value: "1FT•••••••••••2345", full: false, label: "Masked VIN · full VIN unavailable",
  });
  assert.deepEqual(profiles.vehicleIdentityVinPresentation({ full_vin: null, masked_vin: null, vin_digest: "abc" }), {
    value: "VIN unavailable", full: false, label: "Legacy VIN digest only",
  });
});

test("copying a full VIN reports clipboard failures without losing the visible VIN", async () => {
  let copied = "";
  assert.equal(await profiles.copyFullVin("1FTEXAMPLEVIN12345", async (value) => { copied = value; }), true);
  assert.equal(copied, "1FTEXAMPLEVIN12345");
  assert.equal(await profiles.copyFullVin("1FTEXAMPLEVIN12345", async () => { throw new Error("denied"); }), false);
  assert.equal(await profiles.copyFullVin("", async () => {}), false);
});

test("offline decode labels unknown WMI, checksum scope, and unresolved year without guessing", () => {
  assert.deepEqual(profiles.vinDecodeLabels({
    check_digit_status: "unknown_scope", check_digit_scheme: null, year_code: "A",
    candidate_model_years: [1980, 2010], class_backed_model_year: null, wmi_assignment: null,
    decoder_revision: "vin-decode-offline-v1", wmi_dataset_sha256: "abc",
  }), {
    checkDigit: "Check-digit scope unknown", year: "Year unresolved · 1980 or 2010", manufacturer: "Unknown WMI",
  });
  assert.deepEqual(profiles.vinDecodeLabels({
    check_digit_status: "mismatch_warning", check_digit_scheme: "49 CFR §565.15", year_code: "B",
    candidate_model_years: [1981, 2011], class_backed_model_year: 2011,
    wmi_assignment: { manufacturer: "Toyota", source_id: "NHTSA", source_revision: "4.06" },
    decoder_revision: "vin-decode-offline-v1", wmi_dataset_sha256: "abc",
  }), {
    checkDigit: "Check-digit mismatch warning · 49 CFR §565.15", year: "2011 · source-backed class rule", manufacturer: "Toyota",
  });
});
