use super::{
    Mode01Response, Mode01ResponseError, decode_signal, parse_mode01_payload,
    parse_mode01_responses, parse_supported_pid_payload, parse_supported_pids,
};
use crate::standard_obd_signals;

#[test]
fn supported_bitmap_selects_only_the_reported_generic_pids() {
    let bitmap = include_str!("../tests/fixtures/obdlink_sx/mode01_supported.txt");
    let range = parse_supported_pids(bitmap, 0x00).unwrap();

    assert_eq!(
        range.pids,
        vec![
            0x01, 0x03, 0x04, 0x05, 0x06, 0x07, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x13, 0x14,
            0x15, 0x1C, 0x1F
        ]
    );
    assert!(range.has_more);
    assert!(!range.pids.contains(&0x12));
    assert!(!range.pids.contains(&0x20));
}

#[test]
fn supported_pid_continuation_bitmap_maps_the_next_range() {
    let range = parse_supported_pids("0120\r41 20 80 00 00 00 \r\r>", 0x20).unwrap();

    assert_eq!(range.pids, vec![0x21]);
    assert!(!range.has_more);
}

#[test]
fn headered_pid_60_bitmap_from_scion_is_parsed_after_can_discovery() {
    let text = include_str!("../tests/fixtures/obdlink_sx/mode01_supported_60_header.txt");
    let range = parse_supported_pids(text, 0x60).unwrap();

    assert_eq!(range.pids, vec![0x65]);
    assert!(!range.has_more);
    assert_eq!(range.ecu_id.as_deref(), Some("7E8"));
}

#[test]
fn every_later_mode01_supported_bitmap_range_uses_four_data_bytes() {
    for range_start in [0x60, 0x80, 0xA0, 0xC0, 0xE0] {
        let text = format!("7E8 06 41 {range_start:02X} 80 00 00 00\r>");
        let range = parse_supported_pids(&text, range_start).unwrap();

        assert_eq!(range.pids, vec![range_start + 1], "range {range_start:02X}");
        assert!(!range.has_more, "range {range_start:02X}");
        assert_eq!(range.ecu_id.as_deref(), Some("7E8"));
    }
}

#[test]
fn rpm_response_keeps_ecu_and_raw_bytes_and_decodes_to_1726_rpm() {
    let text = include_str!("../tests/fixtures/obdlink_sx/mode01_rpm.txt");
    let response = parse_mode01_responses(text, 0x01, 0x0C).unwrap();
    let signal = standard_obd_signals()
        .into_iter()
        .find(|item| item.id == "engine_rpm")
        .unwrap();

    assert_eq!(
        response,
        vec![Mode01Response {
            ecu_id: Some("7E8".into()),
            raw_bytes: vec![0x41, 0x0C, 0x1A, 0xF8],
            data: vec![0x1A, 0xF8],
        }]
    );
    assert_eq!(decode_signal(&signal, &response[0]).unwrap(), 1726.0);
}

#[test]
fn binary_mode01_payload_keeps_ecu_raw_bytes_and_decodes_rpm() {
    let response = parse_mode01_payload("7E8".into(), &[0x41, 0x0C, 0x1A, 0xF8], 0x0C).unwrap();
    let signal = standard_obd_signals()
        .into_iter()
        .find(|item| item.id == "engine_rpm")
        .unwrap();

    assert_eq!(
        response,
        Mode01Response {
            ecu_id: Some("7E8".into()),
            raw_bytes: vec![0x41, 0x0C, 0x1A, 0xF8],
            data: vec![0x1A, 0xF8],
        }
    );
    assert_eq!(decode_signal(&signal, &response).unwrap(), 1726.0);
}

#[test]
fn binary_supported_pid_bitmap_decodes_reported_pid_and_continuation() {
    let range =
        parse_supported_pid_payload("7E8".into(), &[0x41, 0x00, 0x80, 0x00, 0x00, 0x01], 0x00)
            .unwrap();

    assert_eq!(range.pids, vec![0x01]);
    assert!(range.has_more);
    assert_eq!(range.ecu_id.as_deref(), Some("7E8"));
}

#[test]
fn binary_mode01_payload_rejects_wrong_service_pid_and_truncated_data() {
    assert!(parse_mode01_payload("7E8".into(), &[0x7F, 0x01, 0x11], 0x0C).is_err());
    assert!(parse_mode01_payload("7E8".into(), &[0x41, 0x05, 0x64], 0x0C).is_err());
    assert!(parse_mode01_payload("7E8".into(), &[0x41, 0x0C, 0x1A], 0x0C).is_err());
    assert!(
        parse_mode01_payload("7E8".into(), &[0x41, 0x0C, 0x1A, 0xF8, 0x05, 0x64], 0x0C).is_err()
    );
}

