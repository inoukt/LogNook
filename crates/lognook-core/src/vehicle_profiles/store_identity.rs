use super::*;
use crate::vehicle_profiles::{
    ManualDetailActor, ManualVehicleDetail, ManualVehicleField, VehicleIdentityRecord,
    VinEvidenceSource, VinOrigin, canonical_vin_identity, decode_vin_offline,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

impl VehicleProfileStore {
    pub fn get_vehicle_identity(
        &self,
        scope: ProfileScope,
        vin_digest: &str,
    ) -> Result<Option<VehicleIdentityRecord>, ProfileStoreError> {
        validate_scope_digest(scope, vin_digest)?;
        load_identity(&self.connection, scope, vin_digest)
    }

    pub fn save_vehicle_identity(
        &mut self,
        record: VehicleIdentityRecord,
        expected_revision: Option<u64>,
    ) -> Result<VehicleIdentityRecord, ProfileStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = write_identity(&transaction, record, expected_revision)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn set_manual_vehicle_detail(
        &mut self,
        scope: ProfileScope,
        vin_digest: &str,
        field: ManualVehicleField,
        value: String,
        actor: ManualDetailActor,
        note: Option<String>,
    ) -> Result<VehicleIdentityRecord, ProfileStoreError> {
        validate_scope_digest(scope, vin_digest)?;
        let value = value.trim().to_owned();
        if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
            return Err(ProfileStoreError::Invalid(
                "Manual vehicle details must contain 1-256 printable characters".into(),
            ));
        }
        let note = validate_note(note)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_identity(&transaction, scope, vin_digest)?;
        let expected_revision = current.as_ref().map(|identity| identity.revision);
        let mut record = current.unwrap_or_else(|| VehicleIdentityRecord {
            scope,
            vin_digest: vin_digest.to_owned(),
            full_vin: None,
            origin_evidence: vec![],
            decoder_snapshot: None,
            owner_details: vec![],
            revision: 0,
        });
        record.owner_details.push(ManualVehicleDetail {
            field,
            value,
            actor,
            entered_at_unix_ms: now_unix_ms(),
            note,
        });
        let saved = write_identity(&transaction, record, expected_revision)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn link_legacy_vehicle_vin(
        &mut self,
        profile_id: &str,
        expected_revision: u64,
        full_vin: &str,
        actor: ManualDetailActor,
    ) -> Result<VehicleIdentityRecord, ProfileStoreError> {
        let (normalized_vin, vin_digest, _) = canonicalize_vin(full_vin)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (scope, profile_digest, actual_revision) =
            load_profile_identity(&transaction, profile_id)?;
        if actual_revision != expected_revision {
            return Err(ProfileStoreError::RevisionConflict {
                expected: Some(expected_revision),
                actual: Some(actual_revision),
            });
        }
        if profile_digest.as_deref() != Some(vin_digest.as_str()) {
            return Err(ProfileStoreError::Invalid(
                "The supplied VIN must exactly match the saved profile VIN digest".into(),
            ));
        }

        let current = load_identity(&transaction, scope, &vin_digest)?;
        let identity_revision = current.as_ref().map(|identity| identity.revision);
        let mut record = current.unwrap_or_else(|| empty_identity(scope, &vin_digest));
        record.full_vin = Some(normalized_vin.clone());
        record.decoder_snapshot = Some(decode_vin_offline(&normalized_vin).map_err(|error| {
            ProfileStoreError::Invalid(format!("The supplied VIN could not be decoded: {error}"))
        })?);
        record.origin_evidence.push(manual_vin_evidence(actor));
        let saved = write_identity(&transaction, record, identity_revision)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn assign_manual_profile_vin(
        &mut self,
        profile_id: &str,
        expected_revision: u64,
        full_vin: &str,
        actor: ManualDetailActor,
    ) -> Result<VehicleIdentityRecord, ProfileStoreError> {
        let (normalized_vin, vin_digest, masked_vin) = canonicalize_vin(full_vin)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (scope, manual_association, profile_digest, actual_revision) =
            load_profile_for_manual_vin(&transaction, profile_id)?;
        if actual_revision != expected_revision {
            return Err(ProfileStoreError::RevisionConflict {
                expected: Some(expected_revision),
                actual: Some(actual_revision),
            });
        }
        if !manual_association {
            return Err(ProfileStoreError::Invalid(
                "A full VIN can be assigned manually only to a manually associated profile".into(),
            ));
        }
        if profile_digest
            .as_deref()
            .is_some_and(|saved_digest| saved_digest != vin_digest)
        {
            return Err(ProfileStoreError::Invalid(
                "The supplied VIN differs from the saved profile VIN".into(),
            ));
        }

        let observations = load_json_rows::<VehicleIdentityObservation>(
            &transaction,
            "SELECT value_json FROM identity_observations WHERE profile_id = ?1 ORDER BY ordinal",
            profile_id,
        )?;
        if observations.iter().any(|observation| {
            observation.conflicting_vin_observations
                || observation
                    .vin_digest
                    .as_deref()
                    .is_some_and(|observed_digest| observed_digest != vin_digest)
        }) {
            return Err(ProfileStoreError::Invalid(
                "The saved profile contains conflicting ECU VIN evidence".into(),
            ));
        }

        let current = load_identity(&transaction, scope, &vin_digest)?;
        let identity_revision = current.as_ref().map(|identity| identity.revision);
        let mut record = current.unwrap_or_else(|| empty_identity(scope, &vin_digest));
        if record
            .full_vin
            .as_deref()
            .is_some_and(|saved_vin| saved_vin != normalized_vin)
        {
            return Err(ProfileStoreError::Invalid(
                "A different full VIN is already stored for this profile digest".into(),
            ));
        }
        record.full_vin = Some(normalized_vin.clone());
        record.decoder_snapshot = Some(decode_vin_offline(&normalized_vin).map_err(|error| {
            ProfileStoreError::Invalid(format!("The supplied VIN could not be decoded: {error}"))
        })?);
        record.origin_evidence.push(manual_vin_evidence(actor));

        if profile_digest.is_none() {
            let next_revision = actual_revision.saturating_add(1);
            let updated = transaction.execute(
                "UPDATE profiles SET vin_digest=?1,masked_vin=?2,revision=?3,updated_at_unix_ms=?4 WHERE profile_id=?5 AND revision=?6",
                params![
                    vin_digest,
                    masked_vin,
                    to_sql_u64(next_revision)?,
                    to_sql_u64(now_unix_ms())?,
                    profile_id,
                    to_sql_u64(actual_revision)?,
                ],
            )?;
            if updated != 1 {
                return Err(ProfileStoreError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: transaction
                        .query_row(
                            "SELECT revision FROM profiles WHERE profile_id=?1",
                            [profile_id],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?
                        .map(from_sql_u64)
                        .transpose()?,
                });
            }
        }
        let saved = write_identity(&transaction, record, identity_revision)?;
        transaction.commit()?;
        Ok(saved)
    }
}

fn write_identity(
    transaction: &Transaction<'_>,
    mut incoming: VehicleIdentityRecord,
    expected_revision: Option<u64>,
) -> Result<VehicleIdentityRecord, ProfileStoreError> {
    validate_scope_digest(incoming.scope, &incoming.vin_digest)?;
    validate_identity_sources(&incoming.origin_evidence)?;
    validate_manual_details(&incoming.owner_details)?;

    let current = load_identity(transaction, incoming.scope, &incoming.vin_digest)?;
    let had_full_vin = current
        .as_ref()
        .is_some_and(|identity| identity.full_vin.is_some());
    let actual_revision = current.as_ref().map(|identity| identity.revision);
    if actual_revision != expected_revision {
        return Err(ProfileStoreError::RevisionConflict {
            expected: expected_revision,
            actual: actual_revision,
        });
    }

    if let Some(vin) = incoming.full_vin.as_deref() {
        let (normalized, digest, _) = canonicalize_vin(vin)?;
        if digest != incoming.vin_digest {
            return Err(ProfileStoreError::Invalid(
                "The full VIN does not match the identity digest".into(),
            ));
        }
        incoming.full_vin = Some(normalized);
    }
    if let Some(snapshot) = &incoming.decoder_snapshot
        && incoming.full_vin.as_deref().or_else(|| {
            current
                .as_ref()
                .and_then(|identity| identity.full_vin.as_deref())
        }) != Some(snapshot.normalized_vin.as_str())
    {
        return Err(ProfileStoreError::Invalid(
            "The decoder snapshot must describe the stored full VIN".into(),
        ));
    }

    let mut saved = incoming;
    let (evidence_to_append, details_to_append) = if let Some(current) = current {
        if let (Some(existing_vin), Some(incoming_vin)) =
            (current.full_vin.as_deref(), saved.full_vin.as_deref())
            && existing_vin != incoming_vin.trim().to_ascii_uppercase()
        {
            return Err(ProfileStoreError::Invalid(
                "A stored full VIN cannot be replaced by another VIN".into(),
            ));
        }
        if saved.full_vin.is_none() {
            saved.full_vin = current.full_vin.clone();
        }
        if saved.decoder_snapshot.is_none() {
            saved.decoder_snapshot = current.decoder_snapshot.clone();
        }
        let evidence = append_only_suffix(&current.origin_evidence, &saved.origin_evidence);
        let details = append_only_suffix(&current.owner_details, &saved.owner_details);
        saved.origin_evidence = current.origin_evidence;
        saved.origin_evidence.extend(evidence.iter().cloned());
        saved.owner_details = current.owner_details;
        saved.owner_details.extend(details.iter().cloned());
        (evidence, details)
    } else {
        (saved.origin_evidence.clone(), saved.owner_details.clone())
    };
    validate_identity_sources(&saved.origin_evidence)?;
    validate_manual_details(&saved.owner_details)?;
    if !had_full_vin && saved.full_vin.is_some() && saved.origin_evidence.is_empty() {
        return Err(ProfileStoreError::Invalid(
            "A full VIN requires at least one source evidence record".into(),
        ));
    }

    if let Some(full_vin) = saved.full_vin.clone() {
        let (_, digest, mask) = canonicalize_vin(&full_vin)?;
        if digest != saved.vin_digest {
            return Err(ProfileStoreError::Invalid(
                "The stored full VIN does not match the identity digest".into(),
            ));
        }
        saved.full_vin = Some(full_vin.clone());
        let revision = actual_revision.unwrap_or(0).saturating_add(1);
        let state = "vin_known";
        transaction.execute(
            "INSERT INTO vehicle_identities(scope,vin_digest,full_vin,masked_vin,identity_state,decoder_snapshot_json,revision,updated_at_unix_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(scope,vin_digest) DO UPDATE SET full_vin=excluded.full_vin,masked_vin=excluded.masked_vin,identity_state=excluded.identity_state,decoder_snapshot_json=COALESCE(excluded.decoder_snapshot_json,vehicle_identities.decoder_snapshot_json),revision=excluded.revision,updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                scope_text(saved.scope),
                saved.vin_digest,
                full_vin,
                mask,
                state,
                saved.decoder_snapshot.as_ref().map(encode).transpose()?,
                to_sql_u64(revision)?,
                to_sql_u64(now_unix_ms())?,
            ],
        )?;
        saved.revision = revision;
    } else {
        let revision = actual_revision.unwrap_or(0).saturating_add(1);
        let decoder_json = saved.decoder_snapshot.as_ref().map(encode).transpose()?;
        transaction.execute(
            "INSERT INTO vehicle_identities(scope,vin_digest,full_vin,masked_vin,identity_state,decoder_snapshot_json,revision,updated_at_unix_ms) VALUES(?1,?2,NULL,NULL,'legacy_hash_only',?3,?4,?5)
             ON CONFLICT(scope,vin_digest) DO UPDATE SET identity_state='legacy_hash_only',decoder_snapshot_json=COALESCE(excluded.decoder_snapshot_json,vehicle_identities.decoder_snapshot_json),revision=excluded.revision,updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                scope_text(saved.scope),
                saved.vin_digest,
                decoder_json,
                to_sql_u64(revision)?,
                to_sql_u64(now_unix_ms())?,
            ],
        )?;
        saved.revision = revision;
    }
    append_identity_rows(transaction, &saved, &evidence_to_append, &details_to_append)?;
    Ok(saved)
}

