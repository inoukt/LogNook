export type GraphView = "line" | "scatter" | "bar" | "heatmap";
export type GraphType = "time_series" | "xy_scatter";
export type GraphLineStyle = "solid" | "dashed" | "dotted";
export type GraphValueSample = { elapsed_ns: number; value: number };
export type GraphSignalPreference = {
  min: number | null;
  max: number | null;
  view: GraphView;
  lineStyle: GraphLineStyle;
  lineWidth: number;
};
export type GraphReferenceTrace = {
  signalId: string;
  name: string;
  unit: string;
  color: string;
  samples: GraphValueSample[];
};
export type GraphPreferences = {
  signals: Record<string, GraphSignalPreference>;
  reference: { capturedAt: number; traces: GraphReferenceTrace[] } | null;
  liveWindowSeconds: number;
  liveZoomFactor: number;
  graphType: GraphType;
  scatter: { xSignalId: string | null; ySignalId: string | null; colorSignalId: string | null };
};
export type GraphPreferencesStorage = Pick<Storage, "getItem" | "setItem">;

export const GRAPH_PREFERENCES_KEY = "lognook.graph-preferences.v1";
export const MAX_GRAPH_SAMPLES = 180;
export const SESSION_GRAPH_BUCKETS = 360;
export const MAX_SESSION_GRAPH_SAMPLES = SESSION_GRAPH_BUCKETS * 4;
export const LIVE_GRAPH_WINDOW_NS = 18_000_000_000;
export const MIN_LIVE_WINDOW_SECONDS = 0.5;
export const MAX_LIVE_WINDOW_SECONDS = 600;
const MAX_REFERENCE_TRACES = 24;
const DEFAULT_SIGNAL_PREFERENCE: GraphSignalPreference = {
  min: null,
  max: null,
  view: "line",
  lineStyle: "solid",
  lineWidth: 2,
};

export function defaultGraphSignalPreference(isDerived = false): GraphSignalPreference {
  return { ...DEFAULT_SIGNAL_PREFERENCE, lineStyle: isDerived ? "dotted" : "solid" };
}

export function formatDerivedExpression(expression?: string): string {
  const formula = expression?.trim();
  return formula ? `(${formula})` : "";
}

export function migrateDerivedGraphStyles(
  preferences: GraphPreferences,
  derivedSignalIds: readonly string[],
): GraphPreferences {
  const signals = { ...preferences.signals };
  let changed = false;
  for (const id of derivedSignalIds) {
    const preference = signals[id];
    if (!preference || preference.lineStyle === "dotted") continue;
    signals[id] = { ...preference, lineStyle: "dotted" };
    changed = true;
  }
  return changed ? normalizeGraphPreferences({ ...preferences, signals }) : preferences;
}

export function graphRangeSamples(
  samples: GraphValueSample[],
  sourceSamples: GraphValueSample[],
  includeSource: boolean,
): GraphValueSample[] {
  return includeSource ? [...samples, ...sourceSamples] : samples;
}

export function zoomGraphRange(
  range: { min: number; max: number },
  direction: "in" | "out",
): { min: number; max: number } {
  const limit = 1e12;
  let min = Number.isFinite(range.min) ? Math.min(Math.max(range.min, -limit), limit) : -1;
  let max = Number.isFinite(range.max) ? Math.min(Math.max(range.max, -limit), limit) : 1;
  if (min > max) [min, max] = [max, min];
  const center = min + (max - min) / 2;
  const span = max > min ? max - min : Math.max(2, Math.abs(center) * 0.02);
  const minimumSpan = Math.max(2e-12, Math.abs(center) * Number.EPSILON * 8);
  const nextSpan = Math.min(limit * 2, Math.max(minimumSpan, direction === "in" ? span / 1.25 : span * 1.25));
  const nextMin = Math.min(Math.max(center - nextSpan / 2, -limit), limit - nextSpan);
  return { min: nextMin, max: nextMin + nextSpan };
}

export function resolveFocusedGraphSignalId(signalIds: readonly string[], focusedSignalId: string | null): string | null {
  return focusedSignalId && signalIds.includes(focusedSignalId) ? focusedSignalId : signalIds[0] ?? null;
}

export function graphZoomAvailability(input: {
  graphType: GraphType;
  hasScatterAxes: boolean;
  hasScatterData: boolean;
  hasLiveXData: boolean;
  hasReplayWindow: boolean;
  canZoomReplay: boolean;
  canResetReplay: boolean;
  canResetLive: boolean;
  hasFocusedTraceData: boolean;
  hasManualBounds: boolean;
}): { x: boolean; y: boolean; auto: boolean } {
  if (input.graphType === "xy_scatter") {
    const hasAxes = input.hasScatterAxes;
    const hasData = hasAxes && input.hasScatterData;
    return { x: hasData, y: hasData, auto: hasAxes && (hasData || input.hasManualBounds) };
  }
  const canZoomReplay = input.hasReplayWindow && input.canZoomReplay;
  return {
    x: canZoomReplay || input.hasLiveXData,
    y: input.hasFocusedTraceData,
    auto: input.hasReplayWindow && input.canResetReplay
      || input.canResetLive || input.hasManualBounds || input.hasFocusedTraceData,
  };
}

