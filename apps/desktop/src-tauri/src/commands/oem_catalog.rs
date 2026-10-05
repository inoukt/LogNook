use super::engine;
use crate::AppState;
use lognook_core::{
    OemCandidate, OemCandidateTryResult, OemCatalog, OemCatalogPage, ScionMode21ProbeResult,
    probe_scion_xb_mode21 as run_probe, try_oem_candidate as run_oem_candidate_try,
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
    Ok(result)
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
    use super::append_oem_candidate_try_evidence;
    use lognook_core::{
        DeviceDescriptor, OemCandidate, OemCandidateOutcome, OemCandidateTryResult,
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
}
