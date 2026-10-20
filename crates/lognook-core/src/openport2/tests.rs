use super::unexpected_worker_response;
use super::{OpenPort2Adapter, WorkerSession, available_from_session, identify_from_session};
use crate::{
    DiagnosticOutcome, DiagnosticReadKind, DiagnosticRequest, EngineEvent, HardwareAdapter,
    LogNookEngine, OemCandidate, OemCandidateOutcome, OemCandidateTrySpec, OemPollPlan,
    ResearchTestRequest, ResearchTestResult, Sample, SignalDefinition, standard_obd_signals,
};
use lognook_j2534_protocol::{
    J2534Identity, J2534Provider, Mode01Payload, Mode09Failure, Mode09MalformedFrame,
    Mode09Payload, Mode09Response, OemCandidateFailure, OemCandidatePayload,
    OemCandidateRequestKind, StandardDtcFailure, StandardDtcMalformedFrame, StandardDtcPayload,
    StandardDtcResponse, StandardDtcService, SubaruSsmReadPayload, WorkerErrorKind, WorkerRequest,
    WorkerResponse,
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

fn standard_dtc_payload(ecu_id: &str, bytes: &[u8]) -> StandardDtcResponse {
    StandardDtcResponse::Payload(StandardDtcPayload {
        ecu_id: ecu_id.into(),
        bytes: bytes.to_vec(),
        latency_us: 12_500,
    })
}

fn oem_candidate(service: &str, identifier: &str) -> OemCandidateTrySpec {
    OemCandidateTrySpec {
        source_policy: crate::OemCandidateSourcePolicy::IndexedPack,
        value_format: "uint".into(),
        candidate: OemCandidate {
            candidate_id: "ford-flex-mode21-01-test".into(),
            source_id: "OBDB_FORD_TEST".into(),
            family: "Ford".into(),
            brand: "Ford".into(),
            model_scope: "Flex 2011".into(),
            service: service.into(),
            identifier: identifier.into(),
            request_header: "7E0".into(),
            response_header: "7E8".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "Engine".into(),
            name: "Engine speed test candidate".into(),
            signal_source_id: "test".into(),
            bit_offset: Some(0),
            bit_length: Some(8),
            formula: "x".into(),
            decoder_json: r#"{"bix":0,"len":8,"unit":"rpm"}"#.into(),
            unit: "rpm".into(),
            year_filter_json: "{}".into(),
            ecu_filter: "7E8".into(),
            source_blob_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            research_state: "community_reference".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "research".into(),
            notes: String::new(),
            suggested_metric: "engine_rpm".into(),
            source_scope: "test fixture".into(),
            applicability_specificity: 1,
            min_value: None,
            max_value: None,
            source_url: Some("https://example.test/ford".into()),
            license: Some("CC BY-SA 4.0".into()),
            license_url: Some("https://creativecommons.org/licenses/by-sa/4.0/".into()),
        },
    }
}

fn subaru_ssm_candidate() -> OemCandidateTrySpec {
    let catalog_sha256 = "a".repeat(64);
    OemCandidateTrySpec {
        source_policy: crate::OemCandidateSourcePolicy::LocalSubaruCatalog {
            catalog_sha256: catalog_sha256.clone(),
        },
        value_format: "uint".into(),
        candidate: OemCandidate {
            candidate_id: format!("subaru-candidate_{}", "b".repeat(64)),
            source_id: "SUBARU_OEM_40_01B_ABCD1234".into(),
            family: "Subaru OEM catalog".into(),
            brand: "Subaru".into(),
            model_scope: "WRX".into(),
            service: "SSM-KWP".into(),
            identifier: "00000E".into(),
            request_header: "$10 $20".into(),
            response_header: "F0".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "12000_DTM.xml".into(),
            name: "Engine speed".into(),
            signal_source_id: "test".into(),
            bit_offset: Some(0),
            bit_length: Some(8),
            formula: "raw".into(),
            decoder_json:
                r#"{"bix":0,"len":8,"mul":1,"div":1,"add":0,"unit":"rpm","read_length":1}"#.into(),
            unit: "rpm".into(),
            year_filter_json: "[]".into(),
            ecu_filter: "REQUIRES_INDIVIDUAL_ECU_VERIFICATION".into(),
            source_blob_sha: String::new(),
            research_state: "review_required".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "inferred".into(),
            notes: "local research".into(),
            suggested_metric: "engine_speed".into(),
            source_scope: format!("owner_supplied_subaru_catalog:{catalog_sha256}"),
            applicability_specificity: 0,
            min_value: Some(0.0),
            max_value: Some(255.0),
            source_url: None,
            license: None,
            license_url: None,
        },
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

#[test]
fn subaru_ssm_mode_connects_kline_without_can_pid_discovery() {
    let (session, state) = FakeSession::new([Ok(connected())]);
    let mut adapter = OpenPort2Adapter::new_subaru_ssm(PathBuf::from("worker.exe"), "opta".into())
        .with_session(Box::new(session));
    let capabilities = adapter.connect().unwrap();

    assert!(capabilities.read_only);
    assert_eq!(
        capabilities.protocol,
        "Subaru SSM2 over ISO9141 K-Line (4800 baud) via J2534"
    );
    assert!(capabilities.supported_signals.is_empty());
    assert!(adapter.read_samples(&[]).unwrap().is_empty());
    assert!(adapter.read_samples(&[rpm()]).is_err());
    assert_eq!(
        *state.requests.lock().unwrap(),
        [WorkerRequest::ConnectSubaruSsm {
            provider_id: "opta".into()
        }]
    );
}

#[test]
fn subaru_ssm_poll_uses_typed_memory_addresses_and_returns_a_live_value() {
    let payload = SubaruSsmReadPayload {
        ecu_address: 0x10,
        tool_address: 0xf0,
        addresses: vec![0x000008, 0x00000e],
        values: vec![82, 125],
        init_frame: Some("80F01002FF0182".into()),
        read_frames: vec!["80F01003E8527D3A".into()],
        latency_us: 12_500,
    };
    let (session, state) = FakeSession::new([
        Ok(connected()),
        Ok(WorkerResponse::SubaruSsmRead { payload }),
    ]);
    let mut adapter = OpenPort2Adapter::new_subaru_ssm(PathBuf::from("worker.exe"), "opta".into())
        .with_session(Box::new(session));
    adapter.connect().unwrap();
    let mut engine_speed = subaru_ssm_candidate();
    engine_speed.candidate.bit_offset = Some(8);
    let mut coolant = subaru_ssm_candidate();
    coolant.candidate.candidate_id = format!("subaru-candidate_{}", "c".repeat(64));
    coolant.candidate.identifier = "000008".into();
    coolant.candidate.name = "Coolant temperature".into();
    let result = adapter
        .execute_oem_poll_group(&[engine_speed, coolant])
        .unwrap();

    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.decoded_value.unwrap().value, Some(125.0));
    assert!(result.exact_adapter_command.contains("ISO9141 K-Line"));
    assert!(
        result
            .exact_adapter_command
            .contains("A8 00 00 00 08 00 00 0E")
    );
    assert!(result.raw_response.contains("INIT RX"));
    assert!(state.requests.lock().unwrap().iter().any(|request| matches!(
        request,
        WorkerRequest::ReadSubaruSsmAddresses { addresses, ecu_address: 0x10, tool_address: 0xf0, .. }
            if addresses == &[0x000008, 0x00000e]
    )));
    assert!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| !matches!(
                request,
                WorkerRequest::ReadSubaruCandidate { .. } | WorkerRequest::ReadOemCandidate { .. }
            ))
    );
}

