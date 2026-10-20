use crate::{OemCandidate, OemCandidateSourcePolicy, OemCandidateTrySpec, OemPollPlan};
use rusqlite::{Connection, OpenFlags, Row, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
};

const MAX_PAGE_SIZE: usize = 100;
const MAX_OFFSET: usize = 1_000_000;
const MAX_LOG_SIGNALS: usize = 64;
const SUBARU_LOG_VEHICLE_ID: &str = "subaru-local-research";

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
pub struct SubaruCatalogPage {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub rows: Vec<SubaruCatalogRow>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
pub struct SubaruCatalogRow {
    pub id: String,
    pub definition_id: String,
    pub source_id: String,
    pub source_display_name: String,
    pub source_revision: String,
    pub catalog_sha256: String,
    pub source_table: String,
    pub source_did: String,
    pub ecu_key: String,
    pub ecu_id: String,
    pub ecu_name: String,
    pub protocol: String,
    pub data_rate: Option<i64>,
    pub request_header: String,
    pub response_header: String,
    pub n_ta: Option<String>,
    pub request_hint: String,
    pub request_service: String,
    pub request_identifier: String,
    pub mapping_state: String,
    pub name: String,
    pub unit: String,
    pub vehicle_scope_json: String,
    pub vehicle_validation: String,
    pub auto_request: bool,
    pub can_manual_read: bool,
    pub manual_read_reason: String,
    pub decoder_supported: bool,
    pub can_graph: bool,
    pub graph_reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
pub struct SubaruCatalogDetail {
    pub row: SubaruCatalogRow,
    pub source_did: String,
    pub signal_id: String,
    pub source_pid_id: String,
    pub decoder_json: String,
    pub raw_source_json: String,
    pub link_metadata_json: String,
}

pub struct SubaruCatalog {
    path: std::path::PathBuf,
}

impl SubaruCatalog {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_owned(),
        }
    }

