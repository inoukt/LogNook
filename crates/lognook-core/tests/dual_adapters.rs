use lognook_core::{
    CanByteOrder, CanDirection, CanFrame, CanSignalDefinition, CanTransport, CanTransportError,
    CanTransportErrorKind, CanTransportEvent, CanWorkbenchConfig, ConnectionCapabilities,
    DeviceDescriptor, EngineEvent, HardwareAdapter, LogNookEngine, LogNookError, Sample,
    SampleOrigin, SampleQuality, SignalDefinition, read_recording, standard_obd_signals,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

enum FakeCanInput {
    Frame(CanFrame),
    Disconnect,
}

#[derive(Clone)]
struct FakeCanControl {
    input: mpsc::Sender<FakeCanInput>,
    started: Arc<Mutex<Option<Instant>>>,
    transmissions: Arc<AtomicUsize>,
}

impl FakeCanControl {
    fn send_rx(&self, value: u8) {
        let started = self.started.lock().unwrap().unwrap();
        self.input
            .send(FakeCanInput::Frame(CanFrame {
                elapsed_ns: started.elapsed().as_nanos().min(i64::MAX as u128) as i64,
                wall_time_unix_ns: now_unix_ns(),
                can_id: 0x123,
                extended: false,
                direction: CanDirection::Rx,
                data: vec![value],
                flags: 0,
            }))
            .unwrap();
    }

    fn disconnect(&self) {
        self.input.send(FakeCanInput::Disconnect).unwrap();
    }
}

struct FakeCanTransport {
    input: mpsc::Receiver<FakeCanInput>,
    started: Arc<Mutex<Option<Instant>>>,
    transmissions: Arc<AtomicUsize>,
}

fn fake_can_transport() -> (FakeCanControl, Box<dyn CanTransport>) {
    let (input, receiver) = mpsc::channel();
    let started = Arc::new(Mutex::new(None));
    let transmissions = Arc::new(AtomicUsize::new(0));
    (
        FakeCanControl {
            input,
            started: started.clone(),
            transmissions: transmissions.clone(),
        },
        Box::new(FakeCanTransport {
            input: receiver,
            started,
            transmissions,
        }),
    )
}

impl CanTransport for FakeCanTransport {
    fn source_id(&self) -> String {
        "can:fixture".into()
    }

    fn start(
        &mut self,
        _bitrate: u32,
        _listen_only: bool,
        session_origin: Instant,
    ) -> Result<(), CanTransportError> {
        *self.started.lock().unwrap() = Some(session_origin);
        Ok(())
    }

    fn receive(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<CanTransportEvent>, CanTransportError> {
        match self.input.recv_timeout(timeout) {
            Ok(FakeCanInput::Frame(frame)) => Ok(Some(CanTransportEvent::Rx(frame))),
            Ok(FakeCanInput::Disconnect) => Err(CanTransportError {
                kind: CanTransportErrorKind::Disconnected,
                message: "fixture removed".into(),
                raw_usb: None,
            }),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn transmit(&mut self, _frame: &CanFrame) -> Result<u32, CanTransportError> {
        self.transmissions.fetch_add(1, Ordering::Relaxed);
        Ok(1)
    }

    fn stop(&mut self) -> Result<(), CanTransportError> {
        Ok(())
    }
}

struct SwitchableAdapter {
    fail: Arc<AtomicBool>,
    started: Instant,
}

impl HardwareAdapter for SwitchableAdapter {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        self.started = Instant::now();
        Ok(ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "fixture-obd".into(),
                hardware_family: "Fixture OBD".into(),
                firmware: None,
                path: None,
            },
            protocol: "fixture OBD".into(),
            read_only: true,
            supported_signals: standard_obd_signals()
                .into_iter()
                .filter(|signal| signal.id == "engine_rpm")
                .collect(),
        })
    }

    fn sample_interval(&self) -> Duration {
        Duration::from_millis(10)
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        if self.fail.load(Ordering::Acquire) {
            return Err(LogNookError::new("fixture OBD disconnected"));
        }
        Ok(signals
            .iter()
            .map(|signal| Sample {
                elapsed_ns: self.started.elapsed().as_nanos().min(i64::MAX as u128) as i64,
                wall_time_unix_ns: now_unix_ns(),
                source_id: "fixture-obd".into(),
                ecu_id: Some("engine".into()),
                signal_id: signal.id.clone(),
                value: 900.0,
                unit: signal.unit.clone(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            })
            .collect())
    }

    fn disconnect(&mut self) {}
}

fn can_config() -> CanWorkbenchConfig {
    CanWorkbenchConfig {
        bitrate: 500_000,
        listen_only: true,
    }
}

fn can_field() -> CanSignalDefinition {
    CanSignalDefinition {
        id: "can:fixture-value".into(),
        name: "Fixture value".into(),
        can_id: 0x123,
        extended: false,
        start_bit: 0,
        bit_length: 8,
        byte_order: CanByteOrder::Intel,
        signed: false,
        factor: 1.0,
        offset: 0.0,
        unit: "raw".into(),
    }
}

fn now_unix_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}

fn receive_sample_after(
    events: &mpsc::Receiver<EngineEvent>,
    signal_id: &str,
    wall_time_unix_ns: i64,
) -> Sample {
    loop {
        match events.recv_timeout(Duration::from_secs(2)).unwrap() {
            EngineEvent::Sample(sample)
                if sample.signal_id == signal_id
                    && sample.wall_time_unix_ns >= wall_time_unix_ns =>
            {
                return sample;
            }
            _ => {}
        }
    }
}

fn temp_recording_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lognook-{label}-{}-{}.sqlite",
        std::process::id(),
        now_unix_ns()
    ))
}

