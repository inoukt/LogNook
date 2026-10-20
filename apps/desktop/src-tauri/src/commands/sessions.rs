use super::engine;
use crate::AppState;
use lognook_core::{
    RecordingInfo, RecordingVehicleContextSummary, export_recording_csv, inspect_recording,
};
use std::process::Command;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

type BrowserRecordingRow = (
    String,
    String,
    u64,
    usize,
    Option<RecordingVehicleContextSummary>,
);

#[tauri::command]
pub async fn list_recordings(
    state: State<'_, AppState>,
) -> Result<Vec<BrowserRecordingRow>, String> {
    list_recordings_in(&state.recordings_dir)
}

#[tauri::command]
pub async fn rename_recording(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<(), String> {
    let mut engine = engine(&state)?;
    rename_recording_in(&state.recordings_dir, Path::new(&path), &name, &mut engine)
}

#[tauri::command]
pub async fn delete_recording(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let mut engine = engine(&state)?;
    delete_recording_in(&state.recordings_dir, Path::new(&path), &mut engine)
}

#[tauri::command]
pub async fn open_recording_folder(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let recording = selected_recording_path(&state.recordings_dir, Path::new(&path))?;
    inspect_recording(&recording).map_err(|error| error.to_string())?;
    let folder = recording.parent().ok_or("Recording has no parent folder")?;
    #[cfg(windows)]
    {
        Command::new("explorer.exe")
            .arg(folder)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(folder)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = folder;
        Err("Opening recording folders is unavailable on this platform".into())
    }
}

fn list_recordings_in(recordings_dir: &Path) -> Result<Vec<BrowserRecordingRow>, String> {
    let recordings_dir = recordings_dir
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut recordings = Vec::new();
    for entry in std::fs::read_dir(&recordings_dir).map_err(|error| error.to_string())? {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if !file_type.is_file()
            || path
                .extension()
                .is_none_or(|extension| extension != "lognook")
        {
            continue;
        }
        let Ok(info) = inspect_recording(&path) else {
            continue;
        };
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
            info.vehicle_context
                .as_ref()
                .map(RecordingVehicleContextSummary::from),
        ));
    }
    recordings.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.1.cmp(&right.1)));
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

pub(super) fn selected_recording_path(
    recordings_dir: &Path,
    selected_path: &Path,
) -> Result<PathBuf, String> {
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

pub(super) fn native_recording_path(selected_path: &Path) -> Result<PathBuf, String> {
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

fn recording_for_mutation(
    recordings_dir: &Path,
    selected_path: &Path,
    engine: &lognook_core::LogNookEngine,
) -> Result<PathBuf, String> {
    if engine.is_recording() {
        return Err("Stop recording before changing a session".into());
    }
    let source = selected_recording_path(recordings_dir, selected_path)?;
    if engine
        .active_session_path()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .is_some_and(|active| same_directory(&active, &source))
    {
        return Err("Close the loaded session before changing it".into());
    }
    inspect_recording(&source).map_err(|error| error.to_string())?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{}", source.to_string_lossy(), suffix));
        if sidecar.exists() {
            return Err(
                "Session has active SQLite sidecar files; close it before changing it".into(),
            );
        }
    }
    Ok(source)
}

fn valid_recording_name(name: &str) -> bool {
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !name.is_empty()
        && !name.to_ascii_lowercase().ends_with(".lognook")
        && name.trim() == name
        && !name.ends_with('.')
        && !name
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
        && !reserved.contains(&device.as_str())
        && !["COM", "LPT"].iter().any(|prefix| {
            device.strip_prefix(prefix).is_some_and(|digit| {
                matches!(
                    digit,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

fn rename_recording_in(
    recordings_dir: &Path,
    selected_path: &Path,
    name: &str,
    engine: &lognook_core::LogNookEngine,
) -> Result<(), String> {
    if !valid_recording_name(name) {
        return Err("Enter a valid filename without a .lognook extension".into());
    }
    let source = recording_for_mutation(recordings_dir, selected_path, engine)?;
    let target = source.with_file_name(format!("{name}.lognook"));
    if same_directory(&source, &target) {
        return Err("Enter a different filename".into());
    }
    if target.exists() {
        return Err("A recording with that filename already exists".into());
    }
    std::fs::rename(source, target).map_err(|error| error.to_string())
}

fn delete_recording_in(
    recordings_dir: &Path,
    selected_path: &Path,
    engine: &lognook_core::LogNookEngine,
) -> Result<(), String> {
    let source = recording_for_mutation(recordings_dir, selected_path, engine)?;
    std::fs::remove_file(source).map_err(|error| error.to_string())
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
    #[cfg(windows)]
    use super::same_directory;
    use super::{
        delete_recording_in, list_recordings_in, open_selected_recording, rename_recording_in,
        save_recording_as_in, share_recording_csv_in, valid_recording_name,
    };
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
            recordings
                .iter()
                .map(|recording| recording.1.as_str())
                .collect::<Vec<_>>(),
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
        assert_eq!(
            engine.active_session_path().unwrap(),
            path.canonicalize().unwrap()
        );
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
        assert_eq!(
            engine.active_session_path().unwrap(),
            path.canonicalize().unwrap()
        );
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
    fn renames_and_deletes_only_valid_inactive_native_recordings() {
        let directory = test_directory("session-manage");
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("before.lognook");
        let target = directory.join("after.lognook");
        create_recording(&source, 1);
        let engine = LogNookEngine::new(|_| {});

        rename_recording_in(&directory, &source, "after", &engine).unwrap();

        assert!(target.is_file());
        assert_eq!(
            lognook_core::inspect_recording(&target)
                .unwrap()
                .sample_count,
            1
        );
        assert!(rename_recording_in(&directory, &target, "after", &engine).is_err());
        assert!(rename_recording_in(&directory, &target, "CON", &engine).is_err());
        assert!(rename_recording_in(&directory, &target, "bad/name", &engine).is_err());
        delete_recording_in(&directory, &target, &engine).unwrap();
        assert!(!target.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refuses_to_mutate_loaded_recordings_or_sqlite_files_with_sidecars() {
        let directory = test_directory("session-manage-guard");
        std::fs::create_dir_all(&directory).unwrap();
        let loaded = directory.join("loaded.lognook");
        let other = directory.join("other.lognook");
        create_recording(&loaded, 1);
        create_recording(&other, 1);
        let mut engine = LogNookEngine::new(|_| {});
        engine.load_recording(&loaded).unwrap();

        assert!(
            delete_recording_in(&directory, &loaded, &engine)
                .unwrap_err()
                .contains("Close the loaded session")
        );
        std::fs::write(format!("{}-wal", other.display()), b"pending").unwrap();
        assert!(
            rename_recording_in(&directory, &other, "renamed", &engine)
                .unwrap_err()
                .contains("sidecar")
        );
        assert!(loaded.is_file());
        assert!(other.is_file());
        engine.close_session();
        rename_recording_in(&directory, &loaded, "unloaded", &engine).unwrap();
        delete_recording_in(&directory, &directory.join("unloaded.lognook"), &engine).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_windows_reserved_and_path_like_names() {
        for name in [
            "",
            "..",
            "NUL.txt",
            "COM1",
            "LPT9",
            "COM¹",
            "name.lognook",
            "foo/bar",
            "trailing ",
        ] {
            assert!(!valid_recording_name(name), "accepted {name:?}");
        }
        assert!(valid_recording_name("engine-run 2"));
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
        assert!(
            share_recording_csv_in(&directory, &external, &output_directory.join("bad.csv"))
                .is_err()
        );
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
        std::fs::remove_dir_all(output_directory).unwrap();
    }
}
