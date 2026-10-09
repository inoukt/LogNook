use crate::{
    DiagnosticPhase, DiagnosticProgress, DiagnosticSink, HardwareAdapter, RawAdapterCommand,
    RawAdapterCommandResult, ResearchTestRequest, ResearchTestResult, SignalDefinition,
    model::{LogNookError, OemPollingStatus, Sample, SampleOrigin, SampleQuality},
    oem_catalog::{OemCandidateTrySpec, OemPollPlan},
    pipeline::SampleSender,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_CONSECUTIVE_TRANSIENT_FAILURES: usize = 3;
const RETRY_BACKOFF: Duration = Duration::from_millis(250);
const OEM_POLL_TARGET_INTERVAL: Duration = Duration::from_millis(50);
const OEM_POLL_MAX_BACKOFF: Duration = Duration::from_secs(1);

enum WorkerCommand {
    Stop,
    ResearchTest(
        ResearchTestRequest,
        mpsc::Sender<Result<ResearchTestResult, LogNookError>>,
    ),
    RawCommand(
        RawAdapterCommand,
        mpsc::Sender<Result<RawAdapterCommandResult, LogNookError>>,
    ),
    SetOemPollPlan(Option<OemPollPlan>, mpsc::Sender<Result<(), LogNookError>>),
}

pub(crate) struct AcquisitionHandle {
    commands: SyncSender<WorkerCommand>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    oem_status: Arc<Mutex<OemPollingStatus>>,
}

impl AcquisitionHandle {
    pub(crate) fn start(
        mut adapter: Box<dyn HardwareAdapter>,
        signals: Vec<SignalDefinition>,
        output: SampleSender,
        running: Arc<AtomicBool>,
        diagnostic_sink: DiagnosticSink,
        adapter_id: Option<String>,
    ) -> Self {
        let (commands, command_rx) = mpsc::sync_channel(1);
        let oem_status = Arc::new(Mutex::new(OemPollingStatus::default()));
        running.store(true, Ordering::Release);
        let worker_running = running.clone();
        let worker_oem_status = oem_status.clone();
        let worker = thread::spawn(move || {
            run(
                adapter.as_mut(),
                signals,
                output,
                command_rx,
                worker_running,
                diagnostic_sink,
                adapter_id,
                worker_oem_status,
            )
        });
        Self {
            commands,
            worker: Some(worker),
            running,
            oem_status,
        }
    }

    pub(crate) fn stop(&mut self) -> Result<(), LogNookError> {
        if let Some(worker) = self.worker.take() {
            let _ = self.commands.send(WorkerCommand::Stop);
            worker
                .join()
                .map_err(|_| LogNookError::new("Acquisition worker stopped unexpectedly"))?;
        }
        Ok(())
    }

    pub(crate) fn execute_research_test(
        &self,
        request: ResearchTestRequest,
    ) -> Result<ResearchTestResult, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(WorkerCommand::ResearchTest(request, reply))
            .map_err(|_| LogNookError::new("Acquisition worker has stopped"))?;
        response.recv().map_err(|_| {
            LogNookError::new("Acquisition worker did not complete the research test")
        })?
    }

    pub(crate) fn execute_raw_command(
        &self,
        request: RawAdapterCommand,
    ) -> Result<RawAdapterCommandResult, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(WorkerCommand::RawCommand(request, reply))
            .map_err(|_| LogNookError::new("Acquisition worker has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Acquisition worker did not complete the raw command"))?
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub(crate) fn set_oem_poll_plan(&self, plan: Option<OemPollPlan>) -> Result<(), LogNookError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(WorkerCommand::SetOemPollPlan(plan, reply))
            .map_err(|_| LogNookError::new("Acquisition worker has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Acquisition worker did not update OEM logging"))?
    }

    pub(crate) fn oem_polling_status(&self) -> OemPollingStatus {
        self.oem_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Drop for AcquisitionHandle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run(
    adapter: &mut dyn HardwareAdapter,
    signals: Vec<SignalDefinition>,
    output: SampleSender,
    commands: Receiver<WorkerCommand>,
    running: Arc<AtomicBool>,
    diagnostic_sink: DiagnosticSink,
    adapter_id: Option<String>,
    oem_status: Arc<Mutex<OemPollingStatus>>,
) {
    let signal_ids = signals
        .iter()
        .map(|signal| signal.id.clone())
        .collect::<Vec<_>>();
    let emit_poll = |phase, duration_ms, detail, transient, consecutive_failures| {
        diagnostic_sink(DiagnosticProgress {
            adapter_id: adapter_id.clone(),
            operation: "acquisition".into(),
            stage: "poll".into(),
            phase,
            duration_ms,
            signal_ids: signal_ids.clone(),
            detail,
            transient,
            consecutive_failures,
        });
    };
    let interval = adapter.sample_interval();
    let acquisition_started = Instant::now();
    let mut failed = false;
    let mut consecutive_transient_failures = 0;
    let mut next_poll = Instant::now();
    let mut oem_plan: Option<OemPollPlan> = None;
    let mut next_oem_poll = Instant::now();
    let mut oem_poll_interval = OEM_POLL_TARGET_INTERVAL;
    let mut next_oem_group = 0;
    let mut last_oem_success = None;
    let mut pending = None;
    loop {
        if pending.is_none() {
            match commands.try_recv() {
                Ok(command) => pending = Some(command),
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if matches!(pending, Some(WorkerCommand::Stop)) {
            break;
        }
        if pending
            .as_ref()
            .is_some_and(|command| matches!(command, WorkerCommand::SetOemPollPlan(_, _)))
        {
            let Some(WorkerCommand::SetOemPollPlan(plan, reply)) = pending.take() else {
                unreachable!("the pending command was checked above")
            };
            if let Some(plan) = &plan {
                if let Err(error) = plan
                    .groups
                    .iter()
                    .flatten()
                    .try_for_each(|spec| adapter.validate_oem_candidate_poll(spec))
                {
                    let _ = reply.send(Err(error));
                    continue;
                }
            }
            let mut status = oem_status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *status = plan
                .as_ref()
                .map(|plan| OemPollingStatus {
                    active: true,
                    vehicle_id: Some(plan.vehicle_id.clone()),
                    signal_count: plan.groups.iter().map(Vec::len).sum(),
                    ..OemPollingStatus::default()
                })
                .unwrap_or_default();
            drop(status);
            oem_plan = plan;
            next_oem_group = 0;
            last_oem_success = None;
            oem_poll_interval = OEM_POLL_TARGET_INTERVAL;
            next_oem_poll = Instant::now();
            let _ = reply.send(Ok(()));
            continue;
        }
        // A due poll precedes the pending research read. Serve only one poll here so
        // slow polls cannot perpetually starve that read by making the next poll due.
        if Instant::now() >= next_poll {
            let cycle = Instant::now();
            emit_poll(DiagnosticPhase::Started, None, None, None, None);
            let result = adapter.read_samples(&signals);
            let duration_ms = Some(cycle.elapsed().as_millis().min(u64::MAX as u128) as u64);
            match &result {
                Ok(_) => emit_poll(
                    DiagnosticPhase::Succeeded,
                    duration_ms,
                    None,
                    Some(false),
                    None,
                ),
                Err(error) => emit_poll(
                    DiagnosticPhase::Failed,
                    duration_ms,
                    Some(crate::diagnostics::diagnostic_error_detail(
                        &error.to_string(),
                    )),
                    Some(error.is_transient()),
                    Some(consecutive_transient_failures as u64 + 1),
                ),
            }
            match result {
                Ok(samples) => {
                    consecutive_transient_failures = 0;
                    if samples
                        .into_iter()
                        .any(|sample| output.send_sample(sample).is_err())
                    {
                        break;
                    }
                }
                Err(error) if error.is_transient() => {
                    consecutive_transient_failures += 1;
                    if consecutive_transient_failures < MAX_CONSECUTIVE_TRANSIENT_FAILURES {
                        next_poll = Instant::now() + RETRY_BACKOFF;
                        continue;
                    }
                    let _ = output.send_error(format!(
                        "Read failed after {consecutive_transient_failures} consecutive transient failures: {error}"
                    ));
                    failed = true;
                    break;
                }
                Err(error) => {
                    let _ = output.send_error(error.to_string());
                    failed = true;
                    break;
                }
            }
            next_poll = Instant::now() + interval;
        }
        if let Some(plan) = oem_plan.as_ref()
            && Instant::now() >= next_oem_poll
            && !plan.groups.is_empty()
        {
            let group = &plan.groups[next_oem_group % plan.groups.len()];
            let request_started = Instant::now();
            let Some(healthy) = poll_oem_group(
                adapter,
                group,
                &output,
                &diagnostic_sink,
                adapter_id.as_deref(),
                &plan.vehicle_id,
                acquisition_started,
                &oem_status,
                &mut last_oem_success,
            ) else {
                break;
            };
            oem_poll_interval = next_oem_poll_interval(oem_poll_interval, healthy);
            next_oem_group = (next_oem_group + 1) % plan.groups.len();
            next_oem_poll = request_started + oem_poll_interval;
            continue;
        }
        if let Some(command) = pending.take() {
            match command {
                WorkerCommand::ResearchTest(request, reply) => {
                    // Request errors belong to the caller. A subsequent poll detects link loss.
                    let _ = reply.send(adapter.execute_research_test(request));
                }
                WorkerCommand::RawCommand(request, reply) => {
                    let _ = reply.send(adapter.execute_raw_command(request));
                }
                WorkerCommand::Stop => break,
                WorkerCommand::SetOemPollPlan(_, _) => {
                    unreachable!("OEM poll plan commands are handled before adapter I/O")
                }
            }
        } else {
            let deadline = if oem_plan.is_some() {
                next_poll.min(next_oem_poll)
            } else {
                next_poll
            };
            match commands.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(command) => pending = Some(command),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    oem_status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .active = false;
    adapter.disconnect();
    running.store(false, Ordering::Release);
    if failed {
        let _ = output.send_disconnected();
    }
}

fn poll_oem_group(
    adapter: &mut dyn HardwareAdapter,
    group: &[OemCandidateTrySpec],
    output: &SampleSender,
    diagnostic_sink: &DiagnosticSink,
    adapter_id: Option<&str>,
    vehicle_id: &str,
    acquisition_started: Instant,
    status: &Mutex<OemPollingStatus>,
    last_success: &mut Option<Instant>,
) -> Option<bool> {
    let Some(first) = group.first() else {
        return Some(true);
    };
    let signal_ids = group
        .iter()
        .map(|spec| format!("oem:{}", spec.candidate.candidate_id))
        .collect::<Vec<_>>();
    diagnostic_sink(DiagnosticProgress {
        adapter_id: adapter_id.map(str::to_owned),
        operation: "acquisition".into(),
        stage: "oem_poll".into(),
        phase: DiagnosticPhase::Started,
        duration_ms: None,
        signal_ids: signal_ids.clone(),
        detail: None,
        transient: None,
        consecutive_failures: None,
    });
    let started = Instant::now();
    let result = adapter.execute_research_test(ResearchTestRequest::OemCandidate(first.clone()));
    let mut request_succeeded = false;
    let mut samples_published = 0;
    let mut failed_signals = 0;
    let mut last_error = None;
    let mut out_of_range = false;
    let mut latency_ms = None;
    let mut transient = None;
    match result {
        Ok(ResearchTestResult::OemCandidate(base)) => {
            latency_ms = Some(base.latency_ms);
            for spec in group {
                let mut candidate_result = base.clone();
                candidate_result.candidate_id = spec.candidate.candidate_id.clone();
                if base.outcome == crate::OemCandidateOutcome::Positive {
                    let decoded = crate::decode_oem_candidate_payload(
                        spec,
                        base.responder_header.as_deref(),
                        &base.response_payload,
                    );
                    candidate_result.outcome = decoded.outcome;
                    candidate_result.responder_header = decoded.responder_header;
                    candidate_result.response_payload = decoded.payload;
                    candidate_result.negative_response_code = decoded.negative_response_code;
                    candidate_result.decoded_value = decoded.decoded_value;
                    candidate_result.decode_error = decoded.decode_error;
                    candidate_result.error = decoded.error;
                }
                let Some(decoded) = candidate_result.decoded_value.as_ref() else {
                    last_error = Some(candidate_error(&candidate_result));
                    failed_signals += 1;
                    continue;
                };
                let Some(value) = decoded.value.filter(|value| value.is_finite()) else {
                    last_error = Some(candidate_error(&candidate_result));
                    failed_signals += 1;
                    continue;
                };
                out_of_range |= decoded.within_catalog_range == Some(false);
                let wall_time_unix_ns = base
                    .started_at_unix_ms
                    .saturating_mul(1_000_000)
                    .min(i64::MAX as u64) as i64;
                let sample = Sample {
                    elapsed_ns: acquisition_started
                        .elapsed()
                        .as_nanos()
                        .min(i64::MAX as u128) as i64,
                    wall_time_unix_ns,
                    source_id: format!(
                        "oem:{}:{}:{}@{}:{}{}:{}>{}",
                        vehicle_id,
                        base.adapter.stable_id,
                        spec.candidate.source_id,
                        spec.candidate.source_blob_sha,
                        spec.candidate.service,
                        spec.candidate.identifier,
                        spec.candidate.request_header,
                        spec.candidate.response_header,
                    ),
                    ecu_id: candidate_result.responder_header.clone(),
                    signal_id: format!("oem:{}", spec.candidate.candidate_id),
                    value,
                    unit: spec.candidate.unit.clone(),
                    quality: if decoded.within_catalog_range == Some(false) {
                        SampleQuality::Suspect
                    } else {
                        SampleQuality::Valid
                    },
                    origin: SampleOrigin::Live,
                    raw_bytes: Some(candidate_result.response_payload.clone()),
                };
                if output.send_sample(sample).is_err() {
                    return None;
                }
                samples_published += 1;
                request_succeeded = true;
            }
            transient = Some(matches!(
                base.outcome,
                crate::OemCandidateOutcome::NoResponse
                    | crate::OemCandidateOutcome::TemporarilyUnavailable
                    | crate::OemCandidateOutcome::ResponsePending
            ));
            if !request_succeeded && last_error.is_none() {
                last_error = Some(candidate_error(&base));
            }
        }
        Ok(_) => {
            last_error = Some("Adapter returned an unexpected OEM result type".into());
            failed_signals = group.len() as u64;
        }
        Err(error) => {
            transient = Some(error.is_transient());
            last_error = Some(crate::diagnostics::diagnostic_error_detail(
                &error.to_string(),
            ));
            failed_signals = group.len() as u64;
        }
    }
    let completed_at = Instant::now();
    let healthy = request_succeeded && failed_signals == 0 && !out_of_range;
    {
        let mut snapshot = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        snapshot.request_count += 1;
        snapshot.last_latency_ms =
            latency_ms.or_else(|| Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64));
        snapshot.samples_published += samples_published;
        snapshot.failed_signals += failed_signals;
        if request_succeeded {
            snapshot.successful_requests += 1;
            snapshot.observed_request_rate_hz = last_success.map(|previous: Instant| {
                1.0 / completed_at
                    .duration_since(previous)
                    .as_secs_f64()
                    .max(f64::EPSILON)
            });
            *last_success = Some(completed_at);
        } else {
            snapshot.failed_requests += 1;
        }
        snapshot.last_error = last_error.clone();
    }
    diagnostic_sink(DiagnosticProgress {
        adapter_id: adapter_id.map(str::to_owned),
        operation: "acquisition".into(),
        stage: "oem_poll".into(),
        phase: if request_succeeded {
            DiagnosticPhase::Succeeded
        } else {
            DiagnosticPhase::Failed
        },
        duration_ms: Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
        signal_ids,
        detail: last_error,
        transient,
        consecutive_failures: None,
    });
    Some(healthy)
}

fn next_oem_poll_interval(current: Duration, healthy: bool) -> Duration {
    if healthy {
        (current / 2).max(OEM_POLL_TARGET_INTERVAL)
    } else {
        current.saturating_mul(2).min(OEM_POLL_MAX_BACKOFF)
    }
}

fn candidate_error(result: &crate::OemCandidateTryResult) -> String {
    result
        .error
        .as_deref()
        .or(result.decode_error.as_deref())
        .map(crate::diagnostics::diagnostic_error_detail)
        .unwrap_or_else(|| format!("OEM read outcome: {:?}", result.outcome))
}

#[cfg(test)]
mod tests {
    use super::{
        AcquisitionHandle, OEM_POLL_TARGET_INTERVAL, WorkerCommand, next_oem_poll_interval,
    };
    use crate::{
        A30ReadOutcome, A30ReadTest, A30ReadTestResult, HardwareAdapter, OemCandidate,
        OemCandidateDecodedValue, OemCandidateOutcome, OemCandidateTryResult, OemCandidateTrySpec,
        OemPollPlan, RawAdapterCommand, RawAdapterCommandResult, ResearchTestRequest,
        ResearchTestResult, SampleOrigin, SampleQuality,
        model::{
            ConnectionCapabilities, DeviceDescriptor, EngineEvent, LogNookError, Sample,
            SignalDefinition,
        },
        pipeline::SamplePipeline,
    };
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread::{self, ThreadId},
        time::Duration,
    };

    #[derive(Default)]
    struct AdapterOperations {
        calls: Mutex<Vec<(&'static str, ThreadId)>>,
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    impl AdapterOperations {
        fn start(&self, operation: &'static str) {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            self.calls
                .lock()
                .unwrap()
                .push((operation, thread::current().id()));
        }

        fn finish(&self) {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    struct ResearchAdapter {
        operations: Arc<AdapterOperations>,
        research_started: mpsc::Sender<()>,
        release_research: mpsc::Receiver<()>,
        fail_next_research: bool,
        poll_delay: Duration,
        polls: i64,
    }

    impl HardwareAdapter for ResearchAdapter {
        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            panic!("research must reuse the adapter already owned by acquisition")
        }

        fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
            self.operations.start("poll");
            thread::sleep(self.poll_delay);
            self.polls += 1;
            let sample = Sample {
                elapsed_ns: self.polls * 100_000_000,
                wall_time_unix_ns: self.polls,
                source_id: "active-adapter".into(),
                ecu_id: Some("7E8".into()),
                signal_id: "engine_rpm".into(),
                value: self.polls as f64,
                unit: "rpm".into(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            };
            self.operations.finish();
            Ok(vec![sample])
        }

        fn execute_research_test(
            &mut self,
            request: ResearchTestRequest,
        ) -> Result<ResearchTestResult, LogNookError> {
            let ResearchTestRequest::A30ReadTest(test) = request else {
                panic!("unexpected research request")
            };
            assert_eq!(test, A30ReadTest::Mode01EngineRpm);
            self.operations.start("research");
            self.research_started.send(()).unwrap();
            self.release_research
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            self.operations.finish();
            if std::mem::take(&mut self.fail_next_research) {
                return Err(LogNookError::new("research transport error"));
            }
            Ok(ResearchTestResult::A30ReadTest(A30ReadTestResult {
                test,
                request_description: "Mode 01 RPM".into(),
                exact_adapter_command: "010C".into(),
                adapter: DeviceDescriptor {
                    stable_id: "active-adapter".into(),
                    hardware_family: "fixture".into(),
                    firmware: None,
                    path: None,
                },
                protocol: "fixture".into(),
                started_at_unix_ms: 1,
                latency_ms: 1,
                outcome: A30ReadOutcome::NoResponse,
                responder_header: None,
                response_payloads: Vec::new(),
                negative_response_code: None,
                graph_values: Vec::new(),
                detail: None,
                raw_response: String::new(),
                evidence_saved: false,
                evidence_save_error: None,
            }))
        }

        fn execute_raw_command(
            &mut self,
            command: RawAdapterCommand,
        ) -> Result<RawAdapterCommandResult, LogNookError> {
            let command = command.command_text()?;
            self.operations.start("raw");
            self.operations.finish();
            Ok(RawAdapterCommandResult {
                command,
                response: "fixture response".into(),
                duration_ms: 1,
            })
        }
    }

    fn receive_sample(events: &mpsc::Receiver<EngineEvent>) -> Sample {
        let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
        let EngineEvent::Sample(sample) = event else {
            panic!("unexpected event: {event:?}")
        };
        sample
    }

    #[test]
    fn oem_poll_pacing_targets_twenty_hz_and_backs_off_then_recovers() {
        let target = OEM_POLL_TARGET_INTERVAL;
        assert_eq!(target, Duration::from_millis(50));
        assert_eq!(next_oem_poll_interval(target, true), target);
        assert_eq!(
            next_oem_poll_interval(target, false),
            Duration::from_millis(100)
        );
        assert_eq!(
            next_oem_poll_interval(Duration::from_millis(100), false),
            Duration::from_millis(200)
        );
        assert_eq!(
            next_oem_poll_interval(Duration::from_secs(1), false),
            Duration::from_secs(1)
        );
        assert_eq!(
            next_oem_poll_interval(Duration::from_secs(1), true),
            Duration::from_millis(500)
        );
        assert_eq!(
            next_oem_poll_interval(Duration::from_millis(75), true),
            target
        );
    }

    #[test]
    fn raw_command_runs_on_the_existing_serialized_acquisition_owner() {
        let (events, _event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let operations = Arc::new(AdapterOperations::default());
        let (research_started, _started_rx) = mpsc::channel();
        let (_release, release_research) = mpsc::channel();
        let mut acquisition = AcquisitionHandle::start(
            Box::new(ResearchAdapter {
                operations: operations.clone(),
                research_started,
                release_research,
                fail_next_research: false,
                poll_delay: Duration::ZERO,
                polls: 0,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            Some("fixture-adapter".into()),
        );

        let result = acquisition
            .execute_raw_command(RawAdapterCommand::AdapterTerminal("ATZ".into()))
            .unwrap();
        assert_eq!(result.command, "ATZ");
        assert!(
            operations
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|(operation, _)| *operation == "raw")
        );
        assert_eq!(operations.max_active.load(Ordering::SeqCst), 1);
        acquisition.stop().unwrap();
    }

    struct OemPollingAdapter {
        requests: Arc<AtomicUsize>,
        polls: Arc<AtomicUsize>,
        fail_oem: bool,
        poll_delay: Duration,
    }

    impl HardwareAdapter for OemPollingAdapter {
        fn validate_oem_candidate_poll(
            &self,
            spec: &OemCandidateTrySpec,
        ) -> Result<(), LogNookError> {
            crate::build_oem_candidate_command(spec).map(|_| ())
        }

        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            unreachable!("the acquisition worker receives an already-connected adapter")
        }

        fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
            thread::sleep(self.poll_delay);
            self.polls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        fn execute_research_test(
            &mut self,
            request: ResearchTestRequest,
        ) -> Result<ResearchTestResult, LogNookError> {
            let ResearchTestRequest::OemCandidate(spec) = request else {
                unreachable!("only OEM polling is scheduled in this test")
            };
            self.requests.fetch_add(1, Ordering::SeqCst);
            if self.fail_oem {
                return Err(LogNookError::transient("OEM fixture timeout"));
            }
            Ok(ResearchTestResult::OemCandidate(OemCandidateTryResult {
                candidate_id: spec.candidate.candidate_id,
                exact_adapter_command: "fixture read".into(),
                adapter: DeviceDescriptor {
                    stable_id: "fixture-adapter".into(),
                    hardware_family: "fixture".into(),
                    firmware: None,
                    path: None,
                },
                protocol: "fixture CAN".into(),
                started_at_unix_ms: 1_000,
                latency_ms: 12,
                outcome: OemCandidateOutcome::Positive,
                raw_response: "fixture response".into(),
                responder_header: Some("7E8".into()),
                response_payload: vec![0x61, 0x01, 0x10, 0x20],
                negative_response_code: None,
                decoded_value: Some(OemCandidateDecodedValue {
                    raw_value: "16".into(),
                    value: Some(16.0),
                    enum_value: None,
                    unit: "rpm".into(),
                    formula: "raw".into(),
                    within_catalog_range: Some(true),
                }),
                decode_error: None,
                error: None,
                evidence_saved: false,
                evidence_save_error: None,
            }))
        }
    }

    fn polling_candidate(candidate_id: &str, bit_offset: u32) -> OemCandidateTrySpec {
        OemCandidateTrySpec {
            candidate: OemCandidate {
                candidate_id: candidate_id.into(),
                source_id: "fixture-source".into(),
                family: "Toyota".into(),
                brand: "Scion".into(),
                model_scope: "xB".into(),
                service: "21".into(),
                identifier: "01".into(),
                request_header: "7E0".into(),
                response_header: "7E8".into(),
                frequency_hz: Some(1.0),
                module_type: "Engine".into(),
                category: "Engine".into(),
                name: candidate_id.into(),
                signal_source_id: "fixture-2101".into(),
                bit_offset: Some(bit_offset),
                bit_length: Some(8),
                formula: "raw".into(),
                decoder_json: format!("{{\"bix\":{bit_offset},\"len\":8,\"unit\":\"rpm\"}}"),
                unit: "rpm".into(),
                year_filter_json: "{}".into(),
                ecu_filter: "7E0 → 7E8".into(),
                source_blob_sha: "fixture-sha".into(),
                research_state: "research".into(),
                vehicle_validation: "ecu_variant_verified".into(),
                auto_request: true,
                confidence: "verified".into(),
                notes: String::new(),
                suggested_metric: "rpm".into(),
                source_scope: "fixture".into(),
                applicability_specificity: 100,
                min_value: Some(0.0),
                max_value: Some(255.0),
                source_url: Some("https://fixture.test/source".into()),
                license: Some("fixture".into()),
                license_url: Some("https://fixture.test/license".into()),
            },
            value_format: "uint".into(),
        }
    }

    #[test]
    fn continuous_oem_poll_shares_one_request_and_publishes_each_channel() {
        let (events, event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let recording_path =
            std::env::temp_dir().join(format!("lognook-oem-poll-{}.lognook", std::process::id()));
        let _ = std::fs::remove_file(&recording_path);
        pipeline.start_recording(recording_path.clone()).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(OemPollingAdapter {
                requests: requests.clone(),
                polls: Arc::new(AtomicUsize::new(0)),
                fail_oem: false,
                poll_delay: Duration::ZERO,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            Some("fixture-adapter".into()),
        );
        acquisition
            .set_oem_poll_plan(Some(OemPollPlan {
                vehicle_id: "scion-2014".into(),
                groups: vec![vec![
                    polling_candidate("scion-rpm", 0),
                    polling_candidate("scion-temp", 8),
                ]],
            }))
            .unwrap();

        let first = receive_sample(&event_rx);
        let second = receive_sample(&event_rx);
        assert_eq!(first.value, 16.0);
        assert_eq!(second.value, 32.0);
        assert!(first.source_id.contains("scion-2014"));
        assert_eq!(first.source_id, second.source_id);
        assert_eq!(first.raw_bytes, second.raw_bytes);
        assert_ne!(first.signal_id, second.signal_id);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let status = acquisition.oem_polling_status();
        assert!(status.active);
        assert_eq!(status.signal_count, 2);
        assert_eq!(status.request_count, 1);
        assert_eq!(status.successful_requests, 1);
        assert_eq!(status.samples_published, 2);
        acquisition.stop().unwrap();
        assert!(!acquisition.oem_polling_status().active);
        let summary = pipeline.stop_recording().unwrap();
        let recording = crate::read_recording(&summary.path).unwrap();
        assert_eq!(recording.samples.len(), 2);
        assert_eq!(
            recording.samples[0].raw_bytes,
            Some(vec![0x61, 0x01, 0x10, 0x20])
        );
        let _ = std::fs::remove_file(recording_path);
    }

    #[test]
    fn oem_polling_uses_the_capture_speed_instead_of_a_fixed_one_second_delay() {
        let (events, _event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let requests = Arc::new(AtomicUsize::new(0));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(OemPollingAdapter {
                requests: requests.clone(),
                polls: Arc::new(AtomicUsize::new(0)),
                fail_oem: false,
                poll_delay: Duration::ZERO,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            Some("fixture-adapter".into()),
        );
        acquisition
            .set_oem_poll_plan(Some(OemPollPlan {
                vehicle_id: "scion-2014".into(),
                groups: vec![vec![polling_candidate("scion-rpm", 0)]],
            }))
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_millis(700);
        while requests.load(Ordering::SeqCst) < 4 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert!(requests.load(Ordering::SeqCst) >= 4);
        assert!(
            acquisition
                .oem_polling_status()
                .observed_request_rate_hz
                .is_some_and(|rate| rate >= 5.0)
        );
        acquisition.stop().unwrap();
    }

    #[test]
    fn fast_oem_poll_runs_between_slow_live_sample_cycles() {
        let (events, _event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let requests = Arc::new(AtomicUsize::new(0));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(OemPollingAdapter {
                requests: requests.clone(),
                polls: Arc::new(AtomicUsize::new(0)),
                fail_oem: false,
                poll_delay: Duration::from_millis(120),
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            Some("fixture-adapter".into()),
        );
        acquisition
            .set_oem_poll_plan(Some(OemPollPlan {
                vehicle_id: "scion-2014".into(),
                groups: vec![vec![polling_candidate("scion-rpm", 0)]],
            }))
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while requests.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert!(requests.load(Ordering::SeqCst) > 0);
        acquisition.stop().unwrap();
    }

    #[test]
    fn continuous_oem_request_errors_do_not_stop_standard_polling() {
        let (events, event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let requests = Arc::new(AtomicUsize::new(0));
        let polls = Arc::new(AtomicUsize::new(0));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(OemPollingAdapter {
                requests: requests.clone(),
                polls: polls.clone(),
                fail_oem: true,
                poll_delay: Duration::ZERO,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            Some("fixture-adapter".into()),
        );
        acquisition
            .set_oem_poll_plan(Some(OemPollPlan {
                vehicle_id: "scion-2014".into(),
                groups: vec![vec![polling_candidate("scion-rpm", 0)]],
            }))
            .unwrap();

        let until = std::time::Instant::now() + Duration::from_secs(2);
        while acquisition.oem_polling_status().failed_requests == 0
            && std::time::Instant::now() < until
        {
            thread::sleep(Duration::from_millis(5));
        }
        let polls_after_error = polls.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(180));
        assert!(requests.load(Ordering::SeqCst) >= 1);
        assert!(polls.load(Ordering::SeqCst) > polls_after_error);
        assert!(acquisition.is_running());
        assert!(acquisition.oem_polling_status().failed_requests >= 1);
        assert!(
            event_rx
                .try_iter()
                .all(|event| matches!(event, EngineEvent::Sample(_)))
        );
        acquisition.stop().unwrap();
    }

    #[test]
    fn unsupported_adapter_rejects_oem_plan_before_transmitting() {
        let (events, _event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let (research_started, _started_rx) = mpsc::channel();
        let (_release, release_research) = mpsc::channel();
        let mut acquisition = AcquisitionHandle::start(
            Box::new(ResearchAdapter {
                operations: Arc::new(AdapterOperations::default()),
                research_started,
                release_research,
                fail_next_research: false,
                poll_delay: Duration::ZERO,
                polls: 0,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            Some("fixture-adapter".into()),
        );
        let error = acquisition
            .set_oem_poll_plan(Some(OemPollPlan {
                vehicle_id: "scion-2014".into(),
                groups: vec![vec![polling_candidate("scion-rpm", 0)]],
            }))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not support continuous OEM")
        );
        assert_eq!(acquisition.oem_polling_status().request_count, 0);
        acquisition.stop().unwrap();
    }

    struct SlowPollingAdapter(ResearchAdapter);

    impl HardwareAdapter for SlowPollingAdapter {
        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            self.0.connect()
        }

        fn read_samples(
            &mut self,
            signals: &[SignalDefinition],
        ) -> Result<Vec<Sample>, LogNookError> {
            // Bound the test even if continuously overdue polls starve the pending request.
            if self.0.polls >= 3 {
                return Err(LogNookError::new("slow adapter fixture exhausted"));
            }
            self.0.read_samples(signals)
        }

        fn execute_research_test(
            &mut self,
            request: ResearchTestRequest,
        ) -> Result<ResearchTestResult, LogNookError> {
            self.0.execute_research_test(request)
        }
    }

    #[test]
    fn research_request_progresses_when_live_poll_exceeds_interval() {
        let (events, event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let operations = Arc::new(AdapterOperations::default());
        let (started, _started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let mut acquisition = AcquisitionHandle::start(
            Box::new(SlowPollingAdapter(ResearchAdapter {
                operations: operations.clone(),
                research_started: started,
                release_research: release_rx,
                fail_next_research: false,
                poll_delay: Duration::from_millis(120),
                polls: 0,
            })),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            None,
        );
        assert_eq!(receive_sample(&event_rx).value, 1.0);
        release.send(()).unwrap();
        acquisition
            .execute_research_test(ResearchTestRequest::A30ReadTest(
                A30ReadTest::Mode01EngineRpm,
            ))
            .unwrap();
        assert_eq!(receive_sample(&event_rx).value, 2.0);
        acquisition.stop().unwrap();
        let calls = operations.calls.lock().unwrap();
        let research_index = calls
            .iter()
            .position(|(operation, _)| *operation == "research")
            .unwrap();
        assert_eq!(calls[research_index - 1].0, "poll");
        assert_eq!(operations.max_active.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn research_request_runs_on_acquisition_owner() {
        let (events, event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let operations = Arc::new(AdapterOperations::default());
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let mut acquisition = AcquisitionHandle::start(
            Box::new(ResearchAdapter {
                operations: operations.clone(),
                research_started: started,
                release_research: release_rx,
                fail_next_research: false,
                poll_delay: Duration::ZERO,
                polls: 0,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            None,
        );
        assert_eq!(receive_sample(&event_rx).value, 1.0);
        thread::scope(|scope| {
            let request = scope.spawn(|| {
                acquisition.execute_research_test(ResearchTestRequest::A30ReadTest(
                    A30ReadTest::Mode01EngineRpm,
                ))
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            release.send(()).unwrap();
            let ResearchTestResult::A30ReadTest(result) = request.join().unwrap().unwrap() else {
                panic!("wrong result variant")
            };
            assert_eq!(result.adapter.stable_id, "active-adapter");
            assert_eq!(result.outcome, A30ReadOutcome::NoResponse);
        });
        assert_eq!(receive_sample(&event_rx).value, 2.0);
        acquisition.stop().unwrap();
        let calls = operations.calls.lock().unwrap();
        let owner = calls[0].1;
        assert_ne!(owner, thread::current().id());
        assert!(calls.iter().all(|(_, thread)| *thread == owner));
        assert_eq!(operations.max_active.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn research_request_waits_for_live_poll() {
        let (events, event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let operations = Arc::new(AdapterOperations::default());
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let mut acquisition = AcquisitionHandle::start(
            Box::new(ResearchAdapter {
                operations: operations.clone(),
                research_started: started,
                release_research: release_rx,
                fail_next_research: false,
                poll_delay: Duration::ZERO,
                polls: 0,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            None,
        );
        assert_eq!(receive_sample(&event_rx).value, 1.0);
        thread::scope(|scope| {
            let first = scope.spawn(|| {
                acquisition.execute_research_test(ResearchTestRequest::A30ReadTest(
                    A30ReadTest::Mode01EngineRpm,
                ))
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            // Queue the second request before making the in-flight request exceed the poll interval.
            let (reply, response) = mpsc::channel();
            acquisition
                .commands
                .send(WorkerCommand::ResearchTest(
                    ResearchTestRequest::A30ReadTest(A30ReadTest::Mode01EngineRpm),
                    reply,
                ))
                .unwrap();
            thread::sleep(Duration::from_millis(120));
            release.send(()).unwrap();
            first.join().unwrap().unwrap();
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let calls = operations.calls.lock().unwrap();
            let research_indices = calls
                .iter()
                .enumerate()
                .filter_map(|(index, (operation, _))| (*operation == "research").then_some(index))
                .collect::<Vec<_>>();
            assert_eq!(research_indices.len(), 2);
            assert!(
                calls[research_indices[0] + 1..research_indices[1]]
                    .iter()
                    .any(|(operation, _)| *operation == "poll")
            );
            drop(calls);
            release.send(()).unwrap();
            response
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
        });
        assert_eq!(receive_sample(&event_rx).value, 2.0);
        acquisition.stop().unwrap();
        assert_eq!(operations.max_active.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn research_request_error_returns_without_overlapping_poll() {
        let (events, event_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let operations = Arc::new(AdapterOperations::default());
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let mut acquisition = AcquisitionHandle::start(
            Box::new(ResearchAdapter {
                operations: operations.clone(),
                research_started: started,
                release_research: release_rx,
                fail_next_research: true,
                poll_delay: Duration::ZERO,
                polls: 0,
            }),
            Vec::new(),
            pipeline.sender(),
            Arc::new(AtomicBool::new(false)),
            crate::diagnostics::noop_diagnostic_sink(),
            None,
        );
        assert_eq!(receive_sample(&event_rx).value, 1.0);
        thread::scope(|scope| {
            let request = scope.spawn(|| {
                acquisition.execute_research_test(ResearchTestRequest::A30ReadTest(
                    A30ReadTest::Mode01EngineRpm,
                ))
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            thread::sleep(Duration::from_millis(120));
            release.send(()).unwrap();
            assert_eq!(
                request.join().unwrap().unwrap_err().to_string(),
                "research transport error"
            );
        });
        assert_eq!(receive_sample(&event_rx).value, 2.0);
        assert!(acquisition.is_running());
        thread::scope(|scope| {
            let next = scope.spawn(|| {
                acquisition.execute_research_test(ResearchTestRequest::A30ReadTest(
                    A30ReadTest::Mode01EngineRpm,
                ))
            });
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            release.send(()).unwrap();
            next.join().unwrap().unwrap();
        });
        assert_eq!(receive_sample(&event_rx).value, 3.0);
        acquisition.stop().unwrap();
        assert_eq!(operations.max_active.load(Ordering::SeqCst), 1);
        assert!(
            event_rx
                .try_iter()
                .all(|event| matches!(event, EngineEvent::Sample(_)))
        );
    }

    struct TimedOutAdapter;

    impl HardwareAdapter for TimedOutAdapter {
        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            Ok(ConnectionCapabilities {
                device: DeviceDescriptor {
                    stable_id: "test-timeout".into(),
                    hardware_family: "test".into(),
                    firmware: None,
                    path: None,
                },
                protocol: "test".into(),
                read_only: true,
                supported_signals: Vec::new(),
            })
        }

        fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
            Err(LogNookError::new("response timed out"))
        }
    }

    struct TransientAdapter {
        calls: mpsc::Sender<usize>,
        call_count: usize,
        failures_before_success: usize,
        disconnected: Arc<AtomicBool>,
    }

    impl HardwareAdapter for TransientAdapter {
        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            TimedOutAdapter.connect()
        }

        fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
            self.call_count += 1;
            let _ = self.calls.send(self.call_count);
            if self.call_count <= self.failures_before_success {
                return Err(LogNookError::transient("adapter response timed out"));
            }
            Ok(Vec::new())
        }

        fn disconnect(&mut self) {
            self.disconnected
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }

    #[test]
    fn adapter_read_failure_reports_disconnected_and_clears_running_state() {
        let (tx, rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = tx.send(event);
        }));
        let running = Arc::new(AtomicBool::new(true));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(TimedOutAdapter),
            Vec::new(),
            pipeline.sender(),
            running.clone(),
            crate::diagnostics::noop_diagnostic_sink(),
            None,
        );
        let error = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let disconnected = rx.recv_timeout(Duration::from_secs(1)).unwrap();

        assert!(matches!(error, EngineEvent::Error(_)));
        assert_eq!(disconnected, EngineEvent::Disconnected);
        assert!(!running.load(std::sync::atomic::Ordering::Relaxed));
        acquisition.stop().unwrap();
    }

    #[test]
    fn transient_read_timeout_retries_and_keeps_the_session_connected() {
        let (calls, call_rx) = mpsc::channel();
        let (events, event_rx) = mpsc::channel();
        let (diagnostics, diagnostic_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let running = Arc::new(AtomicBool::new(false));
        let disconnected = Arc::new(AtomicBool::new(false));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(TransientAdapter {
                calls,
                call_count: 0,
                failures_before_success: 1,
                disconnected: disconnected.clone(),
            }),
            Vec::new(),
            pipeline.sender(),
            running.clone(),
            Arc::new(move |event| {
                let _ = diagnostics.send(event);
            }),
            None,
        );

        assert_eq!(call_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(call_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
        for phase in [
            crate::DiagnosticPhase::Started,
            crate::DiagnosticPhase::Failed,
            crate::DiagnosticPhase::Started,
            crate::DiagnosticPhase::Succeeded,
        ] {
            let event = diagnostic_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(event.phase, phase);
            if phase == crate::DiagnosticPhase::Failed {
                assert_eq!(event.transient, Some(true));
                assert_eq!(event.detail.as_deref(), Some("adapter response timed out"));
                assert!(event.duration_ms.is_some());
            }
        }
        assert!(acquisition.is_running());
        assert!(!disconnected.load(std::sync::atomic::Ordering::Acquire));
        assert!(matches!(
            event_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        acquisition.stop().unwrap();
        assert!(!acquisition.is_running());
        assert!(disconnected.load(std::sync::atomic::Ordering::Acquire));
        assert!(matches!(
            event_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn three_consecutive_transient_read_timeouts_end_the_session() {
        let (calls, call_rx) = mpsc::channel();
        let (events, event_rx) = mpsc::channel();
        let (diagnostic_tx, diagnostic_rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = events.send(event);
        }));
        let running = Arc::new(AtomicBool::new(false));
        let disconnected = Arc::new(AtomicBool::new(false));
        let mut acquisition = AcquisitionHandle::start(
            Box::new(TransientAdapter {
                calls,
                call_count: 0,
                failures_before_success: usize::MAX,
                disconnected: disconnected.clone(),
            }),
            Vec::new(),
            pipeline.sender(),
            running.clone(),
            Arc::new(move |event| {
                let _ = diagnostic_tx.send(event);
            }),
            None,
        );

        for expected_call in 1..=3 {
            assert_eq!(
                call_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                expected_call
            );
        }
        for expected in 1..=3 {
            let start = diagnostic_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(start.phase, crate::DiagnosticPhase::Started);
            let failure = diagnostic_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(failure.phase, crate::DiagnosticPhase::Failed);
            assert_eq!(
                serde_json::to_value(failure).unwrap()["consecutive_failures"],
                expected
            );
        }
        let error = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(error, EngineEvent::Error(message) if message.contains("3 consecutive transient failures"))
        );
        assert_eq!(
            event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            EngineEvent::Disconnected
        );
        assert!(!acquisition.is_running());
        assert!(disconnected.load(std::sync::atomic::Ordering::Acquire));

        acquisition.stop().unwrap();
    }
}
