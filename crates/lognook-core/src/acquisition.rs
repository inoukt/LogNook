use crate::{
    DiagnosticPhase, DiagnosticProgress, DiagnosticSink, HardwareAdapter, ResearchTestRequest,
    ResearchTestResult, SignalDefinition, model::LogNookError, pipeline::SampleSender,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_CONSECUTIVE_TRANSIENT_FAILURES: usize = 3;
const RETRY_BACKOFF: Duration = Duration::from_millis(250);

enum WorkerCommand {
    Stop,
    ResearchTest(
        ResearchTestRequest,
        mpsc::Sender<Result<ResearchTestResult, LogNookError>>,
    ),
}

pub(crate) struct AcquisitionHandle {
    commands: SyncSender<WorkerCommand>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
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
        running.store(true, Ordering::Release);
        let worker_running = running.clone();
        let worker = thread::spawn(move || {
            run(
                adapter.as_mut(),
                signals,
                output,
                command_rx,
                worker_running,
                diagnostic_sink,
                adapter_id,
            )
        });
        Self {
            commands,
            worker: Some(worker),
            running,
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

    pub(crate) fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
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
    let mut failed = false;
    let mut consecutive_transient_failures = 0;
    let mut next_poll = Instant::now();
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
            next_poll = cycle + interval;
        }
        if let Some(WorkerCommand::ResearchTest(request, reply)) = pending.take() {
            // Request errors belong to the caller. A subsequent poll detects link loss.
            let _ = reply.send(adapter.execute_research_test(request));
        } else {
            match commands.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
                Ok(command) => pending = Some(command),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    adapter.disconnect();
    running.store(false, Ordering::Release);
    if failed {
        let _ = output.send_disconnected();
    }
}

#[cfg(test)]
mod tests {
    use super::{AcquisitionHandle, WorkerCommand};
    use crate::{
        A30ReadOutcome, A30ReadTest, A30ReadTestResult, HardwareAdapter, ResearchTestRequest,
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
    }

    fn receive_sample(events: &mpsc::Receiver<EngineEvent>) -> Sample {
        let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
        let EngineEvent::Sample(sample) = event else {
            panic!("unexpected event: {event:?}")
        };
        sample
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