    pub fn search(
        &self,
        query: &str,
        model: &str,
        year: Option<i32>,
        ecu_id: &str,
        protocol_family: &str,
        offset: usize,
        limit: usize,
    ) -> Result<SubaruCatalogPage, String> {
        if offset > MAX_OFFSET {
            return Err(format!("Subaru catalog offset exceeds {MAX_OFFSET}"));
        }
        let limit = limit.clamp(1, MAX_PAGE_SIZE);
        let connection = open_read_only(&self.path)?;
        let query = like_filter(query);
        let model = like_filter(model);
        let ecu_id = like_filter(ecu_id);
        let protocol_family = like_filter(protocol_family);
        let total = connection
            .query_row(
                SEARCH_COUNT_SQL,
                params![query, model, year, ecu_id, protocol_family],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;
        let total = usize::try_from(total).map_err(|error| error.to_string())?;
        let mut statement = connection
            .prepare(SEARCH_PAGE_SQL)
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(
                params![
                    query,
                    model,
                    year,
                    ecu_id,
                    protocol_family,
                    limit as i64,
                    offset as i64,
                ],
                read_summary,
            )
            .map_err(|error| error.to_string())?;
        let rows = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(SubaruCatalogPage {
            total,
            offset,
            limit,
            rows,
        })
    }

    pub fn get(&self, candidate_id: &str) -> Result<SubaruCatalogDetail, String> {
        let candidate_id = candidate_id.trim();
        if candidate_id.is_empty() || candidate_id.len() > 128 {
            return Err("Choose a valid Subaru catalog row".into());
        }
        let connection = open_read_only(&self.path)?;
        let row = connection
            .query_row(DETAIL_SQL, [candidate_id], |row| {
                Ok(SubaruCatalogDetail {
                    row: read_summary(row)?,
                    source_did: row.get(29)?,
                    signal_id: row.get(30)?,
                    source_pid_id: row.get(31)?,
                    decoder_json: row.get(32)?,
                    raw_source_json: row.get(33)?,
                    link_metadata_json: row.get(34)?,
                })
            })
            .map_err(|error| error.to_string())?;
        Ok(row)
    }

    /// Build an explicit operator-selected graph plan from local source rows.
    /// These candidates remain inert in the catalog; only this manual selection starts polling.
    pub fn logging_plan(&self, candidate_ids: &[String]) -> Result<OemPollPlan, String> {
        if candidate_ids.is_empty() || candidate_ids.len() > MAX_LOG_SIGNALS {
            return Err(format!(
                "Choose between 1 and {MAX_LOG_SIGNALS} Subaru graph signals"
            ));
        }
        let mut unique = HashSet::new();
        let mut groups: Vec<Vec<OemCandidateTrySpec>> = Vec::new();
        let mut group_keys: Vec<(String, String, String, String)> = Vec::new();
        for candidate_id in candidate_ids {
            if !unique.insert(candidate_id.as_str()) {
                return Err("A Subaru graph signal was selected more than once".into());
            }
            let detail = self.get(candidate_id)?;
            if !detail.row.can_graph {
                return Err(format!(
                    "'{}' cannot be decoded for live graphs: {}",
                    detail.row.name, detail.row.graph_reason
                ));
            }
            let spec = logging_spec(&detail)?;
            let candidate = &spec.candidate;
            let key = if candidate.service == "SSM-KWP" {
                (
                    candidate.request_header.to_ascii_uppercase(),
                    candidate.response_header.to_ascii_uppercase(),
                    candidate.service.to_ascii_uppercase(),
                    String::new(),
                )
            } else {
                (
                    candidate.request_header.to_ascii_uppercase(),
                    candidate.response_header.to_ascii_uppercase(),
                    candidate.service.to_ascii_uppercase(),
                    candidate.identifier.to_ascii_uppercase(),
                )
            };
            if let Some(index) = group_keys.iter().position(|existing| existing == &key) {
                groups[index].push(spec);
            } else {
                group_keys.push(key);
                groups.push(vec![spec]);
            }
        }
        for group in &mut groups {
            if group
                .first()
                .is_some_and(|spec| spec.candidate.service == "SSM-KWP")
            {
                layout_subaru_ssm_group(group)?;
            }
        }
        Ok(OemPollPlan {
            vehicle_id: SUBARU_LOG_VEHICLE_ID.into(),
            groups,
        })
    }
}

const SEARCH_COUNT_SQL: &str = concat!(
    "SELECT COUNT(*) FROM subaru_catalog_search c WHERE ",
    "(?1='%%' OR c.name LIKE ?1 OR c.source_did LIKE ?1 OR c.hint_identifier LIKE ?1 ",
    "OR c.signal_id LIKE ?1 OR c.source_pid_id LIKE ?1 ",
    "OR c.request_hint LIKE ?1 OR c.ecu_id LIKE ?1 OR c.ecu_name LIKE ?1 ",
    "OR c.protocol_family LIKE ?1 OR c.source_table LIKE ?1 ",
    "OR EXISTS (SELECT 1 FROM subaru_vehicle_ecu_links l JOIN subaru_vehicle_specs v ",
    "ON v.vehicle_key=l.vehicle_key WHERE l.ecu_key=c.ecu_key AND v.source_id=c.source_id ",
    "AND v.is_pseudo=0 AND (v.display_label LIKE ?1 OR v.model_group LIKE ?1 ",
    "OR v.models_json LIKE ?1 OR CAST(v.model_year AS TEXT) LIKE ?1))) ",
    "AND (?4='%%' OR c.ecu_id LIKE ?4 OR c.ecu_name LIKE ?4) ",
    "AND (?5='%%' OR c.protocol_family LIKE ?5)",
    " AND ((?2='%%' AND ?3 IS NULL) OR EXISTS (",
    "SELECT 1 FROM subaru_vehicle_ecu_links l ",
    "JOIN subaru_vehicle_specs v ON v.vehicle_key=l.vehicle_key ",
    "WHERE l.ecu_key=c.ecu_key AND v.source_id=c.source_id AND v.is_pseudo=0 ",
    "AND (?2='%%' OR v.display_label LIKE ?2 OR v.model_group LIKE ?2 OR v.models_json LIKE ?2) ",
    "AND (?3 IS NULL OR v.model_year=?3 OR (v.year_range_state='source_range_unverified' ",
    "AND v.year_from<=?3 AND v.year_to>=?3))))"
);

const SEARCH_PAGE_SQL: &str = concat!(
    "SELECT c.candidate_id,c.definition_id,c.source_id,c.source_display_name,c.source_revision,",
    "c.catalog_sha256,c.source_table,c.ecu_key,c.ecu_id,c.ecu_name,c.protocol_family,c.data_rate,",
    "c.request_header,c.response_header,c.n_ta,COALESCE(c.request_hint,''),",
    "CAST(COALESCE(c.hint_service,'') AS TEXT),CAST(COALESCE(c.hint_identifier,'') AS TEXT),",
    "c.mapping_state,CAST(COALESCE(c.name,'') AS TEXT),CAST(COALESCE(c.unit,'') AS TEXT),",
    "CAST(COALESCE(c.conv_method,'') AS TEXT),c.bit_size,CAST(COALESCE(c.expression,'') AS TEXT),c.enums_json,",
    "c.vehicle_validation,c.auto_request,c.vehicle_scope_json,c.decoder_payload_json,COALESCE(c.source_did,''),",
    "COALESCE(c.signal_id,''),COALESCE(c.source_pid_id,'') ",
    "FROM subaru_catalog_search c WHERE ",
    "(?1='%%' OR c.name LIKE ?1 OR c.source_did LIKE ?1 OR c.hint_identifier LIKE ?1 ",
    "OR c.signal_id LIKE ?1 OR c.source_pid_id LIKE ?1 ",
    "OR c.request_hint LIKE ?1 OR c.ecu_id LIKE ?1 OR c.ecu_name LIKE ?1 ",
    "OR c.protocol_family LIKE ?1 OR c.source_table LIKE ?1 ",
    "OR EXISTS (SELECT 1 FROM subaru_vehicle_ecu_links l JOIN subaru_vehicle_specs v ",
    "ON v.vehicle_key=l.vehicle_key WHERE l.ecu_key=c.ecu_key AND v.source_id=c.source_id ",
    "AND v.is_pseudo=0 AND (v.display_label LIKE ?1 OR v.model_group LIKE ?1 ",
    "OR v.models_json LIKE ?1 OR CAST(v.model_year AS TEXT) LIKE ?1))) ",
    "AND (?4='%%' OR c.ecu_id LIKE ?4 OR c.ecu_name LIKE ?4) ",
    "AND (?5='%%' OR c.protocol_family LIKE ?5)",
    " AND ((?2='%%' AND ?3 IS NULL) OR EXISTS (",
    "SELECT 1 FROM subaru_vehicle_ecu_links l ",
    "JOIN subaru_vehicle_specs v ON v.vehicle_key=l.vehicle_key ",
    "WHERE l.ecu_key=c.ecu_key AND v.source_id=c.source_id AND v.is_pseudo=0 ",
    "AND (?2='%%' OR v.display_label LIKE ?2 OR v.model_group LIKE ?2 OR v.models_json LIKE ?2) ",
    "AND (?3 IS NULL OR v.model_year=?3 OR (v.year_range_state='source_range_unverified' ",
    "AND v.year_from<=?3 AND v.year_to>=?3))))",
    " ORDER BY (TRIM(COALESCE(c.name,''))=''),c.name,c.ecu_id,c.protocol_family,c.hint_service,c.hint_identifier,c.candidate_id ",
    "LIMIT ?6 OFFSET ?7"
);

const DETAIL_SQL: &str = concat!(
    "SELECT c.candidate_id,c.definition_id,c.source_id,c.source_display_name,c.source_revision,",
    "c.catalog_sha256,c.source_table,c.ecu_key,c.ecu_id,c.ecu_name,c.protocol_family,c.data_rate,",
    "c.request_header,c.response_header,c.n_ta,COALESCE(c.request_hint,''),",
    "CAST(COALESCE(c.hint_service,'') AS TEXT),CAST(COALESCE(c.hint_identifier,'') AS TEXT),",
    "c.mapping_state,CAST(COALESCE(c.name,'') AS TEXT),CAST(COALESCE(c.unit,'') AS TEXT),",
    "CAST(COALESCE(c.conv_method,'') AS TEXT),c.bit_size,CAST(COALESCE(c.expression,'') AS TEXT),c.enums_json,",
    "c.vehicle_validation,c.auto_request,c.vehicle_scope_json,c.decoder_payload_json,COALESCE(c.source_did,''),",
    "COALESCE(c.signal_id,''),COALESCE(c.source_pid_id,''),",
    "json_object('conv_method',c.conv_method,'bit_size',c.bit_size,'byte_length',c.byte_length,",
    "'byte_pos',c.byte_pos,'bit_pos',c.bit_pos,'endian',c.endian,'signed',c.signed,",
    "'lsb_mul',c.lsb_mul,'lsb_div',c.lsb_div,'offset',c.offset,'significant',c.significant,",
    "'factor',c.factor,'offset_scaled',c.offset_scaled,'expression',c.expression,",
    "'min',c.min,'max',c.max,'enums',json(c.enums_json)),c.raw_row_json,c.link_metadata_json ",
    "FROM subaru_catalog_search c WHERE c.candidate_id=?1"
);

fn read_summary(row: &Row<'_>) -> rusqlite::Result<SubaruCatalogRow> {
    let protocol: String = row.get(10)?;
    let request_header: String = row.get::<_, Option<String>>(12)?.unwrap_or_default();
    let response_header: String = row.get::<_, Option<String>>(13)?.unwrap_or_default();
    let n_ta: Option<String> = row.get(14)?;
    let request_service = optional_text(row, 16)?;
    let request_identifier = optional_text(row, 17)?;
    let conv_method: String = row.get(21)?;
    let bit_size: Option<u32> = row.get(22)?;
    let expression: String = row.get(23)?;
    let enums_json: String = row.get(24)?;
    let manual_read_reason = manual_can_read_reason(
        &protocol,
        &request_header,
        &response_header,
        n_ta.as_deref(),
        &request_service,
        &request_identifier,
    )
    .unwrap_or_default()
    .to_owned();
    let decoder_payload: String = row.get(28)?;
    let source_did: String = row.get(29)?;
    let name = [
        optional_text(row, 19)?,
        optional_text(row, 30)?,
        optional_text(row, 31)?,
    ]
    .into_iter()
    .find(|value| !value.trim().is_empty())
    .map(|value| value.trim().to_owned())
    .unwrap_or_default();
    let graph_reason = subaru_graph_read_reason(
        &protocol,
        row.get(11)?,
        &request_header,
        &response_header,
        n_ta.as_deref(),
        &request_service,
        &request_identifier,
        &source_did,
        &decoder_payload,
    );
    Ok(SubaruCatalogRow {
        id: row.get(0)?,
        definition_id: row.get(1)?,
        source_id: row.get(2)?,
        source_display_name: row.get(3)?,
        source_revision: row.get(4)?,
        catalog_sha256: row.get(5)?,
        source_table: row.get(6)?,
        source_did,
        ecu_key: row.get(7)?,
        ecu_id: row.get(8)?,
        ecu_name: row.get(9)?,
        protocol,
        data_rate: row.get(11)?,
        request_header,
        response_header,
        n_ta,
        request_hint: optional_text(row, 15)?,
        request_service,
        request_identifier,
        mapping_state: row.get(18)?,
        name,
        unit: row.get(20)?,
        vehicle_scope_json: row.get(27)?,
        vehicle_validation: row.get(25)?,
        auto_request: row.get::<_, i64>(26)? != 0,
        can_manual_read: manual_read_reason.is_empty(),
        manual_read_reason,
        decoder_supported: decoder_supported(&conv_method, bit_size, &expression, &enums_json),
        can_graph: graph_reason.is_empty(),
        graph_reason,
    })
}

fn subaru_graph_read_reason(
    protocol: &str,
    data_rate: Option<i64>,
    request_header: &str,
    response_header: &str,
    n_ta: Option<&str>,
    service: &str,
    identifier: &str,
    source_did: &str,
    decoder_json: &str,
) -> String {
    if protocol.trim().eq_ignore_ascii_case("SSM-KWP") {
        if data_rate != Some(4_800) {
            return "SSM-KWP graph reads require the catalogued 4,800-baud K-Line rate".into();
        }
        if n_ta.is_some_and(|value| !value.trim().is_empty()) {
            return "This SSM-KWP row requires an unsupported target-address extension".into();
        }
        if ssm_route(request_header, response_header).is_err() {
            return "The SSM-KWP ECU and tool addresses could not be read from the catalog headers"
                .into();
        }
        if parse_ssm_address(source_did).is_none() {
            return "The SSM-KWP source memory address is missing or outside 24 bits".into();
        }
    } else {
        if let Some(reason) = manual_can_read_reason(
            protocol,
            request_header,
            response_header,
            n_ta,
            service,
            identifier,
        ) {
            return reason.into();
        }
        if can_identifier_width(request_header) != Some(11)
            || can_identifier_width(response_header) != Some(11)
        {
            return "Live OEM graph logging currently requires an 11-bit CAN route".into();
        }
    }
    let Ok(decoder) = serde_json::from_str::<serde_json::Value>(decoder_json) else {
        return "The catalog decoder fields are malformed".into();
    };
    let method = json_text(&decoder, "conv_method");
    let enum_labels_missing = method == "1"
        && !decoder
            .get("enums")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| !values.is_empty());
    let raw_ssm_fallback = protocol.trim().eq_ignore_ascii_case("SSM-KWP")
        && (!matches!(method.as_str(), "1" | "2") || enum_labels_missing);
    if !matches!(method.as_str(), "1" | "2") && !raw_ssm_fallback {
        return "Only numeric scaling and enumerated-state decoders are supported for live graphs"
            .into();
    }
    let number = |key: &str| json_number(&decoder, key);
    let byte_length = number("byte_length").and_then(|value| usize::try_from(value).ok());
    let byte_pos = number("byte_pos");
    let bit_pos = number("bit_pos");
    let bit_size = number("bit_size");
    let Some((byte_length, byte_pos, bit_pos, bit_size)) = byte_length
        .zip(byte_pos)
        .zip(bit_pos)
        .zip(bit_size)
        .map(|(((length, byte), bit), size)| (length, byte, bit, size))
    else {
        return "The catalog decoder has no complete byte/bit layout".into();
    };
    if byte_length == 0
        || byte_length > 512
        || byte_pos == 0
        || bit_pos > 7
        || !(1..=40).contains(&bit_size)
        || (byte_pos - 1)
            .saturating_mul(8)
            .saturating_add(7 - bit_pos)
            .saturating_add(bit_size)
            > (byte_length as u32).saturating_mul(8)
    {
        return "The catalog decoder bit layout is outside the supported response range".into();
    }
    if !json_text(&decoder, "endian").eq_ignore_ascii_case("big") {
        return "Only big-endian catalog fields are supported for live graphs".into();
    }
    if method == "1" && !raw_ssm_fallback {
        if !decoder
            .get("enums")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| !values.is_empty())
        {
            return "The enumerated-state decoder has no labels".into();
        }
    } else if method == "2" {
        let scale = ["lsb_mul", "lsb_div", "offset"]
            .into_iter()
            .map(|key| json_f64(&decoder, key))
            .collect::<Option<Vec<_>>>();
        let significant = json_i32(&decoder, "significant");
        if !scale.is_some_and(|values| {
            let precision = significant
                .filter(|value| (-12..=12).contains(value))
                .map(|value| 10_f64.powi(value));
            values[0].is_finite()
                && values[1].is_finite()
                && values[1] != 0.0
                && values[2].is_finite()
                && precision.is_some_and(|precision| {
                    (values[1] * precision).is_finite()
                        && values[1] * precision != 0.0
                        && (values[2] / precision).is_finite()
                })
        }) || !significant.is_some_and(|value| (-12..=12).contains(&value))
        {
            return "The numeric decoder scale is incomplete or invalid".into();
        }
    }
    String::new()
}

