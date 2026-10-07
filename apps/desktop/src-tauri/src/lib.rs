mod agent;
pub(crate) mod commands;
mod diagnostics;

use lognook_core::LogNookEngine;
use std::{path::PathBuf, sync::Mutex};
use tauri::{Emitter, Manager};

pub(crate) struct AppState {
    pub engine: Mutex<LogNookEngine>,
    pub diagnostics: diagnostics::DiagnosticsService,
    pub diagnostics_path: PathBuf,
    pub recordings_dir: PathBuf,
    pub pid_catalog_path: PathBuf,
    pub oem_validation_log: PathBuf,
    pub oem_candidate_try_log: PathBuf,
    pub a30_read_test_log: PathBuf,
    pub agent_access: Mutex<agent::access::AgentAccessManager>,
    pub agent_ipc: agent::ipc::AgentIpcService,
    pub agent_ipc_endpoint: String,
    pub agent_selection: Mutex<agent::protocol::AgentSignalSelection>,
    pub agent_workbench: Mutex<agent::tools::AgentWorkbenchState>,
    pub trace_store: lognook_core::communication_trace::CommunicationTraceStore,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let recordings_dir = app.path().app_data_dir()?.join("recordings");
            std::fs::create_dir_all(&recordings_dir)?;
            let oem_validation_dir = app.path().app_data_dir()?.join("oem-validation");
            std::fs::create_dir_all(&oem_validation_dir)?;
            let source_catalog = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite");
            let bundled_catalog = app
                .path()
                .resource_dir()
                .ok()
                .map(|directory| directory.join("pids").join("lognook_pids.sqlite"))
                .filter(|path| path.is_file());
            let pid_catalog_path = bundled_catalog.unwrap_or(source_catalog);
            if !pid_catalog_path.is_file() {
                return Err(
                    format!("OEM PID catalog not found: {}", pid_catalog_path.display()).into(),
                );
            }
            let diagnostics_path = app
                .path()
                .app_data_dir()?
                .join("diagnostics")
                .join("operations.jsonl");
            let agent_access = agent::access::AgentAccessManager::open(
                app_data_dir.join("agent-access").join("pin.hash"),
            )
            .map_err(std::io::Error::other)?;
            let agent_ipc = agent::ipc::AgentIpcService::new(&app_data_dir);
            let agent_ipc_endpoint = agent_ipc.endpoint().to_owned();
            let trace_store = lognook_core::communication_trace::CommunicationTraceStore::new();
            let diagnostics_handle = app.handle().clone();
            let diagnostics =
                diagnostics::DiagnosticsService::start(diagnostics_path.clone(), move |record| {
                    let _ = diagnostics_handle.emit("diagnostics-event", record);
                })?;
            let engine_diagnostics = diagnostics.clone();
            let app_handle = app.handle().clone();
            let mut engine = LogNookEngine::new(move |event| {
                engine_diagnostics.observe_engine(&event);
                let _ = app_handle.emit("lognook-event", event);
            });
            engine.set_communication_sink(trace_store.sink());
            app.manage(AppState {
                engine: Mutex::new(engine),
                diagnostics,
                diagnostics_path,
                recordings_dir,
                pid_catalog_path,
                oem_validation_log: oem_validation_dir.join("scion-xb-mode21.jsonl"),
                oem_candidate_try_log: oem_validation_dir.join("oem-candidate-tries.jsonl"),
                a30_read_test_log: oem_validation_dir.join("a30-read-tests.jsonl"),
                agent_access: Mutex::new(agent_access),
                agent_ipc,
                agent_ipc_endpoint,
                agent_selection: Mutex::new(agent::protocol::AgentSignalSelection {
                    log_signal_ids: vec![
                        "engine_rpm".into(),
                        "coolant_temp".into(),
                        "intake_temp".into(),
                    ],
                    graph_signal_ids: vec![
                        "engine_rpm".into(),
                        "coolant_temp".into(),
                        "intake_temp".into(),
                    ],
                }),
                agent_workbench: Mutex::new(agent::tools::AgentWorkbenchState::default()),
                trace_store,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::diagnostic_snapshot,
            commands::agent_access_status,
            commands::configure_agent_pin,
            commands::unlock_agent_access,
            commands::lock_agent_access,
            commands::agent_setup_json,
            commands::agent_sync_signal_selection,
            commands::choose_diagnostics_recording,
            commands::export_support_bundle,
            commands::available_signals,
            commands::search_oem_catalog,
            commands::probe_scion_xb_mode21,
            commands::try_oem_candidate,
            commands::try_a30_oem_candidate,
            commands::try_a30_read_test,
            commands::try_elm_oem_candidate,
            commands::paired_bluetooth_devices,
            commands::identify_elm327,
            commands::identify_a30,
            commands::connect_elm327,
            commands::connect_a30,
            commands::available_obdlink_sx_ports,
            commands::identify_obdlink_sx,
            commands::available_openport2_providers,
            commands::identify_openport2,
            commands::connect_openport2,
            commands::available_can_adapters,
            commands::choose_can_capture_save_path,
            commands::choose_can_capture_open_path,
            commands::choose_can_dbc_export_path,
            commands::choose_can_trigger_save_path,
            commands::connect_candlelight,
            commands::configure_can_signals,
            commands::preview_can_signal,
            commands::start_can_capture,
            commands::stop_can_capture,
            commands::mark_can_event,
            commands::clear_can_event_results,
            commands::send_can_frame,
            commands::start_can_scan,
            commands::stop_can_scan,
            commands::arm_can_trigger,
            commands::trigger_can_now,
            commands::disarm_can_trigger,
            commands::save_can_trigger_capture,
            commands::start_can_oem_candidate,
            commands::replay_can_capture_report,
            commands::compare_can_captures,
            commands::export_can_dbc_skeleton,
            commands::connect_simulator,
            commands::connect_obdlink_sx,
            commands::disconnect,
            commands::configure_derived_signals,
            commands::tare_derived_signal,
            commands::start_recording,
            commands::stop_recording,
            commands::arm_automatic_trigger,
            commands::disarm_automatic_trigger,
            commands::automatic_trigger_status,
            commands::insert_marker,
            commands::export_csv,
            commands::export_live_csv,
            commands::export_research_run_report,
            commands::share_recording_csv,
            commands::save_recording_as,
            commands::replay_last,
            commands::stop_replay,
            commands::open_session_file,
            commands::list_recordings,
            commands::open_recording,
            commands::load_last_recording,
            commands::start_replay,
            commands::pause_replay,
            commands::resume_replay,
            commands::seek_replay,
            commands::set_replay_speed,
            commands::playback_state,
            commands::add_playback_marker,
            commands::update_playback_marker,
            commands::delete_playback_marker,
            commands::recording_window,
            commands::recording_values_at,
            commands::recording_averages,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run LogNook desktop app");
}

pub fn run_agent_mcp() -> Result<(), String> {
    tauri::async_runtime::block_on(agent::mcp::run_agent_mcp())
}
