use crate::model::{ConnectionCapabilities, LogNookError, Sample, SignalDefinition};
use crate::{ResearchTestRequest, ResearchTestResult};
use std::time::Duration;

/// One serialized command owner for a physical or simulated diagnostic interface.
pub trait HardwareAdapter: Send + 'static {
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