pub(crate) fn ssm_route(request_header: &str, response_header: &str) -> Result<(u8, u8), String> {
    // Keep the full source headers on each candidate for audit. SSM2 frames address the
    // controller with the first request-header byte and use the response-header byte as tester.
    let request_bytes = header_bytes(request_header)
        .filter(|bytes| (1..=2).contains(&bytes.len()))
        .ok_or_else(|| "SSM ECU target address is malformed".to_owned())?;
    let response_bytes = header_bytes(response_header)
        .filter(|bytes| bytes.len() == 1)
        .ok_or_else(|| "SSM tool address is malformed".to_owned())?;
    let ecu = request_bytes[0];
    let tool = response_bytes[0];
    if ecu == 0 || ecu >= tool || tool != 0xf0 {
        return Err("SSM route requires a distinct ECU byte and tool address F0".into());
    }
    Ok((ecu, tool))
}

pub(crate) fn parse_ssm_address(value: &str) -> Option<u32> {
    let value = value.trim().trim_start_matches('$');
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    (!value.is_empty() && value.len() <= 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}

pub(crate) fn subaru_ssm_signal_addresses(spec: &OemCandidateTrySpec) -> Result<Vec<u32>, String> {
    let base = parse_ssm_address(&spec.candidate.identifier)
        .ok_or_else(|| "SSM source address is malformed".to_owned())?;
    let decoder: serde_json::Value = serde_json::from_str(&spec.candidate.decoder_json)
        .map_err(|error| format!("SSM decoder data is malformed: {error}"))?;
    let count = decoder
        .get("read_length")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (1..=512).contains(value))
        .ok_or_else(|| "SSM decoder read length is missing or outside 1-512 bytes".to_owned())?;
    let end = base
        .checked_add((count - 1) as u32)
        .filter(|value| *value <= 0x00ff_ffff)
        .ok_or_else(|| "SSM decoder memory range exceeds 24 bits".to_owned())?;
    Ok((base..=end).collect())
}