fn load_identity(
    connection: &Connection,
    scope: ProfileScope,
    vin_digest: &str,
) -> Result<Option<VehicleIdentityRecord>, ProfileStoreError> {
    let row = connection
        .query_row(
            "SELECT full_vin,decoder_snapshot_json,revision FROM vehicle_identities WHERE scope=?1 AND vin_digest=?2",
            params![scope_text(scope), vin_digest],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?)),
        )
        .optional()?;
    let Some((full_vin, decoder_json, revision)) = row else {
        return Ok(None);
    };
    let origin_evidence = load_identity_rows::<VinEvidenceSource>(
        connection,
        "SELECT value_json FROM vehicle_identity_evidence WHERE scope=?1 AND vin_digest=?2 ORDER BY ordinal",
        scope,
        vin_digest,
    )?;
    let owner_details = load_identity_rows::<ManualVehicleDetail>(
        connection,
        "SELECT value_json FROM manual_vehicle_details WHERE scope=?1 AND vin_digest=?2 ORDER BY ordinal",
        scope,
        vin_digest,
    )?;
    let record = VehicleIdentityRecord {
        scope,
        vin_digest: vin_digest.to_owned(),
        full_vin,
        origin_evidence,
        decoder_snapshot: decoder_json.as_deref().map(decode).transpose()?,
        owner_details,
        revision: from_sql_u64(revision)?,
    };
    validate_loaded_identity(&record)?;
    Ok(Some(record))
}

