//! Local vehicle/ECU profile preferences, separate from the researched PID catalog.
mod history;
mod identity;
mod resolve;
mod store;
pub mod vin_decode;

pub use history::{
    DiagnosticDtcCounts, DiagnosticHistoryFilter, DiagnosticHistoryPage, DiagnosticHistoryRow,
    DiagnosticReportDetail, DiagnosticReportLocator, HistoryEvidenceStatus,
    HistoryImportCheckpoint, HistoryIndexState, HistoryIndexStatus,
    MAX_DIAGNOSTIC_REPORT_LINE_BYTES, ManualReportAssociation,
};
pub use identity::{
    ManualDetailActor, ManualVehicleDetail, ManualVehicleField, VehicleIdentityRecord,
    VehicleIdentitySnapshot, VehicleIdentitySource, VinEvidenceSource, VinOrigin,
};
pub use vin_decode::*;

use crate::{CanSignalDefinition, DerivedSignalDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use resolve::{
    canonical_vin_identity, decoder_revision_v1, fingerprint_v1, resolve_profile_match,
};
pub use store::{ProfileStoreError, VehicleProfileStore};

pub const VEHICLE_PROFILE_SCHEMA_VERSION: u32 = 2;
pub const MAX_PROFILE_NAME_CHARS: usize = 120;
pub const MAX_PROFILE_SELECTION_REFS: usize = 500;

pub fn derived_signal_dependencies(
    definitions: &[DerivedSignalDefinition],
) -> Result<HashMap<String, Vec<String>>, crate::LogNookError> {
    crate::derived::compile_derived_signals(definitions, &crate::standard_obd_signals()).map(
        |compiled| {
            compiled
                .into_iter()
                .map(|signal| {
                    let mut dependencies =
                        signal.dependencies().iter().cloned().collect::<Vec<_>>();
                    dependencies.sort();
                    (signal.definition.id, dependencies)
                })
                .collect()
        },
    )
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(schemars::JsonSchema)]
pub enum ProfileScope {
    Live,
    Simulator,
    CanOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(schemars::JsonSchema)]
pub enum IdentityReadState {
    Positive,
    Unsupported,
    NoResponse,
    Malformed,
    TransportError,
    NotAttempted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct EcuProfileVariant {
    pub protocol: String,
    /// Diagnostic request transport/route label, for example `7DF (functional)`.
    pub request_route: String,
    /// Physical request address when the connection explicitly identified one.
    /// Functional 7DF requests do not establish a physical request address.
    pub request_address: Option<String>,
    /// Actual responding ECU address, retained from the diagnostic response.
    pub response_address: String,
    pub ecu_name: Option<String>,
    pub calibration_ids: Vec<String>,
    pub software_ids: Vec<String>,
    pub ecu_name_state: IdentityReadState,
    pub calibration_state: IdentityReadState,
    pub software_state: IdentityReadState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct OemDefinitionReference {
    pub candidate_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub decoder_revision: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ProfileSelectionSnapshot {
    pub log_signal_ids: Vec<String>,
    pub graph_signal_ids: Vec<String>,
    pub derived_definitions: Vec<DerivedSignalDefinition>,
    pub can_fields: Vec<CanSignalDefinition>,
    #[serde(default)]
    pub can_source_id: Option<String>,
    #[serde(default)]
    pub can_bitrate: Option<u32>,
    #[serde(default)]
    pub can_channel: Option<u8>,
    pub oem_references: Vec<OemDefinitionReference>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct VehicleIdentityObservation {
    pub obd_connection_generation: u64,
    pub observed_at_unix_ms: u64,
    pub scope: ProfileScope,
    pub vin_digest: Option<String>,
    pub masked_vin: Option<String>,
    pub vin_status: IdentityReadState,
    #[serde(default)]
    pub conflicting_vin_observations: bool,
    pub ecu_variants: Vec<EcuProfileVariant>,
    pub identity_supported: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct VehicleProfile {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub scope: ProfileScope,
    /// Optional explicit research-catalog association; not vehicle identity evidence.
    pub catalog_vehicle_id: Option<String>,
    pub vin_digest: Option<String>,
    pub masked_vin: Option<String>,
    /// An owner-entered association; never proof of vehicle identity.
    pub manual_association: bool,
    pub ecu_variants: Vec<EcuProfileVariant>,
    pub selection: ProfileSelectionSnapshot,
    pub observations: Vec<VehicleIdentityObservation>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(schemars::JsonSchema)]
pub enum ProfileMatchState {
    Exact,
    ReviewRequired,
    IdentityUnavailable,
    Conflict,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProfileMatch {
    pub state: ProfileMatchState,
    pub fingerprint_digest: Option<String>,
    pub matched_ecu_variants: Vec<String>,
    pub review_reasons: Vec<String>,
}

/// Retain previously observed ECU identity when a refresh cannot repeat a field or module.
pub fn merge_ecu_identity_snapshot(
    saved: &[EcuProfileVariant],
    observed: &[EcuProfileVariant],
) -> Vec<EcuProfileVariant> {
    let mut merged = Vec::with_capacity(saved.len().max(observed.len()));
    for current in observed {
        let previous = saved.iter().find(|saved| same_ecu_route(saved, current));
        let Some(previous) = previous else {
            merged.push(current.clone());
            continue;
        };
        let mut variant = current.clone();
        if current.calibration_ids.is_empty()
            || current.calibration_state != IdentityReadState::Positive
        {
            variant.calibration_ids = previous.calibration_ids.clone();
            variant.calibration_state = if previous.calibration_ids.is_empty() {
                current.calibration_state
            } else {
                previous.calibration_state
            };
        }
        if current.software_ids.is_empty() || current.software_state != IdentityReadState::Positive
        {
            variant.software_ids = previous.software_ids.clone();
            variant.software_state = if previous.software_ids.is_empty() {
                current.software_state
            } else {
                previous.software_state
            };
        }
        if current.ecu_name.as_deref().is_none_or(str::is_empty)
            || current.ecu_name_state != IdentityReadState::Positive
        {
            variant.ecu_name = previous.ecu_name.clone();
            variant.ecu_name_state = if previous.ecu_name.is_some() {
                previous.ecu_name_state
            } else {
                current.ecu_name_state
            };
        }
        merged.push(variant);
    }
    for previous in saved {
        if !observed
            .iter()
            .any(|current| same_ecu_route(previous, current))
        {
            merged.push(previous.clone());
        }
    }
    merged.sort_by(|left, right| {
        (
            &left.protocol,
            &left.request_route,
            &left.request_address,
            &left.response_address,
        )
            .cmp(&(
                &right.protocol,
                &right.request_route,
                &right.request_address,
                &right.response_address,
            ))
    });
    merged
}

fn same_ecu_route(left: &EcuProfileVariant, right: &EcuProfileVariant) -> bool {
    left.protocol.eq_ignore_ascii_case(right.protocol.as_str())
        && left
            .request_route
            .eq_ignore_ascii_case(right.request_route.as_str())
        && left.request_address.as_deref().map(str::to_ascii_uppercase)
            == right
                .request_address
                .as_deref()
                .map(str::to_ascii_uppercase)
        && left
            .response_address
            .eq_ignore_ascii_case(right.response_address.as_str())
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    #[test]
    fn partial_ecu_refresh_preserves_missing_calibration_and_disappeared_modules() {
        let variant = |response: &str, calibration: Vec<&str>, state| EcuProfileVariant {
            protocol: "ISO15765-4 CAN".into(),
            request_route: "7DF functional".into(),
            request_address: None,
            response_address: response.into(),
            ecu_name: Some("PCM".into()),
            calibration_ids: calibration.into_iter().map(str::to_owned).collect(),
            software_ids: vec![],
            ecu_name_state: IdentityReadState::Positive,
            calibration_state: state,
            software_state: IdentityReadState::TransportError,
        };
        let saved = vec![
            variant("7E8", vec!["ABC123"], IdentityReadState::Positive),
            variant("7C8", vec!["BODY01"], IdentityReadState::Positive),
        ];
        let observed = vec![variant("7E8", vec![], IdentityReadState::NoResponse)];
        let merged = merge_ecu_identity_snapshot(&saved, &observed);
        assert_eq!(merged.len(), 2);
        assert_eq!(
            merged
                .iter()
                .find(|ecu| ecu.response_address == "7E8")
                .unwrap()
                .calibration_ids,
            ["ABC123"]
        );
        assert_eq!(
            merged
                .iter()
                .find(|ecu| ecu.response_address == "7C8")
                .unwrap()
                .calibration_ids,
            ["BODY01"]
        );
    }
}
