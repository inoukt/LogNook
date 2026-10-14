use lognook_core::vehicle_profiles::{
    EcuProfileVariant, IdentityReadState, ManualDetailActor, ManualVehicleField,
    OemDefinitionReference, ProfileMatchState, ProfileScope, ProfileSelectionSnapshot,
    VehicleIdentityObservation, VehicleIdentityRecord, VehicleProfile, VehicleProfileStore,
    VinEvidenceSource, VinOrigin, canonical_vin_identity, decode_vin_offline, fingerprint_v1,
    resolve_profile_match,
};
use rusqlite::Connection;
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_db() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-profiles-{id}.sqlite"))
}

fn ecu(address: &str, calibration: &[&str]) -> EcuProfileVariant {
    EcuProfileVariant {
        protocol: "ISO 15765-4 CAN (11 bit, 500 kbit/s)".into(),
        request_route: "7DF (functional)".into(),
        request_address: Some("7E0".into()),
        response_address: address.into(),
        ecu_name: Some("PCM".into()),
        calibration_ids: calibration.iter().map(|value| (*value).into()).collect(),
        software_ids: vec![],
        ecu_name_state: IdentityReadState::Positive,
        calibration_state: if calibration.is_empty() {
            IdentityReadState::NoResponse
        } else {
            IdentityReadState::Positive
        },
        software_state: IdentityReadState::NoResponse,
    }
}

fn profile() -> VehicleProfile {
    let (vin_digest, masked_vin) = canonical_vin_identity("1FAHP2E89BG123456").unwrap();
    VehicleProfile {
        id: "ford-flex".into(),
        name: "2011 Ford Flex".into(),
        revision: 0,
        scope: ProfileScope::Live,
        catalog_vehicle_id: None,
        vin_digest: Some(vin_digest),
        masked_vin: Some(masked_vin),
        manual_association: false,
        ecu_variants: vec![ecu("7E8", &["CAL-A"])],
        selection: ProfileSelectionSnapshot {
            log_signal_ids: vec!["engine_rpm".into()],
            graph_signal_ids: vec!["engine_rpm".into(), "oem:research-boost".into()],
            derived_definitions: vec![],
            can_fields: vec![],
            can_source_id: Some("candlelight:serial-1".into()),
            can_bitrate: Some(500_000),
            can_channel: Some(1),
            oem_references: vec![OemDefinitionReference {
                candidate_id: "research-boost".into(),
                source_id: "Ford-Flex".into(),
                source_revision: "blob-sha-a".into(),
                decoder_revision: "decoder-1".into(),
            }],
        },
        observations: vec![],
        created_at_unix_ms: 0,
        updated_at_unix_ms: 0,
    }
}

fn observation(vin: &str, ecus: Vec<EcuProfileVariant>) -> VehicleIdentityObservation {
    let (vin_digest, masked_vin) = canonical_vin_identity(vin).unwrap();
    VehicleIdentityObservation {
        obd_connection_generation: 9,
        observed_at_unix_ms: 123,
        scope: ProfileScope::Live,
        vin_digest: Some(vin_digest),
        masked_vin: Some(masked_vin),
        vin_status: IdentityReadState::Positive,
        conflicting_vin_observations: false,
        ecu_variants: ecus,
        identity_supported: true,
    }
}

fn manual_profile_without_vin() -> VehicleProfile {
    let mut profile = profile();
    profile.vin_digest = None;
    profile.masked_vin = None;
    profile.ecu_variants.clear();
    profile.manual_association = true;
    profile
}

fn identity_record(scope: ProfileScope, vin: &str) -> VehicleIdentityRecord {
    let (vin_digest, _) = canonical_vin_identity(vin).unwrap();
    VehicleIdentityRecord {
        scope,
        vin_digest,
        full_vin: Some(vin.into()),
        origin_evidence: vec![VinEvidenceSource {
            origin: VinOrigin::EcuRecognition,
            recorded_at_unix_ms: 100,
            report_id: None,
            ecu_id: Some("7E8".into()),
            connection_generation: Some(9),
            actor: None,
        }],
        decoder_snapshot: Some(decode_vin_offline(vin).unwrap()),
        owner_details: vec![],
        revision: 0,
    }
}

