use lognook_core::{
    CanByteOrder, CanCaptureReader, CanDirection, CanFrame, CanScanConfig, CanScanMode,
    CanSignalDefinition, CanTransport, CanTransportError, CanTransportEvent, CanWorkbenchConfig,
    EngineEvent, LogNookEngine, OemCandidate, OemCandidateTrySpec, SampleQuality,
    communication_trace::{
        CommunicationDirection, CommunicationTraceEvent, CommunicationTraceStore,
    },
    read_recording,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread::{self, ThreadId},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct FakeTransport {
    incoming: mpsc::Receiver<FakeInput>,
    transmitted: mpsc::Sender<CanFrame>,
    owners: Arc<Mutex<Vec<ThreadId>>>,
    start_delay: Duration,
    session_origin: Option<Instant>,
    next_echo: u32,
    transmit_error: Option<CanTransportError>,
    communication_sink: Option<lognook_core::communication_trace::CommunicationSink>,
}

enum FakeInput {
    Event(CanTransportEvent),
    Error(CanTransportError),
}

impl FakeTransport {
    fn note_owner(&self) {
        self.owners.lock().unwrap().push(thread::current().id());
    }

    fn trace_rx(&self, frame: &CanFrame) {
        let Some(sink) = &self.communication_sink else {
            return;
        };
        let mut event = CommunicationTraceEvent::new(
            self.source_id(),
            "fake_can_frame",
            CommunicationDirection::Rx,
        );
        event.route = Some(format!("CAN ID 0x{:X}", frame.can_id));
        event.bytes_hex = Some(lognook_core::communication_trace::bytes_to_hex(&frame.data));
        event.outcome = "received".into();
        let _ = sink.try_record(event);
    }
}

impl CanTransport for FakeTransport {
    fn set_communication_sink(
        &mut self,
        sink: lognook_core::communication_trace::CommunicationSink,
    ) {
        self.communication_sink = Some(sink);
    }

    fn source_id(&self) -> String {
        "can:test-adapter".into()
    }

    fn start(
        &mut self,
        _bitrate: u32,
        _listen_only: bool,
        session_origin: Instant,
    ) -> Result<(), CanTransportError> {
        self.note_owner();
        self.session_origin = Some(session_origin);
        thread::sleep(self.start_delay);
        Ok(())
    }

    fn receive(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<CanTransportEvent>, CanTransportError> {
        self.note_owner();
        match self.incoming.recv_timeout(timeout) {
            Ok(FakeInput::Event(CanTransportEvent::Rx(mut frame))) => {
                if frame.elapsed_ns < 0 {
                    let elapsed = self.session_origin.unwrap().elapsed();
                    frame.elapsed_ns = elapsed.as_nanos().min(i64::MAX as u128) as i64;
                    frame.wall_time_unix_ns = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        .min(i64::MAX as u128) as i64;
                }
                self.trace_rx(&frame);
                Ok(Some(CanTransportEvent::Rx(frame)))
            }
            Ok(FakeInput::Event(event)) => Ok(Some(event)),
            Ok(FakeInput::Error(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(CanTransportError::other("fake removed"))
            }
        }
    }

    fn transmit(&mut self, frame: &CanFrame) -> Result<u32, CanTransportError> {
        self.note_owner();
        if let Some(error) = self.transmit_error.take() {
            return Err(error);
        }
        self.transmitted
            .send(frame.clone())
            .map_err(|error| CanTransportError::other(error.to_string()))?;
        let echo_id = self.next_echo;
        self.next_echo = self.next_echo.wrapping_add(1);
        Ok(echo_id)
    }

    fn stop(&mut self) -> Result<(), CanTransportError> {
        self.note_owner();
        Ok(())
    }
}

fn transport() -> (
    Box<dyn CanTransport>,
    mpsc::Sender<FakeInput>,
    mpsc::Receiver<CanFrame>,
    Arc<Mutex<Vec<ThreadId>>>,
) {
    transport_with_start_delay(Duration::ZERO)
}

fn transport_with_start_delay(
    start_delay: Duration,
) -> (
    Box<dyn CanTransport>,
    mpsc::Sender<FakeInput>,
    mpsc::Receiver<CanFrame>,
    Arc<Mutex<Vec<ThreadId>>>,
) {
    transport_with_options(start_delay, None)
}

fn transport_with_transmit_error(
    error: CanTransportError,
) -> (
    Box<dyn CanTransport>,
    mpsc::Sender<FakeInput>,
    mpsc::Receiver<CanFrame>,
    Arc<Mutex<Vec<ThreadId>>>,
) {
    transport_with_options(Duration::ZERO, Some(error))
}

fn transport_with_options(
    start_delay: Duration,
    transmit_error: Option<CanTransportError>,
) -> (
    Box<dyn CanTransport>,
    mpsc::Sender<FakeInput>,
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
            start_delay,
            session_origin: None,
            next_echo: 0,
            transmit_error,
            communication_sink: None,
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

fn can_oem_candidate(service: &str, identifier: &str) -> OemCandidateTrySpec {
    OemCandidateTrySpec {
        candidate: OemCandidate {
            candidate_id: format!("candidate-{service}-{identifier}"),
            source_id: "SCION_SOURCE".into(),
            family: "Toyota".into(),
            brand: "Scion".into(),
            model_scope: "xB 2014".into(),
            service: service.into(),
            identifier: identifier.into(),
            request_header: "7E0".into(),
            response_header: "7E8".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "Engine".into(),
            name: format!("Candidate {service} {identifier}"),
            signal_source_id: "SCION_SOURCE_01".into(),
            bit_offset: Some(0),
            bit_length: Some(8),
            formula: "raw * 2 + 1".into(),
            decoder_json: r#"{"bix":0,"len":8,"mul":2,"div":1,"add":1,"unit":"rpm"}"#.into(),
            unit: "rpm".into(),
            year_filter_json: "{}".into(),
            ecu_filter: String::new(),
            source_blob_sha: "reviewed-sha".into(),
            research_state: "community_reference".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "research".into(),
            notes: String::new(),
            suggested_metric: String::new(),
            source_scope: "vehicle".into(),
            applicability_specificity: 60,
            min_value: Some(0.0),
            max_value: Some(200.0),
            source_url: Some("https://source.test/vehicle".into()),
            license: Some("CC BY-SA 4.0".into()),
            license_url: Some("https://source.test/license".into()),
        },
        value_format: "uint".into(),
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

fn cleanup_capture(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(lognook_core::can_evidence_path(path));
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

fn trigger_fixture() -> (LogNookEngine, Arc<Mutex<Vec<EngineEvent>>>, PathBuf) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, _, _, _) = transport();
    engine
        .connect_can_transport(config(true), transport)
        .unwrap();
    let path = temp_path("candump.csv");
    engine.start_can_capture(&path).unwrap();
    (engine, events, path)
}

fn trigger_events(
    events: &Arc<Mutex<Vec<EngineEvent>>>,
) -> Vec<lognook_core::CanTriggerCaptureResult> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::CanTriggerCapture(capture) => Some(capture.clone()),
            _ => None,
        })
        .collect()
}

fn live_event_snapshots(
    events: &Arc<Mutex<Vec<EngineEvent>>>,
) -> Vec<lognook_core::CanLiveEventSnapshot> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::CanLiveEventAnalysis(snapshot) => Some(snapshot.clone()),
            _ => None,
        })
        .collect()
}

