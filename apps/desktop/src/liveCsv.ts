import type { Sample } from "./types";

export function liveCsvSamples(series: Record<string, readonly Sample[]>, signalIds: readonly string[]): Sample[] {
  return [...new Set(signalIds)]
    .flatMap((signalId) => series[signalId] ?? [])
    .filter((sample) => sample.origin === "live")
    .sort((left, right) => left.elapsed_ns - right.elapsed_ns || left.signal_id.localeCompare(right.signal_id));
}
