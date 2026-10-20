//! Bounded, per-VIN workspace state stored beside vehicle profiles.
use crate::ProfileSelectionSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VEHICLE_PROJECT_WORKSPACE_VERSION: u32 = 1;
pub const MAX_VEHICLE_PROJECT_WORKSPACE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct VehicleProjectWorkspace {
    pub version: u32,
    #[serde(default)]
    pub graph_preferences: Option<Value>,
    #[serde(default)]
    pub layout_preferences: Option<Value>,
    #[serde(default)]
    pub catalog_filters: Value,
    #[serde(default)]
    pub current_preset_id: Option<String>,
}

impl VehicleProjectWorkspace {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VEHICLE_PROJECT_WORKSPACE_VERSION {
            return Err("Vehicle project workspace version is unsupported".into());
        }
        if self.current_preset_id.as_ref().is_some_and(|id| {
            id.is_empty() || id.len() > 128 || id.chars().any(char::is_control)
        }) {
            return Err("Current preset ID is invalid".into());
        }
        let size = serde_json::to_vec(self).map_err(|error| error.to_string())?.len();
        if size > MAX_VEHICLE_PROJECT_WORKSPACE_BYTES {
            return Err("Vehicle project workspace exceeds 256 KiB".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct VehicleProjectSave {
    pub expected_revision: u64,
    pub selection: ProfileSelectionSnapshot,
    pub workspace: VehicleProjectWorkspace,
}
