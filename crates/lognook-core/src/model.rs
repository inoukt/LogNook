use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, path::PathBuf};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Marker {
    pub elapsed_ns: i64,
    pub wall_time_unix_ns: i64,
    pub label: String,
    pub note: String,
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
pub struct RecordingSignal {
    pub id: String,
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingInfo {
    pub sample_count: usize,
    pub duration_ns: i64,
    pub signals: Vec<RecordingSignal>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum EngineEvent {
    Connected(ConnectionCapabilities),
    Disconnected,
    Sample(Sample),
    Marker(Marker),
    RecordingFailed(RecordingSummary),
    Error(String),
    PlaybackStarted,
    PlaybackFinished,
}

#[derive(Debug)]
pub struct LogNookError(String);

impl LogNookError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for LogNookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for LogNookError {}

impl From<std::io::Error> for LogNookError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<rusqlite::Error> for LogNookError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<csv::Error> for LogNookError {
    fn from(error: csv::Error) -> Self {
        Self(error.to_string())
    }
}
