use super::engine;
use crate::AppState;
use lognook_core::{
    ConnectionCapabilities, DiagnosticOutcome, DiagnosticQueryRequest, DiagnosticReport,
    DiagnosticRequest, ResearchTestRequest, ResearchTestResult, VinOrigin,
    supports_vehicle_diagnostics,
};
use std::{
    sync::mpsc::Receiver,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

fn connected_capabilities(state: &AppState) -> Result<ConnectionCapabilities, String> {
    let engine = engine(state)?;
    if !engine.is_obd_connected() {
        return Err("Connect a supported OBD adapter before reading vehicle diagnostics".into());
    }
    let capabilities = engine.connection_capabilities().cloned().ok_or_else(|| {
        "Connect a supported OBD adapter before reading vehicle diagnostics".to_owned()
    })?;
    if !supports_vehicle_diagnostics(Some(&capabilities.device.hardware_family)) {
        return Err(format!(
            "Vehicle diagnostics are not supported by {}",
            capabilities.device.hardware_family
        ));
    }
    Ok(capabilities)
}

fn queue_query(
    state: &AppState,
    query: DiagnosticQueryRequest,
    expected_adapter_id: &str,
) -> Result<Receiver<Result<ResearchTestResult, lognook_core::LogNookError>>, String> {
    query.validate()?;
    let engine = engine(state)?;
    let capabilities = engine
        .connection_capabilities()
        .ok_or_else(|| "The diagnostic adapter disconnected during the read".to_owned())?;
    if capabilities.device.stable_id != expected_adapter_id {
        return Err("The connected adapter changed during the diagnostic read".into());
    }
    if !supports_vehicle_diagnostics(Some(&capabilities.device.hardware_family)) {
        return Err(format!(
            "Vehicle diagnostics are not supported by {}",
            capabilities.device.hardware_family
        ));
    }
    let receiver = engine
        .queue_research_test(ResearchTestRequest::Diagnostic(query))
        .map_err(|error| error.to_string())?;
    drop(engine);
    Ok(receiver)
}

fn report_for(
    capabilities: &ConnectionCapabilities,
    request: DiagnosticRequest,
) -> DiagnosticReport {
    DiagnosticReport {
        report_id: None,
        connection_generation: None,
        vehicle_context: None,
        adapter: capabilities.device.clone(),
        protocol: capabilities.protocol.clone(),
        started_at_unix_ms: now_unix_ms(),
        request,
        reads: Vec::new(),
        route_restore_error: None,
        evidence_saved: false,
        evidence_save_error: None,
    }
}

fn add_transport_error(
    report: &mut DiagnosticReport,
    query: &DiagnosticQueryRequest,
    detail: String,
) {
    report.reads.push(query.transport_error(
        String::new(),
        "Not sent (adapter queue unavailable)".into(),
        now_unix_ms(),
        0,
        detail,
    ));
}

fn collect_result(
    receiver: Receiver<Result<ResearchTestResult, lognook_core::LogNookError>>,
) -> Result<DiagnosticReport, String> {
    let result = receiver
        .recv()
        .map_err(|_| "Acquisition worker did not complete the vehicle diagnostic read".to_owned())?
        .map_err(|error| error.to_string())?;
    match result {
        ResearchTestResult::Diagnostic(report) => Ok(report),
        _ => Err("Active adapter returned the wrong vehicle diagnostic result type".into()),
    }
}

fn add_query_result(
    aggregate: &mut DiagnosticReport,
    query: &DiagnosticQueryRequest,
    result: DiagnosticReport,
) -> Option<DiagnosticQueryRequest> {
    let next = query.next_mode06_bitmap(&result.reads);
    aggregate.reads.extend(result.reads);
    if result.route_restore_error.is_some() {
        aggregate.route_restore_error = result.route_restore_error;
    }
    next
}

fn collect_sync(
    state: &AppState,
    mut request: DiagnosticRequest,
) -> Result<DiagnosticReport, String> {
    request.validate()?;
    let mut queries = request.queries()?;
    let capabilities = connected_capabilities(state)?;
    let mut report = report_for(&capabilities, request);
    while let Some(query) = queries.pop_front() {
        let receiver = match queue_query(state, query.clone(), &report.adapter.stable_id) {
            Ok(receiver) => receiver,
            Err(error) => {
                add_transport_error(&mut report, &query, error);
                break;
            }
        };
        match collect_result(receiver) {
            Ok(result) => {
                let transport_failed = result
                    .reads
                    .iter()
                    .any(|read| read.outcome == DiagnosticOutcome::TransportError);
                if let Some(next) = add_query_result(&mut report, &query, result) {
                    queries.push_front(next);
                }
                if transport_failed {
                    break;
                }
            }
            Err(error) => {
                add_transport_error(&mut report, &query, error);
                break;
            }
        }
    }
    Ok(report)
}

pub(crate) fn execute_vehicle_diagnostic(
    state: &AppState,
    request: DiagnosticRequest,
) -> Result<DiagnosticReport, String> {
    let (adapter_id, generation, scope, recognition) = {
        let current = engine(state)?;
        if !current.is_obd_connected() {
            return Err(
                "Connect a supported OBD adapter before reading vehicle diagnostics".into(),
            );
        }
        let capabilities = current.connection_capabilities().cloned().ok_or_else(|| {
            "Connect a supported OBD adapter before reading vehicle diagnostics".to_owned()
        })?;
        let generation = current.obd_connection_generation();
        (
            Some(capabilities.device.stable_id.clone()),
            generation,
            super::vehicle_profiles::connection_scope(&capabilities.device.hardware_family),
            state.vehicle_profiles.current_observation(generation),
        )
    };
    let signal_ids = request
        .mode06_test_ids
        .iter()
        .map(|id| format!("mode06_test_{id:02X}"))
        .collect();
    let operation =
        state
            .diagnostics
            .begin_operation(adapter_id, "vehicle_diagnostics", signal_ids);
    let mut result = collect_sync(state, request);
    if let Ok(report) = &mut result {
        crate::vehicle_reads::append_vehicle_report(
            state,
            report,
            scope,
            generation,
            VinOrigin::EcuReport,
            recognition.as_ref(),
        );
        if let Some(context) = report.vehicle_context.clone()
            && let Ok(mut current) = engine(state)
        {
            let _ = current.set_vehicle_recording_context(Some(context), generation);
        }
    }
    operation.finish_result(&result);
    result
}

#[tauri::command]
pub async fn run_vehicle_diagnostics(
    app: tauri::AppHandle,
    request: DiagnosticRequest,
) -> Result<DiagnosticReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        execute_vehicle_diagnostic(&state, request)
    })
    .await
    .map_err(|error| format!("Vehicle diagnostic worker failed: {error}"))?
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
