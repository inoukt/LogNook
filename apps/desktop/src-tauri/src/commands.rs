mod player;
pub use player::*;
mod bluetooth;
pub use bluetooth::*;
mod exports;
pub use exports::*;
mod sessions;
pub use sessions::*;
mod oem_catalog;
pub use oem_catalog::*;
mod openport2;
pub use openport2::*;

use crate::AppState;
use lognook_core::{
    AutomaticTriggerConfig, AutomaticTriggerStatus, ConnectionCapabilities,
    DerivedSignalDefinition, DeviceDescriptor, ObdLinkSxAdapter, RecordingSummary,
    SignalDefinition, available_obdlink_sx_ports as list_obdlink_sx_ports, export_recording_csv,
    identify_obdlink_sx_with_diagnostics as identify_sx_device, standard_obd_signals,
};
use std::{
    path::PathBuf,
    sync::MutexGuard,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

#[tauri::command]
pub fn available_signals() -> Vec<SignalDefinition> {
    standard_obd_signals()
}

#[tauri::command]
pub async fn available_obdlink_sx_ports(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    diagnose(&state, None, "provider_discovery", vec![], |_| {
        list_obdlink_sx_ports().map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn identify_obdlink_sx(
    state: State<'_, AppState>,
    port_name: String,
) -> Result<DeviceDescriptor, String> {
    diagnose(
        &state,
        Some(format!("obdlink-sx:{port_name}")),
        "identify",
        vec![],
        |op| identify_sx_device(port_name, op.sink()).map_err(|error| error.to_string()),
    )
}

#[tauri::command]
pub async fn connect_simulator(
    state: State<'_, AppState>,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    connect_with_diagnostics(&state, "simulator".into(), signal_ids, |engine, ids| {
        engine
            .connect_simulated(ids)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn connect_obdlink_sx(
    state: State<'_, AppState>,
    port_name: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    connect_with_diagnostics(
        &state,
        format!("obdlink-sx:{port_name}"),
        signal_ids,
        |engine, ids| {
            engine
                .connect_adapter(Box::new(ObdLinkSxAdapter::new(port_name)), ids)
                .map_err(|error| error.to_string())
        },
    )
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    diagnose(&state, None, "disconnect", vec![], |_| {
        engine(&state)?
            .disconnect()
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn start_recording(state: State<'_, AppState>) -> Result<String, String> {
    let path = session_path(&state.recordings_dir);
    engine(&state)?
        .start_recording(&path)
        .map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, AppState>) -> Result<RecordingSummary, String> {
    engine(&state)?
        .stop_recording()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn configure_derived_signals(
    state: State<'_, AppState>,
    definitions: Vec<DerivedSignalDefinition>,
    active_ids: Vec<String>,
) -> Result<(), String> {
    engine(&state)?
        .configure_derived_signals(&definitions, &active_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn tare_derived_signal(
    state: State<'_, AppState>,
    signal_id: String,
) -> Result<f64, String> {
    engine(&state)?
        .tare_derived_signal(&signal_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn arm_automatic_trigger(
    state: State<'_, AppState>,
    config: AutomaticTriggerConfig,
) -> Result<AutomaticTriggerStatus, String> {
    let path = session_path(&state.recordings_dir);
    engine(&state)?
        .arm_automatic_trigger(&config, &path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disarm_automatic_trigger(
    state: State<'_, AppState>,
) -> Result<AutomaticTriggerStatus, String> {
    engine(&state)?
        .disarm_automatic_trigger()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn automatic_trigger_status(
    state: State<'_, AppState>,
) -> Result<AutomaticTriggerStatus, String> {
    Ok(engine(&state)?.automatic_trigger_status())
}

#[tauri::command]
pub async fn insert_marker(
    state: State<'_, AppState>,
    label: String,
    note: String,
) -> Result<(), String> {
    engine(&state)?
        .insert_marker(&label, &note)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn export_csv(state: State<'_, AppState>) -> Result<String, String> {
    let engine = engine(&state)?;
    let recording = engine
        .last_recording_path()
        .ok_or_else(|| "No completed recording is available".to_owned())?;
    drop(engine);
    let path = unique_csv_path(&recording);
    export_recording_csv(&recording, &path).map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn replay_last(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .replay_last(1.0)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?.stop_replay();
    Ok(())
}

fn engine<'a>(state: &'a AppState) -> Result<MutexGuard<'a, lognook_core::LogNookEngine>, String> {
    state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine lock is unavailable".to_owned())
}

fn session_path(directory: &std::path::Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!("session-{timestamp}.lognook"))
}

fn unique_csv_path(recording: &std::path::Path) -> PathBuf {
    let base = recording.with_extension("csv");
    if !base.exists() {
        return base;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    recording.with_file_name(format!(
        "{}-{timestamp}.csv",
        recording.file_stem().unwrap_or_default().to_string_lossy()
    ))
}

fn diagnose<T>(
    state: &AppState,
    adapter_id: Option<String>,
    operation: &str,
    signal_ids: Vec<String>,
    action: impl FnOnce(&crate::diagnostics::DiagnosticOperation) -> Result<T, String>,
) -> Result<T, String> {
    let op = state
        .diagnostics
        .begin_operation(adapter_id, operation, signal_ids);
    let result = action(&op);
    op.finish_result(&result);
    result
}
fn connect_with_diagnostics(
    state: &AppState,
    adapter_id: String,
    signal_ids: Vec<String>,
    action: impl FnOnce(
        &mut lognook_core::LogNookEngine,
        &[String],
    ) -> Result<ConnectionCapabilities, String>,
) -> Result<ConnectionCapabilities, String> {
    diagnose(
        state,
        Some(adapter_id),
        "connect",
        signal_ids.clone(),
        |op| {
            let mut engine = engine(state)?;
            // A failed second Connect must not replace the existing session's diagnostic route.
            if !engine.is_connected() {
                engine.set_diagnostic_sink(op.sink());
            }
            let result = action(&mut engine, &signal_ids);
            if let Ok(capabilities) = &result {
                op.supported_signals(
                    capabilities
                        .supported_signals
                        .iter()
                        .map(|s| s.id.clone())
                        .collect(),
                );
            }
            result
        },
    )
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;
    use crate::diagnostics::{DiagnosticRecord, DiagnosticsService};
    use lognook_core::DiagnosticPhase;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn state() -> (AppState, mpsc::Receiver<DiagnosticRecord>) {
        let path = std::env::temp_dir().join(format!(
            "lognook-command-diagnostics-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (tx, rx) = mpsc::channel();
        let diagnostics = DiagnosticsService::start(path.join("operations.jsonl"), move |record| {
            let _ = tx.send(record);
        })
        .unwrap();
        let observer = diagnostics.clone();
        (
            AppState {
                engine: std::sync::Mutex::new(lognook_core::LogNookEngine::new(move |event| {
                    observer.observe_engine(&event)
                })),
                diagnostics,
                diagnostics_path: path.join("operations.jsonl"),
                recordings_dir: path.clone(),
                pid_catalog_path: path.join("catalog.sqlite"),
                oem_validation_log: path.join("probe.jsonl"),
                oem_candidate_try_log: path.join("tries.jsonl"),
                a30_read_test_log: path.join("reads.jsonl"),
            },
            rx,
        )
    }
    #[test]
    fn connect_failure_preserves_error_and_logs_redacted_start_and_terminal() {
        let (state, rx) = state();
        let error = "Timed out waiting for the ELM327 > prompt after 010C";
        let result = connect_with_diagnostics(
            &state,
            "adapter".into(),
            vec!["engine_rpm".into()],
            |_, _| Err(error.into()),
        );
        assert_eq!(result.unwrap_err(), error);
        let start = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let end = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(start.phase, DiagnosticPhase::Started);
        assert_eq!(end.phase, DiagnosticPhase::Failed);
        assert_eq!(start.operation_id, end.operation_id);
        assert_eq!(end.signal_ids, ["engine_rpm"]);
        assert_eq!(
            end.detail.as_deref(),
            Some("Timed out waiting for the ELM327 > prompt after [request redacted]")
        );
        assert!(end.duration_ms.is_some());
    }
    #[test]
    fn real_simulator_connect_and_disconnect_use_command_and_session_ids() {
        let (state, rx) = state();
        connect_with_diagnostics(
            &state,
            "simulator".into(),
            vec!["engine_rpm".into()],
            |engine, ids| engine.connect_simulated(ids).map_err(|e| e.to_string()),
        )
        .unwrap();
        let mut records = vec![];
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let record = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let done = record.stage == "signal_summary";
            records.push(record);
            if done {
                break;
            }
            assert!(Instant::now() < deadline);
        }
        let first_count = records
            .iter()
            .find_map(|r| r.signal_summaries.as_ref())
            .unwrap()[0]
            .success_count;
        state
            .diagnostics
            .observe_engine(&lognook_core::EngineEvent::Sample(lognook_core::Sample {
                elapsed_ns: 0,
                wall_time_unix_ns: 0,
                source_id: "simulated-obd".into(),
                ecu_id: None,
                signal_id: "engine_rpm".into(),
                value: 99999.123,
                unit: "rpm".into(),
                quality: lognook_core::SampleQuality::Valid,
                origin: lognook_core::SampleOrigin::Live,
                raw_bytes: Some(vec![1, 2, 3]),
            }));
        diagnose(&state, None, "disconnect", vec![], |_| {
            engine(&state)?.disconnect().map_err(|e| e.to_string())
        })
        .unwrap();
        loop {
            let record = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let done = record.operation == "disconnect"
                && record.stage == "command"
                && record.phase == DiagnosticPhase::Succeeded;
            records.push(record);
            if done {
                break;
            }
        }
        let connect = records
            .iter()
            .find(|r| r.operation == "connect" && r.stage == "command")
            .unwrap();
        let session = records
            .iter()
            .find(|r| r.operation == "acquisition" && r.stage == "session")
            .unwrap();
        let disconnect = records
            .iter()
            .find(|r| r.operation == "disconnect")
            .unwrap();
        assert_ne!(connect.operation_id, session.operation_id);
        assert_ne!(disconnect.operation_id, session.operation_id);
        assert_eq!(disconnect.adapter_id.as_deref(), Some("simulated-obd"));
        assert!(
            records
                .iter()
                .filter(|r| r.stage == "signal_summary")
                .all(|r| r.operation_id == session.operation_id)
        );
        assert!(
            records
                .iter()
                .any(|r| r.stage == "session" && r.phase == DiagnosticPhase::Succeeded)
        );
        let summaries: Vec<_> = records
            .iter()
            .filter(|r| r.stage == "signal_summary")
            .collect();
        assert_eq!(summaries.len(), 2);
        assert!(
            summaries.last().unwrap().signal_summaries.as_ref().unwrap()[0].success_count
                >= first_count + 1
        );
    }
    #[test]
    fn provider_discovery_logs_missing_worker_failure() {
        let (state, rx) = state();
        let result =
            openport2::discover_providers(&state, || Err("OpenPort worker is missing".into()));
        assert!(result.is_err());
        let start = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let end = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(start.operation, "provider_discovery");
        assert_eq!(end.phase, DiagnosticPhase::Failed);
        assert_eq!(start.operation_id, end.operation_id);
        assert!(end.duration_ms.is_some());
    }
}
