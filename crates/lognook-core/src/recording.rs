use crate::model::{
    LogNookError, Marker, Recording, RecordingSummary, Sample, SampleOrigin, SampleQuality,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    path::{Path, PathBuf},
};

pub struct RecordingWriter {
    connection: Connection,
    path: PathBuf,
    sample_count: usize,
    duration_ns: i64,
    signals: BTreeSet<(String, String)>,
}

impl RecordingWriter {
    pub fn create(path: &Path) -> Result<Self, LogNookError> {
        OpenOptions::new().write(true).create_new(true).open(path)?;
        let connection = match Connection::open(path) {
            Ok(connection) => connection,
            Err(error) => {
                let _ = std::fs::remove_file(path);
                return Err(error.into());
            }
        };
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(
            "CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO metadata(key, value) VALUES ('schema_version', '1');
             INSERT INTO metadata(key, value) VALUES ('summary_complete', '0');
             CREATE TABLE samples(
               id INTEGER PRIMARY KEY, elapsed_ns INTEGER NOT NULL, wall_time_unix_ns INTEGER NOT NULL,
               source_id TEXT NOT NULL, ecu_id TEXT, signal_id TEXT NOT NULL, value REAL NOT NULL,
               unit TEXT NOT NULL, quality TEXT NOT NULL, origin TEXT NOT NULL, raw_bytes BLOB
             );
             CREATE INDEX samples_by_time ON samples(elapsed_ns, id);
             CREATE INDEX samples_by_signal_time ON samples(signal_id, elapsed_ns DESC, id DESC);
             CREATE TABLE markers(
               id INTEGER PRIMARY KEY, elapsed_ns INTEGER NOT NULL, wall_time_unix_ns INTEGER NOT NULL,
               label TEXT NOT NULL, note TEXT NOT NULL
             );
             CREATE INDEX markers_by_time ON markers(elapsed_ns DESC);",
        )?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            sample_count: 0,
            duration_ns: 0,
            signals: BTreeSet::new(),
        })
    }

    pub fn append(&mut self, sample: &Sample) -> Result<(), LogNookError> {
        if sample.elapsed_ns < 0 {
            return Err(LogNookError::new(
                "Sample elapsed time must be non-negative",
            ));
        }
        if !sample.value.is_finite() {
            return Err(LogNookError::new("Sample value must be finite"));
        }
        self.connection.execute(
            "INSERT INTO samples(elapsed_ns, wall_time_unix_ns, source_id, ecu_id, signal_id,
             value, unit, quality, origin, raw_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![sample.elapsed_ns, sample.wall_time_unix_ns, sample.source_id, sample.ecu_id,
                sample.signal_id, sample.value, sample.unit, sample.quality.as_db_str(),
                sample.origin.as_db_str(), sample.raw_bytes.as_deref()],
        )?;
        self.sample_count += 1;
        self.duration_ns = self.duration_ns.max(sample.elapsed_ns);
        self.signals
            .insert((sample.signal_id.clone(), sample.unit.clone()));
        Ok(())
    }

    pub fn add_marker(&mut self, marker: &Marker) -> Result<i64, LogNookError> {
        if marker.elapsed_ns < 0 {
            return Err(LogNookError::new(
                "Marker elapsed time must be non-negative",
            ));
        }
        self.connection.execute(
            "INSERT INTO markers(elapsed_ns, wall_time_unix_ns, label, note) VALUES (?1, ?2, ?3, ?4)",
            params![marker.elapsed_ns, marker.wall_time_unix_ns, marker.label, marker.note],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn finish(mut self) -> Result<RecordingSummary, LogNookError> {
        let signals = self
            .signals
            .into_iter()
            .map(|(id, unit)| crate::model::RecordingSignal { id, unit })
            .collect::<Vec<_>>();
        let signals_json = serde_json::to_string(&signals)
            .map_err(|error| LogNookError::new(error.to_string()))?;
        let transaction = self.connection.transaction()?;
        for (key, value) in [
            ("sample_count", self.sample_count.to_string()),
            ("sample_duration_ns", self.duration_ns.to_string()),
            ("signals_json", signals_json),
            ("summary_complete", "1".to_owned()),
        ] {
            transaction.execute(
                "INSERT INTO metadata(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(LogNookError::new(format!(
                "Recording integrity check failed: {integrity}"
            )));
        }
        self.connection
            .pragma_update(None, "journal_mode", "DELETE")?;
        Ok(RecordingSummary {
            path: self.path,
            sample_count: self.sample_count,
        })
    }
}

pub(crate) enum RecordedEvent {
    Sample(Sample),
    Marker(Marker),
}

#[derive(Clone, Copy)]
pub(crate) struct EventCursor {
    pub elapsed_ns: i64,
    pub kind: i64,
    pub id: i64,
}

pub(crate) fn open_recording(path: &Path, flags: OpenFlags) -> Result<Connection, LogNookError> {
    let connection = Connection::open_with_flags(path, flags)?;
    let version: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if version.as_deref() != Some("1") {
        return Err(LogNookError::new(
            "Unsupported or missing .lognook schema version",
        ));
    }
    Ok(connection)
}