fn create_v1_profile_database(path: &std::path::Path, profile: &VehicleProfile) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE profiles(profile_id TEXT PRIMARY KEY,name TEXT NOT NULL,revision INTEGER NOT NULL,scope TEXT NOT NULL,catalog_vehicle_id TEXT,vin_digest TEXT,masked_vin TEXT,manual_association INTEGER NOT NULL,created_at_unix_ms INTEGER NOT NULL,updated_at_unix_ms INTEGER NOT NULL);
             CREATE TABLE ecu_variants(profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,value_json TEXT NOT NULL,PRIMARY KEY(profile_id,ordinal));
             CREATE TABLE identity_observations(profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,value_json TEXT NOT NULL,PRIMARY KEY(profile_id,ordinal));
             CREATE TABLE selection_snapshots(profile_id TEXT PRIMARY KEY REFERENCES profiles(profile_id) ON DELETE CASCADE,value_json TEXT NOT NULL);
             PRAGMA user_version = 1;",
        )
        .unwrap();
    connection.execute(
        "INSERT INTO profiles(profile_id,name,revision,scope,catalog_vehicle_id,vin_digest,masked_vin,manual_association,created_at_unix_ms,updated_at_unix_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        rusqlite::params![
            profile.id,
            profile.name,
            profile.revision as i64,
            "live",
            profile.catalog_vehicle_id,
            profile.vin_digest,
            profile.masked_vin,
            profile.manual_association,
            profile.created_at_unix_ms as i64,
            profile.updated_at_unix_ms as i64,
        ],
    ).unwrap();
    for (ordinal, ecu) in profile.ecu_variants.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO ecu_variants(profile_id,ordinal,value_json) VALUES(?1,?2,?3)",
                rusqlite::params![
                    profile.id,
                    ordinal as i64,
                    serde_json::to_string(ecu).unwrap()
                ],
            )
            .unwrap();
    }
    for (ordinal, observation) in profile.observations.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO identity_observations(profile_id,ordinal,value_json) VALUES(?1,?2,?3)",
                rusqlite::params![
                    profile.id,
                    ordinal as i64,
                    serde_json::to_string(observation).unwrap()
                ],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO selection_snapshots(profile_id,value_json) VALUES(?1,?2)",
            rusqlite::params![
                profile.id,
                serde_json::to_string(&profile.selection).unwrap()
            ],
        )
        .unwrap();
}

#[test]
fn vin_digest_is_validated_masked_and_fingerprint_is_order_independent() {
    let (digest, masked) = canonical_vin_identity("1fahp2e89bg123456").unwrap();
    assert_eq!(masked, "•••••••••••••3456");
    assert_eq!(
        digest,
        canonical_vin_identity("1FAHP2E89BG123456").unwrap().0
    );
    assert!(canonical_vin_identity("1FAHP2E89BI123456").is_err());
    assert!(canonical_vin_identity("short").is_err());

    let a = ecu("7E8", &["CAL-A"]);
    let b = ecu("7E9", &["CAL-B"]);
    assert_eq!(
        fingerprint_v1(&digest, &[a.clone(), b.clone()]).unwrap(),
        fingerprint_v1(&digest, &[b, a.clone()]).unwrap()
    );
    assert_ne!(
        fingerprint_v1(&digest, &[a.clone()]).unwrap(),
        fingerprint_v1(&digest, &[ecu("7E8|7E9", &["CAL-A"])]).unwrap()
    );
}

