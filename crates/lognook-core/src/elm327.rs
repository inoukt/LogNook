use crate::{
    BluetoothByteStream, ConnectionCapabilities, DeviceDescriptor, HardwareAdapter, LogNookError,
    OemCandidateTryResult, OemCandidateTrySpec, Sample, SampleOrigin, SampleQuality,
    SignalDefinition,
    bluetooth_spp::connect_bluetooth_spp,
    build_oem_candidate_command, decode_oem_candidate_response,
    obdlink_sx_protocol::{
        Mode01Response, Mode01ResponseError, decode_signal, parse_mode01_responses,
        parse_supported_pids,
    },
    standard_obd_signals,
};
use std::{
    io::{ErrorKind, Read, Write},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ELM_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_ELM_RESPONSE_BYTES: usize = 4096;

pub struct Elm327Adapter {
    address: String,
    stream: Option<Box<dyn BluetoothByteStream>>,
    firmware: Option<String>,
    supported_pids: Vec<u8>,
    ecu_id: Option<String>,
    started: Option<Instant>,
}

impl Elm327Adapter {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            stream: None,
            firmware: None,
            supported_pids: Vec::new(),
            ecu_id: None,
            started: None,
        }
    }

    #[cfg(test)]
    fn with_stream(address: impl Into<String>, stream: Box<dyn BluetoothByteStream>) -> Self {
        let mut adapter = Self::new(address);
        adapter.stream = Some(stream);
        adapter
    }

    fn open(&mut self) -> Result<(), LogNookError> {
        if self.stream.is_none() {
            self.stream = Some(connect_bluetooth_spp(&self.address)?);
        }
        Ok(())
    }

    fn command(&mut self, command: &str) -> Result<String, LogNookError> {
        if !is_allowed_command(command) {
            return Err(LogNookError::new(
                "ELM command is outside the read-only allowlist",
            ));
        }
        self.exchange(command)
    }

    fn exchange(&mut self, command: &str) -> Result<String, LogNookError> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| LogNookError::new("ELM327 Bluetooth connection is closed"))?;
        stream
            .write_all(command.as_bytes())
            .and_then(|()| stream.write_all(b"\r"))
            .and_then(|()| stream.flush())
            .map_err(|error| LogNookError::new(format!("Could not send ELM command: {error}")))?;

        let deadline = Instant::now() + ELM_RESPONSE_TIMEOUT;
        let mut response = Vec::new();
        let mut chunk = [0_u8; 128];
        loop {
            let read = stream.read(&mut chunk);
            match read {
                Ok(0) => {
                    return Err(LogNookError::new(
                        "ELM327 Bluetooth link closed before the > prompt",
                    ));
                }
                Ok(count) => {
                    let end = chunk[..count].iter().position(|byte| *byte == b'>');
                    let accepted = end.map_or(count, |end| end + 1);
                    if response.len() + accepted > MAX_ELM_RESPONSE_BYTES {
                        return Err(LogNookError::new(
                            "ELM327 response exceeded the 4096-byte limit",
                        ));
                    }
                    response.extend_from_slice(&chunk[..accepted]);
                    if end.is_some() {
                        return Ok(String::from_utf8_lossy(&response).replace('\0', ""));
                    }
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    if Instant::now() >= deadline {
                        return Err(LogNookError::transient(format!(
                            "Timed out waiting for the ELM327 > prompt after {command}"
                        )));
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(LogNookError::new(format!(
                        "ELM327 Bluetooth read failed: {error}"
                    )));
                }
            }
            if response.len() >= MAX_ELM_RESPONSE_BYTES {
                return Err(LogNookError::new(
                    "ELM327 response exceeded the 4096-byte limit",
                ));
            }
        }
    }

    fn set_candidate_header(&mut self, header: &str) -> Result<(), LogNookError> {
        if header.len() != 3
            || !header.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !u16::from_str_radix(header, 16).is_ok_and(|header| header <= 0x7FF)
        {
            return Err(LogNookError::new(
                "Invalid source-backed ELM CAN request header",
            ));
        }
        let command = format!("ATSH{}", header.to_ascii_uppercase());
        if !elm_response_value(&self.exchange(&command)?, &command)?.eq_ignore_ascii_case("OK") {
            return Err(LogNookError::new(
                "ELM327 rejected the source-backed CAN header",
            ));
        }
        Ok(())
    }

    fn read_source_candidate(
        &mut self,
        spec: &OemCandidateTrySpec,
    ) -> Result<(String, String), LogNookError> {
        build_oem_candidate_command(spec)?;
        let command = format!(
            "{}{}",
            spec.candidate.service.to_ascii_uppercase(),
            spec.candidate.identifier.to_ascii_uppercase()
        );
        let response = self.exchange(&command)?;
        Ok((command, response))
    }

    fn identify(&mut self) -> Result<DeviceDescriptor, LogNookError> {
        let response = self.command("ATI")?;
        let firmware = elm_response_value(&response, "ATI")?;
        if !firmware.to_ascii_uppercase().contains("ELM327") {
            return Err(LogNookError::new(format!(
                "Bluetooth device did not identify as ELM327-compatible ({firmware})"
            )));
        }
        self.firmware = Some(firmware.clone());
        let address = crate::parse_bluetooth_address(&self.address)?;
        Ok(DeviceDescriptor {
            stable_id: format!("bt-elm327-{}", crate::format_bluetooth_address(address)),
            hardware_family: "ELM327 Bluetooth SPP".into(),
            firmware: Some(firmware),
            path: Some(crate::format_bluetooth_address(address)),
        })
    }

    fn configure(&mut self) -> Result<(), LogNookError> {
        for command in ["ATE0", "ATL0", "ATS1", "ATH1", "ATSP0"] {
            let response = self.command(command)?;
            if !elm_response_value(&response, command)?.eq_ignore_ascii_case("OK") {
                return Err(LogNookError::new(format!(
                    "ELM327 rejected adapter setup command {command}"
                )));
            }
        }
        Ok(())
    }

    fn discover_supported_pids(&mut self) -> Result<(), LogNookError> {
        let mut start = 0;
        loop {
            let response = self.command(&format!("01{start:02X}"))?;
            let range = parse_supported_pids(&response, start).map_err(LogNookError::new)?;
            if let (Some(previous), Some(current)) = (&self.ecu_id, &range.ecu_id)
                && previous != current
            {
                return Err(LogNookError::new(
                    "ECU identity changed during ELM supported-PID discovery",
                ));
            }
            if self.ecu_id.is_none() {
                self.ecu_id = range.ecu_id.clone();
            }
            self.supported_pids.extend(range.pids);
            if !range.has_more || start == 0xE0 {
                break;
            }
            start += 0x20;
        }
        self.supported_pids.sort_unstable();
        self.supported_pids.dedup();
        if !standard_obd_signals()
            .iter()
            .any(|signal| self.supported_pids.contains(&signal.pid))
        {
            return Err(LogNookError::new(
                "ECU reports no supported LogNook Mode 01 signals",
            ));
        }
        Ok(())
    }

    fn connect_inner(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        self.open()?;
        let device = self.identify()?;
        self.configure()?;
        self.discover_supported_pids()?;
        self.started = Some(Instant::now());
        let supported_signals = standard_obd_signals()
            .into_iter()
            .filter(|signal| self.supported_pids.contains(&signal.pid))
            .collect();
        Ok(ConnectionCapabilities {
            device,
            protocol: "SAE J1979 Mode 01 via ELM327".into(),
            read_only: true,
            supported_signals,
        })
    }
}

