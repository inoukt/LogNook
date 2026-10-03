use crate::AppState;
use lognook_core::{
    ConnectionCapabilities, DeviceDescriptor, Marker, ObdLinkSxAdapter, PlaybackState,
    RecordingInfo, RecordingSummary, Sample, SignalDefinition,
    available_obdlink_sx_ports as list_obdlink_sx_ports, identify_obdlink_sx as identify_sx_device,
    import_csv_to_recording, query_recording_window, standard_obd_signals,
};
use std::{
    path::{Path, PathBuf},
    sync::MutexGuard,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub fn available_signals() -> Vec<SignalDefinition> {
    standard_obd_signals()
}

#[tauri::command]
pub fn available_obdlink_sx_ports() -> Result<Vec<String>, String> {
    list_obdlink_sx_ports().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn identify_obdlink_sx(port_name: String) -> Result<DeviceDescriptor, String> {
    identify_sx_device(port_name).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn connect_simulator(
    state: State<'_, AppState>,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_simulated(&signal_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn connect_obdlink_sx(
    state: State<'_, AppState>,
    port_name: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_adapter(Box::new(ObdLinkSxAdapter::new(port_name)), &signal_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .disconnect()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_recording(state: State<'_, AppState>) -> Result<String, String> {
    let path = session_path(&state.recordings_dir);
    engine(&state)?
        .start_recording(&path)
        .map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn stop_recording(state: State<'_, AppState>) -> Result<RecordingSummary, String> {
    engine(&state)?
        .stop_recording()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn insert_marker(
    state: State<'_, AppState>,
    label: String,
    note: String,
) -> Result<(), String> {
    engine(&state)?
        .insert_marker(&label, &note)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_csv(state: State<'_, AppState>) -> Result<String, String> {
    let engine = engine(&state)?;
    let recording = engine
        .last_recording_path()
        .ok_or_else(|| "No completed recording is available".to_owned())?;
    let path = unique_csv_path(&recording);
    engine
        .export_last_csv(&path)
        .map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn replay_last(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .replay_last(1.0)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stop_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?.stop_replay();
    Ok(())
}

#[tauri::command]
pub async fn open_csv(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<RecordingInfo>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("CSV files", &["csv"])
        .blocking_pick_file()
        .map(|file| file.into_path().map_err(|error| error.to_string()))
        .transpose()?;
    let mut engine = engine(&state)?;
    import_selected_csv(selected, &mut engine, &state.recordings_dir)
}

pub fn import_selected_csv(
    selected: Option<PathBuf>,
    engine: &mut lognook_core::LogNookEngine,
    recordings_dir: &Path,
) -> Result<Option<RecordingInfo>, String> {
    let Some(source) = selected else {
        return Ok(None);
    };
    if engine.is_connected() || engine.is_recording() {
        return Err("Disconnect and stop recording before opening a session".into());
    }
    std::fs::create_dir_all(recordings_dir).map_err(|error| error.to_string())?;
    let target = session_path(recordings_dir);
    import_csv_to_recording(&source, &target).map_err(|error| error.to_string())?;
    engine
        .load_recording(&target)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn load_last_recording(state: State<'_, AppState>) -> Result<RecordingInfo, String> {
    let mut engine = engine(&state)?;
    let path = engine
        .last_recording_path()
        .ok_or_else(|| "No completed recording is available".to_owned())?;
    engine
        .load_recording(&path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_replay(state: State<'_, AppState>, speed: f64) -> Result<(), String> {
    engine(&state)?
        .start_replay(speed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn pause_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .pause_replay()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resume_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .resume_replay()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn seek_replay(state: State<'_, AppState>, position_ns: i64) -> Result<(), String> {
    engine(&state)?
        .seek_replay(position_ns)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_replay_speed(state: State<'_, AppState>, speed: f64) -> Result<(), String> {
    engine(&state)?
        .set_replay_speed(speed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn playback_state(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    engine(&state)?
        .playback_state()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn add_playback_marker(
    state: State<'_, AppState>,
    label: String,
    note: String,
) -> Result<Marker, String> {
    engine(&state)?
        .add_playback_marker(&label, &note)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_playback_marker(
    state: State<'_, AppState>,
    id: i64,
    label: String,
    note: String,
) -> Result<Marker, String> {
    engine(&state)?
        .update_playback_marker(id, &label, &note)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_playback_marker(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    engine(&state)?
        .delete_playback_marker(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn recording_window(
    state: State<'_, AppState>,
    signal_ids: Vec<String>,
    start_ns: i64,
    end_ns: i64,
    max_buckets: usize,
) -> Result<Vec<Sample>, String> {
    let path = engine(&state).and_then(|engine| {
        engine
            .active_session_path()
            .map_err(|error| error.to_string())
    })?;
    query_recording_window(&path, &signal_ids, start_ns, end_ns, max_buckets)
        .map_err(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::import_selected_csv;
    use lognook_core::{LogNookEngine, RecordingWriter, Sample, SampleOrigin, SampleQuality};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn open_csv_cancel_returns_without_changing_session() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("lognook-cancel-{id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("existing.lognook");
        let mut writer = RecordingWriter::create(&path).unwrap();
        writer
            .append(&Sample {
                elapsed_ns: 0,
                wall_time_unix_ns: 1,
                source_id: "test".into(),
                ecu_id: None,
                signal_id: "test_signal".into(),
                value: 1.0,
                unit: "u".into(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            })
            .unwrap();
        writer.finish().unwrap();
        let mut engine = LogNookEngine::new(|_| {});
        engine.load_recording(&path).unwrap();
        assert!(
            import_selected_csv(None, &mut engine, &directory)
                .unwrap()
                .is_none()
        );
        assert_eq!(engine.active_session_path().unwrap(), path);
        drop(engine);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
