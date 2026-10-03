use crate::{
    model::{
        LogNookError, Marker, RecordingInfo, RecordingSignal, Sample, SampleOrigin, SampleQuality,
    },
    recording::open_recording,
};
use rusqlite::{OpenFlags, params};
use std::collections::HashMap;
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

pub fn delete_recording_marker(path: &Path, id: i64) -> Result<(), LogNookError> {
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    if connection.execute("DELETE FROM markers WHERE id = ?1", [id])? == 0 {
        return Err(LogNookError::new("Marker not found"));
    }
    Ok(())
}

pub fn query_recording_window(
    path: &Path,
    signal_ids: &[String],
    start_ns: i64,
    end_ns: i64,
    max_buckets: usize,
) -> Result<Vec<Sample>, LogNookError> {
    if signal_ids.is_empty() {
        return Ok(Vec::new());
    }
    let duration_ns = inspect_recording(path)?.duration_ns.max(0);
    let start_ns = start_ns.clamp(0, duration_ns);
    let end_ns = end_ns.clamp(0, duration_ns);
    if end_ns < start_ns {
        return Ok(Vec::new());
    }

    let bucket_count = max_buckets.clamp(1, 2_000);
    let width = (end_ns as i128 - start_ns as i128).max(1);
    let mut buckets: HashMap<String, Vec<GraphBucket>> = signal_ids
        .iter()
        .cloned()
        .map(|id| {
            (
                id,
                (0..bucket_count).map(|_| GraphBucket::default()).collect(),
            )
        })
        .collect();
    let connection = open_recording(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        "SELECT id, elapsed_ns, wall_time_unix_ns, source_id, ecu_id, signal_id, value,
         unit, quality, origin, raw_bytes FROM samples
         WHERE elapsed_ns >= ?1 AND elapsed_ns <= ?2 ORDER BY elapsed_ns, id",
    )?;
    let rows = statement.query_map(params![start_ns, end_ns], |row| {
        Ok(GraphRow {
            id: row.get(0)?,
            elapsed_ns: row.get(1)?,
            wall_time_unix_ns: row.get(2)?,
            source_id: row.get(3)?,
            ecu_id: row.get(4)?,
            signal_id: row.get(5)?,
            value: row.get(6)?,
            unit: row.get(7)?,
            quality: row.get(8)?,
            origin: row.get(9)?,
            raw_bytes: row.get(10)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let Some(signal_buckets) = buckets.get_mut(&row.signal_id) else {
            continue;
        };
        let bucket_index =
            (((row.elapsed_ns as i128 - start_ns as i128) * bucket_count as i128) / width) as usize;
        let point = GraphPoint {
            id: row.id,
            sample: row.into_sample()?,
        };
        signal_buckets[bucket_index.min(bucket_count - 1)].push(point);
    }

    let mut points = buckets
        .into_values()
        .flat_map(|signal_buckets| {
            signal_buckets
                .into_iter()
                .flat_map(GraphBucket::into_points)
        })
        .collect::<Vec<_>>();
    points.sort_by_key(|point| (point.sample.elapsed_ns, point.id));
    Ok(points.into_iter().map(|point| point.sample).collect())
}

struct GraphRow {
    id: i64,
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
    source_id: String,
    ecu_id: Option<String>,
    signal_id: String,
    value: f64,
    unit: String,
    quality: String,
    origin: String,
    raw_bytes: Option<Vec<u8>>,
}

impl GraphRow {
    fn into_sample(self) -> Result<Sample, LogNookError> {
        Ok(Sample {
            elapsed_ns: self.elapsed_ns,
            wall_time_unix_ns: self.wall_time_unix_ns,
            source_id: self.source_id,
            ecu_id: self.ecu_id,
            signal_id: self.signal_id,
            value: self.value,
            unit: self.unit,
            quality: SampleQuality::from_db(&self.quality)
                .ok_or_else(|| LogNookError::new("Invalid sample quality in recording"))?,
            origin: SampleOrigin::from_db(&self.origin)
                .ok_or_else(|| LogNookError::new("Invalid sample origin in recording"))?,
            raw_bytes: self.raw_bytes,
        })
    }
}

#[derive(Clone)]
struct GraphPoint {
    id: i64,
    sample: Sample,
}

#[derive(Default)]
struct GraphBucket {
    first: Option<GraphPoint>,
    minimum: Option<GraphPoint>,
    maximum: Option<GraphPoint>,
    last: Option<GraphPoint>,
}

impl GraphBucket {
    fn push(&mut self, point: GraphPoint) {
        if self.first.is_none() {
            self.first = Some(point.clone());
        }
        if self
            .minimum
            .as_ref()
            .is_none_or(|current| point.sample.value < current.sample.value)
        {
            self.minimum = Some(point.clone());
        }
        if self
            .maximum
            .as_ref()
            .is_none_or(|current| point.sample.value > current.sample.value)
        {
            self.maximum = Some(point.clone());
        }
        self.last = Some(point);
    }

    fn into_points(self) -> Vec<GraphPoint> {
        let mut points = [self.first, self.minimum, self.maximum, self.last]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        points.sort_by_key(|point| (point.sample.elapsed_ns, point.id));
        points.dedup_by_key(|point| point.id);
        points
    }
}