fn journal_records(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(lognook_core::can_evidence_path(path))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn live_event_results_use_worker_frames_and_clear_without_removing_capture_evidence() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, _, _) = transport();
    engine
        .connect_can_transport(config(true), transport)
        .unwrap();
    let path = temp_path("candump.csv");
    engine.start_can_capture(&path).unwrap();

    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x120,
            CanDirection::Rx,
            &[0],
            -1,
        ))))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanFrames(frames) if frames.iter().any(|frame| frame.can_id == 0x120))
        })
    });
    engine.mark_can_event("switch", 200, 50).unwrap();
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x120,
            CanDirection::Rx,
            &[1],
            -1,
        ))))
        .unwrap();
    wait_until(|| {
        live_event_snapshots(&events).iter().any(|snapshot| {
            snapshot
                .groups
                .iter()
                .any(|group| group.completed_events == 1)
        })
    });

    let result = live_event_snapshots(&events)
        .into_iter()
        .rev()
        .find(|snapshot| {
            snapshot
                .groups
                .iter()
                .any(|group| group.completed_events == 1)
        })
        .unwrap();
    let candidate = &result.groups[0].candidates[0];
    assert_eq!(candidate.correlation.changed_events, 1);
    assert_eq!(candidate.sample_frame.as_ref().unwrap().data, vec![1]);

    engine.clear_can_event_results().unwrap();
    wait_until(|| {
        live_event_snapshots(&events)
            .last()
            .is_some_and(|snapshot| snapshot.groups.is_empty())
    });
    engine.stop_can_capture().unwrap();
    assert!(!live_event_snapshots(&events).last().unwrap().active);
    assert!(
        journal_records(&path)
            .iter()
            .any(|record| record["record"] == "manual_event_marked")
    );
    drop(engine);
    cleanup_capture(&path);
}

#[test]
fn disarming_a_marked_trigger_emits_an_incomplete_window_and_early_end_evidence() {
    let (engine, events, path) = trigger_fixture();
    engine.arm_can_trigger(0, 60_000, 8).unwrap();
    engine.trigger_can_now().unwrap();
    engine.disarm_can_trigger().unwrap();

    let windows = trigger_events(&events);
    assert_eq!(windows.len(), 1);
    assert!(!windows[0].capture.complete);
    assert_eq!(windows[0].capture.post_ns, 60_000_000_000);
    let records = journal_records(&path);
    assert!(records.iter().any(|record| {
        record["record"] == "trigger_ended_early"
            && record["data"]["reason"] == "disarmed"
            && record["data"]["complete"] == false
    }));

    engine.stop_can_capture().unwrap();
    drop(engine);
    cleanup_capture(&path);
}

