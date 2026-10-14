use super::serial::{is_selected_sx_port, is_sx_usb};
use super::{
    ObdLinkSxAdapter, group_signal_indices, isotp_frame_count, serial::MAX_RESPONSE_BYTES,
};
use crate::obdlink_sx_protocol::mode01_pid_data_width;
use crate::{
    HardwareAdapter, RawAdapterCommand, ScionMode21ProbeSpec, ScionProbeChannel,
    standard_obd_signals,
};
use serialport::{SerialPortInfo, SerialPortType, UsbPortInfo};
use std::{
    io::{Cursor, Read, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

struct FixturePort {
    input: Cursor<Vec<u8>>,
    written: Arc<Mutex<Vec<u8>>>,
}

impl Read for FixturePort {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buffer)
    }
}

impl Write for FixturePort {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.written.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn adapter_for(transcript: String) -> (ObdLinkSxAdapter, Arc<Mutex<Vec<u8>>>) {
    let written = Arc::new(Mutex::new(Vec::new()));
    let port = FixturePort {
        input: Cursor::new(transcript.into_bytes()),
        written: written.clone(),
    };
    (ObdLinkSxAdapter::with_port("COM4", Box::new(port)), written)
}

#[test]
fn raw_terminal_and_diagnostic_commands_use_the_serial_command_owner() {
    let (mut terminal_adapter, terminal_written) = adapter_for("STI\rOBDLink SX\r>".into());
    let terminal = terminal_adapter
        .execute_raw_command(RawAdapterCommand::AdapterTerminal("STI".into()))
        .unwrap();
    assert!(terminal.response.contains("OBDLink SX"));
    assert_eq!(&*terminal_written.lock().unwrap(), b"STI\r");

    let (mut diagnostic_adapter, diagnostic_written) = adapter_for("010C\r41 0C 1A F8\r>".into());
    let diagnostic = diagnostic_adapter
        .execute_raw_command(RawAdapterCommand::DiagnosticPayload(vec![0x01, 0x0c]))
        .unwrap();
    assert!(diagnostic.response.contains("41 0C"));
    assert_eq!(diagnostic.command, "010C");
    assert_eq!(&*diagnostic_written.lock().unwrap(), b"010C\r");
}

fn scion_probe_spec() -> ScionMode21ProbeSpec {
    let definitions = [
        (
            "Intake air temperature",
            "celsius",
            48,
            8,
            1.0,
            1.0,
            -40.0,
            "(raw*(1)/(1))+(-40)",
            Some(-40.0),
            Some(215.0),
        ),
        (
            "Coolant temperature",
            "celsius",
            64,
            8,
            1.0,
            1.0,
            -40.0,
            "(raw*(1)/(1))+(-40)",
            Some(-40.0),
            Some(215.0),
        ),
        (
            "Engine speed",
            "rpm",
            72,
            16,
            1.0,
            4.0,
            0.0,
            "(raw*(1)/(4))+(0)",
            None,
            Some(16383.75),
        ),
        (
            "Vehicle speed",
            "kilometersPerHour",
            88,
            8,
            1.0,
            1.0,
            0.0,
            "(raw*(1)/(1))+(0)",
            None,
            Some(255.0),
        ),
        (
            "Throttle sensor position",
            "percent",
            112,
            8,
            100.0,
            255.0,
            0.0,
            "(raw*(100)/(255))+(0)",
            None,
            Some(100.0),
        ),
    ];
    ScionMode21ProbeSpec {
        source_id: crate::oem_catalog::SCION_XB_SOURCE_ID.into(),
        source_blob_sha: crate::oem_catalog::SCION_XB_BLOB_SHA.into(),
        source_url: "https://source.test/scion".into(),
        license: "CC BY-SA 4.0".into(),
        license_url: "https://source.test/license".into(),
        service: 0x21,
        identifier: 0x01,
        request_header: "7E0".into(),
        response_header: "7E8".into(),
        channels: definitions
            .into_iter()
            .map(
                |(
                    name,
                    unit,
                    bit_offset,
                    bit_length,
                    multiplier,
                    divisor,
                    addend,
                    formula,
                    minimum,
                    maximum,
                )| ScionProbeChannel {
                    candidate_id: format!("candidate-{name}"),
                    name: name.into(),
                    unit: unit.into(),
                    bit_offset,
                    bit_length,
                    multiplier,
                    divisor,
                    addend,
                    formula: formula.into(),
                    minimum,
                    maximum,
                },
            )
            .collect(),
    }
}

fn iso_tp_response(ecu: &str, payload: &[u8]) -> String {
    let mut frames = Vec::new();
    let first_len = payload.len().min(6);
    let mut first = vec![
        0x10 | ((payload.len() >> 8) as u8 & 0x0F),
        payload.len() as u8,
    ];
    first.extend_from_slice(&payload[..first_len]);
    frames.push(first);
    let mut offset = first_len;
    let mut sequence = 1;
    while offset < payload.len() {
        let end = (offset + 7).min(payload.len());
        let mut frame = vec![0x20 | sequence];
        frame.extend_from_slice(&payload[offset..end]);
        frames.push(frame);
        offset = end;
        sequence = (sequence + 1) & 0x0F;
    }
    frames
        .into_iter()
        .map(|frame| {
            format!(
                "{ecu} {}",
                frame
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\r")
}

#[test]
fn scion_one_shot_command_and_response_are_source_scoped() {
    let spec = scion_probe_spec();
    assert_eq!(
        super::build_scion_probe_command(&spec).unwrap(),
        "STPX H:7E0, D:2101, T:250"
    );

    let mut data = vec![0_u8; 27];
    data[6] = 20;
    data[8] = 90;
    data[9] = 0x1A;
    data[10] = 0xF8;
    data[11] = 40;
    data[14] = 128;
    let mut payload = vec![0x61, 0x01];
    payload.extend(data);
    let parsed = super::decode_scion_mode21_response(&spec, &iso_tp_response("7E8", &payload));

    assert_eq!(parsed.outcome, "positive");
    assert_eq!(parsed.payload.len(), 29);
    assert_eq!(parsed.decoded[0].value, -20.0);
    assert_eq!(parsed.decoded[1].value, 50.0);
    assert_eq!(parsed.decoded[2].value, 1726.0);
    assert_eq!(parsed.decoded[3].value, 40.0);
    assert!((parsed.decoded[4].value - 50.19607843137255).abs() < 0.000_001);
}

#[test]
fn scion_probe_accepts_the_captured_28_byte_mode21_response() {
    let parsed = super::decode_scion_mode21_response(
        &scion_probe_spec(),
        include_str!("../../tests/fixtures/obdlink_sx/scion_mode21_01_a30.txt"),
    );

    assert_eq!(parsed.outcome, "positive");
    assert_eq!(
        parsed.payload,
        vec![
            0x61, 0x01, 0x00, 0x00, 0x00, 0x00, 0x25, 0x47, 0x65, 0x85, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x30, 0x82, 0x29, 0x52, 0x28, 0x20, 0x01, 0x38, 0x02, 0x33, 0x2f, 0xc2,
        ]
    );
    assert_eq!(parsed.decoded.len(), 5);

    let unobserved_payload = [vec![0x61, 0x01], vec![0; 28]].concat();
    let rejected = super::decode_scion_mode21_response(
        &scion_probe_spec(),
        &iso_tp_response("7E8", &unobserved_payload),
    );
    assert_eq!(rejected.outcome, "wrong_length");
}

#[test]
fn scion_probe_uses_one_exact_request_after_sx_identity_and_can_check() {
    let spec = scion_probe_spec();
    let mut data = vec![0_u8; 27];
    data[6] = 20;
    data[8] = 90;
    data[9] = 0x1A;
    data[10] = 0xF8;
    data[11] = 40;
    data[14] = 128;
    let mut payload = vec![0x61, 0x01];
    payload.extend(data);
    let transcript = format!(
        "{}STPRS\rISO 15765-4 (CAN 11/500) [AT]\r\r>OK\r\r>OK\r\r>OK\r\r>{}\r\r>OK\r\r>OK\r\r>OK\r\r>",
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        iso_tp_response("7E8", &payload),
    );
    let (mut adapter, written) = adapter_for(transcript);

    let result = adapter.probe_scion_inner(&spec).unwrap();
    adapter.disconnect();

    assert_eq!(result.outcome, "positive");
    assert_eq!(result.exact_adapter_command, "STPX H:7E0, D:2101, T:250");
    assert_eq!(result.adapter.stable_id, "113011166292");
    assert_eq!(result.decoded_signals[2].value, 1726.0);
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\rSTPRS\rATE0\rAT H1\rAT SH 7DF\rSTPX H:7E0, D:2101, T:250\rAT H0\rAT SH 7DF\rATE1\r"
    );
}

#[test]
fn scion_one_shot_reports_negative_truncated_and_wrong_ecu_responses() {
    let spec = scion_probe_spec();
    assert_eq!(
        super::decode_scion_mode21_response(&spec, "7E8 03 7F 21 11\r>").outcome,
        "negative_response"
    );
    assert_eq!(
        super::decode_scion_mode21_response(&spec, "7E8 10 1D 61 01 00 00\r7E8 21 00\r>").outcome,
        "malformed_response"
    );
    assert_eq!(
        super::decode_scion_mode21_response(&spec, "7E9 03 61 01 00\r>").outcome,
        "wrong_responder"
    );
}

#[test]
fn scion_one_shot_refuses_any_changed_service_address_or_decoder() {
    let mut spec = scion_probe_spec();
    spec.request_header = "7DF".into();
    assert!(super::build_scion_probe_command(&spec).is_err());
    let mut spec = scion_probe_spec();
    spec.channels[2].divisor = 1.0;
    assert!(super::build_scion_probe_command(&spec).is_err());
}

fn identity_transcript() -> String {
    format!(
        "{}{}{}{}STPRS\rISO 9141-2 [AT]\r\r>",
        include_str!("../../tests/fixtures/obdlink_sx/startup.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/identity.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/mode01_supported.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/mode01_supported_20.txt"),
    )
}

#[test]
fn connects_to_idle_sx_without_unsolicited_startup_banner() {
    let transcript = format!(
        "{}{}{}STPRS\rISO 9141-2 [AT]\r\r>",
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_supported.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/mode01_supported_20.txt"),
    );
    let (mut adapter, written) = adapter_for(transcript);
    let capabilities = adapter.connect().unwrap();
    assert_eq!(capabilities.device.stable_id, "113011166292");
    assert_eq!(
        capabilities.device.firmware.as_deref(),
        Some("STN1130 v5.10.1")
    );
    assert_eq!(capabilities.supported_signals.len(), 17);
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\r0100\r0120\rSTPRS\r"
    );
}

#[test]
fn identity_only_physical_fixture_sends_no_vehicle_query() {
    let (mut adapter, written) = adapter_for(
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt").into(),
    );
    let events = Arc::new(Mutex::new(Vec::<crate::DiagnosticProgress>::new()));
    let captured = events.clone();
    adapter.set_diagnostic_sink(Arc::new(move |event| captured.lock().unwrap().push(event)));
    let device = adapter.identify_inner().unwrap();
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.phase == crate::DiagnosticPhase::Succeeded)
            .map(|event| event.stage.as_str())
            .collect::<Vec<_>>(),
        ["transport_open", "adapter_identity"]
    );
    assert_eq!(
        events.last().unwrap().adapter_id.as_deref(),
        Some("113011166292")
    );
    assert_eq!(device.stable_id, "113011166292");
    assert_eq!(&*written.lock().unwrap(), b"STI\rSTDI\rSTSN\r");
}

