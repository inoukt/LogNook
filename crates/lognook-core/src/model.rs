use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, path::PathBuf};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct DeviceDescriptor {
    pub stable_id: String,
    pub hardware_family: String,
    pub firmware: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SignalDefinition {
    pub id: String,
    pub name: String,
    pub service: u8,
    pub pid: u8,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConnectionCapabilities {
    pub device: DeviceDescriptor,
    pub protocol: String,
    pub read_only: bool,
    pub supported_signals: Vec<SignalDefinition>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleQuality {
    Valid,
    Stale,
    Missing,
    Timeout,
    ParseError,
    Disconnected,
    Suspect,
}

impl SampleQuality {
    pub(crate) fn as_db_str(&self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Timeout => "timeout",
            Self::ParseError => "parse_error",
            Self::Disconnected => "disconnected",
            Self::Suspect => "suspect",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "valid" => Some(Self::Valid),
            "stale" => Some(Self::Stale),
            "missing" => Some(Self::Missing),
            "timeout" => Some(Self::Timeout),
            "parse_error" => Some(Self::ParseError),
            "disconnected" => Some(Self::Disconnected),
            "suspect" => Some(Self::Suspect),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleOrigin {
    Live,
    Replay,
}

impl SampleOrigin {
    pub(crate) fn as_db_str(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Replay => "replay",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "live" => Some(Self::Live),
            "replay" => Some(Self::Replay),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Sample {
    pub elapsed_ns: i64,
    pub wall_time_unix_ns: i64,
    pub source_id: String,
    pub ecu_id: Option<String>,
    pub signal_id: String,
    pub value: f64,
    pub unit: String,
    pub quality: SampleQuality,
    pub origin: SampleOrigin,
    pub raw_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct OemPollingStatus {
    pub active: bool,
    pub vehicle_id: Option<String>,
    pub signal_count: usize,
    pub request_count: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub failed_signals: u64,
    pub samples_published: u64,
    pub observed_request_rate_hz: Option<f64>,
    pub last_latency_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Marker {
    pub id: Option<i64>,
    pub elapsed_ns: i64,
    pub wall_time_unix_ns: i64,
    pub label: String,
    pub note: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct DerivedSignalDefinition {
    pub id: String,
    pub name: String,
    pub expression: String,
    pub unit: String,
    pub tare_offset: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerDirection {
    Above,
    Below,
}

impl TriggerDirection {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Below => "below",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AutomaticTriggerConfig {
    pub signal_id: String,
    pub direction: TriggerDirection,
    pub threshold: f64,
    pub hysteresis: f64,
    pub hold_ms: u64,
    pub pre_trigger_ms: u64,
}

impl AutomaticTriggerConfig {
    pub(crate) fn normalized(&self) -> Result<Self, LogNookError> {
        let signal_id = self.signal_id.trim();
        if signal_id.is_empty() || signal_id.len() > 128 {
            return Err(LogNookError::new("Choose a valid trigger signal"));
        }
        if !self.threshold.is_finite() || !self.hysteresis.is_finite() {
            return Err(LogNookError::new("Trigger values must be finite numbers"));
        }
        Ok(Self {
            signal_id: signal_id.to_owned(),
            direction: self.direction.clone(),
            threshold: self.threshold.clamp(-1_000_000_000.0, 1_000_000_000.0),
            hysteresis: self.hysteresis.clamp(0.0, 1_000_000_000.0),
            hold_ms: self.hold_ms.min(60_000),
            pre_trigger_ms: self.pre_trigger_ms.min(30_000),
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct AutomaticTriggerStatus {
    pub armed: bool,
    pub recording: bool,
    pub config: Option<AutomaticTriggerConfig>,
    pub recording_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub samples: Vec<Sample>,
    pub markers: Vec<Marker>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingSummary {
    pub path: PathBuf,
    pub sample_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RetrospectiveRecordingStart {
    pub backfilled_count: usize,
    pub omitted_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingSignal {
    pub id: String,
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingVehicleContextV1 {
    pub snapshot: crate::vehicle_profiles::VehicleIdentitySnapshot,
    pub context_from_elapsed_ns: i64,
    #[serde(default)]
    pub context_from_sample_index: u64,
}

impl RecordingVehicleContextV1 {
    pub fn applies_to_sample(&self, sample_index: u64, elapsed_ns: i64) -> bool {
        sample_index >= self.context_from_sample_index && elapsed_ns >= self.context_from_elapsed_ns
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingVehicleContextSummary {
    pub scope: crate::vehicle_profiles::ProfileScope,
    pub vin_digest: Option<String>,
    pub masked_vin: Option<String>,
    pub identity_source: crate::vehicle_profiles::VehicleIdentitySource,
    pub vin_conflict: bool,
    pub has_full_vin: bool,
}

impl From<&RecordingVehicleContextV1> for RecordingVehicleContextSummary {
    fn from(context: &RecordingVehicleContextV1) -> Self {
        let snapshot = &context.snapshot;
        Self {
            scope: snapshot.scope,
            vin_digest: snapshot.vin_digest.clone(),
            masked_vin: snapshot.masked_vin.clone(),
            identity_source: snapshot.identity_source,
            vin_conflict: snapshot.vin_conflict,
            has_full_vin: snapshot.full_vin.is_some(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingInfo {
    pub sample_count: usize,
    pub duration_ns: i64,
    pub signals: Vec<RecordingSignal>,
    pub markers: Vec<Marker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle_context: Option<RecordingVehicleContextV1>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PlaybackState {
    pub session_id: u64,
    pub position_ns: i64,
    pub duration_ns: i64,
    pub speed: f64,
    pub playing: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum EngineEvent {
    Connected(ConnectionCapabilities),
    Disconnected,
    Sample(Sample),
    PlaybackSample {
        session_id: u64,
        sample: Sample,
    },
    CanFrames(Vec<crate::CanFrame>),
    CanScanResult(crate::CanScanResult),
    CanScanStarted(crate::CanScanConfig),
    CanScanStopped,
    CanWorkbenchStatus(crate::CanWorkbenchStatus),
    CanIdReplayStatus(crate::CanIdReplayStatus),
    CanLiveEventAnalysis(crate::CanLiveEventSnapshot),
    CanScanCompleted(crate::CanScanConfig),
    CanTxStatus {
        capture_id: u64,
        sequence: u64,
        echo_id: u32,
        frame: crate::CanFrame,
        confirmed: bool,
    },
    CanTransportError(crate::CanTransportError),
    CanTriggerCapture(crate::CanTriggerCaptureResult),
    Marker(Marker),
    TriggerStatus(AutomaticTriggerStatus),
    PlaybackMarker {
        session_id: u64,
        marker: Marker,
    },
    RecordingFailed(RecordingSummary),
    Error(String),
    PlaybackStarted,
    PlaybackFinished,
    PlaybackState(PlaybackState),
}

#[derive(Debug)]
pub struct LogNookError {
    message: String,
    transient: bool,
    signal: bool,
}

impl LogNookError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            transient: false,
            signal: false,
        }
    }

    pub(crate) fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            transient: true,
            signal: false,
        }
    }

    pub(crate) fn signal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            transient: false,
            signal: true,
        }
    }

    pub(crate) fn transient_signal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            transient: true,
            signal: true,
        }
    }

    pub(crate) fn is_transient(&self) -> bool {
        self.transient
    }

    pub(crate) fn is_signal(&self) -> bool {
        self.signal
    }
}

impl fmt::Display for LogNookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for LogNookError {}

impl From<std::io::Error> for LogNookError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<rusqlite::Error> for LogNookError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<csv::Error> for LogNookError {
    fn from(error: csv::Error) -> Self {
        Self::new(error.to_string())
    }
}
