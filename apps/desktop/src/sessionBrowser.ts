export type RecordingVehicleContextSummary = {
  scope: "live" | "simulator" | "can_only";
  vin_digest: string | null;
  masked_vin: string | null;
  identity_source: "report_ecu" | "same_generation_recognition" | "manual_profile" | "conflict" | "unassigned";
  vin_conflict: boolean;
  has_full_vin: boolean;
};

export type BrowserRecording = {
  path: string;
  name: string;
  modified_unix_ms: number;
  sample_count: number;
  vehicle_context: RecordingVehicleContextSummary | null;
};

export type BrowserRecordingRow = readonly [
  path: string,
  name: string,
  modified_unix_ms: number,
  sample_count: number,
] | readonly [
  path: string,
  name: string,
  modified_unix_ms: number,
  sample_count: number,
  vehicle_context: RecordingVehicleContextSummary | null,
];

export function recordingFromRow(row: BrowserRecordingRow): BrowserRecording {
  const [path, name, modified_unix_ms, sample_count, vehicle_context = null] = row;
  return { path, name, modified_unix_ms, sample_count, vehicle_context };
}

export type HistoryGroup = {
  id: string;
  scope: RecordingVehicleContextSummary["scope"];
  vin_digest: string | null;
  masked_vin: string | null;
  identity_source: RecordingVehicleContextSummary["identity_source"];
  vin_conflict: boolean;
  manual_association: boolean;
  reportRows: HistoryReportSummary[];
  recordings: BrowserRecording[];
};

export type HistoryReportSummary = {
  report_id: string;
  started_at_unix_ms: number;
  scope: RecordingVehicleContextSummary["scope"];
  vin_digest: string | null;
  masked_vin: string | null;
  identity_source: RecordingVehicleContextSummary["identity_source"];
  vin_conflict: boolean;
  ecu_ids: string[];
  dtc_counts: { stored: number | null; pending: number | null; permanent: number | null };
  mode06_read_count: number;
  evidence_status: "available" | "missing" | "changed" | "malformed";
};

export type HistoryReportAssociation = { report_id: string; scope: RecordingVehicleContextSummary["scope"]; vin_digest: string };

export function groupVehicleHistory(
  reports: readonly HistoryReportSummary[],
  recordings: readonly BrowserRecording[],
  manualAssociations: readonly HistoryReportAssociation[] = [],
): HistoryGroup[] {
  const groups = new Map<string, HistoryGroup>();
  const groupFor = (
    id: string, scope: HistoryGroup["scope"], vin_digest: string | null, masked_vin: string | null,
    identity_source: HistoryGroup["identity_source"], vin_conflict: boolean, manual_association = false,
  ): HistoryGroup => {
    let group = groups.get(id);
    if (!group) {
      group = { id, scope, vin_digest, masked_vin, identity_source, vin_conflict, manual_association, reportRows: [], recordings: [] };
      groups.set(id, group);
    } else if (manual_association) group.manual_association = true;
    return group;
  };

  for (const report of reports) {
    const conflict = report.vin_conflict || report.identity_source === "conflict";
    const association = !conflict
      ? manualAssociations.find((item) => item.report_id === report.report_id)
      : undefined;
    if (association) {
      groupFor(`vehicle:${association.scope}:${association.vin_digest}`, association.scope, association.vin_digest,
        report.masked_vin, "manual_profile", false, true).reportRows.push(report);
    } else if (conflict) {
      groupFor(`conflict:${report.scope}:${report.report_id}`, report.scope, null, null, "conflict", true).reportRows.push(report);
    } else if (report.vin_digest) {
      groupFor(`vehicle:${report.scope}:${report.vin_digest}`, report.scope, report.vin_digest, report.masked_vin,
        report.identity_source, false).reportRows.push(report);
    } else {
      groupFor(`unassigned:${report.scope}:${report.report_id}`, report.scope, null, null, "unassigned", false).reportRows.push(report);
    }
  }

  for (const recording of recordings) {
    const context = recording.vehicle_context;
    if (context?.vin_conflict || context?.identity_source === "conflict") {
      groupFor(`conflict:${context.scope}:recording:${recording.path}`, context.scope, null, null, "conflict", true).recordings.push(recording);
    } else if (context?.vin_digest) {
      groupFor(`vehicle:${context.scope}:${context.vin_digest}`, context.scope, context.vin_digest, context.masked_vin,
        context.identity_source, false).recordings.push(recording);
    } else {
      const scope = context?.scope ?? "live";
      groupFor(`unassigned:${scope}:recording:${recording.path}`, scope, null, null, "unassigned", false).recordings.push(recording);
    }
  }
  return [...groups.values()].sort((left, right) => left.scope.localeCompare(right.scope) || left.id.localeCompare(right.id));
}

export function recordingPrivacyLabel(hasFullVin: boolean | null): string {
  if (hasFullVin === true) return "Native session metadata contains a full VIN.";
  if (hasFullVin === false) return "Native session metadata has no full VIN.";
  return "Vehicle context is unavailable; this may be an older session.";
}

export function csvVehiclePrivacyLabel(): string {
  return "CSV export does not add VIN columns.";
}

export function recordingContextBoundaryLabel(context: { context_from_elapsed_ns: number; context_from_sample_index: number }): string {
  const elapsed = context.context_from_elapsed_ns;
  const seconds = Math.trunc(Math.abs(elapsed) / 1_000_000_000);
  const nanos = String(Math.abs(Math.trunc(elapsed % 1_000_000_000))).padStart(9, "0");
  const sign = elapsed < 0 ? "-" : "";
  return `Vehicle context starts at row index ${context.context_from_sample_index} (sample ${context.context_from_sample_index + 1}) and ${sign}${seconds}.${nanos} s.`;
}

export function recentRecordingsFirst(recordings: readonly BrowserRecording[]): BrowserRecording[] {
  return [...recordings].sort((left, right) =>
    right.modified_unix_ms - left.modified_unix_ms || left.name.localeCompare(right.name));
}

export async function openAndActivateRecording<T>(
  load: () => Promise<T>,
  activate: (recording: T) => void | Promise<void>,
): Promise<void> {
  await activate(await load());
}

export function formatRecordingTime(unixMs: number): string {
  const date = new Date(unixMs);
  return Number.isNaN(date.getTime())
    ? "Unknown time"
    : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}
