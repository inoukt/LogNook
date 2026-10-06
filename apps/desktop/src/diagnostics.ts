import type { DiagnosticRecord } from "./types";

export function mergeDiagnostics(current: DiagnosticRecord[], incoming: DiagnosticRecord[]): DiagnosticRecord[] {
  return [...new Map([...current, ...incoming].map(record => [record.sequence_id, record])).values()]
    .sort((a, b) => a.sequence_id - b.sequence_id).slice(-500);
}

export function filterDiagnostics(records: DiagnosticRecord[], filters: { adapter: string; operation: string; errorsOnly: boolean }): DiagnosticRecord[] {
  return records.filter(record => (!filters.adapter || (record.adapter_id ?? "") === filters.adapter)
    && (!filters.operation || record.operation === filters.operation)
    && (!filters.errorsOnly || record.phase === "failed"));
}

export function formatDiagnostic(record: DiagnosticRecord) {
  return {
    time: new Date(record.timestamp_unix_ms).toLocaleTimeString(),
    adapter: record.adapter_id ?? "Application",
    operationId: record.operation_id,
    operation: record.operation.replaceAll("_", " "),
    stage: record.stage.replaceAll("_", " "),
    status: record.phase + (record.transient ? " (temporary)" : ""),
    duration: record.duration_ms === null ? "—" : `${record.duration_ms} ms`,
    signals: record.signal_ids.join(", "),
    summaries: (record.signal_summaries ?? []).map(summary =>
      `${summary.signal_id}: ${summary.success_count} successful · ${summary.rate_hz.toFixed(2)} Hz`
      + (summary.latency_ms === null ? "" : ` · ${summary.latency_ms.toFixed(1)} ms`)),
    detail: record.detail,
    omitted: record.omitted_events,
  };
}

export function selectedDiagnosticsRecording(current: string | null, chosen: string | null): string | null {
  return chosen ?? current;
}

export function supportBundleArgs(recordingPath: string | null) {
  return { recordingPath };
}
