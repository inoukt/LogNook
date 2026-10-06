mod oem_probe;
mod scion_probe;
mod serial;
pub use oem_probe::{
    OemCandidateDecodedValue, OemCandidateOutcome, OemCandidateResponse, OemCandidateTryResult,
    build_oem_candidate_command, decode_oem_candidate_response, try_oem_candidate,
};
pub use scion_probe::{ScionMode21DecodedSignal, ScionMode21ProbeResult, probe_scion_xb_mode21};
#[cfg(test)]
use scion_probe::{build_scion_probe_command, decode_scion_mode21_response};
#[cfg(test)]
#[path = "obdlink_sx/tests.rs"]
mod tests;

use crate::{
    ConnectionCapabilities, DeviceDescriptor, HardwareAdapter, LogNookError, Sample, SampleOrigin,
    SampleQuality, SignalDefinition,
    obdlink_sx_protocol::{
        Mode01ResponseError, decode_signal, mode01_pid_data_width, parse_mode01_responses,
        parse_supported_pids,
    },
    standard_obd_signals,
};
use serial::SerialTransport;
pub use serial::available_obdlink_sx_ports;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct ObdLinkSxAdapter {
    transport: SerialTransport,
    serial_number: Option<String>,
    device: Option<DeviceDescriptor>,
    supported_pids: Vec<u8>,
    ecu_id: Option<String>,
    fast_tx_header: Option<String>,
    restore_adapter_settings: bool,
    started: Option<Instant>,
}

fn mode01_read_error(error: Mode01ResponseError) -> LogNookError {
    let message = error.to_string();
    if error.is_transient() {
        LogNookError::transient(message)
    } else {
        LogNookError::new(message)
    }
}

impl ObdLinkSxAdapter {
    pub fn new(port_name: impl Into<String>) -> Self {
        Self {
            transport: SerialTransport::new(port_name.into()),
            serial_number: None,
            device: None,
            supported_pids: Vec::new(),
            ecu_id: None,
            fast_tx_header: None,
            restore_adapter_settings: false,
            started: None,
        }
    }

    #[cfg(test)]
    fn with_port(port_name: impl Into<String>, port: Box<dyn serial::PortIo>) -> Self {
        let port_name = port_name.into();
        let mut adapter = Self::new(port_name.clone());
        adapter.transport = SerialTransport::with_port(port_name, port);
        adapter
    }

    fn open(&mut self) -> Result<(), LogNookError> {
        self.transport.open()
    }

    fn command(&mut self, command: &str) -> Result<String, LogNookError> {
        self.transport.command(command)
    }

    fn read_prompt(&mut self) -> Result<String, LogNookError> {
        self.transport.read_prompt()
    }

    fn connect_inner(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        self.open()?;
        let device = self.read_identity()?;
        let (mut supported_pids, mut ecu_id) = self.discover_supported_pids()?;
        let protocol = self.command("STPRS")?;
        let mut fast_tx_header = None;
        if protocol.contains("ISO 15765-4 (CAN 11/500)") {
            self.configure_can_headers()?;
            (supported_pids, ecu_id) = self.discover_supported_pids()?;
            fast_tx_header = ecu_id.as_deref().and_then(physical_request_header);
        }
        let supported_signals: Vec<_> = standard_obd_signals()
            .into_iter()
            .filter(|signal| supported_pids.contains(&signal.pid))
            .collect();
        if supported_signals.is_empty() {
            return Err(LogNookError::new(
                "ECU reports no supported LogNook Mode 01 signals",
            ));
        }

        self.ecu_id = ecu_id;
        self.fast_tx_header = fast_tx_header;
        self.serial_number = Some(device.stable_id.clone());
        self.device = Some(device.clone());
        self.supported_pids = supported_pids;
        self.started = Some(Instant::now());
        Ok(ConnectionCapabilities {
            device,
            protocol: "SAE J1979 Mode 01".into(),
            read_only: true,
            supported_signals,
        })
    }

    fn discover_supported_pids(&mut self) -> Result<(Vec<u8>, Option<String>), LogNookError> {
        let mut supported = Vec::new();
        let mut ecu_id = None;
        let mut range_start = 0;
        loop {
            let response = self.command(&format!("01{range_start:02X}"))?;
            let range = parse_supported_pids(&response, range_start).map_err(LogNookError::new)?;
            if let (Some(previous), Some(current)) = (&ecu_id, &range.ecu_id)
                && previous != current
            {
                return Err(LogNookError::new(
                    "ECU identity changed during supported-PID discovery",
                ));
            }
            if ecu_id.is_none() {
                ecu_id = range.ecu_id.clone();
            }
            supported.extend(range.pids);
            if !range.has_more || range_start == 0xE0 {
                return Ok((supported, ecu_id));
            }
            range_start += 0x20;
        }
    }

