mod worker_client;

use crate::{
    DiagnosticQueryRequest, DiagnosticRead, DiagnosticReadKind, DiagnosticReport,
    DiagnosticRequest, DiagnosticSink, HardwareAdapter, OemCandidateOutcome, OemCandidateResponse,
    OemCandidateTryResult, OemCandidateTrySpec, ResearchTestRequest, ResearchTestResult,
    adapter::collect_samples_independently,
    build_oem_candidate_command,
    communication_trace::CommunicationSink,
    decode_oem_candidate_payload,
    diagnostics::{diagnostic_stage, noop_diagnostic_sink},
    model::{
        ConnectionCapabilities, DeviceDescriptor, LogNookError, Sample, SampleOrigin,
        SampleQuality, SignalDefinition,
    },
    obdlink_sx_protocol::{
        SupportedPidRange, decode_signal, parse_mode01_payload, parse_supported_pid_payload,
    },
    simulator::standard_obd_signals,
    vehicle_diagnostics::{malformed_response, now_unix_ms},
};
use lognook_j2534_protocol::{
    J2534Identity, J2534ProviderDiscovery, Mode01Payload, Mode09Failure, Mode09MalformedFrame,
    Mode09Payload, Mode09Response, OemCandidateMalformedFrame, OemCandidateRequestKind,
    OemCandidateResponse as WorkerOemCandidateResponse, StandardDtcMalformedFrame,
    StandardDtcResponse, StandardDtcService, WorkerErrorKind, WorkerRequest, WorkerResponse,
    validate_oem_candidate_request_fields,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use worker_client::WorkerClient;

trait WorkerSession: Send {
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, LogNookError>;
    fn set_communication_sink(&mut self, _sink: CommunicationSink) {}
    fn request_with_progress(
        &mut self,
        request: WorkerRequest,
        _sink: &DiagnosticSink,
    ) -> Result<WorkerResponse, LogNookError> {
        self.request(request)
    }
    fn shutdown(&mut self);
}

pub fn available_openport2_providers(
    worker_path: &Path,
) -> Result<J2534ProviderDiscovery, LogNookError> {
    available_openport2_providers_with_diagnostics(worker_path, noop_diagnostic_sink())
}

pub fn available_openport2_providers_with_diagnostics(
    worker_path: &Path,
    sink: DiagnosticSink,
) -> Result<J2534ProviderDiscovery, LogNookError> {
    available_openport2_providers_with_sinks(worker_path, sink, None)
}

pub fn available_openport2_providers_with_sinks(
    worker_path: &Path,
    sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
) -> Result<J2534ProviderDiscovery, LogNookError> {
    let mut session = WorkerClient::spawn(worker_path)?;
    if let Some(communication_sink) = communication_sink {
        session.set_communication_sink(communication_sink);
    }
    let result = available_from_session_with_diagnostics(&mut session, &sink);
    session.shutdown();
    result
}

pub fn identify_openport2(
    worker_path: &Path,
    provider_id: &str,
) -> Result<DeviceDescriptor, LogNookError> {
    identify_openport2_with_diagnostics(worker_path, provider_id, noop_diagnostic_sink())
}

pub fn identify_openport2_with_diagnostics(
    worker_path: &Path,
    provider_id: &str,
    sink: DiagnosticSink,
) -> Result<DeviceDescriptor, LogNookError> {
    identify_openport2_with_sinks(worker_path, provider_id, sink, None)
}

pub fn identify_openport2_with_sinks(
    worker_path: &Path,
    provider_id: &str,
    sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
) -> Result<DeviceDescriptor, LogNookError> {
    let mut session = WorkerClient::spawn(worker_path)?;
    if let Some(communication_sink) = communication_sink {
        session.set_communication_sink(communication_sink);
    }
    let result = identify_from_session_with_diagnostics(&mut session, provider_id, &sink);
    session.shutdown();
    result
}

#[cfg(test)]
fn available_from_session(
    session: &mut dyn WorkerSession,
) -> Result<J2534ProviderDiscovery, LogNookError> {
    available_from_session_with_diagnostics(session, &noop_diagnostic_sink())
}

fn available_from_session_with_diagnostics(
    session: &mut dyn WorkerSession,
    sink: &DiagnosticSink,
) -> Result<J2534ProviderDiscovery, LogNookError> {
    match session.request_with_progress(WorkerRequest::ListProviders, sink)? {
        WorkerResponse::Providers {
            providers,
            diagnostics,
        } => {
            let discovery = J2534ProviderDiscovery {
                providers,
                diagnostics,
            };
            emit_provider_inventory(&discovery, sink);
            Ok(discovery)
        }
        response => Err(unexpected_worker_response("provider listing", response)),
    }
}

const MAX_PROVIDER_OBSERVATIONS: usize = 64;

fn safe_provider_label(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 128
        && value.chars().all(|c| {
            c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')' | '{' | '}' | '&')
        }))
    .then(|| value.to_owned())
}

fn provider_explanation(message: &str) -> (Option<String>, String) {
    for known in [
        "No 32-bit J2534 04.04 registry entries were found.",
        "No registered J2534 provider is eligible for OpenPort 2 connection.",
    ] {
        if message == known {
            return (None, known.into());
        }
    }
    if let Some(rest) = message.strip_prefix("Ignored registered J2534 provider '")
        && let Some((name, rest)) = rest.split_once("' (")
        && let Some(id) = rest.strip_suffix("): LogNook supports Tactrix/OpenPort/OPTA only.")
    {
        let name = safe_provider_label(name).unwrap_or_else(|| "[provider name redacted]".into());
        return (
            safe_provider_label(id),
            format!("Ignored {name}: not an OpenPort/Tactrix/OPTA provider."),
        );
    }
    if let Some(rest) = message.strip_prefix("Ignored registered provider '")
        && let Some((name, reason)) = rest.rsplit_once("': ")
    {
        let reason = match reason {
            "it has no FunctionLibrary path." => Some("no FunctionLibrary entry."),
            "its FunctionLibrary path could not be resolved or the DLL is missing." => {
                Some("FunctionLibrary could not be resolved or the DLL is missing.")
            }
            _ => None,
        };
        if let Some(reason) = reason {
            return (
                None,
                format!(
                    "Ignored {}: {reason}",
                    safe_provider_label(name).unwrap_or_else(|| "[provider name redacted]".into())
                ),
            );
        }
    }
    (
        None,
        "An unrecognized provider discovery explanation was omitted.".into(),
    )
}

