use super::protocol::*;
use crate::AppState;
use lognook_core::{
    A30Adapter, A30ReadOutcome, A30ReadTest, A30ReadTestResult, CanDeviceSelection, CanDirection,
    CanFrame, CanScanConfig, CanScanMode, CanWorkbenchConfig, Elm327Adapter, ObdLinkSxAdapter,
    OemCandidateOutcome, OemCandidateTryResult, OemCatalog, OpenPort2Adapter,
    available_can_adapters, available_obdlink_sx_ports, paired_spp_devices, standard_obd_signals,
};
use std::collections::HashSet;
use std::{
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, async_runtime};

#[derive(Default)]
pub(crate) struct AgentWorkbenchState {
    capture_path: Option<PathBuf>,
    scan_active: bool,
    send_until: Option<Instant>,
}

impl AgentWorkbenchState {
    pub(crate) fn active_operation(&mut self) -> Option<String> {
        if self.scan_active {
            Some("CAN scan".into())
        } else if self
            .send_until
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            Some("CAN frame send".into())
        } else if self.capture_path.is_some() {
            Some("CAN capture".into())
        } else {
            self.send_until = None;
            None
        }
    }
}

/// Lock and expiry both stop the agent's incomplete CAN operation and close its evidence files.
pub(crate) fn stop_active_operations(app: &AppHandle) -> Option<PathBuf> {
    let state = app.state::<AppState>();
    let (capture_path, scan_active, send_active) = match state.agent_workbench.lock() {
        Ok(mut operation) => {
            let active = operation
                .send_until
                .is_some_and(|until| Instant::now() < until);
            let values = (operation.capture_path.take(), operation.scan_active, active);
            operation.scan_active = false;
            operation.send_until = None;
            values
        }
        Err(_) => return None,
    };
    if let Ok(engine) = state.engine.lock() {
        if capture_path.is_some() {
            let _ = engine.stop_can_capture();
        } else if scan_active || send_active {
            let _ = engine.stop_can_scan();
        }
    }
    let _ = state.trace_store.stop_capture();
    capture_path
}