#[test]
fn empty_startup_prompt_is_tolerated_before_firmware_reply() {
    let transcript = format!(
        ">{}{}{}STPRS\rISO 9141-2 [AT]\r\r>",
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_supported.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/mode01_supported_20.txt"),
    );
    let (mut adapter, _) = adapter_for(transcript);
    assert!(adapter.connect().is_ok());
}

#[test]
fn silent_initial_reply_sends_only_read_only_firmware_query() {
    let (mut adapter, written) = adapter_for(String::new());
    assert!(adapter.connect().is_err());
    assert_eq!(&*written.lock().unwrap(), b"STI\r");
}

#[test]
fn unexpected_initial_firmware_stops_before_other_queries() {
    let (mut adapter, written) = adapter_for("STI\rSTN2230 v5.10.1\r\r>".into());
    assert!(adapter.connect().is_err());
    assert_eq!(&*written.lock().unwrap(), b"STI\r");
}

#[test]
fn startup_banner_is_optional_and_reply_prompts_sequence_queries() {
    let (mut adapter, written) = adapter_for(identity_transcript());
    let capabilities = adapter.connect().unwrap();

    assert_eq!(capabilities.device.hardware_family, "OBDLink SX");
    assert_eq!(capabilities.device.stable_id, "113012345678");
    assert_eq!(
        capabilities.device.firmware.as_deref(),
        Some("STN1130 v1.0.0")
    );
    assert_eq!(capabilities.device.path.as_deref(), Some("COM4"));
    assert_eq!(capabilities.protocol, "SAE J1979 Mode 01");
    assert!(capabilities.read_only);
    assert_eq!(
        capabilities
            .supported_signals
            .iter()
            .map(|signal| signal.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "engine_rpm",
            "coolant_temp",
            "intake_temp",
            "throttle_pos",
            "vehicle_speed",
            "fuel_system_status",
            "engine_load",
            "fuel_trim_short_b1",
            "fuel_trim_long_b1",
            "intake_manifold_pressure",
            "timing_advance",
            "o2_sensors_present_1_2",
            "obd_standard_supported",
            "engine_runtime",
            "distance_with_mil"
        ],
    );
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\r0100\r0120\rSTPRS\r"
    );
}

