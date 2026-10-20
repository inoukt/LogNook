import sqlite3


SUBARU_DDL = """
CREATE TABLE IF NOT EXISTS subaru_source_releases (
    source_id TEXT PRIMARY KEY,
    source_display_name TEXT NOT NULL,
    source_revision TEXT NOT NULL,
    catalog_sha256 TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_url TEXT,
    license_status TEXT NOT NULL CHECK (license_status = 'not_cleared'),
    source_metadata_json TEXT NOT NULL,
    imported_at_utc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS subaru_vehicle_specs (
    vehicle_key TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES subaru_source_releases(source_id),
    source_vehicle_id TEXT NOT NULL,
    source_ordinal INTEGER NOT NULL,
    display_label TEXT NOT NULL,
    model_group TEXT NOT NULL,
    models_json TEXT NOT NULL,
    model_year INTEGER,
    year_from INTEGER,
    year_to INTEGER,
    year_range_raw_json TEXT,
    year_range_state TEXT NOT NULL,
    variant TEXT NOT NULL DEFAULT '',
    is_pseudo INTEGER NOT NULL CHECK (is_pseudo IN (0,1)),
    raw_json TEXT NOT NULL,
    UNIQUE(source_id, source_ordinal)
);
CREATE TABLE IF NOT EXISTS subaru_ecu_definitions (
    ecu_key TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES subaru_source_releases(source_id),
    source_ecu_id TEXT NOT NULL,
    source_ordinal INTEGER NOT NULL,
    ecu_name TEXT NOT NULL,
    system_short TEXT NOT NULL DEFAULT '',
    component_id TEXT NOT NULL DEFAULT '',
    protocol_family TEXT NOT NULL,
    data_rate INTEGER,
    request_header TEXT,
    response_header TEXT,
    functional_header TEXT,
    n_ta TEXT,
    monitor_table TEXT,
    alias_of TEXT NOT NULL DEFAULT '',
    raw_metadata_json TEXT NOT NULL,
    UNIQUE(source_id, source_ordinal)
);
CREATE TABLE IF NOT EXISTS subaru_vehicle_ecu_links (
    vehicle_key TEXT NOT NULL REFERENCES subaru_vehicle_specs(vehicle_key),
    ecu_key TEXT NOT NULL REFERENCES subaru_ecu_definitions(ecu_key),
    system_id TEXT NOT NULL DEFAULT '',
    system_name TEXT NOT NULL DEFAULT '',
    source_ordinal INTEGER NOT NULL,
    route_metadata_json TEXT NOT NULL,
    PRIMARY KEY(vehicle_key, ecu_key)
);
CREATE TABLE IF NOT EXISTS subaru_request_hints (
    request_hint_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES subaru_source_releases(source_id),
    ecu_key TEXT NOT NULL REFERENCES subaru_ecu_definitions(ecu_key),
    protocol_family TEXT NOT NULL,
    hint_text TEXT NOT NULL,
    hint_service TEXT,
    hint_identifier TEXT,
    mapping_state TEXT NOT NULL DEFAULT 'inferred' CHECK (mapping_state = 'inferred'),
    auto_request INTEGER NOT NULL DEFAULT 0 CHECK (auto_request = 0),
    UNIQUE(source_id, ecu_key, protocol_family, hint_text)
);
CREATE TABLE IF NOT EXISTS subaru_signal_definitions (
    definition_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES subaru_source_releases(source_id),
    source_table TEXT NOT NULL,
    source_row_ordinal INTEGER NOT NULL,
    raw_row_json TEXT NOT NULL,
    UNIQUE(source_id, source_table, source_row_ordinal)
);
CREATE TABLE IF NOT EXISTS subaru_signal_ecu_links (
    candidate_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES subaru_source_releases(source_id),
    definition_id TEXT NOT NULL REFERENCES subaru_signal_definitions(definition_id),
    ecu_key TEXT NOT NULL REFERENCES subaru_ecu_definitions(ecu_key),
    request_hint_id TEXT REFERENCES subaru_request_hints(request_hint_id),
    source_row_ordinal INTEGER NOT NULL,
    source_table TEXT NOT NULL,
    alias_of TEXT NOT NULL DEFAULT '',
    link_metadata_json TEXT NOT NULL,
    auto_request INTEGER NOT NULL DEFAULT 0 CHECK (auto_request = 0),
    UNIQUE(source_id, definition_id, ecu_key)
);
CREATE TABLE IF NOT EXISTS subaru_orphan_tables (
    source_id TEXT NOT NULL REFERENCES subaru_source_releases(source_id),
    source_table TEXT NOT NULL,
    row_count INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'unlinked_to_vehicle',
    PRIMARY KEY(source_id, source_table)
);
CREATE INDEX IF NOT EXISTS idx_subaru_def_name ON subaru_signal_definitions(json_extract(raw_row_json,'$.name'));
CREATE INDEX IF NOT EXISTS idx_subaru_def_did ON subaru_signal_definitions(json_extract(raw_row_json,'$.did'),json_extract(raw_row_json,'$.pid_id'));
CREATE INDEX IF NOT EXISTS idx_subaru_ecu_protocol ON subaru_ecu_definitions(protocol_family, source_ecu_id);
CREATE INDEX IF NOT EXISTS idx_subaru_link_ecu ON subaru_signal_ecu_links(ecu_key, definition_id);
CREATE INDEX IF NOT EXISTS idx_subaru_vehicle_model_year ON subaru_vehicle_specs(model_group, model_year);
CREATE INDEX IF NOT EXISTS idx_subaru_vehicle_ecu_ecu ON subaru_vehicle_ecu_links(ecu_key, vehicle_key);
CREATE INDEX IF NOT EXISTS idx_subaru_hint_ecu ON subaru_request_hints(ecu_key, hint_service, hint_identifier);
DROP VIEW IF EXISTS subaru_catalog_search;
CREATE VIEW subaru_catalog_search AS
SELECT
    l.candidate_id,
    d.definition_id,
    s.source_id,
    s.source_display_name,
    s.source_revision,
    s.catalog_sha256,
    d.source_table,
    d.source_row_ordinal,
    l.ecu_key,
    e.source_ecu_id AS ecu_id,
    e.ecu_name,
    e.protocol_family,
    e.data_rate,
    e.request_header,
    e.response_header,
    e.functional_header,
    e.n_ta,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.did') AS TEXT),'') AS source_did,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.pid_id') AS TEXT),'') AS source_pid_id,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.signal_id') AS TEXT),'') AS signal_id,
    h.hint_text AS request_hint,
    h.hint_service,
    h.hint_identifier,
    COALESCE(h.mapping_state, 'missing') AS mapping_state,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.name') AS TEXT),'') AS name,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.unit') AS TEXT),'') AS unit,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.unit_id') AS TEXT),'') AS unit_id,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.conv_method') AS TEXT),'') AS conv_method,
    json_extract(d.raw_row_json,'$.did_bytes') AS did_bytes,
    json_extract(d.raw_row_json,'$.byte_length') AS byte_length,
    json_extract(d.raw_row_json,'$.byte_pos') AS byte_pos,
    json_extract(d.raw_row_json,'$.bit_pos') AS bit_pos,
    json_extract(d.raw_row_json,'$.bit_size') AS bit_size,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.bit_mask') AS TEXT),'') AS bit_mask,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.endian') AS TEXT),'') AS endian,
    json_extract(d.raw_row_json,'$.signed') AS signed,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.lsb_mul') AS TEXT),'') AS lsb_mul,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.lsb_div') AS TEXT),'') AS lsb_div,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.offset') AS TEXT),'') AS offset,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.significant') AS TEXT),'') AS significant,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.factor') AS TEXT),'') AS factor,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.offset_scaled') AS TEXT),'') AS offset_scaled,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.expression') AS TEXT),'') AS expression,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.min') AS TEXT),'') AS min,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.max') AS TEXT),'') AS max,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.genre') AS TEXT),'') AS genre,
    COALESCE(CAST(json_extract(d.raw_row_json,'$.enums') AS TEXT),'[]') AS enums_json,
    d.raw_row_json,
    d.raw_row_json AS decoder_payload_json,
    l.link_metadata_json,
    'review_required' AS research_state,
    'not_tested' AS vehicle_validation,
    'REQUIRES_INDIVIDUAL_ECU_VERIFICATION' AS ecu_filter,
    0 AS auto_request,
    COALESCE((
        SELECT json_group_array(json_object(
            'vehicle_key',scope.vehicle_key,
            'display_label',scope.display_label,
            'model_group',scope.model_group,
            'models',json(scope.models_json),
            'model_year',scope.model_year,
            'year_from',scope.year_from,
            'year_to',scope.year_to,
            'year_range_state',scope.year_range_state,
            'variant',scope.variant,
            'is_pseudo',scope.is_pseudo,
            'system_id',scope.system_id,
            'system_name',scope.system_name
        ))
        FROM (
            SELECT v.vehicle_key,v.display_label,v.model_group,v.models_json,v.model_year,
                   v.year_from,v.year_to,v.year_range_state,v.variant,v.is_pseudo,
                   vl.system_id,vl.system_name
            FROM subaru_vehicle_ecu_links vl
            JOIN subaru_vehicle_specs v ON v.vehicle_key=vl.vehicle_key
            WHERE vl.ecu_key=e.ecu_key
            ORDER BY v.model_year,v.display_label,vl.system_id
        ) AS scope
    ), '[]') AS vehicle_scope_json
FROM subaru_signal_ecu_links l
JOIN subaru_signal_definitions d ON d.definition_id=l.definition_id
JOIN subaru_ecu_definitions e ON e.ecu_key=l.ecu_key
JOIN subaru_source_releases s ON s.source_id=l.source_id
LEFT JOIN subaru_request_hints h ON h.request_hint_id=l.request_hint_id;
"""


def ensure_subaru_schema(connection: sqlite3.Connection) -> None:
    connection.execute("PRAGMA foreign_keys=ON")
    connection.executescript(SUBARU_DDL)
    connection.execute("CREATE TABLE IF NOT EXISTS db_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
    version = connection.execute("SELECT value FROM db_meta WHERE key='schema_version'").fetchone()
    if version and version[0] != "3":
        raise RuntimeError(f"Subaru import requires base schema version 3, got {version[0]}")
    connection.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('subaru_catalog_schema_version','1')")
    connection.commit()
