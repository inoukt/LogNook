//! Serializable diagnostic recipe models and a conservative, read-only resolver.
use crate::{ConnectionCapabilities, OemDefinitionReference, ProfileSelectionSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresetTaskFamily {
    EngineTuning,
    CvtDiagnostics,
    RoughnessMisfires,
    TrimsAirFuel,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresetSize {
    Core,
    Extended,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresetScope {
    Generic,
    ModelPowertrain,
    ExactProfile,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresetOrigin {
    Builtin,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresetApplyMode {
    Replace,
    Add,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct StagedPresetIntent {
    pub preset_id: String,
    pub preset_revision: u32,
    pub mode: PresetApplyMode,
}

/// Offline picker preferences. `revision` is assigned by the profile store.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PresetPickerState {
    pub revision: u64,
    pub context: PresetContext,
    pub staged: Option<StagedPresetIntent>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PresetVehicleCriteria {
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u16>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub profile_id: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PresetContext {
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u16>,
    pub engine: Option<String>,
    pub transmission: Option<String>,
    pub profile_id: Option<String>,
    pub revision: u64,
    #[serde(default)]
    pub connection_generation: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PresetChannel {
    pub concept: String,
    /// Exact standard signal ID; absent for research-only concepts.
    pub signal_id: Option<String>,
    #[serde(default)]
    pub oem_alternatives: Vec<OemDefinitionReference>,
    pub task_defining: bool,
    pub priority: u8,
    pub slow_context: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SignalPreset {
    pub id: String,
    pub revision: u32,
    pub name: String,
    pub family: PresetTaskFamily,
    pub size: PresetSize,
    pub origin: PresetOrigin,
    pub scope: PresetScope,
    pub criteria: PresetVehicleCriteria,
    pub channels: Vec<PresetChannel>,
    #[serde(default)]
    pub selection: Option<ProfileSelectionSnapshot>,
    #[serde(default)]
    pub required_adapter: Option<String>,
    #[serde(default)]
    pub required_protocol: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresetSkipReason {
    Unsupported,
    ScopeMismatch,
    MissingVehicleDetails,
    AdapterMismatch,
    ProtocolMismatch,
    OemNotVerified,
    NoExactDefinition,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PresetAvailability {
    pub concept: String,
    pub signal_id: Option<String>,
    pub eligible: bool,
    pub reason: Option<PresetSkipReason>,
    pub task_defining: bool,
    pub priority: u8,
    pub slow_context: bool,
    pub source: Option<OemDefinitionReference>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PresetCatalogEvidence {
    pub reference: OemDefinitionReference,
    pub vehicle_verified: bool,
    pub supported: bool,
    /// Exact profile fingerprint whose ECU verification produced this evidence.
    pub profile_id: Option<String>,
    /// Vehicle context revision against which support was observed.
    pub context_revision: u64,
    pub connection_generation: u64,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct PresetResolvedSelection {
    pub preset_id: String,
    pub preset_revision: u32,
    pub context_revision: u64,
    pub connection_generation: Option<u64>,
    pub eligible: Vec<PresetAvailability>,
    pub skipped: Vec<PresetAvailability>,
    pub task_defining_eligible: usize,
    pub task_defining_total: usize,
    pub selection: ProfileSelectionSnapshot,
}

/// Shared typed request used by the desktop picker and Agent Access.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SignalPresetOperation {
    List,
    Get { id: String },
    Suggest { context: PresetContext },
    SetContext { context: PresetContext, expected_revision: u64 },
    Stage { preset_id: String, preset_revision: u32, mode: PresetApplyMode, expected_revision: u64 },
    Preview { preset_id: String, mode: PresetApplyMode, expected_context_revision: u64, expected_generation: u64 },
    Apply { preview_id: String, expected_context_revision: u64, expected_generation: u64 },
    Save { preset: SignalPreset, expected_revision: Option<u64> },
    Delete { id: String, expected_revision: u64 },
    Status,
    Cancel { preview_id: Option<String> },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalPresetResult {
    Presets { presets: Vec<SignalPreset> },
    Preset { preset: Option<SignalPreset> },
    Suggestions { presets: Vec<SignalPreset>, context: PresetContext },
    PickerState { state: PresetPickerState },
    Preview { resolved: PresetResolvedSelection, preview_id: String, mode: PresetApplyMode, can_apply: bool },
    Applied { resolved: PresetResolvedSelection, mode: PresetApplyMode },
    Status { connected: bool, generation: u64, context: PresetContext, staged: Option<StagedPresetIntent>, preview_id: Option<String> },
    Ok,
}

/// Returns higher scores for a more specific scope match; generic browsing remains valid without vehicle details.
pub fn scope_match_rank(
    scope: PresetScope,
    criteria: &PresetVehicleCriteria,
    context: &PresetContext,
) -> Option<u8> {
    if scope == PresetScope::Generic {
        return Some(0);
    }
    let pairs = [
        (&criteria.make, &context.make),
        (&criteria.model, &context.model),
    ];
    if pairs.iter().any(|(a, b)| {
        a.as_ref()
            .is_some_and(|a| b.as_ref().is_none_or(|b| !a.eq_ignore_ascii_case(b)))
    }) {
        return None;
    }
    if criteria.year.is_some_and(|y| context.year != Some(y))
        || criteria.engine.as_ref().is_some_and(|v| {
            context
                .engine
                .as_ref()
                .is_none_or(|x| !v.eq_ignore_ascii_case(x))
        })
        || criteria.transmission.as_ref().is_some_and(|v| {
            context
                .transmission
                .as_ref()
                .is_none_or(|x| !v.eq_ignore_ascii_case(x))
        })
    {
        return None;
    }
    match scope {
        PresetScope::Generic => Some(0),
        PresetScope::ModelPowertrain
            if criteria.make.as_deref().is_some_and(nonblank)
                && criteria.model.as_deref().is_some_and(nonblank) =>
        {
            Some(1)
        }
        PresetScope::ModelPowertrain => None,
        PresetScope::ExactProfile
            if criteria.profile_id.as_deref().is_some_and(nonblank)
                && criteria.profile_id == context.profile_id =>
        {
            Some(2)
        }
        PresetScope::ExactProfile => None,
    }
}
fn nonblank(s: &str) -> bool {
    !s.trim().is_empty()
}

pub fn resolve_preset(
    preset: &SignalPreset,
    context: &PresetContext,
    capabilities: &ConnectionCapabilities,
    catalog_evidence: &[PresetCatalogEvidence],
) -> PresetResolvedSelection {
    let scope_ok = scope_match_rank(preset.scope, &preset.criteria, context).is_some();
    let mut eligible = Vec::new();
    let mut skipped = Vec::new();
    let saved_selection = preset.selection.clone().unwrap_or_default();
    // Resolved application data starts empty; saved derived/CAN definitions are not validated here.
    let mut selection = ProfileSelectionSnapshot::default();
    let mut task_defining_total = 0;
    let mut task_defining_eligible = 0;
    for slot in &preset.channels {
        if slot.task_defining {
            task_defining_total += 1;
        }
        let mut item = PresetAvailability {
            concept: slot.concept.clone(),
            signal_id: slot.signal_id.clone(),
            eligible: false,
            reason: None,
            task_defining: slot.task_defining,
            priority: slot.priority,
            slow_context: slot.slow_context,
            source: None,
        };
        if !scope_ok {
            item.reason = Some(
                if missing_vehicle_details(preset.scope, &preset.criteria, context) {
                    PresetSkipReason::MissingVehicleDetails
                } else {
                    PresetSkipReason::ScopeMismatch
                },
            );
        } else if preset
            .required_adapter
            .as_ref()
            .is_some_and(|a| !capabilities.device.hardware_family.eq_ignore_ascii_case(a))
        {
            item.reason = Some(PresetSkipReason::AdapterMismatch);
        } else if preset.required_protocol.as_ref().is_some_and(|p| {
            !capabilities
                .protocol
                .to_ascii_lowercase()
                .contains(&p.to_ascii_lowercase())
        }) {
            item.reason = Some(PresetSkipReason::ProtocolMismatch);
        } else if let Some(id) = &slot.signal_id {
            if capabilities.read_only && capabilities.supported_signals.iter().any(|s| &s.id == id)
            {
                item.eligible = true;
            } else {
                item.reason = Some(PresetSkipReason::Unsupported);
            }
        } else {
            let match_evidence = slot.oem_alternatives.iter().find_map(|reference| {
                catalog_evidence
                    .iter()
                    .find(|e| {
                        e.reference == *reference
                            && e.vehicle_verified
                            && e.supported
                            && context
                                .profile_id
                                .as_ref()
                                .is_some_and(|id| e.profile_id.as_ref() == Some(id))
                            && context.connection_generation == Some(e.connection_generation)
                            && e.context_revision == context.revision
                    })
                    .map(|e| (reference, e))
            });
            if let Some((reference, _)) = match_evidence {
                item.eligible = true;
                item.source = Some(reference.clone());
                item.signal_id = Some(oem_signal_id(&reference.candidate_id));
            } else {
                item.reason = Some(if slot.oem_alternatives.is_empty() {
                    PresetSkipReason::NoExactDefinition
                } else {
                    PresetSkipReason::OemNotVerified
                });
            }
        }
        if item.eligible {
            if let Some(id) = &item.signal_id {
                let log_selected =
                    preset.selection.is_none() || saved_selection.log_signal_ids.contains(id);
                let graph_selected =
                    preset.selection.is_none() || saved_selection.graph_signal_ids.contains(id);
                if log_selected {
                    selection.log_signal_ids.push(id.clone());
                    if let Some(source) = &item.source {
                        selection.oem_references.push(source.clone());
                    }
                }
                if graph_selected {
                    selection.graph_signal_ids.push(id.clone());
                }
            }
            if slot.task_defining {
                task_defining_eligible += 1;
            }
            eligible.push(item);
        } else {
            skipped.push(item);
        }
    }
    dedup(&mut selection.log_signal_ids);
    dedup(&mut selection.graph_signal_ids);
    selection
        .oem_references
        .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
    selection.oem_references.dedup_by(|a, b| a == b);
    PresetResolvedSelection {
        preset_id: preset.id.clone(),
        preset_revision: preset.revision,
        context_revision: context.revision,
        connection_generation: context.connection_generation,
        eligible,
        skipped,
        task_defining_eligible,
        task_defining_total,
        selection,
    }
}
fn dedup(ids: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
}
fn oem_signal_id(candidate_id: &str) -> String {
    format!(
        "oem:{}",
        candidate_id.strip_prefix("oem:").unwrap_or(candidate_id)
    )
}
fn missing_vehicle_details(
    scope: PresetScope,
    criteria: &PresetVehicleCriteria,
    context: &PresetContext,
) -> bool {
    if scope == PresetScope::Generic {
        return false;
    }
    (criteria.make.is_some() && context.make.is_none())
        || (criteria.model.is_some() && context.model.is_none())
        || (criteria.year.is_some() && context.year.is_none())
        || (criteria.engine.is_some() && context.engine.is_none())
        || (criteria.transmission.is_some() && context.transmission.is_none())
        || (scope == PresetScope::ExactProfile
            && criteria.profile_id.is_some()
            && context.profile_id.is_none())
}

pub fn builtin_signal_presets() -> Vec<SignalPreset> {
    serde_json::from_str(include_str!("../resources/signal-presets.json"))
        .expect("bundled signal preset data is valid")
}
