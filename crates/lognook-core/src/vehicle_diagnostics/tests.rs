use super::*;

fn query(
    payload: Vec<u8>,
    kind: DiagnosticReadKind,
    identifier: Option<u8>,
) -> DiagnosticQueryRequest {
    DiagnosticQueryRequest {
        service: payload[0],
        identifier,
        payload,
        kind,
        wait_ms: 1,
    }
}

#[test]
fn request_deduplicates_ids_and_rejects_bitmap_pages_and_oversized_lists() {
    let mut request = DiagnosticRequest {
        mode06_test_ids: vec![0x35, 0x02, 0x35],
    };
    request.validate().unwrap();
    assert_eq!(request.mode06_test_ids, [0x02, 0x35]);
    assert!(
        DiagnosticRequest {
            mode06_test_ids: vec![0x20]
        }
        .validate()
        .is_err()
    );
    assert!(
        DiagnosticRequest {
            mode06_test_ids: vec![1; 33]
        }
        .validate()
        .is_err()
    );
}

#[test]
fn diagnostic_response_keeps_each_ecu_and_decodes_dtc_identity_and_bitmap() {
    let parsed = parse_adapter_response(
        &query(vec![0x03], DiagnosticReadKind::StoredDtcs, None),
        "03".into(),
        "7E8 04 43 01 03 01\r>".into(),
        10,
        2,
    );
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(
        parsed[0].values,
        [DiagnosticValue::DtcList {
            codes: vec!["P0301".into()]
        }]
    );

    let parsed = parse_adapter_response(
        &query(vec![0x09, 0x02], DiagnosticReadKind::Vin, Some(0x02)),
        "0902".into(),
        "7E8 10 14 49 02 01 4A 54 4C\r7E8 21 5A 45 34 46 45 31 45\r7E8 22 4A 30 34 39 33 31 34\r>"
            .into(),
        10,
        2,
    );
    assert_eq!(parsed[0].outcome, DiagnosticOutcome::Positive);
    assert!(
        matches!(&parsed[0].values[0], DiagnosticValue::Identity { value, .. } if value == "JTLZE4FE1EJ049314")
    );

    let parsed = parse_adapter_response(
        &query(
            vec![0x06, 0x00],
            DiagnosticReadKind::Mode06Bitmap,
            Some(0x00),
        ),
        "0600".into(),
        "7E8 06 46 00 C0 00 00 01\r>".into(),
        10,
        2,
    );
    assert_eq!(
        parsed[0].values,
        [DiagnosticValue::Mode06Bitmap {
            start: 0,
            test_ids: vec![1, 2],
            has_more: true
        }]
    );
}

