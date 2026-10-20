use crate::{AppState, vehicle_profiles::SaveVehicleProfileArgs};
use lognook_core::{ProfileSelectionSnapshot, VehicleProjectWorkspace, VehicleProfile, SignalPreset, PresetOrigin, PresetScope, PresetSize, PresetTaskFamily, PresetVehicleCriteria, PresetChannel, standard_obd_signals};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use tauri::State;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum VehicleProjectsOperation {
    Status,
    List,
    Get { project_id: String },
    PrepareOnConnect { expected_generation: u64 },
    LoadLastState { project_id: String, expected_revision: u64, expected_generation: u64 },
    StartFresh { project_id: String, expected_revision: u64, expected_generation: u64 },
    Save { project_id: String, expected_revision: u64, expected_generation: u64, selection: ProfileSelectionSnapshot, workspace: VehicleProjectWorkspace },
    SelectManual { project_id: String, expected_generation: u64 },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct VehicleProjectsArgs { pub operation: VehicleProjectsOperation }

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct VehicleProjectView { pub profile: VehicleProfile, pub workspace: VehicleProjectWorkspace }

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct VehicleProjectUpdatedEvent { pub connection_generation: u64, pub result: VehicleProjectsResult }

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VehicleProjectsResult {
    Status { state: String, connection_generation: u64, project_id: Option<String>, project: Option<VehicleProjectView>, choice_required: bool },
    Projects { projects: Vec<VehicleProjectView> },
    Project { project: Option<VehicleProjectView> },
    Prepared { state: String, project: Option<VehicleProjectView>, choice_required: bool, reason: Option<String> },
    Loaded { project: VehicleProjectView, eligibility: crate::vehicle_profiles::ProfileResumePreview, applied: crate::vehicle_profiles::ProfileResumeApplyResult },
    Fresh { state: String, project: VehicleProjectView, selection: ProfileSelectionSnapshot },
    ManualSelected { project: VehicleProjectView, selection: ProfileSelectionSnapshot, skipped_refs: Vec<String> },
    Saved { project: VehicleProjectView },
}

fn view(state: &AppState, profile: VehicleProfile) -> Result<VehicleProjectView, String> {
    Ok(VehicleProjectView { workspace: state.vehicle_profiles.get_project_workspace(&profile.id)?, profile })
}

fn ensure_basic_preset(state: &AppState, profile: &VehicleProfile, supported: &[String]) -> Result<(), String> {
    let preset_id = format!("project-{}", profile.id);
    if state.vehicle_profiles.get_signal_preset(&preset_id)?.is_none() {
        let selection = ProfileSelectionSnapshot { log_signal_ids: supported.to_vec(), graph_signal_ids: supported.to_vec(), ..Default::default() };
        let preset = SignalPreset { id: preset_id.clone(), revision: 0, name: "Basic vehicle signals".into(), family: PresetTaskFamily::TrimsAirFuel, size: PresetSize::Core, origin: PresetOrigin::Custom, scope: PresetScope::ExactProfile, criteria: PresetVehicleCriteria { profile_id: Some(profile.id.clone()), ..Default::default() }, channels: standard_obd_signals().into_iter().filter(|s| supported.contains(&s.id)).map(|s| PresetChannel { concept: s.name.clone(), signal_id: Some(s.id), oem_alternatives: vec![], task_defining: true, priority: 1, slow_context: false }).collect(), selection: Some(selection), required_adapter: None, required_protocol: None };
        state.vehicle_profiles.save_signal_preset_locked(preset, None)?;
    }
    let mut workspace = state.vehicle_profiles.get_project_workspace(&profile.id)?;
    if workspace.current_preset_id.is_none() {
        workspace.current_preset_id = Some(preset_id);
        state.vehicle_profiles.save_project_workspace(&profile.id, &workspace)?;
    }
    Ok(())
}

fn apply_generic_selection(state: &AppState, profile: &VehicleProfile, generation: u64) -> Result<(), String> {
    let logs = profile.selection.log_signal_ids.iter().filter(|id| !id.starts_with("oem:")).cloned().collect();
    let selection = crate::agent::protocol::AgentSignalSelection { log_signal_ids: profile.selection.log_signal_ids.clone(), graph_signal_ids: profile.selection.graph_signal_ids.clone() };
    let mut engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
    if !engine.is_obd_connected() || engine.obd_connection_generation() != generation { return Err("Adapter connection changed before vehicle selection apply".into()); }
    engine.replace_profile_selection_with_oem(logs, profile.selection.derived_definitions.clone(), vec![], None, generation).map_err(|e| e.to_string())?;
    *state.agent_selection.lock().map_err(|_| "Current signal selection is unavailable")? = selection;
    state.vehicle_profiles.set_active_profile(profile.id.clone(), profile.revision, generation)?;
    Ok(())
}

pub(crate) fn execute(state: &AppState, operation: VehicleProjectsOperation) -> Result<VehicleProjectsResult, String> {
    use VehicleProjectsOperation as Op;
    match operation {
        Op::List => Ok(VehicleProjectsResult::Projects { projects: state.vehicle_profiles.list()?.into_iter().map(|p| view(state, p)).collect::<Result<_, _>>()? }),
        Op::Get { project_id } => Ok(VehicleProjectsResult::Project { project: state.vehicle_profiles.get(&project_id)?.map(|p| view(state, p)).transpose()? }),
        Op::Status => {
            let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
            let generation = engine.obd_connection_generation();
            let observation = state.vehicle_profiles.current_observation(generation);
            let digest = observation.as_ref().and_then(|o| o.vin_digest.as_deref());
            let pending = if let Some(digest) = digest { state.vehicle_profiles.pending_project_choice(generation, digest)? } else { None };
            let active = state.vehicle_profiles.active_profile(generation).map(|(id, _)| id);
            let registered = if let (Some(digest), Some(observation)) = (digest, observation.as_ref()) { state.vehicle_profiles.get_vehicle_project_id(observation.scope, digest)? } else { None };
            let choice_required = pending.is_some();
            let chosen = pending.or(active).or(registered);
            let project = chosen.and_then(|id| state.vehicle_profiles.get(&id).ok().flatten()).map(|p| view(state, p)).transpose()?;
            let is_active = project.as_ref().is_some_and(|p| state.vehicle_profiles.active_profile(generation).is_some_and(|(id, _)| id == p.profile.id));
            let status = if choice_required { "pending_choice" } else if digest.is_none() { "unknown_vin" } else if is_active { "ready" } else { "needs_prepare" };
            Ok(VehicleProjectsResult::Status { state: status.into(), connection_generation: generation, project_id: project.as_ref().map(|p| p.profile.id.clone()), project, choice_required })
        }
        Op::PrepareOnConnect { expected_generation } => {
            let _profile_operation = state.vehicle_profiles.lock_operation()?;
            let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
            if !engine.is_obd_connected() || engine.obd_connection_generation() != expected_generation { return Err("Adapter connection changed before VIN project preparation".into()); }
            let capabilities = engine.connection_capabilities().cloned().ok_or("OBD connection identity is unavailable")?;
            drop(engine);
            let observation = state.vehicle_profiles.current_observation(expected_generation).ok_or("Read a fresh VIN identity before preparing a vehicle project")?;
            if observation.vin_status != lognook_core::IdentityReadState::Positive || observation.conflicting_vin_observations || observation.vin_digest.is_none() {
                return Ok(VehicleProjectsResult::Prepared { state: "unknown_vin".into(), project: None, choice_required: false, reason: Some("A unique positive VIN was not available; choose a project manually".into()) });
            }
            let digest = observation.vin_digest.as_deref().unwrap();
            if let Some(project_id) = state.vehicle_profiles.get_vehicle_project_id(observation.scope, digest)? {
                let profile = state.vehicle_profiles.get(&project_id)?.ok_or("Registered vehicle project is missing")?;
                let common = ["engine_rpm", "vehicle_speed", "coolant_temp", "intake_temp", "throttle_pos"];
                let supported = capabilities.supported_signals.iter().filter(|s| common.contains(&s.id.as_str())).map(|s| s.id.clone()).collect::<Vec<_>>();
                ensure_basic_preset(state, &profile, &supported)?;
                state.vehicle_profiles.set_project_choice(profile.id.clone(), expected_generation, digest.into())?;
                return Ok(VehicleProjectsResult::Prepared { state: "pending_choice".into(), project: Some(view(state, profile)?), choice_required: true, reason: None });
            }
            let common = ["engine_rpm", "vehicle_speed", "coolant_temp", "intake_temp", "throttle_pos"];
            let supported = capabilities.supported_signals.iter().filter(|s| common.contains(&s.id.as_str())).map(|s| s.id.clone()).collect::<Vec<_>>();
            let selection = ProfileSelectionSnapshot { log_signal_ids: supported.clone(), graph_signal_ids: supported.clone(), ..Default::default() };
            let profile = crate::commands::vehicle_profiles::save_vehicle_profile_with_workspace_locked_shared(state, SaveVehicleProfileArgs { profile_id: None, expected_revision: None, name: format!("Vehicle {}", observation.masked_vin.as_deref().unwrap_or("(VIN)")), selection: selection.clone(), manual_association: false, catalog_vehicle_id: None }, None, Some(expected_generation))?;
            state.vehicle_profiles.register_vehicle_project(observation.scope, digest, &profile.id)?;
            ensure_basic_preset(state, &profile, &supported)?;
            apply_generic_selection(state, &profile, expected_generation)?;
            Ok(VehicleProjectsResult::Prepared { state: "created".into(), project: Some(view(state, profile)?), choice_required: false, reason: None })
        }
        Op::Save { project_id, expected_revision, expected_generation, selection, workspace } => {
            let _profile_operation = state.vehicle_profiles.lock_operation()?;
            workspace.validate()?;
            let observation = state.vehicle_profiles.current_observation(expected_generation).ok_or("Fresh VIN identity is required before saving a vehicle project")?;
            if observation.vin_status != lognook_core::IdentityReadState::Positive || observation.conflicting_vin_observations { return Err("A unique positive VIN is required to save a vehicle project".into()); }
            let pending = observation.vin_digest.as_deref().map(|d| state.vehicle_profiles.pending_project_choice(expected_generation, d)).transpose()?.flatten();
            if pending.is_some() { return Err("Choose Load last state or Start fresh before saving".into()); }
            let existing = state.vehicle_profiles.get(&project_id)?.ok_or("Vehicle project was not found")?;
            if existing.scope != observation.scope || existing.vin_digest != observation.vin_digest || observation.conflicting_vin_observations { return Err("Connected VIN or scope does not match this vehicle project".into()); }
            if state.vehicle_profiles.get_vehicle_project_id(observation.scope, observation.vin_digest.as_deref().ok_or("Current VIN is unavailable")?)?.as_deref() != Some(project_id.as_str()) { return Err("Only the canonical project for this VIN can be saved".into()); }
            let saved = crate::commands::vehicle_profiles::save_vehicle_profile_with_workspace_locked_shared(state, SaveVehicleProfileArgs { profile_id: Some(project_id.clone()), expected_revision: Some(expected_revision), name: existing.name, selection, manual_association: false, catalog_vehicle_id: existing.catalog_vehicle_id }, Some(&workspace), Some(expected_generation))?;
            if saved.revision == expected_revision { return Err("Vehicle project revision did not advance".into()); }
            Ok(VehicleProjectsResult::Saved { project: view(state, saved)? })
        }
        Op::SelectManual { project_id, expected_generation } => {
            let _profile_operation = state.vehicle_profiles.lock_operation()?;
            let profile = state.vehicle_profiles.get(&project_id)?.ok_or("Vehicle project was not found")?;
            let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
            if !engine.is_obd_connected() || engine.obd_connection_generation() != expected_generation { return Err("Adapter connection changed before manual project selection".into()); }
            let capabilities = engine.connection_capabilities().ok_or("OBD connection identity is unavailable")?;
            let scope = crate::commands::vehicle_profiles::connection_scope(&capabilities.device.hardware_family);
            if profile.scope != scope { return Err("Selected project belongs to a different connection scope".into()); }
            if let Some(observation) = state.vehicle_profiles.current_observation(expected_generation) {
                if observation.conflicting_vin_observations || observation.vin_digest.as_ref().is_some_and(|vin| profile.vin_digest.as_ref() != Some(vin)) || profile.scope != observation.scope {
                    return Err("Connected VIN or scope conflicts with the selected project".into());
                }
            }
            if engine.is_recording() || engine.is_playback_active() || engine.oem_polling_status().active || crate::commands::vehicle_profiles::can_operations_busy(state, &engine)? {
                return Err("Stop recording, playback, OEM polling, and CAN work before selecting a vehicle project".into());
            }
            let supported = capabilities.supported_signals.iter().map(|signal| signal.id.as_str()).collect::<std::collections::HashSet<_>>();
            let standard = standard_obd_signals().into_iter().map(|signal| signal.id).collect::<std::collections::HashSet<_>>();
            let selected_logs = profile.selection.log_signal_ids.iter().filter(|id| standard.contains(id.as_str()) && supported.contains(id.as_str())).cloned().collect::<Vec<_>>();
            let selected_graphs = profile.selection.graph_signal_ids.iter().filter(|id| standard.contains(id.as_str()) && supported.contains(id.as_str())).cloned().collect::<Vec<_>>();
            let logs = if selected_logs.is_empty() {
                ["engine_rpm", "vehicle_speed", "coolant_temp", "intake_temp", "throttle_pos"].into_iter().filter(|id| supported.contains(id)).map(str::to_owned).collect::<Vec<_>>()
            } else { selected_logs };
            if logs.is_empty() { return Err("The connected adapter reports no supported standard signals for this project".into()); }
            let skipped_refs = profile.selection.log_signal_ids.iter().chain(&profile.selection.graph_signal_ids).filter(|id| !standard.contains(id.as_str()) || !supported.contains(id.as_str())).cloned().collect::<Vec<_>>();
            let selection = ProfileSelectionSnapshot { log_signal_ids: logs.clone(), graph_signal_ids: if selected_graphs.is_empty() { logs.clone() } else { selected_graphs }, ..Default::default() };
            let mut engine = engine;
            engine.replace_live_selection_with_oem(logs, Vec::new(), Vec::new(), None, expected_generation).map_err(|error| error.to_string())?;
            *state.agent_selection.lock().map_err(|_| "Current signal selection is unavailable")? = crate::agent::protocol::AgentSignalSelection { log_signal_ids: selection.log_signal_ids.clone(), graph_signal_ids: selection.graph_signal_ids.clone() };
            state.vehicle_profiles.set_active_profile(profile.id.clone(), profile.revision, expected_generation)?;
            state.vehicle_profiles.finish_project_choice(&profile.id, expected_generation)?;
            Ok(VehicleProjectsResult::ManualSelected { project: view(state, profile)?, selection, skipped_refs })
        }
        Op::LoadLastState { project_id, expected_revision, expected_generation } => {
            let profile = state.vehicle_profiles.get(&project_id)?.ok_or("Vehicle project was not found")?;
            if profile.revision != expected_revision { return Err("Vehicle project changed; refresh before loading".into()); }
            let observation = state.vehicle_profiles.current_observation(expected_generation).ok_or("Fresh VIN identity is required")?;
            if profile.vin_digest != observation.vin_digest || profile.scope != observation.scope { return Err("Connected VIN does not match this project".into()); }
            if state.vehicle_profiles.get_vehicle_project_id(observation.scope, observation.vin_digest.as_deref().ok_or("Current VIN is unavailable")?)?.as_deref() != Some(project_id.as_str()) { return Err("This profile is not the canonical project for the connected VIN".into()); }
            let preview = crate::commands::vehicle_profiles::prepare_project_resume_shared(state, &profile.id, expected_generation)?;
            let applied = crate::commands::vehicle_profiles::apply_project_resume_shared(state, &preview.preview_id, profile.revision, expected_generation)?;
            state.vehicle_profiles.finish_project_choice(&profile.id, expected_generation)?;
            Ok(VehicleProjectsResult::Loaded { project: view(state, profile)?, eligibility: preview, applied })
        }
        Op::StartFresh { project_id, expected_revision, expected_generation } => {
            let _profile_operation = state.vehicle_profiles.lock_operation()?;
            let profile = state.vehicle_profiles.get(&project_id)?.ok_or("Vehicle project was not found")?;
            if profile.revision != expected_revision { return Err("Vehicle project changed; refresh before starting fresh".into()); }
            let engine = state.engine.lock().map_err(|_| "Acquisition engine is unavailable")?;
            if !engine.is_obd_connected() || engine.obd_connection_generation() != expected_generation { return Err("Adapter connection changed before Start fresh".into()); }
            if !engine.is_obd_profile_standby() || engine.is_recording() || engine.is_playback_active() || engine.oem_polling_status().active || crate::commands::vehicle_profiles::can_operations_busy(state, &engine)? { return Err("Start fresh requires profile standby with recording, playback, OEM polling, and CAN work stopped".into()); }
            let common = ["engine_rpm", "vehicle_speed", "coolant_temp", "intake_temp", "throttle_pos"];
            let ids = engine.connection_capabilities().ok_or("Connection capability is unavailable")?.supported_signals.iter().filter(|s| common.contains(&s.id.as_str())).map(|s| s.id.clone()).collect::<Vec<_>>();
            let selection = ProfileSelectionSnapshot { log_signal_ids: ids.clone(), graph_signal_ids: ids, ..Default::default() };
            drop(engine);
            let observation = state.vehicle_profiles.current_observation(expected_generation).ok_or("Fresh VIN identity is required before Start fresh")?;
            if observation.vin_status != lognook_core::IdentityReadState::Positive || observation.conflicting_vin_observations || profile.vin_digest != observation.vin_digest || profile.scope != observation.scope || state.vehicle_profiles.get_vehicle_project_id(observation.scope, profile.vin_digest.as_deref().ok_or("Project has no VIN")?)?.as_deref() != Some(project_id.as_str()) { return Err("Connected positive VIN does not match this canonical project".into()); }
            ensure_basic_preset(state, &profile, &selection.log_signal_ids)?;
            let workspace = VehicleProjectWorkspace { version: lognook_core::VEHICLE_PROJECT_WORKSPACE_VERSION, current_preset_id: Some(format!("project-{project_id}")), catalog_filters: serde_json::json!({}), graph_preferences: None, layout_preferences: None };
            let saved = crate::commands::vehicle_profiles::save_vehicle_profile_with_workspace_locked_shared(state, SaveVehicleProfileArgs { profile_id: Some(project_id.clone()), expected_revision: Some(expected_revision), name: profile.name, selection: selection.clone(), manual_association: false, catalog_vehicle_id: profile.catalog_vehicle_id }, Some(&workspace), Some(expected_generation))?;
            apply_generic_selection(state, &saved, expected_generation)?;
            state.vehicle_profiles.finish_project_choice(&project_id, expected_generation)?;
            Ok(VehicleProjectsResult::Fresh { state: "fresh".into(), project: view(state, saved)?, selection })
        }
    }
}

#[tauri::command]
pub fn vehicle_projects(state: State<'_, AppState>, operation: VehicleProjectsOperation) -> Result<VehicleProjectsResult, String> { execute(&state, operation) }