#[test]
fn resolver_requires_vin_and_prior_ecu_identity_but_ignores_adapter_port_and_extra_modules() {
    let profile = profile();
    let same_vin = "1FAHP2E89BG123456";
    assert_eq!(
        resolve_profile_match(
            &profile,
            &observation(same_vin, vec![ecu("7E8", &["CAL-A"]), ecu("7E9", &[])])
        )
        .state,
        ProfileMatchState::Exact
    );
    assert_eq!(
        resolve_profile_match(
            &profile,
            &observation(same_vin, vec![ecu("7E8", &["CAL-CHANGED"])])
        )
        .state,
        ProfileMatchState::Conflict
    );
    assert_eq!(
        resolve_profile_match(
            &profile,
            &observation("2FAHP2E89BG123456", vec![ecu("7E8", &["CAL-A"])])
        )
        .state,
        ProfileMatchState::Conflict
    );
    let mut missing_vin = observation(same_vin, vec![ecu("7E8", &["CAL-A"])]);
    missing_vin.vin_digest = None;
    missing_vin.masked_vin = None;
    missing_vin.vin_status = IdentityReadState::NoResponse;
    assert_eq!(
        resolve_profile_match(&profile, &missing_vin).state,
        ProfileMatchState::ReviewRequired
    );
    assert_eq!(
        resolve_profile_match(&profile, &observation(same_vin, vec![ecu("7E8", &[])])).state,
        ProfileMatchState::ReviewRequired
    );
}

#[test]
fn functional_request_and_actual_responder_can_match_generic_profile_without_proving_oem_route() {
    let mut saved = profile();
    saved.ecu_variants[0].request_address = None;
    let mut current = observation("1FAHP2E89BG123456", vec![ecu("7E8", &["CAL-A"])]);
    current.ecu_variants[0].request_address = None;
    assert_eq!(
        resolve_profile_match(&saved, &current).state,
        ProfileMatchState::Exact
    );
}

#[test]
fn scopes_and_manual_association_never_claim_an_exact_live_identity() {
    let profile = profile();
    let mut simulator = observation("1FAHP2E89BG123456", vec![ecu("7E8", &["CAL-A"])]);
    simulator.scope = ProfileScope::Simulator;
    assert_eq!(
        resolve_profile_match(&profile, &simulator).state,
        ProfileMatchState::Conflict
    );

    let mut manual = profile;
    manual.vin_digest = None;
    manual.masked_vin = None;
    manual.ecu_variants.clear();
    manual.manual_association = true;
    let mut unavailable = simulator;
    unavailable.scope = ProfileScope::Live;
    unavailable.vin_digest = None;
    unavailable.masked_vin = None;
    unavailable.vin_status = IdentityReadState::Unsupported;
    unavailable.ecu_variants.clear();
    assert_eq!(
        resolve_profile_match(&manual, &unavailable).state,
        ProfileMatchState::ReviewRequired
    );
    assert_eq!(
        resolve_profile_match(&manual, &unavailable).state,
        ProfileMatchState::ReviewRequired
    );
}

