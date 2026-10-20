pub const MAX_SSM2_PAYLOAD_BYTES: usize = u8::MAX as usize;
pub const MAX_SSM2_ADDRESSES_PER_REQUEST: usize = (u8::MAX as usize - 2) / 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ssm2Frame {
    pub destination: u8,
    pub source: u8,
    pub payload: Vec<u8>,
}

pub fn build_ssm2_frame(destination: u8, source: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.is_empty() || payload.len() > MAX_SSM2_PAYLOAD_BYTES {
        return Err("SSM2 payload must contain 1-255 bytes".into());
    }
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.extend_from_slice(&[0x80, destination, source, payload.len() as u8]);
    frame.extend_from_slice(payload);
    frame.push(checksum(&frame));
    Ok(frame)
}

pub fn build_ecu_init(destination: u8, source: u8) -> Vec<u8> {
    build_ssm2_frame(destination, source, &[0xbf]).expect("fixed SSM2 init payload is valid")
}

pub fn build_read_addresses(
    destination: u8,
    source: u8,
    addresses: &[u32],
) -> Result<Vec<u8>, String> {
    if addresses.is_empty() || addresses.len() > MAX_SSM2_ADDRESSES_PER_REQUEST {
        return Err(format!(
            "SSM2 multi-address reads accept 1-{} addresses",
            MAX_SSM2_ADDRESSES_PER_REQUEST
        ));
    }
    let mut payload = Vec::with_capacity(2 + addresses.len() * 3);
    payload.extend_from_slice(&[0xa8, 0x00]);
    for address in addresses {
        if *address > 0x00ff_ffff {
            return Err(format!("SSM2 address 0x{address:X} exceeds 24 bits"));
        }
        payload.extend_from_slice(&address.to_be_bytes()[1..]);
    }
    build_ssm2_frame(destination, source, &payload)
}

pub fn parse_ssm2_frame(
    frame: &[u8],
    expected_ecu: u8,
    expected_tool: u8,
) -> Result<Ssm2Frame, String> {
    if frame.len() < 5 || frame[0] != 0x80 {
        return Err("SSM2 frame has no 0x80 header or is too short".into());
    }
    if frame[1] != expected_tool || frame[2] != expected_ecu {
        return Err(format!(
            "SSM2 reply route was {:02X}>{:02X}, expected {:02X}>{:02X}",
            frame[1], frame[2], expected_tool, expected_ecu
        ));
    }
    let expected_len = 4 + usize::from(frame[3]) + 1;
    if frame.len() != expected_len {
        return Err(format!(
            "SSM2 frame length mismatch: declared {}, received {}",
            frame[3],
            frame.len().saturating_sub(5)
        ));
    }
    if checksum(&frame[..frame.len() - 1]) != frame[frame.len() - 1] {
        return Err("SSM2 frame checksum is invalid".into());
    }
    Ok(Ssm2Frame {
        destination: frame[1],
        source: frame[2],
        payload: frame[4..frame.len() - 1].to_vec(),
    })
}

/// Extract one ECU response from a byte stream, discarding the optional exact adapter echo.
/// The stream is retained when it contains only a partial header or frame.
pub fn take_ssm2_reply(
    stream: &mut Vec<u8>,
    request_frame: &[u8],
    ecu_address: u8,
    tool_address: u8,
) -> Result<Option<Vec<u8>>, String> {
    loop {
        let Some(header) = stream.iter().position(|byte| *byte == 0x80) else {
            stream.clear();
            return Ok(None);
        };
        if header > 0 {
            stream.drain(..header);
        }
        if stream.len() < 4 {
            return Ok(None);
        }
        let frame_len = usize::from(stream[3]) + 5;
        if stream.len() < frame_len {
            return Ok(None);
        }
        let frame = stream[..frame_len].to_vec();
        if frame == request_frame {
            stream.drain(..frame_len);
            continue;
        }
        if frame[1] != tool_address || frame[2] != ecu_address {
            return Err(format!(
                "Unexpected SSM2 K-Line frame route {:02X}>{:02X}",
                frame[1], frame[2]
            ));
        }
        parse_ssm2_frame(&frame, ecu_address, tool_address)?;
        stream.drain(..frame_len);
        return Ok(Some(frame));
    }
}

pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SSM2_ADDRESSES_PER_REQUEST, build_ecu_init, build_read_addresses, build_ssm2_frame,
        parse_ssm2_frame, take_ssm2_reply,
    };

    #[test]
    fn init_frame_matches_ssm2_packet_fixture() {
        assert_eq!(
            build_ssm2_frame(0x10, 0xf0, &[0xbf]).unwrap(),
            [0x80, 0x10, 0xf0, 0x01, 0xbf, 0x40]
        );
    }

    #[test]
    fn multi_address_read_matches_ssm2_packet_fixture() {
        assert_eq!(
            build_read_addresses(0x10, 0xf0, &[0x000008, 0x00001c]).unwrap(),
            [
                0x80, 0x10, 0xf0, 0x08, 0xa8, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x1c, 0x54
            ]
        );
    }

    #[test]
    fn reply_parser_checks_route_length_and_checksum() {
        let reply = [0x80, 0xf0, 0x10, 0x03, 0xe8, 0x7d, 0xb1, 0x99];
        let parsed = parse_ssm2_frame(&reply, 0x10, 0xf0).unwrap();
        assert_eq!(parsed.payload, [0xe8, 0x7d, 0xb1]);

        let mut bad = reply;
        *bad.last_mut().unwrap() ^= 1;
        assert!(parse_ssm2_frame(&bad, 0x10, 0xf0).is_err());
        assert!(parse_ssm2_frame(&reply, 0x18, 0xf0).is_err());
    }

    #[test]
    fn requests_reject_empty_oversized_and_out_of_range_addresses() {
        assert_eq!(build_ecu_init(0x10, 0xf0)[4], 0xbf);
        assert!(build_read_addresses(0x10, 0xf0, &[]).is_err());
        assert!(build_read_addresses(0x10, 0xf0, &[0x0100_0000]).is_err());
        assert_eq!(
            build_read_addresses(0x10, 0xf0, &vec![0; MAX_SSM2_ADDRESSES_PER_REQUEST]).unwrap()[3],
            254
        );
    }

    #[test]
    fn stream_parser_skips_adapter_echo_and_reassembles_a_split_reply() {
        let request = build_read_addresses(0x10, 0xf0, &[0x000008]).unwrap();
        let reply = build_ssm2_frame(0xf0, 0x10, &[0xe8, 0x7d]).unwrap();
        let mut stream = request[..3].to_vec();
        assert!(
            take_ssm2_reply(&mut stream, &request, 0x10, 0xf0)
                .unwrap()
                .is_none()
        );
        stream.extend_from_slice(&request[3..]);
        stream.extend_from_slice(&reply[..4]);
        assert!(
            take_ssm2_reply(&mut stream, &request, 0x10, 0xf0)
                .unwrap()
                .is_none()
        );
        stream.extend_from_slice(&reply[4..]);
        assert_eq!(
            take_ssm2_reply(&mut stream, &request, 0x10, 0xf0).unwrap(),
            Some(reply)
        );
        assert!(stream.is_empty());
    }
}
