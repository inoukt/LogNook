import type { SignalOption } from "./workspace";
import type {
  CanCaptureComparison, CanCaptureReplayReport, CanCorrelationHypothesis, CanDeviceCandidate, CanDeviceSelection,
  CanDiagnosticRoute, CanFrame,
  CanManualEventMark, CanScanConfig, CanScanResult, CanSignalDefinition, CanTransportError, CanTriggerCaptureResult,
  CanTxStatus, CanWorkbenchConfig, CanWorkbenchStatus, Sample,
} from "./types";

export const defaultCanWorkbenchConfig: CanWorkbenchConfig = { bitrate: 500_000, listen_only: true };

export type CanInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export function createCanWorkbenchCommands(invoke: CanInvoke) {
  return {
    availableAdapters: () => invoke<CanDeviceCandidate[]>("available_can_adapters"),
    connect: (selection: CanDeviceSelection, config: CanWorkbenchConfig) =>
      invoke<CanWorkbenchStatus>("connect_candlelight", { selection, config }),
    configureFields: (fields: CanSignalDefinition[]) => invoke<void>("configure_can_signals", { fields }),
    startCapture: (path: string) => invoke<void>("start_can_capture", { path }),
    stopCapture: () => invoke<void>("stop_can_capture"),
    markEvent: (label: string, preMs: number, postMs: number) =>
      invoke<CanManualEventMark>("mark_can_event", { label, preMs, postMs }),
    sendFrame: (frame: CanFrame, pacingMs: number, repeat: number) =>
      invoke<void>("send_can_frame", { frame, pacingMs, repeat }),
    startScan: (config: CanScanConfig) => invoke<void>("start_can_scan", { config }),
    stopScan: () => invoke<void>("stop_can_scan"),
    startOemCandidate: (candidateId: string) =>
      invoke<unknown>("start_can_oem_candidate", { candidateId }),
    replay: (path: string, fields: CanSignalDefinition[], obdSamples: Sample[] = []) =>
      invoke<CanCaptureReplayReport>("replay_can_capture_report", { path, fields, obdSamples }),
    compareCaptures: (leftPath: string, rightPath: string) =>
      invoke<CanCaptureComparison>("compare_can_captures", { leftPath, rightPath }),
    exportDbcSkeleton: (capturePath: string, path: string) =>
      invoke<void>("export_can_dbc_skeleton", { capturePath, path }),
    chooseDbcExportPath: () => invoke<string | null>("choose_can_dbc_export_path"),
    armTrigger: (preMs: number, postMs: number, maxFrames: number) =>
      invoke<void>("arm_can_trigger", { preMs, postMs, maxFrames }),
    triggerNow: () => invoke<void>("trigger_can_now"),
    disarmTrigger: () => invoke<void>("disarm_can_trigger"),
    chooseTriggerSavePath: () => invoke<string | null>("choose_can_trigger_save_path"),
    saveTriggerCapture: (path: string, capture: CanTriggerCaptureResult) =>
      invoke<void>("save_can_trigger_capture", { path, capture }),
    chooseCaptureSavePath: () => invoke<string | null>("choose_can_capture_save_path"),
    chooseCaptureOpenPath: () => invoke<string | null>("choose_can_capture_open_path"),
  };
}

export function parseCanId(text: string, extended: boolean): number | null {
  const trimmed = text.trim().replace(/^0x/i, "");
  if (!/^[\da-f]+$/i.test(trimmed)) return null;
  const value = Number.parseInt(trimmed, 16);
  return Number.isSafeInteger(value) && value >= 0 && value <= (extended ? 0x1fff_ffff : 0x7ff)
    ? value
    : null;
}

function integerInRange(value: number, min: number, max: number): boolean {
  return Number.isInteger(value) && value >= min && value <= max;
}

