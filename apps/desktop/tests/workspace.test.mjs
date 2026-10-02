import assert from "node:assert/strict";
import test from "node:test";
import { chartPoints, filterSignals, swapPositions } from "../src/workspace.ts";

const signals = [
  { id: "engine_rpm", name: "Engine speed", service: 1, pid: 12, unit: "rpm", min: 0, max: 8000, description: "Crankshaft rotation speed" },
  { id: "coolant_temp", name: "Coolant temperature", service: 1, pid: 5, unit: "°C", min: -40, max: 215, description: "Engine coolant temperature" },
];

function point(elapsed_ns, value) {
  return { elapsed_ns, value };
}

test("PID search matches names, descriptions, IDs, units, and decimal or hex PID", () => {
  assert.deepEqual(filterSignals(signals, "crankshaft"), [signals[0]]);
  assert.deepEqual(filterSignals(signals, "coolant_temp"), [signals[1]]);
  assert.deepEqual(filterSignals(signals, "rpm"), [signals[0]]);
  assert.deepEqual(filterSignals(signals, "1"), [signals[0]]);
  assert.deepEqual(filterSignals(signals, "0C"), [signals[0]]);
  assert.deepEqual(filterSignals(signals, "12"), [signals[0]]);
  assert.deepEqual(filterSignals(signals, ""), signals);
});

test("drag reorders two graph positions without changing other selections", () => {
  assert.deepEqual(swapPositions(["rpm", "coolant", "speed", "intake"], 1, 3), ["rpm", "intake", "speed", "coolant"]);
  assert.deepEqual(swapPositions(["rpm", "coolant"], -1, 0), ["rpm", "coolant"]);
});

test("multiple traces share elapsed-time X positions and scale values independently", () => {
  const first = chartPoints([point(100, 0), point(200, 10)], 100, 200);
  const second = chartPoints([point(100, 100), point(150, 200), point(200, 300)], 100, 200);
  const coords = (line) => line.split(" ").map((pair) => pair.split(",").map(Number));
  const firstPoints = coords(first);
  const secondPoints = coords(second);

  assert.equal(firstPoints[0][0], secondPoints[0][0]);
  assert.equal(firstPoints.at(-1)[0], secondPoints.at(-1)[0]);
  assert.equal(secondPoints[1][0], 360);
  assert.equal(firstPoints[0][1], secondPoints[0][1]);
  assert.equal(firstPoints.at(-1)[1], secondPoints.at(-1)[1]);
});