impl HardwareAdapter for Elm327Adapter {
    fn execute_research_test(
        &mut self,
        request: crate::ResearchTestRequest,
    ) -> Result<crate::ResearchTestResult, LogNookError> {
        let crate::ResearchTestRequest::OemCandidate(spec) = request else {
            return Err(LogNookError::new(
                "ELM327 does not support A30 captured read tests",
            ));
        };
        if self.started.is_none() {
            return Err(LogNookError::new("ELM327 adapter is not connected"));
        }
        build_oem_candidate_command(&spec)?;
        let result = self.read_connected_candidate(&spec);
        if self.stream.is_some() {
            // Mode 01 acquisition uses functional addressing again after research.
            self.set_candidate_header("7DF")?;
        }
        result.map(crate::ResearchTestResult::OemCandidate)
    }

    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        if self.started.is_some() {
            return Err(LogNookError::new("ELM327 adapter is already connected"));
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
            .ok_or_else(|| LogNookError::new("ELM327 adapter is not connected"))?;
        let catalog = standard_obd_signals();
        for signal in signals {
            if !catalog.contains(signal) || !self.supported_pids.contains(&signal.pid) {
                return Err(LogNookError::new(format!(
                    "Unsupported read-only Mode 01 signal: {}",
                    signal.id
                )));
            }
        }

        signals
            .iter()
            .map(|signal| {
                let response_text = self.command(&format!("01{:02X}", signal.pid))?;
                let mut responses = parse_mode01_responses(&response_text, 0x01, signal.pid)
                    .map_err(elm_response_error)?;
                if responses.len() != 1 {
                    return Err(LogNookError::new(
                        "Multiple ECU responders are not supported by this ELM adapter",
                    ));
                }
                let response = responses.pop().expect("one response was checked");
                sample_from_response(
                    signal,
                    &response,
                    &self.address,
                    started,
                    self.ecu_id.as_deref(),
                )
            })
            .collect()
    }