fn header_bytes(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for token in value.split_ascii_whitespace() {
        let token = token.trim_start_matches('$');
        let token = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
            .unwrap_or(token);
        if token.len() != 2 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        bytes.push(u8::from_str_radix(token, 16).ok()?);
    }
    (!bytes.is_empty()).then_some(bytes)
}

fn layout_subaru_ssm_group(group: &mut [OemCandidateTrySpec]) -> Result<(), String> {
    let mut addresses = BTreeSet::new();
    for spec in group.iter() {
        addresses.extend(subaru_ssm_signal_addresses(spec)?);
    }
    if addresses.is_empty() || addresses.len() > lognook_j2534_protocol::MAX_SUBARU_SSM_ADDRESSES {
        return Err("Selected SSM signals require more than 256 read addresses".into());
    }
    let addresses = addresses.into_iter().collect::<Vec<_>>();
    for spec in group {
        let base = parse_ssm_address(&spec.candidate.identifier)
            .ok_or_else(|| "SSM source address is malformed".to_owned())?;
        let base_index = addresses
            .binary_search(&base)
            .map_err(|_| "SSM source address is missing from its read group".to_owned())?;
        let local_offset = spec.candidate.bit_offset.unwrap_or(0);
        spec.candidate.bit_offset = Some(
            u32::try_from(base_index)
                .map_err(|_| "SSM group offset exceeds decoder range".to_owned())?
                .saturating_mul(8)
                .saturating_add(local_offset),
        );
    }
    Ok(())
}

fn json_text(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| Some(value.to_string()))
        })
        .unwrap_or_default()
}

fn json_number(value: &serde_json::Value, key: &str) -> Option<u32> {
    let value = value.get(key)?;
    value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn json_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    let value = value.get(key)?;
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn json_i32(value: &serde_json::Value, key: &str) -> Option<i32> {
    let value = value.get(key)?;
    value
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_i64().map(|number| number != 0))
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(false)
}

