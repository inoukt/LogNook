use super::unexpected_worker_response;
use super::{OpenPort2Adapter, WorkerSession, available_from_session, identify_from_session};
use crate::{EngineEvent, HardwareAdapter, LogNookEngine, SignalDefinition, standard_obd_signals};
use lognook_j2534_protocol::{
    J2534Identity, J2534Provider, Mode01Payload, WorkerErrorKind, WorkerRequest, WorkerResponse,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone)]
struct FakeState {
    requests: Arc<Mutex<Vec<WorkerRequest>>>,
    shutdowns: Arc<Mutex<usize>>,
}

struct FakeSession {
    responses: VecDeque<Result<WorkerResponse, String>>,
    state: FakeState,
}

impl FakeSession {
    fn new(
        responses: impl IntoIterator<Item = Result<WorkerResponse, String>>,
    ) -> (Self, FakeState) {
        let state = FakeState {
            requests: Arc::new(Mutex::new(Vec::new())),
            shutdowns: Arc::new(Mutex::new(0)),
        };
        (
            Self {
                responses: responses.into_iter().collect(),
                state: state.clone(),
            },
            state,
        )
    }
}

impl WorkerSession for FakeSession {
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, crate::LogNookError> {
        self.state.requests.lock().unwrap().push(request);
        self.responses
            .pop_front()
            .unwrap_or_else(|| Err("unexpected worker request".into()))
            .map_err(crate::LogNookError::new)
    }

    fn shutdown(&mut self) {
        *self.state.shutdowns.lock().unwrap() += 1;
    }
}

fn identity() -> J2534Identity {
    J2534Identity {
        firmware: "OPTA firmware".into(),
        dll_version: "1.2.3".into(),
        api_version: "04.04".into(),
    }
}

fn connected() -> WorkerResponse {
    WorkerResponse::Connected {
        identity: identity(),
    }
}

fn payload(ecu_id: &str, bytes: &[u8]) -> Mode01Payload {
    Mode01Payload {
        ecu_id: ecu_id.into(),
        bytes: bytes.to_vec(),
        latency_us: 12_500,
    }
}

fn discovery() -> WorkerResponse {
    WorkerResponse::Mode01Responses {
        responses: vec![payload("7E8", &[0x41, 0x00, 0x08, 0x10, 0x00, 0x00])],
    }
}

fn adapter(
    responses: impl IntoIterator<Item = Result<WorkerResponse, String>>,
) -> (OpenPort2Adapter, FakeState) {
    let (session, state) = FakeSession::new(responses);
    (
        OpenPort2Adapter::new(PathBuf::from("worker.exe"), "opta".into())
            .with_session(Box::new(session)),
        state,
    )
}

fn rpm() -> SignalDefinition {
    standard_obd_signals()
        .into_iter()
        .find(|signal| signal.id == "engine_rpm")
        .unwrap()
}

#[test]
fn provider_listing_uses_only_typed_worker_provider_response() {
    let (mut session, state) = FakeSession::new([Ok(WorkerResponse::Providers {
        providers: vec![J2534Provider {
            id: "opta".into(),
            name: "Tactrix OpenPort 2.0".into(),
        }],
        diagnostics: vec!["Ignored DENSO: unsupported by LogNook".into()],
    })]);
    let discovery = available_from_session(&mut session).unwrap();
    assert_eq!(discovery.providers[0].id, "opta");
    assert_eq!(
        discovery.diagnostics,
        ["Ignored DENSO: unsupported by LogNook"]
    );
    assert_eq!(
        *state.requests.lock().unwrap(),
        vec![WorkerRequest::ListProviders]
    );
}

#[test]
fn worker_errors_report_the_openport_operation_stage() {
    let error = unexpected_worker_response(
        "Mode 01 PID discovery",
        WorkerResponse::Error {
            kind: WorkerErrorKind::NoResponse,
            message: "No response for standard Mode 01 PID 00".into(),
        },
    );

    assert!(error.to_string().contains("Mode 01 PID discovery"));
    assert!(error.to_string().contains("Mode 01 PID 00"));
}

#[test]
fn identity_uses_provider_id_and_returns_j2534_identity() {
    let (mut session, state) = FakeSession::new([Ok(WorkerResponse::Identity {
        identity: identity(),
    })]);
    let device = identify_from_session(&mut session, "opta").unwrap();
    assert_eq!(device.stable_id, "opta");
    assert_eq!(device.hardware_family, "OpenPort 2.0");
    assert!(device.firmware.unwrap().contains("OPTA firmware"));
    assert_eq!(
        state.requests.lock().unwrap()[0],
        WorkerRequest::Identify {
            provider_id: "opta".into()
        }
    );
}

