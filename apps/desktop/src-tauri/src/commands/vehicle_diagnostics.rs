use super::engine;
use crate::AppState;
use lognook_core::{
    ConnectionCapabilities, DiagnosticOutcome, DiagnosticQueryRequest, DiagnosticReport,
    DiagnosticRequest, ResearchTestRequest, ResearchTestResult, supports_vehicle_diagnostics,
};
use std::{
    fs::OpenOptions,
    io::Write,
    sync::{Mutex, mpsc::Receiver},
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

fn save_evidence(path: &std::path::Path, report: &mut DiagnosticReport) {
    let result = (|| -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut evidence = report.clone();
        evidence.evidence_saved = true;
        evidence.evidence_save_error = None;
        let envelope = serde_json::json!({
            "os": std::env::consts::OS,
            "app_version": env!("CARGO_PKG_VERSION"),
            "report": evidence,
        });
        let mut line = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
        line.push(b'\n');
        static APPEND_LOCK: Mutex<()> = Mutex::new(());
        let _guard = APPEND_LOCK
            .lock()
            .map_err(|_| "Vehicle-read evidence lock is unavailable".to_owned())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| format!("Could not open vehicle-read evidence log: {error}"))?;
        file.write_all(&line).map_err(|error| error.to_string())
    })();
    match result {
        Ok(()) => report.evidence_saved = true,
        Err(error) => report.evidence_save_error = Some(error),
    }
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
    let path = state.diagnostics_path.with_file_name("vehicle-reads.jsonl");
    save_evidence(&path, &mut report);
    Ok(report)
}

pub(crate) fn execute_vehicle_diagnostic(
    state: &AppState,
    request: DiagnosticRequest,
) -> Result<DiagnosticReport, String> {
    let adapter_id = connected_capabilities(state)
        .ok()
        .map(|capabilities| capabilities.device.stable_id);
    let signal_ids = request
        .mode06_test_ids
        .iter()
        .map(|id| format!("mode06_test_{id:02X}"))
        .collect();
    let operation =
        state
            .diagnostics
            .begin_operation(adapter_id, "vehicle_diagnostics", signal_ids);
    let result = collect_sync(state, request);
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

#[cfg(test)]
mod tests {
    use super::*;
    use lognook_core::DeviceDescriptor;

    fn report() -> DiagnosticReport {
        DiagnosticReport {
            adapter: DeviceDescriptor {
                stable_id: "test-obd".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: None,
            },
            protocol: "fixture".into(),
            started_at_unix_ms: 123,
            request: DiagnosticRequest::default(),
            route_restore_error: None,
            reads: vec![
                DiagnosticRequest::default()
                    .queries()
                    .unwrap()
                    .front()
                    .unwrap()
                    .transport_error(
                        "RAW REPLY".into(),
                        "03".into(),
                        124,
                        5,
                        "fixture error".into(),
                    ),
            ],
            evidence_saved: false,
            evidence_save_error: None,
        }
    }

    #[test]
    fn concurrent_evidence_reports_remain_complete_json_lines() {
        let path = std::env::temp_dir().join(format!(
            "lognook-vehicle-evidence-{}.jsonl",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let jobs = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut value = report();
                    save_evidence(&path, &mut value);
                    assert!(value.evidence_saved);
                    assert!(value.evidence_save_error.is_none());
                })
            })
            .collect::<Vec<_>>();
        for job in jobs {
            job.join().unwrap();
        }
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.lines().count(), 8);
        for line in contents.lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["report"]["reads"][0]["raw_response"], "RAW REPLY");
            assert_eq!(value["report"]["started_at_unix_ms"], 123);
            assert_eq!(value["report"]["evidence_saved"], true);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn evidence_failure_keeps_the_diagnostic_results() {
        let mut value = report();
        let before = value.reads.clone();
        save_evidence(&std::env::temp_dir(), &mut value);
        assert!(!value.evidence_saved);
        assert!(value.evidence_save_error.is_some());
        assert_eq!(value.reads, before);
    }
}
