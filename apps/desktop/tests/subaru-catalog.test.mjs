import assert from "node:assert/strict";
import test from "node:test";

const catalog = await import("../src/subaruCatalog.ts").catch(() => null);

test("vehicle scope formats normalized Subaru vehicle rows and filters pseudo entries", () => {
  assert.ok(catalog, "Subaru catalog helpers are available");
  const scope = [
    { display_label: "WRX > 20MY", model_year: 2020, is_pseudo: 0 },
    { display_label: "OBD", model_year: null, is_pseudo: 1 },
  ];
  assert.equal(catalog.formatSubaruVehicleScope(JSON.stringify(scope)), "WRX > 20MY");
});

test("vehicle scope handles unmapped and malformed scope as explicit research states", () => {
  assert.ok(catalog, "Subaru catalog helpers are available");
  assert.equal(catalog.formatSubaruVehicleScope("[]"), "No exact vehicle mapping");
  assert.equal(catalog.formatSubaruVehicleScope("not-json"), "not-json");
});

test("vehicle scope preview limits inline labels and counts the remainder", () => {
  assert.ok(catalog, "Subaru catalog helpers are available");
  const scope = [
    { display_label: "WRX > 20MY", is_pseudo: 0 },
    { display_label: "Legacy > 20MY", is_pseudo: 0 },
    { display_label: "Outback > 20MY", is_pseudo: 0 },
    { display_label: "Forester > 20MY", is_pseudo: 0 },
    { display_label: "Impreza > 20MY", is_pseudo: 0 },
    { display_label: "OBD", is_pseudo: 1 },
  ];
  assert.deepEqual(catalog.previewSubaruVehicleScope(JSON.stringify(scope)), {
    text: "WRX > 20MY · Legacy > 20MY · Outback > 20MY",
    remaining: 2,
  });
});

test("vehicle scope preview preserves unresolved and malformed research states", () => {
  assert.ok(catalog, "Subaru catalog helpers are available");
  assert.deepEqual(catalog.previewSubaruVehicleScope("[]"), {
    text: "No exact vehicle mapping",
    remaining: 0,
  });
  assert.deepEqual(catalog.previewSubaruVehicleScope("not-json"), {
    text: "not-json",
    remaining: 0,
  });
});