#[test]
fn disarming_before_a_trigger_mark_cancels_without_a_window_or_terminal_record() {
    let (engine, events, path) = trigger_fixture();
    engine.arm_can_trigger(100, 100, 8).unwrap();
    engine.disarm_can_trigger().unwrap();
    assert!(trigger_events(&events).is_empty());
    engine.stop_can_capture().unwrap();
    assert!(!journal_records(&path).iter().any(|record| {
        matches!(
            record["record"].as_str(),
            Some("trigger_marked" | "trigger_ended_early")
        )
    }));

    drop(engine);
    cleanup_capture(&path);
}

#[test]
fn stopping_capture_after_a_trigger_mark_finalizes_it_as_capture_stopped() {
    let (engine, events, path) = trigger_fixture();
    engine.arm_can_trigger(0, 60_000, 8).unwrap();
    engine.trigger_can_now().unwrap();
    engine.stop_can_capture().unwrap();

    let windows = trigger_events(&events);
    assert_eq!(windows.len(), 1);
    assert!(!windows[0].capture.complete);
    assert!(journal_records(&path).iter().any(|record| {
        record["record"] == "trigger_ended_early"
            && record["data"]["reason"] == "capture_stopped"
            && record["data"]["complete"] == false
    }));

    drop(engine);
    cleanup_capture(&path);
}

#[test]
fn worker_shutdown_after_a_trigger_mark_emits_and_journals_a_partial_window() {
    let (engine, events, path) = trigger_fixture();
    engine.arm_can_trigger(0, 60_000, 8).unwrap();
    engine.trigger_can_now().unwrap();
    drop(engine);

    let windows = trigger_events(&events);
    assert_eq!(windows.len(), 1);
    assert!(!windows[0].capture.complete);
    assert!(journal_records(&path).iter().any(|record| {
        record["record"] == "trigger_ended_early"
            && record["data"]["reason"] == "session_ended"
            && record["data"]["complete"] == false
    }));

    cleanup_capture(&path);
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
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(received.clone())))
        .unwrap();

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
fn manual_trigger_is_marked_on_the_owner_and_returns_a_bounded_window() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, _incoming, _transmitted, owners) = transport();
    engine
        .connect_can_transport(config(true), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();

    assert!(engine.arm_can_trigger(60_001, 0, 4).is_err());
    assert!(engine.arm_can_trigger(0, 0, 100_001).is_err());
    engine.arm_can_trigger(0, 0, 4).unwrap();
    engine.trigger_can_now().unwrap();
    wait_until(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, EngineEvent::CanTriggerCapture(_)))
    });
    let captured = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanTriggerCapture(capture) => Some(capture.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(captured.source_capture_id, 1);
    assert_eq!(captured.source_id, "can:test-adapter");
    assert!(captured.capture.complete);
    assert!(captured.capture.frames.is_empty());
    assert_eq!(captured.capture.max_frames, 4);
    assert!(
        std::fs::read_to_string(lognook_core::can_evidence_path(&capture_path))
            .unwrap()
            .contains("trigger_completed")
    );
    assert_eq!(
        owners
            .lock()
            .unwrap()
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1
    );

    engine.stop_can_capture().unwrap();
    drop(engine);
    cleanup_capture(&capture_path);
}

#[test]
fn scan_uses_the_configured_id_range_and_stop_cancels_before_the_next_send() {
    let mut engine = LogNookEngine::new(|_| {});
    let (transport, _incoming, transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();

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
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(capture_path);
}

#[test]
fn active_send_and_scan_are_rejected_without_a_capture_journal() {
    let mut engine = LogNookEngine::new(|_| {});
    let (transport, _incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let request = CanFrame {
        elapsed_ns: 0,
        wall_time_unix_ns: 0,
        can_id: 0x123,
        extended: false,
        direction: CanDirection::Tx,
        data: vec![0x55],
        flags: 0,
    };
    assert!(engine.send_can_frame(request, 10, 1).is_err());
    assert!(
        engine
            .start_can_scan(CanScanConfig {
                mode: CanScanMode::CanIdRange {
                    start_id: 0x100,
                    end_id: 0x100,
                    extended: false,
                    payload: vec![0x55],
                },
                pacing_ms: 10,
                response_timeout_ms: 100,
                source_candidate_id: None,
            })
            .is_err()
    );
    assert!(transmitted.try_recv().is_err());
    engine.disconnect().unwrap();
}

#[test]
fn deserialized_scan_config_cannot_claim_catalog_provenance() {
    let config: CanScanConfig = serde_json::from_value(serde_json::json!({
        "mode": {"kind":"mode21", "request_id":2016, "response_id":2024,
                 "extended":false, "start_pid":1, "end_pid":1},
        "pacing_ms":100,
        "response_timeout_ms":1000,
        "source_candidate_id":"forged-candidate"
    }))
    .unwrap();
    assert_eq!(config.source_candidate_id, None);
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
            source_candidate_id: Some("scion:21:odometer".into()),
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
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(rx.clone())))
        .unwrap();
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
    assert_eq!(result.response_frames, [rx.clone()]);

    engine.stop_can_capture().unwrap();
    let capture = std::fs::read_to_string(&capture_path).unwrap();
    assert_eq!(capture.lines().count(), 3, "header, exact TX, and exact RX");
    assert!(capture.contains("tx,7E0,standard,8,0221010000000000,00000000"));
    assert!(capture.contains("rx,7E8,standard,4,036101A7,00000000"));
    let evidence: Vec<lognook_core::CanEvidenceRecord> =
        std::fs::read_to_string(lognook_core::can_evidence_path(&capture_path))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(evidence.iter().any(|record| matches!(record,
        lognook_core::CanEvidenceRecord::ScanStarted { config }
            if config.source_candidate_id.as_deref() == Some("scion:21:odometer")
    )));
    assert!(evidence.iter().any(|record| matches!(record,
        lognook_core::CanEvidenceRecord::ScanResult { result }
            if result.request == tx && result.response_frames == [rx.clone()]
                && result.outcome == lognook_core::CanDiagnosticOutcome::Positive
    )));
    engine.disconnect().unwrap();
    let owners = owners.lock().unwrap();
    assert!(owners.iter().all(|owner| owner == &owners[0]));
    drop(owners);
    cleanup_capture(&capture_path);
}

