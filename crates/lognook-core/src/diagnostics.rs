use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Started,
    Progress,
    Succeeded,
    Failed,
}

/// Observational operation metadata. Never include measurements or raw protocol payloads.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DiagnosticProgress {
    pub adapter_id: Option<String>,
    pub operation: String,
    pub stage: String,
    pub phase: DiagnosticPhase,
    pub duration_ms: Option<u64>,
    pub signal_ids: Vec<String>,
    pub detail: Option<String>,
    pub transient: Option<bool>,
}

/// Called on the adapter owner. Sinks must return promptly (for example, use `try_send`)
/// and must not block on persistence or a full queue.
pub type DiagnosticSink = Arc<dyn Fn(DiagnosticProgress) + Send + Sync>;

pub(crate) fn noop_diagnostic_sink() -> DiagnosticSink {
    Arc::new(|_| {})
}

pub(crate) fn diagnostic_error_detail(message: &str) -> String {
    for prefix in [
        "OBDLink SX did not acknowledge ",
        "ELM327 rejected adapter setup command ",
        "OBDLink SX returned no value for ",
        "ELM327 returned no value for ",
    ] {
        if message.starts_with(prefix) {
            return format!("{prefix}[request redacted]");
        }
    }
    if message.starts_with("Unexpected firmware identity for OBDLink SX: ") {
        return "Unexpected firmware identity for OBDLink SX: [response redacted]".into();
    }
    for prefix in [
        "Selected port is not an OBDLink SX (",
        "Bluetooth device did not identify as ELM327-compatible (",
    ] {
        if message.starts_with(prefix) {
            return format!("{prefix}[response redacted])");
        }
    }
    const ELM_TIMEOUT_PREFIX: &str = "Timed out waiting for the ELM327 > prompt after ";
    const OPENPORT_PARSER_PREFIX: &str =
        "OpenPort worker returned malformed output: Malformed J2534 worker message: ";
    if message.starts_with(ELM_TIMEOUT_PREFIX) {
        format!("{ELM_TIMEOUT_PREFIX}[request redacted]")
    } else if message.starts_with(OPENPORT_PARSER_PREFIX) {
        // Serde and protocol validation errors may quote arbitrary response values or keys.
        // Keep the worker/parser failure class, without copying untrusted parser detail.
        format!("{OPENPORT_PARSER_PREFIX}[parser detail redacted]")
    } else {
        message.to_owned()
    }
}

/// Time one existing adapter stage without inspecting its response or samples.
pub(crate) fn diagnostic_stage<T>(
    sink: &DiagnosticSink,
    adapter_id: Option<String>,
    operation: &str,
    stage: &str,
    signal_ids: Vec<String>,
    action: impl FnOnce() -> Result<T, crate::LogNookError>,
) -> Result<T, crate::LogNookError> {
    let started = std::time::Instant::now();
    let event = DiagnosticProgress {
        adapter_id,
        operation: operation.into(),
        stage: stage.into(),
        phase: DiagnosticPhase::Started,
        duration_ms: None,
        signal_ids,
        detail: None,
        transient: None,
    };
    sink(event.clone());
    let result = action();
    diagnostic_stage_finished(sink, event, started, &result);
    result
}

pub(crate) fn diagnostic_stage_finished<T>(
    sink: &DiagnosticSink,
    mut event: DiagnosticProgress,
    started: std::time::Instant,
    result: &Result<T, crate::LogNookError>,
) {
    event.duration_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
    event.phase = if result.is_ok() {
        DiagnosticPhase::Succeeded
    } else {
        DiagnosticPhase::Failed
    };
    if let Err(error) = result {
        event.detail = Some(diagnostic_error_detail(&error.to_string()));
        event.transient = Some(error.is_transient());
    }
    sink(event);
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostic_error_redacts_adapter_traffic_and_untrusted_identity() {
        for (message, expected) in [
            (
                "ELM327 returned no value for ATI",
                "ELM327 returned no value for [request redacted]",
            ),
            (
                "OBDLink SX did not acknowledge AT SH 7DF",
                "OBDLink SX did not acknowledge [request redacted]",
            ),
            (
                "ELM327 rejected adapter setup command ATE0",
                "ELM327 rejected adapter setup command [request redacted]",
            ),
            (
                "Unexpected firmware identity for OBDLink SX: 41 0C 1A F8",
                "Unexpected firmware identity for OBDLink SX: [response redacted]",
            ),
            (
                "Selected port is not an OBDLink SX (41 0C 1A F8)",
                "Selected port is not an OBDLink SX ([response redacted])",
            ),
            (
                "Bluetooth device did not identify as ELM327-compatible (41 0C 1A F8)",
                "Bluetooth device did not identify as ELM327-compatible ([response redacted])",
            ),
            (
                "OBDLink SX returned no value for STI",
                "OBDLink SX returned no value for [request redacted]",
            ),
        ] {
            assert_eq!(super::diagnostic_error_detail(message), expected);
        }
    }

    #[test]
    fn diagnostic_error_redacts_elm_request_and_preserves_safe_error_context() {
        assert_eq!(
            super::diagnostic_error_detail("Timed out waiting for the ELM327 > prompt after 010C"),
            "Timed out waiting for the ELM327 > prompt after [request redacted]"
        );
        for message in [
            "OpenPort Timeout: during read: provider error 0x09",
            "Mode 01 PID 0C received NRC 0x78",
            "Timed out waiting for the OBDLink SX > prompt",
        ] {
            assert_eq!(super::diagnostic_error_detail(message), message);
        }
    }
}
