use super::worker::{CanCaptureSession, PendingTx, stamp_tx, submit_frame};
use super::{
    CanEvidenceRecord, CanEvidenceScanResult, CanScanConfig, CanScanMode, CanScanResult,
    CanTransport, CanTransportError,
};
use crate::{
    CanDiagnosticOutcome, CanDirection, CanFrame, CanSignalDefinition, EngineEvent,
    SamplePublisher,
    obdlink_sx_protocol::{
        decode_signal, mode01_pid_data_width, parse_mode01_payload, parse_supported_pid_payload,
    },
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_SCAN_RESPONSE_FRAMES: usize = 600;
pub(super) struct ScanTask {
    config: CanScanConfig,
    next_value: u32,
    pages: Vec<u8>,
    page_index: usize,
    due: Instant,
    pending: Option<PendingResponse>,
    repeating: bool,
    transport_error: Option<CanTransportError>,
}
struct PendingResponse {
    identifier: String,
    request: CanFrame,
    frames: Vec<CanFrame>,
    deadline: Instant,
    expected_service: u8,
    expected_identifier: Vec<u8>,
    is_mode01: bool,
    flow_control_sent: bool,
    overflowed: bool,
}

impl ScanTask {
    pub(super) fn new(config: CanScanConfig) -> Self {
        let pages = match &config.mode {
            CanScanMode::Mode01Support { .. } => {
                (0..=0xE0).step_by(0x20).map(|value| value as u8).collect()
            }
            CanScanMode::Mode01Pids { pids, .. } => pids.clone(),
            _ => Vec::new(),
        };
        let next_value = match &config.mode {
            CanScanMode::CanIdRange { start_id, .. } => *start_id,
            CanScanMode::Mode21 { start_pid, .. } => u32::from(*start_pid),
            CanScanMode::Mode22 { start_did, .. } => u32::from(*start_did),
            CanScanMode::Mode01Support { .. } | CanScanMode::Mode01Pids { .. } => 0,
        };
        let repeating = matches!(config.mode, CanScanMode::Mode01Pids { repeat: true, .. });
        Self {
            config,
            next_value,
            pages,
            page_index: 0,
            due: Instant::now(),
            pending: None,
            repeating,
            transport_error: None,
        }
    }

    pub(super) fn config(&self) -> &CanScanConfig {
        &self.config
    }

    pub(super) fn terminal_result(&self, error: super::CanTransportError) -> Option<CanScanResult> {
        let pending = self.pending.as_ref()?;
        Some(CanScanResult {
            config: self.config.clone(),
            identifier: pending.identifier.clone(),
            request: pending.request.clone(),
            response_frames: pending.frames.clone(),
            outcome: CanDiagnosticOutcome::TransportError { error },
            supported_pids: None,
        })
    }

    pub(super) fn take_transport_error(&mut self) -> Option<CanTransportError> {
        self.transport_error.take()
    }

    pub(super) fn has_pending_response(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn advance_after_send_error(&mut self) {
        self.pending = None;
        advance_scan(self);
        self.due = Instant::now() + Duration::from_millis(self.config.pacing_ms);
    }
}

pub(super) fn tick_scan(
    task: &mut ScanTask,
    transport: &mut Box<dyn CanTransport>,
    capture: &mut Option<CanCaptureSession>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
    started: &Instant,
    source_id: &str,
    pending_tx: &mut HashMap<u32, PendingTx>,
    next_tx_sequence: &mut u64,
) {
    if task.transport_error.is_some() {
        return;
    }
    let now = Instant::now();
    if let Some(pending) = task.pending.as_ref() {
        if now < pending.deadline {
            return;
        }
        let pending = task.pending.take().unwrap();
        let outcome = if pending.frames.is_empty() {
            CanDiagnosticOutcome::NoResponse
        } else {
            CanDiagnosticOutcome::Malformed
        };
        if pending.overflowed {
            (event_sink)(EngineEvent::Error(format!(
                "CAN scan response {} exceeded the {}-frame retained response limit; raw traffic remains in the capture",
                pending.identifier, MAX_SCAN_RESPONSE_FRAMES
            )));
        }
        let result = CanScanResult {
            config: task.config.clone(),
            identifier: pending.identifier,
            request: pending.request,
            response_frames: pending.frames,
            outcome,
            supported_pids: None,
        };
        emit_result(result, capture, event_sink);
        task.due = now + Duration::from_millis(task.config.pacing_ms);
        advance_scan(task);
    }
    if Instant::now() < task.due {
        return;
    }
    let Some((
        can_id,
        extended,
        payload,
        response_route,
        identifier,
        expected_service,
        expected_identifier,
        is_mode01,
    )) = scan_request(task)
    else {
        return;
    };
    let mut frame = CanFrame {
        elapsed_ns: 0,
        wall_time_unix_ns: 0,
        can_id,
        extended,
        direction: CanDirection::Tx,
        data: payload,
        flags: 0,
    };
    stamp_tx(&mut frame, started);
    if response_route.is_some() {
        task.pending = Some(PendingResponse {
            identifier,
            request: frame.clone(),
            frames: Vec::new(),
            deadline: Instant::now() + Duration::from_millis(task.config.response_timeout_ms),
            expected_service,
            expected_identifier,
            is_mode01,
            flow_control_sent: false,
            overflowed: false,
        });
    }
    let (frame, _) = match submit_frame(
        frame,
        transport,
        capture,
        fields,
        publisher,
        event_sink,
        batch,
        source_id,
        started,
        pending_tx,
        next_tx_sequence,
    ) {
        Ok(submitted) => submitted,
        Err(error) => {
            task.transport_error = Some(error);
            return;
        }
    };
    if response_route.is_some() {
        if let Some(pending) = task.pending.as_mut() {
            pending.request = frame;
            pending.deadline =
                Instant::now() + Duration::from_millis(task.config.response_timeout_ms);
        }
    } else {
        advance_scan(task);
        task.due = Instant::now() + Duration::from_millis(task.config.pacing_ms);
    }
}

fn scan_request(
    task: &ScanTask,
) -> Option<(u32, bool, Vec<u8>, Option<u32>, String, u8, Vec<u8>, bool)> {
    match &task.config.mode {
        CanScanMode::CanIdRange {
            end_id,
            extended,
            payload,
            ..
        } => (task.next_value <= *end_id).then(|| {
            (
                task.next_value,
                *extended,
                payload.clone(),
                None,
                format!("{:X}", task.next_value),
                0,
                Vec::new(),
                false,
            )
        }),
        CanScanMode::Mode21 {
            request_id,
            response_id,
            extended,
            end_pid,
            ..
        } => (task.next_value <= u32::from(*end_pid)).then(|| {
            (
                *request_id,
                *extended,
                vec![2, 0x21, task.next_value as u8, 0, 0, 0, 0, 0],
                Some(*response_id),
                format!("21 {:02X}", task.next_value),
                0x21,
                vec![task.next_value as u8],
                false,
            )
        }),
        CanScanMode::Mode22 {
            request_id,
            response_id,
            extended,
            end_did,
            ..
        } => (task.next_value <= u32::from(*end_did)).then(|| {
            (
                *request_id,
                *extended,
                vec![
                    3,
                    0x22,
                    (task.next_value >> 8) as u8,
                    task.next_value as u8,
                    0,
                    0,
                    0,
                    0,
                ],
                Some(*response_id),
                format!("22 {:04X}", task.next_value),
                0x22,
                vec![(task.next_value >> 8) as u8, task.next_value as u8],
                false,
            )
        }),
        CanScanMode::Mode01Support {
            request_id,
            response_id,
            extended,
        } => task.pages.get(task.page_index).copied().map(|pid| {
            (
                *request_id,
                *extended,
                vec![2, 0x01, pid, 0, 0, 0, 0, 0],
                Some(*response_id),
                format!("01 {pid:02X}"),
                0x01,
                vec![pid],
                true,
            )
        }),
        CanScanMode::Mode01Pids {
            request_id,
            response_id,
            extended,
            ..
        } => task.pages.get(task.page_index).copied().map(|pid| {
            (
                *request_id,
                *extended,
                vec![2, 0x01, pid, 0, 0, 0, 0, 0],
                Some(*response_id),
                format!("01 {pid:02X}"),
                0x01,
                vec![pid],
                true,
            )
        }),
    }
}

pub(super) fn accept_scan_response(
    task: &mut ScanTask,
    frame: &CanFrame,
    transport: &mut Box<dyn CanTransport>,
    started: &Instant,
    source_id: &str,
    capture: &mut Option<CanCaptureSession>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
    pending_tx: &mut HashMap<u32, PendingTx>,
    next_tx_sequence: &mut u64,
) {
    let Some(pending) = task.pending.as_mut() else {
        return;
    };
    let response_id = match task.config.mode {
        CanScanMode::Mode21 { response_id, .. }
        | CanScanMode::Mode22 { response_id, .. }
        | CanScanMode::Mode01Support { response_id, .. }
        | CanScanMode::Mode01Pids { response_id, .. } => response_id,
        CanScanMode::CanIdRange { .. } => return,
    };
    if frame.direction != CanDirection::Rx
        || frame.can_id != response_id
        || frame.extended != pending.request.extended
    {
        return;
    }
    if pending.frames.len() >= MAX_SCAN_RESPONSE_FRAMES {
        pending.overflowed = true;
        return;
    }
    pending.frames.push(frame.clone());
    if frame.data.first().is_some_and(|byte| byte >> 4 == 1) && !pending.flow_control_sent {
        pending.flow_control_sent = true;
        let request_id = match task.config.mode {
            CanScanMode::Mode21 { request_id, .. }
            | CanScanMode::Mode22 { request_id, .. }
            | CanScanMode::Mode01Support { request_id, .. }
            | CanScanMode::Mode01Pids { request_id, .. } => request_id,
            CanScanMode::CanIdRange { .. } => return,
        };
        let flow_control = CanFrame {
            elapsed_ns: 0,
            wall_time_unix_ns: 0,
            can_id: request_id,
            extended: frame.extended,
            direction: CanDirection::Tx,
            data: vec![0x30, 0, 0, 0, 0, 0, 0, 0],
            flags: 0,
        };
        if let Err(error) = submit_frame(
            flow_control.clone(),
            transport,
            capture,
            fields,
            publisher,
            event_sink,
            batch,
            source_id,
            started,
            pending_tx,
            next_tx_sequence,
        ) {
            task.transport_error = Some(error);
            return;
        }
    }
    if let Some(payload) = isotp_payload(&pending.frames) {
        let mut outcome = diagnostic_outcome(
            &payload,
            pending.expected_service,
            &pending.expected_identifier,
        );
        if matches!(outcome, CanDiagnosticOutcome::ResponsePending { .. }) {
            let interim = CanScanResult {
                config: task.config.clone(),
                identifier: pending.identifier.clone(),
                request: pending.request.clone(),
                response_frames: pending.frames.clone(),
                outcome,
                supported_pids: None,
            };
            emit_result(interim, capture, event_sink);
            pending.frames.clear();
            pending.overflowed = false;
            pending.deadline =
                Instant::now() + Duration::from_millis(task.config.response_timeout_ms);
            return;
        }
        let completed = task.pending.take().unwrap();
        let mut supported_pids = None;
        if completed.is_mode01 && outcome == CanDiagnosticOutcome::Positive {
            if completed.expected_service == 0x01 && completed.expected_identifier.len() == 1 {
                let pid = completed.expected_identifier[0];
                if matches!(&task.config.mode, CanScanMode::Mode01Pids { .. }) {
                    if mode01_pid_data_width(pid).is_some() {
                        match parse_mode01_payload(format!("{:03X}", frame.can_id), &payload, pid) {
                            Ok(parsed) => {
                                if let Some(signal) = crate::standard_obd_signals()
                                    .into_iter()
                                    .find(|signal| signal.pid == pid)
                                {
                                    match decode_signal(&signal, &parsed) {
                                        Ok(value) => {
                                            let sample = crate::Sample {
                                                elapsed_ns: frame.elapsed_ns,
                                                wall_time_unix_ns: frame.wall_time_unix_ns,
                                                source_id: format!("{source_id}:obd"),
                                                ecu_id: Some(format!("{:03X}", frame.can_id)),
                                                signal_id: signal.id,
                                                value,
                                                unit: signal.unit,
                                                quality: crate::SampleQuality::Valid,
                                                origin: crate::SampleOrigin::Live,
                                                raw_bytes: Some(
                                                    completed
                                                        .frames
                                                        .iter()
                                                        .flat_map(|frame| {
                                                            frame.data.iter().copied()
                                                        })
                                                        .collect(),
                                                ),
                                            };
                                            if let Err(error) =
                                                publisher.publish_timestamped_sample(sample)
                                            {
                                                (event_sink)(EngineEvent::Error(error.to_string()));
                                            }
                                        }
                                        Err(error) => {
                                            outcome = CanDiagnosticOutcome::Malformed;
                                            (event_sink)(EngineEvent::Error(format!(
                                                "Mode 01 PID {pid:02X} decode failed: {error}"
                                            )));
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                outcome = CanDiagnosticOutcome::Malformed;
                                (event_sink)(EngineEvent::Error(format!(
                                    "Mode 01 PID {pid:02X} decode failed: {error}"
                                )));
                            }
                        }
                    }
                } else if matches!(&task.config.mode, CanScanMode::Mode01Support { .. }) {
                    match parse_supported_pid_payload(
                        format!("{:03X}", frame.can_id),
                        &payload,
                        pid,
                    ) {
                        Ok(supported) => {
                            supported_pids = Some(supported.pids);
                            if !supported.has_more {
                                task.page_index = task.pages.len();
                            }
                        }
                        Err(error) => {
                            outcome = CanDiagnosticOutcome::Malformed;
                            (event_sink)(EngineEvent::Error(format!(
                                "Mode 01 support page {pid:02X} decode failed: {error}"
                            )));
                        }
                    }
                }
            }
        }
        emit_result(
            CanScanResult {
                config: task.config.clone(),
                identifier: completed.identifier,
                request: completed.request,
                response_frames: completed.frames,
                outcome,
                supported_pids,
            },
            capture,
            event_sink,
        );
        task.due = Instant::now() + Duration::from_millis(task.config.pacing_ms);
        advance_scan(task);
    }
}

pub(super) fn emit_result(
    result: CanScanResult,
    capture: &mut Option<CanCaptureSession>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    if let Some(session) = capture.as_mut()
        && let Err(error) = session.record(&CanEvidenceRecord::ScanResult {
            result: CanEvidenceScanResult::from(&result),
        })
    {
        (event_sink)(EngineEvent::Error(format!(
            "Could not persist CAN scan result evidence: {error}"
        )));
    }
    (event_sink)(EngineEvent::CanScanResult(result));
}

fn isotp_payload(frames: &[CanFrame]) -> Option<Vec<u8>> {
    let first = frames.first()?;
    let pci = *first.data.first()? >> 4;
    match pci {
        0 => {
            let len = usize::from(first.data[0] & 0x0F);
            (len > 0 && first.data.len() >= len + 1).then(|| first.data[1..=len].to_vec())
        }
        1 => {
            if first.data.len() < 3 {
                return None;
            }
            let len = (usize::from(first.data[0] & 0x0F) << 8) | usize::from(first.data[1]);
            let mut payload = first.data[2..].to_vec();
            let mut expected_seq = 1u8;
            for frame in frames.iter().skip(1) {
                if frame
                    .data
                    .first()
                    .is_none_or(|byte| byte >> 4 != 2 || byte & 0x0F != expected_seq)
                {
                    return None;
                }
                payload.extend_from_slice(&frame.data[1..]);
                expected_seq = (expected_seq + 1) & 0x0F;
                if payload.len() >= len {
                    payload.truncate(len);
                    return Some(payload);
                }
            }
            None
        }
        _ => None,
    }
}

fn diagnostic_outcome(payload: &[u8], service: u8, identifier: &[u8]) -> CanDiagnosticOutcome {
    if payload.first() == Some(&0x7F) && payload.len() >= 3 && payload[1] == service {
        return if payload[2] == 0x78 {
            CanDiagnosticOutcome::ResponsePending { nrc: payload[2] }
        } else {
            CanDiagnosticOutcome::Negative { nrc: payload[2] }
        };
    }
    if payload.first() == Some(&service.wrapping_add(0x40))
        && payload.get(1..1 + identifier.len()) == Some(identifier)
    {
        CanDiagnosticOutcome::Positive
    } else if payload.len() < identifier.len() + 2 {
        CanDiagnosticOutcome::Malformed
    } else {
        CanDiagnosticOutcome::WrongEcho
    }
}

fn advance_scan(task: &mut ScanTask) {
    match &task.config.mode {
        CanScanMode::CanIdRange { end_id, .. } if task.next_value < *end_id => task.next_value += 1,
        CanScanMode::CanIdRange { end_id, .. } => task.next_value = end_id.saturating_add(1),
        CanScanMode::Mode21 { end_pid, .. } if task.next_value < u32::from(*end_pid) => {
            task.next_value += 1
        }
        CanScanMode::Mode21 { end_pid, .. } => task.next_value = u32::from(*end_pid) + 1,
        CanScanMode::Mode22 { end_did, .. } if task.next_value < u32::from(*end_did) => {
            task.next_value += 1
        }
        CanScanMode::Mode22 { end_did, .. } => task.next_value = u32::from(*end_did) + 1,
        CanScanMode::Mode01Support { .. } | CanScanMode::Mode01Pids { .. }
            if task.page_index + 1 < task.pages.len() =>
        {
            task.page_index += 1
        }
        _ if task.repeating => task.page_index = 0,
        _ => task.page_index = task.pages.len(),
    }
}

pub(super) fn task_finished(task: &ScanTask) -> bool {
    if task.pending.is_some() {
        return false;
    }
    match &task.config.mode {
        CanScanMode::CanIdRange { end_id, .. } => task.next_value > *end_id,
        CanScanMode::Mode21 { end_pid, .. } => task.next_value > u32::from(*end_pid),
        CanScanMode::Mode22 { end_did, .. } => task.next_value > u32::from(*end_did),
        CanScanMode::Mode01Support { .. } | CanScanMode::Mode01Pids { .. } => {
            task.page_index >= task.pages.len() && !task.repeating
        }
    }
}
