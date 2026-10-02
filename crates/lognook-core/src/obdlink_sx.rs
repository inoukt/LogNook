mod serial;
#[cfg(test)]
#[path = "obdlink_sx/tests.rs"]
mod tests;

use crate::{
    ConnectionCapabilities, DeviceDescriptor, HardwareAdapter, LogNookError, Sample, SampleOrigin,
    SampleQuality, SignalDefinition,
    obdlink_sx_protocol::{decode_signal, parse_mode01_responses, parse_supported_pids},
    standard_obd_signals,
};
use serial::SerialTransport;
pub use serial::available_obdlink_sx_ports;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct ObdLinkSxAdapter {
    transport: SerialTransport,
    serial_number: Option<String>,
    supported_pids: Vec<u8>,
    ecu_id: Option<String>,
    started: Option<Instant>,
}

impl ObdLinkSxAdapter {
    pub fn new(port_name: impl Into<String>) -> Self {
        Self {
            transport: SerialTransport::new(port_name.into()),
            serial_number: None,
            supported_pids: Vec::new(),
            ecu_id: None,
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
        let (supported_pids, ecu_id) = self.discover_supported_pids()?;
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
        self.serial_number = Some(device.stable_id.clone());
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
}

pub fn identify_obdlink_sx(port_name: impl Into<String>) -> Result<DeviceDescriptor, LogNookError> {
    let mut adapter = ObdLinkSxAdapter::new(port_name);
    let result = adapter.identify_inner();
    adapter.disconnect();
    result
}

impl HardwareAdapter for ObdLinkSxAdapter {
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
        let mut samples = Vec::with_capacity(signals.len());

        for signal in signals {
            if !standard_obd_signals()
                .iter()
                .any(|candidate| candidate == signal)
                || !self.supported_pids.contains(&signal.pid)
            {
                return Err(LogNookError::new(format!(
                    "Unsupported read-only Mode 01 signal: {}",
                    signal.id
                )));
            }
            let command = format!("01{:02X}", signal.pid);
            let response_text = self.command(&command)?;
            let responses = parse_mode01_responses(&response_text, 0x01, signal.pid)
                .map_err(LogNookError::new)?;
            if responses.len() != 1 {
                return Err(LogNookError::new(
                    "Multiple ECU responders are not supported by this SX adapter",
                ));
            }
            let response = &responses[0];
            if let (Some(connected_ecu), Some(sample_ecu)) = (&self.ecu_id, &response.ecu_id)
                && connected_ecu != sample_ecu
            {
                return Err(LogNookError::new(
                    "ECU identity changed during the SX session",
                ));
            }
            let elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
            samples.push(Sample {
                elapsed_ns,
                wall_time_unix_ns: now_unix_ns(),
                source_id: format!("obdlink-sx:{serial_number}"),
                ecu_id: response.ecu_id.clone().or_else(|| self.ecu_id.clone()),
                signal_id: signal.id.clone(),
                value: decode_signal(signal, response).map_err(LogNookError::new)?,
                unit: signal.unit.clone(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: Some(response.raw_bytes.clone()),
            });
        }
        Ok(samples)
    }

    fn disconnect(&mut self) {
        self.transport.close();
        self.serial_number = None;
        self.supported_pids.clear();
        self.ecu_id = None;
        self.started = None;
    }
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
