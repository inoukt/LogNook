use super::scan::{ScanTask, accept_scan_response, task_finished, tick_scan};
use super::{CanScanConfig, CanSignalDefinition, CanTransport, CanWorkbenchStatus};
use crate::{
    CanCaptureWriter, CanDirection, CanFrame, EngineEvent, SamplePublisher, decode_can_signal,
};
use std::{
    fs::File,
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
pub(super) fn run_worker(
    mut transport: Box<dyn CanTransport>,
    commands: Receiver<Command>,
    publisher: SamplePublisher,
    event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    running: Arc<AtomicBool>,
    source_id: String,
    bitrate: u32,
    listen_only: bool,
) {
    let started = Instant::now();
    let mut fields = Vec::<CanSignalDefinition>::new();
    let mut capture: Option<CanCaptureWriter<File>> = None;
    let mut frames = Vec::with_capacity(EVENT_BATCH_MAX);
    let mut last_flush = Instant::now();
    let mut send_task: Option<SendTask> = None;
    let mut scan_task: Option<ScanTask> = None;
    let mut disconnected = false;
    loop {
        match commands.try_recv() {
            Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => break,
            Ok(command) => handle_command(
                command,
                &mut transport,
                &mut fields,
                &mut capture,
                &mut send_task,
                &mut scan_task,
                &event_sink,
            ),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if disconnected {
            break;
        }
        if let Some(task) = send_task.as_mut() {
            if Instant::now() >= task.next_at {
                let mut frame = task.frame.clone();
                stamp_tx(&mut frame, &started);
                match transport.transmit(&frame) {
                    Ok(()) => {
                        task.remaining -= 1;
                        accept_frame(
                            frame,
                            &source_id,
                            &mut capture,
                            &mut fields,
                            &publisher,
                            &event_sink,
                            &mut frames,
                        );
                        task.next_at = Instant::now() + task.pacing;
                        if task.remaining == 0 {
                            send_task = None;
                        }
                    }
                    Err(error) => {
                        let _ = (event_sink)(EngineEvent::Error(error));
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
            );
            if task_finished(task) {
                scan_task = None;
            }
        }
        if running.load(Ordering::Acquire) {
            match transport.receive(RECEIVE_SLICE) {
                Ok(Some(frame)) => {
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
                        );
                    }
                    accept_frame(
                        frame,
                        &source_id,
                        &mut capture,
                        &mut fields,
                        &publisher,
                        &event_sink,
                        &mut frames,
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    (event_sink)(EngineEvent::Error(error));
                    (event_sink)(EngineEvent::CanWorkbenchStatus(CanWorkbenchStatus {
                        connected: false,
                        bitrate,
                        listen_only,
                        source_id: source_id.clone(),
                    }));
                    let _ = transport.stop();
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
            (event_sink)(EngineEvent::Error(error));
        }
    }
    if !frames.is_empty() {
        (event_sink)(EngineEvent::CanFrames(frames));
    }
}

fn handle_command(
    command: Command,
    transport: &mut Box<dyn CanTransport>,
    fields: &mut Vec<CanSignalDefinition>,
    capture: &mut Option<CanCaptureWriter<File>>,
    send_task: &mut Option<SendTask>,
    scan_task: &mut Option<ScanTask>,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
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
                CanCaptureWriter::create_new(path)
                    .map(|writer| *capture = Some(writer))
                    .map_err(|error| error.to_string())
            };
            let _ = reply.send(result);
        }
        Command::StopCapture(reply) => {
            let result = capture.take().map_or(Ok(()), |writer| {
                writer.finish().map_err(|error| error.to_string())
            });
            let _ = reply.send(result);
        }
        Command::SendFrame(frame, pacing_ms, repeat, reply) => {
            let result = if transport_mode_conflicts(send_task.is_some(), scan_task.is_some()) {
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
            let result = if transport_mode_conflicts(send_task.is_some(), scan_task.is_some()) {
                Err("A CAN send or scan is already active".to_owned())
            } else {
                *scan_task = Some(ScanTask::new(config.clone()));
                (event_sink)(EngineEvent::CanScanStarted(config));
                Ok(())
            };
            let _ = reply.send(result);
        }
        Command::StopScan(reply) => {
            *scan_task = None;
            *send_task = None;
            (event_sink)(EngineEvent::CanScanStopped);
            let _ = reply.send(());
        }
        Command::Shutdown => {
            let _ = transport.stop();
        }
    }
}

fn transport_mode_conflicts(send: bool, scan: bool) -> bool {
    send || scan
}

pub(super) fn accept_frame(
    frame: CanFrame,
    source_id: &str,
    capture: &mut Option<CanCaptureWriter<File>>,
    fields: &mut [CanSignalDefinition],
    publisher: &SamplePublisher,
    event_sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    batch: &mut Vec<CanFrame>,
) {
    if let Some(writer) = capture.as_mut() {
        if let Err(error) = writer.write_frame(&frame) {
            let message = error.to_string();
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

pub(super) fn stamp_tx(frame: &mut CanFrame, started: &Instant) {
    frame.elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
    frame.wall_time_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64;
    frame.direction = CanDirection::Tx;
}
