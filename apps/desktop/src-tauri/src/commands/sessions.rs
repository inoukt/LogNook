use super::engine;
use crate::AppState;
use lognook_core::{RecordingInfo, export_recording_csv, inspect_recording};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

type BrowserRecordingRow = (String, String, u64, usize);

#[tauri::command]
pub async fn list_recordings(
    state: State<'_, AppState>,
) -> Result<Vec<BrowserRecordingRow>, String> {
    list_recordings_in(&state.recordings_dir)
}

fn list_recordings_in(recordings_dir: &Path) -> Result<Vec<BrowserRecordingRow>, String> {
    let recordings_dir = recordings_dir
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut recordings = Vec::new();
    for entry in std::fs::read_dir(&recordings_dir).map_err(|error| error.to_string())? {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else { continue };
        let path = entry.path();
        if !file_type.is_file()
            || path
                .extension()
                .is_none_or(|extension| extension != "lognook")
        {
            continue;
        }
        let Ok(info) = inspect_recording(&path) else { continue };
        let modified_unix_ms = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or_default();
        recordings.push((
            path.to_string_lossy().into_owned(),
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            modified_unix_ms,
            info.sample_count,
        ));
    }
    recordings.sort_by(|left, right| {
        right.2.cmp(&left.2).then_with(|| left.1.cmp(&right.1))
    });
    Ok(recordings)
}

#[tauri::command]
pub async fn open_recording(
    state: State<'_, AppState>,
    path: String,
) -> Result<RecordingInfo, String> {
    let mut engine = engine(&state)?;
    open_selected_recording(Path::new(&path), &mut engine)
}

