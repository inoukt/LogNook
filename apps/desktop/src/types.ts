import type { AutomaticTriggerStatus } from "./automaticTrigger";
import type { A30ReadTestId } from "./a30ReadTests";

export type SignalDefinition = {
  id: string;
  name: string;
  service: number;
  pid: number;
  unit: string;
  min: number;
  max: number;
  description: string;
};

export type OemCandidate = {
  candidate_id: string;
  source_id: string;
  family: string;
  brand: string;
  model_scope: string;
  service: string;
  identifier: string;
  request_header: string;
  response_header: string;
  frequency_hz: number | null;
  module_type: string;
  category: string;
  name: string;
  signal_source_id: string;
  bit_offset: number | null;
  bit_length: number | null;
  formula: string;
  decoder_json: string;
  unit: string;
  year_filter_json: string;
  ecu_filter: string;
  source_blob_sha: string;
  research_state: string;
  vehicle_validation: string;
  auto_request: boolean;
  confidence: string;
  notes: string;
  suggested_metric: string;
  source_scope: string;
  applicability_specificity: number;
  min_value: number | null;
  max_value: number | null;
  source_url: string | null;
  license: string | null;
  license_url: string | null;
};

export type OemCatalogPage = { total: number; offset: number; limit: number; candidates: OemCandidate[] };

export type ScionMode21ProbeResult = {
  vehicle_profile_id: string;
  source_id: string;
  source_blob_sha: string;
  source_url: string;
  license: string;
  license_url: string;
  request_service: number;
  request_identifier: number;
  request_header: string;
  expected_response_header: string;
  exact_adapter_command: string;
  adapter: DeviceDescriptor;
  protocol: string;
  started_at_unix_ms: number;
  latency_ms: number;
  outcome: "positive" | "negative_response" | "no_response" | "malformed_response" | "wrong_responder" | "wrong_length" | "bad_decoder";
  raw_response: string;
  response_payload: number[];
  decoded_signals: { candidate_id: string; name: string; unit: string; bit_offset: number; bit_length: number; formula: string; raw_value: number; value: number }[];
  error: string | null;
  evidence_saved: boolean;
  evidence_save_error: string | null;
};

export type BluetoothSppDevice = { address: string; name: string };

export type A30GraphValue = {
  signal_id: string;
  name: string;
  service: number;
  pid: number | null;
  value: number;
  unit: string;
  raw_value: string;
};

export type A30ReadTestResult = {
  test: string;
  request_description: string;
  exact_adapter_command: string;
  adapter: DeviceDescriptor;
  protocol: string;
  started_at_unix_ms: number;
  latency_ms: number;
  outcome:
    | "positive"
    | "negative_response"
    | "response_pending"
    | "temporarily_unavailable"
    | "session_required"
    | "security_required"
    | "no_response"
    | "malformed_response"
    | "wrong_responder";
  responder_header: string | null;
  response_payloads: number[][];
  negative_response_code: number | null;
  graph_values: A30GraphValue[];
  detail: string | null;
  raw_response: string;
  evidence_saved: boolean;
  evidence_save_error: string | null;
};

export type OemCandidateTryResult = {
  candidate_id: string;
  exact_adapter_command: string;
  adapter: DeviceDescriptor;
  protocol: string;
  started_at_unix_ms: number;
  latency_ms: number;
  outcome:
    | "positive"
    | "negative_response"
    | "response_pending"
    | "temporarily_unavailable"
    | "session_required"
    | "security_required"
    | "no_response"
    | "malformed_response"
    | "wrong_responder"
    | "invalid_candidate";
  raw_response: string;
  responder_header: string | null;
  response_payload: number[];
  negative_response_code: number | null;
  decoded_value: {
    raw_value: string;
    value: number | null;
    enum_value: string | null;
    unit: string;
    formula: string;
    within_catalog_range: boolean | null;
  } | null;
  decode_error: string | null;
  error: string | null;
  evidence_saved: boolean;
  evidence_save_error: string | null;
};

export type ResearchTestSelection =
  | { type: "oemCandidate"; payload: OemCandidate }
  | { type: "a30ReadTest"; payload: A30ReadTestId };

export type ResearchTestResult =
  | { type: "oemCandidate"; payload: OemCandidateTryResult }
  | { type: "a30ReadTest"; payload: A30ReadTestResult };

