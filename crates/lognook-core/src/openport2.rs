mod worker_client;

use crate::{
    HardwareAdapter,
    model::{
        ConnectionCapabilities, DeviceDescriptor, LogNookError, Sample, SampleOrigin,
        SampleQuality, SignalDefinition,
    },
    obdlink_sx_protocol::{
        SupportedPidRange, decode_signal, parse_mode01_payload, parse_supported_pid_payload,
    },
    simulator::standard_obd_signals,
};
use lognook_j2534_protocol::{
    J2534Identity, J2534Provider, Mode01Payload, WorkerErrorKind, WorkerRequest, WorkerResponse,
};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use worker_client::WorkerClient;

trait WorkerSession: Send {
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, LogNookError>;
    fn shutdown(&mut self);
}

pub fn available_openport2_providers(
    worker_path: &Path,
) -> Result<Vec<J2534Provider>, LogNookError> {
    let mut session = WorkerClient::spawn(worker_path)?;
    let result = available_from_session(&mut session);
    session.shutdown();
    result
}

pub fn identify_openport2(
    worker_path: &Path,
    provider_id: &str,
) -> Result<DeviceDescriptor, LogNookError> {
    let mut session = WorkerClient::spawn(worker_path)?;
    let result = identify_from_session(&mut session, provider_id);
    session.shutdown();
    result
}

fn available_from_session(
    session: &mut dyn WorkerSession,
) -> Result<Vec<J2534Provider>, LogNookError> {
    match session.request(WorkerRequest::ListProviders)? {
        WorkerResponse::Providers { providers } => Ok(providers),
        response => Err(unexpected_worker_response("provider listing", response)),
    }
}

fn identify_from_session(
    session: &mut dyn WorkerSession,
    provider_id: &str,
) -> Result<DeviceDescriptor, LogNookError> {
    let identity = match session.request(WorkerRequest::Identify {
        provider_id: provider_id.to_owned(),
    })? {
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
        WorkerResponse::Error { kind, message } => worker_error(kind, message),
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
    started: Option<Instant>,
    ecu_id: Option<String>,
    supported_pids: HashSet<u8>,
}

impl OpenPort2Adapter {
    pub fn new(worker_path: PathBuf, provider_id: String) -> Self {
        Self {
            worker_path,
            provider_id,
            session: None,
            connected: false,
            started: None,
            ecu_id: None,
            supported_pids: HashSet::new(),
        }
    }

    #[cfg(test)]
    fn with_session(mut self, session: Box<dyn WorkerSession>) -> Self {
        self.session = Some(session);
        self
    }

    fn connect_inner(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.session.is_none() {
            self.session = Some(Box::new(WorkerClient::spawn(&self.worker_path)?));
        }
        let session = self.session.as_deref_mut().expect("session was created");
        let identity = match session.request(WorkerRequest::Connect {
            provider_id: self.provider_id.clone(),
        })? {
            WorkerResponse::Connected { identity } => identity,
            response => return Err(unexpected_worker_response("connect", response)),
        };
        self.connected = true;

        let responses = match session.request(WorkerRequest::DiscoverMode01)? {
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
            device: device_descriptor(&self.provider_id, identity),
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
        match session.request(WorkerRequest::ReadMode01 { pid })? {
            WorkerResponse::Mode01Responses { responses } if responses.len() == 1 => Ok(responses
                .into_iter()
                .next()
                .expect("one response was checked")),
            WorkerResponse::Mode01Responses { .. } => Err(LogNookError::new(
                "Multiple ECU responders are not supported by the OpenPort adapter",
            )),
            response => Err(unexpected_worker_response("Mode 01 PID read", response)),
        }
    }
}

impl HardwareAdapter for OpenPort2Adapter {
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

        let mut samples = Vec::with_capacity(signals.len());
        let mut seen_pids = HashSet::new();
        for pid in signals
            .iter()
            .map(|signal| signal.pid)
            .filter(|pid| seen_pids.insert(*pid))
        {
            let payload = self.read_pid(pid)?;
            if !payload.ecu_id.eq_ignore_ascii_case(&ecu_id) {
                return Err(LogNookError::new(
                    "ECU identity changed during the OpenPort session",
                ));
            }
            let response = parse_mode01_payload(ecu_id.clone(), &payload.bytes, pid)
                .map_err(LogNookError::new)?;
            for signal in signals.iter().filter(|signal| signal.pid == pid) {
                let canonical = catalog
                    .iter()
                    .find(|known| known.id == signal.id)
                    .expect("signal was validated against catalog");
                let value = decode_signal(canonical, &response).map_err(LogNookError::new)?;
                let elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
                samples.push(Sample {
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
        }
        Ok(samples)
    }

    fn sample_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(100)
    }

    fn disconnect(&mut self) {
        if let Some(mut session) = self.session.take() {
            if self.connected {
                let _ = session.request(WorkerRequest::Disconnect);
            }
            session.shutdown();
        }
        self.connected = false;
        self.started = None;
        self.ecu_id = None;
        self.supported_pids.clear();
    }
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
