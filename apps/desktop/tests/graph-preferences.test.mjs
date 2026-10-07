import assert from "node:assert/strict";
import test from "node:test";

const graphPreferences = await import("../src/graphPreferences.ts").catch(() => ({}));

test("graph preferences validate per-signal controls and saved reference data", () => {
  assert.equal(typeof graphPreferences.normalizeGraphPreferences, "function");
  if (typeof graphPreferences.normalizeGraphPreferences !== "function") return;

  const normalized = graphPreferences.normalizeGraphPreferences({
    signals: {
      rpm: { min: 0, max: 8000, view: "heatmap", lineStyle: "dotted", lineWidth: 4.5 },
      coolant: { min: 40, max: 20, view: "invalid", lineStyle: "solid", lineWidth: 99 },
    },
    reference: {
      capturedAt: 12,
      traces: [{
        signalId: "rpm",
        name: "Engine RPM",
        unit: "rpm",
        color: "#56c8bd",
        samples: [{ elapsed_ns: 0, value: 1200 }, { elapsed_ns: Infinity, value: 4 }, { elapsed_ns: 2, value: NaN }],
      }],
    },
  });

  assert.deepEqual(normalized.signals.rpm, { min: 0, max: 8000, view: "heatmap", lineStyle: "dotted", lineWidth: 4.5 });
  assert.deepEqual(normalized.signals.coolant, { min: null, max: null, view: "line", lineStyle: "solid", lineWidth: 5 });
  assert.deepEqual(normalized.reference.traces[0].samples, [{ elapsed_ns: 0, value: 1200 }]);
});

test("derived formulas appear in parenthesized graph readouts and default to a dotted line", () => {
  assert.equal(graphPreferences.formatDerivedExpression(" engine_rpm - 500 "), "(engine_rpm - 500)");
  assert.equal(graphPreferences.formatDerivedExpression(undefined), "");
  assert.equal(graphPreferences.defaultGraphSignalPreference(true).lineStyle, "dotted");
  assert.equal(graphPreferences.defaultGraphSignalPreference(false).lineStyle, "solid");
});

test("existing derived graph styles migrate to dotted without resetting other preferences", () => {
  const original = graphPreferences.normalizeGraphPreferences({ signals: {
    derived_1: { min: 0, max: 2_000, view: "line", lineStyle: "dashed", lineWidth: 3 },
    engine_rpm: { min: null, max: null, view: "line", lineStyle: "solid", lineWidth: 2 },
  } });
  const migrated = graphPreferences.migrateDerivedGraphStyles(original, ["derived_1"]);
  assert.deepEqual(migrated.signals.derived_1, { min: 0, max: 2_000, view: "line", lineStyle: "dotted", lineWidth: 3 });
  assert.deepEqual(migrated.signals.engine_rpm, original.signals.engine_rpm);
});

test("same-unit derived traces include source values in their automatic vertical range", () => {
  const derived = [{ elapsed_ns: 0, value: -400 }, { elapsed_ns: 1, value: -300 }];
  const source = [{ elapsed_ns: 0, value: 100 }, { elapsed_ns: 1, value: 200 }];
  const rangeSamples = graphPreferences.graphRangeSamples(derived, source, true);
  assert.deepEqual(rangeSamples, [...derived, ...source]);
  assert.deepEqual(graphPreferences.graphRangeSamples(derived, source, false), derived);
  const sharedRange = graphPreferences.graphValueRange(rangeSamples, []);
  const sourcePoint = graphPreferences.graphCoordinates(source, 0, 1, sharedRange)[0];
  const derivedPoint = graphPreferences.graphCoordinates(derived, 0, 1, sharedRange)[0];
  assert.ok(sourcePoint.y < derivedPoint.y, "source RPM should plot above RPM minus 500");
});

test("live graph time moves by display time and holds only the last plotted point", () => {
  const first = graphPreferences.liveGraphTimeWindow(20_000_000_000);
  const next = graphPreferences.liveGraphTimeWindow(20_100_000_000);
  assert.equal(first.endNs - first.startNs, 18_000_000_000);
  assert.deepEqual(next, { startNs: 2_100_000_000, endNs: 20_100_000_000 });

  const measured = [{ elapsed_ns: 20_000_000_000, value: 900, x: 713, y: 64 }];
  assert.deepEqual(graphPreferences.holdLastPointAtRightEdge(measured), [
    measured[0],
    { ...measured[0], x: 720 },
  ]);
  assert.equal(measured.length, 1, "visual hold must not alter measured samples");
});

test("live traces become stale using recent sample cadence instead of looking current forever", () => {
  assert.equal(typeof graphPreferences.isLiveTraceStale, "function");
  if (typeof graphPreferences.isLiveTraceStale !== "function") return;

  const samples = Array.from({ length: 5 }, (_, index) => ({ elapsed_ns: 10_000_000_000 + index * 500_000_000 }));
  assert.equal(graphPreferences.isLiveTraceStale(samples, 16_999_000_000), false);
  assert.equal(graphPreferences.isLiveTraceStale(samples, 17_001_000_000), true);

  const slowSamples = [
    { elapsed_ns: 10_000_000_000 },
    { elapsed_ns: 11_500_000_000 },
  ];
  assert.equal(graphPreferences.isLiveTraceStale(slowSamples, 16_499_000_000), false);
  assert.equal(graphPreferences.isLiveTraceStale(slowSamples, 16_501_000_000), true);
  const slowerSamples = [10_000_000_000, 12_000_000_000, 14_000_000_000]
    .map((elapsed_ns) => ({ elapsed_ns }));
  assert.equal(graphPreferences.isLiveTraceStale(slowerSamples, 19_999_000_000), false);
  assert.equal(graphPreferences.isLiveTraceStale(slowerSamples, 20_001_000_000), true);
  assert.equal(graphPreferences.isLiveTraceStale([], 20_000_000_000), false);
});

