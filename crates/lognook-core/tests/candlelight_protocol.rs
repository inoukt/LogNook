use lognook_core::{CanDirection, CanFrame, candlelight::*};

fn constants() -> GsUsbTimingConstants {
    // bxCAN, 48 MHz, advertised classic CAN timing limits.
    let words: [u32; 10] = [1, 48_000_000, 1, 16, 1, 8, 4, 1, 1024, 1];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    GsUsbTimingConstants::decode(&bytes).unwrap()
}

fn fixture(echo: u32, id: u32, dlc: u8) -> Vec<u8> {
    let mut bytes = vec![0; 20];
    bytes[..4].copy_from_slice(&echo.to_le_bytes());
    bytes[4..8].copy_from_slice(&id.to_le_bytes());
    bytes[8] = dlc;
    bytes[12..15].copy_from_slice(&[0xAA, 0xBB, 0xCC]);
    bytes
}

fn frame() -> CanFrame {
    CanFrame {
        elapsed_ns: 7,
        wall_time_unix_ns: 8,
        can_id: 0x18DAF110,
        extended: true,
        direction: CanDirection::Tx,
        data: vec![0xAA, 0xBB],
        flags: 0,
    }
}

#[test]
fn vendor_interface_controls_keep_channel_in_value_and_interface_in_index() {
    for (request, id) in [
        (GsUsbRequest::HostFormat, 0),
        (GsUsbRequest::BitTiming, 1),
        (GsUsbRequest::Mode, 2),
        (GsUsbRequest::BusError, 3),
        (GsUsbRequest::TimingConstants, 4),
        (GsUsbRequest::DeviceConfig, 5),
    ] {
        let setup = control_setup(request, 2, 3);
        assert_eq!(setup.request, id);
        assert_eq!(
            setup.value,
            if matches!(
                request,
                GsUsbRequest::HostFormat | GsUsbRequest::DeviceConfig
            ) {
                1
            } else {
                3
            }
        );
        assert_eq!(setup.index, 2);
        assert!(setup.vendor && setup.interface_recipient);
        let input = setup.input(40);
        assert_eq!(input.control_type, nusb::transfer::ControlType::Vendor);
        assert_eq!(input.recipient, nusb::transfer::Recipient::Interface);
        assert_eq!(
            (input.request, input.value, input.index, input.length),
            (id, setup.value, 2, 40)
        );
        let output = setup.output(&[1, 2]);
        assert_eq!(output.control_type, nusb::transfer::ControlType::Vendor);
        assert_eq!(output.recipient, nusb::transfer::Recipient::Interface);
        assert_eq!(
            (output.request, output.value, output.index, output.data),
            (id, setup.value, 2, &[1, 2][..])
        );
    }
}

#[test]
fn endpoints_come_from_interface_descriptors_not_firmware_defaults() {
    let descriptors = [
        EndpointDescriptor {
            address: 0x81,
            transfer_type: 3,
            max_packet_size: 64,
        },
        EndpointDescriptor {
            address: 0x85,
            transfer_type: 2,
            max_packet_size: 64,
        },
        EndpointDescriptor {
            address: 0x04,
            transfer_type: 2,
            max_packet_size: 64,
        },
    ];
    assert_eq!(
        select_bulk_endpoints(&descriptors).unwrap(),
        BulkEndpoints {
            input: 0x85,
            output: 0x04
        }
    );
    assert!(select_bulk_endpoints(&descriptors[..2]).is_err());
    assert!(select_bulk_endpoints(&[descriptors[1], descriptors[1], descriptors[2]]).is_err());
    assert!(
        select_bulk_endpoints(&[
            EndpointDescriptor {
                address: 0x80,
                transfer_type: 2,
                max_packet_size: 64
            },
            descriptors[2]
        ])
        .is_err()
    );
}

