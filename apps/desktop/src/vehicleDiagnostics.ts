import { invoke } from "@tauri-apps/api/core";
import type { ProfileScope, VehicleIdentitySnapshot, VehicleIdentitySource } from "./vehicleProfiles";

export const VEHICLE_DIAGNOSTICS_ADAPTERS = [
  "Xtool A30/A30M Bluetooth SPP",
  "ELM327 Bluetooth SPP",
  "OBDLink SX",
  "OpenPort 2.0",
  "OpenPort 2 J2534",
  "Candlelight GS-USB OBD-II",
  "simulator",
] as const;

export type DiagnosticOutcome =
  | "positive"
  | "negative_response"
  | "response_pending"
  | "temporarily_unavailable"
  | "session_required"
  | "security_required"
  | "no_response"
  | "malformed_response"
  | "wrong_responder"
  | "transport_error"
  | "unsupported";

export type DiagnosticReadKind =
  | "stored_dtcs"
  | "pending_dtcs"
  | "permanent_dtcs"
  | "vin"
  | "calibration_id"
  | "ecu_name"
  | "mode06_bitmap"
  | "mode06_monitor"
  | "clear_dtcs";

export type DiagnosticCapabilities = {
  dtc_snapshot: boolean;
  identity: boolean;
  mode06: boolean;
  clear_dtcs: boolean;
};

export type DiagnosticValue =
  | { type: "dtcList"; payload: { codes: string[] } }
  | { type: "identity"; payload: { pid: number; label: string; value: string } }
  | { type: "mode06Bitmap"; payload: { start: number; test_ids: number[]; has_more: boolean } }
  | {
      type: "mode06Monitor";
      payload: {
        test_id: number;
        component_id: number;
        uasid: number;
        value: number;
        minimum: number;
        maximum: number;
      };
    }
  | { type: "dtcClearAcknowledgement" };

export type DiagnosticRead = {
  kind: DiagnosticReadKind;
  service: number;
  identifier: number | null;
  request_payload: number[];
  route: string;
  exact_adapter_command: string;
  adapter_started_at_unix_ms: number;
  latency_ms: number;
  ecu_id: string | null;
  outcome: DiagnosticOutcome;
  negative_response_code: number | null;
  response_payload: number[];
  values: DiagnosticValue[];
  detail: string | null;
  raw_response: string;
};

export type DiagnosticReport = {
  report_id?: string | null;
  connection_generation?: number | null;
  vehicle_context?: VehicleIdentitySnapshot | null;
  adapter: {
    stable_id: string;
    hardware_family: string;
    firmware: string | null;
    path: string | null;
  };
  protocol: string;
  started_at_unix_ms: number;
  operation_kind: "snapshot" | "clear_dtcs";
  request: { mode06_test_ids: number[] };
  reads: DiagnosticRead[];
  route_restore_error: string | null;
  evidence_saved: boolean;
  evidence_save_error: string | null;
};

export type DiagnosticDtcCounts = { stored: number | null; pending: number | null; permanent: number | null };
export type DiagnosticHistoryFilter = {
  scope?: ProfileScope | null;
  vin_digest?: string | null;
  ecu_id?: string | null;
  from_unix_ms?: number | null;
  to_unix_ms?: number | null;
  page_size?: number | null;
  cursor?: string | null;
};
export type DiagnosticHistoryRow = {
  report_id: string;
  started_at_unix_ms: number;
  scope: ProfileScope;
  vin_digest: string | null;
  masked_vin: string | null;
  identity_source: VehicleIdentitySource;
  vin_conflict: boolean;
  ecu_ids: string[];
  dtc_counts: DiagnosticDtcCounts;
  mode06_read_count: number;
  evidence_status: HistoryEvidenceStatus;
};
export type DiagnosticHistoryPage = { rows: DiagnosticHistoryRow[]; next_cursor: string | null };
export type HistoryIndexState = "idle" | "indexing" | "complete" | "cancelled" | "failed";
export type HistoryIndexStatus = {
  state: HistoryIndexState;
  indexed_reports: number;
  scanned_bytes: number;
  total_bytes: number;
  malformed_lines: number;
  oversized_lines: number;
  error: string | null;
};
export type HistoryEvidenceStatus = "available" | "missing" | "changed" | "malformed";
export type ManualReportAssociation = {
  report_id: string;
  scope: ProfileScope;
  vin_digest: string;
  actor: "owner_ui" | "agent_access";
  associated_at_unix_ms: number;
  note: string | null;
};
export type DiagnosticReportDetail = {
  index: DiagnosticHistoryRow;
  evidence_status: HistoryEvidenceStatus;
  report: DiagnosticReport | null;
  context_snapshot?: VehicleIdentitySnapshot | null;
  manual_associations?: ManualReportAssociation[];
};

const MODE06_BITMAP_PAGES = new Set([0x00, 0x20, 0x40, 0x60, 0x80, 0xa0, 0xc0, 0xe0]);

export function supportsVehicleDiagnostics(adapterFamily: string | null): boolean {
  return VEHICLE_DIAGNOSTICS_ADAPTERS.includes(adapterFamily as (typeof VEHICLE_DIAGNOSTICS_ADAPTERS)[number]);
}

export function supportsProfileIdentity(adapterFamily: string | null): boolean {
  return supportsVehicleDiagnostics(adapterFamily);
}

