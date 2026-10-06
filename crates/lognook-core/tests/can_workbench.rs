use lognook_core::{
    CanByteOrder, CanDirection, CanFrame, CanScanConfig, CanScanMode, CanSignalDefinition,
    CanTransport, CanWorkbenchConfig, EngineEvent, LogNookEngine, SampleQuality, read_recording,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread::{self, ThreadId},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct FakeTransport {
    incoming: mpsc::Receiver<CanFrame>,
    transmitted: mpsc::Sender<CanFrame>,
    owners: Arc<Mutex<Vec<ThreadId>>>,
}

impl FakeTransport {
    fn note_owner(&self) {
        self.owners.lock().unwrap().push(thread::current().id());
    }
}

impl CanTransport for FakeTransport {
    fn start(&mut self, _bitrate: u32, _listen_only: bool) -> Result<(), String> {
        self.note_owner();
        Ok(())
    }

    fn receive(&mut self, timeout: Duration) -> Result<Option<CanFrame>, String> {
        self.note_owner();
        match self.incoming.recv_timeout(timeout) {
            Ok(frame) => Ok(Some(frame)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("fake removed".into()),
        }
    }

    fn transmit(&mut self, frame: &CanFrame) -> Result<(), String> {
        self.note_owner();
        self.transmitted
            .send(frame.clone())
            .map_err(|error| error.to_string())
    }

    fn stop(&mut self) -> Result<(), String> {
        self.note_owner();
        Ok(())
    }
}

fn transport() -> (
    Box<dyn CanTransport>,
    mpsc::Sender<CanFrame>,
    mpsc::Receiver<CanFrame>,
    Arc<Mutex<Vec<ThreadId>>>,
) {
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let (transmitted_tx, transmitted_rx) = mpsc::channel();
    let owners = Arc::new(Mutex::new(Vec::new()));
    (
        Box::new(FakeTransport {
            incoming: incoming_rx,
            transmitted: transmitted_tx,
            owners: owners.clone(),
        }),
        incoming_tx,
        transmitted_rx,
        owners,
    )
}

fn config(listen_only: bool) -> CanWorkbenchConfig {
    CanWorkbenchConfig {
        bitrate: 500_000,
        listen_only,
    }
}

fn frame(can_id: u32, direction: CanDirection, data: &[u8], elapsed_ns: i64) -> CanFrame {
    CanFrame {
        elapsed_ns,
        wall_time_unix_ns: 1_700_000_000_000_000_000 + elapsed_ns,
        can_id,
        extended: false,
        direction,
        data: data.to_vec(),
        flags: 0,
    }
}

fn temp_path(ext: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-can-{stamp}.{ext}"))
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(predicate(), "timed out waiting for CAN workbench activity");
}

#[test]
fn a_single_worker_owns_io_and_decoded_rx_samples_enter_the_recording_pipeline() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, _transmitted, owners) = transport();
    let status = engine
        .connect_can_transport(config(true), transport)
        .unwrap();
    assert!(status.connected);
    assert!(status.listen_only);
    assert_eq!(status.bitrate, 500_000);
    assert!(engine.connect_simulated(&["engine_rpm".into()]).is_err());
    engine
        .configure_can_signals(vec![CanSignalDefinition {
            id: "engine_rpm_can".into(),
            name: "Engine RPM CAN field".into(),
            can_id: 0x123,
            extended: false,
            start_bit: 0,
            bit_length: 16,
            byte_order: CanByteOrder::Intel,
            signed: false,
            factor: 0.25,
            offset: 0.0,
            unit: "rpm".into(),
        }])
        .unwrap();

    let capture_path = temp_path("candump.csv");
    let recording_path = temp_path("lognook");
    engine.start_can_capture(&capture_path).unwrap();
    engine.start_recording(&recording_path).unwrap();
    let received = frame(0x123, CanDirection::Rx, &[0xD0, 0x07], 9_000_000);
    incoming.send(received.clone()).unwrap();

    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::Sample(sample) if sample.signal_id == "engine_rpm_can")
        })
    });
    engine.stop_can_capture().unwrap();
    engine.stop_recording().unwrap();
    let recording = read_recording(&recording_path).unwrap();
    let sample = recording
        .samples
        .iter()
        .find(|sample| sample.signal_id == "engine_rpm_can")
        .expect("decoded CAN field should be recorded");
    assert_eq!(sample.elapsed_ns, received.elapsed_ns);
    assert_eq!(sample.wall_time_unix_ns, received.wall_time_unix_ns);
    assert_eq!(sample.value, 500.0);
    assert_eq!(sample.raw_bytes.as_deref(), Some(&[0xD0, 0x07][..]));
    assert_eq!(sample.quality, SampleQuality::Suspect);

    let capture = std::fs::read_to_string(&capture_path).unwrap();
    assert_eq!(
        capture.lines().count(),
        2,
        "header plus every captured RX frame"
    );
    assert!(capture.contains("rx,123,standard,2,D007,00000000"));
    let owner_threads = owners.lock().unwrap();
    assert!(!owner_threads.is_empty());
    assert!(
        owner_threads
            .iter()
            .all(|thread| thread == &owner_threads[0])
    );
    drop(owner_threads);

    engine.disconnect().unwrap();
    assert!(events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::CanWorkbenchStatus(status) if !status.connected)
    }));
    let _ = std::fs::remove_file(capture_path);
    let _ = std::fs::remove_file(recording_path);
}

