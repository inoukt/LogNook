use crate::model::{ConnectionCapabilities, LogNookError, Sample, SignalDefinition};
use crate::{
    DiagnosticSink, OemCandidateTryResult, OemCandidateTrySpec, ResearchTestRequest,
    ResearchTestResult, communication_trace::CommunicationSink,
};
use std::time::Duration;

const MAX_RAW_DIAGNOSTIC_BYTES: usize = 64;
const MAX_RAW_TERMINAL_BYTES: usize = 256;

#[derive(Clone, Debug)]
pub enum RawAdapterCommand {
    DiagnosticPayload(Vec<u8>),
    AdapterTerminal(String),
}

impl RawAdapterCommand {
    pub fn command_text(&self) -> Result<String, LogNookError> {
        if matches!(self, Self::DiagnosticPayload(bytes) if bytes.first() == Some(&0x04))
            || matches!(self, Self::AdapterTerminal(command) if is_mode04_terminal_command(command))
        {
            return Err(LogNookError::new(
                "Mode 04 must use the explicit clear_vehicle_dtcs operation",
            ));
        }
        match self {
            Self::DiagnosticPayload(bytes)
                if !bytes.is_empty() && bytes.len() <= MAX_RAW_DIAGNOSTIC_BYTES =>
            {
                Ok(bytes.iter().map(|byte| format!("{byte:02X}")).collect())
            }
            Self::DiagnosticPayload(_) => Err(LogNookError::new(
                "Raw diagnostic payload must contain 1-64 bytes",
            )),
            Self::AdapterTerminal(command)
                if !command.is_empty()
                    && command.len() <= MAX_RAW_TERMINAL_BYTES
                    && command.is_ascii()
                    && command
                        .bytes()
                        .all(|byte| byte.is_ascii_graphic() || byte == b' ') =>
            {
                Ok(command.clone())
            }
            Self::AdapterTerminal(_) => Err(LogNookError::new(
                "Adapter command must be 1-256 printable ASCII bytes without line breaks",
            )),
        }
    }
}

fn is_mode04_terminal_command(command: &str) -> bool {
    let normalized = command.trim().to_ascii_uppercase();
    if normalized
        .split(|character: char| character.is_ascii_whitespace() || matches!(character, ';' | ','))
        .any(|part| {
            let compact = part
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            compact.starts_with("04") && compact.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return true;
    }
    normalized.split_once("D:").is_some_and(|(_, data)| {
        let data = data.trim_start();
        data.bytes()
            .take_while(u8::is_ascii_hexdigit)
            .collect::<Vec<_>>()
            .starts_with(b"04")
    })
}

#[derive(Clone, Debug)]
pub struct RawAdapterCommandResult {
    pub command: String,
    pub response: String,
    pub duration_ms: u64,
}

/// One serialized command owner for a physical or simulated diagnostic interface.
pub trait HardwareAdapter: Send + 'static {
    /// Install an observational route before connecting. Adapters without diagnostics ignore it.
    fn set_diagnostic_sink(&mut self, _sink: DiagnosticSink) {}
    /// Install a nonblocking observer for exchanges exposed by this adapter.
    fn set_communication_sink(&mut self, _sink: CommunicationSink) {}
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
    fn execute_raw_command(
        &mut self,
        _command: RawAdapterCommand,
    ) -> Result<RawAdapterCommandResult, LogNookError> {
        Err(LogNookError::new(
            "This adapter does not support raw terminal or diagnostic commands",
        ))
    }
    /// Run one explicit standard Mode 04 request. Callers cannot supply bytes, an identifier,
    /// or a retry policy; adapters use the same serialized owner as diagnostic reads.
    fn clear_vehicle_dtcs(&mut self) -> Result<Vec<crate::DiagnosticRead>, LogNookError> {
        Err(LogNookError::new(
            "This adapter does not support explicit standard Mode 04 DTC clearing",
        ))
    }
    fn validate_oem_candidate_poll(&self, _spec: &OemCandidateTrySpec) -> Result<(), LogNookError> {
        Err(LogNookError::new(
            "This adapter does not support continuous OEM candidate reads",
        ))
    }
    /// Poll a compatible group in one adapter-owned exchange. CAN/J2534 defaults to the first
    /// candidate; transports such as Subaru SSM can batch multiple explicit memory addresses.
    fn execute_oem_poll_group(
        &mut self,
        specs: &[OemCandidateTrySpec],
    ) -> Result<OemCandidateTryResult, LogNookError> {
        let first = specs
            .first()
            .ok_or_else(|| LogNookError::new("OEM poll group is empty"))?;
        match self.execute_research_test(ResearchTestRequest::OemCandidate(first.clone()))? {
            ResearchTestResult::OemCandidate(result) => Ok(result),
            _ => Err(LogNookError::new(
                "Adapter returned an unexpected OEM poll result type",
            )),
        }
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
    use super::{RawAdapterCommand, collect_samples_independently};
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

        assert_eq!(
            samples
                .iter()
                .map(|sample| sample.value)
                .collect::<Vec<_>>(),
            [1.0, 3.0]
        );
    }

    #[test]
    fn session_failure_still_aborts_the_read_cycle() {
        let result = collect_samples_independently(&[1, 2, 3], |pid| match pid {
            2 => Err(LogNookError::new("adapter disconnected")),
            pid => Ok(vec![sample(*pid as f64)]),
        });

        assert!(result.is_err());
    }

    #[test]
    fn raw_adapter_commands_keep_terminal_text_and_format_diagnostic_bytes() {
        assert_eq!(
            RawAdapterCommand::AdapterTerminal("ATZ".into())
                .command_text()
                .unwrap(),
            "ATZ"
        );
        assert_eq!(
            RawAdapterCommand::DiagnosticPayload(vec![0x01, 0x0c])
                .command_text()
                .unwrap(),
            "010C"
        );
    }

    #[test]
    fn raw_adapter_terminal_rejects_line_breaks_and_empty_payload() {
        assert!(
            RawAdapterCommand::AdapterTerminal("ATZ\r\n010C".into())
                .command_text()
                .is_err()
        );
        assert!(
            RawAdapterCommand::DiagnosticPayload(Vec::new())
                .command_text()
                .is_err()
        );
    }

    #[test]
    fn raw_command_paths_cannot_send_mode04_clear_requests() {
        for command in [
            RawAdapterCommand::DiagnosticPayload(vec![0x04]),
            RawAdapterCommand::DiagnosticPayload(vec![0x04, 0x00]),
            RawAdapterCommand::AdapterTerminal("04".into()),
            RawAdapterCommand::AdapterTerminal("0400".into()),
            RawAdapterCommand::AdapterTerminal("04 00".into()),
            RawAdapterCommand::AdapterTerminal("ATSH7DF;04".into()),
            RawAdapterCommand::AdapterTerminal("ATSH7DF;04 00".into()),
            RawAdapterCommand::AdapterTerminal("ATSH7DF 04".into()),
            RawAdapterCommand::AdapterTerminal("STPX H:7DF, D:0400, T:250".into()),
        ] {
            let error = command.command_text().unwrap_err().to_string();
            assert!(error.contains("clear_vehicle_dtcs"), "{error}");
        }
    }
}
