import assert from "node:assert/strict";
import test from "node:test";
import {
  averageInWindow,
  chartPoints,
  clampSeekPosition,
  filterSignals,
  followPlayhead,
  formatDurationNs,
  formatTimelineNs,
  graphRefreshIntervalMs,
  orderMarkersForTimeline,
  panTimeWindow,
  rememberWorkbenchGraphSignalId,
  positionAtChartX,
  restoreResearchSignalOptions,
  restoreWorkbenchGraphSignalIds,
  restoreWorkspacePreferences,
  sampleTimeRange,
  scrubTimeWindow,
  upsertResearchSignalOption,
  setGraphSignal,
  setLogSignal,
  signalColor,
  swapPositions,
  wheelScrubPosition,
  zoomTimeWindow,
} from "../src/workspace.ts";

const signals = [
  { id: "engine_rpm", name: "Engine speed", service: 1, pid: 12, unit: "rpm", min: 0, max: 8000, description: "Crankshaft rotation speed" },
  { id: "coolant_temp", name: "Coolant temperature", service: 1, pid: 5, unit: "°C", min: -40, max: 215, description: "Engine coolant temperature" },
];

const defaultWorkspacePreferences = {
  logSignalIds: ["engine_rpm", "coolant_temp"],
  graphSignalIds: ["engine_rpm", "coolant_temp"],
  graphMode: "overlay",
  signalPickerVisible: true,
  statusHeight: 42,
  theme: "midnight",
  highContrast: false,
  uiScale: 100,
};

function point(elapsed_ns, value) {
  return { elapsed_ns, value };
}

test("PID average uses only samples in the visible time window", () => {
  assert.equal(averageInWindow([point(0, 10), point(5, 20), point(10, 30)], { startNs: 5, endNs: 10 }), 25);
  assert.equal(averageInWindow([point(0, 10)], { startNs: 5, endNs: 10 }), undefined);
});

test("visible PID ranges share the bounds of the plotted signals", () => {
  const visibleRange = sampleTimeRange([point(100, 1), point(500, 2)]);
  assert.deepEqual(visibleRange, { startNs: 100, endNs: 500 });
  assert.equal(averageInWindow([point(0, 100), point(100, 10), point(200, 30)], visibleRange), 20);
});

test("playhead-follow preserves the visible span and centers even at recording edges", () => {
  assert.deepEqual(followPlayhead({ startNs: 100, endNs: 500 }, 800, 1_000), { startNs: 600, endNs: 1_000 });
  assert.deepEqual(followPlayhead({ startNs: 100, endNs: 500 }, 0, 1_000), { startNs: -200, endNs: 200 });
  assert.deepEqual(followPlayhead({ startNs: 100, endNs: 500 }, 1_000, 1_000), { startNs: 800, endNs: 1_200 });
});

test("graph clicks map screen coordinates to the visible recording time", () => {
  const window = { startNs: 100, endNs: 900 };
  assert.equal(positionAtChartX(25, 10, 100, window), 220);
  assert.equal(positionAtChartX(-10, 10, 100, window), 100);
  assert.equal(positionAtChartX(10, 10, 0, window), 100);
});

test("wheel scrubbing moves playback by a fraction of the visible time range", () => {
  assert.equal(wheelScrubPosition(500, 100, { startNs: 0, endNs: 1_000 }, 2_000), 600);
  assert.equal(wheelScrubPosition(1_950, 100, { startNs: 0, endNs: 1_000 }, 2_000), 2_000);
});

test("wheel scrubbing updates position and centers the visible window together", () => {
  assert.deepEqual(
    scrubTimeWindow(0, -100, { startNs: 0, endNs: 30_000_000_000 }, 60_000_000_000),
    { positionNs: 0, range: { startNs: -15_000_000_000, endNs: 15_000_000_000 } },
  );
});

test("custom PID color overrides the default palette and invalid colors are ignored", () => {
  assert.equal(signalColor("rpm", 0, {}), "#56c8bd");
  assert.equal(signalColor("rpm", 0, { rpm: "#ff0000" }), "#ff0000");
  assert.equal(signalColor("rpm", 0, { rpm: "red" }), "#56c8bd");
});

test("derived trace colors avoid the colors of their formula inputs", () => {
  assert.equal(signalColor("derived_1", 35, {}, ["#56c8bd"]), "#6ca8ff");
  assert.equal(signalColor("derived_1", 35, { derived_1: "#56c8bd" }, ["#56c8bd"]), "#56c8bd");
});

