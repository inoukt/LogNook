//! Profile-aware connection and generic fallback use the same adapter/acquisition owner.
use crate::{
    AppState,
    agent::protocol::{AgentAdapterChoice, AgentSignalSelection},
};
use lognook_core::{
    A30Adapter, ConnectionCapabilities, DerivedSignalDefinition, Elm327Adapter, HardwareAdapter,
    ObdLinkSxAdapter, OpenPort2Adapter, SimulatedObdAdapter,
};
use tauri::{Emitter, State};

pub(crate) fn connect_profile_adapter_shared(
    state: &AppState,
    choice: AgentAdapterChoice,
) -> Result<ConnectionCapabilities, String> {
    let (identity, adapter): (String, Box<dyn HardwareAdapter>) = match choice {
        AgentAdapterChoice::Simulator => ("simulator".into(), Box::new(SimulatedObdAdapter::default())),
        AgentAdapterChoice::ObdLinkSx { port_name } => (
            format!("obdlink-sx:{port_name}"), Box::new(ObdLinkSxAdapter::new(port_name)),
        ),
        AgentAdapterChoice::Elm327 { address } => (
            format!("elm327:{address}"), Box::new(Elm327Adapter::new(address)),
        ),
        AgentAdapterChoice::A30 { address } => (
            format!("a30:{address}"), Box::new(A30Adapter::new(address)),
        ),
        AgentAdapterChoice::OpenPort2 { provider_id } => (
            format!("openport2:{provider_id}"),
            Box::new(OpenPort2Adapter::new(super::worker_path()?, provider_id)),
        ),
        AgentAdapterChoice::Candlelight { .. } => return Err("Vehicle recognition uses an OBD connection; choose a manual CAN profile for CAN-only work".into()),
    };
    super::connect_with_diagnostics(state, identity, vec![], move |engine, _| {
        engine
            .connect_for_profile(adapter)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn connect_profile_adapter(
    state: State<'_, AppState>,
    choice: AgentAdapterChoice,
) -> Result<ConnectionCapabilities, String> {
    connect_profile_adapter_shared(&state, choice)
}

pub(crate) fn continue_profile_selection_shared(
    state: &AppState,
    log_signal_ids: Vec<String>,
    graph_signal_ids: Vec<String>,
    derived_definitions: Vec<DerivedSignalDefinition>,
    expected_obd_generation: u64,
) -> Result<AgentSignalSelection, String> {
    if log_signal_ids.len() > 500 || graph_signal_ids.len() > 500 {
        return Err("Signal selection exceeds the 500-channel profile limit".into());
    }
    if graph_signal_ids
        .iter()
        .any(|id| !log_signal_ids.contains(id))
    {
        return Err("The generic fallback graphs only selected supported logging signals".into());
    }
    let active_derived_ids = derived_definitions
        .iter()
        .filter(|definition| log_signal_ids.contains(&definition.id))
        .map(|definition| definition.id.clone())
        .collect();
    let mut engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable")?;
    let mut agent_selection = state
        .agent_selection
        .lock()
        .map_err(|_| "Agent selection is unavailable")?;
    engine
        .replace_profile_selection(
            log_signal_ids.clone(),
            derived_definitions,
            active_derived_ids,
            expected_obd_generation,
        )
        .map_err(|error| error.to_string())?;
    let selected = AgentSignalSelection {
        log_signal_ids,
        graph_signal_ids,
    };
    *agent_selection = selected.clone();
    Ok(selected)
}

#[tauri::command]
pub async fn continue_profile_selection(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    log_signal_ids: Vec<String>,
    graph_signal_ids: Vec<String>,
    derived_definitions: Vec<DerivedSignalDefinition>,
    expected_obd_generation: u64,
) -> Result<AgentSignalSelection, String> {
    let selection = continue_profile_selection_shared(
        &state,
        log_signal_ids,
        graph_signal_ids,
        derived_definitions,
        expected_obd_generation,
    )?;
    let _ = app.emit("agent-signal-selection", &selection);
    Ok(selection)
}

#[tauri::command]
pub fn cancel_vehicle_profile_operations(state: State<'_, AppState>) {
    state.vehicle_profiles.cancel_pending_work();
}
