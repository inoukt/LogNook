use super::{
    EcuProfileVariant, IdentityReadState, ProfileMatch, ProfileMatchState,
    VehicleIdentityObservation, VehicleProfile,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
struct FingerprintV1<'a> {
    version: u8,
    vin_digest: &'a str,
    ecu_variants: Vec<FingerprintEcuV1>,
}

#[derive(Serialize)]
struct FingerprintEcuV1 {
    protocol: String,
    request_route: String,
    request_address: Option<String>,
    response_address: String,
    calibration_ids: Vec<String>,
    software_ids: Vec<String>,
}

pub fn canonical_vin_identity(vin: &str) -> Result<(String, String), String> {
    let vin = vin.trim().to_ascii_uppercase();
    if vin.len() != 17
        || !vin
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() && !matches!(byte, b'I' | b'O' | b'Q'))
    {
        return Err("VIN must contain 17 valid letters or digits".into());
    }
    let mut hash = Sha256::new();
    hash.update(b"lognook:vehicle-vin:v1\0");
    hash.update(vin.as_bytes());
    let digest = format!("vin:v1:sha256:{:x}", hash.finalize());
    Ok((digest, format!("{}{}", "•".repeat(13), &vin[13..])))
}

pub fn fingerprint_v1(vin_digest: &str, ecus: &[EcuProfileVariant]) -> Result<String, String> {
    if !vin_digest.starts_with("vin:v1:sha256:") {
        return Err("Fingerprint requires a version 1 VIN digest".into());
    }
    let mut ecu_variants = ecus
        .iter()
        .map(|ecu| FingerprintEcuV1 {
            protocol: normalize(&ecu.protocol),
            request_route: normalize(&ecu.request_route),
            request_address: ecu.request_address.as_deref().map(normalize),
            response_address: normalize(&ecu.response_address),
            calibration_ids: normalize_list(&ecu.calibration_ids),
            software_ids: normalize_list(&ecu.software_ids),
        })
        .collect::<Vec<_>>();
    ecu_variants.sort_by(|a, b| {
        (
            &a.protocol,
            &a.request_route,
            &a.request_address,
            &a.response_address,
        )
            .cmp(&(
                &b.protocol,
                &b.request_route,
                &b.request_address,
                &b.response_address,
            ))
    });
    let bytes = serde_json::to_vec(&FingerprintV1 {
        version: 1,
        vin_digest,
        ecu_variants,
    })
    .map_err(|error| format!("Failed to encode profile fingerprint: {error}"))?;
    let mut hash = Sha256::new();
    hash.update(b"lognook:vehicle-profile-fingerprint:v1\0");
    hash.update(bytes);
    Ok(format!("profile:v1:sha256:{:x}", hash.finalize()))
}

pub fn decoder_revision_v1(decoder: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"lognook:oem-decoder:v1\0");
    hash.update(decoder.as_bytes());
    format!("decoder:v1:sha256:{:x}", hash.finalize())
}

pub fn resolve_profile_match(
    profile: &VehicleProfile,
    observation: &VehicleIdentityObservation,
) -> ProfileMatch {
    let conflict = |reason: &str| ProfileMatch {
        state: ProfileMatchState::Conflict,
        fingerprint_digest: None,
        matched_ecu_variants: vec![],
        review_reasons: vec![reason.into()],
    };
    if profile.scope != observation.scope {
        return conflict("The profile connection scope does not match the current connection");
    }
    if observation.conflicting_vin_observations {
        return conflict("Connected ECUs reported conflicting VIN identities");
    }
    if let (Some(saved), Some(current)) = (&profile.vin_digest, &observation.vin_digest)
        && saved != current
    {
        return conflict("The connected VIN differs from the saved profile");
    }
    let (Some(vin_digest), Some(_current_vin)) = (&profile.vin_digest, &observation.vin_digest)
    else {
        let state = if profile.manual_association
            || observation.vin_status == IdentityReadState::NoResponse
            || observation.vin_status == IdentityReadState::Unsupported
        {
            ProfileMatchState::ReviewRequired
        } else {
            ProfileMatchState::IdentityUnavailable
        };
        return ProfileMatch {
            state,
            fingerprint_digest: None,
            matched_ecu_variants: vec![],
            review_reasons: vec![
                "VIN identity is unavailable; confirm the profile manually".into(),
            ],
        };
    };

    let mut matched = Vec::new();
    let mut review = Vec::new();
    if profile.ecu_variants.is_empty() {
        review.push("The saved profile has no ECU route evidence".into());
    }
    for saved in &profile.ecu_variants {
        let Some(current) = observation
            .ecu_variants
            .iter()
            .find(|current| same_route(saved, current))
        else {
            review.push(format!(
                "Saved ECU {} is missing or its route changed",
                saved.response_address
            ));
            continue;
        };
        if identity_differs(&saved.calibration_ids, &current.calibration_ids) {
            return conflict(&format!(
                "Calibration identity changed for ECU {}",
                saved.response_address
            ));
        }
        if identity_differs(&saved.software_ids, &current.software_ids) {
            return conflict(&format!(
                "Software identity changed for ECU {}",
                saved.response_address
            ));
        }
        if evidence_was_lost(
            &saved.calibration_ids,
            &current.calibration_ids,
            current.calibration_state,
        ) {
            review.push(format!(
                "Previously observed calibration identity is missing for ECU {}",
                saved.response_address
            ));
        }
        if evidence_was_lost(
            &saved.software_ids,
            &current.software_ids,
            current.software_state,
        ) {
            review.push(format!(
                "Previously observed software identity is missing for ECU {}",
                saved.response_address
            ));
        }
        matched.push(saved.response_address.clone());
    }
    let fingerprint_digest = fingerprint_v1(vin_digest, &observation.ecu_variants).ok();
    ProfileMatch {
        state: if review.is_empty() {
            ProfileMatchState::Exact
        } else {
            ProfileMatchState::ReviewRequired
        },
        fingerprint_digest,
        matched_ecu_variants: matched,
        review_reasons: review,
    }
}

fn same_route(a: &EcuProfileVariant, b: &EcuProfileVariant) -> bool {
    normalize(&a.protocol) == normalize(&b.protocol)
        && normalize(&a.request_route) == normalize(&b.request_route)
        && a.request_address.as_deref().map(normalize)
            == b.request_address.as_deref().map(normalize)
        && normalize(&a.response_address) == normalize(&b.response_address)
}

fn identity_differs(saved: &[String], observed: &[String]) -> bool {
    !saved.is_empty() && !observed.is_empty() && normalize_list(saved) != normalize_list(observed)
}

fn evidence_was_lost(saved: &[String], observed: &[String], state: IdentityReadState) -> bool {
    !saved.is_empty() && (observed.is_empty() || state != IdentityReadState::Positive)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn normalize_list(values: &[String]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| normalize(value))
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}
