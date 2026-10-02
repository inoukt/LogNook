use crate::model::SignalDefinition;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mode01Response {
    pub(crate) ecu_id: Option<String>,
    pub(crate) raw_bytes: Vec<u8>,
    pub(crate) data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SupportedPidRange {
    pub(crate) pids: Vec<u8>,
    pub(crate) has_more: bool,
    pub(crate) ecu_id: Option<String>,
}

pub(crate) fn parse_mode01_responses(
    text: &str,
    service: u8,
    pid: u8,
) -> Result<Vec<Mode01Response>, String> {
    let positive_service = service.wrapping_add(0x40);
    let mut responses = Vec::new();

    for line in text.split(['\r', '\n']) {
        let mut ecu_id = None;
        let mut bytes = Vec::new();
        for token in line.split_whitespace() {
            let token = token.trim_matches('>');
            if (token.len() == 3 || token.len() == 8)
                && token.bytes().all(|byte| byte.is_ascii_hexdigit())
                && !token.starts_with(&format!("{positive_service:02X}"))
            {
                append_response(&bytes, &ecu_id, positive_service, pid, &mut responses);
                bytes.clear();
                ecu_id = Some(token.to_ascii_uppercase());
                continue;
            }
            if let Some(parsed) = parse_hex_token(token) {
                bytes.extend(parsed);
            }
        }
        append_response(&bytes, &ecu_id, positive_service, pid, &mut responses);
    }

    if responses.is_empty() {
        return Err(format!("No response for Mode {service:02X} PID {pid:02X}"));
    }
    Ok(responses)
}

fn append_response(
    bytes: &[u8],
    ecu_id: &Option<String>,
    positive_service: u8,
    pid: u8,
    responses: &mut Vec<Mode01Response>,
) {
    for index in 0..bytes.len().saturating_sub(1) {
        if bytes[index] == positive_service && bytes[index + 1] == pid {
            responses.push(Mode01Response {
                ecu_id: ecu_id.clone(),
                raw_bytes: bytes[index..].to_vec(),
                data: bytes[index + 2..].to_vec(),
            });
            break;
        }
    }
}

pub(crate) fn parse_supported_pids(
    text: &str,
    range_start: u8,
) -> Result<SupportedPidRange, String> {
    if range_start > 0xE0 || !range_start.is_multiple_of(0x20) {
        return Err("Supported PID range must start at 00, 20, 40, 60, 80, A0, C0, or E0".into());
    }
    let mut responses = parse_mode01_responses(text, 0x01, range_start)?;
    if responses.len() != 1 {
        return Err("Multiple ECUs reported supported PIDs".into());
    }
    let response = responses.pop().expect("one response was checked");
    if response.data.len() < 4 {
        return Err(format!(
            "Mode 01 PID {range_start:02X} response must include four bitmap bytes"
        ));
    }

    let pids = (0..31)
        .filter_map(|bit| {
            let mask = 1 << (7 - bit % 8);
            (response.data[bit / 8] & mask != 0).then_some(range_start + bit as u8 + 1)
        })
        .collect();
    Ok(SupportedPidRange {
        pids,
        has_more: response.data[3] & 0x01 != 0,
        ecu_id: response.ecu_id,
    })
}

pub(crate) fn decode_signal(
    signal: &SignalDefinition,
    response: &Mode01Response,
) -> Result<f64, String> {
    if signal.service != 0x01
        || response.raw_bytes.len() < 3
        || response.raw_bytes[0] != 0x41
        || response.raw_bytes[1] != signal.pid
    {
        return Err("Mode 01 response does not match the requested signal".into());
    }

    let first = *response
        .data
        .first()
        .ok_or_else(|| "Mode 01 response contains no signal data".to_owned())?;
    let value = match signal.pid {
        0x0C => {
            let second = *response
                .data
                .get(1)
                .ok_or_else(|| "RPM response requires two data bytes".to_owned())?;
            (u16::from_be_bytes([first, second]) as f64) / 4.0
        }
        0x05 | 0x0F => first as f64 - 40.0,
        0x11 => first as f64 * 100.0 / 255.0,
        0x0D => first as f64,
        _ => {
            return Err(format!(
                "Unsupported read-only Mode 01 PID {:02X}",
                signal.pid
            ));
        }
    };

    if !value.is_finite() || !(signal.min..=signal.max).contains(&value) {
        return Err(format!(
            "PID {:02X} value is outside its declared range",
            signal.pid
        ));
    }
    Ok(value)
}

fn parse_hex_token(token: &str) -> Option<Vec<u8>> {
    if token.is_empty()
        || !token.len().is_multiple_of(2)
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    token
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digits = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(digits, 16).ok()
        })
        .collect()
}

#[cfg(test)]
#[path = "obdlink_sx_protocol_tests.rs"]
mod tests;