#[test]
fn failed_csv_creation_rolls_back_the_new_sidecar() {
    let mut engine = LogNookEngine::new(|_| {});
    let (transport, _incoming, _transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    std::fs::write(&capture_path, b"keep existing capture").unwrap();
    assert!(engine.start_can_capture(&capture_path).is_err());
    assert_eq!(
        std::fs::read(&capture_path).unwrap(),
        b"keep existing capture"
    );
    assert!(!lognook_core::can_evidence_path(&capture_path).exists());
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn submitted_tx_is_unconfirmed_until_echo_and_transport_raw_bytes_are_evidence() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    let frame = frame(0x321, CanDirection::Tx, &[0x22, 0x33], 0);
    engine.send_can_frame(frame, 10, 1).unwrap();
    let submitted = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                EngineEvent::CanTxStatus {
                    sequence: 1,
                    confirmed: false,
                    ..
                }
            )
        })
    });
    let mut echo = submitted.clone();
    echo.direction = CanDirection::Tx;
    incoming
        .send(FakeInput::Event(CanTransportEvent::TxEcho {
            echo_id: 0,
            frame: echo.clone(),
            raw_usb: vec![0xA1, 0xB2],
        }))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanTxStatus { sequence: 1, confirmed: true, frame, .. } if frame == &echo)
        })
    });
    incoming
        .send(FakeInput::Error(CanTransportError {
            kind: lognook_core::CanTransportErrorKind::Controller,
            message: "controller error frame".into(),
            raw_usb: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        }))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanTransportError(error) if error.raw_usb.as_deref() == Some(&[0xDE, 0xAD, 0xBE, 0xEF]))
        })
    });
    drop(engine);

    let evidence: Vec<lognook_core::CanEvidenceRecord> =
        std::fs::read_to_string(lognook_core::can_evidence_path(&capture_path))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(evidence.iter().any(|record| matches!(record,
        lognook_core::CanEvidenceRecord::TxSubmitted { sequence: 1, frame, .. }
            if frame == &submitted
    )));
    assert!(evidence.iter().any(|record| matches!(record,
        lognook_core::CanEvidenceRecord::TxEchoConfirmed {
            sequence: 1,
            submitted_frame,
            echo_frame,
            raw_usb,
            ..
        } if submitted_frame == &submitted && echo_frame == &echo && raw_usb == &[0xA1, 0xB2]
    )));
    assert!(evidence.iter().any(|record| matches!(record,
        lognook_core::CanEvidenceRecord::TransportError { error, .. }
            if error.raw_usb.as_deref() == Some(&[0xDE, 0xAD, 0xBE, 0xEF])
    )));
    cleanup_capture(&capture_path);
}

