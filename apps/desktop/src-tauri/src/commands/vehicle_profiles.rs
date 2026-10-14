use super::engine;
use crate::{
    AppState,
    vehicle_profiles::{
        ProfileGraphOption, ProfileResumeApplyResult, ProfileResumePreview,
        ProfileResumeSkipReason, SaveVehicleProfileArgs, SkippedProfileSelection, StoredPreview,
        VehicleProfileMatchSummary, VehicleProfileStatus, active_derived_ids,
        unknown_profile_match,
    },
};
use lognook_core::{
    DiagnosticHistoryFilter, DiagnosticOutcome, DiagnosticRead, DiagnosticReadKind,
    DiagnosticReport, DiagnosticReportDetail, DiagnosticRequest, DiagnosticValue,
    HistoryIndexState, HistoryIndexStatus, IdentityReadState, ManualDetailActor,
    ManualReportAssociation, ManualVehicleField, ProfileMatch, ProfileMatchState, ProfileScope,
    ProfileSelectionSnapshot, VehicleIdentityObservation, VehicleIdentityRecord, VehicleProfile,
    VinOrigin, canonical_vin_identity, decoder_revision_v1, derived_signal_dependencies,
    resolve_profile_match, supports_profile_identity,
};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};

pub(crate) fn list_vehicle_profiles_shared(
    state: &AppState,
) -> Result<Vec<VehicleProfile>, String> {
    state.vehicle_profiles.list()
}

pub(crate) fn get_vehicle_profile_shared(
    state: &AppState,
    profile_id: &str,
) -> Result<VehicleProfile, String> {
    state
        .vehicle_profiles
        .get(profile_id)?
        .ok_or_else(|| format!("Vehicle profile '{profile_id}' does not exist"))
}

pub(crate) fn save_vehicle_profile_shared(
    state: &AppState,
    args: SaveVehicleProfileArgs,
) -> Result<VehicleProfile, String> {
    let mut args = hydrate_oem_references(state, args)?;
    if !args.selection.can_fields.is_empty() {
        let current = engine(state)?;
        if let Some(status) = current
            .can_workbench_status()
            .filter(|status| status.connected)
        {
            args.selection.can_source_id = Some(status.source_id.clone());
            args.selection.can_bitrate = Some(status.bitrate);
            args.selection.can_channel = status.target_channel;
        } else {
            args.selection.can_source_id = None;
            args.selection.can_bitrate = None;
            args.selection.can_channel = None;
        }
    }
    let (generation, scope) = {
        let current = engine(state)?;
        if !current.is_obd_connected() {
            return Err(
                "Connect an OBD adapter before saving the current vehicle selection".into(),
            );
        }
        let capabilities = current
            .connection_capabilities()
            .ok_or_else(|| "OBD connection identity is unavailable".to_owned())?;
        (
            current.obd_connection_generation(),
            connection_scope(&capabilities.device.hardware_family),
        )
    };
    state.vehicle_profiles.save(args, generation, scope)
}

fn hydrate_oem_references(
    state: &AppState,
    mut args: SaveVehicleProfileArgs,
) -> Result<SaveVehicleProfileArgs, String> {
    let mut requested: HashSet<String> = args
        .selection
        .log_signal_ids
        .iter()
        .chain(&args.selection.graph_signal_ids)
        .filter_map(|id| id.strip_prefix("oem:"))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let supplied = args
        .selection
        .oem_references
        .iter()
        .map(|reference| (reference.candidate_id.clone(), reference.clone()))
        .collect::<HashMap<_, _>>();
    if supplied
        .keys()
        .any(|candidate_id| !requested.contains(candidate_id))
    {
        return Err(
            "OEM profile references must match a selected graph or logging candidate".into(),
        );
    }
    let catalog = lognook_core::OemCatalog::new(&state.pid_catalog_path);
    if let Some(vehicle_id) = args.catalog_vehicle_id.as_deref() {
        if !catalog
            .vehicle_profiles()?
            .iter()
            .any(|profile| profile.vehicle_id == vehicle_id)
        {
            return Err(
                "Catalog vehicle association must reference an indexed catalog vehicle".into(),
            );
        }
    }
    let mut references = Vec::with_capacity(requested.len());
    for candidate_id in requested.drain() {
        let spec = catalog.candidate_for_try(&candidate_id)?;
        let current = lognook_core::OemDefinitionReference {
            candidate_id: candidate_id.clone(),
            source_id: spec.candidate.source_id,
            source_revision: spec.candidate.source_blob_sha,
            decoder_revision: decoder_revision_v1(&spec.candidate.decoder_json),
        };
        if supplied
            .get(&candidate_id)
            .is_some_and(|provided| provided != &current)
        {
            return Err(format!(
                "OEM candidate '{candidate_id}' source or decoder revision is stale"
            ));
        }
        references.push(current);
    }
    references.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    args.selection.oem_references = references;
    Ok(args)
}