test("graph preferences load safely and round-trip through local storage", () => {
  assert.equal(typeof graphPreferences.loadGraphPreferences, "function");
  assert.equal(typeof graphPreferences.saveGraphPreferences, "function");
  if (typeof graphPreferences.loadGraphPreferences !== "function" || typeof graphPreferences.saveGraphPreferences !== "function") return;

  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
  const preferences = graphPreferences.normalizeGraphPreferences({ signals: { rpm: { min: 500, max: null, view: "bar" } } });
  graphPreferences.saveGraphPreferences(preferences, storage);
  assert.deepEqual(graphPreferences.loadGraphPreferences(storage), preferences);
  assert.deepEqual(graphPreferences.loadGraphPreferences({ getItem: () => "{" }), graphPreferences.normalizeGraphPreferences(null));
});

test("reference capture copies only the latest bounded samples", () => {
  assert.equal(typeof graphPreferences.captureGraphReference, "function");
  if (typeof graphPreferences.captureGraphReference !== "function") return;

  const samples = Array.from({ length: 200 }, (_, elapsed_ns) => ({ elapsed_ns, value: elapsed_ns * 2 }));
  const trace = { signalId: "rpm", name: "Engine RPM", unit: "rpm", color: "#56c8bd", samples };
  const captured = graphPreferences.captureGraphReference(graphPreferences.normalizeGraphPreferences(null), [trace], 99);

  assert.equal(captured.reference.capturedAt, 99);
  assert.equal(captured.reference.traces[0].samples.length, 180);
  assert.deepEqual(captured.reference.traces[0].samples[0], { elapsed_ns: 20, value: 40 });
  assert.deepEqual(captured.reference.traces[0].samples.at(-1), { elapsed_ns: 199, value: 398 });
  assert.equal(samples.length, 200);
  assert.equal(captured.reference.traces[0].samples[0] === samples[20], false);
});

test("manual graph bounds drive bounded plot coordinates", () => {
  assert.equal(typeof graphPreferences.graphValueRange, "function");
  assert.equal(typeof graphPreferences.graphCoordinates, "function");
  if (typeof graphPreferences.graphValueRange !== "function" || typeof graphPreferences.graphCoordinates !== "function") return;

  const samples = Array.from({ length: 200 }, (_, elapsed_ns) => ({ elapsed_ns, value: elapsed_ns }));
  const range = graphPreferences.graphValueRange(samples, [], { min: 20, max: 100 });
  const points = graphPreferences.graphCoordinates(samples, 20, 100, range);

  assert.deepEqual(range, { min: 20, max: 100 });
  assert.equal(points.length, 180);
  assert.equal(points[0].x, 0);
  assert.equal(points.at(-1).x, 720);
  assert.ok(points.every((point) => point.y >= 22 && point.y <= 146));
});

test("session graphs retain all backend downsampled points across the time window", () => {
  const pointLimit = 360 * 4;
  const samples = Array.from({ length: pointLimit }, (_, elapsed_ns) => ({
    elapsed_ns,
    value: elapsed_ns === 0 ? -10_000 : elapsed_ns,
  }));
  const range = graphPreferences.graphValueRange(samples, []);
  const points = graphPreferences.graphCoordinates(samples, 0, pointLimit - 1, range, pointLimit);

  assert.equal(points.length, pointLimit);
  assert.equal(points[0].x, 0);
  assert.equal(points.at(-1).x, 720);
  assert.ok(range.min < -10_000);
  assert.deepEqual(graphPreferences.nearestGraphSample(samples, 0, pointLimit), samples[0]);
});

test("reference samples map to the active time window without changing their values", () => {
  assert.equal(typeof graphPreferences.mapReferenceSamples, "function");
  if (typeof graphPreferences.mapReferenceSamples !== "function") return;

  const samples = [{ elapsed_ns: 100, value: 10 }, { elapsed_ns: 200, value: 20 }];
  assert.deepEqual(graphPreferences.mapReferenceSamples(samples, 1000, 3000), [
    { elapsed_ns: 1000, value: 10 },
    { elapsed_ns: 3000, value: 20 },
  ]);
});

test("cursor readout chooses the nearest sample at the selected time", () => {
  assert.equal(typeof graphPreferences.nearestGraphSample, "function");
  if (typeof graphPreferences.nearestGraphSample !== "function") return;

  const samples = [{ elapsed_ns: 100, value: 1 }, { elapsed_ns: 200, value: 2 }, { elapsed_ns: 300, value: 3 }];
  assert.deepEqual(graphPreferences.nearestGraphSample(samples, 240), { elapsed_ns: 200, value: 2 });
  assert.equal(graphPreferences.nearestGraphSample([], 240), undefined);
});

test("heatmap rows occupy separate bounded lanes", () => {
  assert.equal(typeof graphPreferences.graphHeatmapRow, "function");
  if (typeof graphPreferences.graphHeatmapRow !== "function") return;
  assert.deepEqual(graphPreferences.graphHeatmapRow(0, 2), { y: 22, height: 62 });
  assert.deepEqual(graphPreferences.graphHeatmapRow(1, 2), { y: 84, height: 62 });
});
