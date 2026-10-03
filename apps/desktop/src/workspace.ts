import type { SignalDefinition } from "./types";

export type TimeWindow = { startNs: number; endNs: number };
export type WorkspacePreferences = {
  logSignalIds: string[];
  graphSignalIds: string[];
  graphMode: "overlay" | "separate";
  signalPickerVisible: boolean;
  statusHeight: number;
};
export type SignalOption = {
  id: string;
  name: string;
  unit: string;
  description?: string;
  pid?: number;
};

const defaultSignalColors = ["#56c8bd", "#6ca8ff", "#ffb65c", "#d28bff", "#ff7f86", "#a8d878", "#f0d45c"];

export function restoreWorkspacePreferences(
  serialized: string | null,
  availableSignalIds: string[],
  defaults: WorkspacePreferences,
  maxStatusHeight: number,
): WorkspacePreferences {
  let saved: Partial<WorkspacePreferences> = {};
  try {
    const value: unknown = JSON.parse(serialized ?? "null");
    if (value && typeof value === "object" && !Array.isArray(value)) saved = value as Partial<WorkspacePreferences>;
  } catch { /* Ignore malformed optional preferences. */ }

  const available = new Set(availableSignalIds);
  const validIds = (value: unknown, fallback: string[]) => Array.isArray(value)
    ? [...new Set(value.filter((id): id is string => typeof id === "string" && available.has(id)))]
    : [...new Set(fallback.filter((id) => available.has(id)))];
  const logSignalIds = validIds(saved.logSignalIds, defaults.logSignalIds);
  const graphSignalIds = validIds(saved.graphSignalIds, defaults.graphSignalIds).filter((id) => logSignalIds.includes(id));
  const statusHeight = Number.isFinite(saved.statusHeight) ? saved.statusHeight! : defaults.statusHeight;

  return {
    logSignalIds,
    graphSignalIds,
    graphMode: saved.graphMode === "separate" || saved.graphMode === "overlay" ? saved.graphMode : defaults.graphMode,
    signalPickerVisible: typeof saved.signalPickerVisible === "boolean" ? saved.signalPickerVisible : defaults.signalPickerVisible,
    statusHeight: Math.max(36, Math.min(maxStatusHeight, statusHeight)),
  };
}

export function signalColor(signalId: string, index: number, overrides: Record<string, string>): string {
  const color = overrides[signalId];
  return color && /^#[\da-f]{6}$/i.test(color)
    ? color
    : defaultSignalColors[((index % defaultSignalColors.length) + defaultSignalColors.length) % defaultSignalColors.length];
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
