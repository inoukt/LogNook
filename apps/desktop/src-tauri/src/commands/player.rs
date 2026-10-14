use super::sessions::open_selected_recording;
use super::{engine, session_path};
use crate::AppState;
use lognook_core::{
    Marker, PlaybackState, RecordingInfo, Sample, import_csv_to_recording,
    query_recording_averages, query_recording_values_at, query_recording_window,
};
use std::path::{Path, PathBuf};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn open_session_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<RecordingInfo>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("LogNook sessions and CSV", &["lognook", "csv"])
        .blocking_pick_file()
        .map(|file| file.into_path().map_err(|error| error.to_string()))
        .transpose()?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected;
    let mut engine = engine(&state)?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("lognook") => {
            open_selected_recording(&path, &mut engine).map(Some)
        }
        Some(extension) if extension.eq_ignore_ascii_case("csv") => {
            import_selected_csv(Some(path), &mut engine, &state.recordings_dir)
        }
        _ => Err("Choose a .lognook recording or CSV file".into()),
    }
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
pub async fn load_last_recording(state: State<'_, AppState>) -> Result<RecordingInfo, String> {
    let mut engine = engine(&state)?;
    let path = engine
        .last_recording_path()
        .ok_or_else(|| "No completed recording is available".to_owned())?;
    engine
        .load_recording(&path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn close_session(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?.close_session();
    Ok(())
}

#[tauri::command]
pub async fn start_replay(state: State<'_, AppState>, speed: f64) -> Result<(), String> {
    engine(&state)?
        .start_replay(speed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pause_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .pause_replay()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resume_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .resume_replay()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn seek_replay(state: State<'_, AppState>, position_ns: i64) -> Result<(), String> {
    engine(&state)?
        .seek_replay(position_ns)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_replay_speed(state: State<'_, AppState>, speed: f64) -> Result<(), String> {
    engine(&state)?
        .set_replay_speed(speed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn playback_state(state: State<'_, AppState>) -> Result<PlaybackState, String> {
    engine(&state)?
        .playback_state()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn add_playback_marker(
    state: State<'_, AppState>,
    label: String,
    note: String,
) -> Result<Marker, String> {
    engine(&state)?
        .add_playback_marker(&label, &note)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_playback_marker(
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
pub async fn delete_playback_marker(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    engine(&state)?
        .delete_playback_marker(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn recording_window(
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

#[tauri::command]
pub async fn recording_values_at(
    state: State<'_, AppState>,
    signal_ids: Vec<String>,
    position_ns: i64,
) -> Result<Vec<Sample>, String> {
    let path = engine(&state).and_then(|engine| {
        engine
            .active_session_path()
            .map_err(|error| error.to_string())
    })?;
    query_recording_values_at(&path, &signal_ids, position_ns).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn recording_averages(
    state: State<'_, AppState>,
    start_ns: i64,
    end_ns: i64,
) -> Result<std::collections::HashMap<String, f64>, String> {
    let path = engine(&state).and_then(|engine| {
        engine
            .active_session_path()
            .map_err(|error| error.to_string())
    })?;
    query_recording_averages(&path, start_ns, end_ns).map_err(|error| error.to_string())
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
