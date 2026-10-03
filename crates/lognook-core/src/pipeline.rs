use crate::model::{EngineEvent, LogNookError, Marker, PlaybackState, RecordingSummary, Sample};
use crate::pipeline_worker::process;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

const QUEUE_CAPACITY: usize = 256;

pub(crate) enum Message {
    Sample(Sample),
    StartRecording(PathBuf, mpsc::Sender<Result<(), String>>),
    StopRecording(mpsc::Sender<Result<RecordingSummary, String>>),
    Marker(Marker, mpsc::Sender<Result<Marker, String>>),
    ReplayMarker(Marker),
    PlaybackStarted,
    PlaybackFinished,
    PlaybackState(PlaybackState),
    Disconnected,
    Error(String),
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct SampleSender(SyncSender<Message>);

impl SampleSender {
    pub(crate) fn send_sample(&self, sample: Sample) -> Result<(), LogNookError> {
        self.0
            .send(Message::Sample(sample))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn send_error(&self, message: String) -> Result<(), LogNookError> {
        self.0
            .send(Message::Error(message))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn send_disconnected(&self) -> Result<(), LogNookError> {
        self.0
            .send(Message::Disconnected)
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn send_marker(&self, marker: Marker) -> Result<(), LogNookError> {
        self.0
            .send(Message::ReplayMarker(marker))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn playback_finished(&self) {
        let _ = self.0.send(Message::PlaybackFinished);
    }

    pub(crate) fn send_playback_state(&self, state: PlaybackState) {
        let _ = self.0.send(Message::PlaybackState(state));
    }
}

pub(crate) struct SamplePipeline {
    sender: SampleSender,
    latest_elapsed_ns: Arc<AtomicI64>,
    recording_active: Arc<AtomicBool>,
    last_recording_path: Arc<Mutex<Option<PathBuf>>>,
    worker: Option<JoinHandle<()>>,
}

impl SamplePipeline {
    pub(crate) fn new(event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let latest_elapsed_ns = Arc::new(AtomicI64::new(0));
        let recording_active = Arc::new(AtomicBool::new(false));
        let last_recording_path = Arc::new(Mutex::new(None));
        let worker = thread::spawn({
            let latest = latest_elapsed_ns.clone();
            let recording_active = recording_active.clone();
            let last_recording_path = last_recording_path.clone();
            move || {
                process(
                    rx,
                    latest,
                    recording_active,
                    last_recording_path,
                    event_sink,
                )
            }
        });
        Self {
            sender: SampleSender(tx),
            latest_elapsed_ns,
            recording_active,
            last_recording_path,
            worker: Some(worker),
        }
    }

    pub(crate) fn sender(&self) -> SampleSender {
        self.sender.clone()
    }

    pub(crate) fn is_recording(&self) -> bool {
        self.recording_active.load(Ordering::Acquire)
    }

    pub(crate) fn last_recording_path(&self) -> Option<PathBuf> {
        self.last_recording_path.lock().ok()?.clone()
    }

    pub(crate) fn start_recording(&self, path: PathBuf) -> Result<(), LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::StartRecording(path, reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge recording start"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn stop_recording(&self) -> Result<RecordingSummary, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::StopRecording(reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge recording stop"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn insert_marker(
        &self,
        label: String,
        note: String,
    ) -> Result<Marker, LogNookError> {
        let marker = Marker {
            id: None,
            elapsed_ns: self.latest_elapsed_ns.load(Ordering::Relaxed),
            wall_time_unix_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .min(i64::MAX as u128) as i64,
            label,
            note,
        };
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::Marker(marker.clone(), reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge marker"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn playback_started(&self) {
        let _ = self.sender.0.send(Message::PlaybackStarted);
    }

    pub(crate) fn set_last_recording_path(&self, path: PathBuf) {
        if let Ok(mut last) = self.last_recording_path.lock() {
            *last = Some(path);
        }
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self.sender.0.send(Message::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for SamplePipeline {
    fn drop(&mut self) {
        self.shutdown();
    }
}