#[tauri::command]
pub async fn share_recording_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<String>, String> {
    let source = selected_recording_path(&state.recordings_dir, Path::new(&path))?;
    let name = format!(
        "{}.csv",
        source.file_stem().unwrap_or_default().to_string_lossy()
    );
    let selected = app
        .dialog()
        .file()
        .add_filter("CSV files", &["csv"])
        .set_file_name(name)
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let destination = selected.into_path().map_err(|error| error.to_string())?;
    let destination = share_recording_csv_in(&state.recordings_dir, &source, &destination)?;
    Ok(Some(destination.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn save_recording_as(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<String>, String> {
    let source = selected_recording_path(&state.recordings_dir, Path::new(&path))?;
    let name = format!(
        "{}.lognook",
        source.file_stem().unwrap_or_default().to_string_lossy()
    );
    let selected = app
        .dialog()
        .file()
        .add_filter("LogNook sessions", &["lognook"])
        .set_file_name(name)
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let destination = selected.into_path().map_err(|error| error.to_string())?;
    let destination = save_recording_as_in(&state.recordings_dir, &source, &destination)?;
    Ok(Some(destination.to_string_lossy().into_owned()))
}

fn same_directory(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        let left = left.to_string_lossy();
        let right = right.to_string_lossy();
        left.as_ref().eq_ignore_ascii_case(right.as_ref())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn selected_recording_path(recordings_dir: &Path, selected_path: &Path) -> Result<PathBuf, String> {
    let recordings_dir = recordings_dir
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let selected_path = native_recording_path(selected_path)?;
    if !selected_path
        .parent()
        .is_some_and(|parent| same_directory(parent, &recordings_dir))
    {
        return Err("Selected recording is outside the app recordings directory".into());
    }
    Ok(selected_path)
}

fn native_recording_path(selected_path: &Path) -> Result<PathBuf, String> {
    let selected_path = selected_path
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !selected_path.is_file()
        || selected_path
            .extension()
            .is_none_or(|extension| extension != "lognook")
    {
        return Err("Selected file is not a native .lognook recording".into());
    }
    Ok(selected_path)
}

fn share_recording_csv_in(
    recordings_dir: &Path,
    selected_path: &Path,
    csv_path: &Path,
) -> Result<PathBuf, String> {
    let source = selected_recording_path(recordings_dir, selected_path)?;
    let mut csv_path = csv_path.to_path_buf();
    csv_path.set_extension("csv");
    export_recording_csv(&source, &csv_path).map_err(|error| error.to_string())?;
    Ok(csv_path)
}

fn save_recording_as_in(
    recordings_dir: &Path,
    selected_path: &Path,
    target_path: &Path,
) -> Result<PathBuf, String> {
    let source = selected_recording_path(recordings_dir, selected_path)?;
    let mut target = target_path.to_path_buf();
    target.set_extension("lognook");
    let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
    if canonical_target == source {
        return Err("Choose a different location for the session copy".into());
    }
    std::fs::copy(source, &target).map_err(|error| error.to_string())?;
    Ok(target)
}

pub(super) fn open_selected_recording(
    selected_path: &Path,
    engine: &mut lognook_core::LogNookEngine,
) -> Result<RecordingInfo, String> {
    if engine.is_connected() || engine.is_recording() {
        return Err("Disconnect and stop recording before opening a session".into());
    }
    let selected_path = native_recording_path(selected_path)?;
    engine
        .load_recording(&selected_path)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        list_recordings_in, open_selected_recording, save_recording_as_in, share_recording_csv_in,
    };
    #[cfg(windows)]
    use super::same_directory;
    use lognook_core::{LogNookEngine, RecordingWriter, Sample, SampleOrigin, SampleQuality};
    use std::{
        path::{Path, PathBuf},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn create_recording(path: &Path, sample_count: usize) {
        let mut writer = RecordingWriter::create(path).unwrap();
        for index in 0..sample_count {
            writer
                .append(&Sample {
                    elapsed_ns: index as i64,
                    wall_time_unix_ns: index as i64,
                    source_id: "test".into(),
                    ecu_id: None,
                    signal_id: "test_signal".into(),
                    value: index as f64,
                    unit: "u".into(),
                    quality: SampleQuality::Valid,
                    origin: SampleOrigin::Live,
                    raw_bytes: None,
                })
                .unwrap();
        }
        writer.finish().unwrap();
    }

    fn test_directory(label: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lognook-{label}-{id}"))
    }

    #[cfg(windows)]
    #[test]
    fn directory_comparison_ignores_windows_path_casing() {
        assert!(same_directory(
            Path::new(r"C:\Users\inouk\AppData\recordings"),
            Path::new(r"c:\users\inouk\appdata\Recordings"),
        ));
    }

    #[test]
    fn lists_valid_native_recordings_newest_first_with_sample_counts() {
        let directory = test_directory("session-browser");
        std::fs::create_dir_all(&directory).unwrap();
        create_recording(&directory.join("older.lognook"), 1);
        thread::sleep(Duration::from_millis(20));
        create_recording(&directory.join("newer.lognook"), 2);
        std::fs::write(directory.join("notes.csv"), "not a native recording").unwrap();
        std::fs::write(directory.join("unfinished.lognook"), "not a database").unwrap();

        let recordings = list_recordings_in(&directory).unwrap();

        assert_eq!(
            recordings.iter().map(|recording| recording.1.as_str()).collect::<Vec<_>>(),
            ["newer", "older"],
        );
        assert_eq!(recordings[0].3, 2);
        assert_eq!(recordings[1].3, 1);
        assert!(recordings[0].2 > recordings[1].2);
        let canonical_directory = directory.canonicalize().unwrap();
        assert_eq!(
            Path::new(&recordings[0].0).parent(),
            Some(canonical_directory.as_path()),
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn opens_native_recordings_inside_the_app_directory() {
        let directory = test_directory("session-open");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("selected.lognook");
        create_recording(&path, 1);
        let mut engine = LogNookEngine::new(|_| {});

        let info = open_selected_recording(&path, &mut engine).unwrap();

        assert_eq!(info.sample_count, 1);
        assert_eq!(engine.active_session_path().unwrap(), path.canonicalize().unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn opens_native_recordings_outside_the_app_directory() {
        let directory = test_directory("session-boundary");
        let outside = test_directory("session-outside");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let path = outside.join("outside.lognook");
        create_recording(&path, 1);
        let mut engine = LogNookEngine::new(|_| {});

        let info = open_selected_recording(&path, &mut engine).unwrap();
        assert_eq!(info.sample_count, 1);
        assert_eq!(engine.active_session_path().unwrap(), path.canonicalize().unwrap());
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn save_as_copies_a_native_recording_without_changing_the_source() {
        let directory = test_directory("session-save-as");
        let target_directory = test_directory("session-save-as-target");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::create_dir_all(&target_directory).unwrap();
        let source = directory.join("selected.lognook");
        let target = target_directory.join("copy.lognook");
        create_recording(&source, 3);

        let saved = save_recording_as_in(&directory, &source, &target).unwrap();

        assert_eq!(saved, target);
        assert_eq!(
            lognook_core::inspect_recording(&saved)
                .unwrap()
                .sample_count,
            3
        );
        assert_eq!(
            lognook_core::inspect_recording(&source)
                .unwrap()
                .sample_count,
            3
        );
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(target_directory).unwrap();
    }

    #[test]
    fn share_csv_exports_the_selected_session_and_rejects_external_sources() {
        let directory = test_directory("session-share");
        let outside = test_directory("session-share-outside");
        let output_directory = test_directory("session-share-output");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&output_directory).unwrap();
        let source = directory.join("selected.lognook");
        let external = outside.join("external.lognook");
        let csv = output_directory.join("selected.csv");
        create_recording(&source, 2);
        create_recording(&external, 2);

        let exported = share_recording_csv_in(&directory, &source, &csv).unwrap();

        assert_eq!(exported, csv);
        let contents = std::fs::read_to_string(&csv).unwrap();
        assert!(contents.contains("signal_key"));
        assert!(contents.contains("test_signal"));
        assert!(share_recording_csv_in(
            &directory,
            &external,
            &output_directory.join("bad.csv")
        )
        .is_err());
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
        std::fs::remove_dir_all(output_directory).unwrap();
    }
}
