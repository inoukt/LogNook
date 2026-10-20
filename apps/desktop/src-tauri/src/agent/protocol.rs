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
    ListLogSources,
    QueryLogRange {
        query: AgentLogQueryArgs,
    },
    SendRawCommand {
        operation: AgentRawCommand,
    },
    SetSignalSelection {
        log_signal_ids: Vec<String>,
        graph_signal_ids: Vec<String>,
    },
    CanObd {
        operation: AgentCanObdOperation,
    },
    ConnectAdapter {
        choice: AgentAdapterChoice,
        log_signal_ids: Vec<String>,
        #[serde(default)]
        profile_standby: bool,
    },
    DisconnectAdapter,
    DisconnectSource {
        source: AgentConnectionSource,
    },
    SearchOemCandidates {
        query: String,
        brand: String,
        model: String,
        service: Option<String>,
        offset: usize,
        limit: usize,
    },
    SearchSubaruCatalog {
        query: String,
        model: String,
        year: Option<i32>,
        ecu_id: String,
        protocol_family: String,
        offset: usize,
        limit: usize,
    },
    GetSubaruCatalogDetail {
        candidate_id: String,
    },
    TryOemCandidate {
        candidate_id: String,
    },
    OemDiscovery {
        operation: crate::commands::oem_discovery::OemDiscoveryRequest,
    },
    OemLogging {
        operation: AgentOemLoggingOperation,
    },
    ListMode06Tests,
    TryMode06Test {
        test: AgentMode06Test,
    },
    ReadVehicleDiagnostics {
        /// This action reads diagnostic data only and cannot clear codes.
        #[serde(default)]
        mode06_test_ids: Vec<u8>,
    },
    ClearVehicleDtcs {
        adapter_id: String,
        connection_generation: u64,
        confirm: bool,
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
    VehicleProfiles {
        operation: AgentVehicleProfileOperation,
    },
    VehicleProjects {
        operation: crate::commands::vehicle_projects::VehicleProjectsOperation,
    },
    SignalPresets {
        operation: lognook_core::SignalPresetOperation,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentVehicleProfileOperation {
    List,
    Get {
        profile_id: String,
    },
    Save {
        args: crate::vehicle_profiles::SaveVehicleProfileArgs,
    },
    Status,
    Recognize {
        obd_connection_generation: u64,
    },
    ReadIdentity {
        obd_connection_generation: u64,
    },
    GetIdentity {
        scope: lognook_core::ProfileScope,
        vin_digest: String,
    },
    DecodeVinOffline {
        scope: lognook_core::ProfileScope,
        vin_digest: String,
    },
    SetManualVehicleDetail {
        scope: lognook_core::ProfileScope,
        vin_digest: String,
        field: lognook_core::ManualVehicleField,
        value: String,
        note: Option<String>,
    },
    LinkLegacyVin {
        profile_id: String,
        expected_revision: u64,
        full_vin: String,
    },
    AssignProfileVin {
        profile_id: String,
        expected_revision: u64,
        full_vin: String,
    },
    ListHistory {
        filter: lognook_core::DiagnosticHistoryFilter,
    },
    GetHistoryReport {
        report_id: String,
    },
    AssociateHistoryReport {
        report_id: String,
        scope: lognook_core::ProfileScope,
        vin_digest: String,
        note: Option<String>,
    },
    HistoryImportStatus,
    StartHistoryImport,
    CancelHistoryImport,
    PrepareResume {
        profile_id: String,
        obd_connection_generation: u64,
    },
    ApplyResume {
        preview_id: String,
        profile_revision: u64,
        obd_connection_generation: u64,
    },
    DiscardResume {
        preview_id: String,
    },
    ContinueSelection {
        log_signal_ids: Vec<String>,
        graph_signal_ids: Vec<String>,
        derived_definitions: Vec<lognook_core::DerivedSignalDefinition>,
        obd_connection_generation: u64,
    },
    Cancel,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentVehicleProfileArgs {
    pub operation: AgentVehicleProfileOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentVehicleProfileResult {
    Profiles {
        profiles: Vec<lognook_core::VehicleProfile>,
    },
    Profile {
        profile: lognook_core::VehicleProfile,
    },
    Status {
        status: crate::vehicle_profiles::VehicleProfileStatus,
    },
    Recognized {
        observation: lognook_core::VehicleIdentityObservation,
        status: crate::vehicle_profiles::VehicleProfileStatus,
    },
    IdentityReport {
        report: serde_json::Value,
    },
    IdentityRecord {
        record: Option<lognook_core::VehicleIdentityRecord>,
    },
    IdentityUpdated {
        record: lognook_core::VehicleIdentityRecord,
    },
    History {
        page: lognook_core::DiagnosticHistoryPage,
    },
    HistoryReport {
        detail: serde_json::Value,
    },
    HistoryAssociation {
        association: lognook_core::ManualReportAssociation,
    },
    HistoryImport {
        status: lognook_core::HistoryIndexStatus,
    },
    Preview {
        preview: crate::vehicle_profiles::ProfileResumePreview,
    },
    Resumed {
        result: crate::vehicle_profiles::ProfileResumeApplyResult,
    },
    SelectionContinued {
        selection: AgentSignalSelection,
    },
    Discarded,
    Cancelled,
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
    ReplayCanId {
        can_id: u32,
        extended: bool,
        start_elapsed_ms: u64,
        end_elapsed_ms: u64,
    },
    PrepareSavedCanReplay {
        source_id: String,
        can_id: u32,
        extended: bool,
        start_elapsed_ms: u64,
        end_elapsed_ms: u64,
    },
    InspectSavedCanReplay {
        preview_id: String,
    },
    DiscardSavedCanReplay {
        preview_id: Option<String>,
    },
    StartSavedCanReplay {
        preview_id: String,
        expected_can_generation: u64,
        expected_output_capture_id: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_replay_preview: Option<crate::can_replay_previews::SavedCanReplayPreviewView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_status: Option<lognook_core::CanIdReplayStatus>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentWorkbenchArgs {
    pub operation: AgentWorkbenchOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentSignalPresetsArgs {
    pub operation: lognook_core::SignalPresetOperation,
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
    OpenPort2SubaruSsm {
        provider_id: String,
    },
    Candlelight {
        id: String,
        interface_number: u8,
        channel: u8,
        bitrate: u32,
        listen_only: bool,
    },
    CandlelightObd {
        id: String,
        interface_number: u8,
        channel: u8,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentCanObdOperation {
    Discover,
    Identify {
        selection: lognook_core::CanDeviceSelection,
    },
    Connect {
        selection: lognook_core::CanDeviceSelection,
        log_signal_ids: Vec<String>,
        graph_signal_ids: Vec<String>,
    },
    Select {
        log_signal_ids: Vec<String>,
        graph_signal_ids: Vec<String>,
    },
    Status,
    Disconnect,
    Cancel,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentCanObdArgs {
    pub operation: AgentCanObdOperation,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCanObdResult {
    Devices {
        devices: Vec<lognook_core::CanDeviceCandidate>,
    },
    Identified {
        device: lognook_core::DeviceDescriptor,
    },
    Connected {
        adapter_id: String,
        supported_signal_ids: Vec<String>,
    },
    SelectionUpdated {
        selection: AgentSignalSelection,
    },
    Status {
        connecting: bool,
        connected: bool,
        device: Option<lognook_core::DeviceDescriptor>,
        supported_signal_ids: Vec<String>,
        selection: AgentSignalSelection,
    },
    Disconnected,
    Cancelled {
        requested: bool,
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

fn default_log_query_limit() -> usize {
    100
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentLogQueryArgs {
    pub source_id: String,
    pub kind: AgentLogKind,
    pub start_elapsed_ms: u64,
    pub end_elapsed_ms: u64,
    #[serde(default)]
    pub signal_ids: Vec<String>,
    #[serde(default)]
    pub can_ids: Vec<u32>,
    #[serde(default = "default_log_query_limit")]
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentLogKind {
    Pids,
    Can,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentLogSource {
    pub source_id: String,
    pub name: String,
    pub kind: AgentLogKind,
    pub recorded: bool,
    pub start_elapsed_ms: Option<u64>,
    pub end_elapsed_ms: Option<u64>,
    pub item_count: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentMeasurement {
    pub elapsed_ms: u64,
    pub wall_time_unix_ms: u64,
    pub source_id: String,
    pub ecu_id: Option<String>,
    pub signal_id: String,
    pub value: f64,
    pub unit: String,
    pub quality: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentCanFrame {
    pub elapsed_ms: u64,
    pub wall_time_unix_ms: u64,
    pub can_id: u32,
    pub extended: bool,
    pub direction: String,
    pub data: Vec<u8>,
    pub flags: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentLogPage {
    pub source_id: String,
    pub kind: AgentLogKind,
    pub start_elapsed_ms: u64,
    pub end_elapsed_ms: u64,
    pub truncated: bool,
    pub pid_samples: Vec<AgentMeasurement>,
    pub can_frames: Vec<AgentCanFrame>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRawCommand {
    DiagnosticPayload { payload_hex: String },
    AdapterTerminal { command: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentRawCommandArgs {
    pub operation: AgentRawCommand,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentRawCommandResult {
    pub adapter_id: String,
    pub kind: String,
    pub request: String,
    pub response: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
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
    #[serde(default)]
    pub profile_standby: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentConnectionSource {
    Obd,
    Can,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentDisconnectSourceArgs {
    pub source: AgentConnectionSource,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentVehicleDiagnosticsArgs {
    /// Additional Mode 06 monitor IDs. Empty reads standard DTCs, identity and support bitmaps.
    #[serde(default)]
    pub mode06_test_ids: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentClearVehicleDtcsArgs {
    pub adapter_id: String,
    pub connection_generation: u64,
    /// Must be true to authorize the distinct, one-shot standard Mode 04 action.
    pub confirm: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentOemDiscoveryArgs {
    pub operation: crate::commands::oem_discovery::OemDiscoveryRequest,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentOemLoggingOperation {
    Status {},
    Search {
        vehicle_id: String,
        query: String,
        offset: usize,
        limit: usize,
    },
    Start {
        vehicle_id: String,
        candidate_ids: Vec<String>,
        adapter_id: String,
        connection_generation: u64,
    },
    Stop {
        adapter_id: String,
        connection_generation: u64,
    },
    StartSubaru {
        candidate_ids: Vec<String>,
        adapter_id: String,
        connection_generation: u64,
    },
    StopSubaru {
        adapter_id: String,
        connection_generation: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentOemLoggingArgs {
    pub operation: AgentOemLoggingOperation,
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
pub struct AgentSubaruCatalogSearchArgs {
    pub query: String,
    pub model: String,
    pub year: Option<i32>,
    pub ecu_id: String,
    pub protocol_family: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AgentSubaruCatalogDetailArgs {
    pub candidate_id: String,
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
    #[serde(default)]
    pub obd_connection_generation: Option<u64>,
    #[serde(default)]
    pub diagnostic_capabilities: Option<lognook_core::DiagnosticCapabilities>,
    #[serde(default)]
    pub obd_connected: bool,
    #[serde(default)]
    pub can_connected: bool,
    #[serde(default)]
    pub can_adapter_id: Option<String>,
    #[serde(default)]
    pub can_connection_generation: Option<u64>,
    #[serde(default)]
    pub vehicle_profile_status: Option<crate::vehicle_profiles::VehicleProfileStatus>,
    #[serde(default)]
    pub vehicle_profile_error: Option<String>,
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
    LogSources {
        sources: Vec<AgentLogSource>,
    },
    LogData {
        page: AgentLogPage,
    },
    RawCommand {
        result: AgentRawCommandResult,
    },
    SelectionUpdated {
        selection: AgentSignalSelection,
    },
    CanObd {
        result: AgentCanObdResult,
    },
    Connected {
        adapter_id: String,
        supported_signal_ids: Vec<String>,
    },
    Disconnected,
    OemCandidates {
        result: AgentOemSearch,
    },
    SubaruCatalog {
        result: lognook_core::SubaruCatalogPage,
    },
    SubaruCatalogDetail {
        result: lognook_core::SubaruCatalogDetail,
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
    VehicleDiagnostics {
        report: serde_json::Value,
    },
    VehicleDtcsCleared {
        report: serde_json::Value,
    },
    OemDiscovery {
        result: serde_json::Value,
    },
    OemLogging {
        result: serde_json::Value,
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
    VehicleProfiles {
        result: AgentVehicleProfileResult,
    },
    VehicleProjects {
        result: crate::commands::vehicle_projects::VehicleProjectsResult,
    },
    SignalPresets {
        result: lognook_core::SignalPresetResult,
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
    fn individual_disconnect_requires_an_explicit_known_source() {
        assert!(matches!(
            decode_request(br#"{"action":"disconnect_source","source":"can"}"#).unwrap(),
            AgentRequest::DisconnectSource {
                source: AgentConnectionSource::Can
            }
        ));
        assert!(decode_request(br#"{"action":"disconnect_source","source":"all"}"#).is_err());
    }

    #[test]
    fn diagnostic_monitors_are_byte_sized_and_do_not_accept_raw_commands() {
        assert!(
            matches!(decode_request(br#"{"action":"read_vehicle_diagnostics","mode06_test_ids":[53,162]}"#).unwrap(), AgentRequest::ReadVehicleDiagnostics { mode06_test_ids } if mode06_test_ids == [53,162])
        );
        assert!(
            decode_request(br#"{"action":"read_vehicle_diagnostics","mode06_test_ids":[256]}"#)
                .is_err()
        );
        assert!(
            decode_request(
                br#"{"action":"read_vehicle_diagnostics","mode06_test_ids":[],"clear_codes":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn clear_dtcs_is_a_separate_confirmed_adapter_generation_bound_action() {
        assert!(matches!(
            decode_request(br#"{"action":"clear_vehicle_dtcs","adapter_id":"sx","connection_generation":7,"confirm":true}"#).unwrap(),
            AgentRequest::ClearVehicleDtcs { adapter_id, connection_generation: 7, confirm: true } if adapter_id == "sx"
        ));
        assert!(
            decode_request(
                br#"{"action":"clear_vehicle_dtcs","adapter_id":"sx","connection_generation":7}"#
            )
            .is_err()
        );
        assert!(decode_request(
            br#"{"action":"clear_vehicle_dtcs","adapter_id":"sx","connection_generation":7,"confirm":true,"service":4}"#
        )
        .is_err());
    }

    #[test]
    fn continuous_oem_operations_are_typed_bound_and_have_no_promotion_override() {
        assert!(decode_request(br#"{"action":"oem_logging","operation":{"kind":"start","vehicle_id":"ford-test","candidate_ids":["verified"],"adapter_id":"op","connection_generation":3}}"#).is_ok());
        assert!(decode_request(br#"{"action":"oem_logging","operation":{"kind":"start","vehicle_id":"ford-test","candidate_ids":["verified"],"adapter_id":"op"}}"#).is_err());
        assert!(decode_request(br#"{"action":"oem_logging","operation":{"kind":"stop","adapter_id":"op","connection_generation":3,"payload":"04"}}"#).is_err());
        assert!(
            decode_request(
                br#"{"action":"oem_logging","operation":{"kind":"status","auto_request":true}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn oem_discovery_uses_typed_shared_queue_and_rejects_raw_or_unbound_requests() {
        assert!(matches!(
            decode_request(br#"{"action":"oem_discovery","operation":{"kind":"enqueue_visible","candidate_ids":["source-row"],"adapter_id":"sx","connection_generation":7}}"#).unwrap(),
            AgentRequest::OemDiscovery { .. }
        ));
        for input in [
            br#"{"action":"oem_discovery","operation":{"kind":"enqueue_visible","candidate_ids":["source-row"],"adapter_id":"sx"}}"#.as_slice(),
            br#"{"action":"oem_discovery","operation":{"kind":"enqueue_visible","candidate_ids":["source-row"],"adapter_id":"sx","connection_generation":7,"payload":[46,1,2]}}"#.as_slice(),
            br#"{"action":"oem_discovery","operation":{"kind":"send_raw","service":46}}"#.as_slice(),
        ] {
            assert!(decode_request(input).is_err());
        }
        let schema = serde_json::to_string(&schemars::schema_for!(AgentOemDiscoveryArgs)).unwrap();
        for field in [
            "enqueue_visible",
            "try_filtered",
            "values_first",
            "connection_generation",
        ] {
            assert!(schema.contains(field));
        }
    }

    #[test]
    fn agent_log_query_is_typed_and_keeps_the_requested_range() {
        let request = decode_request(
            br#"{"action":"query_log_range","query":{"source_id":"live:pids","kind":"pids","start_elapsed_ms":100,"end_elapsed_ms":250,"signal_ids":["engine_rpm"],"can_ids":[],"limit":12}}"#,
        )
        .unwrap();
        assert!(matches!(
            request,
            AgentRequest::QueryLogRange {
                query: AgentLogQueryArgs {
                    source_id,
                    start_elapsed_ms: 100,
                    end_elapsed_ms: 250,
                    ..
                }
            } if source_id == "live:pids"
        ));
    }

    #[test]
    fn raw_adapter_terminal_command_is_one_typed_operation() {
        let request = decode_request(
            br#"{"action":"send_raw_command","operation":{"kind":"adapter_terminal","command":"ATZ"}}"#,
        )
        .unwrap();
        assert!(matches!(
            request,
            AgentRequest::SendRawCommand {
                operation: AgentRawCommand::AdapterTerminal { command }
            } if command == "ATZ"
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
    fn replay_can_id_workbench_action_is_typed_and_schema_describes_the_selected_range() {
        let request = decode_request(
            br#"{"action":"workbench","operation":{"kind":"replay_can_id","can_id":801,"extended":false,"start_elapsed_ms":1,"end_elapsed_ms":500}}"#,
        )
        .unwrap();
        assert!(matches!(
            request,
            AgentRequest::Workbench {
                operation: AgentWorkbenchOperation::ReplayCanId {
                    can_id: 801,
                    extended: false,
                    start_elapsed_ms: 1,
                    end_elapsed_ms: 500,
                }
            }
        ));
        assert!(decode_request(
            br#"{"action":"workbench","operation":{"kind":"replay_can_id","can_id":801,"extended":false,"start_elapsed_ms":1,"end_elapsed_ms":500,"frames":[]}}"#
        )
        .is_err());

        let schema = serde_json::to_value(schemars::schema_for!(AgentWorkbenchOperation)).unwrap();
        assert!(schema.to_string().contains("replay_can_id"));
        assert!(schema.to_string().contains("start_elapsed_ms"));
        assert!(schema.to_string().contains("end_elapsed_ms"));
    }

    #[test]
    fn saved_replay_requires_a_source_preview_and_connection_generation() {
        let request = decode_request(br#"{"action":"workbench","operation":{"kind":"prepare_saved_can_replay","source_id":"saved-can-source-1","can_id":801,"extended":true,"start_elapsed_ms":250,"end_elapsed_ms":900}}"#).unwrap();
        assert!(matches!(
            request,
            AgentRequest::Workbench {
                operation: AgentWorkbenchOperation::PrepareSavedCanReplay {
                    can_id: 801,
                    extended: true,
                    start_elapsed_ms: 250,
                    end_elapsed_ms: 900,
                    ..
                }
            }
        ));
        assert!(decode_request(br#"{"action":"workbench","operation":{"kind":"prepare_saved_can_replay","source_id":"saved-can-source-1","can_id":801,"extended":true,"start_elapsed_ms":250,"end_elapsed_ms":900,"frames":[{"data":[1,2]}]}}"#).is_err());
        assert!(decode_request(br#"{"action":"workbench","operation":{"kind":"start_saved_can_replay","preview_id":"old-preview"}}"#).is_err());
        assert!(matches!(decode_request(br#"{"action":"workbench","operation":{"kind":"start_saved_can_replay","preview_id":"p","expected_can_generation":4,"expected_output_capture_id":9}}"#).unwrap(), AgentRequest::Workbench {
            operation: AgentWorkbenchOperation::StartSavedCanReplay { expected_can_generation: 4, .. }
        }));
        let schema = serde_json::to_string(&schemars::schema_for!(AgentWorkbenchArgs)).unwrap();
        assert!(schema.contains("prepare_saved_can_replay"));
        assert!(schema.contains("expected_can_generation"));
        assert!(schema.contains("expected_output_capture_id"));
    }

    #[test]
    fn profile_resume_is_typed_and_bound_to_revision_and_connection() {
        let request = decode_request(br#"{"action":"vehicle_profiles","operation":{"kind":"apply_resume","preview_id":"profile-p","profile_revision":3,"obd_connection_generation":5}}"#).unwrap();
        assert!(matches!(
            request,
            AgentRequest::VehicleProfiles {
                operation: AgentVehicleProfileOperation::ApplyResume {
                    profile_revision: 3,
                    obd_connection_generation: 5,
                    ..
                }
            }
        ));
        assert!(decode_request(br#"{"action":"vehicle_profiles","operation":{"kind":"apply_resume","preview_id":"profile-p","profile_revision":3,"obd_connection_generation":5,"auto_request":true}}"#).is_err());
        assert!(decode_request(br#"{"action":"vehicle_profiles","operation":{"kind":"apply_resume","preview_id":"profile-p","obd_connection_generation":5}}"#).is_err());
        assert!(matches!(decode_request(br#"{"action":"connect_adapter","choice":{"kind":"simulator"},"log_signal_ids":[],"profile_standby":true}"#).unwrap(), AgentRequest::ConnectAdapter { profile_standby: true, .. }));
        assert!(matches!(
            decode_request(br#"{"action":"connect_adapter","choice":{"kind":"open_port2_subaru_ssm","provider_id":"tactrix-openport2"},"log_signal_ids":[],"profile_standby":false}"#).unwrap(),
            AgentRequest::ConnectAdapter { choice: AgentAdapterChoice::OpenPort2SubaruSsm { provider_id }, .. }
                if provider_id == "tactrix-openport2"
        ));
        let schema =
            serde_json::to_string(&schemars::schema_for!(AgentVehicleProfileArgs)).unwrap();
        assert!(schema.contains("apply_resume"));
        assert!(schema.contains("profile_revision"));
        assert!(schema.contains("recognize"));
    }

    #[test]
    fn offline_vin_and_history_operations_are_typed_and_do_not_accept_provenance_claims() {
        assert!(matches!(
            decode_request(br#"{"action":"vehicle_profiles","operation":{"kind":"read_identity","obd_connection_generation":8}}"#).unwrap(),
            AgentRequest::VehicleProfiles {
                operation: AgentVehicleProfileOperation::ReadIdentity { obd_connection_generation: 8 }
            }
        ));
        assert!(matches!(
            decode_request(br#"{"action":"vehicle_profiles","operation":{"kind":"list_history","filter":{"scope":"live","page_size":25}}}"#).unwrap(),
            AgentRequest::VehicleProfiles {
                operation: AgentVehicleProfileOperation::ListHistory { .. }
            }
        ));
        assert!(decode_request(
            br#"{"action":"vehicle_profiles","operation":{"kind":"set_manual_vehicle_detail","scope":"live","vin_digest":"vin:v1:sha256:abc","field":"make","value":"Toyota","note":null,"actor":"owner_ui","ecu_verified":true}}"#
        )
        .is_err());
        let schema =
            serde_json::to_string(&schemars::schema_for!(AgentVehicleProfileArgs)).unwrap();
        assert!(schema.contains("decode_vin_offline"));
        assert!(schema.contains("get_history_report"));
        assert!(schema.contains("cancel_history_import"));
    }

    #[test]
    fn stop_operation_is_a_typed_top_level_action() {
        assert!(matches!(
            decode_request(br#"{"action":"stop_operation"}"#).unwrap(),
            AgentRequest::StopOperation
        ));
    }

    #[test]
    fn candlelight_obd_operations_are_typed_for_the_full_normal_logging_flow() {
        for payload in [
            br#"{"action":"can_obd","operation":{"kind":"discover"}}"#.as_slice(),
            br#"{"action":"can_obd","operation":{"kind":"identify","selection":{"id":"1:2","interface_number":0,"channel":0}}}"#.as_slice(),
            br#"{"action":"can_obd","operation":{"kind":"connect","selection":{"id":"1:2","interface_number":0,"channel":0},"log_signal_ids":["engine_rpm"],"graph_signal_ids":["engine_rpm"]}}"#.as_slice(),
            br#"{"action":"can_obd","operation":{"kind":"select","log_signal_ids":["engine_rpm"],"graph_signal_ids":["engine_rpm"]}}"#.as_slice(),
            br#"{"action":"can_obd","operation":{"kind":"status"}}"#.as_slice(),
            br#"{"action":"can_obd","operation":{"kind":"disconnect"}}"#.as_slice(),
            br#"{"action":"can_obd","operation":{"kind":"cancel"}}"#.as_slice(),
        ] {
            assert!(matches!(decode_request(payload), Ok(AgentRequest::CanObd { .. })));
        }
        assert!(decode_request(br#"{"action":"can_obd","operation":{"kind":"connect","selection":{"id":"1:2","interface_number":0,"channel":0},"log_signal_ids":["engine_rpm"],"graph_signal_ids":[],"auto_request":true}}"#).is_err());
        let schema = serde_json::to_string(&schemars::schema_for!(AgentCanObdArgs)).unwrap();
        assert!(schema.contains("discover"));
        assert!(schema.contains("identify"));
        assert!(schema.contains("connect"));
        assert!(schema.contains("select"));
        assert!(schema.contains("status"));
        assert!(schema.contains("disconnect"));
        assert!(schema.contains("cancel"));
        let choice_schema =
            serde_json::to_string(&schemars::schema_for!(AgentAdapterChoice)).unwrap();
        assert!(choice_schema.contains("candlelight_obd"));
    }
}