fn load_identity_rows<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    scope: ProfileScope,
    vin_digest: &str,
) -> Result<Vec<T>, ProfileStoreError> {
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map(params![scope_text(scope), vin_digest], |row| {
            row.get::<_, String>(0)
        })?
        .map(|value| decode(&value?))
        .collect()
}

fn append_identity_rows(
    transaction: &Transaction<'_>,
    record: &VehicleIdentityRecord,
    evidence: &[VinEvidenceSource],
    details: &[ManualVehicleDetail],
) -> Result<(), ProfileStoreError> {
    let evidence_start = record.origin_evidence.len() - evidence.len();
    let details_start = record.owner_details.len() - details.len();
    for (ordinal, value) in evidence.iter().enumerate() {
        transaction.execute(
            "INSERT INTO vehicle_identity_evidence(scope,vin_digest,ordinal,value_json) VALUES(?1,?2,?3,?4)",
            params![scope_text(record.scope), record.vin_digest, (evidence_start + ordinal) as i64, encode(value)?],
        )?;
    }
    for (ordinal, value) in details.iter().enumerate() {
        transaction.execute(
            "INSERT INTO manual_vehicle_details(scope,vin_digest,ordinal,value_json) VALUES(?1,?2,?3,?4)",
            params![scope_text(record.scope), record.vin_digest, (details_start + ordinal) as i64, encode(value)?],
        )?;
    }
    Ok(())
}

