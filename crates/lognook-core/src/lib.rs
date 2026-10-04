mod acquisition;
mod adapter;
mod csv_import;
mod csv_io;
mod derived;
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
mod session;
mod simulator;

pub use adapter::HardwareAdapter;
pub use csv_import::import_csv_to_recording;
pub use csv_io::{export_csv, export_recording_csv, import_csv};
pub use engine::LogNookEngine;
pub use graph_window::{
    query_recording_averages, query_recording_values_at, query_recording_window,
};
pub use lognook_j2534_protocol::J2534Provider;
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
    identify_obdlink_sx, probe_scion_xb_mode21, try_oem_candidate,
};
pub use obdlink_sx_protocol::{
    Mode01Response, SupportedPidRange, decode_signal, parse_mode01_payload,
    parse_supported_pid_payload,
};
pub use oem_catalog::{
    OemCandidate, OemCandidateTrySpec, OemCatalog, OemCatalogPage, SCION_XB_BLOB_SHA,
    ScionMode21ProbeSpec, ScionProbeChannel,
};
pub use openport2::{OpenPort2Adapter, available_openport2_providers, identify_openport2};
pub use recording::{RecordingWriter, read_recording};
pub use recording_view::{
    add_recording_marker, delete_recording_marker, inspect_recording, update_recording_marker,
};
pub use simulator::{SimulatedObdAdapter, standard_obd_signals};