test("workspace preferences survive a serialized round trip", () => {
  const preferences = {
    logSignalIds: ["coolant_temp", "engine_rpm"],
    graphSignalIds: ["coolant_temp"],
    graphMode: "separate",
    signalPickerVisible: false,
    statusHeight: 96,
    theme: "steel",
    highContrast: true,
    uiScale: 90,
  };
  assert.deepEqual(
    restoreWorkspacePreferences(JSON.stringify(preferences), signals.map(({ id }) => id), defaultWorkspacePreferences, 120),
    preferences,
  );
});

test("workspace preference restoration filters unsupported PIDs and malformed values", () => {
  const restored = restoreWorkspacePreferences(JSON.stringify({
    logSignalIds: ["engine_rpm", "missing", "engine_rpm", 4],
    graphSignalIds: ["coolant_temp", "engine_rpm", "missing"],
    graphMode: "invalid",
    signalPickerVisible: "hidden",
    statusHeight: 900,
  }), ["engine_rpm", "coolant_temp"], defaultWorkspacePreferences, 120);

  assert.deepEqual(restored, {
    logSignalIds: ["engine_rpm"],
    graphSignalIds: ["engine_rpm"],
    graphMode: "overlay",
    signalPickerVisible: true,
    statusHeight: 120,
    theme: "midnight",
    highContrast: false,
    uiScale: 100,
  });
  assert.deepEqual(
    restoreWorkspacePreferences("not json", ["engine_rpm"], defaultWorkspacePreferences, 120),
    { ...defaultWorkspacePreferences, logSignalIds: ["engine_rpm"], graphSignalIds: ["engine_rpm"] },
  );
});

test("research graph signals restore as graph-only options and reject malformed entries", () => {
  const longName = "N".repeat(142);
  const saved = [
    { id: "oem:scion-xb-21-01", name: "Odometer", unit: "km", research: true, pid: 0x28 },
    { id: "a30_mode06_tid_35_mid_01", name: "Mode 06 TID 35 MID 01", unit: "raw count", research: true },
    { id: "oem:long-name", name: longName, unit: "rpm", research: true },
    { id: "engine_rpm", name: "Engine speed", unit: "rpm", research: true },
    { id: "oem:bad", name: "", unit: "rpm", research: true },
    { id: "oem:other", name: "Not research", unit: "rpm", research: false },
  ];
  const researchSignals = restoreResearchSignalOptions(JSON.stringify(saved));

  assert.deepEqual(researchSignals, saved.slice(0, 3));
  const graphableIds = restoreWorkbenchGraphSignalIds(JSON.stringify([
    "engine_rpm", "oem:scion-xb-21-01", "a30_mode06_tid_35_mid_01", "unknown",
  ]));
  const restored = restoreWorkspacePreferences(
    JSON.stringify({ logSignalIds: ["engine_rpm"], graphSignalIds: ["engine_rpm", "oem:scion-xb-21-01", "a30_mode06_tid_35_mid_01", "unknown"] }),
    ["engine_rpm"],
    defaultWorkspacePreferences,
    120,
    graphableIds.filter((id) => id === "engine_rpm" || researchSignals.some((signal) => signal.id === id)),
  );
  assert.deepEqual(restored.logSignalIds, ["engine_rpm"]);
  assert.deepEqual(restored.graphSignalIds, ["engine_rpm", "oem:scion-xb-21-01", "a30_mode06_tid_35_mid_01"]);
});

test("OBDLink SX read-test channels survive research metadata persistence", () => {
  const signals = [
    { id: "obdlink_sx_mode02_engine_rpm", name: "Freeze frame RPM", unit: "rpm", research: true },
    { id: "obdlink_sx_mode06_tid_35_mid_01", name: "Mode 06 TID 35 MID 01", unit: "raw count", research: true },
  ];

  assert.deepEqual(restoreResearchSignalOptions(JSON.stringify(signals)), signals);
  assert.deepEqual(upsertResearchSignalOption([], signals[0]), [signals[0]]);
  assert.deepEqual(
    restoreWorkspacePreferences(
      JSON.stringify({ logSignalIds: ["engine_rpm"], graphSignalIds: ["engine_rpm", ...signals.map(({ id }) => id)] }),
      ["engine_rpm"],
      defaultWorkspacePreferences,
      120,
      signals.map(({ id }) => id),
    ).graphSignalIds,
    ["engine_rpm", ...signals.map(({ id }) => id)],
  );
});

test("Workbench graphable ID restoration is unique, typed, and bounded", () => {
  const saved = ["engine_rpm", "engine_rpm", 4, "", ...Array.from({ length: 510 }, (_, i) => `oem:candidate-${i}`)];
  const ids = restoreWorkbenchGraphSignalIds(JSON.stringify(saved));
  assert.equal(ids.length, 500);
  assert.equal(new Set(ids).size, 500);
  assert.equal(ids[0], "oem:candidate-10");
  assert.equal(ids.at(-1), "oem:candidate-509");
});

