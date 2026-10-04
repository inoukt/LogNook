use super::engine;
use crate::AppState;
use lognook_core::{OemCatalog, OemCatalogPage, ScionMode21ProbeResult, probe_scion_xb_mode21 as run_probe};
use std::{fs::OpenOptions, io::Write};
use tauri::State;

#[tauri::command]
pub fn search_oem_catalog(
    state: State<'_, AppState>,
    query: String,
    brand: String,
    model: String,
    service: Option<String>,
    offset: usize,
    limit: usize,
) -> Result<OemCatalogPage, String> {
    OemCatalog::new(&state.pid_catalog_path)
        .search(&query, &brand, &model, service.as_deref(), offset, limit)
}

#[tauri::command]
pub fn probe_scion_xb_mode21(
    state: State<'_, AppState>,
    port_name: String,
) -> Result<ScionMode21ProbeResult, String> {
    let engine = engine(&state)?;
    if engine.is_connected() || engine.is_recording() {
        return Err("Disconnect acquisition and stop recording before the one-shot OEM read".into());
    }
    let spec = OemCatalog::new(&state.pid_catalog_path).scion_xb_mode21_probe()?;
    let mut result = run_probe(port_name, &spec).map_err(|error| error.to_string())?;
    let save_attempt = (|| -> Result<(), String> {
        let mut file = OpenOptions::new().create(true).append(true).open(&state.oem_validation_log)
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