#[test]
fn obd_and_can_share_a_recording_timeline_in_either_connection_order() {
    for can_first in [true, false] {
        let (events, received) = mpsc::channel();
        let mut engine = LogNookEngine::new(move |event| {
            let _ = events.send(event);
        });
        let (control, transport) = fake_can_transport();
        let (can_before, obd_before) = if can_first {
            engine
                .connect_can_transport(can_config(), transport)
                .unwrap();
            engine.configure_can_signals(vec![can_field()]).unwrap();
            control.send_rx(10);
            let can = receive_sample_after(&received, "can:fixture-value", 0);
            thread::sleep(Duration::from_millis(75));
            engine.connect_simulated(&["engine_rpm".into()]).unwrap();
            let obd = receive_sample_after(&received, "engine_rpm", 0);
            (can, obd)
        } else {
            engine.connect_simulated(&["engine_rpm".into()]).unwrap();
            let obd = receive_sample_after(&received, "engine_rpm", 0);
            thread::sleep(Duration::from_millis(75));
            engine
                .connect_can_transport(can_config(), transport)
                .unwrap();
            engine.configure_can_signals(vec![can_field()]).unwrap();
            control.send_rx(10);
            let can = receive_sample_after(&received, "can:fixture-value", 0);
            (can, obd)
        };

        assert!(engine.is_obd_connected());
        assert!(engine.is_can_connected());
        assert!(engine.connection_capabilities().is_some());
        assert_eq!(control.transmissions.load(Ordering::Relaxed), 0);
        if can_first {
            assert!(obd_before.elapsed_ns - can_before.elapsed_ns >= 50_000_000);
        } else {
            assert!(can_before.elapsed_ns - obd_before.elapsed_ns >= 50_000_000);
        }

        let path = temp_recording_path("dual-source");
        let recording_start_wall = now_unix_ns();
        engine.start_recording(&path).unwrap();
        thread::sleep(Duration::from_millis(30));
        control.send_rx(42);
        let can_recorded =
            receive_sample_after(&received, "can:fixture-value", recording_start_wall);
        let _ = receive_sample_after(&received, "engine_rpm", recording_start_wall);
        assert_eq!(can_recorded.source_id, "can:fixture");
        assert_eq!(can_recorded.ecu_id.as_deref(), Some("123"));

        engine.disconnect().unwrap();
        let recording = read_recording(&path).unwrap();
        assert!(recording.samples.iter().any(|sample| {
            sample.source_id == "can:fixture" && sample.signal_id == "can:fixture-value"
        }));
        assert!(recording.samples.iter().any(|sample| {
            sample.source_id == "simulated-obd" && sample.signal_id == "engine_rpm"
        }));
        assert_eq!(control.transmissions.load(Ordering::Relaxed), 0);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn disconnecting_one_source_keeps_the_other_recording() {
    for disconnect_obd in [true, false] {
        let (events, received) = mpsc::channel();
        let mut engine = LogNookEngine::new(move |event| {
            let _ = events.send(event);
        });
        engine.connect_simulated(&["engine_rpm".into()]).unwrap();
        let (control, transport) = fake_can_transport();
        engine
            .connect_can_transport(can_config(), transport)
            .unwrap();
        engine.configure_can_signals(vec![can_field()]).unwrap();
        let path = temp_recording_path("source-disconnect");
        let start_wall = now_unix_ns();
        engine.start_recording(&path).unwrap();
        thread::sleep(Duration::from_millis(20));
        control.send_rx(1);
        let _ = receive_sample_after(&received, "can:fixture-value", start_wall);
        let _ = receive_sample_after(&received, "engine_rpm", start_wall);

        let after_disconnect_wall = now_unix_ns();
        if disconnect_obd {
            engine.disconnect_obd().unwrap();
            assert!(!engine.is_obd_connected());
            assert!(engine.is_can_connected());
            control.send_rx(2);
            let sample =
                receive_sample_after(&received, "can:fixture-value", after_disconnect_wall);
            assert_eq!(sample.value, 2.0);
        } else {
            engine.disconnect_can().unwrap();
            assert!(engine.is_obd_connected());
            assert!(!engine.is_can_connected());
            let _ = receive_sample_after(&received, "engine_rpm", after_disconnect_wall);
        }
        assert!(engine.is_recording());

        engine.disconnect().unwrap();
        assert!(!engine.is_recording());
        let recording = read_recording(&path).unwrap();
        assert!(
            recording
                .samples
                .iter()
                .any(|sample| sample.signal_id == "engine_rpm")
        );
        assert!(
            recording
                .samples
                .iter()
                .any(|sample| sample.signal_id == "can:fixture-value")
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn terminal_source_failures_leave_the_peer_and_recording_running() {
    let (events, received) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events.send(event);
    });
    let (control, transport) = fake_can_transport();
    engine
        .connect_can_transport(can_config(), transport)
        .unwrap();
    engine.configure_can_signals(vec![can_field()]).unwrap();
    let failed = Arc::new(AtomicBool::new(false));
    engine
        .connect_adapter(
            Box::new(SwitchableAdapter {
                fail: failed.clone(),
                started: Instant::now(),
            }),
            &["engine_rpm".into()],
        )
        .unwrap();
    let path = temp_recording_path("obd-terminal-failure");
    engine.start_recording(&path).unwrap();
    let start_wall = now_unix_ns();
    control.send_rx(1);
    let _ = receive_sample_after(&received, "can:fixture-value", start_wall);
    let _ = receive_sample_after(&received, "engine_rpm", start_wall);
    failed.store(true, Ordering::Release);
    loop {
        if received.recv_timeout(Duration::from_secs(2)).unwrap() == EngineEvent::Disconnected {
            break;
        }
    }

    assert!(!engine.is_obd_connected());
    assert!(engine.is_can_connected());
    assert!(engine.is_recording());
    let after_failure_wall = now_unix_ns();
    control.send_rx(2);
    let sample = receive_sample_after(&received, "can:fixture-value", after_failure_wall);
    assert_eq!(sample.value, 2.0);
    assert!(engine.is_recording());
    engine.disconnect().unwrap();
    let recording = read_recording(&path).unwrap();
    assert!(
        recording
            .samples
            .iter()
            .any(|sample| sample.signal_id == "can:fixture-value")
    );
    assert!(
        recording
            .samples
            .iter()
            .any(|sample| sample.signal_id == "engine_rpm")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn terminal_can_failure_keeps_obd_acquisition_and_recording_running() {
    let (events, received) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events.send(event);
    });
    engine.connect_simulated(&["engine_rpm".into()]).unwrap();
    let (control, transport) = fake_can_transport();
    engine
        .connect_can_transport(can_config(), transport)
        .unwrap();
    engine.configure_can_signals(vec![can_field()]).unwrap();
    let path = temp_recording_path("can-terminal-failure");
    let start_wall = now_unix_ns();
    engine.start_recording(&path).unwrap();
    thread::sleep(Duration::from_millis(20));
    control.send_rx(1);
    let _ = receive_sample_after(&received, "can:fixture-value", start_wall);
    let _ = receive_sample_after(&received, "engine_rpm", start_wall);
    let failure_wall = now_unix_ns();
    control.disconnect();
    loop {
        if matches!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            EngineEvent::CanWorkbenchStatus(status) if !status.connected
        ) {
            break;
        }
    }
    let _ = receive_sample_after(&received, "engine_rpm", failure_wall);

    assert!(engine.is_obd_connected());
    assert!(!engine.is_can_connected());
    assert!(engine.is_recording());
    engine.disconnect().unwrap();
    let recording = read_recording(&path).unwrap();
    assert!(
        recording
            .samples
            .iter()
            .any(|sample| sample.signal_id == "can:fixture-value")
    );
    assert!(
        recording
            .samples
            .iter()
            .any(|sample| sample.signal_id == "engine_rpm")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn terminal_failure_of_last_source_finalizes_recording() {
    let (events, received) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events.send(event);
    });
    let failed = Arc::new(AtomicBool::new(false));
    engine
        .connect_adapter(
            Box::new(SwitchableAdapter {
                fail: failed.clone(),
                started: Instant::now(),
            }),
            &["engine_rpm".into()],
        )
        .unwrap();
    let path = temp_recording_path("last-source-failure");
    let start_wall = now_unix_ns();
    engine.start_recording(&path).unwrap();
    let _ = receive_sample_after(&received, "engine_rpm", start_wall);
    failed.store(true, Ordering::Release);
    let mut recording_stopped = false;
    let mut disconnected = false;
    while !recording_stopped || !disconnected {
        match received.recv_timeout(Duration::from_secs(2)).unwrap() {
            EngineEvent::RecordingFailed(summary) => {
                assert_eq!(summary.path, path);
                recording_stopped = true;
            }
            EngineEvent::Disconnected => disconnected = true,
            _ => {}
        }
    }
    assert!(!engine.is_obd_connected());
    assert!(!engine.is_recording());
    assert!(
        read_recording(&path)
            .unwrap()
            .samples
            .iter()
            .any(|sample| sample.signal_id == "engine_rpm")
    );
    engine.disconnect().unwrap();
    std::fs::remove_file(path).unwrap();
}
