use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, sync::OnceLock};

const VIN_WMI_CSV: &[u8] = include_bytes!("../../data/vin_wmi.csv");
const VIN_DECODER_REVISION: &str = "vin-decode-offline-v1";
const US_CHECK_DIGIT_SCOPE: &str = "us_49_cfr_565";
const CHECK_DIGIT_WEIGHTS: [u32; 17] = [8, 7, 6, 5, 4, 3, 2, 10, 0, 9, 8, 7, 6, 5, 4, 3, 2];
const YEAR_CODES: &[u8] = b"ABCDEFGHJKLMNPRSTVWXY123456789";

static WMI_ROWS: OnceLock<Vec<WmiRow>> = OnceLock::new();
static WMI_DATASET_SHA256: OnceLock<String> = OnceLock::new();

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VinCheckDigitStatus {
    Match,
    MismatchWarning,
    UnknownScope,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct WmiAssignment {
    pub wmi_key: String,
    pub manufacturer: String,
    pub manufacturer_assignment_country: Option<String>,
    pub vehicle_type: Option<String>,
    /// Source-backed class scope used by the included year-cycle rule, when known.
    pub class_scope: Option<String>,
    pub source_id: String,
    pub source_revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct VinDecodeResult {
    pub normalized_vin: String,
    pub check_digit_status: VinCheckDigitStatus,
    pub check_digit_scheme: Option<String>,
    pub year_code: char,
    pub candidate_model_years: Vec<u16>,
    pub class_backed_model_year: Option<u16>,
    pub wmi_assignment: Option<WmiAssignment>,
    pub decoder_revision: String,
    pub wmi_dataset_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VinDecodeError {
    InvalidLength { actual: usize },
    InvalidCharacter { character: char, position: usize },
}

impl fmt::Display for VinDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { actual } => {
                write!(
                    formatter,
                    "VIN must contain 17 characters (received {actual})"
                )
            }
            Self::InvalidCharacter {
                character,
                position,
            } => write!(
                formatter,
                "VIN contains invalid character '{character}' at position {position}"
            ),
        }
    }
}

impl std::error::Error for VinDecodeError {}

/// Decodes VIN structure and the bundled WMI assignments without network access.
pub fn decode_vin_offline(vin: &str) -> Result<VinDecodeResult, VinDecodeError> {
    let normalized_vin = vin.trim().to_ascii_uppercase();
    let character_count = normalized_vin.chars().count();
    if character_count != 17 {
        return Err(VinDecodeError::InvalidLength {
            actual: character_count,
        });
    }

    for (index, character) in normalized_vin.chars().enumerate() {
        if !character.is_ascii_alphanumeric() || matches!(character, 'I' | 'O' | 'Q') {
            return Err(VinDecodeError::InvalidCharacter {
                character,
                position: index + 1,
            });
        }
    }

    let bytes = normalized_vin.as_bytes();
    let wmi_key = if bytes[2] == b'9' {
        format!("{}{}", &normalized_vin[..3], &normalized_vin[11..14])
    } else {
        normalized_vin[..3].to_owned()
    };
    let row = wmi_rows().iter().find(|row| row.key == wmi_key);
    let wmi_assignment = row.map(|row| WmiAssignment {
        wmi_key,
        manufacturer: row.manufacturer.clone(),
        manufacturer_assignment_country: row.manufacturer_assignment_country.clone(),
        vehicle_type: row.vehicle_type.clone(),
        class_scope: row.class_scope.clone(),
        source_id: row.source_id.clone(),
        source_revision: row.source_revision.clone(),
    });

    let check_digit_scheme = row
        .filter(|row| row.check_digit_scope == US_CHECK_DIGIT_SCOPE)
        .map(|_| "49 CFR §565.15".to_owned());
    let check_digit_status = match check_digit_scheme.as_deref() {
        Some(_) if expected_check_digit(&normalized_vin) == bytes[8] as char => {
            VinCheckDigitStatus::Match
        }
        Some(_) => VinCheckDigitStatus::MismatchWarning,
        None => VinCheckDigitStatus::UnknownScope,
    };

    let year_code = bytes[9] as char;
    let candidate_model_years = model_year_candidates(year_code);
    let class_backed_model_year =
        row.and_then(|row| class_backed_model_year(row, bytes[6], &candidate_model_years));

    Ok(VinDecodeResult {
        normalized_vin,
        check_digit_status,
        check_digit_scheme,
        year_code,
        candidate_model_years,
        class_backed_model_year,
        wmi_assignment,
        decoder_revision: VIN_DECODER_REVISION.to_owned(),
        wmi_dataset_sha256: wmi_dataset_sha256().to_owned(),
    })
}

