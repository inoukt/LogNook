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

#[cfg(test)]
mod tests {
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
