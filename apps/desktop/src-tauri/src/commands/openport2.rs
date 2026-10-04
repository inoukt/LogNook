use super::engine;
use crate::AppState;
use lognook_core::{
    ConnectionCapabilities, DeviceDescriptor, J2534ProviderDiscovery, OpenPort2Adapter,
    available_openport2_providers as list_openport2_providers,
    identify_openport2 as identify_openport2_device,
};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub fn available_openport2_providers() -> Result<J2534ProviderDiscovery, String> {
    list_openport2_providers(&worker_path()?).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn identify_openport2(provider_id: String) -> Result<DeviceDescriptor, String> {
    identify_openport2_device(&worker_path()?, &provider_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn connect_openport2(
    state: State<'_, AppState>,
    provider_id: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    engine(&state)?
        .connect_adapter(
            Box::new(OpenPort2Adapter::new(worker_path()?, provider_id)),
            &signal_ids,
        )
        .map_err(|error| error.to_string())
}

fn worker_path() -> Result<PathBuf, String> {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/i686-pc-windows-msvc")
        .join(profile)
        .join("lognook-j2534-worker.exe");
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "OpenPort worker is missing at {}. Build it with `npm run build:j2534-worker` from apps/desktop.",
            path.display()
        ))
    }
}
