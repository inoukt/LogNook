mod worker_client;

use crate::{
    DiagnosticQueryRequest, DiagnosticRead, DiagnosticReadKind, DiagnosticReport,
    DiagnosticRequest, DiagnosticSink, HardwareAdapter, ResearchTestRequest, ResearchTestResult,
    adapter::collect_samples_independently,
    communication_trace::CommunicationSink,
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
    Mode09Payload, Mode09Response, WorkerErrorKind, WorkerRequest, WorkerResponse,
};
use std::{
    collections::{BTreeMap, HashSet},
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
    session: Option<Box<dyn WorkerSession>>,
    connected: bool,
    device: Option<DeviceDescriptor>,
    started: Option<Instant>,
    ecu_id: Option<String>,
    supported_pids: HashSet<u8>,
    diagnostic_sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
}

impl OpenPort2Adapter {
    pub fn new(worker_path: PathBuf, provider_id: String) -> Self {
        Self {
            worker_path,
            provider_id,
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
        let identity = match session.request_with_progress(
            WorkerRequest::Connect {
                provider_id: self.provider_id.clone(),
            },
            &sink,
        )? {
            WorkerResponse::Connected { identity } => identity,
            response => return Err(unexpected_worker_response("connect", response)),
        };
        self.connected = true;
        let device = device_descriptor(&self.provider_id, identity);
        self.device = Some(device.clone());

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

    fn read_identity_query(
        &mut self,
        query: DiagnosticQueryRequest,
    ) -> Result<DiagnosticReport, LogNookError> {
        query.validate().map_err(LogNookError::new)?;
        if !matches!(
            query.kind,
            DiagnosticReadKind::Vin
                | DiagnosticReadKind::CalibrationId
                | DiagnosticReadKind::EcuName
        ) {
            return Err(LogNookError::new(
                "OpenPort supports only Mode 09 VIN, calibration ID, and ECU name reads",
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
        let sink = contextual_sink(&self.diagnostic_sink, &self.provider_id, "profile_identity");
        let response = self
            .session
            .as_deref_mut()
            .expect("connected OpenPort adapter has a worker session")
            .request_with_progress(WorkerRequest::ReadMode09 { pid }, &sink);
        let reads = match response {
            Ok(WorkerResponse::Mode09Responses { responses }) if !responses.is_empty() => responses
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
                .collect(),
            Ok(WorkerResponse::Mode09Responses { .. }) => vec![query.no_response(
                String::new(),
                command.clone(),
                started_at_unix_ms,
                elapsed_ms(started),
                Some("OpenPort returned no Mode 09 responders".into()),
            )],
            Ok(WorkerResponse::Error { kind, message }) => vec![mode09_worker_error_read(
                &query,
                command.clone(),
                started_at_unix_ms,
                elapsed_ms(started),
                kind,
                message,
            )],
            Ok(response) => vec![query.transport_error(
                format!("Unexpected worker response: {response:?}"),
                command.clone(),
                started_at_unix_ms,
                elapsed_ms(started),
                format!("OpenPort returned an unexpected response for Mode 09 PID {pid:02X}"),
            )],
            Err(error) => vec![query.transport_error(
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
            protocol: "SAE J1979 Mode 09 over ISO 15765 (11-bit, 500 kbit/s) via OpenPort 2.0"
                .into(),
            started_at_unix_ms,
            request: DiagnosticRequest::default(),
            reads,
            route_restore_error: None,
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

    fn execute_research_test(
        &mut self,
        request: ResearchTestRequest,
    ) -> Result<ResearchTestResult, LogNookError> {
        match request {
            ResearchTestRequest::Diagnostic(query) => self
                .read_identity_query(query)
                .map(ResearchTestResult::Diagnostic),
            ResearchTestRequest::OemCandidate(_) | ResearchTestRequest::A30ReadTest(_) => Err(
                LogNookError::new("OpenPort supports only read-only Mode 09 identity reads"),
            ),
        }
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
