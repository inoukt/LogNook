use crate::{
    model::{LogNookError, Marker, Sample, SampleOrigin, SampleQuality},
    recording::{RecordedEvent, visit_recording_events},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
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
                let value = required(self.value, "value")?;
                if !value.is_finite() {
                    return Err(LogNookError::new("Sample value must be finite"));
                }
                Ok(RecordedEvent::Sample(Sample {
                    elapsed_ns: required(self.elapsed_ns, "elapsed_ns")?,
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
            "marker" => Ok(RecordedEvent::Marker(Marker {
                elapsed_ns: required(self.elapsed_ns, "elapsed_ns")?,
                wall_time_unix_ns: required(self.wall_time_unix_ns, "wall_time_unix_ns")?,
                label: required(self.label, "label")?,
                note: required(self.note, "note")?,
            })),
            event_type => Err(LogNookError::new(format!(
                "Unknown CSV event_type '{event_type}'"
            ))),
        }
    }
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, LogNookError> {
    value.ok_or_else(|| LogNookError::new(format!("CSV row is missing {field}")))
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
    let mut reader = csv::Reader::from_reader(input);
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