#[test]
fn adapter_discovers_supported_pids_and_emits_timestamped_rpm_samples() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x41, 0x0C, 0x1A, 0xF8])],
        }),
    ]);
    let capabilities = adapter.connect().unwrap();
    assert!(capabilities.read_only);
    assert!(
        capabilities
            .supported_signals
            .iter()
            .any(|signal| signal.id == "engine_rpm")
    );
    assert!(
        !capabilities
            .supported_signals
            .iter()
            .any(|signal| signal.id == "vehicle_speed")
    );

    let sample = adapter.read_samples(&[rpm()]).unwrap().remove(0);
    assert_eq!(sample.signal_id, "engine_rpm");
    assert_eq!(sample.value, 1726.0);
    assert_eq!(sample.ecu_id.as_deref(), Some("7E8"));
    assert!(sample.elapsed_ns >= 0);
    assert!(sample.wall_time_unix_ns > 0);
    assert_eq!(sample.origin, crate::SampleOrigin::Live);
    assert_eq!(sample.raw_bytes, Some(vec![0x41, 0x0C, 0x1A, 0xF8]));
    assert_eq!(adapter.sample_interval(), Duration::from_millis(100));
    adapter.disconnect();

    assert_eq!(
        *state.requests.lock().unwrap(),
        vec![
            WorkerRequest::Connect {
                provider_id: "opta".into()
            },
            WorkerRequest::DiscoverMode01,
            WorkerRequest::ReadMode01 { pid: 0x0C },
            WorkerRequest::Disconnect,
        ]
    );
    assert_eq!(*state.shutdowns.lock().unwrap(), 1);
}

#[test]
fn discovery_rejects_multiple_ecu_responders_without_merging_capabilities() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![
                payload("7E8", &[0x41, 0x00, 0x08, 0x10, 0x00, 0x00]),
                payload("7E9", &[0x41, 0x00, 0x08, 0x10, 0x00, 0x00]),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    assert!(
        adapter
            .connect()
            .unwrap_err()
            .to_string()
            .contains("Multiple ECU")
    );
    assert_eq!(*state.shutdowns.lock().unwrap(), 1);
}

#[test]
fn discovery_follows_only_supported_pid_pages_and_rejects_a_missing_continuation() {
    let (mut adapter_instance, _) = adapter([
        Ok(connected()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![
                payload("7E8", &[0x41, 0x00, 0x00, 0x00, 0x00, 0x01]),
                payload("7E8", &[0x41, 0x20, 0x80, 0x00, 0x00, 0x00]),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    let capabilities = adapter_instance.connect().unwrap();
    assert!(
        capabilities
            .supported_signals
            .iter()
            .any(|signal| signal.id == "distance_with_mil")
    );
    adapter_instance.disconnect();

    let (mut adapter_instance, _) = adapter([
        Ok(connected()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x41, 0x00, 0x00, 0x00, 0x00, 0x01])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    assert!(
        adapter_instance
            .connect()
            .unwrap_err()
            .to_string()
            .contains("omitted supported-PID page 20")
    );
}

#[test]
fn read_rejects_ecu_mismatch_and_never_emits_a_sample() {
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E9", &[0x41, 0x0C, 0x1A, 0xF8])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    assert!(
        adapter
            .read_samples(&[rpm()])
            .unwrap_err()
            .to_string()
            .contains("ECU identity")
    );
    adapter.disconnect();
}

#[test]
fn read_rejects_multiple_ecu_responses_and_malformed_payloads() {
    for responses in [
        vec![
            payload("7E8", &[0x41, 0x0C, 0x1A, 0xF8]),
            payload("7E9", &[0x41, 0x0C, 0x1A, 0xF8]),
        ],
        vec![payload("7E8", &[0x41, 0x0C])],
    ] {
        let (mut adapter, _) = adapter([
            Ok(connected()),
            Ok(discovery()),
            Ok(WorkerResponse::Mode01Responses { responses }),
            Ok(WorkerResponse::Disconnected),
        ]);
        adapter.connect().unwrap();
        assert!(adapter.read_samples(&[rpm()]).is_err());
        adapter.disconnect();
    }
}

#[test]
fn timed_out_worker_request_returns_error_and_shutdown_cleans_up_session() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Err("J2534 worker response timed out".into()),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    assert!(
        adapter
            .read_samples(&[rpm()])
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
    adapter.disconnect();
    assert_eq!(*state.shutdowns.lock().unwrap(), 1);
}

#[test]
fn openport_samples_flow_through_the_shared_engine_pipeline() {
    let (adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x41, 0x0C, 0x1A, 0xF8])],
        }),
    ]);
    let (events, received) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events.send(event);
    });
    engine
        .connect_adapter(Box::new(adapter), &["engine_rpm".into()])
        .unwrap();
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(1)).unwrap(),
        EngineEvent::Connected(_)
    ));
    let sample = loop {
        if let EngineEvent::Sample(sample) = received.recv_timeout(Duration::from_secs(1)).unwrap()
        {
            break sample;
        }
    };
    assert_eq!(sample.signal_id, "engine_rpm");
    assert_eq!(sample.value, 1726.0);
    engine.disconnect().unwrap();
}

