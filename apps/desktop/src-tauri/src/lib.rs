mod commands;

use lognook_core::LogNookEngine;
use std::{path::PathBuf, sync::Mutex};
use tauri::{Emitter, Manager};

pub(crate) struct AppState {
    pub engine: Mutex<LogNookEngine>,
    pub recordings_dir: PathBuf,
    pub pid_catalog_path: PathBuf,
    pub oem_validation_log: PathBuf,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let recordings_dir = app.path().app_data_dir()?.join("recordings");
            std::fs::create_dir_all(&recordings_dir)?;
            let oem_validation_dir = app.path().app_data_dir()?.join("oem-validation");
            std::fs::create_dir_all(&oem_validation_dir)?;
            let source_catalog = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite");
            let bundled_catalog = app.path().resource_dir().ok()
                .map(|directory| directory.join("pids").join("lognook_pids.sqlite"))
                .filter(|path| path.is_file());
            let pid_catalog_path = bundled_catalog.unwrap_or(source_catalog);
            if !pid_catalog_path.is_file() {
                return Err(format!("OEM PID catalog not found: {}", pid_catalog_path.display()).into());
            }
            let app_handle = app.handle().clone();
            let engine = LogNookEngine::new(move |event| {
                let _ = app_handle.emit("lognook-event", event);
            });
            app.manage(AppState {
                engine: Mutex::new(engine),
                recordings_dir,
                pid_catalog_path,
                oem_validation_log: oem_validation_dir.join("scion-xb-mode21.jsonl"),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::available_signals,
            commands::search_oem_catalog,
            commands::probe_scion_xb_mode21,
            commands::available_obdlink_sx_ports,
            commands::identify_obdlink_sx,
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
            commands::share_recording_csv,
            commands::save_recording_as,
            commands::replay_last,
            commands::stop_replay,
            commands::open_csv,
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
