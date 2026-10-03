use crate::{
    HardwareAdapter, SimulatedObdAdapter,
    acquisition::AcquisitionHandle,
    csv_io::export_recording_csv,
    model::{
        ConnectionCapabilities, EngineEvent, LogNookError, RecordingSummary, SignalDefinition,
    },
    pipeline::SamplePipeline,
    playback::PlaybackHandle,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct LogNookEngine {
    event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    pub(crate) pipeline: SamplePipeline,
    acquisition: Option<AcquisitionHandle>,
    pub(crate) playback: Option<PlaybackHandle>,
    pub(crate) session_path: Option<PathBuf>,
    pub(crate) playback_state: crate::PlaybackState,
    pub(crate) session_id: u64,
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
            playback_state: crate::PlaybackState {
                session_id: 0,
                position_ns: 0,
                duration_ns: 0,
                speed: 1.0,
                playing: false,
            },
            session_id: 0,
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
        self.stop_playback();
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

    pub fn last_recording_path(&self) -> Option<PathBuf> {
        self.pipeline.last_recording_path()
    }

    pub(crate) fn disconnect_if_connected(&mut self) -> Result<(), LogNookError> {
        if self.is_connected() {
            self.disconnect()?;
        } else if let Some(mut acquisition) = self.acquisition.take() {
            acquisition.stop()?;
        }
        Ok(())
    }

    pub(crate) fn stop_playback(&mut self) {
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
