mod model;

pub use model::*;

use crate::obdlink_sx_protocol::parse_isotp_messages;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn parse_adapter_response(
    query: &DiagnosticQueryRequest,
    exact_adapter_command: String,
    raw_response: String,
    started_at_unix_ms: u64,
    latency_ms: u64,
) -> Vec<DiagnosticRead> {
    if let Some(groups) = split_responses_by_ecu(&raw_response) {
        let mut reads = Vec::new();
        for (ecu_id, ecu_response) in groups {
            match parse_isotp_messages(&ecu_response) {
                Ok(Some(messages)) if !messages.is_empty() => {
                    reads.extend(messages.into_iter().map(|(ecu_id, payload)| {
                        query.report(
                            Some(ecu_id),
                            payload,
                            raw_response.clone(),
                            exact_adapter_command.clone(),
                            started_at_unix_ms,
                            latency_ms,
                            false,
                        )
                    }))
                }
                Ok(Some(_)) | Ok(None) => reads.push(malformed_response_for_ecu(
                    query,
                    Some(ecu_id),
                    raw_response.clone(),
                    exact_adapter_command.clone(),
                    started_at_unix_ms,
                    latency_ms,
                    "CAN responder did not return a complete ISO-TP payload".into(),
                )),
                Err(error) => reads.push(malformed_response_for_ecu(
                    query,
                    Some(ecu_id),
                    raw_response.clone(),
                    exact_adapter_command.clone(),
                    started_at_unix_ms,
                    latency_ms,
                    error,
                )),
            }
        }
        return reads;
    }
    match parse_isotp_messages(&raw_response) {
        Ok(Some(messages)) if !messages.is_empty() => messages
            .into_iter()
            .map(|(ecu_id, payload)| {
                query.report(
                    Some(ecu_id),
                    payload,
                    raw_response.clone(),
                    exact_adapter_command.clone(),
                    started_at_unix_ms,
                    latency_ms,
                    false,
                )
            })
            .collect(),
        Ok(Some(_)) | Ok(None)
            if raw_response.trim().is_empty() || raw_response.contains("NO DATA") =>
        {
            vec![query.no_response(
                raw_response,
                exact_adapter_command,
                started_at_unix_ms,
                latency_ms,
                None,
            )]
        }
        Ok(Some(_)) | Ok(None) => vec![malformed_response(
            query,
            raw_response,
            exact_adapter_command,
            started_at_unix_ms,
            latency_ms,
            "Adapter response did not contain a parseable CAN/ISO-TP ECU frame".into(),
        )],
        Err(error) => vec![malformed_response(
            query,
            raw_response,
            exact_adapter_command,
            started_at_unix_ms,
            latency_ms,
            error,
        )],
    }
}

pub(crate) fn malformed_response(
    query: &DiagnosticQueryRequest,
    raw_response: String,
    exact_adapter_command: String,
    started_at_unix_ms: u64,
    latency_ms: u64,
    detail: String,
) -> DiagnosticRead {
    malformed_response_for_ecu(
        query,
        None,
        raw_response,
        exact_adapter_command,
        started_at_unix_ms,
        latency_ms,
        detail,
    )
}

fn malformed_response_for_ecu(
    query: &DiagnosticQueryRequest,
    ecu_id: Option<String>,
    raw_response: String,
    exact_adapter_command: String,
    started_at_unix_ms: u64,
    latency_ms: u64,
    detail: String,
) -> DiagnosticRead {
    let mut read = empty_read(
        query,
        ecu_id,
        raw_response,
        exact_adapter_command,
        started_at_unix_ms,
        latency_ms,
    );
    read.outcome = DiagnosticOutcome::MalformedResponse;
    read.detail = Some(detail);
    read
}

