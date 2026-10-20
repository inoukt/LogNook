use lognook_core::{
    OemCandidate, OemCandidateOutcome, OemCandidateTrySpec, build_oem_candidate_command,
    decode_oem_candidate_response,
};

fn spec(
    service: &str,
    identifier: &str,
    offset: u32,
    length: u32,
    value_format: &str,
    decoder: &str,
) -> OemCandidateTrySpec {
    OemCandidateTrySpec {
        source_policy: lognook_core::OemCandidateSourcePolicy::IndexedPack,
        candidate: OemCandidate {
            candidate_id: "candidate-1".into(),
            source_id: "SCION_SOURCE".into(),
            family: "Toyota".into(),
            brand: "Scion".into(),
            model_scope: "xB 2014".into(),
            service: service.into(),
            identifier: identifier.into(),
            request_header: "7E0".into(),
            response_header: "7E8".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "Engine".into(),
            name: "Test signal".into(),
            signal_source_id: "SCION_SOURCE_01".into(),
            bit_offset: Some(offset),
            bit_length: Some(length),
            formula: "test formula".into(),
            decoder_json: decoder.into(),
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
        value_format: value_format.into(),
    }
}

#[test]
fn one_shot_command_preserves_service_width_and_catalog_route() {
    let mode21 = spec(
        "21",
        "01",
        0,
        8,
        "uint",
        "{\"bix\":0,\"len\":8,\"unit\":\"celsius\"}",
    );
    let mode22 = spec(
        "22",
        "1234",
        0,
        16,
        "uint",
        "{\"bix\":0,\"len\":16,\"unit\":\"celsius\"}",
    );

    assert_eq!(
        build_oem_candidate_command(&mode21).unwrap(),
        "STPX H:7E0, D:2101, T:250"
    );
    assert_eq!(
        build_oem_candidate_command(&mode22).unwrap(),
        "STPX H:7E0, D:221234, T:250"
    );
}

#[test]
fn one_shot_rejects_can_ids_outside_the_standard_11_bit_range() {
    let mut candidate = spec("21", "01", 0, 8, "uint", "{}");
    candidate.candidate.request_header = "800".into();
    assert!(build_oem_candidate_command(&candidate).is_err());

    candidate.candidate.request_header = "7E0".into();
    candidate.candidate.response_header = "800".into();
    assert_eq!(
        decode_oem_candidate_response(&candidate, "7E8 03 61 01 64").outcome,
        OemCandidateOutcome::InvalidCandidate
    );
}

#[test]
fn malformed_candidate_metadata_is_reported_without_panicking() {
    let mut candidate = spec("ZZ", "not-hex", 0, 8, "uint", "{}");
    candidate.candidate.request_header = "7E0".into();
    let result = decode_oem_candidate_response(&candidate, "7E8 03 61 01 64");

    assert_eq!(result.outcome, OemCandidateOutcome::InvalidCandidate);
    assert!(result.error.is_some());
}

#[test]
fn unknown_value_format_keeps_the_positive_raw_response_without_unsigned_decode() {
    let candidate = spec(
        "21",
        "01",
        0,
        8,
        "bcd",
        "{\"bix\":0,\"len\":8,\"unit\":\"celsius\"}",
    );
    let result = decode_oem_candidate_response(&candidate, "7E8 03 61 01 64");

    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.payload, [0x61, 0x01, 0x64]);
    assert!(result.decoded_value.is_none());
    assert!(result.decode_error.is_some());
}

#[test]
fn service21_positive_response_checks_echo_and_decodes_the_selected_channel() {
    let candidate = spec(
        "21",
        "01",
        0,
        8,
        "uint",
        "{\"bix\":0,\"len\":8,\"mul\":1,\"div\":2,\"add\":-40,\"unit\":\"celsius\"}",
    );
    let result = decode_oem_candidate_response(&candidate, "7E8 03 61 01 64 00 00 00 00\r>");

    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.responder_header.as_deref(), Some("7E8"));
    assert_eq!(result.payload, [0x61, 0x01, 0x64]);
    let value = result.decoded_value.unwrap();
    assert_eq!(value.raw_value, "100");
    assert_eq!(value.value, Some(10.0));
    assert_eq!(value.unit, "celsius");
}

#[test]
fn negative_response_codes_keep_pending_temporary_session_and_security_states_distinct() {
    let candidate = spec("21", "01", 0, 8, "uint", "{}");
    let check = |nrc: u8, expected| {
        let result = decode_oem_candidate_response(&candidate, &format!("7E8 03 7F 21 {nrc:02X}"));
        assert_eq!(result.outcome, expected);
        assert_eq!(result.negative_response_code, Some(nrc));
    };

    check(0x78, OemCandidateOutcome::ResponsePending);
    check(0x21, OemCandidateOutcome::TemporarilyUnavailable);
    check(0x22, OemCandidateOutcome::TemporarilyUnavailable);
    check(0x7E, OemCandidateOutcome::SessionRequired);
    check(0x7F, OemCandidateOutcome::SessionRequired);
    check(0x33, OemCandidateOutcome::SecurityRequired);
    check(0x11, OemCandidateOutcome::NegativeResponse);
    let security = decode_oem_candidate_response(&candidate, "7E8 03 7F 21 33");
    assert_eq!(
        serde_json::to_value(security.outcome).unwrap(),
        serde_json::json!("security_required")
    );
}

#[test]
fn service22_positive_response_echoes_both_did_bytes() {
    let candidate = spec(
        "22",
        "1234",
        0,
        16,
        "uint",
        "{\"bix\":0,\"len\":16,\"div\":10,\"unit\":\"celsius\"}",
    );
    let result = decode_oem_candidate_response(&candidate, "7E8 05 62 12 34 00 FA 00\r>");

    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.payload, [0x62, 0x12, 0x34, 0x00, 0xFA]);
    assert_eq!(result.decoded_value.unwrap().value, Some(25.0));
}

#[test]
fn one_shot_response_keeps_negative_no_response_and_wrong_route_distinct() {
    let candidate = spec(
        "21",
        "01",
        0,
        8,
        "uint",
        "{\"bix\":0,\"len\":8,\"unit\":\"celsius\"}",
    );

    assert_eq!(
        decode_oem_candidate_response(&candidate, "7E8 03 7F 21 11 00 00 00\r>").outcome,
        OemCandidateOutcome::NegativeResponse
    );
    assert_eq!(
        decode_oem_candidate_response(&candidate, "").outcome,
        OemCandidateOutcome::NoResponse
    );
    assert_eq!(
        decode_oem_candidate_response(&candidate, "7E9 03 61 01 64 00 00 00\r>").outcome,
        OemCandidateOutcome::WrongResponder
    );
    assert_eq!(
        decode_oem_candidate_response(&candidate, "7E8 03 61 02 64 00 00 00\r>").outcome,
        OemCandidateOutcome::MalformedResponse
    );
}
