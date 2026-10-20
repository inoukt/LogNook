use crate::LogNookError;
use crate::communication_trace::{
    CommunicationDirection, CommunicationSink, CommunicationTraceEvent, bytes_to_hex,
};
use crate::{
    BluetoothByteStream, ConnectionCapabilities, DeviceDescriptor, HardwareAdapter,
    OemCandidateOutcome, OemCandidateTryResult, OemCandidateTrySpec, Sample, SampleOrigin,
    SampleQuality, SignalDefinition,
    adapter::collect_samples_independently,
    build_oem_candidate_command, connect_bluetooth_spp, decode_oem_candidate_response,
    obdlink_sx_protocol::{decode_signal, parse_supported_pid_payload},
    standard_obd_signals,
};
use crate::{
    DiagnosticSink,
    diagnostics::{diagnostic_stage, noop_diagnostic_sink},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{ErrorKind, Read, Write},
    time::{Duration, Instant},
};

const MAX_A30_PAYLOAD_BYTES: usize = 2048;
const HOST_MAGIC: [u8; 2] = [0x55, 0xAA];
const DONGLE_MAGIC: [u8; 2] = [0xAA, 0x55];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A30Frame {
    pub sequence: u16,
    pub command: u16,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A30CanReply {
    pub response_header: String,
    pub payload: Vec<u8>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
// Legacy wire names are retained for saved research reports and the desktop quick-test picker.
pub enum A30ReadTest {
    #[serde(rename = "mode_01_engine_rpm")]
    Mode01EngineRpm,
    #[serde(rename = "mode_02_freeze_frame_rpm")]
    Mode02FreezeFrameRpm,
    #[serde(rename = "mode_03_stored_dtcs")]
    Mode03StoredDtcs,
    #[serde(rename = "mode_06_bitmap_00")]
    Mode06Bitmap00,
    #[serde(rename = "mode_06_bitmap_20")]
    Mode06Bitmap20,
    #[serde(rename = "mode_06_bitmap_40")]
    Mode06Bitmap40,
    #[serde(rename = "mode_06_bitmap_60")]
    Mode06Bitmap60,
    #[serde(rename = "mode_06_bitmap_80")]
    Mode06Bitmap80,
    #[serde(rename = "mode_06_bitmap_a0")]
    Mode06BitmapA0,
    #[serde(rename = "mode_06_tid_35")]
    Mode06Tid35,
    #[serde(rename = "mode_06_tid_a2")]
    Mode06TidA2,
    #[serde(rename = "mode_06_tid_a5")]
    Mode06TidA5,
    #[serde(rename = "mode_07_pending_dtcs")]
    Mode07PendingDtcs,
    #[serde(rename = "mode_09_vin")]
    Mode09Vin,
    #[serde(rename = "mode_0a_permanent_dtcs")]
    Mode0aPermanentDtcs,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum A30ReadOutcome {
    Positive,
    NegativeResponse,
    ResponsePending,
    TemporarilyUnavailable,
    SessionRequired,
    SecurityRequired,
    NoResponse,
    MalformedResponse,
    WrongResponder,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct A30GraphValue {
    pub signal_id: String,
    pub name: String,
    pub service: u8,
    pub pid: Option<u8>,
    pub value: f64,
    pub unit: String,
    pub raw_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_payload: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct A30ReadTestResult {
    pub test: A30ReadTest,
    pub request_description: String,
    pub exact_adapter_command: String,
    pub adapter: DeviceDescriptor,
    pub protocol: String,
    pub started_at_unix_ms: u64,
    pub latency_ms: u64,
    pub outcome: A30ReadOutcome,
    pub responder_header: Option<String>,
    pub response_payloads: Vec<Vec<u8>>,
    pub negative_response_code: Option<u8>,
    pub graph_values: Vec<A30GraphValue>,
    pub detail: Option<String>,
    pub raw_response: String,
    pub evidence_saved: bool,
    pub evidence_save_error: Option<String>,
}

#[derive(Default)]
pub struct A30Decoder {
    buffer: Vec<u8>,
}

impl A30Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<A30Frame>, LogNookError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            let Some(start) = self
                .buffer
                .windows(DONGLE_MAGIC.len())
                .position(|window| window == DONGLE_MAGIC)
            else {
                let keep_magic_prefix = self.buffer.last() == Some(&DONGLE_MAGIC[0]);
                let trailing = if keep_magic_prefix {
                    self.buffer.pop()
                } else {
                    None
                };
                self.buffer.clear();
                if let Some(byte) = trailing {
                    self.buffer.push(byte);
                }
                break;
            };
            if start > 0 {
                self.buffer.drain(..start);
            }
            if self.buffer.len() < 6 {
                break;
            }

            let payload_len = u16::from_be_bytes([self.buffer[4], self.buffer[5]]) as usize;
            if payload_len > MAX_A30_PAYLOAD_BYTES {
                self.buffer.clear();
                return Err(LogNookError::new(format!(
                    "A30 frame declares an oversized payload ({payload_len} bytes)"
                )));
            }
            let frame_len = 9 + payload_len;
            if self.buffer.len() < frame_len {
                break;
            }
            let frame = &self.buffer[..frame_len];
            let expected_xor = xor(&frame[..frame_len - 1]);
            if expected_xor != frame[frame_len - 1] {
                self.buffer.drain(..frame_len);
                return Err(LogNookError::new("A30 frame checksum mismatch"));
            }
            frames.push(A30Frame {
                sequence: u16::from_be_bytes([frame[2], frame[3]]),
                command: u16::from_be_bytes([frame[6], frame[7]]),
                data: frame[8..frame_len - 1].to_vec(),
            });
            self.buffer.drain(..frame_len);
        }
        Ok(frames)
    }
}

pub(crate) fn encode_host_frame(frame: &A30Frame) -> Result<Vec<u8>, LogNookError> {
    if frame.data.len() > MAX_A30_PAYLOAD_BYTES || frame.data.len() > u16::MAX as usize {
        return Err(LogNookError::new(
            "A30 frame payload exceeds the 2048-byte limit",
        ));
    }
    let mut encoded = Vec::with_capacity(9 + frame.data.len());
    encoded.extend_from_slice(&HOST_MAGIC);
    encoded.extend_from_slice(&frame.sequence.to_be_bytes());
    encoded.extend_from_slice(&(frame.data.len() as u16).to_be_bytes());
    encoded.extend_from_slice(&frame.command.to_be_bytes());
    encoded.extend_from_slice(&frame.data);
    encoded.push(xor(&encoded));
    Ok(encoded)
}

fn xor(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |checksum, byte| checksum ^ byte)
}

#[derive(Clone, Copy)]
struct A30Timing {
    handshake_wait: Duration,
    retry_wait: Duration,
    status_wait: Duration,
    poll_wait: Duration,
    config_wait: Duration,
    read_wait: Duration,
}

impl Default for A30Timing {
    fn default() -> Self {
        Self {
            handshake_wait: Duration::from_secs(2),
            retry_wait: Duration::from_millis(300),
            status_wait: Duration::from_secs(2),
            poll_wait: Duration::from_millis(50),
            config_wait: Duration::from_millis(40),
            read_wait: Duration::from_millis(400),
        }
    }
}

pub struct A30Session {
    stream: Box<dyn BluetoothByteStream>,
    decoder: A30Decoder,
    next_sequence: u16,
    timing: A30Timing,
    ready: bool,
    announced: bool,
    communication_sink: Option<CommunicationSink>,
    trace_adapter_id: Option<String>,
    last_exchange_started: Option<Instant>,
}

impl A30Session {
    pub fn open(address: &str) -> Result<Self, LogNookError> {
        Self::open_with_timing(connect_bluetooth_spp(address)?, A30Timing::default())
    }

    fn open_with_timing(
        stream: Box<dyn BluetoothByteStream>,
        timing: A30Timing,
    ) -> Result<Self, LogNookError> {
        Self::open_with_diagnostics(
            stream,
            timing,
            &noop_diagnostic_sink(),
            None,
            "identify",
            false,
            None,
        )
    }

    fn open_with_diagnostics(
        stream: Box<dyn BluetoothByteStream>,
        timing: A30Timing,
        sink: &DiagnosticSink,
        adapter_id: Option<String>,
        operation: &str,
        require_announcement: bool,
        communication_sink: Option<CommunicationSink>,
    ) -> Result<Self, LogNookError> {
        let mut session = Self {
            stream,
            decoder: A30Decoder::default(),
            next_sequence: 1,
            timing,
            ready: false,
            announced: false,
            communication_sink,
            trace_adapter_id: adapter_id.clone(),
            last_exchange_started: None,
        };

        diagnostic_stage(
            sink,
            adapter_id.clone(),
            operation,
            "relay_identity_handshake",
            vec![],
            || {
                session.send(0xFFF8, &[1, 0, 4, 0x67, 2, 1, 0])?;
                session.pump(timing.handshake_wait)?;
                for _ in 0..2 {
                    if session.ready {
                        break;
                    }
                    session.send(0xFFF8, &[1, 0, 4, 0x67, 2, 1, 0])?;
                    session.pump(timing.retry_wait)?;
                }
                if !session.ready {
                    return Err(LogNookError::new(
                        "A30 Bluetooth dongle did not answer the initial SETUP frame",
                    ));
                }

                Ok(())
            },
        )?;

        diagnostic_stage(sink, adapter_id, operation, "announcement", vec![], || {
            for _ in 0..3 {
                session.send(0xFFF9, &[1, 0, 3, 0x60, 0x06, 0])?;
                session.pump(timing.status_wait)?;
            }

            for object in [0xCB, 0xCB, 0xD0, 0xC1, 0xC0, 0xC9, 0xCB, 0xC0] {
                session.send(0xFFFA, &[1, 0, 2, 0x60, object])?;
                session.pump(timing.poll_wait)?;
            }
            if !session.announced {
                for object in [0xCB, 0x25, 0xFD] {
                    session.send(0xFFFA, &[1, 0, 2, 0x60, object])?;
                    session.pump(timing.poll_wait)?;
                }
            }

            for (command, data) in [
                (0xFFF8, &[1, 0, 4, 0x67, 2, 1, 0][..]),
                (0xFFF9, &[1, 0, 3, 0x60, 0x06, 0][..]),
                (0xFFF8, &[1, 0, 4, 0x60, 0x05, 0x07, 0x01][..]),
                (0xFFF7, &[1, 0, 5, 0x60, 0x01, 0x00, 0x8C, 0x31][..]),
                (
                    0xFFF4,
                    &[1, 0, 8, 0x60, 0x02, 0x01, 0x04, 0x03, 0x12, 0x00, 0x08][..],
                ),
                (
                    0xFFF2,
                    &[
                        1, 0, 0x0A, 0x60, 0x03, 0x00, 0x3C, 0x00, 0x64, 0x00, 0x05, 0x00, 0x00,
                    ][..],
                ),
                (
                    0xFFF5,
                    &[1, 0, 7, 0x60, 0x08, 0x01, 0x07, 0xE8, 0x07, 0xFF][..],
                ),
            ] {
                session.send(command, data)?;
                session.pump(timing.config_wait)?;
            }
            if require_announcement && !session.announced {
                return Err(LogNookError::new(if operation == "identify" {
                    "Bluetooth device did not announce the captured A30 relay descriptors"
                } else {
                    "A30 answered over Bluetooth but did not announce its relay descriptors"
                }));
            }
            Ok(())
        })?;
        Ok(session)
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_announced(&self) -> bool {
        self.announced
    }

    #[cfg(test)]
    pub(crate) fn read_mode01_pid(
        &mut self,
        pid: u8,
    ) -> Result<crate::obdlink_sx_protocol::Mode01Response, LogNookError> {
        self.read_mode01_pid_for_ecu(pid, None)
    }

    fn read_mode01_pid_for_ecu(
        &mut self,
        pid: u8,
        expected_response_header: Option<&str>,
    ) -> Result<crate::obdlink_sx_protocol::Mode01Response, LogNookError> {
        let is_bitmap = matches!(pid, 0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 | 0xC0 | 0xE0);
        if !is_bitmap
            && !crate::standard_obd_signals()
                .iter()
                .any(|signal| signal.pid == pid)
        {
            return Err(LogNookError::new(format!(
                "A30 Mode 01 PID {pid:02X} is outside the supported request set"
            )));
        }
        let (sequence, frames) = self.request_record_frames(
            0xDF,
            0x02,
            &[0x01, pid],
            0x01,
            self.timing.read_wait,
            expected_response_header,
        )?;
        let replies = frames
            .iter()
            .filter(|frame| frame.sequence == sequence)
            .map(decode_can_replies)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .filter(|reply| {
                expected_response_header
                    .is_none_or(|header| reply.response_header.eq_ignore_ascii_case(header))
            })
            .collect::<Vec<_>>();
        let mut matching = replies.into_iter().filter(|reply| {
            (reply.payload.get(0) == Some(&0x41) && reply.payload.get(1) == Some(&pid))
                || (reply.payload.as_slice().starts_with(&[0x7F, 0x01]))
        });
        let response = matching.next().ok_or_else(|| {
            LogNookError::transient_signal(format!(
                "A30 returned no response for Mode 01 PID {pid:02X}"
            ))
        })?;
        if response.truncated {
            return Err(LogNookError::signal(format!(
                "A30 Mode 01 PID {pid:02X} response was truncated"
            )));
        }
        if matching.next().is_some() {
            return Err(LogNookError::signal(
                "A30 received multiple ECU responses for one Mode 01 request",
            ));
        }
        crate::obdlink_sx_protocol::parse_mode01_payload(
            response.response_header,
            &response.payload,
            pid,
        )
        .map_err(|error| LogNookError::signal(error.to_string()))
    }

    fn send(&mut self, command: u16, data: &[u8]) -> Result<u16, LogNookError> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        let frame = A30Frame {
            sequence,
            command,
            data: data.to_vec(),
        };
        let encoded = encode_host_frame(&frame)?;
        let started = Instant::now();
        self.last_exchange_started = Some(started);
        let result = self
            .stream
            .write_all(&encoded)
            .and_then(|()| self.stream.flush())
            .map_err(|error| LogNookError::new(format!("Could not send A30 frame: {error}")));
        self.trace(
            CommunicationDirection::Tx,
            Some(&encoded),
            Some(format!("{command:04X}")),
            started,
            if result.is_ok() { "sent" } else { "error" },
        );
        result?;
        Ok(sequence)
    }

    fn request_record_frames(
        &mut self,
        object: u8,
        slot: u8,
        payload: &[u8],
        trailer: u8,
        wait: Duration,
        expected_response_header: Option<&str>,
    ) -> Result<(u16, Vec<A30Frame>), LogNookError> {
        if !self.ready || !self.announced {
            return Err(LogNookError::new(if self.ready {
                "A30 replied, but the relay did not announce its descriptors"
            } else {
                "A30 Bluetooth session is not ready"
            }));
        }
        if payload.is_empty() || payload.len() > 7 {
            return Err(LogNookError::new("A30 read payload length is invalid"));
        }
        let mut record = vec![1, 0, 0x10, 0x61, 0x04, 0, 0x0B, 8, 7, object, slot];
        record.extend_from_slice(payload);
        record.resize(18, 0);
        record.push(trailer);
        let sequence = self.send(0xFFEC, &record)?;
        let frames = self.pump_until(
            wait,
            expected_response_header.map(|header| (sequence, header)),
        )?;
        Ok((sequence, frames))
    }

    fn pump(&mut self, duration: Duration) -> Result<Vec<A30Frame>, LogNookError> {
        self.pump_until(duration, None)
    }

    fn pump_until(
        &mut self,
        duration: Duration,
        expected_response: Option<(u16, &str)>,
    ) -> Result<Vec<A30Frame>, LogNookError> {
        let deadline = Instant::now() + duration;
        let exchange_started = self.last_exchange_started.unwrap_or_else(Instant::now);
        let mut first_read = true;
        let mut bytes = [0_u8; 256];
        let mut received = Vec::new();
        loop {
            if !duration.is_zero() && !first_read && Instant::now() >= deadline {
                break;
            }
            first_read = false;
            match self.stream.read(&mut bytes) {
                Ok(0) => {
                    self.trace(
                        CommunicationDirection::Rx,
                        None,
                        None,
                        exchange_started,
                        "disconnected",
                    );
                    return Err(LogNookError::transient(
                        "A30 Bluetooth link closed while receiving dongle frames",
                    ));
                }
                Ok(count) => {
                    self.trace(
                        CommunicationDirection::Rx,
                        Some(&bytes[..count]),
                        None,
                        exchange_started,
                        "frame_bytes",
                    );
                    let mut got_expected_response = false;
                    for frame in self.decoder.feed(&bytes[..count])? {
                        self.ready = true;
                        if !is_bare_ack(&frame) {
                            self.announced = true;
                        }
                        if expected_response.is_some_and(|(sequence, header)| {
                            frame.sequence == sequence && has_complete_reply_for(&frame, header)
                        }) {
                            got_expected_response = true;
                        }
                        received.push(frame);
                    }
                    if got_expected_response {
                        break;
                    }
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    if duration.is_zero() || Instant::now() >= deadline {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => {
                    self.trace(
                        CommunicationDirection::Rx,
                        None,
                        None,
                        exchange_started,
                        "error",
                    );
                    return Err(LogNookError::new(format!(
                        "A30 Bluetooth read failed: {error}"
                    )));
                }
            }
        }
        if received.is_empty() {
            self.trace(
                CommunicationDirection::Rx,
                None,
                None,
                exchange_started,
                "timeout",
            );
        }
        Ok(received)
    }

    fn trace(
        &self,
        direction: CommunicationDirection,
        bytes: Option<&[u8]>,
        route: Option<String>,
        started: Instant,
        outcome: &str,
    ) {
        let Some(sink) = &self.communication_sink else {
            return;
        };
        let Some(adapter_id) = &self.trace_adapter_id else {
            return;
        };
        let mut event = CommunicationTraceEvent::new(adapter_id, "a30_frame_exchange", direction);
        event.elapsed_ns = Some(started.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        event.route = route;
        event.bytes_hex = bytes.map(bytes_to_hex);
        event.outcome = outcome.into();
        if bytes.is_none() {
            event.raw_bytes_unavailable =
                Some("The Bluetooth driver returned no bytes for this outcome".into());
        }
        let _ = sink.try_record(event);
    }
}

pub struct A30Adapter {
    address: String,
    session: Option<A30Session>,
    supported_pids: Vec<u8>,
    ecu_id: Option<String>,
    started: Option<Instant>,
    diagnostic_sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
    #[cfg(test)]
    test_stream: Option<Box<dyn BluetoothByteStream>>,
    #[cfg(test)]
    test_timing: A30Timing,
}

impl A30Adapter {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            session: None,
            supported_pids: Vec::new(),
            ecu_id: None,
            started: None,
            diagnostic_sink: noop_diagnostic_sink(),
            communication_sink: None,
            #[cfg(test)]
            test_stream: None,
            #[cfg(test)]
            test_timing: A30Timing::default(),
        }
    }

    #[cfg(test)]
    fn with_stream_for_test(
        address: impl Into<String>,
        stream: Box<dyn BluetoothByteStream>,
        timing: A30Timing,
    ) -> Self {
        let mut adapter = Self::new(address);
        adapter.test_stream = Some(stream);
        adapter.test_timing = timing;
        adapter
    }

    fn open_session(&mut self, operation: &str) -> Result<A30Session, LogNookError> {
        let sink = self.diagnostic_sink.clone();
        let id = a30_device(&self.address)
            .ok()
            .map(|device| device.stable_id);
        let timing = {
            #[cfg(test)]
            {
                self.test_timing
            }
            #[cfg(not(test))]
            {
                A30Timing::default()
            }
        };
        let stream = diagnostic_stage(
            &sink,
            id.clone(),
            operation,
            "bluetooth_session_open",
            vec![],
            || {
                #[cfg(test)]
                if let Some(stream) = self.test_stream.take() {
                    return Ok(stream);
                }
                connect_bluetooth_spp(&self.address)
            },
        )?;
        A30Session::open_with_diagnostics(
            stream,
            timing,
            &sink,
            id,
            operation,
            true,
            self.communication_sink.clone(),
        )
    }

    fn identify_inner(&mut self) -> Result<DeviceDescriptor, LogNookError> {
        let session = self.open_session("identify")?;
        if !session.is_announced() {
            return Err(LogNookError::new(
                "Bluetooth device did not announce the captured A30 relay descriptors",
            ));
        }
        a30_device(&self.address)
    }

    fn connect_inner(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        let mut session = self.open_session("connect")?;
        if !session.is_announced() {
            return Err(LogNookError::new(
                "A30 answered over Bluetooth but did not announce its relay descriptors",
            ));
        }

        let sink = self.diagnostic_sink.clone();
        let id = a30_device(&self.address)
            .ok()
            .map(|device| device.stable_id);
        let (supported_pids, ecu_id, supported_signals) = diagnostic_stage(
            &sink,
            id,
            "connect",
            "supported_pid_discovery",
            vec![],
            || {
                let mut supported_pids = Vec::new();
                let mut ecu_id: Option<String> = None;
                let mut start = 0;
                loop {
                    let response = session.read_mode01_pid_for_ecu(start, ecu_id.as_deref())?;
                    let responder = response.ecu_id.clone().ok_or_else(|| {
                        LogNookError::new("A30 supported-PID response did not identify its ECU")
                    })?;
                    if ecu_id
                        .as_ref()
                        .is_some_and(|previous| previous != &responder)
                    {
                        return Err(LogNookError::new(
                            "ECU identity changed during A30 supported-PID discovery",
                        ));
                    }
                    ecu_id.get_or_insert(responder.clone());
                    let range = parse_supported_pid_payload(responder, &response.raw_bytes, start)
                        .map_err(LogNookError::new)?;
                    supported_pids.extend(range.pids);
                    if !range.has_more || start == 0xE0 {
                        break;
                    }
                    start += 0x20;
                }
                supported_pids.sort_unstable();
                supported_pids.dedup();
                let supported_signals = standard_obd_signals()
                    .into_iter()
                    .filter(|signal| supported_pids.contains(&signal.pid))
                    .collect::<Vec<_>>();
                if supported_signals.is_empty() {
                    return Err(LogNookError::new(
                        "ECU reports no supported LogNook Mode 01 signals over A30",
                    ));
                }

                Ok((supported_pids, ecu_id, supported_signals))
            },
        )?;

        let device = a30_device(&self.address)?;
        self.session = Some(session);
        self.supported_pids = supported_pids;
        self.ecu_id = ecu_id;
        self.started = Some(Instant::now());
        Ok(ConnectionCapabilities {
            device,
            protocol: "SAE J1979 Mode 01 via Xtool A30/A30M".into(),
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
}

impl HardwareAdapter for A30Adapter {
    fn set_diagnostic_sink(&mut self, sink: DiagnosticSink) {
        self.diagnostic_sink = sink;
    }

    fn set_communication_sink(&mut self, sink: CommunicationSink) {
        self.communication_sink = Some(sink);
    }

    fn validate_oem_candidate_poll(&self, spec: &OemCandidateTrySpec) -> Result<(), LogNookError> {
        candidate_request(spec).map(|_| ())
    }

    fn execute_research_test(
        &mut self,
        request: crate::ResearchTestRequest,
    ) -> Result<crate::ResearchTestResult, LogNookError> {
        if self.started.is_none() {
            return Err(LogNookError::new("A30 adapter is not connected"));
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| LogNookError::new("A30 adapter is not connected"))?;
        match request {
            crate::ResearchTestRequest::OemCandidate(spec) => {
                read_a30_oem_candidate(session, &self.address, &spec)
                    .map(crate::ResearchTestResult::OemCandidate)
            }
            crate::ResearchTestRequest::A30ReadTest(test) => {
                read_a30_test(session, &self.address, test)
                    .map(crate::ResearchTestResult::A30ReadTest)
            }
            crate::ResearchTestRequest::Diagnostic(query) => {
                if query.kind == crate::DiagnosticReadKind::ClearDtcs {
                    return Err(LogNookError::new(
                        "Mode 04 is available only through the explicit DTC clear operation",
                    ));
                }
                read_a30_vehicle_diagnostic(session, &self.address, query)
                    .map(crate::ResearchTestResult::Diagnostic)
            }
        }
    }

    fn clear_vehicle_dtcs(&mut self) -> Result<Vec<crate::DiagnosticRead>, LogNookError> {
        if self.started.is_none() {
            return Err(LogNookError::new("A30 adapter is not connected"));
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| LogNookError::new("A30 adapter is not connected"))?;
        read_a30_vehicle_diagnostic(
            session,
            &self.address,
            crate::DiagnosticQueryRequest::clear_dtcs(),
        )
        .map(|report| report.reads)
    }

    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.started.is_some() {
            return Err(LogNookError::new("A30 adapter is already connected"));
        }
        let result = self.connect_inner();
        if result.is_err() {
            self.disconnect();
        }
        result
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| LogNookError::new("A30 adapter is not connected"))?;
        let started = self
            .started
            .ok_or_else(|| LogNookError::new("A30 adapter is not connected"))?;
        let catalog = standard_obd_signals();
        for signal in signals {
            if !catalog.contains(signal) || !self.supported_pids.contains(&signal.pid) {
                return Err(LogNookError::new(format!(
                    "Unsupported read-only Mode 01 signal: {}",
                    signal.id
                )));
            }
        }

        let sink = self.diagnostic_sink.clone();
        let id = a30_device(&self.address)
            .ok()
            .map(|device| device.stable_id);
        collect_samples_independently(signals, |signal| {
            diagnostic_stage(
                &sink,
                id.clone(),
                "acquisition",
                "pid_read",
                vec![signal.id.clone()],
                || {
                    let response =
                        session.read_mode01_pid_for_ecu(signal.pid, self.ecu_id.as_deref())?;
                    if response.ecu_id.as_deref().is_some_and(|id| {
                        self.ecu_id
                            .as_deref()
                            .is_some_and(|connected| connected != id)
                    }) {
                        return Err(LogNookError::new(
                            "ECU identity changed during the A30 session",
                        ));
                    }
                    let elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
                    Ok(vec![Sample {
                        elapsed_ns,
                        wall_time_unix_ns: now_unix_ns(),
                        source_id: format!(
                            "xtool-a30:{}",
                            canonical_bluetooth_address(&self.address)?
                        ),
                        ecu_id: response.ecu_id.clone().or_else(|| self.ecu_id.clone()),
                        signal_id: signal.id.clone(),
                        value: decode_signal(signal, &response).map_err(LogNookError::signal)?,
                        unit: signal.unit.clone(),
                        quality: SampleQuality::Valid,
                        origin: SampleOrigin::Live,
                        raw_bytes: Some(response.raw_bytes),
                    }])
                },
            )
        })
    }

    fn disconnect(&mut self) {
        self.session = None;
        self.supported_pids.clear();
        self.ecu_id = None;
        self.started = None;
    }
}

pub fn try_a30_oem_candidate(
    address: impl Into<String>,
    spec: &OemCandidateTrySpec,
) -> Result<OemCandidateTryResult, LogNookError> {
    let address = address.into();
    build_oem_candidate_command(spec)?;
    let session = A30Session::open(&address)?;
    run_a30_oem_candidate(session, &address, spec)
}

fn run_a30_oem_candidate(
    mut session: A30Session,
    address: &str,
    spec: &OemCandidateTrySpec,
) -> Result<OemCandidateTryResult, LogNookError> {
    read_a30_oem_candidate(&mut session, address, spec)
}

fn read_a30_oem_candidate(
    session: &mut A30Session,
    address: &str,
    spec: &OemCandidateTrySpec,
) -> Result<OemCandidateTryResult, LogNookError> {
    build_oem_candidate_command(spec)?;
    let (object, slot, payload) = candidate_request(spec)?;
    let device = a30_device(address)?;
    let started_at_unix_ms = now_unix_ns().max(0) as u64 / 1_000_000;
    let request_started = Instant::now();
    let (sequence, frames) = session.request_record_frames(
        object,
        slot,
        &payload,
        0xFF,
        session.timing.read_wait,
        Some(&spec.candidate.response_header),
    )?;
    let latency_ms = request_started.elapsed().as_millis() as u64;
    let record = build_record_payload(object, slot, &payload, 0xFF);
    let request_frame = A30Frame {
        sequence,
        command: 0xFFEC,
        data: record,
    };
    let exact_adapter_command = format!(
        "A30 RFCOMM TX {}",
        hex_bytes(&encode_host_frame(&request_frame)?)
    );
    let raw_response = frames
        .iter()
        .map(|frame| hex_bytes(&encode_dongle_frame(frame)))
        .collect::<Vec<_>>()
        .join("\n");
    let replies = frames
        .iter()
        .filter(|frame| frame.sequence == sequence)
        .map(decode_can_replies)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let truncated_reply = replies.iter().find(|reply| {
        reply
            .response_header
            .eq_ignore_ascii_case(&spec.candidate.response_header)
            && reply.truncated
    });
    let transcript = replies
        .iter()
        .filter(|reply| !reply.truncated)
        .flat_map(isotp_transcript)
        .collect::<String>();
    let mut decoded = decode_oem_candidate_response(spec, &transcript);
    if let Some(reply) = truncated_reply {
        decoded.outcome = OemCandidateOutcome::MalformedResponse;
        decoded.responder_header = Some(reply.response_header.clone());
        decoded.payload = reply.payload.clone();
        decoded.decoded_value = None;
        decoded.error = Some("A30 returned a truncated ISO-TP block".into());
        decoded.decode_error = None;
    }
    Ok(OemCandidateTryResult {
        candidate_id: spec.candidate.candidate_id.clone(),
        exact_adapter_command,
        adapter: device,
        protocol: "Xtool A30/A30M Bluetooth SPP".into(),
        started_at_unix_ms,
        latency_ms,
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

pub fn run_a30_read_test(
    address: impl Into<String>,
    test: A30ReadTest,
) -> Result<A30ReadTestResult, LogNookError> {
    let address = address.into();
    let session = A30Session::open(&address)?;
    run_a30_read_test_with_session(session, &address, test)
}

fn run_a30_read_test_with_session(
    mut session: A30Session,
    address: &str,
    test: A30ReadTest,
) -> Result<A30ReadTestResult, LogNookError> {
    read_a30_test(&mut session, address, test)
}

fn read_a30_test(
    session: &mut A30Session,
    address: &str,
    test: A30ReadTest,
) -> Result<A30ReadTestResult, LogNookError> {
    let device = a30_device(&address)?;
    let (description, service, object, slot, payload, trailer, wait, expected_header) =
        test.request();
    let wait = if session.timing.read_wait.is_zero() {
        Duration::ZERO
    } else {
        wait
    };
    let started_at_unix_ms = now_unix_ns().max(0) as u64 / 1_000_000;
    let request_started = Instant::now();
    let (sequence, frames) =
        session.request_record_frames(object, slot, &payload, trailer, wait, expected_header)?;
    let latency_ms = request_started.elapsed().as_millis() as u64;
    let request_frame = A30Frame {
        sequence,
        command: 0xFFEC,
        data: build_record_payload(object, slot, &payload, trailer),
    };
    let replies = frames
        .iter()
        .filter(|frame| frame.sequence == sequence)
        .map(decode_can_replies)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let raw_response = frames
        .iter()
        .map(|frame| hex_bytes(&encode_dongle_frame(frame)))
        .collect::<Vec<_>>()
        .join("\n");
    let responder_header = replies.first().map(|reply| reply.response_header.clone());
    let multiple_responders = responder_header.as_ref().is_some_and(|first| {
        replies
            .iter()
            .skip(1)
            .any(|reply| !reply.response_header.eq_ignore_ascii_case(first))
    });
    let response_payloads = replies.iter().map(|reply| reply.payload.clone()).collect();
    let negative_response_code = replies.iter().find_map(|reply| {
        (reply.payload.len() >= 3 && reply.payload[0] == 0x7F && reply.payload[1] == service)
            .then(|| reply.payload[2])
    });
    let wrong_responder = multiple_responders
        || expected_header.is_some_and(|expected| {
            !replies.is_empty()
                && replies
                    .iter()
                    .any(|reply| !reply.response_header.eq_ignore_ascii_case(expected))
        });
    let malformed = replies.iter().any(|reply| reply.truncated);
    let has_positive = replies
        .iter()
        .any(|reply| reply.payload.first() == Some(&service.wrapping_add(0x40)));
    let outcome = if replies.is_empty() {
        A30ReadOutcome::NoResponse
    } else if wrong_responder {
        A30ReadOutcome::WrongResponder
    } else if malformed {
        A30ReadOutcome::MalformedResponse
    } else if let Some(nrc) = negative_response_code {
        match nrc {
            0x78 => A30ReadOutcome::ResponsePending,
            0x21 | 0x22 => A30ReadOutcome::TemporarilyUnavailable,
            0x7E | 0x7F => A30ReadOutcome::SessionRequired,
            0x33 => A30ReadOutcome::SecurityRequired,
            _ => A30ReadOutcome::NegativeResponse,
        }
    } else if has_positive {
        A30ReadOutcome::Positive
    } else {
        A30ReadOutcome::MalformedResponse
    };
    let graph_values = if outcome == A30ReadOutcome::Positive {
        decode_read_test_values(test, &replies)
    } else {
        Vec::new()
    };
    Ok(A30ReadTestResult {
        test,
        request_description: description,
        exact_adapter_command: format!(
            "A30 RFCOMM TX {}",
            hex_bytes(&encode_host_frame(&request_frame)?)
        ),
        adapter: device,
        protocol: "Xtool A30/A30M Bluetooth SPP".into(),
        started_at_unix_ms,
        latency_ms,
        outcome,
        responder_header,
        response_payloads,
        negative_response_code,
        graph_values,
        detail: if multiple_responders {
            Some("Multiple ECU responders were returned; raw replies were preserved but values were not graphed.".into())
        } else {
            read_test_detail(test, &replies)
        },
        raw_response,
        evidence_saved: false,
        evidence_save_error: None,
    })
}

fn read_a30_vehicle_diagnostic(
    session: &mut A30Session,
    address: &str,
    query: crate::DiagnosticQueryRequest,
) -> Result<crate::DiagnosticReport, LogNookError> {
    query.validate().map_err(LogNookError::new)?;
    let adapter = a30_device(address)?;
    let started_at_unix_ms = crate::vehicle_diagnostics::now_unix_ms();
    let mut reads = Vec::new();
    let adapter_started_at_unix_ms = crate::vehicle_diagnostics::now_unix_ms();
    let request_started = Instant::now();
    let trailer = if query.service == 0x06 { 0x01 } else { 0xFF };
    let request_frame = A30Frame {
        sequence: session.next_sequence,
        command: 0xFFEC,
        data: build_record_payload(query.service, 0xDF, &query.payload, trailer),
    };
    let exact_adapter_command = format!(
        "A30 RFCOMM TX {}",
        hex_bytes(&encode_host_frame(&request_frame)?)
    );
    let route_restore_error = None;
    match session.request_record_frames(
        query.service,
        0xDF,
        &query.payload,
        trailer,
        Duration::from_millis(query.wait_ms),
        None,
    ) {
        Ok((sequence, frames)) => {
            let latency_ms = request_started.elapsed().as_millis() as u64;
            let raw_response = frames
                .iter()
                .map(|frame| hex_bytes(&encode_dongle_frame(frame)))
                .collect::<Vec<_>>()
                .join("\n");
            let mut replies = Vec::new();
            let mut decode_errors = Vec::new();
            for frame in frames.iter().filter(|frame| frame.sequence == sequence) {
                match decode_can_replies(frame) {
                    Ok(mut decoded) => replies.append(&mut decoded),
                    Err(error) => decode_errors.push(error.to_string()),
                }
            }
            if !decode_errors.is_empty() {
                reads.push(crate::vehicle_diagnostics::malformed_response(
                    &query,
                    raw_response.clone(),
                    exact_adapter_command.clone(),
                    adapter_started_at_unix_ms,
                    latency_ms,
                    decode_errors.join("; "),
                ));
            }
            if replies.is_empty() && decode_errors.is_empty() {
                reads.push(query.no_response(
                    raw_response,
                    exact_adapter_command,
                    adapter_started_at_unix_ms,
                    latency_ms,
                    None,
                ));
            } else {
                reads.extend(replies.into_iter().map(|reply| {
                    query.report(
                        Some(reply.response_header),
                        reply.payload,
                        raw_response.clone(),
                        exact_adapter_command.clone(),
                        adapter_started_at_unix_ms,
                        latency_ms,
                        reply.truncated,
                    )
                }));
            }
        }
        Err(error) if error.is_transient() => reads.push(query.no_response(
            String::new(),
            exact_adapter_command,
            adapter_started_at_unix_ms,
            request_started.elapsed().as_millis() as u64,
            Some(error.to_string()),
        )),
        Err(error) => reads.push(query.transport_error(
            String::new(),
            exact_adapter_command,
            adapter_started_at_unix_ms,
            request_started.elapsed().as_millis() as u64,
            error.to_string(),
        )),
    }
    Ok(crate::DiagnosticReport {
        report_id: None,
        connection_generation: None,
        vehicle_context: None,
        adapter,
        protocol: "Xtool A30/A30M Bluetooth SPP · SAE J1979 functional CAN".into(),
        started_at_unix_ms,
        operation_kind: crate::DiagnosticReportKind::Snapshot,
        request: crate::DiagnosticRequest::default(),
        reads,
        route_restore_error,
        evidence_saved: false,
        evidence_save_error: None,
    })
}

impl A30ReadTest {
    pub(crate) fn request(
        self,
    ) -> (
        String,
        u8,
        u8,
        u8,
        Vec<u8>,
        u8,
        Duration,
        Option<&'static str>,
    ) {
        match self {
            Self::Mode01EngineRpm => (
                "Mode 01 PID 0C · engine speed on 7DF".into(),
                0x01,
                0xDF,
                0x02,
                vec![0x01, 0x0C],
                0x01,
                Duration::from_millis(600),
                None,
            ),
            Self::Mode02FreezeFrameRpm => (
                "Mode 02 freeze frame PID 0C · request 02 0C 00 on 7DF".into(),
                0x02,
                0xDF,
                0x03,
                vec![0x02, 0x0C, 0x00],
                0xFF,
                Duration::from_millis(600),
                None,
            ),
            Self::Mode03StoredDtcs => (
                "Mode 03 stored DTCs · request 03 on 7DF".into(),
                0x03,
                0xDF,
                0x01,
                vec![0x03],
                0xFF,
                Duration::from_millis(900),
                None,
            ),
            Self::Mode06Bitmap00
            | Self::Mode06Bitmap20
            | Self::Mode06Bitmap40
            | Self::Mode06Bitmap60
            | Self::Mode06Bitmap80
            | Self::Mode06BitmapA0 => {
                let start = match self {
                    Self::Mode06Bitmap00 => 0x00,
                    Self::Mode06Bitmap20 => 0x20,
                    Self::Mode06Bitmap40 => 0x40,
                    Self::Mode06Bitmap60 => 0x60,
                    Self::Mode06Bitmap80 => 0x80,
                    Self::Mode06BitmapA0 => 0xA0,
                    _ => unreachable!(),
                };
                (
                    format!("Mode 06 monitor bitmap {start:02X} · request 06 {start:02X} on 7DF"),
                    0x06,
                    0xDF,
                    0x02,
                    vec![0x06, start],
                    0x01,
                    Duration::from_millis(600),
                    None,
                )
            }
            Self::Mode06Tid35 | Self::Mode06TidA2 | Self::Mode06TidA5 => {
                let tid = match self {
                    Self::Mode06Tid35 => 0x35,
                    Self::Mode06TidA2 => 0xA2,
                    Self::Mode06TidA5 => 0xA5,
                    _ => unreachable!(),
                };
                (
                    format!("Mode 06 Scion TID {tid:02X} · result values use raw count"),
                    0x06,
                    0xDF,
                    0x02,
                    vec![0x06, tid],
                    0x01,
                    Duration::from_millis(600),
                    None,
                )
            }
            Self::Mode07PendingDtcs => (
                "Mode 07 pending DTCs · request 07 on 7DF".into(),
                0x07,
                0xDF,
                0x01,
                vec![0x07],
                0xFF,
                Duration::from_millis(900),
                None,
            ),
            Self::Mode09Vin => (
                "Mode 09 VIN · request 09 02 on captured Scion engine route 7E0".into(),
                0x09,
                0xE0,
                0x02,
                vec![0x09, 0x02],
                0xFC,
                Duration::from_millis(1_500),
                Some("7E8"),
            ),
            Self::Mode0aPermanentDtcs => (
                "Mode 0A permanent DTCs · request 0A on 7DF".into(),
                0x0A,
                0xDF,
                0x01,
                vec![0x0A],
                0xFF,
                Duration::from_millis(900),
                None,
            ),
        }
    }
}

pub(crate) fn decode_read_test_values(
    test: A30ReadTest,
    replies: &[A30CanReply],
) -> Vec<A30GraphValue> {
    match test {
        A30ReadTest::Mode01EngineRpm | A30ReadTest::Mode02FreezeFrameRpm => replies
            .iter()
            .filter_map(|reply| {
                let payload = &reply.payload;
                let freeze_frame = test == A30ReadTest::Mode02FreezeFrameRpm;
                let valid_reply = if freeze_frame {
                    payload.len() == 5 && payload[..3] == [0x42, 0x0C, 0]
                } else {
                    payload.len() == 4 && payload[..2] == [0x41, 0x0C]
                };
                if !valid_reply {
                    return None;
                }
                let data = if freeze_frame {
                    &payload[3..]
                } else {
                    &payload[2..]
                };
                let mut raw_bytes = vec![0x41, 0x0C];
                raw_bytes.extend_from_slice(data);
                let response = crate::obdlink_sx_protocol::Mode01Response {
                    ecu_id: Some(reply.response_header.clone()),
                    data: data.to_vec(),
                    raw_bytes,
                };
                let signal = standard_obd_signals()
                    .into_iter()
                    .find(|signal| signal.pid == 0x0C)?;
                let value = decode_signal(&signal, &response).ok()?;
                Some(A30GraphValue {
                    signal_id: if freeze_frame {
                        "a30_mode02_engine_rpm".into()
                    } else {
                        "engine_rpm".into()
                    },
                    name: if freeze_frame {
                        "Mode 02 freeze frame · Engine speed".into()
                    } else {
                        "Engine speed".into()
                    },
                    service: if freeze_frame { 0x02 } else { 0x01 },
                    pid: Some(0x0C),
                    value,
                    unit: signal.unit,
                    raw_value: hex_bytes(data),
                    response_payload: Some(reply.payload.clone()),
                })
            })
            .collect(),
        A30ReadTest::Mode06Tid35 | A30ReadTest::Mode06TidA2 | A30ReadTest::Mode06TidA5 => replies
            .iter()
            .flat_map(|reply| {
                let mut values = Vec::new();
                let payload = &reply.payload;
                if payload.first() != Some(&0x46) {
                    return values;
                }
                for group in payload[1..].chunks_exact(9) {
                    let tid = group[0];
                    let mid = group[1];
                    let value = u16::from_be_bytes([group[3], group[4]]) as f64;
                    values.push(A30GraphValue {
                        signal_id: format!("a30_mode06_tid_{tid:02x}_mid_{mid:02x}"),
                        name: format!("Mode 06 TID {tid:02X} MID {mid:02X}"),
                        service: 0x06,
                        pid: None,
                        value,
                        unit: "raw count".into(),
                        raw_value: format!("{}", value as u64),
                        response_payload: Some(reply.payload.clone()),
                    });
                }
                values
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn read_test_detail(test: A30ReadTest, replies: &[A30CanReply]) -> Option<String> {
    match test {
        A30ReadTest::Mode09Vin => replies.iter().find_map(|reply| {
            let payload = &reply.payload;
            (payload.len() >= 20 && payload[..3] == [0x49, 0x02, 0x01])
                .then(|| String::from_utf8_lossy(&payload[3..20]).into_owned())
        }),
        _ => None,
    }
}

fn candidate_request(spec: &OemCandidateTrySpec) -> Result<(u8, u8, Vec<u8>), LogNookError> {
    let candidate = &spec.candidate;
    let service = u8::from_str_radix(&candidate.service, 16)
        .map_err(|error| LogNookError::new(format!("Invalid candidate service: {error}")))?;
    let request_header = u16::from_str_radix(&candidate.request_header, 16)
        .map_err(|error| LogNookError::new(format!("Invalid A30 request CAN ID: {error}")))?;
    if !(0x700..=0x7FF).contains(&request_header) {
        return Err(LogNookError::new(
            "A30 OEM reads require the source-backed 0x7xx CAN request route",
        ));
    }
    let slot = match service {
        0x21 => 0x02,
        0x22 => 0x03,
        _ => {
            return Err(LogNookError::new(
                "A30 OEM reads support only services 21 and 22",
            ));
        }
    };
    let mut payload = vec![service];
    let id = candidate.identifier.as_bytes();
    for pair in id.chunks_exact(2) {
        let digits = std::str::from_utf8(pair)
            .map_err(|error| LogNookError::new(format!("Invalid candidate identifier: {error}")))?;
        payload.push(u8::from_str_radix(digits, 16).map_err(|error| {
            LogNookError::new(format!("Invalid candidate identifier: {error}"))
        })?);
    }
    Ok((request_header as u8, slot, payload))
}

fn isotp_transcript(reply: &A30CanReply) -> Vec<String> {
    let payload = &reply.payload;
    if payload.is_empty() {
        return Vec::new();
    }
    let mut frames = Vec::new();
    if payload.len() <= 7 {
        let mut frame = vec![payload.len() as u8];
        frame.extend_from_slice(payload);
        frames.push(format!("{} {}\r", reply.response_header, hex_bytes(&frame)));
        return frames;
    }
    let first_len = payload.len().min(6);
    let mut first = vec![
        0x10 | ((payload.len() >> 8) as u8 & 0x0F),
        payload.len() as u8,
    ];
    first.extend_from_slice(&payload[..first_len]);
    frames.push(format!("{} {}\r", reply.response_header, hex_bytes(&first)));
    for (sequence, chunk) in payload[first_len..].chunks(7).enumerate() {
        let mut consecutive = vec![0x20 | (((sequence + 1) as u8) & 0x0F)];
        consecutive.extend_from_slice(chunk);
        frames.push(format!(
            "{} {}\r",
            reply.response_header,
            hex_bytes(&consecutive)
        ));
    }
    frames
}

fn encode_dongle_frame(frame: &A30Frame) -> Vec<u8> {
    let mut encoded = vec![0xAA, 0x55];
    encoded.extend_from_slice(&frame.sequence.to_be_bytes());
    encoded.extend_from_slice(&(frame.data.len() as u16).to_be_bytes());
    encoded.extend_from_slice(&frame.command.to_be_bytes());
    encoded.extend_from_slice(&frame.data);
    encoded.push(xor(&encoded));
    encoded
}

fn build_record_payload(object: u8, slot: u8, payload: &[u8], trailer: u8) -> Vec<u8> {
    let mut record = vec![1, 0, 0x10, 0x61, 0x04, 0, 0x0B, 8, 7, object, slot];
    record.extend_from_slice(payload);
    record.resize(18, 0);
    record.push(trailer);
    record
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn identify_a30(address: impl Into<String>) -> Result<DeviceDescriptor, LogNookError> {
    identify_a30_with_diagnostics(address, noop_diagnostic_sink())
}

pub fn identify_a30_with_diagnostics(
    address: impl Into<String>,
    sink: DiagnosticSink,
) -> Result<DeviceDescriptor, LogNookError> {
    identify_a30_with_sinks(address, sink, None)
}

pub fn identify_a30_with_sinks(
    address: impl Into<String>,
    sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
) -> Result<DeviceDescriptor, LogNookError> {
    let mut adapter = A30Adapter::new(address);
    adapter.set_diagnostic_sink(sink);
    if let Some(communication_sink) = communication_sink {
        adapter.set_communication_sink(communication_sink);
    }
    adapter.identify_inner()
}

fn a30_device(address: &str) -> Result<DeviceDescriptor, LogNookError> {
    let address = canonical_bluetooth_address(address)?;
    Ok(DeviceDescriptor {
        stable_id: format!("xtool-a30:{address}"),
        hardware_family: "Xtool A30/A30M Bluetooth SPP".into(),
        firmware: None,
        path: Some(address),
    })
}

fn canonical_bluetooth_address(address: &str) -> Result<String, LogNookError> {
    let value = crate::parse_bluetooth_address(address)?;
    Ok(crate::format_bluetooth_address(value))
}

fn now_unix_ns() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}

fn is_bare_ack(frame: &A30Frame) -> bool {
    (frame.command == 0xFFFA && frame.data == [0, 0, 2, 0, 0])
        || (frame.command == 0xFFFB && matches!(frame.data.as_slice(), [1, 0, 1, 0] | [1, 0, 1, 1]))
}

pub(crate) fn decode_can_replies(frame: &A30Frame) -> Result<Vec<A30CanReply>, LogNookError> {
    match frame.command {
        0xFFF1 => decode_notify(&frame.data),
        0xFFE4 | 0xFFD7 | 0xFFCA | 0xFFBD | 0xFF6F | 0xFF55 => {
            decode_record_window_block(&frame.data)
        }
        0xFF4F | 0xFA4C => decode_long_record_blocks(&frame.data),
        _ => Ok(Vec::new()),
    }
}

fn has_complete_reply_for(frame: &A30Frame, expected_header: &str) -> bool {
    decode_can_replies(frame).is_ok_and(|replies| {
        replies.iter().any(|reply| {
            !reply.truncated && reply.response_header.eq_ignore_ascii_case(expected_header)
        })
    })
}

fn decode_long_record_blocks(data: &[u8]) -> Result<Vec<A30CanReply>, LogNookError> {
    let count = data
        .first()
        .copied()
        .ok_or_else(|| LogNookError::new("A30 long-record reply has no record count"))?
        as usize;
    if count > 64 {
        return Err(LogNookError::new(
            "A30 long-record reply has an invalid record count",
        ));
    }
    let mut channels = std::collections::BTreeMap::<u8, Vec<u8>>::new();
    let mut offset = 1;
    for _ in 0..count {
        if data.len().saturating_sub(offset) < 7 || data[offset + 5] != 0x10 {
            return Err(LogNookError::new("A30 long-record header is malformed"));
        }
        let object = data[offset + 4];
        let length = data[offset + 6] as usize;
        let end = offset + 7 + length;
        if end > data.len() {
            return Err(LogNookError::new("A30 long-record payload is truncated"));
        }
        channels
            .entry(object)
            .or_default()
            .extend_from_slice(&data[offset + 7..end]);
        offset = end;
    }
    Ok(channels
        .into_iter()
        .map(|(object, payload)| A30CanReply {
            response_header: response_header(object),
            payload,
            truncated: false,
        })
        .collect())
}

fn decode_notify(data: &[u8]) -> Result<Vec<A30CanReply>, LogNookError> {
    let count = data
        .first()
        .copied()
        .ok_or_else(|| LogNookError::new("A30 NOTIFY response has no record count"))?
        as usize;
    let mut replies = Vec::with_capacity(count);
    let mut offset = 1;
    for _ in 0..count {
        if data.len().saturating_sub(offset) < 6
            || data[offset..offset + 4] != [0x00, 0x0B, 0x08, 0x07]
        {
            return Err(LogNookError::new("A30 NOTIFY record header is malformed"));
        }
        let object = data[offset + 4];
        let declared = data[offset + 5] as usize;
        let body_start = offset + 6;
        let body_end = (offset + 14).min(data.len());
        let available = body_end.saturating_sub(body_start);
        let take = declared.min(available);
        replies.push(A30CanReply {
            response_header: response_header(object),
            payload: data[body_start..body_start + take].to_vec(),
            truncated: take < declared,
        });
        offset += 13;
    }
    Ok(replies)
}

fn decode_record_window_block(data: &[u8]) -> Result<Vec<A30CanReply>, LogNookError> {
    let count = data
        .first()
        .copied()
        .ok_or_else(|| LogNookError::new("A30 block reply has no record count"))?
        as usize;
    if count > 26 {
        return Err(LogNookError::new(
            "A30 block reply has an invalid record count",
        ));
    }
    let mut windows = Vec::with_capacity(count);
    let mut offset = 1;
    if data.get(offset) == Some(&0) {
        offset += 1;
    }
    for _ in 0..count {
        if data.len().saturating_sub(offset) < 5 || data[offset..offset + 3] != [0x0B, 0x08, 0x07] {
            return Err(LogNookError::new(
                "A30 block reply record header is malformed",
            ));
        }
        let object = data[offset + 3];
        let pci = data[offset + 4];
        let end = (offset + 13).min(data.len());
        windows.push((object, pci, data[offset + 5..end].to_vec()));
        offset = end;
    }

    let mut replies = Vec::new();
    let mut current: Option<(String, usize, Vec<u8>, bool)> = None;
    for (index, (object, pci, window)) in windows.iter().enumerate() {
        if *pci == 0x10 {
            if let Some((header, expected, payload, saw_continuation)) = current.take()
                && saw_continuation
                && payload.len() >= 3
            {
                let truncated = expected > payload.len();
                replies.push(A30CanReply {
                    response_header: header,
                    payload,
                    truncated,
                });
            }
            if window.len() < 2 {
                continue;
            }
            let expected = window[0] as usize;
            let payload = window[1..]
                .iter()
                .copied()
                .take(6)
                .take(expected)
                .collect::<Vec<_>>();
            if payload.len() >= expected && expected > 0 {
                replies.push(A30CanReply {
                    response_header: response_header(*object),
                    payload,
                    truncated: false,
                });
                current = None;
            } else {
                current = Some((response_header(*object), expected, payload, false));
            }
        } else if (0x21..=0x2F).contains(pci)
            && let Some((header, expected, payload, saw_continuation)) = current.as_mut()
            && *header == response_header(*object)
            && payload.len() < *expected
        {
            let take = (*expected - payload.len()).min(7).min(window.len());
            payload.extend_from_slice(&window[..take]);
            *saw_continuation = true;
        }

        if let Some((_, expected, payload, _)) = current.as_ref()
            && payload.len() >= *expected
        {
            let (header, _, payload, _) = current.take().expect("current PDU was checked");
            replies.push(A30CanReply {
                response_header: header,
                payload,
                truncated: false,
            });
        } else if index + 1 == windows.len()
            && let Some((header, expected, payload, saw_continuation)) = current.take()
            && saw_continuation
            && payload.len() >= 3
        {
            replies.push(A30CanReply {
                response_header: header,
                truncated: expected > payload.len(),
                payload,
            });
        }
    }
    Ok(replies)
}

fn response_header(object: u8) -> String {
    format!("7{object:02X}")
}

#[cfg(test)]
mod tests {
    use super::{
        A30Adapter, A30CanReply, A30Decoder, A30Frame, A30ReadOutcome, A30ReadTest, A30Session,
        A30Timing, OemCandidateOutcome, decode_can_replies, encode_host_frame,
        run_a30_oem_candidate, run_a30_read_test_with_session,
    };
    use crate::HardwareAdapter;
    use crate::{OemCandidate, OemCandidateTrySpec};
    use std::{
        collections::VecDeque,
        io::{self, Read, Write},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    struct ScriptedDongle {
        incoming: VecDeque<u8>,
        outgoing: Vec<u8>,
        requests: Arc<Mutex<Vec<(u16, Vec<u8>)>>>,
        empty_reads: Arc<AtomicUsize>,
        response_sequence_offset: u16,
        multiple_ecu_rpm: bool,
        announce: bool,
    }

    fn captured_mode21_block() -> Vec<u8> {
        vec![
            5, 0, 0x0B, 8, 7, 0xE8, 0x10, 0x1C, 0x61, 0x01, 0, 0, 0, 0, 0, 0x0B, 8, 7, 0xE8, 0x21,
            0x25, 0x47, 0x65, 0x85, 0, 0, 0, 0, 0x0B, 8, 7, 0xE8, 0x22, 0, 0, 0, 0x30, 0x82, 0x29,
            0x52, 0, 0x0B, 8, 7, 0xE8, 0x23, 0x28, 0x20, 0x01, 0x38, 0x02, 0x33, 0x2F, 0, 0x0B, 8,
            7, 0xE8, 0x24, 0xC2, 0, 0, 0, 0, 0, 0,
        ]
    }

    fn captured_mode06_tid35_block() -> Vec<u8> {
        vec![
            2, 0, 0x0B, 8, 7, 0xE8, 0x10, 0x0A, 0x46, 0x35, 0x81, 0x35, 0, 0, 0, 0x0B, 8, 7, 0xE8,
            0x21, 0, 0, 3, 0xB5, 0, 0, 0, 0,
        ]
    }

    fn captured_vin_block() -> Vec<u8> {
        vec![
            3, 0, 0x0B, 8, 7, 0xE8, 0x10, 0x14, 0x49, 0x02, 0x01, b'J', b'T', b'L', 0, 0x0B, 8, 7,
            0xE8, 0x21, b'Z', b'E', b'4', b'F', b'E', b'1', b'E', 0, 0x0B, 8, 7, 0xE8, 0x22, b'J',
            b'0', b'4', b'9', b'3', b'1', b'4',
        ]
    }

    fn record_block_for_pdu(object: u8, payload: &[u8]) -> (u16, Vec<u8>) {
        let mut windows = Vec::new();
        let first_len = payload.len().min(6);
        let mut first = vec![payload.len() as u8];
        first.extend_from_slice(&payload[..first_len]);
        first.resize(8, 0);
        windows.push((0x10, first));
        for (index, chunk) in payload[first_len..].chunks(7).enumerate() {
            let mut window = chunk.to_vec();
            window.resize(8, 0);
            windows.push((0x21 + index as u8, window));
        }
        let count = windows.len();
        let command = match count {
            2 => 0xFFE4,
            3 => 0xFFD7,
            4 => 0xFFCA,
            5 => 0xFFBD,
            _ => unreachable!(),
        };
        let mut data = vec![count as u8, 0];
        for (pci, window) in windows {
            data.extend_from_slice(&[0x0B, 8, 7, object, pci]);
            data.extend_from_slice(&window);
        }
        (command, data)
    }

    fn notify_data(payload: &[u8]) -> Vec<u8> {
        let mut data = vec![1, 0, 0x0B, 8, 7, 0xE8, payload.len() as u8];
        data.extend_from_slice(payload);
        data.resize(14, 0);
        data
    }

    fn notify_two_ecus(first: &[u8], second: &[u8]) -> Vec<u8> {
        let mut data = vec![2];
        for (object, payload) in [(0xE8, first), (0xE9, second)] {
            data.extend_from_slice(&[0, 0x0B, 8, 7, object, payload.len() as u8]);
            data.extend_from_slice(payload);
            data.resize(data.len() + 7 - payload.len(), 0);
        }
        data
    }

    fn scion_mode21_candidate() -> OemCandidateTrySpec {
        OemCandidateTrySpec {
            source_policy: crate::OemCandidateSourcePolicy::IndexedPack,
            value_format: "uint".into(),
            candidate: OemCandidate {
                candidate_id: "scion-xb-engine-mode21-01-test".into(),
                source_id: "scion-xb-test-source".into(),
                family: "Toyota".into(),
                brand: "Scion".into(),
                model_scope: "xB 2014".into(),
                service: "21".into(),
                identifier: "01".into(),
                request_header: "7E0".into(),
                response_header: "7E8".into(),
                frequency_hz: None,
                module_type: "Engine".into(),
                category: "Engine".into(),
                name: "Scion mode 21 sample".into(),
                signal_source_id: "test".into(),
                bit_offset: Some(0),
                bit_length: Some(8),
                formula: "x".into(),
                decoder_json: r#"{"bix":0,"len":8,"unit":"raw"}"#.into(),
                unit: "raw".into(),
                year_filter_json: "{}".into(),
                ecu_filter: "7E8".into(),
                source_blob_sha: "sha256:test".into(),
                research_state: "community_reference".into(),
                vehicle_validation: "not_tested".into(),
                auto_request: false,
                confidence: "research".into(),
                notes: String::new(),
                suggested_metric: String::new(),
                source_scope: "test fixture".into(),
                applicability_specificity: 1,
                min_value: None,
                max_value: None,
                source_url: Some("https://example.test/capture".into()),
                license: Some("CC BY-SA 4.0".into()),
                license_url: Some("https://example.test/license".into()),
            },
        }
    }

    fn source_mode22_candidate() -> OemCandidateTrySpec {
        let mut spec = scion_mode21_candidate();
        spec.candidate.candidate_id = "toyota-mode22-1234-test".into();
        spec.candidate.service = "22".into();
        spec.candidate.identifier = "1234".into();
        spec.candidate.name = "Toyota DID sample".into();
        spec
    }

    impl ScriptedDongle {
        fn new() -> (Self, Arc<Mutex<Vec<(u16, Vec<u8>)>>>) {
            let requests = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    incoming: VecDeque::new(),
                    outgoing: Vec::new(),
                    requests: requests.clone(),
                    empty_reads: Arc::new(AtomicUsize::new(0)),
                    response_sequence_offset: 0,
                    multiple_ecu_rpm: false,
                    announce: true,
                },
                requests,
            )
        }

        fn with_multiple_ecu_rpm() -> (Self, Arc<Mutex<Vec<(u16, Vec<u8>)>>>) {
            let (mut dongle, requests) = Self::new();
            dongle.multiple_ecu_rpm = true;
            (dongle, requests)
        }

        fn respond(&mut self, request: A30Frame) {
            let (command, data): (u16, Vec<u8>) =
                match (request.command, request.data.last().copied()) {
                    (0xFFFA, Some(0xD0)) if self.announce => (0xFFF8, vec![1, 0, 4, 6, 16, 5, 16]),
                    (0xFFFA, Some(0xC1)) if self.announce => {
                        (0xFFEC, vec![1, 0, 16, 0xF5, 0xAB, 0xC9])
                    }
                    (0xFFFA, Some(0xC0)) if self.announce => (0xFFF8, vec![1, 0, 4, 6, 1, 7, 128]),
                    (0xFFEC, _) if request.data.get(11) == Some(&0x02) => {
                        (0xFFF1, notify_data(&[0x42, 0x0C, 0, 0x1A, 0x1E]))
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x03) => {
                        (0xFFF1, notify_data(&[0x43, 0]))
                    }
                    (0xFFEC, _)
                        if request.data.get(11) == Some(&0x06)
                            && request.data.get(12) == Some(&0x35) =>
                    {
                        (0xFFE4, captured_mode06_tid35_block())
                    }
                    (0xFFEC, _)
                        if request.data.get(11) == Some(&0x06)
                            && request.data.get(12) == Some(&0xA2) =>
                    {
                        record_block_for_pdu(
                            0xE8,
                            &[
                                0x46, 0xA2, 0x0B, 0x24, 0, 0, 0, 0, 0xFF, 0xFF, 0xA2, 0x0C, 0x24,
                                0, 1, 0, 0, 0xFF, 0xFF,
                            ],
                        )
                    }
                    (0xFFEC, _)
                        if request.data.get(11) == Some(&0x06)
                            && request.data.get(12) == Some(&0xA5) =>
                    {
                        record_block_for_pdu(
                            0xE8,
                            &[
                                0x46, 0xA5, 0x0B, 0x24, 0, 0, 0, 0, 0xFF, 0xFF, 0xA5, 0x0C, 0x24,
                                0, 0, 0, 0, 0xFF, 0xFF,
                            ],
                        )
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x06) => {
                        let page = request.data.get(12).copied().unwrap_or_default();
                        let map = match page {
                            0x00 => [0xC0, 0, 0, 1],
                            0x20 => [0x80, 0, 0x08, 0x09],
                            0x40 => [0x40, 0, 0, 1],
                            0x60 => [0, 0, 0, 1],
                            0xA0 => [0xF8, 0, 0, 0],
                            _ => [0, 0, 0, 0],
                        };
                        (
                            0xFFF1,
                            notify_data(&[0x46, page, map[0], map[1], map[2], map[3]]),
                        )
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x07) => {
                        (0xFFF1, notify_data(&[0x47, 0]))
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x09) => {
                        (0xFFD7, captured_vin_block())
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x0A) => {
                        (0xFFF1, notify_data(&[0x4A, 0]))
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x04) => {
                        (0xFFF1, notify_data(&[0x44]))
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x21) => {
                        (0xFFBD, captured_mode21_block())
                    }
                    (0xFFEC, _) if request.data.get(11) == Some(&0x22) => {
                        (0xFFF1, notify_data(&[0x62, 0x12, 0x34, 0x64]))
                    }
                    (0xFFEC, _) if self.multiple_ecu_rpm && request.data.get(12) == Some(&0x0C) => {
                        (
                            0xFFF1,
                            notify_two_ecus(&[0x41, 0x0C, 0x1A, 0xF8], &[0x41, 0x0C, 0x13, 0x88]),
                        )
                    }
                    (0xFFEC, _) => {
                        let pid = request.data.get(12).copied().unwrap_or_default();
                        let payload = match pid {
                            0x00 => vec![0x41, 0x00, 0x98, 0x3A, 0x80, 0x13],
                            0x20 => vec![0x41, 0x20, 0x80, 0x00, 0x00, 0x01],
                            0x40 => vec![0x41, 0x40, 0x00, 0x00, 0x00, 0x00],
                            0x0C => vec![0x41, 0x0C, 0x1A, 0xF8],
                            _ => vec![0x7F, 0x01, 0x12],
                        };
                        let mut notify = vec![1, 0, 0x0B, 8, 7, 0xE8, payload.len() as u8];
                        notify.extend_from_slice(&payload);
                        notify.resize(14, 0);
                        (0xFFF1, notify)
                    }
                    _ => (0xFFFA, vec![0, 0, 2, 0, 0]),
                };
            let mut bytes = vec![0xAA, 0x55];
            bytes.extend_from_slice(
                &request
                    .sequence
                    .wrapping_add(self.response_sequence_offset)
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(&(data.len() as u16).to_be_bytes());
            bytes.extend_from_slice(&command.to_be_bytes());
            bytes.extend_from_slice(&data);
            bytes.push(bytes.iter().fold(0, |checksum, byte| checksum ^ byte));
            self.incoming.extend(bytes);
        }
    }

    impl Read for ScriptedDongle {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.incoming.is_empty() {
                self.empty_reads.fetch_add(1, Ordering::Relaxed);
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no scripted reply"));
            }
            let count = buffer.len().min(self.incoming.len()).min(3);
            for byte in &mut buffer[..count] {
                *byte = self.incoming.pop_front().unwrap();
            }
            Ok(count)
        }
    }

    impl Write for ScriptedDongle {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.outgoing.extend_from_slice(buffer);
            loop {
                if self.outgoing.len() < 6 {
                    break;
                }
                let payload_len = u16::from_be_bytes([self.outgoing[4], self.outgoing[5]]) as usize;
                let frame_len = 9 + payload_len;
                if self.outgoing.len() < frame_len {
                    break;
                }
                let frame = self.outgoing.drain(..frame_len).collect::<Vec<_>>();
                let request = A30Frame {
                    sequence: u16::from_be_bytes([frame[2], frame[3]]),
                    command: u16::from_be_bytes([frame[6], frame[7]]),
                    data: frame[8..frame_len - 1].to_vec(),
                };
                self.requests
                    .lock()
                    .unwrap()
                    .push((request.command, request.data.clone()));
                self.respond(request);
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn missing_announcement_fails_the_announcement_stage() {
        let (mut dongle, _) = ScriptedDongle::new();
        dongle.announce = false;
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        let events = std::sync::Arc::new(std::sync::Mutex::new(
            Vec::<crate::DiagnosticProgress>::new(),
        ));
        let captured = events.clone();
        adapter.set_diagnostic_sink(std::sync::Arc::new(move |event| {
            captured.lock().unwrap().push(event)
        }));
        let error = adapter.connect().unwrap_err();
        let events = events.lock().unwrap();
        assert_eq!(events.last().unwrap().stage, "announcement");
        assert_eq!(events.last().unwrap().phase, crate::DiagnosticPhase::Failed);
        assert_eq!(
            events.last().unwrap().detail.as_deref(),
            Some(error.to_string().as_str())
        );
        assert!(events.last().unwrap().duration_ms.is_some());
    }

    #[test]
    fn a30_trace_contains_exact_frame_exchange() {
        let store = crate::communication_trace::CommunicationTraceStore::with_limits(2048, 4096);
        let (dongle, _) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        adapter.set_communication_sink(store.sink());
        adapter.connect().unwrap();
        let page = store.get_trace(0, 500).unwrap();
        let tx = page
            .events
            .iter()
            .find(|event| event.direction == crate::communication_trace::CommunicationDirection::Tx)
            .unwrap();
        let expected = encode_host_frame(&A30Frame {
            sequence: 1,
            command: 0xFFF8,
            data: vec![1, 0, 4, 0x67, 2, 1, 0],
        })
        .unwrap();
        let expected_hex = crate::communication_trace::bytes_to_hex(&expected);
        assert_eq!(tx.bytes_hex.as_deref(), Some(expected_hex.as_str()));
        assert!(page.events.iter().any(|event| event.direction
            == crate::communication_trace::CommunicationDirection::Rx
            && event.bytes_hex.is_some()));
    }

    #[test]
    fn one_a30_pid_failure_keeps_other_samples_and_signal_diagnostics() {
        let (dongle, _) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        let caps = adapter.connect().unwrap();
        let signals: Vec<_> = caps
            .supported_signals
            .into_iter()
            .filter(|signal| [0x0C, 0x05].contains(&signal.pid))
            .collect();
        let events = std::sync::Arc::new(std::sync::Mutex::new(
            Vec::<crate::DiagnosticProgress>::new(),
        ));
        let captured = events.clone();
        adapter.set_diagnostic_sink(std::sync::Arc::new(move |event| {
            captured.lock().unwrap().push(event)
        }));
        let samples = adapter.read_samples(&signals).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].signal_id, "engine_rpm");
        assert_eq!(samples[0].value, 1726.0);
        let events = events.lock().unwrap();
        let event = events.last().unwrap();
        assert_eq!(event.signal_ids, ["coolant_temp"]);
        assert_eq!(event.phase, crate::DiagnosticPhase::Failed);
        assert!(event.detail.as_deref().unwrap().contains("NRC 0x12"));
        assert!(event.duration_ms.is_some());
        assert!(adapter.started.is_some());
    }

    #[test]
    fn legacy_a30_identify_wrapper_preserves_invalid_address_error() {
        assert_eq!(
            super::identify_a30("invalid").unwrap_err().to_string(),
            super::identify_a30_with_diagnostics(
                "invalid",
                crate::diagnostics::noop_diagnostic_sink()
            )
            .unwrap_err()
            .to_string()
        );
    }

    #[test]
    fn identify_stages_do_not_discover_pids() {
        let (dongle, written) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        let events = std::sync::Arc::new(std::sync::Mutex::new(
            Vec::<crate::DiagnosticProgress>::new(),
        ));
        let captured = events.clone();
        adapter.set_diagnostic_sink(std::sync::Arc::new(move |event| {
            captured.lock().unwrap().push(event)
        }));
        let device = adapter.identify_inner().unwrap();
        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.phase == crate::DiagnosticPhase::Succeeded)
                .map(|event| event.stage.as_str())
                .collect::<Vec<_>>(),
            [
                "bluetooth_session_open",
                "relay_identity_handshake",
                "announcement"
            ]
        );
        assert!(events.iter().all(|event| event.operation == "identify"
            && event.adapter_id.as_deref() == Some(device.stable_id.as_str())));
        assert!(
            written
                .lock()
                .unwrap()
                .iter()
                .all(|frame| frame.0 != 0xFFEC)
        );
    }

    #[test]
    fn host_setup_frame_matches_the_captured_bytes_and_xor() {
        let frame = A30Frame {
            sequence: 1,
            command: 0xFFF8,
            data: vec![0x01, 0x00, 0x04, 0x67, 0x02, 0x01, 0x00],
        };
        assert_eq!(
            encode_host_frame(&frame).unwrap(),
            [
                0x55, 0xAA, 0x00, 0x01, 0x00, 0x07, 0xFF, 0xF8, 0x01, 0x00, 0x04, 0x67, 0x02, 0x01,
                0x00, 0x9F,
            ]
        );
    }

    #[test]
    fn decoder_handles_split_magic_and_coalesced_ack_frames() {
        let first = [
            0xAA, 0x55, 0x00, 0x01, 0x00, 0x05, 0xFF, 0xFA, 0x00, 0x00, 0x02, 0x00, 0x00, 0xFC,
        ];
        let second = [
            0xAA, 0x55, 0x00, 0x02, 0x00, 0x05, 0xFF, 0xFA, 0x00, 0x00, 0x02, 0x00, 0x00, 0xFF,
        ];
        let mut decoder = A30Decoder::default();

        assert!(decoder.feed(&first[..1]).unwrap().is_empty());
        assert!(decoder.feed(&first[1..4]).unwrap().is_empty());
        let mut both = first[4..].to_vec();
        both.extend_from_slice(&second);
        let frames = decoder.feed(&both).unwrap();

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].sequence, 1);
        assert_eq!(frames[0].command, 0xFFFA);
        assert_eq!(frames[0].data, [0, 0, 2, 0, 0]);
        assert_eq!(frames[1].sequence, 2);
    }

    #[test]
    fn decoder_rejects_checksum_corruption_and_oversized_payloads() {
        let mut corrupted = [
            0xAA, 0x55, 0x00, 0x01, 0x00, 0x05, 0xFF, 0xFA, 0x00, 0x00, 0x02, 0x00, 0x00, 0xFC,
        ];
        corrupted[13] ^= 1;
        assert!(A30Decoder::default().feed(&corrupted).is_err());

        let oversized = [0xAA, 0x55, 0x00, 0x01, 0x08, 0x01, 0xFF, 0xFA];
        assert!(A30Decoder::default().feed(&oversized).is_err());
    }

    #[test]
    fn open_runs_the_captured_status_poll_and_configuration_sequence() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };

        let session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();

        assert!(session.is_ready());
        assert!(session.is_announced());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 19);
        assert_eq!(requests[0].0, 0xFFF8);
        assert_eq!(requests[0].1, [1, 0, 4, 0x67, 2, 1, 0]);
        assert_eq!(
            &requests[1..4].iter().map(|item| item.0).collect::<Vec<_>>(),
            &[0xFFF9; 3]
        );
        assert_eq!(
            &requests[4..12]
                .iter()
                .map(|item| item.0)
                .collect::<Vec<_>>(),
            &[0xFFFA; 8]
        );
        assert_eq!(
            &requests[12..].iter().map(|item| item.0).collect::<Vec<_>>(),
            &[0xFFF8, 0xFFF9, 0xFFF8, 0xFFF7, 0xFFF4, 0xFFF2, 0xFFF5]
        );
    }

    #[test]
    fn notify_record_trims_padding_and_keeps_the_responder_id() {
        let frame = A30Frame {
            sequence: 42,
            command: 0xFFF1,
            data: vec![1, 0, 0x0B, 8, 7, 0xE8, 4, 0x41, 0x0C, 0x0A, 0x82, 0, 0, 0],
        };

        assert_eq!(
            decode_can_replies(&frame).unwrap(),
            [A30CanReply {
                response_header: "7E8".into(),
                payload: vec![0x41, 0x0C, 0x0A, 0x82],
                truncated: false,
            }]
        );
    }

    #[test]
    fn long_record_blocks_keep_single_response_payloads_together() {
        let frame = A30Frame {
            sequence: 9,
            command: 0xFF4F,
            data: vec![
                2, 0, 0, 7, 0, 0xE8, 0x10, 2, 0x62, 0x12, 0, 0, 7, 0, 0xE8, 0x10, 2, 0x34, 0x55,
            ],
        };

        let replies = decode_can_replies(&frame).unwrap();

        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].response_header, "7E8");
        assert_eq!(replies[0].payload, [0x62, 0x12, 0x34, 0x55]);
        assert!(!replies[0].truncated);
    }

    #[test]
    fn record_window_block_reassembles_the_captured_mode21_response() {
        let frame = A30Frame {
            sequence: 1,
            command: 0xFFBD,
            data: captured_mode21_block(),
        };

        let replies = decode_can_replies(&frame).unwrap();

        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].response_header, "7E8");
        assert_eq!(replies[0].payload.len(), 28);
        assert_eq!(&replies[0].payload[..2], &[0x61, 0x01]);
        assert_eq!(replies[0].payload[27], 0xC2);
        assert!(!replies[0].truncated);
    }

    #[test]
    fn mode01_pid_read_returns_the_capture_decoded_response() {
        let (dongle, _) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();

        let response = session.read_mode01_pid(0x0C).unwrap();

        assert_eq!(response.ecu_id.as_deref(), Some("7E8"));
        assert_eq!(response.raw_bytes, [0x41, 0x0C, 0x1A, 0xF8]);
        assert_eq!(
            crate::obdlink_sx_protocol::decode_signal(
                &crate::standard_obd_signals()
                    .into_iter()
                    .find(|signal| signal.pid == 0x0C)
                    .unwrap(),
                &response,
            )
            .unwrap(),
            1726.0
        );
    }

    #[test]
    fn adapter_discovers_mode01_pages_and_emits_live_samples() {
        let (dongle, _) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        let events = std::sync::Arc::new(std::sync::Mutex::new(
            Vec::<crate::DiagnosticProgress>::new(),
        ));
        let captured = events.clone();
        adapter.set_diagnostic_sink(std::sync::Arc::new(move |event| {
            captured.lock().unwrap().push(event)
        }));
        let capabilities = adapter.connect().unwrap();

        assert_eq!(
            capabilities.device.hardware_family,
            "Xtool A30/A30M Bluetooth SPP"
        );
        assert!(
            capabilities
                .supported_signals
                .iter()
                .any(|signal| signal.pid == 0x0C)
        );
        let rpm = capabilities
            .supported_signals
            .iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap()
            .clone();
        let samples = adapter.read_samples(&[rpm]).unwrap();
        let events = events.lock().unwrap();
        let stages: Vec<_> = events
            .iter()
            .filter(|event| event.phase == crate::DiagnosticPhase::Succeeded)
            .map(|event| event.stage.as_str())
            .collect();
        assert_eq!(
            stages,
            [
                "bluetooth_session_open",
                "relay_identity_handshake",
                "announcement",
                "supported_pid_discovery",
                "pid_read"
            ]
        );
        assert!(
            events
                .iter()
                .filter(|event| event.phase != crate::DiagnosticPhase::Started)
                .all(|event| event.duration_ms.is_some())
        );
        assert_eq!(events.last().unwrap().signal_ids, ["engine_rpm"]);
        assert!(events.iter().all(|event| event.detail.is_none()));
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].value, 1726.0);
        assert_eq!(samples[0].ecu_id.as_deref(), Some("7E8"));
        assert!(samples[0].wall_time_unix_ns > 0);
    }

    #[test]
    fn a30_rpm_samples_flow_through_the_shared_engine_pipeline() {
        let (dongle, _) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        let (events, received) = std::sync::mpsc::channel();
        let mut engine = crate::LogNookEngine::new(move |event| {
            let _ = events.send(event);
        });
        engine
            .connect_adapter(Box::new(adapter), &["engine_rpm".into()])
            .unwrap();
        assert!(matches!(
            received.recv_timeout(Duration::from_secs(1)).unwrap(),
            crate::EngineEvent::Connected(_)
        ));
        let event = received.recv_timeout(Duration::from_secs(1)).unwrap();
        let crate::EngineEvent::Sample(sample) = event else {
            panic!("expected A30 sample through shared pipeline, received {event:?}");
        };
        assert_eq!(sample.signal_id, "engine_rpm");
        assert_eq!(sample.value, 1726.0);
        engine.disconnect().unwrap();
    }

    #[test]
    fn source_backed_mode21_candidate_runs_over_a30_and_decodes_numeric_data() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();

        let result =
            run_a30_oem_candidate(session, "AA:BB:CC:DD:EE:FF", &scion_mode21_candidate()).unwrap();

        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(result.responder_header.as_deref(), Some("7E8"));
        assert_eq!(result.response_payload.len(), 28);
        assert_eq!(result.decoded_value.unwrap().value, Some(0.0));
        assert!(result.exact_adapter_command.contains("55 AA"));
        let requests = requests.lock().unwrap();
        let (command, record) = requests.last().unwrap();
        assert_eq!(*command, 0xFFEC);
        assert_eq!(&record[9..13], &[0xE0, 0x02, 0x21, 0x01]);
        assert_eq!(record[18], 0xFF);
    }

    #[test]
    fn a30_returns_a_matched_physical_oem_reply_without_waiting_for_timeout() {
        let (dongle, _) = ScriptedDongle::new();
        let empty_reads = dongle.empty_reads.clone();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::from_millis(400),
        };
        let session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();
        let before = empty_reads.load(Ordering::Relaxed);

        let result =
            run_a30_oem_candidate(session, "AA:BB:CC:DD:EE:FF", &source_mode22_candidate())
                .unwrap();

        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(empty_reads.load(Ordering::Relaxed), before);
    }

    #[test]
    fn connected_mode01_reads_return_on_the_identified_ecu_reply() {
        let (dongle, _) = ScriptedDongle::new();
        let empty_reads = dongle.empty_reads.clone();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::from_millis(400),
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        adapter.connect().unwrap();
        let before = empty_reads.load(Ordering::Relaxed);
        let rpm = crate::standard_obd_signals()
            .into_iter()
            .find(|signal| signal.id == "engine_rpm")
            .unwrap();

        let samples = adapter.read_samples(&[rpm]).unwrap();

        assert_eq!(samples[0].value, 1726.0);
        assert_eq!(empty_reads.load(Ordering::Relaxed), before);
    }

    #[test]
    fn a30_does_not_decode_a_reply_with_the_wrong_outer_sequence() {
        let (mut dongle, _) = ScriptedDongle::new();
        dongle.response_sequence_offset = 1;
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::from_millis(10),
        };
        let session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();

        let result =
            run_a30_oem_candidate(session, "AA:BB:CC:DD:EE:FF", &source_mode22_candidate())
                .unwrap();

        assert_eq!(result.outcome, OemCandidateOutcome::NoResponse);
        assert!(!result.raw_response.is_empty());
    }

    #[test]
    fn a30_candidate_and_captured_read_reuse_open_session() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        adapter.connect().unwrap();
        requests.lock().unwrap().clear();
        let crate::ResearchTestResult::OemCandidate(candidate) = adapter
            .execute_research_test(crate::ResearchTestRequest::OemCandidate(
                scion_mode21_candidate(),
            ))
            .unwrap()
        else {
            panic!("expected candidate result")
        };
        let crate::ResearchTestResult::A30ReadTest(read) = adapter
            .execute_research_test(crate::ResearchTestRequest::A30ReadTest(
                A30ReadTest::Mode02FreezeFrameRpm,
            ))
            .unwrap()
        else {
            panic!("expected captured read result")
        };
        let crate::ResearchTestResult::OemCandidate(did) = adapter
            .execute_research_test(crate::ResearchTestRequest::OemCandidate(
                source_mode22_candidate(),
            ))
            .unwrap()
        else {
            panic!("expected candidate result")
        };
        assert_eq!(candidate.outcome, OemCandidateOutcome::Positive);
        assert_eq!(read.outcome, A30ReadOutcome::Positive);
        assert_eq!(
            read.graph_values[0].response_payload,
            Some(vec![0x42, 0x0C, 0x00, 0x1A, 0x1E])
        );
        assert_eq!(did.response_payload, [0x62, 0x12, 0x34, 0x64]);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests[0],
            (
                0xFFEC,
                vec![
                    1, 0, 0x10, 0x61, 4, 0, 0x0B, 8, 7, 0xE0, 2, 0x21, 1, 0, 0, 0, 0, 0, 0xFF
                ]
            )
        );
        assert_eq!(
            requests[1],
            (
                0xFFEC,
                vec![
                    1, 0, 0x10, 0x61, 4, 0, 0x0B, 8, 7, 0xDF, 3, 2, 0x0C, 0, 0, 0, 0, 0, 0xFF
                ]
            )
        );
        assert_eq!(
            requests[2],
            (
                0xFFEC,
                vec![
                    1, 0, 0x10, 0x61, 4, 0, 0x0B, 8, 7, 0xE0, 3, 0x22, 0x12, 0x34, 0, 0, 0, 0, 0xFF
                ]
            )
        );
        assert!(adapter.started.is_some());
        drop(requests);
        let mut invalid = scion_mode21_candidate();
        invalid.candidate.request_header = "123".into();
        assert!(
            adapter
                .execute_research_test(crate::ResearchTestRequest::OemCandidate(invalid))
                .is_err()
        );
        let mut unsupported_poll_route = scion_mode21_candidate();
        unsupported_poll_route.candidate.request_header = "6F1".into();
        assert!(
            adapter
                .validate_oem_candidate_poll(&unsupported_poll_route)
                .unwrap_err()
                .to_string()
                .contains("0x7xx")
        );
    }

    #[test]
    fn connected_captured_reads_cover_mode03_mode07_and_mode0a() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        adapter.connect().unwrap();
        requests.lock().unwrap().clear();
        for test in [
            A30ReadTest::Mode03StoredDtcs,
            A30ReadTest::Mode07PendingDtcs,
            A30ReadTest::Mode0aPermanentDtcs,
        ] {
            let crate::ResearchTestResult::A30ReadTest(result) = adapter
                .execute_research_test(crate::ResearchTestRequest::A30ReadTest(test))
                .unwrap()
            else {
                panic!("expected captured read result")
            };
            assert_eq!(result.outcome, A30ReadOutcome::Positive);
            assert!(result.graph_values.is_empty());
        }
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for (request, service) in requests.iter().zip([0x03, 0x07, 0x0A]) {
            assert_eq!(
                request,
                &(
                    0xFFEC,
                    vec![
                        1, 0, 0x10, 0x61, 4, 0, 0x0B, 8, 7, 0xDF, 1, service, 0, 0, 0, 0, 0, 0,
                        0xFF
                    ]
                )
            );
        }
    }

    #[test]
    fn a30_explicit_mode04_uses_one_captured_record_and_keeps_research_read_only() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        let capabilities = adapter.connect().unwrap();
        assert!(capabilities.diagnostics.clear_dtcs);
        requests.lock().unwrap().clear();

        let reads = adapter.clear_vehicle_dtcs().unwrap();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].service, 0x04);
        assert_eq!(reads[0].request_payload, [0x04]);
        assert_eq!(reads[0].outcome, crate::DiagnosticOutcome::Positive);
        assert_eq!(reads[0].response_payload, [0x44]);
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            [(
                0xFFEC,
                vec![
                    1, 0, 0x10, 0x61, 0x04, 0, 0x0B, 8, 7, 0x04, 0xDF, 0x04, 0, 0, 0, 0, 0, 0, 0xFF
                ]
            )]
        );

        requests.lock().unwrap().clear();
        assert!(
            adapter
                .execute_research_test(crate::ResearchTestRequest::Diagnostic(
                    crate::DiagnosticQueryRequest::clear_dtcs()
                ))
                .is_err()
        );
        assert!(requests.lock().unwrap().is_empty());
    }

    #[test]
    fn connected_a30_rejects_invalid_candidates_before_transmitting() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let mut adapter =
            A30Adapter::with_stream_for_test("AA:BB:CC:DD:EE:FF", Box::new(dongle), timing);
        adapter.connect().unwrap();
        let transmitted = requests.lock().unwrap().len();

        let mut invalid_width21 = scion_mode21_candidate();
        invalid_width21.candidate.identifier = "0101".into();
        let mut invalid_width22 = source_mode22_candidate();
        invalid_width22.candidate.identifier = "12".into();
        let mut missing_revision = scion_mode21_candidate();
        missing_revision.candidate.source_blob_sha.clear();
        let mut missing_source = scion_mode21_candidate();
        missing_source.candidate.source_url = None;
        let mut missing_license = scion_mode21_candidate();
        missing_license.candidate.license = None;
        let mut missing_license_url = scion_mode21_candidate();
        missing_license_url.candidate.license_url = None;
        let mut invalid_response_route = scion_mode21_candidate();
        invalid_response_route.candidate.response_header = "800".into();

        for (case, spec) in [
            ("Mode 21 identifier width", invalid_width21),
            ("Mode 22 identifier width", invalid_width22),
            ("source revision", missing_revision),
            ("source URL", missing_source),
            ("license", missing_license),
            ("license URL", missing_license_url),
            ("response route", invalid_response_route),
        ] {
            let result =
                adapter.execute_research_test(crate::ResearchTestRequest::OemCandidate(spec));
            assert_eq!(
                requests.lock().unwrap().len(),
                transmitted,
                "{case} transmitted a frame"
            );
            assert!(result.is_err(), "{case} was accepted");
        }
    }

    #[test]
    fn source_backed_mode22_did_can_be_tried_over_a30() {
        let (dongle, requests) = ScriptedDongle::new();
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();

        let result =
            run_a30_oem_candidate(session, "AA:BB:CC:DD:EE:FF", &source_mode22_candidate())
                .unwrap();

        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(result.response_payload, [0x62, 0x12, 0x34, 0x64]);
        assert_eq!(result.decoded_value.unwrap().value, Some(100.0));
        let requests = requests.lock().unwrap();
        let (command, record) = requests.last().unwrap();
        assert_eq!(*command, 0xFFEC);
        assert_eq!(&record[9..14], &[0xE0, 0x03, 0x22, 0x12, 0x34]);
    }

    #[test]
    fn read_mode_test_covers_freeze_frame_mode06_raw_counts_and_vin() {
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };

        let (freeze_dongle, _) = ScriptedDongle::new();
        let freeze_session = A30Session::open_with_timing(Box::new(freeze_dongle), timing).unwrap();
        let freeze = run_a30_read_test_with_session(
            freeze_session,
            "AA:BB:CC:DD:EE:FF",
            A30ReadTest::Mode02FreezeFrameRpm,
        )
        .unwrap();
        assert_eq!(freeze.outcome, A30ReadOutcome::Positive);
        assert_eq!(freeze.graph_values[0].value, 1671.5);
        assert_eq!(freeze.graph_values[0].unit, "rpm");
        assert_eq!(
            freeze.graph_values[0].response_payload,
            Some(vec![0x42, 0x0C, 0x00, 0x1A, 0x1E])
        );

        let (monitor_dongle, _) = ScriptedDongle::new();
        let monitor_session =
            A30Session::open_with_timing(Box::new(monitor_dongle), timing).unwrap();
        let monitor = run_a30_read_test_with_session(
            monitor_session,
            "AA:BB:CC:DD:EE:FF",
            A30ReadTest::Mode06Tid35,
        )
        .unwrap();
        assert_eq!(monitor.outcome, A30ReadOutcome::Positive);
        assert_eq!(monitor.graph_values[0].value, 0.0);
        assert_eq!(monitor.graph_values[0].unit, "raw count");
        assert_eq!(
            monitor.graph_values[0].response_payload,
            Some(vec![0x46, 0x35, 0x81, 0x35, 0, 0, 0, 0, 3, 0xB5])
        );

        let (vin_dongle, _) = ScriptedDongle::new();
        let vin_session = A30Session::open_with_timing(Box::new(vin_dongle), timing).unwrap();
        let vin = run_a30_read_test_with_session(
            vin_session,
            "AA:BB:CC:DD:EE:FF",
            A30ReadTest::Mode09Vin,
        )
        .unwrap();
        assert_eq!(vin.outcome, A30ReadOutcome::Positive);
        assert_eq!(vin.detail.as_deref(), Some("JTLZE4FE1EJ049314"));
    }

    #[test]
    fn mode01_and_multiple_mode06_results_are_graphable() {
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };

        let (rpm_dongle, _) = ScriptedDongle::new();
        let rpm_session = A30Session::open_with_timing(Box::new(rpm_dongle), timing).unwrap();
        let rpm = run_a30_read_test_with_session(
            rpm_session,
            "AA:BB:CC:DD:EE:FF",
            A30ReadTest::Mode01EngineRpm,
        )
        .unwrap();
        assert_eq!(rpm.outcome, A30ReadOutcome::Positive);
        assert_eq!(rpm.graph_values[0].signal_id, "engine_rpm");
        assert_eq!(rpm.graph_values[0].value, 1726.0);

        let (monitor_dongle, _) = ScriptedDongle::new();
        let monitor_session =
            A30Session::open_with_timing(Box::new(monitor_dongle), timing).unwrap();
        let monitor = run_a30_read_test_with_session(
            monitor_session,
            "AA:BB:CC:DD:EE:FF",
            A30ReadTest::Mode06TidA2,
        )
        .unwrap();
        assert_eq!(monitor.outcome, A30ReadOutcome::Positive);
        assert_eq!(monitor.graph_values.len(), 2);
        assert_eq!(monitor.graph_values[0].value, 0.0);
        assert_eq!(monitor.graph_values[1].value, 1.0);
        assert!(
            monitor
                .graph_values
                .iter()
                .all(|value| value.unit == "raw count")
        );
    }

    #[test]
    fn mode01_multiple_ecu_replies_are_preserved_but_not_graphed() {
        let timing = A30Timing {
            handshake_wait: Duration::ZERO,
            retry_wait: Duration::ZERO,
            status_wait: Duration::ZERO,
            poll_wait: Duration::ZERO,
            config_wait: Duration::ZERO,
            read_wait: Duration::ZERO,
        };
        let (dongle, _) = ScriptedDongle::with_multiple_ecu_rpm();
        let session = A30Session::open_with_timing(Box::new(dongle), timing).unwrap();

        let result = run_a30_read_test_with_session(
            session,
            "AA:BB:CC:DD:EE:FF",
            A30ReadTest::Mode01EngineRpm,
        )
        .unwrap();

        assert_eq!(result.outcome, A30ReadOutcome::WrongResponder);
        assert_eq!(result.responder_header.as_deref(), Some("7E8"));
        assert_eq!(
            result.response_payloads,
            vec![vec![0x41, 0x0C, 0x1A, 0xF8], vec![0x41, 0x0C, 0x13, 0x88],]
        );
        assert!(result.graph_values.is_empty());
        assert!(
            result
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("Multiple ECU responders"))
        );
    }
}
