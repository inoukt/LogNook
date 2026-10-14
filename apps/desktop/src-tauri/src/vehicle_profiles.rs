use lognook_core::{
    DiagnosticHistoryFilter, DiagnosticHistoryPage, DiagnosticReportLocator,
    HistoryImportCheckpoint, IdentityReadState, ManualDetailActor, ManualReportAssociation,
    ManualVehicleField, ProfileMatch, ProfileMatchState, ProfileScope, ProfileSelectionSnapshot,
    VehicleIdentityObservation, VehicleIdentityRecord, VehicleProfile, VehicleProfileStore,
    canonical_vin_identity, decode_vin_offline,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const PREVIEW_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveVehicleProfileArgs {
    pub profile_id: Option<String>,
    pub expected_revision: Option<u64>,
    pub name: String,
    pub selection: ProfileSelectionSnapshot,
    pub manual_association: bool,
    pub catalog_vehicle_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileResumeSkipReason {
    UnsupportedSignal,
    MissingDependency,
    OemExactCatalogValidationRequired,
    CanSourceUnavailable,
    UnknownReference,
    IdentityConflict,
    ReconnectForStandby,
    StaleDefinition,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SkippedProfileSelection {
    pub reference: String,
    pub surface: String,
    pub reason: ProfileResumeSkipReason,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct ProfileResumePreview {
    pub preview_id: String,
    pub profile_id: String,
    pub profile_revision: u64,
    pub obd_connection_generation: u64,
    pub profile_match: ProfileMatch,
    pub eligible_log_refs: Vec<String>,
    pub eligible_graph_refs: Vec<String>,
    pub skipped: Vec<SkippedProfileSelection>,
    pub can_apply: bool,
    pub manual_association_available: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct ProfileGraphOption {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub description: String,
    pub research: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct ProfileResumeApplyResult {
    pub preview_id: String,
    pub profile_id: String,
    pub profile_revision: u64,
    pub obd_connection_generation: u64,
    pub applied_log_refs: Vec<String>,
    pub eligible_graph_refs: Vec<String>,
    pub skipped: Vec<SkippedProfileSelection>,
    pub selection: ProfileSelectionSnapshot,
    pub graph_options: Vec<ProfileGraphOption>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct VehicleProfileMatchSummary {
    pub profile_id: String,
    pub name: String,
    pub revision: u64,
    pub profile_match: ProfileMatch,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
pub struct VehicleProfileStatus {
    pub obd_connected: bool,
    pub profile_standby: bool,
    pub obd_connection_generation: u64,
    pub current_observation: Option<VehicleIdentityObservation>,
    pub profile_matches: Vec<VehicleProfileMatchSummary>,
    pub active_profile_id: Option<String>,
    pub active_profile_revision: Option<u64>,
    pub preview: Option<ProfileResumePreview>,
}

pub(crate) struct VehicleProfileService {
    store: Option<Mutex<VehicleProfileStore>>,
    store_error: Option<String>,
    profile_operation: Mutex<()>,
    observation: Mutex<Option<VehicleIdentityObservation>>,
    preview: Mutex<Option<StoredPreview>>,
    active_profile: Mutex<Option<(String, u64, u64)>>,
    recognition_cancel: Mutex<Option<Arc<AtomicBool>>>,
    history_import_cancel: Mutex<Option<Arc<AtomicBool>>>,
    work_generation: AtomicU64,
    next_preview_id: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct StoredPreview {
    pub result: ProfileResumePreview,
    pub selection: ProfileSelectionSnapshot,
    pub active_derived_ids: Vec<String>,
    pub created: Instant,
}

impl VehicleProfileService {
    pub(crate) fn open(path: &Path) -> Self {
        let (store, store_error) = match VehicleProfileStore::open(path) {
            Ok(store) => (Some(Mutex::new(store)), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            store,
            store_error,
            profile_operation: Mutex::new(()),
            observation: Mutex::new(None),
            preview: Mutex::new(None),
            active_profile: Mutex::new(None),
            recognition_cancel: Mutex::new(None),
            history_import_cancel: Mutex::new(None),
            work_generation: AtomicU64::new(0),
            next_preview_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn list(&self) -> Result<Vec<VehicleProfile>, String> {
        self.store()?.list().map_err(|error| error.to_string())
    }

    pub(crate) fn get(&self, profile_id: &str) -> Result<Option<VehicleProfile>, String> {
        self.store()?
            .get(profile_id)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn get_vehicle_identity(
        &self,
        scope: ProfileScope,
        vin_digest: &str,
    ) -> Result<Option<VehicleIdentityRecord>, String> {
        self.store()?
            .get_vehicle_identity(scope, vin_digest)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn save_vehicle_identity(
        &self,
        record: VehicleIdentityRecord,
        expected_revision: Option<u64>,
    ) -> Result<VehicleIdentityRecord, String> {
        let _operation = self.lock_operation()?;
        let result = self
            .store()?
            .save_vehicle_identity(record, expected_revision)
            .map_err(|error| error.to_string())?;
        self.invalidate_preview()?;
        Ok(result)
    }

    pub(crate) fn decode_vehicle_identity(
        &self,
        scope: ProfileScope,
        vin_digest: &str,
    ) -> Result<VehicleIdentityRecord, String> {
        let _operation = self.lock_operation()?;
        let mut store = self.store()?;
        let mut record = store
            .get_vehicle_identity(scope, vin_digest)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Vehicle identity was not found".to_owned())?;
        let full_vin = record.full_vin.as_deref().ok_or_else(|| {
            "This profile only has a legacy VIN digest; enter the full VIN to decode it offline"
                .to_owned()
        })?;
        record.decoder_snapshot = Some(
            decode_vin_offline(full_vin)
                .map_err(|error| format!("The stored VIN is invalid: {error}"))?,
        );
        let expected_revision = record.revision;
        let saved = store
            .save_vehicle_identity(record, Some(expected_revision))
            .map_err(|error| error.to_string())?;
        drop(store);
        self.invalidate_preview()?;
        Ok(saved)
    }

    pub(crate) fn link_legacy_vehicle_vin(
        &self,
        profile_id: &str,
        expected_revision: u64,
        full_vin: &str,
        actor: ManualDetailActor,
    ) -> Result<VehicleIdentityRecord, String> {
        let _operation = self.lock_operation()?;
        let result = self
            .store()?
            .link_legacy_vehicle_vin(profile_id, expected_revision, full_vin, actor)
            .map_err(|error| error.to_string())?;
        self.invalidate_preview()?;
        Ok(result)
    }

    pub(crate) fn assign_manual_profile_vin(
        &self,
        profile_id: &str,
        expected_revision: u64,
        full_vin: &str,
        actor: ManualDetailActor,
    ) -> Result<VehicleIdentityRecord, String> {
        let _operation = self.lock_operation()?;
        let (digest, _) =
            canonical_vin_identity(full_vin).map_err(|error| format!("Invalid VIN: {error}"))?;
        let profile_scope = self
            .store()?
            .get(profile_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Vehicle profile '{profile_id}' does not exist"))?
            .scope;
        let active = self
            .active_profile
            .lock()
            .map_err(|_| "Active profile state is unavailable".to_owned())?
            .clone();
        let active_target = active
            .as_ref()
            .filter(|(active_id, _, _)| active_id == profile_id)
            .cloned();
        if let Some((_, _, generation)) = &active_target
            && let Some(observation) = self.current_observation(*generation)
            && (observation.scope != profile_scope
                || observation.conflicting_vin_observations
                || observation
                    .vin_digest
                    .as_deref()
                    .is_some_and(|observed| observed != digest))
        {
            return Err("The active connection has conflicting ECU VIN evidence".into());
        }

        let mut store = self.store()?;
        let result = store
            .assign_manual_profile_vin(profile_id, expected_revision, full_vin, actor)
            .map_err(|error| error.to_string())?;
        let saved_profile = if active_target.is_some() {
            store
                .get(profile_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("Vehicle profile '{profile_id}' does not exist"))?
                .revision
        } else {
            0
        };
        drop(store);
        self.invalidate_preview()?;
        if let Some((active_id, _, generation)) = active_target {
            *self
                .active_profile
                .lock()
                .map_err(|_| "Active profile state is unavailable".to_owned())? =
                Some((active_id, saved_profile, generation));
        }
        Ok(result)
    }

    pub(crate) fn set_manual_vehicle_detail(
        &self,
        scope: ProfileScope,
        vin_digest: &str,
        field: ManualVehicleField,
        value: String,
        actor: ManualDetailActor,
        note: Option<String>,
    ) -> Result<VehicleIdentityRecord, String> {
        let _operation = self.lock_operation()?;
        self.store()?
            .set_manual_vehicle_detail(scope, vin_digest, field, value, actor, note)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn add_diagnostic_locator(
        &self,
        locator: DiagnosticReportLocator,
    ) -> Result<(), String> {
        let _operation = self.lock_operation()?;
        self.store()?
            .add_diagnostic_locator(locator)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn get_diagnostic_locator(
        &self,
        report_id: &str,
    ) -> Result<Option<DiagnosticReportLocator>, String> {
        self.store()?
            .get_diagnostic_locator(report_id)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn list_diagnostic_history(
        &self,
        filter: DiagnosticHistoryFilter,
    ) -> Result<DiagnosticHistoryPage, String> {
        self.store()?
            .list_diagnostic_history(filter)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn append_manual_report_association(
        &self,
        report_id: String,
        scope: ProfileScope,
        vin_digest: String,
        actor: ManualDetailActor,
        note: Option<String>,
    ) -> Result<ManualReportAssociation, String> {
        let _operation = self.lock_operation()?;
        self.store()?
            .append_manual_report_association(report_id, scope, vin_digest, actor, note)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn get_manual_report_associations(
        &self,
        report_id: &str,
    ) -> Result<Vec<ManualReportAssociation>, String> {
        self.store()?
            .get_manual_report_associations(report_id)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn get_history_import_checkpoint(&self) -> Result<HistoryImportCheckpoint, String> {
        self.store()?
            .get_history_import_checkpoint()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn save_history_import_checkpoint(
        &self,
        checkpoint: HistoryImportCheckpoint,
    ) -> Result<(), String> {
        let _operation = self.lock_operation()?;
        self.store()?
            .save_history_import_checkpoint(checkpoint)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn begin_history_import(&self) -> Result<Arc<AtomicBool>, String> {
        let mut current = self
            .history_import_cancel
            .lock()
            .map_err(|_| "Vehicle history import state is unavailable".to_owned())?;
        if current.is_some() {
            return Err("Vehicle history import is already running".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *current = Some(cancel.clone());
        Ok(cancel)
    }

    pub(crate) fn cancel_history_import(&self) -> Result<(), String> {
        if let Some(cancel) = self
            .history_import_cancel
            .lock()
            .map_err(|_| "Vehicle history import state is unavailable".to_owned())?
            .as_ref()
        {
            cancel.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub(crate) fn finish_history_import(&self, cancel: &Arc<AtomicBool>) {
        if let Ok(mut current) = self.history_import_cancel.lock()
            && current
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, cancel))
        {
            current.take();
        }
    }

    pub(crate) fn append_observation(
        &self,
        profile_id: &str,
        observation: VehicleIdentityObservation,
    ) -> Result<VehicleProfile, String> {
        let _operation = self.lock_operation()?;
        self.store()?
            .append_observation(profile_id, observation)
            .map_err(|error| error.to_string())
    }

    fn store(&self) -> Result<MutexGuard<'_, VehicleProfileStore>, String> {
        self.store
            .as_ref()
            .ok_or_else(|| {
                self.store_error
                    .clone()
                    .unwrap_or_else(|| "Vehicle profile store is unavailable".into())
            })?
            .lock()
            .map_err(|_| "Vehicle profile store is unavailable".to_owned())
    }

    fn invalidate_preview(&self) -> Result<(), String> {
        self.preview
            .lock()
            .map_err(|_| "Profile preview state is unavailable".to_owned())?
            .take();
        Ok(())
    }

    pub(crate) fn save(
        &self,
        args: SaveVehicleProfileArgs,
        current_generation: u64,
        current_scope: ProfileScope,
    ) -> Result<VehicleProfile, String> {
        let _operation = self.lock_operation()?;
        if args.profile_id.is_some() != args.expected_revision.is_some() {
            return Err(
                "Profile updates require both the profile ID and its expected revision".into(),
            );
        }
        let observation = self.current_observation(current_generation).or_else(|| {
            args.manual_association.then(|| VehicleIdentityObservation {
                obd_connection_generation: current_generation,
                observed_at_unix_ms: now_unix_ms(),
                scope: current_scope,
                vin_digest: None,
                masked_vin: None,
                vin_status: IdentityReadState::Unsupported,
                conflicting_vin_observations: false,
                ecu_variants: vec![],
                identity_supported: false,
            })
        });
        if observation.is_none() && !args.manual_association {
            return Err(
                "Recognize the current vehicle first, or explicitly choose manual association"
                    .into(),
            );
        }
        let mut store = self.store()?;
        let profile = if let Some(profile_id) = args.profile_id.as_deref() {
            let mut saved = store
                .get(profile_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("Vehicle profile '{profile_id}' does not exist"))?;
            if let Some(observed) = &observation
                && saved.vin_digest.is_some()
                && observed.vin_digest.is_some()
                && saved.vin_digest != observed.vin_digest
            {
                return Err("The connected VIN differs from this saved profile; create a separate profile instead".into());
            }
            saved.name = args.name.trim().to_owned();
            saved.selection = args.selection;
            saved.manual_association = args.manual_association;
            if args.catalog_vehicle_id.is_some() {
                saved.catalog_vehicle_id = args.catalog_vehicle_id.clone();
            }
            if let Some(observed) = &observation {
                saved.scope = observed.scope;
                if observed.vin_digest.is_some() {
                    saved.vin_digest = observed.vin_digest.clone();
                    saved.masked_vin = observed.masked_vin.clone();
                    saved.ecu_variants = lognook_core::merge_ecu_identity_snapshot(
                        &saved.ecu_variants,
                        &observed.ecu_variants,
                    );
                }
            }
            saved
        } else {
            let observed = observation
                .clone()
                .unwrap_or_else(|| VehicleIdentityObservation {
                    obd_connection_generation: current_generation,
                    observed_at_unix_ms: now_unix_ms(),
                    scope: current_scope,
                    vin_digest: None,
                    masked_vin: None,
                    vin_status: IdentityReadState::Unsupported,
                    conflicting_vin_observations: false,
                    ecu_variants: vec![],
                    identity_supported: false,
                });
            if observed.vin_digest.is_none() && !args.manual_association {
                return Err(
                    "A profile without a readable VIN requires explicit manual association".into(),
                );
            }
            VehicleProfile {
                id: String::new(),
                name: args.name.trim().to_owned(),
                revision: 0,
                scope: observed.scope,
                catalog_vehicle_id: args.catalog_vehicle_id.clone(),
                vin_digest: observed.vin_digest.clone(),
                masked_vin: observed.masked_vin.clone(),
                manual_association: args.manual_association,
                ecu_variants: observed.ecu_variants.clone(),
                selection: args.selection,
                observations: vec![],
                created_at_unix_ms: 0,
                updated_at_unix_ms: 0,
            }
        };
        let stored = store
            .save_with_observation(profile, args.expected_revision, observation)
            .map_err(|error| error.to_string())?;
        self.preview
            .lock()
            .map_err(|_| "Profile preview state is unavailable".to_owned())?
            .take();
        *self
            .active_profile
            .lock()
            .map_err(|_| "Active profile state is unavailable".to_owned())? =
            Some((stored.id.clone(), stored.revision, current_generation));
        Ok(stored)
    }

    pub(crate) fn begin_recognition(&self) -> Result<Arc<AtomicBool>, String> {
        self.cancel_pending_work();
        let cancel = Arc::new(AtomicBool::new(false));
        *self
            .recognition_cancel
            .lock()
            .map_err(|_| "Profile recognition cancellation is unavailable".to_owned())? =
            Some(cancel.clone());
        Ok(cancel)
    }

    pub(crate) fn finish_recognition(&self, cancel: &Arc<AtomicBool>) {
        if let Ok(mut current) = self.recognition_cancel.lock()
            && current
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, cancel))
        {
            current.take();
        }
    }

    pub(crate) fn cancel_pending_work(&self) {
        self.work_generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut current) = self.recognition_cancel.lock()
            && let Some(cancel) = current.take()
        {
            cancel.store(true, Ordering::Release);
        }
        if let Ok(mut preview) = self.preview.lock() {
            preview.take();
        }
    }

    pub(crate) fn work_generation(&self) -> u64 {
        self.work_generation.load(Ordering::Acquire)
    }

    pub(crate) fn record_observation(
        &self,
        observation: VehicleIdentityObservation,
    ) -> Result<(), String> {
        let _operation = self.lock_operation()?;
        self.cancel_pending_recognition_only();
        *self
            .observation
            .lock()
            .map_err(|_| "Current profile identity is unavailable".to_owned())? = Some(observation);
        self.preview
            .lock()
            .map_err(|_| "Profile preview state is unavailable".to_owned())?
            .take();
        Ok(())
    }

    pub(crate) fn current_observation(
        &self,
        generation: u64,
    ) -> Option<VehicleIdentityObservation> {
        self.observation
            .lock()
            .ok()?
            .as_ref()
            .filter(|observation| observation.obd_connection_generation == generation)
            .cloned()
    }

    pub(crate) fn make_preview_id(&self) -> Result<String, String> {
        let next = self
            .next_preview_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "Vehicle profile preview identifier space is exhausted".to_owned())?;
        Ok(format!(
            "profile-resume-{}-{}-{next}",
            std::process::id(),
            now_unix_ms()
        ))
    }

    pub(crate) fn store_preview(&self, preview: StoredPreview) -> Result<(), String> {
        *self
            .preview
            .lock()
            .map_err(|_| "Profile preview state is unavailable".to_owned())? = Some(preview);
        Ok(())
    }

    pub(crate) fn current_preview(&self) -> Option<StoredPreview> {
        let mut slot = self.preview.lock().ok()?;
        if slot
            .as_ref()
            .is_some_and(|preview| preview.created.elapsed() > PREVIEW_TTL)
        {
            slot.take();
        }
        slot.clone()
    }

    pub(crate) fn discard_preview(&self, preview_id: &str) -> Result<(), String> {
        let mut slot = self
            .preview
            .lock()
            .map_err(|_| "Profile preview state is unavailable".to_owned())?;
        if slot
            .as_ref()
            .is_some_and(|preview| preview.result.preview_id == preview_id)
        {
            slot.take();
        }
        Ok(())
    }

    pub(crate) fn set_active_profile(
        &self,
        profile_id: String,
        revision: u64,
        generation: u64,
    ) -> Result<(), String> {
        *self
            .active_profile
            .lock()
            .map_err(|_| "Active profile state is unavailable".to_owned())? =
            Some((profile_id, revision, generation));
        Ok(())
    }

    pub(crate) fn active_profile(&self, generation: u64) -> Option<(String, u64)> {
        self.active_profile
            .lock()
            .ok()?
            .as_ref()
            .filter(|(_, _, saved_generation)| *saved_generation == generation)
            .map(|(id, revision, _)| (id.clone(), *revision))
    }

    pub(crate) fn lock_operation(&self) -> Result<MutexGuard<'_, ()>, String> {
        self.profile_operation
            .lock()
            .map_err(|_| "Vehicle profile service is unavailable".to_owned())
    }

    fn cancel_pending_recognition_only(&self) {
        if let Ok(mut current) = self.recognition_cancel.lock()
            && let Some(cancel) = current.take()
        {
            cancel.store(true, Ordering::Release);
        }
    }
}

pub(crate) fn unknown_profile_match() -> ProfileMatch {
    ProfileMatch {
        state: ProfileMatchState::IdentityUnavailable,
        fingerprint_digest: None,
        matched_ecu_variants: vec![],
        review_reasons: vec!["No current identity observation is available".into()],
    }
}

pub(crate) fn active_derived_ids(
    selection: &ProfileSelectionSnapshot,
    eligible_log_refs: &[String],
    eligible_graph_refs: &[String],
) -> Vec<String> {
    let available = eligible_log_refs
        .iter()
        .chain(eligible_graph_refs)
        .collect::<HashSet<_>>();
    selection
        .derived_definitions
        .iter()
        .filter(|definition| available.contains(&definition.id))
        .map(|definition| definition.id.clone())
        .collect()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod identity_assignment_tests {
    use super::*;
    use lognook_core::{IdentityReadState, VinOrigin};

    fn temp_db() -> std::path::PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lognook-profile-service-{id}.sqlite"))
    }

    fn blank_manual_profile() -> VehicleProfile {
        VehicleProfile {
            id: "manual-profile".into(),
            name: "Manually associated vehicle".into(),
            revision: 0,
            scope: ProfileScope::Live,
            catalog_vehicle_id: None,
            vin_digest: None,
            masked_vin: None,
            manual_association: true,
            ecu_variants: vec![],
            selection: ProfileSelectionSnapshot::default(),
            observations: vec![],
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }

    fn current_observation(vin: &str, generation: u64) -> VehicleIdentityObservation {
        let (vin_digest, masked_vin) = canonical_vin_identity(vin).unwrap();
        VehicleIdentityObservation {
            obd_connection_generation: generation,
            observed_at_unix_ms: 1,
            scope: ProfileScope::Live,
            vin_digest: Some(vin_digest),
            masked_vin: Some(masked_vin),
            vin_status: IdentityReadState::Positive,
            conflicting_vin_observations: false,
            ecu_variants: vec![],
            identity_supported: true,
        }
    }

    #[test]
    fn active_conflicting_ecu_vin_blocks_manual_identity_assignment() {
        let path = temp_db();
        let service = VehicleProfileService::open(&path);
        let saved = service
            .store()
            .unwrap()
            .save(blank_manual_profile(), None)
            .unwrap();
        service
            .set_active_profile(saved.id.clone(), saved.revision, 77)
            .unwrap();
        service
            .record_observation(current_observation("2FAHP2E89BG123456", 77))
            .unwrap();

        let error = service
            .assign_manual_profile_vin(
                &saved.id,
                saved.revision,
                "1FAHP2E89BG123456",
                ManualDetailActor::OwnerUi,
            )
            .unwrap_err();
        assert!(error.contains("conflicting ECU VIN"));
        let unchanged = service.get(&saved.id).unwrap().unwrap();
        assert_eq!(unchanged.revision, saved.revision);
        assert_eq!(unchanged.vin_digest, None);
        assert_eq!(service.active_profile(77), Some((saved.id, saved.revision)));
        drop(service);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn active_matching_ecu_vin_allows_manual_assignment_without_claiming_ecu_origin() {
        let path = temp_db();
        let service = VehicleProfileService::open(&path);
        let saved = service
            .store()
            .unwrap()
            .save(blank_manual_profile(), None)
            .unwrap();
        service
            .set_active_profile(saved.id.clone(), saved.revision, 77)
            .unwrap();
        service
            .record_observation(current_observation("1FAHP2E89BG123456", 77))
            .unwrap();

        let identity = service
            .assign_manual_profile_vin(
                &saved.id,
                saved.revision,
                "1FAHP2E89BG123456",
                ManualDetailActor::OwnerUi,
            )
            .unwrap();
        let updated = service.get(&saved.id).unwrap().unwrap();
        assert_eq!(updated.revision, saved.revision + 1);
        assert!(updated.manual_association);
        assert!(updated.ecu_variants.is_empty());
        assert_eq!(identity.origin_evidence.len(), 1);
        assert_eq!(identity.origin_evidence[0].origin, VinOrigin::OwnerEntered);
        assert_eq!(
            identity.origin_evidence[0].actor,
            Some(ManualDetailActor::OwnerUi)
        );
        assert!(identity.owner_details.is_empty());
        assert_eq!(
            service.active_profile(77),
            Some((saved.id, updated.revision))
        );
        let current = service.current_observation(77).unwrap();
        assert_eq!(current.vin_status, IdentityReadState::Positive);
        assert!(current.vin_digest.is_some());
        drop(service);
        std::fs::remove_file(path).unwrap();
    }
}
