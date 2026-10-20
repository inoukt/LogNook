import type { SignalDefinition } from "./types";

export type TimeWindow = { startNs: number; endNs: number };
export type WorkspaceTheme = "midnight" | "slate-lift" | "steel" | "mist";
const workspaceThemes: WorkspaceTheme[] = ["midnight", "slate-lift", "steel", "mist"];
export type WorkspacePreferences = {
  logSignalIds: string[];
  graphSignalIds: string[];
  graphMode: "overlay" | "separate";
  signalPickerVisible: boolean;
  statusHeight: number;
  theme: WorkspaceTheme;
  highContrast: boolean;
  uiScale: number;
};
export type SignalOption = {
  id: string;
  name: string;
  unit: string;
  description?: string;
  formula?: string;
  pid?: number;
  research?: boolean;
};

const MAX_WORKBENCH_GRAPH_SIGNALS = 500;

function normalizeResearchSignalOption(value: unknown): SignalOption | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const item = value as Record<string, unknown>;
  const validId = typeof item.id === "string"
    && (/^oem:[A-Za-z0-9._:-]{1,192}$/i.test(item.id)
      || /^subaru:[A-Za-z0-9._:-]{1,192}$/i.test(item.id)
      || item.id === "a30_mode02_engine_rpm"
      || /^a30_mode06_tid_[0-9a-f]{2}_mid_[0-9a-f]{2}$/i.test(item.id)
      || item.id === "obdlink_sx_mode02_engine_rpm"
      || /^obdlink_sx_mode06_tid_[0-9a-f]{2}_mid_[0-9a-f]{2}$/i.test(item.id));
  if (!validId || item.research !== true
    || typeof item.name !== "string" || item.name.trim().length === 0 || item.name.length > 192
    || typeof item.unit !== "string" || item.unit.length > 32) return undefined;

  const option: SignalOption = { id: item.id as string, name: item.name, unit: item.unit, research: true };
  if (typeof item.description === "string" && item.description.length <= 512) option.description = item.description;
  if (typeof item.formula === "string" && item.formula.length <= 256) option.formula = item.formula;
  if (typeof item.pid === "number" && Number.isInteger(item.pid) && item.pid >= 0 && item.pid <= 0xffff) option.pid = item.pid;
  return option;
}

export function restoreResearchSignalOptions(serialized: string | null): SignalOption[] {
  let parsed: unknown;
  try { parsed = JSON.parse(serialized ?? "null"); } catch { return []; }
  if (!Array.isArray(parsed)) return [];
  const options = new Map<string, SignalOption>();
  for (const value of parsed) {
    const option = normalizeResearchSignalOption(value);
    if (!option) continue;
    options.delete(option.id);
    options.set(option.id, option);
  }
  return [...options.values()].slice(-MAX_WORKBENCH_GRAPH_SIGNALS);
}

export function upsertResearchSignalOption(options: SignalOption[], value: SignalOption): SignalOption[] {
  const option = normalizeResearchSignalOption(value);
  if (!option) return options;
  return [...options.filter(({ id }) => id !== option.id), option].slice(-MAX_WORKBENCH_GRAPH_SIGNALS);
}

export function restoreWorkbenchGraphSignalIds(serialized: string | null): string[] {
  let parsed: unknown;
  try { parsed = JSON.parse(serialized ?? "null"); } catch { return []; }
  if (!Array.isArray(parsed)) return [];
  const ids = parsed.filter((id): id is string => typeof id === "string" && id.length > 0 && id.length <= 256);
  return [...new Set(ids)].slice(-MAX_WORKBENCH_GRAPH_SIGNALS);
}

export function rememberWorkbenchGraphSignalId(signalIds: string[], signalId: string): string[] {
  if (!signalId || signalId.length > 256) return signalIds;
  return [...new Set([...signalIds.filter((id) => id !== signalId), signalId])].slice(-MAX_WORKBENCH_GRAPH_SIGNALS);
}

