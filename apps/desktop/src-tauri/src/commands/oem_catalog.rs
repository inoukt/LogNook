use super::engine;
use crate::AppState;
use lognook_core::{
    A30GraphValue, A30ReadTest, A30ReadTestResult, OemCandidate, OemCandidateOutcome,
    OemCandidateTryResult, OemCandidateTrySpec, OemCatalog, OemCatalogPage, Sample, SampleOrigin,
    SampleQuality, ScionMode21ProbeResult, probe_scion_xb_mode21 as run_probe,
    run_a30_read_test as run_a30_mode_test, try_a30_oem_candidate as run_a30_oem_candidate_try,
    try_elm_oem_candidate as run_elm_oem_candidate_try, try_oem_candidate as run_oem_candidate_try,
};
use std::{fs::OpenOptions, io::Write};
use tauri::State;

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
pub async fn probe_scion_xb_mode21(
    state: State<'_, AppState>,
    port_name: String,
) -> Result<ScionMode21ProbeResult, String> {
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
    Ok(result)
}

#[tauri::command]
pub async fn try_oem_candidate(
    state: State<'_, AppState>,
    port_name: String,
    candidate_id: String,
) -> Result<OemCandidateTryResult, String> {
    let engine = engine(&state)?;
    if engine.is_connected() || engine.is_recording() {
        return Err(
            "Disconnect acquisition and stop recording before trying an OEM candidate".into(),
        );
    }
    drop(engine);
    let candidate = OemCatalog::new(&state.pid_catalog_path).candidate_for_try(&candidate_id)?;
    let mut result =
        run_oem_candidate_try(port_name, &candidate).map_err(|error| error.to_string())?;
    match append_oem_candidate_try_evidence(
        &state.oem_candidate_try_log,
        &candidate.candidate,
        &result,
    ) {
        Ok(()) => result.evidence_saved = true,
        Err(error) => result.evidence_save_error = Some(error),
    }
    publish_graphable_candidate_sample(&state, &candidate, &result)?;
    Ok(result)
}

#[tauri::command]
pub async fn try_a30_oem_candidate(
    state: State<'_, AppState>,
    address: String,
    candidate_id: String,
) -> Result<OemCandidateTryResult, String> {
    let engine = engine(&state)?;
    if engine.is_connected() || engine.is_recording() {
        return Err(
            "Disconnect acquisition and stop recording before trying an OEM candidate".into(),
        );
    }
    drop(engine);
    let spec = OemCatalog::new(&state.pid_catalog_path).candidate_for_try(&candidate_id)?;
    let mut result =
        run_a30_oem_candidate_try(address, &spec).map_err(|error| error.to_string())?;
    match append_oem_candidate_try_evidence(&state.oem_candidate_try_log, &spec.candidate, &result)
    {
        Ok(()) => result.evidence_saved = true,
        Err(error) => result.evidence_save_error = Some(error),
    }
    publish_graphable_candidate_sample(&state, &spec, &result)?;
    Ok(result)
}

#[tauri::command]
pub async fn try_elm_oem_candidate(
    state: State<'_, AppState>,
    address: String,
    candidate_id: String,
) -> Result<OemCandidateTryResult, String> {
    let engine = engine(&state)?;
    if engine.is_connected() || engine.is_recording() {
        return Err(
            "Disconnect acquisition and stop recording before trying an OEM candidate".into(),
        );
    }
    drop(engine);
    let spec = OemCatalog::new(&state.pid_catalog_path).candidate_for_try(&candidate_id)?;
    let mut result =
        run_elm_oem_candidate_try(address, &spec).map_err(|error| error.to_string())?;
    match append_oem_candidate_try_evidence(&state.oem_candidate_try_log, &spec.candidate, &result)
    {
        Ok(()) => result.evidence_saved = true,
        Err(error) => result.evidence_save_error = Some(error),
    }
    publish_graphable_candidate_sample(&state, &spec, &result)?;
    Ok(result)
}

#[tauri::command]
pub async fn try_a30_read_test(
    state: State<'_, AppState>,
    address: String,
    test: A30ReadTest,
) -> Result<A30ReadTestResult, String> {
    let engine = engine(&state)?;
    if engine.is_connected() || engine.is_recording() {
        return Err("Disconnect acquisition and stop recording before the A30 read test".into());
    }
    drop(engine);
    let mut result = run_a30_mode_test(address, test).map_err(|error| error.to_string())?;
    match append_a30_read_test_evidence(&state.a30_read_test_log, &result) {
        Ok(()) => result.evidence_saved = true,
        Err(error) => result.evidence_save_error = Some(error),
    }
    for value in &result.graph_values {
        publish_a30_graph_value(&state, value, &result)?;
    }
    Ok(result)
}

fn publish_graphable_candidate_sample(
    state: &State<'_, AppState>,
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
    publish_research_sample(state, sample)
}

fn publish_a30_graph_value(
    state: &State<'_, AppState>,
    value: &A30GraphValue,
    result: &A30ReadTestResult,
) -> Result<(), String> {
    publish_research_sample(
        state,
        Sample {
            elapsed_ns: 0,
            wall_time_unix_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .min(i64::MAX as u128) as i64,
            source_id: format!("a30-research:{}", result.adapter.stable_id),
            ecu_id: result.responder_header.clone(),
            signal_id: value.signal_id.clone(),
            value: value.value,
            unit: value.unit.clone(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: result.response_payloads.first().cloned(),
        },
    )
}

fn publish_research_sample(state: &State<'_, AppState>, sample: Sample) -> Result<(), String> {
    engine(state)?
        .publish_research_sample(sample)
        .map_err(|error| error.to_string())
}

fn append_a30_read_test_evidence(
    path: &std::path::Path,
    result: &A30ReadTestResult,
) -> Result<(), String> {
    let mut evidence = result.clone();
    evidence.evidence_saved = true;
    evidence.evidence_save_error = None;
    let envelope = serde_json::json!({
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
        "probe": evidence,
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Could not open A30 read-test evidence log: {error}"))?;
    serde_json::to_writer(&mut file, &envelope).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

fn append_oem_candidate_try_evidence(
    path: &std::path::Path,
    candidate: &OemCandidate,
    result: &OemCandidateTryResult,
) -> Result<(), String> {
    let mut evidence = result.clone();
    evidence.evidence_saved = true;
    let envelope = serde_json::json!({
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
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
    use super::{append_a30_read_test_evidence, append_oem_candidate_try_evidence};
    use lognook_core::{
        A30ReadOutcome, A30ReadTest, A30ReadTestResult, DeviceDescriptor, OemCandidate,
        OemCandidateOutcome, OemCandidateTryResult,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

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

        append_oem_candidate_try_evidence(&path, &candidate, &result).unwrap();
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

        append_a30_read_test_evidence(&path, &result).unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        let evidence: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(evidence["probe"]["test"], "mode_06_tid_a2");
        assert_eq!(evidence["probe"]["raw_response"], "AA 55 00 01 ...");
        assert_eq!(evidence["probe"]["evidence_saved"], true);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
