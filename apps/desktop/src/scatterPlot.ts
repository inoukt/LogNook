import type { TimeWindow } from "./workspace";

export type ScatterPoint = { elapsed_ns: number; x: number; y: number; colorValue: number | null };
export type NumericSample = { elapsed_ns: number; value: number };

const SECOND_NS = 1_000_000_000;
const MAX_STREAM_POINTS = 256;

export function buildScatterPoints(
  xSamples: readonly NumericSample[],
  ySamples: readonly NumericSample[],
  colorSamples: readonly NumericSample[] | null,
  window: TimeWindow,
  maxPoints = 128,
): ScatterPoint[] {
  if (!Number.isFinite(window.startNs) || !Number.isFinite(window.endNs)
    || window.endNs < window.startNs || maxPoints < 1) return [];
  const x = windowSamples(xSamples, window);
  const y = windowSamples(ySamples, window);
  const color = colorSamples ? windowSamples(colorSamples, window) : [];
  if (!x.length || !y.length) return [];
  const xFreshNs = freshnessLimit(x);
  const yFreshNs = freshnessLimit(y);
  const colorFreshNs = color.length ? freshnessLimit(color) : 0;
  const points: ScatterPoint[] = [];
  let xi = 0;
  let yi = 0;
  let ci = 0;
  let currentX: NumericSample | undefined;
  let currentY: NumericSample | undefined;
  let currentColor: NumericSample | undefined;

  while (xi < x.length || yi < y.length) {
    const time = Math.min(x[xi]?.elapsed_ns ?? Infinity, y[yi]?.elapsed_ns ?? Infinity);
    while (xi < x.length && x[xi].elapsed_ns === time) currentX = x[xi++];
    while (yi < y.length && y[yi].elapsed_ns === time) currentY = y[yi++];
    while (ci < color.length && color[ci].elapsed_ns <= time) currentColor = color[ci++];
    if (time < window.startNs || time > window.endNs || !currentX || !currentY
      || time - currentX.elapsed_ns > xFreshNs || time - currentY.elapsed_ns > yFreshNs) continue;
    const colorValue = currentColor && time - currentColor.elapsed_ns <= colorFreshNs ? currentColor.value : null;
    points.push({ elapsed_ns: time, x: currentX.value, y: currentY.value, colorValue });
  }

  return downsample(points, Math.max(1, Math.floor(maxPoints)));
}

export function scatterRange(values: readonly number[], minOverride: number | null = null, maxOverride: number | null = null): { min: number; max: number } {
  let dataMin = Infinity;
  let dataMax = -Infinity;
  for (const value of values) if (Number.isFinite(value)) { dataMin = Math.min(dataMin, value); dataMax = Math.max(dataMax, value); }
  if (dataMin === Infinity) { dataMin = 0; dataMax = 1; }
  let min = minOverride ?? dataMin;
  let max = maxOverride ?? dataMax;
  if (minOverride === null && maxOverride === null) {
    const padding = (max - min) * 0.08 || Math.max(Math.abs(min) * 0.08, 1);
    min -= padding;
    max += padding;
  } else if (minOverride === null) min = Math.min(dataMin, max) - (Math.abs(max) * 0.08 || 1);
  else if (maxOverride === null) max = Math.max(dataMax, min) + (Math.abs(min) * 0.08 || 1);
  if (!(max > min)) { const padding = Math.max(Math.abs(min) * 0.08, 1); min -= padding; max += padding; }
  return { min, max };
}

export function scatterColor(amount: number): string {
  const t = Math.min(1, Math.max(0, Number.isFinite(amount) ? amount : 0));
  const stops = [[48, 105, 211], [36, 205, 183], [255, 217, 106]];
  const segment = t < 0.5 ? 0 : 1;
  const position = t < 0.5 ? t * 2 : (t - 0.5) * 2;
  const rgb = stops[segment].map((value, index) => Math.round(value + (stops[segment + 1][index] - value) * position));
  return `#${rgb.map((value) => value.toString(16).padStart(2, "0")).join("")}`;
}

function windowSamples(samples: readonly NumericSample[], window: TimeWindow): NumericSample[] {
  const firstInWindow = lowerBound(samples, window.startNs);
  const endOfWindow = upperBound(samples, window.endNs);
  const first = Math.max(0, firstInWindow - 1);
  const count = endOfWindow - first;
  const kept = Math.min(count, MAX_STREAM_POINTS);
  const output: NumericSample[] = [];
  for (let index = 0; index < kept; index++) {
    const sampleIndex = kept === count ? first + index : first + Math.round(index * (count - 1) / (kept - 1));
    const sample = samples[sampleIndex];
    if (Number.isFinite(sample.elapsed_ns) && Number.isFinite(sample.value)) {
      output.push({ elapsed_ns: sample.elapsed_ns, value: sample.value });
    }
  }
  return output;
}

function lowerBound(samples: readonly NumericSample[], elapsedNs: number): number {
  let low = 0;
  let high = samples.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (samples[middle].elapsed_ns < elapsedNs) low = middle + 1;
    else high = middle;
  }
  return low;
}

function upperBound(samples: readonly NumericSample[], elapsedNs: number): number {
  let low = 0;
  let high = samples.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (samples[middle].elapsed_ns <= elapsedNs) low = middle + 1;
    else high = middle;
  }
  return low;
}

function freshnessLimit(samples: readonly NumericSample[]): number {
  let total = 0;
  let count = 0;
  for (let index = 1; index < samples.length; index++) {
    const interval = samples[index].elapsed_ns - samples[index - 1].elapsed_ns;
    if (interval > 0) { total += interval; count++; }
  }
  if (!count) return SECOND_NS;
  return Math.min(10 * SECOND_NS, Math.max(SECOND_NS, total / count * 3));
}

function downsample(points: ScatterPoint[], maxPoints: number): ScatterPoint[] {
  if (points.length <= maxPoints) return points;
  if (maxPoints === 1) return [points[points.length - 1]];
  return Array.from({ length: maxPoints }, (_, index) => points[Math.round(index * (points.length - 1) / (maxPoints - 1))]);
}
