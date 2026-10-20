use super::{
    ObdLinkSxAdapter, OemCandidateOutcome, OemCandidateTrySpec, decode_oem_candidate_payload,
};
use crate::{
    HardwareAdapter, OemCandidate, OemCandidateSourcePolicy, ResearchTestRequest,
    ResearchTestResult,
};
use std::{
    io::{Cursor, Read, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

struct FixturePort {
    input: Cursor<Vec<u8>>,
    written: Arc<Mutex<Vec<u8>>>,
}

impl Read for FixturePort {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buffer)
    }
}

impl Write for FixturePort {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.written.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn spec() -> OemCandidateTrySpec {
    OemCandidateTrySpec {
        source_policy: crate::OemCandidateSourcePolicy::IndexedPack,
        candidate: OemCandidate {
            candidate_id: "candidate-1".into(),
            source_id: "SCION_SOURCE".into(),
            family: "Toyota".into(),
            brand: "Scion".into(),
            model_scope: "xB 2014".into(),
            service: "21".into(),
            identifier: "01".into(),
            request_header: "7E0".into(),
            response_header: "7E8".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "Engine".into(),
            name: "Test signal".into(),
            signal_source_id: "SCION_SOURCE_01".into(),
            bit_offset: Some(0),
            bit_length: Some(8),
            formula: "(raw*(1)/(1))+(0)".into(),
            decoder_json: "{\"bix\":0,\"len\":8,\"unit\":\"celsius\"}".into(),
            unit: "celsius".into(),
            year_filter_json: "{}".into(),
            ecu_filter: String::new(),
            source_blob_sha: "reviewed-sha".into(),
            research_state: "community_reference".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "research".into(),
            notes: String::new(),
            suggested_metric: String::new(),
            source_scope: "vehicle".into(),
            applicability_specificity: 60,
            min_value: None,
            max_value: None,
            source_url: Some("https://source.test/vehicle".into()),
            license: Some("CC BY-SA 4.0".into()),
            license_url: Some("https://source.test/license".into()),
        },
        value_format: "uint".into(),
    }
}

#[test]
fn owner_supplied_subaru_read_uses_local_provenance_without_claiming_a_license() {
    let mut local = spec();
    let catalog_sha = "a".repeat(64);
    local.source_policy = OemCandidateSourcePolicy::LocalSubaruCatalog {
        catalog_sha256: catalog_sha.clone(),
    };
    local.candidate.candidate_id = "subaru-candidate_1234".into();
    local.candidate.source_id = "SUBARU_OEM_40_01B_ABCD1234".into();
    local.candidate.family = "Subaru OEM catalog".into();
    local.candidate.brand = "Subaru".into();
    local.candidate.source_blob_sha.clear();
    local.candidate.source_url = None;
    local.candidate.license = None;
    local.candidate.license_url = None;
    local.candidate.research_state = "review_required".into();
    local.candidate.vehicle_validation = "not_tested".into();
    local.candidate.auto_request = false;
    local.candidate.source_scope = format!("owner_supplied_subaru_catalog:{catalog_sha}");
    assert!(super::build_oem_candidate_command(&local).is_ok());

    local.candidate.family = "Unrelated".into();
    assert!(super::build_oem_candidate_command(&local).is_err());
}

#[test]
fn subaru_ssm_group_payload_decodes_as_memory_bytes_without_mode22_framing() {
    let mut local = spec();
    let catalog_sha = "a".repeat(64);
    local.source_policy = OemCandidateSourcePolicy::LocalSubaruCatalog {
        catalog_sha256: catalog_sha.clone(),
    };
    local.candidate.candidate_id = format!("subaru-candidate_{}", "b".repeat(64));
    local.candidate.source_id = "SUBARU_OEM_40_01B_ABCD1234".into();
    local.candidate.family = "Subaru OEM catalog".into();
    local.candidate.brand = "Subaru".into();
    local.candidate.service = "SSM-KWP".into();
    local.candidate.identifier = "00000E".into();
    local.candidate.request_header = "$10 $20".into();
    local.candidate.response_header = "F0".into();
    local.candidate.bit_offset = Some(8);
    local.candidate.decoder_json =
        "{\"bix\":0,\"len\":8,\"mul\":1,\"div\":1,\"add\":0,\"unit\":\"celsius\"}".into();
    local.candidate.source_blob_sha.clear();
    local.candidate.source_url = None;
    local.candidate.license = None;
    local.candidate.license_url = None;
    local.candidate.research_state = "review_required".into();
    local.candidate.vehicle_validation = "not_tested".into();
    local.candidate.auto_request = false;
    local.candidate.source_scope = format!("owner_supplied_subaru_catalog:{catalog_sha}");

    let decoded = decode_oem_candidate_payload(&local, Some("F0"), &[0x22, 0x7d]);
    assert_eq!(decoded.outcome, OemCandidateOutcome::Positive);
    assert_eq!(decoded.decoded_value.unwrap().value, Some(125.0));
    assert!(super::build_oem_candidate_command(&local).is_err());
}

fn started_adapter(transcript: &str) -> (ObdLinkSxAdapter, Arc<Mutex<Vec<u8>>>) {
    let written = Arc::new(Mutex::new(Vec::new()));
    let mut adapter = ObdLinkSxAdapter::with_port(
        "COM4",
        Box::new(FixturePort {
            input: Cursor::new(transcript.as_bytes().to_vec()),
            written: written.clone(),
        }),
    );
    adapter.device = Some(crate::DeviceDescriptor {
        stable_id: "sx-test".into(),
        hardware_family: "OBDLink SX".into(),
        firmware: None,
        path: Some("COM4".into()),
    });
    adapter.restore_adapter_settings = true;
    adapter.started = Some(Instant::now());
    (adapter, written)
}

#[test]
fn sx_quick_tests_send_the_15_functional_standard_requests() {
    let tests = [
        (
            crate::A30ReadTest::Mode01EngineRpm,
            "STPX H:7DF, D:010C, T:600",
        ),
        (
            crate::A30ReadTest::Mode02FreezeFrameRpm,
            "STPX H:7DF, D:020C00, T:600",
        ),
        (
            crate::A30ReadTest::Mode03StoredDtcs,
            "STPX H:7DF, D:03, T:900",
        ),
        (
            crate::A30ReadTest::Mode06Bitmap00,
            "STPX H:7DF, D:0600, T:600",
        ),
        (
            crate::A30ReadTest::Mode06Bitmap20,
            "STPX H:7DF, D:0620, T:600",
        ),
        (
            crate::A30ReadTest::Mode06Bitmap40,
            "STPX H:7DF, D:0640, T:600",
        ),
        (
            crate::A30ReadTest::Mode06Bitmap60,
            "STPX H:7DF, D:0660, T:600",
        ),
        (
            crate::A30ReadTest::Mode06Bitmap80,
            "STPX H:7DF, D:0680, T:600",
        ),
        (
            crate::A30ReadTest::Mode06BitmapA0,
            "STPX H:7DF, D:06A0, T:600",
        ),
        (crate::A30ReadTest::Mode06Tid35, "STPX H:7DF, D:0635, T:600"),
        (crate::A30ReadTest::Mode06TidA2, "STPX H:7DF, D:06A2, T:600"),
        (crate::A30ReadTest::Mode06TidA5, "STPX H:7DF, D:06A5, T:600"),
        (
            crate::A30ReadTest::Mode07PendingDtcs,
            "STPX H:7DF, D:07, T:900",
        ),
        (crate::A30ReadTest::Mode09Vin, "STPX H:7DF, D:0902, T:1500"),
        (
            crate::A30ReadTest::Mode0aPermanentDtcs,
            "STPX H:7DF, D:0A, T:900",
        ),
    ];
    let (mut adapter, written) = started_adapter(&">".repeat(tests.len()));

    for (test, _) in tests {
        adapter
            .execute_research_test(ResearchTestRequest::A30ReadTest(test))
            .unwrap();
    }

    let expected = tests
        .iter()
        .map(|(_, command)| format!("{command}\r"))
        .collect::<String>();
    assert_eq!(*written.lock().unwrap(), expected.as_bytes());
}

#[test]
fn a30_graph_value_reads_legacy_json_without_a_response_payload() {
    let value: crate::A30GraphValue = serde_json::from_str(
        r#"{"signal_id":"engine_rpm","name":"Engine speed","service":1,"pid":12,"value":1726.0,"unit":"rpm","raw_value":"1AF8"}"#,
    )
    .unwrap();

    assert_eq!(value.response_payload, None);
    assert!(
        serde_json::to_value(value)
            .unwrap()
            .get("response_payload")
            .is_none()
    );
}

#[test]
fn sx_mode02_freeze_frame_rpm_uses_the_functional_route_and_shared_graph_value() {
    let transcript = include_str!("../../tests/fixtures/obdlink_sx/mode02_freeze_frame_rpm.txt");
    let (mut adapter, written) = started_adapter(transcript);

    let ResearchTestResult::A30ReadTest(result) = adapter
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode02FreezeFrameRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };

