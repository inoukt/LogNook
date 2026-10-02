use crate::{
    model::{LogNookError, Marker, Sample, SampleOrigin, SampleQuality},
    recording::{RecordedEvent, visit_recording_events},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
struct CsvSample {
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
    source_id: String,
    ecu_id: Option<String>,
    #[serde(rename = "signal_key")]
    signal_id: String,
    value: f64,
    unit: String,
    quality: SampleQuality,
    origin: SampleOrigin,
    #[serde(default)]
    raw_hex: Option<String>,
}

impl From<&Sample> for CsvSample {
    fn from(sample: &Sample) -> Self {
        Self {
            elapsed_ns: sample.elapsed_ns,
            wall_time_unix_ns: sample.wall_time_unix_ns,
            source_id: sample.source_id.clone(),
            ecu_id: sample.ecu_id.clone(),
            signal_id: sample.signal_id.clone(),
            value: sample.value,
            unit: sample.unit.clone(),
            quality: sample.quality.clone(),
            origin: sample.origin.clone(),
            raw_hex: sample.raw_bytes.as_deref().map(encode_hex),
        }
    }
}

impl CsvSample {
    fn into_sample(self) -> Result<Sample, LogNookError> {
        Ok(Sample {
            elapsed_ns: self.elapsed_ns,
            wall_time_unix_ns: self.wall_time_unix_ns,
            source_id: self.source_id,
            ecu_id: self.ecu_id,
            signal_id: self.signal_id,
            value: self.value,
            unit: self.unit,
            quality: self.quality,
            origin: self.origin,
            raw_bytes: self.raw_hex.map(|hex| decode_hex(&hex)).transpose()?,
        })
    }
}

#[derive(Serialize)]
struct CsvMarker<'a> {
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
    label: &'a str,
    note: &'a str,
}

impl<'a> From<&'a Marker> for CsvMarker<'a> {
    fn from(marker: &'a Marker) -> Self {
        Self {
            elapsed_ns: marker.elapsed_ns,
            wall_time_unix_ns: marker.wall_time_unix_ns,
            label: &marker.label,
            note: &marker.note,
        }
    }
}

pub fn export_csv<W: Write>(samples: &[Sample], output: W) -> Result<(), LogNookError> {
    let mut writer = csv::Writer::from_writer(output);
    for sample in samples {
        writer.serialize(CsvSample::from(sample))?;
    }
    writer.flush()?;
    Ok(())
}

pub fn import_csv<R: Read>(input: R) -> Result<Vec<Sample>, LogNookError> {
    let mut reader = csv::Reader::from_reader(input);
    reader
        .deserialize::<CsvSample>()
        .map(|row| {
            row.map_err(LogNookError::from)
                .and_then(CsvSample::into_sample)
        })
        .collect()
}

pub fn export_recording_csv(recording_path: &Path, csv_path: &Path) -> Result<(), LogNookError> {
    let mut main_created = false;
    let mut marker_created = false;
    let marker_path = marker_csv_path(csv_path);
    let result = (|| {
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(csv_path)?;
        main_created = true;
        let mut samples = csv::Writer::from_writer(output);
        let mut markers: Option<csv::Writer<File>> = None;
        visit_recording_events(recording_path, |event| {
            match event {
                RecordedEvent::Sample(sample) => samples.serialize(CsvSample::from(&sample))?,
                RecordedEvent::Marker(marker) => {
                    if markers.is_none() {
                        let output = OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&marker_path)?;
                        marker_created = true;
                        markers = Some(csv::Writer::from_writer(output));
                    }
                    markers
                        .as_mut()
                        .unwrap()
                        .serialize(CsvMarker::from(&marker))?;
                }
            }
            Ok(true)
        })?;
        samples.flush()?;
        if let Some(markers) = &mut markers {
            markers.flush()?;
        }
        Ok(())
    })();
    if result.is_err() {
        if main_created {
            let _ = std::fs::remove_file(csv_path);
        }
        if marker_created {
            let _ = std::fs::remove_file(&marker_path);
        }
    }
    result
}

fn marker_csv_path(csv_path: &Path) -> PathBuf {
    let stem = csv_path.file_stem().unwrap_or_default().to_string_lossy();
    csv_path.with_file_name(format!("{stem}-markers.csv"))
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
