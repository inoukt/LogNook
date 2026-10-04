use crate::model::{ConnectionCapabilities, LogNookError, Sample, SignalDefinition};
use std::time::Duration;

/// One serialized command owner for a physical or simulated diagnostic interface.
pub trait HardwareAdapter: Send + 'static {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError>;
    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError>;
    /// Target delay between read cycles; slow adapter responses naturally reduce the actual rate.
    fn sample_interval(&self) -> Duration {
        Duration::from_millis(100)
    }
    fn disconnect(&mut self) {}
}