#[test]
fn scan_uses_the_configured_id_range_and_stop_cancels_before_the_next_send() {
    let mut engine = LogNookEngine::new(|_| {});
    let (transport, _incoming, transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();

    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::CanIdRange {
                start_id: 0x120,
                end_id: 0x122,
                extended: false,
                payload: vec![0x55],
            },
            pacing_ms: 5,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let sent = (0..3)
        .map(|_| transmitted.recv_timeout(Duration::from_secs(1)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sent.iter().map(|frame| frame.can_id).collect::<Vec<_>>(),
        [0x120, 0x121, 0x122]
    );
    assert!(
        sent.windows(2)
            .all(|pair| pair[1].elapsed_ns - pair[0].elapsed_ns >= 5_000_000)
    );
    assert!(transmitted.recv_timeout(Duration::from_millis(30)).is_err());

    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::CanIdRange {
                start_id: 0x200,
                end_id: 0x2ff,
                extended: false,
                payload: vec![0xA5],
            },
            pacing_ms: 30,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    assert_eq!(
        transmitted
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .can_id,
        0x200
    );
    engine.stop_can_scan().unwrap();
    thread::sleep(Duration::from_millis(50));
    assert!(
        transmitted.try_recv().is_err(),
        "Stop must prevent later scan transmissions"
    );
    engine.disconnect().unwrap();
}

#[test]
fn mode21_scan_keeps_exact_tx_rx_frames_and_typed_positive_result() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, owners) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode21 {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                start_pid: 0x01,
                end_pid: 0x01,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();

    let tx = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(tx.can_id, 0x7E0);
    assert_eq!(tx.data, [2, 0x21, 0x01, 0, 0, 0, 0, 0]);
    let rx = frame(
        0x7E8,
        CanDirection::Rx,
        &[3, 0x61, 0x01, 0xA7],
        tx.elapsed_ns + 1_000_000,
    );
    incoming.send(rx.clone()).unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.outcome == lognook_core::CanDiagnosticOutcome::Positive)
        })
    });
    let result = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanScanResult(result) => Some(result.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.identifier, "21 01");
    assert_eq!(result.request, tx);
    assert_eq!(result.response_frames, [rx]);

    engine.stop_can_capture().unwrap();
    let capture = std::fs::read_to_string(&capture_path).unwrap();
    assert_eq!(capture.lines().count(), 3, "header, exact TX, and exact RX");
    assert!(capture.contains("tx,7E0,standard,8,0221010000000000,00000000"));
    assert!(capture.contains("rx,7E8,standard,4,036101A7,00000000"));
    engine.disconnect().unwrap();
    let owners = owners.lock().unwrap();
    assert!(owners.iter().all(|owner| owner == &owners[0]));
    drop(owners);
    let _ = std::fs::remove_file(capture_path);
}

