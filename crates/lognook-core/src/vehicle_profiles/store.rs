use super::{
    EcuProfileVariant, MAX_PROFILE_NAME_CHARS, MAX_PROFILE_SELECTION_REFS, ProfileScope,
    ProfileSelectionSnapshot, VEHICLE_PROFILE_SCHEMA_VERSION, VehicleIdentityObservation,
    VehicleProfile,
};
use crate::standard_obd_signals;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::{
    fmt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const SCHEMA_VERSION: i64 = VEHICLE_PROFILE_SCHEMA_VERSION as i64;

#[path = "store_history.rs"]
mod history;
#[path = "store_identity.rs"]
mod identity;
#[path = "store_presets.rs"]
mod presets;

pub struct VehicleProfileStore {
    connection: Connection,
}

#[derive(Debug)]
pub enum ProfileStoreError {
    Sqlite(rusqlite::Error),
    Invalid(String),
    RevisionConflict {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    UnsupportedSchema(i64),
    Encoding(String),
}

impl fmt::Display for ProfileStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "Profile store error: {error}"),
            Self::Invalid(message) | Self::Encoding(message) => f.write_str(message),
            Self::RevisionConflict { expected, actual } => write!(
                f,
                "Profile revision changed (expected {expected:?}, current {actual:?})"
            ),
            Self::UnsupportedSchema(version) => write!(
                f,
                "Profile store schema version {version} is not supported; the database was left untouched"
            ),
        }
    }
}

impl std::error::Error for ProfileStoreError {}

