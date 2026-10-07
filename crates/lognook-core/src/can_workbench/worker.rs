use super::scan::{self, ScanTask, accept_scan_response, task_finished, tick_scan};
use super::{
    CanEvidenceRecord, CanManualEventMark, CanScanConfig, CanSignalDefinition, CanTransport,
    CanTransportError, CanTransportEvent, CanTriggerCaptureResult, CanTriggerConfig,
    CanTriggerEndReason, CanTriggerRecorder, CanWorkbenchStatus,
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
    StopScan(mpsc::Sender<Result<(), String>>),
    ArmTrigger(CanTriggerConfig, mpsc::Sender<Result<(), String>>),
    TriggerNow(mpsc::Sender<Result<(), String>>),
    MarkEvent {
        label: String,
        pre_ns: i64,
        post_ns: i64,
        reply: mpsc::Sender<Result<CanManualEventMark, String>>,
    },
    DisarmTrigger(mpsc::Sender<Result<(), String>>),
    Shutdown,
}

struct SendTask {
    frame: CanFrame,
    pacing: Duration,
    remaining: u32,
    next_at: Instant,
}

pub(super) struct PendingTx {
    capture_id: u64,
    sequence: u64,
    frame: CanFrame,
}

pub(super) struct CanCaptureSession {
    capture_id: u64,
    frames: Option<CanCaptureWriter<File>>,
    journal: BufWriter<File>,
    failed: bool,
}

