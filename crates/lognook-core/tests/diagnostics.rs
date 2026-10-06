use lognook_core::{
    ConnectionCapabilities, DiagnosticPhase, DiagnosticProgress, DiagnosticSink, EngineEvent,
    HardwareAdapter, LogNookEngine, LogNookError, Sample, SampleOrigin, SampleQuality,
    SignalDefinition, SimulatedObdAdapter,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

fn progress(phase: DiagnosticPhase) -> DiagnosticProgress {
    DiagnosticProgress {
        adapter_id: Some("test-adapter".into()),
        operation: "acquisition".into(),
        stage: "poll".into(),
        phase,
        duration_ms: Some(12),
        signal_ids: vec!["engine_rpm".into(), "coolant_temp".into()],
        detail: None,
        transient: Some(false),
    }
}

#[test]
fn diagnostic_progress_round_trips_without_payload_fields() {
    for (phase, serialized_phase) in [
        (DiagnosticPhase::Started, "started"),
        (DiagnosticPhase::Progress, "progress"),
        (DiagnosticPhase::Succeeded, "succeeded"),
        (DiagnosticPhase::Failed, "failed"),
    ] {
        let event = progress(phase);
        let json = serde_json::to_value(&event).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object["phase"], serialized_phase);
        let mut fields = object.keys().map(String::as_str).collect::<Vec<_>>();
        fields.sort_unstable();
        assert_eq!(
            fields,
            [
                "adapter_id",
                "detail",
                "duration_ms",
                "operation",
                "phase",
                "signal_ids",
                "stage",
                "transient"
            ]
        );
        assert_eq!(
            serde_json::from_value::<DiagnosticProgress>(json).unwrap(),
            event
        );
    }
    let mut event = progress(DiagnosticPhase::Started);
    event.adapter_id = None;
    event.duration_ms = None;
    event.transient = None;
    assert_eq!(
        serde_json::from_str::<DiagnosticProgress>(&serde_json::to_string(&event).unwrap())
            .unwrap(),
        event
    );
}

#[test]
fn default_adapter_diagnostic_sink_is_noop() {
    let mut adapter = SimulatedObdAdapter::default();
    adapter.set_diagnostic_sink(Arc::new(|_| panic!("default sink must be a no-op")));
    let capabilities = adapter.connect().unwrap();
    assert!(
        !adapter
            .read_samples(&capabilities.supported_signals[..1])
            .unwrap()
            .is_empty()
    );
    adapter.disconnect();
}

struct ScriptedAdapter {
    sink: Option<DiagnosticSink>,
    polls: usize,
}

impl HardwareAdapter for ScriptedAdapter {
    fn set_diagnostic_sink(&mut self, sink: DiagnosticSink) {
        self.sink = Some(sink);
    }
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        (self.sink.as_ref().expect("sink installed before connect"))(DiagnosticProgress {
            operation: "connect".into(),
            stage: "adapter".into(),
            ..progress(DiagnosticPhase::Started)
        });
        let mut capabilities = SimulatedObdAdapter::default().connect()?;
        capabilities.device.stable_id = "test-adapter".into();
        Ok(capabilities)
    }
    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        assert_eq!(
            signals.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["engine_rpm", "coolant_temp"]
        );
        self.polls += 1;
        if self.polls > 1 {
            return Err(LogNookError::new("poll failed"));
        }
        Ok(vec![expected_sample()])
    }
    fn sample_interval(&self) -> Duration {
        Duration::from_millis(10)
    }
}

fn expected_sample() -> Sample {
    Sample {
        elapsed_ns: 1_000_000,
        wall_time_unix_ns: 123,
        source_id: "test-adapter".into(),
        ecu_id: Some("test-ecu".into()),
        signal_id: "engine_rpm".into(),
        value: 1234.5,
        unit: "rpm".into(),
        quality: SampleQuality::Valid,
        origin: SampleOrigin::Live,
        raw_bytes: Some(vec![0x41, 0x0c, 0x13, 0x4a]),
    }
}