#[test]
fn usb_only_identity_probe_sends_no_vehicle_request() {
    let transcript = format!(
        "{}{}",
        include_str!("../../tests/fixtures/obdlink_sx/startup.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/identity.txt"),
    );
    let (mut adapter, written) = adapter_for(transcript);

    let device = adapter.identify_inner().unwrap();
    assert_eq!(device.hardware_family, "OBDLink SX");
    assert_eq!(device.firmware.as_deref(), Some("STN1130 v1.0.0"));
    assert_eq!(device.stable_id, "113012345678");
    assert_eq!(&*written.lock().unwrap(), b"STI\rSTDI\rSTSN\r");
}

#[test]
fn incomplete_initial_reply_stops_without_a_reset_or_vehicle_query() {
    let (mut adapter, written) = adapter_for("ELM327 v1.4b\r".into());
    assert!(adapter.connect().is_err());
    assert_eq!(&*written.lock().unwrap(), b"STI\r");
}

#[test]
fn prompt_reader_rejects_a_buffer_larger_than_its_limit() {
    let transcript = format!("{}>", "X".repeat(MAX_RESPONSE_BYTES + 1));
    let (mut adapter, written) = adapter_for(transcript);

    assert!(adapter.command("STI").is_err());
    assert_eq!(&*written.lock().unwrap(), b"STI\r");
}