fn split_responses_by_ecu(raw: &str) -> Option<Vec<(String, String)>> {
    let mut groups: Vec<(String, String)> = Vec::new();
    let mut current = None;
    for line in raw.split(['\r', '\n']) {
        let header = line
            .split_whitespace()
            .next()
            .map(|token| token.trim_matches('>').trim_end_matches(':'))
            .filter(|token| {
                matches!(token.len(), 3 | 8) && token.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        if let Some(header) = header {
            let index = groups
                .iter()
                .position(|(existing, _)| existing.eq_ignore_ascii_case(header))
                .unwrap_or_else(|| {
                    groups.push((header.to_ascii_uppercase(), String::new()));
                    groups.len() - 1
                });
            current = Some(index);
        }
        if let Some(index) = current {
            groups[index].1.push_str(line);
            groups[index].1.push('\r');
        }
    }
    (!groups.is_empty()).then_some(groups)
}

pub(crate) fn is_functional_responder(ecu_id: &str) -> bool {
    ecu_id.len() == 3
        && u16::from_str_radix(ecu_id, 16).is_ok_and(|id| (0x7E8..=0x7EF).contains(&id))
}

fn empty_read(
    query: &DiagnosticQueryRequest,
    ecu_id: Option<String>,
    raw_response: String,
    exact_adapter_command: String,
    adapter_started_at_unix_ms: u64,
    latency_ms: u64,
) -> DiagnosticRead {
    DiagnosticRead {
        kind: query.kind.clone(),
        service: query.service,
        identifier: query.identifier,
        request_payload: query.payload.clone(),
        route: query.route().into(),
        exact_adapter_command,
        adapter_started_at_unix_ms,
        latency_ms,
        ecu_id,
        outcome: DiagnosticOutcome::MalformedResponse,
        negative_response_code: None,
        response_payload: Vec::new(),
        values: Vec::new(),
        detail: None,
        raw_response,
    }
}

fn decode_payload(query: &DiagnosticQueryRequest, read: &mut DiagnosticRead) {
    let payload = &read.response_payload;
    if payload.first() == Some(&0x7F) {
        if payload.len() != 3 || payload[1] != query.service {
            read.detail = Some("Malformed or mismatched OBD negative response".into());
            return;
        }
        let nrc = payload[2];
        read.negative_response_code = Some(nrc);
        read.outcome = match nrc {
            0x78 => DiagnosticOutcome::ResponsePending,
            0x21 | 0x22 => DiagnosticOutcome::TemporarilyUnavailable,
            0x7E | 0x7F => DiagnosticOutcome::SessionRequired,
            0x33 => DiagnosticOutcome::SecurityRequired,
            _ => DiagnosticOutcome::NegativeResponse,
        };
        return;
    }
    if payload.first() != Some(&query.service.wrapping_add(0x40)) {
        read.detail = Some(format!(
            "Expected positive Mode {:02X} response",
            query.service
        ));
        return;
    }

    let decoded = match query.kind {
        DiagnosticReadKind::StoredDtcs
        | DiagnosticReadKind::PendingDtcs
        | DiagnosticReadKind::PermanentDtcs => {
            decode_dtcs(&payload[1..]).map(|codes| vec![DiagnosticValue::DtcList { codes }])
        }
        DiagnosticReadKind::Vin
        | DiagnosticReadKind::CalibrationId
        | DiagnosticReadKind::EcuName => decode_identity(query, payload),
        DiagnosticReadKind::Mode06Bitmap => decode_bitmap(query, payload),
        DiagnosticReadKind::Mode06Monitor => decode_mode06_monitors(query, payload),
    };
    match decoded {
        Ok(values) => {
            read.outcome = DiagnosticOutcome::Positive;
            read.values = values;
        }
        Err(error) => read.detail = Some(error),
    }
}

fn decode_dtcs(bytes: &[u8]) -> Result<Vec<String>, String> {
    let Some((&count, dtc_bytes)) = bytes.split_first() else {
        return Err("DTC response is missing its declared code count".into());
    };
    let expected = usize::from(count) * 2;
    if dtc_bytes.len() < expected {
        return Err(format!(
            "DTC response declares {count} code(s) but contains only {} complete code(s)",
            dtc_bytes.len() / 2
        ));
    }
    if !dtc_bytes[expected..].iter().all(|byte| *byte == 0) {
        return Err("DTC response has nonzero bytes beyond its declared code count".into());
    }
    Ok(dtc_bytes[..expected]
        .chunks_exact(2)
        .filter(|pair| *pair != [0, 0])
        .map(|pair| {
            let family = ["P", "C", "B", "U"][usize::from(pair[0] >> 6)];
            format!(
                "{family}{}{:X}{:X}{:X}",
                (pair[0] >> 4) & 0x03,
                pair[0] & 0x0F,
                pair[1] >> 4,
                pair[1] & 0x0F
            )
        })
        .collect())
}

fn decode_identity(
    query: &DiagnosticQueryRequest,
    payload: &[u8],
) -> Result<Vec<DiagnosticValue>, String> {
    let pid = query.identifier.expect("identity query has a PID");
    if payload.get(1) != Some(&pid) || payload.len() < 3 {
        return Err(format!(
            "Mode 09 PID {pid:02X} response has the wrong identifier or is too short"
        ));
    }
    let count = usize::from(payload[2]);
    let data = &payload[3..];
    if count == 0 && data.is_empty() {
        return Ok(Vec::new());
    }
    let clean = |bytes: &[u8]| -> Result<String, String> {
        let content_end = bytes
            .iter()
            .rposition(|byte| *byte != 0 && *byte != b' ')
            .map_or(0, |i| i + 1);
        let bytes = &bytes[..content_end];
        if bytes.is_empty()
            || !bytes.is_ascii()
            || bytes
                .iter()
                .any(|byte| !byte.is_ascii_graphic() && *byte != b' ')
        {
            return Err("Mode 09 identity contains non-printable bytes".into());
        }
        let value = String::from_utf8_lossy(bytes).trim().to_owned();
        if value.is_empty() {
            return Err("Mode 09 identity is empty".into());
        }
        Ok(value)
    };
    let (label, values): (&str, Vec<String>) = match pid {
        0x02 if count == 1 && data.len() == 17 => ("VIN", vec![clean(data)?]),
        0x04 if count > 0 && data.len() == count * 16 => (
            "Calibration ID",
            data.chunks_exact(16).map(clean).collect::<Result<_, _>>()?,
        ),
        0x0A if count == 1 && !data.is_empty() && data.len() <= 20 => {
            ("ECU name", vec![clean(data)?])
        }
        0x02 => return Err("Mode 09 VIN must contain one 17-character record".into()),
        0x04 => return Err("Mode 09 calibration count must match complete 16-byte records".into()),
        _ => return Err("Mode 09 ECU name must contain 1-20 characters".into()),
    };
    Ok(values
        .into_iter()
        .map(|value| DiagnosticValue::Identity {
            pid,
            label: label.into(),
            value,
        })
        .collect())
}

fn decode_bitmap(
    query: &DiagnosticQueryRequest,
    payload: &[u8],
) -> Result<Vec<DiagnosticValue>, String> {
    let start = query.identifier.expect("bitmap query has a start ID");
    if payload.len() != 6 || payload[1] != start || !is_bitmap_page(start) {
        return Err("Mode 06 bitmap must echo its page and contain four bitmap bytes".into());
    }
    let bitmap = &payload[2..6];
    let test_ids = (0..31)
        .filter(|bit| bitmap[bit / 8] & (1 << (7 - bit % 8)) != 0)
        .map(|bit| start.wrapping_add(bit as u8 + 1))
        .collect();
    Ok(vec![DiagnosticValue::Mode06Bitmap {
        start,
        test_ids,
        has_more: bitmap[3] & 0x01 != 0,
    }])
}

fn decode_mode06_monitors(
    query: &DiagnosticQueryRequest,
    payload: &[u8],
) -> Result<Vec<DiagnosticValue>, String> {
    if payload.len() < 10 || !(payload.len() - 1).is_multiple_of(9) {
        return Err(
            "Unsupported Mode 06 payload format; expected complete 9-byte TID/MID/UASID/value/min/max records".into(),
        );
    }
    let values = payload[1..]
        .chunks_exact(9)
        .map(|record| DiagnosticValue::Mode06Monitor {
            test_id: record[0],
            component_id: record[1],
            uasid: record[2],
            value: u16::from_be_bytes([record[3], record[4]]),
            minimum: u16::from_be_bytes([record[5], record[6]]),
            maximum: u16::from_be_bytes([record[7], record[8]]),
        })
        .collect::<Vec<_>>();
    let test_id = query.identifier.expect("monitor query has a TID");
    if values.iter().any(|value| {
        !matches!(value, DiagnosticValue::Mode06Monitor { test_id: received, .. } if *received == test_id)
    }) {
        return Err(format!(
            "Mode 06 response contains a test ID other than requested {test_id:02X}"
        ));
    }
    Ok(values)
}

fn is_bitmap_page(id: u8) -> bool {
    matches!(id, 0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 | 0xC0 | 0xE0)
}

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "vehicle_diagnostics/tests.rs"]
mod tests;
