use super::protocol::{AgentTraceCaptureStatus, AgentTraceEvent, AgentTracePage};
use crate::AppState;
use lognook_core::communication_trace::{CommunicationDirection, TraceCaptureStatus, TracePage};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn get_trace(
    state: &AppState,
    after_sequence: u64,
    limit: usize,
) -> Result<AgentTracePage, String> {
    state
        .trace_store
        .get_trace(after_sequence, limit)
        .map(map_page)
}

pub(crate) fn start_capture(state: &AppState) -> Result<AgentTraceCaptureStatus, String> {
    let path = trace_directory(state)?.join(format!("capture-{}.jsonl", timestamp_ms()));
    state.trace_store.start_capture(&path)?;
    Ok(map_capture(state.trace_store.capture_status()))
}

pub(crate) fn stop_capture(state: &AppState) -> Result<AgentTraceCaptureStatus, String> {
    state.trace_store.stop_capture().map(map_capture)
}

pub(crate) fn export_evidence(state: &AppState) -> Result<String, String> {
    let status = state.trace_store.stop_capture()?;
    let source = status
        .path
        .map(PathBuf::from)
        .ok_or_else(|| "No trace capture is available to export".to_owned())?;
    if !source.is_file() {
        return Err(status
            .error
            .unwrap_or_else(|| "Trace capture file is unavailable".into()));
    }
    let destination_directory = trace_directory(state)?.join("exports");
    fs::create_dir_all(&destination_directory).map_err(|error| error.to_string())?;
    let destination = destination_directory.join(format!("trace-{}.jsonl", timestamp_ms()));
    fs::copy(&source, &destination).map_err(|error| error.to_string())?;
    Ok(destination.to_string_lossy().into_owned())
}

fn trace_directory(state: &AppState) -> Result<PathBuf, String> {
    state
        .recordings_dir
        .parent()
        .map(|directory| directory.join("agent-traces"))
        .ok_or_else(|| "LogNook data directory is unavailable".into())
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn map_capture(status: TraceCaptureStatus) -> AgentTraceCaptureStatus {
    AgentTraceCaptureStatus {
        capturing: status.capturing,
        path: status.path,
        error: status.error,
    }
}

fn map_page(page: TracePage) -> AgentTracePage {
    AgentTracePage {
        events: page
            .events
            .into_iter()
            .map(|event| AgentTraceEvent {
                sequence: event.sequence,
                timestamp_unix_ns: event.timestamp_unix_ns,
                elapsed_ns: event.elapsed_ns,
                adapter_id: event.adapter_id,
                operation: event.operation,
                direction: match event.direction {
                    CommunicationDirection::Tx => "tx",
                    CommunicationDirection::Rx => "rx",
                }
                .into(),
                route: event.route,
                bytes_hex: event.bytes_hex,
                outcome: event.outcome,
                raw_bytes_unavailable: event.raw_bytes_unavailable,
            })
            .collect(),
        next_sequence: page.next_sequence,
        gaps: page.gaps,
        dropped_events: page.dropped_events,
        evicted_events: page.evicted_events,
        capture: map_capture(page.capture),
    }
}
