use lognook_core::{
    Marker, RecordingWriter, Sample, SampleOrigin, SampleQuality, export_recording_csv,
    read_recording,
};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_recording() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-{id}.lognook"))
}

fn sample(signal_id: &str, elapsed_ns: i64, value: f64) -> Sample {
    Sample {
        elapsed_ns,
        wall_time_unix_ns: 1_800_000_000_000_000_000 + elapsed_ns,
        source_id: "simulated-obd".into(),
        ecu_id: Some("engine".into()),
        signal_id: signal_id.into(),
        value,
        unit: if signal_id == "engine_rpm" {
            "rpm"
        } else {
            "°C"
        }
        .into(),
        quality: SampleQuality::Valid,
        origin: SampleOrigin::Live,
        raw_bytes: Some(vec![0x0C, 0x34]),
    }
}

#[test]
fn sqlite_recording_preserves_async_sample_times_and_markers() {
    let path = temp_recording();
    let mut writer = RecordingWriter::create(&path).unwrap();
    writer.append(&sample("coolant_temp", 110, 83.5)).unwrap();
    writer.append(&sample("engine_rpm", 100, 812.0)).unwrap();
    writer
        .add_marker(&Marker {
            elapsed_ns: 115,
            wall_time_unix_ns: 1_800_000_000_000_000_115,
            label: "throttle blip".into(),
            note: "operator marker".into(),
        })
        .unwrap();
    let summary = writer.finish().unwrap();

    let recording = read_recording(&path).unwrap();
    assert_eq!(summary.sample_count, 2);
    assert_eq!(
        recording
            .samples
            .iter()
            .map(|s| s.elapsed_ns)
            .collect::<Vec<_>>(),
        [100, 110]
    );
    assert_eq!(recording.samples[0].raw_bytes, Some(vec![0x0C, 0x34]));
    assert_eq!(recording.markers[0].label, "throttle blip");
    assert_eq!(recording.markers[0].note, "operator marker");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn csv_export_refuses_to_overwrite_the_source_recording() {
    let path = temp_recording();
    let mut writer = RecordingWriter::create(&path).unwrap();
    writer.append(&sample("engine_rpm", 100, 812.0)).unwrap();
    writer.finish().unwrap();

    assert!(export_recording_csv(&path, &path).is_err());
    assert_eq!(read_recording(&path).unwrap().samples[0].value, 812.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recording_export_contains_samples_and_markers_inline() {
    let path = temp_recording();
    let csv_path = path.with_extension("csv");
    let marker_path = csv_path.with_file_name(format!(
        "{}-markers.csv",
        csv_path.file_stem().unwrap().to_string_lossy()
    ));
    let mut writer = RecordingWriter::create(&path).unwrap();
    writer.append(&sample("engine_rpm", 100, 812.0)).unwrap();
    writer
        .add_marker(&Marker {
            elapsed_ns: 120,
            wall_time_unix_ns: 1_800_000_000_000_000_120,
            label: "pull".into(),
            note: "third gear, full load".into(),
        })
        .unwrap();
    writer.finish().unwrap();

    export_recording_csv(&path, &csv_path).unwrap();

    let mut reader = csv::Reader::from_path(&csv_path).unwrap();
    assert_eq!(reader.headers().unwrap().get(0), Some("event_type"));
    let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get(0), Some("sample"));
    assert_eq!(rows[1].get(0), Some("marker"));
    assert_eq!(rows[1].get(12), Some("third gear, full load"));
    assert!(!marker_path.exists());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(csv_path).unwrap();
    if marker_path.exists() {
        std::fs::remove_file(marker_path).unwrap();
    }
}
