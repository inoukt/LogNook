use crate::{
    model::{LogNookError, Marker, RecordingInfo, RecordingSignal},
    recording::open_recording,
};
use rusqlite::{OpenFlags, params};
use std::path::Path;

pub fn inspect_recording(path: &Path) -> Result<RecordingInfo, LogNookError> {
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let sample_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM samples", [], |row| row.get(0))?;
    let duration_ns: i64 = connection.query_row(
        "SELECT COALESCE(MAX(elapsed_ns), 0) FROM (
           SELECT elapsed_ns FROM samples UNION ALL SELECT elapsed_ns FROM markers
         )",
        [],
        |row| row.get(0),
    )?;
    let mut statement = connection
        .prepare("SELECT DISTINCT signal_id, unit FROM samples ORDER BY signal_id, unit")?;
    let signals = statement
        .query_map([], |row| {
            Ok(RecordingSignal {
                id: row.get(0)?,
                unit: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut statement = connection.prepare(
        "SELECT id, elapsed_ns, wall_time_unix_ns, label, note FROM markers ORDER BY elapsed_ns, id",
    )?;
    let markers = statement
        .query_map([], |row| {
            Ok(Marker {
                id: Some(row.get(0)?),
                elapsed_ns: row.get(1)?,
                wall_time_unix_ns: row.get(2)?,
                label: row.get(3)?,
                note: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RecordingInfo {
        sample_count: sample_count as usize,
        duration_ns,
        signals,
        markers,
    })
}

pub fn update_recording_marker(
    path: &Path,
    id: i64,
    label: &str,
    note: &str,
) -> Result<Marker, LogNookError> {
    let mut connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let transaction = connection.transaction()?;
    if transaction.execute(
        "UPDATE markers SET label = ?2, note = ?3 WHERE id = ?1",
        params![id, label, note],
    )? == 0
    {
        return Err(LogNookError::new("Marker not found"));
    }
    let marker = transaction.query_row(
        "SELECT elapsed_ns, wall_time_unix_ns, label, note FROM markers WHERE id = ?1",
        [id],
        |row| {
            Ok(Marker {
                id: Some(id),
                elapsed_ns: row.get(0)?,
                wall_time_unix_ns: row.get(1)?,
                label: row.get(2)?,
                note: row.get(3)?,
            })
        },
    )?;
    transaction.commit()?;
    Ok(marker)
}

pub fn add_recording_marker(path: &Path, marker: &Marker) -> Result<Marker, LogNookError> {
    if marker.elapsed_ns < 0 {
        return Err(LogNookError::new(
            "Marker elapsed time must be non-negative",
        ));
    }
    let mut connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO markers(elapsed_ns, wall_time_unix_ns, label, note) VALUES (?1, ?2, ?3, ?4)",
        params![
            marker.elapsed_ns,
            marker.wall_time_unix_ns,
            marker.label,
            marker.note
        ],
    )?;
    let saved = Marker {
        id: Some(transaction.last_insert_rowid()),
        ..marker.clone()
    };
    transaction.commit()?;
    Ok(saved)
}

pub fn delete_recording_marker(path: &Path, id: i64) -> Result<(), LogNookError> {
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    if connection.execute("DELETE FROM markers WHERE id = ?1", [id])? == 0 {
        return Err(LogNookError::new("Marker not found"));
    }
    Ok(())
}
