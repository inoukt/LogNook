import type { SignalDefinition } from "./types";

export function filterSignals(signals: SignalDefinition[], query: string): SignalDefinition[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return signals;
  return signals.filter((signal) => {
    const hexPid = signal.pid.toString(16).padStart(2, "0");
    const text = [signal.id, signal.name, signal.description, signal.unit,
      signal.pid, hexPid, `0x${hexPid}`, `PID ${hexPid}`].join(" ").toLowerCase();
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
    const x = duration > 0 ? ((sample.elapsed_ns - timeStart) / duration) * width : width;
    const y = 146 - ((sample.value - minimum) / span) * 124;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
}
