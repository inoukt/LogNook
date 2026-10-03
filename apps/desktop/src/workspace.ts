import type { SignalDefinition } from "./types";

export type TimeWindow = { startNs: number; endNs: number };
export type SignalOption = {
  id: string;
  name: string;
  unit: string;
  description?: string;
  pid?: number;
};

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

export function chartPoints(
  samples: Array<{ elapsed_ns: number; value: number }>,
  timeStart: number,
  timeEnd: number,
  width = 720,
): string {
  if (!samples.length) return "";
  const values = samples.map((sample) => sample.value);
  const low = Math.min(...values);
  const high = Math.max(...values);
  const padding = Math.max((high - low) * 0.12, 1);
  const minimum = low - padding;
  const span = high - low + padding * 2;
  const duration = timeEnd - timeStart;
  return samples.map((sample) => {
    const x = duration > 0 ? ((sample.elapsed_ns - timeStart) / duration) * width : 0;
    const y = 146 - ((sample.value - minimum) / span) * 124;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
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
