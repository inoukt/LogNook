use crate::model::{ConnectionCapabilities, LogNookError, Sample, SignalDefinition};

/// One serialized command owner for a physical or simulated diagnostic interface.
pub trait HardwareAdapter: Send + 'static {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError>;
    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError>;
    fn disconnect(&mut self) {}
}