fn emit_provider_inventory(discovery: &J2534ProviderDiscovery, sink: &DiagnosticSink) {
    let emit = |adapter_id, stage: &str, detail| {
        sink(crate::DiagnosticProgress {
            adapter_id,
            operation: "provider_discovery".into(),
            stage: stage.into(),
            phase: crate::DiagnosticPhase::Progress,
            duration_ms: None,
            signal_ids: vec![],
            detail: Some(detail),
            transient: None,
            consecutive_failures: None,
        })
    };
    let mut emitted = 0;
    for provider in discovery.providers.iter().take(MAX_PROVIDER_OBSERVATIONS) {
        emit(
            safe_provider_label(&provider.id),
            "provider_found",
            format!(
                "Discovered registered J2534 provider {}.",
                safe_provider_label(&provider.name)
                    .unwrap_or_else(|| "[provider name redacted]".into())
            ),
        );
        emitted += 1;
    }
    for explanation in discovery
        .diagnostics
        .iter()
        .take(MAX_PROVIDER_OBSERVATIONS - emitted)
    {
        let (adapter_id, detail) = provider_explanation(explanation);
        emit(adapter_id, "provider_explanation", detail);
        emitted += 1;
    }
    let omitted = discovery.providers.len() + discovery.diagnostics.len() - emitted;
    emit(
        None,
        "provider_inventory",
        format!(
            "{} eligible providers; {} discovery explanations; {omitted} entries omitted from diagnostics.",
            discovery.providers.len(),
            discovery.diagnostics.len()
        ),
    );
}

#[cfg(test)]
fn identify_from_session(
    session: &mut dyn WorkerSession,
    provider_id: &str,
) -> Result<DeviceDescriptor, LogNookError> {
    identify_from_session_with_diagnostics(session, provider_id, &noop_diagnostic_sink())
}

fn identify_from_session_with_diagnostics(
    session: &mut dyn WorkerSession,
    provider_id: &str,
    sink: &DiagnosticSink,
) -> Result<DeviceDescriptor, LogNookError> {
    let identity = match session.request_with_progress(
        WorkerRequest::Identify {
            provider_id: provider_id.to_owned(),
        },
        sink,
    )? {
        WorkerResponse::Identity { identity } => identity,
        response => return Err(unexpected_worker_response("adapter identity", response)),
    };
    Ok(device_descriptor(provider_id, identity))
}

fn device_descriptor(provider_id: &str, identity: J2534Identity) -> DeviceDescriptor {
    DeviceDescriptor {
        stable_id: provider_id.to_owned(),
        hardware_family: "OpenPort 2.0".into(),
        firmware: Some(format!(
            "{} (DLL {}; J2534 {})",
            identity.firmware, identity.dll_version, identity.api_version
        )),
        path: Some(provider_id.to_owned()),
    }
}

fn unexpected_worker_response(action: &str, response: WorkerResponse) -> LogNookError {
    match response {
        WorkerResponse::Error { kind, message } => {
            worker_error(kind, format!("during {action}: {message}"))
        }
        other => LogNookError::new(format!(
            "OpenPort worker returned an unexpected response for {action}: {other:?}"
        )),
    }
}

fn worker_error(kind: WorkerErrorKind, message: String) -> LogNookError {
    LogNookError::new(format!("OpenPort {kind:?}: {message}"))
}

pub struct OpenPort2Adapter {
    worker_path: PathBuf,
    provider_id: String,
    mode: OpenPort2Mode,
    session: Option<Box<dyn WorkerSession>>,
    connected: bool,
    device: Option<DeviceDescriptor>,
    started: Option<Instant>,
    ecu_id: Option<String>,
    supported_pids: HashSet<u8>,
    diagnostic_sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OpenPort2Mode {
    #[default]
    ObdCan,
    SubaruSsm,
}

impl OpenPort2Adapter {
    pub fn new(worker_path: PathBuf, provider_id: String) -> Self {
        Self {
            worker_path,
            provider_id,
            mode: OpenPort2Mode::ObdCan,
            session: None,
            connected: false,
            device: None,
            started: None,
            ecu_id: None,
            supported_pids: HashSet::new(),
            diagnostic_sink: noop_diagnostic_sink(),
            communication_sink: None,
        }
    }

    pub fn new_subaru_ssm(worker_path: PathBuf, provider_id: String) -> Self {
        let mut adapter = Self::new(worker_path, provider_id);
        adapter.mode = OpenPort2Mode::SubaruSsm;
        adapter
    }

    #[cfg(test)]
    fn with_session(mut self, session: Box<dyn WorkerSession>) -> Self {
        self.session = Some(session);
        self
    }

    fn connect_inner(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.session.is_none() {
            let mut client = WorkerClient::spawn(&self.worker_path)?;
            if let Some(sink) = &self.communication_sink {
                client.set_communication_sink(sink.clone());
            }
            self.session = Some(Box::new(client));
        }
        let sink = contextual_sink(&self.diagnostic_sink, &self.provider_id, "connect");
        let session = self.session.as_deref_mut().expect("session was created");
        let request = match self.mode {
            OpenPort2Mode::ObdCan => WorkerRequest::Connect {
                provider_id: self.provider_id.clone(),
            },
            OpenPort2Mode::SubaruSsm => WorkerRequest::ConnectSubaruSsm {
                provider_id: self.provider_id.clone(),
            },
        };
        let identity = match session.request_with_progress(request, &sink)? {
            WorkerResponse::Connected { identity } => identity,
            response => return Err(unexpected_worker_response("connect", response)),
        };
        self.connected = true;
        let device = device_descriptor(&self.provider_id, identity);
        self.device = Some(device.clone());

        if self.mode == OpenPort2Mode::SubaruSsm {
            self.started = Some(Instant::now());
            return Ok(ConnectionCapabilities {
                device,
                protocol: "Subaru SSM2 over ISO9141 K-Line (4800 baud) via J2534".into(),
                read_only: true,
                diagnostics: crate::DiagnosticCapabilities {
                    dtc_snapshot: false,
                    identity: false,
                    mode06: false,
                    clear_dtcs: false,
                },
                supported_signals: Vec::new(),
            });
        }

        let responses = match session.request_with_progress(WorkerRequest::DiscoverMode01, &sink)? {
            WorkerResponse::Mode01Responses { responses } => responses,
            response => {
                return Err(unexpected_worker_response(
                    "Mode 01 PID discovery",
                    response,
                ));
            }
        };
        let (ecu_id, supported_pids) = parse_supported_pages(&responses)?;
        self.ecu_id = Some(ecu_id.clone());
        self.supported_pids = supported_pids;

        let supported_signals = standard_obd_signals()
            .into_iter()
            .filter(|signal| signal.service == 0x01 && self.supported_pids.contains(&signal.pid))
            .collect::<Vec<_>>();
        if supported_signals.is_empty() {
            return Err(LogNookError::new(
                "The connected ECU reported no catalogued standard Mode 01 signals",
            ));
        }
        self.started = Some(Instant::now());
        Ok(ConnectionCapabilities {
            device,
            protocol: "SAE J1979 Mode 01 over ISO 15765 (11-bit, 500 kbit/s)".into(),
            read_only: true,
            diagnostics: crate::DiagnosticCapabilities {
                dtc_snapshot: true,
                identity: true,
                mode06: true,
                clear_dtcs: true,
            },
            supported_signals,
        })
    }

    fn read_pid(&mut self, pid: u8) -> Result<Mode01Payload, LogNookError> {
        let session = self
            .session
            .as_deref_mut()
            .ok_or_else(|| LogNookError::new("OpenPort adapter is not connected"))?;
        let sink = contextual_sink(&self.diagnostic_sink, &self.provider_id, "acquisition");
        match session.request_with_progress(WorkerRequest::ReadMode01 { pid }, &sink)? {
            WorkerResponse::Mode01Responses { mut responses } if responses.len() == 1 => {
                Ok(responses.pop().expect("one response was checked"))
            }
            WorkerResponse::Mode01Responses { responses } if responses.is_empty() => {
                Err(LogNookError::transient_signal(format!(
                    "OpenPort returned no response for Mode 01 PID {pid:02X}"
                )))
            }
            WorkerResponse::Mode01Responses { .. } => Err(LogNookError::signal(
                "Multiple ECU responders are not supported by the OpenPort adapter",
            )),
            response => Err(unexpected_worker_response("Mode 01 PID read", response)),
        }
    }

