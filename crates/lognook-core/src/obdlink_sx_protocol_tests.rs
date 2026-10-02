use super::{Mode01Response, decode_signal, parse_mode01_responses, parse_supported_pids};
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
fn multiple_ecu_responses_remain_separate() {
    let text = "7E8 04 41 0C 1A F8\r7E9 04 41 0C 10 00\r>";
    let responses = parse_mode01_responses(text, 0x01, 0x0C).unwrap();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(responses[1].ecu_id.as_deref(), Some("7E9"));
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
