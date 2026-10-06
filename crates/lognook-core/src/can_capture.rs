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

// Generated classic CAN rows and the header fit comfortably within this cap,
// including CRLF. Never grow a line buffer to match malformed input size.
const MAX_LINE_BYTES: usize = 256;

enum PhysicalLine {
    End,
    Bytes(Vec<u8>),
    Oversized,
}

fn read_physical_line(reader: &mut impl BufRead) -> std::io::Result<PhysicalLine> {
    let mut line = Vec::with_capacity(MAX_LINE_BYTES);
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if oversized {
                PhysicalLine::Oversized
            } else if line.is_empty() {
                PhysicalLine::End
            } else {
                PhysicalLine::Bytes(line)
            });
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            if count <= MAX_LINE_BYTES - line.len() {
                line.extend_from_slice(&available[..count]);
            } else {
                oversized = true;
            }
        }
        // Once oversized, consume chunks directly without retaining the tail.
        reader.consume(count);
        if newline.is_some() {
            return Ok(if oversized {
                PhysicalLine::Oversized
            } else {
                PhysicalLine::Bytes(line)
            });
        }
    }
}

fn oversized_line_error() -> LogNookError {
    LogNookError::new("CAN capture physical line exceeds 256-byte limit")
}

pub struct CanCaptureWriter<W: Write = File> {
    csv: csv::Writer<W>,
}

impl CanCaptureWriter<File> {
    /// Atomically refuse existing files, retaining any earlier raw evidence.
    pub fn create_new(path: impl AsRef<Path>) -> Result<Self, LogNookError> {
        let path = path.as_ref();
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        initialize_created_capture(path, file)
    }
}

fn initialize_created_capture<W: Write>(
    path: &Path,
    writer: W,
) -> Result<CanCaptureWriter<W>, LogNookError> {
    let mut capture = match CanCaptureWriter::from_writer(writer) {
        Ok(capture) => capture,
        Err(error) => {
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
    };
    if let Err(error) = capture.csv.flush() {
        drop(capture);
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    Ok(capture)
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

/// Raw token CSV with physical lines capped at 256 bytes, including CRLF.
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
        let header = match read_physical_line(&mut reader)? {
            PhysicalLine::Bytes(header) => header,
            PhysicalLine::Oversized => return Err(oversized_line_error()),
            PhysicalLine::End => return Err(LogNookError::new("Empty CAN capture header")),
        };
        let header = std::str::from_utf8(&header).map_err(|e| LogNookError::new(e.to_string()))?;
        let record = record(header)?;
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
        self.row += 1;
        let result = match read_physical_line(&mut self.reader) {
            Ok(PhysicalLine::End) => {
                self.exhausted = true;
                return None;
            }
            Ok(PhysicalLine::Bytes(line)) => std::str::from_utf8(&line)
                .map_err(|e| LogNookError::new(e.to_string()))
                .and_then(parse_frame),
            Ok(PhysicalLine::Oversized) => Err(oversized_line_error()),
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
    let line = if let Some(line) = line.strip_suffix('\n') {
        line.strip_suffix('\r').unwrap_or(line)
    } else {
        line
    };
    if line.contains('"') || line.contains('\r') || line.contains('\n') {
        return Err(LogNookError::new(
            "Quotes and embedded line terminators are not valid raw CAN tokens",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        io,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct FailHeaderWrite(File);

    impl Write for FailHeaderWrite {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated capture header failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    fn temp_path() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lognook-header-{stamp}.csv"))
    }

    #[test]
    fn header_initialization_failure_removes_only_the_new_capture_file() {
        let path = temp_path();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        assert!(initialize_created_capture(&path, FailHeaderWrite(file)).is_err());
        assert!(!path.exists());

        std::fs::write(&path, b"existing capture").unwrap();
        assert!(CanCaptureWriter::create_new(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing capture");
        let _ = std::fs::remove_file(path);
    }
}