#[test]
fn standard_pid_values_use_the_existing_signal_formulas() {
    let cases = [
        (
            include_str!("../tests/fixtures/obdlink_sx/mode01_coolant.txt"),
            "coolant_temp",
            50.0,
        ),
        (
            include_str!("../tests/fixtures/obdlink_sx/mode01_intake.txt"),
            "intake_temp",
            0.0,
        ),
        (
            include_str!("../tests/fixtures/obdlink_sx/mode01_throttle.txt"),
            "throttle_pos",
            128.0 * 100.0 / 255.0,
        ),
        (
            include_str!("../tests/fixtures/obdlink_sx/mode01_speed.txt"),
            "vehicle_speed",
            0.0,
        ),
    ];

    for (text, signal_id, expected) in cases {
        let signal = standard_obd_signals()
            .into_iter()
            .find(|item| item.id == signal_id)
            .unwrap();
        let response = parse_mode01_responses(text, 0x01, signal.pid).unwrap();
        assert_eq!(
            decode_signal(&signal, &response[0]).unwrap(),
            expected,
            "{signal_id}"
        );
    }
}

#[test]
fn malformed_or_missing_payload_is_rejected() {
    assert!(parse_supported_pids("41 00 01>", 0x00).is_err());
    assert!(parse_mode01_responses("NO DATA\r>", 0x01, 0x0C).is_err());
    let signal = standard_obd_signals()
        .into_iter()
        .find(|item| item.id == "engine_rpm")
        .unwrap();
    let short = Mode01Response {
        ecu_id: None,
        raw_bytes: vec![0x41, 0x0C, 0x10],
        data: vec![0x10],
    };
    assert!(decode_signal(&signal, &short).is_err());
}

#[test]
fn negative_mode01_responses_report_the_echoed_service_and_nrc() {
    let negative = parse_mode01_responses("7E8 03 7F 01 12\r>", 0x01, 0x0C).unwrap_err();
    assert!(matches!(
        &negative,
        Mode01ResponseError::NegativeResponse {
            requested_service: 0x01,
            requested_pid: 0x0C,
            response_service: 0x01,
            nrc: 0x12,
            ..
        }
    ));
    assert!(!negative.is_transient());
    let text_error = negative.to_string();
    assert_eq!(
        text_error,
        "ECU 7E8 returned a negative response to Mode 01 PID 0C (service 0x01): NRC 0x12"
    );

    let legacy_error = parse_mode01_responses("010C\r7F 01 12\r>", 0x01, 0x0C)
        .unwrap_err()
        .to_string();
    assert_eq!(
        legacy_error,
        "returned a negative response to Mode 01 PID 0C (service 0x01): NRC 0x12"
    );

    let binary_error = parse_mode01_payload("7E8".into(), &[0x7F, 0x01, 0x12], 0x0C).unwrap_err();
    assert_eq!(binary_error, text_error);
    assert!(
        parse_mode01_responses("NO DATA\r>", 0x01, 0x0C)
            .unwrap_err()
            .is_transient()
    );
}

#[test]
fn multiple_ecu_responses_remain_separate() {
    let text = "7E8 04 41 0C 1A F8\r7E9 04 41 0C 10 00\r>";
    let responses = parse_mode01_responses(text, 0x01, 0x0C).unwrap();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(responses[1].ecu_id.as_deref(), Some("7E9"));
}

#[test]
fn unknown_trailing_pid_does_not_discard_the_requested_response() {
    let responses = parse_mode01_responses("7E8 06 41 0C 09 54 FE 01", 0x01, 0x0C).unwrap();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].raw_bytes, [0x41, 0x0C, 0x09, 0x54]);
    assert_eq!(responses[0].data, [0x09, 0x54]);
}

#[test]
fn multiple_ecu_responses_on_one_line_remain_separate() {
    let text = "7E8 04 41 0C 1A F8 7E9 04 41 0C 10 00>";
    let responses = parse_mode01_responses(text, 0x01, 0x0C).unwrap();

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(responses[1].ecu_id.as_deref(), Some("7E9"));
}

#[test]
fn supported_pid_discovery_rejects_multiple_ecu_bitmaps() {
    let text = "7E8 06 41 00 BE 3E B8 13\r7E9 06 41 00 BE 3E B8 13\r>";
    assert!(parse_supported_pids(text, 0x00).is_err());
}