export function zoomLiveWindowFactor(
  baseSeconds: number,
  factor: number,
  direction: "in" | "out",
): number {
  const base = Math.min(Math.max(Number.isFinite(baseSeconds) ? baseSeconds : 18, MIN_LIVE_WINDOW_SECONDS), MAX_LIVE_WINDOW_SECONDS);
  const minFactor = MIN_LIVE_WINDOW_SECONDS / base;
  const maxFactor = MAX_LIVE_WINDOW_SECONDS / base;
  const currentFactor = Math.min(Math.max(Number.isFinite(factor) && factor > 0 ? factor : 1, minFactor), maxFactor);
  const currentSpan = base * currentFactor;
  const nextSpan = Math.min(Math.max(currentSpan * (direction === "in" ? 1 / 1.25 : 1.25), MIN_LIVE_WINDOW_SECONDS), MAX_LIVE_WINDOW_SECONDS);
  return nextSpan / base;
}

export function normalizeGraphPreferences(value: unknown): GraphPreferences {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { signals: {}, reference: null, liveWindowSeconds: LIVE_GRAPH_WINDOW_NS / 1_000_000_000,
      liveZoomFactor: 1, graphType: "time_series", scatter: { xSignalId: null, ySignalId: null, colorSignalId: null } };
  }
  const saved = value as Record<string, unknown>;
  const signals: Record<string, GraphSignalPreference> = {};
  if (saved.signals && typeof saved.signals === "object" && !Array.isArray(saved.signals)) {
    for (const [signalId, raw] of Object.entries(saved.signals as Record<string, unknown>).slice(0, 128)) {
      if (!signalId || signalId.length > 160 || !raw || typeof raw !== "object" || Array.isArray(raw)) continue;
      const input = raw as Record<string, unknown>;
      let min = finiteRange(input.min);
      let max = finiteRange(input.max);
      if (min !== null && max !== null && min >= max) [min, max] = [null, null];
      signals[signalId] = {
        min,
        max,
        view: isGraphView(input.view) ? input.view : DEFAULT_SIGNAL_PREFERENCE.view,
        lineStyle: isGraphLineStyle(input.lineStyle) ? input.lineStyle : DEFAULT_SIGNAL_PREFERENCE.lineStyle,
        lineWidth: Number.isFinite(input.lineWidth)
          ? Math.round(Math.min(Math.max(input.lineWidth as number, 1), 5) * 2) / 2
          : DEFAULT_SIGNAL_PREFERENCE.lineWidth,
      };
    }
  }

  const reference = normalizeReference(saved.reference);
  const liveWindowSeconds = typeof saved.liveWindowSeconds === "number" && Number.isFinite(saved.liveWindowSeconds)
    ? Math.round(Math.min(Math.max(saved.liveWindowSeconds, MIN_LIVE_WINDOW_SECONDS), MAX_LIVE_WINDOW_SECONDS) * 2) / 2
    : LIVE_GRAPH_WINDOW_NS / 1_000_000_000;
  const liveZoomFactor = typeof saved.liveZoomFactor === "number" && Number.isFinite(saved.liveZoomFactor) && saved.liveZoomFactor > 0
    ? Math.min(Math.max(saved.liveZoomFactor, MIN_LIVE_WINDOW_SECONDS / liveWindowSeconds), MAX_LIVE_WINDOW_SECONDS / liveWindowSeconds)
    : 1;
  const scatter = saved.scatter && typeof saved.scatter === "object" && !Array.isArray(saved.scatter)
    ? saved.scatter as Record<string, unknown> : {};
  const signalId = (id: unknown) => typeof id === "string" && id.length > 0 && id.length <= 160 ? id : null;
  return { signals, reference, liveWindowSeconds, liveZoomFactor,
    graphType: saved.graphType === "xy_scatter" ? "xy_scatter" : "time_series",
    scatter: { xSignalId: signalId(scatter.xSignalId), ySignalId: signalId(scatter.ySignalId), colorSignalId: signalId(scatter.colorSignalId) } };
}

export function loadGraphPreferences(storage?: GraphPreferencesStorage): GraphPreferences {
  try {
    const target = storage ?? (typeof localStorage === "undefined" ? undefined : localStorage);
    const serialized = target?.getItem(GRAPH_PREFERENCES_KEY);
    return normalizeGraphPreferences(serialized ? JSON.parse(serialized) as unknown : null);
  } catch {
    return normalizeGraphPreferences(null);
  }
}