#[test]
fn mode06_reports_raw_uasid_limits_and_distinct_malformed_and_negative_results() {
    let parsed = parse_adapter_response(
        &query(
            vec![0x06, 0x35],
            DiagnosticReadKind::Mode06Monitor,
            Some(0x35),
        ),
        "0635".into(),
        "7E8 10 0A 46 35 0A 01 00 64\r7E8 21 00 10 00 C8\r>".into(),
        10,
        2,
    );
    assert_eq!(parsed[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(
        parsed[0].values,
        [DiagnosticValue::Mode06Monitor {
            test_id: 0x35,
            component_id: 0x0A,
            uasid: 0x01,
            value: 100,
            minimum: 16,
            maximum: 200
        }]
    );

    let malformed = parse_adapter_response(
        &query(
            vec![0x06, 0x35],
            DiagnosticReadKind::Mode06Monitor,
            Some(0x35),
        ),
        "0635".into(),
        "7E8 04 46 35 01 02\r>".into(),
        10,
        2,
    );
    assert_eq!(malformed[0].outcome, DiagnosticOutcome::MalformedResponse);
    assert!(
        malformed[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("Unsupported Mode 06 payload format")
    );

    let negative = parse_adapter_response(
        &query(vec![0x03], DiagnosticReadKind::StoredDtcs, None),
        "03".into(),
        "7E8 03 7F 03 78\r>".into(),
        10,
        2,
    );
    assert_eq!(negative[0].outcome, DiagnosticOutcome::ResponsePending);
    assert_eq!(negative[0].negative_response_code, Some(0x78));
}

#[test]
fn decodes_counted_dtcs_for_all_modes_and_rejects_truncated_counts() {
    for (kind, service, positive) in [
        (DiagnosticReadKind::StoredDtcs, 0x03, 0x43),
        (DiagnosticReadKind::PendingDtcs, 0x07, 0x47),
        (DiagnosticReadKind::PermanentDtcs, 0x0A, 0x4A),
    ] {
        let parsed = parse_adapter_response(
            &query(vec![service], kind, None),
            format!("{service:02X}"),
            format!("7E8 06 {positive:02X} 02 01 33 01 01\r>"),
            10,
            2,
        );
        assert_eq!(parsed[0].outcome, DiagnosticOutcome::Positive);
        assert_eq!(
            parsed[0].values,
            [DiagnosticValue::DtcList {
                codes: vec!["P0133".into(), "P0101".into()]
            }]
        );
    }

    let zero = parse_adapter_response(
        &query(vec![0x03], DiagnosticReadKind::StoredDtcs, None),
        "03".into(),
        "7E8 02 43 00\r>".into(),
        10,
        2,
    );
    assert_eq!(
        zero[0].values,
        [DiagnosticValue::DtcList { codes: Vec::new() }]
    );

    let padded = parse_adapter_response(
        &query(vec![0x03], DiagnosticReadKind::StoredDtcs, None),
        "03".into(),
        "7E8 06 43 01 03 01 00 00\r>".into(),
        10,
        2,
    );
    assert_eq!(padded[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(
        padded[0].values,
        [DiagnosticValue::DtcList {
            codes: vec!["P0301".into()]
        }]
    );

    let truncated = parse_adapter_response(
        &query(vec![0x03], DiagnosticReadKind::StoredDtcs, None),
        "03".into(),
        "7E8 04 43 02 03 01\r>".into(),
        10,
        2,
    );
    assert_eq!(truncated[0].outcome, DiagnosticOutcome::MalformedResponse);
    assert!(
        truncated[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("declares 2")
    );
}

#[test]
fn mode09_checks_record_counts_and_trims_only_trailing_nul_padding() {
    let calibration_query = query(
        vec![0x09, 0x04],
        DiagnosticReadKind::CalibrationId,
        Some(0x04),
    );
    let mut calibration = vec![0x49, 0x04, 0x01];
    calibration.extend_from_slice(b"CALIBRATION1234\0");
    let calibration = calibration_query.report(
        Some("7E8".into()),
        calibration,
        String::new(),
        "0904".into(),
        10,
        2,
        false,
    );
    assert_eq!(calibration.outcome, DiagnosticOutcome::Positive);
    assert!(
        matches!(&calibration.values[0], DiagnosticValue::Identity { value, .. } if value == "CALIBRATION1234")
    );

    let mut bad_name = vec![0x49, 0x0A, 0x02];
    bad_name.extend_from_slice(b"ECU");
    let bad_name = query(vec![0x09, 0x0A], DiagnosticReadKind::EcuName, Some(0x0A)).report(
        Some("7E8".into()),
        bad_name,
        String::new(),
        "090A".into(),
        10,
        2,
        false,
    );
    assert_eq!(bad_name.outcome, DiagnosticOutcome::MalformedResponse);
    assert!(bad_name.detail.as_deref().unwrap().contains("ECU name"));
}

#[test]
fn bitmap_followup_occurs_only_when_the_ecu_advertises_continuation() {
    let mut reads = parse_adapter_response(
        &query(
            vec![0x06, 0xC0],
            DiagnosticReadKind::Mode06Bitmap,
            Some(0xC0),
        ),
        "06C0".into(),
        "7E8 06 46 C0 00 00 00 01\r>".into(),
        10,
        2,
    );
    let page_c0 = query(
        vec![0x06, 0xC0],
        DiagnosticReadKind::Mode06Bitmap,
        Some(0xC0),
    );
    assert_eq!(
        page_c0.next_mode06_bitmap(&reads).unwrap().identifier,
        Some(0xE0)
    );
    reads = parse_adapter_response(
        &query(
            vec![0x06, 0xE0],
            DiagnosticReadKind::Mode06Bitmap,
            Some(0xE0),
        ),
        "06E0".into(),
        "7E8 06 46 E0 00 00 00 01\r>".into(),
        10,
        2,
    );
    let page_e0 = query(
        vec![0x06, 0xE0],
        DiagnosticReadKind::Mode06Bitmap,
        Some(0xE0),
    );
    assert!(page_e0.next_mode06_bitmap(&reads).is_none());
    assert!(
        DiagnosticRequest {
            mode06_test_ids: vec![0xE0]
        }
        .validate()
        .is_err()
    );
}

#[test]
fn interleaved_iso_tp_replies_are_parsed_per_ecu_and_keep_a_malformed_peer() {
    let raw = "7E8 10 14 49 02 01 4A 54 4C\r7E9 10 14 49 02 01 4A 54 4C\r7E8 22 4A 30 34 39 33 31 34\r7E9 21 5A 45 34 46 45 31 45\r7E9 22 4A 30 34 39 33 31 34\r>";
    let parsed = parse_adapter_response(
        &query(vec![0x09, 0x02], DiagnosticReadKind::Vin, Some(0x02)),
        "ATSH7DF;0902".into(),
        raw.into(),
        10,
        3,
    );
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(parsed[0].outcome, DiagnosticOutcome::MalformedResponse);
    assert_eq!(parsed[1].ecu_id.as_deref(), Some("7E9"));
    assert_eq!(parsed[1].outcome, DiagnosticOutcome::Positive);
    assert!(
        matches!(&parsed[1].values[0], DiagnosticValue::Identity { value, .. } if value == "JTLZE4FE1EJ049314")
    );
    assert_eq!(parsed[0].raw_response, raw);
}

#[test]
fn unexpected_can_response_id_is_preserved_as_wrong_responder() {
    let parsed = parse_adapter_response(
        &query(vec![0x03], DiagnosticReadKind::StoredDtcs, None),
        "ATSH7DF;03".into(),
        "7D8 02 43 00\r>".into(),
        10,
        1,
    );
    assert_eq!(parsed[0].outcome, DiagnosticOutcome::WrongResponder);
    assert_eq!(parsed[0].ecu_id.as_deref(), Some("7D8"));
    assert_eq!(parsed[0].response_payload, [0x43, 0x00]);
    assert!(
        parsed[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("unexpected CAN ID")
    );
}

#[test]
fn diagnostic_query_validation_blocks_mismatched_service_payloads() {
    let forged = DiagnosticQueryRequest {
        kind: DiagnosticReadKind::Mode06Monitor,
        service: 0x09,
        identifier: Some(0x35),
        payload: vec![0x09, 0x35],
        wait_ms: 600,
    };
    assert!(forged.validate().is_err());
    assert_eq!(DiagnosticRequest::default().queries().unwrap().len(), 7);
}