fn logging_spec(detail: &SubaruCatalogDetail) -> Result<OemCandidateTrySpec, String> {
    let row = &detail.row;
    let decoder: serde_json::Value = serde_json::from_str(&detail.decoder_json)
        .map_err(|error| format!("Subaru decoder data is invalid: {error}"))?;
    let byte_pos = json_number(&decoder, "byte_pos")
        .ok_or_else(|| "Subaru decoder byte position is missing".to_owned())?;
    let bit_pos = json_number(&decoder, "bit_pos")
        .ok_or_else(|| "Subaru decoder bit position is missing".to_owned())?;
    let bit_length = json_number(&decoder, "bit_size")
        .ok_or_else(|| "Subaru decoder bit length is missing".to_owned())?;
    let bit_offset = (byte_pos - 1) * 8 + 7 - bit_pos;
    let method = json_text(&decoder, "conv_method");
    let is_ssm = row.protocol.trim().eq_ignore_ascii_case("SSM-KWP");
    let enum_labels_missing = method == "1"
        && !decoder
            .get("enums")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| !values.is_empty());
    let raw_ssm_fallback = is_ssm && (!matches!(method.as_str(), "1" | "2") || enum_labels_missing);
    let (multiplier, divisor, addend, value_format) = if method == "1" && !raw_ssm_fallback {
        (1.0, 1.0, 0.0, "uint")
    } else if method == "2" {
        let scale = 10_f64.powi(
            json_i32(&decoder, "significant")
                .ok_or_else(|| "Subaru decoder precision is missing".to_owned())?,
        );
        let multiplier = json_f64(&decoder, "lsb_mul")
            .ok_or_else(|| "Subaru decoder multiplier is missing".to_owned())?;
        let divisor = json_f64(&decoder, "lsb_div")
            .ok_or_else(|| "Subaru decoder divisor is missing".to_owned())?;
        let offset = json_f64(&decoder, "offset")
            .ok_or_else(|| "Subaru decoder offset is missing".to_owned())?;
        (
            multiplier,
            divisor * scale,
            offset / scale,
            if json_bool(&decoder, "signed") {
                "signed"
            } else {
                "uint"
            },
        )
    } else if raw_ssm_fallback {
        (1.0, 1.0, 0.0, "uint")
    } else {
        return Err(format!(
            "Subaru catalog conversion method {method} is unsupported"
        ));
    };
    let unit = if raw_ssm_fallback || row.unit.trim().is_empty() {
        "code".to_owned()
    } else {
        row.unit.clone()
    };
    let enum_map = decoder
        .get("enums")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let raw = entry.get("value")?.as_str()?.trim_start_matches('$');
            let value = i64::from_str_radix(raw, 16).ok()?;
            let label = entry.get("label")?.as_str()?.to_owned();
            Some((value.to_string(), serde_json::Value::String(label)))
        })
        .collect::<serde_json::Map<_, _>>();
    let identifier = if is_ssm {
        format!(
            "{:06X}",
            parse_ssm_address(&detail.source_did)
                .ok_or_else(|| "SSM source memory address is malformed".to_owned())?
        )
    } else {
        row.request_identifier.clone()
    };
    let service = if is_ssm {
        "SSM-KWP".to_owned()
    } else {
        row.request_service.clone()
    };
    let decoder_json = serde_json::json!({
        "bix": bit_offset,
        "len": bit_length,
        "mul": multiplier,
        "div": divisor,
        "add": addend,
        "map": enum_map,
        "unit": unit,
        "read_length": json_number(&decoder, "byte_length").unwrap_or(1),
    })
    .to_string();
    let source_scope = format!("owner_supplied_subaru_catalog:{}", row.catalog_sha256);
    Ok(OemCandidateTrySpec {
        candidate: OemCandidate {
            candidate_id: row.id.clone(),
            source_id: row.source_id.clone(),
            family: "Subaru OEM catalog".into(),
            brand: "Subaru".into(),
            model_scope: row.vehicle_scope_json.clone(),
            service,
            identifier,
            request_header: if is_ssm {
                row.request_header.clone()
            } else {
                normalize_can_id(&row.request_header)
            },
            response_header: if is_ssm {
                row.response_header
                    .trim()
                    .trim_start_matches('$')
                    .to_ascii_uppercase()
            } else {
                normalize_can_id(&row.response_header)
            },
            frequency_hz: None,
            module_type: row.ecu_name.clone(),
            category: row.source_table.clone(),
            name: row.name.clone(),
            signal_source_id: if detail.signal_id.is_empty() {
                detail.source_pid_id.clone()
            } else {
                detail.signal_id.clone()
            },
            bit_offset: Some(bit_offset),
            bit_length: Some(bit_length),
            formula: if raw_ssm_fallback && enum_labels_missing {
                "raw source code (enum labels missing)".into()
            } else if raw_ssm_fallback {
                format!("raw source code (unsupported conversion method {method})")
            } else if method == "1" {
                "enumerated state numeric code".into()
            } else {
                json_text(&decoder, "expression")
            },
            decoder_json,
            unit,
            year_filter_json: row.vehicle_scope_json.clone(),
            ecu_filter: "REQUIRES_INDIVIDUAL_ECU_VERIFICATION".into(),
            source_blob_sha: String::new(),
            research_state: "review_required".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "inferred".into(),
            notes: format!(
                "{} · {} · source mapping unverified",
                row.source_table, row.source_revision
            ),
            suggested_metric: row.name.clone(),
            source_scope,
            applicability_specificity: 0,
            min_value: if raw_ssm_fallback {
                None
            } else {
                json_f64(&decoder, "min")
            },
            max_value: if raw_ssm_fallback {
                None
            } else {
                json_f64(&decoder, "max")
            },
            source_url: None,
            license: None,
            license_url: None,
        },
        value_format: value_format.into(),
        source_policy: OemCandidateSourcePolicy::LocalSubaruCatalog {
            catalog_sha256: row.catalog_sha256.clone(),
        },
    })
}

fn normalize_can_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('$')
        .strip_prefix("0x")
        .or_else(|| value.trim().trim_start_matches('$').strip_prefix("0X"))
        .unwrap_or_else(|| value.trim().trim_start_matches('$'))
        .to_ascii_uppercase()
}

fn optional_text(row: &Row<'_>, index: usize) -> rusqlite::Result<String> {
    Ok(row.get::<_, Option<String>>(index)?.unwrap_or_default())
}

fn like_filter(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "%%".into()
    } else {
        format!("%{value}%")
    }
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("Could not open Subaru catalog read-only: {error}"))
}

pub fn manual_can_read_reason(
    protocol: &str,
    request_header: &str,
    response_header: &str,
    n_ta: Option<&str>,
    service: &str,
    identifier: &str,
) -> Option<&'static str> {
    let protocol = protocol.trim().to_ascii_uppercase();
    if protocol != "FHI-CAN" {
        return Some("This protocol is not supported by the CAN Workbench");
    }
    let request_width = can_identifier_width(request_header);
    let response_width = can_identifier_width(response_header);
    if request_width.is_none() || response_width.is_none() {
        return Some("The source route is not a supported 11-bit or 29-bit CAN pair");
    }
    if request_width != response_width {
        return Some("The request and response CAN identifiers use different address widths");
    }
    if same_can_identifier(request_header, response_header) {
        return Some("The CAN request and response identifiers must differ");
    }
    if n_ta.is_some_and(|value| !value.trim().is_empty()) {
        return Some("This ECU requires an extended address the CAN Workbench does not support");
    }

    let width = match service.trim() {
        "21" => 2,
        "22" => 4,
        _ => return Some("The inferred request is not a read-only service 21/22 request"),
    };
    if identifier.trim().len() != width
        || !identifier
            .trim()
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Some("The inferred identifier width does not match service 21/22");
    }

    None
}

pub fn decoder_supported(
    conv_method: &str,
    bit_size: Option<u32>,
    expression: &str,
    enums_json: &str,
) -> bool {
    match conv_method.trim() {
        "1" => serde_json::from_str::<serde_json::Value>(enums_json)
            .ok()
            .is_some_and(|value| {
                value.as_array().is_some_and(|items| !items.is_empty())
                    || value.as_object().is_some_and(|items| !items.is_empty())
            }),
        "2" => {
            bit_size.is_some_and(|bits| (1..=64).contains(&bits)) && !expression.trim().is_empty()
        }
        _ => false,
    }
}

fn can_identifier_width(value: &str) -> Option<usize> {
    let value = value.trim().trim_start_matches('$');
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    match value.len() {
        3 if u16::from_str_radix(value, 16).is_ok_and(|id| id <= 0x7ff) => Some(11),
        8 if u32::from_str_radix(value, 16).is_ok_and(|id| id <= 0x1fff_ffff) => Some(29),
        _ => None,
    }
}

