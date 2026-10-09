use crate::{
    CanDeviceSelection, CanScanConfig, CanScanStopReason, CanSignalDefinition, CanTransport,
    CanWorkbench, CanWorkbenchConfig, CanWorkbenchStatus, DiagnosticSink, HardwareAdapter,
    RawAdapterCommand, RawAdapterCommandResult, ResearchTestRequest, ResearchTestResult,
    SimulatedObdAdapter,
    acquisition::AcquisitionHandle,
    csv_io::export_recording_csv,
    derived::{CompiledDerivedSignal, compile_derived_signals},
    model::{
        AutomaticTriggerConfig, AutomaticTriggerStatus, ConnectionCapabilities,
        DerivedSignalDefinition, EngineEvent, LogNookError, OemPollingStatus, RecordingSummary,
        Sample, SampleOrigin, SampleQuality, SignalDefinition,
    },
    oem_catalog::OemPollPlan,
    pipeline::SamplePipeline,
    playback::PlaybackHandle,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

pub struct LogNookEngine {
    diagnostic_sink: DiagnosticSink,
    event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    pub(crate) pipeline: SamplePipeline,
    acquisition: Option<AcquisitionHandle>,
    pub(crate) playback: Option<PlaybackHandle>,
    pub(crate) session_path: Option<PathBuf>,
    pub(crate) playback_state: crate::PlaybackState,
    pub(crate) session_id: u64,
    selected_signal_ids: HashSet<String>,
    derived_signal_definitions: Vec<DerivedSignalDefinition>,
    active_derived_signal_ids: HashSet<String>,
    can_workbench: Option<CanWorkbench>,
    connection_capabilities: Option<ConnectionCapabilities>,
    session_origin: Option<Instant>,
    session_origin_wall_unix_ns: Option<i64>,
    communication_sink: Option<crate::communication_trace::CommunicationSink>,
}

impl LogNookEngine {
    pub fn new(sink: impl Fn(EngineEvent) + Send + Sync + 'static) -> Self {
        let event_sink: Arc<dyn Fn(EngineEvent) + Send + Sync> = Arc::new(sink);
        Self {
            diagnostic_sink: crate::diagnostics::noop_diagnostic_sink(),
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
            selected_signal_ids: HashSet::new(),
            derived_signal_definitions: Vec::new(),
            active_derived_signal_ids: HashSet::new(),
            can_workbench: None,
            connection_capabilities: None,
            session_origin: None,
            session_origin_wall_unix_ns: None,
            communication_sink: None,
        }
    }

    /// Configure diagnostics for subsequent connections, before the adapter connects.
    pub fn set_diagnostic_sink(&mut self, sink: DiagnosticSink) {
        self.diagnostic_sink = sink;
    }

    pub fn set_communication_sink(&mut self, sink: crate::communication_trace::CommunicationSink) {
        self.communication_sink = Some(sink);
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
        if self.pipeline.is_recording() {
            return Err(LogNookError::new(
                "Stop the active recording before connecting another adapter",
            ));
        }
        if self.is_obd_connected() {
            return Err(LogNookError::new("An OBD adapter is already connected"));
        }
        if !self.is_connected() {
            self.session_origin = None;
            self.session_origin_wall_unix_ns = None;
        }
        self.stop_playback();
        if let Some(mut old) = self.acquisition.take() {
            old.stop()?;
        }
        if signal_ids.is_empty() {
            return Err(LogNookError::new("Select at least one signal"));
        }
        let compiled = compile_derived_signals(
            &self.derived_signal_definitions,
            &crate::standard_obd_signals(),
        )?;
        adapter.set_diagnostic_sink(self.diagnostic_sink.clone());
        if let Some(sink) = &self.communication_sink {
            adapter.set_communication_sink(sink.clone());
        }
        let capabilities = match adapter.connect() {
            Ok(capabilities) => capabilities,
            Err(error) => {
                adapter.disconnect();
                return Err(error);
            }
        };
        let adapter_connected_at = Instant::now();
        let adapter_connected_wall_unix_ns = now_unix_ns();
        let derived_by_id = compiled
            .iter()
            .map(|derived| (derived.definition.id.as_str(), derived))
            .collect::<HashMap<_, _>>();
        let all_derived_ids = self
            .derived_signal_definitions
            .iter()
            .map(|definition| definition.id.as_str())
            .collect::<HashSet<_>>();
        let mut requested_source_ids = Vec::new();
        let mut selected_derived_ids = HashSet::new();
        for id in signal_ids {
            if let Some(derived) = derived_by_id.get(id.as_str()) {
                if !self.active_derived_signal_ids.contains(id) {
                    adapter.disconnect();
                    return Err(LogNookError::new(format!(
                        "Enable derived signal '{id}' before connecting"
                    )));
                }
                selected_derived_ids.insert(id.clone());
                requested_source_ids.extend(derived.dependencies().iter().cloned());
            } else if all_derived_ids.contains(id.as_str()) {
                adapter.disconnect();
                return Err(LogNookError::new(format!(
                    "Derived signal '{id}' is not active"
                )));
            } else {
                requested_source_ids.push(id.clone());
            }
        }
        for id in &selected_derived_ids {
            if let Some(derived) = derived_by_id.get(id.as_str()) {
                if let Some(missing) = derived.dependencies().iter().find(|dependency| {
                    !capabilities
                        .supported_signals
                        .iter()
                        .any(|signal| &signal.id == *dependency)
                }) {
                    adapter.disconnect();
                    return Err(LogNookError::new(format!(
                        "Derived signal '{id}' requires unsupported input '{missing}'"
                    )));
                }
            }
        }
        let signals = match select_signals(&capabilities, &requested_source_ids) {
            Ok(signals) => signals,
            Err(error) => {
                adapter.disconnect();
                return Err(error);
            }
        };
        self.pipeline.set_derived_signals(
            compiled
                .iter()
                .filter(|signal| {
                    self.active_derived_signal_ids
                        .contains(&signal.definition.id)
                })
                .cloned()
                .collect(),
        )?;
        let session_origin = *self.session_origin.get_or_insert(adapter_connected_at);
        let session_origin_wall_unix_ns = *self
            .session_origin_wall_unix_ns
            .get_or_insert(adapter_connected_wall_unix_ns);
        let adapter_offset_ns = adapter_connected_at
            .saturating_duration_since(session_origin)
            .as_nanos()
            .min(i64::MAX as u128) as i64;
        (self.event_sink)(EngineEvent::Connected(capabilities.clone()));
        self.selected_signal_ids = signals
            .iter()
            .map(|signal| signal.id.clone())
            .chain(selected_derived_ids)
            .collect();
        self.acquisition = Some(AcquisitionHandle::start_at(
            adapter,
            signals,
            self.pipeline.sender(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            self.diagnostic_sink.clone(),
            Some(capabilities.device.stable_id.clone()),
            session_origin,
            session_origin_wall_unix_ns,
            adapter_offset_ns,
        ));
        self.connection_capabilities = Some(capabilities.clone());
        Ok(capabilities)
    }

    /// Connect a test transport through the engine-owned serialized CAN worker.
    /// This accepts an injected transport for deterministic tests; production uses
    /// `connect_candlelight` below.
    pub fn connect_can_transport(
        &mut self,
        config: CanWorkbenchConfig,
        mut transport: Box<dyn CanTransport>,
    ) -> Result<CanWorkbenchStatus, LogNookError> {
        if self.pipeline.is_recording() {
            return Err(LogNookError::new(
                "Stop the active recording before connecting another adapter",
            ));
        }
        if self.is_can_connected() {
            return Err(LogNookError::new("A CAN adapter is already connected"));
        }
        if !self.is_connected() {
            self.session_origin = None;
            self.session_origin_wall_unix_ns = None;
        }
        self.stop_playback();
        if let Some(sink) = &self.communication_sink {
            transport.set_communication_sink(sink.clone());
        }
        let session_origin = *self.session_origin.get_or_insert_with(Instant::now);
        self.session_origin_wall_unix_ns
            .get_or_insert_with(now_unix_ns);
        let workbench = CanWorkbench::start_at(
            transport,
            config,
            self.sample_publisher(),
            self.event_sink.clone(),
            session_origin,
        )?;
        let status = workbench.status();
        self.can_workbench = Some(workbench);
        (self.event_sink)(EngineEvent::CanWorkbenchStatus(status.clone()));
        Ok(status)
    }

    pub fn connect_candlelight(
        &mut self,
        selection: &CanDeviceSelection,
        config: CanWorkbenchConfig,
    ) -> Result<CanWorkbenchStatus, LogNookError> {
        let transport = crate::open_candlelight(selection)?;
        self.connect_can_transport(config, transport)
    }

    pub fn configure_can_signals(
        &self,
        fields: Vec<CanSignalDefinition>,
    ) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("Connect Candlelight before configuring CAN fields"))?
            .configure_fields(fields)
    }

    pub fn start_can_capture(&self, path: &Path) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("Connect Candlelight before starting raw capture"))?
            .start_capture(path)
    }

    pub fn stop_can_capture(&self) -> Result<(), LogNookError> {
        self.stop_can_capture_with_reason(CanScanStopReason::CaptureStopped)
    }

    pub fn stop_can_capture_with_reason(
        &self,
        reason: CanScanStopReason,
    ) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("No CAN workbench is connected"))?
            .stop_capture_with_reason(reason)
    }

    pub fn mark_can_event(
        &self,
        label: &str,
        pre_ms: u64,
        post_ms: u64,
    ) -> Result<crate::CanManualEventMark, LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("No CAN workbench is connected"))?
            .mark_event(label, pre_ms, post_ms)
    }

    pub fn clear_can_event_results(&self) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("No CAN workbench is connected"))?
            .clear_event_results()
    }

    pub fn arm_can_trigger(
        &self,
        pre_ms: u64,
        post_ms: u64,
        max_frames: usize,
    ) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("Connect Candlelight before arming a CAN trigger"))?
            .arm_trigger(pre_ms, post_ms, max_frames)
    }

    pub fn trigger_can_now(&self) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("No CAN workbench is connected"))?
            .trigger_now()
    }

    pub fn disarm_can_trigger(&self) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("No CAN workbench is connected"))?
            .disarm_trigger()
    }

    pub fn send_can_frame(
        &self,
        frame: crate::CanFrame,
        pacing_ms: u64,
        repeat: u32,
    ) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("Connect Candlelight before sending a CAN frame"))?
            .send_frame(frame, pacing_ms, repeat)
    }

    pub fn start_can_scan(&self, config: CanScanConfig) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("Connect Candlelight before starting a CAN scan"))?
            .start_scan(config)
    }

    pub fn start_can_oem_candidate(
        &self,
        spec: crate::OemCandidateTrySpec,
    ) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("Connect Candlelight before starting a CAN scan"))?
            .start_oem_candidate(spec)
    }

    pub fn stop_can_scan(&self) -> Result<(), LogNookError> {
        self.stop_can_scan_with_reason(CanScanStopReason::UserRequested)
    }

    pub fn stop_can_scan_with_reason(&self, reason: CanScanStopReason) -> Result<(), LogNookError> {
        self.can_workbench
            .as_ref()
            .ok_or_else(|| LogNookError::new("No CAN workbench is connected"))?
            .stop_scan_with_reason(reason)
    }

    pub fn is_connected(&self) -> bool {
        self.is_obd_connected() || self.is_can_connected()
    }

    pub fn is_obd_connected(&self) -> bool {
        self.acquisition
            .as_ref()
            .is_some_and(AcquisitionHandle::is_running)
    }

    pub fn is_can_connected(&self) -> bool {
        self.can_workbench
            .as_ref()
            .is_some_and(CanWorkbench::is_running)
    }

    pub fn connection_capabilities(&self) -> Option<&ConnectionCapabilities> {
        self.is_obd_connected()
            .then_some(self.connection_capabilities.as_ref())
            .flatten()
    }

    pub fn can_workbench_status(&self) -> Option<CanWorkbenchStatus> {
        self.can_workbench.as_ref().map(CanWorkbench::status)
    }

    pub fn selected_signal_ids(&self) -> Vec<String> {
        if !self.is_obd_connected() {
            return Vec::new();
        }
        let mut signal_ids = self.selected_signal_ids.iter().cloned().collect::<Vec<_>>();
        signal_ids.sort();
        signal_ids
    }

    pub fn is_recording(&self) -> bool {
        self.pipeline.is_recording()
    }

    pub fn execute_research_test(
        &self,
        request: ResearchTestRequest,
    ) -> Result<ResearchTestResult, LogNookError> {
        self.acquisition
            .as_ref()
            .filter(|acquisition| acquisition.is_running())
            .ok_or_else(|| LogNookError::new("No adapter is connected"))?
            .execute_research_test(request)
    }

    pub fn queue_research_test(
        &self,
        request: ResearchTestRequest,
    ) -> Result<std::sync::mpsc::Receiver<Result<ResearchTestResult, LogNookError>>, LogNookError>
    {
        self.acquisition
            .as_ref()
            .filter(|acquisition| acquisition.is_running())
            .ok_or_else(|| LogNookError::new("No adapter is connected"))?
            .queue_research_test(request)
    }

    pub fn execute_raw_command(
        &self,
        command: RawAdapterCommand,
    ) -> Result<RawAdapterCommandResult, LogNookError> {
        self.acquisition
            .as_ref()
            .filter(|acquisition| acquisition.is_running())
            .ok_or_else(|| LogNookError::new("No serial OBD adapter is connected"))?
            .execute_raw_command(command)
    }

    pub fn set_oem_poll_plan(&self, plan: Option<OemPollPlan>) -> Result<(), LogNookError> {
        if plan.as_ref().is_some_and(|plan| plan.groups.is_empty()) {
            return Err(LogNookError::new("Select at least one verified OEM signal"));
        }
        self.acquisition
            .as_ref()
            .filter(|acquisition| acquisition.is_running())
            .ok_or_else(|| LogNookError::new("No adapter is connected"))?
            .set_oem_poll_plan(plan)
    }

    pub fn oem_polling_status(&self) -> OemPollingStatus {
        self.acquisition
            .as_ref()
            .map(AcquisitionHandle::oem_polling_status)
            .unwrap_or_default()
    }

    pub fn sample_publisher(&self) -> crate::SamplePublisher {
        self.pipeline.publisher()
    }

    pub fn publish_timestamped_sample(&self, sample: Sample) -> Result<(), LogNookError> {
        self.sample_publisher().publish_timestamped_sample(sample)
    }

    pub fn publish_research_sample(&self, mut sample: Sample) -> Result<(), LogNookError> {
        if sample.origin != SampleOrigin::Live
            || !matches!(
                sample.quality,
                SampleQuality::Valid | SampleQuality::Suspect
            )
            || !sample.value.is_finite()
        {
            return Err(LogNookError::new(
                "Research samples must be finite live measurements with valid or suspect quality",
            ));
        }
        sample.elapsed_ns = self.pipeline.next_research_elapsed_ns();
        self.pipeline.sender().send_sample(sample)
    }

    pub fn configure_derived_signals(
        &mut self,
        definitions: &[DerivedSignalDefinition],
        active_ids: &[String],
    ) -> Result<(), LogNookError> {
        if self.is_obd_connected() || self.pipeline.is_recording() {
            return Err(LogNookError::new(
                "Disconnect and stop recording before changing derived signals",
            ));
        }
        let compiled = compile_derived_signals(definitions, &crate::standard_obd_signals())?;
        let available = compiled
            .iter()
            .map(|signal| signal.definition.id.as_str())
            .collect::<HashSet<_>>();
        let mut active = HashSet::new();
        for id in active_ids {
            if !available.contains(id.as_str()) {
                return Err(LogNookError::new(format!(
                    "Cannot activate missing derived signal '{id}'"
                )));
            }
            active.insert(id.clone());
        }
        let active_compiled = compiled
            .into_iter()
            .filter(|signal| active.contains(&signal.definition.id))
            .collect::<Vec<CompiledDerivedSignal>>();
        self.pipeline.set_derived_signals(active_compiled)?;
        self.derived_signal_definitions = definitions.to_vec();
        self.active_derived_signal_ids = active;
        Ok(())
    }

    pub fn tare_derived_signal(&mut self, id: &str) -> Result<f64, LogNookError> {
        if !self.is_obd_connected() {
            return Err(LogNookError::new(
                "Connect an adapter before taring a derived signal",
            ));
        }
        if !self.active_derived_signal_ids.contains(id) || !self.selected_signal_ids.contains(id) {
            return Err(LogNookError::new(format!(
                "Derived signal '{id}' is not being acquired"
            )));
        }
        let offset = self.pipeline.tare_derived_signal(id.to_owned())?;
        if let Some(definition) = self
            .derived_signal_definitions
            .iter_mut()
            .find(|definition| definition.id == id)
        {
            definition.tare_offset = offset;
        }
        Ok(offset)
    }

    pub fn disconnect(&mut self) -> Result<(), LogNookError> {
        if let Some(workbench) = self.can_workbench.take() {
            let mut status = workbench.status();
            drop(workbench);
            status.connected = false;
            (self.event_sink)(EngineEvent::CanWorkbenchStatus(status));
        }
        if let Some(mut acquisition) = self.acquisition.take() {
            acquisition.stop()?;
        }
        self.selected_signal_ids.clear();
        self.connection_capabilities = None;
        self.session_origin = None;
        self.session_origin_wall_unix_ns = None;
        self.stop_playback();
        if self.pipeline.is_recording() {
            self.stop_recording()?;
        }
        self.pipeline.disarm_automatic_trigger()?;
        (self.event_sink)(EngineEvent::Disconnected);
        Ok(())
    }

    pub fn disconnect_obd(&mut self) -> Result<(), LogNookError> {
        if let Some(mut acquisition) = self.acquisition.take() {
            acquisition.stop()?;
        }
        self.selected_signal_ids.clear();
        self.connection_capabilities = None;
        self.stop_playback();
        (self.event_sink)(EngineEvent::Disconnected);
        self.finish_source_session_if_idle()
    }

    pub fn disconnect_can(&mut self) -> Result<(), LogNookError> {
        if let Some(workbench) = self.can_workbench.take() {
            let mut status = workbench.status();
            drop(workbench);
            status.connected = false;
            (self.event_sink)(EngineEvent::CanWorkbenchStatus(status));
        }
        self.finish_source_session_if_idle()
    }

    fn finish_source_session_if_idle(&mut self) -> Result<(), LogNookError> {
        if !self.is_connected() {
            self.session_origin = None;
            self.session_origin_wall_unix_ns = None;
            if self.pipeline.is_recording() {
                self.stop_recording()?;
            }
            self.pipeline.disarm_automatic_trigger()?;
        }
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

    pub fn arm_automatic_trigger(
        &mut self,
        config: &AutomaticTriggerConfig,
        path: &Path,
    ) -> Result<AutomaticTriggerStatus, LogNookError> {
        if !self.is_obd_connected() {
            return Err(LogNookError::new(
                "Connect an adapter before arming an automatic trigger",
            ));
        }
        if self.pipeline.is_recording() {
            return Err(LogNookError::new(
                "Stop the active recording before arming an automatic trigger",
            ));
        }
        let config = config.normalized()?;
        if !self.selected_signal_ids.contains(&config.signal_id) {
            return Err(LogNookError::new(
                "Choose a trigger signal that is currently acquired",
            ));
        }
        self.pipeline
            .arm_automatic_trigger(config, path.to_path_buf())
    }

    pub fn disarm_automatic_trigger(&self) -> Result<AutomaticTriggerStatus, LogNookError> {
        self.pipeline.disarm_automatic_trigger()
    }

    pub fn automatic_trigger_status(&self) -> AutomaticTriggerStatus {
        self.pipeline.automatic_trigger_status()
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
        } else {
            self.can_workbench.take();
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

fn now_unix_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::{LogNookEngine, select_signals};
    use crate::{
        ConnectionCapabilities, DeviceDescriptor, EngineEvent, HardwareAdapter, LogNookError,
        Sample, SampleOrigin, SampleQuality, SignalDefinition, standard_obd_signals,
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
    fn research_test_requires_an_active_acquisition_adapter() {
        let mut engine = LogNookEngine::new(|_| {});
        let request =
            || crate::ResearchTestRequest::A30ReadTest(crate::A30ReadTest::Mode01EngineRpm);
        assert!(
            engine
                .execute_research_test(request())
                .unwrap_err()
                .to_string()
                .contains("No adapter is connected")
        );
        let disconnected = Arc::new(AtomicBool::new(false));
        engine
            .connect_adapter(
                Box::new(EmptyAdapter {
                    disconnected: disconnected.clone(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        assert!(
            engine
                .execute_research_test(request())
                .unwrap_err()
                .to_string()
                .contains("does not support read-only research tests")
        );
        assert!(engine.is_connected());
        assert!(!disconnected.load(Ordering::Acquire));
        engine.disconnect().unwrap();
        assert!(
            engine
                .execute_research_test(request())
                .unwrap_err()
                .to_string()
                .contains("No adapter is connected")
        );
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

    #[test]
    fn research_measurements_enter_the_shared_sample_event_pipeline() {
        let (events, received) = std::sync::mpsc::channel();
        let engine = LogNookEngine::new(move |event| {
            let _ = events.send(event);
        });
        engine
            .publish_research_sample(Sample {
                elapsed_ns: 0,
                wall_time_unix_ns: 123,
                source_id: "oem:test-adapter".into(),
                ecu_id: Some("7E8".into()),
                signal_id: "oem:test-candidate".into(),
                value: 123.0,
                unit: "raw".into(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: Some(vec![0x61, 0x01]),
            })
            .unwrap();

        let event = received
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let EngineEvent::Sample(sample) = event else {
            panic!("expected sample event, received {event:?}");
        };
        assert_eq!(sample.signal_id, "oem:test-candidate");
        assert!(sample.elapsed_ns > 0);
        assert_eq!(sample.origin, SampleOrigin::Live);
    }

    #[test]
    fn suspect_research_values_remain_graphable_without_validation_promotion() {
        let (events, received) = std::sync::mpsc::channel();
        let engine = LogNookEngine::new(move |event| {
            let _ = events.send(event);
        });
        engine
            .publish_research_sample(Sample {
                elapsed_ns: 0,
                wall_time_unix_ns: 456,
                source_id: "oem:test-adapter".into(),
                ecu_id: Some("7E8".into()),
                signal_id: "oem:outside-range".into(),
                value: 999.0,
                unit: "rpm".into(),
                quality: SampleQuality::Suspect,
                origin: SampleOrigin::Live,
                raw_bytes: Some(vec![0x61, 0x01]),
            })
            .unwrap();

        let EngineEvent::Sample(sample) = received
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected graphable suspect sample");
        };
        assert_eq!(sample.signal_id, "oem:outside-range");
        assert_eq!(sample.quality, SampleQuality::Suspect);
    }

    #[test]
    fn timestamped_publication_preserves_receive_time_and_monotonic_markers() {
        let (events, received) = std::sync::mpsc::channel();
        let engine = LogNookEngine::new(move |event| {
            let _ = events.send(event);
        });
        let path = std::env::temp_dir().join(format!(
            "lognook-can-timestamps-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        engine.pipeline.start_recording(path.clone()).unwrap();
        let sample = Sample {
            elapsed_ns: 9_000_000,
            wall_time_unix_ns: 123,
            source_id: "can:fixture".into(),
            ecu_id: Some("123".into()),
            signal_id: "can:field".into(),
            value: 7.0,
            unit: "raw".into(),
            quality: SampleQuality::Suspect,
            origin: SampleOrigin::Live,
            raw_bytes: Some(vec![7, 8]),
        };
        engine.publish_timestamped_sample(sample.clone()).unwrap();
        assert_eq!(
            received
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            EngineEvent::Sample(sample.clone())
        );
        let mut older = sample.clone();
        older.elapsed_ns = 2_000_000;
        engine
            .sample_publisher()
            .clone()
            .publish_timestamped_sample(older.clone())
            .unwrap();
        assert_eq!(
            received
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            EngineEvent::Sample(older.clone())
        );
        let marker = engine
            .pipeline
            .insert_marker("after older frame".into(), "".into())
            .unwrap();
        assert_eq!(marker.elapsed_ns, 9_000_000);
        engine.publish_research_sample(sample.clone()).unwrap();
        let _ = received
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap(); // marker
        let EngineEvent::Sample(research) = received
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected research sample")
        };
        assert_eq!(research.elapsed_ns, 10_000_000);
        let summary = engine.pipeline.stop_recording().unwrap();
        assert_eq!(summary.sample_count, 3);
        let recording = crate::read_recording(&path).unwrap();
        assert_eq!(recording.samples, vec![older, sample, research]);
        assert_eq!(recording.markers[0].elapsed_ns, 9_000_000);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn timestamped_publication_rejects_non_live_non_finite_and_negative_time() {
        let engine = LogNookEngine::new(|_| {});
        let sample = Sample {
            elapsed_ns: 1,
            wall_time_unix_ns: 1,
            source_id: "fixture".into(),
            ecu_id: None,
            signal_id: "can:field".into(),
            value: 1.0,
            unit: "raw".into(),
            quality: SampleQuality::Suspect,
            origin: SampleOrigin::Live,
            raw_bytes: Some(vec![1]),
        };
        let mut invalid = sample.clone();
        invalid.elapsed_ns = -1;
        assert!(engine.publish_timestamped_sample(invalid).is_err());
        let mut invalid = sample.clone();
        invalid.value = f64::NAN;
        assert!(
            engine
                .sample_publisher()
                .publish_timestamped_sample(invalid)
                .is_err()
        );
        let mut invalid = sample.clone();
        invalid.origin = SampleOrigin::Replay;
        assert!(engine.publish_timestamped_sample(invalid).is_err());
        let mut invalid = sample;
        invalid.quality = SampleQuality::ParseError;
        assert!(engine.publish_timestamped_sample(invalid).is_err());
    }
}