export function saveGraphPreferences(preferences: GraphPreferences, storage?: GraphPreferencesStorage): void {
  try {
    const target = storage ?? (typeof localStorage === "undefined" ? undefined : localStorage);
    target?.setItem(GRAPH_PREFERENCES_KEY, JSON.stringify(normalizeGraphPreferences(preferences)));
    if (typeof window !== "undefined") window.dispatchEvent(new Event("lognook-graph-preferences-changed"));
  } catch { /* Browser storage is optional. */ }
}

export function captureGraphReference(
  preferences: GraphPreferences,
  traces: Array<Pick<GraphReferenceTrace, "signalId" | "name" | "unit" | "samples"> & { color?: string }>,
  capturedAt = Date.now(),
): GraphPreferences {
  return normalizeGraphPreferences({
    ...preferences,
    reference: {
      capturedAt,
      traces: traces.slice(0, MAX_REFERENCE_TRACES).map((trace) => ({
        ...trace,
        color: trace.color ?? "#56c8bd",
        samples: trace.samples.slice(-MAX_GRAPH_SAMPLES).map(({ elapsed_ns, value }) => ({ elapsed_ns, value })),
      })),
    },
  });
}

export function graphValueRange(
  samples: GraphValueSample[],
  referenceSamples: GraphValueSample[],
  preference: Pick<GraphSignalPreference, "min" | "max"> = { min: null, max: null },
): { min: number; max: number } {
  let low = Infinity;
  let high = -Infinity;
  for (const series of [samples, referenceSamples]) {
    for (const sample of series) {
      if (!Number.isFinite(sample.elapsed_ns) || !Number.isFinite(sample.value)) continue;
      low = Math.min(low, sample.value);
      high = Math.max(high, sample.value);
    }
  }
  if (low === Infinity) { low = 0; high = 1; }
  const padding = Math.max((high - low) * 0.12, 1);
  const manualMin = finiteRange(preference.min);
  const manualMax = finiteRange(preference.max);
  let min = manualMin ?? low - padding;
  let max = manualMax ?? high + padding;
  if (min >= max) {
    if (manualMin !== null && manualMax === null) max = min + Math.max(1, Math.abs(min) * 0.01);
    else if (manualMax !== null && manualMin === null) min = max - Math.max(1, Math.abs(max) * 0.01);
    else { min = low - padding; max = high + padding; }
  }
  return { min, max };
}

export function graphCoordinates(
  samples: GraphValueSample[],
  timeStart: number,
  timeEnd: number,
  range: { min: number; max: number },
  maxSamples = MAX_GRAPH_SAMPLES,
): Array<GraphValueSample & { x: number; y: number }> {
  const duration = timeEnd - timeStart;
  const span = range.max - range.min || 1;
  return boundedGraphSamples(samples, maxSamples).map((sample) => {
    const x = duration > 0 ? ((sample.elapsed_ns - timeStart) / duration) * 720 : 0;
    const y = 146 - ((sample.value - range.min) / span) * 124;
    return {
      ...sample,
      x: Math.min(Math.max(x, 0), 720),
      y: Math.min(Math.max(y, 22), 146),
    };
  });
}

export function liveGraphTimeWindow(nowNs: number, spanNs = LIVE_GRAPH_WINDOW_NS): { startNs: number; endNs: number } {
  const span = Number.isFinite(spanNs) && spanNs > 0 ? spanNs : LIVE_GRAPH_WINDOW_NS;
  return { startNs: Math.max(0, nowNs - span), endNs: nowNs };
}

export function isLiveTraceStale(
  samples: readonly { elapsed_ns: number }[],
  nowNs: number,
): boolean {
  const recent: number[] = [];
  for (let index = samples.length - 1; index >= 0 && recent.length < 9; index--) {
    const timestamp = samples[index].elapsed_ns;
    if (Number.isFinite(timestamp)) recent.push(timestamp);
  }
  recent.reverse();
  const last = recent.at(-1);
  if (last === undefined || !Number.isFinite(nowNs)) return false;
  const intervals = recent.slice(1)
    .map((timestamp, index) => timestamp - recent[index])
    .filter((interval) => interval > 0)
    .sort((left, right) => left - right);
  const median = intervals.length
    ? intervals[Math.floor(intervals.length / 2)]
    : 0;
  return nowNs - last > Math.max(5_000_000_000, median * 3);
}

export function holdLastPointAtRightEdge<T extends { x: number }>(points: T[]): T[] {
  const last = points.at(-1);
  return last && last.x < 720 ? [...points, { ...last, x: 720 }] : points;
}

