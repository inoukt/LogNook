//! Separate, streaming raw CAN evidence; never a decoded measurement recording.
use crate::{
    CanAnalyzer, CanDirection, CanFrame, CanSignalDefinition, LogNookError, Sample, SampleOrigin,
    decode_can_signal,
};
use std::{
    fmt,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
};

const HEADER: [&str; 8] = [
    "wall_time_unix_ns",
    "elapsed_ns",
    "direction",
    "can_id",
    "format",
    "dlc",
    "data_hex",
    "flags",
];

pub struct CanCaptureWriter<W: Write = File> {
    csv: csv::Writer<W>,
}

impl CanCaptureWriter<File> {
    /// Atomically refuse existing files, retaining any earlier raw evidence.
    pub fn create_new(path: impl AsRef<Path>) -> Result<Self, LogNookError> {
        Self::from_writer(OpenOptions::new().write(true).create_new(true).open(path)?)
    }
}

impl<W: Write> CanCaptureWriter<W> {
    pub fn from_writer(writer: W) -> Result<Self, LogNookError> {
        let mut csv = csv::Writer::from_writer(writer);
        csv.write_record(HEADER)?;
        Ok(Self { csv })
    }

    /// Stream through CSV's bounded buffer; writes report errors as it drains.
    /// Call `finish` to flush the tail and observe any delayed storage failure.
    pub fn write_frame(&mut self, frame: &CanFrame) -> Result<(), LogNookError> {
        frame.validate()?;
        let id = if frame.extended {
            format!("{:08X}", frame.can_id)
        } else {
            format!("{:03X}", frame.can_id)
        };
        let payload: String = frame.data.iter().map(|b| format!("{b:02X}")).collect();
        self.csv.write_record([
            frame.wall_time_unix_ns.to_string(),
            frame.elapsed_ns.to_string(),
            match frame.direction {
                CanDirection::Rx => "rx",
                CanDirection::Tx => "tx",
            }
            .into(),
            id,
            if frame.extended {
                "extended"
            } else {
                "standard"
            }
            .into(),
            frame.data.len().to_string(),
            payload,
            format!("{:08X}", frame.flags),
        ])?;
        Ok(())
    }

    /// Flush the remaining bytes. This does not guarantee power-loss durability.
    pub fn finish(mut self) -> Result<(), LogNookError> {
        self.csv.flush()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanCaptureRowError {
    /// Physical CSV line, including the header as row 1.
    pub row: u64,
    pub message: String,
}
impl fmt::Display for CanCaptureRowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CAN capture row {}: {}", self.row, self.message)
    }
}
impl std::error::Error for CanCaptureRowError {}

pub struct CanCaptureReader<R: Read = File> {
    reader: BufReader<R>,
    row: u64,
    exhausted: bool,
}
impl CanCaptureReader<File> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LogNookError> {
        Self::from_reader(File::open(path)?)
    }
}
impl<R: Read> CanCaptureReader<R> {
    pub fn from_reader(reader: R) -> Result<Self, LogNookError> {
        let mut reader = BufReader::new(reader);
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let record = record(&header)?;
        if !record.iter().eq(HEADER) {
            return Err(LogNookError::new("Invalid raw CAN capture CSV header"));
        }
        Ok(Self {
            reader,
            row: 1,
            exhausted: false,
        })
    }
}

impl<R: Read> Iterator for CanCaptureReader<R> {
    type Item = Result<CanFrame, CanCaptureRowError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        let mut line = Vec::new();
        self.row += 1;
        let result = match self.reader.read_until(b'\n', &mut line) {
            Ok(0) => {
                self.exhausted = true;
                return None;
            }
            Ok(_) => std::str::from_utf8(&line)
                .map_err(|e| LogNookError::new(e.to_string()))
                .and_then(parse_frame),
            Err(e) => {
                self.exhausted = true;
                Err(e.into())
            }
        };
        Some(result.map_err(|e| CanCaptureRowError {
            row: self.row,
            message: e.to_string(),
        }))
    }
}

// All raw columns are numeric tokens/enums. Restrict to one physical line so
// a damaged quote cannot swallow later valid evidence as a multiline record.
fn record(line: &str) -> Result<csv::StringRecord, LogNookError> {
    if line.contains('"') {
        return Err(LogNookError::new(
            "Quoted fields are not valid raw CAN tokens",
        ));
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(line.as_bytes());
    reader
        .records()
        .next()
        .ok_or_else(|| LogNookError::new("Empty CAN capture row"))?
        .map_err(Into::into)
}

fn parse_frame(line: &str) -> Result<CanFrame, LogNookError> {
    let row = record(line)?;
    if row.len() != HEADER.len() {
        return Err(LogNookError::new("Expected eight CAN capture columns"));
    }
    let number = |value: &str| {
        value
            .parse::<i64>()
            .map_err(|e| LogNookError::new(e.to_string()))
    };
    let hex = |value: &str| {
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(LogNookError::new("Invalid hexadecimal CAN token"));
        }
        u32::from_str_radix(value, 16).map_err(|e| LogNookError::new(e.to_string()))
    };
    let direction = match &row[2] {
        "rx" => CanDirection::Rx,
        "tx" => CanDirection::Tx,
        _ => return Err(LogNookError::new("Invalid CAN direction")),
    };
    let extended = match &row[4] {
        "standard" => false,
        "extended" => true,
        _ => return Err(LogNookError::new("Invalid CAN identifier format")),
    };
    let dlc = row[5]
        .parse::<u8>()
        .map_err(|e| LogNookError::new(e.to_string()))?;
    let payload = &row[6];
    if dlc > 8
        || payload.len() != usize::from(dlc) * 2
        || !payload.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(LogNookError::new(
            "Invalid payload hex or mismatched CAN DLC",
        ));
    }
    let data = (0..payload.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&payload[i..i + 2], 16).unwrap())
        .collect();
    let frame = CanFrame {
        wall_time_unix_ns: number(&row[0])?,
        elapsed_ns: number(&row[1])?,
        direction,
        can_id: hex(&row[3])?,
        extended,
        data,
        flags: hex(&row[7])?,
    };
    frame.validate_dlc(dlc)?;
    Ok(frame)
}

/// Deliver row errors and timestamped decoded research samples as they occur.
/// The caller retains control of pacing and sample publication; no worker is started.
pub fn replay_can_capture<R: Read>(
    mut reader: CanCaptureReader<R>,
    analyzer: &mut CanAnalyzer,
    fields: &[CanSignalDefinition],
    source_id: &str,
    mut on_sample: impl FnMut(Result<Sample, CanCaptureRowError>),
) -> Result<u64, LogNookError> {
    for field in fields {
        field.validate()?;
    }
    let mut frames = 0;
    while let Some(result) = reader.next() {
        let frame = match result {
            Ok(frame) => frame,
            Err(e) => {
                on_sample(Err(e));
                continue;
            }
        };
        if let Err(e) = analyzer.feed(&frame) {
            on_sample(Err(CanCaptureRowError {
                row: reader.row,
                message: e.to_string(),
            }));
            continue;
        }
        frames += 1;
        for field in fields {
            match decode_can_signal(&frame, field, source_id) {
                Ok(Some(mut sample)) => {
                    sample.origin = SampleOrigin::Replay;
                    on_sample(Ok(sample));
                }
                Ok(None) => {}
                Err(e) => on_sample(Err(CanCaptureRowError {
                    row: reader.row,
                    message: e.to_string(),
                })),
            }
        }
    }
    Ok(frames)
}
