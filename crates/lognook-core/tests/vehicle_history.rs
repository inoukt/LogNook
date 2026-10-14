use lognook_core::vehicle_profiles::{
    DiagnosticDtcCounts, DiagnosticHistoryFilter, DiagnosticReportLocator, HistoryImportCheckpoint,
    HistoryIndexState, HistoryIndexStatus, ManualDetailActor, ProfileScope,
    VehicleIdentitySnapshot, VehicleIdentitySource, VehicleProfileStore, canonical_vin_identity,
};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_db() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-history-{id}.sqlite"))
}

fn snapshot(
    scope: ProfileScope,
    vin: Option<&str>,
    source: VehicleIdentitySource,
    conflict: bool,
) -> VehicleIdentitySnapshot {
    let (vin_digest, masked_vin) = vin
        .map(canonical_vin_identity)
        .transpose()
        .unwrap()
        .map(|(digest, mask)| (Some(digest), Some(mask)))
        .unwrap_or((None, None));
    VehicleIdentitySnapshot {
        scope,
        connection_generation: Some(9),
        profile_id: None,
        profile_revision: None,
        full_vin: vin.map(str::to_owned),
        vin_digest,
        masked_vin,
        origin: None,
        identity_source: source,
        vin_conflict: conflict,
        observed_ecus: vec![],
        decoded_snapshot: None,
        owner_details: vec![],
    }
}

fn locator(
    report_id: &str,
    started_at_unix_ms: u64,
    scope: ProfileScope,
    vin: Option<&str>,
    source: VehicleIdentitySource,
    conflict: bool,
    ecu_ids: &[&str],
) -> DiagnosticReportLocator {
    let (vin_digest, masked_vin) = vin
        .map(canonical_vin_identity)
        .transpose()
        .unwrap()
        .map(|(digest, mask)| (Some(digest), Some(mask)))
        .unwrap_or((None, None));
    DiagnosticReportLocator {
        report_id: report_id.into(),
        source_relative_path: "vehicle-reads.jsonl".into(),
        byte_offset: started_at_unix_ms.saturating_mul(1_000_000).saturating_add(
            report_id
                .rsplit('-')
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
                .saturating_mul(1_000),
        ),
        byte_length: 120,
        sha256: "a".repeat(64),
        started_at_unix_ms,
        scope,
        vin_digest: vin_digest.clone(),
        masked_vin,
        identity_source: source,
        vin_conflict: conflict,
        ecu_ids: ecu_ids.iter().map(|id| (*id).into()).collect(),
        dtc_counts: DiagnosticDtcCounts {
            stored: Some(0),
            pending: None,
            permanent: Some(2),
        },
        mode06_read_count: 3,
        context_snapshot: snapshot(scope, vin, source, conflict),
    }
}

fn all_history() -> DiagnosticHistoryFilter {
    DiagnosticHistoryFilter {
        scope: None,
        vin_digest: None,
        ecu_id: None,
        from_unix_ms: None,
        to_unix_ms: None,
        page_size: None,
        cursor: None,
    }
}

