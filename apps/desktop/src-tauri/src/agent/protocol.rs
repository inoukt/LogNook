use serde::{Deserialize, Serialize};

pub const MAX_IPC_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRequest {
    GetStatus,
    ListAdapters,
    ListSignals {
        query: String,
    },
    SetSignalSelection {
        log_signal_ids: Vec<String>,
        graph_signal_ids: Vec<String>,
    },
    ConnectAdapter {
        choice: AgentAdapterChoice,
        log_signal_ids: Vec<String>,
    },
    DisconnectAdapter,
    SearchOemCandidates {
        query: String,
        brand: String,
        model: String,
        service: Option<String>,
        offset: usize,
        limit: usize,
    },
    TryOemCandidate {
        candidate_id: String,
    },
    ListMode06Tests,
    TryMode06Test {
        test: AgentMode06Test,
    },
    GetTrace {
        after_sequence: u64,
        limit: usize,
    },
    StartTraceCapture,
    StopTraceCapture,
    StopOperation,
    ExportEvidence,
    Workbench {
        operation: AgentWorkbenchOperation,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentWorkbenchOperation {
    StartCapture,
    StopCapture,
    MarkEvent {
        label: String,
        before_ms: u64,
        after_ms: u64,
    },
    StartScan {
        config: AgentCanScanConfig,
    },
    StopScan,
    SendFrame {
        can_id: u32,
        extended: bool,
        data: Vec<u8>,
        pacing_ms: u64,
        repeat: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentCanScanMode {
    CanIdRange {
        start_id: u32,
        end_id: u32,
        extended: bool,
        payload: Vec<u8>,
    },
    Mode21 {
        request_id: u32,
        response_id: u32,
        extended: bool,
        start_pid: u8,
        end_pid: u8,
    },
    Mode22 {
        request_id: u32,
        response_id: u32,
        extended: bool,
        start_did: u16,
        end_did: u16,
    },
    Mode01Support {
        request_id: u32,
        response_id: u32,
        extended: bool,
    },
    Mode01Pids {
        request_id: u32,
        response_id: u32,
        extended: bool,
        pids: Vec<u8>,
        repeat: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentCanScanConfig {
    pub mode: AgentCanScanMode,
    pub pacing_ms: u64,
    pub response_timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentWorkbenchResult {
    pub operation: String,
    pub detail: Option<String>,
    pub evidence_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentWorkbenchArgs {
    pub operation: AgentWorkbenchOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentAdapterChoice {
    Simulator,
    ObdLinkSx {
        port_name: String,
    },
    Elm327 {
        address: String,
    },
    A30 {
        address: String,
    },
    OpenPort2 {
        provider_id: String,
    },
    Candlelight {
        id: String,
        interface_number: u8,
        channel: u8,
        bitrate: u32,
        listen_only: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentAdapterInfo {
    pub label: String,
    pub choice: AgentAdapterChoice,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentSignal {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub service: u8,
    pub pid: u8,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentSignalSelection {
    pub log_signal_ids: Vec<String>,
    pub graph_signal_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentQuery {
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentSelectionArgs {
    pub log_signal_ids: Vec<String>,
    pub graph_signal_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentConnectArgs {
    pub choice: AgentAdapterChoice,
    pub log_signal_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub enum AgentMode06Test {
    #[serde(rename = "mode_06_bitmap_00")]
    Bitmap00,
    #[serde(rename = "mode_06_bitmap_20")]
    Bitmap20,
    #[serde(rename = "mode_06_bitmap_40")]
    Bitmap40,
    #[serde(rename = "mode_06_bitmap_60")]
    Bitmap60,
    #[serde(rename = "mode_06_bitmap_80")]
    Bitmap80,
    #[serde(rename = "mode_06_bitmap_a0")]
    BitmapA0,
    #[serde(rename = "mode_06_tid_35")]
    Tid35,
    #[serde(rename = "mode_06_tid_a2")]
    TidA2,
    #[serde(rename = "mode_06_tid_a5")]
    TidA5,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentOemSearchArgs {
    pub query: String,
    pub brand: String,
    pub model: String,
    pub service: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentTryOemArgs {
    pub candidate_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentMode06TryArgs {
    pub test: AgentMode06Test,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentTraceArgs {
    pub after_sequence: u64,
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentTraceEvent {
    pub sequence: u64,
    pub timestamp_unix_ns: u64,
    pub elapsed_ns: Option<u64>,
    pub adapter_id: String,
    pub operation: String,
    pub direction: String,
    pub route: Option<String>,
    pub bytes_hex: Option<String>,
    pub outcome: String,
    pub raw_bytes_unavailable: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentTraceCaptureStatus {
    pub capturing: bool,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentTracePage {
    pub events: Vec<AgentTraceEvent>,
    pub next_sequence: u64,
    pub gaps: u64,
    pub dropped_events: u64,
    pub evicted_events: u64,
    pub capture: AgentTraceCaptureStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentOemCandidate {
    pub candidate_id: String,
    pub source_id: String,
    pub family: String,
    pub brand: String,
    pub model_scope: String,
    pub service: String,
    pub identifier: String,
    pub request_header: String,
    pub response_header: String,
    pub name: String,
    pub unit: String,
    pub source_url: Option<String>,
    pub license: Option<String>,
    pub confidence: String,
    pub research_state: String,
    pub vehicle_validation: String,
    pub auto_request: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentOemSearch {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub candidates: Vec<AgentOemCandidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentOemTrySummary {
    pub candidate_id: String,
    pub exact_adapter_command: String,
    pub adapter_id: String,
    pub protocol: String,
    pub latency_ms: u64,
    pub outcome: String,
    pub raw_response: String,
    pub response_payload: Vec<u8>,
    pub responder_header: Option<String>,
    pub negative_response_code: Option<u8>,
    pub decoded_value: Option<f64>,
    pub unit: Option<String>,
    pub detail: Option<String>,
    pub evidence_saved: bool,
    pub evidence_save_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentMode06Tests {
    pub active_adapter: Option<String>,
    pub tests: Vec<AgentMode06Test>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentGraphValue {
    pub signal_id: String,
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub raw_value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentMode06Summary {
    pub test: String,
    pub request_description: String,
    pub exact_adapter_command: String,
    pub adapter_id: String,
    pub protocol: String,
    pub latency_ms: u64,
    pub outcome: String,
    pub responder_header: Option<String>,
    pub response_payloads: Vec<Vec<u8>>,
    pub negative_response_code: Option<u8>,
    pub graph_values: Vec<AgentGraphValue>,
    pub detail: Option<String>,
    pub raw_response: String,
    pub evidence_saved: bool,
    pub evidence_save_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentAccessSnapshot {
    pub configured: bool,
    pub unlocked: bool,
    pub expires_at_unix_ms: Option<u64>,
    pub host_connected: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentStatus {
    pub configured: bool,
    pub unlocked: bool,
    pub expires_at_unix_ms: Option<u64>,
    pub host_connected: bool,
    pub adapter_id: Option<String>,
    pub adapter_connected: bool,
    pub recording: bool,
    pub active_operation: Option<String>,
    pub supported_signal_ids: Vec<String>,
    pub signal_selection: AgentSignalSelection,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorCode {
    Locked,
    InvalidRequest,
    MessageTooLarge,
    OneAgentHost,
    AppUnavailable,
    Internal,
    Busy,
    UnsupportedAdapter,
    InvalidSelection,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentError {
    pub code: AgentErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentResponse {
    Status {
        status: AgentStatus,
    },
    Adapters {
        adapters: Vec<AgentAdapterInfo>,
        discovery_errors: Vec<String>,
    },
    Signals {
        signals: Vec<AgentSignal>,
    },
    SelectionUpdated {
        selection: AgentSignalSelection,
    },
    Connected {
        adapter_id: String,
        supported_signal_ids: Vec<String>,
    },
    Disconnected,
    OemCandidates {
        result: AgentOemSearch,
    },
    OemCandidateTry {
        result: AgentOemTrySummary,
    },
    Mode06Tests {
        result: AgentMode06Tests,
    },
    Mode06Try {
        result: AgentMode06Summary,
    },
    Trace {
        page: AgentTracePage,
    },
    TraceCapture {
        status: AgentTraceCaptureStatus,
    },
    Workbench {
        result: AgentWorkbenchResult,
    },
    EvidenceExported {
        path: String,
    },
    Error {
        error: AgentError,
    },
}

impl AgentResponse {
    pub fn error(code: AgentErrorCode, message: impl Into<String>) -> Self {
        Self::Error {
            error: AgentError {
                code,
                message: message.into(),
            },
        }
    }
}

pub fn decode_request(bytes: &[u8]) -> Result<AgentRequest, AgentError> {
    if bytes.len() > MAX_IPC_MESSAGE_BYTES {
        return Err(AgentError {
            code: AgentErrorCode::MessageTooLarge,
            message: "Agent request exceeds the 1 MiB limit".into(),
        });
    }
    serde_json::from_slice(bytes).map_err(|error| AgentError {
        code: AgentErrorCode::InvalidRequest,
        message: format!("Invalid Agent Access request: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_rejects_oversized_message() {
        let error = decode_request(&vec![b' '; MAX_IPC_MESSAGE_BYTES + 1]).unwrap_err();
        assert!(matches!(error.code, AgentErrorCode::MessageTooLarge));
    }

    #[test]
    fn ipc_rejects_unknown_action() {
        let error = decode_request(br#"{"action":"run_shell"}"#).unwrap_err();
        assert!(matches!(error.code, AgentErrorCode::InvalidRequest));
    }

    #[test]
    fn get_status_is_a_typed_request() {
        assert!(matches!(
            decode_request(br#"{"action":"get_status"}"#).unwrap(),
            AgentRequest::GetStatus
        ));
    }

    #[test]
    fn workbench_frame_action_is_typed_and_rejects_unknown_fields() {
        let request = decode_request(
            br#"{"action":"workbench","operation":{"kind":"send_frame","can_id":291,"extended":false,"data":[1,2],"pacing_ms":10,"repeat":1}}"#,
        )
        .unwrap();
        assert!(matches!(
            request,
            AgentRequest::Workbench {
                operation: AgentWorkbenchOperation::SendFrame { can_id: 291, .. }
            }
        ));
        assert!(decode_request(
            br#"{"action":"workbench","operation":{"kind":"send_frame","can_id":291,"extended":false,"data":[1,2],"pacing_ms":10,"repeat":1,"shell":"bad"}}"#
        )
        .is_err());
    }

    #[test]
    fn stop_operation_is_a_typed_top_level_action() {
        assert!(matches!(
            decode_request(br#"{"action":"stop_operation"}"#).unwrap(),
            AgentRequest::StopOperation
        ));
    }
}
