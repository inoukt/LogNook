import assert from "node:assert/strict";
import test from "node:test";
import {
  chartPoints,
  clampSeekPosition,
  filterSignals,
  formatDurationNs,
  orderMarkersForTimeline,
  panTimeWindow,
  setGraphSignal,
  setLogSignal,
  swapPositions,
  zoomTimeWindow,
} from "../src/workspace.ts";

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

test("logging a signal adds it to graph by default, then graph can be hidden independently", () => {
  const logged = setLogSignal([], [], "engine_rpm", true);
  assert.deepEqual(logged, { logSignalIds: ["engine_rpm"], graphSignalIds: ["engine_rpm"] });
  assert.deepEqual(setGraphSignal(logged.graphSignalIds, "engine_rpm", false), []);
  assert.deepEqual(logged.logSignalIds, ["engine_rpm"]);
});

test("turning logging off also removes the signal from the graph", () => {
  assert.deepEqual(
    setLogSignal(["engine_rpm", "coolant_temp"], ["engine_rpm", "coolant_temp"], "engine_rpm", false),
    { logSignalIds: ["coolant_temp"], graphSignalIds: ["coolant_temp"] },
  );
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

test("zero-duration samples align with marker and playhead guides at the left edge", () => {
  assert.equal(chartPoints([{ elapsed_ns: 5, value: 10 }], 5, 5, 720).split(",")[0], "0.0");
});

test("duration formatting keeps millisecond precision and switches to hours", () => {
  assert.equal(formatDurationNs(59_999_999_999), "00:59.999");
  assert.equal(formatDurationNs(61_234_000_000), "01:01.234");
  assert.equal(formatDurationNs(3_661_000_000_000), "1:01:01.000");
  assert.equal(formatDurationNs(-1), "00:00.000");
});

test("seek positions stay within the recording duration", () => {
  assert.equal(clampSeekPosition(-10, 100), 0);
  assert.equal(clampSeekPosition(40, 100), 40);
  assert.equal(clampSeekPosition(110, 100), 100);
  assert.equal(clampSeekPosition(50, -1), 0);
  assert.equal(clampSeekPosition(4.6, 100), 5);
});

test("zoom scales around the anchor and keeps the window inside duration", () => {
  assert.deepEqual(
    zoomTimeWindow({ startNs: 200, endNs: 600 }, 300, 2, 1_000),
    { startNs: 250, endNs: 450 },
  );
  assert.deepEqual(
    zoomTimeWindow({ startNs: 0, endNs: 400 }, 0, 0.5, 1_000),
    { startNs: 0, endNs: 800 },
  );
  assert.deepEqual(
    zoomTimeWindow({ startNs: 0, endNs: 101 }, 51, 2, 101),
    { startNs: 25, endNs: 76 },
  );
});

test("panning preserves window width and clamps to the duration edges", () => {
  const window = { startNs: 200, endNs: 600 };
  assert.deepEqual(panTimeWindow(window, 100, 1_000), { startNs: 300, endNs: 700 });
  assert.deepEqual(panTimeWindow(window, -500, 1_000), { startNs: 0, endNs: 400 });
  assert.deepEqual(window, { startNs: 200, endNs: 600 });
});

test("timeline markers are sorted by elapsed time without mutating input", () => {
  const markers = [
    { elapsed_ns: 20, label: "later" },
    { elapsed_ns: 10, label: "first" },
    { elapsed_ns: 10, label: "second" },
  ];
  assert.deepEqual(orderMarkersForTimeline(markers), [
    { elapsed_ns: 10, label: "first" },
    { elapsed_ns: 10, label: "second" },
    { elapsed_ns: 20, label: "later" },
  ]);
  assert.deepEqual(markers.map(({ label }) => label), ["later", "first", "second"]);
});