impl From<rusqlite::Error> for ProfileStoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl VehicleProfileStore {
    pub fn open(path: &Path) -> Result<Self, ProfileStoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ProfileStoreError::Invalid(format!("Could not create profile directory: {error}"))
            })?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => initialize(&connection)?,
            1 => migrate_v1_to_v2(&connection)?,
            2 => {}
            3 => {}
            SCHEMA_VERSION => {}
            other => return Err(ProfileStoreError::UnsupportedSchema(other)),
        }
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 2 {
            migrate_v2_to_v3(&connection)?;
        }
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 3 {
            migrate_v3_to_v4(&connection)?;
        }
        Ok(Self { connection })
    }

    pub fn list(&self) -> Result<Vec<VehicleProfile>, ProfileStoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT profile_id FROM profiles ORDER BY name COLLATE NOCASE, profile_id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.iter()
            .map(|id| {
                self.get(id)
                    .map(|profile| profile.expect("selected profile exists"))
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<Option<VehicleProfile>, ProfileStoreError> {
        let base = self.connection.query_row(
            "SELECT name, revision, scope, catalog_vehicle_id, vin_digest, masked_vin, manual_association, created_at_unix_ms, updated_at_unix_ms FROM profiles WHERE profile_id = ?1",
            [id],
            |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?, row.get::<_, bool>(6)?,
                row.get::<_, i64>(7)?, row.get::<_, i64>(8)?,
            )),
        ).optional()?;
        let Some((
            name,
            revision,
            scope,
            catalog_vehicle_id,
            vin_digest,
            masked_vin,
            manual_association,
            created_at_unix_ms,
            updated_at_unix_ms,
        )) = base
        else {
            return Ok(None);
        };
        let ecu_variants = load_json_rows::<EcuProfileVariant>(
            &self.connection,
            "SELECT value_json FROM ecu_variants WHERE profile_id = ?1 ORDER BY ordinal",
            id,
        )?;
        let selection_json: String = self.connection.query_row(
            "SELECT value_json FROM selection_snapshots WHERE profile_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let selection = decode(&selection_json)?;
        let observations = load_json_rows::<VehicleIdentityObservation>(
            &self.connection,
            "SELECT value_json FROM identity_observations WHERE profile_id = ?1 ORDER BY ordinal",
            id,
        )?;
        Ok(Some(VehicleProfile {
            id: id.into(),
            name,
            revision: from_sql_u64(revision)?,
            scope: parse_scope(&scope)?,
            catalog_vehicle_id,
            vin_digest,
            masked_vin,
            manual_association,
            ecu_variants,
            selection,
            observations,
            created_at_unix_ms: from_sql_u64(created_at_unix_ms)?,
            updated_at_unix_ms: from_sql_u64(updated_at_unix_ms)?,
        }))
    }

    pub fn get_project_workspace(
        &self,
        profile_id: &str,
    ) -> Result<crate::VehicleProjectWorkspace, ProfileStoreError> {
        let value = self.connection.query_row(
            "SELECT workspace_json FROM vehicle_project_workspaces WHERE profile_id=?1",
            [profile_id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        match value {
            Some(json) => serde_json::from_str(&json).map_err(|error| ProfileStoreError::Encoding(error.to_string())),
            None => Ok(crate::VehicleProjectWorkspace { version: crate::VEHICLE_PROJECT_WORKSPACE_VERSION, ..Default::default() }),
        }
    }

    pub fn get_vehicle_project_id(&self, scope: ProfileScope, vin_digest: &str) -> Result<Option<String>, ProfileStoreError> {
        self.connection.query_row("SELECT profile_id FROM vehicle_project_registry WHERE scope=?1 AND vin_digest=?2", params![scope_text(scope), vin_digest], |row| row.get(0)).optional().map_err(Into::into)
    }

    pub fn register_vehicle_project(&self, scope: ProfileScope, vin_digest: &str, profile_id: &str) -> Result<(), ProfileStoreError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("INSERT INTO vehicle_project_registry(scope,vin_digest,profile_id) VALUES(?1,?2,?3)", params![scope_text(scope), vin_digest, profile_id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn save_project_workspace(
        &mut self,
        profile_id: &str,
        workspace: &crate::VehicleProjectWorkspace,
    ) -> Result<(), ProfileStoreError> {
        workspace.validate().map_err(ProfileStoreError::Invalid)?;
        if self.get(profile_id)?.is_none() {
            return Err(ProfileStoreError::Invalid("Vehicle project does not exist".into()));
        }
        let json = serde_json::to_string(workspace).map_err(|error| ProfileStoreError::Encoding(error.to_string()))?;
        self.connection.execute(
            "INSERT INTO vehicle_project_workspaces(profile_id,workspace_json) VALUES(?1,?2) ON CONFLICT(profile_id) DO UPDATE SET workspace_json=excluded.workspace_json",
            params![profile_id, json],
        )?;
        Ok(())
    }

    pub fn save(
        &mut self,
        profile: VehicleProfile,
        expected_revision: Option<u64>,
    ) -> Result<VehicleProfile, ProfileStoreError> {
        self.save_with_observation(profile, expected_revision, None)
    }

    /// Save selection revisions and an observation together so a failed append cannot
    /// leave the user with half of an explicit profile update.
    pub fn save_with_observation(
        &mut self,
        profile: VehicleProfile,
        expected_revision: Option<u64>,
        observation: Option<VehicleIdentityObservation>,
    ) -> Result<VehicleProfile, ProfileStoreError> {
        self.save_with_observation_and_workspace(profile, expected_revision, observation, None)
    }

    pub fn save_with_observation_and_workspace(
        &mut self,
        mut profile: VehicleProfile,
        expected_revision: Option<u64>,
        observation: Option<VehicleIdentityObservation>,
        workspace: Option<&crate::VehicleProjectWorkspace>,
    ) -> Result<VehicleProfile, ProfileStoreError> {
        validate_profile(&profile)?;
        if let Some(workspace) = workspace { workspace.validate().map_err(ProfileStoreError::Invalid)?; }
        if let Some(observation) = &observation {
            validate_observation(observation)?;
        }
        let id = if profile.id.trim().is_empty() {
            new_id()
        } else {
            profile.id.clone()
        };
        if id.len() > 128 || id.chars().any(char::is_control) {
            return Err(ProfileStoreError::Invalid(
                "Profile ID must be at most 128 non-control characters".into(),
            ));
        }
        let transaction = self.connection.transaction()?;
        let actual = transaction
            .query_row(
                "SELECT revision FROM profiles WHERE profile_id = ?1",
                [&id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(from_sql_u64)
            .transpose()?;
        if actual != expected_revision {
            return Err(ProfileStoreError::RevisionConflict {
                expected: expected_revision,
                actual,
            });
        }
        let now = now_unix_ms();
        profile.id = id.clone();
        profile.revision = actual.unwrap_or(0).saturating_add(1);
        profile.created_at_unix_ms = if actual.is_some() {
            let saved: i64 = transaction.query_row(
                "SELECT created_at_unix_ms FROM profiles WHERE profile_id = ?1",
                [&id],
                |row| row.get(0),
            )?;
            from_sql_u64(saved)?
        } else {
            now
        };
        profile.updated_at_unix_ms = now;
        let revision_sql = to_sql_u64(profile.revision)?;
        let created_sql = to_sql_u64(profile.created_at_unix_ms)?;
        let updated_sql = to_sql_u64(profile.updated_at_unix_ms)?;
        transaction.execute(
            "INSERT INTO profiles(profile_id,name,revision,scope,catalog_vehicle_id,vin_digest,masked_vin,manual_association,created_at_unix_ms,updated_at_unix_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(profile_id) DO UPDATE SET name=excluded.name,revision=excluded.revision,scope=excluded.scope,catalog_vehicle_id=excluded.catalog_vehicle_id,vin_digest=excluded.vin_digest,masked_vin=excluded.masked_vin,manual_association=excluded.manual_association,updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![profile.id, profile.name.trim(), revision_sql, scope_text(profile.scope), profile.catalog_vehicle_id, profile.vin_digest, profile.masked_vin, profile.manual_association, created_sql, updated_sql],
        )?;
        transaction.execute("DELETE FROM ecu_variants WHERE profile_id = ?1", [&id])?;
        transaction.execute(
            "DELETE FROM selection_snapshots WHERE profile_id = ?1",
            [&id],
        )?;
        save_json_rows(&transaction, "ecu_variants", &id, &profile.ecu_variants)?;
        transaction.execute(
            "INSERT INTO selection_snapshots(profile_id,value_json) VALUES(?1,?2)",
            params![id, encode(&profile.selection)?],
        )?;
        if actual.is_none() {
            save_json_rows(
                &transaction,
                "identity_observations",
                &id,
                &profile.observations,
            )?;
        }
        if let Some(observation) = observation {
            let count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM identity_observations WHERE profile_id = ?1",
                [&id],
                |row| row.get(0),
            )?;
            if count >= 500 {
                return Err(ProfileStoreError::Invalid(
                    "A profile can retain at most 500 identity observations".into(),
                ));
            }
            transaction.execute(
                "INSERT INTO identity_observations(profile_id,ordinal,value_json) VALUES(?1,?2,?3)",
                params![id, count, encode(&observation)?],
            )?;
        }
        if let Some(workspace) = workspace {
            transaction.execute("INSERT INTO vehicle_project_workspaces(profile_id,workspace_json) VALUES(?1,?2) ON CONFLICT(profile_id) DO UPDATE SET workspace_json=excluded.workspace_json", params![id, encode(workspace)?])?;
        }
        transaction.commit()?;
        profile.observations = self.observations_for(&id)?;
        Ok(profile)
    }

    pub fn append_observation(
        &mut self,
        id: &str,
        observation: VehicleIdentityObservation,
    ) -> Result<VehicleProfile, ProfileStoreError> {
        validate_observation(&observation)?;
        let transaction = self.connection.transaction()?;
        if transaction
            .query_row("SELECT 1 FROM profiles WHERE profile_id = ?1", [id], |_| {
                Ok(())
            })
            .optional()?
            .is_none()
        {
            return Err(ProfileStoreError::Invalid(format!(
                "Vehicle profile '{id}' does not exist"
            )));
        }
        let count: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM identity_observations WHERE profile_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        if count >= 500 {
            return Err(ProfileStoreError::Invalid(
                "A profile can retain at most 500 identity observations".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO identity_observations(profile_id,ordinal,value_json) VALUES(?1,?2,?3)",
            params![id, count, encode(&observation)?],
        )?;
        transaction.commit()?;
        self.get(id)?.ok_or_else(|| {
            ProfileStoreError::Invalid(format!("Vehicle profile '{id}' disappeared"))
        })
    }

    fn observations_for(
        &self,
        id: &str,
    ) -> Result<Vec<VehicleIdentityObservation>, ProfileStoreError> {
        load_json_rows(
            &self.connection,
            "SELECT value_json FROM identity_observations WHERE profile_id = ?1 ORDER BY ordinal",
            id,
        )
    }
}

fn initialize(connection: &Connection) -> Result<(), ProfileStoreError> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "CREATE TABLE profiles(profile_id TEXT PRIMARY KEY,name TEXT NOT NULL,revision INTEGER NOT NULL,scope TEXT NOT NULL,catalog_vehicle_id TEXT,vin_digest TEXT,masked_vin TEXT,manual_association INTEGER NOT NULL,created_at_unix_ms INTEGER NOT NULL,updated_at_unix_ms INTEGER NOT NULL);
         CREATE TABLE ecu_variants(profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,value_json TEXT NOT NULL,PRIMARY KEY(profile_id,ordinal));
         CREATE TABLE identity_observations(profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,value_json TEXT NOT NULL,PRIMARY KEY(profile_id,ordinal));
         CREATE TABLE selection_snapshots(profile_id TEXT PRIMARY KEY REFERENCES profiles(profile_id) ON DELETE CASCADE,value_json TEXT NOT NULL);
         PRAGMA user_version = 1;"
    )?;
    create_v2_tables(&transaction)?;
    transaction.execute_batch("PRAGMA user_version = 2;")?;
    transaction.commit()?;
    Ok(())
}

fn migrate_v1_to_v2(connection: &Connection) -> Result<(), ProfileStoreError> {
    let transaction = connection.unchecked_transaction()?;
    create_v2_tables(&transaction)?;
    transaction.execute(
        "INSERT OR IGNORE INTO vehicle_identities(scope,vin_digest,full_vin,masked_vin,identity_state,revision,updated_at_unix_ms)
         SELECT scope,vin_digest,NULL,masked_vin,'legacy_hash_only',1,updated_at_unix_ms
         FROM profiles WHERE vin_digest IS NOT NULL",
        [],
    )?;
    transaction.execute_batch("PRAGMA user_version = 2;")?;
    transaction.commit()?;
    Ok(())
}

fn migrate_v2_to_v3(connection: &Connection) -> Result<(), ProfileStoreError> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "CREATE TABLE custom_signal_presets(
             preset_id TEXT PRIMARY KEY,
             revision INTEGER NOT NULL CHECK(revision >= 1),
             name TEXT NOT NULL,
             scope TEXT NOT NULL CHECK(scope IN ('generic','model_powertrain','exact_profile')),
             criteria_json TEXT NOT NULL,
             selection_json TEXT NOT NULL,
             preset_json TEXT NOT NULL
         );
         CREATE TABLE preset_picker_state(
             singleton INTEGER PRIMARY KEY CHECK(singleton=1),
             revision INTEGER NOT NULL CHECK(revision >= 0),
             context_json TEXT NOT NULL,
             staged_json TEXT
         );
         INSERT INTO preset_picker_state(singleton,revision,context_json,staged_json) VALUES(1,0,'{}',NULL);
         PRAGMA user_version = 3;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn migrate_v3_to_v4(connection: &Connection) -> Result<(), ProfileStoreError> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "CREATE TABLE vehicle_project_workspaces(
             profile_id TEXT PRIMARY KEY REFERENCES profiles(profile_id) ON DELETE CASCADE,
             workspace_json TEXT NOT NULL
         );
         CREATE TABLE vehicle_project_registry(
             scope TEXT NOT NULL CHECK(scope IN ('live','simulator','can_only')),
             vin_digest TEXT NOT NULL,
             profile_id TEXT NOT NULL UNIQUE REFERENCES profiles(profile_id) ON DELETE CASCADE,
             PRIMARY KEY(scope,vin_digest)
         );
         INSERT INTO vehicle_project_registry(scope,vin_digest,profile_id)
         SELECT p.scope,p.vin_digest,p.profile_id FROM profiles p
         WHERE p.vin_digest IS NOT NULL AND p.profile_id=(
             SELECT candidate.profile_id FROM profiles candidate
             WHERE candidate.scope=p.scope AND candidate.vin_digest=p.vin_digest
             ORDER BY candidate.updated_at_unix_ms DESC,candidate.profile_id DESC LIMIT 1
         );
         PRAGMA user_version = 4;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn create_v2_tables(transaction: &Transaction<'_>) -> Result<(), ProfileStoreError> {
    transaction.execute_batch(
        "CREATE TABLE vehicle_identities(
             scope TEXT NOT NULL CHECK(scope IN ('live','simulator','can_only')),
             vin_digest TEXT NOT NULL,
             full_vin TEXT,
             masked_vin TEXT,
             identity_state TEXT NOT NULL CHECK(identity_state IN ('legacy_hash_only','vin_known')),
             decoder_snapshot_json TEXT,
             revision INTEGER NOT NULL CHECK(revision >= 1),
             updated_at_unix_ms INTEGER NOT NULL CHECK(updated_at_unix_ms >= 0),
             PRIMARY KEY(scope,vin_digest)
         );
         CREATE TABLE vehicle_identity_evidence(
             scope TEXT NOT NULL,
             vin_digest TEXT NOT NULL,
             ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
             value_json TEXT NOT NULL,
             PRIMARY KEY(scope,vin_digest,ordinal),
             FOREIGN KEY(scope,vin_digest) REFERENCES vehicle_identities(scope,vin_digest) ON DELETE CASCADE
         );
         CREATE TABLE manual_vehicle_details(
             scope TEXT NOT NULL,
             vin_digest TEXT NOT NULL,
             ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
             value_json TEXT NOT NULL,
             PRIMARY KEY(scope,vin_digest,ordinal),
             FOREIGN KEY(scope,vin_digest) REFERENCES vehicle_identities(scope,vin_digest) ON DELETE CASCADE
         );
         CREATE TABLE diagnostic_history(
             report_id TEXT PRIMARY KEY,
             source_relative_path TEXT NOT NULL,
             byte_offset INTEGER NOT NULL CHECK(byte_offset >= 0),
             byte_length INTEGER NOT NULL CHECK(byte_length > 0),
             sha256 TEXT NOT NULL,
             started_at_unix_ms INTEGER NOT NULL CHECK(started_at_unix_ms >= 0),
             scope TEXT NOT NULL CHECK(scope IN ('live','simulator','can_only')),
             vin_digest TEXT,
             masked_vin TEXT,
             identity_source TEXT NOT NULL,
             vin_conflict INTEGER NOT NULL CHECK(vin_conflict IN (0,1)),
             ecu_ids_json TEXT NOT NULL,
             dtc_counts_json TEXT NOT NULL,
             mode06_read_count INTEGER NOT NULL CHECK(mode06_read_count >= 0),
             context_snapshot_json TEXT NOT NULL,
             UNIQUE(source_relative_path,byte_offset,byte_length)
         );
         CREATE INDEX diagnostic_history_scope_vin_date ON diagnostic_history(scope,vin_digest,started_at_unix_ms DESC,report_id DESC);
         CREATE INDEX diagnostic_history_date ON diagnostic_history(started_at_unix_ms DESC,report_id DESC);
         CREATE TABLE diagnostic_history_ecus(
             report_id TEXT NOT NULL REFERENCES diagnostic_history(report_id) ON DELETE CASCADE,
             ecu_id TEXT NOT NULL,
             started_at_unix_ms INTEGER NOT NULL CHECK(started_at_unix_ms >= 0),
             scope TEXT NOT NULL CHECK(scope IN ('live','simulator','can_only')),
             vin_digest TEXT,
             PRIMARY KEY(report_id,ecu_id)
         );
         CREATE INDEX diagnostic_history_ecu_date ON diagnostic_history_ecus(ecu_id,started_at_unix_ms DESC,report_id DESC);
         CREATE INDEX diagnostic_history_ecu_scope_vin ON diagnostic_history_ecus(scope,vin_digest,ecu_id,report_id);
         CREATE TABLE manual_report_associations(
             association_id INTEGER PRIMARY KEY AUTOINCREMENT,
             report_id TEXT NOT NULL REFERENCES diagnostic_history(report_id) ON DELETE CASCADE,
             scope TEXT NOT NULL CHECK(scope IN ('live','simulator','can_only')),
             vin_digest TEXT NOT NULL,
             actor TEXT NOT NULL CHECK(actor IN ('owner_ui','agent_access')),
             associated_at_unix_ms INTEGER NOT NULL CHECK(associated_at_unix_ms >= 0),
             note TEXT
         );
         CREATE INDEX manual_report_associations_scoped_vin ON manual_report_associations(scope,vin_digest,report_id);
         CREATE TABLE history_import_state(
             singleton INTEGER PRIMARY KEY CHECK(singleton=1),
             source_relative_path TEXT,
             state TEXT NOT NULL DEFAULT 'idle',
             indexed_reports INTEGER NOT NULL DEFAULT 0 CHECK(indexed_reports >= 0),
             scanned_bytes INTEGER NOT NULL DEFAULT 0 CHECK(scanned_bytes >= 0),
             total_bytes INTEGER NOT NULL DEFAULT 0 CHECK(total_bytes >= 0),
             malformed_lines INTEGER NOT NULL DEFAULT 0 CHECK(malformed_lines >= 0),
             oversized_lines INTEGER NOT NULL DEFAULT 0 CHECK(oversized_lines >= 0),
             next_byte_offset INTEGER NOT NULL DEFAULT 0 CHECK(next_byte_offset >= 0),
             trailing_partial_line BLOB,
             error TEXT,
             updated_at_unix_ms INTEGER NOT NULL DEFAULT 0 CHECK(updated_at_unix_ms >= 0)
         );
         INSERT INTO history_import_state(singleton) VALUES(1);",
    )?;
    Ok(())
}

fn validate_profile(profile: &VehicleProfile) -> Result<(), ProfileStoreError> {
    if profile.name.trim().is_empty() || profile.name.chars().count() > MAX_PROFILE_NAME_CHARS {
        return Err(ProfileStoreError::Invalid(format!(
            "Profile name must contain 1-{MAX_PROFILE_NAME_CHARS} characters"
        )));
    }
    if profile.ecu_variants.len() > 64 || profile.observations.len() > 500 {
        return Err(ProfileStoreError::Invalid(
            "Profile ECU and observation history limits were exceeded".into(),
        ));
    }
    validate_vin_fields(profile.vin_digest.as_deref(), profile.masked_vin.as_deref())?;
    if profile.catalog_vehicle_id.as_ref().is_some_and(|id| {
        id.trim().is_empty() || id.len() > 128 || id.chars().any(char::is_control)
    }) {
        return Err(ProfileStoreError::Invalid(
            "Catalog vehicle association must be a valid explicit ID".into(),
        ));
    }
    let mut ecu_routes = std::collections::HashSet::new();
    if profile.ecu_variants.iter().any(|ecu| {
        !ecu_routes.insert((
            ecu.protocol.as_str(),
            ecu.request_route.as_str(),
            ecu.request_address.as_deref(),
            ecu.response_address.as_str(),
        ))
    }) {
        return Err(ProfileStoreError::Invalid(
            "Profile ECU route identities must be unique".into(),
        ));
    }
    for ecu in &profile.ecu_variants {
        validate_ecu(ecu)?;
    }
    for observation in &profile.observations {
        validate_observation(observation)?;
    }
    validate_selection(&profile.selection)
}

fn validate_selection(selection: &ProfileSelectionSnapshot) -> Result<(), ProfileStoreError> {
    for (label, refs) in [
        ("log", &selection.log_signal_ids),
        ("graph", &selection.graph_signal_ids),
    ] {
        if refs.len() > MAX_PROFILE_SELECTION_REFS {
            return Err(ProfileStoreError::Invalid(format!(
                "A profile can save at most {MAX_PROFILE_SELECTION_REFS} {label} references"
            )));
        }
        let mut seen = std::collections::HashSet::new();
        if refs.iter().any(|id| {
            id.trim().is_empty()
                || id.len() > 256
                || id.chars().any(char::is_control)
                || !seen.insert(id)
        }) {
            return Err(ProfileStoreError::Invalid(format!(
                "Profile {label} references must be nonempty, unique, and at most 256 bytes"
            )));
        }
    }
    if selection.derived_definitions.len() > 100
        || selection.can_fields.len() > MAX_PROFILE_SELECTION_REFS
        || selection.oem_references.len() > MAX_PROFILE_SELECTION_REFS
    {
        return Err(ProfileStoreError::Invalid(
            "Profile definition limit was exceeded".into(),
        ));
    }
    match (&selection.can_source_id, selection.can_bitrate) {
        (None, None) => {}
        (Some(source), Some(bitrate))
            if !source.trim().is_empty()
                && source.len() <= 256
                && !source.chars().any(char::is_control)
                && (10_000..=1_000_000).contains(&bitrate) => {}
        _ => return Err(ProfileStoreError::Invalid(
            "CAN profile context requires a source ID and a bitrate from 10000 to 1000000 bit/s"
                .into(),
        )),
    }
    crate::derived::compile_derived_signals(
        &selection.derived_definitions,
        &standard_obd_signals(),
    )
    .map_err(|error| ProfileStoreError::Invalid(error.to_string()))?;
    let mut can_ids = std::collections::HashSet::new();
    for field in &selection.can_fields {
        if field.id.trim().is_empty()
            || field.name.trim().is_empty()
            || !can_ids.insert(field.id.as_str())
        {
            return Err(ProfileStoreError::Invalid(
                "CAN profile fields require unique IDs and names".into(),
            ));
        }
        field
            .validate()
            .map_err(|error| ProfileStoreError::Invalid(error.to_string()))?;
    }
    let mut oem = std::collections::HashSet::new();
    if selection.oem_references.iter().any(|reference| {
        reference.candidate_id.trim().is_empty()
            || reference.source_id.trim().is_empty()
            || reference.source_revision.trim().is_empty()
            || reference.decoder_revision.trim().is_empty()
            || !oem.insert(reference.candidate_id.as_str())
    }) {
        return Err(ProfileStoreError::Invalid("OEM profile references must be unique and retain candidate, source, and decoder revisions".into()));
    }
    Ok(())
}

fn validate_observation(observation: &VehicleIdentityObservation) -> Result<(), ProfileStoreError> {
    if observation.ecu_variants.len() > 64 {
        return Err(ProfileStoreError::Invalid(
            "An observation can include at most 64 ECU variants".into(),
        ));
    }
    validate_vin_fields(
        observation.vin_digest.as_deref(),
        observation.masked_vin.as_deref(),
    )?;
    if observation.conflicting_vin_observations && observation.vin_digest.is_some() {
        return Err(ProfileStoreError::Invalid(
            "A conflicting VIN observation cannot have a single VIN digest".into(),
        ));
    }
    for ecu in &observation.ecu_variants {
        validate_ecu(ecu)?;
    }
    Ok(())
}

fn validate_ecu(ecu: &EcuProfileVariant) -> Result<(), ProfileStoreError> {
    if ecu.protocol.trim().is_empty()
        || ecu.request_route.trim().is_empty()
        || ecu.response_address.trim().is_empty()
    {
        return Err(ProfileStoreError::Invalid(
            "ECU profiles require protocol, request route, and response address".into(),
        ));
    }
    if ecu.calibration_ids.len() > 32
        || ecu.software_ids.len() > 32
        || ecu
            .calibration_ids
            .iter()
            .chain(&ecu.software_ids)
            .any(|id| id.trim().is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err(ProfileStoreError::Invalid(
            "ECU identity values must be nonempty and limited to 32 values of 128 bytes".into(),
        ));
    }
    Ok(())
}

fn validate_vin_fields(
    digest: Option<&str>,
    masked: Option<&str>,
) -> Result<(), ProfileStoreError> {
    match (digest, masked) {
        (None, None) => Ok(()),
        (Some(digest), Some(masked)) => {
            let valid_digest = digest.strip_prefix("vin:v1:sha256:").is_some_and(|hex| {
                hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
            let chars = masked.chars().collect::<Vec<_>>();
            let valid_mask = chars.len() == 17
                && chars[..13].iter().all(|character| *character == '•')
                && chars[13..].iter().all(|character| {
                    character.is_ascii_alphanumeric()
                        && !matches!(character.to_ascii_uppercase(), 'I' | 'O' | 'Q')
                });
            if valid_digest && valid_mask {
                Ok(())
            } else {
                Err(ProfileStoreError::Invalid("VIN digest and masked VIN must use the versioned digest format and a 13-character mask".into()))
            }
        }
        _ => Err(ProfileStoreError::Invalid(
            "VIN digest and masked VIN must be stored together".into(),
        )),
    }
}

fn load_json_rows<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    id: &str,
) -> Result<Vec<T>, ProfileStoreError> {
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map([id], |row| row.get::<_, String>(0))?
        .map(|value| decode(&value?))
        .collect()
}

fn save_json_rows<T: serde::Serialize>(
    transaction: &Transaction<'_>,
    table: &str,
    id: &str,
    values: &[T],
) -> Result<(), ProfileStoreError> {
    let sql = format!("INSERT INTO {table}(profile_id,ordinal,value_json) VALUES(?1,?2,?3)");
    let mut statement = transaction.prepare(&sql)?;
    for (ordinal, value) in values.iter().enumerate() {
        statement.execute(params![id, ordinal as i64, encode(value)?])?;
    }
    Ok(())
}

fn encode<T: serde::Serialize>(value: &T) -> Result<String, ProfileStoreError> {
    serde_json::to_string(value).map_err(|error| ProfileStoreError::Encoding(error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, ProfileStoreError> {
    serde_json::from_str(json).map_err(|error| {
        ProfileStoreError::Encoding(format!("Stored profile data is invalid: {error}"))
    })
}

fn parse_scope(value: &str) -> Result<ProfileScope, ProfileStoreError> {
    match value {
        "live" => Ok(ProfileScope::Live),
        "simulator" => Ok(ProfileScope::Simulator),
        "can_only" => Ok(ProfileScope::CanOnly),
        _ => Err(ProfileStoreError::Invalid(
            "Stored profile has an unknown connection scope".into(),
        )),
    }
}

fn scope_text(value: ProfileScope) -> &'static str {
    match value {
        ProfileScope::Live => "live",
        ProfileScope::Simulator => "simulator",
        ProfileScope::CanOnly => "can_only",
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn from_sql_u64(value: i64) -> Result<u64, ProfileStoreError> {
    u64::try_from(value).map_err(|_| {
        ProfileStoreError::Invalid("Stored profile counter or timestamp is negative".into())
    })
}

fn to_sql_u64(value: u64) -> Result<i64, ProfileStoreError> {
    i64::try_from(value).map_err(|_| {
        ProfileStoreError::Invalid(
            "Profile counter or timestamp exceeds SQLite's integer range".into(),
        )
    })
}

fn new_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "profile-{}-{}-{}",
        std::process::id(),
        now_unix_ms(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
