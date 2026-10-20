//! SAE J1979 OBD diagnostics and Mode 01 acquisition over a Candlelight GS_USB channel.

mod oem;

use crate::{
    CanDeviceSelection, CanDirection, CanFrame, CanTransport, CanTransportEvent,
    ConnectionCapabilities, DeviceDescriptor, DiagnosticSink, HardwareAdapter, LogNookError,
    Sample, SampleOrigin, SampleQuality, SignalDefinition,
    adapter::collect_samples_independently,
    communication_trace::CommunicationSink,
    diagnostics::{diagnostic_stage, noop_diagnostic_sink},
    obdlink_sx_protocol::{decode_signal, parse_mode01_payload, parse_supported_pid_payload},
    standard_obd_signals,
};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const CAN_OBD_BITRATE: u32 = 500_000;
const FUNCTIONAL_REQUEST_ID: u32 = 0x7DF;
const FIRST_RESPONSE_ID: u32 = 0x7E8;
const LAST_RESPONSE_ID: u32 = 0x7EF;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_OBD_RESPONSE_FRAMES: usize = 32;
const MAX_DIAGNOSTIC_RESPONSE_BYTES: usize = 1024;
const MAX_DIAGNOSTIC_RESPONSE_FRAMES: usize = 150;
const EMPTY_CYCLE_BACKOFF: Duration = Duration::from_millis(250);

pub struct CanObdAdapter {
    selection: CanDeviceSelection,
    transport: Option<Box<dyn CanTransport>>,
    source_id: Option<String>,
    response_id: Option<u32>,
    supported_pids: Vec<u8>,
    started: Option<Instant>,
    diagnostic_sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
    cancelled: Arc<AtomicBool>,
}

impl CanObdAdapter {
    pub fn new(selection: CanDeviceSelection) -> Self {
        Self::with_cancel_token(selection, Arc::new(AtomicBool::new(false)))
    }