#[test]
fn wrong_device_identity_stops_before_serial_or_vehicle_queries() {
    let transcript = format!(
        "{}STI\rSTN1130 v5.10.1\r>STDI\rOBDLink EX r1.0.0\r>",
        include_str!("../../tests/fixtures/obdlink_sx/startup.txt")
    );
    let (mut adapter, written) = adapter_for(transcript);
    assert!(adapter.connect().is_err());
    assert_eq!(&*written.lock().unwrap(), b"STI\rSTDI\r");
}

#[test]
fn read_samples_sends_only_selected_supported_pid_and_keeps_raw_response() {
    let transcript = format!(
        "{}{}",
        identity_transcript(),
        include_str!("../../tests/fixtures/obdlink_sx/mode01_rpm.txt")
    );
    let (mut adapter, written) = adapter_for(transcript);
    let events = Arc::new(Mutex::new(Vec::<crate::DiagnosticProgress>::new()));
    let captured = events.clone();
    adapter.set_diagnostic_sink(Arc::new(move |event| captured.lock().unwrap().push(event)));
    adapter.connect().unwrap();
    let stages: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| event.phase == crate::DiagnosticPhase::Succeeded)
        .map(|event| event.stage.clone())
        .collect();
    assert_eq!(
        stages,
        [
            "transport_open",
            "adapter_identity",
            "supported_pid_discovery",
            "protocol_setup"
        ]
    );
    let rpm = standard_obd_signals()
        .into_iter()
        .find(|signal| signal.id == "engine_rpm")
        .unwrap();

    let samples = adapter.read_samples(&[rpm]).unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].signal_id, "engine_rpm");
    let events = events.lock().unwrap();
    let read = events.last().unwrap();
    assert_eq!(read.stage, "pid_read");
    assert_eq!(read.signal_ids, ["engine_rpm"]);
    assert_eq!(read.phase, crate::DiagnosticPhase::Succeeded);
    assert!(read.duration_ms.is_some());
    assert_eq!(read.adapter_id.as_deref(), Some("113012345678"));
    assert!(read.detail.is_none());
    assert_eq!(samples[0].value, 1726.0);
    assert_eq!(samples[0].ecu_id.as_deref(), Some("7E8"));
    assert_eq!(samples[0].source_id, "obdlink-sx:113012345678");
    assert_eq!(
        samples[0].raw_bytes.as_deref(),
        Some(&[0x41, 0x0C, 0x1A, 0xF8][..])
    );
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\r0100\r0120\rSTPRS\r010C\r"
    );
}

