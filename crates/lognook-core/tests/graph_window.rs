use lognook_core::{
    RecordingWriter, Sample, SampleOrigin, SampleQuality, query_recording_averages,
    query_recording_values_at, query_recording_window,
};
use rusqlite::params;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lognook-graph-window-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample(elapsed_ns: i64, signal_id: &str, value: f64) -> Sample {
    Sample {
        elapsed_ns,
        wall_time_unix_ns: elapsed_ns,
        source_id: "sim".into(),
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
        raw_bytes: None,
    }
}

#[test]
fn query_window_keeps_extrema_and_caps_output() {
    let directory = TempDir::new();
    let path = directory.0.join("small.lognook");
    let mut writer = RecordingWriter::create(&path).unwrap();
    for (time, value) in [5.0, 1.0, 9.0, 4.0].into_iter().enumerate() {
        writer
            .append(&sample(time as i64, "engine_rpm", value))
            .unwrap();
    }
    writer.append(&sample(2, "coolant_temp", 80.0)).unwrap();
    writer.finish().unwrap();

    let points = query_recording_window(&path, &["engine_rpm".into()], 0, 3, 1).unwrap();

    assert_eq!(points.len(), 4);
    assert_eq!(
        points.iter().map(|point| point.value).collect::<Vec<_>>(),
        [5.0, 1.0, 9.0, 4.0]
    );
    assert!(points.iter().all(|point| point.signal_id == "engine_rpm"));
}

#[test]
fn query_values_at_returns_latest_selected_samples_before_playhead() {
    let directory = TempDir::new();
    let path = directory.0.join("values.lognook");
    let mut writer = RecordingWriter::create(&path).unwrap();
    writer.append(&sample(0, "engine_rpm", 800.0)).unwrap();
    writer.append(&sample(3, "coolant_temp", 82.0)).unwrap();
    writer.append(&sample(5, "engine_rpm", 900.0)).unwrap();
    writer.finish().unwrap();

    let values = query_recording_values_at(
        &path,
        &[
            "coolant_temp".into(),
            "engine_rpm".into(),
            "engine_rpm".into(),
            "missing".into(),
        ],
        4,
    )
    .unwrap();

    assert_eq!(values.len(), 3);
    assert_eq!(values[0].signal_id, "coolant_temp");
    assert_eq!(values[0].elapsed_ns, 3);
    assert_eq!(values[1].signal_id, "engine_rpm");
    assert_eq!(values[1].elapsed_ns, 0);
    assert_eq!(values[2].signal_id, "engine_rpm");
    assert_eq!(values[2].elapsed_ns, 0);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let plan = connection
        .prepare(
            "EXPLAIN QUERY PLAN SELECT id, elapsed_ns FROM samples
             WHERE signal_id IN (?) AND elapsed_ns <= ?
             ORDER BY signal_id, elapsed_ns DESC, id DESC",
        )
        .unwrap()
        .query_map(params!["engine_rpm", 4], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("samples_by_signal_time")),
        "values-at query should use its composite index: {plan:?}"
    );
}

#[test]
fn query_averages_returns_per_signal_mean_for_visible_time_window() {
    let directory = TempDir::new();
    let path = directory.0.join("averages.lognook");
    let mut writer = RecordingWriter::create(&path).unwrap();
    for (time, signal, value) in [
        (0, "engine_rpm", 800.0),
        (1, "engine_rpm", 1_000.0),
        (2, "engine_rpm", 1_200.0),
        (1, "coolant_temp", 80.0),
    ] {
        writer.append(&sample(time, signal, value)).unwrap();
    }
    writer.finish().unwrap();

    let averages = query_recording_averages(&path, 1, 2).unwrap();

    assert_eq!(averages.get("engine_rpm"), Some(&1_100.0));
    assert_eq!(averages.get("coolant_temp"), Some(&80.0));
}

#[test]
fn query_window_clamps_bounds_and_caps_output() {
    let directory = TempDir::new();
    let path = directory.0.join("large.lognook");
    RecordingWriter::create(&path).unwrap().finish().unwrap();
    {
        let mut connection = rusqlite::Connection::open(&path).unwrap();
        let transaction = connection.transaction().unwrap();
        for elapsed_ns in 0..16_000_i64 {
            let value = [5.0, 1.0, 9.0, 4.0][elapsed_ns as usize % 4];
            transaction
                .execute(
                    "INSERT INTO samples(elapsed_ns, wall_time_unix_ns, source_id, ecu_id,
                     signal_id, value, unit, quality, origin, raw_bytes)
                     VALUES (?1, ?1, 'sx', 'engine', 'engine_rpm', ?2, 'rpm', 'valid', 'live', NULL)",
                    params![elapsed_ns, value],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        connection
            .execute_batch(
                "DELETE FROM metadata WHERE key IN
                 ('summary_complete', 'sample_count', 'sample_duration_ns', 'signals_json');",
            )
            .unwrap();
    }

    let points = query_recording_window(&path, &["engine_rpm".into()], -10, 20_000, 4_000).unwrap();
    assert_eq!(points.len(), 8_000);
    assert_eq!(points.first().unwrap().elapsed_ns, 0);
    assert_eq!(points.last().unwrap().elapsed_ns, 15_999);
    assert!(
        query_recording_window(&path, &["engine_rpm".into()], 10, 5, 20)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        query_recording_window(&path, &["engine_rpm".into()], 3, 3, 0)
            .unwrap()
            .len(),
        1
    );
}