pub(crate) fn read_vehicle_identity_shared(
    state: &AppState,
    expected_generation: u64,
) -> Result<DiagnosticReport, String> {
    let (capabilities, scope) = {
        let current = engine(state)?;
        if !current.is_obd_connected() {
            return Err("Connect an OBD adapter before recognizing a vehicle".into());
        }
        if current.obd_connection_generation() != expected_generation {
            return Err("OBD connection changed before profile recognition began".into());
        }
        if current.is_recording() {
            return Err("Stop recording before recognizing the connected vehicle".into());
        }
        let capabilities = current
            .connection_capabilities()
            .cloned()
            .ok_or_else(|| "OBD connection identity is unavailable".to_owned())?;
        (
            capabilities.clone(),
            connection_scope(&capabilities.device.hardware_family),
        )
    };
    let operation = state.diagnostics.begin_operation(
        Some(capabilities.device.stable_id.clone()),
        "vehicle_identity_read",
        vec!["vin".into(), "calibration_id".into(), "ecu_name".into()],
    );
    let cancel = state.vehicle_profiles.begin_recognition()?;
    let result = (|| {
        let started = Instant::now();
        let started_at_unix_ms = now_unix_ms();
        let mut reads = Vec::new();
        for query in DiagnosticRequest::identity_queries() {
            if cancel.load(Ordering::Acquire) {
                return Err("Vehicle profile recognition was cancelled".into());
            }
            let current_engine = engine(state)?;
            if current_engine.obd_connection_generation() != expected_generation
                || !current_engine.is_obd_connected()
            {
                return Err("OBD connection changed during profile recognition".into());
            }
            let receiver = current_engine
                .queue_profile_identity_query(expected_generation, query, cancel.clone())
                .map_err(|error| error.to_string())?;
            drop(current_engine);
            let remaining = Duration::from_secs(10).saturating_sub(started.elapsed());
            if remaining.is_zero() {
                cancel.store(true, Ordering::Release);
                return Err("Vehicle profile recognition exceeded its 10-second budget".into());
            }
            let result = match receiver.recv_timeout(remaining.min(Duration::from_secs(3))) {
                Ok(result) => result.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancel.store(true, Ordering::Release);
                    return Err("Vehicle profile identity read timed out".into());
                }
            };
            reads.extend(result);
        }
        {
            let current_engine = engine(state)?;
            if current_engine.obd_connection_generation() != expected_generation
                || !current_engine.is_obd_connected()
            {
                return Err("OBD connection changed before profile recognition completed".into());
            }
        }
        let observation = identity_observation(&capabilities, expected_generation, scope, &reads);
        let mut report = DiagnosticReport {
            report_id: None,
            connection_generation: Some(expected_generation),
            vehicle_context: None,
            adapter: capabilities.device.clone(),
            protocol: capabilities.protocol.clone(),
            started_at_unix_ms,
            request: DiagnosticRequest::default(),
            reads,
            route_restore_error: None,
            evidence_saved: false,
            evidence_save_error: None,
        };
        state
            .vehicle_profiles
            .record_observation(observation.clone())?;
        let profiles = state.vehicle_profiles.list()?;
        let active_id = state
            .vehicle_profiles
            .active_profile(expected_generation)
            .map(|(id, _)| id);
        let exact = profiles
            .iter()
            .filter(|profile| {
                resolve_profile_match(profile, &observation).state == ProfileMatchState::Exact
            })
            .collect::<Vec<_>>();
        let target_profile_id = active_id
            .filter(|id| {
                profiles.iter().any(|profile| {
                    profile.id == *id
                        && resolve_profile_match(profile, &observation).state
                            != ProfileMatchState::Conflict
                })
            })
            .or_else(|| (exact.len() == 1).then(|| exact[0].id.clone()));
        if let Some(profile_id) = target_profile_id {
            state
                .vehicle_profiles
                .append_observation(&profile_id, observation.clone())?;
        }
        crate::vehicle_reads::append_vehicle_report(
            state,
            &mut report,
            scope,
            expected_generation,
            VinOrigin::EcuRecognition,
            Some(&observation),
        );
        if let Some(context) = report.vehicle_context.clone() {
            engine(state)?
                .set_vehicle_recording_context(Some(context), expected_generation)
                .map_err(|error| error.to_string())?;
        }
        Ok(report)
    })();
    state.vehicle_profiles.finish_recognition(&cancel);
    operation.finish_result(&result);
    result
}

pub(crate) fn recognize_vehicle_shared(
    state: &AppState,
    expected_generation: u64,
) -> Result<VehicleIdentityObservation, String> {
    read_vehicle_identity_shared(state, expected_generation)?;
    state
        .vehicle_profiles
        .current_observation(expected_generation)
        .ok_or_else(|| "Vehicle identity read completed without a current observation".into())
}

pub(crate) fn prepare_profile_resume_shared(
    state: &AppState,
    profile_id: &str,
    expected_generation: u64,
) -> Result<ProfileResumePreview, String> {
    let _operation = state.vehicle_profiles.lock_operation()?;
    let profile = get_vehicle_profile_shared(state, profile_id)?;
    let (capabilities, generation, standby, recording, can_status, can_busy) = {
        let current = engine(state)?;
        let generation = current.obd_connection_generation();
        if generation != expected_generation || !current.is_obd_connected() {
            return Err("OBD connection changed before the Resume preview was prepared".into());
        }
        let capabilities = current
            .connection_capabilities()
            .cloned()
            .ok_or_else(|| "OBD connection identity is unavailable".to_owned())?;
        let can_busy = can_operations_busy(state, &current)?;
        (
            capabilities,
            generation,
            current.is_obd_profile_standby(),
            current.is_recording(),
            current.can_workbench_status(),
            can_busy,
        )
    };
    let observation = state.vehicle_profiles.current_observation(generation);
    let profile_match = observation
        .as_ref()
        .map(|observation| resolve_profile_match(&profile, observation))
        .unwrap_or_else(unknown_profile_match);
    let mut preview = build_preview(
        state,
        &profile,
        profile_match,
        &capabilities,
        observation.as_ref(),
        can_status.as_ref(),
        can_busy,
        generation,
        standby,
        recording,
    )?;
    preview.preview_id = state.vehicle_profiles.make_preview_id()?;
    let active_derived_ids = active_derived_ids(
        &profile.selection,
        &preview.eligible_log_refs,
        &preview.eligible_graph_refs,
    );
    state.vehicle_profiles.store_preview(StoredPreview {
        result: preview.clone(),
        selection: profile.selection,
        active_derived_ids,
        created: Instant::now(),
    })?;
    Ok(preview)
}

