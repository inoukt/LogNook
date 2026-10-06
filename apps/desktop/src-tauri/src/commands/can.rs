use super::engine;
use crate::AppState;
use lognook_core::{
    CanAnalysisConfig, CanAnalyzer, CanCaptureReader, CanDeviceCandidate, CanDeviceSelection,
    CanEvidenceRecord, CanFrame, CanScanConfig, CanSignalDefinition, CanWorkbenchConfig,
    CanWorkbenchStatus, OemCandidateTrySpec, OemCatalog,
    available_can_adapters as list_can_adapters, can_evidence_path, decode_can_signal,
    oem_candidate_scan_config,
};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn choose_can_capture_save_path(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("CAN captures", &["csv"])
        .set_file_name("capture.candump.csv")
        .blocking_save_file();
    selected
        .map(|file| {
            let path = file.into_path().map_err(|error| error.to_string())?;
            let path = if path
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".candump.csv")
            {
                path
            } else {
                path.with_extension("candump.csv")
            };
            Ok(path.to_string_lossy().into_owned())
        })
        .transpose()
}

#[tauri::command]
pub async fn choose_can_capture_open_path(app: AppHandle) -> Result<Option<String>, String> {
    app.dialog()
        .file()
        .add_filter("CAN captures", &["csv"])
        .blocking_pick_file()
        .map(|file| file.into_path().map_err(|error| error.to_string()))
        .transpose()
        .map(|path| path.map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn available_can_adapters() -> Result<Vec<CanDeviceCandidate>, String> {
    list_can_adapters().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_candlelight(
    state: State<'_, AppState>,
    selection: CanDeviceSelection,
    config: CanWorkbenchConfig,
) -> Result<CanWorkbenchStatus, String> {
    engine(&state)?
        .connect_candlelight(&selection, config)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn configure_can_signals(
    state: State<'_, AppState>,
    fields: Vec<CanSignalDefinition>,
) -> Result<(), String> {
    engine(&state)?
        .configure_can_signals(fields)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_can_capture(state: State<'_, AppState>, path: String) -> Result<(), String> {
    engine(&state)?
        .start_can_capture(Path::new(&path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_can_capture(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .stop_can_capture()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn send_can_frame(
    state: State<'_, AppState>,
    frame: CanFrame,
    pacing_ms: u64,
    repeat: u32,
) -> Result<(), String> {
    engine(&state)?
        .send_can_frame(frame, pacing_ms, repeat)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_can_scan(
    state: State<'_, AppState>,
    config: CanScanConfig,
) -> Result<(), String> {
    engine(&state)?
        .start_can_scan(config)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_can_scan(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .stop_can_scan()
        .map_err(|error| error.to_string())
}

/// The catalog performs source-revision, license, identifier-width, and exact-route checks
/// before this explicit one-candidate read can transmit.
#[tauri::command]
pub async fn start_can_oem_candidate(
    state: State<'_, AppState>,
    candidate_id: String,
) -> Result<OemCandidateTrySpec, String> {
    let spec = OemCatalog::new(&state.pid_catalog_path)
        .candidate_for_try(&candidate_id)
        .map_err(|error| error.to_string())?;
    let config = oem_candidate_scan_config(&spec).map_err(|error| error.to_string())?;
    engine(&state)?
        .start_can_scan(config)
        .map_err(|error| error.to_string())?;
    Ok(spec)
}

#[derive(Clone, Debug, Serialize)]
pub struct CanCaptureReplayReport {
    pub frames: u64,
    pub decoded_samples: u64,
    pub malformed_rows: Vec<String>,
    pub analyzer_issues: Vec<String>,
    pub census: Vec<lognook_core::CanCensusRow>,
    pub evidence_journal: Option<String>,
    pub session_finalized: Option<bool>,
    pub evidence_records: u64,
    pub scan_results: u64,
    pub tx_submitted: u64,
    pub tx_echo_confirmed: u64,
    pub transport_errors: u64,
    pub evidence_errors: Vec<String>,
}

#[tauri::command]
pub async fn replay_can_capture_report(
    path: String,
    fields: Vec<CanSignalDefinition>,
) -> Result<CanCaptureReplayReport, String> {
    for field in &fields {
        field.validate().map_err(|error| error.to_string())?;
    }
    replay_can_capture_report_path(Path::new(&path), &fields)
}

fn replay_can_capture_report_path(
    path: &Path,
    fields: &[CanSignalDefinition],
) -> Result<CanCaptureReplayReport, String> {
    let mut reader = CanCaptureReader::open(path).map_err(|error| error.to_string())?;
    let mut analyzer =
        CanAnalyzer::new(CanAnalysisConfig::default()).map_err(|error| error.to_string())?;
    let mut report = CanCaptureReplayReport {
        frames: 0,
        decoded_samples: 0,
        malformed_rows: Vec::new(),
        analyzer_issues: Vec::new(),
        census: Vec::new(),
        evidence_journal: None,
        session_finalized: None,
        evidence_records: 0,
        scan_results: 0,
        tx_submitted: 0,
        tx_echo_confirmed: 0,
        transport_errors: 0,
        evidence_errors: Vec::new(),
    };
    while let Some(row) = reader.next() {
        let frame = match row {
            Ok(frame) => frame,
            Err(error) => {
                if report.malformed_rows.len() < 100 {
                    report.malformed_rows.push(error.to_string());
                }
                continue;
            }
        };
        report.frames += 1;
        if let Err(error) = analyzer.feed(&frame) {
            if report.analyzer_issues.len() < 100 {
                report.analyzer_issues.push(error.to_string());
            }
        }
        if frame.direction == lognook_core::CanDirection::Rx {
            for field in fields {
                match decode_can_signal(&frame, field, "can:capture") {
                    Ok(Some(_)) => report.decoded_samples += 1,
                    Ok(None) => {}
                    Err(error) if report.malformed_rows.len() < 100 => {
                        report.malformed_rows.push(error.to_string());
                    }
                    Err(_) => {}
                }
            }
        }
    }
    report.census = analyzer.census();
    summarize_evidence_journal(path, &mut report);
    Ok(report)
}

fn summarize_evidence_journal(path: &Path, report: &mut CanCaptureReplayReport) {
    let journal_path = can_evidence_path(path);
    let Ok(file) = File::open(&journal_path) else {
        return;
    };
    report.evidence_journal = Some(journal_path.to_string_lossy().into_owned());
    let mut reader = BufReader::new(file);
    let mut line_number = 0u64;
    let mut active_capture_id = None;
    loop {
        let line = match read_evidence_line(&mut reader) {
            Ok(EvidenceLine::End) => break,
            Ok(EvidenceLine::Oversized) => {
                line_number += 1;
                push_bounded(
                    &mut report.evidence_errors,
                    format!("Journal line {line_number}: exceeds 256 KiB limit"),
                );
                continue;
            }
            Ok(EvidenceLine::Bytes(line)) => {
                line_number += 1;
                line
            }
            Err(error) => {
                push_bounded(
                    &mut report.evidence_errors,
                    format!("Journal read failed after line {line_number}: {error}"),
                );
                break;
            }
        };
        match serde_json::from_slice::<CanEvidenceRecord>(&line) {
            Ok(record) => {
                report.evidence_records += 1;
                match record {
                    CanEvidenceRecord::SessionStarted { capture_id, .. } => {
                        active_capture_id = Some(capture_id);
                        report.session_finalized = Some(false)
                    }
                    CanEvidenceRecord::SessionFinished { capture_id }
                        if active_capture_id == Some(capture_id) =>
                    {
                        report.session_finalized = Some(true)
                    }
                    CanEvidenceRecord::SessionFinished { .. } => {
                        push_bounded(
                            &mut report.evidence_errors,
                            format!("Journal line {line_number}: unmatched session finish"),
                        );
                    }
                    CanEvidenceRecord::ScanResult { .. } => report.scan_results += 1,
                    CanEvidenceRecord::TxSubmitted { .. } => report.tx_submitted += 1,
                    CanEvidenceRecord::TxEchoConfirmed { .. } => report.tx_echo_confirmed += 1,
                    CanEvidenceRecord::TransportError { .. } => report.transport_errors += 1,
                    _ => {}
                }
            }
            Err(error) => push_bounded(
                &mut report.evidence_errors,
                format!("Journal line {line_number}: {error}"),
            ),
        }
    }
}

const MAX_EVIDENCE_LINE_BYTES: usize = 256 * 1024;

enum EvidenceLine {
    End,
    Bytes(Vec<u8>),
    Oversized,
}

fn read_evidence_line(reader: &mut impl BufRead) -> io::Result<EvidenceLine> {
    let mut line = Vec::with_capacity(MAX_EVIDENCE_LINE_BYTES.min(4096));
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if oversized {
                EvidenceLine::Oversized
            } else if line.is_empty() {
                EvidenceLine::End
            } else {
                EvidenceLine::Bytes(line)
            });
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            if count <= MAX_EVIDENCE_LINE_BYTES - line.len() {
                line.extend_from_slice(&available[..count]);
            } else {
                oversized = true;
            }
        }
        reader.consume(count);
        if newline.is_some() {
            return Ok(if oversized {
                EvidenceLine::Oversized
            } else {
                EvidenceLine::Bytes(line)
            });
        }
    }
}

fn push_bounded(messages: &mut Vec<String>, message: String) {
    if messages.len() < 100 {
        messages.push(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lognook_core::{
        CanByteOrder, CanCaptureWriter, CanDiagnosticOutcome, CanDirection, CanEvidenceScanConfig,
        CanEvidenceScanResult, CanFrame, CanScanMode, CanSignalDefinition,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn replay_counts_and_decodes_valid_rows_after_census_capacity() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-census-{stamp}.candump.csv"));
        let mut writer = CanCaptureWriter::create_new(&path).unwrap();
        for id in 0..2050u32 {
            writer
                .write_frame(&CanFrame {
                    elapsed_ns: i64::from(id),
                    wall_time_unix_ns: 1_700_000_000_000_000_000 + i64::from(id),
                    can_id: id,
                    extended: true,
                    direction: CanDirection::Rx,
                    data: vec![if id == 2049 { 42 } else { id as u8 }],
                    flags: 0,
                })
                .unwrap();
        }
        writer
            .write_frame(&CanFrame {
                elapsed_ns: 2050,
                wall_time_unix_ns: 1_700_000_000_000_002_050,
                can_id: 2049,
                extended: true,
                direction: CanDirection::Tx,
                data: vec![42],
                flags: 0,
            })
            .unwrap();
        writer.finish().unwrap();
        let field = CanSignalDefinition {
            id: "late_value".into(),
            name: "Late value".into(),
            can_id: 2049,
            extended: true,
            start_bit: 0,
            bit_length: 8,
            byte_order: CanByteOrder::Intel,
            signed: false,
            factor: 1.0,
            offset: 0.0,
            unit: "raw".into(),
        };
        let report = replay_can_capture_report_path(&path, &[field]).unwrap();
        assert_eq!(report.frames, 2051);
        assert_eq!(report.decoded_samples, 1);
        assert_eq!(report.census.len(), 2048);
        assert!(
            report
                .analyzer_issues
                .iter()
                .any(|issue| issue.contains("ID limit"))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_journal_lines_are_skipped_and_later_records_are_read() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-journal-cap-{stamp}.candump.csv"));
        CanCaptureWriter::create_new(&path)
            .unwrap()
            .finish()
            .unwrap();
        let journal_path = can_evidence_path(&path);
        let mut journal = File::create(&journal_path).unwrap();
        use std::io::Write;
        journal
            .write_all(&vec![b'x'; MAX_EVIDENCE_LINE_BYTES + 1])
            .unwrap();
        journal.write_all(b"\n").unwrap();
        writeln!(
            journal,
            "{}",
            serde_json::to_string(&CanEvidenceRecord::SessionStarted {
                format_version: 1,
                capture_id: 1,
                bitrate: 500_000,
                listen_only: true,
                source_id: "can:test".into(),
            })
            .unwrap()
        )
        .unwrap();
        drop(journal);

        let report = replay_can_capture_report_path(&path, &[]).unwrap();
        assert_eq!(report.evidence_records, 1);
        assert!(report.evidence_errors[0].contains("exceeds 256 KiB limit"));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(journal_path);
    }

    #[test]
    fn replay_report_summarizes_the_paired_readable_journal() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-journal-{stamp}.candump.csv"));
        CanCaptureWriter::create_new(&path)
            .unwrap()
            .finish()
            .unwrap();
        let journal_path = can_evidence_path(&path);
        let frame = CanFrame {
            elapsed_ns: 10,
            wall_time_unix_ns: 20,
            can_id: 0x7E0,
            extended: false,
            direction: CanDirection::Tx,
            data: vec![2, 0x21, 1],
            flags: 0,
        };
        let config = CanEvidenceScanConfig {
            mode: CanScanMode::Mode21 {
                request_id: 0x7E0,
                response_id: 0x7E8,
                extended: false,
                start_pid: 1,
                end_pid: 1,
            },
            pacing_ms: 100,
            response_timeout_ms: 1000,
            source_candidate_id: Some("scion:21:test".into()),
        };
        let records = [
            CanEvidenceRecord::ScanResult {
                result: CanEvidenceScanResult {
                    config,
                    identifier: "21 01".into(),
                    request: frame.clone(),
                    response_frames: Vec::new(),
                    outcome: CanDiagnosticOutcome::NoResponse,
                    supported_pids: None,
                },
            },
            CanEvidenceRecord::TxSubmitted {
                capture_id: 1,
                sequence: 1,
                echo_id: 9,
                frame: frame.clone(),
            },
            CanEvidenceRecord::TxEchoConfirmed {
                capture_id: 1,
                sequence: 1,
                echo_id: 9,
                submitted_frame: frame.clone(),
                echo_frame: frame,
                raw_usb: vec![0xAA],
            },
        ];
        let mut legacy_submission = serde_json::to_value(&records[1]).unwrap();
        legacy_submission["data"]
            .as_object_mut()
            .unwrap()
            .remove("capture_id");
        let mut lines: Vec<String> = records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap())
            .collect();
        lines.push(legacy_submission.to_string());
        std::fs::write(&journal_path, lines.join("\n")).unwrap();

        let report = replay_can_capture_report_path(&path, &[]).unwrap();
        assert_eq!(report.evidence_records, 4);
        assert_eq!(report.scan_results, 1);
        assert_eq!(report.tx_submitted, 2);
        assert_eq!(report.tx_echo_confirmed, 1);
        assert!(report.evidence_errors.is_empty());
        assert_eq!(
            report.evidence_journal.as_deref(),
            Some(journal_path.to_str().unwrap())
        );
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(journal_path);
    }

    #[test]
    fn replay_report_distinguishes_clean_close_from_interrupted_capture() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let started = serde_json::json!({
            "record":"session_started",
            "data":{"format_version":2,"capture_id":1,"bitrate":500000,"listen_only":true,"source_id":"can:test"}
        });
        let finished = serde_json::json!({"record":"session_finished","data":{"capture_id":1}});
        let mut reports = Vec::new();
        for (suffix, journal) in [
            ("complete", vec![started.clone(), finished]),
            ("interrupted", vec![started]),
            (
                "orphan-finish",
                vec![serde_json::json!({
                    "record":"session_finished",
                    "data":{"capture_id":1}
                })],
            ),
        ] {
            let path =
                std::env::temp_dir().join(format!("lognook-close-{stamp}-{suffix}.candump.csv"));
            CanCaptureWriter::create_new(&path)
                .unwrap()
                .finish()
                .unwrap();
            let journal_path = can_evidence_path(&path);
            std::fs::write(
                &journal_path,
                journal
                    .iter()
                    .map(serde_json::Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            reports.push((
                replay_can_capture_report_path(&path, &[]).unwrap(),
                path,
                journal_path,
            ));
        }
        let complete = serde_json::to_value(&reports[0].0).unwrap();
        let interrupted = serde_json::to_value(&reports[1].0).unwrap();
        assert_eq!(
            complete.get("session_finalized").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            interrupted
                .get("session_finalized")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(reports[2].0.session_finalized.is_none());
        assert!(!reports[2].0.evidence_errors.is_empty());
        for (_, path, journal_path) in reports {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(journal_path);
        }
    }
}
