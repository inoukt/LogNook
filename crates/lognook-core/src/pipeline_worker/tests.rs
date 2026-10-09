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
