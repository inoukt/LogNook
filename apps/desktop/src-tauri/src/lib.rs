mod commands;

use lognook_core::LogNookEngine;
use std::{path::PathBuf, sync::Mutex};
use tauri::{Emitter, Manager};

pub(crate) struct AppState {
    pub engine: Mutex<LogNookEngine>,
    pub recordings_dir: PathBuf,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let recordings_dir = app.path().app_data_dir()?.join("recordings");
            std::fs::create_dir_all(&recordings_dir)?;
            let app_handle = app.handle().clone();
            let engine = LogNookEngine::new(move |event| {
                let _ = app_handle.emit("lognook-event", event);
            });
            app.manage(AppState {
                engine: Mutex::new(engine),
                recordings_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::available_signals,
            commands::available_obdlink_sx_ports,
            commands::identify_obdlink_sx,
            commands::connect_simulator,
            commands::connect_obdlink_sx,
            commands::disconnect,
            commands::start_recording,
            commands::stop_recording,
            commands::insert_marker,
            commands::export_csv,
            commands::replay_last,
            commands::stop_replay,
            commands::open_csv,
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
