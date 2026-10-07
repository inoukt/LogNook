import type { DiagnosticRecord, DiagnosticSignalSummary } from "./types";

export interface CommunicationCheckSignal {
  signalId: string;
  supported: boolean | null;
  attempts: number;
  successes: number;
  errors: number;
}

export interface CommunicationCheckSummary {
  adapterId: string;
  startedAtMs: number;
  finishedAtMs: number;
  status: "no_data" | "healthy" | "issues";
  attempts: number;
  successes: number;
  failures: number;
  medianLatencyMs: number | null;
  p95LatencyMs: number | null;
  signals: CommunicationCheckSignal[];
}

function latestSignalSnapshot(
  records: DiagnosticRecord[],
  adapterId: string,
  fromMs: number,
  throughMs: number,
): DiagnosticRecord | undefined {
  return records
    .filter(record => record.adapter_id === adapterId
      && record.operation === "acquisition"
      && record.timestamp_unix_ms >= fromMs
      && record.timestamp_unix_ms <= throughMs
      && record.signal_summaries?.length)
    .sort((left, right) => right.timestamp_unix_ms - left.timestamp_unix_ms)[0];
}

function delta(current: number, previous: number | undefined): number {
  return previous === undefined || current < previous ? current : current - previous;
}

function percentile(values: number[], fraction: number): number | null {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)];
}

export function appendCommunicationCheckRecords(
  current: DiagnosticRecord[],
  incoming: DiagnosticRecord[],
  adapterId: string,
  startedAtMs: number,
): DiagnosticRecord[] {
  const relevant = incoming.filter(record => record.adapter_id === adapterId
    && record.operation === "acquisition"
    && (record.timestamp_unix_ms < startedAtMs
      ? Boolean(record.signal_summaries?.length)
      : record.stage === "poll" || (record.stage === "pid_read" && record.phase === "failed")
        || Boolean(record.signal_summaries?.length)));
  return [...new Map([...current, ...relevant].map(record => [record.sequence_id, record])).values()]
    .sort((left, right) => left.sequence_id - right.sequence_id)
    .slice(-2_000);
}

function signalDeltas(
  before: DiagnosticSignalSummary[] | undefined,
  after: DiagnosticSignalSummary[] | undefined,
): CommunicationCheckSignal[] {
  if (!after) return [];
  const previous = new Map((before ?? []).map(signal => [signal.signal_id, signal]));
  return after.map(signal => {
    const old = previous.get(signal.signal_id);
    return {
      signalId: signal.signal_id,
      supported: signal.supported ?? null,
      attempts: delta(signal.attempt_count ?? signal.success_count + (signal.error_count ?? 0),
        old?.attempt_count === undefined ? undefined : old.attempt_count),
      successes: delta(signal.success_count, old?.success_count),
      errors: delta(signal.error_count ?? 0, old?.error_count),
    };
  }).sort((left, right) => left.signalId.localeCompare(right.signalId));
}

export function summarizeCommunicationCheck(
  records: DiagnosticRecord[],
  adapterId: string,
  startedAtMs: number,
  finishedAtMs: number,
): CommunicationCheckSummary {
  const polls = records.filter(record => record.adapter_id === adapterId
    && record.operation === "acquisition"
    && record.stage === "poll"
    && record.timestamp_unix_ms >= startedAtMs
    && record.timestamp_unix_ms <= finishedAtMs);
  const outcomes = polls.filter(record => record.phase === "succeeded" || record.phase === "failed");
  const attempts = polls.filter(record => record.phase === "started").length;
  const successes = outcomes.filter(record => record.phase === "succeeded").length;
  const failures = outcomes.filter(record => record.phase === "failed").length;
  const latencies = outcomes.flatMap(record => record.duration_ms === null ? [] : [record.duration_ms]);
  const pidReadFailed = records.some(record => record.adapter_id === adapterId
    && record.operation === "acquisition"
    && record.stage === "pid_read"
    && record.phase === "failed"
    && record.timestamp_unix_ms >= startedAtMs
    && record.timestamp_unix_ms <= finishedAtMs);
  const before = latestSignalSnapshot(records, adapterId, Number.NEGATIVE_INFINITY, startedAtMs - 1);
  const after = latestSignalSnapshot(records, adapterId, startedAtMs, finishedAtMs);
  const signals = signalDeltas(before?.signal_summaries ?? undefined, after?.signal_summaries ?? undefined);
  const signalFailed = signals.some(signal => signal.errors > 0
    || (signal.attempts > 0 && signal.supported === true && signal.successes === 0));

  return {
    adapterId,
    startedAtMs,
    finishedAtMs,
    status: attempts === 0 && !pidReadFailed && !signalFailed ? "no_data"
      : failures === 0 && !pidReadFailed && successes > 0
        && signals.every(signal => signal.errors === 0 && (signal.supported !== true || signal.successes > 0))
        ? "healthy"
        : "issues",
    attempts,
    successes,
    failures,
    medianLatencyMs: percentile(latencies, 0.5),
    p95LatencyMs: percentile(latencies, 0.95),
    signals,
  };
}
