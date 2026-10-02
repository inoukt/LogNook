use lognook_core::{Sample, SampleOrigin, SampleQuality, export_csv, import_csv};

#[test]
fn long_csv_round_trip_preserves_async_values_and_escapes_fields() {
    let samples = vec![Sample {
        elapsed_ns: 123_456,
        wall_time_unix_ns: 1_800_000_000_000_123_456,
        source_id: "sim,usb".into(),
        ecu_id: Some("7E8".into()),
        signal_id: "engine_rpm".into(),
        value: 812.5,
        unit: "rpm".into(),
        quality: SampleQuality::Valid,
        origin: SampleOrigin::Live,
        raw_bytes: Some(vec![0x0C, 0x34]),
    }];
    let mut csv = Vec::new();

    export_csv(&samples, &mut csv).unwrap();
    let restored = import_csv(csv.as_slice()).unwrap();

    assert_eq!(restored, samples);
    assert!(String::from_utf8(csv).unwrap().contains("\"sim,usb\""));
}
