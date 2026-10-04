use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fmt;

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct J2534Provider {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct J2534Identity {
    pub firmware: String,
    pub dll_version: String,
    pub api_version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mode01Payload {
    pub ecu_id: String,
    pub bytes: Vec<u8>,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerErrorKind {
    ProviderNotFound,
    DllNotFound,
    ProviderOpenFailed,
    UnsupportedApi,
    ChannelFailed,
    PassThruFailed,
    NoResponse,
    Timeout,
    MalformedResponse,
    MultipleResponders,
    UnsupportedArchitecture,
    IpcFailure,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerRequest {
    ListProviders,
    Identify { provider_id: String },
    Connect { provider_id: String },
    DiscoverMode01,
    ReadMode01 { pid: u8 },
    Disconnect,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerResponse {
    Providers {
        providers: Vec<J2534Provider>,
    },
    Identity {
        identity: J2534Identity,
    },
    Connected {
        identity: J2534Identity,
    },
    Mode01Responses {
        responses: Vec<Mode01Payload>,
    },
    Disconnected,
    Error {
        kind: WorkerErrorKind,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    TooLarge,
    Malformed(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(
                formatter,
                "J2534 worker message exceeds {MAX_MESSAGE_BYTES} bytes"
            ),
            Self::Malformed(message) => {
                write!(formatter, "Malformed J2534 worker message: {message}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

pub fn encode_request_line(request: &WorkerRequest) -> Result<Vec<u8>, ProtocolError> {
    encode_line(request)
}

pub fn decode_request_line(line: &[u8]) -> Result<WorkerRequest, ProtocolError> {
    let value = parse_json_line(line)?;
    let kind = message_type(&value)?;
    let fields = match kind {
        "list_providers" | "discover_mode01" | "disconnect" => &["type"][..],
        "identify" | "connect" => &["type", "provider_id"][..],
        "read_mode01" => &["type", "pid"][..],
        other => {
            return Err(ProtocolError::Malformed(format!(
                "unknown request type '{other}'"
            )));
        }
    };
    validate_fields(&value, fields)?;
    decode_value(value)
}

pub fn encode_response_line(response: &WorkerResponse) -> Result<Vec<u8>, ProtocolError> {
    encode_line(response)
}

pub fn decode_response_line(line: &[u8]) -> Result<WorkerResponse, ProtocolError> {
    let value = parse_json_line(line)?;
    let kind = message_type(&value)?;
    let fields = match kind {
        "disconnected" => &["type"][..],
        "providers" => &["type", "providers"][..],
        "identity" | "connected" => &["type", "identity"][..],
        "mode01_responses" => &["type", "responses"][..],
        "error" => &["type", "kind", "message"][..],
        other => {
            return Err(ProtocolError::Malformed(format!(
                "unknown response type '{other}'"
            )));
        }
    };
    validate_fields(&value, fields)?;
    decode_value(value)
}

fn encode_line<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let mut line =
        serde_json::to_vec(value).map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if line.len().saturating_add(1) > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    line.push(b'\n');
    Ok(line)
}

fn parse_json_line(line: &[u8]) -> Result<Value, ProtocolError> {
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let content = line.strip_suffix(b"\n").unwrap_or(line);
    let content = content.strip_suffix(b"\r").unwrap_or(content);
    if content.is_empty() {
        return Err(ProtocolError::Malformed("empty line".into()));
    }
    serde_json::from_slice(content).map_err(|error| ProtocolError::Malformed(error.to_string()))
}

fn message_type(value: &Value) -> Result<&str, ProtocolError> {
    value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::Malformed("missing string field 'type'".into()))
}

fn validate_fields(value: &Value, allowed: &[&str]) -> Result<(), ProtocolError> {
    let Some(object) = value.as_object() else {
        return Err(ProtocolError::Malformed(
            "message must be a JSON object".into(),
        ));
    };
    if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(ProtocolError::Malformed(format!(
            "unknown field '{unknown}'"
        )));
    }
    Ok(())
}

fn decode_value<T: DeserializeOwned>(value: Value) -> Result<T, ProtocolError> {
    serde_json::from_value(value).map_err(|error| ProtocolError::Malformed(error.to_string()))
}