    fn read_identity(&mut self) -> Result<DeviceDescriptor, LogNookError> {
        // An already-powered SX may not re-emit its startup banner on port open.
        // STI is adapter-only; its reply prompt synchronizes subsequent commands.
        let mut firmware_response = self.command("STI")?;
        if is_startup_frame(&firmware_response) {
            firmware_response = self.read_prompt()?;
        }
        let firmware = response_value(&firmware_response, "STI")?;
        if !firmware.starts_with("STN1130 v") {
            return Err(LogNookError::new(format!(
                "Unexpected firmware identity for OBDLink SX: {firmware}"
            )));
        }
        let device_id = response_value(&self.command("STDI")?, "STDI")?;
        if !device_id.starts_with("OBDLink SX r") {
            return Err(LogNookError::new(format!(
                "Selected port is not an OBDLink SX ({device_id})"
            )));
        }
        let serial_number = response_value(&self.command("STSN")?, "STSN")?;
        if serial_number.len() != 12
            || !serial_number.starts_with("1130")
            || !serial_number.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(LogNookError::new(
                "OBDLink SX returned an invalid factory serial number",
            ));
        }
        Ok(DeviceDescriptor {
            stable_id: serial_number,
            hardware_family: "OBDLink SX".into(),
            firmware: Some(firmware),
            path: Some(self.transport.port_name().to_owned()),
        })
    }

    fn identify_inner(&mut self) -> Result<DeviceDescriptor, LogNookError> {
        self.open()?;
        self.read_identity()
    }

    fn configure_can_headers(&mut self) -> Result<(), LogNookError> {
        self.restore_adapter_settings = true;
        for command in ["ATE0", "AT H1", "AT SH 7DF"] {
            let response = self.command(command)?;
            if !response.contains("OK") {
                return Err(LogNookError::new(format!(
                    "OBDLink SX did not acknowledge {command}"
                )));
            }
        }
        Ok(())
    }

    fn restore_adapter_settings(&mut self) {
        if !self.restore_adapter_settings {
            return;
        }
        for command in ["AT H0", "AT SH 7DF", "ATE1"] {
            let _ = self.command(command);
        }
        self.restore_adapter_settings = false;
        self.fast_tx_header = None;
    }
}

pub fn identify_obdlink_sx(port_name: impl Into<String>) -> Result<DeviceDescriptor, LogNookError> {
    let mut adapter = ObdLinkSxAdapter::new(port_name);
    let result = adapter.identify_inner();
    adapter.disconnect();
    result
}

impl HardwareAdapter for ObdLinkSxAdapter {
    fn execute_research_test(
        &mut self,
        request: crate::ResearchTestRequest,
    ) -> Result<crate::ResearchTestResult, LogNookError> {
        let crate::ResearchTestRequest::OemCandidate(spec) = request else {
            return Err(LogNookError::new(
                "OBDLink SX does not support A30 captured read tests",
            ));
        };
        if self.started.is_none() {
            return Err(LogNookError::new("OBDLink SX adapter is not connected"));
        }
        self.read_connected_candidate(&spec)
            .map(crate::ResearchTestResult::OemCandidate)
    }

    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.started.is_some() {
            return Err(LogNookError::new("OBDLink SX adapter is already connected"));
        }
        let result = self.connect_inner();
        if result.is_err() {
            self.disconnect();
        }
        result
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        let started = self
            .started
            .ok_or_else(|| LogNookError::new("OBDLink SX adapter is not connected"))?;
        let serial_number = self
            .serial_number
            .clone()
            .ok_or_else(|| LogNookError::new("OBDLink SX identity is unavailable"))?;
        let catalog = standard_obd_signals();

        for signal in signals {
            if !catalog.contains(signal) || !self.supported_pids.contains(&signal.pid) {
                return Err(LogNookError::new(format!(
                    "Unsupported read-only Mode 01 signal: {}",
                    signal.id
                )));
            }
        }

        if let Some(header) = self.fast_tx_header.clone() {
            let groups = group_signal_indices(signals).map_err(LogNookError::new)?;
            let mut samples: Vec<Option<Sample>> = vec![None; signals.len()];
            for group in groups {
                let group_signals: Vec<_> = group.iter().map(|index| &signals[*index]).collect();
                let data = format!(
                    "01{}",
                    group_signals
                        .iter()
                        .map(|signal| format!("{:02X}", signal.pid))
                        .collect::<String>()
                );
                let mut payload_length = 1;
                for signal in &group_signals {
                    let width = mode01_pid_data_width(signal.pid).ok_or_else(|| {
                        LogNookError::new(format!(
                            "No data width is defined for PID {:02X}",
                            signal.pid
                        ))
                    })?;
                    payload_length += 1 + width;
                }
                let frames = isotp_frame_count(payload_length);
                let response_text =
                    self.command(&format!("STPX H:{header}, D:{data}, R:{frames}, T:250"))?;
                for index in group {
                    let signal = &signals[index];
                    let mut responses =
                        parse_mode01_responses(&response_text, signal.service, signal.pid)
                            .map_err(mode01_read_error)?;
                    if responses.len() != 1 {
                        return Err(LogNookError::new(
                            "Multiple ECU responders are not supported by this SX adapter",
                        ));
                    }
                    let response = responses.pop().expect("one response was checked");
                    samples[index] = Some(sample_from_response(
                        signal,
                        &response,
                        &serial_number,
                        started,
                        self.ecu_id.as_deref(),
                    )?);
                }
            }
            return samples
                .into_iter()
                .map(|sample| {
                    sample.ok_or_else(|| LogNookError::new("SX omitted a selected PID sample"))
                })
                .collect();
        }