export function graphRefreshIntervalMs(scrubbing: boolean): number {
  return scrubbing ? 33 : 100;
}

const defaultSignalColors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

export function restoreWorkspacePreferences(
  serialized: string | null,
  availableSignalIds: string[],
  defaults: WorkspacePreferences,
  maxStatusHeight: number,
  graphOnlySignalIds: string[] = [],
): WorkspacePreferences {
  let saved: Partial<WorkspacePreferences> = {};
  try {
    const value: unknown = JSON.parse(serialized ?? "null");
    if (value && typeof value === "object" && !Array.isArray(value)) saved = value as Partial<WorkspacePreferences>;
  } catch { /* Ignore malformed optional preferences. */ }

  const available = new Set(availableSignalIds);
  const graphOnly = new Set(graphOnlySignalIds);
  const validIds = (value: unknown, fallback: string[], allowed = available) => Array.isArray(value)
    ? [...new Set(value.filter((id): id is string => typeof id === "string" && allowed.has(id)))]
    : [...new Set(fallback.filter((id) => allowed.has(id)))];
  const logSignalIds = validIds(saved.logSignalIds, defaults.logSignalIds);
  const graphSignalIds = validIds(saved.graphSignalIds, defaults.graphSignalIds, new Set([...available, ...graphOnly]))
    .filter((id) => logSignalIds.includes(id) || graphOnly.has(id));
  const statusHeight = Number.isFinite(saved.statusHeight) ? saved.statusHeight! : defaults.statusHeight;

  return {
    logSignalIds,
    graphSignalIds,
    graphMode: saved.graphMode === "separate" || saved.graphMode === "overlay" ? saved.graphMode : defaults.graphMode,
    signalPickerVisible: typeof saved.signalPickerVisible === "boolean" ? saved.signalPickerVisible : defaults.signalPickerVisible,
    statusHeight: Math.max(36, Math.min(maxStatusHeight, statusHeight)),
    theme: workspaceThemes.includes(saved.theme ?? defaults.theme) ? saved.theme ?? defaults.theme : defaults.theme,
    highContrast: typeof saved.highContrast === "boolean" ? saved.highContrast : defaults.highContrast,
    uiScale: Number.isFinite(saved.uiScale) ? Math.max(80, Math.min(120, saved.uiScale!)) : defaults.uiScale,
  };
}