#[test]
fn subaru_ssm_group_publishes_decoded_samples_through_acquisition() {
    let payload = SubaruSsmReadPayload {
        ecu_address: 0x10,
        tool_address: 0xf0,
        addresses: vec![0x000008, 0x00000e],
        values: vec![82, 125],
        init_frame: Some("80F01002FF0182".into()),
        read_frames: vec!["80F01003E8527D3A".into()],
        latency_us: 12_500,
    };
    let (session, _) = FakeSession::new([
        Ok(connected()),
        Ok(WorkerResponse::SubaruSsmRead { payload }),
    ]);
    let adapter = OpenPort2Adapter::new_subaru_ssm(PathBuf::from("worker.exe"), "opta".into())
        .with_session(Box::new(session));
    let samples = Arc::new(Mutex::new(Vec::<Sample>::new()));
    let sample_sink = samples.clone();
    let mut engine = LogNookEngine::new(move |event| {
        if let EngineEvent::Sample(sample) = event {
            sample_sink.lock().unwrap().push(sample);
        }
    });
    engine
        .connect_adapter_without_standard_signals(Box::new(adapter))
        .unwrap();
    let mut engine_speed = subaru_ssm_candidate();
    engine_speed.candidate.bit_offset = Some(8);
    let mut coolant = subaru_ssm_candidate();
    coolant.candidate.candidate_id = format!("subaru-candidate_{}", "c".repeat(64));
    coolant.candidate.identifier = "000008".into();
    coolant.candidate.name = "Coolant temperature".into();
    let expected_engine_id = format!("subaru:{}", engine_speed.candidate.candidate_id);
    let expected_coolant_id = format!("subaru:{}", coolant.candidate.candidate_id);
    engine
        .set_oem_poll_plan(Some(OemPollPlan {
            vehicle_id: "subaru-local-research".into(),
            groups: vec![vec![engine_speed, coolant]],
        }))
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while samples.lock().unwrap().len() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let samples = samples.lock().unwrap().clone();
    engine.set_oem_poll_plan(None).unwrap();
    engine.disconnect().unwrap();
    assert_eq!(samples.len(), 2);
    assert!(
        samples
            .iter()
            .any(|sample| sample.signal_id == expected_engine_id && sample.value == 125.0)
    );
    assert!(
        samples
            .iter()
            .any(|sample| sample.signal_id == expected_coolant_id && sample.value == 82.0)
    );
    assert!(
        samples
            .iter()
            .all(|sample| sample.quality == crate::SampleQuality::Suspect)
    );
    assert!(
        samples
            .iter()
            .all(|sample| sample.raw_bytes.as_deref() == Some(&[82, 125][..]))
    );
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
            WorkerRequest::ConnectSubaruSsm { .. } => "filter_setup",
            WorkerRequest::DiscoverMode01 => "mode01_discovery",
            WorkerRequest::ReadMode01 { .. } => "pid_read",
            WorkerRequest::ReadMode09 { .. } => "mode09_identity_read",
            WorkerRequest::ReadMode06 { .. } => "mode06_read",
            WorkerRequest::ReadStandardDtc { service } if *service == StandardDtcService::Clear => {
                "dtc_clear"
            }
            WorkerRequest::ReadStandardDtc { .. } => "dtc_read",
            WorkerRequest::ReadOemCandidate { .. } => "oem_candidate_read",
            WorkerRequest::ReadSubaruCandidate { .. } => "subaru_catalog_read",
            WorkerRequest::ReadSubaruSsmAddresses { .. } => "subaru_ssm_read",
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
fn openport_oem_candidate_reads_use_exact_source_route_and_shared_decoder_once() {
    let first = oem_candidate("21", "01");
    let mut second = oem_candidate("22", "1234");
    second.candidate.candidate_id = "ford-flex-mode22-1234-test".into();
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::OemCandidateResponses {
            responses: vec![lognook_j2534_protocol::OemCandidateResponse::Payload(
                OemCandidatePayload {
                    response_id: 0x7E8,
                    bytes: vec![0x61, 0x01, 0x2A],
                    latency_us: 12_500,
                },
            )],
        }),
        Ok(WorkerResponse::OemCandidateResponses {
            responses: vec![lognook_j2534_protocol::OemCandidateResponse::Payload(
                OemCandidatePayload {
                    response_id: 0x7E8,
                    bytes: vec![0x62, 0x12, 0x34, 0x2B],
                    latency_us: 17_000,
                },
            )],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    for (spec, expected_request) in [
        (
            first.clone(),
            OemCandidateRequestKind::Mode21 { identifier: 0x01 },
        ),
        (
            second.clone(),
            OemCandidateRequestKind::Mode22 { did: 0x1234 },
        ),
    ] {
        let ResearchTestResult::OemCandidate(result) = adapter
            .execute_research_test(ResearchTestRequest::OemCandidate(spec.clone()))
            .unwrap()
        else {
            panic!("OpenPort OEM request returned a non-candidate result");
        };
        assert_eq!(result.candidate_id, spec.candidate.candidate_id);
        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(result.responder_header.as_deref(), Some("7E8"));
        assert!(
            result
                .response_payload
                .starts_with(&[if spec.candidate.service == "21" {
                    0x61
                } else {
                    0x62
                }])
        );
        assert!(result.decoded_value.is_some());
        let request = state
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request, WorkerRequest::ReadOemCandidate { candidate_id, .. } if candidate_id == &spec.candidate.candidate_id))
            .cloned()
            .unwrap();
        assert_eq!(
            request,
            WorkerRequest::ReadOemCandidate {
                candidate_id: spec.candidate.candidate_id,
                source_id: spec.candidate.source_id,
                source_blob_sha: spec.candidate.source_blob_sha,
                source_url: spec.candidate.source_url.unwrap(),
                license: spec.candidate.license.unwrap(),
                license_url: spec.candidate.license_url.unwrap(),
                request_id: 0x7E0,
                response_id: 0x7E8,
                request: expected_request,
            }
        );
    }
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, WorkerRequest::ReadOemCandidate { .. }))
            .count(),
        2,
        "each candidate is requested once"
    );
    adapter.disconnect();
}

