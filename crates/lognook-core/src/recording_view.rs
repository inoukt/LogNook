use crate::{
    model::{LogNookError, Marker, RecordingInfo, RecordingSignal},
    recording::open_recording,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;

pub fn inspect_recording(path: &Path) -> Result<RecordingInfo, LogNookError> {
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let summary = recording_summary(path)?;
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
    let duration_ns = markers
        .iter()
        .map(|marker| marker.elapsed_ns)
        .max()
        .unwrap_or_default()
        .max(summary.duration_ns);
    Ok(RecordingInfo {
        sample_count: summary.sample_count,
        duration_ns,
        signals: summary.signals,
        markers,
    })
}

pub(crate) fn recording_duration_ns(path: &Path) -> Result<i64, LogNookError> {
    let summary = recording_summary(path)?;
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let marker_duration: i64 = connection.query_row(
        "SELECT COALESCE((SELECT elapsed_ns FROM markers
         ORDER BY elapsed_ns DESC LIMIT 1), 0)",
        [],
        |row| row.get(0),
    )?;
    Ok(summary.duration_ns.max(marker_duration))
}

#[derive(Debug)]
struct RecordingSampleSummary {
    sample_count: usize,
    duration_ns: i64,
    signals: Vec<RecordingSignal>,
}

fn recording_summary(path: &Path) -> Result<RecordingSampleSummary, LogNookError> {
    let read_only = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    if let Some(summary) = read_cached_summary(&read_only)? {
        return Ok(summary);
    }
    if summary_is_incomplete(&read_only)? {
        return scan_sample_summary(&read_only);
    }
    drop(read_only);

    let Ok(mut writable) = open_recording(path, OpenFlags::SQLITE_OPEN_READ_WRITE) else {
        let read_only = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        return match read_cached_summary(&read_only)? {
            Some(summary) => Ok(summary),
            None => scan_sample_summary(&read_only),
        };
    };
    let upgraded = (|| -> Result<RecordingSampleSummary, LogNookError> {
        let transaction = writable.transaction()?;
        transaction.execute_batch(
            "CREATE INDEX IF NOT EXISTS samples_by_signal_time
               ON samples(signal_id, elapsed_ns DESC, id DESC);
             CREATE INDEX IF NOT EXISTS markers_by_time ON markers(elapsed_ns DESC);",
        )?;
        if let Some(summary) = read_cached_summary(&transaction)? {
            transaction.commit()?;
            return Ok(summary);
        }
        if summary_is_incomplete(&transaction)? {
            let summary = scan_sample_summary(&transaction)?;
            transaction.commit()?;
            return Ok(summary);
        }
        let summary = scan_sample_summary(&transaction)?;
        write_cached_summary(&transaction, &summary)?;
        transaction.commit()?;
        Ok(summary)
    })();
    match upgraded {
        Ok(summary) => Ok(summary),
        Err(_) => {
            drop(writable);
            let read_only = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            match read_cached_summary(&read_only)? {
                Some(summary) => Ok(summary),
                None => scan_sample_summary(&read_only),
            }
        }
    }
}

fn summary_is_incomplete(connection: &Connection) -> Result<bool, LogNookError> {
    Ok(metadata_value(connection, "summary_complete")?.as_deref() == Some("0"))
}

fn read_cached_summary(
    connection: &Connection,
) -> Result<Option<RecordingSampleSummary>, LogNookError> {
    if metadata_value(connection, "summary_complete")?.as_deref() != Some("1") {
        return Ok(None);
    }
    let Some(sample_count) =
        metadata_value(connection, "sample_count")?.and_then(|value| value.parse::<usize>().ok())
    else {
        return Ok(None);
    };
    let Some(duration_ns) = metadata_value(connection, "sample_duration_ns")?
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return Ok(None);
    };
    let Some(signals) = metadata_value(connection, "signals_json")?
        .and_then(|value| serde_json::from_str::<Vec<RecordingSignal>>(&value).ok())
    else {
        return Ok(None);
    };
    Ok(Some(RecordingSampleSummary {
        sample_count,
        duration_ns,
        signals,
    }))
}

fn metadata_value(connection: &Connection, key: &str) -> Result<Option<String>, LogNookError> {
    Ok(connection
        .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

fn scan_sample_summary(connection: &Connection) -> Result<RecordingSampleSummary, LogNookError> {
    let sample_count: i64 =
        connection.query_row("SELECT COUNT(*) FROM samples", [], |row| row.get(0))?;
    let duration_ns: i64 = connection.query_row(
        "SELECT COALESCE(MAX(elapsed_ns), 0) FROM samples",
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
    Ok(RecordingSampleSummary {
        sample_count: sample_count.max(0) as usize,
        duration_ns: duration_ns.max(0),
        signals,
    })
}

fn write_cached_summary(
    connection: &Connection,
    summary: &RecordingSampleSummary,
) -> Result<(), LogNookError> {
    let signals_json = serde_json::to_string(&summary.signals)
        .map_err(|error| LogNookError::new(error.to_string()))?;
    for (key, value) in [
        ("sample_count", summary.sample_count.to_string()),
        ("sample_duration_ns", summary.duration_ns.to_string()),
        ("signals_json", signals_json),
        ("summary_complete", "1".to_owned()),
    ] {
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::inspect_recording;
    use crate::{RecordingWriter, Sample, SampleOrigin, SampleQuality};
    use rusqlite::Connection;
    use std::{path::PathBuf, time::SystemTime};

    fn temp_recording() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-summary-{nonce}.lognook"));
        let mut writer = RecordingWriter::create(&path).unwrap();
        for (signal_id, unit, elapsed_ns) in [
            ("rpm", "rpm", 10),
            ("speed", "km/h", 30),
            ("rpm", "rpm", 20),
        ] {
            writer
                .append(&Sample {
                    elapsed_ns,
                    wall_time_unix_ns: elapsed_ns,
                    source_id: "test".into(),
                    ecu_id: None,
                    signal_id: signal_id.into(),
                    value: elapsed_ns as f64,
                    unit: unit.into(),
                    quality: SampleQuality::Valid,
                    origin: SampleOrigin::Live,
                    raw_bytes: None,
                })
                .unwrap();
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn finalized_recording_inspection_uses_cached_sample_summary() {
        let path = temp_recording();

        let info = inspect_recording(&path).unwrap();

        assert_eq!(info.sample_count, 3);
        assert_eq!(info.duration_ns, 30);
        assert_eq!(
            info.signals
                .iter()
                .map(|signal| (signal.id.as_str(), signal.unit.as_str()))
                .collect::<Vec<_>>(),
            [("rpm", "rpm"), ("speed", "km/h")]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_recording_is_summarized_and_indexed_once() {
        let path = temp_recording();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DELETE FROM metadata WHERE key IN
                   ('summary_complete', 'sample_count', 'sample_duration_ns', 'signals_json');
                 DROP INDEX IF EXISTS samples_by_signal_time;
                 DROP INDEX IF EXISTS markers_by_time;",
            )
            .unwrap();
        drop(connection);

        let info = inspect_recording(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        let cached_count: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'sample_count'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let signal_index: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'index' AND name = 'samples_by_signal_time')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let marker_index: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'index' AND name = 'markers_by_time')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(info.sample_count, 3);
        assert_eq!(info.duration_ns, 30);
        assert_eq!(cached_count, "3");
        assert!(signal_index);
        assert!(marker_index);
        drop(connection);
        std::fs::remove_file(path).unwrap();
    }
}
