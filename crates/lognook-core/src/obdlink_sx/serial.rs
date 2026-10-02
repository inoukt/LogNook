use crate::LogNookError;
use serialport::{DataBits, FlowControl, Parity, SerialPortType, StopBits};
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    time::{Duration, Instant},
};

const BAUD_RATE: u32 = 115_200;
const IO_TIMEOUT: Duration = Duration::from_millis(100);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const MAX_RESPONSE_BYTES: usize = 4096;

pub(super) trait PortIo: Read + Write + Send {}
impl<T: Read + Write + Send> PortIo for T {}

struct SerialPortIo(Box<dyn serialport::SerialPort>);

impl Read for SerialPortIo {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for SerialPortIo {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

pub(super) struct SerialTransport {
    port_name: String,
    port: Option<Box<dyn PortIo>>,
    pending: VecDeque<u8>,
}

impl SerialTransport {
    pub(super) fn new(port_name: String) -> Self {
        Self {
            port_name,
            port: None,
            pending: VecDeque::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn with_port(port_name: String, port: Box<dyn PortIo>) -> Self {
        Self {
            port_name,
            port: Some(port),
            pending: VecDeque::new(),
        }
    }

    pub(super) fn port_name(&self) -> &str {
        &self.port_name
    }

    pub(super) fn open(&mut self) -> Result<(), LogNookError> {
        if self.port.is_some() {
            return Ok(());
        }
        let ports = serialport::available_ports().map_err(|error| {
            LogNookError::new(format!("Could not verify OBDLink SX port: {error}"))
        })?;
        if !ports
            .iter()
            .any(|port| is_selected_sx_port(&self.port_name, port))
        {
            return Err(LogNookError::new(format!(
                "Port {} is not currently enumerated as an OBDLink SX (USB VID 0403, PID 6015)",
                self.port_name
            )));
        }
        let port = serialport::new(&self.port_name, BAUD_RATE)
            .data_bits(DataBits::Eight)
            .flow_control(FlowControl::None)
            .parity(Parity::None)
            .stop_bits(StopBits::One)
            .timeout(IO_TIMEOUT)
            .open()
            .map_err(|error| {
                LogNookError::new(format!(
                    "Could not open OBDLink SX port {}: {error}",
                    self.port_name
                ))
            })?;
        self.port = Some(Box::new(SerialPortIo(port)));
        Ok(())
    }

    pub(super) fn command(&mut self, command: &str) -> Result<String, LogNookError> {
        let port = self
            .port
            .as_mut()
            .ok_or_else(|| LogNookError::new("OBDLink SX port is closed"))?;
        port.write_all(command.as_bytes())?;
        port.write_all(b"\r")?;
        port.flush()?;
        self.read_prompt()
    }

    pub(super) fn close(&mut self) {
        self.port = None;
        self.pending.clear();
    }

    pub(super) fn read_prompt(&mut self) -> Result<String, LogNookError> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let mut response = Vec::new();
        let mut chunk = [0_u8; 128];

        loop {
            while let Some(byte) = self.pending.pop_front() {
                response.push(byte);
                if response.len() > MAX_RESPONSE_BYTES {
                    return Err(LogNookError::new(
                        "OBDLink response exceeded the 4096-byte limit",
                    ));
                }
                if byte == b'>' {
                    return Ok(String::from_utf8_lossy(&response).into_owned());
                }
            }

            if response.len() >= MAX_RESPONSE_BYTES {
                return Err(LogNookError::new(
                    "OBDLink response exceeded the 4096-byte limit",
                ));
            }
            let read = self
                .port
                .as_mut()
                .ok_or_else(|| LogNookError::new("OBDLink SX port is closed"))?
                .read(&mut chunk);
            match read {
                Ok(0) => return Err(LogNookError::new("OBDLink SX did not provide a > prompt")),
                Ok(count) => {
                    let end = chunk[..count].iter().position(|byte| *byte == b'>');
                    let accepted = end.map_or(count, |end| end + 1);
                    if response.len() + accepted > MAX_RESPONSE_BYTES {
                        return Err(LogNookError::new(
                            "OBDLink response exceeded the 4096-byte limit",
                        ));
                    }
                    if let Some(end) = end {
                        response.extend_from_slice(&chunk[..=end]);
                        self.pending.extend(&chunk[end + 1..count]);
                        return Ok(String::from_utf8_lossy(&response).into_owned());
                    }
                    response.extend_from_slice(&chunk[..count]);
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    if Instant::now() >= deadline {
                        return Err(LogNookError::new(
                            "Timed out waiting for the OBDLink SX > prompt",
                        ));
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(LogNookError::new(format!(
                        "OBDLink SX serial read failed: {error}"
                    )));
                }
            }
        }
    }
}

pub(super) fn is_sx_usb(vid: u16, pid: u16) -> bool {
    vid == 0x0403 && pid == 0x6015
}

pub(super) fn is_selected_sx_port(port_name: &str, port: &serialport::SerialPortInfo) -> bool {
    port.port_name == port_name
        && matches!(&port.port_type, SerialPortType::UsbPort(info) if is_sx_usb(info.vid, info.pid))
}

pub fn available_obdlink_sx_ports() -> Result<Vec<String>, LogNookError> {
    let ports = serialport::available_ports()
        .map_err(|error| LogNookError::new(format!("Could not enumerate serial ports: {error}")))?;
    let mut matches: Vec<_> = ports
        .into_iter()
        .filter_map(|port| match port.port_type {
            SerialPortType::UsbPort(info) if is_sx_usb(info.vid, info.pid) => Some(port.port_name),
            _ => None,
        })
        .collect();
    matches.sort();
    Ok(matches)
}