#[test]
fn openport_oem_failure_is_a_single_no_response_outcome() {
    let spec = oem_candidate("21", "01");
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::OemCandidateResponses {
            responses: vec![lognook_j2534_protocol::OemCandidateResponse::Failure(
                OemCandidateFailure {
                    request_id: 0x7E0,
                    expected_response_id: 0x7E8,
                    kind: WorkerErrorKind::NoResponse,
                    message: "No ECU response within the bounded window".into(),
                    latency_us: 2_500_000,
                },
            )],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(ResearchTestRequest::OemCandidate(spec))
        .unwrap()
    else {
        panic!("OpenPort OEM request returned a non-candidate result");
    };
    assert_eq!(result.outcome, OemCandidateOutcome::NoResponse);
    assert!(result.error.unwrap().contains("No ECU response"));
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, WorkerRequest::ReadOemCandidate { .. }))
            .count(),
        1,
        "a no-response outcome must not retry the candidate"
    );
    adapter.disconnect();
}

#[test]
fn openport_oem_pending_then_positive_preserves_both_raw_frames() {
    let spec = oem_candidate("22", "1234");
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::OemCandidateResponses {
            responses: vec![
                lognook_j2534_protocol::OemCandidateResponse::Payload(OemCandidatePayload {
                    response_id: 0x7E8,
                    bytes: vec![0x7F, 0x22, 0x78],
                    latency_us: 12_000,
                }),
                lognook_j2534_protocol::OemCandidateResponse::Payload(OemCandidatePayload {
                    response_id: 0x7E8,
                    bytes: vec![0x62, 0x12, 0x34, 0x2B],
                    latency_us: 34_000,
                }),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(ResearchTestRequest::OemCandidate(spec))
        .unwrap()
    else {
        panic!("OpenPort OEM request returned a non-candidate result");
    };
    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.response_payload, [0x62, 0x12, 0x34, 0x2B]);
    assert!(result.raw_response.contains("7E8 7F 22 78"));
    assert!(result.raw_response.contains("7E8 62 12 34 2B"));
    assert_eq!(result.latency_ms, 34);
    assert!(result.error.is_none());
    adapter.disconnect();
}

#[test]
fn openport_poll_validation_accepts_source_backed_routes_and_rejects_missing_provenance() {
    let mut invalid = oem_candidate("21", "01");
    invalid.candidate.source_url = None;
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    assert!(adapter.validate_oem_candidate_poll(&invalid).is_err());
    assert!(
        adapter
            .execute_research_test(ResearchTestRequest::OemCandidate(invalid))
            .is_err()
    );
    assert!(
        adapter
            .validate_oem_candidate_poll(&oem_candidate("21", "01"))
            .is_ok()
    );
    assert!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| !matches!(request, WorkerRequest::ReadOemCandidate { .. }))
    );
    adapter.disconnect();
}

