use rusqlite::{Connection, OpenFlags, Row, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCION_XB_SOURCE_ID: &str = "OBDB_SCION_XB_DEFAULT";
pub const SCION_XB_BLOB_SHA: &str = "be4c7ba0624359b6e7bf217af6509bdb12ee37fc";
const MAX_PAGE_SIZE: usize = 100;
const MAX_OFFSET: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OemCatalogPage {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub candidates: Vec<OemCandidate>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OemCandidate {
    pub candidate_id: String,
    pub source_id: String,
    pub family: String,
    pub brand: String,
    pub model_scope: String,
    pub service: String,
    pub identifier: String,
    pub request_header: String,
    pub response_header: String,
    pub frequency_hz: Option<f64>,
    pub module_type: String,
    pub category: String,
    pub name: String,
    pub signal_source_id: String,
    pub bit_offset: Option<u32>,
    pub bit_length: Option<u32>,
    pub formula: String,
    pub decoder_json: String,
    pub unit: String,
    pub year_filter_json: String,
    pub ecu_filter: String,
    pub source_blob_sha: String,
    pub research_state: String,
    pub vehicle_validation: String,
    pub auto_request: bool,
    pub confidence: String,
    pub notes: String,
    pub suggested_metric: String,
    pub source_scope: String,
    pub applicability_specificity: u32,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub source_url: Option<String>,
    pub license: Option<String>,
    pub license_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScionMode21ProbeSpec {
    pub source_id: String,
    pub source_blob_sha: String,
    pub source_url: String,
    pub license: String,
    pub license_url: String,
    pub service: u8,
    pub identifier: u8,
    pub request_header: String,
    pub response_header: String,
    pub channels: Vec<ScionProbeChannel>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OemCandidateTrySpec {
    pub candidate: OemCandidate,
    pub value_format: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScionProbeChannel {
    pub candidate_id: String,
    pub name: String,
    pub unit: String,
    pub bit_offset: u32,
    pub bit_length: u32,
    pub multiplier: f64,
    pub divisor: f64,
    pub addend: f64,
    pub formula: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

pub struct OemCatalog {
    path: std::path::PathBuf,
}

impl OemCatalog {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_owned() }
    }

    pub fn search(
        &self,
        query: &str,
        brand: &str,
        model: &str,
        service: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<OemCatalogPage, String> {
        let limit = limit.clamp(1, MAX_PAGE_SIZE);
        if offset > MAX_OFFSET {
            return Err(format!("Catalog offset exceeds {MAX_OFFSET}"));
        }
        if service.is_some_and(|value| !matches!(value, "21" | "22")) {
            return Err("OEM catalog filters support read-only services 21 and 22".into());
        }
        let connection = open_read_only(&self.path)?;
        let query = like_filter(query);
        let brand = like_filter(brand);
        let model = like_filter(model);
        let service = service.map(str::to_owned);
        let total = connection.query_row(
            "SELECT COUNT(*) FROM oem_candidates c
             WHERE (?1 = '%%' OR c.name LIKE ?1 OR c.category LIKE ?1 OR c.unit LIKE ?1
                OR c.identifier LIKE ?1 OR c.service || c.identifier LIKE ?1
                OR c.service || ' ' || c.identifier LIKE ?1
                OR c.request_header LIKE ?1 OR c.response_header LIKE ?1 OR c.source_id LIKE ?1
                OR c.family LIKE ?1 OR c.signal_source_id LIKE ?1 OR c.suggested_metric LIKE ?1
                OR c.source_scope LIKE ?1 OR c.ecu_filter LIKE ?1 OR c.source_blob_sha LIKE ?1
                OR c.module_type LIKE ?1 OR c.brand LIKE ?1 OR c.model_scope LIKE ?1)
               AND (?2 = '%%' OR c.brand LIKE ?2)
               AND (?3 = '%%' OR c.model_scope LIKE ?3)
               AND (?4 IS NULL OR c.service = ?4)",
            params![query, brand, model, service],
            |row| row.get::<_, i64>(0),
        ).map_err(|error| error.to_string())?;
        let total = usize::try_from(total).map_err(|error| error.to_string())?;
        let mut statement = connection.prepare(
            "SELECT c.candidate_id,c.source_id,c.family,c.brand,c.model_scope,c.service,c.identifier,
                    c.request_header,c.response_header,c.frequency_hz,c.module_type,c.category,c.name,
                    c.signal_source_id,c.bit_offset,c.bit_length,c.formula,c.unit,c.year_filter_json,c.ecu_filter,
                    c.source_blob_sha,c.research_state,c.vehicle_validation,c.auto_request,c.confidence,c.notes,
                    s.source_url,s.license,s.license_url,c.decoder_json,c.suggested_metric,c.source_scope,
                    c.applicability_specificity,c.min_value,c.max_value
             FROM oem_candidates c LEFT JOIN source_corpora s ON s.source_id=c.source_id
             WHERE (?1 = '%%' OR c.name LIKE ?1 OR c.category LIKE ?1 OR c.unit LIKE ?1
                OR c.identifier LIKE ?1 OR c.service || c.identifier LIKE ?1
                OR c.service || ' ' || c.identifier LIKE ?1
                OR c.request_header LIKE ?1 OR c.response_header LIKE ?1 OR c.source_id LIKE ?1
                OR c.family LIKE ?1 OR c.signal_source_id LIKE ?1 OR c.suggested_metric LIKE ?1
                OR c.source_scope LIKE ?1 OR c.ecu_filter LIKE ?1 OR c.source_blob_sha LIKE ?1
                OR c.module_type LIKE ?1 OR c.brand LIKE ?1 OR c.model_scope LIKE ?1)
               AND (?2 = '%%' OR c.brand LIKE ?2)
               AND (?3 = '%%' OR c.model_scope LIKE ?3)
               AND (?4 IS NULL OR c.service = ?4)
             ORDER BY c.applicability_specificity DESC,c.brand,c.model_scope,c.service,c.identifier,
                      c.request_header,c.name,c.candidate_id
             LIMIT ?5 OFFSET ?6",
        ).map_err(|error| error.to_string())?;
        let candidates = statement.query_map(
            params![query, brand, model, service, limit as i64, offset as i64],
            read_candidate,
        ).map_err(|error| error.to_string())?;
        let candidates = candidates.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
        Ok(OemCatalogPage { total, offset, limit, candidates })
    }

    pub fn candidate_for_try(&self, candidate_id: &str) -> Result<OemCandidateTrySpec, String> {
        let candidate_id = candidate_id.trim();
        if candidate_id.is_empty() || candidate_id.len() > 128 {
            return Err("Choose a valid OEM candidate".into());
        }
        let connection = open_read_only(&self.path)?;
        let mut statement = connection.prepare(
            "SELECT c.candidate_id,c.source_id,c.family,c.brand,c.model_scope,c.service,c.identifier,
                    c.request_header,c.response_header,c.frequency_hz,c.module_type,c.category,c.name,
                    c.signal_source_id,c.bit_offset,c.bit_length,c.formula,c.unit,c.year_filter_json,c.ecu_filter,
                    c.source_blob_sha,c.research_state,c.vehicle_validation,c.auto_request,c.confidence,c.notes,
                    s.source_url,s.license,s.license_url,c.decoder_json,c.suggested_metric,c.source_scope,
                    c.applicability_specificity,c.min_value,c.max_value,c.value_format,s.blob_sha
             FROM oem_candidates c JOIN source_corpora s ON s.source_id=c.source_id
             WHERE c.candidate_id=?1",
        ).map_err(|error| error.to_string())?;
        let mut rows = statement.query([candidate_id]).map_err(|error| error.to_string())?;
        let row = rows.next().map_err(|error| error.to_string())?
            .ok_or_else(|| "OEM candidate or its source record is unavailable".to_owned())?;
        let candidate = read_candidate(row).map_err(|error| error.to_string())?;
        let value_format: String = row.get(35).map_err(|error| error.to_string())?;
        let corpus_blob_sha: String = row.get(36).map_err(|error| error.to_string())?;
        if rows.next().map_err(|error| error.to_string())?.is_some() {
            return Err("OEM candidate ID is ambiguous in the source catalog".into());
        }
        if candidate.source_blob_sha.is_empty() || candidate.source_blob_sha != corpus_blob_sha
            || candidate.source_url.as_deref().is_none_or(str::is_empty)
            || candidate.license.as_deref().is_none_or(str::is_empty)
            || candidate.license_url.as_deref().is_none_or(str::is_empty)
        {
            return Err("OEM candidate source revision or license metadata is incomplete".into());
        }
        let identifier_width = match candidate.service.as_str() {
            "21" => 2,
            "22" => 4,
            _ => return Err("Only source-backed read services 21 and 22 can be tried".into()),
        };
        if !is_hex_width(&candidate.identifier, identifier_width)
            || !is_standard_can_header(&candidate.request_header)
            || !is_standard_can_header(&candidate.response_header)
        {
            return Err("OEM candidate needs a one-byte service 21 PID or two-byte service 22 DID and exact 11-bit CAN headers".into());
        }
        Ok(OemCandidateTrySpec { candidate, value_format })
    }

    pub fn scion_xb_mode21_probe(&self) -> Result<ScionMode21ProbeSpec, String> {
        let connection = open_read_only(&self.path)?;
        let (repository, blob_sha, source_url, license, license_url): (String, String, String, String, String) = connection
            .query_row(
                "SELECT repository,blob_sha,source_url,license,license_url FROM source_corpora WHERE source_id=?1",
                [SCION_XB_SOURCE_ID],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
            ).map_err(|error| error.to_string())?;
        if repository != "OBDb/Scion-xB" || blob_sha != SCION_XB_BLOB_SHA {
            return Err("The Scion xB source revision does not match the reviewed Mode 21 definition".into());
        }
        let mut statement = connection.prepare(
            "SELECT candidate_id,name,unit,bit_offset,bit_length,source_blob_sha,auto_request,
                    vehicle_validation,research_state,confidence,decoder_json,formula
             FROM oem_candidates WHERE source_id=?1 AND service='21' AND identifier='01'
                AND request_header='7E0' AND response_header='7E8' AND name=?2",
        ).map_err(|error| error.to_string())?;
        let expected = [
            ("Intake air temperature", 48, 8),
            ("Coolant temperature", 64, 8),
            ("Engine speed", 72, 16),
            ("Vehicle speed", 88, 8),
            ("Throttle sensor position", 112, 8),
        ];
        let mut channels = Vec::with_capacity(expected.len());
        for (name, offset, length) in expected {
            let mut rows = statement.query(params![SCION_XB_SOURCE_ID, name]).map_err(|error| error.to_string())?;
            let row = rows.next().map_err(|error| error.to_string())?
                .ok_or_else(|| format!("Scion 21 01 channel is missing from the catalog: {name}"))?;
            let candidate_id: String = row.get(0).map_err(|error| error.to_string())?;
            let actual_name: String = row.get(1).map_err(|error| error.to_string())?;
            let unit: String = row.get(2).map_err(|error| error.to_string())?;
            let bit_offset: u32 = row.get(3).map_err(|error| error.to_string())?;
            let bit_length: u32 = row.get(4).map_err(|error| error.to_string())?;
            let candidate_sha: String = row.get(5).map_err(|error| error.to_string())?;
            let auto_request: i64 = row.get(6).map_err(|error| error.to_string())?;
            let vehicle_validation: String = row.get(7).map_err(|error| error.to_string())?;
            let research_state: String = row.get(8).map_err(|error| error.to_string())?;
            let confidence: String = row.get(9).map_err(|error| error.to_string())?;
            let decoder_json: String = row.get(10).map_err(|error| error.to_string())?;
            let formula: String = row.get(11).map_err(|error| error.to_string())?;
            if rows.next().map_err(|error| error.to_string())?.is_some()
                || actual_name != name || bit_offset != offset || bit_length != length
                || candidate_sha != blob_sha || auto_request != 0 || vehicle_validation != "not_tested"
                || !matches!(research_state.as_str(), "research" | "community_reference")
                || confidence != "research"
            {
                return Err(format!("Scion 21 01 channel failed source/safety checks: {name}"));
            }
            let decoder: DecoderScale = serde_json::from_str(&decoder_json).map_err(|error| error.to_string())?;
            if decoder.bit_offset != Some(offset) || decoder.bit_length != Some(length)
                || decoder.divisor == 0.0 || decoder.unit.as_deref() != Some(unit.as_str())
            {
                return Err(format!("Scion 21 01 decoder metadata is inconsistent: {name}"));
            }
            channels.push(ScionProbeChannel {
                candidate_id, name: name.into(), unit, bit_offset, bit_length,
                multiplier: decoder.multiplier, divisor: decoder.divisor, addend: decoder.addend,
                formula,
                minimum: decoder.minimum, maximum: decoder.maximum,
            });
        }
        Ok(ScionMode21ProbeSpec {
            source_id: SCION_XB_SOURCE_ID.into(), source_blob_sha: blob_sha,
            source_url, license, license_url, service: 0x21, identifier: 0x01,
            request_header: "7E0".into(), response_header: "7E8".into(), channels,
        })
    }
}

fn read_candidate(row: &Row<'_>) -> rusqlite::Result<OemCandidate> {
    Ok(OemCandidate {
        candidate_id: row.get(0)?, source_id: row.get(1)?, family: row.get(2)?,
        brand: row.get(3)?, model_scope: row.get(4)?, service: row.get(5)?,
        identifier: row.get(6)?, request_header: row.get(7)?, response_header: row.get(8)?,
        frequency_hz: row.get(9)?, module_type: row.get(10)?, category: row.get(11)?,
        name: row.get(12)?, signal_source_id: row.get(13)?, bit_offset: row.get(14)?,
        bit_length: row.get(15)?, formula: row.get(16)?, unit: row.get(17)?,
        year_filter_json: row.get(18)?, ecu_filter: row.get(19)?, source_blob_sha: row.get(20)?,
        research_state: row.get(21)?, vehicle_validation: row.get(22)?,
        auto_request: row.get::<_, i64>(23)? != 0, confidence: row.get(24)?, notes: row.get(25)?,
        source_url: row.get(26)?, license: row.get(27)?, license_url: row.get(28)?,
        decoder_json: row.get(29)?, suggested_metric: row.get(30)?, source_scope: row.get(31)?,
        applicability_specificity: row.get(32)?, min_value: row.get(33)?, max_value: row.get(34)?,
    })
}

fn is_hex_width(value: &str, width: usize) -> bool {
    value.len() == width && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_standard_can_header(value: &str) -> bool {
    is_hex_width(value, 3)
        && u16::from_str_radix(value, 16).is_ok_and(|header| header <= 0x7ff)
}

fn like_filter(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() { "%%".into() } else { format!("%{value}%") }
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|error| format!("Could not open the OEM PID catalog read-only: {error}"))
}

#[derive(Deserialize)]
struct DecoderScale {
    #[serde(rename = "bix")]
    bit_offset: Option<u32>,
    #[serde(rename = "len")]
    bit_length: Option<u32>,
    #[serde(rename = "mul", default = "one")]
    multiplier: f64,
    #[serde(rename = "div", default = "one")]
    divisor: f64,
    #[serde(rename = "add", default)]
    addend: f64,
    #[serde(rename = "min")]
    minimum: Option<f64>,
    #[serde(rename = "max")]
    maximum: Option<f64>,
    unit: Option<String>,
}

fn one() -> f64 { 1.0 }

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("lognook-oem-catalog-{}-{id}.sqlite", std::process::id()));
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(
                "CREATE TABLE source_corpora(source_id TEXT, repository TEXT, blob_sha TEXT, source_url TEXT, license TEXT, license_url TEXT);
                 CREATE TABLE oem_candidates(
                    candidate_id TEXT, source_id TEXT, family TEXT, brand TEXT, model_scope TEXT,
                    service TEXT, identifier TEXT, request_header TEXT, response_header TEXT,
                    frequency_hz REAL, module_type TEXT, category TEXT, name TEXT, signal_source_id TEXT,
                    bit_offset INTEGER, bit_length INTEGER, value_format TEXT, formula TEXT, unit TEXT, year_filter_json TEXT,
                    ecu_filter TEXT, source_blob_sha TEXT, research_state TEXT, vehicle_validation TEXT,
                    auto_request INTEGER, confidence TEXT, notes TEXT, applicability_specificity INTEGER,
                    decoder_json TEXT, min_value REAL, max_value REAL, suggested_metric TEXT, source_scope TEXT);")
            .unwrap();
            connection.execute(
                "INSERT INTO source_corpora VALUES('OBDB_SCION_XB_DEFAULT','OBDb/Scion-xB',?1,'https://source.test/scion','CC BY-SA 4.0','https://source.test/license')",
                [SCION_XB_BLOB_SHA],
            ).unwrap();
            let channels = [
                ("Intake air temperature", 48, 8, "uint", "celsius", "{\"bix\":48,\"len\":8,\"add\":-40,\"unit\":\"celsius\"}", "(raw*(1)/(1))+(-40)"),
                ("Coolant temperature", 64, 8, "uint", "celsius", "{\"bix\":64,\"len\":8,\"add\":-40,\"unit\":\"celsius\"}", "(raw*(1)/(1))+(-40)"),
                ("Engine speed", 72, 16, "uint", "rpm", "{\"bix\":72,\"len\":16,\"div\":4,\"unit\":\"rpm\"}", "(raw*(1)/(4))+(0)"),
                ("Vehicle speed", 88, 8, "uint", "kilometersPerHour", "{\"bix\":88,\"len\":8,\"unit\":\"kilometersPerHour\"}", "(raw*(1)/(1))+(0)"),
                ("Throttle sensor position", 112, 8, "uint", "percent", "{\"bix\":112,\"len\":8,\"mul\":100,\"div\":255,\"unit\":\"percent\"}", "(raw*(100)/(255))+(0)"),
            ];
            for (index, (name, offset, length, value_format, unit, decoder, formula)) in channels.into_iter().enumerate() {
                connection.execute(
                    "INSERT INTO oem_candidates VALUES(?1,'OBDB_SCION_XB_DEFAULT','Toyota','Scion','xB','21','01','7E0','7E8',1.0,'Engine','Engine',?2,'XB_2101',?3,?4,?5,?6,?7,'{}','',?8,'research','not_tested',0,'research','',60,?9,NULL,NULL,'','')",
                    rusqlite::params![format!("scion-{index}"), name, offset, length, value_format, formula, unit, SCION_XB_BLOB_SHA, decoder],
                ).unwrap();
            }
            for index in 0..3 {
                connection.execute(
                    "INSERT INTO oem_candidates VALUES(?1,'OTHER','Toyota','Toyota','Camry','22','1234','7E0','7E8',1.0,'Engine','Engine',?2,'OTHER',0,8,'uint','','rpm','{}','',?3,'research','not_tested',0,'research','',10,'{}',NULL,NULL,'','')",
                    rusqlite::params![format!("other-{index}"), format!("Camry signal {index}"), SCION_XB_BLOB_SHA],
                ).unwrap();
            }
            drop(connection);
            Self(path)
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) { let _ = fs::remove_file(&self.0); }
    }

    #[test]
    fn catalog_search_is_read_only_filtered_and_paged() {
        let db = TestDatabase::new();
        let catalog = OemCatalog::new(&db.0);
        let first = catalog.search("", "", "", None, 0, 2).unwrap();
        assert_eq!(first.total, 8);
        assert_eq!(first.candidates.len(), 2);
        let page_two = catalog.search("", "", "", None, 2, 2).unwrap();
        assert_ne!(first.candidates[0].candidate_id, page_two.candidates[0].candidate_id);

        let filtered = catalog.search("coolant", "Scion", "xB", Some("21"), 0, 50).unwrap();
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.candidates[0].name, "Coolant temperature");
        assert_eq!(filtered.candidates[0].auto_request, false);

        let connection = open_read_only(&db.0).unwrap();
        assert!(connection.execute("UPDATE oem_candidates SET auto_request=1", []).is_err());
    }

    #[test]
    fn scion_probe_definition_requires_exact_source_route_and_channels() {
        let db = TestDatabase::new();
        let spec = OemCatalog::new(&db.0).scion_xb_mode21_probe().unwrap();
        assert_eq!(spec.request_header, "7E0");
        assert_eq!(spec.response_header, "7E8");
        assert_eq!(spec.service, 0x21);
        assert_eq!(spec.identifier, 0x01);
        assert_eq!(spec.channels.len(), 5);
        assert!(spec.channels.iter().any(|channel| channel.name == "Engine speed" && channel.bit_offset == 72));
    }

    #[test]
    fn try_candidate_loads_the_exact_row_and_checks_its_source_revision() {
        let db = TestDatabase::new();
        let spec = OemCatalog::new(&db.0).candidate_for_try("scion-0").unwrap();

        assert_eq!(spec.candidate.candidate_id, "scion-0");
        assert_eq!(spec.candidate.service, "21");
        assert_eq!(spec.candidate.identifier, "01");
        assert_eq!(spec.candidate.request_header, "7E0");
        assert_eq!(spec.candidate.response_header, "7E8");
        assert_eq!(spec.candidate.source_blob_sha, SCION_XB_BLOB_SHA);
        assert_eq!(spec.value_format, "uint");
        assert!(!spec.candidate.auto_request);
    }

    #[test]
    fn try_candidate_accepts_source_backed_two_byte_service_22_did() {
        let db = TestDatabase::new();
        let connection = Connection::open(&db.0).unwrap();
        connection.execute(
            "UPDATE oem_candidates SET service='22',identifier='1234' WHERE candidate_id='scion-0'",
            [],
        ).unwrap();
        drop(connection);

        let spec = OemCatalog::new(&db.0).candidate_for_try("scion-0").unwrap();
        assert_eq!(spec.candidate.service, "22");
        assert_eq!(spec.candidate.identifier, "1234");
    }

    #[test]
    fn try_candidate_rejects_invalid_service_identifier_route_and_provenance() {
        for (column, value) in [
            ("service", "10"),
            ("identifier", "123"),
            ("request_header", "7EX"),
            ("request_header", "800"),
            ("response_header", "7E8X"),
            ("response_header", "800"),
            ("source_blob_sha", "changed"),
        ] {
            let db = TestDatabase::new();
            let connection = Connection::open(&db.0).unwrap();
            connection.execute(
                &format!("UPDATE oem_candidates SET {column}=?1 WHERE candidate_id='scion-0'"),
                [value],
            ).unwrap();
            drop(connection);
            assert!(
                OemCatalog::new(&db.0).candidate_for_try("scion-0").is_err(),
                "candidate should reject {column}={value}"
            );
        }
    }

    #[test]
    fn packaged_catalog_contains_the_scoped_scion_xb_mode21_candidates() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite");
        let catalog = OemCatalog::new(path);
        let page = catalog.search("", "Scion", "xB", Some("21"), 0, 5).unwrap();
        assert_eq!(page.total, 352);
        assert!(page.candidates.iter().all(|candidate| !candidate.auto_request));
        let group = catalog.search("21 01", "Scion", "xB", Some("21"), 0, 50).unwrap();
        assert_eq!(group.total, 18);
        let spec = catalog.scion_xb_mode21_probe().unwrap();
        assert_eq!(spec.source_blob_sha, SCION_XB_BLOB_SHA);
        assert_eq!(spec.channels.len(), 5);
    }
}
