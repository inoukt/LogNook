use lognook_core::{
    ConnectionCapabilities, DeviceDescriptor, OemDefinitionReference, PresetCatalogEvidence,
    PresetContext, PresetScope, PresetSize, PresetTaskFamily, PresetVehicleCriteria,
    builtin_signal_presets, resolve_preset, scope_match_rank,
};

#[test]
fn builtins_are_generic_and_include_knock_intent() {
    let recipes = builtin_signal_presets();
    assert_eq!(recipes.len(), 8);
    for family in [
        PresetTaskFamily::EngineTuning,
        PresetTaskFamily::CvtDiagnostics,
        PresetTaskFamily::RoughnessMisfires,
        PresetTaskFamily::TrimsAirFuel,
    ] {
        let recipe = recipes
            .iter()
            .find(|r| r.family == family && r.size == PresetSize::Core)
            .unwrap();
        assert!(
            recipe
                .channels
                .iter()
                .any(|c| c.concept == "knock_correction")
        );
    }
}

#[test]
fn generic_resolution_is_safe_and_reports_task_coverage() {
    let recipes = builtin_signal_presets();
    let recipe = recipes
        .iter()
        .find(|r| r.family == PresetTaskFamily::CvtDiagnostics && r.size == PresetSize::Core)
        .unwrap();
    let caps = ConnectionCapabilities {
        device: DeviceDescriptor {
            stable_id: "test".into(),
            hardware_family: "generic".into(),
            firmware: None,
            path: None,
        },
        protocol: "SAE J1979 Mode 01".into(),
        read_only: true,
        diagnostics: Default::default(),
        supported_signals: lognook_core::standard_obd_signals()
            .into_iter()
            .filter(|s| s.id == "engine_rpm")
            .collect(),
    };
    let resolved = resolve_preset(recipe, &PresetContext::default(), &caps, &[]);
    assert!(
        resolved
            .eligible
            .iter()
            .any(|c| c.signal_id.as_deref() == Some("engine_rpm"))
    );
    assert_eq!(resolved.task_defining_eligible, 0);
    assert!(!resolved.skipped.is_empty());
}

#[test]
fn zero_values_are_not_filtered_and_knock_is_never_substituted() {
    let recipes = builtin_signal_presets();
    let recipe = recipes
        .iter()
        .find(|r| r.family == PresetTaskFamily::EngineTuning && r.size == PresetSize::Core)
        .unwrap();
    let mut caps = capabilities(&["engine_rpm", "timing_advance"]);
    // Support is a definition list; the current sample value is irrelevant to resolution.
    caps.supported_signals = lognook_core::standard_obd_signals()
        .into_iter()
        .filter(|s| ["engine_rpm", "timing_advance"].contains(&s.id.as_str()))
        .collect();
    let resolved = resolve_preset(recipe, &PresetContext::default(), &caps, &[]);
    assert!(
        resolved
            .eligible
            .iter()
            .any(|c| c.signal_id.as_deref() == Some("engine_rpm"))
    );
    assert!(
        resolved
            .skipped
            .iter()
            .any(|c| c.concept == "knock_correction")
    );
    assert!(
        !resolved
            .eligible
            .iter()
            .any(|c| c.concept == "knock_correction")
    );
}

#[test]
fn mismatched_transport_and_unverified_oem_stay_skipped() {
    let mut preset = builtin_signal_presets().remove(0);
    preset.required_protocol = Some("SSM K-Line".into());
    let resolved = resolve_preset(
        &preset,
        &PresetContext::default(),
        &capabilities(&["engine_rpm"]),
        &[],
    );
    assert!(
        resolved
            .skipped
            .iter()
            .all(|c| c.reason == Some(lognook_core::PresetSkipReason::ProtocolMismatch))
    );
    let mut preset = builtin_signal_presets().remove(0);
    preset.required_adapter = Some("OpenPort".into());
    let resolved = resolve_preset(
        &preset,
        &PresetContext::default(),
        &capabilities(&["engine_rpm"]),
        &[],
    );
    assert!(
        resolved
            .skipped
            .iter()
            .all(|c| c.reason == Some(lognook_core::PresetSkipReason::AdapterMismatch))
    );

    let mut preset = builtin_signal_presets().remove(0);
    let reference = OemDefinitionReference {
        candidate_id: "candidate-1".into(),
        source_id: "repo".into(),
        source_revision: "blob-sha".into(),
        decoder_revision: "decoder-v1".into(),
    };
    preset
        .channels
        .iter_mut()
        .find(|c| c.concept == "knock_correction")
        .unwrap()
        .oem_alternatives
        .push(reference.clone());
    let evidence = [PresetCatalogEvidence {
        reference,
        vehicle_verified: false,
        supported: true,
        profile_id: Some("vehicle-1".into()),
        context_revision: 1,
        connection_generation: 1,
    }];
    let resolved = resolve_preset(
        &preset,
        &PresetContext::default(),
        &capabilities(&[]),
        &evidence,
    );
    assert!(
        resolved
            .skipped
            .iter()
            .any(|c| c.concept == "knock_correction"
                && c.reason == Some(lognook_core::PresetSkipReason::OemNotVerified))
    );
    assert!(resolved.selection.oem_references.is_empty());
}

