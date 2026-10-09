use crate::{
    model::{
        AutomaticTriggerConfig, EngineEvent, Sample, SampleOrigin, SampleQuality, TriggerDirection,
    },
    pipeline::SamplePipeline,
    read_recording,
};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
fn invalid_sample_stops_recording_and_keeps_partial_session_readable() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lognook-write-error-{id}.lognook"));
    let (tx, rx) = mpsc::channel();
    let pipeline = SamplePipeline::new(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    pipeline.start_recording(path.clone()).unwrap();
    pipeline
        .sender()
        .send_sample(Sample {
            elapsed_ns: 10,
            wall_time_unix_ns: 1_800_000_000_000_000_010,
            source_id: "simulated-obd".into(),
            ecu_id: Some("engine".into()),
            signal_id: "engine_rpm".into(),
            value: f64::NAN,
            unit: "rpm".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: None,
        })
        .unwrap();

    let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let second = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(
        matches!(first, EngineEvent::RecordingFailed(_))
            || matches!(second, EngineEvent::RecordingFailed(_))
    );
    assert!(!pipeline.is_recording());
    assert!(read_recording(&path).unwrap().samples.is_empty());
    drop(pipeline);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn disconnect_disarms_trigger_and_drops_samples_from_the_previous_connection() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lognook-disconnect-trigger-{id}.lognook"));
    let (tx, rx) = mpsc::channel();
    let pipeline = SamplePipeline::new(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    let config = AutomaticTriggerConfig {
        signal_id: "engine_rpm".into(),
        direction: TriggerDirection::Above,
        threshold: 100.0,
        hysteresis: 0.0,
        hold_ms: 0,
        pre_trigger_ms: 5_000,
    };

    pipeline
        .arm_automatic_trigger(config.clone(), path.clone())
        .unwrap();
    let _ = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    pipeline
        .sender()
        .send_sample(trigger_test_sample(100_000_000, 50.0))
        .unwrap();
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        EngineEvent::Sample(_)
    ));

    pipeline.sender().source_connected();
    pipeline.sender().source_disconnected(true, true).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while std::time::Instant::now() < deadline {
        if matches!(
            rx.recv_timeout(Duration::from_millis(100)),
            Ok(EngineEvent::Disconnected)
        ) {
            break;
        }
    }
    assert!(!pipeline.automatic_trigger_status().armed);

    pipeline
        .arm_automatic_trigger(config, path.clone())
        .unwrap();
    let _ = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    pipeline
        .sender()
        .send_sample(trigger_test_sample(100_000_000, 150.0))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    let mut triggered = false;
    while std::time::Instant::now() < deadline {
        if matches!(
            rx.recv_timeout(Duration::from_millis(100)),
            Ok(EngineEvent::Marker(_))
        ) {
            triggered = true;
            break;
        }
    }
    assert!(
        triggered,
        "the new connection's sample should trigger recording"
    );
    let summary = pipeline.stop_recording().unwrap();
    let recording = read_recording(&summary.path).unwrap();
    assert_eq!(
        recording.samples.len(),
        1,
        "old connection samples must not enter this session"
    );

    drop(pipeline);
    std::fs::remove_file(path).unwrap();
}

fn trigger_test_sample(elapsed_ns: i64, value: f64) -> Sample {
    Sample {
        elapsed_ns,
        wall_time_unix_ns: 1_800_000_000_000_000_000 + elapsed_ns,
        source_id: "test-adapter".into(),
        ecu_id: Some("engine".into()),
        signal_id: "engine_rpm".into(),
        value,
        unit: "rpm".into(),
        quality: SampleQuality::Valid,
        origin: SampleOrigin::Live,
        raw_bytes: None,
    }
}

#[test]
fn retrospective_recording_backfills_once_then_records_live_samples() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lognook-retro-{id}.lognook"));
    let pipeline = SamplePipeline::new(Arc::new(|_| {}));
    let mut first = trigger_test_sample(10, 1.0);
    first.raw_bytes = Some(vec![0x12, 0x34]);
    let second = trigger_test_sample(20, 2.0);
    pipeline.sender().send_sample(first.clone()).unwrap();
    pipeline.sender().send_sample(second.clone()).unwrap();

    let started = pipeline.start_recording_from_connect(path.clone()).unwrap();
    assert_eq!(started.backfilled_count, 2);
    assert_eq!(started.omitted_count, 0);
    assert!(pipeline.is_recording());
    assert!(pipeline.start_recording_from_connect(path.clone()).is_err());
    pipeline
        .sender()
        .send_sample(trigger_test_sample(30, 3.0))
        .unwrap();
    pipeline.stop_recording().unwrap();

    let recording = read_recording(&path).unwrap();
    assert_eq!(
        recording.samples,
        vec![first, second, trigger_test_sample(30, 3.0)]
    );
    assert_eq!(recording.markers.len(), 1);
    assert_eq!(recording.markers[0].label, "Record From Connect");
    assert!(
        recording.markers[0]
            .note
            .contains("samples evicted before recording: 0")
    );
    let csv = path.with_extension("csv");
    let restored_path = path.with_extension("roundtrip.lognook");
    crate::export_recording_csv(&path, &csv).unwrap();
    crate::import_csv_to_recording(&csv, &restored_path).unwrap();
    let restored = read_recording(&restored_path).unwrap();
    assert_eq!(restored.samples, recording.samples);
    assert_eq!(restored.markers[0].note, recording.markers[0].note);
    drop(pipeline);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(csv).unwrap();
    std::fs::remove_file(restored_path).unwrap();
}

