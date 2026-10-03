export type GraphView = "line" | "scatter" | "bar" | "heatmap";
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
};
export type GraphPreferencesStorage = Pick<Storage, "getItem" | "setItem">;

export const GRAPH_PREFERENCES_KEY = "lognook.graph-preferences.v1";
export const MAX_GRAPH_SAMPLES = 180;
const MAX_REFERENCE_TRACES = 24;
const DEFAULT_SIGNAL_PREFERENCE: GraphSignalPreference = {
  min: null,
  max: null,
  view: "line",
  lineStyle: "solid",
  lineWidth: 2,
};

export function normalizeGraphPreferences(value: unknown): GraphPreferences {
  if (!value || typeof value !== "object" || Array.isArray(value)) return { signals: {}, reference: null };
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
  return { signals, reference };
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
  const values = [...boundedGraphSamples(samples), ...boundedGraphSamples(referenceSamples)];
  const low = values.length ? Math.min(...values.map((sample) => sample.value)) : 0;
  const high = values.length ? Math.max(...values.map((sample) => sample.value)) : 1;
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
): Array<GraphValueSample & { x: number; y: number }> {
  const duration = timeEnd - timeStart;
  const span = range.max - range.min || 1;
  return boundedGraphSamples(samples).map((sample) => {
    const x = duration > 0 ? ((sample.elapsed_ns - timeStart) / duration) * 720 : 0;
    const y = 146 - ((sample.value - range.min) / span) * 124;
    return {
      ...sample,
      x: Math.min(Math.max(x, 0), 720),
      y: Math.min(Math.max(y, 22), 146),
    };
  });
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

export function nearestGraphSample(samples: GraphValueSample[], timeNs: number): GraphValueSample | undefined {
  let nearest: GraphValueSample | undefined;
  let distance = Infinity;
  for (const sample of boundedGraphSamples(samples)) {
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

export function boundedGraphSamples(samples: GraphValueSample[]): GraphValueSample[] {
  return samples.slice(-MAX_GRAPH_SAMPLES).filter((sample) => Number.isFinite(sample.elapsed_ns) && Number.isFinite(sample.value));
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