impl CanCaptureSession {
    fn create_new(
        path: impl AsRef<Path>,
        capture_id: u64,
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
            capture_id,
            frames: Some(frames),
            journal: BufWriter::new(journal_file),
            failed: false,
        };
        if let Err(error) = capture.record(&CanEvidenceRecord::SessionStarted {
            format_version: 2,
            capture_id,
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
        if self.failed {
            return Err("CAN evidence journal is unavailable after a write failure".into());
        }
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

    fn capture_id(&self) -> u64 {
        self.capture_id
    }

    fn finish(mut self) -> Result<(), String> {
        if self.failed {
            return Err("CAN capture cannot close cleanly after an evidence write failure".into());
        }
        if let Some(writer) = self.frames.take()
            && let Err(error) = writer.finish()
        {
            self.failed = true;
            return Err(error.to_string());
        }
        self.record(&CanEvidenceRecord::SessionFinished {
            capture_id: self.capture_id,
        })?;
        self.journal
            .get_ref()
            .sync_data()
            .map_err(|error| error.to_string())
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
    let mut pending_tx = HashMap::<u32, PendingTx>::new();
    let mut next_tx_sequence = 1u64;
    let mut next_capture_id = 1u64;
    let mut trigger: Option<(CanTriggerRecorder, u64, CanTriggerConfig)> = None;
    let mut disconnected = false;
    loop {
        if disconnected {
            break;
        }
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
                &mut next_capture_id,
                &mut trigger,
                &started,
            ),
            Err(mpsc::TryRecvError::Empty) => {}
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
                            error.clone(),
                            Some(task.frame.clone()),
                        );
                        send_task = None;
                        if error.terminates_session() {
                            disconnect_transport(
                                &mut transport,
                                &event_sink,
                                &mut capture,
                                &running,
                                &mut disconnected,
                                source_id.clone(),
                                bitrate,
                                listen_only,
                            );
                        }
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
        }
        if let Some(error) = scan_task.as_mut().and_then(ScanTask::take_transport_error) {
            handle_scan_transport_error(
                error,
                &mut scan_task,
                &mut transport,
                &event_sink,
                &mut capture,
                &running,
                &mut disconnected,
                source_id.clone(),
                bitrate,
                listen_only,
            );
        }
        if let Some(task) = scan_task.as_ref()
            && task_finished(task)
        {
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
                    feed_trigger(&mut trigger, &frame, &event_sink);
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
                    if let Some(error) = scan_task.as_mut().and_then(ScanTask::take_transport_error)
                    {
                        handle_scan_transport_error(
                            error,
                            &mut scan_task,
                            &mut transport,
                            &event_sink,
                            &mut capture,
                            &running,
                            &mut disconnected,
                            source_id.clone(),
                            bitrate,
                            listen_only,
                        );
                    }
                }
                Ok(Some(CanTransportEvent::TxEcho {
                    echo_id,
                    frame,
                    raw_usb,
                })) => {
                    feed_trigger(&mut trigger, &frame, &event_sink);
                    confirm_tx_echo(
                        echo_id,
                        frame,
                        raw_usb,
                        &mut pending_tx,
                        &mut capture,
                        &event_sink,
                    )
                }
                Ok(None) => {}
                Err(error) => {
                    report_transport_error(&event_sink, &mut capture, error.clone(), None);
                    if error.terminates_scan() {
                        if let Some(result) = scan_task
                            .take()
                            .and_then(|task| task.terminal_result(error.clone()))
                        {
                            scan::emit_result(result, &mut capture, &event_sink);
                        }
                    }
                    if error.terminates_session() {
                        disconnect_transport(
                            &mut transport,
                            &event_sink,
                            &mut capture,
                            &running,
                            &mut disconnected,
                            source_id.clone(),
                            bitrate,
                            listen_only,
                        );
                    }
                }
            }
        }
        if !frames.is_empty()
            && (frames.len() >= EVENT_BATCH_MAX || last_flush.elapsed() >= EVENT_FLUSH_INTERVAL)
        {
            (event_sink)(EngineEvent::CanFrames(std::mem::take(&mut frames)));
            last_flush = Instant::now();
        }
        advance_trigger(
            &mut trigger,
            &started,
            &source_id,
            bitrate,
            listen_only,
            &mut capture,
            &event_sink,
        );
    }
    if let Some((mut recorder, capture_id, _config)) = trigger.take()
        && let Some(window) = recorder.take_capture()
    {
        if let Err(error) = record_trigger_termination(
            &mut capture,
            capture_id,
            &window,
            Some(CanTriggerEndReason::SessionEnded),
        ) {
            (event_sink)(EngineEvent::Error(format!(
                "Could not persist trigger completion evidence: {error}"
            )));
        }
        emit_trigger_capture(
            window,
            capture_id,
            &source_id,
            bitrate,
            listen_only,
            &event_sink,
        );
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
    next_capture_id: &mut u64,
    trigger: &mut Option<(CanTriggerRecorder, u64, CanTriggerConfig)>,
    started: &Instant,
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
                let capture_id = *next_capture_id;
                CanCaptureSession::create_new(path, capture_id, source_id, bitrate, listen_only)
                    .map(|session| {
                        *capture = Some(session);
                        *next_capture_id = next_capture_id.saturating_add(1);
                    })
            };
            let _ = reply.send(result);
        }
        Command::StopCapture(reply) => {
            let stop_result = if let Some(task) = scan_task.take() {
                let result = capture
                    .as_mut()
                    .ok_or_else(|| "CAN scan stopped without an active evidence journal".to_owned())
                    .and_then(|capture| {
                        capture.record(&CanEvidenceRecord::ScanStopped {
                            config: task.config().into(),
                        })
                    });
                (event_sink)(EngineEvent::CanScanStopped);
                result
            } else {
                Ok(())
            };
            *send_task = None;
            let trigger_result = if let Some((mut recorder, capture_id, _)) = trigger.take()
                && let Some(window) = recorder.take_capture()
            {
                let result = record_trigger_termination(
                    capture,
                    capture_id,
                    &window,
                    Some(CanTriggerEndReason::CaptureStopped),
                );
                emit_trigger_capture(
                    window,
                    capture_id,
                    source_id,
                    bitrate,
                    listen_only,
                    event_sink,
                );
                result
            } else {
                Ok(())
            };
            let finish_result = capture.take().map_or(Ok(()), |writer| {
                writer.finish().map_err(|error| error.to_string())
            });
            let _ = reply.send(stop_result.and(trigger_result).and(finish_result));
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
            let result = if let Some(task) = scan_task.take() {
                let result = capture
                    .as_mut()
                    .ok_or_else(|| "CAN scan stopped without an active evidence journal".to_owned())
                    .and_then(|capture| {
                        capture.record(&CanEvidenceRecord::ScanStopped {
                            config: task.config().into(),
                        })
                    });
                (event_sink)(EngineEvent::CanScanStopped);
                result
            } else {
                Ok(())
            };
            *send_task = None;
            let _ = reply.send(result);
        }
        Command::ArmTrigger(config, reply) => {
            let result = if let Some(capture) = capture.as_ref() {
                CanTriggerRecorder::new(config.clone())
                    .map_err(|error| error.to_string())
                    .and_then(|recorder| {
                        *trigger = Some((recorder, capture.capture_id(), config));
                        Ok(())
                    })
            } else {
                Err("Start a paired raw capture before arming a CAN trigger".into())
            };
            let _ = reply.send(result);
        }
        Command::TriggerNow(reply) => {
            let now_ns = elapsed_ns(started);
            let result = if let Some(session) = capture.as_mut() {
                trigger
                    .as_mut()
                    .ok_or_else(|| "Arm a CAN trigger before marking its time".to_owned())
                    .and_then(|(recorder, capture_id, config)| {
                        recorder
                            .trigger(now_ns)
                            .map_err(|error| error.to_string())?;
                        session.record(&CanEvidenceRecord::TriggerMarked {
                            capture_id: *capture_id,
                            trigger_elapsed_ns: now_ns,
                            pre_ns: config.pre_ns,
                            post_ns: config.post_ns,
                            max_frames: config.max_frames,
                        })
                    })
            } else {
                Err("Paired raw capture closed before the trigger".into())
            };
            let _ = reply.send(result);
        }
        Command::MarkEvent {
            label,
            pre_ns,
            post_ns,
            reply,
        } => {
            let mark = CanManualEventMark {
                label,
                elapsed_ns: elapsed_ns(started),
                pre_ns,
                post_ns,
            };
            let result = if let Some(session) = capture.as_mut() {
                session
                    .record(&CanEvidenceRecord::ManualEventMarked {
                        capture_id: session.capture_id(),
                        mark: mark.clone(),
                    })
                    .map(|()| mark)
            } else {
                Err("Start a raw CAN capture before marking an event".to_owned())
            };
            let _ = reply.send(result);
        }
        Command::DisarmTrigger(reply) => {
            let result = if let Some((mut recorder, capture_id, _)) = trigger.take()
                && let Some(window) = recorder.take_capture()
            {
                let result = record_trigger_termination(
                    capture,
                    capture_id,
                    &window,
                    Some(CanTriggerEndReason::Disarmed),
                );
                emit_trigger_capture(
                    window,
                    capture_id,
                    source_id,
                    bitrate,
                    listen_only,
                    event_sink,
                );
                result
            } else {
                Ok(())
            };
            let _ = reply.send(result);
        }
        Command::Shutdown => {}
    }
}