#[test]
fn unsupported_receive_error_keeps_pending_scan_capture_and_session_alive() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();

    let capture_path = temp_path("candump.csv");
    let recording_path = temp_path("lognook");
    engine.start_can_capture(&capture_path).unwrap();
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
            response_timeout_ms: 250,
            source_candidate_id: None,
        })
        .unwrap();
    transmitted.recv_timeout(Duration::from_secs(1)).unwrap();

    incoming
        .send(FakeInput::Error(CanTransportError {
            kind: lognook_core::CanTransportErrorKind::UnsupportedFrame,
            message: "unsupported RTR frame".into(),
            raw_usb: Some(vec![0xA1, 0xD0]),
        }))
        .unwrap();
    let response = frame(0x7E8, CanDirection::Rx, &[4, 0x41, 0x0C, 0x1F, 0x40], -1);
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(response.clone())))
        .unwrap();

    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanTransportError(error)
                if error.kind == lognook_core::CanTransportErrorKind::UnsupportedFrame)
        })
    });
    assert!(
        engine.is_connected(),
        "a nonterminal unsupported frame must not disconnect the CAN workbench"
    );
    wait_until(|| {
        let events = events.lock().unwrap();
        events.iter().any(|event| {
            matches!(event, EngineEvent::Sample(sample) if sample.signal_id == "engine_rpm")
        }) && events.iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result)
                if result.outcome == lognook_core::CanDiagnosticOutcome::Positive)
        })
    });

    engine.stop_recording().unwrap();
    engine.stop_can_capture().unwrap();
    let recording = read_recording(&recording_path).unwrap();
    assert!(
        recording
            .samples
            .iter()
            .any(|sample| { sample.signal_id == "engine_rpm" && sample.value == 2000.0 })
    );
    let evidence: Vec<lognook_core::CanEvidenceRecord> =
        std::fs::read_to_string(lognook_core::can_evidence_path(&capture_path))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(evidence.iter().any(|record| matches!(record,
        lognook_core::CanEvidenceRecord::TransportError { error, .. }
            if error.kind == lognook_core::CanTransportErrorKind::UnsupportedFrame
                && error.raw_usb.as_deref() == Some(&[0xA1, 0xD0][..])
    )));
    let capture = std::fs::read_to_string(&capture_path).unwrap();
    assert!(capture.contains("rx,7E8,standard,5,04410C1F40,"));

    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
    let _ = std::fs::remove_file(recording_path);
}

#[test]
fn delayed_echo_from_closed_capture_is_not_written_into_the_next_capture() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_a = temp_path("candump.csv");
    engine.start_can_capture(&capture_a).unwrap();
    engine
        .send_can_frame(frame(0x321, CanDirection::Tx, &[0x11], 0), 10, 1)
        .unwrap();
    let submitted_a = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                EngineEvent::CanTxStatus {
                    sequence: 1,
                    confirmed: false,
                    ..
                }
            )
        })
    });
    engine.stop_can_capture().unwrap();

    let capture_b = temp_path("candump.csv");
    engine.start_can_capture(&capture_b).unwrap();
    let mut delayed_echo = submitted_a.clone();
    delayed_echo.direction = CanDirection::Tx;
    incoming
        .send(FakeInput::Event(CanTransportEvent::TxEcho {
            echo_id: 0,
            frame: delayed_echo,
            raw_usb: vec![0x44],
        }))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                EngineEvent::CanTxStatus {
                    sequence: 1,
                    confirmed: true,
                    ..
                }
            )
        })
    });
    engine.stop_can_capture().unwrap();

    let evidence_b: Vec<lognook_core::CanEvidenceRecord> =
        std::fs::read_to_string(lognook_core::can_evidence_path(&capture_b))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(!evidence_b.iter().any(|record| matches!(
        record,
        lognook_core::CanEvidenceRecord::TxEchoConfirmed { .. }
    )));
    let evidence_a: Vec<lognook_core::CanEvidenceRecord> =
        std::fs::read_to_string(lognook_core::can_evidence_path(&capture_a))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert!(evidence_a.iter().any(|record| matches!(
        record,
        lognook_core::CanEvidenceRecord::TxSubmitted { frame, .. } if frame == &submitted_a
    )));
    assert!(!evidence_a.iter().any(|record| matches!(
        record,
        lognook_core::CanEvidenceRecord::TxEchoConfirmed { .. }
    )));
    engine.disconnect().unwrap();
    cleanup_capture(&capture_a);
    cleanup_capture(&capture_b);
}