export function signalColor(
  signalId: string,
  index: number,
  overrides: Record<string, string>,
  avoidColors: readonly string[] = [],
): string {
  const color = overrides[signalId];
  if (color && /^#[\da-f]{6}$/i.test(color)) return color;
  const start = ((index % defaultSignalColors.length) + defaultSignalColors.length) % defaultSignalColors.length;
  for (let offset = 0; offset < defaultSignalColors.length; offset++) {
    const candidate = defaultSignalColors[(start + offset) % defaultSignalColors.length];
    if (!avoidColors.includes(candidate)) return candidate;
  }
  return defaultSignalColors[start];
}

export function averageInWindow(samples: Array<{ elapsed_ns: number; value: number }>, range: TimeWindow): number | undefined {
  let total = 0;
  let count = 0;
  for (const sample of samples) {
    if (sample.elapsed_ns < range.startNs || sample.elapsed_ns > range.endNs || !Number.isFinite(sample.value)) continue;
    total += sample.value;
    count++;
  }
  return count ? total / count : undefined;
}

export function sampleTimeRange(samples: Array<{ elapsed_ns: number }>): TimeWindow {
  if (!samples.length) return { startNs: 0, endNs: 0 };
  let startNs = Infinity;
  let endNs = -Infinity;
  for (const sample of samples) {
    startNs = Math.min(startNs, sample.elapsed_ns);
    endNs = Math.max(endNs, sample.elapsed_ns);
  }
  return { startNs, endNs };
}

export function followPlayhead(range: TimeWindow, positionNs: number, durationNs: number): TimeWindow {
  const span = Math.max(0, range.endNs - range.startNs);
  const position = clampSeekPosition(positionNs, durationNs);
  const start = Math.round(position - span / 2);
  return { startNs: start, endNs: start + span };
}

export function positionAtChartX(clientX: number, left: number, width: number, range: TimeWindow): number {
  const ratio = width > 0 ? Math.min(Math.max((clientX - left) / width, 0), 1) : 0;
  return Math.round(range.startNs + (range.endNs - range.startNs) * ratio);
}

export function wheelScrubPosition(positionNs: number, deltaY: number, range: TimeWindow, durationNs: number): number {
  return clampSeekPosition(positionNs + deltaY * Math.max(range.endNs - range.startNs, 0) / 1_000, durationNs);
}

export function scrubTimeWindow(
  positionNs: number,
  deltaY: number,
  range: TimeWindow,
  durationNs: number,
): { positionNs: number; range: TimeWindow } {
  const position = wheelScrubPosition(positionNs, deltaY, range, durationNs);
  return { positionNs: position, range: followPlayhead(range, position, durationNs) };
}

export function filterSignals(signals: SignalOption[], query: string): SignalOption[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return signals;
  return signals.filter((signal) => {
    const pid = signal.pid;
    const hexPid = pid?.toString(16).padStart(2, "0");
    const text = [signal.id, signal.name, signal.description, signal.unit,
      ...(pid === undefined ? [] : [pid, hexPid, `0x${hexPid}`, `PID ${hexPid}`])].join(" ").toLowerCase();
    return text.includes(needle);
  });
}

export function swapPositions<T>(items: T[], from: number, to: number): T[] {
  if (from < 0 || to < 0 || from >= items.length || to >= items.length || from === to) return items;
  const reordered = [...items];
  [reordered[from], reordered[to]] = [reordered[to], reordered[from]];
  return reordered;
}

export function setLogSignal(
  logSignalIds: string[],
  graphSignalIds: string[],
  signalId: string,
  enabled: boolean,
): { logSignalIds: string[]; graphSignalIds: string[] } {
  const update = (ids: string[]) => enabled
    ? ids.includes(signalId) ? ids : [...ids, signalId]
    : ids.filter((id) => id !== signalId);
  return { logSignalIds: update(logSignalIds), graphSignalIds: update(graphSignalIds) };
}

export function setGraphSignal(graphSignalIds: string[], signalId: string, enabled: boolean): string[] {
  if (enabled) return graphSignalIds.includes(signalId) ? graphSignalIds : [...graphSignalIds, signalId];
  return graphSignalIds.filter((id) => id !== signalId);
}

export function chartPoints(
  samples: Array<{ elapsed_ns: number; value: number }>,
  timeStart: number,
  timeEnd: number,
  width = 720,
): string {
  if (!samples.length) return "";

  const renderWidth = Number.isFinite(width) ? Math.max(width, 1) : 1;
  const pixelCount = Math.ceil(renderWidth);
  const bins: Array<{ first: number; last: number; minimum: number; maximum: number } | undefined> = Array(pixelCount);
  let low = Infinity;
  let high = -Infinity;
  const duration = timeEnd - timeStart;
  for (let index = 0; index < samples.length; index++) {
    const sample = samples[index];
    low = Math.min(low, sample.value);
    high = Math.max(high, sample.value);
    const x = duration > 0 ? ((sample.elapsed_ns - timeStart) / duration) * renderWidth : 0;
    const column = Math.min(pixelCount - 1, Math.max(0, Math.floor(x)));
    const bin = bins[column];
    if (bin) {
      bin.last = index;
      if (sample.value < samples[bin.minimum].value) bin.minimum = index;
      if (sample.value > samples[bin.maximum].value) bin.maximum = index;
    } else {
      bins[column] = { first: index, last: index, minimum: index, maximum: index };
    }
  }

  const padding = Math.max((high - low) * 0.12, 1);
  const minimum = low - padding;
  const span = high - low + padding * 2;
  const coordinate = (index: number) => {
    const sample = samples[index];
    const x = duration > 0 ? ((sample.elapsed_ns - timeStart) / duration) * renderWidth : 0;
    const y = 146 - ((sample.value - minimum) / span) * 124;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  };
  const points: string[] = [];
  for (const bin of bins) {
    if (!bin) continue;
    const first = bin.minimum === bin.maximum && bin.first !== bin.last
      ? bin.first
      : Math.min(bin.minimum, bin.maximum);
    const last = bin.minimum === bin.maximum && bin.first !== bin.last
      ? bin.last
      : Math.max(bin.minimum, bin.maximum);
    points.push(coordinate(first));
    if (last !== first) points.push(coordinate(last));
  }
  return points.join(" ");
}

export function formatDurationNs(durationNs: number): string {
  const milliseconds = Math.floor(Math.max(Number.isFinite(durationNs) ? durationNs : 0, 0) / 1e6);
  const padded = (value: number) => String(value).padStart(2, "0");
  const seconds = Math.floor(milliseconds / 1_000);
  const minutes = Math.floor(seconds / 60);
  const clock = `${padded(seconds % 60)}.${String(milliseconds % 1_000).padStart(3, "0")}`;
  return minutes >= 60
    ? `${Math.floor(minutes / 60)}:${padded(minutes % 60)}:${clock}`
    : `${padded(minutes)}:${clock}`;
}

export function formatTimelineNs(timeNs: number): string {
  return timeNs < 0 ? `−${formatDurationNs(-timeNs)}` : formatDurationNs(timeNs);
}

export function clampSeekPosition(positionNs: number, durationNs: number): number {
  const duration = Number.isFinite(durationNs) ? Math.max(durationNs, 0) : 0;
  if (!Number.isFinite(positionNs)) return positionNs > 0 ? duration : 0;
  return Math.round(Math.min(Math.max(positionNs, 0), duration));
}

function clampTimeWindow(range: TimeWindow, durationNs: number): TimeWindow {
  const start = clampSeekPosition(range.startNs, durationNs);
  const end = clampSeekPosition(range.endNs, durationNs);
  return start <= end ? { startNs: start, endNs: end } : { startNs: end, endNs: start };
}

export function zoomTimeWindow(
  range: TimeWindow,
  anchorNs: number,
  factor: number,
  durationNs: number,
): TimeWindow {
  const window = clampTimeWindow(range, durationNs);
  const span = window.endNs - window.startNs;
  const duration = Number.isFinite(durationNs) ? Math.max(durationNs, 0) : 0;
  if (span === 0 || duration === 0 || !Number.isFinite(factor) || factor <= 0) return window;

  const anchor = clampSeekPosition(anchorNs, duration);
  const anchorRatio = (Math.min(Math.max(anchor, window.startNs), window.endNs) - window.startNs) / span;
  const nextSpan = Math.min(duration, Math.max(1, Math.round(span / factor)));
  const startNs = Math.round(Math.min(Math.max(anchor - anchorRatio * nextSpan, 0), duration - nextSpan));
  return { startNs, endNs: Math.min(duration, startNs + nextSpan) };
}

export function panTimeWindow(
  range: TimeWindow,
  deltaNs: number,
  durationNs: number,
): TimeWindow {
  const window = clampTimeWindow(range, durationNs);
  const duration = Number.isFinite(durationNs) ? Math.max(durationNs, 0) : 0;
  const delta = Number.isNaN(deltaNs) ? 0 : deltaNs;
  const shift = Math.round(Math.min(Math.max(delta, -window.startNs), duration - window.endNs));
  return { startNs: Math.round(window.startNs + shift), endNs: Math.round(window.endNs + shift) };
}

export function orderMarkersForTimeline<T extends { elapsed_ns: number }>(markers: T[]): T[] {
  return [...markers].sort((a, b) => a.elapsed_ns - b.elapsed_ns);
}
