use lognook_core::{
    EngineEvent, LogNookEngine, Marker, PlaybackState, RecordingWriter, Sample, SampleOrigin,
    SampleQuality, add_recording_marker, inspect_recording,
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
        if let Ok(EngineEvent::PlaybackSample { sample, .. }) =
            rx.recv_timeout(Duration::from_millis(20))
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
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut state_seen = false;
    while Instant::now() < deadline {
        if let Ok(EngineEvent::PlaybackState(state)) = rx.recv_timeout(Duration::from_millis(20))
            && !state.playing
        {
            state_seen = true;
            break;
        }
    }
    assert!(state_seen, "pause did not emit its playback state");
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn stop_does_not_report_natural_completion() {
    let path = fixture();
    let (mut engine, rx) = engine_with(&path);
    engine.start_replay(1.0).unwrap();
    next_replay_sample(&rx, Duration::from_secs(1));
    engine.stop_replay();
    let deadline = Instant::now() + Duration::from_millis(150);
    let mut finished = false;
    while Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(20))
            && matches!(event, EngineEvent::PlaybackFinished)
        {
            finished = true;
            break;
        }
    }
    assert!(!finished, "explicit stop reported natural completion");
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
fn replay_events_carry_the_loaded_session_identity() {
    let first_path = fixture();
    let second_path = fixture();
    let (mut engine, rx) = engine_with(&first_path);
    let first_session = engine.playback_state().unwrap().session_id;
    engine.start_replay(32.0).unwrap();
    let first_sample = loop {
        if let Ok(EngineEvent::PlaybackSample { session_id, sample }) =
            rx.recv_timeout(Duration::from_secs(1))
        {
            break (session_id, sample);
        }
    };
    assert_eq!(first_sample.0, first_session);

    engine.load_recording(&second_path).unwrap();
    let second_session = engine.playback_state().unwrap().session_id;
    assert_ne!(first_session, second_session);
    engine.start_replay(32.0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut found_current = false;
    while Instant::now() < deadline {
        if let Ok(EngineEvent::PlaybackSample { session_id, .. }) =
            rx.recv_timeout(Duration::from_millis(20))
            && session_id == second_session
        {
            found_current = true;
            break;
        }
    }
    assert!(found_current);
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(first_path).unwrap();
    std::fs::remove_file(second_path).unwrap();
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

#[test]
fn deleting_late_marker_updates_playback_duration() {
    let path = fixture();
    let marker = add_recording_marker(
        &path,
        &Marker {
            id: None,
            elapsed_ns: 4_000_000_000,
            wall_time_unix_ns: 1,
            label: "late".into(),
            note: String::new(),
        },
    )
    .unwrap();
    let (mut engine, _) = engine_with(&path);
    assert_eq!(engine.playback_state().unwrap().duration_ns, 4_000_000_000);
    engine.seek_replay(4_000_000_000).unwrap();
    engine.delete_playback_marker(marker.id.unwrap()).unwrap();
    let PlaybackState {
        duration_ns,
        position_ns,
        ..
    } = engine.playback_state().unwrap();
    assert_eq!(duration_ns, 2_000_000_000);
    assert!(position_ns <= duration_ns);
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn deleting_late_marker_during_playback_clamps_without_restarting() {
    let path = fixture();
    let marker = add_recording_marker(
        &path,
        &Marker {
            id: None,
            elapsed_ns: 4_000_000_000,
            wall_time_unix_ns: 1,
            label: "late".into(),
            note: String::new(),
        },
    )
    .unwrap();
    let (mut engine, _) = engine_with(&path);
    engine.start_replay(0.1).unwrap();
    engine.seek_replay(3_500_000_000).unwrap();
    engine.delete_playback_marker(marker.id.unwrap()).unwrap();
    let state = engine.playback_state().unwrap();
    assert_eq!(state.position_ns, 2_000_000_000);
    assert_eq!(state.duration_ns, 2_000_000_000);
    assert!(state.playing);
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(path).unwrap();
}