#[test]
fn retained_sample_batch_rolls_back_rows_and_summary_on_error() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lognook-batch-rollback-{id}.lognook"));
    let mut writer = crate::RecordingWriter::create(&path).unwrap();
    let first = trigger_test_sample(10, 1.0);
    writer.append(&first).unwrap();
    let mut rolled_back = trigger_test_sample(20, 2.0);
    rolled_back.signal_id = "temporary_signal".into();
    assert!(
        writer
            .append_batch(&[rolled_back, trigger_test_sample(-1, 3.0)])
            .is_err()
    );
    writer.append(&trigger_test_sample(30, 4.0)).unwrap();
    assert_eq!(writer.finish().unwrap().sample_count, 2);
    assert_eq!(
        read_recording(&path).unwrap().samples,
        vec![first, trigger_test_sample(30, 4.0)]
    );
    assert_eq!(crate::inspect_recording(&path).unwrap().signals.len(), 1);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn retrospective_history_survives_trigger_disarm_and_one_source_disconnect_only() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lognook-retro-session-{id}.lognook"));
    let pipeline = SamplePipeline::new(Arc::new(|_| {}));
    pipeline.sender().source_connected();
    pipeline.sender().source_connected();
    pipeline
        .sender()
        .send_sample(trigger_test_sample(10, 1.0))
        .unwrap();
    pipeline.sender().source_disconnected(true, false).unwrap();
    pipeline
        .sender()
        .send_sample(trigger_test_sample(20, 2.0))
        .unwrap();
    pipeline.disarm_automatic_trigger().unwrap();

    let started = pipeline.start_recording_from_connect(path.clone()).unwrap();
    assert_eq!(started.backfilled_count, 2);
    pipeline.stop_recording().unwrap();
    assert_eq!(read_recording(&path).unwrap().samples.len(), 2);
    pipeline.sender().source_disconnected(false, false).unwrap();

    pipeline.sender().source_connected();
    pipeline
        .sender()
        .send_sample(trigger_test_sample(10, 3.0))
        .unwrap();
    let next_path = path.with_file_name(format!("lognook-retro-next-{id}.lognook"));
    let started = pipeline
        .start_recording_from_connect(next_path.clone())
        .unwrap();
    assert_eq!(started.backfilled_count, 1);
    pipeline.stop_recording().unwrap();
    assert_eq!(
        read_recording(&next_path).unwrap().samples,
        vec![trigger_test_sample(10, 3.0)]
    );
    drop(pipeline);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(next_path).unwrap();
}

#[test]
fn retrospective_recording_reports_evicted_history_and_supports_empty_buffer() {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lognook-retro-overflow-{id}.lognook"));
    let pipeline = SamplePipeline::new(Arc::new(|_| {}));
    let empty = pipeline.start_recording_from_connect(path.clone()).unwrap();
    assert_eq!(empty.backfilled_count, 0);
    pipeline.stop_recording().unwrap();
    std::fs::remove_file(&path).unwrap();

    for elapsed_ns in 0..=100_000 {
        pipeline
            .sender()
            .send_sample(trigger_test_sample(elapsed_ns, elapsed_ns as f64))
            .unwrap();
    }
    let started = pipeline.start_recording_from_connect(path.clone()).unwrap();
    assert_eq!(started.backfilled_count, 100_000);
    assert_eq!(started.omitted_count, 1);
    pipeline.stop_recording().unwrap();
    let recording = read_recording(&path).unwrap();
    assert_eq!(recording.samples.len(), 100_000);
    assert_eq!(recording.samples.first().unwrap().elapsed_ns, 1);
    assert!(
        recording.markers[0]
            .note
            .contains("samples evicted before recording: 1")
    );
    drop(pipeline);
    std::fs::remove_file(path).unwrap();
}