#[test]
fn openport_standard_dtc_reads_preserve_multiple_ecus_and_use_typed_services() {
    let query = DiagnosticRequest::default()
        .queries()
        .unwrap()
        .into_iter()
        .find(|query| query.kind == DiagnosticReadKind::StoredDtcs)
        .unwrap();
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::StandardDtcResponses {
            responses: vec![
                standard_dtc_payload("7E8", &[0x43, 0x01, 0x33]),
                standard_dtc_payload("7E9", &[0x7F, 0x03, 0x22]),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    let capabilities = adapter.connect().unwrap();
    assert!(capabilities.diagnostics.dtc_snapshot);
    assert!(capabilities.diagnostics.identity);
    assert!(capabilities.diagnostics.mode06);
    assert!(capabilities.diagnostics.clear_dtcs);

    let ResearchTestResult::Diagnostic(report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(query))
        .unwrap()
    else {
        panic!("OpenPort DTC query returned a non-diagnostic result");
    };
    assert_eq!(report.operation_kind, crate::DiagnosticReportKind::Snapshot);
    assert_eq!(report.reads.len(), 2);
    assert_eq!(report.reads[0].kind, DiagnosticReadKind::StoredDtcs);
    assert_eq!(report.reads[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(report.reads[0].response_payload, [0x43, 0x01, 0x33]);
    assert_eq!(report.reads[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(report.reads[1].ecu_id.as_deref(), Some("7E9"));
    assert_eq!(report.reads[1].response_payload, [0x7F, 0x03, 0x22]);
    assert_eq!(
        report.reads[1].outcome,
        DiagnosticOutcome::TemporarilyUnavailable
    );
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(
                request,
                WorkerRequest::ReadStandardDtc {
                    service: StandardDtcService::Stored
                }
            ))
            .count(),
        1
    );
    adapter.disconnect();
}

#[test]
fn openport_clear_is_one_separate_typed_mode04_operation() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::StandardDtcResponses {
            responses: vec![standard_dtc_payload("7E8", &[0x44])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let reads = adapter.clear_vehicle_dtcs().unwrap();

    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].kind, DiagnosticReadKind::ClearDtcs);
    assert_eq!(reads[0].service, 0x04);
    assert_eq!(reads[0].request_payload, [0x04]);
    assert_eq!(reads[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(
        reads[0].values,
        [crate::DiagnosticValue::DtcClearAcknowledgement]
    );
    assert_eq!(reads[0].response_payload, [0x44]);
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(
                request,
                WorkerRequest::ReadStandardDtc {
                    service: StandardDtcService::Clear
                }
            ))
            .count(),
        1,
        "clear must issue exactly one typed worker operation"
    );
    adapter.disconnect();
}

#[test]
fn openport_clear_negative_nrc_is_not_success_and_is_not_retried() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::StandardDtcResponses {
            responses: vec![standard_dtc_payload("7E8", &[0x7F, 0x04, 0x22])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    let reads = adapter.clear_vehicle_dtcs().unwrap();

    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].kind, DiagnosticReadKind::ClearDtcs);
    assert_eq!(reads[0].outcome, DiagnosticOutcome::TemporarilyUnavailable);
    assert_eq!(reads[0].negative_response_code, Some(0x22));
    assert_eq!(reads[0].response_payload, [0x7F, 0x04, 0x22]);
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(
                request,
                WorkerRequest::ReadStandardDtc {
                    service: StandardDtcService::Clear
                }
            ))
            .count(),
        1,
        "negative Mode 04 response must not trigger a retry"
    );
    adapter.disconnect();
}

