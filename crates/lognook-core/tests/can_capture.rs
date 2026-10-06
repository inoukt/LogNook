use lognook_core::{
    CanAnalysisConfig, CanAnalyzer, CanByteOrder, CanCaptureReader, CanCaptureWriter, CanDirection,
    CanFrame, CanSignalDefinition, SampleOrigin, replay_can_capture,
};
use std::{
    fs,
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};

const HEADER: &str = "wall_time_unix_ns,elapsed_ns,direction,can_id,format,dlc,data_hex,flags\n";

fn frame(elapsed_ns: i64, extended: bool) -> CanFrame {
    CanFrame {
        elapsed_ns,
        wall_time_unix_ns: 1_700_000_000_123_456_789 + elapsed_ns,
        can_id: if extended { 0x1234567 } else { 0x123 },
        extended,
        direction: if extended {
            CanDirection::Tx
        } else {
            CanDirection::Rx
        },
        data: vec![0, 0xff, 0x34],
        flags: 0x80000001,
    }
}

#[test]
fn csv_round_trip_preserves_raw_frames_and_order() {
    let expected = vec![
        frame(0, false),
        frame(100, true),
        CanFrame {
            data: vec![],
            ..frame(101, false)
        },
    ];
    let mut bytes = Vec::new();
    let mut writer = CanCaptureWriter::from_writer(&mut bytes).unwrap();
    for frame in &expected {
        writer.write_frame(frame).unwrap();
    }
    writer.finish().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains(",rx,123,standard,3,00FF34,80000001"));
    assert!(text.contains(",tx,01234567,extended,3,00FF34,80000001"));
    let actual: Vec<_> = CanCaptureReader::from_reader(bytes.as_slice())
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn create_new_preserves_existing_capture() {
    let path = std::env::temp_dir().join(format!(
        "lognook-{}-{}.candump.csv",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut writer = CanCaptureWriter::create_new(&path).unwrap();
    writer.write_frame(&frame(0, false)).unwrap();
    writer.finish().unwrap();
    let original = fs::read(&path).unwrap();
    assert!(CanCaptureWriter::create_new(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    let loaded = CanCaptureReader::open(&path)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(loaded, frame(0, false));
    fs::remove_file(path).unwrap();
}

#[test]
fn malformed_rows_report_line_and_allow_later_valid_rows() {
    let csv = format!(
        "{HEADER}0,0,rx,123,standard,2,00,0\n0,1,rx,800,standard,0,,0\n0,2,bad,123,standard,0,,0\nwrong,column,count\n0,3,rx,123,standard,1,FF,0\n"
    );
    let rows: Vec<_> = CanCaptureReader::from_reader(csv.as_bytes())
        .unwrap()
        .collect();
    assert_eq!(rows.len(), 5);
    for (i, row) in rows[..4].iter().enumerate() {
        assert_eq!(row.as_ref().unwrap_err().row, i as u64 + 2);
    }
    assert_eq!(rows[4].as_ref().unwrap().data, vec![255]);
}

#[test]
fn broken_quote_and_non_utf8_row_cannot_swallow_next_frame() {
    let mut csv = HEADER.as_bytes().to_vec();
    csv.extend_from_slice(b"0,0,rx,123,standard,1,\"00,0\n");
    csv.extend_from_slice(b"\xff\n");
    csv.extend_from_slice(b"0,2,rx,123,standard,1,AA,0\n");
    let rows: Vec<_> = CanCaptureReader::from_reader(csv.as_slice())
        .unwrap()
        .collect();
    assert_eq!(rows[0].as_ref().unwrap_err().row, 2);
    assert_eq!(rows[1].as_ref().unwrap_err().row, 3);
    assert_eq!(rows[2].as_ref().unwrap().data, vec![0xAA]);
}

#[test]
fn oversized_rows_are_discarded_and_later_valid_frames_survive() {
    let huge_timestamp = "0".repeat(2 * 1024 * 1024);
    let csv = format!(
        "{HEADER}{huge_timestamp},0,rx,123,standard,1,00,0\n0,1,rx,123,standard,1,AA,0\n{huge_timestamp},2,rx,123,standard,1,00,0"
    );
    let rows: Vec<_> = CanCaptureReader::from_reader(csv.as_bytes())
        .unwrap()
        .collect();
    assert_eq!(rows.len(), 3);
    for index in [0, 2] {
        let error = rows[index].as_ref().unwrap_err();
        assert_eq!(error.row, index as u64 + 2);
        assert!(error.message.contains("exceeds"));
    }
    assert_eq!(rows[1].as_ref().unwrap().data, vec![0xAA]);
}

#[test]
fn oversized_header_is_rejected_by_the_line_bound() {
    let header = "0".repeat(2 * 1024 * 1024);
    let error = CanCaptureReader::from_reader(header.as_bytes())
        .err()
        .unwrap();
    assert!(error.to_string().contains("exceeds"));
}

#[test]
fn embedded_carriage_return_in_header_cannot_hide_extra_content() {
    let header = format!("{}\rhidden,content\n", HEADER.trim_end_matches('\n'));
    assert!(CanCaptureReader::from_reader(header.as_bytes()).is_err());
}

#[test]
fn embedded_carriage_return_in_frame_is_an_error_and_recovery_continues() {
    let csv = format!(
        "{HEADER}0,0,rx,123,standard,1,00,0\r0,1,rx,123,standard,1,BB,0\n0,2,rx,123,standard,1,AA,0\n"
    );
    let rows: Vec<_> = CanCaptureReader::from_reader(csv.as_bytes())
        .unwrap()
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].as_ref().unwrap_err().row, 2);
    assert_eq!(rows[1].as_ref().unwrap().data, vec![0xAA]);
}

#[test]
fn terminal_crlf_remains_valid_for_headers_and_frames() {
    let csv = format!(
        "{}\r\n0,0,rx,123,standard,1,AA,0\r\n",
        HEADER.trim_end_matches('\n')
    );
    assert_eq!(
        CanCaptureReader::from_reader(csv.as_bytes())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .data,
        vec![0xAA]
    );
}

#[test]
fn replay_uses_analyzer_and_decoder_and_surfaces_row_errors() {
    let csv = format!(
        "{HEADER}100,0,rx,123,standard,1,02,0\n100,1,rx,123,standard,2,03,0\n200,10,rx,123,standard,1,04,0\n300,20,tx,123,standard,1,08,0\n"
    );
    let mut analyzer = CanAnalyzer::new(CanAnalysisConfig::default()).unwrap();
    let field = CanSignalDefinition {
        id: "field".into(),
        name: "field".into(),
        can_id: 0x123,
        extended: false,
        start_bit: 0,
        bit_length: 8,
        byte_order: CanByteOrder::Intel,
        signed: false,
        factor: 2.0,
        offset: 1.0,
        unit: "u".into(),
    };
    let mut samples = Vec::new();
    let mut errors = Vec::new();
    let count = replay_can_capture(
        CanCaptureReader::from_reader(csv.as_bytes()).unwrap(),
        &mut analyzer,
        &[field],
        "capture",
        |result| match result {
            Ok(s) => samples.push(s),
            Err(e) => errors.push(e),
        },
    )
    .unwrap();
    assert_eq!(count, 3);
    let census = analyzer.census();
    assert_eq!(
        (census[0].count, census[0].rx_count, census[0].tx_count),
        (3, 2, 1)
    );
    assert_eq!(
        samples.iter().map(|s| s.value).collect::<Vec<_>>(),
        vec![5.0, 9.0]
    );
    assert_eq!(
        (samples[1].elapsed_ns, samples[1].wall_time_unix_ns),
        (10, 200)
    );
    assert!(samples.iter().all(|s| s.origin == SampleOrigin::Replay));
    assert_eq!(errors[0].row, 3);
}

struct BrokenSink;
impl Write for BrokenSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn capture_surfaces_storage_write_failure() {
    let mut writer = CanCaptureWriter::from_writer(BrokenSink).unwrap();
    // Buffered writes fail once CSV drains its bounded buffer.
    let failed = (0..1000).any(|t| writer.write_frame(&frame(t, false)).is_err());
    assert!(failed);
}

#[test]
fn capture_surfaces_final_flush_failure() {
    let mut writer = CanCaptureWriter::from_writer(BrokenSink).unwrap();
    writer.write_frame(&frame(0, false)).unwrap();
    assert!(writer.finish().is_err());
}

#[test]
fn rejects_invalid_header_and_frame_before_writing() {
    assert!(CanCaptureReader::from_reader("value,unit\n1,V\n".as_bytes()).is_err());
    let mut bytes = Vec::new();
    let mut writer = CanCaptureWriter::from_writer(&mut bytes).unwrap();
    assert!(
        writer
            .write_frame(&CanFrame {
                data: vec![0; 9],
                ..frame(0, false)
            })
            .is_err()
    );
    writer.finish().unwrap();
    assert_eq!(
        CanCaptureReader::from_reader(bytes.as_slice())
            .unwrap()
            .count(),
        0
    );
}
