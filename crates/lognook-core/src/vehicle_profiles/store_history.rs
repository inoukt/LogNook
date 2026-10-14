use super::*;
use crate::vehicle_profiles::history::HistoryCursor;
use crate::vehicle_profiles::{
    DiagnosticDtcCounts, DiagnosticHistoryFilter, DiagnosticHistoryPage, DiagnosticHistoryRow,
    DiagnosticReportLocator, HistoryEvidenceStatus, HistoryImportCheckpoint, HistoryIndexState,
    HistoryIndexStatus, MAX_DIAGNOSTIC_REPORT_LINE_BYTES, ManualDetailActor,
    ManualReportAssociation, VehicleIdentitySource, canonical_vin_identity,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use std::path::{Component, Path};

const MAX_HISTORY_PAGE_SIZE: u16 = 100;
const DEFAULT_HISTORY_PAGE_SIZE: u16 = 50;

impl VehicleProfileStore {
    pub fn add_diagnostic_locator(
        &mut self,
        locator: DiagnosticReportLocator,
    ) -> Result<(), ProfileStoreError> {
        let locator = canonicalize_locator(locator)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = load_locator(&transaction, &locator.report_id)? {
            if existing == locator {
                transaction.commit()?;
                return Ok(());
            }
            return Err(ProfileStoreError::Invalid(format!(
                "Diagnostic report ID '{}' already refers to different evidence",
                locator.report_id
            )));
        }
        let range_end = locator
            .byte_offset
            .checked_add(locator.byte_length)
            .ok_or_else(|| {
                ProfileStoreError::Invalid("Diagnostic evidence byte range is invalid".into())
            })?;
        let overlap = transaction
            .query_row(
                "SELECT report_id FROM diagnostic_history WHERE source_relative_path=?1 AND byte_offset<?2 AND byte_offset+byte_length>?3 LIMIT 1",
                params![
                    locator.source_relative_path,
                    to_sql_u64(range_end)?,
                    to_sql_u64(locator.byte_offset)?,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(overlap) = overlap {
            return Err(ProfileStoreError::Invalid(format!(
                "Diagnostic evidence byte range overlaps indexed report '{overlap}'"
            )));
        }

        transaction.execute(
            "INSERT INTO diagnostic_history(report_id,source_relative_path,byte_offset,byte_length,sha256,started_at_unix_ms,scope,vin_digest,masked_vin,identity_source,vin_conflict,ecu_ids_json,dtc_counts_json,mode06_read_count,context_snapshot_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                locator.report_id,
                locator.source_relative_path,
                to_sql_u64(locator.byte_offset)?,
                to_sql_u64(locator.byte_length)?,
                locator.sha256,
                to_sql_u64(locator.started_at_unix_ms)?,
                scope_text(locator.scope),
                locator.vin_digest,
                locator.masked_vin,
                identity_source_text(locator.identity_source),
                locator.vin_conflict,
                encode(&locator.ecu_ids)?,
                encode(&locator.dtc_counts)?,
                i64::from(locator.mode06_read_count),
                encode(&locator.context_snapshot)?,
            ],
        )?;
        let mut indexed = std::collections::HashSet::new();
        for ecu_id in &locator.ecu_ids {
            let normalized = ecu_id.trim().to_ascii_uppercase();
            if indexed.insert(normalized.clone()) {
                transaction.execute(
                    "INSERT INTO diagnostic_history_ecus(report_id,ecu_id,started_at_unix_ms,scope,vin_digest) VALUES(?1,?2,?3,?4,?5)",
                    params![
                        locator.report_id,
                        normalized,
                        to_sql_u64(locator.started_at_unix_ms)?,
                        scope_text(locator.scope),
                        locator.vin_digest,
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_diagnostic_locator(
        &self,
        report_id: &str,
    ) -> Result<Option<DiagnosticReportLocator>, ProfileStoreError> {
        if invalid_text(report_id, 128) || report_id.trim().is_empty() {
            return Err(ProfileStoreError::Invalid(
                "Diagnostic report ID is invalid".into(),
            ));
        }
        load_locator(&self.connection, report_id)
    }

    pub fn list_diagnostic_history(
        &self,
        filter: DiagnosticHistoryFilter,
    ) -> Result<DiagnosticHistoryPage, ProfileStoreError> {
        if filter
            .from_unix_ms
            .zip(filter.to_unix_ms)
            .is_some_and(|(from, to)| from > to)
        {
            return Err(ProfileStoreError::Invalid(
                "History start date must not be later than its end date".into(),
            ));
        }
        if let Some(digest) = filter.vin_digest.as_deref() {
            let scope = filter.scope.unwrap_or(ProfileScope::Live);
            identity::validate_scope_digest(scope, digest)?;
        }
        let cursor = filter
            .cursor
            .as_deref()
            .map(decode_history_cursor)
            .transpose()?;
        let page_size = filter
            .page_size
            .unwrap_or(DEFAULT_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_HISTORY_PAGE_SIZE);

        let mut sql = String::from(
            "SELECT h.report_id,h.started_at_unix_ms,h.scope,h.vin_digest,h.masked_vin,h.identity_source,h.vin_conflict,h.ecu_ids_json,h.dtc_counts_json,h.mode06_read_count FROM diagnostic_history h WHERE 1=1",
        );
        let mut values = Vec::<Value>::new();
        if let Some(scope) = filter.scope {
            sql.push_str(" AND h.scope=?");
            values.push(Value::Text(scope_text(scope).to_owned()));
        }
        if let Some(vin_digest) = filter.vin_digest {
            sql.push_str(" AND (h.vin_digest=? OR EXISTS (SELECT 1 FROM manual_report_associations a WHERE a.report_id=h.report_id AND a.scope=h.scope AND a.vin_digest=?))");
            values.push(Value::Text(vin_digest.clone()));
            values.push(Value::Text(vin_digest));
        }
        if let Some(ecu_id) = filter.ecu_id.as_deref() {
            let ecu_id = ecu_id.trim().to_ascii_uppercase();
            if ecu_id.is_empty() || invalid_text(&ecu_id, 64) {
                return Err(ProfileStoreError::Invalid(
                    "History ECU filter is invalid".into(),
                ));
            }
            sql.push_str(" AND EXISTS (SELECT 1 FROM diagnostic_history_ecus e WHERE e.report_id=h.report_id AND e.ecu_id=?)");
            values.push(Value::Text(ecu_id));
        }
        if let Some(from) = filter.from_unix_ms {
            sql.push_str(" AND h.started_at_unix_ms>=?");
            values.push(Value::Integer(to_sql_u64(from)?));
        }
        if let Some(to) = filter.to_unix_ms {
            sql.push_str(" AND h.started_at_unix_ms<=?");
            values.push(Value::Integer(to_sql_u64(to)?));
        }
        if let Some(cursor) = cursor {
            sql.push_str(
                " AND (h.started_at_unix_ms<? OR (h.started_at_unix_ms=? AND h.report_id<?))",
            );
            values.push(Value::Integer(to_sql_u64(cursor.started_at_unix_ms)?));
            values.push(Value::Integer(to_sql_u64(cursor.started_at_unix_ms)?));
            values.push(Value::Text(cursor.report_id));
        }
        sql.push_str(" ORDER BY h.started_at_unix_ms DESC,h.report_id DESC LIMIT ?");
        values.push(Value::Integer(i64::from(page_size) + 1));

        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(
                |(
                    report_id,
                    started_at_unix_ms,
                    scope,
                    vin_digest,
                    masked_vin,
                    identity_source,
                    vin_conflict,
                    ecu_ids,
                    dtc_counts,
                    mode06_read_count,
                )| {
                    Ok(DiagnosticHistoryRow {
                        report_id,
                        started_at_unix_ms: from_sql_u64(started_at_unix_ms)?,
                        scope: parse_scope(&scope)?,
                        vin_digest,
                        masked_vin,
                        identity_source: parse_identity_source(&identity_source)?,
                        vin_conflict,
                        ecu_ids: decode(&ecu_ids)?,
                        dtc_counts: decode(&dtc_counts)?,
                        mode06_read_count: u32::try_from(mode06_read_count).map_err(|_| {
                            ProfileStoreError::Encoding(
                                "Stored Mode 06 read count is invalid".into(),
                            )
                        })?,
                        evidence_status: HistoryEvidenceStatus::Available,
                    })
                },
            )
            .collect::<Result<Vec<_>, ProfileStoreError>>()?;
        let has_more = rows.len() > usize::from(page_size);
        if has_more {
            rows.pop();
        }
        let next_cursor = if has_more {
            rows.last()
                .map(|row| encode_history_cursor(row.started_at_unix_ms, &row.report_id))
                .transpose()?
        } else {
            None
        };
        Ok(DiagnosticHistoryPage { rows, next_cursor })
    }

    pub fn append_manual_report_association(
        &mut self,
        report_id: String,
        scope: ProfileScope,
        vin_digest: String,
        actor: ManualDetailActor,
        note: Option<String>,
    ) -> Result<ManualReportAssociation, ProfileStoreError> {
        if report_id.trim().is_empty() || invalid_text(&report_id, 128) {
            return Err(ProfileStoreError::Invalid(
                "Diagnostic report ID is invalid".into(),
            ));
        }
        identity::validate_scope_digest(scope, &vin_digest)?;
        let note = identity::validate_note(note)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let report_scope = transaction
            .query_row(
                "SELECT scope FROM diagnostic_history WHERE report_id=?1",
                [&report_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                ProfileStoreError::Invalid(format!(
                    "Diagnostic report '{report_id}' is not indexed"
                ))
            })?;
        if parse_scope(&report_scope)? != scope {
            return Err(ProfileStoreError::Invalid(
                "Manual report associations cannot cross live, simulator, or CAN-only scopes"
                    .into(),
            ));
        }
        identity::ensure_identity_row(&transaction, scope, &vin_digest)?;
        let associated_at_unix_ms = now_unix_ms();
        transaction.execute(
            "INSERT INTO manual_report_associations(report_id,scope,vin_digest,actor,associated_at_unix_ms,note) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                report_id,
                scope_text(scope),
                vin_digest,
                actor_text(actor),
                to_sql_u64(associated_at_unix_ms)?,
                note,
            ],
        )?;
        transaction.commit()?;
        Ok(ManualReportAssociation {
            report_id,
            scope,
            vin_digest,
            actor,
            associated_at_unix_ms,
            note,
        })
    }

    pub fn get_manual_report_associations(
        &self,
        report_id: &str,
    ) -> Result<Vec<ManualReportAssociation>, ProfileStoreError> {
        if report_id.trim().is_empty() || invalid_text(report_id, 128) {
            return Err(ProfileStoreError::Invalid(
                "Diagnostic report ID is invalid".into(),
            ));
        }
        let report_exists = self
            .connection
            .query_row(
                "SELECT 1 FROM diagnostic_history WHERE report_id=?1",
                [report_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !report_exists {
            return Err(ProfileStoreError::Invalid(format!(
                "Diagnostic report '{report_id}' is not indexed"
            )));
        }

        let mut statement = self.connection.prepare(
            "SELECT scope,vin_digest,actor,associated_at_unix_ms,note FROM manual_report_associations WHERE report_id=?1 ORDER BY associated_at_unix_ms DESC,association_id DESC",
        )?;
        statement
            .query_map([report_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .map(|row| {
                let (scope, vin_digest, actor, associated_at_unix_ms, note) = row?;
                Ok(ManualReportAssociation {
                    report_id: report_id.to_owned(),
                    scope: parse_scope(&scope)?,
                    vin_digest,
                    actor: parse_actor(&actor)?,
                    associated_at_unix_ms: from_sql_u64(associated_at_unix_ms)?,
                    note,
                })
            })
            .collect()
    }

    pub fn get_history_import_checkpoint(
        &self,
    ) -> Result<HistoryImportCheckpoint, ProfileStoreError> {
        let checkpoint = self.connection.query_row(
            "SELECT source_relative_path,state,indexed_reports,scanned_bytes,total_bytes,malformed_lines,oversized_lines,error,next_byte_offset,trailing_partial_line FROM history_import_state WHERE singleton=1",
            [],
            |row| Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<Vec<u8>>>(9)?,
            )),
        )?;
        let checkpoint = HistoryImportCheckpoint {
            source_relative_path: checkpoint.0,
            status: HistoryIndexStatus {
                state: parse_history_state(&checkpoint.1)?,
                indexed_reports: from_sql_u64(checkpoint.2)?,
                scanned_bytes: from_sql_u64(checkpoint.3)?,
                total_bytes: from_sql_u64(checkpoint.4)?,
                malformed_lines: from_sql_u64(checkpoint.5)?,
                oversized_lines: from_sql_u64(checkpoint.6)?,
                error: checkpoint.7,
            },
            next_byte_offset: from_sql_u64(checkpoint.8)?,
            trailing_partial_line: checkpoint.9,
        };
        if let Some(path) = checkpoint.source_relative_path.as_deref() {
            let canonical_path = normalize_managed_path(path).map_err(|_| {
                ProfileStoreError::Encoding("Stored history import source path is invalid".into())
            })?;
            if canonical_path != path {
                return Err(ProfileStoreError::Encoding(
                    "Stored history import source path is not canonical".into(),
                ));
            }
        }
        if checkpoint
            .trailing_partial_line
            .as_ref()
            .is_some_and(|line| line.len() > MAX_DIAGNOSTIC_REPORT_LINE_BYTES)
        {
            return Err(ProfileStoreError::Encoding(
                "Stored history import checkpoint is invalid".into(),
            ));
        }
        Ok(checkpoint)
    }

    pub fn save_history_import_checkpoint(
        &mut self,
        mut checkpoint: HistoryImportCheckpoint,
    ) -> Result<(), ProfileStoreError> {
        if let Some(path) = checkpoint.source_relative_path.take() {
            checkpoint.source_relative_path = Some(normalize_managed_path(&path)?);
        }
        if checkpoint
            .trailing_partial_line
            .as_ref()
            .is_some_and(|line| line.len() > MAX_DIAGNOSTIC_REPORT_LINE_BYTES)
        {
            return Err(ProfileStoreError::Invalid(
                "History import trailing line exceeds the 1 MiB report limit".into(),
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE history_import_state SET source_relative_path=?1,state=?2,indexed_reports=?3,scanned_bytes=?4,total_bytes=?5,malformed_lines=?6,oversized_lines=?7,error=?8,next_byte_offset=?9,trailing_partial_line=?10,updated_at_unix_ms=?11 WHERE singleton=1",
            params![
                checkpoint.source_relative_path,
                history_state_text(checkpoint.status.state),
                to_sql_u64(checkpoint.status.indexed_reports)?,
                to_sql_u64(checkpoint.status.scanned_bytes)?,
                to_sql_u64(checkpoint.status.total_bytes)?,
                to_sql_u64(checkpoint.status.malformed_lines)?,
                to_sql_u64(checkpoint.status.oversized_lines)?,
                checkpoint.status.error,
                to_sql_u64(checkpoint.next_byte_offset)?,
                checkpoint.trailing_partial_line,
                to_sql_u64(now_unix_ms())?,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn load_locator(
    connection: &Connection,
    report_id: &str,
) -> Result<Option<DiagnosticReportLocator>, ProfileStoreError> {
    let locator = connection
        .query_row(
            "SELECT source_relative_path,byte_offset,byte_length,sha256,started_at_unix_ms,scope,vin_digest,masked_vin,identity_source,vin_conflict,ecu_ids_json,dtc_counts_json,mode06_read_count,context_snapshot_json FROM diagnostic_history WHERE report_id=?1",
            [report_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, bool>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, String>(13)?,
            )),
        )
        .optional()?
        .map(|row| {
            Ok::<DiagnosticReportLocator, ProfileStoreError>(DiagnosticReportLocator {
                report_id: report_id.to_owned(),
                source_relative_path: row.0,
                byte_offset: from_sql_u64(row.1)?,
                byte_length: from_sql_u64(row.2)?,
                sha256: row.3,
                started_at_unix_ms: from_sql_u64(row.4)?,
                scope: parse_scope(&row.5)?,
                vin_digest: row.6,
                masked_vin: row.7,
                identity_source: parse_identity_source(&row.8)?,
                vin_conflict: row.9,
                ecu_ids: decode(&row.10)?,
                dtc_counts: decode::<DiagnosticDtcCounts>(&row.11)?,
                mode06_read_count: u32::try_from(row.12).map_err(|_| {
                    ProfileStoreError::Encoding("Stored Mode 06 read count is invalid".into())
                })?,
                context_snapshot: decode(&row.13)?,
            })
        })
        .transpose()
        ?;
    if let Some(locator) = &locator {
        validate_locator(locator)?;
    }
    Ok(locator)
}

fn canonicalize_locator(
    mut locator: DiagnosticReportLocator,
) -> Result<DiagnosticReportLocator, ProfileStoreError> {
    locator.source_relative_path = normalize_managed_path(&locator.source_relative_path)?;
    validate_locator(&locator)?;
    Ok(locator)
}

fn validate_locator(locator: &DiagnosticReportLocator) -> Result<(), ProfileStoreError> {
    if locator.report_id.trim().is_empty() || invalid_text(&locator.report_id, 128) {
        return Err(ProfileStoreError::Invalid(
            "Diagnostic report ID is invalid".into(),
        ));
    }
    if normalize_managed_path(&locator.source_relative_path)? != locator.source_relative_path {
        return Err(ProfileStoreError::Invalid(
            "Diagnostic evidence path must use its canonical managed relative form".into(),
        ));
    }
    let Some(range_end) = locator.byte_offset.checked_add(locator.byte_length) else {
        return Err(ProfileStoreError::Invalid(
            "Diagnostic evidence byte range is invalid".into(),
        ));
    };
    if locator.byte_length == 0
        || locator.byte_length > MAX_DIAGNOSTIC_REPORT_LINE_BYTES as u64
        || range_end > i64::MAX as u64
    {
        return Err(ProfileStoreError::Invalid(
            "Diagnostic evidence byte range must fit one 1 MiB JSONL line and SQLite's integer range".into(),
        ));
    }
    let _ = to_sql_u64(locator.byte_offset)?;
    let _ = to_sql_u64(locator.byte_length)?;
    let _ = to_sql_u64(locator.started_at_unix_ms)?;
    if locator.sha256.len() != 64
        || !locator
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProfileStoreError::Invalid(
            "Diagnostic evidence SHA-256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    if let Some(digest) = locator.vin_digest.as_deref() {
        identity::validate_scope_digest(locator.scope, digest)?;
        crate::vehicle_profiles::store::validate_vin_fields(
            Some(digest),
            locator.masked_vin.as_deref(),
        )?;
    } else if locator.masked_vin.is_some() {
        return Err(ProfileStoreError::Invalid(
            "A masked VIN cannot be stored without its digest".into(),
        ));
    }
    let snapshot = &locator.context_snapshot;
    if snapshot.scope != locator.scope
        || snapshot.vin_digest != locator.vin_digest
        || snapshot.masked_vin != locator.masked_vin
        || snapshot.identity_source != locator.identity_source
        || snapshot.vin_conflict != locator.vin_conflict
    {
        return Err(ProfileStoreError::Invalid(
            "Diagnostic locator fields must match the immutable context snapshot".into(),
        ));
    }
    if let Some(full_vin) = snapshot.full_vin.as_deref() {
        let (digest, mask) =
            canonical_vin_identity(full_vin).map_err(ProfileStoreError::Invalid)?;
        if snapshot.vin_digest.as_deref() != Some(digest.as_str())
            || snapshot.masked_vin.as_deref() != Some(mask.as_str())
        {
            return Err(ProfileStoreError::Invalid(
                "Diagnostic context full VIN, digest, and mask do not match".into(),
            ));
        }
    }
    if locator.identity_source == VehicleIdentitySource::Conflict && !locator.vin_conflict {
        return Err(ProfileStoreError::Invalid(
            "Conflict identity source must retain the VIN conflict flag".into(),
        ));
    }
    for ecu_id in &locator.ecu_ids {
        if ecu_id.trim().is_empty() || invalid_text(ecu_id, 64) {
            return Err(ProfileStoreError::Invalid(
                "Diagnostic ECU ID is invalid".into(),
            ));
        }
    }
    Ok(())
}

fn normalize_managed_path(path: &str) -> Result<String, ProfileStoreError> {
    if path.trim().is_empty()
        || path.len() > 512
        || !path.is_ascii()
        || path.chars().any(char::is_control)
    {
        return Err(ProfileStoreError::Invalid(
            "Managed history path must be a non-empty relative ASCII path".into(),
        ));
    }
    let normalized = path.replace('\\', "/");
    if Path::new(&normalized).is_absolute()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || Path::new(&normalized)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ProfileStoreError::Invalid(
            "Managed history path cannot be absolute, prefixed, empty, or traverse directories"
                .into(),
        ));
    }
    #[cfg(windows)]
    let normalized = normalized.to_ascii_lowercase();
    Ok(normalized)
}

fn encode_history_cursor(
    started_at_unix_ms: u64,
    report_id: &str,
) -> Result<String, ProfileStoreError> {
    serde_json::to_string(&HistoryCursor {
        started_at_unix_ms,
        report_id: report_id.to_owned(),
    })
    .map_err(|error| ProfileStoreError::Encoding(error.to_string()))
}

fn decode_history_cursor(value: &str) -> Result<HistoryCursor, ProfileStoreError> {
    if value.len() > 256 {
        return Err(ProfileStoreError::Invalid(
            "History cursor is invalid".into(),
        ));
    }
    let cursor: HistoryCursor = serde_json::from_str(value)
        .map_err(|_| ProfileStoreError::Invalid("History cursor is invalid".into()))?;
    if cursor.report_id.trim().is_empty() || invalid_text(&cursor.report_id, 128) {
        return Err(ProfileStoreError::Invalid(
            "History cursor is invalid".into(),
        ));
    }
    Ok(cursor)
}

fn identity_source_text(source: VehicleIdentitySource) -> &'static str {
    match source {
        VehicleIdentitySource::ReportEcu => "report_ecu",
        VehicleIdentitySource::SameGenerationRecognition => "same_generation_recognition",
        VehicleIdentitySource::ManualProfile => "manual_profile",
        VehicleIdentitySource::Conflict => "conflict",
        VehicleIdentitySource::Unassigned => "unassigned",
    }
}

fn parse_identity_source(value: &str) -> Result<VehicleIdentitySource, ProfileStoreError> {
    match value {
        "report_ecu" => Ok(VehicleIdentitySource::ReportEcu),
        "same_generation_recognition" => Ok(VehicleIdentitySource::SameGenerationRecognition),
        "manual_profile" => Ok(VehicleIdentitySource::ManualProfile),
        "conflict" => Ok(VehicleIdentitySource::Conflict),
        "unassigned" => Ok(VehicleIdentitySource::Unassigned),
        _ => Err(ProfileStoreError::Invalid(
            "Stored report identity source is unknown".into(),
        )),
    }
}

fn actor_text(actor: ManualDetailActor) -> &'static str {
    match actor {
        ManualDetailActor::OwnerUi => "owner_ui",
        ManualDetailActor::AgentAccess => "agent_access",
    }
}

fn parse_actor(value: &str) -> Result<ManualDetailActor, ProfileStoreError> {
    match value {
        "owner_ui" => Ok(ManualDetailActor::OwnerUi),
        "agent_access" => Ok(ManualDetailActor::AgentAccess),
        _ => Err(ProfileStoreError::Invalid(
            "Stored manual report association actor is unknown".into(),
        )),
    }
}

fn parse_history_state(value: &str) -> Result<HistoryIndexState, ProfileStoreError> {
    match value {
        "idle" => Ok(HistoryIndexState::Idle),
        "indexing" => Ok(HistoryIndexState::Indexing),
        "complete" => Ok(HistoryIndexState::Complete),
        "cancelled" => Ok(HistoryIndexState::Cancelled),
        "failed" => Ok(HistoryIndexState::Failed),
        _ => Err(ProfileStoreError::Invalid(
            "Stored history import state is unknown".into(),
        )),
    }
}

fn history_state_text(value: HistoryIndexState) -> &'static str {
    match value {
        HistoryIndexState::Idle => "idle",
        HistoryIndexState::Indexing => "indexing",
        HistoryIndexState::Complete => "complete",
        HistoryIndexState::Cancelled => "cancelled",
        HistoryIndexState::Failed => "failed",
    }
}

fn invalid_text(value: &str, max_chars: usize) -> bool {
    value.chars().count() > max_chars || value.chars().any(char::is_control)
}
