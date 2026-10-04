use crate::{
    model::{LogNookError, Marker, Sample, SampleOrigin, SampleQuality},
    recording::{RecordedEvent, visit_recording_events},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize, Serialize)]
struct CsvEventRow {
    #[serde(default)]
    event_type: Option<String>,
    elapsed_ns: Option<i64>,
    wall_time_unix_ns: Option<i64>,
    source_id: Option<String>,
    ecu_id: Option<String>,
    #[serde(rename = "signal_key")]
    signal_id: Option<String>,
    value: Option<f64>,
    unit: Option<String>,
    quality: Option<SampleQuality>,
    origin: Option<SampleOrigin>,
    raw_hex: Option<String>,
    label: Option<String>,
    note: Option<String>,
}

impl From<&Sample> for CsvEventRow {
    fn from(sample: &Sample) -> Self {
        Self {
            event_type: Some("sample".into()),
            elapsed_ns: Some(sample.elapsed_ns),
            wall_time_unix_ns: Some(sample.wall_time_unix_ns),
            source_id: Some(sample.source_id.clone()),
            ecu_id: sample.ecu_id.clone(),
            signal_id: Some(sample.signal_id.clone()),
            value: Some(sample.value),
            unit: Some(sample.unit.clone()),
            quality: Some(sample.quality.clone()),
            origin: Some(sample.origin.clone()),
            raw_hex: sample.raw_bytes.as_deref().map(encode_hex),
            label: None,
            note: None,
        }
    }
}

impl From<&Marker> for CsvEventRow {
    fn from(marker: &Marker) -> Self {
        Self {
            event_type: Some("marker".into()),
            elapsed_ns: Some(marker.elapsed_ns),
            wall_time_unix_ns: Some(marker.wall_time_unix_ns),
            source_id: None,
            ecu_id: None,
            signal_id: None,
            value: None,
            unit: None,
            quality: None,
            origin: None,
            raw_hex: None,
            label: Some(marker.label.clone()),
            note: Some(marker.note.clone()),
        }
    }
}

impl From<&RecordedEvent> for CsvEventRow {
    fn from(event: &RecordedEvent) -> Self {
        match event {
            RecordedEvent::Sample(sample) => Self::from(sample),
            RecordedEvent::Marker(marker) => Self::from(marker),
        }
    }
}

impl CsvEventRow {
    fn into_event(self) -> Result<RecordedEvent, LogNookError> {
        match self.event_type.as_deref().unwrap_or("sample") {
            "sample" => {
                if has_text(&self.label) || has_text(&self.note) {
                    return Err(LogNookError::new("Sample row contains marker fields"));
                }
                let value = required(self.value, "value")?;
                if !value.is_finite() {
                    return Err(LogNookError::new("Sample value must be finite"));
                }
                Ok(RecordedEvent::Sample(Sample {
                    elapsed_ns: nonnegative_elapsed(self.elapsed_ns)?,
                    wall_time_unix_ns: required(self.wall_time_unix_ns, "wall_time_unix_ns")?,
                    source_id: required(self.source_id, "source_id")?,
                    ecu_id: self.ecu_id,
                    signal_id: required(self.signal_id, "signal_key")?,
                    value,
                    unit: required(self.unit, "unit")?,
                    quality: required(self.quality, "quality")?,
                    origin: required(self.origin, "origin")?,
                    raw_bytes: self.raw_hex.map(|hex| decode_hex(&hex)).transpose()?,
                }))
            }
            "marker" => {
                if has_text(&self.source_id)
                    || has_text(&self.ecu_id)
                    || has_text(&self.signal_id)
                    || self.value.is_some()
                    || has_text(&self.unit)
                    || self.quality.is_some()
                    || self.origin.is_some()
                    || has_text(&self.raw_hex)
                {
                    return Err(LogNookError::new("Marker row contains sample fields"));
                }
                Ok(RecordedEvent::Marker(Marker {
                    id: None,
                    elapsed_ns: nonnegative_elapsed(self.elapsed_ns)?,
                    wall_time_unix_ns: required(self.wall_time_unix_ns, "wall_time_unix_ns")?,
                    label: required(self.label, "label")?,
                    note: self.note.unwrap_or_default(),
                }))
            }
            event_type => Err(LogNookError::new(format!(
                "Unknown CSV event_type '{event_type}'"
            ))),
        }
    }
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, LogNookError> {
    value.ok_or_else(|| LogNookError::new(format!("CSV row is missing {field}")))
}

fn nonnegative_elapsed(value: Option<i64>) -> Result<i64, LogNookError> {
    let elapsed_ns = required(value, "elapsed_ns")?;
    if elapsed_ns < 0 {
        return Err(LogNookError::new("elapsed_ns must be non-negative"));
    }
    Ok(elapsed_ns)
}

fn has_text(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|value| !value.is_empty())
}