#[test]
fn adapter_rejects_unsupported_signal_ids_before_worker_read() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    let mut unsupported = rpm();
    unsupported.id = "not_in_standard_catalog".into();
    assert!(
        adapter
            .read_samples(&[unsupported])
            .unwrap_err()
            .to_string()
            .contains("Unsupported read-only Mode 01 signal")
    );
    let unsupported_pid = standard_obd_signals()
        .into_iter()
        .find(|signal| signal.id == "vehicle_speed")
        .unwrap();
    assert!(
        adapter
            .read_samples(&[unsupported_pid])
            .unwrap_err()
            .to_string()
            .contains("Unsupported read-only Mode 01 signal")
    );
    adapter.disconnect();
    assert!(
        !state
            .requests
            .lock()
            .unwrap()
            .contains(&WorkerRequest::ReadMode01 { pid: 0x0C })
    );
}

#[allow(dead_code)]
fn _worker_error_kind_is_covered(kind: WorkerErrorKind) -> WorkerErrorKind {
    kind
}

struct ProgressSession(FakeSession);
impl WorkerSession for ProgressSession {
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, crate::LogNookError> {
        self.0.request(request)
    }
    fn request_with_progress(
        &mut self,
        request: WorkerRequest,
        sink: &crate::DiagnosticSink,
    ) -> Result<WorkerResponse, crate::LogNookError> {
        let stage = match &request {
            WorkerRequest::ListProviders => "provider_resolution",
            WorkerRequest::Identify { .. } => "version_read",
            WorkerRequest::Connect { .. } => "filter_setup",
            WorkerRequest::DiscoverMode01 => "mode01_discovery",
            WorkerRequest::ReadMode01 { .. } => "pid_read",
            WorkerRequest::Disconnect => "disconnect",
        };
        sink(crate::DiagnosticProgress {
            adapter_id: None,
            operation: "fixture".into(),
            stage: stage.into(),
            phase: crate::DiagnosticPhase::Progress,
            duration_ms: Some(1),
            signal_ids: vec![],
            detail: None,
            transient: None,
        });
        self.request(request)
    }
    fn shutdown(&mut self) {
        self.0.shutdown();
    }
}

#[test]
fn discovery_and_identify_helpers_use_progress_aware_session() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let sink: crate::DiagnosticSink = Arc::new(move |event| captured.lock().unwrap().push(event));
    let (fake, state) = FakeSession::new([
        Ok(WorkerResponse::Providers {
            providers: vec![],
            diagnostics: vec![],
        }),
        Ok(WorkerResponse::Identity {
            identity: identity(),
        }),
    ]);
    let mut session = ProgressSession(fake);
    super::available_from_session_with_diagnostics(&mut session, &sink).unwrap();
    super::identify_from_session_with_diagnostics(&mut session, "opta", &sink).unwrap();
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.stage.clone())
            .collect::<Vec<_>>(),
        ["provider_resolution", "version_read"]
    );
    assert_eq!(
        *state.requests.lock().unwrap(),
        [
            WorkerRequest::ListProviders,
            WorkerRequest::Identify {
                provider_id: "opta".into()
            }
        ]
    );
}

