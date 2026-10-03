use crate::{
    HardwareAdapter, SimulatedObdAdapter,
    acquisition::AcquisitionHandle,
    csv_io::export_recording_csv,
    model::{
        ConnectionCapabilities, EngineEvent, LogNookError, Marker, PlaybackState, RecordingInfo,
        RecordingSummary, SignalDefinition,
    },
    pipeline::SamplePipeline,
    playback::PlaybackHandle,
    recording_view::{
        add_recording_marker, delete_recording_marker, inspect_recording, update_recording_marker,
    },
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct LogNookEngine {
    event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    pipeline: SamplePipeline,
    acquisition: Option<AcquisitionHandle>,
    playback: Option<PlaybackHandle>,
    session_path: Option<PathBuf>,
    playback_state: PlaybackState,
}

impl LogNookEngine {
    pub fn new(sink: impl Fn(EngineEvent) + Send + Sync + 'static) -> Self {
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new(sink);
        Self {
            pipeline: SamplePipeline::new(event_sink.clone()),
            event_sink,
            acquisition: None,
            playback: None,
            session_path: None,
            playback_state: PlaybackState {
                position_ns: 0,
                duration_ns: 0,
                speed: 1.0,
                playing: false,
            },
        }
    }

    pub fn connect_simulated(
        &mut self,
        signal_ids: &[String],
    ) -> Result<ConnectionCapabilities, LogNookError> {
        self.connect_adapter(Box::new(SimulatedObdAdapter::default()), signal_ids)
    }

    pub fn connect_adapter(
        &mut self,
        mut adapter: Box<dyn HardwareAdapter>,
        signal_ids: &[String],
    ) -> Result<ConnectionCapabilities, LogNookError> {
        if self.is_connected() {
            return Err(LogNookError::new("An adapter is already connected"));
        }
        if let Some(mut old) = self.acquisition.take() {
            old.stop()?;
        }
        if signal_ids.is_empty() {
            return Err(LogNookError::new("Select at least one signal"));
        }
        let capabilities = match adapter.connect() {
            Ok(capabilities) => capabilities,
            Err(error) => {
                adapter.disconnect();
                return Err(error);
            }
        };
        let signals = match select_signals(&capabilities, signal_ids) {
            Ok(signals) => signals,
            Err(error) => {
                adapter.disconnect();
                return Err(error);
            }
        };
        (self.event_sink)(EngineEvent::Connected(capabilities.clone()));
        self.acquisition = Some(AcquisitionHandle::start(
            adapter,
            signals,
            self.pipeline.sender(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        ));
        Ok(capabilities)
    }

    pub fn is_connected(&self) -> bool {
        self.acquisition
            .as_ref()
            .is_some_and(AcquisitionHandle::is_running)
    }

    pub fn is_recording(&self) -> bool {
        self.pipeline.is_recording()
    }

    pub fn disconnect(&mut self) -> Result<(), LogNookError> {
        if let Some(mut acquisition) = self.acquisition.take() {
            acquisition.stop()?;
        }
        self.stop_playback();
        if self.pipeline.is_recording() {
            self.stop_recording()?;
        }
        (self.event_sink)(EngineEvent::Disconnected);
        Ok(())
    }

    pub fn start_recording(&mut self, path: &Path) -> Result<(), LogNookError> {
        if !self.is_connected() {
            return Err(LogNookError::new("Connect an adapter before recording"));
        }
        if self.pipeline.is_recording() {
            return Err(LogNookError::new("A recording is already active"));
        }
        self.pipeline.start_recording(path.to_path_buf())?;
        Ok(())
    }

    pub fn insert_marker(&self, label: &str, note: &str) -> Result<(), LogNookError> {
        self.pipeline
            .insert_marker(label.into(), note.into())
            .map(|_| ())
    }

    pub fn stop_recording(&mut self) -> Result<RecordingSummary, LogNookError> {
        let summary = self.pipeline.stop_recording()?;
        self.session_path = Some(summary.path.clone());
        self.pipeline.set_last_recording_path(summary.path.clone());
        Ok(summary)
    }

    pub fn export_last_csv(&self, path: &Path) -> Result<(), LogNookError> {
        let recording = self
            .pipeline
            .last_recording_path()
            .ok_or_else(|| LogNookError::new("No completed recording is available"))?;
        export_recording_csv(&recording, path)
    }

    pub fn load_recording(&mut self, path: &Path) -> Result<RecordingInfo, LogNookError> {
        if self.pipeline.is_recording() {
            return Err(LogNookError::new("Stop recording before loading a session"));
        }
        let info = inspect_recording(path)?;
        if info.sample_count == 0 {
            return Err(LogNookError::new("Recording contains no samples"));
        }
        self.disconnect_if_connected()?;
        self.stop_playback();
        self.session_path = Some(path.to_path_buf());
        self.pipeline.set_last_recording_path(path.to_path_buf());
        self.playback_state = PlaybackState {
            position_ns: 0,
            duration_ns: info.duration_ns,
            speed: 1.0,
            playing: false,
        };
        Ok(info)
    }

    pub fn start_replay(&mut self, speed: f64) -> Result<(), LogNookError> {
        validate_speed(speed)?;
        if self.pipeline.is_recording() {
            return Err(LogNookError::new("Stop recording before replay"));
        }
        let path = self
            .session_path
            .clone()
            .ok_or_else(|| LogNookError::new("Load a recording before replay"))?;
        self.disconnect_if_connected()?;
        if self.playback.is_some() {
            if self.playback_state()?.position_ns >= self.playback_state()?.duration_ns {
                self.seek_replay(0)?;
            }
            self.set_replay_speed(speed)?;
            return self.resume_replay();
        }
        let mut state = self.playback_state.clone();
        if state.position_ns >= state.duration_ns {
            state.position_ns = 0;
        }
        state.speed = speed;
        state.playing = true;
        self.playback_state = state.clone();
        self.pipeline.playback_started();
        self.playback = Some(PlaybackHandle::start(path, state, self.pipeline.sender()));
        Ok(())
    }

    pub fn pause_replay(&mut self) -> Result<(), LogNookError> {
        if let Some(playback) = &self.playback {
            self.playback_state = playback.pause()?;
        } else {
            self.playback_state.playing = false;
        }
        Ok(())
    }

    pub fn resume_replay(&mut self) -> Result<(), LogNookError> {
        if self.playback.is_none() {
            return self.start_replay(self.playback_state.speed);
        }
        if self.playback_state()?.position_ns >= self.playback_state()?.duration_ns {
            self.seek_replay(0)?;
        }
        self.playback_state = self.playback.as_ref().unwrap().resume()?;
        Ok(())
    }

    pub fn seek_replay(&mut self, position_ns: i64) -> Result<(), LogNookError> {
        if let Some(playback) = &self.playback {
            self.playback_state = playback.seek(position_ns)?;
        } else {
            self.playback_state.position_ns = position_ns.clamp(0, self.playback_state.duration_ns);
        }
        Ok(())
    }

    pub fn set_replay_speed(&mut self, speed: f64) -> Result<(), LogNookError> {
        validate_speed(speed)?;
        if let Some(playback) = &self.playback {
            self.playback_state = playback.set_speed(speed)?;
        } else {
            self.playback_state.speed = speed;
        }
        Ok(())
    }

    pub fn playback_state(&mut self) -> Result<PlaybackState, LogNookError> {
        if let Some(playback) = &self.playback {
            self.playback_state = playback.state()?;
        }
        Ok(self.playback_state.clone())
    }

    pub fn add_playback_marker(&mut self, label: &str, note: &str) -> Result<Marker, LogNookError> {
        let path = self.active_session_path()?;
        let was_playing = self.pause_for_marker()?;
        let position_ns = self.playback_state()?.position_ns;
        let marker = Marker {
            id: None,
            elapsed_ns: position_ns,
            wall_time_unix_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .min(i64::MAX as u128) as i64,
            label: label.into(),
            note: note.into(),
        };
        self.finish_marker_change(add_recording_marker(&path, &marker), was_playing)
    }

    pub fn update_playback_marker(
        &mut self,
        id: i64,
        label: &str,
        note: &str,
    ) -> Result<Marker, LogNookError> {
        let path = self.active_session_path()?;
        let was_playing = self.pause_for_marker()?;
        self.finish_marker_change(update_recording_marker(&path, id, label, note), was_playing)
    }

    pub fn delete_playback_marker(&mut self, id: i64) -> Result<(), LogNookError> {
        let path = self.active_session_path()?;
        let was_playing = self.pause_for_marker()?;
        self.finish_marker_change(delete_recording_marker(&path, id), was_playing)
    }

    pub fn active_session_path(&self) -> Result<PathBuf, LogNookError> {
        self.session_path
            .clone()
            .ok_or_else(|| LogNookError::new("No session is loaded"))
    }

    fn pause_for_marker(&mut self) -> Result<bool, LogNookError> {
        let was_playing = self.playback_state()?.playing;
        if was_playing {
            self.pause_replay()?;
        }
        Ok(was_playing)
    }

    fn finish_marker_change<T>(
        &mut self,
        result: Result<T, LogNookError>,
        was_playing: bool,
    ) -> Result<T, LogNookError> {
        if was_playing {
            let resume = self.resume_replay();
            if result.is_ok() {
                resume?;
            }
        }
        result
    }

    pub fn replay_recording(&mut self, path: &Path, speed: f64) -> Result<(), LogNookError> {
        self.load_recording(path)?;
        self.start_replay(speed)
    }

    pub fn replay_last(&mut self, speed: f64) -> Result<(), LogNookError> {
        let path = self
            .pipeline
            .last_recording_path()
            .ok_or_else(|| LogNookError::new("No completed recording is available"))?;
        self.replay_recording(&path, speed)
    }

    pub fn stop_replay(&mut self) {
        self.stop_playback();
    }

    pub fn last_recording_path(&self) -> Option<PathBuf> {
        self.pipeline.last_recording_path()
    }

    fn disconnect_if_connected(&mut self) -> Result<(), LogNookError> {
        if self.is_connected() {
            self.disconnect()?;
        } else if let Some(mut acquisition) = self.acquisition.take() {
            acquisition.stop()?;
        }
        Ok(())
    }

    fn stop_playback(&mut self) {
        if let Some(mut playback) = self.playback.take() {
            playback.stop();
            if let Ok(state) = playback.state() {
                self.playback_state = state;
            }
        }
    }
}

impl Drop for LogNookEngine {
    fn drop(&mut self) {
        let _ = self.disconnect();
        self.pipeline.shutdown();
    }
}

fn select_signals(
    capabilities: &ConnectionCapabilities,
    ids: &[String],
) -> Result<Vec<SignalDefinition>, LogNookError> {
    let mut selected: Vec<SignalDefinition> = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(signal) = capabilities
            .supported_signals
            .iter()
            .find(|signal| signal.id == *id)
            && !selected.iter().any(|existing| existing.id == signal.id)
        {
            selected.push(signal.clone());
        }
    }
    if selected.is_empty() {
        return Err(LogNookError::new(
            "None of the selected signals are supported by this adapter",
        ));
    }
    Ok(selected)
}

fn validate_speed(speed: f64) -> Result<(), LogNookError> {
    if speed.is_finite() && (0.1..=32.0).contains(&speed) {
        Ok(())
    } else {
        Err(LogNookError::new(
            "Replay speed must be between 0.1x and 32x",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{LogNookEngine, select_signals};
    use crate::{
        ConnectionCapabilities, DeviceDescriptor, EngineEvent, HardwareAdapter, LogNookError,
        Sample, SignalDefinition, standard_obd_signals,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    struct EmptyAdapter {
        disconnected: Arc<AtomicBool>,
    }

    impl HardwareAdapter for EmptyAdapter {
        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            Ok(ConnectionCapabilities {
                device: DeviceDescriptor {
                    stable_id: "fixture-sx".into(),
                    hardware_family: "OBDLink SX".into(),
                    firmware: Some("STN1130 v1.0.0".into()),
                    path: Some("COM4".into()),
                },
                protocol: "SAE J1979 Mode 01".into(),
                read_only: true,
                supported_signals: standard_obd_signals()
                    .into_iter()
                    .filter(|signal| signal.id == "engine_rpm")
                    .collect(),
            })
        }

        fn read_samples(
            &mut self,
            _signals: &[SignalDefinition],
        ) -> Result<Vec<Sample>, LogNookError> {
            Ok(Vec::new())
        }

        fn disconnect(&mut self) {
            self.disconnected.store(true, Ordering::Release);
        }
    }

    #[test]
    fn selection_uses_only_the_signals_the_connected_ecu_supports() {
        let supported = standard_obd_signals()
            .into_iter()
            .filter(|signal| signal.id == "engine_rpm")
            .collect();
        let capabilities = ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "fixture".into(),
                hardware_family: "fixture".into(),
                firmware: None,
                path: None,
            },
            protocol: "fixture".into(),
            read_only: true,
            supported_signals: supported,
        };

        let selected =
            select_signals(&capabilities, &["engine_rpm".into(), "intake_temp".into()]).unwrap();

        assert_eq!(
            selected
                .iter()
                .map(|signal| signal.id.as_str())
                .collect::<Vec<_>>(),
            vec!["engine_rpm"]
        );
    }

    #[test]
    fn generic_adapter_connection_starts_the_shared_pipeline_and_disconnects_it() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_sink = events.clone();
        let mut engine = LogNookEngine::new(move |event| event_sink.lock().unwrap().push(event));
        let disconnected = Arc::new(AtomicBool::new(false));
        let signal_ids = vec!["engine_rpm".into()];

        let capabilities = engine
            .connect_adapter(
                Box::new(EmptyAdapter {
                    disconnected: disconnected.clone(),
                }),
                &signal_ids,
            )
            .unwrap();
        assert_eq!(capabilities.device.hardware_family, "OBDLink SX");
        assert!(capabilities.read_only);
        assert!(engine.is_connected());

        engine.disconnect().unwrap();
        assert!(disconnected.load(Ordering::Acquire));
        let events = events.lock().unwrap();
        assert!(matches!(events.first(), Some(EngineEvent::Connected(_))));
        assert_eq!(events.last(), Some(&EngineEvent::Disconnected));
    }
}
