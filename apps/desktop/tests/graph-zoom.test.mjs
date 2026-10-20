import assert from "node:assert/strict";
import test from "node:test";
import { graphZoomAvailability, loadGraphPreferences, normalizeGraphPreferences, resolveFocusedGraphSignalId, saveGraphPreferences, zoomGraphRange, zoomLiveWindowFactor } from "../src/graphPreferences.ts";
import { buildScatterPoints } from "../src/scatterPlot.ts";
import { readFile } from "node:fs/promises";

const signalGraphs = await readFile(new URL("../src/SignalGraphs.svelte", import.meta.url), "utf8");
const liveChart = await readFile(new URL("../src/LiveChart.svelte", import.meta.url), "utf8");
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");

test("graph value zoom uses a stable 1.25 factor around the current range center", () => {
  assert.deepEqual(zoomGraphRange({ min: 10, max: 50 }, "in"), { min: 14, max: 46 });
  assert.deepEqual(zoomGraphRange({ min: 10, max: 50 }, "out"), { min: 5, max: 55 });
});

test("graph value zoom keeps constant negative ranges finite and nonzero", () => {
  const range = zoomGraphRange({ min: -5, max: -5 }, "in");
  assert.ok(Number.isFinite(range.min) && Number.isFinite(range.max));
  assert.ok(range.min < -5 && range.max > -5);
  assert.ok(Math.abs((range.min + range.max) / 2 + 5) < 1e-9);

  let largeRange = { min: 999_999_999_999, max: 1_000_000_000_001 };
  for (let index = 0; index < 500; index++) largeRange = zoomGraphRange(largeRange, "in");
  assert.ok(Number.isFinite(largeRange.min) && Number.isFinite(largeRange.max));
  assert.ok(largeRange.min < largeRange.max);
  assert.deepEqual(zoomGraphRange({ min: -Number.MAX_VALUE, max: Number.MAX_VALUE }, "out"), { min: -1e12, max: 1e12 });
});

test("live zoom is a persisted factor and stays inside supported time-span limits", () => {
  const preferences = normalizeGraphPreferences({ liveWindowSeconds: 18, liveZoomFactor: 0.8 });
  assert.equal(preferences.liveZoomFactor, 0.8);
  assert.equal(normalizeGraphPreferences({ liveWindowSeconds: 18, liveZoomFactor: 99999 }).liveZoomFactor, 600 / 18);
  assert.equal(zoomLiveWindowFactor(18, 1, "in"), 0.8);
  assert.equal(zoomLiveWindowFactor(18, 0.8, "out"), 1);
  assert.equal(zoomLiveWindowFactor(18, 1 / 36, "in"), 1 / 36);
  assert.equal(zoomLiveWindowFactor(18, 600 / 18, "out"), 600 / 18);

  const values = new Map();
  const storage = { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
  saveGraphPreferences(preferences, storage);
  assert.equal(loadGraphPreferences(storage).liveZoomFactor, 0.8);
});

test("focused graph signal falls back to the first remaining trace after removal", () => {
  assert.equal(resolveFocusedGraphSignalId(["rpm", "boost"], "boost"), "boost");
  assert.equal(resolveFocusedGraphSignalId(["rpm"], "boost"), "rpm");
  assert.equal(resolveFocusedGraphSignalId([], "boost"), null);
});

test("replay time controls remain available to recover an empty playback window", () => {
  assert.deepEqual(graphZoomAvailability({
    graphType: "time_series",
    hasScatterAxes: false,
    hasScatterData: false,
    hasLiveXData: false,
    canZoomReplay: true,
    canResetReplay: true,
    canResetLive: false,
    hasReplayWindow: true,
    hasFocusedTraceData: false,
    hasManualBounds: false,
  }), { x: true, y: false, auto: true });
});

test("Auto remains available to clear stale manual bounds, live zoom, and empty scatter axes", () => {
  const empty = {
    hasScatterAxes: true,
    hasScatterData: false,
    hasLiveXData: false,
    canZoomReplay: false,
    canResetReplay: false,
    canResetLive: false,
    hasReplayWindow: false,
    hasFocusedTraceData: false,
  };
  assert.equal(graphZoomAvailability({ ...empty, graphType: "time_series", hasManualBounds: true }).auto, true);
  assert.equal(graphZoomAvailability({ ...empty, graphType: "time_series", hasManualBounds: false, canResetLive: true }).auto, true);
  assert.deepEqual(graphZoomAvailability({ ...empty, graphType: "xy_scatter", hasManualBounds: true }), { x: false, y: false, auto: true });
  assert.deepEqual(graphZoomAvailability({ ...empty, graphType: "xy_scatter", hasManualBounds: false }), { x: false, y: false, auto: false });
});

test("graph settings keeps zoom controls compact and exposes trace focus accessibly", () => {
  assert.match(signalGraphs, /class="graph-zoom-controls"/);
  assert.match(signalGraphs, /<span class="graph-settings-title">Graph settings<\/span>/);
  assert.match(signalGraphs, /class="graph-focus-title"/);
  assert.match(signalGraphs, /aria-label=\{xZoomOutTitle\}/);
  assert.match(signalGraphs, /graphPreferences\.graphType === "xy_scatter" && scatterX && scatterY/);
  assert.match(signalGraphs, /graphZoomAvailability\(/);
  assert.match(signalGraphs, /aria-label=\{yZoomInTitle\}/);
  assert.match(signalGraphs, /disabled=\{!canZoomX\}/);
  assert.match(signalGraphs, /disabled=\{!canZoomY\}/);
  assert.match(signalGraphs, /disabled=\{!autoZoomAvailable\}/);
  assert.match(signalGraphs, /title=\{autoZoomTitle\}/);
  assert.match(liveChart, /aria-pressed=\{item\.trace\.signalId === focusedSignalId\}/);
  assert.match(liveChart, /onSelectFocusedTrace/);
  assert.match(app, /onZoomTime=\{session \? zoom : undefined\}/);
  assert.match(app, /onResetTimeZoom=\{session \? resetZoom : undefined\}/);
});

test("scatter zoom continues to derive its view from the capped aligned sample set", () => {
  const samples = Array.from({ length: 400 }, (_, index) => ({ elapsed_ns: index * 100_000_000, value: index }));
  const points = buildScatterPoints(samples, samples, null, { startNs: 0, endNs: 39_900_000_000 });
  assert.equal(points.length, 128);
  assert.deepEqual(points[0], { elapsed_ns: 0, x: 0, y: 0, colorValue: null });
  assert.deepEqual(points.at(-1), { elapsed_ns: 39_900_000_000, x: 399, y: 399, colorValue: null });
});
