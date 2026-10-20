use crate::AppState;
use lognook_core::{OemPollingStatus, SubaruCatalog, SubaruCatalogDetail, SubaruCatalogPage};
use tauri::State;

#[tauri::command]
pub async fn search_subaru_catalog(
    state: State<'_, AppState>,
    query: String,
    model: String,
    year: Option<i32>,
    ecu_id: String,
    protocol_family: String,
    offset: usize,
    limit: usize,
) -> Result<SubaruCatalogPage, String> {
    SubaruCatalog::new(&state.pid_catalog_path).search(
        &query,
        &model,
        year,
        &ecu_id,
        &protocol_family,
        offset,
        limit,
    )
}

#[tauri::command]
pub async fn get_subaru_catalog_detail(
    state: State<'_, AppState>,
    candidate_id: String,
) -> Result<SubaruCatalogDetail, String> {
    SubaruCatalog::new(&state.pid_catalog_path).get(&candidate_id)
}

#[tauri::command]
pub async fn set_subaru_logging(
    state: State<'_, AppState>,
    candidate_ids: Vec<String>,
    adapter_id: String,
    connection_generation: u64,
) -> Result<OemPollingStatus, String> {
    set_subaru_logging_bound(&state, &candidate_ids, &adapter_id, connection_generation)
}

pub(crate) fn set_subaru_logging_bound(
    state: &AppState,
    candidate_ids: &[String],
    adapter_id: &str,
    connection_generation: u64,
) -> Result<OemPollingStatus, String> {
    let plan = if candidate_ids.is_empty() {
        None
    } else {
        Some(SubaruCatalog::new(&state.pid_catalog_path).logging_plan(candidate_ids)?)
    };
    let engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
    let status = engine.oem_polling_status();
    if status.active && status.vehicle_id.as_deref() != Some("subaru-local-research") {
        return Err("Stop the other OEM logging plan before selecting Subaru graph signals".into());
    }
    if candidate_ids.is_empty() && !status.active {
        return Ok(status);
    }
    let capabilities = engine
        .connection_capabilities()
        .ok_or_else(|| "Connect an OBD adapter before logging Subaru signals".to_owned())?;
    if capabilities.device.stable_id != adapter_id
        || engine.obd_connection_generation() != connection_generation
    {
        return Err("The OBD adapter or connection changed before Subaru logging started".into());
    }
    engine
        .set_oem_poll_plan(plan)
        .map_err(|error| error.to_string())?;
    Ok(engine.oem_polling_status())
}