#[test]
fn selected_mode01_pid_sample_uses_the_response_timestamp_and_recording_pipeline() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let recording_path = temp_path("lognook");
    engine.start_recording(&recording_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode01Pids {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                pids: vec![0x0C],
                repeat: false,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let tx = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(tx.data, [2, 0x01, 0x0C, 0, 0, 0, 0, 0]);
    let rx = frame(
        0x7E8,
        CanDirection::Rx,
        &[4, 0x41, 0x0C, 0x1F, 0x40],
        tx.elapsed_ns + 2_000_000,
    );
    incoming.send(rx.clone()).unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::Sample(sample) if sample.signal_id == "engine_rpm")
        })
    });
    engine.stop_recording().unwrap();
    let recording = read_recording(&recording_path).unwrap();
    let sample = recording
        .samples
        .iter()
        .find(|sample| sample.signal_id == "engine_rpm")
        .unwrap();
    assert_eq!(sample.value, 2000.0);
    assert_eq!(sample.elapsed_ns, rx.elapsed_ns);
    assert_eq!(sample.wall_time_unix_ns, rx.wall_time_unix_ns);
    assert_eq!(
        sample.raw_bytes.as_deref(),
        Some(&[4, 0x41, 0x0C, 0x1F, 0x40][..])
    );
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(recording_path);
}

#[test]
fn mode01_support_scan_reports_only_pids_from_the_explicit_response() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode01Support {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(request.data, [2, 0x01, 0, 0, 0, 0, 0, 0]);
    incoming
        .send(frame(
            0x7E8,
            CanDirection::Rx,
            &[6, 0x41, 0, 0x10, 0, 0, 0],
            request.elapsed_ns + 1_000_000,
        ))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.supported_pids.is_some())
        })
    });
    let result = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanScanResult(result) => result.supported_pids.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(result, [4]);
    assert!(transmitted.recv_timeout(Duration::from_millis(30)).is_err());
    engine.disconnect().unwrap();
}

#[test]
fn multi_frame_mode22_reply_gets_serialized_flow_control_and_keeps_raw_frames() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode22 {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                start_did: 0xF190,
                end_did: 0xF190,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(request.data, [3, 0x22, 0xF1, 0x90, 0, 0, 0, 0]);
    let first = frame(
        0x7E8,
        CanDirection::Rx,
        &[0x10, 0x0A, 0x62, 0xF1, 0x90, 1, 2, 3],
        request.elapsed_ns + 1_000_000,
    );
    incoming.send(first.clone()).unwrap();
    let flow_control = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(flow_control.can_id, 0x7E0);
    assert_eq!(flow_control.data, [0x30, 0, 0, 0, 0, 0, 0, 0]);
    let continuation = frame(
        0x7E8,
        CanDirection::Rx,
        &[0x21, 4, 5, 6, 7, 8, 9, 0],
        first.elapsed_ns + 1_000_000,
    );
    incoming.send(continuation.clone()).unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.outcome == lognook_core::CanDiagnosticOutcome::Positive)
        })
    });
    let result = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanScanResult(result) => Some(result.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.identifier, "22 F190");
    assert_eq!(result.response_frames, [first, continuation]);
    engine.disconnect().unwrap();
}

#[test]
fn a_frame_burst_is_written_in_order_without_dropping_capture_rows() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, _transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(true), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    let burst_started = Instant::now();
    for index in 0..128i64 {
        incoming
            .send(frame(
                0x321,
                CanDirection::Rx,
                &[index as u8],
                1_000_000 + index,
            ))
            .unwrap();
    }
    wait_until(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .map(|event| match event {
                EngineEvent::CanFrames(frames) => frames.len(),
                _ => 0,
            })
            .sum::<usize>()
            >= 128
    });
    assert!(
        burst_started.elapsed() < Duration::from_millis(500),
        "ready frames should not incur a per-frame poll delay"
    );
    engine.stop_can_capture().unwrap();
    let mut reader = lognook_core::CanCaptureReader::open(&capture_path).unwrap();
    let frames = std::iter::from_fn(|| reader.next())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(frames.len(), 128);
    assert!(
        frames
            .windows(2)
            .all(|pair| pair[0].elapsed_ns < pair[1].elapsed_ns)
    );
    assert_eq!(frames.first().unwrap().data, [0]);
    assert_eq!(frames.last().unwrap().data, [127]);
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(capture_path);
}