#[test]
fn natural_scan_completion_has_its_own_event_and_journal_record() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, _incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::CanIdRange {
                start_id: 0x123,
                end_id: 0x123,
                extended: false,
                payload: vec![0x55],
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    assert_eq!(
        transmitted
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .can_id,
        0x123
    );
    wait_until(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, EngineEvent::CanScanCompleted(_)))
    });
    let events = events.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EngineEvent::CanScanStopped))
    );
    drop(events);
    engine.stop_can_capture().unwrap();
    let journal = std::fs::read_to_string(lognook_core::can_evidence_path(&capture_path)).unwrap();
    assert!(journal.lines().any(|line| line.contains("scan_completed")));
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn stopping_capture_cancels_pending_scan_transmissions() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, _incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::CanIdRange {
                start_id: 0x200,
                end_id: 0x210,
                extended: false,
                payload: vec![0x55],
            },
            pacing_ms: 100,
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
    engine.stop_can_capture().unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!(transmitted.try_recv().is_err());
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, EngineEvent::CanScanStopped))
    );
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn stopping_capture_cancels_repeated_manual_transmissions() {
    let mut engine = LogNookEngine::new(|_| {});
    let (transport, _incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .send_can_frame(frame(0x321, CanDirection::Tx, &[0x55], 0), 100, 10)
        .unwrap();
    transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    engine.stop_can_capture().unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!(transmitted.try_recv().is_err());
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn excessive_scan_response_storage_is_bounded_while_raw_frames_are_kept() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
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
                start_pid: 1,
                end_pid: 1,
            },
            pacing_ms: 10,
            response_timeout_ms: 500,
            source_candidate_id: None,
        })
        .unwrap();
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    for index in 0..601 {
        incoming
            .send(FakeInput::Event(CanTransportEvent::Rx(frame(
                0x7E8,
                CanDirection::Rx,
                &[0],
                request.elapsed_ns + i64::from(index) + 1,
            ))))
            .unwrap();
    }
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.outcome == lognook_core::CanDiagnosticOutcome::Malformed)
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
    assert_eq!(result.response_frames.len(), 600);
    assert!(events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::Error(message) if message.contains("retained response limit"))
    }));
    engine.stop_can_capture().unwrap();
    let raw = std::fs::read_to_string(&capture_path).unwrap();
    assert_eq!(
        raw.lines().count(),
        603,
        "header, request, and every RX frame"
    );
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn rx_then_fast_next_scan_tx_stays_ordered_across_slow_transport_startup() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) =
        transport_with_start_delay(Duration::from_millis(40));
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
                end_pid: 0x02,
            },
            pacing_ms: 1,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let tx1 = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x7E8,
            CanDirection::Rx,
            &[3, 0x61, 1, 0xA7],
            -1,
        ))))
        .unwrap();
    let tx2 = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(tx2.data, [2, 0x21, 2, 0, 0, 0, 0, 0]);
    wait_until(|| {
        let events = events.lock().unwrap();
        events.iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.identifier == "21 01")
        }) && events.iter().any(|event| {
            matches!(event, EngineEvent::CanFrames(frames) if frames.iter().any(|frame| frame.can_id == 0x7E8 && frame.data == [3, 0x61, 1, 0xA7]))
        })
    });
    let rx = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanFrames(frames) => frames
                .iter()
                .find(|frame| frame.can_id == 0x7E8 && frame.data == [3, 0x61, 1, 0xA7])
                .cloned(),
            _ => None,
        })
        .unwrap();
    assert!(tx2.elapsed_ns > rx.elapsed_ns);
    assert!(rx.elapsed_ns > tx1.elapsed_ns);
    engine.stop_can_scan().unwrap();
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
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
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
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
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(rx.clone())))
        .unwrap();
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
    assert_eq!(sample.source_id, "can:test-adapter:obd");
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(capture_path);
    let _ = std::fs::remove_file(recording_path);
}

#[test]
fn catalog_backed_can_mode21_and_mode22_reads_decode_and_record_samples() {
    for (service, identifier, request, response_frames, response_pdu) in [
        (
            "21",
            "01",
            vec![2, 0x21, 0x01],
            vec![vec![3, 0x61, 0x01, 0x64]],
            vec![0x61, 0x01, 0x64],
        ),
        (
            "22",
            "1234",
            vec![3, 0x22, 0x12, 0x34],
            vec![
                vec![0x10, 0x08, 0x62, 0x12, 0x34, 0x64, 0xAA, 0xBB],
                vec![0x21, 0xCC, 0xDD, 0, 0, 0, 0, 0],
            ],
            vec![0x62, 0x12, 0x34, 0x64, 0xAA, 0xBB, 0xCC, 0xDD],
        ),
    ] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_sink = events.clone();
        let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
        let (transport, incoming, transmitted, _owners) = transport();
        engine
            .connect_can_transport(config(false), transport)
            .unwrap();
        let capture_path = temp_path("candump.csv");
        let recording_path = temp_path("lognook");
        engine.start_can_capture(&capture_path).unwrap();
        engine.start_recording(&recording_path).unwrap();
        let candidate = can_oem_candidate(service, identifier);
        let candidate_id = candidate.candidate.candidate_id.clone();
        engine.start_can_oem_candidate(candidate).unwrap();

        let tx = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(&tx.data[..request.len()], request);
        let mut last_rx = frame(
            0x7E8,
            CanDirection::Rx,
            &response_frames[0],
            tx.elapsed_ns + 2_000_000,
        );
        incoming
            .send(FakeInput::Event(CanTransportEvent::Rx(last_rx.clone())))
            .unwrap();
        for (index, response_frame) in response_frames.iter().enumerate().skip(1) {
            let flow_control = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(flow_control.can_id, 0x7E0);
            assert_eq!(flow_control.data[0], 0x30);
            last_rx = frame(
                0x7E8,
                CanDirection::Rx,
                response_frame,
                tx.elapsed_ns + 2_000_000 + index as i64 * 1_000_000,
            );
            incoming
                .send(FakeInput::Event(CanTransportEvent::Rx(last_rx.clone())))
                .unwrap();
        }
        let signal_id = format!("oem:{candidate_id}");
        wait_until(|| {
            events.lock().unwrap().iter().any(|event| {
                matches!(event, EngineEvent::Sample(sample) if sample.signal_id == signal_id)
            })
        });

        engine.stop_recording().unwrap();
        let recording = read_recording(&recording_path).unwrap();
        let sample = recording
            .samples
            .iter()
            .find(|sample| sample.signal_id == signal_id)
            .unwrap();
        assert_eq!(sample.value, 201.0);
        assert_eq!(sample.unit, "rpm");
        assert_eq!(sample.elapsed_ns, last_rx.elapsed_ns);
        assert_eq!(sample.wall_time_unix_ns, last_rx.wall_time_unix_ns);
        assert_eq!(sample.ecu_id.as_deref(), Some("7E8"));
        assert_eq!(sample.quality, SampleQuality::Suspect);
        assert_eq!(sample.raw_bytes.as_deref(), Some(response_pdu.as_slice()));
        assert!(events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result)
                if result.config.source_candidate_id.as_deref() == Some(candidate_id.as_str())
                    && result.response_frames.iter().map(|frame| frame.data.clone()).collect::<Vec<_>>() == response_frames)
        }));

        engine.stop_can_capture().unwrap();
        engine.disconnect().unwrap();
        cleanup_capture(&capture_path);
        let _ = std::fs::remove_file(recording_path);
    }
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
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
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
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x7E8,
            CanDirection::Rx,
            &[6, 0x41, 0, 0x10, 0, 0, 0],
            request.elapsed_ns + 1_000_000,
        ))))
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
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(capture_path);
}

