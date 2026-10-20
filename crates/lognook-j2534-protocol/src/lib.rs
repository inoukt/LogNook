use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fmt;

pub mod subaru_ssm;

// A functional DTC request can receive full ISO-TP payloads from eight
// standard responders. Hex-encoded bytes need a bounded IPC allowance above
// 16 KiB so those raw per-ECU results are not silently replaced by an error.
pub const MAX_MESSAGE_BYTES: usize = 128 * 1024;
// At most one pending response, one terminal ECU outcome, and one filter cleanup failure.
pub const MAX_OEM_CANDIDATE_RESPONSES: usize = 3;
pub const MAX_OEM_CANDIDATE_RAW_BYTES: usize = 4_128;
pub const MAX_SUBARU_SSM_ADDRESSES: usize = 256;
pub const MAX_SUBARU_SSM_CANDIDATES: usize = 64;

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
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OemCandidateRequestKind {
    Mode21 { identifier: u8 },
    Mode22 { did: u16 },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OemCandidatePayload {
    pub response_id: u16,
    #[serde(with = "hex_byte_vec")]
    pub bytes: Vec<u8>,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OemCandidateMalformedFrame {
    pub response_id: Option<u16>,
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
pub struct OemCandidateFailure {
    pub request_id: u16,
    pub expected_response_id: u16,
    pub kind: WorkerErrorKind,
    pub message: String,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubaruSsmReadPayload {
    pub ecu_address: u8,
    pub tool_address: u8,
    pub addresses: Vec<u32>,
    #[serde(with = "hex_byte_vec")]
    pub values: Vec<u8>,
    pub init_frame: Option<String>,
    pub read_frames: Vec<String>,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OemCandidateResponse {
    Payload(OemCandidatePayload),
    MalformedFrame(OemCandidateMalformedFrame),
    Failure(OemCandidateFailure),
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

/// The only standard diagnostic services accepted by the OpenPort worker.
/// `Clear` is used only by the separate explicit clear operation.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardDtcService {
    Stored,
    Pending,
    Permanent,
    Clear,
}

impl StandardDtcService {
    pub const fn request_byte(self) -> u8 {
        match self {
            Self::Stored => 0x03,
            Self::Pending => 0x07,
            Self::Permanent => 0x0A,
            Self::Clear => 0x04,
        }
    }

    pub const fn positive_response_byte(self) -> u8 {
        self.request_byte().wrapping_add(0x40)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StandardDtcPayload {
    pub ecu_id: String,
    #[serde(with = "hex_byte_vec")]
    pub bytes: Vec<u8>,
    pub latency_us: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StandardDtcMalformedFrame {
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
pub struct StandardDtcFailure {
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
pub enum StandardDtcResponse {
    Payload(StandardDtcPayload),
    MalformedFrame(StandardDtcMalformedFrame),
    Failure(StandardDtcFailure),
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
    Identify {
        provider_id: String,
    },
    Connect {
        provider_id: String,
    },
    ConnectSubaruSsm {
        provider_id: String,
    },
    DiscoverMode01,
    ReadMode01 {
        pid: u8,
    },
    ReadMode09 {
        pid: u8,
    },
    ReadMode06 {
        test_id: u8,
    },
    ReadStandardDtc {
        service: StandardDtcService,
    },
    ReadOemCandidate {
        candidate_id: String,
        source_id: String,
        source_blob_sha: String,
        source_url: String,
        license: String,
        license_url: String,
        request_id: u16,
        response_id: u16,
        request: OemCandidateRequestKind,
    },
    ReadSubaruCandidate {
        candidate_id: String,
        source_id: String,
        catalog_sha256: String,
        request_id: u16,
        response_id: u16,
        request: OemCandidateRequestKind,
    },
    ReadSubaruSsmAddresses {
        candidate_ids: Vec<String>,
        source_id: String,
        catalog_sha256: String,
        ecu_address: u8,
        tool_address: u8,
        addresses: Vec<u32>,
    },
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
    Mode06Responses {
        // Standard PID reads share the address/raw-payload/failure envelope;
        // core decoding uses the original Mode 06 query rather than Mode 09 rules.
        responses: Vec<Mode09Response>,
    },
    StandardDtcResponses {
        responses: Vec<StandardDtcResponse>,
    },
    OemCandidateResponses {
        responses: Vec<OemCandidateResponse>,
    },
    SubaruSsmRead {
        payload: SubaruSsmReadPayload,
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
    if let WorkerRequest::ReadOemCandidate {
        candidate_id,
        source_id,
        source_blob_sha,
        source_url,
        license,
        license_url,
        request_id,
        response_id,
        ..
    } = request
    {
        validate_oem_candidate_request_fields(
            candidate_id,
            source_id,
            source_blob_sha,
            source_url,
            license,
            license_url,
            *request_id,
            *response_id,
        )?;
    }
    if let WorkerRequest::ReadSubaruCandidate {
        candidate_id,
        source_id,
        catalog_sha256,
        request_id,
        response_id,
        ..
    } = request
    {
        validate_subaru_catalog_request_fields(
            candidate_id,
            source_id,
            catalog_sha256,
            *request_id,
            *response_id,
        )?;
    }
    if let WorkerRequest::ReadSubaruSsmAddresses {
        candidate_ids,
        source_id,
        catalog_sha256,
        ecu_address,
        tool_address,
        addresses,
    } = request
    {
        validate_subaru_ssm_request_fields(
            candidate_ids,
            source_id,
            catalog_sha256,
            *ecu_address,
            *tool_address,
            addresses,
        )?;
    }
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
        "identify" | "connect" | "connect_subaru_ssm" => &["type", "provider_id"][..],
        "read_mode01" => &["type", "pid"][..],
        "read_mode09" => &["type", "pid"][..],
        "read_mode06" => &["type", "test_id"][..],
        "read_standard_dtc" => &["type", "service"][..],
        "read_oem_candidate" => &[
            "type",
            "candidate_id",
            "source_id",
            "source_blob_sha",
            "source_url",
            "license",
            "license_url",
            "request_id",
            "response_id",
            "request",
        ][..],
        "read_subaru_candidate" => &[
            "type",
            "candidate_id",
            "source_id",
            "catalog_sha256",
            "request_id",
            "response_id",
            "request",
        ][..],
        "read_subaru_ssm_addresses" => &[
            "type",
            "candidate_ids",
            "source_id",
            "catalog_sha256",
            "ecu_address",
            "tool_address",
            "addresses",
        ][..],
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
    if let WorkerRequest::ReadOemCandidate {
        candidate_id,
        source_id,
        source_blob_sha,
        source_url,
        license,
        license_url,
        request_id,
        response_id,
        ..
    } = &request
    {
        validate_oem_candidate_request_fields(
            candidate_id,
            source_id,
            source_blob_sha,
            source_url,
            license,
            license_url,
            *request_id,
            *response_id,
        )?;
    }
    if let WorkerRequest::ReadSubaruCandidate {
        candidate_id,
        source_id,
        catalog_sha256,
        request_id,
        response_id,
        ..
    } = &request
    {
        validate_subaru_catalog_request_fields(
            candidate_id,
            source_id,
            catalog_sha256,
            *request_id,
            *response_id,
        )?;
    }
    if let WorkerRequest::ReadSubaruSsmAddresses {
        candidate_ids,
        source_id,
        catalog_sha256,
        ecu_address,
        tool_address,
        addresses,
    } = &request
    {
        validate_subaru_ssm_request_fields(
            candidate_ids,
            source_id,
            catalog_sha256,
            *ecu_address,
            *tool_address,
            addresses,
        )?;
    }
    Ok(request)
}

pub fn encode_response_line(response: &WorkerResponse) -> Result<Vec<u8>, ProtocolError> {
    if let WorkerResponse::OemCandidateResponses { responses } = response {
        validate_oem_candidate_responses(responses)?;
    }
    if let WorkerResponse::SubaruSsmRead { payload } = response {
        validate_subaru_ssm_payload(payload)?;
    }
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
        "mode06_responses" => &["type", "responses"][..],
        "standard_dtc_responses" => &["type", "responses"][..],
        "oem_candidate_responses" => &["type", "responses"][..],
        "subaru_ssm_read" => &["type", "payload"][..],
        "error" => &["type", "kind", "message"][..],
        other => {
            return Err(ProtocolError::Malformed(format!(
                "unknown response type '{other}'"
            )));
        }
    };
    validate_fields(&value, fields)?;
    let response: WorkerResponse = decode_value(value)?;
    if let WorkerResponse::OemCandidateResponses { responses } = &response {
        validate_oem_candidate_responses(responses)?;
    }
    if let WorkerResponse::SubaruSsmRead { payload } = &response {
        validate_subaru_ssm_payload(payload)?;
    }
    Ok(response)
}

pub fn validate_oem_candidate_request_fields(
    candidate_id: &str,
    source_id: &str,
    source_blob_sha: &str,
    source_url: &str,
    license: &str,
    license_url: &str,
    request_id: u16,
    response_id: u16,
) -> Result<(), ProtocolError> {
    if [
        candidate_id,
        source_id,
        source_blob_sha,
        source_url,
        license,
        license_url,
    ]
    .iter()
    .any(|value| value.trim().is_empty() || value.chars().any(char::is_control))
    {
        return Err(ProtocolError::Malformed(
            "OEM candidate request requires source, revision, URL, and license provenance".into(),
        ));
    }
    if candidate_id.len() > 512
        || source_id.len() > 256
        || source_blob_sha.len() != 40
        || !source_blob_sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        || source_url.len() > 2_048
        || license.len() > 128
        || license_url.len() > 2_048
    {
        return Err(ProtocolError::Malformed(
            "OEM candidate provenance fields are malformed or exceed their bounds".into(),
        ));
    }
    if request_id > 0x7FF || response_id > 0x7FF || request_id == response_id {
        return Err(ProtocolError::Malformed(
            "OEM candidate routes must use distinct explicit 11-bit CAN identifiers".into(),
        ));
    }
    Ok(())
}

pub fn validate_subaru_catalog_request_fields(
    candidate_id: &str,
    source_id: &str,
    catalog_sha256: &str,
    request_id: u16,
    response_id: u16,
) -> Result<(), ProtocolError> {
    let candidate_hash = candidate_id.strip_prefix("subaru-candidate_");
    if candidate_hash
        .is_none_or(|hash| hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || source_id.len() > 256
        || !source_id.starts_with("SUBARU_OEM_")
        || source_id.chars().any(char::is_control)
        || candidate_id.chars().any(char::is_control)
        || catalog_sha256.len() != 64
        || !catalog_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ProtocolError::Malformed(
            "Subaru request requires a bounded candidate ID and catalog SHA-256".into(),
        ));
    }
    if request_id > 0x7FF || response_id > 0x7FF || request_id == response_id {
        return Err(ProtocolError::Malformed(
            "Subaru request routes must use distinct explicit 11-bit CAN identifiers".into(),
        ));
    }
    Ok(())
}

pub fn validate_subaru_ssm_request_fields(
    candidate_ids: &[String],
    source_id: &str,
    catalog_sha256: &str,
    ecu_address: u8,
    tool_address: u8,
    addresses: &[u32],
) -> Result<(), ProtocolError> {
    let source_hash = |candidate_id: &str| {
        candidate_id
            .strip_prefix("subaru-candidate_")
            .is_some_and(|hash| {
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
    };
    if candidate_ids.is_empty()
        || candidate_ids.len() > MAX_SUBARU_SSM_CANDIDATES
        || candidate_ids.iter().any(|id| !source_hash(id))
        || candidate_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != candidate_ids.len()
        || source_id.len() > 256
        || !source_id.starts_with("SUBARU_OEM_")
        || source_id.chars().any(char::is_control)
        || catalog_sha256.len() != 64
        || !catalog_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ProtocolError::Malformed(
            "Subaru SSM reads require unique catalog candidates and the local catalog SHA-256".into(),
        ));
    }
    if ecu_address == 0 || ecu_address >= tool_address || tool_address != 0xf0 {
        return Err(ProtocolError::Malformed(
            "Subaru SSM route must use a distinct ECU byte and tool address F0".into(),
        ));
    }
    if addresses.is_empty()
        || addresses.len() > MAX_SUBARU_SSM_ADDRESSES
        || addresses.iter().any(|address| *address > 0x00ff_ffff)
    {
        return Err(ProtocolError::Malformed(
            "Subaru SSM read addresses must contain 1-256 24-bit memory addresses".into(),
        ));
    }
    Ok(())
}

fn validate_subaru_ssm_payload(payload: &SubaruSsmReadPayload) -> Result<(), ProtocolError> {
    if payload.ecu_address == 0
        || payload.ecu_address >= payload.tool_address
        || payload.tool_address != 0xf0
        || payload.addresses.is_empty()
        || payload.addresses.len() > MAX_SUBARU_SSM_ADDRESSES
        || payload.addresses.iter().any(|address| *address > 0x00ff_ffff)
        || payload.values.len() != payload.addresses.len()
        || payload.init_frame.as_ref().is_some_and(|frame| frame.len() > 1_024)
        || payload.read_frames.len() > 128
        || payload.read_frames.iter().any(|frame| frame.len() > 1_024)
    {
        return Err(ProtocolError::Malformed(
            "Subaru SSM read response fields are malformed or exceed their bounds".into(),
        ));
    }
    Ok(())
}

fn validate_oem_candidate_responses(
    responses: &[OemCandidateResponse],
) -> Result<(), ProtocolError> {
    if responses.len() > MAX_OEM_CANDIDATE_RESPONSES {
        return Err(ProtocolError::Malformed(
            "OEM candidate response exceeds the one-outcome limit".into(),
        ));
    }
    for response in responses {
        match response {
            OemCandidateResponse::Payload(payload)
                if payload.response_id > 0x7FF
                    || payload.bytes.is_empty()
                    || payload.bytes.len() > MAX_OEM_CANDIDATE_RAW_BYTES =>
            {
                return Err(ProtocolError::Malformed(
                    "OEM payload must have an 11-bit responder and bounded diagnostic bytes".into(),
                ));
            }
            OemCandidateResponse::MalformedFrame(frame)
                if frame
                    .response_id
                    .is_some_and(|response_id| response_id > 0x7FF)
                    || frame.raw_bytes.len() > MAX_OEM_CANDIDATE_RAW_BYTES =>
            {
                return Err(ProtocolError::Malformed(
                    "OEM malformed frame must have a bounded raw frame and 11-bit responder".into(),
                ));
            }
            OemCandidateResponse::Failure(failure)
                if failure.request_id > 0x7FF
                    || failure.expected_response_id > 0x7FF
                    || failure.request_id == failure.expected_response_id
                    || failure.message.len() > 4_096 =>
            {
                return Err(ProtocolError::Malformed(
                    "OEM failure must have a distinct 11-bit route and bounded message".into(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
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
        OemCandidateRequestKind,
        OemCandidatePayload, OemCandidateResponse, ProtocolError, StandardDtcFailure,
        StandardDtcMalformedFrame, StandardDtcPayload, StandardDtcResponse, StandardDtcService,
        SubaruSsmReadPayload, WorkerErrorKind, WorkerRequest, WorkerResponse, decode_request_line, decode_response_line,
        encode_request_line, encode_response_line,
    };

    #[test]
    fn mode06_wire_request_is_typed_and_rejects_payload_injection() {
        for test_id in [0, 0x20, 0x35, 0xFF] {
            let request = WorkerRequest::ReadMode06 { test_id };
            assert_eq!(
                decode_request_line(&encode_request_line(&request).unwrap()).unwrap(),
                request
            );
        }
        assert!(decode_request_line(br#"{"type":"read_mode06","test_id":256}"#).is_err());
        assert!(
            decode_request_line(br#"{"type":"read_mode06","test_id":0,"payload":"04"}"#).is_err()
        );
    }

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

    #[test]
    fn standard_dtc_services_are_a_closed_typed_allow_list() {
        for (service, byte) in [
            (StandardDtcService::Stored, 0x03),
            (StandardDtcService::Pending, 0x07),
            (StandardDtcService::Permanent, 0x0A),
            (StandardDtcService::Clear, 0x04),
        ] {
            assert_eq!(service.request_byte(), byte);
            assert_eq!(
                decode_request_line(
                    &encode_request_line(&WorkerRequest::ReadStandardDtc { service }).unwrap()
                )
                .unwrap(),
                WorkerRequest::ReadStandardDtc { service }
            );
        }
        assert!(
            decode_request_line(br#"{"type":"read_standard_dtc","service":"mode_22"}"#).is_err()
        );
        assert!(
            decode_request_line(
                br#"{"type":"read_standard_dtc","service":"stored","payload":[3]}"#
            )
            .is_err()
        );
        assert!(
            decode_request_line(br#"{"type":"read_standard_dtc","service":"stored","extra":true}"#)
                .is_err()
        );
    }

    #[test]
    fn standard_dtc_response_round_trips_each_responder_and_outcome_kind() {
        let response = WorkerResponse::StandardDtcResponses {
            responses: vec![
                StandardDtcResponse::Payload(StandardDtcPayload {
                    ecu_id: "7E8".into(),
                    bytes: vec![0x43, 0x01, 0x33],
                    latency_us: 11_000,
                }),
                StandardDtcResponse::Payload(StandardDtcPayload {
                    ecu_id: "7E9".into(),
                    bytes: vec![0x7F, 0x03, 0x22],
                    latency_us: 12_000,
                }),
                StandardDtcResponse::MalformedFrame(StandardDtcMalformedFrame {
                    ecu_id: Some("7EA".into()),
                    raw_bytes: vec![0x00, 0x00, 0x07, 0xEA, 0x43],
                    latency_us: 13_000,
                    protocol_id: 6,
                    rx_status: 0,
                    declared_data_size: 5,
                    detail: "response truncated".into(),
                }),
                StandardDtcResponse::Failure(StandardDtcFailure {
                    kind: WorkerErrorKind::PassThruFailed,
                    message: "read failed after partial replies".into(),
                    latency_us: 14_000,
                }),
            ],
        };
        let encoded = encode_response_line(&response).unwrap();
        assert_eq!(decode_response_line(&encoded).unwrap(), response);
    }

    #[test]
    fn standard_dtc_keeps_full_raw_frames_for_all_standard_responders_within_ipc_limit() {
        let response = WorkerResponse::StandardDtcResponses {
            responses: (0..8)
                .map(|index| {
                    StandardDtcResponse::MalformedFrame(StandardDtcMalformedFrame {
                        ecu_id: Some(format!("7E{:X}", 8 + index)),
                        raw_bytes: (0..4_128).map(|byte| byte as u8).collect(),
                        latency_us: 1_000,
                        protocol_id: 6,
                        rx_status: 0,
                        declared_data_size: 4_128,
                        detail: "full retained fixture frame".into(),
                    })
                })
                .collect(),
        };
        let encoded = encode_response_line(&response).unwrap();

        assert!(encoded.len() < MAX_MESSAGE_BYTES);
        assert_eq!(decode_response_line(&encoded).unwrap(), response);
    }

    #[test]
    fn source_backed_oem_candidate_requests_round_trip_with_exact_routes_and_widths() {
        for request in [
            br#"{"type":"read_oem_candidate","candidate_id":"ford-21","source_id":"OBDB_FORD","source_blob_sha":"0123456789abcdef0123456789abcdef01234567","source_url":"https://example.test/source","license":"CC BY-SA 4.0","license_url":"https://creativecommons.org/licenses/by-sa/4.0/","request_id":2016,"response_id":2024,"request":{"type":"mode21","identifier":42}}"#.as_slice(),
            br#"{"type":"read_oem_candidate","candidate_id":"toyota-22","source_id":"OBDB_TOYOTA","source_blob_sha":"0123456789abcdef0123456789abcdef01234567","source_url":"https://example.test/source","license":"CC BY-SA 4.0","license_url":"https://creativecommons.org/licenses/by-sa/4.0/","request_id":1808,"response_id":1816,"request":{"type":"mode22","did":4660}}"#.as_slice(),
        ] {
            let decoded = decode_request_line(request).unwrap();
            assert_eq!(decode_request_line(&encode_request_line(&decoded).unwrap()).unwrap(), decoded);
        }
    }

    #[test]
    fn oem_candidate_request_rejects_unverifiable_sources_routes_and_identifier_widths() {
        let base = r#"{"type":"read_oem_candidate","candidate_id":"test","source_id":"OBDB_TEST","source_blob_sha":"0123456789abcdef0123456789abcdef01234567","source_url":"https://example.test/source","license":"CC BY-SA 4.0","license_url":"https://creativecommons.org/licenses/by-sa/4.0/","request_id":2016,"response_id":2024,"request":{"type":"mode21","identifier":42}}"#;
        for invalid in [
            base.replace(
                "\"source_blob_sha\":\"0123456789abcdef0123456789abcdef01234567\",",
                "",
            ),
            base.replace("0123456789abcdef0123456789abcdef01234567", "not-a-git-sha"),
            base.replace("\"request_id\":2016", "\"request_id\":2048"),
            base.replace("\"response_id\":2024", "\"response_id\":-1"),
            base.replace("\"response_id\":2024", "\"response_id\":2016"),
            base.replace("\"identifier\":42", "\"identifier\":256"),
            base.replace("\"type\":\"mode21\"", "\"type\":\"mode10\""),
        ] {
            assert!(
                decode_request_line(invalid.as_bytes()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn owner_supplied_subaru_reads_keep_local_hash_without_fake_license_fields() {
        let request = WorkerRequest::ReadSubaruCandidate {
            candidate_id: format!("subaru-candidate_{}", "a".repeat(64)),
            source_id: "SUBARU_OEM_40_01B_ABCD1234".into(),
            catalog_sha256: "a".repeat(64),
            request_id: 0x7e0,
            response_id: 0x7e8,
            request: OemCandidateRequestKind::Mode22 { did: 0x147e },
        };
        let encoded = encode_request_line(&request).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("license"));
        assert_eq!(decode_request_line(&encoded).unwrap(), request);
    }

    #[test]
    fn owner_supplied_subaru_reads_still_require_exact_local_identity_and_can_route() {
        let base = format!(
            r#"{{"type":"read_subaru_candidate","candidate_id":"subaru-candidate_{}","source_id":"SUBARU_OEM_40_01B_ABCD1234","catalog_sha256":"{}","request_id":2016,"response_id":2024,"request":{{"type":"mode22","did":5246}}}}"#,
            "a".repeat(64),
            "a".repeat(64)
        );
        for invalid in [
            base.replace("subaru-candidate_", "candidate-"),
            base.replace("SUBARU_OEM_40_01B_ABCD1234", "OBDB_FORD"),
            base.replace(&"a".repeat(64), "not-a-sha"),
            base.replace("\"request_id\":2016", "\"request_id\":2048"),
            base.replace("\"response_id\":2024", "\"response_id\":2016"),
        ] {
            assert!(
                decode_request_line(invalid.as_bytes()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn subaru_ssm_wire_uses_only_validated_read_addresses() {
        let requests = [
            WorkerRequest::ConnectSubaruSsm {
                provider_id: "tactrix-openport2".into(),
            },
            WorkerRequest::ReadSubaruSsmAddresses {
                candidate_ids: vec![format!("subaru-candidate_{}", "a".repeat(64))],
                source_id: "SUBARU_OEM_40_01B_ABCD1234".into(),
                catalog_sha256: "a".repeat(64),
                ecu_address: 0x10,
                tool_address: 0xf0,
                addresses: vec![0x000008, 0x00000e],
            },
        ];
        for request in requests {
            let encoded = encode_request_line(&request).unwrap();
            assert_eq!(decode_request_line(&encoded).unwrap(), request);
        }

        let payload = WorkerResponse::SubaruSsmRead {
            payload: SubaruSsmReadPayload {
                ecu_address: 0x10,
                tool_address: 0xf0,
                addresses: vec![0x000008, 0x00000e],
                values: vec![0x7d, 0x33],
                init_frame: Some("80F01003FF0100E2".into()),
                read_frames: vec!["80F01003E87D33A0".into()],
                latency_us: 42_000,
            },
        };
        let encoded = encode_response_line(&payload).unwrap();
        assert_eq!(decode_response_line(&encoded).unwrap(), payload);

        let invalid = br#"{"type":"read_subaru_ssm_addresses","candidate_ids":["subaru-candidate_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"source_id":"SUBARU_OEM_40_01B_ABCD1234","catalog_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","ecu_address":16,"tool_address":240,"addresses":[16777216],"payload":"B0"}"#;
        assert!(decode_request_line(invalid).is_err());
    }

    #[test]
    fn oem_candidate_responses_are_bounded_to_one_raw_outcome() {
        let too_many = WorkerResponse::OemCandidateResponses {
            responses: (0..4)
                .map(|_| {
                    OemCandidateResponse::Payload(OemCandidatePayload {
                        response_id: 0x7E8,
                        bytes: vec![0x61, 0x01],
                        latency_us: 1,
                    })
                })
                .collect(),
        };
        let too_large = WorkerResponse::OemCandidateResponses {
            responses: vec![OemCandidateResponse::Payload(OemCandidatePayload {
                response_id: 0x7E8,
                bytes: vec![0x61; super::MAX_OEM_CANDIDATE_RAW_BYTES + 1],
                latency_us: 1,
            })],
        };

        assert!(encode_response_line(&too_many).is_err());
        assert!(encode_response_line(&too_large).is_err());
    }
}