        let mut samples = Vec::with_capacity(signals.len());
        for signal in signals {
            let command = format!("01{:02X}", signal.pid);
            let response_text = self.command(&command)?;
            let responses = parse_mode01_responses(&response_text, 0x01, signal.pid)
                .map_err(mode01_read_error)?;
            if responses.len() != 1 {
                return Err(LogNookError::new(
                    "Multiple ECU responders are not supported by this SX adapter",
                ));
            }
            let response = &responses[0];
            samples.push(sample_from_response(
                signal,
                response,
                &serial_number,
                started,
                self.ecu_id.as_deref(),
            )?);
        }
        Ok(samples)
    }

    fn disconnect(&mut self) {
        self.restore_adapter_settings();
        self.transport.close();
        self.serial_number = None;
        self.device = None;
        self.supported_pids.clear();
        self.ecu_id = None;
        self.fast_tx_header = None;
        self.started = None;
    }
}

fn physical_request_header(response_header: &str) -> Option<String> {
    if response_header.len() != 3 || !response_header.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let response_id = u16::from_str_radix(response_header, 16).ok()?;
    (0x7E8..=0x7EF)
        .contains(&response_id)
        .then(|| format!("{:03X}", response_id - 8))
}

fn group_signal_indices(signals: &[SignalDefinition]) -> Result<Vec<Vec<usize>>, String> {
    let mut order: Vec<_> = (0..signals.len()).collect();
    order.sort_by_key(|index| (mode01_pid_data_width(signals[*index].pid), *index));

    let mut groups = Vec::new();
    let mut group = Vec::new();
    let mut response_length = 1usize;
    for index in order {
        let width = mode01_pid_data_width(signals[index].pid).ok_or_else(|| {
            format!(
                "No data width is defined for PID {:02X}",
                signals[index].pid
            )
        })?;
        let addition = 1 + width;
        if !group.is_empty() && response_length + addition > 13 {
            groups.push(std::mem::take(&mut group));
            response_length = 1;
        }
        if response_length + addition > 13 {
            return Err(format!(
                "PID {:02X} cannot fit in the read-only STPX response group",
                signals[index].pid
            ));
        }
        group.push(index);
        response_length += addition;
    }
    if !group.is_empty() {
        groups.push(group);
    }
    Ok(groups)
}

fn isotp_frame_count(payload_length: usize) -> usize {
    if payload_length <= 7 {
        1
    } else {
        1 + (payload_length - 6).div_ceil(7)
    }
}

fn sample_from_response(
    signal: &SignalDefinition,
    response: &crate::obdlink_sx_protocol::Mode01Response,
    serial_number: &str,
    started: Instant,
    connected_ecu: Option<&str>,
) -> Result<Sample, LogNookError> {
    if let (Some(connected_ecu), Some(sample_ecu)) = (connected_ecu, response.ecu_id.as_deref())
        && connected_ecu != sample_ecu
    {
        return Err(LogNookError::new(
            "ECU identity changed during the SX session",
        ));
    }
    let elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
    Ok(Sample {
        elapsed_ns,
        wall_time_unix_ns: now_unix_ns(),
        source_id: format!("obdlink-sx:{serial_number}"),
        ecu_id: response
            .ecu_id
            .clone()
            .or_else(|| connected_ecu.map(str::to_owned)),
        signal_id: signal.id.clone(),
        value: decode_signal(signal, response).map_err(LogNookError::new)?,
        unit: signal.unit.clone(),
        quality: SampleQuality::Valid,
        origin: SampleOrigin::Live,
        raw_bytes: Some(response.raw_bytes.clone()),
    })
}

fn is_startup_frame(response: &str) -> bool {
    let mut lines = response
        .split(['\r', '\n'])
        .map(|line| line.trim().trim_matches('>').trim())
        .filter(|line| !line.is_empty());
    match lines.next() {
        None => true,
        Some(line) => line.starts_with("ELM327 v") && lines.next().is_none(),
    }
}

fn response_value(response: &str, command: &str) -> Result<String, LogNookError> {
    response
        .split(['\r', '\n'])
        .map(str::trim)
        .map(|line| line.trim_matches('>').trim())
        .find(|line| {
            !line.is_empty()
                && !line.eq_ignore_ascii_case(command)
                && !line.to_ascii_uppercase().starts_with("SEARCHING")
        })
        .map(str::to_owned)
        .ok_or_else(|| LogNookError::new(format!("OBDLink SX returned no value for {command}")))
}

fn now_unix_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}
