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

#[test]
fn imports_legacy_sample_csv_without_event_type_column() {
    let csv = b"elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex\n1,2,sx,7E8,engine_rpm,850,rpm,valid,live,0C34\n";

    let samples = import_csv(csv.as_slice()).unwrap();

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].signal_id, "engine_rpm");
    assert_eq!(samples[0].raw_bytes, Some(vec![0x0C, 0x34]));
}