    assert_eq!(result.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(result.responder_header.as_deref(), Some("7E8"));
    assert_eq!(result.graph_values.len(), 1);
    assert_eq!(
        result.graph_values[0].signal_id,
        "obdlink_sx_mode02_engine_rpm"
    );
    assert_eq!(result.graph_values[0].value, 1726.0);
    assert_eq!(
        result.graph_values[0].response_payload.as_deref(),
        Some(&[0x42, 0x0C, 0x00, 0x1A, 0xF8][..])
    );
    assert_eq!(*written.lock().unwrap(), b"STPX H:7DF, D:020C00, T:600\r");
}

#[test]
fn sx_mode02_uses_a_final_reply_after_pending_and_keeps_pending_only_typed() {
    let (mut final_reply, _) = started_adapter("020C00\r7E8 03 7F 02 78\r7E8 05 42 0C 00 1A F8\r>");
    let ResearchTestResult::A30ReadTest(final_reply) = final_reply
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode02FreezeFrameRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(final_reply.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(final_reply.response_payloads.len(), 2);
    assert_eq!(final_reply.negative_response_code, None);
    assert_eq!(final_reply.graph_values.len(), 1);

    let (mut pending_only, _) = started_adapter("020C00\r7E8 03 7F 02 78\r>");
    let ResearchTestResult::A30ReadTest(pending_only) = pending_only
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode02FreezeFrameRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(pending_only.outcome, crate::A30ReadOutcome::ResponsePending);
    assert_eq!(pending_only.negative_response_code, Some(0x78));
    assert!(pending_only.graph_values.is_empty());
}

#[test]
fn sx_quick_test_keeps_a_valid_reply_when_no_data_text_follows() {
    let (mut adapter, _) = started_adapter("010C\r7E8 04 41 0C 1A F8\rNO DATA\r>");
    let ResearchTestResult::A30ReadTest(result) = adapter
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(result.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(result.graph_values[0].value, 1726.0);
    assert!(result.raw_response.contains("NO DATA"));
}

#[test]
fn sx_quick_test_reports_decoded_vin_dtcs_and_bitmap_details() {
    let cases = [
        (
            crate::A30ReadTest::Mode09Vin,
            include_str!("../../tests/fixtures/obdlink_sx/mode09_vin.txt"),
            "1HGCM82633A004352",
        ),
        (
            crate::A30ReadTest::Mode03StoredDtcs,
            include_str!("../../tests/fixtures/obdlink_sx/mode03_dtc.txt"),
            "P0123",
        ),
        (
            crate::A30ReadTest::Mode06Bitmap00,
            include_str!("../../tests/fixtures/obdlink_sx/mode06_bitmap_00.txt"),
            "Mode 06 test IDs: 01",
        ),
    ];

    for (test, transcript, detail) in cases {
        let (mut adapter, _) = started_adapter(transcript);
        let ResearchTestResult::A30ReadTest(result) = adapter
            .execute_research_test(ResearchTestRequest::A30ReadTest(test))
            .unwrap()
        else {
            panic!("expected quick-test result")
        };
        assert_eq!(result.outcome, crate::A30ReadOutcome::Positive);
        assert_eq!(result.detail.as_deref(), Some(detail));
    }

    let (mut empty_dtcs, _) = started_adapter("03\r7E8 02 43 00\r>");
    let ResearchTestResult::A30ReadTest(empty_dtcs) = empty_dtcs
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode03StoredDtcs,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(empty_dtcs.detail.as_deref(), Some("No DTCs reported"));

    let (mut multiple_dtcs, _) = started_adapter("03\r7E8 04 43 01 01 23\r7E9 04 43 01 04 56\r>");
    let ResearchTestResult::A30ReadTest(multiple_dtcs) = multiple_dtcs
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode03StoredDtcs,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(multiple_dtcs.outcome, crate::A30ReadOutcome::WrongResponder);
    assert!(multiple_dtcs.graph_values.is_empty());
    let detail = multiple_dtcs.detail.as_deref().unwrap();
    assert!(detail.contains("ECU 7E8: P0123"));
    assert!(detail.contains("ECU 7E9: P0456"));

    let (mut invalid_ecu_id, _) = started_adapter("03\r7E8 04 43 01 01 23\r7E0 04 43 01 04 56\r>");
    let ResearchTestResult::A30ReadTest(invalid_ecu_id) = invalid_ecu_id
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode03StoredDtcs,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(
        invalid_ecu_id.outcome,
        crate::A30ReadOutcome::WrongResponder
    );
    assert_eq!(invalid_ecu_id.response_payloads.len(), 2);
    let detail = invalid_ecu_id.detail.as_deref().unwrap();
    assert!(detail.contains("unexpected CAN ID"));
    assert!(!detail.contains("Multiple ECU responders"));

    let (mut empty_vin, _) = started_adapter("0902\r7E8 03 49 02 00\r>");
    let ResearchTestResult::A30ReadTest(empty_vin) = empty_vin
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode09Vin,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(empty_vin.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(empty_vin.detail.as_deref(), Some("No VIN record reported"));

    let (mut mixed_vin_negative, _) = started_adapter(
        "0902\r7E8 10 14 49 02 01 31 48 47\r7E8 21 43 4D 38 32 36 33 33\r7E8 22 41 30 30 34 33 35 32\r7E9 03 7F 09 31\r>",
    );
    let ResearchTestResult::A30ReadTest(mixed_vin_negative) = mixed_vin_negative
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode09Vin,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(
        mixed_vin_negative.outcome,
        crate::A30ReadOutcome::WrongResponder
    );
    let detail = mixed_vin_negative.detail.as_deref().unwrap();
    assert!(detail.contains("ECU 7E8: 1HGCM82633A004352"));
    assert!(detail.contains("ECU 7E9: NegativeResponse (NRC 0x31)"));
    assert!(!detail.contains("No VIN record reported"));
}

#[test]
fn sx_quick_test_rejects_wrong_echo_and_truncated_isotp() {
    let (mut wrong_echo, _) = started_adapter("010C\r7E8 03 41 0D 64\r>");
    let ResearchTestResult::A30ReadTest(wrong_echo) = wrong_echo
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(wrong_echo.outcome, crate::A30ReadOutcome::MalformedResponse);
    assert!(wrong_echo.graph_values.is_empty());

    let (mut truncated, _) = started_adapter("010C\r7E8 10 08 41 0C 1A F8\r>");
    let ResearchTestResult::A30ReadTest(truncated) = truncated
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(truncated.outcome, crate::A30ReadOutcome::MalformedResponse);
    assert!(truncated.graph_values.is_empty());

    let (mut terminal_error, _) = started_adapter("010C\r7E8 04 41 0C 1A F8\r7E8 03 7F 03 78\r>");
    let ResearchTestResult::A30ReadTest(terminal_error) = terminal_error
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(
        terminal_error.outcome,
        crate::A30ReadOutcome::MalformedResponse
    );
    assert!(terminal_error.graph_values.is_empty());
}

#[test]
fn sx_quick_test_preserves_negative_responses_and_multiple_ecus_without_graphing() {
    let (mut negative, _) = started_adapter("030C\r7E8 03 7F 01 78\r>");
    let ResearchTestResult::A30ReadTest(negative) = negative
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(negative.outcome, crate::A30ReadOutcome::ResponsePending);
    assert_eq!(negative.negative_response_code, Some(0x78));

    let (mut mismatched_negative, _) = started_adapter("010C\r7E8 03 7F 03 78\r>");
    let ResearchTestResult::A30ReadTest(mismatched_negative) = mismatched_negative
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(
        mismatched_negative.outcome,
        crate::A30ReadOutcome::MalformedResponse
    );
    assert_eq!(mismatched_negative.negative_response_code, None);
    assert!(mismatched_negative.graph_values.is_empty());

    let (mut wrong_responder, _) = started_adapter("010C\r7E0 04 41 0C 1A F8\r>");
    let ResearchTestResult::A30ReadTest(wrong_responder) = wrong_responder
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(
        wrong_responder.outcome,
        crate::A30ReadOutcome::WrongResponder
    );
    assert!(wrong_responder.graph_values.is_empty());

    let (mut multiple, _) = started_adapter("010C\r7E8 04 41 0C 1A F8\r7E9 04 41 0C 20 00\r>");
    let ResearchTestResult::A30ReadTest(multiple) = multiple
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(multiple.response_payloads.len(), 2);
    assert_eq!(multiple.outcome, crate::A30ReadOutcome::WrongResponder);
    assert!(multiple.graph_values.is_empty());

    let (mut multiple_with_pending, _) =
        started_adapter("010C\r7E8 04 41 0C 1A F8\r7E9 03 7F 01 78\r>");
    let ResearchTestResult::A30ReadTest(multiple_with_pending) = multiple_with_pending
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(
        multiple_with_pending.outcome,
        crate::A30ReadOutcome::WrongResponder
    );
    assert_eq!(multiple_with_pending.negative_response_code, Some(0x78));
    assert!(multiple_with_pending.graph_values.is_empty());

    let (mut mixed_headers, _) = started_adapter("010C\r7E8 04 41 0C 1A F8\r7E0 04 41 0C 20 00\r>");
    let ResearchTestResult::A30ReadTest(mixed_headers) = mixed_headers
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(mixed_headers.outcome, crate::A30ReadOutcome::WrongResponder);
    assert!(
        mixed_headers
            .detail
            .as_deref()
            .unwrap()
            .contains("unexpected CAN ID")
    );
    assert!(mixed_headers.graph_values.is_empty());
}

#[test]
fn sx_quick_test_records_no_data_and_propagates_fatal_serial_errors() {
    let (mut no_response, _) = started_adapter("NO DATA\r>");
    let ResearchTestResult::A30ReadTest(no_response) = no_response
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode03StoredDtcs,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(no_response.outcome, crate::A30ReadOutcome::NoResponse);

    let written = Arc::new(Mutex::new(Vec::new()));
    let mut broken = ObdLinkSxAdapter::with_port(
        "COM4",
        Box::new(BrokenPort {
            written: written.clone(),
        }),
    );
    broken.device = Some(crate::DeviceDescriptor {
        stable_id: "sx-test".into(),
        hardware_family: "OBDLink SX".into(),
        firmware: None,
        path: Some("COM4".into()),
    });
    broken.restore_adapter_settings = true;
    broken.started = Some(Instant::now());

    let error = broken
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap_err();
    assert!(error.to_string().contains("USB transport disconnected"));
    assert_eq!(*written.lock().unwrap(), b"STPX H:7DF, D:010C, T:600\r");
}

#[test]
fn sx_mode06_monitor_requires_the_requested_tid_and_graphs_raw_counts() {
    let transcript = include_str!("../../tests/fixtures/obdlink_sx/mode06_tid_35.txt");
    let (mut adapter, _) = started_adapter(transcript);

    let ResearchTestResult::A30ReadTest(result) = adapter
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode06Tid35,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };

    assert_eq!(result.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(result.graph_values.len(), 1);
    assert_eq!(
        result.graph_values[0].signal_id,
        "obdlink_sx_mode06_tid_35_mid_01"
    );
    assert_eq!(result.graph_values[0].value, 100.0);
    assert_eq!(result.graph_values[0].unit, "raw count");
    assert!(result.detail.as_deref().unwrap().contains("UASID 00: 100"));
    assert_eq!(
        result.graph_values[0].response_payload.as_deref(),
        Some(&[0x46, 0x35, 0x01, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0xFF][..])
    );

    let (mut wrong_tid, _) = started_adapter("0635\r7E8 06 46 36 01 00 00 64 00\r>");
    let ResearchTestResult::A30ReadTest(wrong_tid) = wrong_tid
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode06Tid35,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(wrong_tid.outcome, crate::A30ReadOutcome::MalformedResponse);
    assert!(wrong_tid.graph_values.is_empty());

    let (mut mixed_tids, _) = started_adapter(
        "0635\r7E8 10 13 46 35 0A 01 00 64\r7E8 21 00 10 00 C8 36 0B 01\r7E8 22 00 C8 00 20 00 FA\r>",
    );
    let ResearchTestResult::A30ReadTest(mixed_tids) = mixed_tids
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode06Tid35,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(mixed_tids.outcome, crate::A30ReadOutcome::MalformedResponse);
    assert!(mixed_tids.graph_values.is_empty());
    assert_eq!(mixed_tids.response_payloads.len(), 1);
    assert_eq!(mixed_tids.response_payloads[0][0], 0x46);

    let (mut pending_final, _) = started_adapter(
        "0635\r7E8 03 7F 06 78\r7E8 10 0A 46 35 01 00 00 64\r7E8 21 00 00 00 FF\r7E8 10 0A 46 35 02 00 00 C8\r7E8 21 00 00 00 FF\r>",
    );
    let ResearchTestResult::A30ReadTest(pending_final) = pending_final
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode06Tid35,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(pending_final.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(pending_final.response_payloads.len(), 3);
    assert_eq!(pending_final.response_payloads[0], [0x7F, 0x06, 0x78]);
    assert_eq!(pending_final.graph_values.len(), 2);
    assert_eq!(
        pending_final.graph_values[1].signal_id,
        "obdlink_sx_mode06_tid_35_mid_02"
    );
    assert_eq!(pending_final.negative_response_code, None);
    assert_eq!(
        pending_final.graph_values[0].response_payload.as_deref(),
        Some(&[0x46, 0x35, 0x01, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0xFF][..])
    );
    assert_eq!(
        pending_final.graph_values[1].response_payload.as_deref(),
        Some(&[0x46, 0x35, 0x02, 0x00, 0x00, 0xC8, 0x00, 0x00, 0x00, 0xFF][..])
    );

    let (mut pending_only, _) = started_adapter("0635\r7E8 03 7F 06 78\r>");
    let ResearchTestResult::A30ReadTest(pending_only) = pending_only
        .execute_research_test(ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode06Tid35,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(pending_only.outcome, crate::A30ReadOutcome::ResponsePending);
    assert_eq!(pending_only.negative_response_code, Some(0x78));
    assert!(pending_only.graph_values.is_empty());
}

#[test]
fn sx_one_shot_sends_one_catalog_command_and_restores_adapter_settings() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let port = FixturePort {
        input: Cursor::new(format!(
            "{}STPRS\rISO 15765-4 (CAN 11/500) [AT]\r\r>OK\r\r>OK\r\r>OK\r\r>7E8 03 61 01 64 00 00 00 00\r\r>OK\r\r>OK\r\r>OK\r\r>",
            include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        ).into_bytes()),
        written: written.clone(),
    };
    let mut adapter = ObdLinkSxAdapter::with_port("COM4", Box::new(port));
    let spec = spec();
    let result = adapter.try_oem_candidate_inner(&spec).unwrap();
    adapter.disconnect();

    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.decoded_value.unwrap().raw_value, "100");
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\rSTPRS\rATE0\rAT H1\rAT SH 7DF\rSTPX H:7E0, D:2101, T:250\rAT H0\rAT SH 7DF\rATE1\r"
    );
}

#[test]
fn sx_candidate_reuses_connected_session() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let port = FixturePort {
        input: Cursor::new(format!(
            "{}4100 00 10 00 00\r>ISO 15765-4 (CAN 11/500)\r>OK\r>OK\r>OK\r>7E8 06 41 00 00 10 00 00\r>7E8 03 61 01 64\r>7E8 04 62 12 34 64\r>7E8 04 41 0C 1A F8\r>",
            include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        ).into_bytes()),
        written: written.clone(),
    };
    let mut adapter = ObdLinkSxAdapter::with_port("no-second-open", Box::new(port));
    let capabilities = adapter.connect().unwrap();
    written.lock().unwrap().clear();

    let crate::ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(crate::ResearchTestRequest::OemCandidate(spec()))
        .unwrap()
    else {
        panic!("expected candidate result")
    };

    assert_eq!(result.adapter, capabilities.device);
    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(&*written.lock().unwrap(), b"STPX H:7E0, D:2101, T:250\r");

    let mut did = spec();
    did.candidate.service = "22".into();
    did.candidate.identifier = "1234".into();
    let crate::ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(crate::ResearchTestRequest::OemCandidate(did))
        .unwrap()
    else {
        panic!("expected candidate result")
    };
    assert_eq!(result.response_payload, [0x62, 0x12, 0x34, 0x64]);
    assert_eq!(
        &*written.lock().unwrap(),
        b"STPX H:7E0, D:2101, T:250\rSTPX H:7E0, D:221234, T:250\r"
    );

    written.lock().unwrap().clear();
    let mut invalid = spec();
    invalid.candidate.request_header = "unverified".into();
    assert!(
        adapter
            .execute_research_test(crate::ResearchTestRequest::OemCandidate(invalid))
            .is_err()
    );
    let ResearchTestResult::A30ReadTest(result) = adapter
        .execute_research_test(crate::ResearchTestRequest::A30ReadTest(
            crate::A30ReadTest::Mode01EngineRpm,
        ))
        .unwrap()
    else {
        panic!("expected quick-test result")
    };
    assert_eq!(result.outcome, crate::A30ReadOutcome::Positive);
    assert_eq!(result.graph_values[0].signal_id, "engine_rpm");
    assert_eq!(*written.lock().unwrap(), b"STPX H:7DF, D:010C, T:600\r");
}

struct BrokenPort {
    written: Arc<Mutex<Vec<u8>>>,
}

impl Read for BrokenPort {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "USB transport disconnected",
        ))
    }
}