#[test]
fn adapter_progress_keeps_connect_and_acquisition_context_without_payloads() {
    let (fake, state) = FakeSession::new([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x41, 0x0C, 0x1A, 0xF8])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let mut adapter = OpenPort2Adapter::new(PathBuf::from("worker.exe"), "opta".into())
        .with_session(Box::new(ProgressSession(fake)));
    adapter.set_diagnostic_sink(Arc::new(move |event| captured.lock().unwrap().push(event)));
    adapter.connect().unwrap();
    let samples = adapter.read_samples(&[rpm()]).unwrap();
    assert_eq!(samples.len(), 1);
    adapter.disconnect();
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| (e.operation.as_str(), e.stage.as_str()))
            .collect::<Vec<_>>(),
        [
            ("connect", "filter_setup"),
            ("connect", "mode01_discovery"),
            ("acquisition", "pid_read"),
            ("acquisition", "pid_read"),
            ("acquisition", "pid_read"),
        ]
    );
    assert!(
        events
            .iter()
            .all(|e| e.adapter_id.as_deref() == Some("opta") && e.detail.is_none())
    );
    assert_eq!(events[2].phase, crate::DiagnosticPhase::Started);
    assert_eq!(events[3].phase, crate::DiagnosticPhase::Progress);
    assert_eq!(events[4].phase, crate::DiagnosticPhase::Succeeded);
    assert_eq!(events[4].signal_ids, ["engine_rpm"]);
    assert!(events[4].duration_ms.is_some());
    assert_eq!(state.requests.lock().unwrap().len(), 4);
}

struct CompletionTimingSession(FakeSession);
impl WorkerSession for CompletionTimingSession {
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, crate::LogNookError> {
        self.0.request(request)
    }
    fn request_with_progress(
        &mut self,
        request: WorkerRequest,
        sink: &crate::DiagnosticSink,
    ) -> Result<WorkerResponse, crate::LogNookError> {
        if matches!(request, WorkerRequest::ReadMode01 { .. }) {
            sink(crate::DiagnosticProgress {
                adapter_id: None,
                operation: "acquisition".into(),
                stage: "pid_read".into(),
                phase: crate::DiagnosticPhase::Progress,
                duration_ms: Some(777),
                signal_ids: vec![],
                detail: None,
                transient: None,
            });
            std::thread::sleep(Duration::from_millis(25));
        }
        self.0.request(request)
    }
    fn shutdown(&mut self) {
        self.0.shutdown();
    }
}

#[test]
fn selected_openport_reads_emit_real_completion_timing_without_extra_requests() {
    for response in [
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x41, 0x0C, 0x1A, 0xF8])],
        }),
        Err("OpenPort Timeout: during read: provider error 0x09".into()),
    ] {
        let expected_error = response.as_ref().err().cloned();
        let (session, state) = FakeSession::new([Ok(connected()), Ok(discovery()), response]);
        let mut adapter = OpenPort2Adapter::new(PathBuf::from("worker.exe"), "opta".into())
            .with_session(Box::new(CompletionTimingSession(session)));
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        adapter.set_diagnostic_sink(Arc::new(move |event| captured.lock().unwrap().push(event)));
        adapter.connect().unwrap();
        events.lock().unwrap().clear();
        let result = adapter.read_samples(&[rpm()]);
        if let Some(error) = &expected_error {
            assert_eq!(result.unwrap_err().to_string(), *error);
        } else {
            let samples = result.unwrap();
            assert_eq!(samples.len(), 1);
            assert_eq!(samples[0].value, 1726.0);
            assert_eq!(
                samples[0].raw_bytes.as_deref(),
                Some(&[0x41, 0x0C, 0x1A, 0xF8][..])
            );
        }
        let records = events.lock().unwrap();
        let terminal = records
            .iter()
            .find(|r| {
                r.operation == "acquisition"
                    && r.stage == "pid_read"
                    && matches!(
                        r.phase,
                        crate::DiagnosticPhase::Succeeded | crate::DiagnosticPhase::Failed
                    )
            })
            .expect("OpenPort PID read must emit actual completion timing");
        assert_eq!(terminal.adapter_id.as_deref(), Some("opta"));
        assert_eq!(terminal.signal_ids, ["engine_rpm"]);
        assert!(terminal.duration_ms.unwrap() >= 20);
        assert_ne!(terminal.duration_ms, Some(777));
        assert_eq!(
            terminal.phase,
            if expected_error.is_some() {
                crate::DiagnosticPhase::Failed
            } else {
                crate::DiagnosticPhase::Succeeded
            }
        );
        assert_eq!(terminal.detail, expected_error);
        assert_eq!(records[0].phase, crate::DiagnosticPhase::Started);
        assert_eq!(records[1].phase, crate::DiagnosticPhase::Progress);
        assert_eq!(records.len(), 3);
        let json = serde_json::to_string(&*records).unwrap();
        assert!(!json.contains("raw_bytes"));
        assert!(!json.contains("1726"));
        assert!(!json.contains("65,12,26,248"));
        assert_eq!(
            *state.requests.lock().unwrap(),
            [
                WorkerRequest::Connect {
                    provider_id: "opta".into()
                },
                WorkerRequest::DiscoverMode01,
                WorkerRequest::ReadMode01 { pid: 0x0C }
            ]
        );
    }
}
