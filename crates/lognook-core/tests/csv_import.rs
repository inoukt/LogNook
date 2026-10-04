use lognook_core::{
    Marker, RecordingSignal, RecordingWriter, Sample, SampleOrigin, SampleQuality,
    export_recording_csv, import_csv_to_recording, inspect_recording, read_recording,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lognook-csv-import-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn legacy_csv(rows: &str) -> String {
    format!(
        "elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex\n{rows}"
    )
}

fn sidecar_path(csv: &Path) -> PathBuf {
    let stem = csv.file_stem().unwrap().to_string_lossy();
    csv.with_file_name(format!("{stem}-markers.csv"))
}

#[test]
fn imports_sample_only_csv_without_event_type() {
    let directory = TempDir::new();
    let source = directory.path("legacy.csv");
    let target = directory.path("legacy.lognook");
    fs::write(
        &source,
        legacy_csv("100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34\n"),
    )
    .unwrap();
    let original = fs::read(&source).unwrap();

    let summary = import_csv_to_recording(&source, &target).unwrap();
    let recording = read_recording(&target).unwrap();

    assert_eq!(summary.sample_count, 1);
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(recording.samples[0].signal_id, "engine_rpm");
    assert_eq!(recording.samples[0].raw_bytes, Some(vec![0x0C, 0x34]));
}

#[test]
fn imports_forscan_semicolon_wide_csv_as_numeric_samples() {
    let directory = TempDir::new();
    let source = directory.path("forscan.csv");
    let target = directory.path("forscan.lognook");
    fs::write(
        &source,
        "time(ms);MISFIRE;RPM(1/min);SPARKADV(°);SPARKADV(°)\n0;No;750;1.0;1.5\n100;Yes;800;2.0;2.5\n200;No;850;-;3.5\n",
    )
    .unwrap();

    let summary = import_csv_to_recording(&source, &target).unwrap();
    let recording = read_recording(&target).unwrap();

    assert_eq!(summary.sample_count, 11);
    assert_eq!(recording.samples.len(), 11);
    assert_eq!(recording.samples[0].signal_id, "MISFIRE");
    assert_eq!(recording.samples[0].value, 0.0);
    assert_eq!(recording.samples[4].elapsed_ns, 100_000_000);
    assert_eq!(recording.samples[4].value, 1.0);
    assert_eq!(recording.samples[1].signal_id, "RPM");
    assert_eq!(recording.samples[1].unit, "1/min");
    assert_eq!(recording.samples[3].signal_id, "SPARKADV #2");
    assert_eq!(recording.samples[3].unit, "°");
}

#[test]
fn imports_legacy_marker_sidecar_once() {
    let directory = TempDir::new();
    let source = directory.path("old.csv");
    let target = directory.path("old.lognook");
    fs::write(
        &source,
        legacy_csv("100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34\n"),
    )
    .unwrap();
    fs::write(
        sidecar_path(&source),
        "elapsed_ns,wall_time_unix_ns,label,note\n120,220,pull,third gear\n",
    )
    .unwrap();

    import_csv_to_recording(&source, &target).unwrap();
    let recording = read_recording(&target).unwrap();

    assert_eq!(recording.samples.len(), 1);
    assert_eq!(recording.markers.len(), 1);
    assert_eq!(recording.markers[0].label, "pull");
    assert_eq!(recording.markers[0].note, "third gear");
}

#[test]
fn inline_markers_take_precedence_over_sidecar() {
    let directory = TempDir::new();
    let source = directory.path("mixed.csv");
    let target = directory.path("mixed.lognook");
    fs::write(
        &source,
        "event_type,elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex,label,note\nsample,100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34,,\nmarker,120,220,,,,,,,,,inline marker,kept\n",
    )
    .unwrap();
    fs::write(
        sidecar_path(&source),
        "elapsed_ns,wall_time_unix_ns,label,note\n120,220,sidecar marker,ignored\n",
    )
    .unwrap();

    import_csv_to_recording(&source, &target).unwrap();
    let recording = read_recording(&target).unwrap();

    assert_eq!(recording.markers.len(), 1);
    assert_eq!(recording.markers[0].label, "inline marker");
}

#[test]
fn unknown_event_type_reports_row() {
    let directory = TempDir::new();
    let source = directory.path("unknown.csv");
    let target = directory.path("unknown.lognook");
    fs::write(
        &source,
        "event_type,elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex,label,note\nsample,100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34,,\nother,120,220,,,,,,,,,,\n",
    )
    .unwrap();

    let error = import_csv_to_recording(&source, &target).unwrap_err();

    assert!(error.to_string().contains("CSV row 3"));
    assert!(!target.exists());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn malformed_mixed_rows_and_negative_times_report_the_csv_row() {
    let directory = TempDir::new();
    let header = "event_type,elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex,label,note\n";
    let rows = [
        "sample,-1,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34,,\n",
        "sample,100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34,unexpected label,\n",
        "marker,100,200,,,,850,,,,,marker,\n",
    ];
    for (index, row) in rows.into_iter().enumerate() {
        let source = directory.path(&format!("malformed-{index}.csv"));
        let target = directory.path(&format!("malformed-{index}.lognook"));
        fs::write(&source, format!("{header}{row}")).unwrap();
        let error = import_csv_to_recording(&source, &target).unwrap_err();
        assert!(error.to_string().contains("CSV row 2"));
        assert!(!target.exists());
    }
}

#[test]
fn empty_csv_is_rejected() {
    let directory = TempDir::new();
    let source = directory.path("empty.csv");
    let target = directory.path("empty.lognook");
    fs::write(&source, "").unwrap();

    assert!(import_csv_to_recording(&source, &target).is_err());
    assert!(!target.exists());
}

#[test]
fn failed_import_removes_temp_and_preserves_target() {
    let directory = TempDir::new();
    let source = directory.path("valid.csv");
    let target = directory.path("existing.lognook");
    fs::write(
        &source,
        legacy_csv("100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34\n"),
    )
    .unwrap();
    fs::write(&target, b"existing session").unwrap();

    assert!(import_csv_to_recording(&source, &target).is_err());

    assert_eq!(fs::read(&target).unwrap(), b"existing session");
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
}

#[test]
fn inspect_recording_reports_duration_and_signals() {
    let directory = TempDir::new();
    let source = directory.path("session.csv");
    let target = directory.path("session.lognook");
    fs::write(
        &source,
        "event_type,elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex,label,note\nsample,100,200,sx,7E8,engine_rpm,850,rpm,valid,live,0C34,,\nsample,200,300,sx,7E8,coolant_temp,82,°C,valid,live,52,,\nmarker,300,400,,,,,,,,,End,last event\n",
    )
    .unwrap();

    import_csv_to_recording(&source, &target).unwrap();
    let info = inspect_recording(&target).unwrap();

    assert_eq!(info.sample_count, 2);
    assert_eq!(info.duration_ns, 300);
    assert_eq!(
        info.signals,
        vec![
            RecordingSignal {
                id: "coolant_temp".into(),
                unit: "°C".into()
            },
            RecordingSignal {
                id: "engine_rpm".into(),
                unit: "rpm".into()
            }
        ]
    );
}

#[test]
fn mixed_event_csv_round_trip_preserves_raw_samples_and_markers() {
    let directory = TempDir::new();
    let recording_path = directory.path("source.lognook");
    let csv_path = directory.path("source.csv");
    let imported_path = directory.path("imported.lognook");
    let sample = Sample {
        elapsed_ns: 123_456,
        wall_time_unix_ns: 1_800_000_000_000_123_456,
        source_id: "adapter,with comma".into(),
        ecu_id: Some("7E8".into()),
        signal_id: "engine_rpm".into(),
        value: 812.5,
        unit: "rpm".into(),
        quality: SampleQuality::Valid,
        origin: SampleOrigin::Live,
        raw_bytes: Some(vec![0x0C, 0x34]),
    };
    let marker = Marker {
        id: None,
        elapsed_ns: 456_789,
        wall_time_unix_ns: 1_800_000_000_000_456_789,
        label: "3rd gear, full load".into(),
        note: "operator note, kept intact".into(),
    };
    let optional_note_marker = Marker {
        id: None,
        elapsed_ns: 500_000,
        wall_time_unix_ns: 1_800_000_000_000_500_000,
        label: "no note".into(),
        note: String::new(),
    };
    let mut writer = RecordingWriter::create(&recording_path).unwrap();
    writer.append(&sample).unwrap();
    writer.add_marker(&marker).unwrap();
    writer.add_marker(&optional_note_marker).unwrap();
    writer.finish().unwrap();

    export_recording_csv(&recording_path, &csv_path).unwrap();
    import_csv_to_recording(&csv_path, &imported_path).unwrap();
    let restored = read_recording(&imported_path).unwrap();

    assert_eq!(restored.samples, vec![sample]);
    assert_eq!(restored.markers.len(), 2);
    assert_eq!(restored.markers[0].elapsed_ns, marker.elapsed_ns);
    assert_eq!(
        restored.markers[0].wall_time_unix_ns,
        marker.wall_time_unix_ns
    );
    assert_eq!(restored.markers[0].label, marker.label);
    assert_eq!(restored.markers[0].note, marker.note);
    assert_eq!(restored.markers[1].label, optional_note_marker.label);
    assert_eq!(restored.markers[1].note, "");
}