#[test]
fn locator_round_trips_and_conflicting_duplicate_does_not_replace_evidence() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let first = locator(
        "report-a",
        100,
        ProfileScope::Live,
        Some("1FAHP2E89BG123456"),
        VehicleIdentitySource::ReportEcu,
        false,
        &["7E8"],
    );
    store.add_diagnostic_locator(first.clone()).unwrap();
    store.add_diagnostic_locator(first.clone()).unwrap();
    assert_eq!(
        store.get_diagnostic_locator("report-a").unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        store.list_diagnostic_history(all_history()).unwrap().rows[0].dtc_counts,
        DiagnosticDtcCounts {
            stored: Some(0),
            pending: None,
            permanent: Some(2)
        }
    );

    let mut changed = first.clone();
    changed.byte_offset += 1;
    assert!(store.add_diagnostic_locator(changed).is_err());
    assert_eq!(
        store.get_diagnostic_locator("report-a").unwrap(),
        Some(first)
    );

    let mut duplicate_range = locator(
        "report-overlap",
        100,
        ProfileScope::Live,
        Some("1FAHP2E89BG123456"),
        VehicleIdentitySource::ReportEcu,
        false,
        &["7E8"],
    );
    duplicate_range.byte_offset += 1;
    assert!(store.add_diagnostic_locator(duplicate_range).is_err());

    let mut unsafe_path = locator(
        "report-b",
        101,
        ProfileScope::Live,
        None,
        VehicleIdentitySource::Unassigned,
        false,
        &[],
    );
    unsafe_path.source_relative_path = "../outside.jsonl".into();
    assert!(store.add_diagnostic_locator(unsafe_path).is_err());
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn locator_path_aliases_cannot_index_overlapping_bytes_twice() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let mut first = locator(
        "path-first",
        100,
        ProfileScope::Live,
        None,
        VehicleIdentitySource::Unassigned,
        false,
        &[],
    );
    first.source_relative_path = "Reports\\Vehicle-Reads.jsonl".into();
    first.byte_offset = 10_000;
    store.add_diagnostic_locator(first).unwrap();
    let stored = store.get_diagnostic_locator("path-first").unwrap().unwrap();
    let expected_path = if cfg!(windows) {
        "reports/vehicle-reads.jsonl"
    } else {
        "Reports/Vehicle-Reads.jsonl"
    };
    assert_eq!(stored.source_relative_path, expected_path);

    let mut alias = locator(
        "path-alias",
        101,
        ProfileScope::Live,
        None,
        VehicleIdentitySource::Unassigned,
        false,
        &[],
    );
    alias.source_relative_path = if cfg!(windows) {
        "reports/VEHICLE-READS.JSONL".into()
    } else {
        "Reports/Vehicle-Reads.jsonl".into()
    };
    alias.byte_offset = 10_050;
    assert!(store.add_diagnostic_locator(alias).is_err());
    assert!(
        store
            .get_diagnostic_locator("path-alias")
            .unwrap()
            .is_none()
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn locator_accepts_one_mib_and_rejects_oversize_and_unrepresentable_ranges() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let mut max_line = locator(
        "max-line",
        100,
        ProfileScope::Live,
        None,
        VehicleIdentitySource::Unassigned,
        false,
        &[],
    );
    max_line.byte_offset = 0;
    max_line.byte_length = 1_048_576;
    store.add_diagnostic_locator(max_line).unwrap();
    assert!(store.get_diagnostic_locator("max-line").unwrap().is_some());

    let mut oversized = locator(
        "oversized-line",
        101,
        ProfileScope::Live,
        None,
        VehicleIdentitySource::Unassigned,
        false,
        &[],
    );
    oversized.byte_offset = 1_048_576;
    oversized.byte_length = 1_048_577;
    assert!(store.add_diagnostic_locator(oversized).is_err());

    for (report_id, byte_offset) in [
        ("u64-overflow", u64::MAX),
        ("sqlite-end-overflow", i64::MAX as u64),
    ] {
        let mut unrepresentable = locator(
            report_id,
            102,
            ProfileScope::Live,
            None,
            VehicleIdentitySource::Unassigned,
            false,
            &[],
        );
        unrepresentable.byte_offset = byte_offset;
        unrepresentable.byte_length = 1;
        assert!(store.add_diagnostic_locator(unrepresentable).is_err());
    }
    assert!(
        store
            .get_diagnostic_locator("oversized-line")
            .unwrap()
            .is_none()
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn history_filters_and_pages_by_scope_vehicle_date_and_ecu_stably() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    for item in [
        locator(
            "report-a",
            100,
            ProfileScope::Live,
            Some("1FAHP2E89BG123456"),
            VehicleIdentitySource::ReportEcu,
            false,
            &["7E8"],
        ),
        locator(
            "report-b",
            200,
            ProfileScope::Live,
            Some("1FAHP2E89BG123456"),
            VehicleIdentitySource::SameGenerationRecognition,
            false,
            &["7E9"],
        ),
        locator(
            "report-c",
            300,
            ProfileScope::Simulator,
            Some("1FAHP2E89BG123456"),
            VehicleIdentitySource::ManualProfile,
            false,
            &["7E9"],
        ),
        locator(
            "report-d",
            400,
            ProfileScope::Live,
            Some("2FAHP2E89BG123456"),
            VehicleIdentitySource::ReportEcu,
            false,
            &["7E9"],
        ),
    ] {
        store.add_diagnostic_locator(item).unwrap();
    }
    let (digest, _) = canonical_vin_identity("1FAHP2E89BG123456").unwrap();
    let filtered = store
        .list_diagnostic_history(DiagnosticHistoryFilter {
            scope: Some(ProfileScope::Live),
            vin_digest: Some(digest),
            ecu_id: Some("7E9".into()),
            from_unix_ms: Some(150),
            to_unix_ms: Some(250),
            page_size: Some(10),
            cursor: None,
        })
        .unwrap();
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0].report_id, "report-b");

    for number in 0..51 {
        let report_id = format!("page-{number:03}");
        store
            .add_diagnostic_locator(locator(
                &report_id,
                500,
                ProfileScope::Live,
                None,
                VehicleIdentitySource::Unassigned,
                false,
                &["7E1"],
            ))
            .unwrap();
    }
    let first = store
        .list_diagnostic_history(DiagnosticHistoryFilter {
            ecu_id: Some("7E1".into()),
            ..all_history()
        })
        .unwrap();
    assert_eq!(first.rows.len(), 50);
    assert_eq!(first.rows[0].report_id, "page-050");
    let next = store
        .list_diagnostic_history(DiagnosticHistoryFilter {
            cursor: first.next_cursor,
            ecu_id: Some("7E1".into()),
            ..all_history()
        })
        .unwrap();
    assert_eq!(next.rows.len(), 1);
    assert_eq!(next.rows[0].report_id, "page-000");
    assert_eq!(next.next_cursor, None);

    for number in 0..101 {
        let report_id = format!("cap-{number:03}");
        store
            .add_diagnostic_locator(locator(
                &report_id,
                600,
                ProfileScope::Live,
                None,
                VehicleIdentitySource::Unassigned,
                false,
                &["7E2"],
            ))
            .unwrap();
    }
    let capped = store
        .list_diagnostic_history(DiagnosticHistoryFilter {
            page_size: Some(500),
            ecu_id: Some("7E2".into()),
            ..all_history()
        })
        .unwrap();
    assert_eq!(capped.rows.len(), 100);
    assert!(capped.next_cursor.is_some());
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn manual_association_preserves_conflict_and_rejects_cross_scope_link() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let conflicting = locator(
        "conflict-report",
        600,
        ProfileScope::Live,
        None,
        VehicleIdentitySource::Conflict,
        true,
        &["7E8", "7E9"],
    );
    store.add_diagnostic_locator(conflicting).unwrap();
    let (digest, _) = canonical_vin_identity("1FAHP2E89BG123456").unwrap();
    let association = store
        .append_manual_report_association(
            "conflict-report".into(),
            ProfileScope::Live,
            digest.clone(),
            ManualDetailActor::OwnerUi,
            Some("selected by owner".into()),
        )
        .unwrap();
    assert_eq!(association.actor, ManualDetailActor::OwnerUi);
    assert_eq!(association.vin_digest, digest);
    assert!(
        store
            .append_manual_report_association(
                "conflict-report".into(),
                ProfileScope::Simulator,
                association.vin_digest.clone(),
                ManualDetailActor::AgentAccess,
                None,
            )
            .is_err()
    );

    let rows = store
        .list_diagnostic_history(DiagnosticHistoryFilter {
            scope: Some(ProfileScope::Live),
            vin_digest: Some(association.vin_digest.clone()),
            ..all_history()
        })
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].report_id, "conflict-report");
    assert_eq!(rows[0].identity_source, VehicleIdentitySource::Conflict);
    assert!(rows[0].vin_conflict);
    assert_eq!(rows[0].vin_digest, None);
    assert_eq!(
        rows[0].evidence_status,
        lognook_core::vehicle_profiles::HistoryEvidenceStatus::Available
    );

    store
        .append_manual_report_association(
            "conflict-report".into(),
            ProfileScope::Live,
            association.vin_digest.clone(),
            ManualDetailActor::AgentAccess,
            None,
        )
        .unwrap();
    drop(store);
    let reopened = VehicleProfileStore::open(&path).unwrap();
    let associations = reopened
        .get_manual_report_associations("conflict-report")
        .unwrap();
    assert_eq!(associations.len(), 2);
    assert_eq!(associations[0].actor, ManualDetailActor::AgentAccess);
    assert_eq!(associations[0].note, None);
    assert_eq!(associations[1].actor, ManualDetailActor::OwnerUi);
    assert_eq!(associations[1].note.as_deref(), Some("selected by owner"));
    assert_eq!(associations[0].scope, ProfileScope::Live);
    assert_eq!(associations[1].scope, ProfileScope::Live);
    let unchanged = reopened
        .list_diagnostic_history(DiagnosticHistoryFilter {
            scope: Some(ProfileScope::Live),
            vin_digest: Some(association.vin_digest),
            ..all_history()
        })
        .unwrap()
        .rows;
    assert_eq!(unchanged.len(), 1);
    assert_eq!(
        unchanged[0].identity_source,
        VehicleIdentitySource::Conflict
    );
    assert!(unchanged[0].vin_conflict);
    assert_eq!(unchanged[0].vin_digest, None);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn history_import_checkpoint_keeps_source_cursor_and_partial_line_across_reopen() {
    let path = temp_db();
    let mut store = VehicleProfileStore::open(&path).unwrap();
    let initial = store.get_history_import_checkpoint().unwrap();
    assert_eq!(initial.status.state, HistoryIndexState::Idle);
    assert_eq!(initial.next_byte_offset, 0);
    assert_eq!(initial.trailing_partial_line, None);

    store
        .save_history_import_checkpoint(HistoryImportCheckpoint {
            status: HistoryIndexStatus {
                state: HistoryIndexState::Cancelled,
                indexed_reports: 3,
                scanned_bytes: 140,
                total_bytes: 160,
                malformed_lines: 1,
                oversized_lines: 0,
                error: None,
            },
            source_relative_path: Some("vehicle-reads.jsonl".into()),
            next_byte_offset: 140,
            trailing_partial_line: Some(br#"{"report_id":"incomplete""#.to_vec()),
        })
        .unwrap();
    drop(store);

    let reopened = VehicleProfileStore::open(&path).unwrap();
    let saved = reopened.get_history_import_checkpoint().unwrap();
    assert_eq!(saved.status.state, HistoryIndexState::Cancelled);
    assert_eq!(saved.status.total_bytes, 160);
    assert_eq!(saved.next_byte_offset, 140);
    assert_eq!(
        saved.trailing_partial_line,
        Some(br#"{"report_id":"incomplete""#.to_vec())
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