export function canSignalDefinitionError(field: CanSignalDefinition): string | null {
  if (!field.id.trim() || !field.name.trim()) return "Signal ID and name are required";
  if (!/^can:[a-z0-9:_-]+$/i.test(field.id.trim())) return "CAN signal ID must start with can:";
  if (!integerInRange(field.can_id, 0, field.extended ? 0x1fff_ffff : 0x7ff)) return "CAN ID exceeds the selected frame format";
  if (!integerInRange(field.bit_length, 1, 64)) return "Bit length must be 1–64";
  if (!integerInRange(field.start_bit, 0, 63)) return "Start bit must be 0–63";
  if (!Number.isFinite(field.factor) || !Number.isFinite(field.offset)) return "Factor and offset must be finite numbers";
  let bit = field.start_bit;
  for (let index = 0; index < field.bit_length; index++) {
    if (bit >= 64) return "Signal field exceeds the 8-byte CAN payload";
    bit = field.byte_order === "motorola" ? bit % 8 === 0 ? bit + 15 : bit - 1 : bit + 1;
  }
  return null;
}

export function canSignalBitPositions(
  startBit: number,
  bitLength: number,
  byteOrder: "intel" | "motorola",
): number[] | null {
  if (!Number.isInteger(startBit) || startBit < 0 || startBit > 63
    || !Number.isInteger(bitLength) || bitLength < 1 || bitLength > 64) return null;
  const positions: number[] = [];
  let bit = startBit;
  for (let index = 0; index < bitLength; index++) {
    if (bit >= 64) return null;
    positions.push(bit);
    bit = byteOrder === "intel" ? bit + 1 : bit % 8 === 0 ? bit + 15 : bit - 1;
  }
  return positions;
}

export function extractCanSignalRaw(
  data: number[],
  startBit: number,
  bitLength: number,
  byteOrder: "intel" | "motorola",
  signed: boolean,
): bigint | null {
  const positions = canSignalBitPositions(startBit, bitLength, byteOrder);
  if (!positions || positions.some(bit => bit >= data.length * 8)) return null;
  let raw = 0n;
  for (const [index, bit] of positions.entries()) {
    const value = BigInt((data[Math.floor(bit / 8)] >> (bit % 8)) & 1);
    raw = byteOrder === "intel"
      ? raw | (value << BigInt(index))
      : (raw << 1n) | value;
  }
  const sign = 1n << BigInt(bitLength - 1);
  return signed && (raw & sign) !== 0n ? raw - (1n << BigInt(bitLength)) : raw;
}

export function restoreCanSignalDefinitions(serialized: string | null): CanSignalDefinition[] {
  let parsed: unknown;
  try { parsed = JSON.parse(serialized ?? "null"); } catch { return []; }
  if (!Array.isArray(parsed)) return [];
  const fields = new Map<string, CanSignalDefinition>();
  for (const value of parsed) {
    if (!value || typeof value !== "object" || Array.isArray(value)) continue;
    const item = value as Record<string, unknown>;
    if (typeof item.id !== "string" || typeof item.name !== "string"
      || typeof item.can_id !== "number" || typeof item.extended !== "boolean"
      || typeof item.start_bit !== "number" || typeof item.bit_length !== "number"
      || (item.byte_order !== "intel" && item.byte_order !== "motorola")
      || typeof item.signed !== "boolean" || typeof item.factor !== "number"
      || typeof item.offset !== "number" || typeof item.unit !== "string") continue;
    const field: CanSignalDefinition = {
      id: item.id,
      name: item.name,
      can_id: item.can_id,
      extended: item.extended,
      start_bit: item.start_bit,
      bit_length: item.bit_length,
      byte_order: item.byte_order,
      signed: item.signed,
      factor: item.factor,
      offset: item.offset,
      unit: item.unit,
    };
    if (canSignalDefinitionError(field)) continue;
    fields.delete(field.id);
    fields.set(field.id, field);
  }
  return [...fields.values()].slice(-500);
}

function validRoute(mode: { request_id: number; response_id: number; extended: boolean }): boolean {
  const max = mode.extended ? 0x1fff_ffff : 0x7ff;
  return integerInRange(mode.request_id, 0, max)
    && integerInRange(mode.response_id, 0, max)
    && mode.request_id !== mode.response_id;
}

