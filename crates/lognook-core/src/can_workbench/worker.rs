use super::scan::{self, ScanTask, accept_scan_response, task_finished, tick_scan};
use super::{
    CAN_ID_REPLAY_MAX_INTERVAL_MS, CAN_ID_REPLAY_MIN_INTERVAL_MS, CanEvidenceRecord,
    CanIdReplayState, CanIdReplayStatus, CanManualEventMark, CanReplayFrame, CanReplayPlan,
    CanReplaySource, CanScanConfig, CanScanStopReason, CanSignalDefinition, CanTransport,
    CanTransportError, CanTransportEvent, CanTriggerCaptureResult, CanTriggerConfig,
    CanTriggerEndReason, CanTriggerRecorder, CanWorkbenchStatus, OemCandidateTrySpec,
};
use crate::{
    CanCaptureWriter, CanDirection, CanFrame, EngineEvent, LiveCanEventAnalyzer, SamplePublisher,
    decode_can_signal,
};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    sync::{
        Arc, Mutex,
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
    StopCapture(CanScanStopReason, mpsc::Sender<Result<(), String>>),
    SendFrame(CanFrame, u64, u32, mpsc::Sender<Result<(), String>>),
    StartCanIdReplay(
        u64,
        CanReplayPlan,
        Option<u8>,
        mpsc::Sender<Result<CanIdReplayStatus, String>>,
    ),
    StopCanIdReplay(mpsc::Sender<Result<(), String>>),
    StartScan(
        CanScanConfig,
        Option<OemCandidateTrySpec>,
        mpsc::Sender<Result<(), String>>,
    ),
    StopScan(CanScanStopReason, mpsc::Sender<Result<(), String>>),
    ArmTrigger(CanTriggerConfig, mpsc::Sender<Result<(), String>>),
    TriggerNow(mpsc::Sender<Result<(), String>>),
    MarkEvent {
        label: String,
        pre_ns: i64,
        post_ns: i64,
        reply: mpsc::Sender<Result<CanManualEventMark, String>>,
    },
    ClearEventResults(mpsc::Sender<Result<(), String>>),
    GetActiveOperations(mpsc::Sender<Result<bool, String>>),
    DisarmTrigger(mpsc::Sender<Result<(), String>>),
    Shutdown,
}

struct SendTask {
    frame: CanFrame,
    pacing: Duration,
    remaining: u32,
    next_at: Instant,
}

struct CanIdReplayTask {
    status: CanIdReplayStatus,
    source_frames: VecDeque<CanReplayFrame>,
    next_at: Instant,
}

pub(super) struct CanSubmitError {
    pub(super) error: CanTransportError,
    pub(super) submitted: bool,
}

impl CanSubmitError {
    fn before_submission(error: CanTransportError) -> Self {
        Self {
            error,
            submitted: false,
        }
    }

    fn after_submission(error: CanTransportError) -> Self {
        Self {
            error,
            submitted: true,
        }
    }
}

pub(super) struct PendingTx {
    capture_id: u64,
    sequence: u64,
    frame: CanFrame,
}

pub(super) struct CanCaptureSession<W: Write = File> {
    capture_id: u64,
    frames: Option<CanCaptureWriter<File>>,
    journal: BufWriter<W>,
    failed: bool,
}

impl CanCaptureSession<File> {
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
}

impl<W: Write> CanCaptureSession<W> {
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
}

