use super::ObdLinkSxAdapter;
use crate::{
    DeviceDescriptor, HardwareAdapter, LogNookError, ScionMode21ProbeSpec,
    obdlink_sx_protocol::parse_isotp_messages,
    oem_catalog::SCION_XB_BLOB_SHA,
};
use serde::{Deserialize, Serialize};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScionMode21ProbeResult {
    pub vehicle_profile_id: String,
    pub source_id: String,
    pub source_blob_sha: String,
    pub source_url: String,
    pub license: String,
    pub license_url: String,
    pub request_service: u8,
    pub request_identifier: u8,
    pub request_header: String,
    pub expected_response_header: String,
    pub exact_adapter_command: String,
    pub adapter: DeviceDescriptor,
    pub protocol: String,
    pub started_at_unix_ms: u64,
    pub latency_ms: u64,
    pub outcome: String,
    pub raw_response: String,
    pub response_payload: Vec<u8>,
    pub decoded_signals: Vec<ScionMode21DecodedSignal>,
    pub error: Option<String>,
    pub evidence_saved: bool,
    pub evidence_save_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScionMode21DecodedSignal {
    pub candidate_id: String,
    pub name: String,
    pub unit: String,
    pub bit_offset: u32,
    pub bit_length: u32,
    pub formula: String,
    pub raw_value: u32,
    pub value: f64,
}

pub fn probe_scion_xb_mode21(
    port_name: impl Into<String>,
    spec: &ScionMode21ProbeSpec,
) -> Result<ScionMode21ProbeResult, LogNookError> {
    validate_scion_probe_spec(spec)?;
    let mut adapter = ObdLinkSxAdapter::new(port_name);
    let result = adapter.probe_scion_inner(spec);
    adapter.disconnect();
    result
}

impl ObdLinkSxAdapter {
    pub(super) fn probe_scion_inner(
        &mut self,
        spec: &ScionMode21ProbeSpec,
    ) -> Result<ScionMode21ProbeResult, LogNookError> {
        self.open()?;
        let device = self.read_identity()?;
        let protocol = self.command("STPRS")?;
        if !protocol.contains("ISO 15765-4 (CAN 11/500)") {
            return Err(LogNookError::new(
                "Scion Mode 21 test requires the OBDLink SX to report ISO 15765-4 CAN 11/500",
            ));
        }
        self.configure_can_headers()?;

        let exact_adapter_command = build_scion_probe_command(spec)?;
        let started_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let request_started = Instant::now();
        let raw_response = self.command(&exact_adapter_command);
        let latency_ms = request_started.elapsed().as_millis() as u64;
        let (raw_response, decoded) = match raw_response {
            Ok(response) => {
                let decoded = decode_scion_mode21_response(spec, &response);
                (response, decoded)
            }
            Err(error) => (
                String::new(),
                ScionResponseDecode {
                    outcome: "no_response".into(),
                    payload: Vec::new(),
                    decoded: Vec::new(),
                    error: Some(error.to_string()),
                },
            ),
        };
        Ok(ScionMode21ProbeResult {
            vehicle_profile_id: "SCION_XB_2014_2AZFE".into(),
            source_id: spec.source_id.clone(),
            source_blob_sha: spec.source_blob_sha.clone(),
            source_url: spec.source_url.clone(),
            license: spec.license.clone(),
            license_url: spec.license_url.clone(),
            request_service: spec.service,
            request_identifier: spec.identifier,
            request_header: spec.request_header.clone(),
            expected_response_header: spec.response_header.clone(),
            exact_adapter_command,
            adapter: device,
            protocol: "ISO 15765-4 (CAN 11/500)".into(),
            started_at_unix_ms,
            latency_ms,
            outcome: decoded.outcome,
            raw_response,
            response_payload: decoded.payload,
            decoded_signals: decoded.decoded,
            error: decoded.error,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }
}

pub(super) fn build_scion_probe_command(
    spec: &ScionMode21ProbeSpec,
) -> Result<String, LogNookError> {
    validate_scion_probe_spec(spec)?;
    Ok("STPX H:7E0, D:2101, T:250".into())
}

fn validate_scion_probe_spec(spec: &ScionMode21ProbeSpec) -> Result<(), LogNookError> {
    let expected = [
        ("Intake air temperature", "celsius", 48, 8, 1.0, 1.0, -40.0, "(raw*(1)/(1))+(-40)"),
        ("Coolant temperature", "celsius", 64, 8, 1.0, 1.0, -40.0, "(raw*(1)/(1))+(-40)"),
        ("Engine speed", "rpm", 72, 16, 1.0, 4.0, 0.0, "(raw*(1)/(4))+(0)"),
        ("Vehicle speed", "kilometersPerHour", 88, 8, 1.0, 1.0, 0.0, "(raw*(1)/(1))+(0)"),
        ("Throttle sensor position", "percent", 112, 8, 100.0, 255.0, 0.0, "(raw*(100)/(255))+(0)"),
    ];
    if spec.source_id != crate::oem_catalog::SCION_XB_SOURCE_ID
        || spec.source_blob_sha != SCION_XB_BLOB_SHA
        || spec.service != 0x21
        || spec.identifier != 0x01
        || spec.request_header != "7E0"
        || spec.response_header != "7E8"
        || spec.channels.len() != expected.len()
    {
        return Err(LogNookError::new(
            "Scion Mode 21 probe definition failed fixed source/addressing checks",
        ));
    }
    for (channel, (name, unit, offset, length, multiplier, divisor, addend, formula))
        in spec.channels.iter().zip(expected)
    {
        if channel.name != name
            || channel.unit != unit
            || channel.bit_offset != offset
            || channel.bit_length != length
            || channel.multiplier != multiplier
            || channel.divisor != divisor
            || channel.addend != addend
            || channel.formula != formula
        {
            return Err(LogNookError::new(format!(
                "Scion Mode 21 decoder definition failed fixed checks: {name}"
            )));
        }
    }
    Ok(())
}

pub(super) struct ScionResponseDecode {
    pub(super) outcome: String,
    pub(super) payload: Vec<u8>,
    pub(super) decoded: Vec<ScionMode21DecodedSignal>,
    pub(super) error: Option<String>,
}

pub(super) fn decode_scion_mode21_response(
    spec: &ScionMode21ProbeSpec,
    raw: &str,
) -> ScionResponseDecode {
    let messages = match parse_isotp_messages(raw) {
        Ok(Some(messages)) => messages,
        Ok(None) => {
            return ScionResponseDecode {
                outcome: "no_response".into(), payload: Vec::new(), decoded: Vec::new(),
                error: Some("No ISO-TP response frames were returned".into()),
            };
        }
        Err(error) => {
            return ScionResponseDecode {
                outcome: "malformed_response".into(), payload: Vec::new(), decoded: Vec::new(),
                error: Some(error),
            };
        }
    };
    let matching: Vec<_> = messages.into_iter()
        .filter(|(ecu, _)| ecu == &spec.response_header).collect();
    if matching.is_empty() {
        return ScionResponseDecode {
            outcome: "wrong_responder".into(), payload: Vec::new(), decoded: Vec::new(),
            error: Some(format!("Expected ECU {}, received a response from another header", spec.response_header)),
        };
    }
    if matching.len() != 1 {
        return ScionResponseDecode {
            outcome: "malformed_response".into(), payload: Vec::new(), decoded: Vec::new(),
            error: Some("Multiple responses arrived from the target ECU".into()),
        };
    }
    let (_, payload) = matching.into_iter().next().expect("one target response was checked");
    if payload.starts_with(&[0x7F, spec.service]) {
        return ScionResponseDecode {
            outcome: "negative_response".into(), payload, decoded: Vec::new(),
            error: Some("ECU returned a negative response to service 21".into()),
        };
    }
    if payload.first() != Some(&0x61) || payload.get(1) != Some(&0x01) {
        return ScionResponseDecode {
            outcome: "malformed_response".into(), payload, decoded: Vec::new(),
            error: Some("Target ECU did not return positive response 61 01".into()),
        };
    }
    if payload.len() != 29 {
        let length = payload.len();
        return ScionResponseDecode {
            outcome: "wrong_length".into(), payload, decoded: Vec::new(),
            error: Some(format!("Scion 21 01 expected 29 reassembled bytes, got {length}")),
        };
    }
    let data = &payload[2..];
    let mut decoded = Vec::with_capacity(spec.channels.len());
    for channel in &spec.channels {
        let byte_offset = (channel.bit_offset / 8) as usize;
        let raw_value = match channel.bit_length {
            8 => data.get(byte_offset).copied().map(u32::from),
            16 => data.get(byte_offset..byte_offset + 2)
                .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]) as u32),
            _ => None,
        };
        let Some(raw_value) = raw_value else {
            return ScionResponseDecode {
                outcome: "bad_decoder".into(), payload, decoded: Vec::new(),
                error: Some(format!("Decoder points outside the response: {}", channel.name)),
            };
        };
        let value = (raw_value as f64 * channel.multiplier / channel.divisor) + channel.addend;
        if !value.is_finite()
            || channel.minimum.is_some_and(|minimum| value < minimum)
            || channel.maximum.is_some_and(|maximum| value > maximum)
        {
            return ScionResponseDecode {
                outcome: "bad_decoder".into(), payload, decoded: Vec::new(),
                error: Some(format!("Decoded value is outside the candidate range: {}", channel.name)),
            };
        }
        decoded.push(ScionMode21DecodedSignal {
            candidate_id: channel.candidate_id.clone(), name: channel.name.clone(),
            unit: channel.unit.clone(), bit_offset: channel.bit_offset, bit_length: channel.bit_length,
            formula: channel.formula.clone(), raw_value, value,
        });
    }
    ScionResponseDecode { outcome: "positive".into(), payload, decoded, error: None }
}