export function canScanConfigError(config: CanScanConfig): string | null {
  if (!integerInRange(config.pacing_ms, 1, 60_000)) return "Pacing must be 1–60000 ms";
  if (!integerInRange(config.response_timeout_ms, 10, 10_000)) return "Response timeout must be 10–10000 ms";
  const mode = config.mode;
  if (mode.kind === "can_id_range") {
    const max = mode.extended ? 0x1fff_ffff : 0x7ff;
    if (!integerInRange(mode.start_id, 0, max) || !integerInRange(mode.end_id, mode.start_id, max)) return "Invalid CAN ID range";
    return mode.payload.length <= 8 && mode.payload.every((byte) => integerInRange(byte, 0, 0xff))
      ? null
      : "Classic CAN payload must contain up to 8 bytes";
  }
  if (!validRoute(mode)) return "Request and response IDs must be valid and different";
  if (mode.kind === "mode_21" && !integerInRange(mode.start_pid, 0, 0xff)) return "Mode 21 PID range must fit one byte";
  if (mode.kind === "mode_21" && !integerInRange(mode.end_pid, mode.start_pid, 0xff)) return "Mode 21 start PID must not exceed end PID";
  if (mode.kind === "mode_22" && !integerInRange(mode.start_did, 0, 0xffff)) return "Mode 22 DID range must fit two bytes";
  if (mode.kind === "mode_22" && !integerInRange(mode.end_did, mode.start_did, 0xffff)) return "Mode 22 start DID must not exceed end DID";
  if (mode.kind === "mode_01_pids" && (!mode.pids.length || mode.pids.some((pid) => !integerInRange(pid, 0, 0xff)))) return "Mode 01 requires one or more byte-sized PIDs";
  if (mode.kind === "mode_01_pids" && new Set(mode.pids).size !== mode.pids.length) return "Mode 01 PID list cannot contain duplicates";
  return null;
}

export type CanScanKind = "can_id_range" | "mode_01_support" | "mode_01_pids" | "mode_21" | "mode_22";
export type CanScanInput = {
  kind: CanScanKind;
  requestId: string;
  responseId: string;
  extended: boolean;
  start: string;
  end: string;
  payload?: string;
  pacingMs: number;
  responseTimeoutMs: number;
};

