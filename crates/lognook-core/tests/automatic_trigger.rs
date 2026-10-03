use lognook_core::{
    AutomaticTriggerConfig, ConnectionCapabilities, DeviceDescriptor, EngineEvent, HardwareAdapter,
    LogNookEngine, LogNookError, Sample, SignalDefinition, TriggerDirection, read_recording,
};
use std::{
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn temp_recording() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-auto-trigger-{id}.lognook"))
}

#[test]
fn automatic_trigger_records_original_pretrigger_samples_and_a_marker() {
    let (event_tx, event_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = event_tx.send(event);
    });
    let path = temp_recording();
    engine.connect_simulated(&["engine_rpm".into()]).unwrap();

    // Let the shared live pipeline collect a rolling window before arming.
    thread::sleep(Duration::from_millis(350));
    let mut live_samples = event_rx
        .try_iter()
        .filter_map(|event| match event {
            EngineEvent::Sample(sample) if sample.origin == lognook_core::SampleOrigin::Live => {
                Some(sample)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let config = AutomaticTriggerConfig {
        signal_id: "engine_rpm".into(),
        direction: TriggerDirection::Above,
        threshold: -1_000_000_000.0,
        hysteresis: 0.0,
        hold_ms: 0,
        pre_trigger_ms: 2_000,
    };
    let armed = engine.arm_automatic_trigger(&config, &path).unwrap();
    assert!(armed.armed);

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut trigger_status_seen = false;
    while Instant::now() < deadline {
        match event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(EngineEvent::Sample(sample)) if sample.origin == lognook_core::SampleOrigin::Live => {
                live_samples.push(sample);
            }
            Ok(EngineEvent::TriggerStatus(status)) if status.recording => {
                trigger_status_seen = true;
                break;
            }
            _ => {}
        }
    }
    assert!(trigger_status_seen, "automatic trigger did not start recording");

    let summary = engine.stop_recording().unwrap();
    assert_eq!(summary.path, path);
    let recording = read_recording(&path).unwrap();
    live_samples.extend(event_rx.try_iter().filter_map(|event| match event {
        EngineEvent::Sample(sample) if sample.origin == lognook_core::SampleOrigin::Live => {
            Some(sample)
        }
        _ => None,
    }));
    let marker = recording
        .markers
        .iter()
        .find(|marker| marker.label == "Automatic trigger")
        .expect("trigger marker should be recorded");
    assert!(
        recording.samples.iter().any(|sample| sample.elapsed_ns < marker.elapsed_ns),
        "recording should include buffered samples before the trigger timestamp"
    );
    assert!(recording.samples.iter().any(|sample| sample.elapsed_ns == marker.elapsed_ns));
    let cutoff_ns = marker.elapsed_ns - 2_000_000_000;
    let expected = live_samples
        .iter()
        .filter(|sample| sample.elapsed_ns >= cutoff_ns && sample.elapsed_ns <= marker.elapsed_ns)
        .map(|sample| (sample.elapsed_ns, sample.wall_time_unix_ns, sample.value))
        .collect::<Vec<_>>();
    let actual = recording
        .samples
        .iter()
        .filter(|sample| sample.elapsed_ns >= cutoff_ns && sample.elapsed_ns <= marker.elapsed_ns)
        .map(|sample| (sample.elapsed_ns, sample.wall_time_unix_ns, sample.value))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "buffered samples must retain their live timestamps and values");
    assert!(!engine.automatic_trigger_status().recording);

    engine.disconnect().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn automatic_trigger_requires_an_acquired_live_signal() {
    let mut engine = LogNookEngine::new(|_| {});
    engine.connect_simulated(&["engine_rpm".into()]).unwrap();
    let config = AutomaticTriggerConfig {
        signal_id: "coolant_temp".into(),
        direction: TriggerDirection::Below,
        threshold: 0.0,
        hysteresis: 0.0,
        hold_ms: 0,
        pre_trigger_ms: 0,
    };
    let error = engine
        .arm_automatic_trigger(&config, &temp_recording())
        .unwrap_err()
        .to_string();
    assert!(error.contains("acquired"), "unexpected validation error: {error}");
    engine.disconnect().unwrap();
}

struct ReadFailureAdapter;

impl HardwareAdapter for ReadFailureAdapter {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        Ok(ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "test-read-failure".into(),
                hardware_family: "test".into(),
                firmware: None,
                path: None,
            },
            protocol: "test".into(),
            read_only: true,
            supported_signals: lognook_core::standard_obd_signals(),
        })
    }

    fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        Err(LogNookError::new("test read failure"))
    }
}

#[test]
fn failed_adapter_cannot_be_replaced_while_its_partial_recording_is_open() {
    let (event_tx, _event_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = event_tx.send(event);
    });
    let path = temp_recording();
    engine
        .connect_adapter(Box::new(ReadFailureAdapter), &["engine_rpm".into()])
        .unwrap();
    engine.start_recording(&path).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while engine.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!engine.is_connected(), "test adapter should have disconnected");

    let error = engine.connect_simulated(&["engine_rpm".into()]).unwrap_err().to_string();
    assert!(error.contains("recording"), "unexpected error: {error}");
    engine.stop_recording().unwrap();
    engine.disconnect().unwrap();
    std::fs::remove_file(path).unwrap();
}
