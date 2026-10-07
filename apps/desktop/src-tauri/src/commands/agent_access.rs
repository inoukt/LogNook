use crate::{
    AppState,
    agent::{access::AgentAccessStatus, protocol::AgentSignalSelection},
};
use tauri::{AppHandle, State};

#[tauri::command]
pub fn agent_access_status(state: State<'_, AppState>) -> Result<AgentAccessStatus, String> {
    let mut status = state
        .agent_access
        .lock()
        .map_err(|_| "Agent Access state is unavailable".to_owned())
        .map(|mut access| access.status())?;
    status.host_connected = state.agent_ipc.connected();
    status.active_operation = state
        .agent_workbench
        .lock()
        .map_err(|_| "Agent Workbench state is unavailable".to_owned())?
        .active_operation(
            state
                .agent_scan_active
                .load(std::sync::atomic::Ordering::Acquire),
        );
    status.trace_gaps = state.trace_store.gap_count();
    Ok(status)
}

#[tauri::command]
pub fn configure_agent_pin(
    app: AppHandle,
    state: State<'_, AppState>,
    pin: String,
) -> Result<AgentAccessStatus, String> {
    let mut access = state
        .agent_access
        .lock()
        .map_err(|_| "Agent Access state is unavailable".to_owned())?;
    access.configure_pin(&pin)?;
    let status = access.status();
    drop(access);
    state.agent_ipc.stop();
    crate::agent::tools::stop_active_operations(
        &app,
        lognook_core::CanScanStopReason::AgentAccessRevoked,
    )?;
    Ok(status)
}

#[tauri::command]
pub async fn unlock_agent_access(
    app: AppHandle,
    state: State<'_, AppState>,
    pin: String,
) -> Result<AgentAccessStatus, String> {
    {
        let mut access = state
            .agent_access
            .lock()
            .map_err(|_| "Agent Access state is unavailable".to_owned())?;
        access.unlock(&pin)?;
    }
    if let Err(error) = state.agent_ipc.start(app).await {
        if let Ok(mut access) = state.agent_access.lock() {
            access.lock();
        }
        return Err(error);
    }
    agent_access_status(state)
}

#[tauri::command]
pub fn lock_agent_access(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AgentAccessStatus, String> {
    let mut access = state
        .agent_access
        .lock()
        .map_err(|_| "Agent Access state is unavailable".to_owned())?;
    access.lock();
    state.agent_ipc.stop();
    let status = access.status();
    drop(access);
    crate::agent::tools::stop_active_operations(
        &app,
        lognook_core::CanScanStopReason::AgentAccessRevoked,
    )?;
    Ok(status)
}

#[tauri::command]
pub fn agent_setup_json(state: State<'_, AppState>) -> Result<String, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not find the LogNook executable: {error}"))?;
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": {
            "lognook": {
                "command": executable,
                "args": ["--agent-mcp"],
                "env": { "LOGNOOK_AGENT_SOCKET": state.agent_ipc_endpoint }
            }
        }
    }))
    .map_err(|error| format!("Could not create Agent Access setup: {error}"))
}

#[tauri::command]
pub fn agent_sync_signal_selection(
    state: State<'_, AppState>,
    log_signal_ids: Vec<String>,
    graph_signal_ids: Vec<String>,
) -> Result<(), String> {
    if log_signal_ids.len() > 256 || graph_signal_ids.len() > 256 {
        return Err("Signal selection is too large".into());
    }
    let engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
    let mut selection = state
        .agent_selection
        .lock()
        .map_err(|_| "Agent selection is unavailable".to_owned())?;
    if engine.is_connected() && !same_ids(&selection.log_signal_ids, &log_signal_ids) {
        return Err("Disconnect before changing logged signals".into());
    }
    *selection = AgentSignalSelection {
        log_signal_ids,
        graph_signal_ids,
    };
    Ok(())
}

fn same_ids(left: &[String], right: &[String]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort();
    right.sort();
    left == right
}