#[test]
fn read_samples_stuffs_five_supported_pids_into_one_complete_can_response() {
    let transcript =
        include_str!("../../tests/fixtures/obdlink_sx/mode01_stuffed_five_sorted.txt").repeat(5);
    let (mut adapter, written) = adapter_for(transcript);
    let signals: Vec<_> = standard_obd_signals()
        .into_iter()
        .filter(|signal| [0x0C, 0x05, 0x0F, 0x11, 0x0D].contains(&signal.pid))
        .collect();
    adapter.serial_number = Some("113011166292".into());
    adapter.supported_pids = signals.iter().map(|signal| signal.pid).collect();
    adapter.ecu_id = Some("7E8".into());
    adapter.fast_tx_header = Some("7E0".into());
    adapter.started = Some(Instant::now());
    adapter.open().unwrap();

    let samples = adapter.read_samples(&signals).unwrap();

    assert_eq!(samples.len(), 5);
    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.value)
            .collect::<Vec<_>>(),
        vec![597.0, 100.0, 59.0, 30.0 * 100.0 / 255.0, 0.0,]
    );
    assert!(
        samples
            .iter()
            .all(|sample| sample.ecu_id.as_deref() == Some("7E8"))
    );
    assert_eq!(
        &*written.lock().unwrap(),
        b"STPX H:7E0, D:01050F110D0C, R:2, T:250\r"
    );
}

#[test]
fn all_35_seeded_signals_pack_into_seven_two_frame_response_groups() {
    let signals = standard_obd_signals();
    let groups = group_signal_indices(&signals).unwrap();

    assert_eq!(groups.len(), 7);
    let mut grouped_indices: Vec<_> = groups.iter().flatten().copied().collect();
    grouped_indices.sort_unstable();
    assert_eq!(grouped_indices, (0..signals.len()).collect::<Vec<_>>());
    for group in groups {
        let payload_length = 1 + group
            .iter()
            .map(|index| 1 + mode01_pid_data_width(signals[*index].pid).unwrap())
            .sum::<usize>();
        assert!(payload_length <= 13);
        assert!(isotp_frame_count(payload_length) <= 2);
    }
}

#[test]
fn can_connection_uses_stpx_stuffing_and_restores_volatile_adapter_settings() {
    let transcript = format!(
        "{}{}{}STPRS\rISO 15765-4 (CAN 11/500) [AT]\r\r>ATE0\rOK\r\r>OK\r\r>OK\r\r>{}{}{}{}{}{}",
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/physical_ford_supported.txt"),
        include_str!("../../tests/fixtures/obdlink_sx/mode01_supported_20.txt"),
        "0100\r7E8 06 41 00 BF BE A8 93\r\r>",
        "0120\r7E8 06 41 20 80 00 00 00\r\r>",
        include_str!("../../tests/fixtures/obdlink_sx/mode01_stuffed_five_sorted.txt"),
        "OK\r\r>",
        "OK\r\r>",
        "OK\r\r>"
    );
    let (mut adapter, written) = adapter_for(transcript);
    let capabilities = adapter.connect().unwrap();
    let signals: Vec<_> = capabilities
        .supported_signals
        .into_iter()
        .filter(|signal| [0x0C, 0x05, 0x0F, 0x11, 0x0D].contains(&signal.pid))
        .collect();

    let samples = adapter.read_samples(&signals).unwrap();

    assert_eq!(samples.len(), 5);
    assert_eq!(samples[0].value, 597.0);
    assert_eq!(samples[1].value, 100.0);
    assert_eq!(samples[2].value, 59.0);
    adapter.disconnect();
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\r0100\r0120\rSTPRS\rATE0\rAT H1\rAT SH 7DF\r0100\r0120\rSTPX H:7E0, D:01050F110D0C, R:2, T:250\rAT H0\rAT SH 7DF\rATE1\r"
    );
}

#[test]
fn unsupported_pid_is_rejected_before_any_vehicle_request() {
    let (mut adapter, written) = adapter_for(identity_transcript());
    adapter.connect().unwrap();
    let mut unsupported = standard_obd_signals()
        .into_iter()
        .find(|signal| signal.id == "engine_rpm")
        .unwrap();
    unsupported.id = "unsupported_pid".into();
    unsupported.pid = 0x12;

    assert!(adapter.read_samples(&[unsupported]).is_err());
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\r0100\r0120\rSTPRS\r"
    );
}

