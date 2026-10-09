use super::ObdLinkSxAdapter;
use crate::{
    A30GraphValue, A30ReadOutcome, A30ReadTest, A30ReadTestResult, DiagnosticOutcome,
    DiagnosticQueryRequest, DiagnosticRead, DiagnosticReadKind, DiagnosticValue, LogNookError,
    obdlink_sx_protocol::parse_isotp_messages,
};
use std::{collections::HashSet, time::Instant};

struct ParsedQuickTest {
    outcome: A30ReadOutcome,
    responder_header: Option<String>,
    response_payloads: Vec<Vec<u8>>,
    negative_response_code: Option<u8>,
    graph_values: Vec<A30GraphValue>,
    detail: Option<String>,
    raw_response: String,
}

impl ParsedQuickTest {
    fn empty(outcome: A30ReadOutcome, raw_response: String, detail: Option<String>) -> Self {
        Self {
            outcome,
            responder_header: None,
            response_payloads: Vec::new(),
            negative_response_code: None,
            graph_values: Vec::new(),
            detail,
            raw_response,
        }
    }
}

impl ObdLinkSxAdapter {
    pub(super) fn read_connected_quick_test(
        &mut self,
        test: A30ReadTest,
    ) -> Result<A30ReadTestResult, LogNookError> {
        if self.started.is_none() {
            return Err(LogNookError::new("OBDLink SX adapter is not connected"));
        }
        if !self.restore_adapter_settings {
            return Err(LogNookError::new(
                "OBDLink SX quick tests require ISO 15765-4 (CAN 11/500)",
            ));
        }
        let adapter = self
            .device
            .clone()
            .ok_or_else(|| LogNookError::new("OBDLink SX identity is unavailable"))?;
        let (description, service, _, _, payload, _, timeout, _) = test.request();
        let request_description = if test == A30ReadTest::Mode09Vin {
            "Mode 09 VIN · request 09 02 on functional 7DF; responding ECU is captured".into()
        } else {
            description
        };
        let data = payload
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        let exact_adapter_command = format!("STPX H:7DF, D:{data}, T:{}", timeout.as_millis());
        let started_at_unix_ms = crate::vehicle_diagnostics::now_unix_ms();
        let request_started = Instant::now();
        let command_result = self.command(&exact_adapter_command);
        let latency_ms = request_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let parsed = match command_result {
            Ok(raw_response) => parse_quick_test_response(
                test,
                service,
                payload,
                timeout.as_millis().min(u64::MAX as u128) as u64,
                &exact_adapter_command,
                started_at_unix_ms,
                latency_ms,
                raw_response,
            )?,
            Err(error) if error.is_transient() => ParsedQuickTest::empty(
                A30ReadOutcome::NoResponse,
                String::new(),
                Some(error.to_string()),
            ),
            Err(error) => return Err(error),
        };

        Ok(A30ReadTestResult {
            test,
            request_description,
            exact_adapter_command,
            adapter,
            protocol: "ISO 15765-4 (CAN 11/500) via OBDLink SX".into(),
            started_at_unix_ms,
            latency_ms,
            outcome: parsed.outcome,
            responder_header: parsed.responder_header,
            response_payloads: parsed.response_payloads,
            negative_response_code: parsed.negative_response_code,
            graph_values: parsed.graph_values,
            detail: parsed.detail,
            raw_response: parsed.raw_response,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }
}

fn parse_quick_test_response(
    test: A30ReadTest,
    service: u8,
    payload: Vec<u8>,
    timeout_ms: u64,
    command: &str,
    started_at_unix_ms: u64,
    latency_ms: u64,
    raw_response: String,
) -> Result<ParsedQuickTest, LogNookError> {
    if is_no_response(&raw_response, command) {
        return Ok(ParsedQuickTest::empty(
            A30ReadOutcome::NoResponse,
            raw_response,
            None,
        ));
    }
    if matches!(
        test,
        A30ReadTest::Mode01EngineRpm | A30ReadTest::Mode02FreezeFrameRpm
    ) {
        return Ok(parse_rpm_test(test, service, raw_response));
    }

    let query = diagnostic_query(test, service, payload, timeout_ms);
    query.validate().map_err(LogNookError::new)?;
    let reads = crate::vehicle_diagnostics::parse_adapter_response(
        &query,
        command.into(),
        raw_response.clone(),
        started_at_unix_ms,
        latency_ms,
    );
    Ok(parse_diagnostic_reads(test, reads, raw_response))
}

fn parse_rpm_test(test: A30ReadTest, service: u8, raw_response: String) -> ParsedQuickTest {
    let messages = match parse_isotp_messages(&raw_response) {
        Ok(Some(messages)) if !messages.is_empty() => messages,
        Ok(_) => {
            return ParsedQuickTest::empty(
                A30ReadOutcome::MalformedResponse,
                raw_response,
                Some("Adapter response did not contain a parseable CAN/ISO-TP ECU frame".into()),
            );
        }
        Err(error) => {
            return ParsedQuickTest::empty(
                A30ReadOutcome::MalformedResponse,
                raw_response,
                Some(error),
            );
        }
    };
    let response_payloads = messages
        .iter()
        .map(|(_, payload)| payload.clone())
        .collect::<Vec<_>>();
    let responder_header = messages.first().map(|(header, _)| header.clone());
    let responders = messages
        .iter()
        .map(|(header, _)| header.to_ascii_uppercase())
        .collect::<HashSet<_>>();
    if let Some((wrong_header, _)) = messages
        .iter()
        .find(|(header, _)| !crate::vehicle_diagnostics::is_functional_responder(header))
    {
        let mut parsed = ParsedQuickTest::empty(
            A30ReadOutcome::WrongResponder,
            raw_response,
            Some(
                "Functional 11-bit request on 7DF received a response from an unexpected CAN ID"
                    .into(),
            ),
        );
        parsed.responder_header = Some(wrong_header.clone());
        parsed.response_payloads = response_payloads;
        parsed.negative_response_code = messages.iter().find_map(|(_, payload)| {
            (payload.len() == 3 && payload[0] == 0x7F && payload[1] == service)
                .then_some(payload[2])
        });
        return parsed;
    }
    if responders.len() > 1 {
        let mut parsed = ParsedQuickTest::empty(
            A30ReadOutcome::WrongResponder,
            raw_response,
            Some(multiple_responders_detail()),
        );
        parsed.responder_header = responder_header;
        parsed.response_payloads = response_payloads;
        parsed.negative_response_code = messages.iter().find_map(|(_, payload)| {
            (payload.len() == 3 && payload[0] == 0x7F && payload[1] == service)
                .then_some(payload[2])
        });
        return parsed;
    }
    let mut graph_values = Vec::new();
    let mut terminal = Vec::new();
    let mut pending = false;
    for (header, response_payload) in &messages {
        if is_response_pending(service, response_payload) {
            pending = true;
        } else if response_payload.first() == Some(&0x7F) {
            if response_payload.len() != 3 || response_payload[1] != service {
                terminal.push((
                    A30ReadOutcome::MalformedResponse,
                    None,
                    Some("Malformed or mismatched OBD negative response".into()),
                ));
            } else {
                terminal.push((
                    negative_response_outcome(response_payload[2]),
                    Some(response_payload[2]),
                    None,
                ));
            }
        } else {
            let reply = crate::A30CanReply {
                response_header: header.clone(),
                payload: response_payload.clone(),
                truncated: false,
            };
            let values = crate::a30::decode_read_test_values(test, &[reply]);
            if values.len() == 1 {
                graph_values.extend(values);
            } else {
                terminal.push((
                    A30ReadOutcome::MalformedResponse,
                    None,
                    Some(
                        "RPM response did not echo the requested service, PID, frame, and exact data length".into(),
                    ),
                ));
            }
        }
    }
    let terminal_failure = terminal
        .iter()
        .rev()
        .find(|(outcome, _, _)| *outcome != A30ReadOutcome::Positive);
    let has_positive = !graph_values.is_empty();
    let (outcome, negative_response_code, detail) =
        if let Some((outcome, code, detail)) = terminal_failure {
            (outcome.clone(), *code, detail.clone())
        } else if has_positive {
            (A30ReadOutcome::Positive, None, None)
        } else if pending {
            (A30ReadOutcome::ResponsePending, Some(0x78), None)
        } else {
            (
                A30ReadOutcome::MalformedResponse,
                None,
                Some("RPM response did not contain a complete positive or negative reply".into()),
            )
        };
    if outcome != A30ReadOutcome::Positive {
        graph_values.clear();
    } else if test == A30ReadTest::Mode02FreezeFrameRpm {
        for value in &mut graph_values {
            value.signal_id = "obdlink_sx_mode02_engine_rpm".into();
        }
    }

    ParsedQuickTest {
        outcome,
        responder_header,
        response_payloads,
        negative_response_code,
        graph_values,
        detail,
        raw_response,
    }
}

fn diagnostic_query(
    test: A30ReadTest,
    service: u8,
    payload: Vec<u8>,
    timeout_ms: u64,
) -> DiagnosticQueryRequest {
    let kind = match test {
        A30ReadTest::Mode03StoredDtcs => DiagnosticReadKind::StoredDtcs,
        A30ReadTest::Mode06Bitmap00
        | A30ReadTest::Mode06Bitmap20
        | A30ReadTest::Mode06Bitmap40
        | A30ReadTest::Mode06Bitmap60
        | A30ReadTest::Mode06Bitmap80
        | A30ReadTest::Mode06BitmapA0 => DiagnosticReadKind::Mode06Bitmap,
        A30ReadTest::Mode06Tid35 | A30ReadTest::Mode06TidA2 | A30ReadTest::Mode06TidA5 => {
            DiagnosticReadKind::Mode06Monitor
        }
        A30ReadTest::Mode07PendingDtcs => DiagnosticReadKind::PendingDtcs,
        A30ReadTest::Mode09Vin => DiagnosticReadKind::Vin,
        A30ReadTest::Mode0aPermanentDtcs => DiagnosticReadKind::PermanentDtcs,
        A30ReadTest::Mode01EngineRpm | A30ReadTest::Mode02FreezeFrameRpm => unreachable!(),
    };
    let identifier = match kind {
        DiagnosticReadKind::Mode06Bitmap
        | DiagnosticReadKind::Mode06Monitor
        | DiagnosticReadKind::Vin => payload.get(1).copied(),
        _ => None,
    };
    DiagnosticQueryRequest {
        kind,
        service,
        identifier,
        payload,
        wait_ms: timeout_ms,
    }
}

fn parse_diagnostic_reads(
    test: A30ReadTest,
    reads: Vec<DiagnosticRead>,
    raw_response: String,
) -> ParsedQuickTest {
    let response_payloads = reads
        .iter()
        .filter(|read| !read.response_payload.is_empty())
        .map(|read| read.response_payload.clone())
        .collect();
    let responder_header = reads.iter().find_map(|read| read.ecu_id.clone());
    let responders = reads
        .iter()
        .filter_map(|read| read.ecu_id.as_deref())
        .map(str::to_ascii_uppercase)
        .collect::<HashSet<_>>();
    if let Some(wrong_responder) = reads
        .iter()
        .find(|read| read.outcome == DiagnosticOutcome::WrongResponder)
    {
        let mut parsed = ParsedQuickTest::empty(
            A30ReadOutcome::WrongResponder,
            raw_response,
            wrong_responder.detail.clone().or_else(|| {
                Some(
                    "Functional 11-bit request on 7DF received a response from an unexpected CAN ID"
                        .into(),
                )
            }),
        );
        parsed.responder_header = wrong_responder.ecu_id.clone();
        parsed.response_payloads = response_payloads;
        parsed.negative_response_code = reads.iter().find_map(|read| read.negative_response_code);
        return parsed;
    }
    if responders.len() > 1 {
        let summaries = reads
            .iter()
            .filter_map(|read| {
                Some(format!(
                    "ECU {}: {}",
                    read.ecu_id.as_deref()?,
                    diagnostic_read_summary(test, read)?
                ))
            })
            .collect::<Vec<_>>();
        let detail = if summaries.is_empty() {
            multiple_responders_detail()
        } else {
            format!("{} {}", multiple_responders_detail(), summaries.join("; "))
        };
        let mut parsed =
            ParsedQuickTest::empty(A30ReadOutcome::WrongResponder, raw_response, Some(detail));
        parsed.responder_header = responder_header;
        parsed.response_payloads = response_payloads;
        parsed.negative_response_code = reads.iter().find_map(|read| read.negative_response_code);
        return parsed;
    }

    let has_positive = reads
        .iter()
        .any(|read| read.outcome == DiagnosticOutcome::Positive);
    let terminal_failure = reads.iter().rev().find(|read| {
        read.outcome != DiagnosticOutcome::Positive
            && !(has_positive && read.outcome == DiagnosticOutcome::ResponsePending)
    });
    let outcome = terminal_failure
        .map(|read| a30_diagnostic_outcome(&read.outcome))
        .unwrap_or_else(|| {
            if has_positive {
                A30ReadOutcome::Positive
            } else {
                reads
                    .last()
                    .map(|read| a30_diagnostic_outcome(&read.outcome))
                    .unwrap_or(A30ReadOutcome::NoResponse)
            }
        });
    let positive_reads = reads
        .iter()
        .filter(|read| read.outcome == DiagnosticOutcome::Positive)
        .collect::<Vec<_>>();
    let graph_values = if outcome == A30ReadOutcome::Positive && test_is_mode06_monitor(test) {
        positive_reads
            .iter()
            .flat_map(|read| {
                read.values
                    .iter()
                    .filter_map(|value| mode06_graph_value(value, &read.response_payload))
            })
            .collect()
    } else {
        Vec::new()
    };
    let detail = if outcome == A30ReadOutcome::Positive {
        let values = positive_reads
            .iter()
            .flat_map(|read| read.values.iter().cloned())
            .collect::<Vec<_>>();
        diagnostic_detail(test, &values)
    } else {
        terminal_failure
            .or_else(|| reads.last())
            .and_then(|read| read.detail.clone())
    };
    let negative_response_code = if outcome == A30ReadOutcome::Positive {
        None
    } else {
        terminal_failure
            .or_else(|| reads.last())
            .and_then(|read| read.negative_response_code)
    };

    ParsedQuickTest {
        outcome,
        responder_header,
        response_payloads,
        negative_response_code,
        graph_values,
        detail,
        raw_response,
    }
}

fn is_no_response(raw_response: &str, command: &str) -> bool {
    raw_response.split(['\r', '\n']).all(|line| {
        let line = line.trim();
        line.is_empty()
            || line == ">"
            || line.eq_ignore_ascii_case(command)
            || line.eq_ignore_ascii_case("NO DATA")
    })
}

fn is_response_pending(service: u8, payload: &[u8]) -> bool {
    payload == [0x7F, service, 0x78]
}

fn test_is_mode06_monitor(test: A30ReadTest) -> bool {
    matches!(
        test,
        A30ReadTest::Mode06Tid35 | A30ReadTest::Mode06TidA2 | A30ReadTest::Mode06TidA5
    )
}

fn mode06_graph_value(value: &DiagnosticValue, response_payload: &[u8]) -> Option<A30GraphValue> {
    match value {
        DiagnosticValue::Mode06Monitor {
            test_id,
            component_id,
            value,
            ..
        } => Some(A30GraphValue {
            signal_id: format!("obdlink_sx_mode06_tid_{test_id:02x}_mid_{component_id:02x}"),
            name: format!("Mode 06 TID {test_id:02X} MID {component_id:02X}"),
            service: 0x06,
            pid: None,
            value: f64::from(*value),
            unit: "raw count".into(),
            raw_value: value.to_string(),
            response_payload: Some(response_payload.to_vec()),
        }),
        _ => None,
    }
}

fn diagnostic_detail(test: A30ReadTest, values: &[DiagnosticValue]) -> Option<String> {
    let details = values
        .iter()
        .filter_map(|value| match value {
            DiagnosticValue::DtcList { codes } => Some(if codes.is_empty() {
                "No DTCs reported".into()
            } else {
                codes.join(", ")
            }),
            DiagnosticValue::Identity { value, .. } if test == A30ReadTest::Mode09Vin => {
                Some(value.clone())
            }
            DiagnosticValue::Mode06Bitmap { test_ids, .. } => Some(if test_ids.is_empty() {
                "No Mode 06 test IDs reported".into()
            } else {
                format!(
                    "Mode 06 test IDs: {}",
                    test_ids
                        .iter()
                        .map(|id| format!("{id:02X}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }),
            DiagnosticValue::Mode06Monitor {
                test_id,
                component_id,
                uasid,
                value,
                minimum,
                maximum,
            } => Some(format!(
                "TID {test_id:02X} MID {component_id:02X} UASID {uasid:02X}: {value} (min {minimum}, max {maximum})"
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    if details.is_empty() && test == A30ReadTest::Mode09Vin {
        Some("No VIN record reported".into())
    } else {
        (!details.is_empty()).then(|| details.join("; "))
    }
}

fn diagnostic_read_summary(test: A30ReadTest, read: &DiagnosticRead) -> Option<String> {
    if read.outcome == DiagnosticOutcome::Positive {
        return diagnostic_detail(test, &read.values);
    }
    let mut summary = format!("{:?}", read.outcome);
    if let Some(code) = read.negative_response_code {
        summary.push_str(&format!(" (NRC 0x{code:02X})"));
    }
    if let Some(detail) = &read.detail {
        summary.push_str(&format!(": {detail}"));
    }
    Some(summary)
}

fn multiple_responders_detail() -> String {
    "Multiple ECU responders were returned; raw replies were preserved but values were not graphed."
        .into()
}

fn negative_response_outcome(nrc: u8) -> A30ReadOutcome {
    match nrc {
        0x78 => A30ReadOutcome::ResponsePending,
        0x21 | 0x22 => A30ReadOutcome::TemporarilyUnavailable,
        0x7E | 0x7F => A30ReadOutcome::SessionRequired,
        0x33 => A30ReadOutcome::SecurityRequired,
        _ => A30ReadOutcome::NegativeResponse,
    }
}

fn a30_diagnostic_outcome(outcome: &DiagnosticOutcome) -> A30ReadOutcome {
    match outcome {
        DiagnosticOutcome::Positive => A30ReadOutcome::Positive,
        DiagnosticOutcome::NegativeResponse => A30ReadOutcome::NegativeResponse,
        DiagnosticOutcome::ResponsePending => A30ReadOutcome::ResponsePending,
        DiagnosticOutcome::TemporarilyUnavailable => A30ReadOutcome::TemporarilyUnavailable,
        DiagnosticOutcome::SessionRequired => A30ReadOutcome::SessionRequired,
        DiagnosticOutcome::SecurityRequired => A30ReadOutcome::SecurityRequired,
        DiagnosticOutcome::NoResponse => A30ReadOutcome::NoResponse,
        DiagnosticOutcome::MalformedResponse => A30ReadOutcome::MalformedResponse,
        DiagnosticOutcome::WrongResponder => A30ReadOutcome::WrongResponder,
        DiagnosticOutcome::TransportError => A30ReadOutcome::TemporarilyUnavailable,
    }
}
