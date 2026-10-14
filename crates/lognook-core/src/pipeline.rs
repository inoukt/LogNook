use crate::derived::CompiledDerivedSignal;
use crate::model::{
    AutomaticTriggerConfig, AutomaticTriggerStatus, EngineEvent, LogNookError, Marker,
    PlaybackState, RecordingSummary, RetrospectiveRecordingStart, Sample,
};
use crate::pipeline_worker::process;
use crate::vehicle_profiles::VehicleIdentitySnapshot;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

const QUEUE_CAPACITY: usize = 256;

pub(crate) enum Message {
    Sample(Sample),
    SetDerivedSignals(Vec<CompiledDerivedSignal>, mpsc::Sender<Result<(), String>>),
    TareDerivedSignal(String, mpsc::Sender<Result<f64, String>>),
    PlaybackSample(u64, Sample),
    StartRecording(PathBuf, mpsc::Sender<Result<(), String>>),
    StartRecordingFromConnect(
        PathBuf,
        mpsc::Sender<Result<RetrospectiveRecordingStart, String>>,
    ),
    UpdateVehicleRecordingContext(
        Option<VehicleIdentitySnapshot>,
        u64,
        mpsc::Sender<Result<(), String>>,
    ),
    ArmAutomaticTrigger(
        AutomaticTriggerConfig,
        PathBuf,
        mpsc::Sender<Result<AutomaticTriggerStatus, String>>,
    ),
    DisarmAutomaticTrigger(mpsc::Sender<Result<AutomaticTriggerStatus, String>>),
    StopRecording(mpsc::Sender<Result<RecordingSummary, String>>),
    Marker(Marker, mpsc::Sender<Result<Marker, String>>),
    PlaybackMarker(u64, Marker),
    PlaybackStarted,
    PlaybackFinished,
    PlaybackState(PlaybackState),
    SourceDisconnected {
        obd: bool,
        failed: bool,
        last_source: bool,
    },
    Error(String),
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct SampleSender(SyncSender<Message>, Arc<AtomicUsize>);

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

    pub(crate) fn source_connected(&self) {
        self.1.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn source_disconnected(&self, obd: bool, failed: bool) -> Result<(), LogNookError> {
        let Ok(previous) = self
            .1
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_sub(1)
            })
        else {
            return Err(LogNookError::new("Acquisition source was not connected"));
        };
        self.0
            .send(Message::SourceDisconnected {
                obd,
                failed,
                last_source: previous == 1,
            })
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn send_playback_sample(
        &self,
        session_id: u64,
        sample: Sample,
    ) -> Result<(), LogNookError> {
        self.0
            .send(Message::PlaybackSample(session_id, sample))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn send_playback_marker(
        &self,
        session_id: u64,
        marker: Marker,
    ) -> Result<(), LogNookError> {
        self.0
            .send(Message::PlaybackMarker(session_id, marker))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))
    }

    pub(crate) fn playback_finished(&self) {
        let _ = self.0.send(Message::PlaybackFinished);
    }

    pub(crate) fn send_playback_state(&self, state: PlaybackState) {
        let _ = self.0.send(Message::PlaybackState(state));
    }
}

/// Cloneable, bounded producer for receive-timestamped live measurements.
#[derive(Clone)]
pub struct SamplePublisher {
    sender: SampleSender,
    latest_elapsed_ns: Arc<AtomicI64>,
}

impl SamplePublisher {
    pub(crate) fn source_connected(&self) {
        self.sender.source_connected();
    }

    pub(crate) fn source_disconnected(&self, obd: bool, failed: bool) -> Result<(), LogNookError> {
        self.sender.source_disconnected(obd, failed)
    }

    pub fn publish_timestamped_sample(&self, sample: Sample) -> Result<(), LogNookError> {
        if sample.elapsed_ns < 0
            || sample.origin != crate::SampleOrigin::Live
            || !matches!(
                sample.quality,
                crate::SampleQuality::Valid | crate::SampleQuality::Suspect
            )
            || !sample.value.is_finite()
        {
            return Err(LogNookError::new(
                "Timestamped samples must be finite live measurements with non-negative elapsed time and valid or suspect quality",
            ));
        }
        self.latest_elapsed_ns
            .fetch_max(sample.elapsed_ns, Ordering::AcqRel);
        self.sender.send_sample(sample)
    }
}