#[test]
fn only_the_exact_sx_vid_pid_pair_is_enumerated() {
    assert!(is_sx_usb(0x0403, 0x6015));
    assert!(!is_sx_usb(0x0403, 0xCC4D));
    assert!(!is_sx_usb(0x0404, 0x6015));
}

#[test]
fn selected_com_port_must_still_match_the_sx_usb_identity() {
    let sx = SerialPortInfo {
        port_name: "COM4".into(),
        port_type: SerialPortType::UsbPort(UsbPortInfo {
            vid: 0x0403,
            pid: 0x6015,
            serial_number: Some("113011166292".into()),
            manufacturer: Some("FTDI".into()),
            product: Some("OBDLink SX".into()),
        }),
    };
    let openport = SerialPortInfo {
        port_name: "COM4".into(),
        port_type: SerialPortType::UsbPort(UsbPortInfo {
            vid: 0x0403,
            pid: 0xCC4D,
            serial_number: None,
            manufacturer: Some("FTDI".into()),
            product: Some("OpenPort 2.0".into()),
        }),
    };

    assert!(is_selected_sx_port("COM4", &sx));
    assert!(!is_selected_sx_port("COM5", &sx));
    assert!(!is_selected_sx_port("COM4", &openport));
}

#[test]
fn grouped_read_failure_reports_all_requested_signals() {
    let (mut adapter, _) = adapter_for("NO DATA\r>".into());
    let signals: Vec<_> = standard_obd_signals()
        .into_iter()
        .filter(|s| [0x0C, 0x05].contains(&s.pid))
        .collect();
    adapter.serial_number = Some("113011166292".into());
    adapter.supported_pids = signals.iter().map(|s| s.pid).collect();
    adapter.fast_tx_header = Some("7E0".into());
    adapter.started = Some(Instant::now());
    let events = Arc::new(Mutex::new(Vec::<crate::DiagnosticProgress>::new()));
    let captured = events.clone();
    adapter.set_diagnostic_sink(Arc::new(move |event| captured.lock().unwrap().push(event)));
    assert!(adapter.read_samples(&signals).unwrap().is_empty());
    let events = events.lock().unwrap();
    let mut actual = events
        .iter()
        .filter(|event| event.phase == crate::DiagnosticPhase::Failed)
        .flat_map(|event| event.signal_ids.clone())
        .collect::<Vec<_>>();
    actual.sort();
    let mut expected: Vec<_> = signals.iter().map(|s| s.id.clone()).collect();
    expected.sort();
    assert_eq!(actual, expected);
    assert!(adapter.started.is_some());
}

#[test]
fn grouped_read_keeps_valid_samples_when_one_pid_is_rejected() {
    let (mut adapter, _) = adapter_for("7E8 04 41 0C 1A F8\r>".into());
    let signals: Vec<_> = standard_obd_signals()
        .into_iter()
        .filter(|signal| [0x0C, 0x05].contains(&signal.pid))
        .collect();
    adapter.serial_number = Some("113011166292".into());
    adapter.supported_pids = signals.iter().map(|signal| signal.pid).collect();
    adapter.fast_tx_header = Some("7E0".into());
    adapter.started = Some(Instant::now());
    let events = Arc::new(Mutex::new(Vec::<crate::DiagnosticProgress>::new()));
    let captured = events.clone();
    adapter.set_diagnostic_sink(Arc::new(move |event| captured.lock().unwrap().push(event)));

    let samples = adapter.read_samples(&signals).unwrap();

    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].signal_id, "engine_rpm");
    assert_eq!(samples[0].value, 1726.0);
    assert!(events.lock().unwrap().iter().any(|event| {
        event.phase == crate::DiagnosticPhase::Failed && event.signal_ids == ["coolant_temp"]
    }));
}

#[test]
fn legacy_sx_identify_wrapper_preserves_missing_port_error() {
    let port = "lognook-test-nonexistent-port";
    assert_eq!(
        super::identify_obdlink_sx(port).unwrap_err().to_string(),
        super::identify_obdlink_sx_with_diagnostics(
            port,
            crate::diagnostics::noop_diagnostic_sink()
        )
        .unwrap_err()
        .to_string()
    );
}
