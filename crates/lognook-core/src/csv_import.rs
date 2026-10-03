use crate::{
    csv_io::visit_csv_events,
    model::{LogNookError, Marker, RecordingSummary},
    recording::{RecordedEvent, RecordingWriter},
};
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn import_csv_to_recording(
    source: &Path,
    target: &Path,
) -> Result<RecordingSummary, LogNookError> {
    let temporary = temporary_session_path(target);
    let result = (|| {
        let mut writer = RecordingWriter::create(&temporary)?;
        let mut sample_count = 0;
        let mut inline_markers = 0;
        visit_csv_events(File::open(source)?, |event| match event {
            RecordedEvent::Sample(sample) => {
                sample_count += 1;
                writer.append(&sample)
            }
            RecordedEvent::Marker(marker) => {
                inline_markers += 1;
                writer.add_marker(&marker).map(|_| ())
            }
        })?;
        if inline_markers == 0 {
            let sidecar = marker_csv_path(source);
            if sidecar.exists() {
                visit_legacy_markers(File::open(sidecar)?, |marker| {
                    writer.add_marker(&marker).map(|_| ())
                })?;
            }
        }
        if sample_count == 0 {
            return Err(LogNookError::new("CSV contains no samples"));
        }
        let summary = writer.finish()?;
        std::fs::hard_link(&temporary, target)?;
        let _ = std::fs::remove_file(&temporary);
        Ok(RecordingSummary {
            path: target.to_path_buf(),
            sample_count: summary.sample_count,
        })
    })();
    if result.is_err() {
        remove_import_files(&temporary);
    }
    result
}

#[derive(Deserialize)]
struct LegacyCsvMarker {
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
    label: String,
    note: String,
}

fn visit_legacy_markers<R: Read>(
    input: R,
    mut visit: impl FnMut(Marker) -> Result<(), LogNookError>,
) -> Result<(), LogNookError> {
    let mut reader = csv::Reader::from_reader(input);
    for (index, row) in reader.deserialize::<LegacyCsvMarker>().enumerate() {
        let row_number = index + 2;
        let marker = row
            .map_err(|error| {
                LogNookError::new(format!("CSV marker sidecar row {row_number}: {error}"))
            })
            .map(|row| Marker {
                id: None,
                elapsed_ns: row.elapsed_ns,
                wall_time_unix_ns: row.wall_time_unix_ns,
                label: row.label,
                note: row.note,
            })?;
        visit(marker).map_err(|error| {
            LogNookError::new(format!("CSV marker sidecar row {row_number}: {error}"))
        })?;
    }
    Ok(())
}

fn marker_csv_path(csv_path: &Path) -> PathBuf {
    let stem = csv_path.file_stem().unwrap_or_default().to_string_lossy();
    csv_path.with_file_name(format!("{stem}-markers.csv"))
}

fn temporary_session_path(target: &Path) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(
        ".{name}.{}-{now}-{id}.importing",
        std::process::id()
    ))
}

fn remove_import_files(path: &Path) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    for path in [
        path.to_path_buf(),
        path.with_file_name(format!("{name}-wal")),
        path.with_file_name(format!("{name}-shm")),
    ] {
        let _ = std::fs::remove_file(path);
    }
}