#[test]
fn malformed_known_width_mode01_reply_is_not_reported_as_positive() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
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
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x7E8,
            CanDirection::Rx,
            &[3, 0x41, 0x0C, 0x1F],
            request.elapsed_ns + 1_000_000,
        ))))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.outcome == lognook_core::CanDiagnosticOutcome::Malformed)
        })
    });
    assert!(events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::Error(message) if message.contains("Mode 01 PID 0C"))
    }));
    assert!(!events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::CanScanResult(result) if result.outcome == lognook_core::CanDiagnosticOutcome::Positive)
    }));
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(capture_path);
}

#[test]
fn unknown_width_mode01_positive_reply_stays_positive_raw_evidence() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode01Pids {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                pids: vec![0xAA, 0x00],
                repeat: false,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    let response = frame(
        0x7E8,
        CanDirection::Rx,
        &[3, 0x41, 0xAA, 0x01],
        request.elapsed_ns + 1_000_000,
    );
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(response.clone())))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result) if result.identifier == "01 AA")
        })
    });
    let result = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanScanResult(result) if result.identifier == "01 AA" => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(result.outcome, lognook_core::CanDiagnosticOutcome::Positive);
    assert_eq!(result.response_frames, [response]);
    assert!(result.supported_pids.is_none());
    assert!(!events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::Error(message) if message.contains("Mode 01 PID AA"))
    }));
    let request_without_decoder = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(request_without_decoder.data, [2, 0x01, 0x00, 0, 0, 0, 0, 0]);
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x7E8,
            CanDirection::Rx,
            &[5, 0x41, 0x00, 0x10, 0x00, 0x00],
            request_without_decoder.elapsed_ns + 1_000_000,
        ))))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result)
                if result.identifier == "01 00" && result.outcome == lognook_core::CanDiagnosticOutcome::Malformed)
        })
    });
    assert!(events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::Error(message) if message.contains("Mode 01 PID 00"))
    }));
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn known_width_mode01_without_decoder_accepts_a_valid_raw_reply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode01Pids {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                pids: vec![0x00],
                repeat: false,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    let response = frame(
        0x7E8,
        CanDirection::Rx,
        &[6, 0x41, 0x00, 0x10, 0x00, 0x00, 0x00],
        request.elapsed_ns + 1_000_000,
    );
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(response.clone())))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result)
                if result.identifier == "01 00" && result.outcome == lognook_core::CanDiagnosticOutcome::Positive)
        })
    });
    let result = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanScanResult(result) if result.identifier == "01 00" => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(result.response_frames, [response]);
    assert!(result.supported_pids.is_none());
    assert!(
        !events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, EngineEvent::Sample(_)))
    );
    engine.stop_can_capture().unwrap();
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
}

#[test]
fn pending_scan_gets_a_typed_terminal_result_on_device_removal() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, incoming, transmitted, _) = transport();
    engine
        .connect_can_transport(config(false), transport)
        .unwrap();
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    engine
        .start_can_scan(CanScanConfig {
            mode: CanScanMode::Mode22 {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                start_did: 0xF190,
                end_did: 0xF191,
            },
            pacing_ms: 10,
            response_timeout_ms: 1000,
            source_candidate_id: Some("scion:22:vin".into()),
        })
        .unwrap();
    let request = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    let partial = frame(
        0x7E8,
        CanDirection::Rx,
        &[0x10, 0x0A, 0x62, 0xF1, 0x90, 1, 2, 3],
        request.elapsed_ns + 1_000_000,
    );
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(partial.clone())))
        .unwrap();
    assert_eq!(
        transmitted
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .data,
        [0x30, 0, 0, 0, 0, 0, 0, 0]
    );
    incoming
        .send(FakeInput::Error(CanTransportError {
            kind: lognook_core::CanTransportErrorKind::Disconnected,
            message: "adapter removed".into(),
            raw_usb: Some(vec![0xBA, 0xD0]),
        }))
        .unwrap();
    wait_until(|| {
        events.lock().unwrap().iter().any(|event| {
            matches!(event, EngineEvent::CanScanResult(result)
                if result.request == request && result.response_frames == [partial.clone()])
        })
    });
    let result = events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            EngineEvent::CanScanResult(result) if result.request == request => Some(result.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        serde_json::to_value(result.outcome)
            .unwrap()
            .get("TransportError")
            .is_some()
    );
    assert!(transmitted.recv_timeout(Duration::from_millis(50)).is_err());
    drop(engine);
    let journal = std::fs::read_to_string(lognook_core::can_evidence_path(&capture_path)).unwrap();
    assert!(
        journal
            .lines()
            .any(|line| line.contains("scan_result") && line.contains("TransportError"))
    );
    assert!(
        journal
            .lines()
            .any(|line| line.contains("transport_error") && line.contains("adapter removed"))
    );
    cleanup_capture(&capture_path);
}

