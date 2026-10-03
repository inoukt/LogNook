use lognook_core::{
    EngineEvent, LogNookEngine, Marker, RecordingWriter, Sample, SampleOrigin, SampleQuality,
    inspect_recording,
};
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn fixture() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lognook-playback-{}.lognook",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut writer = RecordingWriter::create(&path).unwrap();
    for elapsed_ns in [0, 500_000_000, 2_000_000_000] {
        writer
            .append(&Sample {
                elapsed_ns,
                wall_time_unix_ns: 1_800_000_000_000_000_000 + elapsed_ns,
                source_id: "fixture".into(),
                ecu_id: Some("engine".into()),
                signal_id: "engine_rpm".into(),
                value: 800.0 + elapsed_ns as f64 / 1_000_000.0,
                unit: "rpm".into(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            })
            .unwrap();
    }
    writer.finish().unwrap();
    path
}

fn engine_with(path: &Path) -> (LogNookEngine, mpsc::Receiver<EngineEvent>) {
    let (tx, rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = tx.send(event);
    });
    engine.load_recording(path).unwrap();
    (engine, rx)
}

fn next_replay_sample(rx: &mpsc::Receiver<EngineEvent>, timeout: Duration) -> Sample {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(EngineEvent::Sample(sample)) = rx.recv_timeout(Duration::from_millis(20))
            && sample.origin == SampleOrigin::Replay
        {
            return sample;
        }
    }
    panic!("timed out waiting for replay sample");
}

#[test]
fn pause_returns_before_pending_delay() {
    let path = fixture();
    let (mut engine, rx) = engine_with(&path);
    engine.start_replay(1.0).unwrap();
    next_replay_sample(&rx, Duration::from_secs(1));
    let started = Instant::now();
    engine.pause_replay().unwrap();
    assert!(started.elapsed() < Duration::from_millis(250));
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn seek_restarts_at_or_after_target() {
    let path = fixture();
    let (mut engine, rx) = engine_with(&path);
    engine.seek_replay(400_000_000).unwrap();
    engine.start_replay(32.0).unwrap();
    assert_eq!(
        next_replay_sample(&rx, Duration::from_secs(1)).elapsed_ns,
        500_000_000
    );
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn speed_change_updates_pending_delay() {
    let path = fixture();
    let (mut engine, rx) = engine_with(&path);
    engine.start_replay(1.0).unwrap();
    next_replay_sample(&rx, Duration::from_secs(1));
    engine.set_replay_speed(8.0).unwrap();
    let started = Instant::now();
    assert_eq!(
        next_replay_sample(&rx, Duration::from_secs(1)).elapsed_ns,
        500_000_000
    );
    assert!(started.elapsed() < Duration::from_millis(900));
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_replay_speed_does_not_change_state() {
    let path = fixture();
    let (mut engine, _) = engine_with(&path);
    assert!(engine.set_replay_speed(0.0).is_err());
    assert_eq!(engine.playback_state().unwrap().speed, 1.0);
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn playback_marker_persists_at_playhead() {
    let path = fixture();
    let (mut engine, _) = engine_with(&path);
    engine.seek_replay(350_000_000).unwrap();
    let marker = engine
        .add_playback_marker("shift", "full throttle")
        .unwrap();
    assert_eq!(marker.elapsed_ns, 350_000_000);
    assert!(marker.id.is_some());
    assert_eq!(
        inspect_recording(&path).unwrap().markers,
        vec![Marker {
            id: marker.id,
            elapsed_ns: 350_000_000,
            wall_time_unix_ns: marker.wall_time_unix_ns,
            label: "shift".into(),
            note: "full throttle".into()
        }]
    );
    drop(engine);
    std::fs::remove_file(path).unwrap();
}