pub fn export_csv<W: Write>(samples: &[Sample], output: W) -> Result<(), LogNookError> {
    let mut writer = csv::Writer::from_writer(output);
    for sample in samples {
        writer.serialize(CsvEventRow::from(sample))?;
    }
    writer.flush()?;
    Ok(())
}

pub fn import_csv<R: Read>(input: R) -> Result<Vec<Sample>, LogNookError> {
    let mut samples = Vec::new();
    visit_csv_events(input, |event| match event {
        RecordedEvent::Sample(sample) => {
            samples.push(sample);
            Ok(())
        }
        RecordedEvent::Marker(_) => Err(LogNookError::new(
            "CSV contains markers; import it as a recording to preserve them",
        )),
    })?;
    Ok(samples)
}

pub fn export_recording_csv(recording_path: &Path, csv_path: &Path) -> Result<(), LogNookError> {
    let mut output_created = false;
    let result = (|| {
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(csv_path)?;
        output_created = true;
        let mut writer = csv::Writer::from_writer(output);
        visit_recording_events(recording_path, |event| {
            writer.serialize(CsvEventRow::from(&event))?;
            Ok(true)
        })?;
        writer.flush().map_err(LogNookError::from)
    })();
    if result.is_err() && output_created {
        let _ = std::fs::remove_file(csv_path);
    }
    result
}

pub(crate) fn visit_csv_events<R: Read>(
    input: R,
    mut visit: impl FnMut(RecordedEvent) -> Result<(), LogNookError>,
) -> Result<(), LogNookError> {
    let mut reader = reader_with_detected_delimiter(input)?;
    let headers = reader.headers()?.clone();
    if let Some(time_index) = forscan_time_index(&headers) {
        return visit_forscan_events(reader, &headers, time_index, visit);
    }
    for (index, row) in reader.deserialize::<CsvEventRow>().enumerate() {
        let row_number = index + 2;
        let event = row
            .map_err(|error| LogNookError::new(format!("CSV row {row_number}: {error}")))?
            .into_event()
            .map_err(|error| LogNookError::new(format!("CSV row {row_number}: {error}")))?;
        visit(event)
            .map_err(|error| LogNookError::new(format!("CSV row {row_number}: {error}")))?;
    }
    Ok(())
}

fn reader_with_detected_delimiter<R: Read>(
    input: R,
) -> Result<csv::Reader<BufReader<R>>, LogNookError> {
    let mut input = BufReader::new(input);
    let delimiter = {
        let prefix = input.fill_buf()?;
        let line_end = prefix
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(prefix.len());
        let (commas, semicolons) = delimiter_counts(&prefix[..line_end]);
        if semicolons > commas { b';' } else { b',' }
    };
    Ok(csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(input))
}

fn delimiter_counts(line: &[u8]) -> (usize, usize) {
    let mut commas = 0;
    let mut semicolons = 0;
    let mut quoted = false;
    let mut index = 0;
    while index < line.len() {
        match line[index] {
            b'"' if quoted && line.get(index + 1) == Some(&b'"') => index += 1,
            b'"' => quoted = !quoted,
            b',' if !quoted => commas += 1,
            b';' if !quoted => semicolons += 1,
            _ => {}
        }
        index += 1;
    }
    (commas, semicolons)
}

