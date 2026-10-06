use super::scan::{ScanTask, accept_scan_response, task_finished, tick_scan};
use super::{
    CanEvidenceRecord, CanScanConfig, CanSignalDefinition, CanTransport, CanTransportError,
    CanTransportEvent, CanWorkbenchStatus,
};
use crate::{
    CanCaptureWriter, CanDirection, CanFrame, EngineEvent, SamplePublisher, decode_can_signal,
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const EVENT_BATCH_MAX: usize = 64;
const EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(25);
const RECEIVE_SLICE: Duration = Duration::from_millis(5);
pub(super) enum Command {
    SetFields(Vec<CanSignalDefinition>, mpsc::Sender<Result<(), String>>),
    StartCapture(std::path::PathBuf, mpsc::Sender<Result<(), String>>),
    StopCapture(mpsc::Sender<Result<(), String>>),
    SendFrame(CanFrame, u64, u32, mpsc::Sender<Result<(), String>>),
    StartScan(CanScanConfig, mpsc::Sender<Result<(), String>>),
    StopScan(mpsc::Sender<()>),
    Shutdown,
}

struct SendTask {
    frame: CanFrame,
    pacing: Duration,
    remaining: u32,
    next_at: Instant,
}

pub(super) struct CanCaptureSession {
    frames: Option<CanCaptureWriter<File>>,
    journal: BufWriter<File>,
    failed: bool,
}

impl CanCaptureSession {
    fn create_new(
        path: impl AsRef<Path>,
        source_id: &str,
        bitrate: u32,
        listen_only: bool,
    ) -> Result<Self, String> {
        let path = path.as_ref();
        let sidecar_path = super::can_evidence_path(path);
        let journal_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&sidecar_path)
            .map_err(|error| format!("Cannot create CAN evidence journal: {error}"))?;
        let frames = match CanCaptureWriter::create_new(path) {
            Ok(writer) => writer,
            Err(error) => {
                let _ = fs::remove_file(&sidecar_path);
                return Err(error.to_string());
            }
        };
        let mut capture = Self {
            frames: Some(frames),
            journal: BufWriter::new(journal_file),
            failed: false,
        };
        if let Err(error) = capture.record(&CanEvidenceRecord::SessionStarted {
            format_version: 1,
            bitrate,
            listen_only,
            source_id: source_id.to_owned(),
        }) {
            drop(capture);
            let _ = fs::remove_file(path);
            let _ = fs::remove_file(sidecar_path);
            return Err(error);
        }
        Ok(capture)
    }

    pub(super) fn record(&mut self, record: &CanEvidenceRecord) -> Result<(), String> {
        let result = serde_json::to_writer(&mut self.journal, record)
            .and_then(|()| self.journal.write_all(b"\n").map_err(serde_json::Error::io))
            .and_then(|()| self.journal.flush().map_err(serde_json::Error::io));
        if let Err(error) = result {
            self.failed = true;
            return Err(error.to_string());
        }
        Ok(())
    }

    pub(super) fn write_frame(&mut self, frame: &CanFrame) -> Result<(), String> {
        if self.failed {
            return Err("CAN evidence journal is unavailable".into());
        }
        self.frames
            .as_mut()
            .ok_or_else(|| "CAN raw capture is already closed".to_owned())?
            .write_frame(frame)
            .map_err(|error| error.to_string())
    }

    pub(super) fn failed(&self) -> bool {
        self.failed
    }

    fn finish(mut self) -> Result<(), String> {
        let frame_result = self.frames.take().map_or(Ok(()), |writer| {
            writer.finish().map_err(|error| error.to_string())
        });
        let journal_result = self
            .journal
            .flush()
            .and_then(|()| self.journal.get_ref().sync_data());
        frame_result.and_then(|()| journal_result.map_err(|error| error.to_string()))
    }
}

