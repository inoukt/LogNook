use lognook_core::{
    CanByteOrder, CanDirection, CanFrame, CanSignalDefinition, SampleOrigin, SampleQuality,
    decode_can_signal,
};

fn frame(data: &[u8]) -> CanFrame {
    CanFrame {
        elapsed_ns: 987_654,
        wall_time_unix_ns: 123_456,
        can_id: 0x123,
        extended: false,
        direction: CanDirection::Rx,
        data: data.to_vec(),
        flags: 0,
    }
}
fn signal() -> CanSignalDefinition {
    CanSignalDefinition {
        id: "can:test".into(),
        name: "Research field".into(),
        can_id: 0x123,
        extended: false,
        start_bit: 0,
        bit_length: 8,
        byte_order: CanByteOrder::Intel,
        signed: false,
        factor: 1.0,
        offset: 0.0,
        unit: "raw".into(),
    }
}
fn value(data: &[u8], definition: &CanSignalDefinition) -> f64 {
    decode_can_signal(&frame(data), definition, "candlelight:test")
        .unwrap()
        .unwrap()
        .value
}
#[test]
fn classic_can_id_and_payload_bounds() {
    for (id, extended, valid) in [
        (0x7ff, false, true),
        (0x800, false, false),
        (0x1fffffff, true, true),
        (0x20000000, true, false),
    ] {
        let mut f = frame(&[0; 8]);
        f.can_id = id;
        f.extended = extended;
        assert_eq!(f.validate().is_ok(), valid);
    }
    assert!(frame(&[]).validate().is_ok());
    assert!(frame(&[0; 9]).validate().is_err());
}
#[test]
fn classic_can_dlc_matches_payload() {
    assert!(frame(&[1, 2]).validate_dlc(2).is_ok());
    assert!(frame(&[1, 2]).validate_dlc(1).is_err());
    assert!(frame(&[0; 8]).validate_dlc(9).is_err());
}
#[test]
fn intel_extracts_unaligned_lsb_first_fields() {
    let mut s = signal();
    s.start_bit = 4;
    s.bit_length = 12;
    assert_eq!(value(&[0xa0, 0xbc], &s), 0xbca as f64);
}
#[test]
fn motorola_extracts_sawtooth_cross_byte_fields() {
    let mut s = signal();
    s.byte_order = CanByteOrder::Motorola;
    s.start_bit = 3;
    s.bit_length = 12;
    assert_eq!(value(&[0x0a, 0xbc], &s), 0xabc as f64);
}
#[test]
fn signed_scaling_sign_extends_before_scaling() {
    let mut s = signal();
    s.signed = true;
    s.factor = 0.5;
    s.offset = 3.0;
    assert_eq!(value(&[0xfc], &s), 1.0);
    s.bit_length = 12;
    s.start_bit = 4;
    assert_eq!(value(&[0xc0, 0xff], &s), 1.0);
}
#[test]
fn unsigned_scaling_and_full_width_fields() {
    let mut s = signal();
    s.factor = 2.0;
    s.offset = -1.0;
    assert_eq!(value(&[128], &s), 255.0);
    s.factor = 1.0;
    s.offset = 0.0;
    s.bit_length = 64;
    assert_eq!(value(&[0xff; 8], &s), u64::MAX as f64);
    s.signed = true;
    assert_eq!(value(&[0xff; 8], &s), -1.0);
    s.byte_order = CanByteOrder::Motorola;
    s.start_bit = 7;
    assert_eq!(value(&[0xff; 8], &s), -1.0);
}
#[test]
fn short_frames_and_invalid_definitions_are_rejected() {
    let mut s = signal();
    s.bit_length = 16;
    assert!(decode_can_signal(&frame(&[1]), &s, "test").is_err());
    for length in [0, 65] {
        s.bit_length = length;
        assert!(s.validate().is_err());
    }
    s.bit_length = 2;
    s.start_bit = 63;
    assert!(s.validate().is_err());
    s.byte_order = CanByteOrder::Motorola;
    s.start_bit = 56;
    assert!(s.validate().is_err());
    s = signal();
    s.can_id = 0x800;
    assert!(s.validate().is_err());
}
#[test]
fn non_finite_scaling_or_results_are_rejected() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut s = signal();
        s.factor = bad;
        assert!(s.validate().is_err());
        s = signal();
        s.offset = bad;
        assert!(s.validate().is_err());
    }
    let mut s = signal();
    s.factor = f64::MAX;
    assert!(decode_can_signal(&frame(&[2]), &s, "test").is_err());
}
#[test]
fn samples_preserve_source_signal_frame_bytes_and_receive_timestamps() {
    let f = frame(&[7, 8]);
    let s = signal();
    let sample = decode_can_signal(&f, &s, "candlelight:test")
        .unwrap()
        .unwrap();
    assert_eq!(sample.elapsed_ns, 987_654);
    assert_eq!(sample.wall_time_unix_ns, 123_456);
    assert_eq!(sample.source_id, "candlelight:test");
    assert_eq!(sample.signal_id, "can:test");
    assert_eq!(sample.ecu_id.as_deref(), Some("123"));
    assert_eq!(sample.raw_bytes, Some(vec![7, 8]));
    assert_eq!(sample.unit, "raw");
    assert_eq!(sample.quality, SampleQuality::Suspect);
    assert_eq!(sample.origin, SampleOrigin::Live);
}
#[test]
fn unrelated_id_format_or_transmitted_frame_is_not_a_received_signal() {
    let s = signal();
    let mut f = frame(&[7]);
    f.can_id = 0x124;
    assert!(decode_can_signal(&f, &s, "test").unwrap().is_none());
    f.can_id = 0x123;
    f.extended = true;
    assert!(decode_can_signal(&f, &s, "test").unwrap().is_none());
    f.extended = false;
    f.direction = CanDirection::Tx;
    assert!(decode_can_signal(&f, &s, "test").unwrap().is_none());
}
