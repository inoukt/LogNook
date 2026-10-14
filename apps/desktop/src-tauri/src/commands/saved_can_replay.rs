use crate::{
    AppState,
    agent::{log_data, protocol::AgentLogKind},
    can_replay_previews::{
        SavedCanReplayPreviewView, SavedCanReplaySourceRef, SavedCanReplayTargetView,
        StoredPreview, preview_view, same_path, source_is_active, target_view,
    },
};
use lognook_core::{CanIdReplayStatus, SavedCanReplaySelection};
use std::path::{Path, PathBuf};
use tauri::State;

#[tauri::command]
pub fn register_saved_can_replay_source(
    state: State<'_, AppState>,
    path: String,
) -> Result<SavedCanReplaySourceRef, String> {
    register_saved_can_replay_source_with_state(&state, Path::new(&path))
}

#[tauri::command]
pub fn list_saved_can_replay_sources(
    state: State<'_, AppState>,
) -> Result<Vec<SavedCanReplaySourceRef>, String> {
    list_saved_can_replay_sources_with_state(&state)
}

#[tauri::command]
pub async fn prepare_saved_can_replay(
    state: State<'_, AppState>,
    source_id: String,
    selection: SavedCanReplaySelection,
) -> Result<SavedCanReplayPreviewView, String> {
    prepare_saved_can_replay_with_state(&state, source_id, selection).await
}

#[tauri::command]
pub fn get_saved_can_replay_preview(
    state: State<'_, AppState>,
    preview_id: String,
) -> Result<SavedCanReplayPreviewView, String> {
    inspect_saved_can_replay_with_state(&state, &preview_id)
}

#[tauri::command]
pub fn discard_saved_can_replay_preview(
    state: State<'_, AppState>,
    preview_id: Option<String>,
) -> Result<(), String> {
    discard_saved_can_replay_with_state(&state, preview_id.as_deref())
}

#[tauri::command]
pub fn start_saved_can_replay(
    state: State<'_, AppState>,
    preview_id: String,
    expected_can_generation: u64,
    expected_output_capture_id: u64,
) -> Result<CanIdReplayStatus, String> {
    start_saved_can_replay_with_state(
        &state,
        &preview_id,
        expected_can_generation,
        expected_output_capture_id,
    )
}

pub(crate) fn register_saved_can_replay_source_with_state(
    state: &AppState,
    path: &Path,
) -> Result<SavedCanReplaySourceRef, String> {
    state.can_replay_previews.register_source(path)
}

pub(crate) fn list_saved_can_replay_sources_with_state(
    state: &AppState,
) -> Result<Vec<SavedCanReplaySourceRef>, String> {
    state.can_replay_previews.registered_sources()
}

pub(crate) fn resolve_saved_can_replay_source(
    state: &AppState,
    source_id: &str,
) -> Result<PathBuf, String> {
    if let Some(path) = state.can_replay_previews.registered_path(source_id)? {
        return Ok(path);
    }
    let source = log_data::list_log_sources(state)?
        .into_iter()
        .find(|source| {
            source.source_id == source_id && source.kind == AgentLogKind::Can && source.recorded
        })
        .ok_or_else(|| "Saved CAN replay source must be listed by LogNook".to_owned())?;
    let listed_path = PathBuf::from(source.source_id);
    let path = super::super::can_replay_previews::canonical_can_capture(&listed_path)?;
    if !same_path(&path, &listed_path) {
        return Err("Saved CAN source path changed after it was listed".into());
    }
    Ok(path)
}

pub(crate) async fn prepare_saved_can_replay_with_state(
    state: &AppState,
    source_id: String,
    selection: SavedCanReplaySelection,
) -> Result<SavedCanReplayPreviewView, String> {
    let path = resolve_saved_can_replay_source(state, &source_id)?;
    let source_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "CAN capture".into());
    prepare_saved_can_replay_path_with_state(state, path, source_id, source_name, selection).await
}