test("remembering a Workbench graphable ID deduplicates and keeps the newest 500", () => {
  const existing = Array.from({ length: 500 }, (_, i) => `oem:candidate-${i}`);
  const next = rememberWorkbenchGraphSignalId(existing, "oem:candidate-500");
  assert.equal(next.length, 500);
  assert.equal(next[0], "oem:candidate-1");
  assert.equal(next.at(-1), "oem:candidate-500");
  assert.equal(rememberWorkbenchGraphSignalId(next, "oem:candidate-500").length, 500);
  const refreshed = rememberWorkbenchGraphSignalId(next, "oem:candidate-1");
  assert.equal(refreshed[0], "oem:candidate-2");
  assert.equal(refreshed.at(-1), "oem:candidate-1");
});

test("research graph signal restoration is bounded and upsert replaces by ID", () => {
  const options = Array.from({ length: 500 }, (_, index) => ({
    id: `oem:candidate-${index}`,
    name: `Candidate ${index}`,
    unit: "rpm",
    research: true,
  }));
  const restored = restoreResearchSignalOptions(JSON.stringify(options));
  assert.equal(restored.length, 500);
  assert.equal(restoreResearchSignalOptions(JSON.stringify([...options, {
    id: "oem:candidate-500", name: "Candidate 500", unit: "rpm", research: true,
  }])).length, 500);

  const updated = upsertResearchSignalOption(restored, {
    id: "oem:candidate-250", name: "Updated candidate", unit: "km/h", research: true,
  });
  assert.equal(updated.length, 500);
  assert.equal(updated.filter(({ id }) => id === "oem:candidate-250").length, 1);
  assert.deepEqual(updated.at(-1), {
    id: "oem:candidate-250", name: "Updated candidate", unit: "km/h", research: true,
  });
});

test("workspace appearance preferences accept known themes and reject unknown values", () => {
  assert.deepEqual(
    restoreWorkspacePreferences(JSON.stringify({ theme: "mist", highContrast: true }), ["engine_rpm"], defaultWorkspacePreferences, 120),
    { ...defaultWorkspacePreferences, logSignalIds: ["engine_rpm"], graphSignalIds: ["engine_rpm"], theme: "mist", highContrast: true },
  );
  assert.deepEqual(
    restoreWorkspacePreferences(JSON.stringify({ theme: "solarized", highContrast: "yes" }), ["engine_rpm"], defaultWorkspacePreferences, 120),
    { ...defaultWorkspacePreferences, logSignalIds: ["engine_rpm"], graphSignalIds: ["engine_rpm"] },
  );
});

test("workspace UI scale stays within 80–120 percent and defaults malformed values", () => {
  const restoreScale = (uiScale) => restoreWorkspacePreferences(
    JSON.stringify({ uiScale }), ["engine_rpm"], defaultWorkspacePreferences, 120,
  ).uiScale;
  assert.equal(restoreScale(70), 80);
  assert.equal(restoreScale(125), 120);
  assert.equal(restoreScale("90"), 100);
});

test("graph refresh interval is faster while scrubbing the timeline", () => {
  assert.equal(graphRefreshIntervalMs(true), 33);
  assert.equal(graphRefreshIntervalMs(false), 100);
});

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

test("graph rendering limits points to screen resolution while preserving signal peaks", () => {
  const samples = [point(0, 1), point(1, 50), point(2, 2), point(3, 100), point(4, -100), point(5, 3), point(6, 4)];
  const coords = chartPoints(samples, 0, 6, 2).split(" ").map((pair) => pair.split(",").map(Number));

  assert.ok(coords.length <= 4);
  assert.ok(coords.some(([, y]) => y === 34));
  assert.ok(coords.some(([, y]) => y === 134));
});

test("zero-duration samples align with marker and playhead guides at the left edge", () => {
  assert.equal(chartPoints([{ elapsed_ns: 5, value: 10 }], 5, 5, 720).split(",")[0], "0.0");
});

test("duration formatting keeps millisecond precision and switches to hours", () => {
  assert.equal(formatDurationNs(59_999_999_999), "00:59.999");
  assert.equal(formatDurationNs(61_234_000_000), "01:01.234");
  assert.equal(formatDurationNs(3_661_000_000_000), "1:01:01.000");
  assert.equal(formatDurationNs(-1), "00:00.000");
  assert.equal(formatTimelineNs(-1_500_000_000), "−00:01.500");
  assert.equal(formatTimelineNs(1_500_000_000), "00:01.500");
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