fn append_only_suffix<T: Clone + PartialEq>(current: &[T], incoming: &[T]) -> Vec<T> {
    if incoming.starts_with(current) {
        incoming[current.len()..].to_vec()
    } else if current.starts_with(incoming) {
        vec![]
    } else {
        incoming.to_vec()
    }
}

pub(super) fn ensure_identity_row(
    transaction: &Transaction<'_>,
    scope: ProfileScope,
    vin_digest: &str,
) -> Result<(), ProfileStoreError> {
    validate_scope_digest(scope, vin_digest)?;
    transaction.execute(
        "INSERT OR IGNORE INTO vehicle_identities(scope,vin_digest,full_vin,masked_vin,identity_state,revision,updated_at_unix_ms) VALUES(?1,?2,NULL,NULL,'legacy_hash_only',1,?3)",
        params![scope_text(scope), vin_digest, to_sql_u64(now_unix_ms())?],
    )?;
    Ok(())
}

fn validate_identity_sources(sources: &[VinEvidenceSource]) -> Result<(), ProfileStoreError> {
    if sources.len() > 500 {
        return Err(ProfileStoreError::Invalid(
            "A vehicle identity can retain at most 500 source records".into(),
        ));
    }
    for source in sources {
        let actor_matches = match source.origin {
            VinOrigin::OwnerEntered => source.actor == Some(ManualDetailActor::OwnerUi),
            VinOrigin::AgentEntered => source.actor == Some(ManualDetailActor::AgentAccess),
            VinOrigin::EcuReport | VinOrigin::EcuRecognition | VinOrigin::LegacyReportImport => {
                source.actor.is_none()
            }
        };
        if !actor_matches
            || source
                .report_id
                .as_ref()
                .is_some_and(|id| invalid_text(id, 128))
            || source
                .ecu_id
                .as_ref()
                .is_some_and(|id| invalid_text(id, 64))
        {
            return Err(ProfileStoreError::Invalid(
                "VIN source provenance contains an invalid actor, report ID, or ECU ID".into(),
            ));
        }
    }
    Ok(())
}

fn validate_manual_details(details: &[ManualVehicleDetail]) -> Result<(), ProfileStoreError> {
    if details.len() > 500 {
        return Err(ProfileStoreError::Invalid(
            "A vehicle identity can retain at most 500 manual details".into(),
        ));
    }
    for detail in details {
        if detail.value.trim().is_empty()
            || invalid_text(&detail.value, 256)
            || detail
                .note
                .as_ref()
                .is_some_and(|note| invalid_text(note, 1024))
        {
            return Err(ProfileStoreError::Invalid(
                "Manual vehicle detail value or note is invalid".into(),
            ));
        }
    }
    Ok(())
}