    fn read_standard_pid_query(
        &mut self,
        query: DiagnosticQueryRequest,
    ) -> Result<DiagnosticReport, LogNookError> {
        query.validate().map_err(LogNookError::new)?;
        if !matches!(
            query.kind,
            DiagnosticReadKind::Vin
                | DiagnosticReadKind::CalibrationId
                | DiagnosticReadKind::EcuName
                | DiagnosticReadKind::Mode06Bitmap
                | DiagnosticReadKind::Mode06Monitor
        ) {
            return Err(LogNookError::new(
                "OpenPort standard reads support Mode 09 identity and Mode 06 tests",
            ));
        }
        if !self.connected {
            return Err(LogNookError::new("OpenPort adapter is not connected"));
        }
        let adapter = self
            .device
            .clone()
            .ok_or_else(|| LogNookError::new("OpenPort adapter identity is unavailable"))?;
        let pid = query
            .identifier
            .expect("validated identity query has a PID");
        let started_at_unix_ms = now_unix_ms();
        let started = Instant::now();
        let command = format!(
            "J2534 ISO15765 CAN 11/500 7DF {}",
            hex_bytes(query.request_payload())
        );
        let sink = contextual_sink(
            &self.diagnostic_sink,
            &self.provider_id,
            "standard_pid_read",
        );
        let request = if query.service == 0x06 {
            WorkerRequest::ReadMode06 { test_id: pid }
        } else {
            WorkerRequest::ReadMode09 { pid }
        };
        let response = self
            .session
            .as_deref_mut()
            .expect("connected OpenPort adapter has a worker session")
            .request_with_progress(request, &sink);
        let reads = match (query.service, response) {
            (0x09, Ok(WorkerResponse::Mode09Responses { responses }))
            | (0x06, Ok(WorkerResponse::Mode06Responses { responses }))
                if !responses.is_empty() =>
            {
                responses
                    .into_iter()
                    .map(|response| {
                        mode09_response_read(
                            &query,
                            response,
                            command.clone(),
                            started_at_unix_ms,
                            elapsed_ms(started),
                        )
                    })
                    .collect()
            }
            (0x09, Ok(WorkerResponse::Mode09Responses { .. }))
            | (0x06, Ok(WorkerResponse::Mode06Responses { .. })) => {
                vec![query.no_response(
                    String::new(),
                    command.clone(),
                    started_at_unix_ms,
                    elapsed_ms(started),
                    Some("OpenPort returned no standard diagnostic responders".into()),
                )]
            }
            (_, Ok(WorkerResponse::Error { kind, message })) => vec![mode09_worker_error_read(
                &query,
                command.clone(),
                started_at_unix_ms,
                elapsed_ms(started),
                kind,
                message,
            )],
            (_, Ok(response)) => vec![query.transport_error(
                format!("Unexpected worker response: {response:?}"),
                command.clone(),
                started_at_unix_ms,
                elapsed_ms(started),
                format!(
                    "OpenPort returned an unexpected response for Mode {:02X} ID {pid:02X}",
                    query.service
                ),
            )],
            (_, Err(error)) => vec![query.transport_error(
                error.to_string(),
                command.clone(),
                started_at_unix_ms,
                elapsed_ms(started),
                error.to_string(),
            )],
        };
        Ok(DiagnosticReport {
            report_id: None,
            connection_generation: None,
            vehicle_context: None,
            adapter,
            protocol: format!(
                "SAE J1979 Mode {:02X} over ISO 15765 (11-bit, 500 kbit/s) via OpenPort 2.0",
                query.service
            ),
            started_at_unix_ms,
            operation_kind: crate::DiagnosticReportKind::Snapshot,
            request: DiagnosticRequest::default(),
            reads,
            route_restore_error: None,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }

    fn read_standard_dtc_query(
        &mut self,
        query: DiagnosticQueryRequest,
        operation_kind: crate::DiagnosticReportKind,
    ) -> Result<DiagnosticReport, LogNookError> {
        query.validate().map_err(LogNookError::new)?;
        let service = match query.kind {
            DiagnosticReadKind::StoredDtcs => StandardDtcService::Stored,
            DiagnosticReadKind::PendingDtcs => StandardDtcService::Pending,
            DiagnosticReadKind::PermanentDtcs => StandardDtcService::Permanent,
            DiagnosticReadKind::ClearDtcs
                if operation_kind == crate::DiagnosticReportKind::ClearDtcs =>
            {
                StandardDtcService::Clear
            }
            _ => {
                return Err(LogNookError::new(
                    "Unsupported OpenPort standard DTC operation",
                ));
            }
        };
        if !self.connected {
            return Err(LogNookError::new("OpenPort adapter is not connected"));
        }
        let adapter = self
            .device
            .clone()
            .ok_or_else(|| LogNookError::new("OpenPort adapter identity is unavailable"))?;
        let started_at_unix_ms = now_unix_ms();
        let started = Instant::now();
        let command = format!(
            "J2534 ISO15765 CAN 11/500 7DF {}",
            hex_bytes(query.request_payload())
        );
        let sink = contextual_sink(
            &self.diagnostic_sink,
            &self.provider_id,
            "vehicle_diagnostics",
        );
        let response = self
            .session
            .as_deref_mut()
            .expect("connected OpenPort adapter has a worker session")
            .request_with_progress(WorkerRequest::ReadStandardDtc { service }, &sink);
        let fallback_latency_ms = elapsed_ms(started);
        let reads = match response {
            Ok(WorkerResponse::StandardDtcResponses { responses }) if !responses.is_empty() => {
                responses
                    .into_iter()
                    .map(|response| {
                        standard_dtc_response_read(
                            &query,
                            response,
                            command.clone(),
                            started_at_unix_ms,
                            fallback_latency_ms,
                        )
                    })
                    .collect()
            }
            Ok(WorkerResponse::StandardDtcResponses { .. }) => vec![query.no_response(
                String::new(),
                command.clone(),
                started_at_unix_ms,
                fallback_latency_ms,
                Some("OpenPort returned no standard DTC responders".into()),
            )],
            Ok(WorkerResponse::Error { kind, message }) => vec![standard_dtc_worker_error_read(
                &query,
                command.clone(),
                started_at_unix_ms,
                fallback_latency_ms,
                kind,
                message,
            )],
            Ok(response) => vec![query.transport_error(
                format!("Unexpected worker response: {response:?}"),
                command.clone(),
                started_at_unix_ms,
                fallback_latency_ms,
                format!(
                    "OpenPort returned an unexpected response for standard Mode {:02X}",
                    service.request_byte()
                ),
            )],
            Err(error) => vec![query.transport_error(
                error.to_string(),
                command.clone(),
                started_at_unix_ms,
                fallback_latency_ms,
                error.to_string(),
            )],
        };
        Ok(DiagnosticReport {
            report_id: None,
            connection_generation: None,
            vehicle_context: None,
            adapter,
            protocol: "SAE J1979 standard DTC services over ISO 15765 (11-bit, 500 kbit/s) via OpenPort 2.0".into(),
            started_at_unix_ms,
            operation_kind,
            request: DiagnosticRequest::default(),
            reads,
            route_restore_error: None,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }

    fn read_oem_candidate(
        &mut self,
        spec: OemCandidateTrySpec,
    ) -> Result<OemCandidateTryResult, LogNookError> {
        if spec.candidate.service == "SSM-KWP" {
            return self.read_subaru_ssm_group(std::slice::from_ref(&spec));
        }
        if self.mode == OpenPort2Mode::SubaruSsm {
            return Err(LogNookError::new(
                "Connect OpenPort 2 in OBD-II CAN mode for generic OEM CAN reads",
            ));
        }
        let request = oem_candidate_worker_request(&spec)?;
        if !self.connected {
            return Err(LogNookError::new("OpenPort adapter is not connected"));
        }
        let device = self
            .device
            .clone()
            .ok_or_else(|| LogNookError::new("OpenPort adapter identity is unavailable"))?;
        let (request_id, response_id, request_bytes) = oem_candidate_route_and_bytes(&request);
        let exact_adapter_command = format!(
            "J2534 ISO15765 CAN 11/500 TX {request_id:03X} {} -> RX {response_id:03X}",
            hex_bytes(&request_bytes)
        );
        let started_at_unix_ms = now_unix_ms();
        let started = Instant::now();
        let sink = contextual_sink(
            &self.diagnostic_sink,
            &self.provider_id,
            "oem_candidate_read",
        );
        let response = self
            .session
            .as_deref_mut()
            .expect("connected OpenPort adapter has a worker session")
            .request_with_progress(request, &sink);
        let fallback_latency_ms = elapsed_ms(started);
        let mut raw_response = String::new();
        let (decoded, response_latency_us) = match response {
            Ok(WorkerResponse::OemCandidateResponses { mut responses }) => {
                raw_response = responses
                    .iter()
                    .filter_map(oem_candidate_raw_response_line)
                    .collect::<Vec<_>>()
                    .join("\n");
                let primary_index = responses
                    .iter()
                    .position(|response| {
                        !oem_candidate_is_pending(response, &spec)
                            && matches!(
                                response,
                                WorkerOemCandidateResponse::Payload(_)
                                    | WorkerOemCandidateResponse::MalformedFrame(_)
                            )
                    })
                    .or_else(|| {
                        responses
                            .iter()
                            .position(|response| oem_candidate_is_pending(response, &spec))
                    })
                    .or_else(|| {
                        responses.iter().position(|response| {
                            matches!(response, WorkerOemCandidateResponse::Failure(_))
                        })
                    });
                let primary = primary_index.map(|index| responses.remove(index));
                let (mut decoded, latency_us) = match primary {
                    Some(WorkerOemCandidateResponse::Payload(payload)) => {
                        let responder = format!("{:03X}", payload.response_id);
                        (
                            decode_oem_candidate_payload(&spec, Some(&responder), &payload.bytes),
                            payload.latency_us,
                        )
                    }
                    Some(WorkerOemCandidateResponse::MalformedFrame(frame)) => {
                        let latency_us = frame.latency_us;
                        (oem_candidate_malformed(&spec, frame), latency_us)
                    }
                    Some(WorkerOemCandidateResponse::Failure(failure)) => {
                        let latency_us = failure.latency_us;
                        (
                            oem_candidate_worker_failure(&spec, failure.kind, failure.message),
                            latency_us,
                        )
                    }
                    None => (
                        oem_candidate_worker_failure(
                            &spec,
                            WorkerErrorKind::NoResponse,
                            "OpenPort returned no response for the OEM candidate".into(),
                        ),
                        0,
                    ),
                };
                for additional in responses {
                    let detail = match additional {
                        WorkerOemCandidateResponse::Payload(payload)
                            if oem_candidate_is_pending_payload(
                                &payload.bytes,
                                &spec.candidate.service,
                            ) =>
                        {
                            continue;
                        }
                        WorkerOemCandidateResponse::Failure(failure) => {
                            format!("OpenPort {:?}: {}", failure.kind, failure.message)
                        }
                        WorkerOemCandidateResponse::MalformedFrame(frame) => frame.detail,
                        WorkerOemCandidateResponse::Payload(payload) => format!(
                            "Unexpected additional OEM response from {:03X}",
                            payload.response_id
                        ),
                    };
                    decoded.error = Some(match decoded.error.take() {
                        Some(error) => format!("{error}; {detail}"),
                        None => detail,
                    });
                }
                (decoded, latency_us)
            }
            Ok(WorkerResponse::Error { kind, message }) => {
                (oem_candidate_worker_failure(&spec, kind, message), 0)
            }
            Ok(response) => (
                oem_candidate_worker_failure(
                    &spec,
                    WorkerErrorKind::MalformedResponse,
                    format!("Unexpected worker response: {response:?}"),
                ),
                0,
            ),
            Err(error) => (
                oem_candidate_worker_failure(&spec, WorkerErrorKind::NoResponse, error.to_string()),
                0,
            ),
        };
        Ok(OemCandidateTryResult {
            candidate_id: spec.candidate.candidate_id,
            exact_adapter_command,
            adapter: device,
            protocol: "ISO 15765 over J2534 (CAN 11/500) via OpenPort 2.0".into(),
            started_at_unix_ms,
            latency_ms: if response_latency_us == 0 {
                fallback_latency_ms
            } else {
                response_latency_us / 1_000
            },
            outcome: decoded.outcome,
            raw_response,
            responder_header: decoded.responder_header,
            response_payload: decoded.payload,
            negative_response_code: decoded.negative_response_code,
            decoded_value: decoded.decoded_value,
            decode_error: decoded.decode_error,
            error: decoded.error,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }

    fn read_subaru_ssm_group(
        &mut self,
        specs: &[OemCandidateTrySpec],
    ) -> Result<OemCandidateTryResult, LogNookError> {
        if self.mode != OpenPort2Mode::SubaruSsm {
            return Err(LogNookError::new(
                "Connect OpenPort 2 in Subaru SSM K-Line mode before logging SSM-KWP signals",
            ));
        }
        let first = specs
            .first()
            .ok_or_else(|| LogNookError::new("Subaru SSM graph group is empty"))?;
        let catalog_sha256 = first
            .local_subaru_catalog_sha256()
            .ok_or_else(|| LogNookError::new("Subaru SSM reads require the local catalog hash"))?
            .to_owned();
        let (ecu_address, tool_address) = crate::subaru_catalog::ssm_route(
            &first.candidate.request_header,
            &first.candidate.response_header,
        )
        .map_err(LogNookError::new)?;
        let mut addresses = BTreeSet::new();
        let mut candidate_ids = Vec::with_capacity(specs.len());
        for spec in specs {
            if spec.candidate.service != "SSM-KWP"
                || spec.candidate.source_id != first.candidate.source_id
                || spec.local_subaru_catalog_sha256() != Some(catalog_sha256.as_str())
            {
                return Err(LogNookError::new(
                    "A Subaru SSM read group must share its local catalog source and protocol",
                ));
            }
            let route = crate::subaru_catalog::ssm_route(
                &spec.candidate.request_header,
                &spec.candidate.response_header,
            )
            .map_err(LogNookError::new)?;
            if route != (ecu_address, tool_address) {
                return Err(LogNookError::new(
                    "A Subaru SSM read group cannot mix ECU addresses",
                ));
            }
            addresses.extend(
                crate::subaru_catalog::subaru_ssm_signal_addresses(spec)
                    .map_err(LogNookError::new)?,
            );
            candidate_ids.push(spec.candidate.candidate_id.clone());
        }
        let addresses = addresses.into_iter().collect::<Vec<_>>();
        if addresses.is_empty()
            || addresses.len() > lognook_j2534_protocol::MAX_SUBARU_SSM_ADDRESSES
        {
            return Err(LogNookError::new(
                "Subaru SSM graph group must read between 1 and 256 memory bytes",
            ));
        }
        if !self.connected {
            return Err(LogNookError::new("OpenPort adapter is not connected"));
        }
        let device = self
            .device
            .clone()
            .ok_or_else(|| LogNookError::new("OpenPort adapter identity is unavailable"))?;
        let request = WorkerRequest::ReadSubaruSsmAddresses {
            candidate_ids,
            source_id: first.candidate.source_id.clone(),
            catalog_sha256,
            ecu_address,
            tool_address,
            addresses: addresses.clone(),
        };
        let started_at_unix_ms = now_unix_ms();
        let started = Instant::now();
        let sink = contextual_sink(&self.diagnostic_sink, &self.provider_id, "subaru_ssm_read");
        let response = self
            .session
            .as_deref_mut()
            .expect("connected OpenPort adapter has a worker session")
            .request_with_progress(request, &sink);
        let fallback_latency_ms = elapsed_ms(started);
        let (raw_response, exact_adapter_command, response_payload, latency_ms) = match response {
            Ok(WorkerResponse::SubaruSsmRead { payload }) => {
                if payload.ecu_address != ecu_address
                    || payload.tool_address != tool_address
                    || payload.addresses != addresses
                    || payload.values.len() != addresses.len()
                {
                    return Err(LogNookError::new(
                        "OpenPort returned Subaru SSM bytes for a different route or address list",
                    ));
                }
                let mut tx_frames = Vec::new();
                if payload.init_frame.is_some() {
                    tx_frames.push(lognook_j2534_protocol::subaru_ssm::build_ecu_init(
                        ecu_address,
                        tool_address,
                    ));
                }
                for chunk in addresses.chunks(32) {
                    tx_frames.push(
                        lognook_j2534_protocol::subaru_ssm::build_read_addresses(
                            ecu_address,
                            tool_address,
                            chunk,
                        )
                        .map_err(LogNookError::new)?,
                    );
                }
                let tx_text = tx_frames
                    .iter()
                    .map(|frame| hex_bytes(frame))
                    .collect::<Vec<_>>()
                    .join("; ");
                let mut raw_frames = Vec::new();
                if let Some(frame) = payload.init_frame {
                    raw_frames.push(format!("INIT RX {frame}"));
                }
                raw_frames.extend(
                    payload
                        .read_frames
                        .into_iter()
                        .map(|frame| format!("READ RX {frame}")),
                );
                (
                    raw_frames.join("\n"),
                    format!("J2534 ISO9141 K-Line 4800 TX {tx_text}"),
                    payload.values,
                    payload.latency_us / 1_000,
                )
            }
            Ok(WorkerResponse::Error { kind, message }) => {
                return Err(worker_error(kind, message));
            }
            Ok(response) => {
                return Err(LogNookError::new(format!(
                    "Unexpected J2534 worker response for Subaru SSM read: {response:?}"
                )));
            }
            Err(error) => return Err(error),
        };
        let responder_header = format!("{tool_address:02X}");
        let decoded =
            decode_oem_candidate_payload(first, Some(&responder_header), &response_payload);
        Ok(OemCandidateTryResult {
            candidate_id: first.candidate.candidate_id.clone(),
            exact_adapter_command,
            adapter: device,
            protocol: "Subaru SSM2 over ISO9141 K-Line (4800 baud) via OpenPort 2.0".into(),
            started_at_unix_ms,
            latency_ms: if latency_ms == 0 {
                fallback_latency_ms
            } else {
                latency_ms
            },
            outcome: decoded.outcome,
            raw_response,
            responder_header: decoded.responder_header,
            response_payload: decoded.payload,
            negative_response_code: None,
            decoded_value: decoded.decoded_value,
            decode_error: decoded.decode_error,
            error: decoded.error,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }
}

impl HardwareAdapter for OpenPort2Adapter {
    fn set_diagnostic_sink(&mut self, sink: DiagnosticSink) {
        self.diagnostic_sink = sink;
    }

    fn set_communication_sink(&mut self, sink: CommunicationSink) {
        self.communication_sink = Some(sink);
    }
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.connected {
            return Err(LogNookError::new("OpenPort adapter is already connected"));
        }
        let result = self.connect_inner();
        if result.is_err() {
            self.disconnect();
        }
        result
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        if self.mode == OpenPort2Mode::SubaruSsm {
            return if signals.is_empty() {
                Ok(Vec::new())
            } else {
                Err(LogNookError::new(
                    "Subaru SSM signals are read through the selected Subaru catalog graph plan",
                ))
            };
        }
        let started = self
            .started
            .ok_or_else(|| LogNookError::new("OpenPort adapter is not connected"))?;
        let ecu_id = self
            .ecu_id
            .clone()
            .ok_or_else(|| LogNookError::new("OpenPort ECU identity is unavailable"))?;
        let catalog = standard_obd_signals();
        for signal in signals {
            if signal.service != 0x01
                || !self.supported_pids.contains(&signal.pid)
                || !catalog.iter().any(|known| {
                    known.id == signal.id
                        && known.service == signal.service
                        && known.pid == signal.pid
                })
            {
                return Err(LogNookError::new(format!(
                    "Unsupported read-only Mode 01 signal: {}",
                    signal.id
                )));
            }
        }

        let mut seen_pids = HashSet::new();
        let pids = signals
            .iter()
            .map(|signal| signal.pid)
            .filter(|pid| seen_pids.insert(*pid))
            .collect::<Vec<_>>();
        let sink = self.diagnostic_sink.clone();
        collect_samples_independently(&pids, |pid| {
            let signal_ids = signals
                .iter()
                .filter(|signal| signal.pid == *pid)
                .map(|signal| signal.id.clone())
                .collect();
            diagnostic_stage(
                &sink,
                Some(self.provider_id.clone()),
                "acquisition",
                "pid_read",
                signal_ids,
                || {
                    let payload = self.read_pid(*pid)?;
                    if !payload.ecu_id.eq_ignore_ascii_case(&ecu_id) {
                        return Err(LogNookError::new(
                            "ECU identity changed during the OpenPort session",
                        ));
                    }
                    let response = parse_mode01_payload(ecu_id.clone(), &payload.bytes, *pid)
                        .map_err(|error| LogNookError::signal(error.to_string()))?;
                    let mut pid_samples = Vec::new();
                    for signal in signals.iter().filter(|signal| signal.pid == *pid) {
                        let canonical = catalog
                            .iter()
                            .find(|known| known.id == signal.id)
                            .expect("signal was validated against catalog");
                        let value =
                            decode_signal(canonical, &response).map_err(LogNookError::signal)?;
                        let elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
                        pid_samples.push(Sample {
                            elapsed_ns,
                            wall_time_unix_ns: now_unix_ns(),
                            source_id: format!("openport2:{}", self.provider_id),
                            ecu_id: Some(ecu_id.clone()),
                            signal_id: signal.id.clone(),
                            value,
                            unit: canonical.unit.clone(),
                            quality: SampleQuality::Valid,
                            origin: SampleOrigin::Live,
                            raw_bytes: Some(response.raw_bytes.clone()),
                        });
                    }
                    Ok(pid_samples)
                },
            )
        })
    }

    fn validate_oem_candidate_poll(&self, spec: &OemCandidateTrySpec) -> Result<(), LogNookError> {
        if spec.candidate.service == "SSM-KWP" {
            if self.mode != OpenPort2Mode::SubaruSsm {
                return Err(LogNookError::new(
                    "Connect OpenPort 2 in Subaru SSM K-Line mode before selecting SSM-KWP graphs",
                ));
            }
            if spec.local_subaru_catalog_sha256().is_none() {
                return Err(LogNookError::new(
                    "SSM-KWP reads require a local Subaru catalog row and catalog hash",
                ));
            }
            crate::subaru_catalog::ssm_route(
                &spec.candidate.request_header,
                &spec.candidate.response_header,
            )
            .map_err(LogNookError::new)?;
            crate::subaru_catalog::subaru_ssm_signal_addresses(spec)
                .map(|_| ())
                .map_err(LogNookError::new)
        } else if self.mode == OpenPort2Mode::SubaruSsm {
            Err(LogNookError::new(
                "Connect OpenPort 2 in OBD-II CAN mode for Mode 21/22 logging",
            ))
        } else {
            oem_candidate_worker_request(spec).map(|_| ())
        }
    }

    fn execute_oem_poll_group(
        &mut self,
        specs: &[OemCandidateTrySpec],
    ) -> Result<OemCandidateTryResult, LogNookError> {
        if specs
            .first()
            .is_some_and(|spec| spec.candidate.service == "SSM-KWP")
        {
            self.read_subaru_ssm_group(specs)
        } else {
            let first = specs
                .first()
                .ok_or_else(|| LogNookError::new("OEM poll group is empty"))?;
            self.read_oem_candidate(first.clone())
        }
    }

    fn execute_research_test(
        &mut self,
        request: ResearchTestRequest,
    ) -> Result<ResearchTestResult, LogNookError> {
        if self.mode == OpenPort2Mode::SubaruSsm
            && !matches!(&request, ResearchTestRequest::OemCandidate(spec) if spec.candidate.service == "SSM-KWP")
        {
            return Err(LogNookError::new(
                "Subaru SSM K-Line mode accepts only selected read-only Subaru SSM catalog signals",
            ));
        }
        match request {
            ResearchTestRequest::Diagnostic(query) => match query.kind {
                DiagnosticReadKind::StoredDtcs
                | DiagnosticReadKind::PendingDtcs
                | DiagnosticReadKind::PermanentDtcs => self
                    .read_standard_dtc_query(query, crate::DiagnosticReportKind::Snapshot)
                    .map(ResearchTestResult::Diagnostic),
                DiagnosticReadKind::Vin
                | DiagnosticReadKind::CalibrationId
                | DiagnosticReadKind::EcuName => self
                    .read_standard_pid_query(query)
                    .map(ResearchTestResult::Diagnostic),
                DiagnosticReadKind::ClearDtcs => Err(LogNookError::new(
                    "OpenPort DTC clearing is available only through the explicit clear operation",
                )),
                DiagnosticReadKind::Mode06Bitmap | DiagnosticReadKind::Mode06Monitor => self
                    .read_standard_pid_query(query)
                    .map(ResearchTestResult::Diagnostic),
            },
            ResearchTestRequest::OemCandidate(spec) => self
                .read_oem_candidate(spec)
                .map(ResearchTestResult::OemCandidate),
            ResearchTestRequest::A30ReadTest(_) => Err(LogNookError::new(
                "OpenPort supports read-only standard DTC, Mode 09 identity, and one-shot source-backed OEM reads",
            )),
        }
    }

    fn clear_vehicle_dtcs(&mut self) -> Result<Vec<DiagnosticRead>, LogNookError> {
        if self.mode == OpenPort2Mode::SubaruSsm {
            return Err(LogNookError::new(
                "Standard DTC operations are unavailable in Subaru SSM K-Line mode",
            ));
        }
        self.read_standard_dtc_query(
            DiagnosticQueryRequest::clear_dtcs(),
            crate::DiagnosticReportKind::ClearDtcs,
        )
        .map(|report| report.reads)
    }

    fn sample_interval(&self) -> std::time::Duration {
        // Each bounded, serialized J2534 exchange already waits for the ECU.
        // An extra cycle delay needlessly caps measured live response throughput.
        std::time::Duration::ZERO
    }

    fn disconnect(&mut self) {
        if let Some(mut session) = self.session.take() {
            if self.connected {
                let _ = session.request(WorkerRequest::Disconnect);
            }
            session.shutdown();
        }
        self.connected = false;
        self.device = None;
        self.started = None;
        self.ecu_id = None;
        self.supported_pids.clear();
    }
}

fn oem_candidate_worker_request(spec: &OemCandidateTrySpec) -> Result<WorkerRequest, LogNookError> {
    build_oem_candidate_command(spec)?;
    let candidate = &spec.candidate;
    let request_id = oem_candidate_can_id(&candidate.request_header, "request")?;
    let response_id = oem_candidate_can_id(&candidate.response_header, "response")?;
    let request = match candidate.service.as_str() {
        "21" => OemCandidateRequestKind::Mode21 {
            identifier: u8::from_str_radix(&candidate.identifier, 16)
                .map_err(|_| LogNookError::new("Mode 21 candidate identifier is invalid"))?,
        },
        "22" => OemCandidateRequestKind::Mode22 {
            did: u16::from_str_radix(&candidate.identifier, 16)
                .map_err(|_| LogNookError::new("Mode 22 candidate DID is invalid"))?,
        },
        _ => {
            return Err(LogNookError::new(
                "Only read-only services 21 and 22 are allowed",
            ));
        }
    };
    if let Some(catalog_sha256) = spec.local_subaru_catalog_sha256() {
        lognook_j2534_protocol::validate_subaru_catalog_request_fields(
            &candidate.candidate_id,
            &candidate.source_id,
            catalog_sha256,
            request_id,
            response_id,
        )
        .map_err(|error| LogNookError::new(error.to_string()))?;
        return Ok(WorkerRequest::ReadSubaruCandidate {
            candidate_id: candidate.candidate_id.clone(),
            source_id: candidate.source_id.clone(),
            catalog_sha256: catalog_sha256.to_owned(),
            request_id,
            response_id,
            request,
        });
    }
    let source_url = candidate
        .source_url
        .as_deref()
        .ok_or_else(|| LogNookError::new("OEM candidate source URL is missing"))?;
    let license = candidate
        .license
        .as_deref()
        .ok_or_else(|| LogNookError::new("OEM candidate license is missing"))?;
    let license_url = candidate
        .license_url
        .as_deref()
        .ok_or_else(|| LogNookError::new("OEM candidate license URL is missing"))?;
    validate_oem_candidate_request_fields(
        &candidate.candidate_id,
        &candidate.source_id,
        &candidate.source_blob_sha,
        source_url,
        license,
        license_url,
        request_id,
        response_id,
    )
    .map_err(|error| LogNookError::new(error.to_string()))?;
    Ok(WorkerRequest::ReadOemCandidate {
        candidate_id: candidate.candidate_id.clone(),
        source_id: candidate.source_id.clone(),
        source_blob_sha: candidate.source_blob_sha.clone(),
        source_url: source_url.to_owned(),
        license: license.to_owned(),
        license_url: license_url.to_owned(),
        request_id,
        response_id,
        request,
    })
}

fn oem_candidate_can_id(value: &str, route: &str) -> Result<u16, LogNookError> {
    if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LogNookError::new(format!(
            "OEM {route} route must be an exact 11-bit CAN identifier"
        )));
    }
    let id = u16::from_str_radix(value, 16)
        .map_err(|_| LogNookError::new(format!("OEM {route} route is invalid")))?;
    if id > 0x7FF {
        return Err(LogNookError::new(format!(
            "OEM {route} route must be an exact 11-bit CAN identifier"
        )));
    }
    Ok(id)
}

fn oem_candidate_route_and_bytes(request: &WorkerRequest) -> (u16, u16, Vec<u8>) {
    let (request_id, response_id, request) = match request {
        WorkerRequest::ReadOemCandidate {
            request_id,
            response_id,
            request,
            ..
        }
        | WorkerRequest::ReadSubaruCandidate {
            request_id,
            response_id,
            request,
            ..
        } => (request_id, response_id, request),
        _ => unreachable!("OEM command details require an OEM request"),
    };
    let bytes = match request {
        OemCandidateRequestKind::Mode21 { identifier } => vec![0x21, *identifier],
        OemCandidateRequestKind::Mode22 { did } => {
            vec![0x22, (*did >> 8) as u8, *did as u8]
        }
    };
    (*request_id, *response_id, bytes)
}

fn oem_candidate_is_pending(
    response: &WorkerOemCandidateResponse,
    spec: &OemCandidateTrySpec,
) -> bool {
    matches!(response, WorkerOemCandidateResponse::Payload(payload)
        if oem_candidate_is_pending_payload(&payload.bytes, &spec.candidate.service))
}

fn oem_candidate_is_pending_payload(bytes: &[u8], service: &str) -> bool {
    match service {
        "21" => bytes == [0x7F, 0x21, 0x78],
        "22" => bytes == [0x7F, 0x22, 0x78],
        _ => false,
    }
}

fn oem_candidate_raw_response_line(response: &WorkerOemCandidateResponse) -> Option<String> {
    match response {
        WorkerOemCandidateResponse::Payload(payload) => Some(format!(
            "{:03X} {}",
            payload.response_id,
            hex_bytes(&payload.bytes)
        )),
        WorkerOemCandidateResponse::MalformedFrame(frame) => Some(format!(
            "{}{}",
            frame
                .response_id
                .map(|response_id| format!("{response_id:03X} "))
                .unwrap_or_default(),
            hex_bytes(&frame.raw_bytes)
        )),
        WorkerOemCandidateResponse::Failure(_) => None,
    }
}

fn oem_candidate_malformed(
    spec: &OemCandidateTrySpec,
    frame: OemCandidateMalformedFrame,
) -> OemCandidateResponse {
    let responder = frame.response_id.map(|id| format!("{id:03X}"));
    let mut decoded = decode_oem_candidate_payload(spec, responder.as_deref(), &[]);
    decoded.outcome = OemCandidateOutcome::MalformedResponse;
    decoded.payload = frame.raw_bytes;
    decoded.negative_response_code = None;
    decoded.decoded_value = None;
    decoded.decode_error = None;
    decoded.error = Some(frame.detail);
    decoded
}

fn oem_candidate_worker_failure(
    spec: &OemCandidateTrySpec,
    kind: WorkerErrorKind,
    message: String,
) -> OemCandidateResponse {
    let mut decoded = decode_oem_candidate_payload(spec, None, &[]);
    decoded.outcome = match &kind {
        WorkerErrorKind::NoResponse | WorkerErrorKind::Timeout => OemCandidateOutcome::NoResponse,
        WorkerErrorKind::MalformedResponse => OemCandidateOutcome::MalformedResponse,
        _ => OemCandidateOutcome::NoResponse,
    };
    decoded.error = Some(format!("OpenPort {kind:?}: {message}"));
    decoded
}

fn mode09_report_read(
    query: &DiagnosticQueryRequest,
    response: Mode09Payload,
    command: String,
    started_at_unix_ms: u64,
    latency_ms: u64,
) -> DiagnosticRead {
    let raw = format!("{} {}", response.ecu_id, hex_bytes(&response.bytes));
    query.report(
        Some(response.ecu_id),
        response.bytes,
        raw,
        command,
        started_at_unix_ms,
        latency_ms,
        false,
    )
}

fn mode09_response_read(
    query: &DiagnosticQueryRequest,
    response: Mode09Response,
    command: String,
    started_at_unix_ms: u64,
    fallback_latency_ms: u64,
) -> DiagnosticRead {
    match response {
        Mode09Response::Payload(response) => {
            let latency_ms = response.latency_us / 1_000;
            mode09_report_read(query, response, command, started_at_unix_ms, latency_ms)
        }
        Mode09Response::MalformedFrame(frame) => mode09_malformed_read(
            query,
            frame,
            command,
            started_at_unix_ms,
            fallback_latency_ms,
        ),
        Mode09Response::Failure(failure) => mode09_worker_failure_read(
            query,
            failure,
            command,
            started_at_unix_ms,
            fallback_latency_ms,
        ),
    }
}

fn mode09_malformed_read(
    query: &DiagnosticQueryRequest,
    frame: Mode09MalformedFrame,
    command: String,
    started_at_unix_ms: u64,
    fallback_latency_ms: u64,
) -> DiagnosticRead {
    let bytes_hex = hex_bytes(&frame.raw_bytes);
    let detail = format!(
        "{}; retained {} raw J2534 data byte(s) (declared {}); request route {}",
        frame.detail,
        frame.raw_bytes.len(),
        frame.declared_data_size,
        query.route()
    );
    let mut read = malformed_response(
        query,
        format!("J2534 malformed frame data: {bytes_hex}"),
        command,
        started_at_unix_ms,
        if frame.latency_us == 0 {
            fallback_latency_ms
        } else {
            frame.latency_us / 1_000
        },
        detail.clone(),
    );
    read.ecu_id = frame.ecu_id;
    read.response_payload = frame.raw_bytes;
    read.detail = Some(detail);
    read
}

fn mode09_worker_failure_read(
    query: &DiagnosticQueryRequest,
    failure: Mode09Failure,
    command: String,
    started_at_unix_ms: u64,
    fallback_latency_ms: u64,
) -> DiagnosticRead {
    mode09_worker_error_read(
        query,
        command,
        started_at_unix_ms,
        if failure.latency_us == 0 {
            fallback_latency_ms
        } else {
            failure.latency_us / 1_000
        },
        failure.kind,
        failure.message,
    )
}

fn mode09_worker_error_read(
    query: &DiagnosticQueryRequest,
    command: String,
    started_at_unix_ms: u64,
    latency_ms: u64,
    kind: WorkerErrorKind,
    message: String,
) -> DiagnosticRead {
    match kind {
        WorkerErrorKind::NoResponse | WorkerErrorKind::Timeout => query.no_response(
            message.clone(),
            command,
            started_at_unix_ms,
            latency_ms,
            Some(message),
        ),
        WorkerErrorKind::MalformedResponse => malformed_response(
            query,
            message.clone(),
            command,
            started_at_unix_ms,
            latency_ms,
            message,
        ),
        _ => query.transport_error(
            message.clone(),
            command,
            started_at_unix_ms,
            latency_ms,
            format!("OpenPort {kind:?}: {message}"),
        ),
    }
}

fn standard_dtc_response_read(
    query: &DiagnosticQueryRequest,
    response: StandardDtcResponse,
    command: String,
    started_at_unix_ms: u64,
    fallback_latency_ms: u64,
) -> DiagnosticRead {
    match response {
        StandardDtcResponse::Payload(response) => {
            let raw = format!("{} {}", response.ecu_id, hex_bytes(&response.bytes));
            query.report(
                Some(response.ecu_id),
                response.bytes,
                raw,
                command,
                started_at_unix_ms,
                response.latency_us / 1_000,
                false,
            )
        }
        StandardDtcResponse::MalformedFrame(frame) => standard_dtc_malformed_read(
            query,
            frame,
            command,
            started_at_unix_ms,
            fallback_latency_ms,
        ),
        StandardDtcResponse::Failure(failure) => standard_dtc_worker_error_read(
            query,
            command,
            started_at_unix_ms,
            if failure.latency_us == 0 {
                fallback_latency_ms
            } else {
                failure.latency_us / 1_000
            },
            failure.kind,
            failure.message,
        ),
    }
}

fn standard_dtc_malformed_read(
    query: &DiagnosticQueryRequest,
    frame: StandardDtcMalformedFrame,
    command: String,
    started_at_unix_ms: u64,
    fallback_latency_ms: u64,
) -> DiagnosticRead {
    let bytes_hex = hex_bytes(&frame.raw_bytes);
    let detail = format!(
        "{}; retained {} raw J2534 data byte(s) (declared {})",
        frame.detail,
        frame.raw_bytes.len(),
        frame.declared_data_size,
    );
    let mut read = malformed_response(
        query,
        format!("J2534 malformed frame data: {bytes_hex}"),
        command,
        started_at_unix_ms,
        if frame.latency_us == 0 {
            fallback_latency_ms
        } else {
            frame.latency_us / 1_000
        },
        detail.clone(),
    );
    read.ecu_id = frame.ecu_id;
    read.response_payload = frame.raw_bytes;
    read.detail = Some(detail);
    read
}

fn standard_dtc_worker_error_read(
    query: &DiagnosticQueryRequest,
    command: String,
    started_at_unix_ms: u64,
    latency_ms: u64,
    kind: WorkerErrorKind,
    message: String,
) -> DiagnosticRead {
    match kind {
        WorkerErrorKind::NoResponse | WorkerErrorKind::Timeout => query.no_response(
            message.clone(),
            command,
            started_at_unix_ms,
            latency_ms,
            Some(message),
        ),
        WorkerErrorKind::MalformedResponse => malformed_response(
            query,
            message.clone(),
            command,
            started_at_unix_ms,
            latency_ms,
            message,
        ),
        _ => query.transport_error(
            message.clone(),
            command,
            started_at_unix_ms,
            latency_ms,
            format!("OpenPort {kind:?}: {message}"),
        ),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn contextual_sink(sink: &DiagnosticSink, provider_id: &str, operation: &str) -> DiagnosticSink {
    let sink = sink.clone();
    let adapter_id = provider_id.to_owned();
    let operation = operation.to_owned();
    std::sync::Arc::new(move |mut event| {
        event.adapter_id = Some(adapter_id.clone());
        event.operation = operation.clone();
        sink(event);
    })
}

fn parse_supported_pages(
    responses: &[Mode01Payload],
) -> Result<(String, HashSet<u8>), LogNookError> {
    const PAGE_STARTS: [u8; 8] = [0x00, 0x20, 0x40, 0x60, 0x80, 0xA0, 0xC0, 0xE0];
    if responses.is_empty() {
        return Err(LogNookError::new(
            "The ECU did not report supported standard Mode 01 PIDs",
        ));
    }

    let ecu_id = responses[0].ecu_id.to_ascii_uppercase();
    if !valid_ecu_id(&ecu_id) {
        return Err(LogNookError::new(
            "OpenPort returned an invalid ECU identity",
        ));
    }
    if responses
        .iter()
        .any(|response| !response.ecu_id.eq_ignore_ascii_case(&ecu_id))
    {
        return Err(LogNookError::new(
            "Multiple ECU responders are not supported during OpenPort PID discovery",
        ));
    }

    let mut pages = BTreeMap::new();
    for response in responses {
        if response.bytes.len() != 6
            || response.bytes[0] != 0x41
            || !PAGE_STARTS.contains(&response.bytes[1])
        {
            return Err(LogNookError::new(
                "OpenPort returned a malformed supported-PID page",
            ));
        }
        if pages.insert(response.bytes[1], response).is_some() {
            return Err(LogNookError::new(
                "Multiple ECU responses were returned for a supported-PID page",
            ));
        }
    }

    let mut supported_pids = HashSet::new();
    let mut next_index = 0;
    loop {
        let Some(start) = PAGE_STARTS.get(next_index).copied() else {
            break;
        };
        let response = pages.get(&start).ok_or_else(|| {
            LogNookError::new(format!("OpenPort omitted supported-PID page {start:02X}"))
        })?;
        let SupportedPidRange { pids, has_more, .. } =
            parse_supported_pid_payload(ecu_id.clone(), &response.bytes, start)
                .map_err(LogNookError::new)?;
        supported_pids.extend(pids);
        next_index += 1;
        if !has_more {
            break;
        }
    }
    if pages.len() != next_index {
        return Err(LogNookError::new(
            "OpenPort returned an unrequested supported-PID page",
        ));
    }
    Ok((ecu_id, supported_pids))
}

fn valid_ecu_id(value: &str) -> bool {
    value.len() == 3
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && u16::from_str_radix(value, 16).is_ok_and(|address| (0x7E8..=0x7EF).contains(&address))
}

fn now_unix_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
#[path = "openport2/tests.rs"]
mod tests;
