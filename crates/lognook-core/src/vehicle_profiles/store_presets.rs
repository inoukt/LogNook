use super::{ProfileStoreError, VehicleProfileStore, decode, encode, validate_selection};
use crate::{
    PresetContext, PresetOrigin, PresetPickerState, PresetScope, ProfileSelectionSnapshot,
    SignalPreset, builtin_signal_presets,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

impl VehicleProfileStore {
    pub fn list_signal_presets(&self) -> Result<Vec<SignalPreset>, ProfileStoreError> {
        let mut presets = builtin_signal_presets();
        let mut statement = self.connection.prepare(
            "SELECT preset_json FROM custom_signal_presets ORDER BY name COLLATE NOCASE,preset_id",
        )?;
        presets.extend(
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .map(|json| decode(&json?))
                .collect::<Result<Vec<_>, ProfileStoreError>>()?,
        );
        Ok(presets)
    }

    pub fn get_signal_preset(&self, id: &str) -> Result<Option<SignalPreset>, ProfileStoreError> {
        if let Some(preset) = builtin_signal_presets()
            .into_iter()
            .find(|preset| preset.id == id)
        {
            return Ok(Some(preset));
        }
        let json = self
            .connection
            .query_row(
                "SELECT preset_json FROM custom_signal_presets WHERE preset_id=?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        json.as_deref().map(decode).transpose()
    }

    pub fn save_signal_preset(
        &mut self,
        mut preset: SignalPreset,
        expected_revision: Option<u64>,
    ) -> Result<SignalPreset, ProfileStoreError> {
        validate_preset(self, &preset)?;
        if builtin_signal_presets()
            .iter()
            .any(|item| item.id == preset.id)
        {
            return Err(ProfileStoreError::Invalid(
                "Built-in presets cannot be overwritten; save a copy with a custom ID".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let actual = transaction
            .query_row(
                "SELECT revision FROM custom_signal_presets WHERE preset_id=?1",
                [&preset.id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|revision| {
                u64::try_from(revision).map_err(|_| {
                    ProfileStoreError::Invalid("Stored preset revision is negative".into())
                })
            })
            .transpose()?;
        if actual != expected_revision {
            return Err(ProfileStoreError::RevisionConflict {
                expected: expected_revision,
                actual,
            });
        }
        preset.revision = actual
            .unwrap_or(0)
            .saturating_add(1)
            .try_into()
            .map_err(|_| {
                ProfileStoreError::Invalid("Preset revision exceeds its supported range".into())
            })?;
        preset.origin = PresetOrigin::Custom;
        if preset.selection.is_none() {
            let mut selection = ProfileSelectionSnapshot::default();
            for channel in &preset.channels {
                if let Some(signal_id) = &channel.signal_id {
                    selection.log_signal_ids.push(signal_id.clone());
                    selection.graph_signal_ids.push(signal_id.clone());
                }
                for reference in &channel.oem_alternatives {
                    let id = format!(
                        "oem:{}",
                        reference
                            .candidate_id
                            .strip_prefix("oem:")
                            .unwrap_or(&reference.candidate_id)
                    );
                    selection.log_signal_ids.push(id.clone());
                    selection.graph_signal_ids.push(id);
                    selection.oem_references.push(reference.clone());
                }
            }
            selection.log_signal_ids.sort();
            selection.log_signal_ids.dedup();
            selection.graph_signal_ids.sort();
            selection.graph_signal_ids.dedup();
            selection
                .oem_references
                .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
            selection.oem_references.dedup_by(|a, b| a == b);
            preset.selection = Some(selection);
        }
        validate_selection(
            preset
                .selection
                .as_ref()
                .expect("custom selection normalized"),
        )?;
        transaction.execute(
            "INSERT INTO custom_signal_presets(preset_id,revision,name,scope,criteria_json,selection_json,preset_json) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(preset_id) DO UPDATE SET revision=excluded.revision,name=excluded.name,scope=excluded.scope,criteria_json=excluded.criteria_json,selection_json=excluded.selection_json,preset_json=excluded.preset_json",
            params![preset.id, preset.revision, preset.name.trim(), scope_text(preset.scope), encode(&preset.criteria)?, encode(preset.selection.as_ref().expect("custom selection normalized"))?, encode(&preset)?],
        )?;
        transaction.commit()?;
        Ok(preset)
    }

    pub fn delete_signal_preset(
        &mut self,
        id: &str,
        expected_revision: Option<u64>,
    ) -> Result<(), ProfileStoreError> {
        if builtin_signal_presets().iter().any(|item| item.id == id) {
            return Err(ProfileStoreError::Invalid(
                "Built-in presets cannot be deleted".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let actual = transaction
            .query_row(
                "SELECT revision FROM custom_signal_presets WHERE preset_id=?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|revision| {
                u64::try_from(revision).map_err(|_| {
                    ProfileStoreError::Invalid("Stored preset revision is negative".into())
                })
            })
            .transpose()?;
        if actual != expected_revision {
            return Err(ProfileStoreError::RevisionConflict {
                expected: expected_revision,
                actual,
            });
        }
        if actual.is_none() {
            return Err(ProfileStoreError::Invalid(format!(
                "Custom preset '{id}' does not exist"
            )));
        }
        transaction.execute("DELETE FROM custom_signal_presets WHERE preset_id=?1", [id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_preset_picker_state(&self) -> Result<PresetPickerState, ProfileStoreError> {
        let (revision, context_json, staged_json): (i64, String, Option<String>) = self.connection.query_row(
            "SELECT revision,context_json,staged_json FROM preset_picker_state WHERE singleton=1",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        )?;
        let revision = u64::try_from(revision)
            .map_err(|_| ProfileStoreError::Invalid("Stored picker revision is negative".into()))?;
        let mut context: PresetContext = if context_json == "{}" {
            PresetContext::default()
        } else {
            decode(&context_json)?
        };
        context.connection_generation = None;
        context.revision = revision;
        Ok(PresetPickerState {
            revision,
            context,
            staged: staged_json.as_deref().map(decode).transpose()?,
        })
    }

    pub fn save_preset_picker_state(
        &mut self,
        mut state: PresetPickerState,
        expected_revision: Option<u64>,
    ) -> Result<PresetPickerState, ProfileStoreError> {
        validate_context(&state.context)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual: i64 = transaction.query_row(
            "SELECT revision FROM preset_picker_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        let actual = u64::try_from(actual)
            .map_err(|_| ProfileStoreError::Invalid("Stored picker revision is negative".into()))?;
        if Some(actual) != expected_revision {
            return Err(ProfileStoreError::RevisionConflict {
                expected: expected_revision,
                actual: Some(actual),
            });
        }
        if let Some(staged) = &state.staged {
            let builtin = builtin_signal_presets()
                .into_iter()
                .find(|preset| preset.id == staged.preset_id);
            let current_revision = if let Some(preset) = builtin {
                Some(preset.revision)
            } else {
                transaction
                    .query_row(
                        "SELECT revision FROM custom_signal_presets WHERE preset_id=?1",
                        [&staged.preset_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .map(|revision| {
                        u32::try_from(revision).map_err(|_| {
                            ProfileStoreError::Invalid(
                                "Stored preset revision is outside its supported range".into(),
                            )
                        })
                    })
                    .transpose()?
            };
            let Some(current_revision) = current_revision else {
                return Err(ProfileStoreError::Invalid(
                    "Staged preset does not exist".into(),
                ));
            };
            if current_revision != staged.preset_revision {
                return Err(ProfileStoreError::RevisionConflict {
                    expected: Some(staged.preset_revision as u64),
                    actual: Some(current_revision as u64),
                });
            }
        }
        state.revision = actual.saturating_add(1);
        state.context.revision = state.revision;
        state.context.connection_generation = None;
        transaction.execute(
            "UPDATE preset_picker_state SET revision=?1,context_json=?2,staged_json=?3 WHERE singleton=1",
            params![i64::try_from(state.revision).map_err(|_| ProfileStoreError::Invalid("Picker revision exceeds SQLite's range".into()))?, encode(&state.context)?, state.staged.as_ref().map(encode).transpose()?],
        )?;
        transaction.commit()?;
        Ok(state)
    }
}

fn validate_preset(
    store: &VehicleProfileStore,
    preset: &SignalPreset,
) -> Result<(), ProfileStoreError> {
    if preset.id.trim().is_empty()
        || preset.id.len() > 128
        || preset.id.chars().any(char::is_control)
    {
        return Err(ProfileStoreError::Invalid(
            "Preset ID must be nonempty and at most 128 non-control characters".into(),
        ));
    }
    if preset.name.trim().is_empty()
        || preset.name.chars().count() > 120
        || preset.channels.len() > 500
    {
        return Err(ProfileStoreError::Invalid(
            "Preset name or channel count is outside the supported bounds".into(),
        ));
    }
    match preset.scope {
        PresetScope::Generic if preset.criteria != Default::default() => {
            return Err(ProfileStoreError::Invalid(
                "Generic presets cannot have vehicle criteria".into(),
            ));
        }
        PresetScope::ModelPowertrain
            if preset
                .criteria
                .make
                .as_deref()
                .is_none_or(|v| v.trim().is_empty())
                || preset
                    .criteria
                    .model
                    .as_deref()
                    .is_none_or(|v| v.trim().is_empty())
                || preset.criteria.profile_id.is_some() =>
        {
            return Err(ProfileStoreError::Invalid(
                "Model/powertrain presets require make and model and cannot name a profile".into(),
            ));
        }
        PresetScope::ExactProfile => {
            let id = preset
                .criteria
                .profile_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    ProfileStoreError::Invalid("Exact-profile presets require a profile ID".into())
                })?;
            if store.get(id)?.is_none() {
                return Err(ProfileStoreError::Invalid(
                    "Exact-profile presets must reference an existing vehicle profile".into(),
                ));
            }
        }
        _ => {}
    }
    if let Some(criteria) = [
        &preset.criteria.make,
        &preset.criteria.model,
        &preset.criteria.engine,
        &preset.criteria.transmission,
        &preset.criteria.profile_id,
    ]
    .into_iter()
    .flatten()
    .find(|v| v.trim().is_empty() || v.chars().count() > 120 || v.chars().any(char::is_control))
    {
        let _ = criteria;
        return Err(ProfileStoreError::Invalid(
            "Preset vehicle criteria must be nonblank and at most 120 characters".into(),
        ));
    }
    if preset
        .criteria
        .year
        .is_some_and(|year| !(1886..=2100).contains(&year))
    {
        return Err(ProfileStoreError::Invalid(
            "Preset model year must be between 1886 and 2100".into(),
        ));
    }
    let selection = preset.selection.clone().unwrap_or_default();
    validate_selection(&selection)?;
    if selection
        .graph_signal_ids
        .iter()
        .any(|id| !selection.log_signal_ids.contains(id))
    {
        return Err(ProfileStoreError::Invalid(
            "Every preset graph signal must also be selected for logging".into(),
        ));
    }
    if preset.channels.iter().any(|channel| {
        channel.concept.trim().is_empty() || channel.concept.len() > 120 || channel.priority > 100
    }) {
        return Err(ProfileStoreError::Invalid(
            "Preset channels require bounded concepts and priorities".into(),
        ));
    }
    Ok(())
}

fn validate_context(context: &PresetContext) -> Result<(), ProfileStoreError> {
    if [
        &context.make,
        &context.model,
        &context.engine,
        &context.transmission,
        &context.profile_id,
    ]
    .into_iter()
    .flatten()
    .any(|value| {
        value.trim().is_empty()
            || value.chars().count() > 120
            || value.chars().any(char::is_control)
    }) || context
        .year
        .is_some_and(|year| !(1886..=2100).contains(&year))
    {
        return Err(ProfileStoreError::Invalid(
            "Picker context values are outside the supported bounds".into(),
        ));
    }
    Ok(())
}

fn scope_text(scope: PresetScope) -> &'static str {
    match scope {
        PresetScope::Generic => "generic",
        PresetScope::ModelPowertrain => "model_powertrain",
        PresetScope::ExactProfile => "exact_profile",
    }
}
