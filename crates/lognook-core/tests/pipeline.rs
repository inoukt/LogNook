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
    let mut replay_latest = std::collections::HashMap::new();
    let mut replay_marker = false;
    let mut replay_finished = false;
    while Instant::now() < deadline {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(EngineEvent::PlaybackSample { sample, .. }) => {
                replay_latest.insert(sample.signal_id.clone(), sample);
            }
            Ok(EngineEvent::PlaybackMarker { marker, .. }) => {
                replay_marker |= marker.label == "test marker";
            }
            Ok(EngineEvent::PlaybackFinished) => {
                replay_finished = true;
                break;
            }
            _ => {}
        }
    }
    assert!(replay_finished, "replay did not finish");
    assert_eq!(replay_latest.len(), 2);
    for signal_id in ["engine_rpm", "coolant_temp"] {
        let recorded_latest = recorded
            .samples
            .iter()
            .filter(|sample| sample.signal_id == signal_id)
            .max_by_key(|sample| sample.elapsed_ns)
            .unwrap();
        let replayed = replay_latest.get(signal_id).unwrap();
        assert_eq!(replayed.elapsed_ns, recorded_latest.elapsed_ns);
        assert_eq!(replayed.value, recorded_latest.value);
    }
    assert!(replay_marker, "replay did not preserve marker events");
    engine.disconnect().unwrap();
    std::fs::remove_file(path).unwrap();
}
