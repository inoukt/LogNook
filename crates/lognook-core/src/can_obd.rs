//! Read-only SAE J1979 Mode 01 acquisition over a Candlelight GS_USB channel.

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
    use super::CanObdAdapter;
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