pub(crate) async fn dispatch(app: &AppHandle, request: AgentRequest) -> AgentResponse {
    if let Err(error) = authorize_action(app) {
        return AgentResponse::Error { error };
    }
    match request {
        AgentRequest::GetStatus => get_status(app),
        AgentRequest::ListAdapters => {
            let app = app.clone();
            let result = async_runtime::spawn_blocking(move || list_adapters(&app)).await;
            match result {
                Ok((adapters, discovery_errors)) => AgentResponse::Adapters {
                    adapters,
                    discovery_errors,
                },
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::ListSignals { query } => AgentResponse::Signals {
            signals: list_signals(&query),
        },
        AgentRequest::SetSignalSelection {
            log_signal_ids,
            graph_signal_ids,
        } => set_signal_selection(app, log_signal_ids, graph_signal_ids),
        AgentRequest::ConnectAdapter {
            choice,
            log_signal_ids,
        } => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || {
                connect_adapter(&app, choice, log_signal_ids)
            })
            .await
            {
                Ok(Ok((adapter_id, supported_signal_ids))) => AgentResponse::Connected {
                    adapter_id,
                    supported_signal_ids,
                },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::UnsupportedAdapter, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::DisconnectAdapter => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || disconnect_adapter(&app)).await {
                Ok(Ok(())) => AgentResponse::Disconnected,
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::Internal, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::SearchOemCandidates {
            query,
            brand,
            model,
            service,
            offset,
            limit,
        } => {
            let state = app.state::<AppState>();
            let path = state.pid_catalog_path.clone();
            match async_runtime::spawn_blocking(move || {
                search_oem_candidates(
                    &OemCatalog::new(path),
                    &query,
                    &brand,
                    &model,
                    service.as_deref(),
                    offset,
                    limit,
                )
            })
            .await
            {
                Ok(Ok(result)) => AgentResponse::OemCandidates { result },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::InvalidRequest, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::TryOemCandidate { candidate_id } => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || try_oem_candidate(&app, &candidate_id))
                .await
            {
                Ok(Ok(result)) => AgentResponse::OemCandidateTry { result },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::UnsupportedAdapter, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::ListMode06Tests => AgentResponse::Mode06Tests {
            result: list_mode06_tests(app),
        },
        AgentRequest::TryMode06Test { test } => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || try_mode06_test(&app, test)).await {
                Ok(Ok(result)) => AgentResponse::Mode06Try { result },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::UnsupportedAdapter, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::GetTrace {
            after_sequence,
            limit,
        } => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || {
                let state = app.state::<AppState>();
                super::trace::get_trace(&state, after_sequence, limit)
            })
            .await
            {
                Ok(Ok(page)) => AgentResponse::Trace { page },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::Internal, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::StartTraceCapture => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || {
                let state = app.state::<AppState>();
                super::trace::start_capture(&state)
            })
            .await
            {
                Ok(Ok(status)) => AgentResponse::TraceCapture { status },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::Internal, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::StopTraceCapture => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || {
                let state = app.state::<AppState>();
                super::trace::stop_capture(&state)
            })
            .await
            {
                Ok(Ok(status)) => AgentResponse::TraceCapture { status },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::Internal, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::ExportEvidence => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || {
                let state = app.state::<AppState>();
                super::trace::export_evidence(&state)
            })
            .await
            {
                Ok(Ok(path)) => AgentResponse::EvidenceExported { path },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::Internal, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
        AgentRequest::Workbench { operation } => {
            let app = app.clone();
            match async_runtime::spawn_blocking(move || workbench(&app, operation)).await {
                Ok(Ok(result)) => AgentResponse::Workbench { result },
                Ok(Err(error)) => AgentResponse::error(AgentErrorCode::InvalidRequest, error),
                Err(error) => AgentResponse::error(AgentErrorCode::Internal, error.to_string()),
            }
        }
    }
}

fn workbench(
    app: &AppHandle,
    operation: AgentWorkbenchOperation,
) -> Result<AgentWorkbenchResult, String> {
    use AgentWorkbenchOperation as Operation;

    if let Operation::StopOperation = operation {
        let path = stop_active_operations(app);
        return Ok(workbench_result("stopped", path));
    }

    let state = app.state::<AppState>();
    let mut active = state
        .agent_workbench
        .lock()
        .map_err(|_| "Agent Workbench state is unavailable".to_owned())?;
    if !is_agent_access_unlocked(&state) {
        return Err("Agent Access is locked; unlock it in LogNook Settings".into());
    }
    let engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;

    match operation {
        Operation::StartCapture => {
            if active.capture_path.is_some() {
                return Err("An agent CAN capture is already active".into());
            }
            let path = agent_can_capture_path(&state)?;
            engine
                .start_can_capture(&path)
                .map_err(|error| error.to_string())?;
            active.capture_path = Some(path.clone());
            Ok(workbench_result("capture_started", Some(path)))
        }
        Operation::StopCapture => {
            let path = active.capture_path.clone();
            engine
                .stop_can_capture()
                .map_err(|error| error.to_string())?;
            active.capture_path = None;
            active.scan_active = false;
            active.send_until = None;
            Ok(workbench_result("capture_stopped", path))
        }
        Operation::MarkEvent {
            label,
            before_ms,
            after_ms,
        } => {
            engine
                .mark_can_event(&label, before_ms, after_ms)
                .map_err(|error| error.to_string())?;
            Ok(AgentWorkbenchResult {
                operation: "event_marked".into(),
                detail: Some(label),
                evidence_path: active
                    .capture_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
            })
        }
        Operation::StartScan { config } => {
            let config = convert_scan_config(config)?;
            config.validate().map_err(|error| error.to_string())?;
            ensure_can_transmit(&engine)?;
            ensure_agent_capture(&state, &mut active, &engine)?;
            engine
                .start_can_scan(config)
                .map_err(|error| error.to_string())?;
            active.scan_active = true;
            Ok(workbench_result(
                "scan_started",
                active.capture_path.clone(),
            ))
        }
        Operation::StopScan => {
            engine.stop_can_scan().map_err(|error| error.to_string())?;
            active.scan_active = false;
            active.send_until = None;
            Ok(workbench_result(
                "scan_stopped",
                active.capture_path.clone(),
            ))
        }
        Operation::SendFrame {
            can_id,
            extended,
            data,
            pacing_ms,
            repeat,
        } => {
            let frame = CanFrame {
                elapsed_ns: 0,
                wall_time_unix_ns: 0,
                can_id,
                extended,
                direction: CanDirection::Tx,
                data,
                flags: 0,
            };
            frame.validate().map_err(|error| error.to_string())?;
            if !(1..=60_000).contains(&pacing_ms) || !(1..=10_000).contains(&repeat) {
                return Err("CAN send pacing must be 1-60000 ms and repeat 1-10000".into());
            }
            ensure_can_transmit(&engine)?;
            ensure_agent_capture(&state, &mut active, &engine)?;
            if !state.trace_store.capture_status().capturing {
                super::trace::start_capture(&state)?;
            }
            engine
                .send_can_frame(frame, pacing_ms, repeat)
                .map_err(|error| error.to_string())?;
            active.send_until = Some(
                Instant::now()
                    + std::time::Duration::from_millis(pacing_ms.saturating_mul(u64::from(repeat))),
            );
            Ok(workbench_result(
                "frame_send_started",
                active.capture_path.clone(),
            ))
        }
        Operation::StopOperation => unreachable!(),
    }
}

fn is_agent_access_unlocked(state: &AppState) -> bool {
    state
        .agent_access
        .lock()
        .is_ok_and(|mut access| access.status().unlocked)
}

fn ensure_can_transmit(engine: &lognook_core::LogNookEngine) -> Result<(), String> {
    match engine.can_workbench_status() {
        Some(status) if status.listen_only => {
            Err("Switch Candlelight to active mode before this CAN operation".into())
        }
        Some(_) => Ok(()),
        None => Err("Connect Candlelight before starting a CAN Workbench operation".into()),
    }
}

fn ensure_agent_capture(
    state: &AppState,
    active: &mut AgentWorkbenchState,
    engine: &lognook_core::LogNookEngine,
) -> Result<(), String> {
    if active.capture_path.is_some() {
        return Ok(());
    }
    let path = agent_can_capture_path(state)?;
    engine
        .start_can_capture(&path)
        .map_err(|error| error.to_string())?;
    active.capture_path = Some(path);
    Ok(())
}

fn agent_can_capture_path(state: &AppState) -> Result<PathBuf, String> {
    let directory = state
        .recordings_dir
        .parent()
        .ok_or_else(|| "LogNook data directory is unavailable".to_owned())?
        .join("agent-traces")
        .join("can");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(directory.join(format!("agent-can-{timestamp}.candump.csv")))
}

fn workbench_result(operation: &str, path: Option<PathBuf>) -> AgentWorkbenchResult {
    AgentWorkbenchResult {
        operation: operation.into(),
        detail: None,
        evidence_path: path.map(|path| path.to_string_lossy().into_owned()),
    }
}

fn convert_scan_config(config: AgentCanScanConfig) -> Result<CanScanConfig, String> {
    let mode = match config.mode {
        AgentCanScanMode::CanIdRange {
            start_id,
            end_id,
            extended,
            payload,
        } => CanScanMode::CanIdRange {
            start_id,
            end_id,
            extended,
            payload,
        },
        AgentCanScanMode::Mode21 {
            request_id,
            response_id,
            extended,
            start_pid,
            end_pid,
        } => CanScanMode::Mode21 {
            request_id,
            response_id,
            extended,
            start_pid,
            end_pid,
        },
        AgentCanScanMode::Mode22 {
            request_id,
            response_id,
            extended,
            start_did,
            end_did,
        } => CanScanMode::Mode22 {
            request_id,
            response_id,
            extended,
            start_did,
            end_did,
        },
        AgentCanScanMode::Mode01Support {
            request_id,
            response_id,
            extended,
        } => CanScanMode::Mode01Support {
            request_id,
            response_id,
            extended,
        },
        AgentCanScanMode::Mode01Pids {
            request_id,
            response_id,
            extended,
            pids,
            repeat,
        } => CanScanMode::Mode01Pids {
            request_id,
            response_id,
            extended,
            pids,
            repeat,
        },
    };
    let config = CanScanConfig {
        mode,
        pacing_ms: config.pacing_ms,
        response_timeout_ms: config.response_timeout_ms,
        source_candidate_id: None,
    };
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

fn search_oem_candidates(
    catalog: &OemCatalog,
    query: &str,
    brand: &str,
    model: &str,
    service: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<AgentOemSearch, String> {
    if [query, brand, model].iter().any(|value| value.len() > 512) || !(1..=100).contains(&limit) {
        return Err("OEM search text or page size is out of range".into());
    }
    let page = catalog.search(query, brand, model, service, offset, limit)?;
    Ok(AgentOemSearch {
        total: page.total,
        offset: page.offset,
        limit: page.limit,
        candidates: page
            .candidates
            .into_iter()
            .map(|candidate| AgentOemCandidate {
                candidate_id: candidate.candidate_id,
                source_id: candidate.source_id,
                family: candidate.family,
                brand: candidate.brand,
                model_scope: candidate.model_scope,
                service: candidate.service,
                identifier: candidate.identifier,
                request_header: candidate.request_header,
                response_header: candidate.response_header,
                name: candidate.name,
                unit: candidate.unit,
                source_url: candidate.source_url,
                license: candidate.license,
                confidence: candidate.confidence,
                research_state: candidate.research_state,
                vehicle_validation: candidate.vehicle_validation,
                auto_request: candidate.auto_request,
            })
            .collect(),
    })
}

fn summarize_oem_try(result: OemCandidateTryResult) -> AgentOemTrySummary {
    AgentOemTrySummary {
        candidate_id: result.candidate_id,
        exact_adapter_command: result.exact_adapter_command,
        adapter_id: result.adapter.stable_id,
        protocol: result.protocol,
        latency_ms: result.latency_ms,
        outcome: oem_outcome_name(&result.outcome).into(),
        raw_response: result.raw_response,
        response_payload: result.response_payload,
        responder_header: result.responder_header,
        negative_response_code: result.negative_response_code,
        decoded_value: result.decoded_value.as_ref().and_then(|value| value.value),
        unit: result.decoded_value.map(|value| value.unit),
        detail: result.decode_error.or(result.error),
        evidence_saved: result.evidence_saved,
        evidence_save_error: result.evidence_save_error,
    }
}

fn oem_outcome_name(outcome: &OemCandidateOutcome) -> &'static str {
    match outcome {
        OemCandidateOutcome::Positive => "positive",
        OemCandidateOutcome::NegativeResponse => "negative_response",
        OemCandidateOutcome::ResponsePending => "response_pending",
        OemCandidateOutcome::TemporarilyUnavailable => "temporarily_unavailable",
        OemCandidateOutcome::SessionRequired => "session_required",
        OemCandidateOutcome::SecurityRequired => "security_required",
        OemCandidateOutcome::NoResponse => "no_response",
        OemCandidateOutcome::MalformedResponse => "malformed_response",
        OemCandidateOutcome::WrongResponder => "wrong_responder",
        OemCandidateOutcome::InvalidCandidate => "invalid_candidate",
    }
}

fn supported_mode06_tests(adapter_family: Option<&str>) -> Vec<AgentMode06Test> {
    if !adapter_family.is_some_and(|family| family.to_ascii_lowercase().contains("a30")) {
        return vec![];
    }
    vec![
        AgentMode06Test::Bitmap00,
        AgentMode06Test::Bitmap20,
        AgentMode06Test::Bitmap40,
        AgentMode06Test::Bitmap60,
        AgentMode06Test::Bitmap80,
        AgentMode06Test::BitmapA0,
        AgentMode06Test::Tid35,
        AgentMode06Test::TidA2,
        AgentMode06Test::TidA5,
    ]
}

fn ensure_mode06_supported(adapter_family: Option<&str>) -> Result<(), String> {
    if supported_mode06_tests(adapter_family).is_empty() {
        Err("Mode 06 reads are currently available only through a connected A30 adapter".into())
    } else {
        Ok(())
    }
}

fn list_mode06_tests(app: &AppHandle) -> AgentMode06Tests {
    let state = app.state::<AppState>();
    let family = state.engine.lock().ok().and_then(|engine| {
        engine
            .connection_capabilities()
            .map(|caps| caps.device.hardware_family.clone())
    });
    let tests = supported_mode06_tests(family.as_deref());
    AgentMode06Tests {
        active_adapter: family.clone(),
        message: tests
            .is_empty()
            .then(|| "Connect a Bluetooth A30 adapter to run the available Mode 06 tests".into()),
        tests,
    }
}

fn try_oem_candidate(app: &AppHandle, candidate_id: &str) -> Result<AgentOemTrySummary, String> {
    if candidate_id.trim().is_empty() || candidate_id.len() > 256 {
        return Err("Choose a valid OEM candidate ID".into());
    }
    let state = app.state::<AppState>();
    let run_id = agent_run_id();
    crate::commands::execute_candidate_try(&state, candidate_id, &run_id, 1, 1)
        .map(summarize_oem_try)
}

fn try_mode06_test(app: &AppHandle, test: AgentMode06Test) -> Result<AgentMode06Summary, String> {
    let state = app.state::<AppState>();
    let family = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?
        .connection_capabilities()
        .map(|caps| caps.device.hardware_family.clone());
    ensure_mode06_supported(family.as_deref())?;
    let run_id = agent_run_id();
    crate::commands::execute_a30_read_test(&state, to_a30_test(test), &run_id, 1, 1)
        .map(summarize_mode06)
}

fn agent_run_id() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("agent-{timestamp}")
}

fn to_a30_test(test: AgentMode06Test) -> A30ReadTest {
    match test {
        AgentMode06Test::Bitmap00 => A30ReadTest::Mode06Bitmap00,
        AgentMode06Test::Bitmap20 => A30ReadTest::Mode06Bitmap20,
        AgentMode06Test::Bitmap40 => A30ReadTest::Mode06Bitmap40,
        AgentMode06Test::Bitmap60 => A30ReadTest::Mode06Bitmap60,
        AgentMode06Test::Bitmap80 => A30ReadTest::Mode06Bitmap80,
        AgentMode06Test::BitmapA0 => A30ReadTest::Mode06BitmapA0,
        AgentMode06Test::Tid35 => A30ReadTest::Mode06Tid35,
        AgentMode06Test::TidA2 => A30ReadTest::Mode06TidA2,
        AgentMode06Test::TidA5 => A30ReadTest::Mode06TidA5,
    }
}

fn summarize_mode06(result: A30ReadTestResult) -> AgentMode06Summary {
    AgentMode06Summary {
        test: a30_test_name(result.test).into(),
        request_description: result.request_description,
        exact_adapter_command: result.exact_adapter_command,
        adapter_id: result.adapter.stable_id,
        protocol: result.protocol,
        latency_ms: result.latency_ms,
        outcome: a30_outcome_name(&result.outcome).into(),
        responder_header: result.responder_header,
        response_payloads: result.response_payloads,
        negative_response_code: result.negative_response_code,
        graph_values: result
            .graph_values
            .into_iter()
            .map(|value| AgentGraphValue {
                signal_id: value.signal_id,
                name: value.name,
                value: value.value,
                unit: value.unit,
                raw_value: value.raw_value,
            })
            .collect(),
        detail: result.detail,
        raw_response: result.raw_response,
        evidence_saved: result.evidence_saved,
        evidence_save_error: result.evidence_save_error,
    }
}

fn a30_test_name(test: A30ReadTest) -> &'static str {
    match test {
        A30ReadTest::Mode01EngineRpm => "mode_01_engine_rpm",
        A30ReadTest::Mode02FreezeFrameRpm => "mode_02_freeze_frame_rpm",
        A30ReadTest::Mode03StoredDtcs => "mode_03_stored_dtcs",
        A30ReadTest::Mode06Bitmap00 => "mode_06_bitmap_00",
        A30ReadTest::Mode06Bitmap20 => "mode_06_bitmap_20",
        A30ReadTest::Mode06Bitmap40 => "mode_06_bitmap_40",
        A30ReadTest::Mode06Bitmap60 => "mode_06_bitmap_60",
        A30ReadTest::Mode06Bitmap80 => "mode_06_bitmap_80",
        A30ReadTest::Mode06BitmapA0 => "mode_06_bitmap_a0",
        A30ReadTest::Mode06Tid35 => "mode_06_tid_35",
        A30ReadTest::Mode06TidA2 => "mode_06_tid_a2",
        A30ReadTest::Mode06TidA5 => "mode_06_tid_a5",
        A30ReadTest::Mode07PendingDtcs => "mode_07_pending_dtcs",
        A30ReadTest::Mode09Vin => "mode_09_vin",
        A30ReadTest::Mode0aPermanentDtcs => "mode_0a_permanent_dtcs",
    }
}

fn a30_outcome_name(outcome: &A30ReadOutcome) -> &'static str {
    match outcome {
        A30ReadOutcome::Positive => "positive",
        A30ReadOutcome::NegativeResponse => "negative_response",
        A30ReadOutcome::ResponsePending => "response_pending",
        A30ReadOutcome::TemporarilyUnavailable => "temporarily_unavailable",
        A30ReadOutcome::SessionRequired => "session_required",
        A30ReadOutcome::SecurityRequired => "security_required",
        A30ReadOutcome::NoResponse => "no_response",
        A30ReadOutcome::MalformedResponse => "malformed_response",
        A30ReadOutcome::WrongResponder => "wrong_responder",
    }
}

fn authorize_action(app: &AppHandle) -> Result<(), AgentError> {
    let state = app.state::<AppState>();
    state
        .agent_access
        .lock()
        .map_err(|_| AgentError {
            code: AgentErrorCode::Internal,
            message: "Agent Access state is unavailable".into(),
        })?
        .authorize_action()
        .map_err(|message| AgentError {
            code: AgentErrorCode::Locked,
            message,
        })
}

fn get_status(app: &AppHandle) -> AgentResponse {
    let state = app.state::<AppState>();
    let active_operation = match state.agent_workbench.lock() {
        Ok(mut operation) => operation.active_operation(),
        Err(_) => {
            return AgentResponse::error(
                AgentErrorCode::Internal,
                "Agent Workbench state is unavailable",
            );
        }
    };
    let engine = match state.engine.lock() {
        Ok(engine) => engine,
        Err(_) => {
            return AgentResponse::error(
                AgentErrorCode::Internal,
                "Acquisition engine is unavailable",
            );
        }
    };
    let selection = match state.agent_selection.lock() {
        Ok(selection) => selection.clone(),
        Err(_) => {
            return AgentResponse::error(
                AgentErrorCode::Internal,
                "Agent selection is unavailable",
            );
        }
    };
    let access = match state.agent_access.lock() {
        Ok(mut access) => access.status(),
        Err(_) => {
            return AgentResponse::error(
                AgentErrorCode::Internal,
                "Agent Access state is unavailable",
            );
        }
    };
    AgentResponse::Status {
        status: build_agent_status(
            &engine,
            AgentAccessSnapshot {
                configured: access.configured,
                unlocked: access.unlocked,
                expires_at_unix_ms: access.expires_at_unix_ms,
                host_connected: state.agent_ipc.connected(),
            },
            selection,
            active_operation,
        ),
    }
}

fn build_agent_status(
    engine: &lognook_core::LogNookEngine,
    access: AgentAccessSnapshot,
    signal_selection: AgentSignalSelection,
    active_operation: Option<String>,
) -> AgentStatus {
    let capabilities = engine.connection_capabilities();
    let can = engine.can_workbench_status();
    AgentStatus {
        configured: access.configured,
        unlocked: access.unlocked,
        expires_at_unix_ms: access.expires_at_unix_ms,
        host_connected: access.host_connected,
        adapter_id: capabilities
            .map(|caps| caps.device.stable_id.clone())
            .or_else(|| can.map(|status| status.source_id)),
        adapter_connected: engine.is_connected(),
        recording: engine.is_recording(),
        active_operation,
        supported_signal_ids: capabilities
            .map(|caps| {
                caps.supported_signals
                    .iter()
                    .map(|signal| signal.id.clone())
                    .collect()
            })
            .unwrap_or_default(),
        signal_selection,
    }
}

fn list_signals(query: &str) -> Vec<AgentSignal> {
    let query = query.trim().to_ascii_lowercase();
    standard_obd_signals()
        .into_iter()
        .filter(|signal| {
            query.is_empty()
                || [
                    signal.id.as_str(),
                    signal.name.as_str(),
                    signal.unit.as_str(),
                    signal.description.as_str(),
                ]
                .iter()
                .any(|field| field.to_ascii_lowercase().contains(&query))
        })
        .take(200)
        .map(|signal| AgentSignal {
            id: signal.id,
            name: signal.name,
            unit: signal.unit,
            service: signal.service,
            pid: signal.pid,
            description: signal.description,
        })
        .collect()
}

fn list_adapters(app: &AppHandle) -> (Vec<AgentAdapterInfo>, Vec<String>) {
    let mut adapters = vec![AgentAdapterInfo {
        label: "Simulator".into(),
        choice: AgentAdapterChoice::Simulator,
    }];
    let mut errors = Vec::new();
    match available_obdlink_sx_ports() {
        Ok(ports) => adapters.extend(ports.into_iter().map(|port_name| AgentAdapterInfo {
            label: format!("OBDLink SX · {port_name}"),
            choice: AgentAdapterChoice::ObdLinkSx { port_name },
        })),
        Err(error) => errors.push(format!("OBDLink SX discovery: {error}")),
    }
    match paired_spp_devices() {
        Ok(devices) => {
            for device in devices {
                adapters.push(AgentAdapterInfo {
                    label: format!("{} · ELM327", device.name),
                    choice: AgentAdapterChoice::Elm327 {
                        address: device.address.clone(),
                    },
                });
                adapters.push(AgentAdapterInfo {
                    label: format!("{} · A30", device.name),
                    choice: AgentAdapterChoice::A30 {
                        address: device.address,
                    },
                });
            }
        }
        Err(error) => errors.push(format!("Bluetooth discovery: {error}")),
    }
    let state = app.state::<AppState>();
    match crate::commands::discover_providers(&state, crate::commands::worker_path) {
        Ok(discovery) => {
            errors.extend(discovery.diagnostics);
            adapters.extend(
                discovery
                    .providers
                    .into_iter()
                    .map(|provider| AgentAdapterInfo {
                        label: format!("OpenPort 2 · {}", provider.name),
                        choice: AgentAdapterChoice::OpenPort2 {
                            provider_id: provider.id,
                        },
                    }),
            );
        }
        Err(error) => errors.push(format!("OpenPort 2 discovery: {error}")),
    }
    match available_can_adapters() {
        Ok(devices) => adapters.extend(devices.into_iter().map(|device| AgentAdapterInfo {
            label: format!(
                "Candlelight · {}",
                device.product.as_deref().unwrap_or("USB CAN")
            ),
            choice: AgentAdapterChoice::Candlelight {
                id: device.id,
                interface_number: 0,
                channel: 0,
                bitrate: 500_000,
                listen_only: true,
            },
        })),
        Err(error) => errors.push(format!("Candlelight discovery: {error}")),
    }
    (adapters, errors)
}

fn validate_signal_selection(
    engine: &lognook_core::LogNookEngine,
    current_log_signal_ids: &[String],
    log_signal_ids: &[String],
    graph_signal_ids: &[String],
) -> Result<(), String> {
    if log_signal_ids.len() > 128 || graph_signal_ids.len() > 128 {
        return Err("Signal selection is too large".into());
    }
    if log_signal_ids.is_empty() {
        return Err("Select at least one logged PID".into());
    }
    let standard_ids = standard_obd_signals()
        .into_iter()
        .map(|signal| signal.id)
        .collect::<HashSet<_>>();
    let known_ids = standard_ids
        .into_iter()
        .chain(current_log_signal_ids.iter().cloned())
        .collect::<HashSet<_>>();
    if log_signal_ids.iter().any(|id| !known_ids.contains(id))
        || graph_signal_ids.iter().any(|id| !known_ids.contains(id))
    {
        return Err("Signal selection includes an unknown PID".into());
    }
    if log_signal_ids.iter().collect::<HashSet<_>>().len() != log_signal_ids.len()
        || graph_signal_ids.iter().collect::<HashSet<_>>().len() != graph_signal_ids.len()
    {
        return Err("Signal selections must not contain duplicate IDs".into());
    }
    if graph_signal_ids
        .iter()
        .any(|id| !log_signal_ids.contains(id))
    {
        return Err("Graphed PIDs must also be selected for logging".into());
    }
    if engine.is_connected() && !same_ids(current_log_signal_ids, log_signal_ids) {
        return Err("Disconnect before changing logged PIDs".into());
    }
    Ok(())
}

fn same_ids(left: &[String], right: &[String]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort();
    right.sort();
    left == right
}

fn set_signal_selection(
    app: &AppHandle,
    log_signal_ids: Vec<String>,
    graph_signal_ids: Vec<String>,
) -> AgentResponse {
    let state = app.state::<AppState>();
    let engine = match state.engine.lock() {
        Ok(engine) => engine,
        Err(_) => {
            return AgentResponse::error(
                AgentErrorCode::Internal,
                "Acquisition engine is unavailable",
            );
        }
    };
    let mut current = match state.agent_selection.lock() {
        Ok(selection) => selection,
        Err(_) => {
            return AgentResponse::error(
                AgentErrorCode::Internal,
                "Agent selection is unavailable",
            );
        }
    };
    if let Err(error) = validate_signal_selection(
        &engine,
        &current.log_signal_ids,
        &log_signal_ids,
        &graph_signal_ids,
    ) {
        return AgentResponse::error(AgentErrorCode::InvalidSelection, error);
    }
    *current = AgentSignalSelection {
        log_signal_ids,
        graph_signal_ids,
    };
    let selection = current.clone();
    drop(current);
    drop(engine);
    let _ = app.emit("agent-signal-selection", &selection);
    AgentResponse::SelectionUpdated { selection }
}

fn connect_adapter(
    app: &AppHandle,
    choice: AgentAdapterChoice,
    log_signal_ids: Vec<String>,
) -> Result<(String, Vec<String>), String> {
    let state = app.state::<AppState>();
    if let AgentAdapterChoice::Candlelight {
        id,
        interface_number,
        channel,
        bitrate,
        listen_only,
    } = choice.clone()
    {
        let mut engine = state
            .engine
            .lock()
            .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
        let operation =
            state
                .diagnostics
                .begin_operation(Some(format!("candlelight:{id}")), "connect", vec![]);
        engine.set_diagnostic_sink(operation.sink());
        let result = engine
            .connect_candlelight(
                &CanDeviceSelection {
                    id,
                    interface_number,
                    channel,
                },
                CanWorkbenchConfig {
                    bitrate,
                    listen_only,
                },
            )
            .map_err(|error| error.to_string());
        operation.finish_result(&result);
        let status = result?;
        return Ok((status.source_id, vec![]));
    }
    let engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
    let selection = state
        .agent_selection
        .lock()
        .map_err(|_| "Agent selection is unavailable".to_owned())?;
    let graph_signal_ids = selection
        .graph_signal_ids
        .iter()
        .filter(|id| log_signal_ids.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    validate_signal_selection(
        &engine,
        &selection.log_signal_ids,
        &log_signal_ids,
        &graph_signal_ids,
    )?;
    drop(selection);
    drop(engine);
    let adapter_id = adapter_id(&choice);
    let capabilities = crate::commands::connect_with_diagnostics(
        &state,
        adapter_id.clone(),
        log_signal_ids.clone(),
        |engine, ids| connect_adapter_in_engine(engine, choice, ids),
    )?;
    let mut selection = state
        .agent_selection
        .lock()
        .map_err(|_| "Agent selection is unavailable".to_owned())?;
    selection.log_signal_ids = log_signal_ids;
    let selected_log_ids = selection.log_signal_ids.clone();
    selection
        .graph_signal_ids
        .retain(|id| selected_log_ids.contains(id));
    let updated_selection = selection.clone();
    drop(selection);
    let _ = app.emit("agent-signal-selection", &updated_selection);
    Ok((
        capabilities.device.stable_id,
        capabilities
            .supported_signals
            .into_iter()
            .map(|signal| signal.id)
            .collect(),
    ))
}

fn connect_adapter_in_engine(
    engine: &mut lognook_core::LogNookEngine,
    choice: AgentAdapterChoice,
    signal_ids: &[String],
) -> Result<lognook_core::ConnectionCapabilities, String> {
    let adapter: Box<dyn lognook_core::HardwareAdapter> = match choice {
        AgentAdapterChoice::Simulator => {
            return engine
                .connect_simulated(signal_ids)
                .map_err(|error| error.to_string());
        }
        AgentAdapterChoice::ObdLinkSx { port_name } => Box::new(ObdLinkSxAdapter::new(port_name)),
        AgentAdapterChoice::Elm327 { address } => Box::new(Elm327Adapter::new(address)),
        AgentAdapterChoice::A30 { address } => Box::new(A30Adapter::new(address)),
        AgentAdapterChoice::OpenPort2 { provider_id } => Box::new(OpenPort2Adapter::new(
            crate::commands::worker_path()?,
            provider_id,
        )),
        AgentAdapterChoice::Candlelight { .. } => {
            return Err("Use the explicit CAN adapter connection option for Candlelight".into());
        }
    };
    engine
        .connect_adapter(adapter, signal_ids)
        .map_err(|error| error.to_string())
}

fn adapter_id(choice: &AgentAdapterChoice) -> String {
    match choice {
        AgentAdapterChoice::Simulator => "simulator".into(),
        AgentAdapterChoice::ObdLinkSx { port_name } => format!("obdlink-sx:{port_name}"),
        AgentAdapterChoice::Elm327 { address } => format!("elm327:{address}"),
        AgentAdapterChoice::A30 { address } => format!("a30:{address}"),
        AgentAdapterChoice::OpenPort2 { provider_id } => provider_id.clone(),
        AgentAdapterChoice::Candlelight { id, .. } => format!("candlelight:{id}"),
    }
}

fn disconnect_adapter(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
    let adapter_id = engine
        .connection_capabilities()
        .map(|caps| caps.device.stable_id.clone());
    let operation = state
        .diagnostics
        .begin_operation(adapter_id, "disconnect", vec![]);
    let result = engine.disconnect().map_err(|error| error.to_string());
    operation.finish_result(&result);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        AgentWorkbenchState, build_agent_status, connect_adapter_in_engine, convert_scan_config,
        ensure_mode06_supported, search_oem_candidates, summarize_oem_try, supported_mode06_tests,
        validate_signal_selection,
    };
    use crate::agent::protocol::{
        AgentAccessSnapshot, AgentAdapterChoice, AgentCanScanConfig, AgentCanScanMode,
        AgentSignalSelection,
    };
    use lognook_core::{
        DeviceDescriptor, LogNookEngine, OemCandidateOutcome, OemCandidateTryResult, OemCatalog,
    };

    fn engine() -> LogNookEngine {
        LogNookEngine::new(|_| {})
    }

    #[test]
    fn agent_connect_reuses_existing_engine() {
        let mut engine = engine();
        connect_adapter_in_engine(
            &mut engine,
            AgentAdapterChoice::Simulator,
            &["engine_rpm".into()],
        )
        .unwrap();
        assert!(engine.is_connected());
    }

    #[test]
    fn agent_rejects_log_selection_change_while_connected() {
        let mut engine = engine();
        connect_adapter_in_engine(
            &mut engine,
            AgentAdapterChoice::Simulator,
            &["engine_rpm".into()],
        )
        .unwrap();
        assert!(
            validate_signal_selection(
                &engine,
                &["engine_rpm".into()],
                &["coolant_temp".into()],
                &["coolant_temp".into()]
            )
            .is_err()
        );
        assert!(
            validate_signal_selection(&engine, &["engine_rpm".into()], &["engine_rpm".into()], &[])
                .is_ok()
        );
    }

    #[test]
    fn agent_status_reports_adapter_capabilities() {
        let mut engine = engine();
        connect_adapter_in_engine(
            &mut engine,
            AgentAdapterChoice::Simulator,
            &["engine_rpm".into()],
        )
        .unwrap();
        let status = build_agent_status(
            &engine,
            AgentAccessSnapshot {
                configured: true,
                unlocked: true,
                expires_at_unix_ms: Some(1),
                host_connected: true,
            },
            AgentSignalSelection {
                log_signal_ids: vec!["engine_rpm".into()],
                graph_signal_ids: vec!["engine_rpm".into()],
            },
            None,
        );
        assert!(status.adapter_connected);
        assert!(status.host_connected);
        assert_eq!(status.adapter_id.as_deref(), Some("simulated-obd"));
    }

    #[test]
    fn agent_oem_search_preserves_source_and_route() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let catalog = OemCatalog::new(
            root.join("../../../PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite"),
        );
        let page = search_oem_candidates(&catalog, "", "Scion", "xB", Some("21"), 0, 10).unwrap();
        let candidate = page.candidates.first().unwrap();
        assert!(!candidate.source_id.is_empty());
        assert!(!candidate.request_header.is_empty());
        assert!(!candidate.response_header.is_empty());
        assert_eq!(candidate.service, "21");
    }

    fn oem_try(outcome: OemCandidateOutcome, nrc: Option<u8>) -> OemCandidateTryResult {
        OemCandidateTryResult {
            candidate_id: "candidate".into(),
            exact_adapter_command: "STPX".into(),
            adapter: DeviceDescriptor {
                stable_id: "sx".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: None,
            },
            protocol: "CAN".into(),
            started_at_unix_ms: 1,
            latency_ms: 20,
            outcome,
            raw_response: String::new(),
            responder_header: None,
            response_payload: vec![],
            negative_response_code: nrc,
            decoded_value: None,
            decode_error: None,
            error: None,
            evidence_saved: false,
            evidence_save_error: None,
        }
    }

    #[test]
    fn agent_oem_try_returns_nrc_and_timeout_distinctly() {
        let nrc = summarize_oem_try(oem_try(OemCandidateOutcome::NegativeResponse, Some(0x12)));
        let timeout = summarize_oem_try(oem_try(OemCandidateOutcome::NoResponse, None));
        assert_eq!(nrc.outcome, "negative_response");
        assert_eq!(nrc.negative_response_code, Some(0x12));
        assert_eq!(timeout.outcome, "no_response");
        assert_eq!(timeout.negative_response_code, None);
    }

    #[test]
    fn agent_mode06_lists_adapter_capabilities() {
        assert!(!supported_mode06_tests(Some("Xtool A30/A30M")).is_empty());
        assert!(supported_mode06_tests(Some("OBDLink SX")).is_empty());
        assert!(ensure_mode06_supported(Some("Xtool A30/A30M")).is_ok());
    }

    #[test]
    fn agent_mode06_rejects_unsupported_adapter() {
        assert!(ensure_mode06_supported(Some("OBDLink SX")).is_err());
        assert!(ensure_mode06_supported(None).is_err());
    }

    #[test]
    fn agent_workbench_rejects_invalid_scan_before_transport() {
        let config = AgentCanScanConfig {
            mode: AgentCanScanMode::Mode22 {
                request_id: 0x7e0,
                response_id: 0x7e8,
                extended: false,
                start_did: 0x2000,
                end_did: 0x1000,
            },
            pacing_ms: 100,
            response_timeout_ms: 1000,
        };
        assert!(convert_scan_config(config).is_err());
    }

    #[test]
    fn agent_status_tracks_owned_workbench_operation() {
        let mut state = AgentWorkbenchState {
            scan_active: true,
            ..AgentWorkbenchState::default()
        };
        assert_eq!(state.active_operation().as_deref(), Some("CAN scan"));
        state.scan_active = false;
        assert_eq!(state.active_operation(), None);
    }
}
