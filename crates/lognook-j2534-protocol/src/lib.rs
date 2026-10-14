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
pub struct J2534ProviderDiscovery {
    pub providers: Vec<J2534Provider>,
    pub diagnostics: Vec<String>,
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
#[serde(deny_unknown_fields)]
pub struct Mode09Payload {
    pub ecu_id: String,
    #[serde(with = "hex_byte_vec")]
    pub bytes: Vec<u8>,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mode09MalformedFrame {
    pub ecu_id: Option<String>,
    #[serde(with = "hex_byte_vec")]
    pub raw_bytes: Vec<u8>,
    pub latency_us: u64,
    pub protocol_id: u32,
    pub rx_status: u32,
    pub declared_data_size: u32,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mode09Failure {
    pub kind: WorkerErrorKind,
    pub message: String,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Mode09Response {
    Payload(Mode09Payload),
    MalformedFrame(Mode09MalformedFrame),
    Failure(Mode09Failure),
}

mod hex_byte_vec {
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write;
            write!(hex, "{byte:02X}").expect("writing into a string cannot fail");
        }
        serializer.serialize_str(&hex)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex = String::deserialize(deserializer)?;
        if hex.len() % 2 != 0 {
            return Err(de::Error::custom(
                "hex byte string must have an even length",
            ));
        }
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        for pair in hex.as_bytes().chunks_exact(2) {
            let high =
                digit(pair[0]).ok_or_else(|| de::Error::custom("invalid hex byte string"))?;
            let low = digit(pair[1]).ok_or_else(|| de::Error::custom("invalid hex byte string"))?;
            bytes.push((high << 4) | low);
        }
        Ok(bytes)
    }

    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
}

pub const fn is_supported_mode09_pid(pid: u8) -> bool {
    matches!(pid, 0x02 | 0x04 | 0x0A)
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
    ReadMode09 { pid: u8 },
    Disconnect,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerResponse {
    Progress {
        stage: String,
        detail: Option<String>,
        elapsed_ms: Option<u64>,
    },
    Providers {
        providers: Vec<J2534Provider>,
        diagnostics: Vec<String>,
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
    Mode09Responses {
        responses: Vec<Mode09Response>,
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
    if let WorkerRequest::ReadMode09 { pid } = request
        && !is_supported_mode09_pid(*pid)
    {
        return Err(ProtocolError::Malformed(format!(
            "Mode 09 PID {pid:02X} is not an identity PID"
        )));
    }
    encode_line(request)
}

pub fn decode_request_line(line: &[u8]) -> Result<WorkerRequest, ProtocolError> {
    let value = parse_json_line(line)?;
    let kind = message_type(&value)?;
    let fields = match kind {
        "list_providers" | "discover_mode01" | "disconnect" => &["type"][..],
        "identify" | "connect" => &["type", "provider_id"][..],
        "read_mode01" => &["type", "pid"][..],
        "read_mode09" => &["type", "pid"][..],
        other => {
            return Err(ProtocolError::Malformed(format!(
                "unknown request type '{other}'"
            )));
        }
    };
    validate_fields(&value, fields)?;
    let request: WorkerRequest = decode_value(value)?;
    if let WorkerRequest::ReadMode09 { pid } = &request
        && !is_supported_mode09_pid(*pid)
    {
        return Err(ProtocolError::Malformed(format!(
            "Mode 09 PID {pid:02X} is not an identity PID"
        )));
    }
    Ok(request)
}

pub fn encode_response_line(response: &WorkerResponse) -> Result<Vec<u8>, ProtocolError> {
    encode_line(response)
}

pub fn decode_response_line(line: &[u8]) -> Result<WorkerResponse, ProtocolError> {
    let value = parse_json_line(line)?;
    let kind = message_type(&value)?;
    let fields = match kind {
        "progress" => &["type", "stage", "detail", "elapsed_ms"][..],
        "disconnected" => &["type"][..],
        "providers" => &["type", "providers", "diagnostics"][..],
        "identity" | "connected" => &["type", "identity"][..],
        "mode01_responses" => &["type", "responses"][..],
        "mode09_responses" => &["type", "responses"][..],
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

#[cfg(test)]
mod tests {
    use super::{
        MAX_MESSAGE_BYTES, Mode09Failure, Mode09MalformedFrame, Mode09Payload, Mode09Response,
        ProtocolError, WorkerErrorKind, WorkerRequest, WorkerResponse, decode_request_line,
        decode_response_line, encode_request_line, encode_response_line,
    };

    #[test]
    fn mode09_request_and_response_round_trip_for_identity_pids() {
        for pid in [0x02, 0x04, 0x0A] {
            let request = WorkerRequest::ReadMode09 { pid };
            let encoded = encode_request_line(&request).unwrap();
            assert_eq!(decode_request_line(&encoded).unwrap(), request);
        }

        let response = WorkerResponse::Mode09Responses {
            responses: vec![
                Mode09Response::Payload(Mode09Payload {
                    ecu_id: "7E8".into(),
                    bytes: vec![0x49, 0x02, 0x01, b'1'],
                    latency_us: 12_500,
                }),
                Mode09Response::MalformedFrame(Mode09MalformedFrame {
                    ecu_id: None,
                    raw_bytes: vec![0x00, 0x00, 0x07, 0xE9, 0x49, 0x02],
                    latency_us: 13_000,
                    protocol_id: 5,
                    rx_status: 0,
                    declared_data_size: 6,
                    detail: "Malformed J2534 frame".into(),
                }),
                Mode09Response::Failure(Mode09Failure {
                    kind: WorkerErrorKind::PassThruFailed,
                    message: "fixture trailing read failure".into(),
                    latency_us: 14_000,
                }),
            ],
        };
        let encoded = encode_response_line(&response).unwrap();
        assert_eq!(decode_response_line(&encoded).unwrap(), response);
    }

    #[test]
    fn mode09_rejects_non_identity_pids_and_malformed_or_unknown_fields() {
        assert!(matches!(
            encode_request_line(&WorkerRequest::ReadMode09 { pid: 0x03 }),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(decode_request_line(br#"{"type":"read_mode09","pid":3}"#).is_err());
        assert!(decode_request_line(br#"{"type":"read_mode09",}"#).is_err());
        assert!(decode_request_line(br#"{"type":"read_mode09","pid":2,"extra":true}"#).is_err());
        assert!(
            decode_response_line(br#"{"type":"mode09_responses","responses":[],"extra":true}"#)
                .is_err()
        );
        assert!(decode_response_line(
            br#"{"type":"mode09_responses","responses":[{"type":"payload","payload":{"ecu_id":"7E8","bytes":"4902","latency_us":1,"extra":true}}]}"#
        )
        .is_err());
    }

    #[test]
    fn mode09_full_j2534_frame_fits_compact_wire_encoding() {
        let response = WorkerResponse::Mode09Responses {
            responses: vec![Mode09Response::MalformedFrame(Mode09MalformedFrame {
                ecu_id: None,
                raw_bytes: (0..4_128).map(|byte| byte as u8).collect(),
                latency_us: 10_000,
                protocol_id: 5,
                rx_status: 0,
                declared_data_size: 4_128,
                detail: "bad protocol".into(),
            })],
        };

        let encoded = encode_response_line(&response).unwrap();

        assert!(encoded.len() < MAX_MESSAGE_BYTES);
        assert_eq!(decode_response_line(&encoded).unwrap(), response);
    }
}
