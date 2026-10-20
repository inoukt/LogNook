use crate::j2534::{J2534Failure, PassThruApi};
use lognook_j2534_protocol::{
    J2534Identity, MAX_SUBARU_SSM_ADDRESSES, Mode01Payload, Mode09Response,
    OemCandidateRequestKind, OemCandidateResponse, StandardDtcResponse, StandardDtcService,
    SubaruSsmReadPayload, WorkerErrorKind, is_supported_mode09_pid,
};
use lognook_j2534_protocol::subaru_ssm::{
    build_ecu_init, build_read_addresses, parse_ssm2_frame,
};
use std::collections::HashSet;
use std::time::Instant;

const SSM2_ADDRESS_BATCH_SIZE: usize = 32;
const SSM2_READ_TIMEOUT_MS: u32 = 2_500;

pub(crate) struct J2534Session {
    api: Box<dyn PassThruApi>,
    device: Option<u32>,
    channel: Option<u32>,
    expected_ecu: Option<u16>,
    subaru_ssm: bool,
    initialized_ssm_ecus: HashSet<u8>,
}

impl J2534Session {
    pub(crate) fn open(api: Box<dyn PassThruApi>) -> Result<Self, J2534Failure> {
        let mut session = Self {
            api,
            device: None,
            channel: None,
            expected_ecu: None,
            subaru_ssm: false,
            initialized_ssm_ecus: HashSet::new(),
        };
        session.device = Some(session.api.open_device()?);
        Ok(session)
    }