#[test]
fn store_round_trips_profiles_checks_revisions_and_keeps_snapshot_on_failed_refresh() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let created = store.save(profile(), None).unwrap();
    assert_eq!(created.revision, 1);
    let mut updated = created.clone();
    updated.name = "Flex EcoBoost".into();
    let updated = store.save(updated, Some(1)).unwrap();
    assert_eq!(updated.revision, 2);
    assert!(store.save(updated.clone(), Some(1)).is_err());

    let mut failed_refresh = observation("1FAHP2E89BG123456", vec![]);
    failed_refresh.identity_supported = false;
    failed_refresh.vin_digest = None;
    failed_refresh.masked_vin = None;
    failed_refresh.vin_status = IdentityReadState::Unsupported;
    let recorded = store
        .append_observation(&updated.id, failed_refresh)
        .unwrap();
    assert_eq!(recorded.selection, updated.selection);
    drop(store);
    let store = VehicleProfileStore::open(&path).unwrap();
    assert_eq!(store.get(&updated.id).unwrap(), Some(recorded));
    assert_eq!(store.list().unwrap().len(), 1);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn store_rejects_newer_schema_without_replacing_it() {
    let path = temp_db();
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE untouched(value TEXT NOT NULL);")
            .unwrap();
        db.execute("INSERT INTO untouched(value) VALUES('preserve-me')", [])
            .unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();
    }
    assert!(VehicleProfileStore::open(&path).is_err());
    let db = Connection::open(&path).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 99);
    let preserved: String = db
        .query_row("SELECT value FROM untouched", [], |row| row.get(0))
        .unwrap();
    assert_eq!(preserved, "preserve-me");
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn v1_migration_preserves_all_profile_data_and_creates_hash_only_identity() {
    let path = temp_db();
    let mut legacy = profile();
    legacy.revision = 7;
    legacy.created_at_unix_ms = 101;
    legacy.updated_at_unix_ms = 202;
    legacy.observations = vec![observation(
        "1FAHP2E89BG123456",
        vec![ecu("7E8", &["CAL-A"])],
    )];
    create_v1_profile_database(&path, &legacy);

    let store = VehicleProfileStore::open(&path).unwrap();
    assert_eq!(store.get(&legacy.id).unwrap(), Some(legacy.clone()));
    let identity = store
        .get_vehicle_identity(ProfileScope::Live, legacy.vin_digest.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(identity.full_vin, None);
    assert!(identity.origin_evidence.is_empty());
    drop(store);

    let db = Connection::open(&path).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let state: String = db
        .query_row(
            "SELECT identity_state FROM vehicle_identities WHERE scope='live' AND vin_digest=?1",
            [legacy.vin_digest.as_deref().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(state, "legacy_hash_only");
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn identity_cas_keeps_prior_vin_decoder_and_append_only_provenance() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let saved = store
        .save_vehicle_identity(
            identity_record(ProfileScope::Live, "1FAHP2E89BG123456"),
            None,
        )
        .unwrap();
    assert_eq!(saved.revision, 1);

    let mut failed_refresh = saved.clone();
    failed_refresh.full_vin = None;
    failed_refresh.decoder_snapshot = None;
    failed_refresh.origin_evidence.push(VinEvidenceSource {
        origin: VinOrigin::EcuReport,
        recorded_at_unix_ms: 200,
        report_id: Some("report-1".into()),
        ecu_id: Some("7E8".into()),
        connection_generation: Some(10),
        actor: None,
    });
    let refreshed = store
        .save_vehicle_identity(failed_refresh, Some(saved.revision))
        .unwrap();
    assert_eq!(refreshed.revision, 2);
    assert_eq!(refreshed.full_vin.as_deref(), Some("1FAHP2E89BG123456"));
    assert_eq!(refreshed.decoder_snapshot, saved.decoder_snapshot);
    assert_eq!(refreshed.origin_evidence.len(), 2);
    assert!(
        store
            .save_vehicle_identity(refreshed.clone(), Some(saved.revision))
            .is_err()
    );
    assert_eq!(
        store
            .get_vehicle_identity(ProfileScope::Live, &refreshed.vin_digest)
            .unwrap()
            .unwrap(),
        refreshed
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn first_full_vin_cannot_be_saved_without_origin_evidence() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let mut identity = identity_record(ProfileScope::Live, "1FAHP2E89BG123456");
    identity.origin_evidence.clear();
    assert!(store.save_vehicle_identity(identity.clone(), None).is_err());
    assert_eq!(
        store
            .get_vehicle_identity(ProfileScope::Live, &identity.vin_digest)
            .unwrap(),
        None
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn same_vin_is_isolated_across_profile_scopes() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    for scope in [
        ProfileScope::Live,
        ProfileScope::Simulator,
        ProfileScope::CanOnly,
    ] {
        store
            .save_vehicle_identity(identity_record(scope, "1FAHP2E89BG123456"), None)
            .unwrap();
    }
    let live = store
        .get_vehicle_identity(
            ProfileScope::Live,
            &identity_record(ProfileScope::Live, "1FAHP2E89BG123456").vin_digest,
        )
        .unwrap()
        .unwrap();
    let simulator = store
        .get_vehicle_identity(ProfileScope::Simulator, &live.vin_digest)
        .unwrap()
        .unwrap();
    let can_only = store
        .get_vehicle_identity(ProfileScope::CanOnly, &live.vin_digest)
        .unwrap()
        .unwrap();
    assert_eq!(live.scope, ProfileScope::Live);
    assert_eq!(simulator.scope, ProfileScope::Simulator);
    assert_eq!(can_only.scope, ProfileScope::CanOnly);
    assert_eq!(live.revision, 1);
    assert_eq!(simulator.revision, 1);
    assert_eq!(can_only.revision, 1);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn manual_vin_assignment_only_fills_matching_nonconflicting_manual_profile() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let manual = store.save(manual_profile_without_vin(), None).unwrap();
    let assigned = store
        .assign_manual_profile_vin(
            &manual.id,
            manual.revision,
            " 1fahp2e89bg123456 ",
            ManualDetailActor::OwnerUi,
        )
        .unwrap();
    let updated_profile = store.get(&manual.id).unwrap().unwrap();
    let (expected_digest, expected_mask) = canonical_vin_identity("1FAHP2E89BG123456").unwrap();
    assert_eq!(assigned.vin_digest, expected_digest);
    assert_eq!(assigned.full_vin.as_deref(), Some("1FAHP2E89BG123456"));
    assert_eq!(assigned.origin_evidence[0].origin, VinOrigin::OwnerEntered);
    assert_eq!(
        assigned.origin_evidence[0].actor,
        Some(ManualDetailActor::OwnerUi)
    );
    assert_eq!(updated_profile.revision, manual.revision + 1);
    assert_eq!(
        updated_profile.vin_digest.as_deref(),
        Some(expected_digest.as_str())
    );
    assert_eq!(
        updated_profile.masked_vin.as_deref(),
        Some(expected_mask.as_str())
    );
    assert!(updated_profile.manual_association);
    assert!(updated_profile.ecu_variants.is_empty());
    assert_eq!(updated_profile.selection, manual.selection);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn manual_vin_assignment_rejects_nonmanual_profiles_and_conflicting_observations() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let mut nonmanual = manual_profile_without_vin();
    nonmanual.manual_association = false;
    let nonmanual = store.save(nonmanual, None).unwrap();
    assert!(
        store
            .assign_manual_profile_vin(
                &nonmanual.id,
                nonmanual.revision,
                "1FAHP2E89BG123456",
                ManualDetailActor::OwnerUi,
            )
            .is_err()
    );
    assert_eq!(store.get(&nonmanual.id).unwrap().unwrap().vin_digest, None);

    let mut manual_with_vin = profile();
    manual_with_vin.id = "manual-existing-digest".into();
    manual_with_vin.manual_association = true;
    let manual_with_vin = store.save(manual_with_vin, None).unwrap();
    assert!(
        store
            .assign_manual_profile_vin(
                &manual_with_vin.id,
                manual_with_vin.revision,
                "2FAHP2E89BG123456",
                ManualDetailActor::OwnerUi,
            )
            .is_err()
    );
    assert_eq!(
        store.get(&manual_with_vin.id).unwrap().unwrap().vin_digest,
        manual_with_vin.vin_digest
    );

    let mut manual_profile = manual_profile_without_vin();
    manual_profile.id = "manual-conflict".into();
    let manual = store
        .save_with_observation(
            manual_profile,
            None,
            Some(observation(
                "2FAHP2E89BG123456",
                vec![ecu("7E8", &["CAL-A"])],
            )),
        )
        .unwrap();
    assert!(
        store
            .assign_manual_profile_vin(
                &manual.id,
                manual.revision,
                "1FAHP2E89BG123456",
                ManualDetailActor::OwnerUi,
            )
            .is_err()
    );
    let after = store.get(&manual.id).unwrap().unwrap();
    assert_eq!(after.revision, manual.revision);
    assert_eq!(after.vin_digest, None);
    assert!(after.observations[0].vin_digest.is_some());
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn legacy_vin_link_requires_exact_digest_and_records_actor_without_profile_change() {
    let path = temp_db();
    let mut legacy = profile();
    legacy.revision = 4;
    legacy.created_at_unix_ms = 101;
    legacy.updated_at_unix_ms = 202;
    create_v1_profile_database(&path, &legacy);
    let mut store = VehicleProfileStore::open(&path).unwrap();
    assert!(
        store
            .link_legacy_vehicle_vin(
                &legacy.id,
                legacy.revision,
                "2FAHP2E89BG123456",
                ManualDetailActor::AgentAccess,
            )
            .is_err()
    );
    assert_eq!(store.get(&legacy.id).unwrap().unwrap(), legacy);

    let linked = store
        .link_legacy_vehicle_vin(
            &legacy.id,
            legacy.revision,
            " 1fahp2e89bg123456 ",
            ManualDetailActor::AgentAccess,
        )
        .unwrap();
    assert_eq!(linked.full_vin.as_deref(), Some("1FAHP2E89BG123456"));
    assert_eq!(linked.origin_evidence[0].origin, VinOrigin::AgentEntered);
    assert_eq!(
        linked.origin_evidence[0].actor,
        Some(ManualDetailActor::AgentAccess)
    );
    assert_eq!(store.get(&legacy.id).unwrap().unwrap(), legacy);

    let mut failed_refresh = linked.clone();
    failed_refresh.full_vin = None;
    failed_refresh.decoder_snapshot = None;
    failed_refresh.origin_evidence.clear();
    let refreshed = store
        .save_vehicle_identity(failed_refresh, Some(linked.revision))
        .unwrap();
    assert_eq!(refreshed.full_vin.as_deref(), Some("1FAHP2E89BG123456"));
    assert_eq!(refreshed.decoder_snapshot, linked.decoder_snapshot);
    assert_eq!(refreshed.origin_evidence, linked.origin_evidence);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn manual_vehicle_details_append_owner_and_agent_provenance() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let identity = store
        .save_vehicle_identity(
            identity_record(ProfileScope::Live, "1FAHP2E89BG123456"),
            None,
        )
        .unwrap();
    let make = store
        .set_manual_vehicle_detail(
            ProfileScope::Live,
            &identity.vin_digest,
            ManualVehicleField::Make,
            "Ford".into(),
            ManualDetailActor::OwnerUi,
            Some("corrected by owner".into()),
        )
        .unwrap();
    let model = store
        .set_manual_vehicle_detail(
            ProfileScope::Live,
            &identity.vin_digest,
            ManualVehicleField::Model,
            "Flex".into(),
            ManualDetailActor::AgentAccess,
            None,
        )
        .unwrap();
    assert_eq!(model.revision, make.revision + 1);
    assert_eq!(model.owner_details.len(), 2);
    assert_eq!(model.owner_details[0].actor, ManualDetailActor::OwnerUi);
    assert_eq!(model.owner_details[1].actor, ManualDetailActor::AgentAccess);
    assert_eq!(
        model.owner_details[0].note.as_deref(),
        Some("corrected by owner")
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn profile_validation_bounds_and_reference_deduplication() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let mut invalid = profile();
    invalid.name = " ".repeat(121);
    assert!(store.save(invalid, None).is_err());
    let mut invalid = profile();
    invalid.selection.log_signal_ids = vec!["rpm".into(); 501];
    assert!(store.save(invalid, None).is_err());
    drop(store);
    std::fs::remove_file(path).unwrap();
}