#[derive(Deserialize)]
struct WmiRow {
    key: String,
    manufacturer: String,
    manufacturer_assignment_country: Option<String>,
    vehicle_type: Option<String>,
    class_scope: Option<String>,
    check_digit_scope: String,
    source_id: String,
    source_revision: String,
}

fn wmi_rows() -> &'static [WmiRow] {
    WMI_ROWS
        .get_or_init(|| {
            csv::Reader::from_reader(VIN_WMI_CSV)
                .deserialize()
                .map(|row| row.expect("bundled VIN WMI CSV must be valid"))
                .collect()
        })
        .as_slice()
}

fn wmi_dataset_sha256() -> &'static str {
    WMI_DATASET_SHA256
        .get_or_init(|| format!("{:x}", Sha256::digest(VIN_WMI_CSV)))
        .as_str()
}

fn model_year_candidates(code: char) -> Vec<u16> {
    let Some(index) = YEAR_CODES
        .iter()
        .position(|candidate| *candidate == code as u8)
    else {
        return Vec::new();
    };
    let index = index as u16;
    vec![1980 + index, 2010 + index]
}

fn class_backed_model_year(row: &WmiRow, position_seven: u8, candidates: &[u16]) -> Option<u16> {
    if row.check_digit_scope != US_CHECK_DIGIT_SCOPE
        || row.class_scope.as_deref() != Some("passenger_car")
    {
        return None;
    }
    match (position_seven.is_ascii_digit(), candidates) {
        (true, [first, _]) => Some(*first),
        (false, [_, second]) => Some(*second),
        _ => None,
    }
}

fn expected_check_digit(vin: &str) -> char {
    let remainder = vin
        .bytes()
        .zip(CHECK_DIGIT_WEIGHTS)
        .map(|(character, weight)| transliterated_value(character) * weight)
        .sum::<u32>()
        % 11;
    if remainder == 10 {
        'X'
    } else {
        char::from_digit(remainder, 10).expect("check digit remainder is one decimal digit")
    }
}

fn transliterated_value(character: u8) -> u32 {
    match character {
        b'0'..=b'9' => u32::from(character - b'0'),
        b'A' => 1,
        b'B' => 2,
        b'C' => 3,
        b'D' => 4,
        b'E' => 5,
        b'F' => 6,
        b'G' => 7,
        b'H' => 8,
        b'J' => 1,
        b'K' => 2,
        b'L' => 3,
        b'M' => 4,
        b'N' => 5,
        b'P' => 7,
        b'R' => 9,
        b'S' => 2,
        b'T' => 3,
        b'U' => 4,
        b'V' => 5,
        b'W' => 6,
        b'X' => 7,
        b'Y' => 8,
        b'Z' => 9,
        _ => unreachable!("VIN characters are validated before checksum calculation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_scope_foreign_passenger_car_keeps_year_ambiguous() {
        let row = WmiRow {
            key: "WVW".into(),
            manufacturer: "VOLKSWAGEN AG".into(),
            manufacturer_assignment_country: Some("GERMANY".into()),
            vehicle_type: Some("Passenger Car".into()),
            class_scope: Some("passenger_car".into()),
            check_digit_scope: "unknown".into(),
            source_id: "test".into(),
            source_revision: "test".into(),
        };

        assert_eq!(class_backed_model_year(&row, b'A', &[1980, 2010]), None);
    }
}
