use super::{diagnose, engine};
use crate::AppState;
use lognook_core::{
    A30GraphValue, A30ReadOutcome, A30ReadTest, A30ReadTestResult, OemCandidate,
    OemCandidateOutcome, OemCandidateTryResult, OemCandidateTrySpec, OemCatalog, OemCatalogPage,
    OemPollingStatus, OemVehicleProfile, ResearchTestRequest, ResearchTestResult, Sample,
    SampleOrigin, SampleQuality, ScionMode21ProbeResult, probe_scion_xb_mode21 as run_probe,
};
use std::{fs::OpenOptions, io::Write};
use tauri::{Manager, State};

const A30_READ_TEST_FAMILY: &str = "Xtool A30/A30M Bluetooth SPP";
const SX_READ_TEST_FAMILY: &str = "OBDLink SX";

#[tauri::command]
pub async fn search_oem_catalog(
    state: State<'_, AppState>,
    query: String,
    brand: String,
    model: String,
    service: Option<String>,
    offset: usize,
    limit: usize,
) -> Result<OemCatalogPage, String> {
    OemCatalog::new(&state.pid_catalog_path).search(
        &query,
        &brand,
        &model,
        service.as_deref(),
        offset,
        limit,
    )
}

#[tauri::command]
pub async fn oem_vehicle_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<OemVehicleProfile>, String> {
    OemCatalog::new(&state.pid_catalog_path).vehicle_profiles()
}

#[tauri::command]
pub async fn search_oem_poll_candidates(
    state: State<'_, AppState>,
    vehicle_id: String,
    query: String,
    offset: usize,
    limit: usize,
) -> Result<OemCatalogPage, String> {
    OemCatalog::new(&state.pid_catalog_path).search_oem_poll_candidates(
        &vehicle_id,
        &query,
        offset,
        limit,
    )
}

#[tauri::command]
pub async fn start_oem_logging(
    state: State<'_, AppState>,
    vehicle_id: String,
    candidate_ids: Vec<String>,
) -> Result<OemPollingStatus, String> {
    set_oem_logging(&state, Some(&vehicle_id), &candidate_ids, None)
}

#[tauri::command]
pub async fn stop_oem_logging(state: State<'_, AppState>) -> Result<OemPollingStatus, String> {
    set_oem_logging(&state, None, &[], None)
}

pub(crate) fn set_oem_logging(
    state: &AppState,
    vehicle_id: Option<&str>,
    candidate_ids: &[String],
    binding: Option<(&str, u64)>,
) -> Result<OemPollingStatus, String> {
    let plan = vehicle_id
        .map(|id| OemCatalog::new(&state.pid_catalog_path).oem_poll_plan(id, candidate_ids))
        .transpose()?;
    let engine = engine(state)?;
    if let Some((adapter_id, generation)) = binding {
        let capabilities = engine
            .connection_capabilities()
            .ok_or_else(|| "No OBD adapter is connected".to_owned())?;
        if capabilities.device.stable_id != adapter_id
            || engine.obd_connection_generation() != generation
        {
            return Err(
                "The OBD adapter or connection changed before the OEM logging action".into(),
            );
        }
    }
    engine
        .set_oem_poll_plan(plan)
        .map_err(|error| error.to_string())?;
    Ok(engine.oem_polling_status())
}

#[tauri::command]
pub async fn oem_logging_status(state: State<'_, AppState>) -> Result<OemPollingStatus, String> {
    Ok(engine(&state)?.oem_polling_status())
}

#[tauri::command]
pub async fn probe_scion_xb_mode21(
    state: State<'_, AppState>,
    port_name: String,
) -> Result<ScionMode21ProbeResult, String> {
    diagnose(
        &state,
        Some(format!("obdlink-sx:{port_name}")),
        "research",
        vec!["scion_xb_mode21".into()],
        |op| {
            let engine = engine(&state)?;
            if engine.is_connected() || engine.is_recording() {
                return Err(
                    "Disconnect acquisition and stop recording before the one-shot OEM read".into(),
                );
            }
            let spec = OemCatalog::new(&state.pid_catalog_path).scion_xb_mode21_probe()?;
            let mut result = run_probe(port_name, &spec).map_err(|error| error.to_string())?;
            let save_attempt = (|| -> Result<(), String> {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&state.oem_validation_log)
                    .map_err(|error| format!("Could not open evidence log: {error}"))?;
                let mut evidence = result.clone();
                evidence.evidence_saved = true;
                let envelope = serde_json::json!({
                    "os": std::env::consts::OS,
                    "app_version": env!("CARGO_PKG_VERSION"),
                    "probe": evidence,
                });
                serde_json::to_writer(&mut file, &envelope).map_err(|error| error.to_string())?;
                file.write_all(b"\n").map_err(|error| error.to_string())
            })();
            match save_attempt {
                Ok(()) => result.evidence_saved = true,
                Err(error) => result.evidence_save_error = Some(error),
            }
            op.research_outcome(
                &result.outcome,
                None,
                result.evidence_saved,
                result.evidence_save_error.as_deref(),
            );
            Ok(result)
        },
    )
}

#[tauri::command]
pub async fn try_oem_candidate(
    state: State<'_, AppState>,
    candidate_id: String,
    run_id: String,
    queue_index: usize,
    queue_size: usize,
) -> Result<OemCandidateTryResult, String> {
    execute_candidate_try(&state, &candidate_id, &run_id, queue_index, queue_size)
}

#[tauri::command]
pub async fn try_a30_oem_candidate(
    state: State<'_, AppState>,
    candidate_id: String,
    run_id: String,
    queue_index: usize,
    queue_size: usize,
) -> Result<OemCandidateTryResult, String> {
    execute_candidate_try(&state, &candidate_id, &run_id, queue_index, queue_size)
}

