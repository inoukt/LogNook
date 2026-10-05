use crate::{HardwareAdapter, SignalDefinition, model::LogNookError, pipeline::SampleSender};
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

pub(crate) struct AcquisitionHandle {
    stop: SyncSender<()>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl AcquisitionHandle {
    pub(crate) fn start(
        mut adapter: Box<dyn HardwareAdapter>,
        signals: Vec<SignalDefinition>,
        output: SampleSender,
        running: Arc<AtomicBool>,
    ) -> Self {
        let (stop, stop_rx) = mpsc::sync_channel(1);
        running.store(true, Ordering::Release);
        let worker_running = running.clone();
        let worker =
            thread::spawn(move || run(adapter.as_mut(), signals, output, stop_rx, worker_running));
        Self {
            stop,
            worker: Some(worker),
            running,
        }
    }

    pub(crate) fn stop(&mut self) -> Result<(), LogNookError> {
        let _ = self.stop.try_send(());
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| LogNookError::new("Acquisition worker stopped unexpectedly"))?;
        }
        Ok(())
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
    stop: Receiver<()>,
    running: Arc<AtomicBool>,
) {
    let interval = adapter.sample_interval();
    let mut failed = false;
    let mut consecutive_transient_failures = 0;
    loop {
        let cycle = Instant::now();
        match adapter.read_samples(&signals) {
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
                    match stop.recv_timeout(RETRY_BACKOFF) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    }
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
        match stop.recv_timeout(interval.saturating_sub(cycle.elapsed())) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
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
    use super::AcquisitionHandle;
    use crate::{
        HardwareAdapter,
        model::{
            ConnectionCapabilities, DeviceDescriptor, EngineEvent, LogNookError, Sample,
            SignalDefinition,
        },
        pipeline::SamplePipeline,
    };
    use std::{
        sync::{Arc, atomic::AtomicBool, mpsc},
        time::Duration,
    };

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
        );

        assert_eq!(call_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(call_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
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
        );

        for expected_call in 1..=3 {
            assert_eq!(
                call_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                expected_call
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
