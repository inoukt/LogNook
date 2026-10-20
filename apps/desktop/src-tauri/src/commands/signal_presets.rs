use crate::{AppState, vehicle_profiles::StoredSignalPresetPreview};
use lognook_core::{PresetApplyMode, SignalPresetOperation, SignalPresetResult, resolve_preset, scope_match_rank};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Copy)]
pub(crate) enum PresetCaller { Ui, Agent }

pub(crate) fn execute(state: &AppState, operation: SignalPresetOperation, caller: PresetCaller) -> Result<SignalPresetResult, String> {
    let caller_generation = state.vehicle_profiles.work_generation();
    match operation {
        SignalPresetOperation::List => Ok(SignalPresetResult::Presets { presets: state.vehicle_profiles.list_signal_presets()? }),
        SignalPresetOperation::Get { id } => Ok(SignalPresetResult::Preset { preset: state.vehicle_profiles.get_signal_preset(&id)? }),
        SignalPresetOperation::Suggest { context } => {
            let presets = state.vehicle_profiles.list_signal_presets()?.into_iter()
                .filter_map(|preset| scope_match_rank(preset.scope, &preset.criteria, &context).map(|rank| (rank, preset)))
                .collect::<Vec<_>>();
            let mut presets = presets;
            presets.sort_by_key(|(rank, preset)| (std::cmp::Reverse(*rank), preset.origin as u8, preset.name.clone()));
            Ok(SignalPresetResult::Suggestions { presets: presets.into_iter().map(|(_, preset)| preset).collect(), context })
        }
        SignalPresetOperation::SetContext { mut context, expected_revision } => {
            let mut current = state.vehicle_profiles.get_preset_picker_state()?;
            if current.revision != expected_revision { return Err("Preset context changed; refresh before saving".into()); }
            context.revision = expected_revision;
            context.connection_generation = None;
            current.context = context;
            current.staged = None;
            let saved = state.vehicle_profiles.save_preset_picker_state(current, Some(expected_revision))?;
            state.vehicle_profiles.discard_signal_preset_preview(None)?;
            Ok(SignalPresetResult::PickerState { state: saved })
        }
        SignalPresetOperation::Stage { preset_id, preset_revision, mode, expected_revision } => {
            let mut current = state.vehicle_profiles.get_preset_picker_state()?;
            if current.revision != expected_revision { return Err("Preset picker state changed; refresh before staging".into()); }
            current.staged = Some(lognook_core::StagedPresetIntent { preset_id, preset_revision, mode });
            let saved = state.vehicle_profiles.save_preset_picker_state(current, Some(expected_revision))?;
            Ok(SignalPresetResult::PickerState { state: saved })
        }
        SignalPresetOperation::Save { preset, expected_revision } => {
            state.vehicle_profiles.discard_signal_preset_preview(None)?;
            Ok(SignalPresetResult::Preset { preset: Some(state.vehicle_profiles.save_signal_preset(preset, expected_revision)?) })
        }
        SignalPresetOperation::Delete { id, expected_revision } => {
            state.vehicle_profiles.delete_signal_preset(&id, Some(expected_revision))?;
            state.vehicle_profiles.discard_signal_preset_preview(None)?;
            Ok(SignalPresetResult::Ok)
        }
        SignalPresetOperation::Preview { preset_id, mode, expected_context_revision, expected_generation } => {
            let _operation = state.vehicle_profiles.lock_operation()?;
            let preset = state.vehicle_profiles.get_signal_preset(&preset_id)?.ok_or("Signal preset was not found")?;
            let mut context = state.vehicle_profiles.get_preset_picker_state()?.context;
            if context.revision != expected_context_revision { return Err("Preset context changed; prepare a new preview".into()); }
            let (capabilities, generation, safe_to_apply) = {
                let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
                if !engine.is_obd_connected() || engine.obd_connection_generation() != expected_generation { return Err("Connect the vehicle or refresh the preset preview".into()); }
                let busy = super::vehicle_profiles::can_operations_busy(state, &engine)?;
                (engine.connection_capabilities().cloned().ok_or("OBD connection identity is unavailable")?, engine.obd_connection_generation(), !engine.is_recording() && !engine.is_playback_active() && !engine.oem_polling_status().active && !busy)
            };
            context.connection_generation = Some(generation);
            let mut resolved = resolve_preset(&preset, &context, &capabilities, &[]);
            let base_selection = state.agent_selection.lock().map_err(|_| "Current selection is unavailable")?.clone();
            let unsupported_base = if mode == PresetApplyMode::Add {
                base_selection.log_signal_ids.iter().chain(&base_selection.graph_signal_ids)
                    .filter(|id| !capabilities.supported_signals.iter().any(|signal| &signal.id == *id))
                    .cloned().collect::<Vec<_>>()
            } else { Vec::new() };
            if mode == PresetApplyMode::Add {
                for id in &base_selection.log_signal_ids { if !resolved.selection.log_signal_ids.contains(id) { resolved.selection.log_signal_ids.push(id.clone()); } }
                for id in &base_selection.graph_signal_ids { if !resolved.selection.graph_signal_ids.contains(id) { resolved.selection.graph_signal_ids.push(id.clone()); } }
                for id in &unsupported_base { resolved.skipped.push(lognook_core::PresetAvailability { concept: format!("Existing selection {id}"), signal_id: Some(id.clone()), eligible: false, reason: Some(lognook_core::PresetSkipReason::Unsupported), task_defining: false, priority: 0, slow_context: false, source: None }); }
            }
            let preview_id = state.vehicle_profiles.make_preview_id()?;
            let can_apply = !resolved.selection.log_signal_ids.is_empty() && safe_to_apply && unsupported_base.is_empty();
            state.vehicle_profiles.store_signal_preset_preview(StoredSignalPresetPreview { preview_id: preview_id.clone(), preset_id, preset_revision: preset.revision, context_revision: context.revision, generation, mode, resolved: resolved.clone(), base_selection, agent_work_generation: matches!(caller, PresetCaller::Agent).then_some(caller_generation), can_apply, created: Instant::now() })?;
            Ok(SignalPresetResult::Preview { resolved, preview_id, mode, can_apply })
        }
        SignalPresetOperation::Apply { preview_id, expected_context_revision, expected_generation } => {
            let _operation = state.vehicle_profiles.lock_operation()?;
            let stored = state.vehicle_profiles.current_signal_preset_preview().filter(|preview| preview.preview_id == preview_id).ok_or("Preset preview is missing, expired, or was replaced")?;
            if !stored.can_apply { return Err("The preview contains unavailable channels or conflicting acquisition work".into()); }
            if stored.agent_work_generation.is_some() != matches!(caller, PresetCaller::Agent) { return Err("Preset preview belongs to a different caller".into()); }
            if stored.agent_work_generation.is_some_and(|revision| revision != caller_generation) { return Err("Agent Access was locked or cancelled after preview".into()); }
            if stored.context_revision != expected_context_revision || stored.generation != expected_generation { return Err("Preset context or connection changed after preview".into()); }
            let preset = state.vehicle_profiles.get_signal_preset(&stored.preset_id)?.ok_or("Signal preset was deleted")?;
            if preset.revision != stored.preset_revision { return Err("Signal preset changed after preview; prepare a new preview".into()); }
            let mut context = state.vehicle_profiles.get_preset_picker_state()?.context;
            if context.revision != stored.context_revision { return Err("Preset context changed after preview".into()); }
            let capabilities = {
                let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
                if !engine.is_obd_connected() || engine.obd_connection_generation() != stored.generation { return Err("OBD connection changed after preview".into()); }
                if engine.is_recording() || engine.is_playback_active() || engine.oem_polling_status().active { return Err("Stop recording, playback, and OEM polling before applying a preset".into()); }
                if super::vehicle_profiles::can_operations_busy(state, &engine)? { return Err("Stop conflicting CAN work before applying a preset".into()); }
                engine.connection_capabilities().cloned().ok_or("OBD connection identity is unavailable")?
            };
            if state.agent_selection.lock().map_err(|_| "Current selection is unavailable")?.ne(&stored.base_selection) { return Err("Current signal selection changed after preview".into()); }
            context.connection_generation = Some(stored.generation);
            let fresh = resolve_preset(&preset, &context, &capabilities, &[]);
            if fresh.eligible != stored.resolved.eligible || fresh.skipped != stored.resolved.skipped { return Err("Signal support changed after preview".into()); }
            if state.vehicle_profiles.work_generation() != caller_generation { return Err("Preset apply was cancelled".into()); }
            let selected = &stored.resolved.selection;
            let standard_logs = selected.log_signal_ids.iter().filter(|id| !id.starts_with("oem:")).cloned().collect::<Vec<_>>();
            let definitions = preset.selection.as_ref().map(|selection| selection.derived_definitions.clone()).unwrap_or_default();
            let active_derived = definitions.iter().filter(|definition| selected.log_signal_ids.contains(&definition.id) || selected.graph_signal_ids.contains(&definition.id)).map(|definition| definition.id.clone()).collect();
            let selection = crate::agent::protocol::AgentSignalSelection { log_signal_ids: selected.log_signal_ids.clone(), graph_signal_ids: selected.graph_signal_ids.clone() };
            {
                let mut engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
                if state.vehicle_profiles.work_generation() != caller_generation { return Err("Preset apply was cancelled".into()); }
                if engine.is_recording() || engine.is_playback_active() || engine.oem_polling_status().active || super::vehicle_profiles::can_operations_busy(state, &engine)? { return Err("Acquisition became busy before preset Apply".into()); }
                let mut current_selection = state.agent_selection.lock().map_err(|_| "Current selection is unavailable")?;
                if *current_selection != stored.base_selection { return Err("Current signal selection changed before preset Apply".into()); }
                let _agent_access_guard = if matches!(caller, PresetCaller::Agent) {
                    let mut access = state.agent_access.lock().map_err(|_| "Agent Access state is unavailable")?;
                    access.authorize_action().map_err(|_| {
                        let _ = state.vehicle_profiles.discard_signal_preset_preview(Some(&preview_id));
                        "Agent Access locked before preset Apply"
                    })?;
                    Some(access)
                } else { None };
                engine.replace_live_selection_with_oem(standard_logs, definitions, active_derived, None, stored.generation).map_err(|error| error.to_string())?;
                *current_selection = selection.clone();
            }
            state.vehicle_profiles.discard_signal_preset_preview(Some(&preview_id))?;
            Ok(SignalPresetResult::Applied { resolved: stored.resolved, mode: stored.mode })
        }
        SignalPresetOperation::Status => {
            let picker = state.vehicle_profiles.get_preset_picker_state()?;
            let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
            let generation = engine.obd_connection_generation();
            let current_preview = state.vehicle_profiles.current_signal_preset_preview();
            if current_preview.as_ref().is_some_and(|preview| preview.generation != generation) { state.vehicle_profiles.discard_signal_preset_preview(None)?; }
            let preview = current_preview.filter(|preview| preview.generation == generation);
            Ok(SignalPresetResult::Status { connected: engine.is_obd_connected(), generation, context: picker.context, staged: picker.staged, preview_id: preview.map(|preview| preview.preview_id) })
        }
        SignalPresetOperation::Cancel { preview_id } => {
            state.vehicle_profiles.discard_signal_preset_preview(preview_id.as_deref())?;
            Ok(SignalPresetResult::Ok)
        }
    }
}

#[tauri::command]
pub fn signal_presets(app: AppHandle, state: State<'_, AppState>, operation: SignalPresetOperation) -> Result<SignalPresetResult, String> {
    let result = execute(&state, operation, PresetCaller::Ui)?;
    if let SignalPresetResult::Applied { resolved, .. } = &result {
        let _ = app.emit("agent-signal-selection", crate::agent::protocol::AgentSignalSelection { log_signal_ids: resolved.selection.log_signal_ids.clone(), graph_signal_ids: resolved.selection.graph_signal_ids.clone() });
    }
    Ok(result)
}
