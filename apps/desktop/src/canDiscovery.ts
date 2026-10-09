import type { CanEventCandidate, CanFrame } from "./types";
import { extractCanSignalRaw } from "./canWorkbench";

export function discoverySeed(candidate: CanEventCandidate, label: string, byteIndex?: number, bit?: number) {
  const byte = byteIndex ?? [...candidate.changed_bytes].sort((a, b) => b.changed_events - a.changed_events)[0]?.byte_index ?? 0;
  return { startBit: byte * 8 + (bit ?? 0), bitLength: bit === undefined ? 8 : 1, name: `${label} · B${byte}${bit === undefined ? "" : `.${bit}`}` };
}

export function changedBits(candidate: CanEventCandidate, byteIndex: number): number[] {
  const byte = candidate.changed_bytes.find((item) => item.byte_index === byteIndex);
  const mask = byte?.transitions.reduce((mask, item) => mask | (item.before === null || item.after === null ? 0 : item.before ^ item.after), 0) ?? 0;
  return Array.from({ length: 8 }, (_, bit) => bit).filter((bit) => mask & (1 << bit));
}

export function canScopePoints(frames: CanFrame[], id: number, extended: boolean, start: number, length: number, order: "intel" | "motorola", signed: boolean, factor: number, offset: number) {
  return frames.filter((frame) => frame.direction === "rx" && frame.can_id === id && frame.extended === extended)
    .slice(-120).flatMap((frame) => {
      const raw = extractCanSignalRaw(frame.data, start, length, order, signed);
      const value = raw === null ? NaN : Number(raw) * factor + offset;
      return Number.isFinite(value) ? [{ time: frame.wall_time_unix_ns, value }] : [];
    });
}

export function scopePath(points: { time: number; value: number }[]): string {
  if (points.length < 2) return "";
  const min = Math.min(...points.map((point) => point.value));
  const max = Math.max(...points.map((point) => point.value));
  const first = points[0].time;
  const span = Math.max(1, points.at(-1)!.time - first);
  return points.map((point, index) => `${index ? "L" : "M"}${(100 * (point.time - first) / span).toFixed(2)},${(max === min ? 25 : 46 - 42 * (point.value - min) / (max - min)).toFixed(2)}`).join(" ");
}