fn validate_loaded_identity(record: &VehicleIdentityRecord) -> Result<(), ProfileStoreError> {
    validate_scope_digest(record.scope, &record.vin_digest)?;
    if let Some(full_vin) = record.full_vin.as_deref() {
        let (_, digest, _) = canonicalize_vin(full_vin)?;
        if digest != record.vin_digest {
            return Err(ProfileStoreError::Encoding(
                "Stored full VIN does not match its identity digest".into(),
            ));
        }
    }
    if record.decoder_snapshot.as_ref().is_some_and(|snapshot| {
        record.full_vin.as_deref() != Some(snapshot.normalized_vin.as_str())
    }) {
        return Err(ProfileStoreError::Encoding(
            "Stored decoder snapshot does not match its full VIN".into(),
        ));
    }
    validate_identity_sources(&record.origin_evidence)?;
    validate_manual_details(&record.owner_details)
}

pub(super) fn validate_scope_digest(
    scope: ProfileScope,
    vin_digest: &str,
) -> Result<(), ProfileStoreError> {
    let _ = scope_text(scope);
    let Some(digest) = vin_digest.strip_prefix("vin:v1:sha256:") else {
        return Err(ProfileStoreError::Invalid(
            "Vehicle identity requires a version 1 VIN digest".into(),
        ));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProfileStoreError::Invalid(
            "Vehicle identity VIN digest must contain 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

fn canonicalize_vin(vin: &str) -> Result<(String, String, String), ProfileStoreError> {
    let normalized = vin.trim().to_ascii_uppercase();
    let (digest, masked) =
        canonical_vin_identity(&normalized).map_err(ProfileStoreError::Invalid)?;
    Ok((normalized, digest, masked))
}

fn load_profile_identity(
    connection: &Connection,
    profile_id: &str,
) -> Result<(ProfileScope, Option<String>, u64), ProfileStoreError> {
    let profile = connection
        .query_row(
            "SELECT scope,vin_digest,revision FROM profiles WHERE profile_id=?1",
            [profile_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            ProfileStoreError::Invalid(format!("Vehicle profile '{profile_id}' does not exist"))
        })?;
    Ok((
        parse_scope(&profile.0)?,
        profile.1,
        from_sql_u64(profile.2)?,
    ))
}

fn load_profile_for_manual_vin(
    connection: &Connection,
    profile_id: &str,
) -> Result<(ProfileScope, bool, Option<String>, u64), ProfileStoreError> {
    let profile = connection
        .query_row(
            "SELECT scope,manual_association,vin_digest,revision FROM profiles WHERE profile_id=?1",
            [profile_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            ProfileStoreError::Invalid(format!("Vehicle profile '{profile_id}' does not exist"))
        })?;
    Ok((
        parse_scope(&profile.0)?,
        profile.1,
        profile.2,
        from_sql_u64(profile.3)?,
    ))
}

fn empty_identity(scope: ProfileScope, vin_digest: &str) -> VehicleIdentityRecord {
    VehicleIdentityRecord {
        scope,
        vin_digest: vin_digest.to_owned(),
        full_vin: None,
        origin_evidence: vec![],
        decoder_snapshot: None,
        owner_details: vec![],
        revision: 0,
    }
}

fn manual_vin_evidence(actor: ManualDetailActor) -> VinEvidenceSource {
    VinEvidenceSource {
        origin: match actor {
            ManualDetailActor::OwnerUi => VinOrigin::OwnerEntered,
            ManualDetailActor::AgentAccess => VinOrigin::AgentEntered,
        },
        recorded_at_unix_ms: now_unix_ms(),
        report_id: None,
        ecu_id: None,
        connection_generation: None,
        actor: Some(actor),
    }
}

pub(super) fn validate_note(note: Option<String>) -> Result<Option<String>, ProfileStoreError> {
    if note.as_ref().is_some_and(|value| invalid_text(value, 1024)) {
        return Err(ProfileStoreError::Invalid(
            "Manual vehicle detail note must be at most 1024 characters".into(),
        ));
    }
    Ok(note
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty()))
}

fn invalid_text(value: &str, max_chars: usize) -> bool {
    value.chars().count() > max_chars || value.chars().any(char::is_control)
}
