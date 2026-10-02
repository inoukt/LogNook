use lognook_core::{EngineEvent, LogNookEngine, SampleOrigin, read_recording};
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn temp_recording() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-pipeline-{id}.lognook"))
}

#[test]
fn live_recording_and_replay_share_the_sample_event_pipeline() {
    let (event_tx, event_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = event_tx.send(event);
    });
    let path = temp_recording();
    engine
        .connect_simulated(&["engine_rpm".into(), "coolant_temp".into()])
        .unwrap();
    engine.start_recording(&path).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut live_count = 0;
    while live_count < 3 && Instant::now() < deadline {
        if let Ok(EngineEvent::Sample(sample)) = event_rx.recv_timeout(Duration::from_millis(250))
            && sample.origin == SampleOrigin::Live
        {
            live_count += 1;
        }
    }
    assert!(
        live_count >= 3,
        "simulator did not deliver live samples through the pipeline"
    );
    engine
        .insert_marker("test marker", "captured during acquisition")
        .unwrap();
    let summary = engine.stop_recording().unwrap();
    assert!(summary.sample_count >= 2);

    let recorded = read_recording(&path).unwrap();
    assert!(!recorded.samples.is_empty());
    assert_eq!(recorded.markers[0].label, "test marker");
    engine.replay_recording(&path, 32.0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut replay_count = 0;
    while Instant::now() < deadline {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(EngineEvent::Sample(sample)) if sample.origin == SampleOrigin::Replay => {
                replay_count += 1
            }
            Ok(EngineEvent::PlaybackFinished) => break,
            _ => {}
        }
    }
    assert_eq!(replay_count, recorded.samples.len());
    engine.disconnect().unwrap();
    std::fs::remove_file(path).unwrap();
}
