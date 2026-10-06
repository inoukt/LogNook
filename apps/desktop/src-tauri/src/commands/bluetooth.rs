use super::engine;
use crate::AppState;
use lognook_core::{
    A30Adapter, BluetoothSppDevice, ConnectionCapabilities, DeviceDescriptor, Elm327Adapter,
    identify_a30 as identify_a30_device, identify_elm327 as identify_elm327_device,
    paired_spp_devices,
};
use tauri::State;

#[tauri::command]
pub async fn paired_bluetooth_devices() -> Result<Vec<BluetoothSppDevice>, String> {
    paired_spp_devices().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn identify_elm327(address: String) -> Result<DeviceDescriptor, String> {
    identify_elm327_device(address).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn identify_a30(address: String) -> Result<DeviceDescriptor, String> {
    identify_a30_device(address).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_elm327(
    state: State<'_, AppState>,
    address: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_adapter(Box::new(Elm327Adapter::new(address)), &signal_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_a30(
    state: State<'_, AppState>,
    address: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_adapter(Box::new(A30Adapter::new(address)), &signal_ids)
        .map_err(|error| error.to_string())
}