fn transport_mode_conflicts(send: bool, scan: bool) -> bool {
    send || scan
}

fn elapsed_ns(started: &Instant) -> i64 {
    started.elapsed().as_nanos().min(i64::MAX as u128) as i64
}

fn feed_trigger(
    trigger: &mut Option<(CanTriggerRecorder, u64, CanTriggerConfig)>,
    frame: &CanFrame,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    if let Some((recorder, _, _)) = trigger
        && let Err(error) = recorder.feed(frame)
    {
        trigger.take();
        (event_sink)(EngineEvent::Error(format!(
            "CAN trigger capture stopped: {error}"
        )));
    }
}

fn advance_trigger(
    trigger: &mut Option<(CanTriggerRecorder, u64, CanTriggerConfig)>,
    started: &Instant,
    source_id: &str,
    bitrate: u32,
    listen_only: bool,
    capture: &mut Option<CanCaptureSession>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    let Some((recorder, _, _)) = trigger.as_mut() else {
        return;
    };
    recorder.advance_time(elapsed_ns(started));
    let window = recorder.take_completed_capture();
    if let Some(window) = window {
        let Some((_, source_capture_id, _)) = trigger.take() else {
            return;
        };
        if let Err(error) = record_trigger_termination(capture, source_capture_id, &window, None) {
            (event_sink)(EngineEvent::Error(format!(
                "Could not persist trigger completion evidence: {error}"
            )));
        }
        emit_trigger_capture(
            window,
            source_capture_id,
            source_id,
            bitrate,
            listen_only,
            event_sink,
        );
    }
}