#[test]
fn usb_ids_are_detection_hints_and_identity_requires_valid_control_replies() {
    assert!(is_candlelight_hint(0x1D50, 0x606F));
    assert!(is_candlelight_hint(0x1209, 0x2323));
    assert!(!is_candlelight_hint(0x1209, 0x9999));
    let config = GsUsbDeviceConfig::decode(&[0, 0, 0, 1, 0x34, 0x12, 0, 0, 2, 0, 0, 0]).unwrap();
    assert_eq!(config.channel_count, 2);
    assert_eq!(config.software_version, 0x1234);
    assert_eq!(config.hardware_version, 2);
    assert!(GsUsbDeviceConfig::decode(&[0; 11]).is_err());
    assert!(GsUsbTimingConstants::decode(&[0; 40]).is_err());
    assert!(GsUsbTimingConstants::decode(&[0; 39]).is_err());
}

#[test]
fn bitrate_uses_advertised_clock_and_bounds_and_encodes_little_endian() {
    let c = constants();
    let timing = c.for_bitrate(500_000).unwrap();
    assert_eq!(
        c.clock_hz / (timing.brp * (1 + timing.prop_seg + timing.phase_seg1 + timing.phase_seg2)),
        500_000
    );
    assert_eq!(
        timing.encode(),
        [
            timing.prop_seg,
            timing.phase_seg1,
            timing.phase_seg2,
            timing.sjw,
            timing.brp
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>()
    );
    assert!(c.for_bitrate(0).is_err());
    assert!(c.for_bitrate(123_457).is_err());
    assert!(c.for_bitrate(100_000_000).is_err());
}

#[test]
fn passive_mode_requires_advertised_listen_only_and_never_falls_back() {
    assert_eq!(mode_bytes(1, true).unwrap(), [1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(mode_bytes(0, false).unwrap(), [1, 0, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(
        mode_bytes(0, true),
        Err(CandlelightError::ListenOnlyUnavailable)
    ));
    assert_eq!(reset_mode_bytes(), [0; 8]);
    assert_eq!(
        mode_bytes(1 | (1 << 12), true).unwrap(),
        [1, 0, 0, 0, 1, 0x10, 0, 0]
    );
}

#[test]
fn transmit_header_is_little_endian_with_assigned_echo_and_extended_flag() {
    let bytes = encode_frame(&frame(), 0x12345678, 2).unwrap();
    assert_eq!(
        &bytes[..12],
        &[0x78, 0x56, 0x34, 0x12, 0x10, 0xF1, 0xDA, 0x98, 2, 2, 0, 0]
    );
    assert_eq!(&bytes[12..], &[0xAA, 0xBB, 0, 0, 0, 0, 0, 0]);
    assert!(encode_frame(&frame(), RX_ECHO_ID, 0).is_err());
    let mut f = frame();
    f.extended = false;
    assert!(encode_frame(&f, 0, 0).is_err());
    f.can_id = 0x7FF;
    f.data = vec![0; 9];
    assert!(encode_frame(&f, 0, 0).is_err());
}

#[test]
fn receives_standard_and_extended_identifiers_and_preserves_overflow() {
    let mut bytes = fixture(RX_ECHO_ID, 0x321, 3);
    bytes[9] = 2;
    bytes[10] = 1;
    let decoded = decode_frame(&bytes, 11, 12).unwrap();
    assert_eq!(decoded.channel, 2);
    assert_eq!(decoded.echo_id, RX_ECHO_ID);
    assert_eq!(decoded.frame.can_id, 0x321);
    assert!(!decoded.frame.extended);
    assert_eq!(decoded.frame.direction, CanDirection::Rx);
    assert_eq!(decoded.frame.data, [0xAA, 0xBB, 0xCC]);
    assert_eq!(decoded.frame.flags, 1);
    assert_eq!(decoded.frame.elapsed_ns, 11);
    assert_eq!(decoded.frame.wall_time_unix_ns, 12);
    let extended = decode_frame(&fixture(RX_ECHO_ID, 0x98DAF110, 2), 0, 0).unwrap();
    assert_eq!(extended.frame.can_id, 0x18DAF110);
    assert!(extended.frame.extended);
}

#[test]
fn transmit_echo_is_not_an_rx_measurement_and_header_only_echo_is_valid() {
    let decoded = decode_frame(&fixture(3, 0x123, 3), 0, 0).unwrap();
    assert_eq!(decoded.frame.direction, CanDirection::Tx);
    assert_eq!(decoded.echo_id, 3);
    // Linux accepts a TX echo header without payload; owner restores original TX bytes.
    let short = fixture(3, 0x123, 3)[..12].to_vec();
    assert_eq!(
        decode_frame(&short, 0, 0).unwrap().frame.direction,
        CanDirection::Tx
    );
}

#[test]
fn malformed_frames_and_unsupported_fd_or_remote_frames_fail_explicitly() {
    assert!(decode_frame(&[0; 11], 0, 0).is_err());
    assert!(decode_frame(&fixture(RX_ECHO_ID, 0x123, 9), 0, 0).is_err());
    assert!(decode_frame(&fixture(RX_ECHO_ID, 0x123, 3)[..14], 0, 0).is_err());
    assert!(decode_frame(&fixture(RX_ECHO_ID, 0x800, 1), 0, 0).is_err());
    let mut bytes = fixture(RX_ECHO_ID, 0x123, 3);
    bytes[10] = 2;
    assert!(matches!(
        decode_frame(&bytes, 0, 0).unwrap_err().diagnostic(),
        CandlelightError::UnsupportedFrame(_)
    ));
    assert!(matches!(
        decode_frame(&fixture(RX_ECHO_ID, 0x40000123, 3), 0, 0)
            .unwrap_err()
            .diagnostic(),
        CandlelightError::UnsupportedFrame(_)
    ));
}

#[test]
fn bus_off_retains_raw_usb_evidence_and_is_distinct_from_timeout_and_removal() {
    let bytes = fixture(RX_ECHO_ID, 0x20000040, 8);
    assert_eq!(
        decode_frame(&bytes, 0, 0).unwrap_err(),
        CandlelightError::BusOff(bytes)
    );
    assert_eq!(
        CandlelightError::from(nusb::transfer::TransferError::Cancelled),
        CandlelightError::Timeout
    );
    assert_eq!(
        CandlelightError::from(nusb::transfer::TransferError::Disconnected),
        CandlelightError::Disconnected
    );
    assert!(matches!(
        CandlelightError::from(nusb::transfer::TransferError::Stall),
        CandlelightError::Usb(_)
    ));
    assert_ne!(CandlelightError::Timeout, CandlelightError::Disconnected);
    assert!(CandlelightError::Timeout.to_string().contains("timeout"));
    assert!(
        CandlelightError::Disconnected
            .to_string()
            .contains("disconnected")
    );
}

#[test]
fn bulk_packet_size_can_be_smaller_than_the_can_frame() {
    let descriptors = [
        EndpointDescriptor {
            address: 0x83,
            transfer_type: 2,
            max_packet_size: 16,
        },
        EndpointDescriptor {
            address: 0x02,
            transfer_type: 2,
            max_packet_size: 16,
        },
    ];
    assert_eq!(
        select_bulk_endpoints(&descriptors).unwrap(),
        BulkEndpoints {
            input: 0x83,
            output: 0x02
        }
    );
}

#[test]
fn malformed_usb_packets_keep_raw_evidence_and_typed_diagnostics() {
    let raw = fixture(RX_ECHO_ID, 0x123, 3)[..14].to_vec();
    let error = decode_frame(&raw, 0, 0).unwrap_err();
    assert_eq!(error.raw_usb(), Some(&raw[..]));
    assert!(matches!(error.diagnostic(), CandlelightError::Protocol(_)));
}