#[tauri::command]
pub async fn try_elm_oem_candidate(
    state: State<'_, AppState>,
    candidate_id: String,
    run_id: String,
    queue_index: usize,
    queue_size: usize,
) -> Result<OemCandidateTryResult, String> {
    execute_candidate_try(&state, &candidate_id, &run_id, queue_index, queue_size)
}

#[tauri::command]
pub async fn try_a30_read_test(
    state: State<'_, AppState>,
    test: A30ReadTest,
    run_id: String,
    queue_index: usize,
    queue_size: usize,
) -> Result<A30ReadTestResult, String> {
    execute_a30_read_test(&state, test, &run_id, queue_index, queue_size)
}

#[tauri::command]
pub async fn try_obd_read_test(
    app: tauri::AppHandle,
    test: A30ReadTest,
    run_id: String,
    queue_index: usize,
    queue_size: usize,
) -> Result<A30ReadTestResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        execute_obd_read_test(&state, test, &run_id, queue_index, queue_size)
    })
    .await
    .map_err(|error| format!("OBD read-test worker failed: {error}"))?
}

fn validate_run_item(run_id: &str, queue_index: usize, queue_size: usize) -> Result<(), String> {
    if run_id.trim().is_empty() || run_id.len() > 256 {
        return Err("Choose a valid research run ID".into());
    }
    if queue_index == 0 || queue_index > queue_size {
        return Err("Research queue index must be between 1 and its queue size".into());
    }
    Ok(())
}

pub(crate) fn execute_candidate_try(
    state: &AppState,
    candidate_id: &str,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
) -> Result<OemCandidateTryResult, String> {
    execute_candidate_try_inner(
        state,
        candidate_id,
        run_id,
        queue_index,
        queue_size,
        None,
        true,
    )
}

pub(crate) fn execute_candidate_try_bound(
    state: &AppState,
    candidate_id: &str,
    expected_source_blob_sha: &str,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
    expected_adapter_id: &str,
    expected_generation: u64,
) -> Result<OemCandidateTryResult, String> {
    execute_candidate_try_inner(
        state,
        candidate_id,
        run_id,
        queue_index,
        queue_size,
        Some((
            expected_adapter_id,
            expected_generation,
            expected_source_blob_sha,
        )),
        false,
    )
}

fn execute_candidate_try_inner(
    state: &AppState,
    candidate_id: &str,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
    expected_connection: Option<(&str, u64, &str)>,
    publish_sample: bool,
) -> Result<OemCandidateTryResult, String> {
    diagnose(
        state,
        None,
        "research",
        vec![candidate_id.to_owned()],
        |op| {
            validate_run_item(run_id, queue_index, queue_size)?;
            let engine = engine(state)?;
            if let Some((expected_adapter_id, expected_generation, _)) = expected_connection {
                let capabilities = engine
                    .connection_capabilities()
                    .ok_or_else(|| "No OBD adapter is connected".to_owned())?;
                if engine.obd_connection_generation() != expected_generation {
                    return Err("OBD connection changed before the OEM read".into());
                }
                if capabilities.device.stable_id != expected_adapter_id {
                    return Err("The connected adapter changed before the OEM read".into());
                }
            }
            let spec = OemCatalog::new(&state.pid_catalog_path).candidate_for_try(candidate_id)?;
            if expected_connection.is_some_and(|(_, _, sha)| spec.candidate.source_blob_sha != sha)
            {
                return Err("OEM candidate source revision changed before the request".into());
            }
            let result = engine
                .execute_research_test(ResearchTestRequest::OemCandidate(spec.clone()))
                .map_err(|error| error.to_string())?;
            let ResearchTestResult::OemCandidate(mut result) = result else {
                return Err("Active adapter returned the wrong research result type".into());
            };
            match append_oem_candidate_try_evidence(
                &state.oem_candidate_try_log,
                &spec.candidate,
                &result,
                run_id,
                queue_index,
                queue_size,
            ) {
                Ok(()) => result.evidence_saved = true,
                Err(error) => result.evidence_save_error = Some(error),
            }
            if publish_sample {
                publish_graphable_candidate_sample(&engine, &spec, &result)?;
            }
            op.research_outcome(
                serde_json::to_value(&result.outcome)
                    .unwrap()
                    .as_str()
                    .unwrap(),
                result.negative_response_code,
                result.evidence_saved,
                result.evidence_save_error.as_deref(),
            );
            Ok(result)
        },
    )
}

fn execute_a30_read_test(
    state: &AppState,
    test: A30ReadTest,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
) -> Result<A30ReadTestResult, String> {
    execute_obd_read_test(state, test, run_id, queue_index, queue_size)
}

pub(crate) fn execute_obd_read_test(
    state: &AppState,
    test: A30ReadTest,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
) -> Result<A30ReadTestResult, String> {
    diagnose(
        state,
        None,
        "research",
        vec![
            serde_json::to_value(test)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        ],
        |op| {
            validate_run_item(run_id, queue_index, queue_size)?;
            let (expected_adapter, response) = {
                let engine = engine(state)?;
                if !engine.is_obd_connected() {
                    return Err(
                        "Connect an OBDLink SX or Xtool A30 before running a one-shot OBD read test"
                            .into(),
                    );
                }
                let capabilities = engine.connection_capabilities().ok_or_else(|| {
                    "The OBD adapter disconnected before the read test".to_owned()
                })?;
                let family = capabilities.device.hardware_family.as_str();
                if family != SX_READ_TEST_FAMILY && family != A30_READ_TEST_FAMILY {
                    return Err(format!(
                        "One-shot OBD read tests are not supported by {family}"
                    ));
                }
                let expected_adapter = capabilities.device.clone();
                let response = engine
                    .queue_research_test(ResearchTestRequest::A30ReadTest(test))
                    .map_err(|error| error.to_string())?;
                (expected_adapter, response)
            };
            let result = response
                .recv()
                .map_err(|_| "Acquisition worker did not complete the OBD read test".to_owned())?
                .map_err(|error| error.to_string())?;
            let ResearchTestResult::A30ReadTest(mut result) = result else {
                return Err("Active adapter returned the wrong research result type".into());
            };
            if result.adapter.stable_id != expected_adapter.stable_id
                || result.adapter.hardware_family != expected_adapter.hardware_family
            {
                return Err(
                    "Active adapter returned a read-test result for a different adapter".into(),
                );
            }
            let evidence_path = read_test_evidence_path(
                &state.a30_read_test_log,
                &expected_adapter.hardware_family,
            )?;
            match append_read_test_evidence(
                &evidence_path,
                &result,
                run_id,
                queue_index,
                queue_size,
            ) {
                Ok(()) => result.evidence_saved = true,
                Err(error) => result.evidence_save_error = Some(error),
            }
            let engine = engine(state)?;
            if engine.connection_capabilities().is_some_and(|active| {
                active.device.stable_id == expected_adapter.stable_id
                    && active.device.hardware_family == expected_adapter.hardware_family
            }) {
                for value in &result.graph_values {
                    publish_read_test_graph_value(&engine, value, &result)?;
                }
            }
            op.research_outcome(
                serde_json::to_value(&result.outcome)
                    .unwrap()
                    .as_str()
                    .unwrap(),
                result.negative_response_code,
                result.evidence_saved,
                result.evidence_save_error.as_deref(),
            );
            Ok(result)
        },
    )
}

