type TimedValue = { elapsed_ns: number; value: number };

const HISTORY_SPAN_NS = 600_000_000_000;
const MAX_HISTORY_SAMPLES = 60_000;

function boundary(samples: readonly TimedValue[], timeNs: number, after = false): number {
  let low = 0;
  let high = samples.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (after ? samples[middle].elapsed_ns <= timeNs : samples[middle].elapsed_ns < timeNs) low = middle + 1;
    else high = middle;
  }
  return low;
}

// This buffer belongs to the display; recordings and CSV retain their original samples separately.
export function appendLiveGraphSample<T extends TimedValue>(samples: T[], sample: T): T[] {
  if (!Number.isFinite(sample.elapsed_ns) || !Number.isFinite(sample.value)) return samples;
  if (samples.length && sample.elapsed_ns < samples[samples.length - 1].elapsed_ns) return [sample];
  samples.push(sample);
  const first = Math.max(0, boundary(samples, sample.elapsed_ns - HISTORY_SPAN_NS) - 1, samples.length - MAX_HISTORY_SAMPLES);
  if (first) samples.splice(0, first);
  return samples;
}

export function graphWindowSamples<T extends TimedValue>(samples: T[], startNs: number, endNs: number): T[] {
  const first = boundary(samples, startNs);
  const end = boundary(samples, endNs, true);
  if (first >= end) return [];
  return samples.slice(Math.max(0, first - 1), end);
}
