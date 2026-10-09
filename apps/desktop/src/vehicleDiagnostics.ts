export const VEHICLE_DIAGNOSTICS_ADAPTERS = [
  "Xtool A30/A30M Bluetooth SPP",
  "ELM327 Bluetooth SPP",
  "OBDLink SX",
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
  | "transport_error";

export type DiagnosticReadKind =
  | "stored_dtcs"
  | "pending_dtcs"
  | "permanent_dtcs"
  | "vin"
  | "calibration_id"
  | "ecu_name"
  | "mode06_bitmap"
  | "mode06_monitor";

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
    };

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
  adapter: {
    stable_id: string;
    hardware_family: string;
    firmware: string | null;
    path: string | null;
  };
  protocol: string;
  started_at_unix_ms: number;
  request: { mode06_test_ids: number[] };
  reads: DiagnosticRead[];
  route_restore_error: string | null;
  evidence_saved: boolean;
  evidence_save_error: string | null;
};

const MODE06_BITMAP_PAGES = new Set([0x00, 0x20, 0x40, 0x60, 0x80, 0xa0, 0xc0, 0xe0]);

export function supportsVehicleDiagnostics(adapterFamily: string | null): boolean {
  return VEHICLE_DIAGNOSTICS_ADAPTERS.includes(adapterFamily as (typeof VEHICLE_DIAGNOSTICS_ADAPTERS)[number]);
}

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
