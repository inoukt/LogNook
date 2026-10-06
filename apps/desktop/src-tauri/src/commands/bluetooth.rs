use super::{connect_with_diagnostics, diagnose};
use crate::AppState;
use lognook_core::{
    A30Adapter, BluetoothSppDevice, ConnectionCapabilities, DeviceDescriptor, Elm327Adapter,
    identify_a30_with_diagnostics as identify_a30_device,
    identify_elm327_with_diagnostics as identify_elm327_device, paired_spp_devices,
};
use tauri::State;

#[tauri::command]
pub async fn paired_bluetooth_devices(
    state: State<'_, AppState>,
) -> Result<Vec<BluetoothSppDevice>, String> {
    diagnose(&state, None, "provider_discovery", vec![], |_| {
        paired_spp_devices().map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn identify_elm327(
    state: State<'_, AppState>,
    address: String,
) -> Result<DeviceDescriptor, String> {
    diagnose(
        &state,
        Some(format!("elm327:{address}")),
        "identify",
        vec![],
        |op| identify_elm327_device(address, op.sink()).map_err(|error| error.to_string()),
    )
}

#[tauri::command]
pub async fn identify_a30(
    state: State<'_, AppState>,
    address: String,
) -> Result<DeviceDescriptor, String> {
    diagnose(
        &state,
        Some(format!("a30:{address}")),
        "identify",
        vec![],
        |op| identify_a30_device(address, op.sink()).map_err(|error| error.to_string()),
    )
}

#[tauri::command]
pub async fn connect_elm327(
    state: State<'_, AppState>,
    address: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    connect_with_diagnostics(
        &state,
        format!("elm327:{address}"),
        signal_ids,
        |engine, ids| {
            engine
                .connect_adapter(Box::new(Elm327Adapter::new(address)), ids)
                .map_err(|error| error.to_string())
        },
    )
}

#[tauri::command]
pub async fn connect_a30(
    state: State<'_, AppState>,
    address: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    connect_with_diagnostics(
        &state,
        format!("a30:{address}"),
        signal_ids,
        |engine, ids| {
            engine
                .connect_adapter(Box::new(A30Adapter::new(address)), ids)
                .map_err(|error| error.to_string())
        },
    )
}