export type ResearchRunItemStatus = "completed" | "error" | "skipped";

export type ResearchRunItem = {
  queue_index: number;
  queue_size: number;
  selection: ResearchTestSelection;
  status: ResearchRunItemStatus;
  result: ResearchTestResult | null;
  error: string | null;
};

export type ResearchRunReport = {
  run_id: string;
  adapter: DeviceDescriptor;
  started_at_unix_ms: number;
  completed_at_unix_ms: number;
  items: ResearchRunItem[];
};

export type ResearchRunRow = {
  queue_index: number;
  queue_size: number;
  selection: ResearchTestSelection;
  status: "queued" | "running" | "completed" | "error" | "skipped";
  result: ResearchTestResult | null;
  error: string | null;
};

export type ResearchRunViewState = {
  rows: ResearchRunRow[];
  running: boolean;
  currentItem: string;
  completedCount: number;
  totalCount: number;
  report: ResearchRunReport | null;
};

export type Sample = {
  elapsed_ns: number;
  wall_time_unix_ns: number;
  source_id: string;
  ecu_id: string | null;
  signal_id: string;
  value: number;
  unit: string;
  quality: "valid" | "stale" | "missing" | "timeout" | "parse_error" | "disconnected" | "suspect";
  origin: "live" | "replay";
  raw_bytes: number[] | null;
};

export type ChartTrace = {
  signalId: string;
  name: string;
  pid?: number;
  unit: string;
  formula?: string;
  sourceSignalIds?: string[];
  scaleSamples?: Sample[];
  color?: string;
  samples: Sample[];
  current?: Sample;
  sampleRate: string;
};

export type DeviceDescriptor = {
  stable_id: string;
  hardware_family: string;
  firmware: string | null;
  path: string | null;
};

export type J2534Provider = { id: string; name: string };
export type J2534ProviderDiscovery = {
  providers: J2534Provider[];
  diagnostics: string[];
};

export type ConnectionCapabilities = {
  device: DeviceDescriptor;
  protocol: string;
  read_only: boolean;
  supported_signals: SignalDefinition[];
};

export type Marker = {
  id: number | null;
  elapsed_ns: number;
  wall_time_unix_ns: number;
  label: string;
  note: string;
};

export type RecordingSignal = { id: string; unit: string };

export type RecordingInfo = {
  sample_count: number;
  duration_ns: number;
  signals: RecordingSignal[];
  markers: Marker[];
};

export type PlaybackState = {
  session_id: number;
  position_ns: number;
  duration_ns: number;
  speed: number;
  playing: boolean;
};

export type EngineEvent =
  | { type: "connected"; payload: ConnectionCapabilities }
  | { type: "disconnected"; payload?: null }
  | { type: "sample"; payload: Sample }
  | { type: "playbackSample"; payload: { session_id: number; sample: Sample } }
  | { type: "marker"; payload: Marker }
  | { type: "triggerStatus"; payload: AutomaticTriggerStatus }
  | { type: "playbackMarker"; payload: { session_id: number; marker: Marker } }
  | { type: "recordingFailed"; payload: { path: string; sample_count: number } }
  | { type: "error"; payload: string }
  | { type: "playbackStarted"; payload?: null }
  | { type: "playbackFinished"; payload?: null }
  | { type: "playbackState"; payload: PlaybackState };

export interface DiagnosticSignalSummary {
  signal_id: string;
  requested?: boolean | null;
  supported?: boolean | null;
  attempt_count?: number;
  success_count: number;
  error_count?: number;
  first_response_delay_ms?: number | null;
  rate_hz: number;
  latency_ms: number | null;
}

/** Operation metadata only: no measurement values or protocol payloads. */
export interface DiagnosticRecord {
  sequence_id: number;
  timestamp_unix_ms: number;
  operation_id: string;
  adapter_id: string | null;
  operation: string;
  stage: string;
  phase: "started" | "progress" | "succeeded" | "failed";
  duration_ms: number | null;
  signal_ids: string[];
  signal_summaries: DiagnosticSignalSummary[] | null;
  /** Sanitized by the backend diagnostics boundary. */
  detail: string | null;
  transient: boolean | null;
  consecutive_failures?: number | null;
  omitted_events: number;
}
