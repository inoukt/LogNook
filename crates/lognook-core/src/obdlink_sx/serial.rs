use crate::{
    LogNookError,
    communication_trace::{
        CommunicationDirection, CommunicationSink, CommunicationTraceEvent, bytes_to_hex,
    },
};
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
    incomplete_response: Option<Vec<u8>>,
    incomplete_response_truncated: bool,
    communication_sink: Option<CommunicationSink>,
}

impl SerialTransport {
    pub(super) fn new(port_name: String) -> Self {
        Self {
            port_name,
            port: None,
            pending: VecDeque::new(),
            incomplete_response: None,
            incomplete_response_truncated: false,
            communication_sink: None,
        }
    }

    #[cfg(test)]
    pub(super) fn with_port(port_name: String, port: Box<dyn PortIo>) -> Self {
        Self {
            port_name,
            port: Some(port),
            pending: VecDeque::new(),
            incomplete_response: None,
            incomplete_response_truncated: false,
            communication_sink: None,
        }
    }

    pub(super) fn set_communication_sink(&mut self, sink: CommunicationSink) {
        self.communication_sink = Some(sink);
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
        let started = Instant::now();
        let port = self
            .port
            .as_mut()
            .ok_or_else(|| LogNookError::new("OBDLink SX port is closed"))?;
        let mut request = command.as_bytes().to_vec();
        request.push(b'\r');
        let write_result = port
            .write_all(command.as_bytes())
            .map_err(|error| {
                LogNookError::new(format!(
                    "Could not write to the OBDLink SX serial port: {error}"
                ))
            })
            .and_then(|()| {
                port.write_all(b"\r").map_err(|error| {
                    LogNookError::new(format!(
                        "Could not terminate the OBDLink SX command: {error}"
                    ))
                })
            })
            .and_then(|()| {
                port.flush().map_err(|error| {
                    LogNookError::new(format!("Could not flush the OBDLink SX command: {error}"))
                })
            });
        self.trace(
            CommunicationDirection::Tx,
            Some(&request),
            started,
            if write_result.is_ok() {
                "sent"
            } else {
                "write_error"
            },
            write_result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .as_deref(),
        );
        write_result?;
        match self.read_prompt_for_command(Some(command)) {
            Ok(response) => {
                self.trace(
                    CommunicationDirection::Rx,
                    Some(&response),
                    started,
                    "response",
                    None,
                );
                Ok(String::from_utf8_lossy(&response).into_owned())
            }
            Err(error) => {
                let truncated = std::mem::take(&mut self.incomplete_response_truncated);
                let partial = self.incomplete_response.take();
                let outcome = if truncated {
                    "response_truncated"
                } else {
                    serial_error_outcome(&error)
                };
                let unavailable = if truncated {
                    Some(
                        "Response exceeded the 4096-byte trace limit; only a prefix was retained"
                            .to_owned(),
                    )
                } else {
                    partial
                        .is_none()
                        .then(|| format!("serial response bytes unavailable: {error}"))
                };
                self.trace(
                    CommunicationDirection::Rx,
                    partial.as_deref(),
                    started,
                    outcome,
                    unavailable.as_deref(),
                );
                Err(error)
            }
        }
    }

    pub(super) fn close(&mut self) {
        self.port = None;
        self.pending.clear();
    }

    pub(super) fn read_prompt(&mut self) -> Result<String, LogNookError> {
        let started = Instant::now();
        match self.read_prompt_for_command(None) {
            Ok(response) => {
                self.trace(
                    CommunicationDirection::Rx,
                    Some(&response),
                    started,
                    "prompt",
                    None,
                );
                Ok(String::from_utf8_lossy(&response).into_owned())
            }
            Err(error) => {
                let truncated = std::mem::take(&mut self.incomplete_response_truncated);
                let partial = self.incomplete_response.take();
                let outcome = if truncated {
                    "response_truncated"
                } else {
                    serial_error_outcome(&error)
                };
                let unavailable = if truncated {
                    Some(
                        "Response exceeded the 4096-byte trace limit; only a prefix was retained"
                            .to_owned(),
                    )
                } else {
                    partial
                        .is_none()
                        .then(|| format!("serial prompt bytes unavailable: {error}"))
                };
                self.trace(
                    CommunicationDirection::Rx,
                    partial.as_deref(),
                    started,
                    outcome,
                    unavailable.as_deref(),
                );
                Err(error)
            }
        }
    }