#[test]
fn scope_ranking_and_knock_context_are_explicit() {
    let context = PresetContext {
        make: Some("Toyota".into()),
        model: Some("xB".into()),
        year: Some(2014),
        engine: Some("2.4L".into()),
        transmission: Some("CVT".into()),
        profile_id: Some("xb-1".into()),
        revision: 9,
        connection_generation: None,
    };
    let generic = PresetVehicleCriteria::default();
    let exact = PresetVehicleCriteria {
        make: Some("Toyota".into()),
        model: Some("xB".into()),
        year: Some(2014),
        engine: Some("2.4L".into()),
        transmission: Some("CVT".into()),
        profile_id: Some("xb-1".into()),
    };
    assert!(
        scope_match_rank(PresetScope::ExactProfile, &exact, &context).unwrap()
            > scope_match_rank(PresetScope::Generic, &generic, &context).unwrap()
    );
    assert!(scope_match_rank(PresetScope::ModelPowertrain, &generic, &context).is_none());
    let cvt = builtin_signal_presets()
        .into_iter()
        .find(|r| r.family == PresetTaskFamily::CvtDiagnostics && r.size == PresetSize::Core)
        .unwrap();
    let caps = capabilities(&["engine_rpm", "control_module_voltage"]);
    let result = resolve_preset(&cvt, &context, &caps, &[]);
    assert_eq!(result.task_defining_eligible, 0);
    assert!(
        result
            .skipped
            .iter()
            .any(|c| c.concept == "knock_correction")
    );
    assert!(
        result
            .eligible
            .iter()
            .all(|c| c.concept != "knock_correction")
    );
}