fn same_can_identifier(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        let value = value.trim().trim_start_matches('$');
        value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value)
            .to_ascii_uppercase()
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use super::{SubaruCatalog, decoder_supported, manual_can_read_reason};
    use rusqlite::{Connection, OpenFlags};
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        time::SystemTime,
    };

    struct TestCatalog(PathBuf);

    impl TestCatalog {
        fn new() -> Self {
            let id = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("lognook-subaru-catalog-{id}.sqlite"));
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE subaru_catalog_search(
                       candidate_id TEXT, definition_id TEXT, source_id TEXT,
                       source_display_name TEXT, source_revision TEXT, catalog_sha256 TEXT,
                       source_table TEXT, source_did TEXT, signal_id TEXT, source_pid_id TEXT,
                       ecu_key TEXT, ecu_id TEXT, ecu_name TEXT, protocol_family TEXT,
                       data_rate INTEGER, request_header TEXT, response_header TEXT, n_ta TEXT,
                       request_hint TEXT, hint_service TEXT, hint_identifier TEXT, mapping_state TEXT,
                       name TEXT, unit TEXT, conv_method TEXT, byte_length INTEGER, byte_pos INTEGER,
                       bit_pos INTEGER, bit_size INTEGER, endian TEXT, signed INTEGER, lsb_mul TEXT,
                       lsb_div TEXT, offset TEXT, significant TEXT, factor TEXT, offset_scaled TEXT,
                       expression TEXT, min TEXT, max TEXT, unit_id TEXT, genre TEXT,
                       enums_json TEXT, decoder_payload_json TEXT, raw_row_json TEXT, link_metadata_json TEXT,
                       vehicle_validation TEXT, auto_request INTEGER, vehicle_scope_json TEXT
                     );
                     CREATE TABLE subaru_vehicle_specs(
                       vehicle_key TEXT, source_id TEXT, display_label TEXT, model_group TEXT,
                       models_json TEXT, model_year INTEGER, year_from INTEGER, year_to INTEGER,
                       year_range_state TEXT, is_pseudo INTEGER
                     );
                     CREATE TABLE subaru_vehicle_ecu_links(vehicle_key TEXT, ecu_key TEXT);",
                )
                .unwrap();
            connection.execute_batch(
                "INSERT INTO subaru_catalog_search VALUES (
                   'cand-rpm','def-rpm','SUBARU_LOCAL','Subaru OEM catalog','db-40.01b','sha256-catalog',
                   '12002_DTM.xml','$3F0C','10074','10074','ecu-12002','12002','Engine','FHI-CAN',500000,
                   '$7E0','$7E8',NULL,'22 3F0C','22','3F0C','inferred','Engine Speed','rpm','2',2,1,7,16,
                   'big',0,'25','1','0','2','0.25','0','raw * 0.25','0','16383.75','', '0','[]',
                   '{\"bit_size\":16}','{\"name\":\"Engine Speed\"}','{\"ecu_id\":\"12002\"}','not_tested',0,'[]'
                 );
                 INSERT INTO subaru_catalog_search VALUES (
                   'subaru-candidate_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                   'def-kwp','SUBARU_OEM_40_01B_ABCD1234','Subaru OEM catalog','40.01b',
                   'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                   '12000_DTM.xml','$000E','10000','10000','ecu-12000','12000','Engine','SSM-KWP',4800,
                   '$10 $20','$F0',NULL,'22 000E','22','000E','inferred','Engine Speed','rpm','2',2,1,7,16,
                   'big',0,'25','1','0','2','0.25','0','raw * 0.25','0','16383.75','rpm', '0','[]',
                   '{\"conv_method\":\"2\",\"bit_size\":16,\"byte_length\":2,\"byte_pos\":1,\"bit_pos\":7,\"endian\":\"big\",\"lsb_mul\":\"25\",\"lsb_div\":\"1\",\"offset\":\"0\",\"significant\":\"2\",\"min\":\"0\",\"max\":\"16383.75\",\"enums\":[]}',
                   '{\"name\":\"Engine Speed\"}','{\"ecu_id\":\"12000\"}','not_tested',0,'[]'
                 );
                 INSERT INTO subaru_catalog_search VALUES (
                   'subaru-candidate_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                   'def-kwp-coolant','SUBARU_OEM_40_01B_ABCD1234','Subaru OEM catalog','40.01b',
                   'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                   '12000_DTM.xml','$0008','10001','10001','ecu-12000','12000','Engine','SSM-KWP',4800,
                   '$10 $20','$F0',NULL,'22 0008','22','0008','inferred','Coolant Temp.','°C','2',1,1,7,8,
                   'big',0,'1','1','-40','0','1','-40','raw - 40','-40','215','degC', '0','[]',
                   '{\"conv_method\":\"2\",\"bit_size\":8,\"byte_length\":1,\"byte_pos\":1,\"bit_pos\":7,\"endian\":\"big\",\"lsb_mul\":\"1\",\"lsb_div\":\"1\",\"offset\":\"-40\",\"significant\":\"0\",\"min\":\"-40\",\"max\":\"215\",\"enums\":[]}',
                   '{\"name\":\"Coolant Temp.\"}','{\"ecu_id\":\"12000\"}','not_tested',0,'[]'
                 );
                 INSERT INTO subaru_catalog_search VALUES (
                   'subaru-candidate_dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
                   'def-kwp-raw','SUBARU_OEM_40_01B_ABCD1234','Subaru OEM catalog','40.01b',
                   'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                   '12000_DTM.xml','$0227','10565','10565','ecu-12000','12000','Engine','SSM-KWP',4800,
                   '$10 $20','$F0',NULL,'22 0227','22','0227','inferred','Injector Multi Mode for Monitor','',
                   '3',4,1,7,32,'big',0,'1','1','0','0','','','', '00000000','4294967295','', '0','[]',
                   '{\"conv_method\":\"3\",\"bit_size\":32,\"byte_length\":4,\"byte_pos\":1,\"bit_pos\":7,\"endian\":\"big\",\"lsb_mul\":\"1\",\"lsb_div\":\"1\",\"offset\":\"0\",\"significant\":\"0\",\"min\":\"00000000\",\"max\":\"4294967295\",\"enums\":[]}',
                   '{\"name\":\"Injector Multi Mode for Monitor\"}','{\"ecu_id\":\"12000\"}','not_tested',0,'[]'
                 );
                 INSERT INTO subaru_vehicle_specs VALUES
                   ('wrx-2020','SUBARU_LOCAL','WRX > 20MY','WRX','[\"WRX\"]',2020,NULL,NULL,'exact_year',0),
                   ('legacy-2020','SUBARU_LOCAL','Legacy > 20MY','Legacy','[\"Legacy\"]',2020,NULL,NULL,'exact_year',0),
                   ('outback-range','SUBARU_LOCAL','Outback > 18-22MY','Outback','[\"Outback\"]',NULL,2018,2022,'source_range_unverified',0),
                   ('forester-unknown','SUBARU_LOCAL','Forester year unresolved','Forester','[\"Forester\"]',NULL,NULL,NULL,'unresolved',0);
                 INSERT INTO subaru_vehicle_ecu_links VALUES
                   ('wrx-2020','ecu-12002'),('legacy-2020','ecu-12000'),
                   ('outback-range','ecu-12002'),('forester-unknown','ecu-12002');",
            )
            .unwrap();
            drop(connection);
            Self(path)
        }
    }

    impl Drop for TestCatalog {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn manual_can_read_checks_only_transport_and_request_shape() {
        assert_eq!(
            manual_can_read_reason("FHI-CAN", "0x18DAF110", "0x18DA10F1", None, "22", "3F0C",),
            None
        );

        assert!(manual_can_read_reason("SSM-KWP", "$10 $20", "$F0", None, "22", "3F0C",).is_some());
        assert!(
            manual_can_read_reason("Phase4-CAN", "$7E0", "$7E8", None, "22", "3F0C",).is_some()
        );
        assert!(
            manual_can_read_reason("Phase5-CAN", "$7E0", "$7E8", None, "22", "3F0C",).is_some()
        );
        assert!(
            manual_can_read_reason("FHI-CAN", "$7E0", "$7E8", Some("$12"), "22", "3F0C",).is_some()
        );
        assert!(manual_can_read_reason("FHI-CAN", "$7E0", "$7E8", None, "22", "3F0C",).is_none());
        assert!(manual_can_read_reason("FHI-CAN", "$7E0", "$7E0", None, "22", "3F0C",).is_some());
        assert!(manual_can_read_reason("FHI-CAN", "$7E0", "$7E8", None, "22", "3F",).is_some());
        assert!(
            manual_can_read_reason("FHI-CAN", "7E0", "18DA10F1", None, "22", "3F0C",).is_some()
        );
        assert!(
            manual_can_read_reason("FHI-CAN", "000007E0", "000007E8", None, "22", "3F0C",)
                .is_none()
        );
    }

    #[test]
    fn decoder_support_is_reported_separately_from_manual_readability() {
        assert!(decoder_supported("2", Some(16), "raw * 0.25", "[]"));
        assert!(decoder_supported("1", Some(1), "", "[\"$00=OFF\"]"));
        assert!(!decoder_supported("2", Some(16), "", "[]"));
        assert!(!decoder_supported("116", Some(16), "raw", "[]"));
    }

    #[test]
    fn unnamed_entries_sort_after_named_entries_before_pagination() {
        let db = TestCatalog::new();
        let candidate_id =
            "subaru-candidate_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let connection = Connection::open(&db.0).unwrap();
        connection
            .execute(
                "UPDATE subaru_catalog_search SET name='  ', signal_id='IDS_BLANK' WHERE candidate_id=?1",
                [candidate_id],
            )
            .unwrap();
        drop(connection);
        let catalog = SubaruCatalog::new(&db.0);
        let first = catalog
            .search("", "", None, "12000", "SSM-KWP", 0, 1)
            .unwrap();
        let last = catalog
            .search("", "", None, "12000", "SSM-KWP", 2, 1)
            .unwrap();
        assert_eq!(first.total, 3);
        assert_eq!(first.rows[0].name, "Engine Speed");
        assert_eq!(last.rows[0].id, candidate_id);
        assert_eq!(last.rows[0].name, "IDS_BLANK");
        assert_eq!(
            catalog
                .search("IDS_BLANK", "", None, "", "", 0, 10)
                .unwrap()
                .total,
            1
        );
    }

    #[test]
    fn unnamed_signals_use_source_signal_id_in_search_details_and_logging() {
        let db = TestCatalog::new();
        let candidate_id =
            "subaru-candidate_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let connection = Connection::open(&db.0).unwrap();
        connection
            .execute(
                "UPDATE subaru_catalog_search SET name='  ', signal_id='IDS_DLTITEM_ROUGHNESS_1', raw_row_json='{\"name\":\"\"}' WHERE candidate_id=?1",
                [candidate_id],
            )
            .unwrap();
        drop(connection);
        let catalog = SubaruCatalog::new(&db.0);
        let page = catalog.search("", "", None, "", "SSM-KWP", 0, 10).unwrap();
        let row = page.rows.iter().find(|row| row.id == candidate_id).unwrap();
        assert_eq!(row.name, "IDS_DLTITEM_ROUGHNESS_1");
        assert_eq!(
            catalog
                .search("IDS_DLTITEM_ROUGHNESS_1", "", None, "", "", 0, 10)
                .unwrap()
                .total,
            1
        );
        let detail = catalog.get(candidate_id).unwrap();
        assert_eq!(detail.row.name, "IDS_DLTITEM_ROUGHNESS_1");
        assert_eq!(detail.raw_source_json, r#"{"name":""}"#);
        let plan = catalog.logging_plan(&[candidate_id.to_owned()]).unwrap();
        assert_eq!(plan.groups[0][0].candidate.name, "IDS_DLTITEM_ROUGHNESS_1");
    }

    #[test]
    fn search_filters_vehicle_ecu_and_protocol_without_losing_provenance() {
        let db = TestCatalog::new();
        let catalog = SubaruCatalog::new(&db.0);
        let page = catalog
            .search("3F0C", "WRX", Some(2020), "12002", "FHI-CAN", 0, 10)
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].id, "cand-rpm");
        assert_eq!(page.rows[0].source_display_name, "Subaru OEM catalog");
        assert_eq!(page.rows[0].source_revision, "db-40.01b");
        assert_eq!(page.rows[0].request_hint, "22 3F0C");
        assert_eq!(page.rows[0].mapping_state, "inferred");
        assert_eq!(page.rows[0].vehicle_validation, "not_tested");
        assert!(!page.rows[0].auto_request);
        assert!(page.rows[0].can_manual_read);
        assert!(page.rows[0].decoder_supported);

        let detail = catalog.get("cand-rpm").unwrap();
        assert_eq!(detail.raw_source_json, "{\"name\":\"Engine Speed\"}");
        assert_eq!(detail.row.catalog_sha256, "sha256-catalog");
    }

    #[test]
    fn search_paginates_and_exposes_ssm_rows_for_live_graphs_without_can_prefill() {
        let db = TestCatalog::new();
        let connection = Connection::open(&db.0).unwrap();
        connection
            .execute(
                "INSERT INTO subaru_catalog_search SELECT * FROM subaru_catalog_search WHERE candidate_id=?1",
                ["subaru-candidate_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"],
            )
            .unwrap();
        let rowid = connection.last_insert_rowid();
        connection
            .execute(
                "UPDATE subaru_catalog_search SET candidate_id='subaru-candidate_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', definition_id='def-kwp-unlabeled-enum', source_did='$0049', signal_id='10002', source_pid_id='10002', request_hint='22 0049', hint_identifier='0049', name='ATF Temp.', conv_method='1', enums_json='[]', decoder_payload_json=?1, raw_row_json='{}' WHERE rowid=?2",
                rusqlite::params![
                    r#"{"conv_method":"1","bit_size":8,"byte_length":1,"byte_pos":1,"bit_pos":7,"endian":"big","enums":[]}"#,
                    rowid,
                ],
            )
            .unwrap();
        drop(connection);
        let catalog = SubaruCatalog::new(&db.0);
        let first = catalog.search("", "", None, "", "SSM-KWP", 0, 1).unwrap();
        let second = catalog.search("", "", None, "", "SSM-KWP", 1, 1).unwrap();
        assert_eq!(first.total, 4);
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_ne!(first.rows[0].id, second.rows[0].id);
        let rows = catalog.search("", "", None, "", "SSM-KWP", 0, 10).unwrap();
        let engine_row = rows
            .rows
            .iter()
            .find(|row| row.name == "Engine Speed")
            .unwrap();
        let coolant_row = rows
            .rows
            .iter()
            .find(|row| row.name == "Coolant Temp.")
            .unwrap();
        let raw_row = rows
            .rows
            .iter()
            .find(|row| row.name == "Injector Multi Mode for Monitor")
            .unwrap();
        let enum_without_labels = rows
            .rows
            .iter()
            .find(|row| row.name == "ATF Temp.")
            .unwrap();
        for row in [engine_row, coolant_row] {
            assert_eq!(row.protocol, "SSM-KWP");
            assert!(!row.can_manual_read);
            assert!(!row.manual_read_reason.is_empty());
            assert!(row.decoder_supported);
            assert!(row.can_graph, "{}", row.graph_reason);
            assert!(!row.auto_request);
        }
        assert_eq!(engine_row.source_did, "$000E");
        assert_eq!(engine_row.request_hint, "22 000E");
        assert!(!raw_row.decoder_supported);
        assert!(raw_row.can_graph, "{}", raw_row.graph_reason);
        assert!(!enum_without_labels.decoder_supported);
        assert!(
            enum_without_labels.can_graph,
            "{}",
            enum_without_labels.graph_reason
        );
        let selected = rows
            .rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>();
        let plan = catalog.logging_plan(&selected).unwrap();
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].len(), 4);
        let engine_speed = plan.groups[0]
            .iter()
            .find(|spec| spec.candidate.name == "Engine Speed")
            .unwrap();
        let coolant = plan.groups[0]
            .iter()
            .find(|spec| spec.candidate.name == "Coolant Temp.")
            .unwrap();
        let raw_signal = plan.groups[0]
            .iter()
            .find(|spec| spec.candidate.name == "Injector Multi Mode for Monitor")
            .unwrap();
        assert_eq!(engine_speed.candidate.service, "SSM-KWP");
        assert_eq!(engine_speed.candidate.identifier, "00000E");
        assert_eq!(engine_speed.candidate.request_header, "$10 $20");
        assert_eq!(engine_speed.candidate.bit_offset, Some(8));
        assert_eq!(coolant.candidate.bit_offset, Some(0));
        assert_eq!(raw_signal.value_format, "uint");
        assert_eq!(raw_signal.candidate.unit, "code");
        assert!(raw_signal.candidate.formula.contains("raw source code"));
        let raw_enum = plan.groups[0]
            .iter()
            .find(|spec| spec.candidate.name == "ATF Temp.")
            .unwrap();
        assert_eq!(raw_enum.value_format, "uint");
        assert_eq!(raw_enum.candidate.unit, "code");
        assert!(raw_enum.candidate.formula.contains("raw source code"));
    }

    #[test]
    fn year_filter_uses_exact_and_valid_range_scope_but_not_unresolved_ranges() {
        let db = TestCatalog::new();
        let catalog = SubaruCatalog::new(&db.0);
        let year_only = catalog.search("", "", Some(2021), "", "", 0, 10).unwrap();
        let model_and_year = catalog
            .search("", "Outback", Some(2021), "", "", 0, 10)
            .unwrap();
        let mismatched_model_year = catalog
            .search("", "WRX", Some(2019), "", "", 0, 10)
            .unwrap();
        let unresolved_year = catalog
            .search("", "Forester", Some(2020), "", "", 0, 10)
            .unwrap();
        assert_eq!(year_only.total, 1);
        assert_eq!(model_and_year.total, 1);
        assert_eq!(mismatched_model_year.total, 0);
        assert_eq!(unresolved_year.total, 0);
    }

    #[test]
    #[ignore = "requires owner-supplied local catalog"]
    fn authoritative_catalog_search_and_detail_smoke() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite");
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        let mut statement = connection
            .prepare(
                "SELECT protocol_family,request_header,response_header,n_ta,hint_service,
                        hint_identifier,conv_method,bit_size,expression,enums_json,
                        vehicle_validation,auto_request FROM subaru_catalog_search",
            )
            .unwrap();
        let mut by_protocol = BTreeMap::<String, (usize, usize, usize)>::new();
        let mut total = 0usize;
        let mut manual_total = 0usize;
        let mut decoder_total = 0usize;
        let mut auto_request_total = 0usize;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<u32>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            })
            .unwrap();
        for row in rows {
            let (
                protocol,
                request,
                response,
                n_ta,
                service,
                identifier,
                conv,
                bits,
                expression,
                enums,
                validation,
                auto,
            ) = row.unwrap();
            total += 1;
            let requestable = manual_can_read_reason(
                &protocol,
                request.as_deref().unwrap_or_default(),
                response.as_deref().unwrap_or_default(),
                n_ta.as_deref(),
                service.as_deref().unwrap_or_default(),
                identifier.as_deref().unwrap_or_default(),
            )
            .is_none();
            let decodable = decoder_supported(&conv, bits, &expression, &enums);
            if requestable {
                manual_total += 1;
            }
            if decodable {
                decoder_total += 1;
            }
            auto_request_total += if auto != 0 { 1 } else { 0 };
            let counts = by_protocol.entry(protocol).or_default();
            counts.0 += 1;
            counts.1 += if requestable { 1 } else { 0 };
            counts.2 += if decodable { 1 } else { 0 };
            assert_eq!(validation, "not_tested");
            assert_eq!(auto, 0);
        }
        println!(
            "Subaru catalog rows={total} manual_can_prefill={manual_total} decoder_fields_supported={decoder_total} auto_request={auto_request_total} per_protocol={by_protocol:?}"
        );
        drop(statement);
        drop(connection);

        let catalog = SubaruCatalog::new(path);
        let page = catalog.search("", "WRX", Some(2020), "", "", 0, 1).unwrap();
        assert!(page.total > 0);
        let row = &page.rows[0];
        assert_eq!(row.source_display_name, "Subaru OEM catalog");
        assert_eq!(row.vehicle_validation, "not_tested");
        assert!(!row.auto_request);
        assert_eq!(row.mapping_state, "inferred");
        assert!(row.catalog_sha256.len() == 64);
        assert!(!row.vehicle_scope_json.is_empty());

        let detail = catalog.get(&row.id).unwrap();
        assert_eq!(detail.row.id, row.id);
        assert!(!detail.raw_source_json.is_empty());
        assert!(!detail.link_metadata_json.is_empty());
        assert!(!detail.decoder_json.is_empty());
    }
}