pub(crate) async fn prepare_saved_can_replay_path_with_state(
    state: &AppState,
    path: PathBuf,
    source_id: String,
    source_name: String,
    selection: SavedCanReplaySelection,
) -> Result<SavedCanReplayPreviewView, String> {
    let resolved = resolve_saved_can_replay_source(state, &source_id)?;
    if !same_path(&resolved, &path) {
        return Err("Saved CAN replay source does not match its registered source ID".into());
    }
    ensure_source_not_active(state, &resolved)?;
    let registry = state.can_replay_previews.clone();
    let preview = tauri::async_runtime::spawn_blocking(move || {
        registry.prepare(resolved, source_id, source_name, selection)
    })
    .await
    .map_err(|error| format!("Saved CAN replay preparation worker failed: {error}"))??;
    if let Err(error) = ensure_source_not_active(state, &preview.source_path) {
        let _ = state.can_replay_previews.discard(Some(&preview.preview_id));
        return Err(error);
    }
    inspect_saved_can_replay_with_state(state, &preview.preview_id)
}

pub(crate) fn inspect_saved_can_replay_with_state(
    state: &AppState,
    preview_id: &str,
) -> Result<SavedCanReplayPreviewView, String> {
    let preview = state.can_replay_previews.preview(preview_id)?;
    let target = current_target_view(state, &preview)?;
    Ok(preview_view(&preview, target))
}

pub(crate) fn discard_saved_can_replay_with_state(
    state: &AppState,
    preview_id: Option<&str>,
) -> Result<(), String> {
    state.can_replay_previews.discard(preview_id)
}

pub(crate) fn start_saved_can_replay_with_state(
    state: &AppState,
    preview_id: &str,
    expected_can_generation: u64,
    expected_output_capture_id: u64,
) -> Result<CanIdReplayStatus, String> {
    state.can_replay_previews.start(preview_id, |preview| {
        let engine = state
            .engine
            .lock()
            .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
        let status = engine
            .can_workbench_status()
            .filter(|status| status.connected)
            .ok_or_else(|| "Connect the intended Candlelight target before replaying".to_owned())?;
        if status.listen_only {
            return Err("Switch Candlelight to active mode before replaying".into());
        }
        if preview
            .summary
            .source_bitrate
            .is_some_and(|bitrate| bitrate != status.bitrate)
        {
            return Err("Target bitrate does not match the source capture".into());
        }
        let output_path = engine
            .active_can_capture_path()
            .ok_or_else(|| "Start a separate paired output capture before replaying".to_owned())?;
        if engine.active_can_capture_id() != Some(expected_output_capture_id) {
            return Err("The output capture changed after review; inspect the updated preview before replaying".into());
        }
        if same_path(&preview.source_path, &output_path) {
            return Err("Choose an output capture separate from the source".into());
        }
        engine
            .start_prepared_can_replay_for_generation(preview.plan.clone(), expected_can_generation)
            .map_err(|error| error.to_string())
    })
}

fn ensure_source_not_active(state: &AppState, source_path: &Path) -> Result<(), String> {
    let engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
    if source_is_active(source_path, engine.active_can_capture_path().as_deref()) {
        return Err("A CAN capture being written by LogNook cannot be a replay source".into());
    }
    Ok(())
}

fn current_target_view(
    state: &AppState,
    preview: &StoredPreview,
) -> Result<SavedCanReplayTargetView, String> {
    let engine = state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine is unavailable".to_owned())?;
    let status = engine.can_workbench_status();
    let output_path = engine.active_can_capture_path();
    let output_capture_id = engine.active_can_capture_id();
    Ok(target_view(
        engine.can_connection_generation(),
        status.as_ref().is_some_and(|status| status.connected),
        status.as_ref().map(|status| status.source_id.clone()),
        engine.can_target_channel(),
        status.as_ref().map(|status| status.bitrate),
        status.as_ref().map(|status| status.listen_only),
        output_path.is_some(),
        output_capture_id,
        &preview.source_path,
        preview.summary.source_bitrate,
        output_path.as_deref(),
    ))
}