#[test]
fn openport_dtc_read_keeps_malformed_frame_and_partial_worker_failure() {
    let query = DiagnosticRequest::default()
        .queries()
        .unwrap()
        .into_iter()
        .find(|query| query.kind == DiagnosticReadKind::StoredDtcs)
        .unwrap();
    let raw = vec![0x00, 0x00, 0x07, 0xE9, 0x43, 0x01];
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::StandardDtcResponses {
            responses: vec![
                standard_dtc_payload("7E8", &[0x43, 0x01, 0x33]),
                StandardDtcResponse::MalformedFrame(StandardDtcMalformedFrame {
                    ecu_id: Some("7E9".into()),
                    raw_bytes: raw.clone(),
                    latency_us: 13_000,
                    protocol_id: 6,
                    rx_status: 0,
                    declared_data_size: raw.len() as u32,
                    detail: "malformed DTC data fixture".into(),
                }),
                StandardDtcResponse::Failure(StandardDtcFailure {
                    kind: WorkerErrorKind::PassThruFailed,
                    message: "read failed after partial DTC responses".into(),
                    latency_us: 14_000,
                }),
            ],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    let ResearchTestResult::Diagnostic(report) = adapter
        .execute_research_test(ResearchTestRequest::Diagnostic(query))
        .unwrap()
    else {
        panic!("OpenPort DTC query returned a non-diagnostic result");
    };

    assert_eq!(report.reads.len(), 3);
    assert_eq!(report.reads[0].outcome, DiagnosticOutcome::Positive);
    assert_eq!(
        report.reads[1].outcome,
        DiagnosticOutcome::MalformedResponse
    );
    assert_eq!(report.reads[1].ecu_id.as_deref(), Some("7E9"));
    assert_eq!(report.reads[1].response_payload, raw);
    assert_eq!(report.reads[2].outcome, DiagnosticOutcome::TransportError);
    assert!(
        report.reads[2]
            .detail
            .as_deref()
            .unwrap()
            .contains("partial DTC responses")
    );
    adapter.disconnect();
}

#[test]
fn research_diagnostic_read_rejects_mode04_before_worker_io() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    assert!(
        adapter
            .execute_research_test(ResearchTestRequest::Diagnostic(
                crate::DiagnosticQueryRequest::clear_dtcs()
            ))
            .is_err()
    );
    assert!(
        !state
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| matches!(
                request,
                WorkerRequest::ReadStandardDtc {
                    service: StandardDtcService::Clear
                }
            ))
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
fn openport_rejects_clear_diagnostic_queries_before_worker_io() {
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();

    for query in [crate::DiagnosticQueryRequest::clear_dtcs()] {
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

#[test]
fn openport_mode06_bitmap_and_monitor_use_typed_reads_and_preserve_raw_values() {
    let queries = [
        crate::DiagnosticQueryRequest {
            kind: DiagnosticReadKind::Mode06Bitmap,
            service: 6,
            identifier: Some(0),
            payload: vec![6, 0],
            wait_ms: 600,
        },
        crate::DiagnosticQueryRequest {
            kind: DiagnosticReadKind::Mode06Monitor,
            service: 6,
            identifier: Some(0x35),
            payload: vec![6, 0x35],
            wait_ms: 600,
        },
    ];
    let payloads = [
        vec![0x46, 0, 0x80, 0, 0, 1],
        vec![0x46, 0x35, 0x0A, 1, 0, 100, 0, 16, 0, 200],
    ];
    let (mut adapter, state) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode06Responses {
            responses: vec![mode09_payload("7E8", &payloads[0])],
        }),
        Ok(WorkerResponse::Mode06Responses {
            responses: vec![mode09_payload("7E8", &payloads[1])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    assert!(adapter.connect().unwrap().diagnostics.mode06);
    for (query, payload) in queries.into_iter().zip(payloads) {
        let ResearchTestResult::Diagnostic(report) = adapter
            .execute_research_test(ResearchTestRequest::Diagnostic(query))
            .unwrap()
        else {
            panic!("diagnostic result expected");
        };
        assert_eq!(report.reads[0].outcome, DiagnosticOutcome::Positive);
        assert_eq!(report.reads[0].response_payload, payload);
        assert_eq!(report.reads[0].ecu_id.as_deref(), Some("7E8"));
        assert!(!report.reads[0].values.is_empty());
    }
    assert_eq!(
        state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, WorkerRequest::ReadMode06 { .. }))
            .count(),
        2
    );
    assert!(
        state
            .requests
            .lock()
            .unwrap()
            .contains(&WorkerRequest::ReadMode06 { test_id: 0x35 })
    );
    adapter.disconnect();
}

#[test]
fn mode06_negative_and_wrong_worker_type_do_not_become_positive_data() {
    let query = crate::DiagnosticQueryRequest {
        kind: DiagnosticReadKind::Mode06Bitmap,
        service: 6,
        identifier: Some(0),
        payload: vec![6, 0],
        wait_ms: 600,
    };
    let (mut adapter, _) = adapter([
        Ok(connected()),
        Ok(discovery()),
        Ok(WorkerResponse::Mode06Responses {
            responses: vec![mode09_payload("7E8", &[0x7F, 6, 0x11])],
        }),
        Ok(WorkerResponse::Mode09Responses {
            responses: vec![mode09_payload("7E8", &[0x46, 0, 0x80, 0, 0, 0])],
        }),
        Ok(WorkerResponse::Disconnected),
    ]);
    adapter.connect().unwrap();
    for expected in [
        DiagnosticOutcome::NegativeResponse,
        DiagnosticOutcome::TransportError,
    ] {
        let ResearchTestResult::Diagnostic(report) = adapter
            .execute_research_test(ResearchTestRequest::Diagnostic(query.clone()))
            .unwrap()
        else {
            panic!("diagnostic report expected");
        };
        assert_eq!(report.reads[0].outcome, expected);
        assert!(report.reads[0].values.is_empty());
    }
    adapter.disconnect();
}