#[test]
fn engine_installs_diagnostic_sink_before_adapter_connect() {
    let (tx, rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(|_| {});
    engine.set_diagnostic_sink(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    engine
        .connect_adapter(
            Box::new(ScriptedAdapter {
                sink: None,
                polls: 0,
            }),
            &["engine_rpm".into(), "coolant_temp".into()],
        )
        .unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap().operation,
        "connect"
    );
    engine.disconnect().unwrap();
}

#[test]
fn acquisition_emits_poll_start_and_finish_without_changing_samples() {
    let (events, event_rx) = mpsc::channel();
    let (diagnostics, diagnostic_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events.send(event);
    });
    engine.set_diagnostic_sink(Arc::new(move |event| {
        let _ = diagnostics.send(event);
    }));
    engine
        .connect_adapter(
            Box::new(ScriptedAdapter {
                sink: None,
                polls: 0,
            }),
            &["engine_rpm".into(), "coolant_temp".into()],
        )
        .unwrap();
    assert_eq!(
        diagnostic_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .operation,
        "connect"
    );
    for phase in [
        DiagnosticPhase::Started,
        DiagnosticPhase::Succeeded,
        DiagnosticPhase::Started,
        DiagnosticPhase::Failed,
    ] {
        let event = diagnostic_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(event.phase, phase);
        assert_eq!(event.adapter_id.as_deref(), Some("test-adapter"));
        assert_eq!(event.signal_ids, ["engine_rpm", "coolant_temp"]);
        assert_eq!(event.operation, "acquisition");
        assert_eq!(event.stage, "poll");
        if phase == DiagnosticPhase::Failed {
            assert_eq!(event.detail.as_deref(), Some("poll failed"));
        } else {
            assert_eq!(event.detail, None);
        }
        if phase == DiagnosticPhase::Started {
            assert_eq!(event.duration_ms, None);
        } else {
            assert!(event.duration_ms.is_some());
            assert_eq!(event.transient, Some(false));
        }
    }
    assert!(matches!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Connected(_)
    ));
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Sample(expected_sample())
    );
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Error("poll failed".into())
    );
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Disconnected
    );
    engine.disconnect().unwrap();
}

struct MalformedOpenPortAdapter;

const MALFORMED_OPENPORT_REPLY: &[u8] = br#"{"type":"mode01_responses","responses":[{"ecu_id":"7E8","bytes":["41 0C 13 4A"],"latency_us":12}]}"#;

fn malformed_openport_error() -> String {
    let error = lognook_j2534_protocol::decode_response_line(MALFORMED_OPENPORT_REPLY).unwrap_err();
    format!("OpenPort worker returned malformed output: {error}")
}

impl HardwareAdapter for MalformedOpenPortAdapter {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        SimulatedObdAdapter::default().connect()
    }

    fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        // Reproduce the real worker client's wrapper around the protocol's parser error.
        Err(LogNookError::new(malformed_openport_error()))
    }
}

#[test]
fn acquisition_redacts_openport_parser_payload_without_changing_engine_error() {
    let original_error = malformed_openport_error();
    assert!(original_error.contains("41 0C 13 4A"));
    let (events, event_rx) = mpsc::channel();
    let (diagnostics, diagnostic_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events.send(event);
    });
    engine.set_diagnostic_sink(Arc::new(move |event| {
        let _ = diagnostics.send(event);
    }));
    engine
        .connect_adapter(Box::new(MalformedOpenPortAdapter), &["engine_rpm".into()])
        .unwrap();
    assert_eq!(
        diagnostic_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .phase,
        DiagnosticPhase::Started
    );
    let failed = diagnostic_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(failed.phase, DiagnosticPhase::Failed);
    assert_eq!(failed.signal_ids, ["engine_rpm"]);
    assert_eq!(failed.transient, Some(false));
    assert_eq!(
        failed.detail.as_deref(),
        Some(
            "OpenPort worker returned malformed output: Malformed J2534 worker message: [parser detail redacted]"
        )
    );
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("41 0C 13 4A")
    );
    assert!(matches!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Connected(_)
    ));
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Error(original_error)
    );
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        EngineEvent::Disconnected
    );
    engine.disconnect().unwrap();
}
