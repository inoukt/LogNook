use lognook_core::{
    EngineEvent, LogNookEngine, Marker, PlaybackState, RecordingWriter, Sample, SampleOrigin,
    SampleQuality, add_recording_marker, inspect_recording,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn temp_session(prefix: &str) -> std::path::PathBuf {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{time}-{counter}.lognook",
        std::process::id()
    ))
}

fn fixture() -> std::path::PathBuf {
    let path = temp_session("lognook-playback");
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

fn next_replay_sample_for(
    rx: &mpsc::Receiver<EngineEvent>,
    timeout: Duration,
    expected_session_id: u64,
) -> Sample {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(EngineEvent::PlaybackSample { session_id, sample }) =
            rx.recv_timeout(Duration::from_millis(20))
            && session_id == expected_session_id
        {
            return sample;
        }
    }
    panic!("timed out waiting for replay sample from session {expected_session_id}");
}

#[test]
fn close_session_unloads_replay_without_deleting_recording() {
    let path = fixture();
    let (mut engine, rx) = engine_with(&path);
    let old_session_id = engine.playback_state().unwrap().session_id;
    engine.start_replay(1.0).unwrap();
    next_replay_sample(&rx, Duration::from_secs(1));

    engine.close_session();

    assert!(engine.active_session_path().is_err());
    let state = engine.playback_state().unwrap();
    assert_ne!(state.session_id, old_session_id);
    assert_eq!(state.position_ns, 0);
    assert_eq!(state.duration_ns, 0);
    assert!(!state.playing);
    assert!(engine.start_replay(1.0).is_err());
    assert!(
        path.exists(),
        "closing a session must not delete its recording"
    );

    engine.load_recording(&path).unwrap();
    assert_eq!(engine.active_session_path().unwrap(), path);
    let new_session_id = engine.playback_state().unwrap().session_id;
    engine.start_replay(1.0).unwrap();
    next_replay_sample_for(&rx, Duration::from_secs(1), new_session_id);
    engine.stop_replay();
    assert!(path.exists());
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn seek_restarts_at_or_after_target() {
    let path = fixture();
    let (mut engine, rx) = engine_with(&path);
    engine.seek_replay(400_000_000).unwrap();
    engine.start_replay(32.0).unwrap();
    assert!(next_replay_sample(&rx, Duration::from_secs(1)).elapsed_ns >= 500_000_000);
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
    assert!(next_replay_sample(&rx, Duration::from_secs(1)).elapsed_ns >= 500_000_000);
    assert!(started.elapsed() < Duration::from_millis(900));
    engine.stop_replay();
    drop(engine);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn fast_replay_coalesces_readouts_but_preserves_latest_values_and_markers() {
    let path = temp_session("lognook-fast-playback");
    let mut writer = RecordingWriter::create(&path).unwrap();
    for index in 0..2_000_i64 {
        for (signal_id, value) in [
            ("engine_rpm", index as f64),
            ("coolant_temp", (index * 2) as f64),
        ] {
            writer
                .append(&Sample {
                    elapsed_ns: index * 1_000_000,
                    wall_time_unix_ns: 1_800_000_000_000_000_000 + index * 1_000_000,
                    source_id: "fixture".into(),
                    ecu_id: Some("engine".into()),
                    signal_id: signal_id.into(),
                    value,
                    unit: "unit".into(),
                    quality: SampleQuality::Valid,
                    origin: SampleOrigin::Live,
                    raw_bytes: None,
                })
                .unwrap();
        }
    }
    writer
        .add_marker(&Marker {
            id: None,
            elapsed_ns: 1_000_000_000,
            wall_time_unix_ns: 1_800_000_001_000_000_000,
            label: String::new(),
            note: String::new(),
        })
        .unwrap();
    writer.finish().unwrap();

    let (mut engine, rx) = engine_with(&path);
    engine.start_replay(32.0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut samples = 0;
    let mut latest = std::collections::HashMap::new();
    let mut found_marker = false;
    let mut progress_updates = 0;
    let mut finished = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(EngineEvent::PlaybackSample { sample, .. }) => {
                samples += 1;
                latest.insert(sample.signal_id.clone(), sample.value);
            }
            Ok(EngineEvent::PlaybackMarker { marker, .. }) => {
                found_marker |= marker.elapsed_ns == 1_000_000_000
                    && marker.label.is_empty()
                    && marker.note.is_empty();
            }
            Ok(EngineEvent::PlaybackState(state)) if state.playing && state.position_ns > 0 => {
                progress_updates += 1;
            }
            Ok(EngineEvent::PlaybackFinished) => {
                finished = true;
                break;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    assert!(finished, "32× replay did not finish promptly");
    assert!(samples < 100, "replay sent {samples} individual readouts");
    assert_eq!(latest.get("engine_rpm"), Some(&1_999.0));
    assert_eq!(latest.get("coolant_temp"), Some(&3_998.0));
    assert!(
        progress_updates > 0,
        "replay did not report live position progress"
    );
    assert!(found_marker, "replay dropped the marker event");
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