export function clearConfirmationMatchesConnection(
  activeAdapterId: string | null,
  activeGeneration: number | null,
  confirmedAdapterId: string | null,
  confirmedGeneration: number | null,
): boolean {
  return activeAdapterId !== null
    && activeGeneration !== null
    && activeAdapterId === confirmedAdapterId
    && activeGeneration === confirmedGeneration;
}

export function historyDtcCountLabel(count: number | null | undefined): string {
  return count === null || count === undefined ? "Unknown" : count.toLocaleString();
}

export function historyPageFilter(filter: DiagnosticHistoryFilter, cursor: string | null): DiagnosticHistoryFilter {
  return { ...filter, cursor };
}

export function mergeHistoryRows(previous: DiagnosticHistoryRow[], next: DiagnosticHistoryRow[]): DiagnosticHistoryRow[] {
  const rows = new Map(previous.map((row) => [row.report_id, row]));
  for (const row of next) rows.set(row.report_id, row);
  return [...rows.values()];
}

export function identitySourceLabel(source: VehicleIdentitySource): string {
  return {
    report_ecu: "VIN read from this report",
    same_generation_recognition: "Same-connection recognition",
    manual_profile: "Manual profile context",
    conflict: "Conflicting VINs",
    unassigned: "Unassigned vehicle",
  }[source];
}

export const readVehicleIdentity = (obdConnectionGeneration: number) =>
  invoke<DiagnosticReport>("read_vehicle_identity", { obdConnectionGeneration });
export const listVehicleDiagnosticHistory = (filter: DiagnosticHistoryFilter) =>
  invoke<DiagnosticHistoryPage>("list_vehicle_diagnostic_history", { filter });
export const getVehicleDiagnosticReport = (reportId: string) =>
  invoke<DiagnosticReportDetail>("get_vehicle_diagnostic_report", { reportId });
export const associateVehicleDiagnosticReport = (reportId: string, scope: ProfileScope, vinDigest: string, note: string | null) =>
  invoke<ManualReportAssociation>("associate_vehicle_diagnostic_report", { reportId, scope, vinDigest, note });
export const vehicleHistoryImportStatus = () => invoke<HistoryIndexStatus>("vehicle_history_import_status");
export const startVehicleHistoryImport = () => invoke<HistoryIndexStatus>("start_vehicle_history_import");
export const cancelVehicleHistoryImport = () => invoke<HistoryIndexStatus>("cancel_vehicle_history_import");

export function parseMode06TestIds(text: string): { ids: number[]; error: string | null } {
  const parts = text.split(/[\s,;]+/).filter(Boolean);
  if (parts.length > 32) return { ids: [], error: "Select no more than 32 Mode 06 test IDs." };
  const ids: number[] = [];
  for (const part of parts) {
    const digits = part.replace(/^0x/i, "");
    if (!/^[0-9a-f]{1,2}$/i.test(digits)) return { ids: [], error: `Invalid Mode 06 test ID: ${part}` };
    const id = Number.parseInt(digits, 16);
    if (MODE06_BITMAP_PAGES.has(id)) return { ids: [], error: `${formatHexByte(id)} is a bitmap page, not a monitor test ID.` };
    if (!ids.includes(id)) ids.push(id);
  }
  if (ids.length > 32) return { ids: [], error: "Select no more than 32 unique Mode 06 test IDs." };
  return { ids: ids.sort((a, b) => a - b), error: null };
}

export function formatHexByte(value: number): string {
  return value.toString(16).toUpperCase().padStart(2, "0");
}

export function availableMode06TestIds(reads: DiagnosticRead[]): number[] {
  return [...new Set(reads.flatMap((read) =>
    read.kind === "mode06_bitmap" && read.outcome === "positive"
      ? read.values.flatMap((value) => value.type === "mode06Bitmap" ? value.payload.test_ids : [])
      : [],
  ))].sort((a, b) => a - b);
}

export function diagnosticReadLabel(read: DiagnosticRead): string {
  if (read.kind === "clear_dtcs") return "DTC clear acknowledgement";
  if (read.kind === "mode06_bitmap") return `Mode 06 bitmap ${formatHexByte(read.identifier ?? 0)}`;
  if (read.kind === "mode06_monitor") return `Mode 06 test ${formatHexByte(read.identifier ?? 0)}`;
  return {
    stored_dtcs: "Stored DTCs",
    pending_dtcs: "Pending DTCs",
    permanent_dtcs: "Permanent DTCs",
    vin: "VIN",
    calibration_id: "Calibration ID",
    ecu_name: "ECU name",
  }[read.kind];
}

export function diagnosticOutcomeLabel(outcome: DiagnosticOutcome): string {
  return outcome.replaceAll("_", " ");
}

export function mode06MonitorSummary(value: Extract<DiagnosticValue, { type: "mode06Monitor" }>['payload']): string {
  return `TID ${formatHexByte(value.test_id)} · MID ${formatHexByte(value.component_id)} · UASID ${formatHexByte(value.uasid)} · raw count ${value.value} (min ${value.minimum}, max ${value.maximum})`;
}

export function groupDiagnosticReadsByEcu(reads: DiagnosticRead[]): [string, DiagnosticRead[]][] {
  const groups = new Map<string, DiagnosticRead[]>();
  for (const read of reads) {
    const key = read.ecu_id ?? "No ECU response";
    groups.set(key, [...(groups.get(key) ?? []), read]);
  }
  return [...groups.entries()];
}