#[test]
fn fatal_bus_off_transmit_stops_the_remaining_scan_range() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = events.clone();
    let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
    let (transport, _incoming, transmitted, _) = transport_with_transmit_error(CanTransportError {
        kind: lognook_core::CanTransportErrorKind::BusOff,
        message: "CAN controller bus-off".into(),
        raw_usb: Some(vec![0xBA, 0xD1]),
    });
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
                start_pid: 1,
                end_pid: 3,
            },
            pacing_ms: 10,
            response_timeout_ms: 100,
            source_candidate_id: None,
        })
        .unwrap();
    assert!(
        transmitted
            .recv_timeout(Duration::from_millis(150))
            .is_err()
    );
    assert!(events.lock().unwrap().iter().any(|event| {
        matches!(event, EngineEvent::CanTransportError(error)
            if matches!(error.kind, lognook_core::CanTransportErrorKind::BusOff))
    }));
    engine.disconnect().unwrap();
    cleanup_capture(&capture_path);
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
    let capture_path = temp_path("candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
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
        -1,
    );
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(first.clone())))
        .unwrap();
    let flow_control = transmitted.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(flow_control.can_id, 0x7E0);
    assert_eq!(flow_control.data, [0x30, 0, 0, 0, 0, 0, 0, 0]);
    let continuation = frame(0x7E8, CanDirection::Rx, &[0x21, 4, 5, 6, 7, 8, 9, 0], -1);
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(
            continuation.clone(),
        )))
        .unwrap();
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
    assert_eq!(
        result
            .response_frames
            .iter()
            .map(|frame| frame.data.clone())
            .collect::<Vec<_>>(),
        [first.data.clone(), continuation.data.clone()]
    );
    engine.stop_can_capture().unwrap();
    let mut reader = lognook_core::CanCaptureReader::open(&capture_path).unwrap();
    let captured = std::iter::from_fn(|| reader.next())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        captured
            .iter()
            .map(|frame| frame.direction)
            .collect::<Vec<_>>(),
        [
            CanDirection::Tx,
            CanDirection::Rx,
            CanDirection::Tx,
            CanDirection::Rx
        ],
        "the triggering RX frame must precede its flow-control TX"
    );
    let (replayed, _) = lognook_core::replay_can_capture_file(&capture_path, &[], |_| {}).unwrap();
    assert_eq!(
        replayed, 4,
        "replay should accept all capture frames in time order"
    );
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(capture_path);
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
            .send(FakeInput::Event(CanTransportEvent::Rx(frame(
                0x321,
                CanDirection::Rx,
                &[index as u8],
                1_000_000 + index,
            ))))
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

#[test]
fn communication_trace_and_can_capture_share_the_workbench_owner_frames() {
    let store = CommunicationTraceStore::new();
    let mut engine = LogNookEngine::new(|_| {});
    engine.set_communication_sink(store.sink());
    let (transport, incoming, _transmitted, _owners) = transport();
    engine
        .connect_can_transport(config(true), transport)
        .unwrap();
    let capture_path = temp_path("agent-can-trace.candump.csv");
    engine.start_can_capture(&capture_path).unwrap();
    incoming
        .send(FakeInput::Event(CanTransportEvent::Rx(frame(
            0x321,
            CanDirection::Rx,
            &[0x12, 0x34],
            1_000_000,
        ))))
        .unwrap();

    wait_until(|| {
        store
            .get_trace(0, 10)
            .is_ok_and(|page| !page.events.is_empty())
    });
    engine.stop_can_capture().unwrap();
    let page = store.get_trace(0, 10).unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].route.as_deref(), Some("CAN ID 0x321"));
    assert_eq!(page.events[0].bytes_hex.as_deref(), Some("1234"));

    let mut reader = CanCaptureReader::open(&capture_path).unwrap();
    let captured = std::iter::from_fn(|| reader.next())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].can_id, 0x321);
    assert_eq!(captured[0].data, [0x12, 0x34]);
    engine.disconnect().unwrap();
    let _ = std::fs::remove_file(&capture_path);
    let _ = std::fs::remove_file(lognook_core::can_evidence_path(&capture_path));
}