fn record_trigger_termination(
    capture: &mut Option<CanCaptureSession>,
    capture_id: u64,
    window: &crate::CanTriggerCapture,
    reason: Option<CanTriggerEndReason>,
) -> Result<(), String> {
    let Some(session) = capture.as_mut() else {
        return Ok(());
    };
    if window.complete {
        session.record(&CanEvidenceRecord::TriggerCompleted {
            capture_id,
            trigger_elapsed_ns: window.trigger_elapsed_ns,
            max_frames: window.max_frames,
            frame_count: window.frames.len(),
            truncated: window.truncated,
            complete: true,
        })
    } else {
        let reason = reason.ok_or_else(|| {
            "Incomplete CAN trigger window requires an explicit terminal reason".to_owned()
        })?;
        session.record(&CanEvidenceRecord::TriggerEndedEarly {
            capture_id,
            trigger_elapsed_ns: window.trigger_elapsed_ns,
            max_frames: window.max_frames,
            frame_count: window.frames.len(),
            truncated: window.truncated,
            complete: false,
            reason,
        })
    }
}

fn emit_trigger_capture(
    capture: crate::CanTriggerCapture,
    source_capture_id: u64,
    source_id: &str,
    bitrate: u32,
    listen_only: bool,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    (event_sink)(EngineEvent::CanTriggerCapture(CanTriggerCaptureResult {
        source_capture_id,
        source_id: source_id.to_owned(),
        bitrate,
        listen_only,
        capture,
    }));
}

#[allow(clippy::too_many_arguments)]
fn handle_scan_transport_error(
    error: CanTransportError,
    scan_task: &mut Option<ScanTask>,
    transport: &mut Box<dyn CanTransport>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    capture: &mut Option<CanCaptureSession>,
    running: &Arc<AtomicBool>,
    disconnected: &mut bool,
    source_id: String,
    bitrate: u32,
    listen_only: bool,
) {
    report_transport_error(event_sink, capture, error.clone(), None);
    let pending_response = scan_task
        .as_ref()
        .is_some_and(ScanTask::has_pending_response);
    if error.terminates_scan() {
        if let Some(result) = scan_task
            .take()
            .and_then(|task| task.terminal_result(error.clone()))
        {
            scan::emit_result(result, capture, event_sink);
        }
    } else if let Some(mut task) = scan_task.take() {
        if pending_response && let Some(result) = task.terminal_result(error.clone()) {
            scan::emit_result(result, capture, event_sink);
        }
        task.advance_after_send_error();
        *scan_task = Some(task);
    }
    if error.terminates_session() {
        disconnect_transport(
            transport,
            event_sink,
            capture,
            running,
            disconnected,
            source_id,
            bitrate,
            listen_only,
        );
    }
}