    fn read_prompt_for_command(&mut self, command: Option<&str>) -> Result<Vec<u8>, LogNookError> {
        self.incomplete_response = None;
        self.incomplete_response_truncated = false;
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let mut response = Vec::new();
        let mut chunk = [0_u8; 128];

        loop {
            while let Some(byte) = self.pending.pop_front() {
                response.push(byte);
                if response.len() > MAX_RESPONSE_BYTES {
                    response.truncate(MAX_RESPONSE_BYTES);
                    self.incomplete_response = Some(response);
                    self.incomplete_response_truncated = true;
                    return Err(LogNookError::new(
                        "OBDLink response exceeded the 4096-byte limit",
                    ));
                }
                if byte == b'>' {
                    self.incomplete_response = None;
                    return Ok(response);
                }
            }

            if response.len() >= MAX_RESPONSE_BYTES {
                self.incomplete_response = Some(response.clone());
                self.incomplete_response_truncated = true;
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
                Ok(0) => {
                    self.incomplete_response = Some(response.clone());
                    return Err(LogNookError::new("OBDLink SX did not provide a > prompt"));
                }
                Ok(count) => {
                    let end = chunk[..count].iter().position(|byte| *byte == b'>');
                    let accepted = end.map_or(count, |end| end + 1);
                    if response.len() + accepted > MAX_RESPONSE_BYTES {
                        response.extend_from_slice(
                            &chunk[..count.min(MAX_RESPONSE_BYTES.saturating_sub(response.len()))],
                        );
                        self.incomplete_response = Some(response);
                        self.incomplete_response_truncated = true;
                        return Err(LogNookError::new(
                            "OBDLink response exceeded the 4096-byte limit",
                        ));
                    }
                    if let Some(end) = end {
                        response.extend_from_slice(&chunk[..=end]);
                        self.pending.extend(&chunk[end + 1..count]);
                        self.incomplete_response = None;
                        return Ok(response);
                    }
                    response.extend_from_slice(&chunk[..count]);
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    if Instant::now() >= deadline {
                        return if response.is_empty()
                            || command
                                .is_some_and(|command| is_only_command_echo(&response, command))
                        {
                            self.incomplete_response = Some(response);
                            Err(LogNookError::transient(
                                "Timed out waiting for the OBDLink SX > prompt",
                            ))
                        } else {
                            self.incomplete_response = Some(response);
                            Err(LogNookError::new(
                                "OBDLink SX response ended before the > prompt",
                            ))
                        };
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => {
                    self.incomplete_response = Some(response);
                    return Err(LogNookError::new(format!(
                        "OBDLink SX serial read failed: {error}"
                    )));
                }
            }
        }
    }

    fn trace(
        &self,
        direction: CommunicationDirection,
        bytes: Option<&[u8]>,
        started: Instant,
        outcome: &str,
        unavailable: Option<&str>,
    ) {
        let Some(sink) = &self.communication_sink else {
            return;
        };
        let mut event = CommunicationTraceEvent::new(
            format!("obdlink-sx:{}", self.port_name),
            "serial_command",
            direction,
        );
        event.elapsed_ns = Some(started.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        event.bytes_hex = bytes.map(bytes_to_hex);
        event.outcome = outcome.into();
        event.raw_bytes_unavailable = unavailable.map(str::to_owned);
        let _ = sink.try_record(event);
    }
}

fn serial_error_outcome(error: &LogNookError) -> &'static str {
    if error.to_string().contains("Timed out")
        || error.to_string().contains("ended before the > prompt")
    {
        "timeout"
    } else {
        "error"
    }
}

fn is_only_command_echo(response: &[u8], command: &str) -> bool {
    let mut lines = response
        .split(|byte| matches!(byte, b'\r' | b'\n'))
        .filter(|line| !line.is_empty());
    matches!(lines.next(), Some(line) if line.eq_ignore_ascii_case(command.as_bytes()))
        && lines.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::is_only_command_echo;

    #[test]
    fn command_echo_is_not_mistaken_for_a_partial_vehicle_response() {
        assert!(is_only_command_echo(b"\r010C\r\n", "010C"));
        assert!(is_only_command_echo(b"AT SH 7DF\r", "AT SH 7DF"));
        assert!(!is_only_command_echo(b"010C\r7E8 03 7F 01 12", "010C"));
        assert!(!is_only_command_echo(b"010D\r", "010C"));
    }
}

#[cfg(test)]
mod communication_trace_tests {
    use super::{PortIo, SerialTransport};
    use crate::communication_trace::{CommunicationDirection, CommunicationTraceStore};
    use std::{
        collections::VecDeque,
        io::{self, Read, Write},
        sync::{Arc, Mutex},
    };

    struct ScriptedPort {
        incoming: VecDeque<u8>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for ScriptedPort {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.incoming.is_empty() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no reply"));
            }
            let count = buffer.len().min(self.incoming.len());
            for byte in &mut buffer[..count] {
                *byte = self.incoming.pop_front().unwrap();
            }
            Ok(count)
        }
    }

    impl Write for ScriptedPort {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn sx_trace_contains_exact_serial_exchange() {
        let store = CommunicationTraceStore::with_limits(8, 8);
        let written = Arc::new(Mutex::new(Vec::new()));
        let port = ScriptedPort {
            incoming: b"010C\r41 0C 1A F8\r>".iter().copied().collect(),
            written: written.clone(),
        };
        let mut transport =
            SerialTransport::with_port("COM9".into(), Box::new(port) as Box<dyn PortIo>);
        transport.set_communication_sink(store.sink());
        assert!(transport.command("010C").unwrap().ends_with('>'));
        assert_eq!(&*written.lock().unwrap(), b"010C\r");
        let page = store.get_trace(0, 8).unwrap();
        assert_eq!(page.events[0].direction, CommunicationDirection::Tx);
        assert_eq!(page.events[0].bytes_hex.as_deref(), Some("303130430D"));
        assert_eq!(page.events[1].direction, CommunicationDirection::Rx);
        assert_eq!(
            page.events[1].bytes_hex.as_deref(),
            Some("303130430D34312030432031412046380D3E")
        );
    }

    #[test]
    fn sx_timeout_trace_keeps_partial_response_bytes() {
        let store = CommunicationTraceStore::with_limits(8, 8);
        let written = Arc::new(Mutex::new(Vec::new()));
        let port = ScriptedPort {
            incoming: b"010C\r7E8 03 41 0C".iter().copied().collect(),
            written,
        };
        let mut transport =
            SerialTransport::with_port("COM9".into(), Box::new(port) as Box<dyn PortIo>);
        transport.set_communication_sink(store.sink());
        assert!(transport.command("010C").is_err());
        let page = store.get_trace(0, 8).unwrap();
        let response = page
            .events
            .iter()
            .find(|event| event.direction == CommunicationDirection::Rx)
            .unwrap();
        assert_eq!(response.outcome, "timeout");
        assert_eq!(
            response.bytes_hex.as_deref(),
            Some("303130430D374538203033203431203043")
        );
        assert_eq!(response.raw_bytes_unavailable, None);
    }

    #[test]
    fn sx_oversized_response_trace_marks_prefix_as_truncated() {
        let store = CommunicationTraceStore::with_limits(8, 8);
        let mut incoming = b"010C\r".to_vec();
        incoming.extend(std::iter::repeat(b'A').take(super::MAX_RESPONSE_BYTES));
        let port = ScriptedPort {
            incoming: incoming.into(),
            written: Arc::new(Mutex::new(Vec::new())),
        };
        let mut transport =
            SerialTransport::with_port("COM9".into(), Box::new(port) as Box<dyn PortIo>);
        transport.set_communication_sink(store.sink());
        assert!(transport.command("010C").is_err());
        let page = store.get_trace(0, 8).unwrap();
        let response = page
            .events
            .iter()
            .find(|event| event.direction == CommunicationDirection::Rx)
            .unwrap();
        assert_eq!(response.outcome, "response_truncated");
        assert_eq!(
            response.bytes_hex.as_ref().unwrap().len(),
            super::MAX_RESPONSE_BYTES * 2
        );
        assert_eq!(
            response.raw_bytes_unavailable.as_deref(),
            Some("Response exceeded the 4096-byte trace limit; only a prefix was retained")
        );
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
