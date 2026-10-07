mod a30;
mod acquisition;
mod adapter;
mod bluetooth_spp;
mod can;
mod can_analysis;
mod can_capture;
mod can_live_events;
mod can_workbench;
pub mod candlelight;
pub mod communication_trace;
mod csv_import;
mod csv_io;
mod derived;
mod diagnostics;
mod elm327;
mod engine;
mod graph_window;
mod model;
mod obdlink_sx;
mod obdlink_sx_protocol;
mod oem_catalog;
mod openport2;
mod pipeline;
mod pipeline_worker;
mod playback;
mod recording;
mod recording_view;
mod research;
mod session;
mod simulator;

pub use a30::{
    A30Adapter, A30CanReply, A30Decoder, A30Frame, A30GraphValue, A30ReadOutcome, A30ReadTest,
    A30ReadTestResult, A30Session, identify_a30, identify_a30_with_diagnostics,
    identify_a30_with_sinks, run_a30_read_test, try_a30_oem_candidate,
};
pub use adapter::HardwareAdapter;
pub use bluetooth_spp::{
    BluetoothByteStream, BluetoothSppDevice, SERIAL_PORT_SERVICE_CLASS_UUID, connect_bluetooth_spp,
    format_bluetooth_address, paired_spp_devices, parse_bluetooth_address,
};
pub use can::{CanByteOrder, CanDirection, CanFrame, CanSignalDefinition, decode_can_signal};
pub use can_analysis::{
    CanAnalysisConfig, CanAnalyzer, CanByteDistributionComparison, CanByteSummary, CanCensusRow,
    CanCorrelationHypothesis, CanDiagnosticOutcome, CanDiagnosticRoute, CanEventByteChange,
    CanEventByteTransition, CanEventCandidate, CanEventCorrelation, CanEventCorrelator,
    CanIdComparison, CanManualEventMark, CanRequestTiming, CanTriggerCapture, CanTriggerConfig,
    CanTriggerRecorder, can_dbc_skeleton, compare_can_census, correlate_can_events,
};
pub use can_capture::{CanCaptureReader, CanCaptureRowError, CanCaptureWriter, replay_can_capture};
pub use can_live_events::{
    CanLiveEventCandidate, CanLiveEventGroup, CanLiveEventSnapshot, LiveCanEventAnalyzer,
};
pub use can_workbench::{
    CanDeviceCandidate, CanDeviceSelection, CanEvidenceRecord, CanEvidenceScanConfig,
    CanEvidenceScanResult, CanScanConfig, CanScanMode, CanScanResult, CanScanStopReason,
    CanTransport, CanTransportError, CanTransportErrorKind, CanTransportEvent,
    CanTriggerCaptureResult, CanTriggerEndReason, CanWorkbench, CanWorkbenchConfig,
    CanWorkbenchStatus, available_can_adapters, can_evidence_path, oem_candidate_scan_config,
    open_candlelight, replay_can_capture_file,
};
pub use csv_import::import_csv_to_recording;
pub use csv_io::{export_csv, export_recording_csv, import_csv};
pub use diagnostics::{
    DiagnosticPhase, DiagnosticProgress, DiagnosticSink, diagnostic_error_detail,
};
pub use elm327::{
    Elm327Adapter, identify_elm327, identify_elm327_with_diagnostics, identify_elm327_with_sinks,
    try_elm_oem_candidate,
};
pub use engine::LogNookEngine;
pub use graph_window::{
    query_recording_averages, query_recording_values_at, query_recording_window,
};
pub use lognook_j2534_protocol::{J2534Provider, J2534ProviderDiscovery};
pub use model::{
    AutomaticTriggerConfig, AutomaticTriggerStatus, ConnectionCapabilities,
    DerivedSignalDefinition, DeviceDescriptor, EngineEvent, LogNookError, Marker, PlaybackState,
    Recording, RecordingInfo, RecordingSignal, RecordingSummary, Sample, SampleOrigin,
    SampleQuality, SignalDefinition, TriggerDirection,
};
pub use obdlink_sx::{
    ObdLinkSxAdapter, OemCandidateDecodedValue, OemCandidateOutcome, OemCandidateResponse,
    OemCandidateTryResult, ScionMode21DecodedSignal, ScionMode21ProbeResult,
    available_obdlink_sx_ports, build_oem_candidate_command, decode_oem_candidate_response,
    identify_obdlink_sx, identify_obdlink_sx_with_diagnostics, identify_obdlink_sx_with_sinks,
    probe_scion_xb_mode21, try_oem_candidate,
};
pub use obdlink_sx_protocol::{
    Mode01Response, SupportedPidRange, decode_signal, parse_mode01_payload,
    parse_supported_pid_payload,
};
pub use oem_catalog::{
    OemCandidate, OemCandidateTrySpec, OemCatalog, OemCatalogPage, SCION_XB_BLOB_SHA,
    ScionMode21ProbeSpec, ScionProbeChannel,
};
pub use openport2::{
    OpenPort2Adapter, available_openport2_providers,
    available_openport2_providers_with_diagnostics, available_openport2_providers_with_sinks,
    identify_openport2, identify_openport2_with_diagnostics, identify_openport2_with_sinks,
};
pub use pipeline::SamplePublisher;
pub use recording::{RecordingWriter, read_recording};
pub use recording_view::{
    add_recording_marker, delete_recording_marker, inspect_recording, update_recording_marker,
};
pub use research::{
    ResearchRunItem, ResearchRunItemStatus, ResearchRunReport, ResearchTestRequest,
    ResearchTestResult, ResearchTestSelection,
};
pub use simulator::{SimulatedObdAdapter, standard_obd_signals};