    pub fn with_cancel_token(selection: CanDeviceSelection, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            selection,
            transport: None,
            source_id: None,
            response_id: None,
            supported_pids: Vec::new(),
            started: None,
            diagnostic_sink: noop_diagnostic_sink(),
            communication_sink: None,
            cancelled,
        }
    }

    #[cfg(test)]
    fn with_transport(transport: impl CanTransport) -> Self {
        let source_id = transport.source_id();
        Self {
            selection: CanDeviceSelection {
                id: "test-device".into(),
                interface_number: 0,
                channel: 0,
            },
            transport: Some(Box::new(transport)),
            source_id: Some(source_id),
            response_id: None,
            supported_pids: Vec::new(),
            started: None,
            diagnostic_sink: noop_diagnostic_sink(),
            communication_sink: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn adapter_id(&self) -> String {
        format!(
            "candlelight-obd:{}:{}:{}",
            self.selection.id, self.selection.interface_number, self.selection.channel
        )
    }

    fn stage<T>(
        sink: &DiagnosticSink,
        adapter_id: String,
        operation: &str,
        name: &str,
        signals: Vec<String>,
        action: impl FnOnce() -> Result<T, LogNookError>,
    ) -> Result<T, LogNookError> {
        diagnostic_stage(sink, Some(adapter_id), operation, name, signals, action)
    }

    fn ensure_not_cancelled(&self) -> Result<(), LogNookError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(LogNookError::new("CAN OBD connection was cancelled"))
        } else {
            Ok(())
        }
    }

    fn open_transport(&mut self) -> Result<(), LogNookError> {
        if self.transport.is_none() {
            self.ensure_not_cancelled()?;
            let mut transport = crate::open_candlelight(&self.selection)?;
            if let Some(sink) = &self.communication_sink {
                transport.set_communication_sink(sink.clone());
            }
            self.source_id = Some(transport.source_id());
            self.transport = Some(transport);
        }
        Ok(())
    }

    fn start_transport(&mut self, session_origin: Instant) -> Result<(), LogNookError> {
        self.ensure_not_cancelled()?;
        self.transport
            .as_mut()
            .ok_or_else(|| LogNookError::new("Candlelight transport is not open"))?
            .start(CAN_OBD_BITRATE, false, session_origin)
            .map_err(|error| LogNookError::new(error.to_string()))
    }

    fn receive_obd_frames(
        &mut self,
        timeout: Duration,
        pid: u8,
        expected_response_id: Option<u32>,
    ) -> Result<Vec<CanFrame>, LogNookError> {
        let mut frames = Vec::new();
        let mut pending_frames = Vec::new();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.ensure_not_cancelled()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            let event = self
                .transport
                .as_mut()
                .ok_or_else(|| LogNookError::new("Candlelight transport is not open"))?
                .receive(remaining)
                .map_err(|error| LogNookError::new(error.to_string()))?;
            let Some(CanTransportEvent::Rx(frame)) = event else {
                if Instant::now() < deadline {
                    continue;
                }
                break;
            };
            frame
                .validate()
                .map_err(|error| LogNookError::new(error.to_string()))?;
            if frame.direction == CanDirection::Rx
                && !frame.extended
                && (FIRST_RESPONSE_ID..=LAST_RESPONSE_ID).contains(&frame.can_id)
                && frame.flags == 0
                && mode01_reply_for_pid(&frame, pid).map_err(LogNookError::signal)?
            {
                if expected_response_id.is_some_and(|expected| expected != frame.can_id) {
                    return Err(LogNookError::signal(
                        "A second ECU responded during a physically addressed Mode 01 read",
                    ));
                }
                if frames.len() + pending_frames.len() == MAX_OBD_RESPONSE_FRAMES {
                    return Err(LogNookError::new(
                        "Too many CAN OBD response frames arrived for one request",
                    ));
                }
                if is_response_pending(&frame) {
                    pending_frames.push(frame);
                } else {
                    frames.push(frame);
                }
                if expected_response_id.is_some() {
                    if !frames.is_empty() {
                        break;
                    }
                }
                if frames
                    .iter()
                    .chain(pending_frames.iter())
                    .map(|frame| frame.can_id)
                    .collect::<HashSet<_>>()
                    .len()
                    > 1
                {
                    break;
                }
            }
        }
        if expected_response_id.is_none() {
            frames.extend(pending_frames);
        } else if frames.is_empty() {
            frames = pending_frames;
        }
        Ok(frames)
    }

    fn request_mode01(
        &mut self,
        request_id: u32,
        pid: u8,
        expected_response_id: Option<u32>,
    ) -> Result<Vec<CanFrame>, LogNookError> {
        self.ensure_not_cancelled()?;
        let frame = CanFrame {
            elapsed_ns: 0,
            wall_time_unix_ns: 0,
            can_id: request_id,
            extended: false,
            direction: CanDirection::Tx,
            data: vec![2, 0x01, pid, 0, 0, 0, 0, 0],
            flags: 0,
        };
        self.transport
            .as_mut()
            .ok_or_else(|| LogNookError::new("Candlelight transport is not open"))?
            .transmit(&frame)
            .map_err(|error| LogNookError::new(error.to_string()))?;
        self.receive_obd_frames(RESPONSE_TIMEOUT, pid, expected_response_id)
    }

    fn decode_support_page(
        frame: &CanFrame,
        requested_pid: u8,
    ) -> Result<(Vec<u8>, bool), LogNookError> {
        let payload = single_frame_payload(frame).map_err(LogNookError::new)?;
        let response =
            parse_supported_pid_payload(format!("{:03X}", frame.can_id), &payload, requested_pid)
                .map_err(LogNookError::new)?;
        Ok((response.pids, response.has_more))
    }

    fn discover_supported_pids(&mut self) -> Result<(), LogNookError> {
        let first_page = self.request_mode01(FUNCTIONAL_REQUEST_ID, 0x00, None)?;
        let responders = first_page
            .iter()
            .map(|frame| frame.can_id)
            .collect::<HashSet<_>>();
        let response_id = match responders.len() {
            0 => {
                return Err(LogNookError::new(
                    "No ECU responded to generic Mode 01 discovery",
                ));
            }
            1 => *responders.iter().next().expect("one responder was checked"),
            _ => {
                return Err(LogNookError::new(
                    "Multiple ECUs responded to generic Mode 01 discovery; support was not merged",
                ));
            }
        };
        let first = first_page
            .iter()
            .rev()
            .find(|frame| frame.can_id == response_id && !is_response_pending(frame))
            .or_else(|| first_page.iter().find(|frame| frame.can_id == response_id))
            .expect("responder set came from response frames");
        let (mut supported_pids, mut has_more) = Self::decode_support_page(first, 0x00)?;
        self.response_id = Some(response_id);
        let mut range_start = 0x20;
        while has_more && range_start <= 0xE0 {
            let request_id = response_id.checked_sub(8).ok_or_else(|| {
                LogNookError::new("ECU response ID has no standard OBD request route")
            })?;
            let frames = self.request_mode01(request_id, range_start, Some(response_id))?;
            let frame = unique_physical_response(frames, response_id)?;
            let (page, continuation) = Self::decode_support_page(&frame, range_start)?;
            supported_pids.extend(page);
            has_more = continuation;
            if range_start == 0xE0 {
                break;
            }
            range_start += 0x20;
        }
        supported_pids.sort_unstable();
        supported_pids.dedup();
        self.supported_pids = supported_pids;
        if !standard_obd_signals()
            .iter()
            .any(|signal| self.supported_pids.contains(&signal.pid))
        {
            return Err(LogNookError::new(
                "ECU reports no supported LogNook generic Mode 01 signals",
            ));
        }
        Ok(())
    }

    fn read_vehicle_diagnostic(
        &mut self,
        query: crate::DiagnosticQueryRequest,
    ) -> Result<crate::DiagnosticReport, LogNookError> {
        if self.started.is_none() {
            return Err(LogNookError::new(
                "Candlelight OBD adapter is not connected",
            ));
        }
        query.validate().map_err(LogNookError::new)?;
        let response_id = self
            .response_id
            .ok_or_else(|| LogNookError::new("CAN OBD ECU identity is unavailable"))?;
        let request_id = response_id.checked_sub(8).ok_or_else(|| {
            LogNookError::new("ECU response ID has no standard OBD request route")
        })?;
        let started_at_unix_ms = crate::vehicle_diagnostics::now_unix_ms();
        let request_started = Instant::now();
        let payload = query.request_payload();
        if payload.is_empty() || payload.len() > 7 {
            return Err(LogNookError::new(
                "Invalid standard OBD diagnostic request length",
            ));
        }
        let mut frame_data = vec![0u8; 8];
        frame_data[0] = payload.len() as u8;
        frame_data[1..=payload.len()].copy_from_slice(payload);
        let command = format!("CAN 11-bit TX {request_id:03X} {}", hex_bytes(payload));
        let request_frame = CanFrame {
            elapsed_ns: 0,
            wall_time_unix_ns: 0,
            can_id: request_id,
            extended: false,
            direction: CanDirection::Tx,
            data: frame_data,
            flags: 0,
        };
        let result = self
            .transport
            .as_mut()
            .ok_or_else(|| LogNookError::new("Candlelight transport is not open"))?
            .transmit(&request_frame)
            .map_err(|error| LogNookError::new(error.to_string()))
            .and_then(|_| self.receive_diagnostic_payloads(&query, request_id, response_id));
        let latency_ms = request_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let route = format!("CAN 11-bit physical {request_id:03X} → {response_id:03X}");
        let reads = match result {
            Ok(responses) if !responses.is_empty() => responses
                .into_iter()
                .map(|(frames, response, truncated)| {
                    let mut read = query.report(
                        Some(format!("{response_id:03X}")),
                        response,
                        frames.iter().map(frame_hex).collect::<Vec<_>>().join("\n"),
                        command.clone(),
                        started_at_unix_ms,
                        latency_ms,
                        truncated,
                    );
                    read.route = route.clone();
                    read
                })
                .collect(),
            Ok(_) => {
                let mut read = query.no_response(
                    String::new(),
                    command,
                    started_at_unix_ms,
                    latency_ms,
                    Some("No ECU response before the standard OBD timeout".into()),
                );
                read.ecu_id = Some(format!("{response_id:03X}"));
                read.route = route;
                vec![read]
            }
            Err(error) if error.is_transient() => {
                let mut read = query.no_response(
                    String::new(),
                    command,
                    started_at_unix_ms,
                    latency_ms,
                    Some(error.to_string()),
                );
                read.ecu_id = Some(format!("{response_id:03X}"));
                read.route = route;
                vec![read]
            }
            Err(error) => {
                let mut read = query.transport_error(
                    String::new(),
                    command,
                    started_at_unix_ms,
                    latency_ms,
                    error.to_string(),
                );
                read.ecu_id = Some(format!("{response_id:03X}"));
                read.route = route;
                vec![read]
            }
        };
        Ok(crate::DiagnosticReport {
            report_id: None,
            connection_generation: None,
            vehicle_context: None,
            adapter: DeviceDescriptor {
                stable_id: format!(
                    "{}:channel:{}:obd",
                    self.source_id
                        .as_deref()
                        .ok_or_else(|| LogNookError::new("Candlelight identity is unavailable"))?,
                    self.transport
                        .as_ref()
                        .and_then(|transport| transport.target_channel())
                        .ok_or_else(|| LogNookError::new("Candlelight channel is unavailable"))?
                ),
                hardware_family: "Candlelight GS-USB OBD-II".into(),
                firmware: None,
                path: Some(self.selection.id.clone()),
            },
            protocol: "SAE J1979 over 11-bit CAN at 500 kbit/s".into(),
            started_at_unix_ms,
            operation_kind: if query.kind == crate::DiagnosticReadKind::ClearDtcs {
                crate::DiagnosticReportKind::ClearDtcs
            } else {
                crate::DiagnosticReportKind::Snapshot
            },
            request: crate::DiagnosticRequest::default(),
            reads,
            route_restore_error: None,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }

    fn receive_diagnostic_payloads(
        &mut self,
        query: &crate::DiagnosticQueryRequest,
        request_id: u32,
        response_id: u32,
    ) -> Result<Vec<(Vec<CanFrame>, Vec<u8>, bool)>, LogNookError> {
        let identifier = query.identifier.into_iter().collect::<Vec<_>>();
        self.receive_routed_payloads(
            query.service,
            &identifier,
            query.wait_ms,
            request_id,
            response_id,
        )
    }

    fn receive_routed_payloads(
        &mut self,
        service: u8,
        identifier: &[u8],
        wait_ms: u64,
        request_id: u32,
        response_id: u32,
    ) -> Result<Vec<(Vec<CanFrame>, Vec<u8>, bool)>, LogNookError> {
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        let mut responses = Vec::new();
        let mut frames = Vec::new();
        let mut expected_length = None;
        let mut next_sequence = 1u8;
        while Instant::now() < deadline {
            self.ensure_not_cancelled()?;
            let timeout = deadline.saturating_duration_since(Instant::now());
            let event = self
                .transport
                .as_mut()
                .ok_or_else(|| LogNookError::new("Candlelight transport is not open"))?
                .receive(timeout)
                .map_err(|error| LogNookError::new(error.to_string()))?;
            let Some(CanTransportEvent::Rx(frame)) = event else {
                continue;
            };
            if frame.direction != CanDirection::Rx
                || frame.extended
                || frame.can_id != response_id
                || frame.flags != 0
            {
                continue;
            }
            frame
                .validate()
                .map_err(|error| LogNookError::new(error.to_string()))?;
            let Some(&pci) = frame.data.first() else {
                continue;
            };
            match pci >> 4 {
                0 => {
                    let length = usize::from(pci & 0x0F);
                    if length == 0 || length > frame.data.len().saturating_sub(1) {
                        continue;
                    }
                    let payload = frame.data[1..=length].to_vec();
                    if !diagnostic_payload_matches(service, identifier, &payload) {
                        continue;
                    }
                    let pending = payload.as_slice() == [0x7F, service, 0x78];
                    responses.push((vec![frame], payload, false));
                    if !pending {
                        break;
                    }
                }
                1 => {
                    if frame.data.len() < 3
                        || !diagnostic_payload_matches(service, identifier, &frame.data[2..])
                    {
                        continue;
                    }
                    let length =
                        (usize::from(frame.data[0] & 0x0F) << 8) | usize::from(frame.data[1]);
                    if length <= 6 || length > MAX_DIAGNOSTIC_RESPONSE_BYTES {
                        responses.push((vec![frame.clone()], frame.data[2..].to_vec(), true));
                        break;
                    }
                    expected_length = Some(length);
                    frames.clear();
                    frames.push(frame.clone());
                    next_sequence = 1;
                    let flow_control = CanFrame {
                        elapsed_ns: 0,
                        wall_time_unix_ns: 0,
                        can_id: request_id,
                        extended: false,
                        direction: CanDirection::Tx,
                        data: vec![0x30, 0, 0, 0, 0, 0, 0, 0],
                        flags: 0,
                    };
                    self.transport
                        .as_mut()
                        .ok_or_else(|| LogNookError::new("Candlelight transport is not open"))?
                        .transmit(&flow_control)
                        .map_err(|error| LogNookError::new(error.to_string()))?;
                }
                2 if expected_length.is_some() => {
                    if pci & 0x0F != next_sequence {
                        frames.push(frame);
                        let partial = can_isotp_payload_prefix(&frames).unwrap_or_default();
                        responses.push((std::mem::take(&mut frames), partial, true));
                        expected_length = None;
                        break;
                    }
                    frames.push(frame);
                    if frames.len() > MAX_DIAGNOSTIC_RESPONSE_FRAMES {
                        let partial = can_isotp_payload_prefix(&frames).unwrap_or_default();
                        responses.push((std::mem::take(&mut frames), partial, true));
                        expected_length = None;
                        break;
                    }
                    next_sequence = (next_sequence + 1) & 0x0F;
                    let length = expected_length.expect("checked above");
                    if let Some(payload) = can_isotp_payload(&frames, length) {
                        if diagnostic_payload_matches(service, identifier, &payload) {
                            let pending = payload.as_slice() == [0x7F, service, 0x78];
                            responses.push((std::mem::take(&mut frames), payload, false));
                            expected_length = None;
                            if !pending {
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if expected_length.is_some() && !frames.is_empty() {
            let payload = can_isotp_payload_prefix(&frames).unwrap_or_default();
            responses.push((std::mem::take(&mut frames), payload, true));
        }
        Ok(responses)
    }

    fn connect_inner(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        let sink = self.diagnostic_sink.clone();
        let adapter_id = self.adapter_id();
        Self::stage(
            &sink,
            adapter_id,
            "connect",
            "transport_open",
            vec![],
            || self.open_transport(),
        )?;
        let source_id = self
            .source_id
            .clone()
            .ok_or_else(|| LogNookError::new("Candlelight source identity is unavailable"))?;
        let target_channel = self
            .transport
            .as_ref()
            .and_then(|transport| transport.target_channel())
            .ok_or_else(|| LogNookError::new("Candlelight channel identity is unavailable"))?;
        let device = DeviceDescriptor {
            stable_id: format!("{source_id}:channel:{target_channel}:obd"),
            hardware_family: "Candlelight GS-USB OBD-II".into(),
            firmware: None,
            path: Some(format!(
                "{} · interface {} · channel {}",
                self.selection.id, self.selection.interface_number, target_channel
            )),
        };
        let sink = self.diagnostic_sink.clone();
        let adapter_id = self.adapter_id();
        Self::stage(
            &sink,
            adapter_id,
            "connect",
            "protocol_setup",
            vec![],
            || self.start_transport(Instant::now()),
        )?;
        let sink = self.diagnostic_sink.clone();
        let adapter_id = self.adapter_id();
        Self::stage(
            &sink,
            adapter_id,
            "connect",
            "supported_pid_discovery",
            vec![],
            || self.discover_supported_pids(),
        )?;
        self.started = Some(Instant::now());
        let supported_signals = standard_obd_signals()
            .into_iter()
            .filter(|signal| self.supported_pids.contains(&signal.pid))
            .collect();
        Ok(ConnectionCapabilities {
            device,
            protocol: "SAE J1979 Mode 01 over 11-bit CAN at 500 kbit/s".into(),
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

    fn read_pid(&mut self, pid: u8) -> Result<CanFrame, LogNookError> {
        let response_id = self
            .response_id
            .ok_or_else(|| LogNookError::new("CAN OBD ECU identity is unavailable"))?;
        let request_id = response_id.checked_sub(8).ok_or_else(|| {
            LogNookError::new("ECU response ID has no standard OBD request route")
        })?;
        let frames = self.request_mode01(request_id, pid, Some(response_id))?;
        unique_physical_response(frames, response_id)
    }

    #[cfg(test)]
    fn supported_pids(&self) -> &[u8] {
        &self.supported_pids
    }
}

fn diagnostic_payload_matches(service: u8, identifier: &[u8], payload: &[u8]) -> bool {
    if payload.first() == Some(&0x7F) {
        return payload.get(1) == Some(&service);
    }
    payload.first() == Some(&service.wrapping_add(0x40))
        && payload.get(1..1 + identifier.len()) == Some(identifier)
}

fn can_isotp_payload(frames: &[CanFrame], expected_length: usize) -> Option<Vec<u8>> {
    let payload = can_isotp_payload_prefix(frames)?;
    (payload.len() >= expected_length).then(|| payload[..expected_length].to_vec())
}

fn can_isotp_payload_prefix(frames: &[CanFrame]) -> Option<Vec<u8>> {
    let first = frames.first()?;
    if first.data.first()? >> 4 != 1 || first.data.len() < 3 {
        return None;
    }
    let mut payload = first.data[2..].to_vec();
    for frame in frames.iter().skip(1) {
        if frame.data.first().is_none_or(|byte| byte >> 4 != 2) {
            return None;
        }
        payload.extend_from_slice(&frame.data[1..]);
    }
    Some(payload)
}

fn frame_hex(frame: &CanFrame) -> String {
    format!("{:03X} {}", frame.can_id, hex_bytes(&frame.data))
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn single_frame_payload(frame: &CanFrame) -> Result<Vec<u8>, String> {
    if frame.extended || frame.direction != CanDirection::Rx {
        return Err("Mode 01 response must be a standard CAN receive frame".into());
    }
    let Some(&pci) = frame.data.first() else {
        return Err("Mode 01 response has no ISO-TP payload length".into());
    };
    let length = usize::from(pci & 0x0F);
    if pci >> 4 != 0 || length == 0 || length > frame.data.len().saturating_sub(1) {
        return Err("Mode 01 response has a malformed or unsupported ISO-TP single frame".into());
    }
    Ok(frame.data[1..=length].to_vec())
}

fn mode01_reply_for_pid(frame: &CanFrame, pid: u8) -> Result<bool, String> {
    let Some(pci) = frame.data.first() else {
        return Ok(false);
    };
    if pci >> 4 == 1 {
        return if frame.data.get(2) == Some(&0x41) && frame.data.get(3) == Some(&pid) {
            Err("Multi-frame Mode 01 response is not supported by this logger".into())
        } else {
            Ok(false)
        };
    }
    if pci >> 4 != 0 {
        return Ok(false);
    }
    if (pci & 0x0F) as usize > frame.data.len().saturating_sub(1) {
        return if frame.data.get(1) == Some(&0x41) && frame.data.get(2) == Some(&pid) {
            Err("Mode 01 response has a truncated ISO-TP single frame".into())
        } else {
            Ok(false)
        };
    }
    let payload = single_frame_payload(frame)?;
    Ok(
        (payload.first() == Some(&0x41) && payload.get(1) == Some(&pid))
            || (payload.first() == Some(&0x7F) && payload.get(1) == Some(&0x01)),
    )
}

fn is_response_pending(frame: &CanFrame) -> bool {
    single_frame_payload(frame).is_ok_and(|payload| payload.as_slice() == [0x7F, 0x01, 0x78])
}

fn unique_physical_response(
    frames: Vec<CanFrame>,
    response_id: u32,
) -> Result<CanFrame, LogNookError> {
    if frames.iter().any(|frame| frame.can_id != response_id) {
        return Err(LogNookError::signal(
            "A second ECU responded during a physically addressed Mode 01 read",
        ));
    }
    match frames.len() {
        0 => Err(LogNookError::signal(
            "No ECU response before the Mode 01 timeout",
        )),
        1 => Ok(frames.into_iter().next().expect("one response was checked")),
        _ => Err(LogNookError::signal(
            "ECU returned multiple responses to one Mode 01 request",
        )),
    }
}

pub fn identify_can_obd(selection: &CanDeviceSelection) -> Result<DeviceDescriptor, LogNookError> {
    identify_can_obd_with_sinks(selection, noop_diagnostic_sink(), None)
}

pub fn identify_can_obd_with_sinks(
    selection: &CanDeviceSelection,
    sink: DiagnosticSink,
    communication_sink: Option<CommunicationSink>,
) -> Result<DeviceDescriptor, LogNookError> {
    let adapter_id = format!(
        "candlelight-obd:{}:{}:{}",
        selection.id, selection.interface_number, selection.channel
    );
    let transport = diagnostic_stage(
        &sink,
        Some(adapter_id.clone()),
        "identify",
        "usb_claim_and_identity",
        vec![],
        || {
            let mut transport = crate::open_candlelight(selection)?;
            if let Some(sink) = communication_sink {
                transport.set_communication_sink(sink);
            }
            Ok(transport)
        },
    )?;
    let source_id = transport.source_id();
    let target_channel = transport
        .target_channel()
        .ok_or_else(|| LogNookError::new("Candlelight channel identity is unavailable"))?;
    let descriptor = DeviceDescriptor {
        stable_id: format!("{source_id}:channel:{target_channel}:obd"),
        hardware_family: "Candlelight GS-USB OBD-II".into(),
        firmware: None,
        path: Some(format!(
            "{} · interface {} · channel {}",
            selection.id, selection.interface_number, target_channel
        )),
    };
    drop(transport);
    Ok(descriptor)
}

impl HardwareAdapter for CanObdAdapter {
    fn set_diagnostic_sink(&mut self, sink: DiagnosticSink) {
        self.diagnostic_sink = sink;
    }

    fn set_communication_sink(&mut self, sink: CommunicationSink) {
        self.communication_sink = Some(sink.clone());
        if let Some(transport) = self.transport.as_mut() {
            transport.set_communication_sink(sink);
        }
    }

    fn validate_oem_candidate_poll(
        &self,
        spec: &crate::OemCandidateTrySpec,
    ) -> Result<(), LogNookError> {
        crate::build_oem_candidate_command(spec)?;
        if spec
            .candidate
            .request_header
            .eq_ignore_ascii_case(&spec.candidate.response_header)
        {
            return Err(LogNookError::new(
                "OEM request and response CAN IDs must differ",
            ));
        }
        Ok(())
    }

    fn execute_research_test(
        &mut self,
        request: crate::ResearchTestRequest,
    ) -> Result<crate::ResearchTestResult, LogNookError> {
        if let crate::ResearchTestRequest::OemCandidate(ref spec) = request {
            return self
                .read_oem_candidate(spec)
                .map(crate::ResearchTestResult::OemCandidate);
        }
        let crate::ResearchTestRequest::Diagnostic(query) = request else {
            return Err(LogNookError::new(
                "Candlelight OBD supports standard read-only vehicle diagnostics only",
            ));
        };
        if query.kind == crate::DiagnosticReadKind::ClearDtcs {
            return Err(LogNookError::new(
                "Mode 04 is available only through the explicit DTC clear operation",
            ));
        }
        self.read_vehicle_diagnostic(query)
            .map(crate::ResearchTestResult::Diagnostic)
    }

    fn clear_vehicle_dtcs(&mut self) -> Result<Vec<crate::DiagnosticRead>, LogNookError> {
        self.read_vehicle_diagnostic(crate::DiagnosticQueryRequest::clear_dtcs())
            .map(|report| report.reads)
    }

    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.started.is_some() {
            return Err(LogNookError::new("CAN OBD adapter is already connected"));
        }
        let result = self.connect_inner();
        if result.is_err() {
            self.disconnect();
        }
        result
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        if self.started.is_none() {
            return Err(LogNookError::new("CAN OBD adapter is not connected"));
        }
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
                    "Unsupported read-only generic Mode 01 signal: {}",
                    signal.id
                )));
            }
        }

        let mut requested_pids = HashSet::new();
        let pids = signals
            .iter()
            .map(|signal| signal.pid)
            .filter(|pid| requested_pids.insert(*pid))
            .collect::<Vec<_>>();
        let signal_ids_by_pid = |pid: u8| {
            signals
                .iter()
                .filter(|signal| signal.pid == pid)
                .map(|signal| signal.id.clone())
                .collect::<Vec<_>>()
        };
        let sink = self.diagnostic_sink.clone();
        let source_id = self
            .source_id
            .as_ref()
            .and_then(|source_id| {
                self.transport
                    .as_ref()
                    .and_then(|transport| transport.target_channel())
                    .map(|channel| format!("{source_id}:channel:{channel}:obd"))
            })
            .ok_or_else(|| LogNookError::new("Candlelight source identity is unavailable"))?;
        let samples = collect_samples_independently(&pids, |pid| {
            diagnostic_stage(
                &sink,
                Some(source_id.clone()),
                "acquisition",
                "pid_read",
                signal_ids_by_pid(*pid),
                || {
                    let frame = self.read_pid(*pid)?;
                    let payload = single_frame_payload(&frame).map_err(LogNookError::signal)?;
                    let ecu_id = format!("{:03X}", frame.can_id);
                    let response = parse_mode01_payload(ecu_id.clone(), &payload, *pid)
                        .map_err(LogNookError::signal)?;
                    let mut samples = Vec::new();
                    for signal in signals.iter().filter(|signal| signal.pid == *pid) {
                        let canonical = catalog
                            .iter()
                            .find(|known| known.id == signal.id)
                            .expect("signal was validated against generic Mode 01 catalog");
                        let value =
                            decode_signal(canonical, &response).map_err(LogNookError::signal)?;
                        samples.push(Sample {
                            elapsed_ns: frame.elapsed_ns,
                            wall_time_unix_ns: frame.wall_time_unix_ns,
                            source_id: source_id.clone(),
                            ecu_id: Some(ecu_id.clone()),
                            signal_id: canonical.id.clone(),
                            value,
                            unit: canonical.unit.clone(),
                            quality: SampleQuality::Valid,
                            origin: SampleOrigin::Live,
                            raw_bytes: Some(frame.data.clone()),
                        });
                    }
                    Ok(samples)
                },
            )
        })?;
        if !signals.is_empty() && samples.is_empty() {
            std::thread::sleep(EMPTY_CYCLE_BACKOFF);
        }
        Ok(samples)
    }

    fn sample_interval(&self) -> Duration {
        Duration::ZERO
    }

    fn disconnect(&mut self) {
        if let Some(transport) = self.transport.as_mut() {
            let _ = transport.stop();
        }
        self.transport = None;
        self.source_id = None;
        self.response_id = None;
        self.supported_pids.clear();
        self.started = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{CanObdAdapter, hex_bytes};
    use crate::{
        CanDirection, CanFrame, CanTransport, CanTransportError, CanTransportEvent,
        HardwareAdapter, SignalDefinition, standard_obd_signals,
    };
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    #[derive(Default)]
    struct FakeState {
        responses: VecDeque<VecDeque<Option<CanTransportEvent>>>,
        current_response: VecDeque<Option<CanTransportEvent>>,
        transmitted: Vec<CanFrame>,
        stopped: usize,
    }

    struct FakeTransport(Arc<Mutex<FakeState>>);

    impl CanTransport for FakeTransport {
        fn source_id(&self) -> String {
            "can:fake-gs-usb:1234".into()
        }

        fn target_channel(&self) -> Option<u8> {
            Some(0)
        }

        fn start(
            &mut self,
            bitrate: u32,
            listen_only: bool,
            _session_origin: Instant,
        ) -> Result<(), crate::CanTransportError> {
            assert_eq!(bitrate, 500_000);
            assert!(!listen_only);
            Ok(())
        }

        fn receive(
            &mut self,
            timeout: Duration,
        ) -> Result<Option<CanTransportEvent>, CanTransportError> {
            let event = self.0.lock().unwrap().current_response.pop_front();
            if event.is_none() {
                std::thread::sleep(timeout);
            }
            Ok(event.flatten())
        }

        fn transmit(&mut self, frame: &CanFrame) -> Result<u32, CanTransportError> {
            let mut state = self.0.lock().unwrap();
            state.transmitted.push(frame.clone());
            state.current_response = state.responses.pop_front().unwrap_or_default();
            Ok(1)
        }

        fn stop(&mut self) -> Result<(), CanTransportError> {
            self.0.lock().unwrap().stopped += 1;
            Ok(())
        }
    }

    fn fake(
        responses: impl IntoIterator<Item = Vec<Option<CanTransportEvent>>>,
    ) -> (FakeTransport, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState {
            responses: responses.into_iter().map(VecDeque::from).collect(),
            current_response: VecDeque::new(),
            transmitted: Vec::new(),
            stopped: 0,
        }));
        (FakeTransport(state.clone()), state)
    }

    fn rx(can_id: u32, data: &[u8]) -> Option<CanTransportEvent> {
        Some(CanTransportEvent::Rx(CanFrame {
            elapsed_ns: 5_000_000,
            wall_time_unix_ns: 1_000_000_000,
            can_id,
            extended: false,
            direction: CanDirection::Rx,
            data: data.to_vec(),
            flags: 0,
        }))
    }

    fn tx_echo(can_id: u32, data: &[u8]) -> Option<CanTransportEvent> {
        Some(CanTransportEvent::TxEcho {
            echo_id: 1,
            frame: CanFrame {
                elapsed_ns: 5_000_000,
                wall_time_unix_ns: 1_000_000_000,
                can_id,
                extended: false,
                direction: CanDirection::Tx,
                data: data.to_vec(),
                flags: 0,
            },
            raw_usb: vec![1],
        })
    }

    fn adapter_with_page00(page: &[u8]) -> (CanObdAdapter, Arc<Mutex<FakeState>>) {
        let (transport, state) = fake([vec![rx(0x7E8, page), None]]);
        (CanObdAdapter::with_transport(transport), state)
    }

    fn request_ids(state: &Arc<Mutex<FakeState>>) -> Vec<(u32, Vec<u8>)> {
        state
            .lock()
            .unwrap()
            .transmitted
            .iter()
            .map(|frame| (frame.can_id, frame.data.clone()))
            .collect()
    }

    fn diagnostic_query(kind: crate::DiagnosticReadKind) -> crate::DiagnosticQueryRequest {
        crate::DiagnosticRequest::default()
            .queries()
            .unwrap()
            .into_iter()
            .find(|query| query.kind == kind)
            .expect("requested query is part of the standard diagnostic snapshot")
    }

    #[test]
    fn continuous_oem_validation_checks_source_and_route_without_can_io() {
        let (adapter, state) = adapter_with_page00(&[]);
        let mut spec = crate::oem_test_fixture::candidate_spec();
        assert!(adapter.validate_oem_candidate_poll(&spec).is_ok());
        spec.candidate.request_header = spec.candidate.response_header.clone();
        assert!(adapter.validate_oem_candidate_poll(&spec).is_err());
        spec = crate::oem_test_fixture::candidate_spec();
        spec.candidate.source_blob_sha.clear();
        assert!(adapter.validate_oem_candidate_poll(&spec).is_err());
        assert!(state.lock().unwrap().transmitted.is_empty());
    }

    #[test]
    fn source_oem_reads_use_exact_routes_and_do_not_change_the_mode01_ecu() {
        for (service, identifier, payload) in [
            ("21", "01", vec![0x61, 0x01, 0]),
            ("22", "1234", vec![0x62, 0x12, 0x34, 0]),
        ] {
            let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
            adapter.connect().unwrap();
            let mut spec = crate::oem_test_fixture::candidate_spec();
            spec.candidate.service = service.into();
            spec.candidate.identifier = identifier.into();
            state
                .lock()
                .unwrap()
                .responses
                .push_back(VecDeque::from([positive_single_frame(0x708, &payload)]));
            let crate::ResearchTestResult::OemCandidate(result) = adapter
                .execute_research_test(crate::ResearchTestRequest::OemCandidate(spec.clone()))
                .unwrap()
            else {
                panic!("OEM result expected")
            };
            assert_eq!(result.outcome, crate::OemCandidateOutcome::Positive);
            assert_eq!(result.decoded_value.unwrap().value, Some(0.0));
            assert_eq!(result.responder_header.as_deref(), Some("708"));
            assert!(result.raw_response.starts_with("708 "));
            assert_eq!(adapter.response_id, Some(0x7E8));
            let requests = request_ids(&state);
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1].0, 0x700);
            assert_eq!(requests[1].1[1], u8::from_str_radix(service, 16).unwrap());
            assert!(!spec.candidate.auto_request);
        }
    }

    #[test]
    fn invalid_oem_candidate_is_rejected_before_can_transmission() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        let mut spec = crate::oem_test_fixture::candidate_spec();
        for service in ["04", "2E", "27"] {
            spec.candidate.service = service.into();
            assert!(adapter.read_oem_candidate(&spec).is_err());
        }
        spec.candidate.service = "21".into();
        spec.candidate.source_blob_sha.clear();
        assert!(adapter.read_oem_candidate(&spec).is_err());
        assert_eq!(request_ids(&state).len(), 1);
    }

    #[test]
    fn oem_negative_reply_is_preserved_without_retry() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        state
            .lock()
            .unwrap()
            .responses
            .push_back(VecDeque::from([positive_single_frame(
                0x708,
                &[0x7F, 0x21, 0x22],
            )]));
        let result = adapter
            .read_oem_candidate(&crate::oem_test_fixture::candidate_spec())
            .unwrap();
        assert_eq!(
            result.outcome,
            crate::OemCandidateOutcome::TemporarilyUnavailable
        );
        assert_eq!(result.negative_response_code, Some(0x22));
        assert_eq!(result.response_payload, [0x7F, 0x21, 0x22]);
        assert_eq!(request_ids(&state).len(), 2);
    }

    #[test]
    fn source_mode22_multiframe_reply_uses_the_source_flow_control_route() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        let mut spec = crate::oem_test_fixture::candidate_spec();
        spec.candidate.service = "22".into();
        spec.candidate.identifier = "1234".into();
        {
            let mut state = state.lock().unwrap();
            state.responses.push_back(VecDeque::from([rx(
                0x708,
                &[0x10, 0x0A, 0x62, 0x12, 0x34, 0, 1, 2],
            )]));
            state
                .responses
                .push_back(VecDeque::from([rx(0x708, &[0x21, 3, 4, 5, 6, 0, 0, 0])]));
        }
        let result = adapter.read_oem_candidate(&spec).unwrap();
        assert_eq!(result.outcome, crate::OemCandidateOutcome::Positive);
        assert_eq!(
            result.response_payload,
            [0x62, 0x12, 0x34, 0, 1, 2, 3, 4, 5, 6]
        );
        let requests = request_ids(&state);
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1], (0x700, vec![3, 0x22, 0x12, 0x34, 0, 0, 0, 0]));
        assert_eq!(requests[2], (0x700, vec![0x30, 0, 0, 0, 0, 0, 0, 0]));
        assert!(result.raw_response.contains("10 0A 62 12 34"));
        assert!(result.raw_response.contains("21 03 04 05 06"));
    }

    fn positive_single_frame(can_id: u32, payload: &[u8]) -> Option<CanTransportEvent> {
        assert!(!payload.is_empty() && payload.len() <= 7);
        let mut frame = vec![0; 8];
        frame[0] = payload.len() as u8;
        frame[1..=payload.len()].copy_from_slice(payload);
        rx(can_id, &frame)
    }

    #[test]
    fn discovery_follows_advertised_continuation_and_targets_the_unique_ecu() {
        let (transport, state) = fake([
            vec![rx(0x7E8, &[0x06, 0x41, 0x00, 0, 0x10, 0, 1]), None],
            vec![rx(0x7E8, &[0x06, 0x41, 0x20, 0, 0, 0, 0]), None],
        ]);
        let mut adapter = CanObdAdapter::with_transport(transport);

        let capabilities = adapter.connect().unwrap();

        assert_eq!(
            request_ids(&state),
            [
                (0x7DF, vec![2, 1, 0, 0, 0, 0, 0, 0]),
                (0x7E0, vec![2, 1, 0x20, 0, 0, 0, 0, 0]),
            ]
        );
        assert!(
            capabilities
                .supported_signals
                .iter()
                .any(|signal| signal.pid == 0x0C)
        );
    }

    #[test]
    fn discovery_stops_at_page_e0_even_if_its_continuation_bit_is_set() {
        let mut responses = vec![vec![rx(0x7E8, &[0x06, 0x41, 0x00, 0, 0x10, 0, 1]), None]];
        for pid in (0x20..=0xE0).step_by(0x20) {
            responses.push(vec![rx(0x7E8, &[0x06, 0x41, pid, 0, 0, 0, 1]), None]);
        }
        let (transport, state) = fake(responses);
        let mut adapter = CanObdAdapter::with_transport(transport);

        adapter.connect().unwrap();

        let requests = request_ids(&state);
        assert_eq!(requests.len(), 8);
        assert_eq!(requests.last().unwrap().1[2], 0xE0);
        assert!(standard_obd_signals().iter().all(|signal| {
            crate::obdlink_sx_protocol::mode01_pid_data_width(signal.pid)
                .is_some_and(|width| width <= 2)
        }));
    }

    #[test]
    fn discovery_keeps_listening_after_an_empty_poll_and_rejects_multiple_ecus() {
        let (transport, _) = fake([vec![
            rx(0x7E8, &[0x06, 0x41, 0x00, 0, 0x10, 0, 0]),
            None,
            rx(0x7E9, &[0x06, 0x41, 0x00, 0, 0, 0x10, 0]),
            None,
        ]]);
        let mut adapter = CanObdAdapter::with_transport(transport);

        let error = adapter.connect().unwrap_err().to_string();

        assert!(error.to_ascii_lowercase().contains("multiple"));
        assert!(adapter.supported_pids().is_empty());
    }

    #[test]
    fn discovery_ignores_tx_echoes_extended_frames_flags_and_unrelated_ids() {
        let (transport, _) = fake([vec![
            tx_echo(0x7E8, &[0x06, 0x41, 0x00, 0, 0x10, 0, 0]),
            Some(CanTransportEvent::Rx(CanFrame {
                elapsed_ns: 1,
                wall_time_unix_ns: 1,
                can_id: 0x7E8,
                extended: true,
                direction: CanDirection::Rx,
                data: vec![0x06, 0x41, 0x00, 0, 0x10, 0, 0],
                flags: 0,
            })),
            Some(CanTransportEvent::Rx(CanFrame {
                elapsed_ns: 1,
                wall_time_unix_ns: 1,
                can_id: 0x7E8,
                extended: false,
                direction: CanDirection::Rx,
                data: vec![0x06, 0x41, 0x00, 0, 0x10, 0, 0],
                flags: 1,
            })),
            rx(0x123, &[0x06, 0x41, 0x00, 0, 0x10, 0, 0]),
            None,
        ]]);
        let mut adapter = CanObdAdapter::with_transport(transport);

        assert!(
            adapter
                .connect()
                .unwrap_err()
                .to_string()
                .contains("No ECU")
        );
        assert!(adapter.supported_pids().is_empty());
    }

    #[test]
    fn discovery_rejects_timeout_and_malformed_support_response() {
        let (transport, _) = fake([vec![None]]);
        assert!(
            CanObdAdapter::with_transport(transport)
                .connect()
                .unwrap_err()
                .to_string()
                .contains("No ECU")
        );

        let (transport, _) = fake([vec![rx(0x7E8, &[0x05, 0x41, 0x00, 0, 0x10, 0]), None]]);
        assert!(
            CanObdAdapter::with_transport(transport)
                .connect()
                .unwrap_err()
                .to_string()
                .contains("exactly 4 data byte")
        );

        let (transport, _) = fake([vec![
            rx(0x7E8, &[0x10, 0x06, 0x41, 0x00, 0, 0x10, 0, 0]),
            None,
        ]]);
        assert!(
            CanObdAdapter::with_transport(transport)
                .connect()
                .unwrap_err()
                .to_string()
                .contains("Multi-frame")
        );
    }

    #[test]
    fn live_reads_use_standard_decoder_and_preserve_ecu_and_raw_can_bytes() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        let capabilities = adapter.connect().unwrap();
        let rpm = standard_obd_signals()
            .into_iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();
        state.lock().unwrap().responses.push_back(VecDeque::from([
            rx(0x7E8, &[0x04, 0x41, 0x0C, 0x1A, 0xF8, 0, 0, 0]),
            None,
        ]));

        let started = Instant::now();
        let samples = adapter.read_samples(std::slice::from_ref(&rpm)).unwrap();

        assert!(started.elapsed() < Duration::from_millis(250));
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].value, 1726.0);
        assert_eq!(samples[0].ecu_id.as_deref(), Some("7E8"));
        assert_eq!(samples[0].source_id, capabilities.device.stable_id);
        assert_eq!(
            samples[0].raw_bytes,
            Some(vec![4, 0x41, 0x0C, 0x1A, 0xF8, 0, 0, 0])
        );
        assert_eq!(request_ids(&state).last().unwrap().0, 0x7E0);
    }

    #[test]
    fn live_read_waits_for_response_pending_then_returns_the_final_measurement() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        let rpm = standard_obd_signals()
            .into_iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();
        state.lock().unwrap().responses.push_back(VecDeque::from([
            rx(0x7E8, &[0x03, 0x7F, 0x01, 0x78, 0, 0, 0, 0]),
            rx(0x7E8, &[0x04, 0x41, 0x0C, 0x1A, 0xF8, 0, 0, 0]),
        ]));

        let samples = adapter.read_samples(std::slice::from_ref(&rpm)).unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].value, 1726.0);
    }

    #[test]
    fn live_reads_ignore_unrelated_frames_and_return_no_sample_for_timeout_or_bad_reply() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        let rpm: SignalDefinition = standard_obd_signals()
            .into_iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();
        state.lock().unwrap().responses.push_back(VecDeque::from([
            rx(0x123, &[0x04, 0x41, 0x0C, 0x1A, 0xF8, 0, 0, 0]),
            rx(0x7E8, &[0x04, 0x41, 0x0D, 0x1A, 0xF8, 0, 0, 0]),
            None,
        ]));

        assert!(
            adapter
                .read_samples(std::slice::from_ref(&rpm))
                .unwrap()
                .is_empty()
        );
        assert!(
            adapter
                .read_samples(std::slice::from_ref(&rpm))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn diagnostics_read_modes_03_07_and_0a_from_the_discovered_ecu_with_raw_route() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        let capabilities = adapter.connect().unwrap();
        assert!(capabilities.diagnostics.dtc_snapshot);
        assert!(capabilities.diagnostics.identity);
        assert!(capabilities.diagnostics.mode06);
        assert!(capabilities.diagnostics.clear_dtcs);

        state.lock().unwrap().responses.extend([
            VecDeque::from([positive_single_frame(0x7E8, &[0x43, 0x01, 0x33])]),
            VecDeque::from([positive_single_frame(0x7E8, &[0x47, 0x00])]),
            VecDeque::from([positive_single_frame(0x7E8, &[0x4A, 0x00])]),
        ]);

        let mut reads = Vec::new();
        for kind in [
            crate::DiagnosticReadKind::StoredDtcs,
            crate::DiagnosticReadKind::PendingDtcs,
            crate::DiagnosticReadKind::PermanentDtcs,
        ] {
            let crate::ResearchTestResult::Diagnostic(report) = adapter
                .execute_research_test(crate::ResearchTestRequest::Diagnostic(diagnostic_query(
                    kind,
                )))
                .unwrap()
            else {
                panic!("diagnostic query returns a diagnostic report")
            };
            reads.push(report.reads.into_iter().next().unwrap());
        }

        assert_eq!(
            reads.iter().map(|read| read.service).collect::<Vec<_>>(),
            [0x03, 0x07, 0x0A]
        );
        for read in &reads {
            assert_eq!(read.outcome, crate::DiagnosticOutcome::Positive);
            assert_eq!(read.ecu_id.as_deref(), Some("7E8"));
            assert_eq!(read.route, "CAN 11-bit physical 7E0 → 7E8");
            assert!(read.raw_response.starts_with("7E8 "));
            assert_eq!(read.request_payload, [read.service]);
        }
        assert_eq!(
            reads[0].values,
            [crate::DiagnosticValue::DtcList {
                codes: vec!["P0133".into()]
            }]
        );
        assert_eq!(
            request_ids(&state)
                .iter()
                .map(|(_, frame)| frame[1])
                .collect::<Vec<_>>(),
            [0x01, 0x03, 0x07, 0x0A]
        );
    }

    #[test]
    fn mode09_vin_uses_iso_tp_flow_control_and_keeps_each_raw_frame() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        let vin = b"1HGCM82633A004352";
        let mut payload = vec![0x49, 0x02, 0x01];
        payload.extend_from_slice(vin);
        assert_eq!(payload.len(), 20);
        let first = vec![
            0x10, 0x14, payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
        ];
        let consecutive = [
            [
                0x21,
                payload[6],
                payload[7],
                payload[8],
                payload[9],
                payload[10],
                payload[11],
                payload[12],
            ],
            [
                0x22,
                payload[13],
                payload[14],
                payload[15],
                payload[16],
                payload[17],
                payload[18],
                payload[19],
            ],
        ];
        {
            let mut state = state.lock().unwrap();
            state
                .responses
                .push_back(VecDeque::from([rx(0x7E8, &first)]));
            state.responses.push_back(VecDeque::from(
                consecutive
                    .iter()
                    .map(|frame| rx(0x7E8, frame))
                    .collect::<Vec<_>>(),
            ));
        }

        let crate::ResearchTestResult::Diagnostic(report) = adapter
            .execute_research_test(crate::ResearchTestRequest::Diagnostic(diagnostic_query(
                crate::DiagnosticReadKind::Vin,
            )))
            .unwrap()
        else {
            panic!("VIN query returns a diagnostic report")
        };
        let read = report.reads.into_iter().next().unwrap();

        assert_eq!(read.outcome, crate::DiagnosticOutcome::Positive);
        assert_eq!(read.ecu_id.as_deref(), Some("7E8"));
        assert_eq!(read.response_payload, payload);
        assert_eq!(
            read.values,
            [crate::DiagnosticValue::Identity {
                pid: 0x02,
                label: "VIN".into(),
                value: "1HGCM82633A004352".into(),
            }]
        );
        let requests = request_ids(&state);
        assert_eq!(requests[1], (0x7E0, vec![2, 0x09, 0x02, 0, 0, 0, 0, 0]));
        assert_eq!(requests[2], (0x7E0, vec![0x30, 0, 0, 0, 0, 0, 0, 0]));
        for frame in [&first, &consecutive[0].to_vec(), &consecutive[1].to_vec()] {
            assert!(read.raw_response.contains(&hex_bytes(frame)));
        }
        assert_eq!(read.route, "CAN 11-bit physical 7E0 → 7E8");
    }

    #[test]
    fn incomplete_iso_tp_diagnostic_response_is_preserved_as_malformed() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        let first = [0x10, 0x14, 0x49, 0x02, 0x01, b'1', b'H', b'G'];
        {
            let mut state = state.lock().unwrap();
            state
                .responses
                .push_back(VecDeque::from([rx(0x7E8, &first)]));
            state.responses.push_back(VecDeque::new());
        }
        let mut query = diagnostic_query(crate::DiagnosticReadKind::Vin);
        query.wait_ms = 5;

        let read = adapter
            .read_vehicle_diagnostic(query)
            .unwrap()
            .reads
            .remove(0);

        assert_eq!(read.outcome, crate::DiagnosticOutcome::MalformedResponse);
        assert_eq!(read.ecu_id.as_deref(), Some("7E8"));
        assert!(read.detail.unwrap().contains("truncated"));
        assert!(read.raw_response.contains("10 14 49 02 01 31 48 47"));
        assert_eq!(request_ids(&state).last().unwrap().1[0], 0x30);
    }

    #[test]
    fn mode06_bitmap_is_read_only_and_decoded_over_the_same_transport() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        state
            .lock()
            .unwrap()
            .responses
            .push_back(VecDeque::from([positive_single_frame(
                0x7E8,
                &[0x46, 0x00, 0, 0, 0, 0x20],
            )]));

        let crate::ResearchTestResult::Diagnostic(report) = adapter
            .execute_research_test(crate::ResearchTestRequest::Diagnostic(diagnostic_query(
                crate::DiagnosticReadKind::Mode06Bitmap,
            )))
            .unwrap()
        else {
            panic!("Mode 06 query returns a diagnostic report")
        };
        let read = report.reads.into_iter().next().unwrap();

        assert_eq!(read.outcome, crate::DiagnosticOutcome::Positive);
        assert_eq!(read.service, 0x06);
        assert_eq!(read.identifier, Some(0x00));
        assert_eq!(request_ids(&state).last().unwrap().1[1..3], [0x06, 0x00]);
    }

    #[test]
    fn dtc_clear_is_one_mode04_transmission_even_when_ecu_reports_pending_first() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        state.lock().unwrap().responses.push_back(VecDeque::from([
            rx(0x7E8, &[0x03, 0x7F, 0x04, 0x78, 0, 0, 0, 0]),
            positive_single_frame(0x7E8, &[0x44]),
        ]));

        assert!(
            adapter
                .execute_research_test(crate::ResearchTestRequest::Diagnostic(
                    crate::DiagnosticQueryRequest::clear_dtcs(),
                ))
                .is_err()
        );
        assert_eq!(
            request_ids(&state).len(),
            1,
            "research scans cannot clear DTCs"
        );

        let reads = adapter.clear_vehicle_dtcs().unwrap();

        assert_eq!(reads.len(), 2);
        assert_eq!(reads[0].outcome, crate::DiagnosticOutcome::ResponsePending);
        assert_eq!(reads[1].outcome, crate::DiagnosticOutcome::Positive);
        assert_eq!(reads[1].kind, crate::DiagnosticReadKind::ClearDtcs);
        assert_eq!(
            reads[1].values,
            [crate::DiagnosticValue::DtcClearAcknowledgement]
        );
        let requests = request_ids(&state);
        assert_eq!(requests.len(), 2, "Mode 04 is sent once, without retries");
        assert_eq!(requests[1], (0x7E0, vec![1, 0x04, 0, 0, 0, 0, 0, 0]));
        assert_eq!(reads[1].ecu_id.as_deref(), Some("7E8"));
        assert_eq!(reads[1].route, "CAN 11-bit physical 7E0 → 7E8");
    }

    #[test]
    fn diagnostics_preserve_negative_and_malformed_response_outcomes() {
        let (mut adapter, state) = adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        adapter.connect().unwrap();
        state.lock().unwrap().responses.extend([
            VecDeque::from([positive_single_frame(0x7E8, &[0x7F, 0x03, 0x11])]),
            VecDeque::from([positive_single_frame(0x7E8, &[0x43, 0x01])]),
            VecDeque::new(),
        ]);
        let mut stored = diagnostic_query(crate::DiagnosticReadKind::StoredDtcs);
        stored.wait_ms = 5;
        let pending = adapter
            .read_vehicle_diagnostic(stored.clone())
            .unwrap()
            .reads
            .remove(0);
        let malformed = adapter
            .read_vehicle_diagnostic(stored)
            .unwrap()
            .reads
            .remove(0);
        let mut no_response_query = diagnostic_query(crate::DiagnosticReadKind::StoredDtcs);
        no_response_query.wait_ms = 5;
        let no_response = adapter
            .read_vehicle_diagnostic(no_response_query)
            .unwrap()
            .reads
            .remove(0);

        assert_eq!(pending.outcome, crate::DiagnosticOutcome::NegativeResponse);
        assert_eq!(pending.negative_response_code, Some(0x11));
        assert_eq!(pending.ecu_id.as_deref(), Some("7E8"));
        assert_eq!(
            malformed.outcome,
            crate::DiagnosticOutcome::MalformedResponse
        );
        assert!(malformed.detail.is_some());
        assert_eq!(malformed.ecu_id.as_deref(), Some("7E8"));
        assert_eq!(no_response.outcome, crate::DiagnosticOutcome::NoResponse);
        assert_eq!(no_response.ecu_id.as_deref(), Some("7E8"));
    }

    #[test]
    fn failed_discovery_releases_transport_and_a_new_adapter_can_reconnect() {
        let (transport, failed_state) = fake([vec![None]]);
        let mut failed = CanObdAdapter::with_transport(transport);
        assert!(failed.connect().is_err());
        assert_eq!(failed_state.lock().unwrap().stopped, 1);

        let (mut reconnected, reconnected_state) =
            adapter_with_page00(&[0x06, 0x41, 0x00, 0, 0x10, 0, 0]);
        assert!(reconnected.connect().is_ok());
        reconnected.disconnect();
        assert_eq!(reconnected_state.lock().unwrap().stopped, 1);
    }

    #[test]
    fn normal_engine_acquisition_records_and_exports_can_obd_samples() {
        let (transport, _) = fake([
            vec![rx(0x7E8, &[0x06, 0x41, 0x00, 0, 0x10, 0, 0]), None],
            vec![rx(0x7E8, &[0x04, 0x41, 0x0C, 0x1A, 0xF8, 0, 0, 0]), None],
        ]);
        let mut engine = crate::LogNookEngine::new(|_| {});
        engine
            .connect_adapter(
                Box::new(CanObdAdapter::with_transport(transport)),
                &["engine_rpm".into()],
            )
            .unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-can-obd-{nonce}.lognook"));
        let csv_path = path.with_extension("csv");
        engine.start_recording_from_connect(&path).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        engine.stop_recording().unwrap();
        engine.disconnect().unwrap();

        let recording = crate::read_recording(&path).unwrap();
        assert!(recording.samples.iter().any(|sample| {
            sample.signal_id == "engine_rpm"
                && sample.value == 1726.0
                && sample.ecu_id.as_deref() == Some("7E8")
                && sample.raw_bytes.as_deref() == Some(&[4, 0x41, 0x0C, 0x1A, 0xF8, 0, 0, 0][..])
        }));
        crate::export_recording_csv(&path, &csv_path).unwrap();
        assert!(
            std::fs::read_to_string(&csv_path)
                .unwrap()
                .contains("04410C1AF8000000")
        );
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(csv_path);
    }
}