    fn disconnect(&mut self) {
        self.stream = None;
        self.firmware = None;
        self.supported_pids.clear();
        self.ecu_id = None;
        self.started = None;
    }
}

pub fn identify_elm327(address: impl Into<String>) -> Result<DeviceDescriptor, LogNookError> {
    let mut adapter = Elm327Adapter::new(address);
    adapter.open()?;
    let result = adapter.identify();
    adapter.disconnect();
    result
}

pub fn try_elm_oem_candidate(
    address: impl Into<String>,
    spec: &OemCandidateTrySpec,
) -> Result<OemCandidateTryResult, LogNookError> {
    build_oem_candidate_command(spec)?;
    let address = address.into();
    let stream = connect_bluetooth_spp(&address)?;
    run_elm_oem_candidate_with_stream(address, stream, spec)
}

fn run_elm_oem_candidate_with_stream(
    address: String,
    stream: Box<dyn BluetoothByteStream>,
    spec: &OemCandidateTrySpec,
) -> Result<OemCandidateTryResult, LogNookError> {
    let mut adapter = Elm327Adapter::new(address);
    adapter.stream = Some(stream);
    adapter.identify()?;
    adapter.configure()?;
    let result = adapter.read_connected_candidate(spec);
    adapter.disconnect();
    result
}

