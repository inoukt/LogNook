use super::unexpected_worker_response;
use super::{OpenPort2Adapter, WorkerSession, available_from_session, identify_from_session};
use crate::{
    DiagnosticOutcome, DiagnosticReadKind, DiagnosticRequest, EngineEvent, HardwareAdapter,
    LogNookEngine, ResearchTestRequest, ResearchTestResult, SignalDefinition, standard_obd_signals,
};
use lognook_j2534_protocol::{
    J2534Identity, J2534Provider, Mode01Payload, Mode09Failure, Mode09MalformedFrame,
    Mode09Payload, Mode09Response, WorkerErrorKind, WorkerRequest, WorkerResponse,
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

fn mode09_payload(ecu_id: &str, bytes: &[u8]) -> Mode09Response {
    Mode09Response::Payload(Mode09Payload {
        ecu_id: ecu_id.into(),
        bytes: bytes.to_vec(),
        latency_us: 12_500,
    })
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

fn coolant() -> SignalDefinition {
    standard_obd_signals()
        .into_iter()
        .find(|signal| signal.id == "coolant_temp")
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
    assert_eq!(adapter.sample_interval(), Duration::ZERO);
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
        assert!(adapter.read_samples(&[rpm()]).unwrap().is_empty());
        assert!(
            adapter.connected,
            "a signal-level failure keeps the J2534 session open"
        );
    }
}

#[test]
fn one_bad_pid_keeps_good_openport_samples_in_the_same_poll() {
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x41, 0x0C, 0x1A, 0xF8])],
        }),
        Ok(WorkerResponse::Mode01Responses {
            responses: vec![payload("7E8", &[0x7F, 0x01, 0x12])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let samples = adapter.read_samples(&[rpm(), coolant()]).unwrap();

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].signal_id, "engine_rpm");
    assert_eq!(samples[0].value, 1726.0);
    assert!(adapter.connected);
    adapter.disconnect();
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
            WorkerRequest::ReadMode09 { .. } => "mode09_identity_read",
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
            consecutive_failures: None,
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
        ["provider_resolution", "provider_inventory", "version_read"]
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
                consecutive_failures: None,
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

#[test]
fn discovery_result_observations_preserve_ids_and_safe_reasons_without_paths() {
    let providers = vec![J2534Provider {
        id: "opta".into(),
        name: "Tactrix OpenPort 2.0".into(),
    }];
    let diagnostics = vec!["Ignored registered J2534 provider 'DENSO' (DST-010): LogNook supports Tactrix/OpenPort/OPTA only.".into(), "Ignored registered provider 'OPTA': its FunctionLibrary path could not be resolved or the DLL is missing.".into(), "Ignored registered provider 'OpenPort': it has no FunctionLibrary path.".into(), "No 32-bit J2534 04.04 registry entries were found.".into(), "Private provider detail C:\\Users\\private-user\\secret.dll raw 41 0C 1A F8".into()];
    let (mut session, state) = FakeSession::new([Ok(WorkerResponse::Providers {
        providers: providers.clone(),
        diagnostics: diagnostics.clone(),
    })]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let sink: crate::DiagnosticSink = Arc::new(move |event| captured.lock().unwrap().push(event));
    let result = super::available_from_session_with_diagnostics(&mut session, &sink).unwrap();
    assert_eq!(result.providers, providers);
    assert_eq!(result.diagnostics, diagnostics);
    let events = events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.adapter_id.as_deref() == Some("opta") && e.stage == "provider_found")
    );
    let detail = serde_json::to_string(&*events).unwrap();
    assert!(detail.contains("DENSO"));
    assert!(detail.contains("not an OpenPort"));
    assert!(detail.contains("DLL is missing"));
    assert!(detail.contains("no FunctionLibrary"));
    assert!(detail.contains("No 32-bit J2534"));
    assert!(!detail.contains("private-user"));
    assert!(!detail.contains("secret.dll"));
    assert!(!detail.contains("41 0C 1A F8"));
    assert_eq!(
        *state.requests.lock().unwrap(),
        [WorkerRequest::ListProviders]
    );
}
#[test]
fn discovery_result_observations_are_bounded_without_changing_returned_inventory() {
    let providers = (0..100)
        .map(|i| J2534Provider {
            id: format!("opta-{i}"),
            name: "OPTA".into(),
        })
        .collect::<Vec<_>>();
    let (mut session, _) = FakeSession::new([Ok(WorkerResponse::Providers {
        providers,
        diagnostics: vec![],
    })]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let sink: crate::DiagnosticSink = Arc::new(move |event| captured.lock().unwrap().push(event));
    assert_eq!(
        super::available_from_session_with_diagnostics(&mut session, &sink)
            .unwrap()
            .providers
            .len(),
        100
    );
    let events = events.lock().unwrap();
    assert!(!events.is_empty());
    assert!(events.len() <= 65);
    assert!(events.iter().any(|e| {
        e.detail
            .as_ref()
            .is_some_and(|s| s.contains("36 entries omitted"))
    }));
}

#[test]
fn openport_identity_read_keeps_responder_route_raw_bytes_and_latency() {
    let vin = b"1HGCM82633A004352";
    let mut payload = vec![0x49, 0x02, 0x01];
    payload.extend_from_slice(vin);
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode09Responses {
            responses: vec![
                mode09_payload("7E8", &payload),
                mode09_payload("7E9", &payload),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    let query = DiagnosticRequest::identity_queries().pop_front().unwrap();

    let ResearchTestResult::Diagnostic(report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(query))
        .unwrap()
    else {
        panic!("OpenPort identity request returned a non-diagnostic result");
    };

    assert_eq!(report.reads.len(), 2);
    assert_eq!(report.reads[0].kind, DiagnosticReadKind::Vin);
    assert_eq!(report.reads[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(report.reads[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(report.reads[0].route, "7DF (functional)");
    assert_eq!(report.reads[0].response_payload, payload);
    assert_eq!(report.reads[0].latency_ms, 12);
    assert!(report.reads[0].exact_adapter_command.contains("7DF 09 02"));
    assert_eq!(report.reads[1].ecu_id.as_deref(), Some("7E9"));
    assert_eq!(report.reads[1].response_payload, payload);
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, WorkerRequest::ReadMode09 { pid: 0x02 }))
            .count(),
        1
    );
    adapter.disconnect();
}

#[test]
fn stale_or_wrong_mode_response_is_malformed_and_keeps_its_raw_bytes_separate() {
    let mut vin = vec![0x49, 0x02, 0x01];
    vin.extend_from_slice(b"1HGCM82633A004352");
    let stale = vec![0x41, 0x0C, 0x1A, 0xF8];
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode09Responses {
            responses: vec![mode09_payload("7E8", &stale), mode09_payload("7E8", &vin)],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    let query = DiagnosticRequest::identity_queries().pop_front().unwrap();

    let ResearchTestResult::Diagnostic(report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(query))
        .unwrap()
    else {
        panic!("OpenPort identity request returned a non-diagnostic result");
    };

    assert_eq!(
        report.reads[0].outcome,
        DiagnosticOutcome::MalformedResponse
    );
    assert_eq!(report.reads[0].response_payload, stale);
    assert_eq!(report.reads[1].outcome, DiagnosticOutcome::Positive);
    assert_eq!(report.reads[1].response_payload, vin);
    adapter.disconnect();
}

#[test]
fn malformed_j2534_peer_keeps_full_frame_between_positive_vin_responders() {
    let mut vin_payload = vec![0x49, 0x02, 0x01];
    vin_payload.extend_from_slice(b"1HGCM82633A004352");
    let mut raw_frame = vec![0x00, 0x00, 0x07, 0xE9];
    raw_frame.extend_from_slice(&[0x49; 28]);
    let declared_data_size = raw_frame.len() as u32;
    let malformed = Mode09Response::MalformedFrame(Mode09MalformedFrame {
        ecu_id: None,
        raw_bytes: raw_frame.clone(),
        latency_us: 8_000,
        protocol_id: 0x05,
        rx_status: 0,
        declared_data_size,
        detail: "Malformed J2534 Mode 09 frame: message is not ISO15765 (ProtocolID=0x00000005, RxStatus=0x00000000, DataSize=32)".into(),
    });
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode09Responses {
            responses: vec![
                mode09_payload("7E8", &vin_payload),
                malformed,
                mode09_payload("7EA", &vin_payload),
                Mode09Response::Failure(Mode09Failure {
                    kind: WorkerErrorKind::PassThruFailed,
                    message: "PassThruReadMsgs returned 0x00000001 after partial replies".into(),
                    latency_us: 18_000,
                }),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    let query = DiagnosticRequest::identity_queries().pop_front().unwrap();

    let ResearchTestResult::Diagnostic(report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(query))
        .unwrap()
    else {
        panic!("OpenPort identity request returned a non-diagnostic result");
    };

    assert_eq!(report.reads.len(), 4);
    assert_eq!(report.reads[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(report.reads[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(report.reads[0].response_payload, vin_payload);
    assert_eq!(
        report.reads[1].outcome,
        DiagnosticOutcome::MalformedResponse
    );
    assert_eq!(
        report.reads[1].ecu_id, None,
        "invalid protocol must not invent a responder"
    );
    assert_eq!(report.reads[1].response_payload, raw_frame);
    assert!(report.reads[1].response_payload.len() > 16);
    assert!(
        report.reads[1].values.is_empty(),
        "malformed data cannot decode as a VIN"
    );
    assert!(
        report.reads[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("ProtocolID=0x00000005")
    );
    assert!(
        report.reads[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("7DF (functional)")
    );
    assert_eq!(report.reads[2].outcome, DiagnosticOutcome::Positive);
    assert_eq!(report.reads[2].ecu_id.as_deref(), Some("7EA"));
    assert_eq!(report.reads[2].response_payload, vin_payload);
    assert_eq!(report.reads[3].outcome, DiagnosticOutcome::TransportError);
    assert!(
        report.reads[3]
            .detail
            .as_deref()
            .unwrap()
            .contains("partial replies")
    );
    adapter.disconnect();
}

#[test]
fn unavailable_identity_pid_does_not_hide_later_identity_read() {
    let mut queries = DiagnosticRequest::identity_queries();
    let vin = queries.pop_front().unwrap();
    let calibration = queries.pop_front().unwrap();
    let mut calibration_payload = vec![0x49, 0x04, 0x01];
    calibration_payload.extend_from_slice(b"CALIBRATIONID123");
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Error {
            kind: WorkerErrorKind::NoResponse,
            message: "No response for standard Mode 09 PID 02 after 1500 ms".into(),
        }),
        Ok(WorkerResponse::Mode09Responses {
            responses: vec![mode09_payload("7E8", &calibration_payload)],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let ResearchTestResult::Diagnostic(vin_report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(vin))
        .unwrap()
    else {
        panic!("OpenPort VIN request returned a non-diagnostic result");
    };
    let ResearchTestResult::Diagnostic(calibration_report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(calibration))
        .unwrap()
    else {
        panic!("OpenPort calibration request returned a non-diagnostic result");
    };

    assert_eq!(vin_report.reads[0].outcome, DiagnosticOutcome::NoResponse);
    assert_eq!(
        calibration_report.reads[0].outcome,
        DiagnosticOutcome::Positive
    );
    assert_eq!(
        calibration_report.reads[0].response_payload,
        calibration_payload
    );
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, WorkerRequest::ReadMode09 { .. }))
            .count(),
        2
    );
    adapter.disconnect();
}

#[test]
fn negative_and_malformed_mode09_outcomes_keep_their_typed_results() {
    let mut queries = DiagnosticRequest::identity_queries();
    let vin = queries.pop_front().unwrap();
    let calibration = queries.pop_front().unwrap();
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode09Responses {
            responses: vec![mode09_payload("7E8", &[0x7F, 0x09, 0x12])],
        }),
        Ok(WorkerResponse::Error {
            kind: WorkerErrorKind::MalformedResponse,
            message: "J2534 returned an invalid ISO15765 Mode 09 response".into(),
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let ResearchTestResult::Diagnostic(negative) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(vin))
        .unwrap()
    else {
        panic!("OpenPort VIN request returned a non-diagnostic result");
    };
    let ResearchTestResult::Diagnostic(malformed) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(calibration))
        .unwrap()
    else {
        panic!("OpenPort calibration request returned a non-diagnostic result");
    };

    assert_eq!(
        negative.reads[0].outcome,
        DiagnosticOutcome::NegativeResponse
    );
    assert_eq!(negative.reads[0].negative_response_code, Some(0x12));
    assert_eq!(negative.reads[0].response_payload, [0x7F, 0x09, 0x12]);
    assert_eq!(
        malformed.reads[0].outcome,
        DiagnosticOutcome::MalformedResponse
    );
    assert!(
        malformed.reads[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("invalid ISO15765")
    );
    adapter.disconnect();
}

#[test]
fn openport_rejects_dtc_and_mode06_diagnostic_queries_before_worker_io() {
    let queries = DiagnosticRequest::default().queries().unwrap();
    let stored_dtcs = queries
        .iter()
        .find(|query| query.kind == DiagnosticReadKind::StoredDtcs)
        .unwrap()
        .clone();
    let mode06 = queries
        .iter()
        .find(|query| query.kind == DiagnosticReadKind::Mode06Bitmap)
        .unwrap()
        .clone();
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    for query in [stored_dtcs, mode06] {
        assert!(
            adapter
                .execute_research_test(ResearchTestRequest::Diagnostic(query))
                .is_err()
        );
    }
    assert_eq!(
        *state.requests.lock().unwrap(),
        [
            WorkerRequest::Connect {
                provider_id: "opta".into()
            },
            WorkerRequest::DiscoverMode01,
        ]
    );
    adapter.disconnect();
}