fn disconnect_transport(
    transport: &mut Box<dyn CanTransport>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    capture: &mut Option<CanCaptureSession>,
    running: &Arc<AtomicBool>,
    disconnected: &mut bool,
    source_id: String,
    bitrate: u32,
    listen_only: bool,
) {
    (event_sink)(EngineEvent::CanWorkbenchStatus(CanWorkbenchStatus {
        connected: false,
        bitrate,
        listen_only,
        source_id,
    }));
    if let Err(error) = transport.stop() {
        report_transport_error(event_sink, capture, error, None);
    }
    running.store(false, Ordering::Release);
    *disconnected = true;
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
    pending_tx: &mut HashMap<u32, PendingTx>,
    next_tx_sequence: &mut u64,
) -> Result<(CanFrame, u64), CanTransportError> {
    let session = capture
        .as_mut()
        .filter(|session| !session.failed())
        .ok_or_else(|| {
            CanTransportError::other("Active CAN transmission requires an evidence journal")
        })?;
    let capture_id = session.capture_id();
    if frame.elapsed_ns == 0 && frame.wall_time_unix_ns == 0 {
        stamp_tx(&mut frame, started);
    }
    let sequence = *next_tx_sequence;
    *next_tx_sequence = next_tx_sequence.saturating_add(1);
    session
        .record(&CanEvidenceRecord::TxAttempt {
            capture_id,
            sequence,
            frame: frame.clone(),
        })
        .map_err(CanTransportError::other)?;
    let echo_id = match transport.transmit(&frame) {
        Ok(echo_id) => echo_id,
        Err(error) => return Err(error),
    };
    pending_tx.insert(
        echo_id,
        PendingTx {
            capture_id,
            sequence,
            frame: frame.clone(),
        },
    );
    let evidence_result = capture
        .as_mut()
        .ok_or_else(|| CanTransportError::other("CAN capture closed after TX"))?
        .record(&CanEvidenceRecord::TxSubmitted {
            capture_id,
            sequence,
            echo_id,
            frame: frame.clone(),
        })
        .map_err(CanTransportError::other);
    (event_sink)(EngineEvent::CanTxStatus {
        capture_id,
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
    pending_tx: &mut HashMap<u32, PendingTx>,
    capture: &mut Option<CanCaptureSession>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    let Some(pending) = pending_tx.remove(&echo_id) else {
        let error = CanTransportError {
            kind: super::CanTransportErrorKind::Protocol,
            message: format!("Received unassigned Candlelight TX echo {echo_id}"),
            raw_usb: Some(raw_usb),
        };
        report_transport_error(event_sink, capture, error, None);
        return;
    };
    let same_capture = capture
        .as_ref()
        .is_some_and(|session| session.capture_id() == pending.capture_id);
    let record_result = if same_capture {
        capture
            .as_mut()
            .unwrap()
            .record(&CanEvidenceRecord::TxEchoConfirmed {
                capture_id: pending.capture_id,
                sequence: pending.sequence,
                echo_id,
                submitted_frame: pending.frame.clone(),
                echo_frame: echo_frame.clone(),
                raw_usb,
            })
    } else {
        Ok(())
    };
    (event_sink)(EngineEvent::CanTxStatus {
        capture_id: pending.capture_id,
        sequence: pending.sequence,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanScanMode, CanTransportErrorKind, LogNookEngine};
    use std::thread;

    struct GateThenDisconnect {
        entered: Option<mpsc::Sender<()>>,
        release: Receiver<()>,
    }

    impl CanTransport for GateThenDisconnect {
        fn start(
            &mut self,
            _bitrate: u32,
            _listen_only: bool,
            _session_origin: Instant,
        ) -> Result<(), CanTransportError> {
            Ok(())
        }

        fn receive(
            &mut self,
            _timeout: Duration,
        ) -> Result<Option<CanTransportEvent>, CanTransportError> {
            if let Some(entered) = self.entered.take() {
                let _ = entered.send(());
                let _ = self.release.recv();
                return Err(CanTransportError {
                    kind: CanTransportErrorKind::Disconnected,
                    message: "device removed".into(),
                    raw_usb: None,
                });
            }
            Ok(None)
        }

        fn transmit(&mut self, _frame: &CanFrame) -> Result<u32, CanTransportError> {
            panic!("terminal fake must not transmit")
        }

        fn stop(&mut self) -> Result<(), CanTransportError> {
            Ok(())
        }
    }

    fn temp_capture_path() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lognook-worker-test-{stamp}.candump.csv"))
    }

    #[test]
    fn failed_capture_session_does_not_finalize_as_successful() {
        let path = temp_capture_path();
        let mut capture =
            CanCaptureSession::create_new(&path, 1, "can:test", 500_000, false).unwrap();
        capture.failed = true;
        assert!(capture.finish().is_err());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(super::super::can_evidence_path(&path));
    }

    #[test]
    fn successful_capture_finish_appends_a_clean_close_record() {
        let path = temp_capture_path();
        let capture = CanCaptureSession::create_new(&path, 7, "can:test", 500_000, false).unwrap();
        capture.finish().unwrap();
        let journal = fs::read_to_string(super::super::can_evidence_path(&path)).unwrap();
        let records: Vec<CanEvidenceRecord> = journal
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            records.iter().any(|record| matches!(
                record,
                CanEvidenceRecord::SessionFinished { capture_id: 7 }
            ))
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(super::super::can_evidence_path(&path));
    }

    #[test]
    fn journal_refuses_to_claim_success_after_a_prior_write_failure() {
        let path = temp_capture_path();
        let mut capture =
            CanCaptureSession::create_new(&path, 1, "can:test", 500_000, false).unwrap();
        capture.failed = true;
        let result = capture.record(&CanEvidenceRecord::SessionStarted {
            format_version: 1,
            capture_id: 1,
            bitrate: 500_000,
            listen_only: false,
            source_id: "can:test".into(),
        });
        assert!(result.is_err());
        drop(capture);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(super::super::can_evidence_path(&path));
    }

    #[test]
    fn stop_scan_propagates_a_failed_journal_write() {
        let path = temp_capture_path();
        let capture = CanCaptureSession::create_new(&path, 1, "can:test", 500_000, false).unwrap();
        let mut capture = Some(capture);
        capture.as_mut().unwrap().failed = true;
        let mut scan_task = Some(ScanTask::new(CanScanConfig {
            mode: CanScanMode::Mode21 {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                start_pid: 1,
                end_pid: 1,
            },
            pacing_ms: 100,
            response_timeout_ms: 1000,
            source_candidate_id: None,
        }));
        let mut fields = Vec::new();
        let mut send_task = None;
        let mut next_capture_id = 2;
        let mut trigger = None;
        let started = Instant::now();
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new(|_| {});
        let (reply, response) = mpsc::channel();
        handle_command(
            Command::StopScan(reply),
            &mut fields,
            &mut capture,
            &mut send_task,
            &mut scan_task,
            &event_sink,
            "can:test",
            500_000,
            false,
            &mut next_capture_id,
            &mut trigger,
            &started,
        );
        assert!(response.recv().unwrap().is_err());
        assert!(scan_task.is_none());
        assert!(capture.take().unwrap().finish().is_err());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(super::super::can_evidence_path(&path));
    }

    #[test]
    fn terminal_worker_drops_queued_capture_command_without_acknowledging_success() {
        let path = temp_capture_path();
        let engine = LogNookEngine::new(|_| {});
        let publisher = engine.sample_publisher();
        let (commands, receiver) = mpsc::sync_channel(super::super::COMMAND_CAPACITY);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new(|_| {});
        let worker = thread::spawn(move || {
            run_worker(
                Box::new(GateThenDisconnect {
                    entered: Some(entered_tx),
                    release: release_rx,
                }),
                receiver,
                publisher,
                event_sink,
                worker_running,
                "can:test".into(),
                500_000,
                false,
                Instant::now(),
            );
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (reply, response) = mpsc::channel();
        commands
            .send(Command::StartCapture(path.clone(), reply))
            .unwrap();
        release_tx.send(()).unwrap();
        assert!(response.recv_timeout(Duration::from_secs(1)).is_err());
        worker.join().unwrap();
        assert!(!path.exists());
        assert!(!super::super::can_evidence_path(&path).exists());
        drop(engine);
    }
}
