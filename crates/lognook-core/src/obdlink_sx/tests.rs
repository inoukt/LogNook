use super::serial::{is_selected_sx_port, is_sx_usb};
use super::{
    ObdLinkSxAdapter, group_signal_indices, isotp_frame_count, serial::MAX_RESPONSE_BYTES,
};
use crate::obdlink_sx_protocol::mode01_pid_data_width;
use crate::{HardwareAdapter, standard_obd_signals};
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
    let device = adapter.identify_inner().unwrap();
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
    adapter.connect().unwrap();
    let rpm = standard_obd_signals()
        .into_iter()
        .find(|signal| signal.id == "engine_rpm")
        .unwrap();

    let samples = adapter.read_samples(&[rpm]).unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].signal_id, "engine_rpm");
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