fn publish_graphable_candidate_sample(
    engine: &lognook_core::LogNookEngine,
    spec: &OemCandidateTrySpec,
    result: &OemCandidateTryResult,
) -> Result<(), String> {
    if result.outcome != OemCandidateOutcome::Positive {
        return Ok(());
    }
    let Some(decoded) = result.decoded_value.as_ref() else {
        return Ok(());
    };
    let Some(value) = decoded.value.filter(|value| value.is_finite()) else {
        return Ok(());
    };
    let sample = Sample {
        elapsed_ns: 0,
        wall_time_unix_ns: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(i64::MAX as u128) as i64,
        source_id: format!(
            "oem:{}:{}",
            result.adapter.stable_id, spec.candidate.candidate_id
        ),
        ecu_id: result.responder_header.clone(),
        signal_id: format!("oem:{}", spec.candidate.candidate_id),
        value,
        unit: spec.candidate.unit.clone(),
        quality: if decoded.within_catalog_range == Some(false) {
            SampleQuality::Suspect
        } else {
            SampleQuality::Valid
        },
        origin: SampleOrigin::Live,
        raw_bytes: Some(result.response_payload.clone()),
    };
    publish_research_sample(engine, sample)
}

fn publish_read_test_graph_value(
    engine: &lognook_core::LogNookEngine,
    value: &A30GraphValue,
    result: &A30ReadTestResult,
) -> Result<(), String> {
    if result.outcome != A30ReadOutcome::Positive || !value.value.is_finite() {
        return Ok(());
    }
    let positive_service = value.service.checked_add(0x40);
    let raw_bytes = value.response_payload.clone().or_else(|| {
        result
            .response_payloads
            .iter()
            .find(|payload| {
                positive_service.is_some_and(|service| payload.first() == Some(&service))
            })
            .cloned()
    });
    publish_research_sample(
        engine,
        Sample {
            elapsed_ns: 0,
            wall_time_unix_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .min(i64::MAX as u128) as i64,
            source_id: if result.adapter.hardware_family == A30_READ_TEST_FAMILY {
                format!("a30-research:{}", result.adapter.stable_id)
            } else {
                format!("obd-research:{}", result.adapter.stable_id)
            },
            ecu_id: result.responder_header.clone(),
            signal_id: value.signal_id.clone(),
            value: value.value,
            unit: value.unit.clone(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes,
        },
    )
}

fn publish_research_sample(
    engine: &lognook_core::LogNookEngine,
    sample: Sample,
) -> Result<(), String> {
    engine
        .publish_research_sample(sample)
        .map_err(|error| error.to_string())
}

fn read_test_evidence_path(
    a30_path: &std::path::Path,
    family: &str,
) -> Result<std::path::PathBuf, String> {
    match family {
        A30_READ_TEST_FAMILY => Ok(a30_path.to_path_buf()),
        SX_READ_TEST_FAMILY => Ok(a30_path.with_file_name("obd-read-tests.jsonl")),
        _ => Err(format!(
            "One-shot OBD read tests are not supported by {family}"
        )),
    }
}

fn append_read_test_evidence(
    path: &std::path::Path,
    result: &A30ReadTestResult,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
) -> Result<(), String> {
    let mut evidence = result.clone();
    evidence.evidence_saved = true;
    evidence.evidence_save_error = None;
    let envelope = serde_json::json!({
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
        "run_id": run_id,
        "queue_index": queue_index,
        "queue_size": queue_size,
        "probe": evidence,
    });
    let mut line = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
    line.push(b'\n');
    static APPEND_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = APPEND_LOCK
        .lock()
        .map_err(|_| "OBD read-test evidence lock is unavailable".to_owned())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Could not open OBD read-test evidence log: {error}"))?;
    file.write_all(&line).map_err(|error| error.to_string())
}

fn append_oem_candidate_try_evidence(
    path: &std::path::Path,
    candidate: &OemCandidate,
    result: &OemCandidateTryResult,
    run_id: &str,
    queue_index: usize,
    queue_size: usize,
) -> Result<(), String> {
    let mut evidence = result.clone();
    evidence.evidence_saved = true;
    evidence.evidence_save_error = None;
    let envelope = serde_json::json!({
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
        "run_id": run_id,
        "queue_index": queue_index,
        "queue_size": queue_size,
        "candidate": candidate,
        "probe": evidence,
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Could not open OEM candidate evidence log: {error}"))?;
    serde_json::to_writer(&mut file, &envelope).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        append_oem_candidate_try_evidence, append_read_test_evidence, execute_a30_read_test,
        execute_candidate_try, execute_candidate_try_bound, execute_obd_read_test,
    };
    use lognook_core::{
        A30ReadOutcome, A30ReadTest, A30ReadTestResult, DeviceDescriptor, OemCandidate,
        OemCandidateOutcome, OemCandidateTryResult,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn catalog_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite")
    }

    fn test_directory(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("lognook-{label}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn adapter() -> DeviceDescriptor {
        adapter_for("Xtool A30/A30M Bluetooth SPP", "active-adapter")
    }

    fn adapter_for(hardware_family: &str, stable_id: &str) -> DeviceDescriptor {
        DeviceDescriptor {
            stable_id: stable_id.into(),
            hardware_family: hardware_family.into(),
            firmware: None,
            path: None,
        }
    }

    fn candidate_result(candidate_id: &str) -> OemCandidateTryResult {
        OemCandidateTryResult {
            candidate_id: candidate_id.into(),
            exact_adapter_command: "STPX H:7E0, D:2101, T:250".into(),
            adapter: adapter(),
            protocol: "CAN".into(),
            started_at_unix_ms: 1,
            latency_ms: 2,
            outcome: OemCandidateOutcome::NoResponse,
            raw_response: "NO DATA".into(),
            responder_header: None,
            response_payload: Vec::new(),
            negative_response_code: None,
            decoded_value: None,
            decode_error: None,
            error: None,
            evidence_saved: false,
            evidence_save_error: None,
        }
    }

    fn a30_result(test: A30ReadTest) -> A30ReadTestResult {
        A30ReadTestResult {
            test,
            request_description: "captured read".into(),
            exact_adapter_command: "A30 RFCOMM TX".into(),
            adapter: adapter(),
            protocol: "A30".into(),
            started_at_unix_ms: 3,
            latency_ms: 4,
            outcome: A30ReadOutcome::NegativeResponse,
            responder_header: Some("7E8".into()),
            response_payloads: vec![vec![0x7F, 0x06, 0x12]],
            negative_response_code: Some(0x12),
            graph_values: Vec::new(),
            detail: None,
            raw_response: "7F 06 12".into(),
            evidence_saved: false,
            evidence_save_error: None,
        }
    }

    struct ActiveAdapter {
        device: DeviceDescriptor,
        wrong_result_type: bool,
        requests: std::sync::Arc<std::sync::Mutex<Vec<lognook_core::OemCandidateTrySpec>>>,
        reads: std::sync::Arc<std::sync::Mutex<Vec<A30ReadTest>>>,
    }

    impl lognook_core::HardwareAdapter for ActiveAdapter {
        fn connect(
            &mut self,
        ) -> Result<lognook_core::ConnectionCapabilities, lognook_core::LogNookError> {
            Ok(lognook_core::ConnectionCapabilities {
                device: self.device.clone(),
                protocol: "CAN".into(),
                read_only: true,
                diagnostics: Default::default(),
                supported_signals: lognook_core::standard_obd_signals(),
            })
        }
        fn read_samples(
            &mut self,
            _: &[lognook_core::SignalDefinition],
        ) -> Result<Vec<lognook_core::Sample>, lognook_core::LogNookError> {
            Ok(Vec::new())
        }
        fn execute_research_test(
            &mut self,
            request: lognook_core::ResearchTestRequest,
        ) -> Result<lognook_core::ResearchTestResult, lognook_core::LogNookError> {
            Ok(match request {
                lognook_core::ResearchTestRequest::OemCandidate(spec) => {
                    let result = candidate_result(&spec.candidate.candidate_id);
                    self.requests.lock().unwrap().push(spec);
                    lognook_core::ResearchTestResult::OemCandidate(result)
                }
                lognook_core::ResearchTestRequest::A30ReadTest(test) => {
                    self.reads.lock().unwrap().push(test);
                    if self.wrong_result_type {
                        lognook_core::ResearchTestResult::OemCandidate(candidate_result("wrong"))
                    } else {
                        let mut result = a30_result(test);
                        result.adapter = self.device.clone();
                        if self.device.hardware_family == "OBDLink SX" {
                            result.exact_adapter_command = "010C".into();
                            result.protocol = "ISO 15765-4 (CAN 11/500)".into();
                            result.raw_response = "7E8 04 41 0C 1F 40".into();
                            result.outcome = A30ReadOutcome::Positive;
                            result.responder_header = Some("7E8".into());
                            result.response_payloads = vec![vec![0x41, 0x0C, 0x1F, 0x40]];
                            result.graph_values = vec![lognook_core::A30GraphValue {
                                signal_id: "engine_rpm".into(),
                                name: "Engine RPM".into(),
                                service: 1,
                                pid: Some(0x0C),
                                value: 1984.0,
                                unit: "rpm".into(),
                                raw_value: "1F40".into(),
                                response_payload: Some(vec![0x41, 0x0C, 0x1F, 0x40]),
                            }];
                        }
                        lognook_core::ResearchTestResult::A30ReadTest(result)
                    }
                }
                lognook_core::ResearchTestRequest::Diagnostic(_) => {
                    return Err(lognook_core::LogNookError::new(
                        "This fixture does not implement vehicle diagnostics",
                    ));
                }
            })
        }
    }

    fn app_state(directory: &std::path::Path, catalog: std::path::PathBuf) -> crate::AppState {
        crate::AppState {
            engine: std::sync::Mutex::new(lognook_core::LogNookEngine::new(|_| {})),
            can_obd_operations: crate::commands::can_obd::CanObdOperationRegistry::default(),
            diagnostics: crate::diagnostics::DiagnosticsService::start(
                directory.join("diagnostics/operations.jsonl"),
                |_| {},
            )
            .unwrap(),
            diagnostics_path: directory.join("diagnostics/operations.jsonl"),
            recordings_dir: directory.into(),
            pid_catalog_path: catalog,
            oem_validation_log: directory.join("probe.jsonl"),
            oem_candidate_try_log: directory.join("tries.jsonl"),
            a30_read_test_log: directory.join("reads.jsonl"),
            agent_access: std::sync::Mutex::new(
                crate::agent::access::AgentAccessManager::open(directory.join("pin.hash")).unwrap(),
            ),
            agent_ipc: crate::agent::ipc::AgentIpcService::new(directory),
            agent_ipc_endpoint: crate::agent::ipc::endpoint_name(directory),
            agent_scan_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            agent_selection: std::sync::Mutex::new(crate::agent::protocol::AgentSignalSelection {
                log_signal_ids: vec![
                    "engine_rpm".into(),
                    "coolant_temp".into(),
                    "intake_temp".into(),
                ],
                graph_signal_ids: vec![
                    "engine_rpm".into(),
                    "coolant_temp".into(),
                    "intake_temp".into(),
                ],
            }),
            agent_workbench: std::sync::Mutex::new(
                crate::agent::tools::AgentWorkbenchState::default(),
            ),
            agent_log_buffer: std::sync::Arc::new(std::sync::Mutex::new(
                crate::agent::log_data::AgentLogBuffer::default(),
            )),
            trace_store: lognook_core::communication_trace::CommunicationTraceStore::new(),
            can_replay_previews: crate::can_replay_previews::CanReplayPreviewRegistry::default(),
            agent_can_replay_status: std::sync::Arc::new(std::sync::Mutex::new(None)),
            vehicle_profiles: crate::vehicle_profiles::VehicleProfileService::open(
                &directory.join("profiles.sqlite"),
            ),
        }
    }

    #[test]
    fn candidate_is_reloaded_before_active_session_request() {
        let directory = test_directory("reload-candidate");
        let copied_catalog = directory.join("catalog.sqlite");
        std::fs::copy(catalog_path(), &copied_catalog).unwrap();
        let catalog = lognook_core::OemCatalog::new(&copied_catalog);
        let candidate = catalog
            .search("", "Scion", "xB", Some("21"), 0, 100)
            .unwrap()
            .candidates
            .into_iter()
            .find(|candidate| catalog.candidate_for_try(&candidate.candidate_id).is_ok())
            .unwrap();
        let original = catalog.candidate_for_try(&candidate.candidate_id).unwrap();
        let state = app_state(&directory, copied_catalog.clone());
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter(),
                    wrong_result_type: false,
                    requests: requests.clone(),
                    reads: Default::default(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        state
            .engine
            .lock()
            .unwrap()
            .start_recording(&directory.join("active.lognook"))
            .unwrap();

        for (run_id, index, size) in [
            ("", 1, 1),
            ("run-1", 0, 1),
            ("run-1", 2, 1),
            ("run-1", 1, 0),
        ] {
            assert!(
                execute_candidate_try(&state, &candidate.candidate_id, run_id, index, size)
                    .is_err()
            );
            assert!(requests.lock().unwrap().is_empty());
        }
        assert!(execute_candidate_try(&state, "missing-candidate", "run-1", 1, 1).is_err());
        assert!(requests.lock().unwrap().is_empty());
        let connection = rusqlite::Connection::open(&copied_catalog).unwrap();
        for sql in [
            "UPDATE oem_candidates SET source_blob_sha='stale' WHERE candidate_id=?1",
            "UPDATE oem_candidates SET service='2E' WHERE candidate_id=?1",
            "UPDATE oem_candidates SET request_header='' WHERE candidate_id=?1",
            "UPDATE oem_candidates SET identifier='000001' WHERE candidate_id=?1",
        ] {
            connection.execute(sql, [&candidate.candidate_id]).unwrap();
            assert!(execute_candidate_try(&state, &candidate.candidate_id, "run-1", 1, 1).is_err());
            assert!(requests.lock().unwrap().is_empty());
            connection.execute("UPDATE oem_candidates SET source_blob_sha=?1,service=?2,request_header=?3,identifier=?4 WHERE candidate_id=?5",
                rusqlite::params![candidate.source_blob_sha, candidate.service, candidate.request_header, candidate.identifier, candidate.candidate_id]).unwrap();
        }
        connection
            .execute("CREATE TEMP TABLE removed_candidate AS SELECT * FROM oem_candidates WHERE candidate_id=?1", [&candidate.candidate_id])
            .unwrap();
        connection
            .execute(
                "DELETE FROM oem_candidates WHERE candidate_id=?1",
                [&candidate.candidate_id],
            )
            .unwrap();
        assert!(execute_candidate_try(&state, &candidate.candidate_id, "run-1", 1, 1).is_err());
        assert!(requests.lock().unwrap().is_empty());
        connection
            .execute(
                "INSERT INTO oem_candidates SELECT * FROM removed_candidate",
                [],
            )
            .unwrap();
        // A selection held by the UI is only an ID; a valid backend change must be reloaded.
        connection
            .execute(
                "UPDATE oem_candidates SET notes='fresh backend definition' WHERE candidate_id=?1",
                [&candidate.candidate_id],
            )
            .unwrap();
        let result = execute_candidate_try(&state, &candidate.candidate_id, "run-1", 1, 1).unwrap();
        assert_eq!(result.adapter.stable_id, "active-adapter");
        assert_eq!(result.outcome, OemCandidateOutcome::NoResponse);
        assert!(result.evidence_saved);
        let sent = requests.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].candidate.notes, "fresh backend definition");
        assert_eq!(
            sent[0].candidate.source_blob_sha,
            original.candidate.source_blob_sha
        );
        drop(sent);
        state.engine.lock().unwrap().stop_recording().unwrap();
        state.engine.lock().unwrap().disconnect().unwrap();
        drop(connection);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bound_candidate_try_rejects_stale_adapter_and_generation_before_sending() {
        let directory = test_directory("bound-candidate-try");
        let copied_catalog = directory.join("catalog.sqlite");
        std::fs::copy(catalog_path(), &copied_catalog).unwrap();
        let catalog = lognook_core::OemCatalog::new(&copied_catalog);
        let candidate = catalog
            .search("", "Scion", "xB", Some("21"), 0, 100)
            .unwrap()
            .candidates
            .into_iter()
            .find(|candidate| catalog.candidate_for_try(&candidate.candidate_id).is_ok())
            .unwrap();
        let state = app_state(&directory, copied_catalog);
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter(),
                    wrong_result_type: false,
                    requests: requests.clone(),
                    reads: Default::default(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        let generation = state.engine.lock().unwrap().obd_connection_generation();

        assert!(
            super::set_oem_logging(&state, None, &[], Some(("different-adapter", generation)))
                .is_err()
        );
        assert!(
            super::set_oem_logging(&state, None, &[], Some(("active-adapter", generation + 1)))
                .is_err()
        );

        assert!(
            execute_candidate_try_bound(
                &state,
                &candidate.candidate_id,
                "stale-source-revision",
                "discovery-1",
                1,
                1,
                "active-adapter",
                generation,
            )
            .is_err()
        );
        assert!(
            execute_candidate_try_bound(
                &state,
                &candidate.candidate_id,
                &candidate.source_blob_sha,
                "discovery-1",
                1,
                1,
                "different-adapter",
                generation,
            )
            .is_err()
        );
        assert!(
            execute_candidate_try_bound(
                &state,
                &candidate.candidate_id,
                &candidate.source_blob_sha,
                "discovery-1",
                1,
                1,
                "active-adapter",
                generation + 1,
            )
            .is_err()
        );
        assert!(requests.lock().unwrap().is_empty());

        let result = execute_candidate_try_bound(
            &state,
            &candidate.candidate_id,
            &candidate.source_blob_sha,
            "discovery-1",
            1,
            1,
            "active-adapter",
            generation,
        )
        .unwrap();
        assert_eq!(result.adapter.stable_id, "active-adapter");
        assert_eq!(requests.lock().unwrap().len(), 1);
        state.engine.lock().unwrap().disconnect().unwrap();
        drop(state);
        drop(catalog);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn research_evidence_records_run_order() {
        let directory = test_directory("run-evidence");
        let state = app_state(&directory, catalog_path());
        let candidate = lognook_core::OemCatalog::new(catalog_path())
            .search("", "Scion", "xB", Some("21"), 0, 1)
            .unwrap()
            .candidates
            .remove(0);
        let reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter(),
                    wrong_result_type: false,
                    requests: Default::default(),
                    reads: reads.clone(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        let mut negative = candidate_result(&candidate.candidate_id);
        negative.outcome = OemCandidateOutcome::NegativeResponse;
        negative.raw_response = "7F 21 12".into();
        negative.response_payload = vec![0x7F, 0x21, 0x12];
        negative.negative_response_code = Some(0x12);
        append_oem_candidate_try_evidence(
            &state.oem_candidate_try_log,
            &candidate,
            &negative,
            "run-order",
            1,
            3,
        )
        .unwrap();
        append_oem_candidate_try_evidence(
            &state.oem_candidate_try_log,
            &candidate,
            &candidate_result(&candidate.candidate_id),
            "run-order",
            2,
            3,
        )
        .unwrap();
        let result =
            execute_a30_read_test(&state, A30ReadTest::Mode06TidA2, "run-order", 3, 3).unwrap();
        assert_eq!(result.outcome, A30ReadOutcome::NegativeResponse);
        assert!(result.evidence_saved);
        assert_eq!(*reads.lock().unwrap(), [A30ReadTest::Mode06TidA2]);
        let candidate_text = std::fs::read_to_string(&state.oem_candidate_try_log).unwrap();
        let a30_text = std::fs::read_to_string(&state.a30_read_test_log).unwrap();
        let records = candidate_text
            .lines()
            .chain(a30_text.lines())
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        for (position, evidence) in records.iter().enumerate() {
            assert_eq!(evidence["run_id"], "run-order");
            assert_eq!(evidence["queue_index"], position + 1);
            assert_eq!(evidence["queue_size"], 3);
            assert_eq!(evidence["probe"]["evidence_saved"], true);
        }
        assert_eq!(records[0]["probe"]["outcome"], "negative_response");
        assert_eq!(records[0]["probe"]["negative_response_code"], 0x12);
        assert_eq!(records[1]["probe"]["outcome"], "no_response");
        assert_eq!(
            records[2]["probe"]["response_payloads"],
            serde_json::json!([[0x7F, 0x06, 0x12]])
        );
        state.engine.lock().unwrap().disconnect().unwrap();
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn sx_read_tests_use_neutral_evidence_and_publish_ordered_samples() {
        let directory = test_directory("sx-read-test");
        let state = app_state(&directory, catalog_path());
        let reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter_for("OBDLink SX", "sx-serial"),
                    wrong_result_type: false,
                    requests: Default::default(),
                    reads: reads.clone(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        let recording_path = directory.join("sx.lognook");
        state
            .engine
            .lock()
            .unwrap()
            .start_recording(&recording_path)
            .unwrap();

        for (test, queue_index) in [
            (A30ReadTest::Mode01EngineRpm, 1),
            (A30ReadTest::Mode02FreezeFrameRpm, 2),
        ] {
            let result = execute_obd_read_test(&state, test, "sx-run", queue_index, 2).unwrap();
            assert_eq!(result.adapter.hardware_family, "OBDLink SX");
            assert!(result.evidence_saved);
        }

        assert_eq!(
            *reads.lock().unwrap(),
            [
                A30ReadTest::Mode01EngineRpm,
                A30ReadTest::Mode02FreezeFrameRpm
            ]
        );
        assert!(!state.a30_read_test_log.exists());
        let sx_log = state
            .a30_read_test_log
            .with_file_name("obd-read-tests.jsonl");
        let records = std::fs::read_to_string(sx_log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        for (index, evidence) in records.iter().enumerate() {
            assert_eq!(evidence["run_id"], "sx-run");
            assert_eq!(evidence["queue_index"], index + 1);
            assert_eq!(evidence["queue_size"], 2);
            assert_eq!(evidence["probe"]["adapter"]["stable_id"], "sx-serial");
            assert_eq!(
                evidence["probe"]["adapter"]["hardware_family"],
                "OBDLink SX"
            );
            assert_eq!(evidence["probe"]["protocol"], "ISO 15765-4 (CAN 11/500)");
            assert_eq!(evidence["probe"]["exact_adapter_command"], "010C");
            assert_eq!(
                evidence["probe"]["response_payloads"],
                serde_json::json!([[0x41, 0x0C, 0x1F, 0x40]])
            );
        }

        state.engine.lock().unwrap().stop_recording().unwrap();
        state.engine.lock().unwrap().disconnect().unwrap();
        let recording = lognook_core::read_recording(&recording_path).unwrap();
        assert_eq!(recording.samples.len(), 2);
        assert_eq!(recording.samples[0].source_id, "obd-research:sx-serial");
        assert_eq!(recording.samples[0].ecu_id.as_deref(), Some("7E8"));
        assert_eq!(recording.samples[0].signal_id, "engine_rpm");
        assert_eq!(recording.samples[0].value, 1984.0);
        assert_eq!(
            recording.samples[0].raw_bytes.as_deref(),
            Some(&[0x41, 0x0C, 0x1F, 0x40][..])
        );
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn read_test_rejects_disconnected_unsupported_and_wrong_result_types() {
        let directory = test_directory("read-test-gates");
        let state = app_state(&directory, catalog_path());
        let reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        assert!(
            execute_obd_read_test(&state, A30ReadTest::Mode06TidA2, "run", 1, 1)
                .unwrap_err()
                .contains("Connect an OBDLink SX or Xtool A30")
        );

        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter_for("ELM327 Bluetooth SPP", "elm-serial"),
                    wrong_result_type: false,
                    requests: Default::default(),
                    reads: reads.clone(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        assert!(
            execute_obd_read_test(&state, A30ReadTest::Mode06TidA2, "run", 1, 1)
                .unwrap_err()
                .contains("not supported by ELM327")
        );
        assert!(reads.lock().unwrap().is_empty());
        state.engine.lock().unwrap().disconnect().unwrap();

        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter(),
                    wrong_result_type: true,
                    requests: Default::default(),
                    reads: reads.clone(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        assert!(
            execute_obd_read_test(&state, A30ReadTest::Mode06TidA2, "run", 1, 1)
                .unwrap_err()
                .contains("wrong research result type")
        );
        assert_eq!(*reads.lock().unwrap(), [A30ReadTest::Mode06TidA2]);
        state.engine.lock().unwrap().disconnect().unwrap();
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn candidate_try_evidence_appends_source_request_and_raw_result() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("lognook-oem-try-{id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("tries.jsonl");
        let candidate = OemCandidate {
            candidate_id: "candidate-1".into(),
            source_id: "source-1".into(),
            family: "Toyota".into(),
            brand: "Scion".into(),
            model_scope: "xB 2014".into(),
            service: "21".into(),
            identifier: "01".into(),
            request_header: "7E0".into(),
            response_header: "7E8".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "Engine".into(),
            name: "RPM".into(),
            signal_source_id: String::new(),
            bit_offset: Some(0),
            bit_length: Some(8),
            formula: String::new(),
            decoder_json: "{}".into(),
            unit: "rpm".into(),
            year_filter_json: "{}".into(),
            ecu_filter: String::new(),
            source_blob_sha: "sha".into(),
            research_state: "community_reference".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "research".into(),
            notes: String::new(),
            suggested_metric: String::new(),
            source_scope: String::new(),
            applicability_specificity: 60,
            min_value: None,
            max_value: None,
            source_url: Some("https://source.test".into()),
            license: Some("CC BY-SA 4.0".into()),
            license_url: Some("https://source.test/license".into()),
        };
        let result = OemCandidateTryResult {
            candidate_id: "candidate-1".into(),
            exact_adapter_command: "STPX H:7E0, D:2101, T:250".into(),
            adapter: DeviceDescriptor {
                stable_id: "sx-serial".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: None,
            },
            protocol: "ISO 15765-4 (CAN 11/500)".into(),
            started_at_unix_ms: 1,
            latency_ms: 2,
            outcome: OemCandidateOutcome::Positive,
            raw_response: "7E8 03 61 01 64".into(),
            responder_header: Some("7E8".into()),
            response_payload: vec![0x61, 0x01, 0x64],
            negative_response_code: None,
            decoded_value: None,
            decode_error: None,
            error: None,
            evidence_saved: false,
            evidence_save_error: None,
        };

        append_oem_candidate_try_evidence(&path, &candidate, &result, "one-shot", 1, 1).unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        let evidence: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(evidence["candidate"]["source_blob_sha"], "sha");
        assert_eq!(evidence["probe"]["candidate_id"], "candidate-1");
        assert_eq!(
            evidence["probe"]["exact_adapter_command"],
            "STPX H:7E0, D:2101, T:250"
        );
        assert_eq!(
            evidence["probe"]["response_payload"],
            serde_json::json!([0x61, 0x01, 0x64])
        );
        assert_eq!(evidence["probe"]["evidence_saved"], true);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn research_graph_publication_records_only_successful_finite_measurements() {
        let directory = test_directory("research-graph");
        let state = app_state(&directory, catalog_path());
        let candidate = lognook_core::OemCatalog::new(catalog_path())
            .search("", "Scion", "xB", Some("21"), 0, 1)
            .unwrap()
            .candidates
            .remove(0);
        let spec = lognook_core::OemCatalog::new(catalog_path())
            .candidate_for_try(&candidate.candidate_id)
            .unwrap();
        state
            .engine
            .lock()
            .unwrap()
            .connect_adapter(
                Box::new(ActiveAdapter {
                    device: adapter(),
                    wrong_result_type: false,
                    requests: Default::default(),
                    reads: Default::default(),
                }),
                &["engine_rpm".into()],
            )
            .unwrap();
        let recording_path = directory.join("active.lognook");
        state
            .engine
            .lock()
            .unwrap()
            .start_recording(&recording_path)
            .unwrap();
        let mut result = candidate_result(&candidate.candidate_id);
        result.outcome = OemCandidateOutcome::Positive;
        result.response_payload = vec![0x61, 0x01, 0x64];
        result.decoded_value = Some(lognook_core::OemCandidateDecodedValue {
            raw_value: "64".into(),
            value: Some(123.0),
            enum_value: None,
            unit: spec.candidate.unit.clone(),
            formula: "A".into(),
            within_catalog_range: Some(false),
        });
        super::publish_graphable_candidate_sample(&state.engine.lock().unwrap(), &spec, &result)
            .unwrap();
        result.decoded_value.as_mut().unwrap().value = Some(f64::NAN);
        super::publish_graphable_candidate_sample(&state.engine.lock().unwrap(), &spec, &result)
            .unwrap();
        result.decoded_value.as_mut().unwrap().value = Some(999.0);
        result.outcome = OemCandidateOutcome::NoResponse;
        super::publish_graphable_candidate_sample(&state.engine.lock().unwrap(), &spec, &result)
            .unwrap();

        let mut read = a30_result(A30ReadTest::Mode06TidA2);
        let tid_a2_payload = vec![0x46, 0xA2, 0x0B, 0x24];
        let tid_a5_payload = vec![0x46, 0xA5, 0x0C, 0x34];
        read.response_payloads = vec![
            vec![0x7F, 0x06, 0x78],
            tid_a2_payload.clone(),
            tid_a5_payload.clone(),
        ];
        let legacy_graph = lognook_core::A30GraphValue {
            signal_id: "mode06_tid_a2".into(),
            name: "Raw count".into(),
            service: 6,
            pid: Some(0xA2),
            value: 456.0,
            unit: "raw count".into(),
            raw_value: "456".into(),
            response_payload: None,
        };
        let mut precise_graph = lognook_core::A30GraphValue {
            signal_id: "mode06_tid_a5".into(),
            name: "Second raw count".into(),
            service: 6,
            pid: Some(0xA5),
            value: 789.0,
            unit: "raw count".into(),
            raw_value: "789".into(),
            response_payload: Some(tid_a5_payload.clone()),
        };
        super::publish_read_test_graph_value(&state.engine.lock().unwrap(), &legacy_graph, &read)
            .unwrap();
        read.outcome = A30ReadOutcome::Positive;
        super::publish_read_test_graph_value(&state.engine.lock().unwrap(), &legacy_graph, &read)
            .unwrap();
        super::publish_read_test_graph_value(&state.engine.lock().unwrap(), &precise_graph, &read)
            .unwrap();
        precise_graph.value = f64::INFINITY;
        super::publish_read_test_graph_value(&state.engine.lock().unwrap(), &precise_graph, &read)
            .unwrap();
        state.engine.lock().unwrap().stop_recording().unwrap();
        state.engine.lock().unwrap().disconnect().unwrap();
        let recording = lognook_core::read_recording(&recording_path).unwrap();
        assert_eq!(
            recording
                .samples
                .iter()
                .map(|sample| sample.value)
                .collect::<Vec<_>>(),
            [123.0, 456.0, 789.0]
        );
        assert_eq!(
            recording.samples[0].quality,
            lognook_core::SampleQuality::Suspect
        );
        assert_eq!(
            recording.samples[0].raw_bytes.as_deref(),
            Some(&[0x61, 0x01, 0x64][..])
        );
        assert_eq!(recording.samples[1].signal_id, "mode06_tid_a2");
        assert_eq!(recording.samples[1].ecu_id.as_deref(), Some("7E8"));
        assert_eq!(
            recording.samples[1].raw_bytes.as_deref(),
            Some(&tid_a2_payload[..])
        );
        assert_eq!(recording.samples[2].signal_id, "mode06_tid_a5");
        assert_eq!(recording.samples[2].ecu_id.as_deref(), Some("7E8"));
        assert_eq!(
            recording.samples[2].raw_bytes.as_deref(),
            Some(&tid_a5_payload[..])
        );
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a30_read_test_evidence_keeps_the_request_and_raw_frame() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("lognook-a30-read-{id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("reads.jsonl");
        let result = A30ReadTestResult {
            test: A30ReadTest::Mode06TidA2,
            request_description: "Mode 06 TID A2".into(),
            exact_adapter_command: "A30 RFCOMM TX 55 AA ...".into(),
            adapter: DeviceDescriptor {
                stable_id: "xtool-a30:AA:BB:CC:DD:EE:FF".into(),
                hardware_family: "Xtool A30/A30M Bluetooth SPP".into(),
                firmware: None,
                path: Some("AA:BB:CC:DD:EE:FF".into()),
            },
            protocol: "Xtool A30/A30M Bluetooth SPP".into(),
            started_at_unix_ms: 1,
            latency_ms: 2,
            outcome: A30ReadOutcome::Positive,
            responder_header: Some("7E8".into()),
            response_payloads: vec![vec![0x46, 0xA2, 0x0B, 0x24]],
            negative_response_code: None,
            graph_values: Vec::new(),
            detail: None,
            raw_response: "AA 55 00 01 ...".into(),
            evidence_saved: false,
            evidence_save_error: None,
        };

        append_read_test_evidence(&path, &result, "one-shot", 1, 1).unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        let evidence: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(evidence["probe"]["test"], "mode_06_tid_a2");
        assert_eq!(evidence["probe"]["raw_response"], "AA 55 00 01 ...");
        assert_eq!(evidence["probe"]["evidence_saved"], true);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
