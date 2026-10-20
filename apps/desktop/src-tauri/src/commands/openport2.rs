use super::{connect_with_diagnostics, diagnose};
use crate::AppState;
use lognook_core::{
    ConnectionCapabilities, DeviceDescriptor, J2534ProviderDiscovery, OpenPort2Adapter,
    available_openport2_providers_with_sinks as list_openport2_providers,
    identify_openport2_with_sinks as identify_openport2_device,
};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub async fn available_openport2_providers(
    state: State<'_, AppState>,
) -> Result<J2534ProviderDiscovery, String> {
    discover_providers(&state, worker_path)
}
pub(crate) fn discover_providers(
    state: &AppState,
    resolve_worker: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<J2534ProviderDiscovery, String> {
    diagnose(state, None, "provider_discovery", vec![], |op| {
        list_openport2_providers(
            &resolve_worker()?,
            op.sink(),
            Some(state.trace_store.sink()),
        )
        .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn identify_openport2(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<DeviceDescriptor, String> {
    diagnose(
        &state,
        Some(provider_id.clone()),
        "identify",
        vec![],
        |op| {
            identify_openport2_device(
                &worker_path()?,
                &provider_id,
                op.sink(),
                Some(state.trace_store.sink()),
            )
            .map_err(|error| error.to_string())
        },
    )
}

#[tauri::command]
pub async fn connect_openport2(
    state: State<'_, AppState>,
    provider_id: String,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    connect_with_diagnostics(&state, provider_id.clone(), signal_ids, |engine, ids| {
        engine
            .connect_adapter(
                Box::new(OpenPort2Adapter::new(worker_path()?, provider_id)),
                ids,
            )
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn connect_openport2_ssm(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<ConnectionCapabilities, String> {
    connect_with_diagnostics(
        &state,
        format!("openport2:ssm:{provider_id}"),
        Vec::new(),
        |engine, _| {
            engine
                .connect_adapter_without_standard_signals(Box::new(OpenPort2Adapter::new_subaru_ssm(
                    worker_path()?,
                    provider_id,
                )))
                .map_err(|error| error.to_string())
        },
    )
}

pub(crate) fn worker_path() -> Result<PathBuf, String> {
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
