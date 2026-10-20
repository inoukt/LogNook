import assert from "node:assert/strict";
import test from "node:test";
import { graphCoordinates, graphValueRange, liveGraphTimeWindow } from "../src/graphPreferences.ts";

const history = await import("../src/liveGraphHistory.ts").catch(() => ({}));

test("live time windows begin at zero and scroll after the selected span fills", () => {
  assert.deepEqual(liveGraphTimeWindow(4e9, 10e9), { startNs: 0, endNs: 4e9 });
  assert.deepEqual(liveGraphTimeWindow(14e9, 10e9), { startNs: 4e9, endNs: 14e9 });
});

test("dense live graph renders the full selected window and preserves an early narrow peak", () => {
  const samples = Array.from({ length: 1001 }, (_, index) => ({
    elapsed_ns: 10_000_000_000 + index * 10_000_000,
    value: index === 20 ? 100 : 0,
  }));
  const points = graphCoordinates(samples, 10_000_000_000, 20_000_000_000, { min: 0, max: 100 });
  assert.equal(points[0].x, 0, "history must reach the left edge after the point budget fills");
  assert.equal(points.at(-1).x, 720);
  assert.ok(points.some(({ value }) => value === 100), "render reduction must preserve brief peaks");
  assert.ok(points.length <= 180);
});

test("live history retains ten seconds of fast samples after the first 180 arrivals", () => {
  assert.equal(typeof history.appendLiveGraphSample, "function");
  let samples = [];
  for (let index = 0; index <= 2000; index++) {
    samples = history.appendLiveGraphSample(samples, { elapsed_ns: index * 10_000_000, value: index });
  }
  const visible = history.graphWindowSamples(samples, 10_000_000_000, 20_000_000_000);
  assert.equal(visible.at(-1).elapsed_ns, 20_000_000_000);
  assert.ok(visible[0].elapsed_ns <= 10_000_000_000);
  assert.ok(visible.length >= 1001);
});

test("live history evicts old time while retaining a boundary point and resets on reconnect", () => {
  assert.equal(typeof history.appendLiveGraphSample, "function");
  let samples = [];
  for (let second = 0; second <= 700; second++) {
    samples = history.appendLiveGraphSample(samples, { elapsed_ns: second * 1e9, value: second });
  }
  assert.equal(samples[0].elapsed_ns, 99_000_000_000);
  assert.equal(samples.at(-1).elapsed_ns, 700_000_000_000);
  assert.deepEqual(history.appendLiveGraphSample(samples, { elapsed_ns: 1e9, value: 1 }), [{ elapsed_ns: 1e9, value: 1 }]);
});

test("automatic value scaling handles large retained source histories without a stack overflow", () => {
  const samples = Array.from({ length: 180_001 }, (_, index) => ({ elapsed_ns: index, value: index - 90_000 }));
  const range = graphValueRange(samples, []);
  assert.ok(range.min < -90_000);
  assert.ok(range.max > 90_000);
});