impl CanCaptureSession<File> {
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
    active_capture_path: Arc<Mutex<Option<std::path::PathBuf>>>,
    active_capture_id: Arc<std::sync::atomic::AtomicU64>,
) {
    let mut fields = Vec::<CanSignalDefinition>::new();
    let mut capture: Option<CanCaptureSession> = None;
    let mut frames = Vec::with_capacity(EVENT_BATCH_MAX);
    let mut last_flush = Instant::now();
    let mut send_task: Option<SendTask> = None;
    let mut replay_task: Option<CanIdReplayTask> = None;
    let mut scan_task: Option<ScanTask> = None;
    let mut pending_tx = HashMap::<u32, PendingTx>::new();
    let mut next_tx_sequence = 1u64;
    let mut next_capture_id = 1u64;
    let mut trigger: Option<(CanTriggerRecorder, u64, CanTriggerConfig)> = None;
    let mut live_events: Option<LiveCanEventAnalyzer> = None;
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
                &active_capture_path,
                &active_capture_id,
                &mut send_task,
                &mut replay_task,
                &mut scan_task,
                &event_sink,
                &source_id,
                bitrate,
                listen_only,
                &mut next_capture_id,
                &mut trigger,
                &mut live_events,
                &started,
            ),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if capture.is_none() {
            clear_active_capture_path(&active_capture_path);
            active_capture_id.store(0, Ordering::Release);
        }
        if capture.as_ref().is_none_or(CanCaptureSession::failed) {
            send_task = None;
            scan_task = None;
            if let Some(task) = replay_task.take() {
                let _ = finish_can_id_replay(
                    task,
                    CanIdReplayState::Failed,
                    "evidence_journal_unavailable",
                    Some("CAN replay stopped because its evidence journal is unavailable".into()),
                    &mut capture,
                    &event_sink,
                );
            }
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
                    None,
                ) {
                    Ok(_) => {
                        task.remaining -= 1;
                        task.next_at = Instant::now() + task.pacing;
                        if task.remaining == 0 {
                            send_task = None;
                        }
                    }
                    Err(error) => {
                        let error = error.error;
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
        if let Some(error) = tick_can_id_replay(
            &mut replay_task,
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
            report_transport_error(&event_sink, &mut capture, error.clone(), None);
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
                    let live_event_error =
                        if capture.as_ref().is_some_and(|session| !session.failed()) {
                            live_events
                                .as_mut()
                                .and_then(|analyzer| analyzer.feed(&frame).err())
                        } else {
                            None
                        };
                    if let Some(error) = live_event_error {
                        (event_sink)(EngineEvent::Error(format!(
                            "Live CAN event analysis stopped: {error}"
                        )));
                        if let Some(mut analyzer) = live_events.take() {
                            (event_sink)(EngineEvent::CanLiveEventAnalysis(analyzer.finish()));
                        }
                    } else if capture.as_ref().is_none_or(|session| session.failed())
                        && let Some(mut analyzer) = live_events.take()
                    {
                        (event_sink)(EngineEvent::CanLiveEventAnalysis(analyzer.finish()));
                    }
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
                    if error.terminates_session()
                        && let Some(task) = replay_task.take()
                        && let Err(finish_error) = finish_can_id_replay(
                            task,
                            CanIdReplayState::Failed,
                            "transport_error",
                            Some(error.message.clone()),
                            &mut capture,
                            &event_sink,
                        )
                    {
                        (event_sink)(EngineEvent::Error(format!(
                            "Could not persist CAN replay failure evidence: {finish_error}"
                        )));
                    }
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
        let live_event_advance = live_events
            .as_mut()
            .map(|analyzer| analyzer.advance(elapsed_ns(&started)));
        if let Some(result) = live_event_advance {
            match result {
                Ok(true) => {
                    if let Some(analyzer) = live_events.as_ref() {
                        (event_sink)(EngineEvent::CanLiveEventAnalysis(analyzer.snapshot()));
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    (event_sink)(EngineEvent::Error(format!(
                        "Live CAN event analysis stopped: {error}"
                    )));
                    if let Some(mut analyzer) = live_events.take() {
                        (event_sink)(EngineEvent::CanLiveEventAnalysis(analyzer.finish()));
                    }
                }
            }
        }
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
    if let Some(task) = replay_task.take() {
        let _ = finish_can_id_replay(
            task,
            CanIdReplayState::Stopped,
            "session_ended",
            None,
            &mut capture,
            &event_sink,
        );
    }
    if let Some(live_events) = live_events.as_mut() {
        (event_sink)(EngineEvent::CanLiveEventAnalysis(live_events.finish()));
    }
    if let Some(writer) = capture.take() {
        if let Err(error) = writer.finish() {
            (event_sink)(EngineEvent::Error(format!(
                "CAN capture finalization failed: {error}"
            )));
        }
    }
    clear_active_capture_path(&active_capture_path);
    active_capture_id.store(0, Ordering::Release);
    if running.swap(false, Ordering::AcqRel) {
        if let Err(error) = transport.stop() {
            (event_sink)(EngineEvent::CanTransportError(error.clone()));
            (event_sink)(EngineEvent::Error(error.to_string()));
        }
    }
    if !frames.is_empty() {
        (event_sink)(EngineEvent::CanFrames(frames));
    }
    let _ = publisher.source_disconnected(false, disconnected);
}

fn handle_command(
    command: Command,
    fields: &mut Vec<CanSignalDefinition>,
    capture: &mut Option<CanCaptureSession>,
    active_capture_path: &Arc<Mutex<Option<std::path::PathBuf>>>,
    active_capture_id: &Arc<std::sync::atomic::AtomicU64>,
    send_task: &mut Option<SendTask>,
    replay_task: &mut Option<CanIdReplayTask>,
    scan_task: &mut Option<ScanTask>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    source_id: &str,
    bitrate: u32,
    listen_only: bool,
    next_capture_id: &mut u64,
    trigger: &mut Option<(CanTriggerRecorder, u64, CanTriggerConfig)>,
    live_events: &mut Option<LiveCanEventAnalyzer>,
    started: &Instant,
) {
    match command {
        Command::SetFields(next, reply) => {
            *fields = next;
            let _ = reply.send(Ok(()));
        }
        Command::StartCapture(path, reply) => {
            let mut capture_id = None;
            let mut result = if capture.is_some() {
                Err("A raw CAN capture is already active".to_owned())
            } else {
                let next_id = *next_capture_id;
                CanCaptureSession::create_new(&path, next_id, source_id, bitrate, listen_only).map(
                    |session| {
                        *capture = Some(session);
                        *next_capture_id = next_capture_id.saturating_add(1);
                        capture_id = Some(next_id);
                        *live_events = Some(LiveCanEventAnalyzer::new(next_id, source_id));
                    },
                )
            };
            if result.is_ok() {
                let canonical_path = path.canonicalize().unwrap_or(path);
                match active_capture_path.lock() {
                    Ok(mut active_path) => {
                        *active_path = Some(canonical_path);
                        if let Some(capture_id) = capture_id {
                            active_capture_id.store(capture_id, Ordering::Release);
                        }
                    }
                    Err(_) => {
                        if let Some(session) = capture.take() {
                            let _ = session.finish();
                        }
                        result = Err("CAN capture path state is unavailable".into());
                    }
                }
            }
            if result.is_ok()
                && let Some(live_events) = live_events.as_ref()
            {
                (event_sink)(EngineEvent::CanLiveEventAnalysis(live_events.snapshot()));
            }
            let _ = reply.send(result);
        }
        Command::StopCapture(reason, reply) => {
            let stop_result = if let Some(task) = scan_task.take() {
                let result = capture
                    .as_mut()
                    .ok_or_else(|| "CAN scan stopped without an active evidence journal".to_owned())
                    .and_then(|capture| {
                        capture.record(&CanEvidenceRecord::ScanStopped {
                            config: task.config().into(),
                            reason: reason.clone(),
                        })
                    });
                (event_sink)(EngineEvent::CanScanStopped);
                result
            } else {
                Ok(())
            };
            *send_task = None;
            let replay_result = if let Some(task) = replay_task.take() {
                finish_can_id_replay(
                    task,
                    CanIdReplayState::Stopped,
                    "capture_stopped",
                    None,
                    capture,
                    event_sink,
                )
                .map(|_| ())
            } else {
                Ok(())
            };
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
            clear_active_capture_path(active_capture_path);
            active_capture_id.store(0, Ordering::Release);
            if let Some(mut live_events) = live_events.take() {
                (event_sink)(EngineEvent::CanLiveEventAnalysis(live_events.finish()));
            }
            let _ = reply.send(
                stop_result
                    .and(replay_result)
                    .and(trigger_result)
                    .and(finish_result),
            );
        }
        Command::SendFrame(frame, pacing_ms, repeat, reply) => {
            let result = if capture.as_ref().is_none_or(CanCaptureSession::failed) {
                Err(
                    "Start a raw CAN capture before sending; TX requires a paired evidence journal"
                        .to_owned(),
                )
            } else if transport_mode_conflicts(
                send_task.is_some(),
                scan_task.is_some(),
                replay_task.is_some(),
            ) {
                Err("A CAN send, scan, or replay is already active".to_owned())
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
        Command::StartCanIdReplay(replay_id, plan, target_channel, reply) => {
            let result = if capture.as_ref().is_none_or(CanCaptureSession::failed) {
                Err("Start a raw CAN capture before replaying; TX requires a paired evidence journal".into())
            } else if transport_mode_conflicts(
                send_task.is_some(),
                scan_task.is_some(),
                replay_task.is_some(),
            ) {
                Err("A CAN send, scan, or replay is already active".into())
            } else {
                let first = &plan.frames()[0].frame;
                let status = CanIdReplayStatus {
                    replay_id,
                    can_id: first.can_id,
                    extended: first.extended,
                    total_frames: plan.frames().len(),
                    sent_frames: 0,
                    state: CanIdReplayState::Running,
                    error: None,
                };
                let session = capture.as_mut().unwrap();
                let journal_result = record_can_replay_source(
                    session,
                    &plan,
                    replay_id,
                    source_id,
                    bitrate,
                    target_channel,
                );
                match journal_result {
                    Ok(()) => {
                        *replay_task = Some(CanIdReplayTask {
                            status: status.clone(),
                            source_frames: plan.frames().to_vec().into(),
                            next_at: Instant::now(),
                        });
                        (event_sink)(EngineEvent::CanIdReplayStatus(status.clone()));
                        Ok(status)
                    }
                    Err(error) => Err(error),
                }
            };
            let _ = reply.send(result);
        }
        Command::StopCanIdReplay(reply) => {
            let result = if let Some(task) = replay_task.take() {
                finish_can_id_replay(
                    task,
                    CanIdReplayState::Stopped,
                    "user_requested",
                    None,
                    capture,
                    event_sink,
                )
                .map(|_| ())
            } else {
                Ok(())
            };
            let _ = reply.send(result);
        }
        Command::StartScan(config, candidate, reply) => {
            let result = if capture.as_ref().is_none_or(CanCaptureSession::failed) {
                Err("Start a raw CAN capture before scanning; scans require a paired evidence journal".to_owned())
            } else if transport_mode_conflicts(
                send_task.is_some(),
                scan_task.is_some(),
                replay_task.is_some(),
            ) {
                Err("A CAN send, scan, or replay is already active".to_owned())
            } else if let Some(capture) = capture.as_mut() {
                match capture.record(&CanEvidenceRecord::ScanStarted {
                    config: (&config).into(),
                }) {
                    Ok(()) => {
                        *scan_task = Some(ScanTask::new(config.clone(), candidate));
                        (event_sink)(EngineEvent::CanScanStarted(config));
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else if transport_mode_conflicts(
                send_task.is_some(),
                scan_task.is_some(),
                replay_task.is_some(),
            ) {
                Err("A CAN send, scan, or replay is already active".to_owned())
            } else {
                Err("Start a raw CAN capture before scanning".to_owned())
            };
            let _ = reply.send(result);
        }
        Command::StopScan(reason, reply) => {
            let result = if let Some(task) = scan_task.take() {
                let result = capture
                    .as_mut()
                    .ok_or_else(|| "CAN scan stopped without an active evidence journal".to_owned())
                    .and_then(|capture| {
                        capture.record(&CanEvidenceRecord::ScanStopped {
                            config: task.config().into(),
                            reason,
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
                live_events
                    .as_ref()
                    .ok_or_else(|| "Live CAN event analyzer is unavailable".to_owned())
                    .and_then(|analyzer| {
                        analyzer
                            .validate_mark(&mark)
                            .map_err(|error| error.to_string())
                    })
                    .and_then(|()| {
                        session
                            .record(&CanEvidenceRecord::ManualEventMarked {
                                capture_id: session.capture_id(),
                                mark: mark.clone(),
                            })
                            .map_err(|error| error.to_owned())
                    })
                    .and_then(|()| {
                        live_events
                            .as_mut()
                            .ok_or_else(|| "Live CAN event analyzer is unavailable".to_owned())?
                            .mark(mark.clone())
                            .map_err(|error| error.to_string())
                    })
                    .map(|()| mark)
            } else {
                Err("Start a raw CAN capture before marking an event".to_owned())
            };
            if result.is_ok()
                && let Some(live_events) = live_events.as_ref()
            {
                (event_sink)(EngineEvent::CanLiveEventAnalysis(live_events.snapshot()));
            }
            let _ = reply.send(result);
        }
        Command::ClearEventResults(reply) => {
            let result = live_events
                .as_mut()
                .ok_or_else(|| {
                    "Start a raw CAN capture before clearing live event results".to_owned()
                })
                .map(|analyzer| {
                    analyzer.clear_results();
                    (event_sink)(EngineEvent::CanLiveEventAnalysis(analyzer.snapshot()));
                });
            let _ = reply.send(result);
        }
        Command::GetActiveOperations(reply) => {
            let active = capture.is_some()
                || send_task.is_some()
                || replay_task.is_some()
                || scan_task.is_some()
                || trigger.is_some();
            let _ = reply.send(Ok(active));
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

fn clear_active_capture_path(path: &Arc<Mutex<Option<std::path::PathBuf>>>) {
    if let Ok(mut path) = path.lock() {
        *path = None;
    }
}

fn record_can_replay_source<W: Write>(
    session: &mut CanCaptureSession<W>,
    plan: &CanReplayPlan,
    replay_id: u64,
    target_source_id: &str,
    target_bitrate: u32,
    target_channel: Option<u8>,
) -> Result<(), String> {
    let capture_id = session.capture_id();
    match plan.source() {
        CanReplaySource::LiveSnapshot => session.record(&CanEvidenceRecord::CanIdReplayStarted {
            format_version: 1,
            capture_id,
            replay_id,
            can_id: plan.frames()[0].frame.can_id,
            extended: plan.frames()[0].frame.extended,
            source_frames: plan
                .frames()
                .iter()
                .map(|item| item.frame.clone())
                .collect(),
            minimum_interval_ms: CAN_ID_REPLAY_MIN_INTERVAL_MS,
            maximum_interval_ms: CAN_ID_REPLAY_MAX_INTERVAL_MS,
            no_catch_up: true,
        }),
        CanReplaySource::SavedCapture { summary } => {
            session.record(&CanEvidenceRecord::SavedCanIdReplayStarted {
                format_version: 1,
                capture_id,
                replay_id,
                summary: summary.clone(),
                target_source_id: target_source_id.to_owned(),
                target_channel,
                target_bitrate,
                minimum_interval_ms: CAN_ID_REPLAY_MIN_INTERVAL_MS,
                maximum_interval_ms: CAN_ID_REPLAY_MAX_INTERVAL_MS,
                no_catch_up: true,
            })?;
            for (chunk_index, frames) in plan.frames().chunks(400).enumerate() {
                session.record(&CanEvidenceRecord::SavedCanIdReplaySourceFrames {
                    capture_id,
                    replay_id,
                    chunk_index,
                    frames: frames.to_vec(),
                })?;
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn tick_can_id_replay<W: Write>(
    replay_task: &mut Option<CanIdReplayTask>,
    transport: &mut Box<dyn CanTransport>,
    capture: &mut Option<CanCaptureSession<W>>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
    source_id: &str,
    started: &Instant,
    pending_tx: &mut HashMap<u32, PendingTx>,
    next_tx_sequence: &mut u64,
) -> Option<CanTransportError> {
    if !replay_task
        .as_ref()
        .is_some_and(|task| Instant::now() >= task.next_at)
    {
        return None;
    }
    let mut task = replay_task.take()?;
    let source_frame = task.source_frames.pop_front()?.frame;
    let source_elapsed_ns = source_frame.elapsed_ns;
    let mut tx_frame = source_frame;
    tx_frame.elapsed_ns = 0;
    tx_frame.wall_time_unix_ns = 0;
    tx_frame.direction = CanDirection::Tx;
    let replay_id = task.status.replay_id;
    let submission_started = Instant::now();
    match submit_frame(
        tx_frame,
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
        Some((replay_id, source_elapsed_ns)),
    ) {
        Ok(_) => {
            task.status.sent_frames += 1;
            if let Some(next_source_frame) = task.source_frames.front() {
                let source_interval_ns = next_source_frame.frame.elapsed_ns - source_elapsed_ns;
                let effective_interval_ns = source_interval_ns
                    .max((CAN_ID_REPLAY_MIN_INTERVAL_MS as i64) * 1_000_000)
                    as u64;
                let scheduled_at = submission_started + Duration::from_nanos(effective_interval_ns);
                let no_catch_up_at =
                    Instant::now() + Duration::from_millis(CAN_ID_REPLAY_MIN_INTERVAL_MS);
                task.next_at = scheduled_at.max(no_catch_up_at);
                (event_sink)(EngineEvent::CanIdReplayStatus(task.status.clone()));
                *replay_task = Some(task);
            } else if let Err(error) = finish_can_id_replay(
                task,
                CanIdReplayState::Completed,
                "completed",
                None,
                capture,
                event_sink,
            ) {
                return Some(CanTransportError::other(error));
            }
        }
        Err(submit_error) => {
            task.status.sent_frames += usize::from(submit_error.submitted);
            let reason = if submit_error.submitted {
                "post_submit_evidence_error"
            } else {
                "transmit_error"
            };
            let error = submit_error.error;
            if let Err(finish_error) = finish_can_id_replay(
                task,
                CanIdReplayState::Failed,
                reason,
                Some(error.message.clone()),
                capture,
                event_sink,
            ) {
                (event_sink)(EngineEvent::Error(format!(
                    "Could not persist CAN replay failure evidence: {finish_error}"
                )));
            }
            return Some(error);
        }
    }
    None
}

fn finish_can_id_replay<W: Write>(
    mut task: CanIdReplayTask,
    state: CanIdReplayState,
    reason: &str,
    error: Option<String>,
    capture: &mut Option<CanCaptureSession<W>>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) -> Result<CanIdReplayStatus, String> {
    let mut state = state;
    let mut error = error;
    let record_result = if let Some(session) = capture.as_mut().filter(|session| !session.failed())
    {
        session.record(&CanEvidenceRecord::CanIdReplayFinished {
            capture_id: session.capture_id(),
            replay_id: task.status.replay_id,
            total_frames: task.status.total_frames,
            sent_frames: task.status.sent_frames,
            state,
            reason: reason.to_owned(),
            error: error.clone(),
        })
    } else {
        Err("CAN evidence journal is unavailable".into())
    };
    let record_error = record_result.err();
    if let Some(message) = &record_error {
        state = CanIdReplayState::Failed;
        error = Some(match error {
            Some(reason) => format!("{reason}; terminal evidence write failed: {message}"),
            None => format!("Could not persist CAN replay terminal evidence: {message}"),
        });
    }
    task.status.state = state;
    task.status.error = error;
    (event_sink)(EngineEvent::CanIdReplayStatus(task.status.clone()));
    match record_error {
        Some(message) => Err(message),
        None => Ok(task.status),
    }
}

fn transport_mode_conflicts(send: bool, scan: bool, replay: bool) -> bool {
    send || scan || replay
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
        target_channel: None,
    }));
    if let Err(error) = transport.stop() {
        report_transport_error(event_sink, capture, error, None);
    }
    running.store(false, Ordering::Release);
    *disconnected = true;
}

pub(super) fn accept_frame<W: Write>(
    frame: CanFrame,
    source_id: &str,
    capture: &mut Option<CanCaptureSession<W>>,
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
pub(super) fn submit_frame<W: Write>(
    mut frame: CanFrame,
    transport: &mut Box<dyn CanTransport>,
    capture: &mut Option<CanCaptureSession<W>>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
    source_id: &str,
    started: &Instant,
    pending_tx: &mut HashMap<u32, PendingTx>,
    next_tx_sequence: &mut u64,
    replay_source: Option<(u64, i64)>,
) -> Result<(CanFrame, u64), CanSubmitError> {
    let session = capture
        .as_mut()
        .filter(|session| !session.failed())
        .ok_or_else(|| {
            CanSubmitError::before_submission(CanTransportError::other(
                "Active CAN transmission requires an evidence journal",
            ))
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
            replay_id: replay_source.map(|(replay_id, _)| replay_id),
            source_elapsed_ns: replay_source.map(|(_, elapsed_ns)| elapsed_ns),
        })
        .map_err(CanTransportError::other)
        .map_err(CanSubmitError::before_submission)?;
    let echo_id = match transport.transmit(&frame) {
        Ok(echo_id) => echo_id,
        Err(error) => return Err(CanSubmitError::before_submission(error)),
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
        .ok_or_else(|| CanTransportError::other("CAN capture closed after TX"))
        .and_then(|session| {
            session
                .record(&CanEvidenceRecord::TxSubmitted {
                    capture_id,
                    sequence,
                    echo_id,
                    frame: frame.clone(),
                })
                .map_err(CanTransportError::other)
        })
        .map_err(CanSubmitError::after_submission);
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
    use crate::{
        CanCaptureWriter, CanScanMode, CanTransportErrorKind, LogNookEngine,
        SavedCanReplaySelection, prepare_saved_can_replay,
    };
    use std::io;
    use std::sync::Mutex;
    use std::thread;

    #[derive(Default)]
    struct FailSecondFlush {
        flushes: usize,
    }

    impl Write for FailSecondFlush {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.flushes == 2 {
                Err(io::Error::other("injected TxSubmitted journal failure"))
            } else {
                Ok(())
            }
        }
    }

    struct TransmitOnce {
        sent: mpsc::Sender<CanFrame>,
    }

    impl CanTransport for TransmitOnce {
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
            Ok(None)
        }

        fn transmit(&mut self, frame: &CanFrame) -> Result<u32, CanTransportError> {
            self.sent
                .send(frame.clone())
                .map_err(|error| CanTransportError::other(error.to_string()))?;
            Ok(1)
        }

        fn stop(&mut self) -> Result<(), CanTransportError> {
            Ok(())
        }
    }

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
    fn replay_counts_a_successful_transport_submission_when_tx_evidence_fails() {
        let engine = LogNookEngine::new(|_| {});
        let publisher = engine.sample_publisher();
        let path = temp_capture_path();
        let raw_capture = CanCaptureWriter::create_new(&path).unwrap();
        let mut capture = Some(CanCaptureSession::<FailSecondFlush> {
            capture_id: 7,
            frames: Some(raw_capture),
            journal: BufWriter::new(FailSecondFlush::default()),
            failed: false,
        });
        let source_frames = VecDeque::from([
            CanFrame {
                elapsed_ns: 100_000_000,
                wall_time_unix_ns: 1_700_000_000_100_000_000,
                can_id: 0x321,
                extended: false,
                direction: CanDirection::Rx,
                data: vec![0x10],
                flags: 0,
            },
            CanFrame {
                elapsed_ns: 200_000_000,
                wall_time_unix_ns: 1_700_000_000_200_000_000,
                can_id: 0x321,
                extended: false,
                direction: CanDirection::Rx,
                data: vec![0x20],
                flags: 0,
            },
        ]);
        let total_frames = source_frames.len();
        let source_frames = source_frames
            .into_iter()
            .map(|frame| CanReplayFrame {
                source_row: None,
                frame,
            })
            .collect();
        let mut replay_task = Some(CanIdReplayTask {
            status: CanIdReplayStatus {
                replay_id: 3,
                can_id: 0x321,
                extended: false,
                total_frames,
                sent_frames: 0,
                state: CanIdReplayState::Running,
                error: None,
            },
            source_frames,
            next_at: Instant::now(),
        });
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_log = events.clone();
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> =
            Arc::new(move |event| event_log.lock().unwrap().push(event));
        let (sent, transmitted) = mpsc::channel();
        let mut transport: Box<dyn CanTransport> = Box::new(TransmitOnce { sent });
        let mut batch = Vec::new();
        let mut pending_tx = HashMap::new();
        let mut next_tx_sequence = 1;
        let started = Instant::now();

        let error = tick_can_id_replay(
            &mut replay_task,
            &mut transport,
            &mut capture,
            &mut [],
            &publisher,
            &event_sink,
            &mut batch,
            "can:test",
            &started,
            &mut pending_tx,
            &mut next_tx_sequence,
        )
        .unwrap();

        assert!(
            error
                .message
                .contains("injected TxSubmitted journal failure")
        );
        assert_eq!(transmitted.recv().unwrap().data, [0x10]);
        assert!(
            transmitted.try_recv().is_err(),
            "the evidence failure cancels remaining frames"
        );
        assert!(replay_task.is_none());
        assert!(events.lock().unwrap().iter().any(|event| matches!(event,
            EngineEvent::CanIdReplayStatus(status)
                if status.replay_id == 3
                    && status.state == CanIdReplayState::Failed
                    && status.sent_frames == 1
        )));
        drop(capture);
        let _ = fs::remove_file(&path);
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
    fn saved_replay_source_evidence_failure_prevents_any_tx() {
        let engine = LogNookEngine::new(|_| {});
        let publisher = engine.sample_publisher();
        let source_path = temp_capture_path();
        let mut source_writer = CanCaptureWriter::create_new(&source_path).unwrap();
        source_writer
            .write_frame(&CanFrame {
                elapsed_ns: 0,
                wall_time_unix_ns: 1_700_000_000_000_000_000,
                can_id: 0x321,
                extended: false,
                direction: CanDirection::Rx,
                data: vec![0x55],
                flags: 0,
            })
            .unwrap();
        source_writer.finish().unwrap();
        let prepared = prepare_saved_can_replay(
            &source_path,
            SavedCanReplaySelection {
                can_id: 0x321,
                extended: false,
                start_elapsed_ns: 0,
                end_elapsed_ns: 0,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let output_path = temp_capture_path();
        let raw_capture = CanCaptureWriter::create_new(&output_path).unwrap();
        let mut capture = Some(CanCaptureSession::<FailSecondFlush> {
            capture_id: 7,
            frames: Some(raw_capture),
            journal: BufWriter::new(FailSecondFlush::default()),
            failed: false,
        });
        let mut fields = Vec::new();
        let mut replay_task = None;
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_log = events.clone();
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> =
            Arc::new(move |event| event_log.lock().unwrap().push(event));
        let started = Instant::now();
        let error = record_can_replay_source(
            capture.as_mut().unwrap(),
            &prepared.plan,
            4,
            "can:test-adapter",
            500_000,
            Some(2),
        )
        .unwrap_err();
        assert!(error.contains("injected TxSubmitted journal failure"));
        assert!(replay_task.is_none());

        let (sent, transmitted) = mpsc::channel();
        let mut transport: Box<dyn CanTransport> = Box::new(TransmitOnce { sent });
        let mut batch = Vec::new();
        let mut pending_tx = HashMap::new();
        let mut next_tx_sequence = 1;
        assert!(
            tick_can_id_replay(
                &mut replay_task,
                &mut transport,
                &mut capture,
                &mut fields,
                &publisher,
                &event_sink,
                &mut batch,
                "can:test-adapter",
                &started,
                &mut pending_tx,
                &mut next_tx_sequence,
            )
            .is_none()
        );
        assert!(transmitted.try_recv().is_err());
        drop(capture);
        let _ = fs::remove_file(source_path);
        let _ = fs::remove_file(output_path);
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
        let mut scan_task = Some(ScanTask::new(
            CanScanConfig {
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
            },
            None,
        ));
        let mut fields = Vec::new();
        let mut send_task = None;
        let mut replay_task = None;
        let mut next_capture_id = 2;
        let mut trigger = None;
        let mut live_events = None;
        let started = Instant::now();
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new(|_| {});
        let (reply, response) = mpsc::channel();
        let active_capture_path = Arc::new(Mutex::new(None));
        let active_capture_id = Arc::new(std::sync::atomic::AtomicU64::new(0));
        handle_command(
            Command::StopScan(CanScanStopReason::UserRequested, reply),
            &mut fields,
            &mut capture,
            &active_capture_path,
            &active_capture_id,
            &mut send_task,
            &mut replay_task,
            &mut scan_task,
            &event_sink,
            "can:test",
            500_000,
            false,
            &mut next_capture_id,
            &mut trigger,
            &mut live_events,
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
                Arc::new(Mutex::new(None)),
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
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
