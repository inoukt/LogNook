use super::engine;
use crate::AppState;
use lognook_core::{
    CanAnalysisConfig, CanAnalyzer, CanCaptureReader, CanCaptureWriter, CanCorrelationHypothesis,
    CanDeviceCandidate, CanDeviceSelection, CanDiagnosticRoute, CanEventCorrelation,
    CanEventCorrelator, CanEvidenceRecord, CanFrame, CanIdComparison, CanIdReplayStatus,
    CanManualEventMark, CanRequestTiming, CanScanConfig, CanScanMode, CanSignalDefinition,
    CanTriggerCapture, CanTriggerCaptureResult, CanWorkbenchConfig, CanWorkbenchStatus,
    OemCandidateTrySpec, OemCatalog, Sample, SampleQuality,
    available_can_adapters as list_can_adapters, can_dbc_skeleton, can_evidence_path,
    compare_can_census, decode_can_signal,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
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
pub async fn choose_can_dbc_export_path(app: AppHandle) -> Result<Option<String>, String> {
    app.dialog()
        .file()
        .add_filter("DBC files", &["dbc"])
        .set_file_name("can-research-skeleton.dbc")
        .blocking_save_file()
        .map(|file| file.into_path().map_err(|error| error.to_string()))
        .transpose()
        .map(|path| path.map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn choose_can_signal_save_path(
    app: AppHandle,
    signal_id: String,
) -> Result<Option<String>, String> {
    let file_name = format!("{}.can-signal.json", safe_can_signal_file_stem(&signal_id));
    app.dialog()
        .file()
        .add_filter("CAN signal definition", &["json"])
        .set_file_name(file_name)
        .blocking_save_file()
        .map(|file| file.into_path().map_err(|error| error.to_string()))
        .transpose()
        .map(|path| path.map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn save_can_signal_definition(
    path: String,
    definition: CanSignalDefinition,
) -> Result<(), String> {
    definition.validate().map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(&definition).map_err(|error| error.to_string())?;
    std::fs::write(&path, json)
        .map_err(|error| format!("Could not save CAN signal definition: {error}"))
}

fn safe_can_signal_file_stem(signal_id: &str) -> String {
    let stem: String = signal_id
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let stem: String = stem.trim_matches('-').chars().take(80).collect();
    if stem.is_empty() {
        "can-field".into()
    } else {
        stem
    }
}

#[tauri::command]
pub async fn choose_can_trigger_save_path(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("CAN captures", &["csv"])
        .set_file_name("trigger-window.candump.csv")
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
pub async fn disconnect_can(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .disconnect_can()
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
pub async fn preview_can_signal(
    frame: CanFrame,
    definition: CanSignalDefinition,
) -> Result<Option<Sample>, String> {
    decode_can_signal(&frame, &definition, "can:preview").map_err(|error| error.to_string())
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
pub async fn mark_can_event(
    state: State<'_, AppState>,
    label: String,
    pre_ms: u64,
    post_ms: u64,
) -> Result<CanManualEventMark, String> {
    engine(&state)?
        .mark_can_event(&label, pre_ms, post_ms)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_can_event_results(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .clear_can_event_results()
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
pub async fn start_can_id_replay(
    state: State<'_, AppState>,
    frames: Vec<CanFrame>,
) -> Result<CanIdReplayStatus, String> {
    engine(&state)?
        .start_can_id_replay(frames)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_can_id_replay(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .stop_can_id_replay()
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

#[tauri::command]
pub async fn arm_can_trigger(
    state: State<'_, AppState>,
    pre_ms: u64,
    post_ms: u64,
    max_frames: usize,
) -> Result<(), String> {
    engine(&state)?
        .arm_can_trigger(pre_ms, post_ms, max_frames)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn trigger_can_now(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .trigger_can_now()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disarm_can_trigger(state: State<'_, AppState>) -> Result<(), String> {
    engine(&state)?
        .disarm_can_trigger()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_can_trigger_capture(
    path: String,
    capture: CanTriggerCaptureResult,
) -> Result<(), String> {
    save_trigger_capture_path(Path::new(&path), &capture)
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
    engine(&state)?
        .start_can_oem_candidate(spec.clone())
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
    /// Exact routes read from the paired journal; absent when no route was recorded.
    pub diagnostic_routes: Vec<CanDiagnosticRoute>,
    pub route_provenance: Vec<CanRouteProvenance>,
    pub timings: Vec<CanRequestTiming>,
    pub correlation_sample_count: usize,
    pub correlation_hypotheses: Vec<CanCorrelationHypothesis>,
    pub evidence_journal: Option<String>,
    pub evidence_journal_error: Option<String>,
    pub session_finalized: Option<bool>,
    pub source_id: Option<String>,
    pub bitrate: Option<u32>,
    pub listen_only: Option<bool>,
    pub evidence_records: u64,
    pub scan_results: u64,
    pub tx_submitted: u64,
    pub tx_echo_confirmed: u64,
    pub transport_errors: u64,
    pub trigger_marks: u64,
    pub trigger_completions: u64,
    pub trigger_early_ends: u64,
    pub trigger_window_exports: u64,
    pub manual_event_marks: Vec<CanManualEventMark>,
    pub event_correlations: Vec<CanEventCorrelation>,
    pub evidence_errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CanRouteProvenance {
    pub route: CanDiagnosticRoute,
    pub scan_modes: Vec<String>,
    pub source_candidate_ids: Vec<String>,
}

#[tauri::command]
pub async fn replay_can_capture_report(
    path: String,
    fields: Vec<CanSignalDefinition>,
    obd_samples: Vec<Sample>,
) -> Result<CanCaptureReplayReport, String> {
    for field in &fields {
        field.validate().map_err(|error| error.to_string())?;
    }
    replay_can_capture_report_path_with_samples(Path::new(&path), &fields, &obd_samples)
}

fn replay_can_capture_report_path(
    path: &Path,
    fields: &[CanSignalDefinition],
) -> Result<CanCaptureReplayReport, String> {
    replay_can_capture_report_path_with_samples(path, fields, &[])
}

fn replay_can_capture_report_path_with_samples(
    path: &Path,
    fields: &[CanSignalDefinition],
    obd_samples: &[Sample],
) -> Result<CanCaptureReplayReport, String> {
    let obd_samples = &obd_samples[..obd_samples.len().min(1024)];
    let mut reader = CanCaptureReader::open(path).map_err(|error| error.to_string())?;
    let mut report = CanCaptureReplayReport {
        frames: 0,
        decoded_samples: 0,
        malformed_rows: Vec::new(),
        analyzer_issues: Vec::new(),
        census: Vec::new(),
        diagnostic_routes: Vec::new(),
        route_provenance: Vec::new(),
        timings: Vec::new(),
        correlation_sample_count: 0,
        correlation_hypotheses: Vec::new(),
        evidence_journal: None,
        evidence_journal_error: None,
        session_finalized: None,
        source_id: None,
        bitrate: None,
        listen_only: None,
        evidence_records: 0,
        scan_results: 0,
        tx_submitted: 0,
        tx_echo_confirmed: 0,
        transport_errors: 0,
        trigger_marks: 0,
        trigger_completions: 0,
        trigger_early_ends: 0,
        trigger_window_exports: 0,
        manual_event_marks: Vec::new(),
        event_correlations: Vec::new(),
        evidence_errors: Vec::new(),
    };
    summarize_evidence_journal(path, &mut report);
    let mut event_correlator =
        CanEventCorrelator::new(&report.manual_event_marks).map_err(|error| error.to_string())?;
    let mut analysis_config = CanAnalysisConfig::default();
    analysis_config.diagnostic_routes = report.diagnostic_routes.clone();
    let mut analyzer = CanAnalyzer::new(analysis_config).map_err(|error| error.to_string())?;
    if let Some((capture_wall_ns, capture_elapsed_ns)) = first_capture_time(path) {
        for sample in obd_samples {
            let mut sample = sample.clone();
            sample.elapsed_ns = sample
                .wall_time_unix_ns
                .saturating_sub(capture_wall_ns)
                .saturating_add(capture_elapsed_ns);
            analyzer.feed_obd_sample(&sample);
            if sample.quality == SampleQuality::Valid
                && sample.elapsed_ns >= 0
                && sample.value.is_finite()
            {
                report.correlation_sample_count += 1;
            }
        }
    }
    let mut last_elapsed_ns = None;
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
        last_elapsed_ns = Some(frame.elapsed_ns);
        if let Err(error) = analyzer.feed(&frame) {
            if report.analyzer_issues.len() < 100 {
                report.analyzer_issues.push(error.to_string());
            }
        }
        if let Err(error) = event_correlator.feed(&frame)
            && report.analyzer_issues.len() < 100
        {
            report.analyzer_issues.push(error.to_string());
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
    report.event_correlations = event_correlator.finish();
    if let Some(last) = last_elapsed_ns {
        analyzer.advance_time(last.saturating_add(1_000_000_001));
    }
    report.timings = analyzer.timings();
    report.correlation_hypotheses = correlation_hypotheses(&analyzer, fields, obd_samples);
    Ok(report)
}

fn first_capture_time(path: &Path) -> Option<(i64, i64)> {
    CanCaptureReader::open(path)
        .ok()?
        .find_map(Result::ok)
        .map(|frame| (frame.wall_time_unix_ns, frame.elapsed_ns))
}

fn correlation_hypotheses(
    analyzer: &CanAnalyzer,
    fields: &[CanSignalDefinition],
    samples: &[Sample],
) -> Vec<CanCorrelationHypothesis> {
    let sources: BTreeSet<_> = samples
        .iter()
        .map(|sample| {
            (
                sample.source_id.as_str(),
                sample.ecu_id.as_deref(),
                sample.signal_id.as_str(),
            )
        })
        .collect();
    let mut hypotheses = Vec::new();
    for field in fields.iter().take(64) {
        for (source, ecu, signal) in sources.iter().take(64) {
            if let Ok(Some(hypothesis)) = analyzer.correlate(field, source, *ecu, signal)
                && hypotheses.len() < 256
            {
                hypotheses.push(hypothesis);
            }
        }
    }
    hypotheses
}

#[derive(Clone, Debug, Serialize)]
pub struct CanCaptureComparisonSource {
    pub frames: u64,
    pub evidence_journal: Option<String>,
    pub evidence_journal_error: Option<String>,
    pub session_finalized: Option<bool>,
    pub source_id: Option<String>,
    pub bitrate: Option<u32>,
    pub listen_only: Option<bool>,
    pub evidence_errors: Vec<String>,
    pub malformed_rows: Vec<String>,
    pub analyzer_issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CanCaptureComparison {
    pub left: CanCaptureComparisonSource,
    pub right: CanCaptureComparisonSource,
    pub differences: Vec<CanIdComparison>,
}

#[tauri::command]
pub async fn compare_can_captures(
    left_path: String,
    right_path: String,
) -> Result<CanCaptureComparison, String> {
    compare_can_capture_paths(Path::new(&left_path), Path::new(&right_path))
}

fn compare_can_capture_paths(
    left_path: &Path,
    right_path: &Path,
) -> Result<CanCaptureComparison, String> {
    let left = replay_can_capture_report_path(left_path, &[])?;
    let right = replay_can_capture_report_path(right_path, &[])?;
    Ok(CanCaptureComparison {
        left: comparison_source(&left),
        right: comparison_source(&right),
        differences: compare_can_census(&left.census, &right.census),
    })
}

fn comparison_source(report: &CanCaptureReplayReport) -> CanCaptureComparisonSource {
    CanCaptureComparisonSource {
        frames: report.frames,
        evidence_journal: report.evidence_journal.clone(),
        evidence_journal_error: report.evidence_journal_error.clone(),
        session_finalized: report.session_finalized,
        source_id: report.source_id.clone(),
        bitrate: report.bitrate,
        listen_only: report.listen_only,
        evidence_errors: report.evidence_errors.clone(),
        malformed_rows: report.malformed_rows.clone(),
        analyzer_issues: report.analyzer_issues.clone(),
    }
}

#[tauri::command]
pub async fn export_can_dbc_skeleton(capture_path: String, path: String) -> Result<(), String> {
    let report = replay_can_capture_report_path(Path::new(&capture_path), &[])?;
    let output = can_dbc_skeleton(&report.census);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    if let Err(error) = file
        .write_all(output.as_bytes())
        .and_then(|()| file.sync_data())
    {
        let _ = std::fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

fn save_trigger_capture_path(path: &Path, result: &CanTriggerCaptureResult) -> Result<(), String> {
    validate_trigger_capture(&result.capture)?;
    let journal_path = can_evidence_path(path);
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&journal_path)
        .map_err(|error| format!("Cannot create trigger evidence journal: {error}"))?;
    let mut writer = match CanCaptureWriter::create_new(path) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = std::fs::remove_file(journal_path);
            return Err(error.to_string());
        }
    };
    let write_result = (|| {
        for frame in &result.capture.frames {
            writer
                .write_frame(frame)
                .map_err(|error| error.to_string())?;
        }
        writer.finish().map_err(|error| error.to_string())?;
        let records = [
            CanEvidenceRecord::SessionStarted {
                format_version: 2,
                capture_id: result.source_capture_id,
                bitrate: result.bitrate,
                listen_only: result.listen_only,
                source_id: result.source_id.clone(),
            },
            CanEvidenceRecord::TriggerWindowExported {
                source_capture_id: result.source_capture_id,
                source_id: result.source_id.clone(),
                bitrate: result.bitrate,
                listen_only: result.listen_only,
                trigger_elapsed_ns: result.capture.trigger_elapsed_ns,
                pre_ns: result.capture.pre_ns,
                post_ns: result.capture.post_ns,
                max_frames: result.capture.max_frames,
                frame_count: result.capture.frames.len(),
                truncated: result.capture.truncated,
                complete: result.capture.complete,
            },
            CanEvidenceRecord::SessionFinished {
                capture_id: result.source_capture_id,
            },
        ];
        for record in records {
            serde_json::to_writer(&mut journal, &record).map_err(|error| error.to_string())?;
            journal
                .write_all(b"\n")
                .map_err(|error| error.to_string())?;
        }
        journal.flush().map_err(|error| error.to_string())?;
        journal.sync_data().map_err(|error| error.to_string())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(journal_path);
        return Err(error);
    }
    Ok(())
}

fn validate_trigger_capture(capture: &CanTriggerCapture) -> Result<(), String> {
    if capture.trigger_elapsed_ns < 0
        || capture.pre_ns < 0
        || capture.post_ns < 0
        || capture.max_frames == 0
        || capture.frames.len() > capture.max_frames
    {
        return Err("Invalid CAN trigger capture bounds".into());
    }
    let mut last_time = None;
    for frame in &capture.frames {
        frame.validate().map_err(|error| error.to_string())?;
        if last_time.is_some_and(|previous| frame.elapsed_ns < previous) {
            return Err("Trigger capture frames must be in timestamp order".into());
        }
        last_time = Some(frame.elapsed_ns);
    }
    Ok(())
}

fn summarize_evidence_journal(path: &Path, report: &mut CanCaptureReplayReport) {
    let mut routes = BTreeMap::<(u32, u32, bool), CanDiagnosticRoute>::new();
    let mut route_modes = BTreeMap::<(u32, u32, bool), BTreeSet<String>>::new();
    let mut route_candidates = BTreeMap::<(u32, u32, bool), BTreeSet<String>>::new();
    let mut route_limit_reported = false;
    let mut event_mark_limit_reported = false;
    let journal_path = can_evidence_path(path);
    let file = match File::open(&journal_path) {
        Ok(file) => file,
        Err(error) => {
            if let Some(message) = evidence_journal_open_error(&error) {
                report.evidence_journal = Some(journal_path.to_string_lossy().into_owned());
                report.evidence_journal_error = Some(message);
            }
            return;
        }
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
                let scan_config = match &record {
                    CanEvidenceRecord::ScanStarted { config }
                    | CanEvidenceRecord::ScanStopped { config, .. }
                    | CanEvidenceRecord::ScanCompleted { config } => Some(config),
                    CanEvidenceRecord::ScanResult { result } => Some(&result.config),
                    _ => None,
                };
                if let Some(config) = scan_config
                    && let Some(route) = diagnostic_route(&config.mode)
                {
                    let key = (route.request_id, route.response_id, route.extended);
                    if routes.contains_key(&key) || routes.len() < 256 {
                        routes.entry(key).or_insert(route);
                        let modes = route_modes.entry(key).or_default();
                        if modes.len() < 32 {
                            modes.insert(serde_json::to_string(&config.mode).unwrap_or_default());
                        }
                        if let Some(candidate_id) = &config.source_candidate_id {
                            let candidates = route_candidates.entry(key).or_default();
                            if candidates.len() < 32 {
                                candidates.insert(candidate_id.clone());
                            }
                        }
                    } else if !route_limit_reported {
                        route_limit_reported = true;
                        push_bounded(
                            &mut report.evidence_errors,
                            "Journal has more than 256 diagnostic routes; timing analysis was capped"
                                .into(),
                        );
                    }
                }
                report.evidence_records += 1;
                match record {
                    CanEvidenceRecord::SessionStarted {
                        capture_id,
                        source_id,
                        bitrate,
                        listen_only,
                        ..
                    } => {
                        active_capture_id = Some(capture_id);
                        report.session_finalized = Some(false);
                        report.source_id = Some(source_id);
                        report.bitrate = Some(bitrate);
                        report.listen_only = Some(listen_only);
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
                    CanEvidenceRecord::TriggerMarked { .. } => report.trigger_marks += 1,
                    CanEvidenceRecord::ManualEventMarked { capture_id, mark }
                        if active_capture_id == Some(capture_id) =>
                    {
                        if report.manual_event_marks.len() < 256 {
                            report.manual_event_marks.push(mark);
                        } else if !event_mark_limit_reported {
                            event_mark_limit_reported = true;
                            push_bounded(
                                &mut report.evidence_errors,
                                "Event correlation analysis is capped at 256 marks; remaining marks stay in the JSONL sidecar".into(),
                            );
                        }
                    }
                    CanEvidenceRecord::ManualEventMarked { .. } => {}
                    CanEvidenceRecord::TriggerCompleted { complete: true, .. } => {
                        report.trigger_completions += 1
                    }
                    CanEvidenceRecord::TriggerCompleted { .. }
                    | CanEvidenceRecord::TriggerEndedEarly { .. } => report.trigger_early_ends += 1,
                    CanEvidenceRecord::TriggerWindowExported { .. } => {
                        report.trigger_window_exports += 1
                    }
                    _ => {}
                }
            }
            Err(error) => push_bounded(
                &mut report.evidence_errors,
                format!("Journal line {line_number}: {error}"),
            ),
        }
    }
    report.diagnostic_routes = routes.values().cloned().collect();
    report.route_provenance = routes
        .into_iter()
        .map(|(key, route)| CanRouteProvenance {
            route,
            scan_modes: route_modes
                .remove(&key)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            source_candidate_ids: route_candidates
                .remove(&key)
                .unwrap_or_default()
                .into_iter()
                .collect(),
        })
        .collect();
}

fn diagnostic_route(mode: &CanScanMode) -> Option<CanDiagnosticRoute> {
    let (request_id, response_id, extended) = match mode {
        CanScanMode::CanIdRange { .. } => return None,
        CanScanMode::Mode21 {
            request_id,
            response_id,
            extended,
            ..
        }
        | CanScanMode::Mode22 {
            request_id,
            response_id,
            extended,
            ..
        }
        | CanScanMode::Mode01Support {
            request_id,
            response_id,
            extended,
        }
        | CanScanMode::Mode01Pids {
            request_id,
            response_id,
            extended,
            ..
        } => (*request_id, *response_id, *extended),
    };
    Some(CanDiagnosticRoute {
        request_id,
        response_id,
        extended,
    })
}

fn evidence_journal_open_error(error: &io::Error) -> Option<String> {
    (error.kind() != io::ErrorKind::NotFound).then(|| error.to_string())
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
        CanEvidenceScanResult, CanFrame, CanManualEventMark, CanScanMode, CanSignalDefinition,
        CanTriggerCapture, CanTriggerCaptureResult, CanTriggerEndReason, SampleOrigin,
        SampleQuality,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sidecar_open_errors_only_classify_not_found_as_absent() {
        assert_eq!(
            evidence_journal_open_error(&io::Error::from(io::ErrorKind::NotFound)),
            None
        );
        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
        assert_eq!(
            evidence_journal_open_error(&denied),
            Some("permission denied".to_owned())
        );
    }

    #[test]
    fn can_signal_export_saves_one_valid_definition_as_json() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-can-field-{stamp}.json"));
        let definition = CanSignalDefinition {
            id: "can:engine_rpm".into(),
            name: "Engine RPM".into(),
            can_id: 0x120,
            extended: false,
            start_bit: 0,
            bit_length: 16,
            byte_order: lognook_core::CanByteOrder::Intel,
            signed: false,
            factor: 0.25,
            offset: 0.0,
            unit: "rpm".into(),
        };
        save_can_signal_definition(path.to_string_lossy().into_owned(), definition.clone())
            .unwrap();
        let saved: CanSignalDefinition =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, definition);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn can_signal_export_suggests_a_sanitized_individual_filename() {
        assert_eq!(
            safe_can_signal_file_stem("can:engine rpm/left"),
            "can-engine-rpm-left"
        );
        assert_eq!(safe_can_signal_file_stem("::"), "can-field");
    }

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
    fn replay_ranks_messages_that_change_at_repeated_manual_event_marks() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-event-mark-{stamp}.candump.csv"));
        let mut writer = CanCaptureWriter::create_new(&path).unwrap();
        for (elapsed_ns, can_id, data) in [
            (80, 0x123, vec![0]),
            (85, 0x124, vec![7]),
            (110, 0x123, vec![1]),
            (115, 0x124, vec![7]),
            (280, 0x123, vec![1]),
            (285, 0x124, vec![7]),
            (310, 0x123, vec![0]),
            (315, 0x124, vec![7]),
        ] {
            writer
                .write_frame(&CanFrame {
                    elapsed_ns,
                    wall_time_unix_ns: 1_000 + elapsed_ns,
                    can_id,
                    extended: false,
                    direction: CanDirection::Rx,
                    data,
                    flags: 0,
                })
                .unwrap();
        }
        writer.finish().unwrap();
        let journal_path = can_evidence_path(&path);
        let mut journal = File::create(&journal_path).unwrap();
        for record in [
            CanEvidenceRecord::SessionStarted {
                format_version: 2,
                capture_id: 9,
                bitrate: 500_000,
                listen_only: true,
                source_id: "can:test".into(),
            },
            CanEvidenceRecord::ManualEventMarked {
                capture_id: 9,
                mark: CanManualEventMark {
                    label: "Traction control toggle".into(),
                    elapsed_ns: 100,
                    pre_ns: 50,
                    post_ns: 50,
                },
            },
            CanEvidenceRecord::ManualEventMarked {
                capture_id: 9,
                mark: CanManualEventMark {
                    label: "Traction control toggle".into(),
                    elapsed_ns: 300,
                    pre_ns: 50,
                    post_ns: 50,
                },
            },
            CanEvidenceRecord::SessionFinished { capture_id: 9 },
        ] {
            serde_json::to_writer(&mut journal, &record).unwrap();
            use std::io::Write;
            journal.write_all(b"\n").unwrap();
        }
        drop(journal);

        let report = replay_can_capture_report_path(&path, &[]).unwrap();
        assert_eq!(report.manual_event_marks.len(), 2);
        assert_eq!(report.event_correlations.len(), 1);
        assert_eq!(report.event_correlations[0].event_count, 2);
        assert_eq!(report.event_correlations[0].candidates[0].can_id, 0x123);
        assert_eq!(report.event_correlations[0].candidates[0].changed_events, 2);
        assert_eq!(
            report.event_correlations[0].candidates[0].changed_bytes[0].byte_index,
            0
        );
        assert_eq!(
            report.event_correlations[0].candidates[0].changed_bytes[0]
                .transitions
                .len(),
            2
        );

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(journal_path);
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

    #[test]
    fn replay_report_pairs_only_journal_routes_and_preserves_nrc_and_raw_payloads() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-timing-{stamp}.candump.csv"));
        let mut writer = CanCaptureWriter::create_new(&path).unwrap();
        let mut request = CanFrame {
            elapsed_ns: 100,
            wall_time_unix_ns: 1_000,
            can_id: 0x700,
            extended: false,
            direction: CanDirection::Tx,
            data: vec![2, 0x21, 1],
            flags: 0,
        };
        writer.write_frame(&request).unwrap();
        request.elapsed_ns = 120;
        request.wall_time_unix_ns = 1_020;
        request.can_id = 0x708;
        request.direction = CanDirection::Rx;
        request.data = vec![3, 0x7f, 0x21, 0x33];
        writer.write_frame(&request).unwrap();
        writer.finish().unwrap();

        let journal_path = can_evidence_path(&path);
        std::fs::write(
            &journal_path,
            serde_json::to_vec(&CanEvidenceRecord::ScanStarted {
                config: CanEvidenceScanConfig {
                    mode: CanScanMode::Mode21 {
                        request_id: 0x700,
                        response_id: 0x708,
                        extended: false,
                        start_pid: 1,
                        end_pid: 1,
                    },
                    pacing_ms: 100,
                    response_timeout_ms: 1_000,
                    source_candidate_id: Some("source:research".into()),
                },
            })
            .unwrap(),
        )
        .unwrap();

        let report = replay_can_capture_report_path(&path, &[]).unwrap();
        assert_eq!(report.timings.len(), 1);
        assert_eq!(report.timings[0].latency_ns, 20);
        assert_eq!(report.timings[0].request_payload, vec![0x21, 1]);
        assert_eq!(report.timings[0].response_payload, vec![0x7f, 0x21, 0x33]);
        assert_eq!(
            report.timings[0].outcome,
            CanDiagnosticOutcome::Negative { nrc: 0x33 }
        );
        assert_eq!(report.diagnostic_routes.len(), 1);
        assert_eq!(
            report.route_provenance[0].source_candidate_ids,
            vec!["source:research"]
        );

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(journal_path);
    }

    #[test]
    fn saved_report_is_deterministic_and_correlates_explicit_timestamped_obd_samples() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-correlation-{stamp}.candump.csv"));
        let mut writer = CanCaptureWriter::create_new(&path).unwrap();
        for index in 0..4i64 {
            writer
                .write_frame(&CanFrame {
                    elapsed_ns: index * 10,
                    wall_time_unix_ns: 10_000 + index * 10,
                    can_id: 0x123,
                    extended: false,
                    direction: CanDirection::Rx,
                    data: vec![index as u8],
                    flags: 0,
                })
                .unwrap();
        }
        writer.finish().unwrap();
        let field = CanSignalDefinition {
            id: "can:observed-byte".into(),
            name: "Observed byte".into(),
            can_id: 0x123,
            extended: false,
            start_bit: 0,
            bit_length: 8,
            byte_order: CanByteOrder::Intel,
            signed: false,
            factor: 1.0,
            offset: 0.0,
            unit: "raw".into(),
        };
        let samples: Vec<_> = (0..4i64)
            .map(|index| Sample {
                elapsed_ns: 50_000,
                wall_time_unix_ns: 10_000 + index * 10,
                source_id: "obd:test".into(),
                ecu_id: Some("engine".into()),
                signal_id: "rpm".into(),
                value: index as f64 * 100.0,
                unit: "rpm".into(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            })
            .collect();

        let first =
            replay_can_capture_report_path_with_samples(&path, &[field.clone()], &samples).unwrap();
        let second =
            replay_can_capture_report_path_with_samples(&path, &[field], &samples).unwrap();
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(first.census[0].bytes[0].value_counts.len(), 4);
        assert_eq!(first.correlation_sample_count, 4);
        assert_eq!(first.correlation_hypotheses.len(), 1);
        assert_eq!(first.correlation_hypotheses[0].pairs, 4);
        assert!((first.correlation_hypotheses[0].pearson_r - 1.0).abs() < 1e-12);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn saved_capture_comparison_includes_ids_present_on_either_side_and_byte_counts() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let left = std::env::temp_dir().join(format!("lognook-left-{stamp}.candump.csv"));
        let right = std::env::temp_dir().join(format!("lognook-right-{stamp}.candump.csv"));
        for (path, frames) in [
            (&left, vec![(0x100, vec![1, 2]), (0x100, vec![1, 2])]),
            (&right, vec![(0x100, vec![1, 3]), (0x101, vec![9])]),
        ] {
            let mut writer = CanCaptureWriter::create_new(path).unwrap();
            for (index, (can_id, data)) in frames.into_iter().enumerate() {
                writer
                    .write_frame(&CanFrame {
                        elapsed_ns: index as i64,
                        wall_time_unix_ns: 1_000 + index as i64,
                        can_id,
                        extended: false,
                        direction: CanDirection::Rx,
                        data,
                        flags: 0,
                    })
                    .unwrap();
            }
            writer.finish().unwrap();
        }
        let comparison = compare_can_capture_paths(&left, &right).unwrap();
        assert_eq!(comparison.differences.len(), 2);
        assert_eq!(comparison.differences[0].left_count, 2);
        assert_eq!(comparison.differences[0].right_count, 1);
        assert_eq!(comparison.differences[0].bytes[1].left.get(&2), Some(&2));
        assert_eq!(comparison.differences[0].bytes[1].right.get(&3), Some(&1));
        assert_eq!(comparison.differences[1].can_id, 0x101);
        assert_eq!(comparison.differences[1].right_count, 1);

        let _ = std::fs::remove_file(left);
        let _ = std::fs::remove_file(right);
    }

    #[test]
    fn triggered_window_export_writes_a_paired_csv_and_clean_provenance_sidecar() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-trigger-{stamp}.candump.csv"));
        let result = CanTriggerCaptureResult {
            source_capture_id: 44,
            source_id: "can:rh02-test".into(),
            bitrate: 500_000,
            listen_only: true,
            capture: CanTriggerCapture {
                trigger_elapsed_ns: 100,
                pre_ns: 10,
                post_ns: 10,
                max_frames: 4,
                frames: vec![
                    CanFrame {
                        elapsed_ns: 90,
                        wall_time_unix_ns: 1_090,
                        can_id: 0x123,
                        extended: false,
                        direction: CanDirection::Rx,
                        data: vec![1],
                        flags: 0,
                    },
                    CanFrame {
                        elapsed_ns: 100,
                        wall_time_unix_ns: 1_100,
                        can_id: 0x123,
                        extended: false,
                        direction: CanDirection::Rx,
                        data: vec![2],
                        flags: 0,
                    },
                ],
                truncated: false,
                complete: true,
            },
        };
        save_trigger_capture_path(&path, &result).unwrap();
        let report = replay_can_capture_report_path(&path, &[]).unwrap();
        assert_eq!(report.frames, 2);
        assert_eq!(report.session_finalized, Some(true));
        assert_eq!(report.source_id.as_deref(), Some("can:rh02-test"));
        assert_eq!(report.trigger_window_exports, 1);
        assert!(report.evidence_errors.is_empty());
        assert!(save_trigger_capture_path(&path, &result).is_err());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(can_evidence_path(&path));
    }

    #[test]
    fn replay_report_counts_disarmed_partial_trigger_as_early_not_complete() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-early-trigger-{stamp}.candump.csv"));
        CanCaptureWriter::create_new(&path)
            .unwrap()
            .finish()
            .unwrap();
        let records = [
            CanEvidenceRecord::SessionStarted {
                format_version: 2,
                capture_id: 9,
                bitrate: 500_000,
                listen_only: true,
                source_id: "can:test".into(),
            },
            CanEvidenceRecord::TriggerMarked {
                capture_id: 9,
                trigger_elapsed_ns: 100,
                pre_ns: 0,
                post_ns: 60_000_000_000,
                max_frames: 8,
            },
            CanEvidenceRecord::TriggerEndedEarly {
                capture_id: 9,
                trigger_elapsed_ns: 100,
                max_frames: 8,
                frame_count: 0,
                truncated: false,
                complete: false,
                reason: CanTriggerEndReason::Disarmed,
            },
            CanEvidenceRecord::SessionFinished { capture_id: 9 },
        ];
        let journal = can_evidence_path(&path);
        std::fs::write(
            &journal,
            records
                .iter()
                .map(|record| serde_json::to_string(record).unwrap())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let report = replay_can_capture_report_path(&path, &[]).unwrap();
        assert_eq!(report.trigger_marks, 1);
        assert_eq!(report.trigger_completions, 0);
        assert_eq!(report.trigger_early_ends, 1);
        assert_eq!(report.session_finalized, Some(true));
        assert!(report.evidence_errors.is_empty());

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(journal);
    }
}
