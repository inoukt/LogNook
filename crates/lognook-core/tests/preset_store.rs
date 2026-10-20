use lognook_core::{
    ManualDetailActor, ManualVehicleField, PresetApplyMode, PresetContext, PresetOrigin,
    PresetPickerState, PresetScope, PresetSize, PresetTaskFamily, PresetVehicleCriteria,
    ProfileScope, ProfileSelectionSnapshot, SignalPreset, VehicleIdentityRecord, VehicleProfile,
    VehicleProfileStore, builtin_signal_presets, canonical_vin_identity, scope_match_rank,
};
use rusqlite::Connection;
use std::path::PathBuf;

fn db_path() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    std::env::temp_dir().join(format!(
        "lognook-preset-store-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}
fn generic(id: &str) -> SignalPreset {
    SignalPreset {
        id: id.into(),
        revision: 0,
        name: "Boost research".into(),
        family: PresetTaskFamily::EngineTuning,
        size: PresetSize::Core,
        origin: PresetOrigin::Custom,
        scope: PresetScope::Generic,
        criteria: PresetVehicleCriteria::default(),
        channels: vec![],
        selection: Some(ProfileSelectionSnapshot::default()),
        required_adapter: None,
        required_protocol: None,
    }
}
fn profile() -> VehicleProfile {
    VehicleProfile {
        id: "flex-profile".into(),
        name: "Flex".into(),
        revision: 0,
        scope: ProfileScope::Live,
        catalog_vehicle_id: None,
        vin_digest: None,
        masked_vin: None,
        manual_association: false,
        ecu_variants: vec![],
        selection: ProfileSelectionSnapshot::default(),
        observations: vec![],
        created_at_unix_ms: 0,
        updated_at_unix_ms: 0,
    }
}
#[test]
fn custom_presets_are_revision_checked_and_builtin_ids_are_copy_only() {
    let path = db_path();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    assert_eq!(
        store.list_signal_presets().unwrap().len(),
        builtin_signal_presets().len()
    );
    let saved = store
        .save_signal_preset(generic("custom-boost"), None)
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(
        store.get_signal_preset("custom-boost").unwrap(),
        Some(saved.clone())
    );
    let mut edit = saved.clone();
    edit.name = "Updated".into();
    let edit = store
        .save_signal_preset(edit, Some(saved.revision.into()))
        .unwrap();
    assert_eq!(edit.revision, 2);
    assert!(store.save_signal_preset(saved.clone(), Some(1)).is_err());
    let mut builtin = generic("builtin.engine-tuning.core");
    builtin.origin = PresetOrigin::Builtin;
    assert!(store.save_signal_preset(builtin, None).is_err());
    let mut copy = store
        .get_signal_preset("builtin.engine-tuning.core")
        .unwrap()
        .unwrap();
    copy.id = "custom.engine-tuning.copy".into();
    copy.origin = PresetOrigin::Builtin;
    let copy = store.save_signal_preset(copy, None).unwrap();
    assert_eq!(copy.origin, PresetOrigin::Custom);
    assert!(copy.selection.is_some());
    assert!(!copy.selection.unwrap().log_signal_ids.is_empty());
    assert!(
        store
            .delete_signal_preset("builtin.engine-tuning.core", None)
            .is_err()
    );
    assert!(store.delete_signal_preset("custom-boost", Some(1)).is_err());
    store.delete_signal_preset("custom-boost", Some(2)).unwrap();
    drop(store);
    let _ = std::fs::remove_file(path);
}
#[test]
fn exact_profile_presets_require_an_existing_profile_and_do_not_match_sibling_profile() {
    let path = db_path();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    store.save(profile(), None).unwrap();
    let mut p = generic("flex-only");
    p.scope = PresetScope::ExactProfile;
    p.criteria = PresetVehicleCriteria {
        make: Some("Ford".into()),
        model: Some("Flex".into()),
        year: Some(2011),
        engine: Some("3.5L EcoBoost".into()),
        transmission: None,
        profile_id: Some("flex-profile".into()),
    };
    assert!(store.save_signal_preset(p.clone(), None).is_ok());
    let mut model = generic("ford-flex-model");
    model.scope = PresetScope::ModelPowertrain;
    model.criteria = PresetVehicleCriteria {
        make: Some("Ford".into()),
        model: Some("Flex".into()),
        year: Some(2011),
        engine: Some("3.5L EcoBoost".into()),
        transmission: None,
        profile_id: None,
    };
    let model = store.save_signal_preset(model, None).unwrap();
    assert_eq!(
        store.get_signal_preset(&model.id).unwrap(),
        Some(model.clone())
    );
    let same_make_model_other_profile = lognook_core::PresetContext {
        make: Some("Ford".into()),
        model: Some("Flex".into()),
        year: Some(2011),
        engine: Some("3.5L EcoBoost".into()),
        transmission: None,
        profile_id: Some("other-profile".into()),
        revision: 1,
        connection_generation: None,
    };
    assert_eq!(
        scope_match_rank(model.scope, &model.criteria, &same_make_model_other_profile),
        Some(1)
    );
    assert_eq!(
        scope_match_rank(p.scope, &p.criteria, &same_make_model_other_profile),
        None
    );
    p.id = "missing-profile".into();
    p.criteria.profile_id = Some("other-profile".into());
    assert!(store.save_signal_preset(p, None).is_err());
    drop(store);
    let _ = std::fs::remove_file(path);
}
#[test]
fn picker_context_and_staged_intent_round_trip_without_connection_generation() {
    let path = db_path();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let staged_preset = store
        .save_signal_preset(generic("custom-boost"), None)
        .unwrap();
    let state = PresetPickerState {
        revision: 0,
        context: PresetContext {
            make: Some("Ford".into()),
            model: Some("Flex".into()),
            year: Some(2011),
            engine: Some("3.5L EcoBoost".into()),
            transmission: Some("automatic".into()),
            profile_id: Some("profile-1".into()),
            revision: 7,
            connection_generation: Some(42),
        },
        staged: Some(lognook_core::StagedPresetIntent {
            preset_id: "custom-boost".into(),
            preset_revision: staged_preset.revision,
            mode: PresetApplyMode::Add,
        }),
    };
    let saved = store.save_preset_picker_state(state, Some(0)).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.staged.as_ref().unwrap().mode, PresetApplyMode::Add);
    assert_eq!(saved.context.connection_generation, None);
    assert_eq!(store.get_preset_picker_state().unwrap(), saved);
    assert!(
        store
            .save_preset_picker_state(saved.clone(), Some(0))
            .is_err()
    );
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn picker_save_rejects_staged_presets_that_were_updated_or_deleted() {
    let path = db_path();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    for (id, delete) in [("stale-stage", false), ("deleted-stage", true)] {
        let original = store.save_signal_preset(generic(id), None).unwrap();
        if delete {
            store
                .delete_signal_preset(id, Some(original.revision.into()))
                .unwrap();
        } else {
            let mut edited = original.clone();
            edited.name.push_str(" updated");
            store
                .save_signal_preset(edited, Some(original.revision.into()))
                .unwrap();
        }
        let stale_state = PresetPickerState {
            revision: 0,
            context: PresetContext::default(),
            staged: Some(lognook_core::StagedPresetIntent {
                preset_id: id.into(),
                preset_revision: original.revision,
                mode: PresetApplyMode::Replace,
            }),
        };
        assert!(
            store
                .save_preset_picker_state(stale_state, Some(0))
                .is_err()
        );
        assert_eq!(store.get_preset_picker_state().unwrap().revision, 0);
    }
    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn v2_migration_preserves_existing_profile_and_manual_transmission_actor() {
    let path = db_path();
    {
        let mut store = VehicleProfileStore::open(&path).unwrap();
        store.save(profile(), None).unwrap();
        let (digest, _) = canonical_vin_identity("1FAHP2E89BG123456").unwrap();
        store
            .save_vehicle_identity(
                VehicleIdentityRecord {
                    scope: ProfileScope::Live,
                    vin_digest: digest.clone(),
                    full_vin: None,
                    origin_evidence: vec![],
                    decoder_snapshot: None,
                    owner_details: vec![],
                    revision: 0,
                },
                None,
            )
            .unwrap();
        store
            .set_manual_vehicle_detail(
                ProfileScope::Live,
                &digest,
                ManualVehicleField::Transmission,
                "CVT".into(),
                ManualDetailActor::AgentAccess,
                None,
            )
            .unwrap();
    }
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE vehicle_project_registry; DROP TABLE vehicle_project_workspaces; DROP TABLE custom_signal_presets; DROP TABLE preset_picker_state; PRAGMA user_version=2;").unwrap();
    }
    let store = VehicleProfileStore::open(&path).unwrap();
    assert_eq!(store.get("flex-profile").unwrap().unwrap().name, "Flex");
    let identity = store
        .get_vehicle_identity(
            ProfileScope::Live,
            &canonical_vin_identity("1FAHP2E89BG123456").unwrap().0,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        identity.owner_details[0].field,
        ManualVehicleField::Transmission
    );
    assert_eq!(
        identity.owner_details[0].actor,
        ManualDetailActor::AgentAccess
    );
    drop(store);
    let db = Connection::open(&path).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
    drop(db);
    let _ = std::fs::remove_file(path);
}
