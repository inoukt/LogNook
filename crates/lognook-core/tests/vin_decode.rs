use lognook_core::vehicle_profiles::vin_decode::{
    VinCheckDigitStatus, VinDecodeError, decode_vin_offline,
};
use sha2::{Digest, Sha256};

#[test]
fn accepts_and_normalizes_a_valid_17_character_vin() {
    let result = decode_vin_offline(" 1g4ah59h45g118341 ").unwrap();

    assert_eq!(result.normalized_vin, "1G4AH59H45G118341");
    assert_eq!(result.check_digit_status, VinCheckDigitStatus::Match);
}

#[test]
fn rejects_each_prohibited_vin_letter() {
    for prohibited in ['I', 'O', 'Q'] {
        let vin = format!("1{prohibited}4AH59H45G118341");
        assert_eq!(
            decode_vin_offline(&vin),
            Err(VinDecodeError::InvalidCharacter {
                character: prohibited,
                position: 2,
            })
        );
    }
}

#[test]
fn applies_the_published_section_565_15_check_digit_vector() {
    let result = decode_vin_offline("1G4AH59H45G118341").unwrap();

    assert_eq!(result.check_digit_status, VinCheckDigitStatus::Match);
    assert_eq!(result.candidate_model_years, [2005, 2035]);
    assert_eq!(result.class_backed_model_year, Some(2005));
}

#[test]
fn warns_without_rejecting_a_check_digit_mismatch_in_known_scope() {
    let result = decode_vin_offline("1G4AH59H55G118341").unwrap();

    assert_eq!(
        result.check_digit_status,
        VinCheckDigitStatus::MismatchWarning
    );
}

#[test]
fn leaves_a_checksum_unassessed_when_wmi_scope_is_unknown() {
    let result = decode_vin_offline("ZZZ00000X5G118341").unwrap();

    assert_eq!(result.check_digit_status, VinCheckDigitStatus::UnknownScope);
    assert!(result.wmi_assignment.is_none());
}

#[test]
fn reports_an_unknown_but_structurally_valid_wmi() {
    let result = decode_vin_offline("ZZZ00000X5G118341").unwrap();

    assert_eq!(result.normalized_vin, "ZZZ00000X5G118341");
    assert!(result.wmi_assignment.is_none());
}

#[test]
fn requires_an_exact_six_character_low_volume_wmi_match() {
    let result = decode_vin_offline(concat!("1F9", "00000", "AAX", "407", "123")).unwrap();
    let assignment = result.wmi_assignment.unwrap();

    assert_eq!(assignment.wmi_key, "1F9407");
    assert_eq!(assignment.manufacturer, "FORD STEEL");
    assert_eq!(assignment.vehicle_type.as_deref(), Some("Trailer"));
}

#[test]
fn does_not_fall_back_to_a_three_character_wmi_for_position_three_nine() {
    let result = decode_vin_offline(concat!("1F9", "00000", "AAX", "999", "123")).unwrap();

    assert!(result.wmi_assignment.is_none());
    assert_eq!(result.check_digit_status, VinCheckDigitStatus::UnknownScope);
}

#[test]
fn leaves_the_30_year_model_year_cycle_unresolved_without_a_supporting_class() {
    let result = decode_vin_offline(concat!("1FM", "00000", "XB", "0", "000001")).unwrap();

    assert_eq!(result.year_code, 'B');
    assert_eq!(result.candidate_model_years, [1981, 2011]);
    assert_eq!(result.class_backed_model_year, None);
}

#[test]
fn resolves_a_passenger_car_year_cycle_from_position_seven() {
    let result = decode_vin_offline(concat!("1G4", "000A0", "X", "B", "0", "000001")).unwrap();

    assert_eq!(result.candidate_model_years, [1981, 2011]);
    assert_eq!(result.class_backed_model_year, Some(2011));
}

#[test]
fn reports_the_hash_of_the_bundled_wmi_data() {
    let result = decode_vin_offline("1G4AH59H45G118341").unwrap();
    let csv_hash = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../data/vin_wmi.csv"))
    );
    let metadata: serde_json::Value =
        serde_json::from_str(include_str!("../data/vin_wmi_sources.json")).unwrap();

    assert_eq!(result.wmi_dataset_sha256, csv_hash);
    assert_eq!(
        metadata["dataset_sha256"].as_str(),
        Some(result.wmi_dataset_sha256.as_str())
    );
    assert_eq!(metadata["record_count"].as_u64(), Some(79));
}

#[test]
fn rejects_a_missing_vin() {
    assert_eq!(
        decode_vin_offline(""),
        Err(VinDecodeError::InvalidLength { actual: 0 })
    );
}
