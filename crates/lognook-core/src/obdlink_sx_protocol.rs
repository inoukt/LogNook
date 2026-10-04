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

pub(crate) fn mode01_pid_data_width(pid: u8) -> Option<usize> {
    match pid {
        0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 | 0xC0 | 0xE0 => Some(4),
        0x03 | 0x0C | 0x1F | 0x21 | 0x23 | 0x31 | 0x32 | 0x3C | 0x3D | 0x42 | 0x43 | 0x44 => {
            Some(2)
        }
        0x04 | 0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x0B | 0x0D | 0x0E | 0x0F | 0x11 | 0x13
        | 0x1C | 0x2E | 0x2F | 0x30 | 0x33 | 0x45 | 0x46 | 0x47 | 0x49 | 0x4A | 0x4C => Some(1),
        _ => None,
    }
}

pub(crate) fn parse_mode01_responses(
    text: &str,
    service: u8,
    pid: u8,
) -> Result<Vec<Mode01Response>, String> {
    let positive_service = service.wrapping_add(0x40);
    let mut responses = Vec::new();

    if let Some(messages) = parse_isotp_messages(text)? {
        for (ecu_id, payload) in messages {
            responses.extend(parse_mode01_payload(
                &payload,
                Some(ecu_id),
                positive_service,
                pid,
            )?);
        }
        if responses.is_empty() {
            return Err(format!("No response for Mode {service:02X} PID {pid:02X}"));
        }
        return Ok(responses);
    }

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

fn parse_mode01_payload(
    payload: &[u8],
    ecu_id: Option<String>,
    positive_service: u8,
    requested_pid: u8,
) -> Result<Vec<Mode01Response>, String> {
    if payload.first() != Some(&positive_service) {
        return Ok(Vec::new());
    }

    let mut responses = Vec::new();
    let mut offset = 1;
    while offset < payload.len() {
        let pid = payload[offset];
        let width = mode01_pid_data_width(pid)
            .ok_or_else(|| format!("Unknown Mode 01 PID {pid:02X} in combined response"))?;
        let data_start = offset + 1;
        let data_end = data_start + width;
        if data_end > payload.len() {
            return Err(format!("Mode 01 PID {pid:02X} response is truncated"));
        }
        if pid == requested_pid {
            let data = payload[data_start..data_end].to_vec();
            let mut raw_bytes = vec![positive_service, pid];
            raw_bytes.extend_from_slice(&data);
            responses.push(Mode01Response {
                ecu_id: ecu_id.clone(),
                raw_bytes,
                data,
            });
        }
        offset = data_end;
    }
    Ok(responses)
}

fn parse_isotp_messages(text: &str) -> Result<Option<Vec<(String, Vec<u8>)>>, String> {
    let mut frames = Vec::new();
    for line in text.split(['\r', '\n']) {
        let mut ecu_id = None;
        let mut frame = Vec::new();
        for token in line.split_whitespace() {
            let token = token.trim_matches('>');
            if matches!(token.len(), 3 | 8) && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                if let Some(previous_id) = ecu_id.replace(token.to_ascii_uppercase())
                    && !frame.is_empty()
                {
                    frames.push((previous_id, std::mem::take(&mut frame)));
                }
                continue;
            }
            if ecu_id.is_some()
                && let Some(bytes) = parse_hex_token(token)
            {
                frame.extend(bytes);
            }
        }
        if let Some(ecu_id) = ecu_id
            && !frame.is_empty()
        {
            frames.push((ecu_id, frame));
        }
    }
    if frames.is_empty() {
        return Ok(None);
    }

    let mut messages = Vec::new();
    let mut pending: Option<(String, usize, u8, Vec<u8>)> = None;
    for (ecu_id, frame) in frames {
        let pci = frame[0];
        match pci >> 4 {
            0 => {
                if pending.is_some() {
                    return Err("ISO-TP single frame interrupted an incomplete response".into());
                }
                let length = (pci & 0x0F) as usize;
                if length == 0 || frame.len() < length + 1 {
                    return Err("ISO-TP single frame is truncated".into());
                }
                messages.push((ecu_id, frame[1..=length].to_vec()));
            }
            1 => {
                if pending.is_some() || frame.len() < 2 {
                    return Err("Unexpected ISO-TP first frame".into());
                }
                let length = (((pci & 0x0F) as usize) << 8) | frame[1] as usize;
                if length <= frame.len() - 2 {
                    messages.push((ecu_id, frame[2..(2 + length)].to_vec()));
                } else {
                    pending = Some((ecu_id, length, 1, frame[2..].to_vec()));
                }
            }
            2 => {
                let Some((expected_ecu, length, sequence, payload)) = pending.as_mut() else {
                    return Err("ISO-TP consecutive frame has no first frame".into());
                };
                if *expected_ecu != ecu_id || (pci & 0x0F) != *sequence {
                    return Err("ISO-TP response ECU or sequence changed".into());
                }
                *sequence = (*sequence + 1) & 0x0F;
                let remaining = *length - payload.len();
                payload.extend(frame.iter().skip(1).take(remaining).copied());
                if payload.len() == *length {
                    let (ecu_id, _, _, payload) = pending.take().expect("pending frame exists");
                    messages.push((ecu_id, payload));
                }
            }
            _ => return Err("Unexpected ISO-TP frame type in OBD response".into()),
        }
    }
    if pending.is_some() {
        return Err("ISO-TP response ended before all declared bytes arrived".into());
    }
    Ok(Some(messages))
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

    let width = mode01_pid_data_width(signal.pid)
        .ok_or_else(|| format!("Unsupported read-only Mode 01 PID {:02X}", signal.pid))?;
    if response.data.len() < width {
        return Err(format!(
            "PID {:02X} response requires {width} data byte(s)",
            signal.pid
        ));
    }
    let first = response.data[0];
    let word = || u16::from_be_bytes([response.data[0], response.data[1]]);
    let value = match signal.pid {
        0x03 | 0x13 | 0x1C => match signal.pid {
            0x03 => word() as f64,
            _ => first as f64,
        },
        0x04 | 0x11 | 0x2E | 0x2F | 0x45 | 0x47 | 0x49 | 0x4A | 0x4C => {
            first as f64 * 100.0 / 255.0
        }
        0x05 | 0x0F | 0x46 => first as f64 - 40.0,
        0x06..=0x09 => (first as f64 - 128.0) * 100.0 / 128.0,
        0x0B | 0x0D | 0x30 | 0x33 => first as f64,
        0x0C => word() as f64 / 4.0,
        0x0E => first as f64 / 2.0 - 64.0,
        0x1F | 0x21 | 0x31 => word() as f64,
        0x23 => word() as f64 * 10.0,
        0x32 => i16::from_be_bytes([response.data[0], response.data[1]]) as f64 / 4.0,
        0x3C | 0x3D => word() as f64 / 10.0 - 40.0,
        0x42 => word() as f64 / 1_000.0,
        0x43 => word() as f64 * 100.0 / 255.0,
        0x44 => word() as f64 * 2.0 / 65_536.0,
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