fn forscan_time_index(headers: &csv::StringRecord) -> Option<usize> {
    headers.iter().position(|header| {
        header
            .trim_start_matches('\u{feff}')
            .trim()
            .eq_ignore_ascii_case("time(ms)")
    })
}

struct ForscanSignalColumn {
    index: usize,
    id: String,
    unit: String,
}

fn forscan_signal_columns(
    headers: &csv::StringRecord,
    time_index: usize,
) -> Vec<ForscanSignalColumn> {
    let mut occurrences = HashMap::new();
    headers
        .iter()
        .enumerate()
        .filter_map(|(index, header)| {
            if index == time_index {
                return None;
            }
            let header = header.trim_start_matches('\u{feff}').trim();
            let (name, unit) = header
                .strip_suffix(')')
                .and_then(|without_close| without_close.rsplit_once('('))
                .map(|(name, unit)| (name.trim(), unit.trim()))
                .unwrap_or((header, ""));
            if name.is_empty() {
                return None;
            }
            let occurrence = occurrences.entry(name.to_ascii_lowercase()).or_insert(0);
            *occurrence += 1;
            let id = if *occurrence == 1 {
                name.to_owned()
            } else {
                format!("{name} #{}", occurrence)
            };
            Some(ForscanSignalColumn {
                index,
                id,
                unit: unit.to_owned(),
            })
        })
        .collect()
}

fn visit_forscan_events<R: Read>(
    mut reader: csv::Reader<R>,
    headers: &csv::StringRecord,
    time_index: usize,
    mut visit: impl FnMut(RecordedEvent) -> Result<(), LogNookError>,
) -> Result<(), LogNookError> {
    let columns = forscan_signal_columns(headers, time_index);
    if columns.is_empty() {
        return Err(LogNookError::new("FORScan CSV has no signal columns"));
    }
    // FORScan exports elapsed time only; anchor the imported samples at import time.
    let wall_time_base_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64;
    for (index, row) in reader.records().enumerate() {
        let row_number = index + 2;
        let row = row
            .map_err(|error| LogNookError::new(format!("FORScan CSV row {row_number}: {error}")))?;
        if row.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let time_ms = row
            .get(time_index)
            .and_then(parse_forscan_number)
            .filter(|value| *value >= 0.0 && *value <= i64::MAX as f64 / 1_000_000.0)
            .ok_or_else(|| {
                LogNookError::new(format!(
                    "FORScan CSV row {row_number}: invalid time(ms) value"
                ))
            })?;
        let elapsed_ns = (time_ms * 1_000_000.0).round() as i64;
        for column in &columns {
            let Some(value) = row.get(column.index).and_then(parse_forscan_value) else {
                continue;
            };
            visit(RecordedEvent::Sample(Sample {
                elapsed_ns,
                wall_time_unix_ns: wall_time_base_ns.saturating_add(elapsed_ns),
                source_id: "forscan".into(),
                ecu_id: None,
                signal_id: column.id.clone(),
                value,
                unit: column.unit.clone(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            }))
            .map_err(|error| LogNookError::new(format!("FORScan CSV row {row_number}: {error}")))?;
        }
    }
    Ok(())
}

fn parse_forscan_value(value: &str) -> Option<f64> {
    match value.trim() {
        value if value.eq_ignore_ascii_case("yes") => Some(1.0),
        value if value.eq_ignore_ascii_case("no") => Some(0.0),
        value => parse_forscan_number(value),
    }
}

fn parse_forscan_number(value: &str) -> Option<f64> {
    let value = value.trim();
    let parsed = match value.parse::<f64>() {
        Ok(parsed) => parsed,
        Err(_) if value.contains(',') && !value.contains('.') => {
            value.replace(',', ".").parse::<f64>().ok()?
        }
        Err(_) => return None,
    };
    parsed.is_finite().then_some(parsed)
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0F) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, LogNookError> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LogNookError::new(
            "CSV raw_hex must contain pairs of hexadecimal digits",
        ));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| LogNookError::new("Invalid raw_hex byte in CSV"))
        })
        .collect()
}
