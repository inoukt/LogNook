use super::{ManualDetailActor, ProfileScope, VehicleIdentitySnapshot, VehicleIdentitySource};
use crate::vehicle_diagnostics::DiagnosticReport;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_DIAGNOSTIC_REPORT_LINE_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DiagnosticDtcCounts {
    pub stored: Option<u32>,
    pub pending: Option<u32>,
    pub permanent: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DiagnosticHistoryFilter {
    pub scope: Option<ProfileScope>,
    pub vin_digest: Option<String>,
    pub ecu_id: Option<String>,
    pub from_unix_ms: Option<u64>,
    pub to_unix_ms: Option<u64>,
    pub page_size: Option<u16>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DiagnosticHistoryRow {
    pub report_id: String,
    pub started_at_unix_ms: u64,
    pub scope: ProfileScope,
    pub vin_digest: Option<String>,
    pub masked_vin: Option<String>,
    pub identity_source: VehicleIdentitySource,
    pub vin_conflict: bool,
    pub ecu_ids: Vec<String>,
    pub dtc_counts: DiagnosticDtcCounts,
    pub mode06_read_count: u32,
    /// `available` reflects an indexed source; the report-detail reader must verify its bytes and SHA-256.
    pub evidence_status: HistoryEvidenceStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DiagnosticReportLocator {
    pub report_id: String,
    pub source_relative_path: String,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub sha256: String,
    pub started_at_unix_ms: u64,
    pub scope: ProfileScope,
    pub vin_digest: Option<String>,
    pub masked_vin: Option<String>,
    pub identity_source: VehicleIdentitySource,
    pub vin_conflict: bool,
    pub ecu_ids: Vec<String>,
    pub dtc_counts: DiagnosticDtcCounts,
    pub mode06_read_count: u32,
    pub context_snapshot: VehicleIdentitySnapshot,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryIndexState {
    Idle,
    Indexing,
    Complete,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HistoryIndexStatus {
    pub state: HistoryIndexState,
    pub indexed_reports: u64,
    pub scanned_bytes: u64,
    pub total_bytes: u64,
    pub malformed_lines: u64,
    pub oversized_lines: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HistoryImportCheckpoint {
    pub status: HistoryIndexStatus,
    pub source_relative_path: Option<String>,
    pub next_byte_offset: u64,
    pub trailing_partial_line: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEvidenceStatus {
    Available,
    Missing,
    Changed,
    Malformed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiagnosticReportDetail {
    pub index: DiagnosticHistoryRow,
    pub evidence_status: HistoryEvidenceStatus,
    pub report: Option<DiagnosticReport>,
    #[serde(default)]
    pub context_snapshot: Option<VehicleIdentitySnapshot>,
    #[serde(default)]
    pub manual_associations: Vec<ManualReportAssociation>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DiagnosticHistoryPage {
    pub rows: Vec<DiagnosticHistoryRow>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ManualReportAssociation {
    pub report_id: String,
    pub scope: ProfileScope,
    pub vin_digest: String,
    pub actor: ManualDetailActor,
    pub associated_at_unix_ms: u64,
    pub note: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct HistoryCursor {
    pub started_at_unix_ms: u64,
    pub report_id: String,
}

#[cfg(test)]
mod tests {
    use super::DiagnosticReportDetail;

    #[test]
    fn old_report_detail_payload_defaults_new_history_metadata() {
        let legacy: DiagnosticReportDetail = serde_json::from_value(serde_json::json!({
            "index": {
                "report_id": "old-report",
                "started_at_unix_ms": 1,
                "scope": "live",
                "vin_digest": null,
                "masked_vin": null,
                "identity_source": "unassigned",
                "vin_conflict": false,
                "ecu_ids": [],
                "dtc_counts": { "stored": null, "pending": null, "permanent": null },
                "mode06_read_count": 0,
                "evidence_status": "missing"
            },
            "evidence_status": "missing",
            "report": null
        }))
        .unwrap();
        assert_eq!(legacy.context_snapshot, None);
        assert!(legacy.manual_associations.is_empty());
    }
}
