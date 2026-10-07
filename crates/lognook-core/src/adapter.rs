use crate::model::{ConnectionCapabilities, LogNookError, Sample, SignalDefinition};
use crate::{DiagnosticSink, ResearchTestRequest, ResearchTestResult};
use std::time::Duration;

/// One serialized command owner for a physical or simulated diagnostic interface.
pub trait HardwareAdapter: Send + 'static {
    /// Install an observational route before connecting. Adapters without diagnostics ignore it.
    fn set_diagnostic_sink(&mut self, _sink: DiagnosticSink) {}
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError>;
    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError>;
    fn execute_research_test(
        &mut self,
        _request: ResearchTestRequest,
    ) -> Result<ResearchTestResult, LogNookError> {
        Err(LogNookError::new(
            "This adapter does not support read-only research tests",
        ))
    }
    /// Target delay between read cycles; slow adapter responses naturally reduce the actual rate.
    fn sample_interval(&self) -> Duration {
        Duration::from_millis(100)
    }
    fn disconnect(&mut self) {}
}

pub(crate) fn collect_samples_independently<T>(
    items: &[T],
    mut read: impl FnMut(&T) -> Result<Vec<Sample>, LogNookError>,
) -> Result<Vec<Sample>, LogNookError> {
    let mut samples = Vec::new();
    for item in items {
        match read(item) {
            Ok(mut read_samples) => samples.append(&mut read_samples),
            Err(error) if error.is_signal() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::collect_samples_independently;
    use crate::{LogNookError, Sample, SampleOrigin, SampleQuality};

    fn sample(value: f64) -> Sample {
        Sample {
            elapsed_ns: 1,
            wall_time_unix_ns: 1,
            source_id: "test".into(),
            ecu_id: None,
            signal_id: value.to_string(),
            value,
            unit: "".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: None,
        }
    }

    #[test]
    fn one_signal_failure_does_not_discard_other_readings() {
        let samples = collect_samples_independently(&[1, 2, 3], |pid| match pid {
            2 => Err(LogNookError::signal("ECU rejected PID 02")),
            pid => Ok(vec![sample(*pid as f64)]),
        })
        .unwrap();

        assert_eq!(samples.iter().map(|sample| sample.value).collect::<Vec<_>>(), [1.0, 3.0]);
    }

    #[test]
    fn session_failure_still_aborts_the_read_cycle() {
        let result = collect_samples_independently(&[1, 2, 3], |pid| match pid {
            2 => Err(LogNookError::new("adapter disconnected")),
            pid => Ok(vec![sample(*pid as f64)]),
        });

        assert!(result.is_err());
    }
}