impl Write for BrokenPort {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.written.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn connected_candidate_propagates_serial_transport_failure() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let mut adapter = ObdLinkSxAdapter::with_port(
        "COM4",
        Box::new(BrokenPort {
            written: written.clone(),
        }),
    );
    adapter.device = Some(crate::DeviceDescriptor {
        stable_id: "sx-test".into(),
        hardware_family: "OBDLink SX".into(),
        firmware: None,
        path: Some("COM4".into()),
    });
    adapter.restore_adapter_settings = true;
    adapter.started = Some(Instant::now());

    let error = adapter
        .execute_research_test(ResearchTestRequest::OemCandidate(spec()))
        .unwrap_err();

    assert!(error.to_string().contains("USB transport disconnected"));
    assert_eq!(*written.lock().unwrap(), b"STPX H:7E0, D:2101, T:250\r");
}

#[test]
fn ecu_no_response_text_remains_a_typed_candidate_outcome() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let mut adapter = ObdLinkSxAdapter::with_port(
        "COM4",
        Box::new(FixturePort {
            input: Cursor::new(b"NO DATA\r>".to_vec()),
            written: written.clone(),
        }),
    );
    adapter.device = Some(crate::DeviceDescriptor {
        stable_id: "sx-test".into(),
        hardware_family: "OBDLink SX".into(),
        firmware: None,
        path: Some("COM4".into()),
    });
    adapter.restore_adapter_settings = true;
    adapter.started = Some(Instant::now());

    let ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(ResearchTestRequest::OemCandidate(spec()))
        .unwrap()
    else {
        panic!("expected candidate result")
    };

    assert_eq!(result.outcome, OemCandidateOutcome::NoResponse);
    assert_eq!(result.raw_response, "NO DATA\r>");
    assert_eq!(*written.lock().unwrap(), b"STPX H:7E0, D:2101, T:250\r");
}