export function mapReferenceSamples(samples: GraphValueSample[], timeStart: number, timeEnd: number): GraphValueSample[] {
  const bounded = boundedGraphSamples(samples);
  if (!bounded.length) return [];
  let sourceStart = Infinity;
  let sourceEnd = -Infinity;
  for (const sample of bounded) {
    sourceStart = Math.min(sourceStart, sample.elapsed_ns);
    sourceEnd = Math.max(sourceEnd, sample.elapsed_ns);
  }
  const sourceDuration = sourceEnd - sourceStart;
  return bounded.map((sample) => ({
    elapsed_ns: sourceDuration > 0
      ? timeStart + ((sample.elapsed_ns - sourceStart) / sourceDuration) * (timeEnd - timeStart)
      : timeStart,
    value: sample.value,
  }));
}

export function nearestGraphSample(
  samples: GraphValueSample[],
  timeNs: number,
  maxSamples = MAX_GRAPH_SAMPLES,
): GraphValueSample | undefined {
  let nearest: GraphValueSample | undefined;
  let distance = Infinity;
  for (const sample of boundedGraphSamples(samples, maxSamples)) {
    const nextDistance = Math.abs(sample.elapsed_ns - timeNs);
    if (nextDistance < distance) {
      nearest = sample;
      distance = nextDistance;
    }
  }
  return nearest;
}

export function graphHeatmapRow(index: number, count: number): { y: number; height: number } {
  const rows = Number.isFinite(count) ? Math.max(1, Math.floor(count)) : 1;
  const row = Number.isFinite(index) ? Math.min(Math.max(Math.floor(index), 0), rows - 1) : 0;
  const height = 124 / rows;
  return { y: 22 + row * height, height };
}

export function boundedGraphSamples(samples: GraphValueSample[], maxSamples = MAX_GRAPH_SAMPLES): GraphValueSample[] {
  const limit = Number.isFinite(maxSamples) ? Math.max(1, Math.floor(maxSamples)) : MAX_GRAPH_SAMPLES;
  const valid = samples.filter((sample) => Number.isFinite(sample.elapsed_ns) && Number.isFinite(sample.value));
  if (valid.length <= limit) return valid;
  if (limit === 1) return valid.slice(-1);
  if (limit < 4) return Array.from({ length: limit }, (_, index) => valid[Math.round(index * (valid.length - 1) / (limit - 1))]);

  const selected = new Set([0, valid.length - 1]);
  const buckets = Math.floor((limit - 2) / 2);
  for (let bucket = 0; bucket < buckets; bucket++) {
    const first = 1 + Math.floor(bucket * (valid.length - 2) / buckets);
    const end = 1 + Math.floor((bucket + 1) * (valid.length - 2) / buckets);
    let low = first;
    let high = first;
    for (let index = first + 1; index < end; index++) {
      if (valid[index].value < valid[low].value) low = index;
      if (valid[index].value >= valid[high].value) high = index;
    }
    selected.add(low);
    selected.add(high);
  }
  return [...selected].sort((left, right) => left - right).map((index) => valid[index]);
}

function normalizeReference(value: unknown): GraphPreferences["reference"] {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const saved = value as Record<string, unknown>;
  if (!Number.isFinite(saved.capturedAt) || !Array.isArray(saved.traces)) return null;
  const traces: GraphReferenceTrace[] = [];
  for (const raw of saved.traces.slice(0, MAX_REFERENCE_TRACES)) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) continue;
    const trace = raw as Record<string, unknown>;
    if (typeof trace.signalId !== "string" || !trace.signalId || trace.signalId.length > 160
      || typeof trace.name !== "string" || trace.name.length > 160
      || typeof trace.unit !== "string" || trace.unit.length > 32
      || !Array.isArray(trace.samples)) continue;
    const samples = trace.samples.slice(-MAX_GRAPH_SAMPLES).flatMap((sample) => {
      if (!sample || typeof sample !== "object" || Array.isArray(sample)) return [];
      const point = sample as Record<string, unknown>;
      return Number.isFinite(point.elapsed_ns) && Number.isFinite(point.value)
        ? [{ elapsed_ns: point.elapsed_ns as number, value: point.value as number }]
        : [];
    });
    traces.push({
      signalId: trace.signalId,
      name: trace.name,
      unit: trace.unit,
      color: typeof trace.color === "string" && /^#[\da-f]{6}$/i.test(trace.color) ? trace.color : "#56c8bd",
      samples,
    });
  }
  return { capturedAt: saved.capturedAt as number, traces };
}

function finiteRange(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 1e12 ? value : null;
}

function isGraphView(value: unknown): value is GraphView {
  return value === "line" || value === "scatter" || value === "bar" || value === "heatmap";
}

function isGraphLineStyle(value: unknown): value is GraphLineStyle {
  return value === "solid" || value === "dashed" || value === "dotted";
}
