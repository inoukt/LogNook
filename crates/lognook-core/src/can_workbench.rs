//! Engine-owned Candlelight CAN workbench API and device selection.
use crate::candlelight::{CandlelightError, CandlelightTransport};
use crate::{
    CanAnalysisConfig, CanAnalyzer, CanCaptureReader, CanDiagnosticOutcome, CanDirection, CanFrame,
    CanSignalDefinition, CanTriggerCapture, CanTriggerConfig, CanTriggerRecorder, EngineEvent,
    LogNookError, SamplePublisher,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const COMMAND_CAPACITY: usize = 32;
const TRANSMIT_TIMEOUT: Duration = Duration::from_millis(500);
/// A candidate is a USB-ID hint. Interface/GS_USB evidence is verified on open.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanDeviceCandidate {
    pub id: String,
    pub bus_id: String,
    pub device_address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: Option<String>,
    pub serial_number: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanDeviceSelection {
    pub id: String,
    pub interface_number: u8,
    pub channel: u8,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanWorkbenchConfig {
    pub bitrate: u32,
    pub listen_only: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanWorkbenchStatus {
    pub connected: bool,
    pub bitrate: u32,
    pub listen_only: bool,
    /// Candlelight VID/PID/serial-derived source identity; USB IDs alone are not authenticity proof.
    pub source_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanTriggerCaptureResult {
    pub source_capture_id: u64,
    pub source_id: String,
    pub bitrate: u32,
    pub listen_only: bool,
    pub capture: CanTriggerCapture,
}

impl CanWorkbenchConfig {
    fn validate(&self) -> Result<(), LogNookError> {
        if !(10_000..=1_000_000).contains(&self.bitrate) {
            return Err(LogNookError::new("CAN bitrate must be 10000-1000000 bit/s"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanScanMode {
    CanIdRange {
        start_id: u32,
        end_id: u32,
        extended: bool,
        payload: Vec<u8>,
    },
    Mode21 {
        request_id: u32,
        response_id: u32,
        extended: bool,
        start_pid: u8,
        end_pid: u8,
    },
    Mode22 {
        request_id: u32,
        response_id: u32,
        extended: bool,
        start_did: u16,
        end_did: u16,
    },
    Mode01Support {
        request_id: u32,
        response_id: u32,
        extended: bool,
    },
    Mode01Pids {
        request_id: u32,
        response_id: u32,
        extended: bool,
        pids: Vec<u8>,
        repeat: bool,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanScanConfig {
    pub mode: CanScanMode,
    pub pacing_ms: u64,
    pub response_timeout_ms: u64,
    /// Set only by source-backed catalog reads; retained with every result for provenance.
    #[serde(skip_deserializing)]
    pub source_candidate_id: Option<String>,
}

impl CanScanConfig {
    pub fn validate(&self) -> Result<(), LogNookError> {
        if !(1..=60_000).contains(&self.pacing_ms) {
            return Err(LogNookError::new("CAN scan pacing must be 1-60000 ms"));
        }
        if !(10..=10_000).contains(&self.response_timeout_ms) {
            return Err(LogNookError::new(
                "CAN response timeout must be 10-10000 ms",
            ));
        }
        if self
            .source_candidate_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty() || id.len() > 128)
        {
            return Err(LogNookError::new(
                "Invalid source candidate ID for CAN scan",
            ));
        }
        match &self.mode {
            CanScanMode::CanIdRange {
                start_id,
                end_id,
                extended,
                payload,
            } => {
                validate_can_route(*start_id, *extended)?;
                validate_can_route(*end_id, *extended)?;
                if start_id > end_id || payload.len() > 8 {
                    return Err(LogNookError::new("Invalid CAN-ID scan range or payload"));
                }
            }
            CanScanMode::Mode21 {
                request_id,
                response_id,
                extended,
                start_pid,
                end_pid,
            } => {
                validate_diag_route(*request_id, *response_id, *extended)?;
                if start_pid > end_pid {
                    return Err(LogNookError::new("Mode 21 start PID exceeds end PID"));
                }
            }
            CanScanMode::Mode22 {
                request_id,
                response_id,
                extended,
                start_did,
                end_did,
            } => {
                validate_diag_route(*request_id, *response_id, *extended)?;
                if start_did > end_did {
                    return Err(LogNookError::new("Mode 22 start DID exceeds end DID"));
                }
            }
            CanScanMode::Mode01Support {
                request_id,
                response_id,
                extended,
            } => {
                validate_diag_route(*request_id, *response_id, *extended)?;
            }
            CanScanMode::Mode01Pids {
                request_id,
                response_id,
                extended,
                pids,
                ..
            } => {
                validate_diag_route(*request_id, *response_id, *extended)?;
                if pids.is_empty() || pids.len() > 256 {
                    return Err(LogNookError::new("Choose 1-256 Mode 01 PIDs"));
                }
                if pids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != pids.len()
                {
                    return Err(LogNookError::new("Mode 01 PID list contains duplicates"));
                }
            }
        }
        Ok(())
    }
}

/// Construct an explicit one-candidate bench read from the validated source-backed catalog row.
pub fn oem_candidate_scan_config(
    spec: &crate::OemCandidateTrySpec,
) -> Result<CanScanConfig, LogNookError> {
    let candidate = &spec.candidate;
    let parse_header = |header: &str| {
        u32::from_str_radix(header.trim(), 16)
            .map_err(|_| LogNookError::new("OEM catalog CAN header is not hexadecimal"))
    };
    let request_id = parse_header(&candidate.request_header)?;
    let response_id = parse_header(&candidate.response_header)?;
    let mode = match candidate.service.as_str() {
        "21" => {
            let pid = u8::from_str_radix(&candidate.identifier, 16)
                .map_err(|_| LogNookError::new("OEM service 21 identifier is invalid"))?;
            CanScanMode::Mode21 {
                request_id,
                response_id,
                extended: false,
                start_pid: pid,
                end_pid: pid,
            }
        }
        "22" => {
            let did = u16::from_str_radix(&candidate.identifier, 16)
                .map_err(|_| LogNookError::new("OEM service 22 identifier is invalid"))?;
            CanScanMode::Mode22 {
                request_id,
                response_id,
                extended: false,
                start_did: did,
                end_did: did,
            }
        }
        _ => {
            return Err(LogNookError::new(
                "OEM bench reads support catalog services 21 and 22",
            ));
        }
    };
    let config = CanScanConfig {
        mode,
        pacing_ms: 100,
        response_timeout_ms: 1_000,
        source_candidate_id: Some(candidate.candidate_id.clone()),
    };
    config.validate()?;
    Ok(config)
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanScanResult {
    pub config: CanScanConfig,
    pub identifier: String,
    pub request: CanFrame,
    pub response_frames: Vec<CanFrame>,
    pub outcome: CanDiagnosticOutcome,
    /// Reported only from an explicit Mode 01 support-page response; not validation metadata.
    pub supported_pids: Option<Vec<u8>>,
}

/// Persisted form keeps catalog provenance readable without allowing commands to deserialize it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanEvidenceScanConfig {
    pub mode: CanScanMode,
    pub pacing_ms: u64,
    pub response_timeout_ms: u64,
    pub source_candidate_id: Option<String>,
}

impl From<&CanScanConfig> for CanEvidenceScanConfig {
    fn from(config: &CanScanConfig) -> Self {
        Self {
            mode: config.mode.clone(),
            pacing_ms: config.pacing_ms,
            response_timeout_ms: config.response_timeout_ms,
            source_candidate_id: config.source_candidate_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanEvidenceScanResult {
    pub config: CanEvidenceScanConfig,
    pub identifier: String,
    pub request: CanFrame,
    pub response_frames: Vec<CanFrame>,
    pub outcome: CanDiagnosticOutcome,
    pub supported_pids: Option<Vec<u8>>,
}

impl From<&CanScanResult> for CanEvidenceScanResult {
    fn from(result: &CanScanResult) -> Self {
        Self {
            config: CanEvidenceScanConfig::from(&result.config),
            identifier: result.identifier.clone(),
            request: result.request.clone(),
            response_frames: result.response_frames.clone(),
            outcome: result.outcome.clone(),
            supported_pids: result.supported_pids.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanTransportErrorKind {
    Timeout,
    Disconnected,
    BusOff,
    Controller,
    UnsupportedFrame,
    Protocol,
    Usb,
    Worker,
    Other,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CanTransportError {
    pub kind: CanTransportErrorKind,
    pub message: String,
    pub raw_usb: Option<Vec<u8>>,
}

impl std::fmt::Display for CanTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl CanTransportError {
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            kind: CanTransportErrorKind::Other,
            message: message.into(),
            raw_usb: None,
        }
    }

    /// Only a frame-format rejection is safe to skip while scanning a range.
    pub fn terminates_scan(&self) -> bool {
        !matches!(self.kind, CanTransportErrorKind::UnsupportedFrame)
    }

    /// These adapter/controller failures close the serialized transport session.
    pub fn terminates_session(&self) -> bool {
        matches!(
            self.kind,
            CanTransportErrorKind::Disconnected
                | CanTransportErrorKind::BusOff
                | CanTransportErrorKind::Controller
                | CanTransportErrorKind::Protocol
                | CanTransportErrorKind::Usb
                | CanTransportErrorKind::Worker
        )
    }

    fn from_candlelight(error: CandlelightError) -> Self {
        let kind = match error.diagnostic() {
            CandlelightError::Timeout => CanTransportErrorKind::Timeout,
            CandlelightError::Disconnected => CanTransportErrorKind::Disconnected,
            CandlelightError::BusOff(_) => CanTransportErrorKind::BusOff,
            CandlelightError::ControllerError(_) => CanTransportErrorKind::Controller,
            CandlelightError::UnsupportedFrame(_) => CanTransportErrorKind::UnsupportedFrame,
            CandlelightError::Protocol(_) => CanTransportErrorKind::Protocol,
            CandlelightError::Usb(_) => CanTransportErrorKind::Usb,
            CandlelightError::Worker(_) => CanTransportErrorKind::Worker,
            CandlelightError::ListenOnlyUnavailable
            | CandlelightError::NotStarted
            | CandlelightError::PassiveTransmit
            | CandlelightError::FrameError { .. } => CanTransportErrorKind::Other,
        };
        Self {
            kind,
            message: error.to_string(),
            raw_usb: error.raw_usb().map(<[u8]>::to_vec),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CanTransportEvent {
    Rx(CanFrame),
    TxEcho {
        echo_id: u32,
        frame: CanFrame,
        raw_usb: Vec<u8>,
    },
}

/// Append-only JSONL metadata paired with a `.candump.csv` raw frame capture.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "record", content = "data", rename_all = "snake_case")]
pub enum CanEvidenceRecord {
    SessionStarted {
        format_version: u32,
        #[serde(default)]
        capture_id: u64,
        bitrate: u32,
        listen_only: bool,
        source_id: String,
    },
    SessionFinished {
        capture_id: u64,
    },
    ScanStarted {
        config: CanEvidenceScanConfig,
    },
    ScanStopped {
        config: CanEvidenceScanConfig,
    },
    ScanCompleted {
        config: CanEvidenceScanConfig,
    },
    ScanResult {
        result: CanEvidenceScanResult,
    },
    TxAttempt {
        #[serde(default)]
        capture_id: u64,
        sequence: u64,
        frame: CanFrame,
    },
    TxSubmitted {
        #[serde(default)]
        capture_id: u64,
        sequence: u64,
        echo_id: u32,
        frame: CanFrame,
    },
    TxEchoConfirmed {
        #[serde(default)]
        capture_id: u64,
        sequence: u64,
        echo_id: u32,
        submitted_frame: CanFrame,
        echo_frame: CanFrame,
        raw_usb: Vec<u8>,
    },
    TransportError {
        error: CanTransportError,
        frame: Option<CanFrame>,
    },
    TriggerMarked {
        capture_id: u64,
        trigger_elapsed_ns: i64,
        pre_ns: i64,
        post_ns: i64,
        max_frames: usize,
    },
    TriggerCompleted {
        capture_id: u64,
        trigger_elapsed_ns: i64,
        max_frames: usize,
        frame_count: usize,
        truncated: bool,
        complete: bool,
    },
    TriggerWindowExported {
        source_capture_id: u64,
        source_id: String,
        bitrate: u32,
        listen_only: bool,
        trigger_elapsed_ns: i64,
        pre_ns: i64,
        post_ns: i64,
        max_frames: usize,
        frame_count: usize,
        truncated: bool,
        complete: bool,
    },
}

pub fn can_evidence_path(path: impl AsRef<Path>) -> std::path::PathBuf {
    path.as_ref().with_extension("jsonl")
}

/// The transport is moved to the engine-owned worker; every method is called on that thread.
pub trait CanTransport: Send + 'static {
    fn source_id(&self) -> String {
        "can:unknown".into()
    }
    fn start(
        &mut self,
        bitrate: u32,
        listen_only: bool,
        session_origin: Instant,
    ) -> Result<(), CanTransportError>;
    fn receive(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<CanTransportEvent>, CanTransportError>;
    fn transmit(&mut self, frame: &CanFrame) -> Result<u32, CanTransportError>;
    fn stop(&mut self) -> Result<(), CanTransportError>;
}

impl CanTransport for CandlelightTransport {
    fn source_id(&self) -> String {
        let identity = self.identity();
        format!(
            "can:gs_usb:{:04X}:{:04X}:{}",
            identity.vendor_id,
            identity.product_id,
            identity.serial_number.as_deref().unwrap_or("unknown")
        )
    }

    fn start(
        &mut self,
        bitrate: u32,
        listen_only: bool,
        session_origin: Instant,
    ) -> Result<(), CanTransportError> {
        CandlelightTransport::start_at(self, bitrate, listen_only, session_origin)
            .map_err(CanTransportError::from_candlelight)
    }

    fn receive(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<CanTransportEvent>, CanTransportError> {
        match CandlelightTransport::receive(self, timeout) {
            Ok(frame) if frame.frame.direction == CanDirection::Rx => {
                Ok(Some(CanTransportEvent::Rx(frame.frame)))
            }
            Ok(frame) => Ok(Some(CanTransportEvent::TxEcho {
                echo_id: frame.echo_id,
                frame: frame.frame,
                raw_usb: frame.raw_usb,
            })),
            Err(error) if matches!(error.diagnostic(), CandlelightError::Timeout) => Ok(None),
            Err(error) => Err(CanTransportError::from_candlelight(error)),
        }
    }

    fn transmit(&mut self, frame: &CanFrame) -> Result<u32, CanTransportError> {
        CandlelightTransport::transmit(self, frame.clone(), TRANSMIT_TIMEOUT)
            .map_err(CanTransportError::from_candlelight)
    }

    fn stop(&mut self) -> Result<(), CanTransportError> {
        CandlelightTransport::stop(self).map_err(CanTransportError::from_candlelight)
    }
}

mod scan;
mod worker;
use worker::{Command, run_worker};

/// Clone-free command handle. The engine owns this until disconnect/drop.
pub struct CanWorkbench {
    commands: SyncSender<Command>,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    listen_only: bool,
    bitrate: u32,
    source_id: String,
}

impl CanWorkbench {
    pub fn start(
        mut transport: Box<dyn CanTransport>,
        config: CanWorkbenchConfig,
        publisher: SamplePublisher,
        event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    ) -> Result<Self, LogNookError> {
        config.validate()?;
        let bitrate = config.bitrate;
        let listen_only = config.listen_only;
        let source_id = transport.source_id();
        let worker_source_id = source_id.clone();
        let session_origin = Instant::now();
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(false));
        let thread_running = running.clone();
        let worker = thread::Builder::new()
            .name("can-workbench-owner".into())
            .spawn(move || {
                if let Err(error) =
                    transport.start(config.bitrate, config.listen_only, session_origin)
                {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
                thread_running.store(true, Ordering::Release);
                if ready_tx.send(Ok(())).is_err() {
                    let _ = transport.stop();
                    thread_running.store(false, Ordering::Release);
                    return;
                }
                run_worker(
                    transport,
                    receiver,
                    publisher,
                    event_sink,
                    thread_running.clone(),
                    worker_source_id,
                    bitrate,
                    listen_only,
                    session_origin,
                );
                let _ = thread_running;
            })
            .map_err(|error| LogNookError::new(error.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                commands,
                running,
                worker: Some(worker),
                listen_only,
                bitrate,
                source_id,
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(LogNookError::new(error))
            }
            Err(error) => {
                let _ = worker.join();
                Err(LogNookError::new(format!(
                    "CAN worker failed to start: {error}"
                )))
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn status(&self) -> CanWorkbenchStatus {
        CanWorkbenchStatus {
            connected: self.is_running(),
            bitrate: self.bitrate,
            listen_only: self.listen_only,
            source_id: self.source_id.clone(),
        }
    }

    pub fn configure_fields(&self, fields: Vec<CanSignalDefinition>) -> Result<(), LogNookError> {
        for field in &fields {
            field.validate()?;
        }
        self.request(|reply| Command::SetFields(fields, reply))
    }

    pub fn start_capture(&self, path: &Path) -> Result<(), LogNookError> {
        self.request(|reply| Command::StartCapture(path.to_owned(), reply))
    }

    pub fn stop_capture(&self) -> Result<(), LogNookError> {
        self.request(Command::StopCapture)
    }

    pub fn arm_trigger(
        &self,
        pre_ms: u64,
        post_ms: u64,
        max_frames: usize,
    ) -> Result<(), LogNookError> {
        if pre_ms > 60_000 || post_ms > 60_000 || !(1..=100_000).contains(&max_frames) {
            return Err(LogNookError::new(
                "CAN trigger windows must be at most 60000 ms and 1-100000 frames",
            ));
        }
        self.request(|reply| {
            Command::ArmTrigger(
                CanTriggerConfig {
                    pre_ns: (pre_ms * 1_000_000) as i64,
                    post_ns: (post_ms * 1_000_000) as i64,
                    max_frames,
                },
                reply,
            )
        })
    }

    pub fn trigger_now(&self) -> Result<(), LogNookError> {
        self.request(Command::TriggerNow)
    }

    pub fn disarm_trigger(&self) -> Result<(), LogNookError> {
        self.request(Command::DisarmTrigger)
    }

    pub fn send_frame(
        &self,
        frame: CanFrame,
        pacing_ms: u64,
        repeat: u32,
    ) -> Result<(), LogNookError> {
        if self.listen_only {
            return Err(LogNookError::new(
                "Switch Candlelight to active mode before sending CAN frames",
            ));
        }
        frame.validate()?;
        if !(1..=60_000).contains(&pacing_ms) || !(1..=10_000).contains(&repeat) {
            return Err(LogNookError::new(
                "CAN send pacing must be 1-60000 ms and repeat 1-10000",
            ));
        }
        self.request(|reply| Command::SendFrame(frame, pacing_ms, repeat, reply))
    }

    pub fn start_scan(&self, config: CanScanConfig) -> Result<(), LogNookError> {
        if self.listen_only {
            return Err(LogNookError::new(
                "Switch Candlelight to active mode before starting a CAN scan",
            ));
        }
        config.validate()?;
        self.request(|reply| Command::StartScan(config, reply))
    }

    pub fn stop_scan(&self) -> Result<(), LogNookError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(Command::StopScan(reply))
            .map_err(|_| LogNookError::new("CAN worker has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("CAN worker did not acknowledge scan stop"))?
            .map_err(LogNookError::new)?;
        Ok(())
    }

    fn request(
        &self,
        command: impl FnOnce(mpsc::Sender<Result<(), String>>) -> Command,
    ) -> Result<(), LogNookError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(command(reply))
            .map_err(|_| LogNookError::new("CAN worker has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("CAN worker did not acknowledge the command"))?
            .map_err(LogNookError::new)
    }
}

impl Drop for CanWorkbench {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn validate_can_route(id: u32, extended: bool) -> Result<(), LogNookError> {
    if id > if extended { 0x1fff_ffff } else { 0x7ff } {
        return Err(LogNookError::new(
            "CAN ID exceeds the selected standard/extended range",
        ));
    }
    Ok(())
}
fn validate_diag_route(request: u32, response: u32, extended: bool) -> Result<(), LogNookError> {
    validate_can_route(request, extended)?;
    validate_can_route(response, extended)?;
    if request == response {
        return Err(LogNookError::new(
            "Diagnostic request and response IDs must differ",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candlelight_error_adaptation_preserves_the_exact_usb_packet() {
        let raw = vec![0x01, 0x02, 0xDE, 0xAD];
        let error = CanTransportError::from_candlelight(CandlelightError::FrameError {
            raw_usb: raw.clone(),
            diagnostic: Box::new(CandlelightError::ControllerError(vec![0x99])),
        });
        assert_eq!(error.kind, CanTransportErrorKind::Controller);
        assert_eq!(error.raw_usb, Some(raw));
    }

    #[test]
    fn transport_error_classification_stops_bus_off_and_removal_but_skips_unsupported_frames() {
        for kind in [
            CanTransportErrorKind::BusOff,
            CanTransportErrorKind::Disconnected,
        ] {
            let error = CanTransportError {
                kind,
                message: "terminal".into(),
                raw_usb: None,
            };
            assert!(error.terminates_scan());
            assert!(error.terminates_session());
        }
        let timeout = CanTransportError {
            kind: CanTransportErrorKind::Timeout,
            message: "timeout".into(),
            raw_usb: None,
        };
        assert!(timeout.terminates_scan());
        assert!(!timeout.terminates_session());
        let unsupported = CanTransportError {
            kind: CanTransportErrorKind::UnsupportedFrame,
            message: "unsupported frame".into(),
            raw_usb: None,
        };
        assert!(!unsupported.terminates_scan());
        assert!(!unsupported.terminates_session());
    }
}

pub fn available_can_adapters() -> Result<Vec<CanDeviceCandidate>, LogNookError> {
    crate::candlelight::available_candlelight_devices()
        .map(|devices| {
            devices
                .into_iter()
                .map(|device| {
                    let usb = device.usb;
                    let bus_id = usb.bus_id().to_owned();
                    let device_address = usb.device_address();
                    CanDeviceCandidate {
                        id: format!("{bus_id}:{device_address}"),
                        bus_id,
                        device_address,
                        vendor_id: usb.vendor_id(),
                        product_id: usb.product_id(),
                        product: usb.product_string().map(str::to_owned),
                        serial_number: usb.serial_number().map(str::to_owned),
                    }
                })
                .collect()
        })
        .map_err(|error| LogNookError::new(error.to_string()))
}

pub fn open_candlelight(
    selection: &CanDeviceSelection,
) -> Result<Box<dyn CanTransport>, LogNookError> {
    let devices = crate::candlelight::available_candlelight_devices()
        .map_err(|error| LogNookError::new(error.to_string()))?;
    let mut matches = devices.into_iter().filter(|device| {
        format!("{}:{}", device.usb.bus_id(), device.usb.device_address()) == selection.id
    });
    let device = matches.next().ok_or_else(|| {
        LogNookError::new("Selected Candlelight USB device is no longer available")
    })?;
    if matches.next().is_some() {
        return Err(LogNookError::new(
            "Candlelight device selection is ambiguous; refresh the device list",
        ));
    }
    let transport =
        CandlelightTransport::open(device, selection.interface_number, selection.channel)
            .map_err(|error| LogNookError::new(error.to_string()))?;
    Ok(Box::new(transport))
}

pub fn replay_can_capture_file(
    path: impl AsRef<Path>,
    fields: &[CanSignalDefinition],
    mut on_sample: impl FnMut(crate::Sample),
) -> Result<(u64, Vec<crate::CanCensusRow>), LogNookError> {
    let mut analyzer = CanAnalyzer::new(CanAnalysisConfig::default())?;
    let reader = CanCaptureReader::open(path)?;
    let frames = crate::replay_can_capture(reader, &mut analyzer, fields, "can:capture", |item| {
        if let Ok(sample) = item {
            on_sample(sample);
        }
    })?;
    Ok((frames, analyzer.census()))
}
