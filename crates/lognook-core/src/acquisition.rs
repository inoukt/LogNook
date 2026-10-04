use crate::{HardwareAdapter, SignalDefinition, model::LogNookError, pipeline::SampleSender};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

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
    loop {
        let cycle = Instant::now();
        match adapter.read_samples(&signals) {
            Ok(samples) => {
                if samples
                    .into_iter()
                    .any(|sample| output.send_sample(sample).is_err())
                {
                    break;
                }
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
}