    pub(crate) fn identity(&mut self) -> Result<J2534Identity, J2534Failure> {
        let device = self.device.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ProviderNotFound,
                "J2534 provider is not open",
            )
        })?;
        let identity = self.api.read_version(device)?;
        if identity.api_version.trim() != "04.04" {
            return Err(J2534Failure::new(
                WorkerErrorKind::UnsupportedApi,
                format!("Expected J2534 API 04.04, got {}", identity.api_version),
            ));
        }
        Ok(identity)
    }

    #[cfg(test)]
    pub(crate) fn connect(&mut self) -> Result<J2534Identity, J2534Failure> {
        self.connect_with_progress(&mut |_| {})
    }

    pub(crate) fn connect_with_progress(
        &mut self,
        progress: &mut dyn FnMut(&str),
    ) -> Result<J2534Identity, J2534Failure> {
        if self.channel.is_some() {
            return Err(J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is already connected",
            ));
        }
        progress("version_read");
        let identity = self.identity()?;
        let device = self.device.expect("identity checked the device");
        progress("channel_connect");
        let channel = self.api.connect_iso15765(device)?;
        self.channel = Some(channel);
        progress("filter_setup");
        self.api.start_obd_filter(channel)?;
        Ok(identity)
    }

    pub(crate) fn connect_subaru_ssm_with_progress(
        &mut self,
        progress: &mut dyn FnMut(&str),
    ) -> Result<J2534Identity, J2534Failure> {
        if self.channel.is_some() {
            return Err(J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is already connected",
            ));
        }
        progress("version_read");
        let identity = self.identity()?;
        let device = self.device.expect("identity checked the device");
        progress("channel_connect");
        let channel = self.api.connect_subaru_ssm(device)?;
        self.channel = Some(channel);
        progress("filter_setup");
        self.api.start_subaru_ssm_filter(channel)?;
        self.subaru_ssm = true;
        Ok(identity)
    }

    pub(crate) fn read_subaru_ssm_addresses(
        &mut self,
        candidate_ids: &[String],
        source_id: &str,
        catalog_sha256: &str,
        ecu_address: u8,
        tool_address: u8,
        addresses: &[u32],
    ) -> Result<SubaruSsmReadPayload, J2534Failure> {
        if !self.subaru_ssm {
            return Err(J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "OpenPort is not connected in Subaru SSM K-Line mode",
            ));
        }
        lognook_j2534_protocol::validate_subaru_ssm_request_fields(
            candidate_ids,
            source_id,
            catalog_sha256,
            ecu_address,
            tool_address,
            addresses,
        )
        .map_err(|error| J2534Failure::new(WorkerErrorKind::MalformedResponse, error.to_string()))?;
        if ecu_address == 0
            || ecu_address >= tool_address
            || tool_address != 0xf0
            || addresses.is_empty()
            || addresses.len() > MAX_SUBARU_SSM_ADDRESSES
            || addresses.iter().any(|address| *address > 0x00ff_ffff)
        {
            return Err(J2534Failure::new(
                WorkerErrorKind::MalformedResponse,
                "Subaru SSM read needs a valid ECU route and 1-256 24-bit addresses",
            ));
        }
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 K-Line channel is not connected",
            )
        })?;
        let started = Instant::now();
        let mut init_frame = None;
        let mut read_frames = Vec::new();
        let mut values = Vec::with_capacity(addresses.len());
        if !self.initialized_ssm_ecus.contains(&ecu_address) {
            let request = build_ecu_init(ecu_address, tool_address);
            let exchange = self.api.exchange_subaru_ssm(
                channel,
                &request,
                ecu_address,
                tool_address,
                SSM2_READ_TIMEOUT_MS,
            )?;
            let parsed = parse_ssm2_frame(&exchange.response_frame, ecu_address, tool_address)
                .map_err(|detail| {
                    J2534Failure::new(WorkerErrorKind::MalformedResponse, detail)
                })?;
            if parsed.payload.first() != Some(&0xff) {
                return Err(J2534Failure::new(
                    WorkerErrorKind::MalformedResponse,
                    format!(
                        "Subaru SSM ECU init returned an unexpected response byte ({} bytes)",
                        parsed.payload.len()
                    ),
                ));
            }
            init_frame = Some(hex_bytes(&exchange.response_frame));
            self.initialized_ssm_ecus.insert(ecu_address);
        }
        for address_chunk in addresses.chunks(SSM2_ADDRESS_BATCH_SIZE) {
            let request = build_read_addresses(ecu_address, tool_address, address_chunk)
                .map_err(|detail| {
                    J2534Failure::new(WorkerErrorKind::MalformedResponse, detail)
                })?;
            let exchange = self.api.exchange_subaru_ssm(
                channel,
                &request,
                ecu_address,
                tool_address,
                SSM2_READ_TIMEOUT_MS,
            )?;
            let parsed = parse_ssm2_frame(&exchange.response_frame, ecu_address, tool_address)
                .map_err(|detail| {
                    J2534Failure::new(WorkerErrorKind::MalformedResponse, detail)
                })?;
            if parsed.payload.first() != Some(&0xe8)
                || parsed.payload.len() != address_chunk.len() + 1
            {
                return Err(J2534Failure::new(
                    WorkerErrorKind::MalformedResponse,
                    format!(
                        "Subaru SSM address read expected E8 plus {} data bytes, received {} bytes",
                        address_chunk.len(),
                        parsed.payload.len()
                    ),
                ));
            }
            values.extend_from_slice(&parsed.payload[1..]);
            read_frames.push(hex_bytes(&exchange.response_frame));
        }
        Ok(SubaruSsmReadPayload {
            ecu_address,
            tool_address,
            addresses: addresses.to_vec(),
            values,
            init_frame,
            read_frames,
            latency_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
        })
    }

    pub(crate) fn discover_mode01(&mut self) -> Result<Vec<Mode01Payload>, J2534Failure> {
        self.expected_ecu = None;
        let mut responses = Vec::new();
        for pid in [0x00, 0x20, 0x40, 0x60, 0x80, 0xA0, 0xC0, 0xE0] {
            let page = self.read_mode01(pid)?;
            let follows = page.len() == 1
                && page[0].bytes.len() == 6
                && page[0].bytes[0] == 0x41
                && page[0].bytes[1] == pid
                && page[0].bytes[5] & 0x01 != 0;
            responses.extend(page);
            if !follows {
                break;
            }
        }
        if let Some(first) = responses.first()
            && responses
                .iter()
                .all(|reply| reply.ecu_id.eq_ignore_ascii_case(&first.ecu_id))
            && let Ok(ecu @ 0x7E8..=0x7EF) = u16::from_str_radix(&first.ecu_id, 16)
        {
            self.expected_ecu = Some(ecu);
        }
        Ok(responses)
    }

    pub(crate) fn read_mode01(&mut self, pid: u8) -> Result<Vec<Mode01Payload>, J2534Failure> {
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is not connected",
            )
        })?;
        self.api.read_mode01(channel, pid, 250, self.expected_ecu)
    }

    pub(crate) fn read_mode09(&mut self, pid: u8) -> Result<Vec<Mode09Response>, J2534Failure> {
        if !is_supported_mode09_pid(pid) {
            return Err(J2534Failure::new(
                WorkerErrorKind::MalformedResponse,
                format!("Mode 09 PID {pid:02X} is not an identity PID"),
            ));
        }
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is not connected",
            )
        })?;
        self.api.read_mode09(channel, pid, 1_500)
    }

    pub(crate) fn read_mode06(&mut self, test_id: u8) -> Result<Vec<Mode09Response>, J2534Failure> {
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is not connected",
            )
        })?;
        self.api.read_mode06(channel, test_id, 1_500)
    }

    pub(crate) fn read_standard_dtc(
        &mut self,
        service: StandardDtcService,
    ) -> Result<Vec<StandardDtcResponse>, J2534Failure> {
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is not connected",
            )
        })?;
        // One bounded, typed operation. For Clear this method calls the provider once;
        // neither the session nor the worker retries the state-changing Mode 04 request.
        self.api.read_standard_dtc(channel, service, 2_500)
    }

    pub(crate) fn read_oem_candidate(
        &mut self,
        request_id: u16,
        response_id: u16,
        request: OemCandidateRequestKind,
    ) -> Result<Vec<OemCandidateResponse>, J2534Failure> {
        if request_id > 0x7FF || response_id > 0x7FF || request_id == response_id {
            return Err(J2534Failure::new(
                WorkerErrorKind::MalformedResponse,
                "OEM candidate requires distinct exact 11-bit request and response IDs",
            ));
        }
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is not connected",
            )
        })?;
        self.api
            .read_oem_candidate(channel, request_id, response_id, request, 2_500)
    }

    pub(crate) fn shutdown(&mut self) {
        self.expected_ecu = None;
        self.subaru_ssm = false;
        self.initialized_ssm_ecus.clear();
        if let Some(channel) = self.channel.take()
            && let Err(error) = self.api.disconnect(channel)
        {
            eprintln!("OpenPort channel cleanup failed: {error}");
        }
        if let Some(device) = self.device.take()
            && let Err(error) = self.api.close_device(device)
        {
            eprintln!("OpenPort provider cleanup failed: {error}");
        }
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

impl Drop for J2534Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{J2534Session, PassThruApi};
    use crate::j2534::{J2534Failure, SubaruSsmExchange};
    use lognook_j2534_protocol::{
        J2534Identity, Mode01Payload, Mode09Payload, Mode09Response, OemCandidatePayload,
        OemCandidateRequestKind, OemCandidateResponse, StandardDtcPayload, StandardDtcResponse,
        StandardDtcService, WorkerErrorKind,
    };
    use lognook_j2534_protocol::subaru_ssm::{build_ssm2_frame, parse_ssm2_frame};
    use std::sync::{Arc, Mutex};

    struct FakeApi {
        calls: Arc<Mutex<Vec<String>>>,
        fail_at: Option<&'static str>,
        multiple_page_zero_responders: bool,
        fail_mode09_once: bool,
        mode09_responders: Vec<String>,
    }

    impl FakeApi {
        fn record(&self, call: impl Into<String>) {
            self.calls.lock().unwrap().push(call.into());
        }

        fn fail(&self, point: &'static str) -> Result<(), J2534Failure> {
            if self.fail_at == Some(point) {
                Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    format!("fixture failure at {point}"),
                ))
            } else {
                Ok(())
            }
        }

        fn response(pid: u8, has_more: bool, ecu_id: &str) -> Mode01Payload {
            Mode01Payload {
                ecu_id: ecu_id.into(),
                bytes: vec![0x41, pid, 0x00, 0x00, 0x00, u8::from(has_more)],
                latency_us: 1_000,
            }
        }
    }

    impl PassThruApi for FakeApi {
        fn open_device(&mut self) -> Result<u32, J2534Failure> {
            self.record("open");
            self.fail("open")?;
            Ok(1)
        }

        fn read_version(&mut self, _device: u32) -> Result<J2534Identity, J2534Failure> {
            self.record("version");
            self.fail("version")?;
            Ok(J2534Identity {
                firmware: "fixture".into(),
                dll_version: "1.0".into(),
                api_version: "04.04".into(),
            })
        }

        fn connect_iso15765(&mut self, _device: u32) -> Result<u32, J2534Failure> {
            self.record("connect");
            self.fail("connect")?;
            Ok(2)
        }

        fn connect_subaru_ssm(&mut self, _device: u32) -> Result<u32, J2534Failure> {
            self.record("connect_ssm");
            self.fail("connect_ssm")?;
            Ok(2)
        }

        fn start_obd_filter(&mut self, _channel: u32) -> Result<(), J2534Failure> {
            self.record("filter");
            self.fail("filter")
        }

        fn start_subaru_ssm_filter(&mut self, _channel: u32) -> Result<(), J2534Failure> {
            self.record("filter_ssm");
            self.fail("filter_ssm")
        }

        fn exchange_subaru_ssm(
            &mut self,
            _channel: u32,
            request_frame: &[u8],
            ecu_address: u8,
            tool_address: u8,
            _timeout_ms: u32,
        ) -> Result<SubaruSsmExchange, J2534Failure> {
            let request = parse_ssm2_frame(request_frame, tool_address, ecu_address)
                .map_err(|error| J2534Failure::new(WorkerErrorKind::MalformedResponse, error))?;
            match request.payload[0] {
                0xbf => self.record("ssm_exchange:BF"),
                0xa8 => self.record(format!("ssm_exchange:A8:{}", request.payload.len())),
                command => self.record(format!("ssm_exchange:{command:02X}")),
            }
            self.fail("ssm_exchange")?;
            let response_payload = match request.payload[0] {
                0xbf => vec![0xff, 0x01],
                0xa8 => {
                    let mut response = vec![0xe8];
                    for address in request.payload[2..].chunks_exact(3) {
                        let address = (u32::from(address[0]) << 16)
                            | (u32::from(address[1]) << 8)
                            | u32::from(address[2]);
                        response.push(match address {
                            0x000008 => 0x7d,
                            0x00001c => 0xb1,
                            _ => address as u8,
                        });
                    }
                    response
                }
                _ => unreachable!("the session only builds SSM read commands"),
            };
            Ok(SubaruSsmExchange {
                response_frame: build_ssm2_frame(tool_address, ecu_address, &response_payload)
                    .unwrap(),
                latency_us: 1_000,
            })
        }

        fn read_mode01(
            &mut self,
            _channel: u32,
            pid: u8,
            _timeout_ms: u32,
            expected_ecu: Option<u16>,
        ) -> Result<Vec<Mode01Payload>, J2534Failure> {
            self.record(match expected_ecu {
                Some(ecu) => format!("read:{pid:02X}:ecu={ecu:03X}"),
                None => format!("read:{pid:02X}"),
            });
            self.fail("read")?;
            if pid == 0x00 && self.multiple_page_zero_responders {
                return Ok(vec![
                    Self::response(0x00, true, "7E8"),
                    Self::response(0x00, true, "7E9"),
                ]);
            }
            Ok(vec![Self::response(pid, pid == 0x00, "7E8")])
        }

        fn read_mode09(
            &mut self,
            _channel: u32,
            pid: u8,
            timeout_ms: u32,
        ) -> Result<Vec<Mode09Response>, J2534Failure> {
            self.record(format!("read09:{pid:02X}:7DF:{timeout_ms}"));
            if self.fail_mode09_once {
                self.fail_mode09_once = false;
                return Err(J2534Failure::new(
                    WorkerErrorKind::NoResponse,
                    "fixture Mode 09 timeout",
                ));
            }
            self.fail("read_mode09")?;
            let bytes = match pid {
                0x02 => vec![0x49, 0x02, 0x01, b'1'],
                0x04 => [vec![0x49, 0x04, 0x01], b"CALIBRATIONID123".to_vec()].concat(),
                0x0A => [vec![0x49, 0x0A, 0x01], b"ECU NAME".to_vec()].concat(),
                _ => unreachable!("session validates Mode 09 PIDs"),
            };
            Ok(self
                .mode09_responders
                .iter()
                .map(|ecu_id| {
                    Mode09Response::Payload(Mode09Payload {
                        ecu_id: ecu_id.clone(),
                        bytes: bytes.clone(),
                        latency_us: 12_500,
                    })
                })
                .collect())
        }

        fn read_mode06(
            &mut self,
            _channel: u32,
            test_id: u8,
            timeout_ms: u32,
        ) -> Result<Vec<Mode09Response>, J2534Failure> {
            self.record(format!("read_mode06:{test_id:02X}:7DF:{timeout_ms}"));
            self.fail("read_mode06")?;
            Ok(vec![Mode09Response::Payload(Mode09Payload {
                ecu_id: "7E8".into(),
                bytes: vec![0x46, test_id, 0x80, 0, 0, 0],
                latency_us: 12_500,
            })])
        }

        fn read_standard_dtc(
            &mut self,
            _channel: u32,
            service: StandardDtcService,
            timeout_ms: u32,
        ) -> Result<Vec<StandardDtcResponse>, J2534Failure> {
            self.record(format!(
                "read_dtc:{:02X}:7DF:{timeout_ms}",
                service.request_byte()
            ));
            self.fail("read_standard_dtc")?;
            let bytes = match service {
                StandardDtcService::Stored => vec![0x43, 0x00, 0x00],
                StandardDtcService::Pending => vec![0x47, 0x00, 0x00],
                StandardDtcService::Permanent => vec![0x4A, 0x00, 0x00],
                StandardDtcService::Clear => vec![0x44],
            };
            Ok(self
                .mode09_responders
                .iter()
                .map(|ecu_id| {
                    StandardDtcResponse::Payload(StandardDtcPayload {
                        ecu_id: ecu_id.clone(),
                        bytes: bytes.clone(),
                        latency_us: 12_500,
                    })
                })
                .collect())
        }

        fn read_oem_candidate(
            &mut self,
            _channel: u32,
            request_id: u16,
            response_id: u16,
            request: OemCandidateRequestKind,
            timeout_ms: u32,
        ) -> Result<Vec<OemCandidateResponse>, J2534Failure> {
            let (service, identifier) = match request {
                OemCandidateRequestKind::Mode21 { identifier } => (0x21, vec![identifier]),
                OemCandidateRequestKind::Mode22 { did } => {
                    (0x22, vec![(did >> 8) as u8, did as u8])
                }
            };
            self.record(format!(
                "read_oem:{service:02X}:{}:{request_id:03X}->{response_id:03X}:{timeout_ms}",
                identifier
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<String>()
            ));
            self.fail("read_oem_candidate")?;
            let mut bytes = vec![service + 0x40];
            bytes.extend_from_slice(&identifier);
            bytes.push(0x2A);
            Ok(vec![OemCandidateResponse::Payload(OemCandidatePayload {
                response_id,
                bytes,
                latency_us: 12_500,
            })])
        }

        fn disconnect(&mut self, _channel: u32) -> Result<(), J2534Failure> {
            self.record("disconnect");
            Ok(())
        }

        fn close_device(&mut self, _device: u32) -> Result<(), J2534Failure> {
            self.record("close");
            Ok(())
        }
    }

    fn fake_api(fail_at: Option<&'static str>) -> (FakeApi, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            FakeApi {
                calls: calls.clone(),
                fail_at,
                multiple_page_zero_responders: false,
                fail_mode09_once: false,
                mode09_responders: vec!["7E8".into()],
            },
            calls,
        )
    }

    #[test]
    fn subaru_ssm_connect_reads_selected_addresses_and_initializes_each_ecu_once() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        let mut progress = Vec::new();
        let identity = session
            .connect_subaru_ssm_with_progress(&mut |stage| progress.push(stage.to_owned()))
            .unwrap();
        assert_eq!(identity.api_version, "04.04");
        assert_eq!(progress, ["version_read", "channel_connect", "filter_setup"]);

        let candidate = format!("subaru-candidate_{}", "a".repeat(64));
        let first = session
            .read_subaru_ssm_addresses(
                std::slice::from_ref(&candidate),
                "SUBARU_OEM_40_01B_ABCD1234",
                &"a".repeat(64),
                0x10,
                0xf0,
                &[0x000008, 0x00001c],
            )
            .unwrap();
        let second = session
            .read_subaru_ssm_addresses(
                &[candidate],
                "SUBARU_OEM_40_01B_ABCD1234",
                &"a".repeat(64),
                0x10,
                0xf0,
                &[0x000008],
            )
            .unwrap();
        assert_eq!(first.values, [0x7d, 0xb1]);
        assert_eq!(first.addresses, [0x000008, 0x00001c]);
        assert!(first.init_frame.is_some());
        assert_eq!(first.read_frames.len(), 1);
        assert!(second.init_frame.is_none());
        assert_eq!(second.values, [0x7d]);

        let calls = calls.lock().unwrap();
        assert_eq!(calls.iter().filter(|call| *call == "ssm_exchange:BF").count(), 1);
        assert_eq!(calls.iter().filter(|call| call.starts_with("ssm_exchange:A8:")).count(), 2);
        assert!(calls.iter().all(|call| !call.contains("B0") && !call.contains("B8")));
    }

    #[test]
    fn mode06_requires_a_connected_channel_and_sends_one_bounded_read() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        assert!(session.read_mode06(0x35).is_err());
        assert_eq!(*calls.lock().unwrap(), ["open"]);
        session.connect().unwrap();
        let responses = session.read_mode06(0x35).unwrap();
        assert!(
            matches!(&responses[0], Mode09Response::Payload(payload) if payload.bytes[0..2] == [0x46, 0x35])
        );
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with("read_mode06:"))
                .cloned()
                .collect::<Vec<_>>(),
            ["read_mode06:35:7DF:1500"]
        );
    }

    #[test]
    fn channel_setup_failure_disconnects_then_closes_the_provider() {
        let (api, calls) = fake_api(Some("filter"));
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        assert!(session.connect().is_err());
        drop(session);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "open",
                "version",
                "connect",
                "filter",
                "disconnect",
                "close"
            ]
        );
    }

    #[test]
    fn channel_connect_failure_closes_the_provider() {
        let (api, calls) = fake_api(Some("connect"));
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        assert!(session.connect().is_err());
        drop(session);
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["open", "version", "connect", "close"]
        );
    }

    #[test]
    fn supported_pid_discovery_follows_only_reported_continuation_pages() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();
        let responses = session.discover_mode01().unwrap();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].bytes[1], 0x00);
        assert_eq!(responses[1].bytes[1], 0x20);
        let reads: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("read:"))
            .cloned()
            .collect();
        assert_eq!(reads, vec!["read:00", "read:20"]);
    }

    #[test]
    fn multiple_responders_stop_supported_pid_discovery_at_the_first_page() {
        let (mut api, calls) = fake_api(None);
        api.multiple_page_zero_responders = true;
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();
        let responses = session.discover_mode01().unwrap();
        assert_eq!(
            responses
                .iter()
                .map(|sample| sample.ecu_id.as_str())
                .collect::<Vec<_>>(),
            vec!["7E8", "7E9"]
        );
        let reads: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("read:"))
            .cloned()
            .collect();
        assert_eq!(reads, vec!["read:00"]);
    }

    #[test]
    fn physical_live_route_requires_a_unique_discovered_ecu() {
        for multiple in [false, true] {
            let (mut api, calls) = fake_api(None);
            api.multiple_page_zero_responders = multiple;
            let mut session = J2534Session::open(Box::new(api)).unwrap();
            session.connect().unwrap();
            session.discover_mode01().unwrap();
            session.read_mode01(0x0C).unwrap();
            assert_eq!(
                calls.lock().unwrap().last().unwrap(),
                if multiple {
                    "read:0C"
                } else {
                    "read:0C:ecu=7E8"
                }
            );
        }
    }

    #[test]
    fn mode09_uses_the_functional_standard_route_and_keeps_all_responders() {
        let (mut api, calls) = fake_api(None);
        api.mode09_responders = vec!["7E8".into(), "7E9".into()];
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();

        for (pid, expected_len) in [(0x02, 4), (0x04, 19), (0x0A, 11)] {
            let responses = session.read_mode09(pid).unwrap();
            assert_eq!(
                responses
                    .iter()
                    .map(|response| match response {
                        Mode09Response::Payload(payload) => payload.ecu_id.as_str(),
                        _ => panic!("fixture returned an unexpected Mode 09 response"),
                    })
                    .collect::<Vec<_>>(),
                ["7E8", "7E9"]
            );
            assert!(responses.iter().all(|response| match response {
                Mode09Response::Payload(payload) => {
                    payload.bytes[0..2] == [0x49, pid]
                        && payload.bytes.len() == expected_len
                        && payload.latency_us == 12_500
                }
                _ => false,
            }));
        }
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with("read09:"))
                .cloned()
                .collect::<Vec<_>>(),
            [
                "read09:02:7DF:1500",
                "read09:04:7DF:1500",
                "read09:0A:7DF:1500"
            ]
        );
    }

    #[test]
    fn mode09_failure_does_not_block_the_next_identity_query_or_send_other_pids() {
        let (mut api, calls) = fake_api(None);
        api.fail_mode09_once = true;
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();

        assert_eq!(
            session.read_mode09(0x02).unwrap_err().kind,
            WorkerErrorKind::NoResponse
        );
        let calibration = session.read_mode09(0x04).unwrap();
        assert!(matches!(
            &calibration[0],
            Mode09Response::Payload(payload) if payload.bytes[1] == 0x04
        ));
        assert!(session.read_mode09(0x03).is_err());
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with("read09:"))
                .cloned()
                .collect::<Vec<_>>(),
            ["read09:02:7DF:1500", "read09:04:7DF:1500"]
        );
    }

    #[test]
    fn standard_dtc_requests_keep_each_responder_and_clear_is_one_typed_call() {
        let (mut api, calls) = fake_api(None);
        api.mode09_responders = vec!["7E8".into(), "7E9".into()];
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();

        for (service, response_service) in [
            (StandardDtcService::Stored, 0x43),
            (StandardDtcService::Pending, 0x47),
            (StandardDtcService::Permanent, 0x4A),
            (StandardDtcService::Clear, 0x44),
        ] {
            let responses = session.read_standard_dtc(service).unwrap();
            assert_eq!(responses.len(), 2);
            assert!(
                responses
                    .iter()
                    .enumerate()
                    .all(|(index, response)| matches!(
                        response,
                        StandardDtcResponse::Payload(payload)
                            if payload.ecu_id == ["7E8", "7E9"][index]
                                && payload.bytes.first() == Some(&response_service)
                    ))
            );
        }
        let reads = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("read_dtc:"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            reads,
            [
                "read_dtc:03:7DF:2500",
                "read_dtc:07:7DF:2500",
                "read_dtc:0A:7DF:2500",
                "read_dtc:04:7DF:2500"
            ]
        );
    }

    #[test]
    fn oem_mode21_and_mode22_are_single_typed_calls_with_exact_routes() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();

        for (request, expected) in [
            (
                OemCandidateRequestKind::Mode21 { identifier: 0x2A },
                "read_oem:21:2A:7E0->7E8:2500",
            ),
            (
                OemCandidateRequestKind::Mode22 { did: 0x1234 },
                "read_oem:22:1234:710->718:2500",
            ),
        ] {
            let (request_id, response_id) = if expected.contains("7E0") {
                (0x7E0, 0x7E8)
            } else {
                (0x710, 0x718)
            };
            let responses = session
                .read_oem_candidate(request_id, response_id, request)
                .unwrap();
            assert!(matches!(
                responses.as_slice(),
                [OemCandidateResponse::Payload(payload)]
                    if payload.response_id == response_id
                        && payload.bytes.first().is_some_and(|service| *service == 0x61 || *service == 0x62)
            ));
            assert_eq!(calls.lock().unwrap().last().unwrap(), expected);
        }
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with("read_oem:"))
                .count(),
            2
        );
    }

    #[test]
    fn invalid_oem_route_is_rejected_before_the_provider_call() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();

        assert!(
            session
                .read_oem_candidate(
                    0x7E8,
                    0x7E8,
                    OemCandidateRequestKind::Mode21 { identifier: 1 },
                )
                .is_err()
        );
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .all(|call| !call.starts_with("read_oem:"))
        );
    }

    #[test]
    fn functional_mode09_read_does_not_change_the_discovered_mode01_route() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();
        session.discover_mode01().unwrap();

        session.read_mode09(0x02).unwrap();
        session.read_mode01(0x0C).unwrap();

        assert_eq!(calls.lock().unwrap().last().unwrap(), "read:0C:ecu=7E8");
    }
    #[test]
    fn connect_progress_tracks_actual_calls_and_stops_at_failure() {
        for fail_at in [None, Some("version"), Some("connect"), Some("filter")] {
            let (api, calls) = fake_api(fail_at);
            let mut session = J2534Session::open(Box::new(api)).unwrap();
            let mut stages = Vec::new();
            let result = session.connect_with_progress(&mut |stage| stages.push(stage.to_owned()));
            assert_eq!(result.is_err(), fail_at.is_some());
            let expected = match fail_at {
                Some("version") => vec!["version_read"],
                Some("connect") => vec!["version_read", "channel_connect"],
                _ => vec!["version_read", "channel_connect", "filter_setup"],
            };
            assert_eq!(stages, expected);
            assert_eq!(calls.lock().unwrap().len(), stages.len() + 1);
        }
    }
}
