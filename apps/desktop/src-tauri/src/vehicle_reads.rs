use crate::{AppState, commands::vehicle_profiles::identity_observation};
use lognook_core::{
    ConnectionCapabilities, DiagnosticDtcCounts, DiagnosticHistoryRow, DiagnosticOutcome,
    DiagnosticRead, DiagnosticReadKind, DiagnosticReport, DiagnosticReportDetail,
    DiagnosticReportLocator, DiagnosticValue, EcuProfileVariant, HistoryEvidenceStatus,
    HistoryImportCheckpoint, HistoryIndexState, HistoryIndexStatus, IdentityReadState,
    ProfileScope, VehicleIdentityObservation, VehicleIdentityRecord, VehicleIdentitySnapshot,
    VehicleIdentitySource, VehicleProfile, VinEvidenceSource, VinOrigin, canonical_vin_identity,
    decode_vin_offline,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const VEHICLE_READS_RELATIVE_PATH: &str = "diagnostics/vehicle-reads.jsonl";
static APPEND_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PositiveVinObservation {
    pub vin: String,
    pub ecu_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AppendedLine {
    pub byte_offset: u64,
    pub byte_length: u64,
    pub sha256: [u8; 32],
}

pub(crate) fn positive_vin_observations(reads: &[DiagnosticRead]) -> Vec<PositiveVinObservation> {
    let mut unique = HashSet::new();
    let mut result = Vec::new();
    for read in reads {
        if read.outcome != DiagnosticOutcome::Positive
            || read.kind != DiagnosticReadKind::Vin
            || read.service != 0x09
            || read.identifier != Some(0x02)
            || read.request_payload != [0x09, 0x02]
        {
            continue;
        }
        let raw_vin = decode_positive_vin_payload(&read.response_payload);
        let mut vins = read
            .values
            .iter()
            .filter_map(|value| match value {
                DiagnosticValue::Identity {
                    pid: 0x02, value, ..
                } => normalize_valid_vin(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(raw_vin) = raw_vin {
            if !vins.is_empty() && vins.iter().any(|vin| *vin != raw_vin) {
                continue;
            }
            if vins.is_empty() {
                vins.push(raw_vin);
            }
        }
        for vin in vins {
            let observation = PositiveVinObservation {
                vin,
                ecu_id: read
                    .ecu_id
                    .as_deref()
                    .filter(|id| !id.trim().is_empty())
                    .map(|id| id.trim().to_ascii_uppercase()),
            };
            if unique.insert((observation.vin.clone(), observation.ecu_id.clone())) {
                result.push(observation);
            }
        }
    }
    result
}

fn decode_positive_vin_payload(payload: &[u8]) -> Option<String> {
    if payload.len() != 20 || payload[..3] != [0x49, 0x02, 0x01] {
        return None;
    }
    normalize_valid_vin(std::str::from_utf8(&payload[3..]).ok()?)
}

fn normalize_valid_vin(value: &str) -> Option<String> {
    canonical_vin_identity(value).ok()?;
    Some(value.trim().to_ascii_uppercase())
}

pub(crate) fn report_context(
    report: &DiagnosticReport,
    scope: ProfileScope,
    generation: Option<u64>,
    origin: VinOrigin,
    recognition: Option<&VehicleIdentityObservation>,
    active_profile: Option<&VehicleProfile>,
    identities: &[VehicleIdentityRecord],
) -> VehicleIdentitySnapshot {
    let capabilities = ConnectionCapabilities {
        device: report.adapter.clone(),
        protocol: report.protocol.clone(),
        read_only: true,
        diagnostics: Default::default(),
        supported_signals: vec![],
    };
    let ecu_observation = identity_observation(
        &capabilities,
        generation.unwrap_or_default(),
        scope,
        &report.reads,
    );
    let observed_ecus: Vec<EcuProfileVariant> = ecu_observation.ecu_variants;
    let report_vins = positive_vin_observations(&report.reads)
        .into_iter()
        .map(|entry| entry.vin)
        .collect::<HashSet<_>>();

    if report_vins.len() > 1 {
        return empty_snapshot(
            scope,
            generation,
            observed_ecus,
            VehicleIdentitySource::Conflict,
            true,
        );
    }
    if let Some(vin) = report_vins.into_iter().next() {
        let (digest, masked) = match canonical_vin_identity(&vin) {
            Ok(value) => value,
            Err(_) => {
                return empty_snapshot(
                    scope,
                    generation,
                    observed_ecus,
                    VehicleIdentitySource::Unassigned,
                    false,
                );
            }
        };
        let record = identity_for(identities, scope, &digest)
            .filter(|record| record.full_vin.as_deref() == Some(vin.as_str()));
        let profile = matching_profile(active_profile, scope, &digest);
        return VehicleIdentitySnapshot {
            scope,
            connection_generation: generation,
            profile_id: profile.map(|p| p.id.clone()),
            profile_revision: profile.map(|p| p.revision),
            full_vin: Some(vin.clone()),
            vin_digest: Some(digest),
            masked_vin: Some(masked),
            origin: Some(origin),
            identity_source: VehicleIdentitySource::ReportEcu,
            vin_conflict: false,
            observed_ecus,
            decoded_snapshot: record
                .and_then(|r| r.decoder_snapshot.clone())
                .or_else(|| decode_vin_offline(&vin).ok()),
            owner_details: record.map_or_else(Vec::new, |r| r.owner_details.clone()),
        };
    }

    if let Some(observation) = recognition.filter(|observation| observation.scope == scope) {
        if observation.conflicting_vin_observations {
            return empty_snapshot(
                scope,
                generation,
                observed_ecus,
                VehicleIdentitySource::Conflict,
                true,
            );
        }
        if observation.vin_status == IdentityReadState::Positive {
            if let Some(digest) = observation.vin_digest.as_deref() {
                let record = identity_for(identities, scope, digest);
                let vin = record.as_ref().and_then(|r| r.full_vin.clone());
                let profile = matching_profile(active_profile, scope, digest);
                return VehicleIdentitySnapshot {
                    scope,
                    connection_generation: generation,
                    profile_id: profile.map(|p| p.id.clone()),
                    profile_revision: profile.map(|p| p.revision),
                    full_vin: vin.clone(),
                    vin_digest: Some(digest.to_owned()),
                    masked_vin: vin
                        .as_deref()
                        .and_then(|v| canonical_vin_identity(v).ok().map(|x| x.1))
                        .or_else(|| observation.masked_vin.clone()),
                    origin: Some(VinOrigin::EcuRecognition),
                    identity_source: VehicleIdentitySource::SameGenerationRecognition,
                    vin_conflict: false,
                    observed_ecus,
                    decoded_snapshot: record
                        .as_ref()
                        .and_then(|r| r.decoder_snapshot.clone())
                        .or_else(|| vin.as_deref().and_then(|v| decode_vin_offline(v).ok())),
                    owner_details: record.map_or_else(Vec::new, |r| r.owner_details.clone()),
                };
            }
        }
    }

    if let Some(profile) = active_profile.filter(|p| p.scope == scope && p.manual_association) {
        let record = profile
            .vin_digest
            .as_deref()
            .and_then(|digest| identity_for(identities, scope, digest));
        let full_vin = record.as_ref().and_then(|r| r.full_vin.clone());
        return VehicleIdentitySnapshot {
            scope,
            connection_generation: generation,
            profile_id: Some(profile.id.clone()),
            profile_revision: Some(profile.revision),
            full_vin: full_vin.clone(),
            vin_digest: profile.vin_digest.clone(),
            masked_vin: full_vin
                .as_deref()
                .and_then(|v| canonical_vin_identity(v).ok().map(|x| x.1))
                .or_else(|| profile.masked_vin.clone()),
            origin: Some(
                record
                    .as_ref()
                    .and_then(|r| r.origin_evidence.last().map(|e| e.origin))
                    .unwrap_or(VinOrigin::OwnerEntered),
            ),
            identity_source: VehicleIdentitySource::ManualProfile,
            vin_conflict: false,
            observed_ecus,
            decoded_snapshot: record
                .as_ref()
                .and_then(|r| r.decoder_snapshot.clone())
                .or_else(|| full_vin.as_deref().and_then(|v| decode_vin_offline(v).ok())),
            owner_details: record.map_or_else(Vec::new, |r| r.owner_details.clone()),
        };
    }

    empty_snapshot(
        scope,
        generation,
        observed_ecus,
        VehicleIdentitySource::Unassigned,
        false,
    )
}

pub(crate) fn sync_active_recording_context(
    state: &AppState,
    expected_generation: u64,
) -> Result<(), String> {
    let (capabilities, scope, recognition) = {
        let current = state
            .engine
            .lock()
            .map_err(|_| "Acquisition engine lock is unavailable".to_owned())?;
        if !current.is_obd_connected() || current.obd_connection_generation() != expected_generation
        {
            return Ok(());
        }
        let capabilities = current
            .connection_capabilities()
            .cloned()
            .ok_or_else(|| "OBD connection identity is unavailable".to_owned())?;
        let scope = crate::commands::vehicle_profiles::connection_scope(
            &capabilities.device.hardware_family,
        );
        (
            capabilities,
            scope,
            state
                .vehicle_profiles
                .current_observation(expected_generation),
        )
    };
    let active_profile = state
        .vehicle_profiles
        .active_profile(expected_generation)
        .and_then(|(id, _)| state.vehicle_profiles.get(&id).ok().flatten());
    let mut identities = Vec::new();
    for digest in recognition
        .as_ref()
        .and_then(|observation| observation.vin_digest.as_deref())
        .into_iter()
        .chain(
            active_profile
                .as_ref()
                .and_then(|profile| profile.vin_digest.as_deref()),
        )
    {
        if let Ok(Some(record)) = state.vehicle_profiles.get_vehicle_identity(scope, digest)
            && !identities.iter().any(|existing: &VehicleIdentityRecord| {
                existing.scope == record.scope && existing.vin_digest == record.vin_digest
            })
        {
            identities.push(record);
        }
    }
    let report = DiagnosticReport {
        report_id: None,
        connection_generation: Some(expected_generation),
        vehicle_context: None,
        adapter: capabilities.device.clone(),
        protocol: capabilities.protocol.clone(),
        started_at_unix_ms: now_unix_ms(),
        operation_kind: lognook_core::DiagnosticReportKind::Snapshot,
        request: Default::default(),
        reads: Vec::new(),
        route_restore_error: None,
        evidence_saved: false,
        evidence_save_error: None,
    };
    let snapshot = report_context(
        &report,
        scope,
        Some(expected_generation),
        VinOrigin::EcuRecognition,
        recognition.as_ref(),
        active_profile.as_ref(),
        &identities,
    );
    state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine lock is unavailable".to_owned())?
        .set_vehicle_recording_context(Some(snapshot), expected_generation)
        .map_err(|error| error.to_string())
}

fn empty_snapshot(
    scope: ProfileScope,
    generation: Option<u64>,
    observed_ecus: Vec<EcuProfileVariant>,
    source: VehicleIdentitySource,
    conflict: bool,
) -> VehicleIdentitySnapshot {
    VehicleIdentitySnapshot {
        scope,
        connection_generation: generation,
        profile_id: None,
        profile_revision: None,
        full_vin: None,
        vin_digest: None,
        masked_vin: None,
        origin: None,
        identity_source: source,
        vin_conflict: conflict,
        observed_ecus,
        decoded_snapshot: None,
        owner_details: vec![],
    }
}

fn matching_profile<'a>(
    profile: Option<&'a VehicleProfile>,
    scope: ProfileScope,
    digest: &str,
) -> Option<&'a VehicleProfile> {
    profile
        .filter(|profile| profile.scope == scope && profile.vin_digest.as_deref() == Some(digest))
}

fn identity_for<'a>(
    records: &'a [VehicleIdentityRecord],
    scope: ProfileScope,
    digest: &str,
) -> Option<&'a VehicleIdentityRecord> {
    records
        .iter()
        .find(|record| record.scope == scope && record.vin_digest == digest)
}

pub(crate) fn append_vehicle_report(
    state: &AppState,
    report: &mut DiagnosticReport,
    scope: ProfileScope,
    generation: u64,
    origin: VinOrigin,
    recognition: Option<&VehicleIdentityObservation>,
) {
    report.report_id.get_or_insert_with(new_report_id);
    report.connection_generation = Some(generation);
    let active_profile = state
        .vehicle_profiles
        .active_profile(generation)
        .and_then(|(id, _revision)| state.vehicle_profiles.get(&id).ok().flatten())
        .filter(|profile| {
            profile.revision
                == state
                    .vehicle_profiles
                    .active_profile(generation)
                    .map_or(0, |(_, revision)| revision)
        });
    let vins = positive_vin_observations(&report.reads);
    let mut identities = Vec::new();
    for vin in vins
        .iter()
        .map(|entry| &entry.vin)
        .chain(
            recognition
                .and_then(|obs| obs.vin_digest.as_ref())
                .into_iter(),
        )
        .chain(
            active_profile
                .as_ref()
                .and_then(|p| p.vin_digest.as_ref())
                .into_iter(),
        )
    {
        let digest = if vin.starts_with("vin:v1:sha256:") {
            vin.clone()
        } else {
            canonical_vin_identity(vin)
                .ok()
                .map(|x| x.0)
                .unwrap_or_default()
        };
        if !digest.is_empty()
            && !identities.iter().any(|record: &VehicleIdentityRecord| {
                record.vin_digest == digest && record.scope == scope
            })
        {
            if let Ok(Some(record)) = state.vehicle_profiles.get_vehicle_identity(scope, &digest) {
                identities.push(record);
            }
        }
    }
    report.vehicle_context = Some(report_context(
        report,
        scope,
        Some(generation),
        origin,
        recognition,
        active_profile.as_ref(),
        &identities,
    ));
    report.evidence_saved = true;
    report.evidence_save_error = None;
    let path = vehicle_reads_path(state);
    let appended = match append_report_to_path(&path, report) {
        Ok(appended) => appended,
        Err(error) => {
            report.evidence_saved = false;
            report.evidence_save_error = Some(error);
            return;
        }
    };
    let context = report.vehicle_context.clone().unwrap();
    let locator = locator_for(report, scope, context, appended);
    if let Err(error) = save_report_identity_evidence(
        &state.vehicle_profiles,
        scope,
        report.report_id.as_deref().unwrap_or_default(),
        generation,
        report.started_at_unix_ms,
        origin,
        &vins,
    ) {
        mark_history_error(
            &state.vehicle_profiles,
            &format!(
                "Saved report {} but VIN provenance could not be indexed: {error}",
                report.report_id.as_deref().unwrap_or("unknown")
            ),
        );
        return;
    }
    if let Err(error) = state.vehicle_profiles.add_diagnostic_locator(locator) {
        mark_history_error(
            &state.vehicle_profiles,
            &format!(
                "Saved report {} but history indexing failed: {error}",
                report.report_id.as_deref().unwrap_or("unknown")
            ),
        );
    }
}

pub(crate) fn vehicle_reads_path(state: &AppState) -> PathBuf {
    state.diagnostics_path.with_file_name("vehicle-reads.jsonl")
}

fn append_report_to_path(
    path: &Path,
    report: &mut DiagnosticReport,
) -> Result<AppendedLine, String> {
    if report.report_id.is_none() {
        report.report_id = Some(new_report_id());
    }
    report.evidence_saved = true;
    report.evidence_save_error = None;
    let mut line = serde_json::to_vec(&serde_json::json!({
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
        "report": report,
    }))
    .map_err(|error| error.to_string())?;
    line.push(b'\n');
    if line.len() > lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES {
        return Err("Vehicle report exceeds the 1 MiB JSONL line limit".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let _guard = APPEND_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Vehicle-read evidence lock is unavailable".to_owned())?;
    let mut prefix_newline = false;
    if let Ok(mut existing) = File::open(path) {
        let length = existing
            .metadata()
            .map_err(|error| error.to_string())?
            .len();
        if length > 0 {
            existing
                .seek(SeekFrom::End(-1))
                .map_err(|error| error.to_string())?;
            let mut last = [0u8; 1];
            existing
                .read_exact(&mut last)
                .map_err(|error| error.to_string())?;
            prefix_newline = last[0] != b'\n';
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Could not open vehicle-read evidence log: {error}"))?;
    if prefix_newline {
        file.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    let byte_offset = file.metadata().map_err(|error| error.to_string())?.len();
    file.write_all(&line).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())?;
    Ok(AppendedLine {
        byte_offset,
        byte_length: line.len() as u64,
        sha256: Sha256::digest(&line).into(),
    })
}

fn locator_for(
    report: &DiagnosticReport,
    scope: ProfileScope,
    context: VehicleIdentitySnapshot,
    appended: AppendedLine,
) -> DiagnosticReportLocator {
    let report_id = report.report_id.clone().unwrap_or_default();
    let mut ecu_ids = report
        .reads
        .iter()
        .filter_map(|read| read.ecu_id.as_deref())
        .map(|id| id.trim().to_ascii_uppercase())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    ecu_ids.sort();
    ecu_ids.dedup();
    let count = |kind: DiagnosticReadKind| -> Option<u32> {
        let values = report
            .reads
            .iter()
            .filter(|read| read.kind == kind && read.outcome == DiagnosticOutcome::Positive)
            .flat_map(|read| read.values.iter())
            .filter_map(|value| match value {
                DiagnosticValue::DtcList { codes } => Some(codes.len() as u32),
                _ => None,
            })
            .collect::<Vec<_>>();
        (!values.is_empty()).then(|| values.into_iter().fold(0u32, u32::saturating_add))
    };
    DiagnosticReportLocator {
        report_id,
        source_relative_path: VEHICLE_READS_RELATIVE_PATH.into(),
        byte_offset: appended.byte_offset,
        byte_length: appended.byte_length,
        sha256: hex(&appended.sha256),
        started_at_unix_ms: report.started_at_unix_ms,
        scope,
        vin_digest: context.vin_digest.clone(),
        masked_vin: context.masked_vin.clone(),
        identity_source: context.identity_source,
        vin_conflict: context.vin_conflict,
        ecu_ids,
        dtc_counts: DiagnosticDtcCounts {
            stored: count(DiagnosticReadKind::StoredDtcs),
            pending: count(DiagnosticReadKind::PendingDtcs),
            permanent: count(DiagnosticReadKind::PermanentDtcs),
        },
        mode06_read_count: report
            .reads
            .iter()
            .filter(|read| {
                matches!(
                    read.kind,
                    DiagnosticReadKind::Mode06Bitmap | DiagnosticReadKind::Mode06Monitor
                )
            })
            .count()
            .min(u32::MAX as usize) as u32,
        context_snapshot: context,
    }
}

pub(crate) fn read_report_detail(
    service: &crate::vehicle_profiles::VehicleProfileService,
    report_path: &Path,
    report_id: &str,
) -> Result<DiagnosticReportDetail, String> {
    let locator = service
        .get_diagnostic_locator(report_id)?
        .ok_or_else(|| format!("Diagnostic report '{report_id}' is not indexed"))?;
    let associations = service.get_manual_report_associations(report_id)?;
    let mut index = row_from_locator(&locator, HistoryEvidenceStatus::Missing);
    let mut context_snapshot = Some(locator.context_snapshot.clone());
    let (evidence_status, report) = match read_verified_report(report_path, &locator) {
        Ok(mut report) => {
            if let Some(saved_id) = report.report_id.as_deref() {
                if saved_id != locator.report_id {
                    (HistoryEvidenceStatus::Malformed, None)
                } else {
                    (HistoryEvidenceStatus::Available, Some(report))
                }
            } else {
                report.report_id = Some(locator.report_id.clone());
                (HistoryEvidenceStatus::Available, Some(report))
            }
        }
        Err(status) => (status, None),
    };
    index.evidence_status = evidence_status;
    Ok(DiagnosticReportDetail {
        index,
        evidence_status,
        report,
        context_snapshot: context_snapshot.take(),
        manual_associations: associations,
    })
}

fn read_verified_report(
    path: &Path,
    locator: &DiagnosticReportLocator,
) -> Result<DiagnosticReport, HistoryEvidenceStatus> {
    if locator.source_relative_path != VEHICLE_READS_RELATIVE_PATH
        || locator.byte_length as usize > lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES
    {
        return Err(HistoryEvidenceStatus::Malformed);
    }
    let mut file = File::open(path).map_err(|_| HistoryEvidenceStatus::Missing)?;
    let end = locator
        .byte_offset
        .checked_add(locator.byte_length)
        .ok_or(HistoryEvidenceStatus::Changed)?;
    if file
        .metadata()
        .map_err(|_| HistoryEvidenceStatus::Missing)?
        .len()
        < end
    {
        return Err(HistoryEvidenceStatus::Changed);
    }
    file.seek(SeekFrom::Start(locator.byte_offset))
        .map_err(|_| HistoryEvidenceStatus::Changed)?;
    let mut bytes = vec![0; locator.byte_length as usize];
    file.read_exact(&mut bytes)
        .map_err(|_| HistoryEvidenceStatus::Changed)?;
    if Sha256::digest(&bytes).as_slice()
        != decode_hex(&locator.sha256).as_deref().unwrap_or_default()
    {
        return Err(HistoryEvidenceStatus::Changed);
    }
    let body = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let body = body.strip_suffix(b"\r").unwrap_or(body);
    let envelope: VehicleReadsEnvelope =
        serde_json::from_slice(body).map_err(|_| HistoryEvidenceStatus::Malformed)?;
    Ok(envelope.report)
}

#[derive(serde::Deserialize)]
struct VehicleReadsEnvelope {
    report: DiagnosticReport,
}

fn row_from_locator(
    locator: &DiagnosticReportLocator,
    evidence_status: HistoryEvidenceStatus,
) -> DiagnosticHistoryRow {
    DiagnosticHistoryRow {
        report_id: locator.report_id.clone(),
        started_at_unix_ms: locator.started_at_unix_ms,
        scope: locator.scope,
        vin_digest: locator.vin_digest.clone(),
        masked_vin: locator.masked_vin.clone(),
        identity_source: locator.identity_source,
        vin_conflict: locator.vin_conflict,
        ecu_ids: locator.ecu_ids.clone(),
        dtc_counts: locator.dtc_counts.clone(),
        mode06_read_count: locator.mode06_read_count,
        evidence_status,
    }
}

fn save_report_identity_evidence(
    service: &crate::vehicle_profiles::VehicleProfileService,
    scope: ProfileScope,
    report_id: &str,
    generation: u64,
    recorded_at_unix_ms: u64,
    origin: VinOrigin,
    vins: &[PositiveVinObservation],
) -> Result<(), String> {
    let mut grouped: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for item in vins {
        let (digest, _) = canonical_vin_identity(&item.vin)?;
        grouped
            .entry(item.vin.clone())
            .or_default()
            .push(item.ecu_id.clone());
        let _ = digest;
    }
    for (vin, ecus) in grouped {
        let (digest, _) = canonical_vin_identity(&vin)?;
        let current = service.get_vehicle_identity(scope, &digest)?;
        let mut record = current.clone().unwrap_or(VehicleIdentityRecord {
            scope,
            vin_digest: digest,
            full_vin: None,
            origin_evidence: vec![],
            decoder_snapshot: None,
            owner_details: vec![],
            revision: 0,
        });
        let mut changed = current.is_none();
        if record.full_vin.as_deref() != Some(vin.as_str()) {
            record.full_vin = Some(vin.clone());
            changed = true;
        }
        if record.decoder_snapshot.is_none() {
            record.decoder_snapshot = decode_vin_offline(&vin).ok();
            changed |= record.decoder_snapshot.is_some();
        }
        for ecu_id in ecus {
            let evidence = VinEvidenceSource {
                origin,
                recorded_at_unix_ms,
                report_id: Some(report_id.into()),
                ecu_id: ecu_id.clone(),
                connection_generation: Some(generation),
                actor: None,
            };
            if !record.origin_evidence.contains(&evidence) {
                record.origin_evidence.push(evidence);
                changed = true;
            }
        }
        if changed {
            service.save_vehicle_identity(record, current.map(|record| record.revision))?;
        }
    }
    Ok(())
}

pub(crate) fn save_imported_vin_evidence(
    service: &crate::vehicle_profiles::VehicleProfileService,
    report: &DiagnosticReport,
    scope: ProfileScope,
    report_id: &str,
    origin: VinOrigin,
) -> Result<(), String> {
    save_report_identity_evidence(
        service,
        scope,
        report_id,
        report.connection_generation.unwrap_or_default(),
        report.started_at_unix_ms,
        origin,
        &positive_vin_observations(&report.reads),
    )
}

pub(crate) fn import_vehicle_history(
    state: &AppState,
    cancel: &AtomicBool,
) -> Result<HistoryIndexStatus, String> {
    import_vehicle_history_from(&state.vehicle_profiles, &vehicle_reads_path(state), cancel)
}

fn import_vehicle_history_from(
    service: &crate::vehicle_profiles::VehicleProfileService,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<HistoryIndexStatus, String> {
    let file_len = std::fs::metadata(&path)
        .map_err(|error| format!("Could not inspect saved vehicle-read history: {error}"))?
        .len();
    let mut checkpoint = service.get_history_import_checkpoint()?;
    if checkpoint.source_relative_path.as_deref() != Some(VEHICLE_READS_RELATIVE_PATH)
        || checkpoint.next_byte_offset > file_len
    {
        checkpoint = HistoryImportCheckpoint {
            status: HistoryIndexStatus {
                state: HistoryIndexState::Idle,
                indexed_reports: 0,
                scanned_bytes: 0,
                total_bytes: file_len,
                malformed_lines: 0,
                oversized_lines: 0,
                error: None,
            },
            source_relative_path: Some(VEHICLE_READS_RELATIVE_PATH.into()),
            next_byte_offset: 0,
            trailing_partial_line: None,
        };
    }
    if checkpoint.next_byte_offset == file_len && checkpoint.trailing_partial_line.is_none() {
        checkpoint.status.state = HistoryIndexState::Complete;
        checkpoint.status.total_bytes = file_len;
        service.save_history_import_checkpoint(checkpoint.clone())?;
        return Ok(checkpoint.status);
    }
    checkpoint.status.state = HistoryIndexState::Indexing;
    checkpoint.status.total_bytes = file_len;
    checkpoint.status.error = None;
    service.save_history_import_checkpoint(checkpoint.clone())?;

    let mut file = File::open(&path).map_err(|error| error.to_string())?;
    let mut offset = checkpoint.next_byte_offset;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut line = checkpoint.trailing_partial_line.take().unwrap_or_default();
    let mut line_start = offset.saturating_sub(line.len() as u64);
    let mut oversized = line.len() >= lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES;
    if oversized {
        line.clear();
    }
    let mut buffer = [0u8; 8192];
    loop {
        if cancel.load(Ordering::Acquire) {
            checkpoint.status.state = HistoryIndexState::Cancelled;
            checkpoint.status.scanned_bytes = offset;
            checkpoint.next_byte_offset = offset;
            checkpoint.trailing_partial_line = if oversized {
                Some(vec![0; lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES])
            } else {
                (!line.is_empty()).then_some(line)
            };
            service.save_history_import_checkpoint(checkpoint.clone())?;
            return Ok(checkpoint.status);
        }
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            checkpoint.status.state = HistoryIndexState::Complete;
            checkpoint.status.scanned_bytes = offset;
            checkpoint.status.total_bytes = file_len;
            checkpoint.next_byte_offset = offset;
            checkpoint.trailing_partial_line = if oversized {
                Some(vec![0; lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES])
            } else {
                (!line.is_empty()).then_some(line)
            };
            service.save_history_import_checkpoint(checkpoint.clone())?;
            return Ok(checkpoint.status);
        }
        for byte in &buffer[..count] {
            offset = offset.saturating_add(1);
            if !oversized {
                line.push(*byte);
                if line.len() > lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES {
                    line.clear();
                    oversized = true;
                }
            }
            if *byte != b'\n' {
                continue;
            }
            if oversized {
                checkpoint.status.oversized_lines =
                    checkpoint.status.oversized_lines.saturating_add(1);
            } else if !line.is_empty() {
                match import_history_line(service, &line, line_start)? {
                    HistoryLineOutcome::Indexed => {
                        checkpoint.status.indexed_reports =
                            checkpoint.status.indexed_reports.saturating_add(1);
                    }
                    HistoryLineOutcome::Malformed => {
                        checkpoint.status.malformed_lines =
                            checkpoint.status.malformed_lines.saturating_add(1);
                    }
                    HistoryLineOutcome::AlreadyIndexed => {}
                }
            }
            line.clear();
            oversized = false;
            line_start = offset;
            checkpoint.next_byte_offset = offset;
            checkpoint.trailing_partial_line = None;
            checkpoint.status.scanned_bytes = offset;
            checkpoint.status.total_bytes = file_len;
            service.save_history_import_checkpoint(checkpoint.clone())?;
            if cancel.load(Ordering::Acquire) {
                checkpoint.status.state = HistoryIndexState::Cancelled;
                service.save_history_import_checkpoint(checkpoint.clone())?;
                return Ok(checkpoint.status);
            }
        }
    }
}

enum HistoryLineOutcome {
    Indexed,
    AlreadyIndexed,
    Malformed,
}

fn import_history_line(
    service: &crate::vehicle_profiles::VehicleProfileService,
    bytes: &[u8],
    byte_offset: u64,
) -> Result<HistoryLineOutcome, String> {
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let body = body.strip_suffix(b"\r").unwrap_or(body);
    let envelope: VehicleReadsEnvelope = match serde_json::from_slice(body) {
        Ok(envelope) => envelope,
        Err(_) => return Ok(HistoryLineOutcome::Malformed),
    };
    let mut report = envelope.report;
    let report_id = report
        .report_id
        .clone()
        .unwrap_or_else(|| format!("legacy-report-{}", sha256_hex(bytes)));
    report.report_id = Some(report_id.clone());
    let scope = report.vehicle_context.as_ref().map_or_else(
        || crate::commands::vehicle_profiles::connection_scope(&report.adapter.hardware_family),
        |context| context.scope,
    );
    let vin_origin = report
        .vehicle_context
        .as_ref()
        .and_then(|context| context.origin)
        .unwrap_or(VinOrigin::LegacyReportImport);
    if service.get_diagnostic_locator(&report_id)?.is_some() {
        save_imported_vin_evidence(service, &report, scope, &report_id, vin_origin)?;
        return Ok(HistoryLineOutcome::AlreadyIndexed);
    }
    let context = if let Some(context) = report.vehicle_context.clone() {
        context
    } else {
        let vins = positive_vin_observations(&report.reads);
        let mut identities = Vec::new();
        for vin in &vins {
            let (digest, _) = canonical_vin_identity(&vin.vin)?;
            if let Some(identity) = service.get_vehicle_identity(scope, &digest)? {
                identities.push(identity);
            }
        }
        report_context(
            &report,
            scope,
            report.connection_generation,
            VinOrigin::LegacyReportImport,
            None,
            None,
            &identities,
        )
    };
    let line_digest: [u8; 32] = Sha256::digest(bytes).into();
    let locator = locator_for(
        &report,
        scope,
        context,
        AppendedLine {
            byte_offset,
            byte_length: bytes.len() as u64,
            sha256: line_digest,
        },
    );
    save_imported_vin_evidence(service, &report, scope, &report_id, vin_origin)?;
    service.add_diagnostic_locator(locator)?;
    Ok(HistoryLineOutcome::Indexed)
}

fn mark_history_error(service: &crate::vehicle_profiles::VehicleProfileService, message: &str) {
    let Ok(mut checkpoint) = service.get_history_import_checkpoint() else {
        return;
    };
    checkpoint.status.state = HistoryIndexState::Failed;
    checkpoint.status.error = Some(message.to_owned());
    let _ = service.save_history_import_checkpoint(checkpoint);
}

fn new_report_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed);
    format!("vehicle-read-{}-{nanos:x}-{sequence:x}", std::process::id())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(CHARS[(byte >> 4) as usize] as char);
        output.push(CHARS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 {
        return None;
    }
    (0..64)
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
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
mod tests {
    use super::*;
    use lognook_core::{DeviceDescriptor, DiagnosticRequest};
    use std::{sync::atomic::AtomicBool, time::UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lognook-{label}-{}-{unique}", std::process::id()))
    }

    fn report() -> DiagnosticReport {
        DiagnosticReport {
            report_id: Some("history-import-report".into()),
            connection_generation: Some(3),
            vehicle_context: None,
            adapter: DeviceDescriptor {
                stable_id: "obd:test".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: Some("COM9".into()),
            },
            protocol: "ISO 15765-4 CAN (11 bit, 500 kbaud)".into(),
            started_at_unix_ms: 1234,
            operation_kind: lognook_core::DiagnosticReportKind::Snapshot,
            request: DiagnosticRequest::default(),
            reads: Vec::new(),
            route_restore_error: None,
            evidence_saved: true,
            evidence_save_error: None,
        }
    }

    #[test]
    fn history_import_resumes_a_partial_trailing_line_and_is_idempotent() {
        let directory = temp_path("history-import");
        std::fs::create_dir_all(&directory).unwrap();
        let db = directory.join("profiles.sqlite");
        let path = directory.join("vehicle-reads.jsonl");
        let service = crate::vehicle_profiles::VehicleProfileService::open(&db);
        let mut line = serde_json::to_vec(&serde_json::json!({ "report": report() })).unwrap();
        line.push(b'\n');
        let split = line.len() / 2;
        std::fs::write(&path, &line[..split]).unwrap();

        let first = import_vehicle_history_from(&service, &path, &AtomicBool::new(false)).unwrap();
        assert_eq!(first.state, HistoryIndexState::Complete);
        assert_eq!(first.indexed_reports, 0);
        assert!(
            service
                .get_history_import_checkpoint()
                .unwrap()
                .trailing_partial_line
                .is_some()
        );

        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&line[split..])
            .unwrap();
        let second = import_vehicle_history_from(&service, &path, &AtomicBool::new(false)).unwrap();
        assert_eq!(second.indexed_reports, 1);
        assert_eq!(second.state, HistoryIndexState::Complete);
        assert!(
            service
                .get_history_import_checkpoint()
                .unwrap()
                .trailing_partial_line
                .is_none()
        );

        let detail = read_report_detail(&service, &path, "history-import-report").unwrap();
        assert_eq!(detail.evidence_status, HistoryEvidenceStatus::Available);
        assert_eq!(
            detail.report.unwrap().report_id.as_deref(),
            Some("history-import-report")
        );

        let mut changed_bytes = std::fs::read(&path).unwrap();
        changed_bytes[0] = b'!';
        std::fs::write(&path, changed_bytes).unwrap();
        let changed = read_report_detail(&service, &path, "history-import-report").unwrap();
        assert_eq!(changed.evidence_status, HistoryEvidenceStatus::Changed);
        assert!(changed.report.is_none());
        assert!(changed.context_snapshot.is_some());

        let third = import_vehicle_history_from(&service, &path, &AtomicBool::new(false)).unwrap();
        assert_eq!(third.indexed_reports, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn history_import_counts_malformed_and_oversized_lines_without_storing_them() {
        let directory = temp_path("history-import-limits");
        std::fs::create_dir_all(&directory).unwrap();
        let db = directory.join("profiles.sqlite");
        let path = directory.join("vehicle-reads.jsonl");
        let service = crate::vehicle_profiles::VehicleProfileService::open(&db);
        let mut bytes = b"{not-json}\n".to_vec();
        bytes.extend(vec![
            b'x';
            lognook_core::MAX_DIAGNOSTIC_REPORT_LINE_BYTES + 1
        ]);
        bytes.push(b'\n');
        let mut line = serde_json::to_vec(&serde_json::json!({ "report": report() })).unwrap();
        line.push(b'\n');
        bytes.extend(line);
        std::fs::write(&path, bytes).unwrap();

        let status = import_vehicle_history_from(&service, &path, &AtomicBool::new(false)).unwrap();
        assert_eq!(status.state, HistoryIndexState::Complete);
        assert_eq!(status.indexed_reports, 1);
        assert_eq!(status.malformed_lines, 1);
        assert_eq!(status.oversized_lines, 1);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn history_import_cancellation_saves_a_resumable_cancelled_checkpoint() {
        let directory = temp_path("history-import-cancel");
        std::fs::create_dir_all(&directory).unwrap();
        let db = directory.join("profiles.sqlite");
        let path = directory.join("vehicle-reads.jsonl");
        let service = crate::vehicle_profiles::VehicleProfileService::open(&db);
        let line = serde_json::to_vec(&serde_json::json!({ "report": report() })).unwrap();
        std::fs::write(&path, [line, vec![b'\n']].concat()).unwrap();
        let cancel = AtomicBool::new(true);

        let status = import_vehicle_history_from(&service, &path, &cancel).unwrap();
        assert_eq!(status.state, HistoryIndexState::Cancelled);
        assert_eq!(status.indexed_reports, 0);
        let checkpoint = service.get_history_import_checkpoint().unwrap();
        assert_eq!(checkpoint.status.state, HistoryIndexState::Cancelled);
        assert_eq!(checkpoint.next_byte_offset, 0);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn report_append_assigns_id_and_separates_a_partial_jsonl_tail() {
        let directory = temp_path("vehicle-report-append");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("vehicle-reads.jsonl");
        let prefix = b"{\"unfinished\":";
        std::fs::write(&path, prefix).unwrap();
        let mut value = report();

        let appended = append_report_to_path(&path, &mut value).unwrap();
        assert!(value.report_id.as_deref().is_some_and(|id| !id.is_empty()));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes[prefix.len()], b'\n');
        assert_eq!(appended.byte_offset, prefix.len() as u64 + 1);
        let saved_line = &bytes[appended.byte_offset as usize..];
        assert_eq!(saved_line.len(), appended.byte_length as usize);
        assert_eq!(Sha256::digest(saved_line).as_slice(), appended.sha256);
        let envelope: serde_json::Value = serde_json::from_slice(saved_line).unwrap();
        assert_eq!(
            envelope["report"]["report_id"],
            value.report_id.as_deref().unwrap()
        );
        assert_eq!(envelope["report"]["evidence_saved"], true);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn legacy_vin_recovery_requires_a_positive_mode09_pid02_payload_or_decoded_value() {
        let query = DiagnosticRequest::identity_queries().pop_front().unwrap();
        let vin = b"1FAHP2E89BG123456";
        let mut payload = vec![0x49, 0x02, 0x01];
        payload.extend_from_slice(vin);
        let mut positive = query.report(
            Some("7E8".into()),
            payload,
            "7E8 49 02 01 ...".into(),
            "0902".into(),
            1,
            1,
            false,
        );
        positive.values.clear();
        assert_eq!(positive_vin_observations(&[positive.clone()]).len(), 1);

        positive.outcome = DiagnosticOutcome::MalformedResponse;
        positive.values.push(DiagnosticValue::Identity {
            pid: 0x02,
            label: "VIN".into(),
            value: String::from_utf8(vin.to_vec()).unwrap(),
        });
        assert!(positive_vin_observations(&[positive]).is_empty());
    }

    #[test]
    fn retry_repairs_vin_provenance_when_the_report_locator_already_exists() {
        let directory = temp_path("vehicle-history-provenance-repair");
        std::fs::create_dir_all(&directory).unwrap();
        let db = directory.join("profiles.sqlite");
        let service = crate::vehicle_profiles::VehicleProfileService::open(&db);
        let vin = "1FAHP2E89BG123456";
        let query = DiagnosticRequest::identity_queries().pop_front().unwrap();
        let mut payload = vec![0x49, 0x02, 0x01];
        payload.extend_from_slice(vin.as_bytes());
        let read = query.report(
            Some("7E8".into()),
            payload,
            "VIN response".into(),
            "0902".into(),
            1,
            2,
            false,
        );
        let mut value = report();
        value.reads = vec![read];
        let bytes = serde_json::to_vec(&serde_json::json!({ "report": &value })).unwrap();
        let (digest, _) = canonical_vin_identity(vin).unwrap();
        let context = report_context(
            &value,
            ProfileScope::Live,
            Some(3),
            VinOrigin::LegacyReportImport,
            None,
            None,
            &[],
        );
        let locator = locator_for(
            &value,
            ProfileScope::Live,
            context,
            AppendedLine {
                byte_offset: 0,
                byte_length: bytes.len() as u64,
                sha256: Sha256::digest(&bytes).into(),
            },
        );
        service.add_diagnostic_locator(locator).unwrap();

        assert!(matches!(
            import_history_line(&service, &bytes, 0).unwrap(),
            HistoryLineOutcome::AlreadyIndexed
        ));
        let identity = service
            .get_vehicle_identity(ProfileScope::Live, &digest)
            .unwrap()
            .unwrap();
        assert_eq!(identity.full_vin.as_deref(), Some(vin));
        assert!(identity.origin_evidence.iter().any(|item| {
            item.report_id.as_deref() == Some("history-import-report")
                && item.origin == VinOrigin::LegacyReportImport
        }));
        let _ = std::fs::remove_dir_all(directory);
    }
}

fn _checkpoint_shape(status: HistoryIndexStatus) -> HistoryImportCheckpoint {
    HistoryImportCheckpoint {
        status,
        source_relative_path: None,
        next_byte_offset: 0,
        trailing_partial_line: None,
    }
}