#[test]
fn knock_subconcepts_remain_distinct_and_recipes_serialize() {
    let recipes = builtin_signal_presets();
    let extended = recipes
        .iter()
        .find(|r| r.family == PresetTaskFamily::RoughnessMisfires && r.size == PresetSize::Extended)
        .unwrap();
    for concept in [
        "knock_correction",
        "learned_knock_correction",
        "knock_counts",
        "advance_multiplier",
    ] {
        assert_eq!(
            extended
                .channels
                .iter()
                .filter(|c| c.concept == concept)
                .count(),
            1
        );
    }
    for recipe in recipes {
        let json = serde_json::to_string(&recipe).unwrap();
        let _: lognook_core::SignalPreset = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn exact_oem_evidence_is_bound_to_profile_and_context_revision() {
    let mut preset = builtin_signal_presets().remove(0);
    let reference = OemDefinitionReference {
        candidate_id: "oem-knock".into(),
        source_id: "source".into(),
        source_revision: "blob".into(),
        decoder_revision: "decoder".into(),
    };
    preset
        .channels
        .iter_mut()
        .find(|c| c.concept == "knock_correction")
        .unwrap()
        .oem_alternatives
        .push(reference.clone());
    let context = PresetContext {
        profile_id: Some("vehicle-1".into()),
        revision: 7,
        connection_generation: Some(12),
        ..Default::default()
    };
    let evidence = [PresetCatalogEvidence {
        reference: reference.clone(),
        vehicle_verified: true,
        supported: true,
        profile_id: Some("vehicle-1".into()),
        context_revision: 7,
        connection_generation: 12,
    }];
    let resolved = resolve_preset(&preset, &context, &capabilities(&[]), &evidence);
    assert!(
        resolved
            .eligible
            .iter()
            .any(|c| c.concept == "knock_correction")
    );
    assert_eq!(resolved.selection.oem_references, vec![reference.clone()]);
    assert!(
        resolved
            .selection
            .log_signal_ids
            .contains(&"oem:oem-knock".into())
    );
    let wrong_vehicle = [PresetCatalogEvidence {
        profile_id: Some("vehicle-2".into()),
        ..evidence[0].clone()
    }];
    let wrong_revision = [PresetCatalogEvidence {
        context_revision: 6,
        ..evidence[0].clone()
    }];
    let wrong_connection = [PresetCatalogEvidence {
        connection_generation: 11,
        ..evidence[0].clone()
    }];
    for stale in [
        &wrong_vehicle[..],
        &wrong_revision[..],
        &wrong_connection[..],
    ] {
        let result = resolve_preset(&preset, &context, &capabilities(&[]), stale);
        assert!(
            !result
                .eligible
                .iter()
                .any(|c| c.concept == "knock_correction")
        );
        assert!(result.selection.oem_references.is_empty());
    }
}

#[test]
fn saved_snapshot_controls_selection_even_if_origin_is_spoofed_builtin() {
    let mut preset = builtin_signal_presets().remove(0);
    let reference = OemDefinitionReference {
        candidate_id: "verified-knock".into(),
        source_id: "source".into(),
        source_revision: "blob".into(),
        decoder_revision: "decoder".into(),
    };
    preset
        .channels
        .iter_mut()
        .find(|c| c.concept == "knock_correction")
        .unwrap()
        .oem_alternatives
        .push(reference.clone());
    preset.selection = Some(lognook_core::ProfileSelectionSnapshot {
        log_signal_ids: vec!["engine_rpm".into()],
        graph_signal_ids: vec!["engine_rpm".into()],
        ..Default::default()
    });
    // Origin is serialized data and cannot override an explicit selection snapshot.
    preset.origin = lognook_core::PresetOrigin::Builtin;
    let context = PresetContext {
        profile_id: Some("vehicle-1".into()),
        revision: 3,
        connection_generation: Some(8),
        ..Default::default()
    };
    let evidence = [PresetCatalogEvidence {
        reference,
        vehicle_verified: true,
        supported: true,
        profile_id: Some("vehicle-1".into()),
        context_revision: 3,
        connection_generation: 8,
    }];
    let result = resolve_preset(&preset, &context, &capabilities(&["engine_rpm"]), &evidence);
    assert!(
        result
            .eligible
            .iter()
            .any(|c| c.concept == "knock_correction")
    );
    assert_eq!(result.selection.log_signal_ids, vec!["engine_rpm"]);
    assert_eq!(result.selection.graph_signal_ids, vec!["engine_rpm"]);
    assert!(result.selection.oem_references.is_empty());
    assert!(
        !result
            .selection
            .log_signal_ids
            .iter()
            .any(|id| id == "oem:verified-knock")
    );
}

#[test]
fn custom_log_graph_intent_is_filtered_and_snapshot_payload_is_not_applied() {
    let mut preset = builtin_signal_presets().remove(0);
    preset.origin = lognook_core::PresetOrigin::Custom;
    preset.selection = Some(lognook_core::ProfileSelectionSnapshot {
        log_signal_ids: vec!["engine_rpm".into()],
        graph_signal_ids: vec!["engine_load".into()],
        derived_definitions: vec![lognook_core::DerivedSignalDefinition {
            id: "unsafe-derived".into(),
            name: "unsafe".into(),
            expression: "engine_rpm".into(),
            unit: "rpm".into(),
            tare_offset: 0.0,
        }],
        can_fields: vec![lognook_core::CanSignalDefinition {
            id: "raw-can".into(),
            name: "raw".into(),
            can_id: 1,
            extended: false,
            start_bit: 0,
            bit_length: 8,
            byte_order: lognook_core::CanByteOrder::Intel,
            signed: false,
            factor: 1.0,
            offset: 0.0,
            unit: "x".into(),
        }],
        ..Default::default()
    });
    let result = resolve_preset(
        &preset,
        &PresetContext::default(),
        &capabilities(&["engine_rpm", "engine_load"]),
        &[],
    );
    assert_eq!(result.selection.log_signal_ids, vec!["engine_rpm"]);
    assert_eq!(result.selection.graph_signal_ids, vec!["engine_load"]);
    assert!(result.selection.derived_definitions.is_empty());
    assert!(result.selection.can_fields.is_empty());
}

#[test]
fn companion_concepts_are_present_and_canonicalized() {
    let presets = builtin_signal_presets();
    let engine = presets
        .iter()
        .find(|p| p.id == "builtin.engine-tuning.extended")
        .unwrap();
    for concept in [
        "boost_target",
        "boost_actual",
        "map_target",
        "map_actual",
        "lambda_commanded",
        "lambda_actual",
        "o2_voltage_b1s1",
        "cam_angle_target",
        "cam_angle_actual",
    ] {
        assert!(
            engine.channels.iter().any(|c| c.concept == concept),
            "missing {concept}"
        );
    }
    for preset in presets {
        assert_eq!(
            preset
                .channels
                .iter()
                .filter(|c| c.concept == "iat")
                .count(),
            1
        );
        assert!(!preset.channels.iter().any(|c| c.concept == "intake_temp"));
    }
    assert_eq!(builtin_signal_presets().len(), 8);
}

fn capabilities(ids: &[&str]) -> ConnectionCapabilities {
    ConnectionCapabilities {
        device: DeviceDescriptor {
            stable_id: "test".into(),
            hardware_family: "generic".into(),
            firmware: None,
            path: None,
        },
        protocol: "SAE J1979 Mode 01".into(),
        read_only: true,
        diagnostics: Default::default(),
        supported_signals: lognook_core::standard_obd_signals()
            .into_iter()
            .filter(|s| ids.contains(&s.id.as_str()))
            .collect(),
    }
}