function parseHexValue(text: string): number | null {
  const value = text.trim().replace(/^0x/i, "");
  if (!/^[\da-f]+$/i.test(value)) return null;
  const parsed = Number.parseInt(value, 16);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

export function buildCanScanConfig(input: CanScanInput): CanScanConfig | null {
  if (input.kind === "can_id_range") {
    const start_id = parseCanId(input.start, input.extended);
    const end_id = parseCanId(input.end, input.extended);
    const payload = parseCanPayload(input.payload ?? "");
    if (start_id === null || end_id === null || payload === null) return null;
    const config: CanScanConfig = {
      mode: { kind: input.kind, start_id, end_id, extended: input.extended, payload },
      pacing_ms: input.pacingMs,
      response_timeout_ms: input.responseTimeoutMs,
    };
    return canScanConfigError(config) ? null : config;
  }
  const request_id = parseCanId(input.requestId, input.extended);
  const response_id = parseCanId(input.responseId, input.extended);
  if (request_id === null || response_id === null) return null;
  const mode = input.kind === "mode_01_support"
    ? { kind: input.kind, request_id, response_id, extended: input.extended } as const
    : (() => {
        const start = parseHexValue(input.start);
        const end = parseHexValue(input.end);
        if (start === null || end === null) return null;
        if (input.kind === "mode_01_pids") {
          if (start < 0 || start > 0xff || end < 0 || end > 0xff || end < start) return null;
          return { kind: input.kind, request_id, response_id, extended: input.extended, pids: Array.from({ length: end - start + 1 }, (_, index) => start + index), repeat: false } as const;
        }
        return input.kind === "mode_21"
          ? { kind: input.kind, request_id, response_id, extended: input.extended, start_pid: start, end_pid: end } as const
          : { kind: input.kind, request_id, response_id, extended: input.extended, start_did: start, end_did: end } as const;
      })();
  if (!mode) return null;
  const config = { mode, pacing_ms: input.pacingMs, response_timeout_ms: input.responseTimeoutMs } as CanScanConfig;
  return canScanConfigError(config) ? null : config;
}

export function buildCanCommunicationCheck(
  requestId: string,
  responseId: string,
  extended: boolean,
  pacingMs: number,
  responseTimeoutMs: number,
): CanScanConfig | null {
  return buildCanScanConfig({
    kind: "mode_01_pids",
    requestId,
    responseId,
    extended,
    start: "00",
    end: "00",
    pacingMs,
    responseTimeoutMs,
  });
}

export function canTransmitEnabled(
  status: Pick<CanWorkbenchStatus, "connected" | "listen_only"> | null,
  captureOpen: boolean,
): boolean {
  return Boolean(status?.connected && !status.listen_only && captureOpen);
}

export function canTriggerConfigError(preMs: number, postMs: number, maxFrames: number): string | null {
  if (!integerInRange(preMs, 0, 60_000) || !integerInRange(postMs, 0, 60_000)) {
    return "Trigger windows must be 0–60000 ms";
  }
  if (!integerInRange(maxFrames, 1, 100_000)) return "Trigger frame limit must be 1–100000";
  return null;
}

export function formatCanDiagnosticRoute(route: CanDiagnosticRoute): string {
  const width = route.extended ? "29-bit" : "11-bit";
  const request = `0x${route.request_id.toString(16).toUpperCase()}`;
  const response = `0x${route.response_id.toString(16).toUpperCase()}`;
  return `${width} ${request} → ${response}`;
}

export function canDiagnosticRouteKey(route: CanDiagnosticRoute): string {
  return `${route.extended ? "29" : "11"}:${route.request_id.toString(16)}:${route.response_id.toString(16)}`;
}

export function formatCanCorrelationSource(
  hypothesis: Pick<CanCorrelationHypothesis, "obd_source_id" | "obd_ecu_id">,
): string {
  const source = hypothesis.obd_source_id?.trim() || "unknown source";
  const ecu = hypothesis.obd_ecu_id?.trim() || "unknown ECU";
  return `${source} · ECU ${ecu}`;
}

export function canCorrelationHypothesisKey(
  hypothesis: Pick<CanCorrelationHypothesis, "field_id" | "obd_signal_id" | "obd_source_id" | "obd_ecu_id">,
): string {
  return JSON.stringify([
    hypothesis.field_id,
    hypothesis.obd_signal_id,
    hypothesis.obd_source_id,
    hypothesis.obd_ecu_id ?? "unknown ECU",
  ]);
}

export function canSignalOption(field: CanSignalDefinition): SignalOption {
  const bitRange = field.byte_order === "intel"
    ? `bits ${field.start_bit}–${field.start_bit + field.bit_length - 1} · Intel`
    : `start bit ${field.start_bit} · ${field.bit_length}-bit Motorola`;
  return {
    id: field.id,
    name: field.name,
    unit: field.unit,
    description: `CAN 0x${field.can_id.toString(16).toUpperCase()} · ${bitRange}`,
  };
}

export function canSignalIdsNeedingReset(
  previous: CanSignalDefinition[],
  next: CanSignalDefinition[],
): string[] {
  const nextById = new Map(next.map((field) => [field.id, field]));
  return previous
    .filter((field) => {
      const replacement = nextById.get(field.id);
      if (!replacement) return true;
      const keys = new Set([...Object.keys(field), ...Object.keys(replacement)]);
      return [...keys].some((key) =>
        field[key as keyof CanSignalDefinition] !== replacement[key as keyof CanSignalDefinition]);
    })
    .map(({ id }) => id);
}

export function appendCanFrames(current: CanFrame[], incoming: CanFrame[], limit = 400): CanFrame[] {
  return [...current, ...incoming].slice(-limit);
}

export function parseCanPayload(text: string): number[] | null {
  const tokens = text.trim() ? text.trim().split(/[\s,]+/) : [];
  if (tokens.length > 8 || tokens.some((token) => !/^(?:0x)?[\da-f]{1,2}$/i.test(token))) return null;
  return tokens.map((token) => Number.parseInt(token.replace(/^0x/i, ""), 16));
}

export function createCanTxFrame(canId: number, extended: boolean, data: number[]): CanFrame | null {
  const maximum = extended ? 0x1fff_ffff : 0x7ff;
  if (!integerInRange(canId, 0, maximum) || data.length > 8
    || data.some((byte) => !integerInRange(byte, 0, 0xff))) return null;
  return { elapsed_ns: 0, wall_time_unix_ns: 0, can_id: canId, extended, direction: "tx", data, flags: 0 };
}

export type CanTrafficSummary = {
  can_id: number;
  extended: boolean;
  rx_count: number;
  tx_count: number;
  last_data: number[];
  last_direction: CanFrame["direction"];
  first_rx_elapsed_ns: number | null;
  last_rx_elapsed_ns: number | null;
  period_ms: number | null;
  rate_hz: number | null;
  ascii: string;
};

export function appendCanTrafficSummary(
  current: CanTrafficSummary[], incoming: CanFrame[],
): CanTrafficSummary[] {
  const rows = new Map(current.map((row) => [
    `${row.extended ? 1 : 0}:${row.can_id}`,
    { ...row, last_data: [...row.last_data] },
  ]));
  for (const frame of incoming) {
    const key = `${frame.extended ? 1 : 0}:${frame.can_id}`;
    const row = rows.get(key) ?? {
      can_id: frame.can_id,
      extended: frame.extended,
      rx_count: 0,
      tx_count: 0,
      last_data: [],
      last_direction: frame.direction,
      first_rx_elapsed_ns: null,
      last_rx_elapsed_ns: null,
      period_ms: null,
      rate_hz: null,
      ascii: "",
    };
    row.last_data = [...frame.data];
    row.last_direction = frame.direction;
    row.ascii = frame.data.map((byte) => byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : ".").join("");
    if (frame.direction === "rx") {
      if (row.rx_count === 0) row.first_rx_elapsed_ns = frame.elapsed_ns;
      row.rx_count++;
      row.last_rx_elapsed_ns = frame.elapsed_ns;
      const spanNs = frame.elapsed_ns - (row.first_rx_elapsed_ns ?? frame.elapsed_ns);
      row.period_ms = row.rx_count > 1 && spanNs > 0 ? spanNs / (row.rx_count - 1) / 1e6 : null;
      row.rate_hz = row.period_ms === null ? null : 1000 / row.period_ms;
    } else {
      row.tx_count++;
    }
    rows.set(key, row);
  }
  return [...rows.values()].sort((left, right) =>
    Number(left.extended) - Number(right.extended) || left.can_id - right.can_id);
}

export function clampCanMonitorHeight(heightPx: number, availableHeightPx: number): number {
  const minimum = 120;
  const maximum = Math.max(minimum, availableHeightPx - 168);
  return Math.max(minimum, Math.min(maximum, heightPx));
}

export function adjustCanMonitorHeight(heightPx: number, deltaPx: number, availableHeightPx: number): number {
  return clampCanMonitorHeight(heightPx + deltaPx, availableHeightPx);
}

export type CanSessionEvents = {
  frames: CanFrame[];
  trafficSummary: CanTrafficSummary[];
  scanResults: CanScanResult[];
  txStatuses: CanTxStatus[];
  transportErrors: CanTransportError[];
  scanRunning: boolean;
  triggerCapture: null;
};

export function emptyCanSessionEvents(): CanSessionEvents {
  return { frames: [], trafficSummary: [], scanResults: [], txStatuses: [], transportErrors: [], scanRunning: false, triggerCapture: null };
}

export function clearCanSessionSamples(
  latest: Record<string, Sample>,
  series: Record<string, Sample[]>,
  canSignalIds: readonly string[],
): { latest: Record<string, Sample>; series: Record<string, Sample[]> } {
  const canIds = new Set(canSignalIds);
  return {
    latest: Object.fromEntries(Object.entries(latest).filter(([id]) => !canIds.has(id))),
    series: Object.fromEntries(Object.entries(series).filter(([id]) => !canIds.has(id))),
  };
}

export function replaySidecarStatus(
  report: Pick<CanCaptureReplayReport, "evidence_journal" | "evidence_journal_error" | "session_finalized">,
): string {
  if (report.evidence_journal_error !== null) return "JSONL sidecar is unreadable";
  if (report.evidence_journal === null) return "No JSONL sidecar found";
  if (report.session_finalized === true) return "JSONL session closed cleanly";
  if (report.session_finalized === false) return "JSONL session is incomplete";
  return "JSONL sidecar found; session state is unrecognized";
}