pub(super) fn run_worker(
    mut transport: Box<dyn CanTransport>,
    commands: Receiver<Command>,
    publisher: SamplePublisher,
    event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    running: Arc<AtomicBool>,
    source_id: String,
    bitrate: u32,
    listen_only: bool,
    started: Instant,
) {
    let mut fields = Vec::<CanSignalDefinition>::new();
    let mut capture: Option<CanCaptureSession> = None;
    let mut frames = Vec::with_capacity(EVENT_BATCH_MAX);
    let mut last_flush = Instant::now();
    let mut send_task: Option<SendTask> = None;
    let mut scan_task: Option<ScanTask> = None;
    let mut pending_tx = HashMap::<u32, (u64, CanFrame)>::new();
    let mut next_tx_sequence = 1u64;
    let mut disconnected = false;
    loop {
        match commands.try_recv() {
            Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => break,
            Ok(command) => handle_command(
                command,
                &mut fields,
                &mut capture,
                &mut send_task,
                &mut scan_task,
                &event_sink,
                &source_id,
                bitrate,
                listen_only,
            ),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if disconnected {
            break;
        }
        if capture.as_ref().is_none_or(CanCaptureSession::failed) {
            send_task = None;
            scan_task = None;
        }
        if let Some(task) = send_task.as_mut() {
            if Instant::now() >= task.next_at {
                match submit_frame(
                    task.frame.clone(),
                    &mut transport,
                    &mut capture,
                    &mut fields,
                    &publisher,
                    &event_sink,
                    &mut frames,
                    &source_id,
                    &started,
                    &mut pending_tx,
                    &mut next_tx_sequence,
                ) {
                    Ok(_) => {
                        task.remaining -= 1;
                        task.next_at = Instant::now() + task.pacing;
                        if task.remaining == 0 {
                            send_task = None;
                        }
                    }
                    Err(error) => {
                        report_transport_error(
                            &event_sink,
                            &mut capture,
                            error,
                            Some(task.frame.clone()),
                        );
                        send_task = None;
                    }
                }
            }
        }
        if let Some(task) = scan_task.as_mut() {
            tick_scan(
                task,
                &mut transport,
                &mut capture,
                &mut fields,
                &publisher,
                &event_sink,
                &mut frames,
                &started,
                &source_id,
                &mut pending_tx,
                &mut next_tx_sequence,
            );
            if task_finished(task) {
                let config = task.config().clone();
                if let Some(session) = capture.as_mut()
                    && let Err(error) = session.record(&CanEvidenceRecord::ScanCompleted {
                        config: (&config).into(),
                    })
                {
                    (event_sink)(EngineEvent::Error(format!(
                        "Could not persist CAN scan completion evidence: {error}"
                    )));
                }
                (event_sink)(EngineEvent::CanScanCompleted(config));
                scan_task = None;
            }
        }
        if running.load(Ordering::Acquire) {
            match transport.receive(RECEIVE_SLICE) {
                Ok(Some(CanTransportEvent::Rx(frame))) => {
                    accept_frame(
                        frame.clone(),
                        &source_id,
                        &mut capture,
                        &mut fields,
                        &publisher,
                        &event_sink,
                        &mut frames,
                    );
                    if let Some(task) = scan_task.as_mut() {
                        accept_scan_response(
                            task,
                            &frame,
                            &mut transport,
                            &started,
                            &source_id,
                            &mut capture,
                            &mut fields,
                            &publisher,
                            &event_sink,
                            &mut frames,
                            &mut pending_tx,
                            &mut next_tx_sequence,
                        );
                    }
                }
                Ok(Some(CanTransportEvent::TxEcho {
                    echo_id,
                    frame,
                    raw_usb,
                })) => confirm_tx_echo(
                    echo_id,
                    frame,
                    raw_usb,
                    &mut pending_tx,
                    &mut capture,
                    &event_sink,
                ),
                Ok(None) => {}
                Err(error) => {
                    report_transport_error(&event_sink, &mut capture, error, None);
                    (event_sink)(EngineEvent::CanWorkbenchStatus(CanWorkbenchStatus {
                        connected: false,
                        bitrate,
                        listen_only,
                        source_id: source_id.clone(),
                    }));
                    if let Err(error) = transport.stop() {
                        report_transport_error(&event_sink, &mut capture, error, None);
                    }
                    running.store(false, Ordering::Release);
                    disconnected = true;
                }
            }
        }
        if !frames.is_empty()
            && (frames.len() >= EVENT_BATCH_MAX || last_flush.elapsed() >= EVENT_FLUSH_INTERVAL)
        {
            (event_sink)(EngineEvent::CanFrames(std::mem::take(&mut frames)));
            last_flush = Instant::now();
        }
    }
    if let Some(writer) = capture.take() {
        if let Err(error) = writer.finish() {
            (event_sink)(EngineEvent::Error(format!(
                "CAN capture finalization failed: {error}"
            )));
        }
    }
    if running.swap(false, Ordering::AcqRel) {
        if let Err(error) = transport.stop() {
            (event_sink)(EngineEvent::CanTransportError(error.clone()));
            (event_sink)(EngineEvent::Error(error.to_string()));
        }
    }
    if !frames.is_empty() {
        (event_sink)(EngineEvent::CanFrames(frames));
    }
}

fn handle_command(
    command: Command,
    fields: &mut Vec<CanSignalDefinition>,
    capture: &mut Option<CanCaptureSession>,
    send_task: &mut Option<SendTask>,
    scan_task: &mut Option<ScanTask>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    source_id: &str,
    bitrate: u32,
    listen_only: bool,
) {
    match command {
        Command::SetFields(next, reply) => {
            *fields = next;
            let _ = reply.send(Ok(()));
        }
        Command::StartCapture(path, reply) => {
            let result = if capture.is_some() {
                Err("A raw CAN capture is already active".to_owned())
            } else {
                CanCaptureSession::create_new(path, source_id, bitrate, listen_only)
                    .map(|session| *capture = Some(session))
            };
            let _ = reply.send(result);
        }
        Command::StopCapture(reply) => {
            if let Some(task) = scan_task.take() {
                if let Some(capture) = capture.as_mut() {
                    let _ = capture.record(&CanEvidenceRecord::ScanStopped {
                        config: (&*task.config()).into(),
                    });
                }
                (event_sink)(EngineEvent::CanScanStopped);
            }
            *send_task = None;
            let result = capture.take().map_or(Ok(()), |writer| {
                writer.finish().map_err(|error| error.to_string())
            });
            let _ = reply.send(result);
        }
        Command::SendFrame(frame, pacing_ms, repeat, reply) => {
            let result = if capture.as_ref().is_none_or(CanCaptureSession::failed) {
                Err(
                    "Start a raw CAN capture before sending; TX requires a paired evidence journal"
                        .to_owned(),
                )
            } else if transport_mode_conflicts(send_task.is_some(), scan_task.is_some()) {
                Err("A CAN send or scan is already active".to_owned())
            } else {
                *send_task = Some(SendTask {
                    frame,
                    pacing: Duration::from_millis(pacing_ms),
                    remaining: repeat,
                    next_at: Instant::now(),
                });
                Ok(())
            };
            let _ = reply.send(result);
        }
        Command::StartScan(config, reply) => {
            let result = if capture.as_ref().is_none_or(CanCaptureSession::failed) {
                Err("Start a raw CAN capture before scanning; scans require a paired evidence journal".to_owned())
            } else if transport_mode_conflicts(send_task.is_some(), scan_task.is_some()) {
                Err("A CAN send or scan is already active".to_owned())
            } else if let Some(capture) = capture.as_mut() {
                match capture.record(&CanEvidenceRecord::ScanStarted {
                    config: (&config).into(),
                }) {
                    Ok(()) => {
                        *scan_task = Some(ScanTask::new(config.clone()));
                        (event_sink)(EngineEvent::CanScanStarted(config));
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else if transport_mode_conflicts(send_task.is_some(), scan_task.is_some()) {
                Err("A CAN send or scan is already active".to_owned())
            } else {
                Err("Start a raw CAN capture before scanning".to_owned())
            };
            let _ = reply.send(result);
        }
        Command::StopScan(reply) => {
            if let Some(task) = scan_task.take() {
                if let Some(capture) = capture.as_mut() {
                    let _ = capture.record(&CanEvidenceRecord::ScanStopped {
                        config: (&*task.config()).into(),
                    });
                }
                (event_sink)(EngineEvent::CanScanStopped);
            }
            *send_task = None;
            let _ = reply.send(());
        }
        Command::Shutdown => {}
    }
}

fn transport_mode_conflicts(send: bool, scan: bool) -> bool {
    send || scan
}

pub(super) fn accept_frame(
    frame: CanFrame,
    source_id: &str,
    capture: &mut Option<CanCaptureSession>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
) {
    if let Some(session) = capture.as_mut() {
        if let Err(error) = session.write_frame(&frame) {
            let message = error;
            *capture = None;
            (event_sink)(EngineEvent::Error(format!(
                "CAN capture stopped after a write failure: {message}"
            )));
        }
    }
    if frame.direction == CanDirection::Rx {
        for field in fields.iter() {
            match decode_can_signal(&frame, field, source_id) {
                Ok(Some(sample)) => {
                    if let Err(error) = publisher.publish_timestamped_sample(sample) {
                        (event_sink)(EngineEvent::Error(error.to_string()));
                    }
                }
                Ok(None) => {}
                Err(error) => (event_sink)(EngineEvent::Error(error.to_string())),
            }
        }
    }
    batch.push(frame);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn submit_frame(
    mut frame: CanFrame,
    transport: &mut Box<dyn CanTransport>,
    capture: &mut Option<CanCaptureSession>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
    source_id: &str,
    started: &Instant,
    pending_tx: &mut HashMap<u32, (u64, CanFrame)>,
    next_tx_sequence: &mut u64,
) -> Result<(CanFrame, u64), CanTransportError> {
    let session = capture
        .as_mut()
        .filter(|session| !session.failed())
        .ok_or_else(|| {
            CanTransportError::other("Active CAN transmission requires an evidence journal")
        })?;
    stamp_tx(&mut frame, started);
    let sequence = *next_tx_sequence;
    *next_tx_sequence = next_tx_sequence.saturating_add(1);
    session
        .record(&CanEvidenceRecord::TxAttempt {
            sequence,
            frame: frame.clone(),
        })
        .map_err(CanTransportError::other)?;
    let echo_id = match transport.transmit(&frame) {
        Ok(echo_id) => echo_id,
        Err(error) => {
            if let Some(session) = capture.as_mut() {
                let _ = session.record(&CanEvidenceRecord::TransportError {
                    error: error.clone(),
                    frame: Some(frame.clone()),
                });
            }
            return Err(error);
        }
    };
    pending_tx.insert(echo_id, (sequence, frame.clone()));
    let evidence_result = capture
        .as_mut()
        .ok_or_else(|| CanTransportError::other("CAN capture closed after TX"))?
        .record(&CanEvidenceRecord::TxSubmitted {
            sequence,
            echo_id,
            frame: frame.clone(),
        })
        .map_err(CanTransportError::other);
    (event_sink)(EngineEvent::CanTxStatus {
        sequence,
        echo_id,
        frame: frame.clone(),
        confirmed: false,
    });
    accept_frame(
        frame.clone(),
        source_id,
        capture,
        fields,
        publisher,
        event_sink,
        batch,
    );
    evidence_result?;
    Ok((frame, sequence))
}

fn confirm_tx_echo(
    echo_id: u32,
    echo_frame: CanFrame,
    raw_usb: Vec<u8>,
    pending_tx: &mut HashMap<u32, (u64, CanFrame)>,
    capture: &mut Option<CanCaptureSession>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    let Some((sequence, frame)) = pending_tx.remove(&echo_id) else {
        let error = CanTransportError {
            kind: super::CanTransportErrorKind::Protocol,
            message: format!("Received unassigned Candlelight TX echo {echo_id}"),
            raw_usb: Some(raw_usb),
        };
        report_transport_error(event_sink, capture, error, None);
        return;
    };
    let record_result = capture
        .as_mut()
        .ok_or_else(|| "CAN TX echo arrived after evidence capture stopped".to_owned())
        .and_then(|session| {
            session.record(&CanEvidenceRecord::TxEchoConfirmed {
                sequence,
                echo_id,
                submitted_frame: frame.clone(),
                echo_frame: echo_frame.clone(),
                raw_usb,
            })
        });
    (event_sink)(EngineEvent::CanTxStatus {
        sequence,
        echo_id,
        frame: echo_frame,
        confirmed: true,
    });
    if let Err(message) = record_result {
        (event_sink)(EngineEvent::Error(format!(
            "Could not record Candlelight TX echo confirmation: {message}"
        )));
    }
}

pub(super) fn report_transport_error(
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    capture: &mut Option<CanCaptureSession>,
    error: CanTransportError,
    frame: Option<CanFrame>,
) {
    if let Some(session) = capture.as_mut() {
        if let Err(journal_error) = session.record(&CanEvidenceRecord::TransportError {
            error: error.clone(),
            frame,
        }) {
            (event_sink)(EngineEvent::Error(format!(
                "Could not persist CAN transport error evidence: {journal_error}"
            )));
        }
    }
    (event_sink)(EngineEvent::CanTransportError(error.clone()));
    (event_sink)(EngineEvent::Error(error.to_string()));
}

pub(super) fn stamp_tx(frame: &mut CanFrame, started: &Instant) {
    frame.elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
    frame.wall_time_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64;
    frame.direction = CanDirection::Tx;
}
