import type { Sample } from "./types";

export const MAX_LIVE_CSV_SAMPLES = 100_000;

export class LiveCsvBuffer {
  private readonly samples: Array<Sample | undefined>;
  private readonly signalCounts = new Map<string, number>();
  private first = 0;
  private size = 0;
  private dropped = 0;
  readonly capacity: number;

  constructor(capacity = MAX_LIVE_CSV_SAMPLES) {
    if (!Number.isInteger(capacity) || capacity < 1) throw new Error("Live CSV buffer capacity must be positive");
    this.capacity = capacity;
    this.samples = new Array(capacity);
  }

  append(sample: Sample) {
    if (sample.origin !== "live") return;
    if (this.size === this.capacity) {
      const oldest = this.samples[this.first];
      if (oldest) this.count(oldest.signal_id, -1);
      this.samples[this.first] = sample;
      this.first = (this.first + 1) % this.capacity;
      this.dropped++;
    } else {
      this.samples[(this.first + this.size++) % this.capacity] = sample;
    }
    this.count(sample.signal_id, 1);
  }

  hasSignals(signalIds: readonly string[]) {
    return signalIds.some((signalId) => (this.signalCounts.get(signalId) ?? 0) > 0);
  }

  snapshot() {
    return Array.from({ length: this.size }, (_, index) => this.samples[(this.first + index) % this.capacity]!)
      .filter((sample): sample is Sample => sample !== undefined);
  }

  clear() {
    this.samples.fill(undefined);
    this.signalCounts.clear();
    this.first = 0;
    this.size = 0;
    this.dropped = 0;
  }

  get droppedSamples() { return this.dropped; }

  private count(signalId: string, delta: number) {
    const next = (this.signalCounts.get(signalId) ?? 0) + delta;
    if (next > 0) this.signalCounts.set(signalId, next);
    else this.signalCounts.delete(signalId);
  }
}

export function liveCsvSamples(buffer: readonly Sample[], signalIds: readonly string[]): Sample[] {
  const selected = new Set(signalIds);
  return buffer
    .filter((sample) => sample.origin === "live" && selected.has(sample.signal_id))
    .sort((left, right) => left.elapsed_ns - right.elapsed_ns || left.signal_id.localeCompare(right.signal_id));
}
