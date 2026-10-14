use super::{EcuProfileVariant, ProfileScope, vin_decode::VinDecodeResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VinOrigin {
    EcuReport,
    EcuRecognition,
    LegacyReportImport,
    OwnerEntered,
    AgentEntered,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualVehicleField {
    Make,
    Model,
    ModelYear,
    Engine,
    Trim,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualDetailActor {
    OwnerUi,
    AgentAccess,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ManualVehicleDetail {
    pub field: ManualVehicleField,
    pub value: String,
    pub actor: ManualDetailActor,
    pub entered_at_unix_ms: u64,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct VinEvidenceSource {
    pub origin: VinOrigin,
    pub recorded_at_unix_ms: u64,
    pub report_id: Option<String>,
    pub ecu_id: Option<String>,
    pub connection_generation: Option<u64>,
    pub actor: Option<ManualDetailActor>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VehicleIdentitySource {
    ReportEcu,
    SameGenerationRecognition,
    ManualProfile,
    Conflict,
    Unassigned,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct VehicleIdentityRecord {
    pub scope: ProfileScope,
    pub vin_digest: String,
    pub full_vin: Option<String>,
    pub origin_evidence: Vec<VinEvidenceSource>,
    pub decoder_snapshot: Option<VinDecodeResult>,
    pub owner_details: Vec<ManualVehicleDetail>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct VehicleIdentitySnapshot {
    pub scope: ProfileScope,
    pub connection_generation: Option<u64>,
    pub profile_id: Option<String>,
    pub profile_revision: Option<u64>,
    pub full_vin: Option<String>,
    pub vin_digest: Option<String>,
    pub masked_vin: Option<String>,
    pub origin: Option<VinOrigin>,
    pub identity_source: VehicleIdentitySource,
    pub vin_conflict: bool,
    pub observed_ecus: Vec<EcuProfileVariant>,
    pub decoded_snapshot: Option<VinDecodeResult>,
    pub owner_details: Vec<ManualVehicleDetail>,
}
