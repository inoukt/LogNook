mod player;
pub use player::*;
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
    identify_obdlink_sx as identify_sx_device, standard_obd_signals,
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
pub async fn available_obdlink_sx_ports() -> Result<Vec<String>, String> {
    list_obdlink_sx_ports().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn identify_obdlink_sx(port_name: String) -> Result<DeviceDescriptor, String> {
    identify_sx_device(port_name).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_simulator(
    state: State<'_, AppState>,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_simulated(&signal_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_obdlink_sx(
    state: State<'_, AppState>,
    port_name: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_adapter(Box::new(ObdLinkSxAdapter::new(port_name)), &signal_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .disconnect()
        .map_err(|error| error.to_string())
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

fn engine<'a>(
    state: &'a State<'a, AppState>,
) -> Result<MutexGuard<'a, lognook_core::LogNookEngine>, String> {
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