#[test]
fn signal_catalog_contains_all_35_advertised_seed_candidates() {
    let mut pids: Vec<_> = standard_obd_signals()
        .into_iter()
        .map(|signal| signal.pid)
        .collect();
    pids.sort_unstable();
    assert_eq!(
        pids,
        vec![
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x13,
            0x1C, 0x1F, 0x21, 0x23, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x3C, 0x3D, 0x42, 0x43,
            0x44, 0x45, 0x46, 0x47, 0x49, 0x4A, 0x4C
        ]
    );
}

#[test]
fn advertised_signal_decoders_match_seed_formulas_and_raw_codes() {
    let cases: &[(u8, &[u8], f64)] = &[
        (0x03, &[0x01, 0x02], 258.0),
        (0x04, &[0x80], 50.19607843137255),
        (0x05, &[0x8C], 100.0),
        (0x06, &[0x80], 0.0),
        (0x07, &[0x00], -100.0),
        (0x08, &[0xFF], 99.21875),
        (0x09, &[0x80], 0.0),
        (0x0B, &[0x64], 100.0),
        (0x0C, &[0x1A, 0xF8], 1726.0),
        (0x0D, &[0x64], 100.0),
        (0x0E, &[0x80], 0.0),
        (0x0F, &[0x63], 59.0),
        (0x11, &[0x80], 50.19607843137255),
        (0x13, &[0x28], 40.0),
        (0x1C, &[0x01], 1.0),
        (0x1F, &[0x01, 0x2C], 300.0),
        (0x21, &[0x01, 0x2C], 300.0),
        (0x23, &[0x01, 0x2C], 3000.0),
        (0x2E, &[0x80], 50.19607843137255),
        (0x2F, &[0xFF], 100.0),
        (0x30, &[0x2A], 42.0),
        (0x31, &[0x01, 0x2C], 300.0),
        (0x32, &[0xFF, 0x00], -64.0),
        (0x33, &[0x64], 100.0),
        (0x3C, &[0x02, 0x8A], 25.0),
        (0x3D, &[0x03, 0xE8], 60.0),
        (0x42, &[0x2E, 0xE0], 12.0),
        (0x43, &[0x40, 0x00], 6425.098039215686),
        (0x44, &[0x80, 0x00], 1.0),
        (0x45, &[0x80], 50.19607843137255),
        (0x46, &[0x50], 40.0),
        (0x47, &[0x80], 50.19607843137255),
        (0x49, &[0x80], 50.19607843137255),
        (0x4A, &[0x80], 50.19607843137255),
        (0x4C, &[0x80], 50.19607843137255),
    ];
    let signals = standard_obd_signals();

    for (pid, data, expected) in cases {
        let signal = signals
            .iter()
            .find(|signal| signal.pid == *pid)
            .unwrap_or_else(|| panic!("missing catalog PID {pid:02X}"));
        let mut raw_bytes = vec![0x41, *pid];
        raw_bytes.extend_from_slice(data);
        let response = Mode01Response {
            ecu_id: Some("7E8".into()),
            raw_bytes,
            data: data.to_vec(),
        };
        let actual = decode_signal(signal, &response)
            .unwrap_or_else(|error| panic!("PID {pid:02X} did not decode: {error}"));
        assert!(
            (actual - expected).abs() < 1e-9,
            "PID {pid:02X}: got {actual}, expected {expected}"
        );
    }
}

#[test]
fn stuffed_iso_tp_response_decodes_each_requested_pid() {
    let text = include_str!("../tests/fixtures/obdlink_sx/mode01_stuffed_five.txt");
    let signals = standard_obd_signals();
    let cases = [
        ("engine_rpm", 597.0),
        ("coolant_temp", 100.0),
        ("intake_temp", 59.0),
        ("throttle_pos", 30.0 * 100.0 / 255.0),
        ("vehicle_speed", 0.0),
    ];

    for (signal_id, expected) in cases {
        let signal = signals
            .iter()
            .find(|signal| signal.id == signal_id)
            .unwrap();
        let responses = parse_mode01_responses(text, 0x01, signal.pid).unwrap();
        assert_eq!(responses.len(), 1, "{signal_id}");
        let actual = decode_signal(signal, &responses[0]).unwrap();
        assert!((actual - expected).abs() < 1e-9, "{signal_id}");
    }
}

#[test]
fn truncated_isotp_response_does_not_accept_the_first_pid_as_complete() {
    let first_frame_only = "7E8 10 0C 41 0C 09 54 05 8C\r>";
    assert!(parse_mode01_responses(first_frame_only, 0x01, 0x0C).is_err());
}
