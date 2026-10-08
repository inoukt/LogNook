use super::ObdLinkSxAdapter;
use crate::{
    DeviceDescriptor, HardwareAdapter, LogNookError, OemCandidate, OemCandidateTrySpec,
    obdlink_sx_protocol::parse_isotp_messages,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OemCandidateOutcome {
    Positive,
    NegativeResponse,
    ResponsePending,
    TemporarilyUnavailable,
    SessionRequired,
    SecurityRequired,
    NoResponse,
    MalformedResponse,
    WrongResponder,
    InvalidCandidate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OemCandidateDecodedValue {
    pub raw_value: String,
    pub value: Option<f64>,
    pub enum_value: Option<String>,
    pub unit: String,
    pub formula: String,
    pub within_catalog_range: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OemCandidateResponse {
    pub outcome: OemCandidateOutcome,
    pub responder_header: Option<String>,
    pub payload: Vec<u8>,
    pub negative_response_code: Option<u8>,
    pub decoded_value: Option<OemCandidateDecodedValue>,
    pub decode_error: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OemCandidateTryResult {
    pub candidate_id: String,
    pub exact_adapter_command: String,
    pub adapter: DeviceDescriptor,
    pub protocol: String,
    pub started_at_unix_ms: u64,
    pub latency_ms: u64,
    pub outcome: OemCandidateOutcome,
    pub raw_response: String,
    pub responder_header: Option<String>,
    pub response_payload: Vec<u8>,
    pub negative_response_code: Option<u8>,
    pub decoded_value: Option<OemCandidateDecodedValue>,
    pub decode_error: Option<String>,
    pub error: Option<String>,
    pub evidence_saved: bool,
    pub evidence_save_error: Option<String>,
}

#[derive(Deserialize)]
struct Decoder {
    #[serde(rename = "bix")]
    bit_offset: Option<u32>,
    #[serde(rename = "len")]
    bit_length: Option<u32>,
    #[serde(rename = "mul", default = "one")]
    multiplier: f64,
    #[serde(rename = "div", default = "one")]
    divisor: f64,
    #[serde(rename = "add", default)]
    addend: f64,
    #[serde(rename = "map")]
    mapping: Option<HashMap<String, Value>>,
    #[serde(rename = "unit")]
    unit: Option<String>,
}

fn one() -> f64 {
    1.0
}

pub fn build_oem_candidate_command(spec: &OemCandidateTrySpec) -> Result<String, LogNookError> {
    validate_candidate(&spec.candidate)?;
    Ok(format!(
        "STPX H:{}, D:{}{}, T:250",
        spec.candidate.request_header.to_ascii_uppercase(),
        spec.candidate.service,
        spec.candidate.identifier.to_ascii_uppercase(),
    ))
}

fn validate_candidate(candidate: &OemCandidate) -> Result<(), LogNookError> {
    let id_width = match candidate.service.as_str() {
        "21" => 2,
        "22" => 4,
        _ => {
            return Err(LogNookError::new(
                "Only source-backed service 21 and 22 reads are allowed",
            ));
        }
    };
    let valid_hex = |value: &str, width| {
        value.len() == width && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    let valid_standard_can_header = |value: &str| {
        valid_hex(value, 3) && u16::from_str_radix(value, 16).is_ok_and(|header| header <= 0x7ff)
    };
    if candidate.candidate_id.is_empty()
        || !valid_hex(&candidate.identifier, id_width)
        || !valid_standard_can_header(&candidate.request_header)
        || !valid_standard_can_header(&candidate.response_header)
        || candidate.source_blob_sha.is_empty()
        || candidate.source_url.as_deref().is_none_or(str::is_empty)
        || candidate.license.as_deref().is_none_or(str::is_empty)
        || candidate.license_url.as_deref().is_none_or(str::is_empty)
    {
        return Err(LogNookError::new(
            "OEM candidate lacks a valid source, service identifier, license, or exact 11-bit CAN route",
        ));
    }
    Ok(())
}

pub fn decode_oem_candidate_response(
    spec: &OemCandidateTrySpec,
    raw: &str,
) -> OemCandidateResponse {
    if let Err(error) = validate_candidate(&spec.candidate) {
        return response(
            OemCandidateOutcome::InvalidCandidate,
            None,
            Vec::new(),
            None,
            None,
            Some(error.to_string()),
        );
    }
    let messages = match parse_isotp_messages(raw) {
        Ok(Some(messages)) => messages,
        Ok(None) => {
            return response(
                OemCandidateOutcome::NoResponse,
                None,
                Vec::new(),
                None,
                None,
                Some("No ISO-TP response frames were returned".into()),
            );
        }
        Err(error) => {
            return response(
                OemCandidateOutcome::MalformedResponse,
                None,
                Vec::new(),
                None,
                Some(error.clone()),
                Some(error),
            );
        }
    };
    let matching: Vec<_> = messages
        .into_iter()
        .filter(|(header, _)| header.eq_ignore_ascii_case(&spec.candidate.response_header))
        .collect();
    if matching.is_empty() {
        return response(
            OemCandidateOutcome::WrongResponder,
            None,
            Vec::new(),
            None,
            None,
            Some(format!(
                "Expected ECU {}, received a response from another header",
                spec.candidate.response_header
            )),
        );
    }
    if matching.len() != 1 {
        return response(
            OemCandidateOutcome::MalformedResponse,
            Some(spec.candidate.response_header.clone()),
            Vec::new(),
            None,
            None,
            Some("Multiple responses arrived from the expected ECU".into()),
        );
    }
    let (responder, payload) = matching
        .into_iter()
        .next()
        .expect("one response was checked");
    decode_oem_candidate_payload(spec, Some(&responder), &payload)
}

pub fn decode_oem_candidate_payload(
    spec: &OemCandidateTrySpec,
    responder_header: Option<&str>,
    payload: &[u8],
) -> OemCandidateResponse {
    if let Err(error) = validate_candidate(&spec.candidate) {
        return response(
            OemCandidateOutcome::InvalidCandidate,
            responder_header.map(str::to_owned),
            payload.to_vec(),
            None,
            None,
            Some(error.to_string()),
        );
    }
    let Some(responder) = responder_header
        .filter(|header| header.eq_ignore_ascii_case(&spec.candidate.response_header))
    else {
        return response(
            OemCandidateOutcome::WrongResponder,
            responder_header.map(str::to_owned),
            payload.to_vec(),
            None,
            None,
            Some(format!(
                "Expected ECU {}, received a response from another header",
                spec.candidate.response_header
            )),
        );
    };
    let payload = payload.to_vec();
    let Ok(service) = u8::from_str_radix(&spec.candidate.service, 16) else {
        return response(
            OemCandidateOutcome::InvalidCandidate,
            Some(responder.to_owned()),
            payload,
            None,
            None,
            Some("Candidate service is not hexadecimal".into()),
        );
    };
    if payload.first() == Some(&0x7F) && payload.get(1) == Some(&service) && payload.len() == 3 {
        let nrc = payload[2];
        let outcome = match nrc {
            0x78 => OemCandidateOutcome::ResponsePending,
            0x21 | 0x22 => OemCandidateOutcome::TemporarilyUnavailable,
            0x7E | 0x7F => OemCandidateOutcome::SessionRequired,
            0x33 => OemCandidateOutcome::SecurityRequired,
            _ => OemCandidateOutcome::NegativeResponse,
        };
        let mut result = response(
            outcome,
            Some(responder.to_owned()),
            payload,
            None,
            None,
            Some(format!("ECU returned NRC 0x{nrc:02X}")),
        );
        result.negative_response_code = Some(nrc);
        return result;
    }
    let mut expected = vec![service + 0x40];
    for pair in spec.candidate.identifier.as_bytes().chunks_exact(2) {
        let Ok(pair) = std::str::from_utf8(pair) else {
            return response(
                OemCandidateOutcome::InvalidCandidate,
                Some(responder.to_owned()),
                payload,
                None,
                None,
                Some("Candidate identifier is not ASCII hexadecimal".into()),
            );
        };
        let Ok(byte) = u8::from_str_radix(pair, 16) else {
            return response(
                OemCandidateOutcome::InvalidCandidate,
                Some(responder.to_owned()),
                payload,
                None,
                None,
                Some("Candidate identifier is not hexadecimal".into()),
            );
        };
        expected.push(byte);
    }
    if payload.len() < expected.len() || payload[..expected.len()] != expected {
        return response(
            OemCandidateOutcome::MalformedResponse,
            Some(responder.to_owned()),
            payload,
            None,
            None,
            Some(format!(
                "Positive response did not echo service {} identifier {}",
                spec.candidate.service, spec.candidate.identifier
            )),
        );
    }
    let (decoded_value, decode_error) = match decode_candidate_value(
        &spec.candidate,
        spec.value_format.as_str(),
        &payload[expected.len()..],
    ) {
        Ok(value) => (value, None),
        Err(error) => (None, Some(error)),
    };
    response(
        OemCandidateOutcome::Positive,
        Some(responder.to_owned()),
        payload,
        decoded_value,
        decode_error,
        None,
    )
}

fn response(
    outcome: OemCandidateOutcome,
    responder_header: Option<String>,
    payload: Vec<u8>,
    decoded_value: Option<OemCandidateDecodedValue>,
    decode_error: Option<String>,
    error: Option<String>,
) -> OemCandidateResponse {
    OemCandidateResponse {
        outcome,
        responder_header,
        payload,
        negative_response_code: None,
        decoded_value,
        decode_error,
        error,
    }
}

fn decode_candidate_value(
    candidate: &OemCandidate,
    value_format: &str,
    data: &[u8],
) -> Result<Option<OemCandidateDecodedValue>, String> {
    if !matches!(value_format, "uint" | "signed" | "enum") {
        return Err(format!(
            "Unsupported candidate value format: {value_format}"
        ));
    }
    let decoder: Decoder = serde_json::from_str(&candidate.decoder_json)
        .map_err(|error| format!("Candidate decoder JSON is invalid: {error}"))?;
    let Some(bit_offset) = candidate.bit_offset.or(decoder.bit_offset) else {
        return Ok(None);
    };
    let Some(bit_length) = candidate.bit_length.or(decoder.bit_length) else {
        return Ok(None);
    };
    if decoder
        .bit_offset
        .is_some_and(|offset| offset != bit_offset)
        || decoder
            .bit_length
            .is_some_and(|length| length != bit_length)
        || !(1..=40).contains(&bit_length)
        || bit_offset.saturating_add(bit_length) as usize > data.len().saturating_mul(8)
        || decoder.divisor == 0.0
        || !decoder.multiplier.is_finite()
        || !decoder.divisor.is_finite()
        || !decoder.addend.is_finite()
        || decoder
            .unit
            .as_ref()
            .is_some_and(|unit| unit != &candidate.unit)
    {
        return Err("Candidate decoder does not match the returned data range".into());
    }
    let mut raw = 0_u64;
    for bit in bit_offset..bit_offset + bit_length {
        let byte = data[(bit / 8) as usize];
        let shift = 7 - bit % 8;
        raw = (raw << 1) | u64::from((byte >> shift) & 1);
    }
    let raw_signed = if value_format == "signed" && raw & (1 << (bit_length - 1)) != 0 {
        raw as i128 - (1_i128 << bit_length)
    } else {
        raw as i128
    };
    let raw_value = raw_signed.to_string();
    if value_format == "enum" {
        let enum_value = decoder
            .mapping
            .as_ref()
            .and_then(|mapping| mapping.get(&raw_value))
            .and_then(enum_display_value);
        return Ok(Some(OemCandidateDecodedValue {
            raw_value,
            value: None,
            enum_value,
            unit: candidate.unit.clone(),
            formula: candidate.formula.clone(),
            within_catalog_range: None,
        }));
    }
    let value = raw_signed as f64 * decoder.multiplier / decoder.divisor + decoder.addend;
    if !value.is_finite() {
        return Err("Candidate decoder produced a non-finite value".into());
    }
    let within_catalog_range = Some(
        candidate.min_value.is_none_or(|min| value >= min)
            && candidate.max_value.is_none_or(|max| value <= max),
    );
    Ok(Some(OemCandidateDecodedValue {
        raw_value,
        value: Some(value),
        enum_value: None,
        unit: candidate.unit.clone(),
        formula: candidate.formula.clone(),
        within_catalog_range,
    }))
}

fn enum_display_value(value: &Value) -> Option<String> {
    let mapped = value.get("value").or_else(|| value.get("description"))?;
    Some(
        mapped
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| mapped.to_string()),
    )
}

pub fn try_oem_candidate(
    port_name: impl Into<String>,
    spec: &OemCandidateTrySpec,
) -> Result<OemCandidateTryResult, LogNookError> {
    build_oem_candidate_command(spec)?;
    let mut adapter = ObdLinkSxAdapter::new(port_name);
    let result = adapter.try_oem_candidate_inner(spec);
    adapter.disconnect();
    result
}

impl ObdLinkSxAdapter {
    fn try_oem_candidate_inner(
        &mut self,
        spec: &OemCandidateTrySpec,
    ) -> Result<OemCandidateTryResult, LogNookError> {
        self.open()?;
        let device = self.read_identity()?;
        let protocol = self.command("STPRS")?;
        if !protocol.contains("ISO 15765-4 (CAN 11/500)") {
            return Err(LogNookError::new(
                "OEM one-shot requires OBDLink SX ISO 15765-4 CAN 11/500",
            ));
        }
        self.configure_can_headers()?;
        self.device = Some(device);
        self.read_connected_candidate(spec)
    }

    pub(super) fn read_connected_candidate(
        &mut self,
        spec: &OemCandidateTrySpec,
    ) -> Result<OemCandidateTryResult, LogNookError> {
        let exact_adapter_command = build_oem_candidate_command(spec)?;
        if !self.restore_adapter_settings {
            return Err(LogNookError::new(
                "OEM read requires OBDLink SX ISO 15765-4 CAN 11/500",
            ));
        }
        let device = self
            .device
            .clone()
            .ok_or_else(|| LogNookError::new("OBDLink SX identity is unavailable"))?;
        let started_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let request_started = Instant::now();
        let raw_response = self.command(&exact_adapter_command)?;
        let latency_ms = request_started.elapsed().as_millis() as u64;
        let decoded = decode_oem_candidate_response(spec, &raw_response);
        Ok(OemCandidateTryResult {
            candidate_id: spec.candidate.candidate_id.clone(),
            exact_adapter_command,
            adapter: device,
            protocol: "ISO 15765-4 (CAN 11/500)".into(),
            started_at_unix_ms,
            latency_ms,
            outcome: decoded.outcome,
            raw_response,
            responder_header: decoded.responder_header,
            response_payload: decoded.payload,
            negative_response_code: decoded.negative_response_code,
            decoded_value: decoded.decoded_value,
            decode_error: decoded.decode_error,
            error: decoded.error,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }
}

#[cfg(test)]
#[path = "oem_probe_tests.rs"]
mod tests;
