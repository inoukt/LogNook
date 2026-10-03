import test from "node:test";
import assert from "node:assert/strict";
import {
  derivedSignalOptions,
  formulaSignalIds,
  graphScaleSignalIds,
  isAvailableDerivedSignalId,
  newDerivedSignalDefinition,
  restoreDerivedSignalDefinitions,
} from "../src/derivedSignals.ts";

test("derived definitions restore with stable ids, formulas, units, and tare offsets", () => {
  const definitions = restoreDerivedSignalDefinitions(JSON.stringify([
    { id: "temp_f", name: "Coolant °F", expression: "coolant_temp * 1.8 + 32", unit: "°F", tare_offset: 10 },
    { id: "engine_rpm", name: "Collision", expression: "engine_rpm", unit: "rpm", tare_offset: 0 },
    { id: "temp_f", name: "Duplicate", expression: "coolant_temp", unit: "°C", tare_offset: 0 },
    { id: "bad-id", name: "Invalid", expression: "coolant_temp", unit: "°C", tare_offset: 0 },
  ]), ["engine_rpm", "coolant_temp"]);

  assert.deepEqual(definitions, [
    { id: "temp_f", name: "Coolant °F", expression: "coolant_temp * 1.8 + 32", unit: "°F", tare_offset: 10 },
  ]);
});

test("derived definitions appear as signal options with their formula in the description", () => {
  const options = derivedSignalOptions([
    { id: "temp_f", name: "Coolant °F", expression: "coolant_temp * 1.8 + 32", unit: "°F", tare_offset: 0 },
  ]);
  assert.deepEqual(options, [{
    id: "temp_f",
    name: "Coolant °F",
    unit: "°F",
    description: "Derived from coolant_temp * 1.8 + 32",
    formula: "coolant_temp * 1.8 + 32",
  }]);
});

test("derived signal ids must be valid, unique, and avoid base signal ids", () => {
  const definitions = [
    { id: "speed_mph", name: "Speed", expression: "vehicle_speed", unit: "mph", tare_offset: 0 },
    { id: "temp_f", name: "Temp", expression: "coolant_temp", unit: "°F", tare_offset: 0 },
  ];

  assert.equal(isAvailableDerivedSignalId("rpm_delta", "temp_f", definitions, ["engine_rpm"]), true);
  assert.equal(isAvailableDerivedSignalId("speed_mph", "temp_f", definitions, []), false);
  assert.equal(isAvailableDerivedSignalId("engine_rpm", "temp_f", definitions, ["engine_rpm"]), false);
  assert.equal(isAvailableDerivedSignalId("bad-id", "temp_f", definitions, []), false);
  assert.equal(isAvailableDerivedSignalId("temp_f", "temp_f", definitions, []), true);
});

test("new derived signals start from the PID that opened the editor", () => {
  assert.deepEqual(newDerivedSignalDefinition([
    { id: "derived_1", name: "Existing", expression: "engine_rpm", unit: "rpm", tare_offset: 0 },
  ], { id: "vehicle_speed", name: "Vehicle speed", unit: "km/h" }), {
    id: "derived_2",
    name: "Vehicle speed derived",
    expression: "vehicle_speed * 1",
    unit: "km/h",
    tare_offset: 0,
  });
});

test("derived formula inputs resolve to acquired PIDs and can match the output unit", () => {
  const inputs = [
    { id: "engine_rpm", unit: "rpm" },
    { id: "coolant_temp", unit: "°C" },
  ];
  assert.deepEqual(formulaSignalIds("engine_rpm - 500", inputs), ["engine_rpm"]);
  assert.deepEqual(formulaSignalIds("engine_rpm + coolant_temp", inputs, "rpm"), ["engine_rpm"]);
  assert.deepEqual(formulaSignalIds("engine_rpm + coolant_temp", inputs, "°F"), []);
});

test("same-unit derived traces share a range with their source while conversions stay separate", () => {
  const visible = [
    { id: "engine_rpm", unit: "rpm" },
    { id: "derived_1", unit: "rpm", formula: "engine_rpm - 500" },
    { id: "temp_f", unit: "°F", formula: "coolant_temp * 1.8 + 32" },
  ];
  const available = [
    { id: "engine_rpm", unit: "rpm" },
    { id: "coolant_temp", unit: "°C" },
    { id: "derived_1", unit: "rpm" },
    { id: "temp_f", unit: "°F" },
  ];
  assert.deepEqual(graphScaleSignalIds("derived_1", visible, available), ["derived_1", "engine_rpm"]);
  assert.deepEqual(graphScaleSignalIds("engine_rpm", visible, available), ["engine_rpm", "derived_1"]);
  assert.deepEqual(graphScaleSignalIds("temp_f", visible, available), ["temp_f"]);
});