pub(crate) struct SamplePipeline {
    sender: SampleSender,
    latest_elapsed_ns: Arc<AtomicI64>,
    recording_active: Arc<AtomicBool>,
    automatic_trigger_status: Arc<Mutex<AutomaticTriggerStatus>>,
    last_recording_path: Arc<Mutex<Option<PathBuf>>>,
    worker: Option<JoinHandle<()>>,
}

impl SamplePipeline {
    pub(crate) fn new(event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let active_sources = Arc::new(AtomicUsize::new(0));
        let latest_elapsed_ns = Arc::new(AtomicI64::new(0));
        let recording_active = Arc::new(AtomicBool::new(false));
        let automatic_trigger_status = Arc::new(Mutex::new(AutomaticTriggerStatus::default()));
        let last_recording_path = Arc::new(Mutex::new(None));
        let worker = thread::spawn({
            let latest = latest_elapsed_ns.clone();
            let recording_active = recording_active.clone();
            let automatic_trigger_status = automatic_trigger_status.clone();
            let last_recording_path = last_recording_path.clone();
            move || {
                process(
                    rx,
                    latest,
                    recording_active,
                    automatic_trigger_status,
                    last_recording_path,
                    event_sink,
                )
            }
        });
        Self {
            sender: SampleSender(tx, active_sources),
            latest_elapsed_ns,
            recording_active,
            automatic_trigger_status,
            last_recording_path,
            worker: Some(worker),
        }
    }

    pub(crate) fn sender(&self) -> SampleSender {
        self.sender.clone()
    }

    pub(crate) fn publisher(&self) -> SamplePublisher {
        SamplePublisher {
            sender: self.sender(),
            latest_elapsed_ns: self.latest_elapsed_ns.clone(),
        }
    }

    pub(crate) fn next_research_elapsed_ns(&self) -> i64 {
        self.latest_elapsed_ns
            .try_update(Ordering::AcqRel, Ordering::Acquire, |elapsed| {
                Some(elapsed.saturating_add(1_000_000))
            })
            .unwrap_or(0)
            .saturating_add(1_000_000)
    }

    pub(crate) fn set_derived_signals(
        &self,
        signals: Vec<CompiledDerivedSignal>,
    ) -> Result<(), LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::SetDerivedSignals(signals, reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge derived signals"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn tare_derived_signal(&self, id: String) -> Result<f64, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::TareDerivedSignal(id, reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge tare"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn is_recording(&self) -> bool {
        self.recording_active.load(Ordering::Acquire)
    }

    pub(crate) fn automatic_trigger_status(&self) -> AutomaticTriggerStatus {
        self.automatic_trigger_status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
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

    pub(crate) fn start_recording_from_connect(
        &self,
        path: PathBuf,
    ) -> Result<RetrospectiveRecordingStart, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::StartRecordingFromConnect(path, reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge recording start"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn update_vehicle_recording_context(
        &self,
        snapshot: Option<VehicleIdentitySnapshot>,
        expected_generation: u64,
    ) -> Result<(), LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::UpdateVehicleRecordingContext(
                snapshot,
                expected_generation,
                reply,
            ))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge vehicle context"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn arm_automatic_trigger(
        &self,
        config: AutomaticTriggerConfig,
        path: PathBuf,
    ) -> Result<AutomaticTriggerStatus, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::ArmAutomaticTrigger(config, path, reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Sample pipeline did not acknowledge trigger arming"))?
            .map_err(LogNookError::new)
    }

    pub(crate) fn disarm_automatic_trigger(&self) -> Result<AutomaticTriggerStatus, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.sender
            .0
            .send(Message::DisarmAutomaticTrigger(reply))
            .map_err(|_| LogNookError::new("Sample pipeline has stopped"))?;
        response
            .recv()
            .map_err(|_| {
                LogNookError::new("Sample pipeline did not acknowledge trigger disarming")
            })?
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