pub(crate) fn apply_profile_resume_shared(
    state: &AppState,
    preview_id: &str,
    expected_revision: u64,
    expected_generation: u64,
) -> Result<ProfileResumeApplyResult, String> {
    let work_generation = state.vehicle_profiles.work_generation();
    let _operation = state.vehicle_profiles.lock_operation()?;
    let stored = state
        .vehicle_profiles
        .current_preview()
        .filter(|preview| preview.result.preview_id == preview_id)
        .ok_or_else(|| "Profile Resume preview is missing, expired, or was replaced".to_owned())?;
    if stored.result.profile_revision != expected_revision
        || stored.result.obd_connection_generation != expected_generation
    {
        return Err("Profile revision or OBD connection changed after the Resume preview".into());
    }
    let profile = get_vehicle_profile_shared(state, &stored.result.profile_id)?;
    if profile.revision != expected_revision {
        return Err("Saved profile changed after the Resume preview; prepare a new preview".into());
    }
    let (capabilities, generation, standby, recording, can_status, can_busy) = {
        let current = engine(state)?;
        if !current.is_obd_connected() || current.obd_connection_generation() != expected_generation
        {
            return Err("OBD connection changed after the Resume preview".into());
        }
        let capabilities = current
            .connection_capabilities()
            .cloned()
            .ok_or_else(|| "OBD connection identity is unavailable".to_owned())?;
        let can_busy = can_operations_busy(state, &current)?;
        (
            capabilities,
            current.obd_connection_generation(),
            current.is_obd_profile_standby(),
            current.is_recording(),
            current.can_workbench_status(),
            can_busy,
        )
    };
    let observation = state.vehicle_profiles.current_observation(generation);
    let fresh_match = observation
        .as_ref()
        .map(|observation| resolve_profile_match(&profile, observation))
        .unwrap_or_else(unknown_profile_match);
    let fresh = build_preview(
        state,
        &profile,
        fresh_match,
        &capabilities,
        observation.as_ref(),
        can_status.as_ref(),
        can_busy,
        generation,
        standby,
        recording,
    )?;
    if fresh.eligible_log_refs != stored.result.eligible_log_refs
        || fresh.eligible_graph_refs != stored.result.eligible_graph_refs
        || fresh.skipped != stored.result.skipped
        || !fresh.can_apply
    {
        return Err(
            "Current support or identity changed after the Resume preview; prepare a new preview"
                .into(),
        );
    }
    if stored.result.eligible_log_refs.is_empty() {
        return Err("No saved logging signals are eligible on this connection".into());
    }
    let eligible_derived_ids = stored.active_derived_ids.clone();
    let eligible_ids = stored
        .result
        .eligible_log_refs
        .iter()
        .chain(&stored.result.eligible_graph_refs)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let derived_definitions = stored
        .selection
        .derived_definitions
        .iter()
        .filter(|definition| eligible_ids.contains(definition.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let generic_log_refs = stored
        .result
        .eligible_log_refs
        .iter()
        .filter(|id| !id.starts_with("oem:"))
        .cloned()
        .collect::<Vec<_>>();
    let oem_ids = stored
        .result
        .eligible_log_refs
        .iter()
        .filter_map(|id| id.strip_prefix("oem:"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let oem_plan = if oem_ids.is_empty() {
        None
    } else {
        let catalog = lognook_core::OemCatalog::new(&state.pid_catalog_path);
        let vehicle_id = profile
            .catalog_vehicle_id
            .as_deref()
            .ok_or_else(|| "OEM resume lost its catalog vehicle association".to_owned())?;
        Some(
            catalog
                .oem_poll_plan(vehicle_id, &oem_ids)
                .map_err(|error| {
                    format!(
                        "Current catalog polling gate rejected the saved OEM selection: {error}"
                    )
                })?,
        )
    };
    {
        let mut current = engine(state)?;
        if current.obd_connection_generation() != expected_generation
            || !current.is_obd_profile_standby()
        {
            return Err("Resume requires the same connection in profile standby".into());
        }
        if current.is_recording() || current.is_playback_active() {
            return Err("Stop recording and playback before resuming a profile".into());
        }
        if state.vehicle_profiles.work_generation() != work_generation {
            return Err("Agent/profile operation was cancelled before Resume".into());
        }
        if current
            .can_workbench_busy()
            .map_err(|error| error.to_string())?
        {
            return Err(
                "Stop the CAN capture, scan, send, and replay before resuming a profile".into(),
            );
        }
        let can_fields = stored
            .selection
            .can_fields
            .iter()
            .filter(|field| stored.result.eligible_graph_refs.contains(&field.id))
            .cloned()
            .collect::<Vec<_>>();
        let previous_can_fields = current.configured_can_fields();
        if !can_fields.is_empty() {
            current
                .configure_can_signals(can_fields.clone())
                .map_err(|error| error.to_string())?;
        }
        if state.vehicle_profiles.work_generation() != work_generation {
            if !can_fields.is_empty() {
                current
                    .configure_can_signals(previous_can_fields)
                    .map_err(|error| error.to_string())?;
            }
            return Err(
                "Agent/profile operation was cancelled before selection replacement".into(),
            );
        }
        if let Err(error) = current.replace_profile_selection_with_oem(
            generic_log_refs,
            derived_definitions.clone(),
            eligible_derived_ids.clone(),
            oem_plan,
            expected_generation,
        ) {
            if !can_fields.is_empty() {
                if let Err(rollback) = current.configure_can_signals(previous_can_fields) {
                    return Err(format!(
                        "{error}; previous CAN fields could not be restored: {rollback}"
                    ));
                }
            }
            return Err(error.to_string());
        }
    }
    let applied_can_fields = stored
        .selection
        .can_fields
        .iter()
        .filter(|field| stored.result.eligible_graph_refs.contains(&field.id))
        .cloned()
        .collect::<Vec<_>>();
    let catalog = lognook_core::OemCatalog::new(&state.pid_catalog_path);
    let graph_options = stored
        .result
        .eligible_graph_refs
        .iter()
        .filter_map(|id| {
            let candidate_id = id.strip_prefix("oem:")?;
            let spec = catalog.candidate_for_try(candidate_id).ok()?;
            Some(ProfileGraphOption {
                id: id.clone(),
                name: spec.candidate.name,
                unit: spec.candidate.unit,
                description: spec.candidate.notes,
                research: true,
            })
        })
        .collect();
    let selection = ProfileSelectionSnapshot {
        log_signal_ids: stored.result.eligible_log_refs.clone(),
        graph_signal_ids: stored.result.eligible_graph_refs.clone(),
        derived_definitions,
        can_fields: applied_can_fields.clone(),
        can_source_id: (!applied_can_fields.is_empty())
            .then(|| stored.selection.can_source_id.clone())
            .flatten(),
        can_bitrate: (!applied_can_fields.is_empty())
            .then_some(stored.selection.can_bitrate)
            .flatten(),
        can_channel: (!applied_can_fields.is_empty())
            .then_some(stored.selection.can_channel)
            .flatten(),
        oem_references: stored
            .selection
            .oem_references
            .iter()
            .filter(|reference| {
                stored
                    .result
                    .eligible_graph_refs
                    .iter()
                    .any(|id| id == &format!("oem:{}", reference.candidate_id))
            })
            .cloned()
            .collect(),
    };
    let applied = ProfileResumeApplyResult {
        preview_id: preview_id.to_owned(),
        profile_id: profile.id.clone(),
        profile_revision: profile.revision,
        obd_connection_generation: expected_generation,
        applied_log_refs: stored.result.eligible_log_refs.clone(),
        eligible_graph_refs: stored.result.eligible_graph_refs.clone(),
        skipped: stored.result.skipped.clone(),
        selection,
        graph_options,
    };
    state.vehicle_profiles.discard_preview(preview_id)?;
    state
        .vehicle_profiles
        .set_active_profile(profile.id, profile.revision, expected_generation)?;
    Ok(applied)
}

pub(crate) fn discard_profile_resume_shared(
    state: &AppState,
    preview_id: &str,
) -> Result<(), String> {
    state.vehicle_profiles.discard_preview(preview_id)
}

pub(crate) fn vehicle_profiles_status_shared(
    state: &AppState,
) -> Result<VehicleProfileStatus, String> {
    let (connected, standby, generation, capabilities) = {
        let current = engine(state)?;
        (
            current.is_obd_connected(),
            current.is_obd_profile_standby(),
            current.obd_connection_generation(),
            current.connection_capabilities().cloned(),
        )
    };
    let observation = connected
        .then(|| state.vehicle_profiles.current_observation(generation))
        .flatten();
    let mut matches = Vec::new();
    if let Some(observation) = observation.as_ref() {
        for profile in state.vehicle_profiles.list()? {
            matches.push(VehicleProfileMatchSummary {
                profile_id: profile.id.clone(),
                name: profile.name.clone(),
                revision: profile.revision,
                profile_match: resolve_profile_match(&profile, observation),
            });
        }
        matches.sort_by_key(|summary| match summary.profile_match.state {
            ProfileMatchState::Exact => 0,
            ProfileMatchState::ReviewRequired => 1,
            ProfileMatchState::IdentityUnavailable => 2,
            ProfileMatchState::Conflict => 3,
        });
    } else if capabilities.is_some() {
        matches = state
            .vehicle_profiles
            .list()?
            .into_iter()
            .map(|profile| VehicleProfileMatchSummary {
                profile_id: profile.id,
                name: profile.name,
                revision: profile.revision,
                profile_match: unknown_profile_match(),
            })
            .collect();
    }
    let active = state.vehicle_profiles.active_profile(generation);
    Ok(VehicleProfileStatus {
        obd_connected: connected,
        profile_standby: standby,
        obd_connection_generation: generation,
        current_observation: observation,
        profile_matches: matches,
        active_profile_id: active.as_ref().map(|(id, _)| id.clone()),
        active_profile_revision: active.map(|(_, revision)| revision),
        preview: state
            .vehicle_profiles
            .current_preview()
            .map(|preview| preview.result),
    })
}

fn build_preview(
    state: &AppState,
    profile: &VehicleProfile,
    profile_match: ProfileMatch,
    capabilities: &lognook_core::ConnectionCapabilities,
    observation: Option<&VehicleIdentityObservation>,
    can_status: Option<&lognook_core::CanWorkbenchStatus>,
    can_busy: bool,
    generation: u64,
    standby: bool,
    recording: bool,
) -> Result<ProfileResumePreview, String> {
    let supported = capabilities
        .supported_signals
        .iter()
        .map(|signal| signal.id.as_str())
        .collect::<HashSet<_>>();
    let dependencies = derived_signal_dependencies(&profile.selection.derived_definitions)
        .map_err(|error| format!("Saved derived definitions are invalid: {error}"))?;
    let definitions = profile
        .selection
        .derived_definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let can_fields = profile
        .selection
        .can_fields
        .iter()
        .map(|field| (field.id.as_str(), field))
        .collect::<HashMap<_, _>>();
    let can_context_matches = profile
        .selection
        .can_source_id
        .as_deref()
        .zip(profile.selection.can_bitrate)
        .zip(profile.selection.can_channel)
        .zip(can_status)
        .is_some_and(|(((source_id, bitrate), channel), status)| {
            status.connected
                && status.source_id == source_id
                && status.bitrate == bitrate
                && status.target_channel == Some(channel)
        });
    let oem_refs = profile
        .selection
        .oem_references
        .iter()
        .map(|reference| (format!("oem:{}", reference.candidate_id), reference))
        .collect::<HashMap<_, _>>();
    let catalog = lognook_core::OemCatalog::new(&state.pid_catalog_path);
    let mut eligible_log = Vec::new();
    let mut eligible_graph = Vec::new();
    let mut skipped = Vec::new();
    let mut dependency_inputs = HashSet::new();
    let conflict = profile_match.state == ProfileMatchState::Conflict;
    let oem_log_eligibility =
        oem_log_eligibility(profile, &profile_match, capabilities, observation, &catalog);

    for id in &profile.selection.log_signal_ids {
        if conflict {
            skipped.push(skip(
                id,
                "log",
                ProfileResumeSkipReason::IdentityConflict,
                "Vehicle or ECU identity conflicts with this profile",
            ));
        } else if can_fields.contains_key(id.as_str()) {
            skipped.push(skip(
                id,
                "log",
                ProfileResumeSkipReason::CanSourceUnavailable,
                "CAN fields remain graph-only and cannot enter the OBD poll queue",
            ));
        } else if id.starts_with("oem:") {
            let candidate_id = id.trim_start_matches("oem:");
            match oem_log_eligibility.get(candidate_id) {
                Some(None) => eligible_log.push(id.clone()),
                Some(Some(detail)) => skipped.push(skip(
                    id,
                    "log",
                    ProfileResumeSkipReason::OemExactCatalogValidationRequired,
                    detail,
                )),
                None => skipped.push(skip(
                    id,
                    "log",
                    ProfileResumeSkipReason::StaleDefinition,
                    "OEM source and decoder provenance is missing",
                )),
            }
        } else if let Some(inputs) = dependencies.get(id) {
            if let Some(missing) = inputs
                .iter()
                .find(|input| !supported.contains(input.as_str()))
            {
                skipped.push(skip(
                    id,
                    "log",
                    ProfileResumeSkipReason::MissingDependency,
                    &format!("Derived signal requires unsupported input '{missing}'"),
                ));
            } else {
                eligible_log.push(id.clone());
                dependency_inputs.extend(inputs.iter().cloned());
            }
        } else if definitions.contains_key(id.as_str()) {
            skipped.push(skip(
                id,
                "log",
                ProfileResumeSkipReason::StaleDefinition,
                "Saved derived definition could not be resolved",
            ));
        } else if supported.contains(id.as_str()) {
            eligible_log.push(id.clone());
        } else {
            skipped.push(skip(
                id,
                "log",
                ProfileResumeSkipReason::UnsupportedSignal,
                "Signal is not supported on the current connection",
            ));
        }
    }

    for id in &profile.selection.graph_signal_ids {
        if conflict {
            skipped.push(skip(
                id,
                "graph",
                ProfileResumeSkipReason::IdentityConflict,
                "Vehicle or ECU identity conflicts with this profile",
            ));
        } else if can_fields.contains_key(id.as_str()) {
            if can_context_matches && !can_busy {
                eligible_graph.push(id.clone());
            } else {
                skipped.push(skip(
                    id,
                    "graph",
                    ProfileResumeSkipReason::CanSourceUnavailable,
                    if can_busy {
                        "Stop CAN scans, replay, and capture before resuming CAN fields"
                    } else {
                        "The saved CAN source and bitrate are not currently connected"
                    },
                ));
            }
        } else if let Some(reference) = oem_refs.get(id) {
            match catalog.candidate_for_try(&reference.candidate_id) {
                Ok(spec)
                    if spec.candidate.source_id == reference.source_id
                        && spec.candidate.source_blob_sha == reference.source_revision
                        && decoder_revision_v1(&spec.candidate.decoder_json)
                            == reference.decoder_revision =>
                {
                    eligible_graph.push(id.clone());
                }
                Ok(_) => skipped.push(skip(
                    id,
                    "graph",
                    ProfileResumeSkipReason::StaleDefinition,
                    "Saved OEM source or decoder revision no longer matches the research catalog",
                )),
                Err(_) => skipped.push(skip(
                    id,
                    "graph",
                    ProfileResumeSkipReason::StaleDefinition,
                    "Saved OEM candidate is unavailable in the current research catalog",
                )),
            }
        } else if let Some(inputs) = dependencies.get(id) {
            if let Some(missing) = inputs
                .iter()
                .find(|input| !supported.contains(input.as_str()))
            {
                skipped.push(skip(
                    id,
                    "graph",
                    ProfileResumeSkipReason::MissingDependency,
                    &format!("Derived graph requires unsupported input '{missing}'"),
                ));
            } else {
                eligible_graph.push(id.clone());
                dependency_inputs.extend(inputs.iter().cloned());
            }
        } else if definitions.contains_key(id.as_str()) {
            skipped.push(skip(
                id,
                "graph",
                ProfileResumeSkipReason::StaleDefinition,
                "Saved derived definition could not be resolved",
            ));
        } else if supported.contains(id.as_str()) {
            eligible_graph.push(id.clone());
        } else {
            skipped.push(skip(
                id,
                "graph",
                ProfileResumeSkipReason::UnsupportedSignal,
                "Signal is not supported on the current connection",
            ));
        }
    }
    for input in dependency_inputs {
        if !eligible_log.contains(&input) {
            eligible_log.push(input);
        }
    }

    let eligible_oem = eligible_log
        .iter()
        .filter_map(|id| id.strip_prefix("oem:"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if eligible_oem.len() > 64 {
        for candidate_id in eligible_oem.iter().skip(64) {
            eligible_log.retain(|id| id != &format!("oem:{candidate_id}"));
            skipped.push(skip(
                &format!("oem:{candidate_id}"),
                "log",
                ProfileResumeSkipReason::OemExactCatalogValidationRequired,
                "The automatic OEM plan is limited to 64 candidates",
            ));
        }
    }
    if !eligible_oem.is_empty() {
        if let Some(vehicle_id) = profile.catalog_vehicle_id.as_deref() {
            if let Err(error) =
                catalog.oem_poll_plan(vehicle_id, &eligible_oem[..eligible_oem.len().min(64)])
            {
                for candidate_id in eligible_oem.iter().take(64) {
                    eligible_log.retain(|id| id != &format!("oem:{candidate_id}"));
                    skipped.push(skip(
                        &format!("oem:{candidate_id}"),
                        "log",
                        ProfileResumeSkipReason::OemExactCatalogValidationRequired,
                        &format!("Current catalog polling gate rejected this candidate: {error}"),
                    ));
                }
            }
        }
    }

    if !standby {
        skipped.push(skip(
            "profile",
            "log",
            ProfileResumeSkipReason::ReconnectForStandby,
            "Reconnect with profile standby enabled before applying a saved selection",
        ));
    }
    let can_apply = standby && !recording && !conflict && !eligible_log.is_empty();
    Ok(ProfileResumePreview {
        preview_id: String::new(),
        profile_id: profile.id.clone(),
        profile_revision: profile.revision,
        obd_connection_generation: generation,
        profile_match,
        eligible_log_refs: eligible_log,
        eligible_graph_refs: eligible_graph,
        skipped,
        can_apply,
        manual_association_available: profile_match_state_requires_manual(observation),
    })
}

fn can_operations_busy(
    state: &AppState,
    engine: &lognook_core::LogNookEngine,
) -> Result<bool, String> {
    let scan = state.agent_scan_active.load(Ordering::Acquire);
    let replay = state
        .agent_can_replay_status
        .lock()
        .map_err(|_| "CAN replay status is unavailable".to_owned())?
        .as_ref()
        .is_some_and(|status| status.state == lognook_core::CanIdReplayState::Running);
    Ok(scan
        || replay
        || engine
            .can_workbench_busy()
            .map_err(|error| error.to_string())?)
}

fn oem_log_eligibility(
    profile: &VehicleProfile,
    profile_match: &ProfileMatch,
    capabilities: &lognook_core::ConnectionCapabilities,
    observation: Option<&VehicleIdentityObservation>,
    catalog: &lognook_core::OemCatalog,
) -> HashMap<String, Option<String>> {
    let selected = profile
        .selection
        .log_signal_ids
        .iter()
        .filter_map(|id| id.strip_prefix("oem:"))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let references = profile
        .selection
        .oem_references
        .iter()
        .map(|reference| (reference.candidate_id.as_str(), reference))
        .collect::<HashMap<_, _>>();
    selected
        .into_iter()
        .map(|candidate_id| {
            let fail = |detail: &str| (candidate_id.clone(), Some(detail.to_owned()));
            if profile.scope != ProfileScope::Live || profile_match.state != ProfileMatchState::Exact {
                return fail("Continuous OEM reads require an exact live vehicle and ECU profile match");
            }
            let Some(vehicle_id) = profile.catalog_vehicle_id.as_deref() else {
                return fail("Select an explicit research-catalog vehicle association before polling OEM candidates");
            };
            let Some(reference) = references.get(candidate_id.as_str()) else {
                return fail("OEM source and decoder provenance is missing");
            };
            let spec = match catalog.candidate_for_try(&candidate_id) {
                Ok(spec) => spec,
                Err(_) => return fail("OEM candidate is unavailable in the current catalog"),
            };
            if spec.candidate.source_id != reference.source_id
                || spec.candidate.source_blob_sha != reference.source_revision
                || decoder_revision_v1(&spec.candidate.decoder_json) != reference.decoder_revision
            {
                return fail("OEM source or decoder revision changed since the profile was saved");
            }
            let route_matches = profile.ecu_variants.iter().any(|saved| {
                let Some(request_address) = saved.request_address.as_deref() else {
                    return false;
                };
                if saved.protocol != capabilities.protocol
                    || !request_address.eq_ignore_ascii_case(spec.candidate.request_header.trim())
                    || !saved
                        .response_address
                        .eq_ignore_ascii_case(spec.candidate.response_header.trim())
                {
                    return false;
                }
                observation.is_some_and(|observation| {
                    observation.ecu_variants.iter().any(|current| {
                        current.protocol == saved.protocol
                            && current.request_route == saved.request_route
                            && current.request_address.as_deref() == Some(request_address)
                            && current.response_address == saved.response_address
                    })
                })
            });
            if !route_matches {
                return fail("A verified physical request and responding ECU route is not available");
            }
            match catalog.oem_poll_plan(vehicle_id, &[candidate_id.clone()]) {
                Ok(_) => (candidate_id, None),
                Err(error) => fail(&format!("Current catalog polling gate rejected this candidate: {error}")),
            }
        })
        .collect()
}

fn profile_match_state_requires_manual(observation: Option<&VehicleIdentityObservation>) -> bool {
    observation.is_none_or(|observation| {
        observation.vin_digest.is_none() || observation.conflicting_vin_observations
    })
}

fn skip(
    reference: &str,
    surface: &str,
    reason: ProfileResumeSkipReason,
    detail: &str,
) -> SkippedProfileSelection {
    SkippedProfileSelection {
        reference: reference.into(),
        surface: surface.into(),
        reason,
        detail: detail.into(),
    }
}

pub(crate) fn identity_observation(
    capabilities: &lognook_core::ConnectionCapabilities,
    generation: u64,
    scope: ProfileScope,
    reads: &[DiagnosticRead],
) -> VehicleIdentityObservation {
    let identity_supported = supports_profile_identity(Some(&capabilities.device.hardware_family));
    let mut vin_values = HashSet::new();
    let mut vin_state = if identity_supported {
        IdentityReadState::NotAttempted
    } else {
        IdentityReadState::Unsupported
    };
    let mut ecus: HashMap<String, lognook_core::EcuProfileVariant> = HashMap::new();
    for read in reads {
        if read.kind == DiagnosticReadKind::Vin {
            vin_state = identity_state(read);
        }
        let Some(address) = read.ecu_id.as_ref() else {
            continue;
        };
        let ecu = ecus.entry(address.to_ascii_uppercase()).or_insert_with(|| {
            lognook_core::EcuProfileVariant {
                protocol: capabilities.protocol.clone(),
                request_route: read.route.clone(),
                request_address: None,
                response_address: address.to_ascii_uppercase(),
                ecu_name: None,
                calibration_ids: vec![],
                software_ids: vec![],
                ecu_name_state: IdentityReadState::NotAttempted,
                calibration_state: IdentityReadState::NotAttempted,
                software_state: IdentityReadState::NotAttempted,
            }
        });
        match read.kind {
            DiagnosticReadKind::Vin => {
                if read.outcome == DiagnosticOutcome::Positive {
                    for value in &read.values {
                        if let DiagnosticValue::Identity {
                            pid: 0x02, value, ..
                        } = value
                        {
                            vin_values.insert(value.clone());
                        }
                    }
                }
            }
            DiagnosticReadKind::CalibrationId => {
                ecu.calibration_state = identity_state(read);
                if read.outcome == DiagnosticOutcome::Positive {
                    for value in &read.values {
                        if let DiagnosticValue::Identity {
                            pid: 0x04, value, ..
                        } = value
                        {
                            ecu.calibration_ids.push(value.clone());
                        }
                    }
                }
            }
            DiagnosticReadKind::EcuName => {
                ecu.ecu_name_state = identity_state(read);
                if read.outcome == DiagnosticOutcome::Positive {
                    ecu.ecu_name = read.values.iter().find_map(|value| match value {
                        DiagnosticValue::Identity {
                            pid: 0x0A, value, ..
                        } => Some(value.clone()),
                        _ => None,
                    });
                }
            }
            _ => {}
        }
    }
    for ecu in ecus.values_mut() {
        ecu.calibration_ids.sort();
        ecu.calibration_ids.dedup();
    }
    let conflicting_vin_observations = vin_values.len() > 1;
    let valid_vin = if conflicting_vin_observations {
        None
    } else {
        vin_values.into_iter().next()
    };
    let identity = valid_vin
        .as_deref()
        .and_then(|vin| canonical_vin_identity(vin).ok());
    if identity.is_some() {
        vin_state = IdentityReadState::Positive;
    }
    if valid_vin.is_some() && identity.is_none() {
        vin_state = IdentityReadState::Malformed;
    }
    if conflicting_vin_observations {
        vin_state = IdentityReadState::Malformed;
    }
    VehicleIdentityObservation {
        obd_connection_generation: generation,
        observed_at_unix_ms: now_unix_ms(),
        scope,
        vin_digest: identity.as_ref().map(|(digest, _)| digest.clone()),
        masked_vin: identity.map(|(_, masked)| masked),
        vin_status: vin_state,
        conflicting_vin_observations,
        ecu_variants: ecus.into_values().collect(),
        identity_supported,
    }
}

fn identity_state(read: &DiagnosticRead) -> IdentityReadState {
    match read.outcome {
        DiagnosticOutcome::Positive => IdentityReadState::Positive,
        DiagnosticOutcome::NoResponse
        | DiagnosticOutcome::ResponsePending
        | DiagnosticOutcome::TemporarilyUnavailable => IdentityReadState::NoResponse,
        DiagnosticOutcome::MalformedResponse | DiagnosticOutcome::WrongResponder => {
            IdentityReadState::Malformed
        }
        DiagnosticOutcome::TransportError => IdentityReadState::TransportError,
        DiagnosticOutcome::NegativeResponse
            if matches!(read.negative_response_code, Some(0x11 | 0x12)) =>
        {
            IdentityReadState::Unsupported
        }
        DiagnosticOutcome::NegativeResponse
        | DiagnosticOutcome::SessionRequired
        | DiagnosticOutcome::SecurityRequired => IdentityReadState::NoResponse,
    }
}

pub(crate) fn connection_scope(hardware_family: &str) -> ProfileScope {
    if hardware_family.to_ascii_lowercase().contains("simulator") {
        ProfileScope::Simulator
    } else {
        ProfileScope::Live
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod task3_identity_tests {
    use super::*;
    use lognook_core::{ConnectionCapabilities, DeviceDescriptor, SignalDefinition};

    #[test]
    fn malformed_vin_read_values_are_not_identity_evidence() {
        let capabilities = ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "fixture".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: None,
            },
            protocol: "ISO15765-4 CAN".into(),
            read_only: true,
            supported_signals: Vec::<SignalDefinition>::new(),
        };
        let mut read = DiagnosticRequest::identity_queries()
            .pop_front()
            .unwrap()
            .report(
                Some("7E8".into()),
                vec![0x49, 0x02, 0x01],
                "7E8 49 02 01".into(),
                "0902".into(),
                10,
                1,
                false,
            );
        read.outcome = DiagnosticOutcome::MalformedResponse;
        read.values.push(DiagnosticValue::Identity {
            pid: 0x02,
            label: "VIN".into(),
            value: "1FAHP2E89BG123456".into(),
        });

        let observation = identity_observation(&capabilities, 7, ProfileScope::Live, &[read]);
        assert_eq!(observation.vin_digest, None);
        assert_eq!(observation.vin_status, IdentityReadState::Malformed);
    }

    #[test]
    fn a_positive_vin_remains_positive_when_another_ecu_does_not_respond() {
        let capabilities = ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "fixture".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: None,
            },
            protocol: "ISO15765-4 CAN".into(),
            read_only: true,
            supported_signals: Vec::<SignalDefinition>::new(),
        };
        let query = DiagnosticRequest::identity_queries().pop_front().unwrap();
        let mut payload = vec![0x49, 0x02, 0x01];
        payload.extend_from_slice(b"1FAHP2E89BG123456");
        let positive = query.report(
            Some("7E8".into()),
            payload,
            "7E8 49 02 01 ...".into(),
            "0902".into(),
            10,
            1,
            false,
        );
        let negative = query.report(
            Some("7E9".into()),
            vec![0x7F, 0x09, 0x12],
            "7E9 7F 09 12".into(),
            "0902".into(),
            11,
            1,
            false,
        );

        let observation =
            identity_observation(&capabilities, 7, ProfileScope::Live, &[positive, negative]);
        assert!(observation.vin_digest.is_some());
        assert_eq!(observation.vin_status, IdentityReadState::Positive);
        assert!(!observation.conflicting_vin_observations);
        assert_eq!(observation.ecu_variants.len(), 2);
    }
}

#[tauri::command]
pub fn list_vehicle_profiles(state: State<'_, AppState>) -> Result<Vec<VehicleProfile>, String> {
    list_vehicle_profiles_shared(&state)
}

#[tauri::command]
pub fn get_vehicle_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<VehicleProfile, String> {
    get_vehicle_profile_shared(&state, &profile_id)
}

#[tauri::command]
pub fn save_vehicle_profile(
    state: State<'_, AppState>,
    args: SaveVehicleProfileArgs,
) -> Result<VehicleProfile, String> {
    save_vehicle_profile_shared(&state, args)
}

#[tauri::command]
pub async fn recognize_vehicle(
    app: tauri::AppHandle,
    obd_connection_generation: u64,
) -> Result<VehicleIdentityObservation, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        recognize_vehicle_shared(&state, obd_connection_generation)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn prepare_profile_resume(
    state: State<'_, AppState>,
    profile_id: String,
    obd_connection_generation: u64,
) -> Result<ProfileResumePreview, String> {
    prepare_profile_resume_shared(&state, &profile_id, obd_connection_generation)
}

#[tauri::command]
pub fn apply_profile_resume(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    preview_id: String,
    profile_revision: u64,
    obd_connection_generation: u64,
) -> Result<ProfileResumeApplyResult, String> {
    let result = apply_profile_resume_shared(
        &state,
        &preview_id,
        profile_revision,
        obd_connection_generation,
    )?;
    crate::vehicle_reads::sync_active_recording_context(&state, obd_connection_generation)?;
    crate::agent::tools::sync_profile_agent_selection(&state, &result)?;
    app.emit("vehicle-profile-resumed", &result)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn discard_profile_resume(
    state: State<'_, AppState>,
    preview_id: String,
) -> Result<(), String> {
    discard_profile_resume_shared(&state, &preview_id)
}

#[tauri::command]
pub fn vehicle_profiles_status(state: State<'_, AppState>) -> Result<VehicleProfileStatus, String> {
    vehicle_profiles_status_shared(&state)
}

#[tauri::command]
pub async fn read_vehicle_identity(
    app: tauri::AppHandle,
    obd_connection_generation: u64,
) -> Result<DiagnosticReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        read_vehicle_identity_shared(&state, obd_connection_generation)
    })
    .await
    .map_err(|error| format!("Vehicle identity worker failed: {error}"))?
}

#[tauri::command]
pub fn get_vehicle_identity(
    state: State<'_, AppState>,
    scope: ProfileScope,
    vin_digest: String,
) -> Result<Option<VehicleIdentityRecord>, String> {
    state
        .vehicle_profiles
        .get_vehicle_identity(scope, &vin_digest)
}

#[tauri::command]
pub fn decode_vehicle_vin_offline(
    state: State<'_, AppState>,
    scope: ProfileScope,
    vin_digest: String,
) -> Result<VehicleIdentityRecord, String> {
    let record = state
        .vehicle_profiles
        .decode_vehicle_identity(scope, &vin_digest)?;
    let generation = engine(&state)?.obd_connection_generation();
    crate::vehicle_reads::sync_active_recording_context(&state, generation)?;
    Ok(record)
}

#[tauri::command]
pub fn set_manual_vehicle_detail(
    state: State<'_, AppState>,
    scope: ProfileScope,
    vin_digest: String,
    field: ManualVehicleField,
    value: String,
    note: Option<String>,
) -> Result<VehicleIdentityRecord, String> {
    let record = state.vehicle_profiles.set_manual_vehicle_detail(
        scope,
        &vin_digest,
        field,
        value,
        ManualDetailActor::OwnerUi,
        note,
    )?;
    let generation = engine(&state)?.obd_connection_generation();
    crate::vehicle_reads::sync_active_recording_context(&state, generation)?;
    Ok(record)
}

#[tauri::command]
pub fn link_legacy_vehicle_vin(
    state: State<'_, AppState>,
    profile_id: String,
    expected_revision: u64,
    full_vin: String,
) -> Result<VehicleIdentityRecord, String> {
    let record = state.vehicle_profiles.link_legacy_vehicle_vin(
        &profile_id,
        expected_revision,
        &full_vin,
        ManualDetailActor::OwnerUi,
    )?;
    let generation = engine(&state)?.obd_connection_generation();
    crate::vehicle_reads::sync_active_recording_context(&state, generation)?;
    Ok(record)
}

#[tauri::command]
pub fn assign_manual_profile_vin(
    state: State<'_, AppState>,
    profile_id: String,
    expected_revision: u64,
    full_vin: String,
) -> Result<VehicleIdentityRecord, String> {
    let record = state.vehicle_profiles.assign_manual_profile_vin(
        &profile_id,
        expected_revision,
        &full_vin,
        ManualDetailActor::OwnerUi,
    )?;
    let generation = engine(&state)?.obd_connection_generation();
    crate::vehicle_reads::sync_active_recording_context(&state, generation)?;
    Ok(record)
}

#[tauri::command]
pub fn list_vehicle_diagnostic_history(
    state: State<'_, AppState>,
    filter: DiagnosticHistoryFilter,
) -> Result<lognook_core::DiagnosticHistoryPage, String> {
    state.vehicle_profiles.list_diagnostic_history(filter)
}

#[tauri::command]
pub fn get_vehicle_diagnostic_report(
    state: State<'_, AppState>,
    report_id: String,
) -> Result<DiagnosticReportDetail, String> {
    crate::vehicle_reads::read_report_detail(
        &state.vehicle_profiles,
        &crate::vehicle_reads::vehicle_reads_path(&state),
        &report_id,
    )
}

#[tauri::command]
pub fn associate_vehicle_diagnostic_report(
    state: State<'_, AppState>,
    report_id: String,
    scope: ProfileScope,
    vin_digest: String,
    note: Option<String>,
) -> Result<ManualReportAssociation, String> {
    state.vehicle_profiles.append_manual_report_association(
        report_id,
        scope,
        vin_digest,
        ManualDetailActor::OwnerUi,
        note,
    )
}

#[tauri::command]
pub fn vehicle_history_import_status(
    state: State<'_, AppState>,
) -> Result<HistoryIndexStatus, String> {
    state
        .vehicle_profiles
        .get_history_import_checkpoint()
        .map(|checkpoint| checkpoint.status)
}

#[tauri::command]
pub async fn start_vehicle_history_import(
    app: tauri::AppHandle,
) -> Result<HistoryIndexStatus, String> {
    let state = app.state::<AppState>();
    let mut checkpoint = state.vehicle_profiles.get_history_import_checkpoint()?;
    let cancel = state.vehicle_profiles.begin_history_import()?;
    checkpoint.status.state = HistoryIndexState::Indexing;
    checkpoint.status.error = None;
    if let Err(error) = state
        .vehicle_profiles
        .save_history_import_checkpoint(checkpoint)
    {
        state.vehicle_profiles.finish_history_import(&cancel);
        return Err(error);
    }
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let worker_state = worker_app.state::<AppState>();
        let result = crate::vehicle_reads::import_vehicle_history(&worker_state, &cancel);
        if let Err(error) = result {
            if let Ok(mut checkpoint) = worker_state
                .vehicle_profiles
                .get_history_import_checkpoint()
            {
                checkpoint.status.state = HistoryIndexState::Failed;
                checkpoint.status.error = Some(error);
                let _ = worker_state
                    .vehicle_profiles
                    .save_history_import_checkpoint(checkpoint);
            }
        }
        worker_state.vehicle_profiles.finish_history_import(&cancel);
    });
    Ok(state
        .vehicle_profiles
        .get_history_import_checkpoint()?
        .status)
}

#[tauri::command]
pub fn cancel_vehicle_history_import(
    state: State<'_, AppState>,
) -> Result<HistoryIndexStatus, String> {
    state.vehicle_profiles.cancel_history_import()?;
    state
        .vehicle_profiles
        .get_history_import_checkpoint()
        .map(|checkpoint| checkpoint.status)
}