pub(crate) fn visit_recording_events(
    path: &Path,
    mut visit: impl FnMut(RecordedEvent) -> Result<bool, LogNookError>,
) -> Result<bool, LogNookError> {
    visit_recording_events_from(path, 0, None, |event, _| visit(event))
}

pub(crate) fn visit_recording_events_from(
    path: &Path,
    start_ns: i64,
    cursor: Option<EventCursor>,
    mut visit: impl FnMut(RecordedEvent, EventCursor) -> Result<bool, LogNookError>,
) -> Result<bool, LogNookError> {
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let selection = "SELECT kind, id, elapsed_ns, wall_time_unix_ns, source_id, ecu_id, signal_id,
         value, unit, quality, origin, raw_bytes, label, note FROM (
           SELECT 0 AS kind, id, elapsed_ns, wall_time_unix_ns, source_id, ecu_id,
             signal_id, value, unit, quality, origin, raw_bytes, NULL AS label, NULL AS note
             FROM samples
           UNION ALL
           SELECT 1 AS kind, id, elapsed_ns, wall_time_unix_ns, NULL, NULL, NULL,
             NULL, NULL, NULL, NULL, NULL, label, note FROM markers
         ) WHERE elapsed_ns >= ?1";
    let query = if cursor.is_some() {
        format!(
            "{selection} AND (elapsed_ns > ?2 OR (elapsed_ns = ?2 AND (kind > ?3 OR (kind = ?3 AND id > ?4)))) ORDER BY elapsed_ns, kind, id"
        )
    } else {
        format!("{selection} ORDER BY elapsed_ns, kind, id")
    };
    let mut statement = connection.prepare(&query)?;
    let map_row = |row: &rusqlite::Row<'_>| {
        Ok(RecordedEventRow {
            kind: row.get(0)?,
            id: row.get(1)?,
            elapsed_ns: row.get(2)?,
            wall_time_unix_ns: row.get(3)?,
            source_id: row.get(4)?,
            ecu_id: row.get(5)?,
            signal_id: row.get(6)?,
            value: row.get(7)?,
            unit: row.get(8)?,
            quality: row.get(9)?,
            origin: row.get(10)?,
            raw_bytes: row.get(11)?,
            label: row.get(12)?,
            note: row.get(13)?,
        })
    };
    let rows = if let Some(cursor) = cursor {
        statement.query_map(
            rusqlite::params![start_ns, cursor.elapsed_ns, cursor.kind, cursor.id],
            map_row,
        )?
    } else {
        statement.query_map([start_ns], map_row)?
    };
    for row in rows {
        let row = row?;
        let cursor = EventCursor {
            elapsed_ns: row.elapsed_ns,
            kind: row.kind,
            id: row.id,
        };
        let event = row.into_event()?;
        if !visit(event, cursor)? {
            return Ok(false);
        }
    }
    Ok(true)
}

struct RecordedEventRow {
    kind: i64,
    id: i64,
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
    source_id: Option<String>,
    ecu_id: Option<String>,
    signal_id: Option<String>,
    value: Option<f64>,
    unit: Option<String>,
    quality: Option<String>,
    origin: Option<String>,
    raw_bytes: Option<Vec<u8>>,
    label: Option<String>,
    note: Option<String>,
}

impl RecordedEventRow {
    fn into_event(self) -> Result<RecordedEvent, LogNookError> {
        if self.kind == 0 {
            let quality = required(self.quality, "quality")?;
            let origin = required(self.origin, "origin")?;
            Ok(RecordedEvent::Sample(Sample {
                elapsed_ns: self.elapsed_ns,
                wall_time_unix_ns: self.wall_time_unix_ns,
                source_id: required(self.source_id, "source_id")?,
                ecu_id: self.ecu_id,
                signal_id: required(self.signal_id, "signal_id")?,
                value: required(self.value, "value")?,
                unit: required(self.unit, "unit")?,
                quality: SampleQuality::from_db(&quality)
                    .ok_or_else(|| LogNookError::new("Invalid sample quality in recording"))?,
                origin: SampleOrigin::from_db(&origin)
                    .ok_or_else(|| LogNookError::new("Invalid sample origin in recording"))?,
                raw_bytes: self.raw_bytes,
            }))
        } else {
            Ok(RecordedEvent::Marker(Marker {
                id: Some(self.id),
                elapsed_ns: self.elapsed_ns,
                wall_time_unix_ns: self.wall_time_unix_ns,
                label: required(self.label, "marker label")?,
                note: required(self.note, "marker note")?,
            }))
        }
    }
}

fn required<T>(value: Option<T>, name: &str) -> Result<T, LogNookError> {
    value.ok_or_else(|| LogNookError::new(format!("Missing {name} in recording")))
}

pub fn read_recording(path: &Path) -> Result<Recording, LogNookError> {
    let mut recording = Recording {
        samples: Vec::new(),
        markers: Vec::new(),
    };
    visit_recording_events(path, |event| {
        match event {
            RecordedEvent::Sample(sample) => recording.samples.push(sample),
            RecordedEvent::Marker(marker) => recording.markers.push(marker),
        }
        Ok(true)
    })?;
    Ok(recording)
}
