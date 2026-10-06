use super::engine;
use crate::AppState;
use lognook_core::{
    CanAnalysisConfig, CanAnalyzer, CanCaptureReader, CanDeviceCandidate, CanDeviceSelection,
    CanFrame, CanScanConfig, CanSignalDefinition, CanWorkbenchConfig, CanWorkbenchStatus,
    OemCandidateTrySpec, OemCatalog, available_can_adapters as list_can_adapters,
    decode_can_signal, oem_candidate_scan_config,
};
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[tauri::command]
pub async fn available_can_adapters() -> Result<Vec<CanDeviceCandidate>, String> {
    list_can_adapters().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_candlelight(
    state: State<'_, AppState>,
    selection: CanDeviceSelection,
    config: CanWorkbenchConfig,
) -> Result<CanWorkbenchStatus, String> {
    engine(&state)?
        .connect_candlelight(&selection, config)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn configure_can_signals(
    state: State<'_, AppState>,
    fields: Vec<CanSignalDefinition>,
) -> Result<(), String> {
    engine(&state)?
        .configure_can_signals(fields)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_can_capture(state: State<'_, AppState>, path: String) -> Result<(), String> {
    engine(&state)?
        .start_can_capture(Path::new(&path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_can_capture(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .stop_can_capture()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn send_can_frame(
    state: State<'_, AppState>,
    frame: CanFrame,
    pacing_ms: u64,
    repeat: u32,
) -> Result<(), String> {
    engine(&state)?
        .send_can_frame(frame, pacing_ms, repeat)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_can_scan(
    state: State<'_, AppState>,
    config: CanScanConfig,
) -> Result<(), String> {
    engine(&state)?
        .start_can_scan(config)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_can_scan(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .stop_can_scan()
        .map_err(|error| error.to_string())
}

/// The catalog performs source-revision, license, identifier-width, and exact-route checks
/// before this explicit one-candidate read can transmit.
#[tauri::command]
pub async fn start_can_oem_candidate(
    state: State<'_, AppState>,
    candidate_id: String,
) -> Result<OemCandidateTrySpec, String> {
    let spec = OemCatalog::new(&state.pid_catalog_path)
        .candidate_for_try(&candidate_id)
        .map_err(|error| error.to_string())?;
    let config = oem_candidate_scan_config(&spec).map_err(|error| error.to_string())?;
    engine(&state)?
        .start_can_scan(config)
        .map_err(|error| error.to_string())?;
    Ok(spec)
}

#[derive(Clone, Debug, Serialize)]
pub struct CanCaptureReplayReport {
    pub frames: u64,
    pub decoded_samples: u64,
    pub malformed_rows: Vec<String>,
    pub census: Vec<lognook_core::CanCensusRow>,
}

#[tauri::command]
pub async fn replay_can_capture_report(
    path: String,
    fields: Vec<CanSignalDefinition>,
) -> Result<CanCaptureReplayReport, String> {
    for field in &fields {
        field.validate().map_err(|error| error.to_string())?;
    }
    let mut reader = CanCaptureReader::open(path).map_err(|error| error.to_string())?;
    let mut analyzer =
        CanAnalyzer::new(CanAnalysisConfig::default()).map_err(|error| error.to_string())?;
    let mut report = CanCaptureReplayReport {
        frames: 0,
        decoded_samples: 0,
        malformed_rows: Vec::new(),
        census: Vec::new(),
    };
    while let Some(row) = reader.next() {
        let frame = match row {
            Ok(frame) => frame,
            Err(error) => {
                if report.malformed_rows.len() < 100 {
                    report.malformed_rows.push(error.to_string());
                }
                continue;
            }
        };
        if let Err(error) = analyzer.feed(&frame) {
            if report.malformed_rows.len() < 100 {
                report.malformed_rows.push(error.to_string());
            }
            continue;
        }
        report.frames += 1;
        for field in &fields {
            match decode_can_signal(&frame, field, "can:capture") {
                Ok(Some(_)) => report.decoded_samples += 1,
                Ok(None) => {}
                Err(error) if report.malformed_rows.len() < 100 => {
                    report.malformed_rows.push(error.to_string());
                }
                Err(_) => {}
            }
        }
    }
    report.census = analyzer.census();
    Ok(report)
}