impl Elm327Adapter {
    fn read_connected_candidate(
        &mut self,
        spec: &OemCandidateTrySpec,
    ) -> Result<OemCandidateTryResult, LogNookError> {
        build_oem_candidate_command(spec)?;
        let device = DeviceDescriptor {
            stable_id: format!(
                "bt-elm327-{}",
                crate::format_bluetooth_address(crate::parse_bluetooth_address(&self.address)?)
            ),
            hardware_family: "ELM327 Bluetooth SPP".into(),
            firmware: self.firmware.clone(),
            path: Some(crate::format_bluetooth_address(
                crate::parse_bluetooth_address(&self.address)?,
            )),
        };
        self.set_candidate_header(&spec.candidate.request_header)?;

        let started_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let request_started = Instant::now();
        let response = self.read_source_candidate(spec);
        let latency_ms = request_started.elapsed().as_millis() as u64;
        let (raw_response, decoded) = match response {
            Ok((_command, raw)) => (raw.clone(), decode_oem_candidate_response(spec, &raw)),
            Err(error) => {
                let mut decoded = decode_oem_candidate_response(spec, "");
                decoded.error = Some(error.to_string());
                (String::new(), decoded)
            }
        };
        let exact_adapter_command = format!(
            "ATSH{}\\r{}{}\\r",
            spec.candidate.request_header.to_ascii_uppercase(),
            spec.candidate.service.to_ascii_uppercase(),
            spec.candidate.identifier.to_ascii_uppercase(),
        );
        Ok(OemCandidateTryResult {
            candidate_id: spec.candidate.candidate_id.clone(),
            exact_adapter_command,
            adapter: device,
            protocol: "ISO 15765-4 (CAN 11/500) via ELM327 SPP".into(),
            started_at_unix_ms,
            latency_ms,
            outcome: decoded.outcome,
            raw_response,
            responder_header: decoded.responder_header,
            response_payload: decoded.payload,
            negative_response_code: decoded.negative_response_code,
            decoded_value: decoded.decoded_value,
            decode_error: decoded.decode_error,
            error: decoded.error,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }
}

fn is_allowed_command(command: &str) -> bool {
    if matches!(command, "ATI" | "ATE0" | "ATL0" | "ATS1" | "ATH1" | "ATSP0") {
        return true;
    }
    let Some(pid) = command
        .strip_prefix("01")
        .filter(|pid| pid.len() == 2)
        .and_then(|pid| u8::from_str_radix(pid, 16).ok())
    else {
        return false;
    };
    matches!(pid, 0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 | 0xC0 | 0xE0)
        || standard_obd_signals()
            .iter()
            .any(|signal| signal.pid == pid)
}

fn elm_response_value(response: &str, command: &str) -> Result<String, LogNookError> {
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
        .ok_or_else(|| LogNookError::new(format!("ELM327 returned no value for {command}")))
}

fn elm_response_error(error: Mode01ResponseError) -> LogNookError {
    if error.is_transient() {
        LogNookError::transient(error.to_string())
    } else {
        LogNookError::new(error.to_string())
    }
}

fn sample_from_response(
    signal: &SignalDefinition,
    response: &Mode01Response,
    address: &str,
    started: Instant,
    connected_ecu: Option<&str>,
) -> Result<Sample, LogNookError> {
    if let (Some(connected_ecu), Some(sample_ecu)) = (connected_ecu, response.ecu_id.as_deref())
        && connected_ecu != sample_ecu
    {
        return Err(LogNookError::new(
            "ECU identity changed during the ELM session",
        ));
    }
    let address = crate::format_bluetooth_address(crate::parse_bluetooth_address(address)?);
    Ok(Sample {
        elapsed_ns: started.elapsed().as_nanos().min(i64::MAX as u128) as i64,
        wall_time_unix_ns: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(i64::MAX as u128) as i64,
        source_id: format!("elm327:{address}"),
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

#[cfg(test)]
mod tests {
    use super::{Elm327Adapter, run_elm_oem_candidate_with_stream};
    use crate::{
        HardwareAdapter, OemCandidate, OemCandidateOutcome, OemCandidateTrySpec, SampleQuality,
        standard_obd_signals,
    };
    use std::{
        collections::VecDeque,
        io::{self, Read, Write},
        sync::{Arc, Mutex},
    };

    struct ScriptedStream {
        incoming: VecDeque<Vec<u8>>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl ScriptedStream {
        fn new(responses: impl IntoIterator<Item = Vec<u8>>) -> (Self, Arc<Mutex<Vec<u8>>>) {
            let written = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    incoming: responses.into_iter().collect(),
                    written: written.clone(),
                },
                written,
            )
        }
    }

    impl Read for ScriptedStream {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let Some(mut chunk) = self.incoming.pop_front() else {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "script exhausted"));
            };
            let count = output.len().min(chunk.len());
            output[..count].copy_from_slice(&chunk[..count]);
            if count < chunk.len() {
                chunk.drain(..count);
                self.incoming.push_front(chunk);
            }
            Ok(count)
        }
    }

    impl Write for ScriptedStream {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(input);
            Ok(input.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn connected_adapter(
        extra: &[&str],
    ) -> (
        Elm327Adapter,
        crate::ConnectionCapabilities,
        Arc<Mutex<Vec<u8>>>,
    ) {
        let replies = [
            "ELM327 v2.2\r\0>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "SEARCHING...\r7E8 06 41 00 98 3A 80 13\r>",
            "7E8 06 41 20 80 00 00 01\r>",
            "7E8 06 41 40 00 00 00 00\r>",
        ];
        let mut replies = replies
            .into_iter()
            .chain(extra.iter().copied())
            .map(|reply| reply.as_bytes().to_vec())
            .collect::<Vec<_>>();
        let banner = replies.remove(0);
        let mut fragmented_banner = vec![banner[..5].to_vec(), banner[5..12].to_vec()];
        fragmented_banner.push(banner[12..].to_vec());
        fragmented_banner.extend(replies);
        let (stream, written) = ScriptedStream::new(fragmented_banner);
        let mut adapter = Elm327Adapter::with_stream("AA:BB:CC:DD:EE:FF", Box::new(stream));
        let capabilities = adapter.connect().unwrap();
        (adapter, capabilities, written)
    }

    fn source_candidate() -> OemCandidateTrySpec {
        OemCandidateTrySpec {
            value_format: "uint".into(),
            candidate: OemCandidate {
                candidate_id: "elm-source-candidate".into(),
                source_id: "reviewed-source".into(),
                family: "Toyota".into(),
                brand: "Scion".into(),
                model_scope: "xB 2014".into(),
                service: "21".into(),
                identifier: "01".into(),
                request_header: "7E0".into(),
                response_header: "7E8".into(),
                frequency_hz: None,
                module_type: "Engine".into(),
                category: "Engine".into(),
                name: "Scion RPM".into(),
                signal_source_id: "test".into(),
                bit_offset: Some(0),
                bit_length: Some(8),
                formula: "x".into(),
                decoder_json: r#"{"bix":0,"len":8,"unit":"raw"}"#.into(),
                unit: "raw".into(),
                year_filter_json: "{}".into(),
                ecu_filter: "7E8".into(),
                source_blob_sha: "reviewed-sha".into(),
                research_state: "community_reference".into(),
                vehicle_validation: "not_tested".into(),
                auto_request: false,
                confidence: "research".into(),
                notes: String::new(),
                suggested_metric: String::new(),
                source_scope: "exact test vehicle".into(),
                applicability_specificity: 1,
                min_value: None,
                max_value: None,
                source_url: Some("https://source.test/capture".into()),
                license: Some("CC BY-SA 4.0".into()),
                license_url: Some("https://source.test/license".into()),
            },
        }
    }

    fn source_mode22_candidate() -> OemCandidateTrySpec {
        let mut spec = source_candidate();
        spec.candidate.candidate_id = "elm-source-did".into();
        spec.candidate.service = "22".into();
        spec.candidate.identifier = "1234".into();
        spec
    }

    #[test]
    fn elm_candidate_reuses_paired_stream() {
        let (mut adapter, capabilities, written) = connected_adapter(&[
            "OK\r>",
            "7E8 03 61 01 64\r>",
            "OK\r>",
            "OK\r>",
            "7E8 04 62 12 34 64\r>",
            "OK\r>",
            "7E8 04 41 0C 1A F8\r>",
        ]);
        written.lock().unwrap().clear();
        let crate::ResearchTestResult::OemCandidate(result) = adapter
            .execute_research_test(crate::ResearchTestRequest::OemCandidate(source_candidate()))
            .unwrap()
        else {
            panic!("expected candidate result")
        };

        assert_eq!(result.adapter, capabilities.device);
        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(&*written.lock().unwrap(), b"ATSH7E0\r2101\rATSH7DF\r");
        assert!(adapter.started.is_some());
        let crate::ResearchTestResult::OemCandidate(result) = adapter
            .execute_research_test(crate::ResearchTestRequest::OemCandidate(
                source_mode22_candidate(),
            ))
            .unwrap()
        else {
            panic!("expected candidate result")
        };
        assert_eq!(result.response_payload, [0x62, 0x12, 0x34, 0x64]);
        let rpm = capabilities
            .supported_signals
            .iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();
        assert_eq!(
            adapter.read_samples(std::slice::from_ref(rpm)).unwrap()[0].value,
            1726.0
        );
        assert_eq!(
            &*written.lock().unwrap(),
            b"ATSH7E0\r2101\rATSH7DF\rATSH7E0\r221234\rATSH7DF\r010C\r"
        );

        written.lock().unwrap().clear();
        let mut invalid = source_candidate();
        invalid.candidate.identifier = "0101".into();
        assert!(
            adapter
                .execute_research_test(crate::ResearchTestRequest::OemCandidate(invalid))
                .is_err()
        );
        assert!(
            adapter
                .execute_research_test(crate::ResearchTestRequest::A30ReadTest(
                    crate::A30ReadTest::Mode03StoredDtcs
                ))
                .is_err()
        );
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn identifies_elm327_and_discovers_supported_pid_continuations() {
        let (_adapter, capabilities, written) = connected_adapter(&[]);
        assert_eq!(capabilities.device.hardware_family, "ELM327 Bluetooth SPP");
        assert_eq!(capabilities.device.firmware.as_deref(), Some("ELM327 v2.2"));
        assert!(capabilities.read_only);
        assert!(
            capabilities
                .supported_signals
                .contains(&standard_obd_signals()[0])
        );
        let commands = String::from_utf8(written.lock().unwrap().clone()).unwrap();
        assert!(commands.contains("ATI\rATE0\rATL0\rATS1\rATH1\rATSP0\r0100\r0120\r0140\r"));
    }

    #[test]
    fn selected_mode01_pid_emits_timestamped_sample_through_the_adapter() {
        let (mut adapter, _capabilities, _written) = connected_adapter(&["7E8 04 41 0C 1A F8\r>"]);
        let rpm = standard_obd_signals()
            .into_iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();

        let samples = adapter.read_samples(&[rpm]).unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].value, 1726.0);
        assert!(samples[0].elapsed_ns >= 0);
        assert!(samples[0].wall_time_unix_ns > 0);
        assert_eq!(samples[0].ecu_id.as_deref(), Some("7E8"));
        assert_eq!(samples[0].quality, SampleQuality::Valid);
    }

    #[test]
    fn negative_or_wrong_pid_reply_does_not_emit_a_sample() {
        let (mut adapter, _capabilities, _written) = connected_adapter(&["7E8 03 7F 01 12\r>"]);
        let rpm = standard_obd_signals()
            .into_iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();

        assert!(adapter.read_samples(&[rpm]).is_err());
    }

    #[test]
    fn a_wrong_positive_pid_echo_is_rejected() {
        let (mut adapter, _capabilities, _written) = connected_adapter(&["7E8 04 41 05 5A\r>"]);
        let rpm = standard_obd_signals()
            .into_iter()
            .find(|signal| signal.pid == 0x0C)
            .unwrap();

        assert!(adapter.read_samples(&[rpm]).is_err());
    }

    #[test]
    fn the_text_protocol_cannot_send_clear_write_or_arbitrary_requests() {
        let (stream, written) = ScriptedStream::new([]);
        let mut adapter = Elm327Adapter::with_stream("AA:BB:CC:DD:EE:FF", Box::new(stream));

        for command in ["04", "220101", "STPX", "01FF"] {
            assert!(adapter.command(command).is_err(), "{command}");
        }
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn source_backed_service21_candidate_can_be_tried_over_elm_bluetooth() {
        let replies = [
            "ELM327 v2.2\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "7E8 03 61 01 64\r>",
        ];
        let (stream, written) =
            ScriptedStream::new(replies.iter().map(|reply| reply.as_bytes().to_vec()));

        let result = run_elm_oem_candidate_with_stream(
            "AA:BB:CC:DD:EE:FF".into(),
            Box::new(stream),
            &source_candidate(),
        )
        .unwrap();

        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(result.responder_header.as_deref(), Some("7E8"));
        assert_eq!(result.decoded_value.unwrap().value, Some(100.0));
        assert!(result.exact_adapter_command.contains("ATSH7E0"));
        assert!(result.exact_adapter_command.contains("2101"));
        let commands = String::from_utf8(written.lock().unwrap().clone()).unwrap();
        assert!(commands.ends_with("ATSH7E0\r2101\r"));
    }

    #[test]
    fn source_backed_service22_did_can_be_tried_over_elm_bluetooth() {
        let replies = [
            "ELM327 v2.2\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "OK\r>",
            "7E8 04 62 12 34 64\r>",
        ];
        let (stream, written) =
            ScriptedStream::new(replies.iter().map(|reply| reply.as_bytes().to_vec()));

        let result = run_elm_oem_candidate_with_stream(
            "AA:BB:CC:DD:EE:FF".into(),
            Box::new(stream),
            &source_mode22_candidate(),
        )
        .unwrap();

        assert_eq!(result.outcome, OemCandidateOutcome::Positive);
        assert_eq!(result.response_payload, [0x62, 0x12, 0x34, 0x64]);
        assert_eq!(result.decoded_value.unwrap().value, Some(100.0));
        assert!(
            String::from_utf8(written.lock().unwrap().clone())
                .unwrap()
                .ends_with("ATSH7E0\r221234\r")
        );
    }
}
